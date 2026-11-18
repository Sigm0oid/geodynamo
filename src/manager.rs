//! High-level geospatial CRUD, concurrent radius/bounding-box search, and cursor pagination (`GeoDataManager`).

use aws_sdk_dynamodb::operation::delete_item::DeleteItemOutput;
use aws_sdk_dynamodb::operation::get_item::GetItemOutput;
use aws_sdk_dynamodb::operation::put_item::PutItemOutput;
use aws_sdk_dynamodb::operation::query::builders::QueryInputBuilder;
use aws_sdk_dynamodb::operation::update_item::UpdateItemOutput;
use aws_sdk_dynamodb::types::{AttributeValue, WriteRequest};
use futures::future::try_join_all;

use crate::config::GeoDataManagerConfiguration;
use crate::dynamodb_manager::DynamoDBManager;
use crate::error::{GeoError, Result};
use crate::model::requests::DynamoDbItem;
use crate::model::{
    Covering, DeletePointInput, GeoPoint, GetPointInput, PaginatedQueryOutput, PutPointInput,
    QueryRadiusRequest, QueryRectangleRequest, UpdatePointInput,
};
use crate::pagination::{
    decode_token, encode_token, map_to_wire, wire_to_map, PaginationToken,
};
use crate::s2::{S2Cap, S2LatLngRect, S2RegionCoverer};

/// Primary entry point for geospatial CRUD operations, spatial queries, and pagination on DynamoDB.
#[derive(Debug, Clone)]
pub struct GeoDataManager {
    config: GeoDataManagerConfiguration,
    dynamodb_manager: DynamoDBManager,
}

impl GeoDataManager {
    /// Creates a new `GeoDataManager` backed by `config`.
    #[must_use]
    pub fn new(config: GeoDataManagerConfiguration) -> Self {
        let dynamodb_manager = DynamoDBManager::new(config.clone());
        Self {
            config,
            dynamodb_manager,
        }
    }

    /// Borrows the underlying [`GeoDataManagerConfiguration`].
    #[must_use]
    pub const fn config(&self) -> &GeoDataManagerConfiguration {
        &self.config
    }

    /// Inserts or replaces a geospatial point item.
    pub async fn put_point(&self, input: PutPointInput) -> Result<PutItemOutput> {
        self.dynamodb_manager.put_point(input).await
    }

    /// Inserts multiple geospatial points in batches of 25 via `BatchWriteItem`.
    pub async fn batch_write_points(&self, inputs: Vec<PutPointInput>) -> Result<Vec<WriteRequest>> {
        self.dynamodb_manager.batch_write_points(inputs).await
    }

    /// Retrieves a single point item by its coordinates and `rangeKey`.
    pub async fn get_point(&self, input: GetPointInput) -> Result<GetItemOutput> {
        self.dynamodb_manager.get_point(input).await
    }

    /// Updates non-key attributes on an existing point item.
    pub async fn update_point(&self, input: UpdatePointInput) -> Result<UpdateItemOutput> {
        self.dynamodb_manager.update_point(input).await
    }

    /// Deletes a point item by its coordinates and `rangeKey`.
    pub async fn delete_point(&self, input: DeletePointInput) -> Result<DeleteItemOutput> {
        self.dynamodb_manager.delete_point(input).await
    }

    /// Queries all items strictly inside the bounding rectangle `[min_point, max_point]`,
    /// dispatching S2 geohash range queries concurrently across partitions.
    pub async fn query_rectangle(&self, request: QueryRectangleRequest) -> Result<Vec<DynamoDbItem>> {
        let rect = S2LatLngRect::new(
            request.min_point().to_s2_lat_lng(),
            request.max_point().to_s2_lat_lng(),
        );
        let coverer = S2RegionCoverer::new().with_max_cells(self.config.max_cells);
        let covering = Covering::new(coverer.get_covering_rect(&rect));

        let candidates = self
            .dispatch_queries_concurrent(&covering, request.query_builder())
            .await?;
        self.filter_by_rectangle(candidates, &rect)
    }

    /// Executes a cursor-paginated bounding-box query returning at most `limit` matching items per page.
    pub async fn query_rectangle_paginated(
        &self,
        request: QueryRectangleRequest,
        limit: usize,
        next_token: Option<&str>,
    ) -> Result<PaginatedQueryOutput> {
        let rect = S2LatLngRect::new(
            request.min_point().to_s2_lat_lng(),
            request.max_point().to_s2_lat_lng(),
        );
        let coverer = S2RegionCoverer::new().with_max_cells(self.config.max_cells);
        let covering = Covering::new(coverer.get_covering_rect(&rect));

        self.paginate_covering(
            &covering,
            request.query_builder(),
            limit,
            next_token,
            |point| rect.contains(point.to_s2_lat_lng()),
        )
        .await
    }

