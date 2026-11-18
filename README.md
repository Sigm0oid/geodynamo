# geodynamo — Geospatial Library for Amazon DynamoDB in Rust

[![CI](https://github.com/Sigm0oid/geodynamo/actions/workflows/ci.yml/badge.svg)](https://github.com/Sigm0oid/geodynamo/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/geodynamo.svg)](https://crates.io/crates/geodynamo)
[![Docs.rs](https://docs.rs/geodynamo/badge.svg)](https://docs.rs/geodynamo)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

`geodynamo` is an asynchronous, `unsafe`-free Rust geospatial indexing and query library for [Amazon DynamoDB][dynamodb], built directly on the **official AWS SDK for Rust** ([`aws-sdk-dynamodb`][aws-sdk-dynamodb]) and Google S2 Geometry. It is a full-featured Rust port of [`awslabs/dynamodb-geo`][awslabs-geo] and [`dynamodb-geo.py`][python-geo].

---

## Features

- **Official AWS SDK Integration:** Wraps official `aws_sdk_dynamodb` v1 fluent builders (`PutItemInputBuilder`, `UpdateItemInputBuilder`, `DeleteItemInputBuilder`, `QueryInputBuilder`, `CreateTableInputBuilder`) so you retain full compile-time access to `ConditionExpression`, `FilterExpression`, `ProjectionExpression`, `ReturnValues`, `BillingMode`, and SDK waiters.
- **Radius & Bounding-Box Queries:** Concurrent S2 geohash range fan-out across DynamoDB Local Secondary Index partitions with exact spherical post-filtering, optional nearest-first sorting (`with_sort(true)`), and accurate handling across the `±180°` antimeridian and `±90°` poles.
- **Cursor-Based Pagination:** Stream spatial results page-by-page with `query_radius_paginated` and `query_rectangle_paginated` (`limit` + `next_token`), preserving all DynamoDB `AttributeValue` types losslessly while bounding memory and Read Capacity Unit (RCU) consumption.
- **CRUD & Batch Writes:** `put_point`, `batch_write_points` (auto-chunked into 25-item `BatchWriteItem` calls with retry on `UnprocessedItems`), `get_point`, `update_point`, and `delete_point`.
- **Cross-Language Table Compatibility:** Supports both unsigned 64-bit geohashes + `"lat,lng"` coordinates (compatible with [`dynamodb-geo.py`][python-geo]) and signed 64-bit geohashes + GeoJSON Point objects (compatible with Java [`awslabs/dynamodb-geo`][awslabs-geo] and JS `dynamodb-geo.js`).
- **Zero `unsafe` & Pure-Rust S2 Engine:** Self-contained 64-bit Hilbert-curve `S2CellId`, `S2LatLng`, `S2LatLngRect`, `S2Cap`, and `S2RegionCoverer` with `#![forbid(unsafe_code)]`.

---

## How It Works

1. **Indexing on Write:** Each `(latitude, longitude)` point is projected onto a level-30 Google S2 leaf cell (`64-bit` geohash). The leading `hash_key_length` decimal digits form the DynamoDB partition key (`hashKey`), your item identifier forms the sort key (`rangeKey`), and a Local Secondary Index (`geohash-index`) indexes `(hashKey, geohash)`.
2. **Querying on Read:** Given a search circle or bounding rectangle, `S2RegionCoverer` computes a compact covering of S2 cells, merges adjacent cell ID ranges, splits ranges along `hashKey` boundaries, dispatches `BETWEEN` queries concurrently against `geohash-index`, and post-filters candidates using exact Haversine / spherical bounds.

---

## Installation

Add `geodynamo`, `aws-config`, `aws-sdk-dynamodb`, and `tokio` to your `Cargo.toml`:

```toml
[dependencies]
geodynamo = "0.1"
aws-config = { version = "1", features = ["behavior-version-latest"] }
aws-sdk-dynamodb = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

---

## Quick Start

```rust
use aws_sdk_dynamodb::operation::query::QueryInput;
use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::Client;
use geodynamo::{
    GeoDataManager, GeoDataManagerConfiguration, GeoPoint, GeoTableUtil, PutPointInput,
    QueryRadiusRequest,
};

#[tokio::main]
async fn main() -> geodynamo::Result<()> {
    // 1. Initialize the official AWS SDK DynamoDB client
    let sdk_config = aws_config::load_from_env().await;
    let client = Client::new(&sdk_config);

    // 2. Configure the geospatial table
    let config = GeoDataManagerConfiguration::new(client, "places_geo")
        .with_hash_key_length(4);

    // 3. Provision the table (with On-Demand capacity & geohash-index LSI) if needed
    let table_util = GeoTableUtil::new(config.clone());
    let create_req = table_util.get_create_table_on_demand_request()?;
    table_util.create_table(Some(create_req)).await?;

    // 4. Create the GeoDataManager
    let manager = GeoDataManager::new(config);

    // 5. Insert a point (latitude first, then longitude)
    manager
        .put_point(
            PutPointInput::new(GeoPoint::new(36.879_163, 10.243_120)?, "carthage-1")
                .item("name", AttributeValue::S("Carthage Station".into()))
                .item("country", AttributeValue::S("Tunisia".into()))
                .condition_expression("attribute_not_exists(hashKey)"),
        )
        .await?;

    // 6. Query within 500 meters, filtered by attribute and sorted nearest-first
    let filter = QueryInput::builder()
        .filter_expression("country = :country")
        .expression_attribute_values(":country", AttributeValue::S("Tunisia".into()));

    let nearby = manager
        .query_radius(
            QueryRadiusRequest::new(GeoPoint::new(36.879_131, 10.243_057)?, 500.0)?
                .with_query_input(filter)
                .with_sort(true),
        )
        .await?;

    println!("Found {} nearby point(s)", nearby.len());
    Ok(())
}
```

---

## Usage Guide

> **Coordinate Convention:** `GeoPoint::new(latitude, longitude)` always expects **`latitude` first** (`-90.0..=90.0`) and **`longitude` second** (`-180.0..=180.0`) in decimal degrees.

### 1. Creating & Deleting Tables (`GeoTableUtil`)

```rust
use aws_sdk_dynamodb::types::ProvisionedThroughput;
use geodynamo::GeoTableUtil;

let table_util = GeoTableUtil::new(config.clone());

// Option A: On-Demand (PAY_PER_REQUEST)
let on_demand = table_util.get_create_table_on_demand_request()?;
table_util.create_table(Some(on_demand)).await?;

// Option B: Custom Provisioned Throughput (defaults to 10 RCU / 10 WCU)
let provisioned = table_util
    .get_create_table_request()?
    .provisioned_throughput(
        ProvisionedThroughput::builder()
            .read_capacity_units(25)
            .write_capacity_units(25)
            .build()?,
    );
table_util.create_table(Some(provisioned)).await?;

// Clean up table when finished
table_util.delete_table().await?;
```

### 2. Adding & Batch-Loading Points (`put_point` & `batch_write_points`)

```rust
use aws_sdk_dynamodb::types::AttributeValue;
use geodynamo::{GeoPoint, PutPointInput};

// Single point write with ConditionExpression
manager
    .put_point(
        PutPointInput::new(GeoPoint::new(36.879_163, 10.243_120)?, "point-uuid-1")
            .item("Country", AttributeValue::S("Tunisia".into()))
            .item("Capital", AttributeValue::S("Tunis".into()))
            .condition_expression("attribute_not_exists(hashKey)"),
    )
    .await?;

// Bulk write (automatically chunked into 25-item DynamoDB BatchWriteItem requests)
let batch = vec![
    PutPointInput::new(GeoPoint::new(48.8566, 2.3522)?, "paris-1")
        .item("name", AttributeValue::S("Louvre".into())),
    PutPointInput::new(GeoPoint::new(48.8584, 2.2945)?, "paris-2")
        .item("name", AttributeValue::S("Eiffel Tower".into())),
];
let unprocessed = manager.batch_write_points(batch).await?;
assert!(unprocessed.is_empty());
```

### 3. Retrieving, Updating & Deleting a Point

> **Important:** Do **not** modify `hashKey`, `rangeKey`, `geohash`, or `geoJson` via `update_point`. To move an entity to new coordinates, delete the old point and insert the updated point.

```rust
use aws_sdk_dynamodb::operation::delete_item::DeleteItemInput;
use aws_sdk_dynamodb::operation::get_item::GetItemInput;
use aws_sdk_dynamodb::operation::update_item::UpdateItemInput;
use aws_sdk_dynamodb::types::{AttributeValue, ReturnValue};
use geodynamo::{DeletePointInput, GeoPoint, GetPointInput, UpdatePointInput};

let coords = GeoPoint::new(36.879_163, 10.243_120)?;

// Strongly consistent GetItem
let get_out = manager
    .get_point(
        GetPointInput::new(coords, "point-uuid-1")
            .with_get_item_builder(GetItemInput::builder().consistent_read(true)),
    )
    .await?;

// Update non-key attributes
let update_builder = UpdateItemInput::builder()
    .update_expression("SET Capital = :cap")
    .expression_attribute_values(":cap", AttributeValue::S("Tunis".into()))
    .return_values(ReturnValue::AllNew);
manager
    .update_point(UpdatePointInput::new(coords, "point-uuid-1", update_builder))
    .await?;

// Conditional DeleteItem
let delete_builder = DeleteItemInput::builder()
    .condition_expression("attribute_exists(Country)");
manager
    .delete_point(
        DeletePointInput::new(coords, "point-uuid-1")
            .with_delete_item_builder(delete_builder),
    )
    .await?;
```

### 4. Rectangular & Radius Queries

```rust
use aws_sdk_dynamodb::operation::query::QueryInput;
use aws_sdk_dynamodb::types::AttributeValue;
use geodynamo::{GeoPoint, QueryRadiusRequest, QueryRectangleRequest};

// Bounding-box search (minPoint = SW, maxPoint = NE)
let in_box = manager
    .query_rectangle(
        QueryRectangleRequest::new(
            GeoPoint::new(36.878_184, 10.242_358)?,
            GeoPoint::new(36.879_317, 10.243_648)?,
        )
        .with_query_input(
            QueryInput::builder()
                .filter_expression("Country = :c")
                .expression_attribute_values(":c", AttributeValue::S("Tunisia".into())),
        ),
    )
    .await?;

// Radius search within 2,000 meters, sorted nearest-first
let in_radius = manager
    .query_radius(
        QueryRadiusRequest::new(GeoPoint::new(36.879_131, 10.243_057)?, 2_000.0)?
            .with_sort(true),
    )
    .await?;
```

### 5. Cursor-Based Pagination (`query_radius_paginated` & `query_rectangle_paginated`)

```rust
use geodynamo::{GeoPoint, QueryRadiusRequest};

let request = QueryRadiusRequest::new(GeoPoint::new(36.879_131, 10.243_057)?, 5_000.0)?;
let mut cursor: Option<String> = None;

loop {
    let page = manager
        .query_radius_paginated(request.clone(), 25, cursor.as_deref())
        .await?;

    for item in &page.items {
        println!("Item: {:?}", item.get("rangeKey"));
    }

    cursor = page.next_token;
    if cursor.is_none() {
        break;
    }
}
```

> **Note:** When `sort = false` (default) and in `query_rectangle_paginated`, pagination streams lazily across S2 ranges and DynamoDB pages, stopping immediately once `limit` matching items are collected. When `sort = true`, all candidates within the radius are fetched and sorted by distance before slicing by cursor offset.

---

## Examples

Runnable example binaries using the official AWS SDK are located in [`examples/`](examples/):

| Example | Description | Command |
| :--- | :--- | :--- |
| [`quickstart.rs`](examples/quickstart.rs) | End-to-end table creation, `put_point`, filtered/sorted `query_radius`, and table cleanup | `cargo run --example quickstart` |
| [`nearby_coffee_shops.rs`](examples/nearby_coffee_shops.rs) | Store locator finding open coffee shops within 2 km ordered nearest-first | `cargo run --example nearby_coffee_shops` |
| [`map_viewport_pagination.rs`](examples/map_viewport_pagination.rs) | Paginated bounding-box viewport search with `ProjectionExpression` | `cargo run --example map_viewport_pagination` |
| [`batch_loader.rs`](examples/batch_loader.rs) | Bulk-loading 60 sensor coordinates via `batch_write_points` | `cargo run --example batch_loader` |

---

## Choosing `hash_key_length` & Cross-Language Compatibility

`hash_key_length` sets how many leading decimal digits of the 64-bit geohash form the partition key (`hashKey`):

| Typical Search Radius | Suggested `hash_key_length` |
| :--- | :--- |
| ~100 km+ (regional) | `2` – `3` |
| ~10 km (city-wide) | `4` – `5` |
| ~1 km or less (neighborhood) | `5` – `6` |

### Sharing Tables with Python, Java, or JavaScript

| Runtime Library | Geohash Sign | Default `hashKeyLength` | `geoJson` Format | Rust Configuration |
| :--- | :--- | :--- | :--- | :--- |
| **`dynamodb-geo.py`** (Python) | Unsigned `u64` | `2` | `"<lat>,<lng>"` | `GeoDataManagerConfiguration::new(client, table)` *(default)* |
| **`awslabs/dynamodb-geo`** (Java / JS) | Signed `i64` | `6` | `{"type":"Point","coordinates":[lng,lat]}` | `GeoDataManagerConfiguration::new(client, table).with_java_js_defaults()` |

*(On read, `geodynamo` automatically parses both `"<lat>,<lng>"` and GeoJSON Point strings.)*

---

## Configuration Reference

| Builder Method | Field | Default | Description |
| :--- | :--- | :--- | :--- |
| `GeoDataManagerConfiguration::new` | `client`, `table_name` | *(required)* | Official `aws_sdk_dynamodb::Client` and table name. |
| `.with_hash_key_length(usize)` | `hash_key_length` | `2` | Leading base-10 digits of `geohash` used as `hashKey`. |
| `.with_hash_key_attribute_name(...)` | `hash_key_attribute_name` | `"hashKey"` | Partition key attribute name (`N`). |
| `.with_range_key_attribute_name(...)` | `range_key_attribute_name` | `"rangeKey"` | Sort key attribute name (`S`). |
| `.with_geohash_attribute_name(...)` | `geohash_attribute_name` | `"geohash"` | Full 64-bit S2 geohash attribute name (`N`). |
| `.with_geojson_attribute_name(...)` | `geojson_attribute_name` | `"geoJson"` | Coordinate string attribute name (`S`). |
| `.with_geohash_index_name(...)` | `geohash_index_name` | `"geohash-index"` | Local Secondary Index name. |
| `.with_max_cells(usize)` | `max_cells` | `8` | Max S2 cells generated per spatial query covering. |
| `.with_geohash_encoding(...)` | `geohash_encoding` | `Unsigned` | `GeohashEncoding::Unsigned` (`u64`) or `Signed` (`i64`). |
| `.with_coordinate_format(...)` | `coordinate_format` | `LatLngCsv` | `CoordinateFormat::LatLngCsv` or `GeoJsonPoint`. |
| `.with_java_js_defaults()` | *(preset)* | — | Sets `hash_key_length = 6`, `Signed`, and `GeoJsonPoint`. |

---

## Local Development & Running Tests

```bash
# Run offline unit tests (S2 geometry, range splitting, cursor codec)
cargo test --lib

# Start DynamoDB Local and run end-to-end integration tests
docker run --rm -p 8000:8000 amazon/dynamodb-local
AWS_ACCESS_KEY_ID=test AWS_SECRET_ACCESS_KEY=test AWS_REGION=local \
  cargo test --test integration_dynamodb_local -- --include-ignored
```

---

## License

Licensed under the [MIT License](LICENSE).

[dynamodb]: https://aws.amazon.com/dynamodb/
[aws-sdk-dynamodb]: https://docs.rs/aws-sdk-dynamodb/latest/aws_sdk_dynamodb/
[awslabs-geo]: https://github.com/awslabs/dynamodb-geo
[python-geo]: https://github.com/Sigm0oid/dynamodb-geo.py
