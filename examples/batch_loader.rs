//! Batch Write Example: Bulk-load 60 geospatial points across DynamoDB `BatchWriteItem` chunks.
//!
//! ```bash
//! docker run --rm -p 8000:8000 amazon/dynamodb-local
//! AWS_ACCESS_KEY_ID=test AWS_SECRET_ACCESS_KEY=test AWS_REGION=local \
//!   cargo run --example batch_loader
//! ```

use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::Client;
use geodynamo::{
    GeoDataManager, GeoDataManagerConfiguration, GeoPoint, GeoTableUtil, PutPointInput,
    QueryRadiusRequest,
};

#[tokio::main]
async fn main() -> geodynamo::Result<()> {
    let shared_config = aws_config::from_env()
        .endpoint_url("http://localhost:8000")
        .load()
        .await;
    let client = Client::new(&shared_config);

    let config = GeoDataManagerConfiguration::new(client, "batch_points_demo")
        .with_hash_key_length(4);

    let table_util = GeoTableUtil::new(config.clone());
    let req = table_util.get_create_table_on_demand_request()?;
    table_util.create_table(Some(req)).await?;

    let geo = GeoDataManager::new(config);

    let mut batch = Vec::with_capacity(60);
    for idx in 0..60_u32 {
        let lat = 48.8566 + (f64::from(idx) - 30.0) * 0.0005;
        let lng = 2.3522 + (f64::from(idx) - 30.0) * 0.0005;
        batch.push(
            PutPointInput::new(GeoPoint::new(lat, lng)?, format!("sensor-{idx}"))
                .item("city", AttributeValue::S("Paris".into())),
        );
    }

    let unprocessed = geo.batch_write_points(batch).await?;
    println!("Batch load finished (unprocessed items: {})", unprocessed.len());

    let found = geo
        .query_radius(QueryRadiusRequest::new(GeoPoint::new(48.8566, 2.3522)?, 3_000.0)?)
        .await?;
    println!("Queried {} sensors within 3 km of central Paris", found.len());

    table_util.delete_table().await?;
    Ok(())
}
