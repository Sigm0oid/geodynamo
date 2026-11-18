//! Store Locator Example: Seed San Francisco coffee shops and query open locations
//! within a 2 km radius ordered nearest-first.
//!
//! ```bash
//! docker run --rm -p 8000:8000 amazon/dynamodb-local
//! AWS_ACCESS_KEY_ID=test AWS_SECRET_ACCESS_KEY=test AWS_REGION=local \
//!   cargo run --example nearby_coffee_shops
//! ```

use aws_sdk_dynamodb::operation::query::QueryInput;
use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::Client;
use geodynamo::{
    GeoDataManager, GeoDataManagerConfiguration, GeoPoint, GeoTableUtil, PutPointInput,
    QueryRadiusRequest,
};
use uuid::Uuid;

#[tokio::main]
async fn main() -> geodynamo::Result<()> {
    let shared_config = aws_config::from_env()
        .endpoint_url("http://localhost:8000")
        .load()
        .await;
    let client = Client::new(&shared_config);

    let config = GeoDataManagerConfiguration::new(client, "coffee_shops_demo")
        .with_hash_key_length(5);

    let table_util = GeoTableUtil::new(config.clone());
    table_util.create_table(None).await?;

    let geo = GeoDataManager::new(config);

    let shops = [
        ("Blue Bottle Ferry Building", 37.7955, -122.3937, true, "4.7"),
        ("Sightglass SoMa", 37.7771, -122.4084, true, "4.6"),
        ("Ritual Coffee Valencia", 37.7565, -122.4212, false, "4.5"),
        ("Philz Embarcadero", 37.7999, -122.3982, true, "4.8"),
    ];

    for (name, lat, lng, is_open, rating) in shops {
        geo.put_point(
            PutPointInput::new(GeoPoint::new(lat, lng)?, Uuid::new_v4().to_string())
                .item("name", AttributeValue::S(name.into()))
                .item("is_open", AttributeValue::Bool(is_open))
                .item("rating", AttributeValue::N(rating.into())),
        )
        .await?;
    }

    let filter = QueryInput::builder()
        .filter_expression("is_open = :open")
        .expression_attribute_values(":open", AttributeValue::Bool(true));

    let results = geo
        .query_radius(
            QueryRadiusRequest::new(GeoPoint::new(37.7894, -122.4013)?, 2_000.0)?
                .with_query_input(filter)
                .with_sort(true),
        )
        .await?;

    println!("Open coffee shops within 2 km (nearest first):");
    for (idx, item) in results.iter().enumerate() {
        let name = item
            .get("name")
            .and_then(|v| v.as_s().ok())
            .map_or("unknown", String::as_str);
        let coords = item
            .get("geoJson")
            .and_then(|v| v.as_s().ok())
            .map_or("", String::as_str);
        println!("  {}. {} @ ({})", idx + 1, name, coords);
    }

    table_util.delete_table().await?;
    Ok(())
}
