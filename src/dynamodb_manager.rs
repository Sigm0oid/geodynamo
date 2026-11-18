//! Low-level DynamoDB CRUD, batch write, and LSI geohash range query execution (`DynamoDBManager`).

use aws_sdk_dynamodb::operation::delete_item::DeleteItemOutput;
use aws_sdk_dynamodb::operation::get_item::GetItemOutput;
use aws_sdk_dynamodb::operation::put_item::PutItemOutput;
use aws_sdk_dynamodb::operation::query::builders::QueryInputBuilder;
use aws_sdk_dynamodb::operation::update_item::UpdateItemOutput;
use aws_sdk_dynamodb::types::{AttributeValue, PutRequest, WriteRequest};

use crate::config::GeoDataManagerConfiguration;
use crate::error::Result;
use crate::model::requests::DynamoDbItem;
use crate::model::{
    DeletePointInput, GeohashRange, GetPointInput, PutPointInput, UpdatePointInput,
};
use crate::s2::S2Manager;

/// Maximum number of `WriteRequest` items allowed per DynamoDB `BatchWriteItem` call.
const DYNAMODB_BATCH_WRITE_LIMIT: usize = 25;

/// Executes DynamoDB operations enriched with S2 `hashKey`, `geohash`, and `geoJson` attributes.
#[derive(Debug, Clone)]
pub struct DynamoDBManager {
    config: GeoDataManagerConfiguration,
}

impl DynamoDBManager {
    /// Creates a new `DynamoDBManager` from `config`.
    #[must_use]
    pub fn new(config: GeoDataManagerConfiguration) -> Self {
        Self { config }
    }

    /// Borrows the underlying configuration.
    #[must_use]
    pub const fn config(&self) -> &GeoDataManagerConfiguration {
        &self.config
    }

    /// Queries a single DynamoDB page on `geohash-index` for `(hash_key, range)`,
    /// isolating query builder state so `ExclusiveStartKey` never leaks across ranges.
    pub async fn query_geohash_page(
        &self,
        template: &QueryInputBuilder,
        hash_key: i64,
        range: GeohashRange,
        exclusive_start_key: Option<DynamoDbItem>,
    ) -> Result<(Vec<DynamoDbItem>, Option<DynamoDbItem>)> {
        let builder = template
            .clone()
            .table_name(&self.config.table_name)
            .index_name(&self.config.geohash_index_name)
            .set_exclusive_start_key(exclusive_start_key)
            .key_condition_expression(
                "#ddb_geo_hk = :ddb_geo_hk AND #ddb_geo_gh BETWEEN :ddb_geo_gh_min AND :ddb_geo_gh_max",
            )
            .expression_attribute_names("#ddb_geo_hk", &self.config.hash_key_attribute_name)
            .expression_attribute_names("#ddb_geo_gh", &self.config.geohash_attribute_name)
            .expression_attribute_values(":ddb_geo_hk", AttributeValue::N(hash_key.to_string()))
            .expression_attribute_values(
                ":ddb_geo_gh_min",
                AttributeValue::N(range.range_min.to_string()),
            )
            .expression_attribute_values(
                ":ddb_geo_gh_max",
                AttributeValue::N(range.range_max.to_string()),
            );

        let output = builder
            .send_with(&self.config.client)
            .await
            .map_err(aws_sdk_dynamodb::Error::from)?;

        Ok((output.items.unwrap_or_default(), output.last_evaluated_key))
    }

    /// Queries all paginated items on `geohash-index` matching `(hash_key, range)`.
    pub async fn query_geohash(
        &self,
        template: &QueryInputBuilder,
        hash_key: i64,
        range: GeohashRange,
    ) -> Result<Vec<DynamoDbItem>> {
        let (mut all_items, mut lek) = self
            .query_geohash_page(template, hash_key, range, None)
            .await?;

        while let Some(start_key) = lek {
            let (page_items, next_lek) = self
                .query_geohash_page(template, hash_key, range, Some(start_key))
                .await?;
            all_items.extend(page_items);
            lek = next_lek;
        }

        Ok(all_items)
    }

    /// Inserts or replaces a geospatial point item in DynamoDB.
    pub async fn put_point(&self, input: PutPointInput) -> Result<PutItemOutput> {
        let geohash = S2Manager::generate_geohash(input.geo_point, self.config.geohash_encoding);
        let hash_key = S2Manager::generate_hash_key(geohash, self.config.hash_key_length);
        let geo_str = input.geo_point.format_attr(self.config.coordinate_format);

        let builder = input
            .put_item_builder
            .table_name(&self.config.table_name)
            .item(
                &self.config.hash_key_attribute_name,
                AttributeValue::N(hash_key.to_string()),
            )
            .item(&self.config.range_key_attribute_name, input.range_key_value)
            .item(
                &self.config.geohash_attribute_name,
                AttributeValue::N(geohash.to_string()),
            )
            .item(&self.config.geojson_attribute_name, AttributeValue::S(geo_str));

        let output = builder
            .send_with(&self.config.client)
            .await
            .map_err(aws_sdk_dynamodb::Error::from)?;
        Ok(output)
    }

