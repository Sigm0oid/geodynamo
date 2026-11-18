//! Error types returned by `geodynamo` operations.

use thiserror::Error;

/// Result alias for `geodynamo` operations.
pub type Result<T> = std::result::Result<T, GeoError>;

/// Errors that can occur during geospatial validation, cursor decoding, or DynamoDB calls.
#[derive(Debug, Error)]
pub enum GeoError {
    /// Coordinates fell outside valid geographic bounds (`[-90, 90]` lat, `[-180, 180]` lng) or were non-finite.
    #[error("invalid coordinates (latitude={latitude}, longitude={longitude}): {reason}")]
    InvalidCoordinates {
        /// Provided latitude in degrees.
        latitude: f64,
        /// Provided longitude in degrees.
        longitude: f64,
        /// Explanation of why validation failed.
        reason: &'static str,
    },

    /// Radius or configuration parameter was invalid.
    #[error("invalid parameter `{name}`: {reason}")]
    InvalidParameter {
        /// Parameter name.
        name: &'static str,
        /// Validation failure reason.
        reason: String,
    },

    /// Stored `geoJson` attribute on a DynamoDB item was missing or malformed.
    #[error("missing or malformed `{attribute}` attribute on DynamoDB item: {details}")]
    MalformedGeoAttribute {
        /// Configured coordinate attribute name.
        attribute: String,
        /// Details of the parse error.
        details: String,
    },

    /// Pagination cursor token could not be decoded or did not match the query mode.
    #[error("invalid pagination token: {0}")]
    InvalidPaginationToken(String),

    /// Underlying AWS SDK DynamoDB error.
    #[error("DynamoDB SDK error: {0}")]
    DynamoDb(#[from] aws_sdk_dynamodb::Error),

    /// Underlying AWS SDK builder error when constructing key schemas or requests.
    #[error("DynamoDB request build error: {0}")]
    Build(#[from] aws_sdk_dynamodb::error::BuildError),
}
