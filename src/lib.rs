//! # geodynamo
//!
//! Geospatial indexing, CRUD, radius/bounding-box queries, and cursor pagination for
//! **Amazon DynamoDB** built on Google S2 Geometry and the official [`aws_sdk_dynamodb`] crate.
//!
//! ## Quick Example
//!
//! ```no_run
//! use aws_sdk_dynamodb::types::AttributeValue;
//! use aws_sdk_dynamodb::Client;
//! use geodynamo::{
//!     GeoDataManager, GeoDataManagerConfiguration, GeoPoint, GeoTableUtil, PutPointInput,
//!     QueryRadiusRequest,
//! };
//!
//! #[tokio::main]
//! async fn main() -> geodynamo::Result<()> {
//!     let sdk_config = aws_config::load_from_env().await;
//!     let client = Client::new(&sdk_config);
//!
//!     let config = GeoDataManagerConfiguration::new(client, "places_geo")
//!         .with_hash_key_length(4);
//!
//!     let table_util = GeoTableUtil::new(config.clone());
//!     table_util.create_table(None).await?;
//!
//!     let manager = GeoDataManager::new(config);
//!     manager
//!         .put_point(
//!             PutPointInput::new(GeoPoint::new(36.879_163, 10.243_120)?, "point-1")
//!                 .item("name", AttributeValue::S("Carthage Station".into())),
//!         )
//!         .await?;
//!
//!     let nearby = manager
//!         .query_radius(
//!             QueryRadiusRequest::new(GeoPoint::new(36.879_131, 10.243_057)?, 500.0)?
//!                 .with_sort(true),
//!         )
//!         .await?;
//!
//!     println!("Found {} nearby points", nearby.len());
//!     Ok(())
//! }
//! ```

pub mod config;
pub mod dynamodb_manager;
pub mod error;
pub mod manager;
pub mod model;
mod pagination;
pub mod s2;
pub mod util;

pub use config::{CoordinateFormat, GeoDataManagerConfiguration, GeohashEncoding};
pub use dynamodb_manager::DynamoDBManager;
pub use error::{GeoError, Result};
pub use manager::GeoDataManager;
pub use model::requests::DynamoDbItem;
pub use model::{
    Covering, DeletePointInput, GeoPoint, GeohashRange, GetPointInput, PaginatedQueryOutput,
    PutPointInput, QueryRadiusRequest, QueryRectangleRequest, UpdatePointInput,
};
pub use util::GeoTableUtil;
