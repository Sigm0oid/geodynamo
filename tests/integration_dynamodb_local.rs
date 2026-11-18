//! Live integration test suite exercising `geodynamo` against DynamoDB Local (`http://localhost:8000`).
//!
//! Marked `#[ignore]` by default so `cargo test` succeeds offline; executed in CI via:
//! `cargo test --test integration_dynamodb_local -- --include-ignored`

use aws_sdk_dynamodb::operation::query::QueryInput;
use aws_sdk_dynamodb::operation::update_item::UpdateItemInput;
use aws_sdk_dynamodb::types::{AttributeValue, ReturnValue};
use aws_sdk_dynamodb::Client;
use geodynamo::{
    DeletePointInput, GeoDataManager, GeoDataManagerConfiguration, GeoPoint, GeoTableUtil,
    GetPointInput, PutPointInput, QueryRadiusRequest, QueryRectangleRequest, UpdatePointInput,
};
use uuid::Uuid;

async fn local_client() -> Client {
    let endpoint =
        std::env::var("DYNAMODB_ENDPOINT").unwrap_or_else(|_| "http://localhost:8000".to_owned());
    let config = aws_config::from_env()
        .endpoint_url(endpoint)
        .region(aws_config::Region::new("local"))
        .load()
        .await;
    Client::new(&config)
}

#[tokio::test]
#[ignore = "requires DynamoDB Local on port 8000"]
async fn crud_box_radius_batch_and_pagination_end_to_end() -> geodynamo::Result<()> {
    let client = local_client().await;
    let table_name = format!("geo_it_{}", Uuid::new_v4().simple());
    let config = GeoDataManagerConfiguration::new(client.clone(), &table_name)
        .with_hash_key_length(3);

    let table_util = GeoTableUtil::new(config.clone());
    let created = table_util.create_table(None).await?;
    assert!(created.is_some());
    // Idempotent second call skips cleanly
    let second = table_util.create_table(None).await?;
    assert!(second.is_none());

    let geo = GeoDataManager::new(config);

    // 1. Put + Get + Update
    let inside_pt = GeoPoint::new(36.879_163, 10.243_120)?;
    let outside_pt = GeoPoint::new(36.895_000, 10.260_000)?;

    geo.put_point(
        PutPointInput::new(inside_pt, "inside")
            .item("Country", AttributeValue::S("Tunisia".into()))
            .item("Capital", AttributeValue::S("Ariana".into()))
            .condition_expression("attribute_not_exists(hashKey)"),
    )
    .await?;

    geo.put_point(
        PutPointInput::new(outside_pt, "outside")
            .item("Country", AttributeValue::S("Tunisia".into())),
    )
    .await?;

    let fetched = geo.get_point(GetPointInput::new(inside_pt, "inside")).await?;
    assert!(fetched.item.is_some());

    let update_builder = UpdateItemInput::builder()
        .update_expression("SET Capital = :new_val")
        .expression_attribute_values(":new_val", AttributeValue::S("Tunis".into()))
        .return_values(ReturnValue::AllNew);
    let updated = geo
        .update_point(UpdatePointInput::new(inside_pt, "inside", update_builder))
        .await?;
    assert_eq!(
        updated
            .attributes
            .as_ref()
            .and_then(|a| a.get("Capital"))
            .and_then(|v| v.as_s().ok())
            .map(String::as_str),
        Some("Tunis")
    );

    // 2. Rectangle query with FilterExpression
    let rect_filter = QueryInput::builder()
        .filter_expression("Country = :c")
        .expression_attribute_values(":c", AttributeValue::S("Tunisia".into()));
    let box_items = geo
        .query_rectangle(
            QueryRectangleRequest::new(
                GeoPoint::new(36.878_184, 10.242_358)?,
                GeoPoint::new(36.879_317, 10.243_648)?,
            )
            .with_query_input(rect_filter),
        )
        .await?;
    assert_eq!(box_items.len(), 1);
    assert_eq!(box_items[0].get("rangeKey").unwrap().as_s().unwrap(), "inside");

    // 3. Batch write + Sorted Radius query + Paginated Radius query
    let batch_pts = vec![
        PutPointInput::new(GeoPoint::new(36.874_419, 10.241_062)?, "r1"),
        PutPointInput::new(GeoPoint::new(36.874_507, 10.240_857)?, "r2"),
        PutPointInput::new(GeoPoint::new(36.874_617, 10.241_441)?, "r3"),
    ];
    let unprocessed = geo.batch_write_points(batch_pts).await?;
    assert!(unprocessed.is_empty());

    let center = GeoPoint::new(36.874_444, 10.241_059)?;
    let sorted = geo
        .query_radius(QueryRadiusRequest::new(center, 95.0)?.with_sort(true))
        .await?;
    let keys: Vec<&str> = sorted
        .iter()
        .map(|i| i.get("rangeKey").unwrap().as_s().unwrap().as_str())
        .collect();
    assert_eq!(keys, vec!["r1", "r2", "r3"]);

    // Streaming pagination (sort = false, limit = 2)
    let mut streamed = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let page = geo
            .query_radius_paginated(
                QueryRadiusRequest::new(center, 95.0)?,
                2,
                token.as_deref(),
            )
            .await?;
        streamed.extend(page.items);
        token = page.next_token;
        if token.is_none() {
            break;
        }
    }
    assert_eq!(streamed.len(), 3);

    // 4. Antimeridian (±180°) radius query
    geo.put_point(PutPointInput::new(GeoPoint::new(-42.0, 179.6)?, "nz-west"))
        .await?;
    geo.put_point(PutPointInput::new(GeoPoint::new(-42.0, -179.6)?, "nz-east"))
        .await?;
    let antimeridian = geo
        .query_radius(QueryRadiusRequest::new(GeoPoint::new(-42.0, 179.9)?, 100_000.0)?)
        .await?;
    assert_eq!(antimeridian.len(), 2);

    // 5. Delete point & delete table
    geo.delete_point(DeletePointInput::new(inside_pt, "inside")).await?;
    let after_del = geo.get_point(GetPointInput::new(inside_pt, "inside")).await?;
    assert!(after_del.item.is_none());

    let deleted_table = table_util.delete_table().await?;
    assert!(deleted_table.is_some());
    Ok(())
}
