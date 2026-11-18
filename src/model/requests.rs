//! Request and response types wrapping official `aws_sdk_dynamodb` fluent input builders.

use std::collections::HashMap;

use aws_sdk_dynamodb::operation::delete_item::builders::DeleteItemInputBuilder;
use aws_sdk_dynamodb::operation::delete_item::DeleteItemInput;
use aws_sdk_dynamodb::operation::get_item::builders::GetItemInputBuilder;
use aws_sdk_dynamodb::operation::get_item::GetItemInput;
use aws_sdk_dynamodb::operation::put_item::builders::PutItemInputBuilder;
use aws_sdk_dynamodb::operation::put_item::PutItemInput;
use aws_sdk_dynamodb::operation::query::builders::QueryInputBuilder;
use aws_sdk_dynamodb::operation::query::QueryInput;
use aws_sdk_dynamodb::operation::update_item::builders::UpdateItemInputBuilder;
use aws_sdk_dynamodb::operation::update_item::UpdateItemInput;
use aws_sdk_dynamodb::types::AttributeValue;

use crate::error::{GeoError, Result};

use super::geo_point::GeoPoint;

/// Type alias for a DynamoDB item attribute map.
pub type DynamoDbItem = HashMap<String, AttributeValue>;

/// Input for [`crate::GeoDataManager::put_point`] and [`crate::GeoDataManager::batch_write_points`].
#[derive(Debug, Clone)]
pub struct PutPointInput {
    /// Point coordinates.
    pub geo_point: GeoPoint,
    /// Application-supplied unique sort key (`rangeKey`) value.
    pub range_key_value: AttributeValue,
    /// Underlying `aws_sdk_dynamodb` `PutItemInputBuilder` holding extra attributes, conditions, etc.
    pub put_item_builder: PutItemInputBuilder,
}

impl PutPointInput {
    /// Creates a new `PutPointInput` with a string `rangeKey` value.
    #[must_use]
    pub fn new(geo_point: GeoPoint, range_key_value: impl Into<String>) -> Self {
        Self {
            geo_point,
            range_key_value: AttributeValue::S(range_key_value.into()),
            put_item_builder: PutItemInput::builder(),
        }
    }

    /// Creates a `PutPointInput` with an arbitrary `AttributeValue` as the sort key.
    #[must_use]
    pub fn with_range_key_attr(geo_point: GeoPoint, range_key_value: AttributeValue) -> Self {
        Self {
            geo_point,
            range_key_value,
            put_item_builder: PutItemInput::builder(),
        }
    }

    /// Adds a non-key attribute `(name, value)` to the item being written.
    #[must_use]
    pub fn item(mut self, name: impl Into<String>, value: AttributeValue) -> Self {
        self.put_item_builder = self.put_item_builder.item(name, value);
        self
    }

    /// Supplies a custom `PutItemInputBuilder` (e.g., with `condition_expression`, `expression_attribute_values`, or `return_values`).
    #[must_use]
    pub fn with_put_item_builder(mut self, builder: PutItemInputBuilder) -> Self {
        self.put_item_builder = builder;
        self
    }

    /// Sets a DynamoDB `ConditionExpression` on the `PutItem` call.
    #[must_use]
    pub fn condition_expression(mut self, expr: impl Into<String>) -> Self {
        self.put_item_builder = self.put_item_builder.condition_expression(expr);
        self
    }
}

/// Input for [`crate::GeoDataManager::get_point`].
#[derive(Debug, Clone)]
pub struct GetPointInput {
    /// Point coordinates (used to compute partition `hashKey`).
    pub geo_point: GeoPoint,
    /// Item sort key (`rangeKey`) value.
    pub range_key_value: AttributeValue,
    /// Optional `GetItemInputBuilder` (for `ConsistentRead`, `ProjectionExpression`, etc.).
    pub get_item_builder: GetItemInputBuilder,
}

impl GetPointInput {
    /// Creates a `GetPointInput` with a string `rangeKey`.
    #[must_use]
    pub fn new(geo_point: GeoPoint, range_key_value: impl Into<String>) -> Self {
        Self {
            geo_point,
            range_key_value: AttributeValue::S(range_key_value.into()),
            get_item_builder: GetItemInput::builder(),
        }
    }

    /// Supplies a custom `GetItemInputBuilder` (e.g., `.consistent_read(true)`).
    #[must_use]
    pub fn with_get_item_builder(mut self, builder: GetItemInputBuilder) -> Self {
        self.get_item_builder = builder;
        self
    }
}

/// Input for [`crate::GeoDataManager::update_point`].
///
/// Note: `hashKey`, `rangeKey`, `geohash`, and `geoJson` must not be modified in place;
/// to move a point's coordinates, delete the old record and insert a new one.
#[derive(Debug, Clone)]
pub struct UpdatePointInput {
    /// Point coordinates (used to compute partition `hashKey`).
    pub geo_point: GeoPoint,
    /// Item sort key (`rangeKey`) value.
    pub range_key_value: AttributeValue,
    /// Underlying `UpdateItemInputBuilder` specifying `UpdateExpression`, `ConditionExpression`, etc.
    pub update_item_builder: UpdateItemInputBuilder,
}

impl UpdatePointInput {
    /// Creates an `UpdatePointInput` with a string `rangeKey` and a preconfigured `UpdateItemInputBuilder`.
    #[must_use]
    pub fn new(
        geo_point: GeoPoint,
        range_key_value: impl Into<String>,
        update_item_builder: UpdateItemInputBuilder,
    ) -> Self {
        Self {
            geo_point,
            range_key_value: AttributeValue::S(range_key_value.into()),
            update_item_builder,
        }
    }

