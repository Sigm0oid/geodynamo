//! Map Viewport Pagination Example: Stream bounding-box markers with `ProjectionExpression`
//! and cursor tokens (`limit` + `next_token`).
//!
//! ```bash
//! docker run --rm -p 8000:8000 amazon/dynamodb-local
//! AWS_ACCESS_KEY_ID=test AWS_SECRET_ACCESS_KEY=test AWS_REGION=local \
//!   cargo run --example map_viewport_pagination
//! ```

use aws_sdk_dynamodb::operation::query::QueryInput;
use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::Client;
use geodynamo::{
    GeoDataManager, GeoDataManagerConfiguration, GeoPoint, GeoTableUtil, PutPointInput,
    QueryRectangleRequest,
};

#[tokio::main]
async fn main() -> geodynamo::Result<()> {
    let shared_config = aws_config::from_env()
        .endpoint_url("http://localhost:8000")
        .load()
        .await;
    let client = Client::new(&shared_config);

    let config = GeoDataManagerConfiguration::new(client, "viewport_markers_demo")
        .with_hash_key_length(4);

    let table_util = GeoTableUtil::new(config.clone());
    table_util.create_table(None).await?;

    let geo = GeoDataManager::new(config);

    for i in 0..10_u32 {
        let lat = 36.8750 + f64::from(i) * 0.0010;
        let lng = 10.2400 + f64::from(i) * 0.0010;
        geo.put_point(
            PutPointInput::new(GeoPoint::new(lat, lng)?, format!("landmark-{}", i + 1))
                .item("name", AttributeValue::S(format!("Landmark #{}", i + 1)))
                .item("category", AttributeValue::S("museum".into())),
        )
        .await?;
    }

    let projection = QueryInput::builder()
        .projection_expression("rangeKey, geoJson, #n, category")
        .expression_attribute_names("#n", "name");

    let request = QueryRectangleRequest::new(
        GeoPoint::new(36.8740, 10.2390)?,
        GeoPoint::new(36.8860, 10.2520)?,
    )
    .with_query_input(projection);

    let mut page_num = 1_usize;
    let mut cursor: Option<String> = None;

    loop {
        let page = geo
            .query_rectangle_paginated(request.clone(), 3, cursor.as_deref())
            .await?;

        let names: Vec<&str> = page
            .items
            .iter()
            .filter_map(|m| m.get("name").and_then(|v| v.as_s().ok()).map(String::as_str))
            .collect();
        println!("Page {page_num}: {names:?}");

        cursor = page.next_token;
        if cursor.is_none() {
            break;
        }
        page_num += 1;
    }

    table_util.delete_table().await?;
    Ok(())
}