    /// Writes multiple geospatial point items in chunks of 25 using `BatchWriteItem`,
    /// returning any unprocessed `WriteRequest`s remaining after retries.
    pub async fn batch_write_points(&self, inputs: Vec<PutPointInput>) -> Result<Vec<WriteRequest>> {
        let mut write_requests = Vec::with_capacity(inputs.len());
        for input in inputs {
            let geohash =
                S2Manager::generate_geohash(input.geo_point, self.config.geohash_encoding);
            let hash_key = S2Manager::generate_hash_key(geohash, self.config.hash_key_length);
            let geo_str = input.geo_point.format_attr(self.config.coordinate_format);

            let built = input
                .put_item_builder
                .table_name(&self.config.table_name)
                .item(
                    &self.config.hash_key_attribute_name,
                    AttributeValue::N(hash_key.to_string()),
                )
                .item(&self.config.range_key_attribute_name, input.range_key_value)
                .item(
                    &self.config.geohash_attribute_name,
                    AttributeValue::N(geohash.to_string()),
                )
                .item(&self.config.geojson_attribute_name, AttributeValue::S(geo_str))
                .build()?;

            let put_req = PutRequest::builder()
                .set_item(built.item)
                .build()?;
            write_requests.push(WriteRequest::builder().put_request(put_req).build());
        }

        let mut leftover_unprocessed = Vec::new();
        for chunk in write_requests.chunks(DYNAMODB_BATCH_WRITE_LIMIT) {
            let mut pending = chunk.to_vec();
            let mut attempts = 0_u8;
            while !pending.is_empty() && attempts < 4 {
                let output = self
                    .config
                    .client
                    .batch_write_item()
                    .request_items(&self.config.table_name, pending)
                    .send()
                    .await
                    .map_err(aws_sdk_dynamodb::Error::from)?;

                pending = output
                    .unprocessed_items
                    .and_then(|mut m| m.remove(&self.config.table_name))
                    .unwrap_or_default();
                attempts += 1;
            }
            leftover_unprocessed.extend(pending);
        }

        Ok(leftover_unprocessed)
    }

    /// Retrieves a single point item by `(GeoPoint, RangeKeyValue)`.
    pub async fn get_point(&self, input: GetPointInput) -> Result<GetItemOutput> {
        let geohash = S2Manager::generate_geohash(input.geo_point, self.config.geohash_encoding);
        let hash_key = S2Manager::generate_hash_key(geohash, self.config.hash_key_length);

        let builder = input
            .get_item_builder
            .table_name(&self.config.table_name)
            .key(
                &self.config.hash_key_attribute_name,
                AttributeValue::N(hash_key.to_string()),
            )
            .key(&self.config.range_key_attribute_name, input.range_key_value);

        let output = builder
            .send_with(&self.config.client)
            .await
            .map_err(aws_sdk_dynamodb::Error::from)?;
        Ok(output)
    }

    /// Updates non-key attributes on an existing point item.
    pub async fn update_point(&self, input: UpdatePointInput) -> Result<UpdateItemOutput> {
        let geohash = S2Manager::generate_geohash(input.geo_point, self.config.geohash_encoding);
        let hash_key = S2Manager::generate_hash_key(geohash, self.config.hash_key_length);

        let builder = input
            .update_item_builder
            .table_name(&self.config.table_name)
            .key(
                &self.config.hash_key_attribute_name,
                AttributeValue::N(hash_key.to_string()),
            )
            .key(&self.config.range_key_attribute_name, input.range_key_value);

        let output = builder
            .send_with(&self.config.client)
            .await
            .map_err(aws_sdk_dynamodb::Error::from)?;
        Ok(output)
    }

    /// Deletes a point item by `(GeoPoint, RangeKeyValue)`.
    pub async fn delete_point(&self, input: DeletePointInput) -> Result<DeleteItemOutput> {
        let geohash = S2Manager::generate_geohash(input.geo_point, self.config.geohash_encoding);
        let hash_key = S2Manager::generate_hash_key(geohash, self.config.hash_key_length);

        let builder = input
            .delete_item_builder
            .table_name(&self.config.table_name)
            .key(
                &self.config.hash_key_attribute_name,
                AttributeValue::N(hash_key.to_string()),
            )
            .key(&self.config.range_key_attribute_name, input.range_key_value);

        let output = builder
            .send_with(&self.config.client)
            .await
            .map_err(aws_sdk_dynamodb::Error::from)?;
        Ok(output)
    }
}
