# Contributing to geodynamo

Contributions, bug reports, and feature requests are welcome!

## Development Setup

1. Install the latest stable Rust toolchain via [rustup](https://rustup.rs/).
2. Start DynamoDB Local for integration tests and examples:
   ```bash
   docker run --rm -p 8000:8000 amazon/dynamodb-local
   ```
3. Run formatting, lints, unit tests, and integration tests:
   ```bash
   cargo fmt --all -- --check
   cargo clippy --all-targets -- -D warnings
   cargo test
   # Run live DynamoDB Local integration tests:
   cargo test -- --ignored
   ```

## Pull Request Checklist

- All public APIs include rustdoc comments and examples where helpful.
- `unsafe` code remains forbidden (`#![forbid(unsafe_code)]`).
- Unit tests cover edge cases (such as `±90°` poles, `±180°` antimeridian crossings, and multi-page S2 range boundaries).
