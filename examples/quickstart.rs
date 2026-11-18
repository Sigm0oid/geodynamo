//! End-to-end Quick Start demonstrating table setup, point insertion, radius search, and cleanup
//! using the official AWS SDK for Rust (`aws-config` + `aws-sdk-dynamodb`).
//!
//! Run against DynamoDB Local:
//! ```bash
//! docker run --rm -p 8000:8000 amazon/dynamodb-local
//! AWS_ACCESS_KEY_ID=test AWS_SECRET_ACCESS_KEY=test AWS_REGION=local \
//!   cargo run --example quickstart
//! ```

use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::Client;
use geodynamo::{
    GeoDataManager, GeoDataManagerConfiguration, GeoPoint, GeoTableUtil, PutPointInput,
    QueryRadiusRequest,
};
use uuid::Uuid;

#[tokio::main]
async fn main() -> geodynamo::Result<()> {
    let endpoint =
        std::env::var("DYNAMODB_ENDPOINT").unwrap_or_else(|_| "http://localhost:8000".to_owned());
    let shared_config = aws_config::from_env()
        .endpoint_url(endpoint)
        .load()
        .await;
    let client = Client::new(&shared_config);

    let config = GeoDataManagerConfiguration::new(client, "places_quickstart")
        .with_hash_key_length(4);

    let table_util = GeoTableUtil::new(config.clone());
    let create_req = table_util.get_create_table_on_demand_request()?;
    table_util.create_table(Some(create_req)).await?;

    let manager = GeoDataManager::new(config);

    let point_id = Uuid::new_v4().to_string();
    manager
        .put_point(
            PutPointInput::new(GeoPoint::new(36.879_163, 10.243_120)?, &point_id)
                .item("name", AttributeValue::S("Carthage Station".into()))
                .item("country", AttributeValue::S("Tunisia".into()))
                .condition_expression("attribute_not_exists(hashKey)"),
        )
        .await?;

    let query = aws_sdk_dynamodb::operation::query::QueryInput::builder()
        .filter_expression("country = :country")
        .expression_attribute_values(":country", AttributeValue::S("Tunisia".into()));

    let nearby = manager
        .query_radius(
            QueryRadiusRequest::new(GeoPoint::new(36.879_131, 10.243_057)?, 500.0)?
                .with_query_input(query)
                .with_sort(true),
        )
        .await?;

    println!("Matched {} nearby item(s):", nearby.len());
    for item in &nearby {
        println!("  - {:?}", item.get("name"));
    }

    table_util.delete_table().await?;
    Ok(())
}