    /// Queries all items within `radius_in_meter` meters of `center_point`,
    /// optionally sorted ascending by exact spherical distance when `request.sort()` is `true`.
    pub async fn query_radius(&self, request: QueryRadiusRequest) -> Result<Vec<DynamoDbItem>> {
        let center = request.center_point();
        let radius = request.radius_in_meter();
        let cap = S2Cap::from_center_meters(center.to_s2_lat_lng(), radius);
        let coverer = S2RegionCoverer::new().with_max_cells(self.config.max_cells);
        let covering = Covering::new(coverer.get_covering_cap(&cap));

        let candidates = self
            .dispatch_queries_concurrent(&covering, request.query_builder())
            .await?;

        let mut matched_with_dist = Vec::new();
        for item in candidates {
            let point = self.extract_item_point(&item)?;
            let dist = center.distance_meters(point);
            if dist <= radius {
                matched_with_dist.push((dist, item));
            }
        }

        if request.sort() {
            matched_with_dist.sort_by(|a, b| a.0.total_cmp(&b.0));
        }

        Ok(matched_with_dist.into_iter().map(|(_, item)| item).collect())
    }

    /// Executes a cursor-paginated radius query returning at most `limit` matching items.
    ///
    /// - When `request.sort() == false`, streams lazily across S2 ranges and DynamoDB pages,
    ///   stopping as soon as `limit` matching items are collected to save Read Capacity Units.
    /// - When `request.sort() == true`, evaluates and distance-sorts all matching items in the radius
    ///   before slicing by cursor offset.
    pub async fn query_radius_paginated(
        &self,
        request: QueryRadiusRequest,
        limit: usize,
        next_token: Option<&str>,
    ) -> Result<PaginatedQueryOutput> {
        if limit == 0 {
            return Err(GeoError::InvalidParameter {
                name: "limit",
                reason: "limit must be greater than 0".to_owned(),
            });
        }

        if request.sort() {
            let offset = match next_token {
                None => 0,
                Some(tok) => match decode_token(tok)? {
                    PaginationToken::Sorted { offset } => offset,
                    PaginationToken::Stream { .. } => {
                        return Err(GeoError::InvalidPaginationToken(
                            "token mode `stream` cannot be used with `sort=true`".to_owned(),
                        ));
                    }
                },
            };

            let all_sorted = self.query_radius(request).await?;
            let items: Vec<DynamoDbItem> =
                all_sorted.iter().skip(offset).take(limit).cloned().collect();
            let next_offset = offset + items.len();
            let out_token = if next_offset < all_sorted.len() && !items.is_empty() {
                Some(encode_token(&PaginationToken::Sorted {
                    offset: next_offset,
                })?)
            } else {
                None
            };

            return Ok(PaginatedQueryOutput {
                items,
                next_token: out_token,
            });
        }

        let center = request.center_point();
        let radius = request.radius_in_meter();
        let cap = S2Cap::from_center_meters(center.to_s2_lat_lng(), radius);
        let coverer = S2RegionCoverer::new().with_max_cells(self.config.max_cells);
        let covering = Covering::new(coverer.get_covering_cap(&cap));

        self.paginate_covering(
            &covering,
            request.query_builder(),
            limit,
            next_token,
            |point| center.distance_meters(point) <= radius,
        )
        .await
    }

    async fn dispatch_queries_concurrent(
        &self,
        covering: &Covering,
        query_builder: &QueryInputBuilder,
    ) -> Result<Vec<DynamoDbItem>> {
        let ranges = covering.get_geo_hash_ranges(
            self.config.hash_key_length,
            self.config.geohash_encoding,
            self.config.merge_threshold,
        );

        let futures = ranges.into_iter().map(|range| {
            let hk = range.hash_key(self.config.hash_key_length);
            self.dynamodb_manager.query_geohash(query_builder, hk, range)
        });

        let nested = try_join_all(futures).await?;
        Ok(nested.into_iter().flatten().collect())
    }