    /// Creates an empty `UpdatePointInput` that can be customized via [`Self::with_update_item_builder`].
    #[must_use]
    pub fn empty(geo_point: GeoPoint, range_key_value: impl Into<String>) -> Self {
        Self::new(geo_point, range_key_value, UpdateItemInput::builder())
    }

    /// Replaces the `UpdateItemInputBuilder`.
    #[must_use]
    pub fn with_update_item_builder(mut self, builder: UpdateItemInputBuilder) -> Self {
        self.update_item_builder = builder;
        self
    }
}

/// Input for [`crate::GeoDataManager::delete_point`].
#[derive(Debug, Clone)]
pub struct DeletePointInput {
    /// Point coordinates (used to compute partition `hashKey`).
    pub geo_point: GeoPoint,
    /// Item sort key (`rangeKey`) value.
    pub range_key_value: AttributeValue,
    /// Optional `DeleteItemInputBuilder` (e.g., `ConditionExpression`, `ReturnValues`).
    pub delete_item_builder: DeleteItemInputBuilder,
}

impl DeletePointInput {
    /// Creates a `DeletePointInput` with a string `rangeKey`.
    #[must_use]
    pub fn new(geo_point: GeoPoint, range_key_value: impl Into<String>) -> Self {
        Self {
            geo_point,
            range_key_value: AttributeValue::S(range_key_value.into()),
            delete_item_builder: DeleteItemInput::builder(),
        }
    }

    /// Supplies a custom `DeleteItemInputBuilder` (e.g., with `condition_expression` or `return_values`).
    #[must_use]
    pub fn with_delete_item_builder(mut self, builder: DeleteItemInputBuilder) -> Self {
        self.delete_item_builder = builder;
        self
    }
}

/// Input for rectangular (bounding-box) spatial queries.
#[derive(Debug, Clone)]
pub struct QueryRectangleRequest {
    min_point: GeoPoint,
    max_point: GeoPoint,
    query_builder: QueryInputBuilder,
}

impl QueryRectangleRequest {
    /// Creates a bounding-box query from `min_point` (southwest) and `max_point` (northeast).
    #[must_use]
    pub fn new(min_point: GeoPoint, max_point: GeoPoint) -> Self {
        Self {
            min_point,
            max_point,
            query_builder: QueryInput::builder(),
        }
    }

    /// Attaches a custom `QueryInputBuilder` (e.g., `filter_expression`, `projection_expression`, `expression_attribute_values`).
    #[must_use]
    pub fn with_query_input(mut self, query_builder: QueryInputBuilder) -> Self {
        self.query_builder = query_builder;
        self
    }

    /// Southwest corner point.
    #[must_use]
    pub const fn min_point(&self) -> GeoPoint {
        self.min_point
    }

    /// Northeast corner point.
    #[must_use]
    pub const fn max_point(&self) -> GeoPoint {
        self.max_point
    }

    /// Underlying `QueryInputBuilder` template cloned per S2 geohash query.
    #[must_use]
    pub const fn query_builder(&self) -> &QueryInputBuilder {
        &self.query_builder
    }
}

/// Input for circular radius spatial queries.
#[derive(Debug, Clone)]
pub struct QueryRadiusRequest {
    center_point: GeoPoint,
    radius_in_meter: f64,
    sort: bool,
    query_builder: QueryInputBuilder,
}

impl QueryRadiusRequest {
    /// Creates a validated radius query around `center_point` within `radius_in_meter` meters (`>= 0.0`).
    pub fn new(center_point: GeoPoint, radius_in_meter: f64) -> Result<Self> {
        if !radius_in_meter.is_finite() || radius_in_meter < 0.0 {
            return Err(GeoError::InvalidParameter {
                name: "radius_in_meter",
                reason: "radius must be finite and non-negative".to_owned(),
            });
        }
        Ok(Self {
            center_point,
            radius_in_meter,
            sort: false,
            query_builder: QueryInput::builder(),
        })
    }

    /// Enables or disables ascending distance sorting from `center_point`.
    #[must_use]
    pub const fn with_sort(mut self, sort: bool) -> Self {
        self.sort = sort;
        self
    }

    /// Attaches a custom `QueryInputBuilder` (e.g., `filter_expression`, `projection_expression`, `expression_attribute_values`).
    #[must_use]
    pub fn with_query_input(mut self, query_builder: QueryInputBuilder) -> Self {
        self.query_builder = query_builder;
        self
    }

    /// Center coordinate of the search circle.
    #[must_use]
    pub const fn center_point(&self) -> GeoPoint {
        self.center_point
    }

    /// Search radius in meters.
    #[must_use]
    pub const fn radius_in_meter(&self) -> f64 {
        self.radius_in_meter
    }

    /// Whether results should be sorted nearest-first by distance from `center_point`.
    #[must_use]
    pub const fn sort(&self) -> bool {
        self.sort
    }

    /// Underlying `QueryInputBuilder` template cloned per S2 geohash query.
    #[must_use]
    pub const fn query_builder(&self) -> &QueryInputBuilder {
        &self.query_builder
    }
}

/// Single page returned by [`crate::GeoDataManager::query_radius_paginated`] or [`crate::GeoDataManager::query_rectangle_paginated`].
#[derive(Debug, Clone, PartialEq)]
pub struct PaginatedQueryOutput {
    /// Matching DynamoDB items for this page (at most `limit` items).
    pub items: Vec<DynamoDbItem>,
    /// Opaque base64 cursor to pass as `next_token` on the subsequent request, or `None` when exhausted.
    pub next_token: Option<String>,
}
