# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-11-18

### Added
- Initial Rust port of `geodynamo` built on the official `aws-sdk-dynamodb` v1 SDK.
- Pure-Rust, `unsafe`-free Google S2 Geometry implementation (`CellId`, `LatLng`, `LatLngRect`, `Cap`, and `RegionCoverer`) with exact polar (`±90°`) and antimeridian (`±180°`) handling.
- Configurable unsigned (`u64`, compatible with `dynamodb-geo.py`) and signed (`i64`, compatible with `awslabs/dynamodb-geo` Java/JS) geohash encodings.
- `GeoDataManager` supporting `put_point`, `batch_write_points`, `get_point`, `update_point`, `delete_point`, `query_rectangle`, `query_radius` (with optional nearest-first sorting), `query_rectangle_paginated`, and `query_radius_paginated`.
- Concurrent S2 range query fan-out across DynamoDB Local Secondary Index partitions.
- Lossless base64-encoded JSON cursor pagination supporting all DynamoDB `AttributeValue` types.
- `GeoTableUtil` helpers for provisioned and on-demand (`PAY_PER_REQUEST`) table creation and deletion with SDK waiters.