    async fn paginate_covering<F>(
        &self,
        covering: &Covering,
        query_builder: &QueryInputBuilder,
        limit: usize,
        next_token: Option<&str>,
        predicate: F,
    ) -> Result<PaginatedQueryOutput>
    where
        F: Fn(GeoPoint) -> bool,
    {
        if limit == 0 {
            return Err(GeoError::InvalidParameter {
                name: "limit",
                reason: "limit must be greater than 0".to_owned(),
            });
        }

        let ranges = covering.get_geo_hash_ranges(
            self.config.hash_key_length,
            self.config.geohash_encoding,
            self.config.merge_threshold,
        );

        let (mut range_index, mut exclusive_start_key, mut buffer) = match next_token {
            None => (0_usize, None, Vec::new()),
            Some(tok) => match decode_token(tok)? {
                PaginationToken::Stream {
                    range_index,
                    exclusive_start_key,
                    buffer,
                } => {
                    let esk = exclusive_start_key.map(wire_to_map).transpose()?;
                    let mut decoded_buf = Vec::with_capacity(buffer.len());
                    for item in buffer {
                        decoded_buf.push(wire_to_map(item)?);
                    }
                    (range_index, esk, decoded_buf)
                }
                PaginationToken::Sorted { .. } => {
                    return Err(GeoError::InvalidPaginationToken(
                        "token mode `sorted` cannot be used with unsorted streaming query".to_owned(),
                    ));
                }
            },
        };

        if buffer.len() > limit {
            let remaining = buffer.split_off(limit);
            let wire_buf = remaining
                .iter()
                .map(map_to_wire)
                .collect::<Result<Vec<_>>>()?;
            let wire_esk = exclusive_start_key.as_ref().map(map_to_wire).transpose()?;
            let tok = encode_token(&PaginationToken::Stream {
                range_index,
                exclusive_start_key: wire_esk,
                buffer: wire_buf,
            })?;
            return Ok(PaginatedQueryOutput {
                items: buffer,
                next_token: Some(tok),
            });
        }

        let mut collected = buffer;
        if collected.len() == limit {
            let has_more = exclusive_start_key.is_some() || range_index < ranges.len();
            let tok = if has_more {
                let wire_esk = exclusive_start_key.as_ref().map(map_to_wire).transpose()?;
                Some(encode_token(&PaginationToken::Stream {
                    range_index,
                    exclusive_start_key: wire_esk,
                    buffer: Vec::new(),
                })?)
            } else {
                None
            };
            return Ok(PaginatedQueryOutput {
                items: collected,
                next_token: tok,
            });
        }

        while range_index < ranges.len() {
            let current_range = ranges[range_index];
            let hk = current_range.hash_key(self.config.hash_key_length);
            let (raw_items, lek) = self
                .dynamodb_manager
                .query_geohash_page(query_builder, hk, current_range, exclusive_start_key.take())
                .await?;

            let mut iter = raw_items.into_iter();
            while let Some(item) = iter.next() {
                let point = self.extract_item_point(&item)?;
                if predicate(point) {
                    collected.push(item);
                    if collected.len() == limit {
                        let mut leftover = Vec::new();
                        for rest_item in iter {
                            let rest_pt = self.extract_item_point(&rest_item)?;
                            if predicate(rest_pt) {
                                leftover.push(rest_item);
                            }
                        }

                        let (next_range_idx, next_esk) = if let Some(page_lek) = lek {
                            (range_index, Some(page_lek))
                        } else {
                            (range_index + 1, None)
                        };

                        let has_more = !leftover.is_empty()
                            || next_esk.is_some()
                            || next_range_idx < ranges.len();

                        let tok = if has_more {
                            let wire_esk = next_esk.as_ref().map(map_to_wire).transpose()?;
                            let wire_buf = leftover
                                .iter()
                                .map(map_to_wire)
                                .collect::<Result<Vec<_>>>()?;
                            Some(encode_token(&PaginationToken::Stream {
                                range_index: next_range_idx,
                                exclusive_start_key: wire_esk,
                                buffer: wire_buf,
                            })?)
                        } else {
                            None
                        };

                        return Ok(PaginatedQueryOutput {
                            items: collected,
                            next_token: tok,
                        });
                    }
                }
            }

            if let Some(page_lek) = lek {
                exclusive_start_key = Some(page_lek);
            } else {
                range_index += 1;
            }
        }

        Ok(PaginatedQueryOutput {
            items: collected,
            next_token: None,
        })
    }

    fn filter_by_rectangle(
        &self,
        items: Vec<DynamoDbItem>,
        rect: &S2LatLngRect,
    ) -> Result<Vec<DynamoDbItem>> {
        let mut out = Vec::new();
        for item in items {
            let point = self.extract_item_point(&item)?;
            if rect.contains(point.to_s2_lat_lng()) {
                out.push(item);
            }
        }
        Ok(out)
    }

    fn extract_item_point(&self, item: &DynamoDbItem) -> Result<GeoPoint> {
        let attr_name = &self.config.geojson_attribute_name;
        let attr = item
            .get(attr_name)
            .ok_or_else(|| GeoError::MalformedGeoAttribute {
                attribute: attr_name.clone(),
                details: "attribute missing from item (ensure ProjectionExpression includes it)"
                    .to_owned(),
            })?;

        match attr {
            AttributeValue::S(raw) => GeoPoint::parse_attr(raw, attr_name),
            _ => Err(GeoError::MalformedGeoAttribute {
                attribute: attr_name.clone(),
                details: "expected DynamoDB String (`S`) attribute".to_owned(),
            }),
        }
    }
}
