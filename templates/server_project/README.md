# Server Project Template for OxiGDAL

A project template for building geospatial web services powered by OxiGDAL and Axum.

## What This Template Provides

- [Axum](https://docs.rs/axum) web framework with Tower middleware stack
- CORS, static file serving, and request tracing via `tower-http`
- OxiGDAL core, server, and GeoTIFF driver dependencies
- Async runtime (Tokio) for high-performance concurrent request handling
- JSON serialization with `serde` and `serde_json`
- Structured error handling with `anyhow` and `thiserror`
- Health check endpoint pre-configured

## Getting Started

1. Copy this template directory to your workspace
2. Update `Cargo.toml` with your project name and any additional dependencies
3. Define your API routes and handlers in `src/main.rs`
4. Run:

```sh
cargo run --release
# Server starts at http://127.0.0.1:3000
```

## Example Usage

```rust
use axum::{routing::get, Router, Json};
use serde_json::json;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app = Router::new()
        .route("/", get(|| async { "OxiGDAL Server" }))
        .route("/health", get(|| async { "OK" }))
        .route("/tiles/{z}/{x}/{y}", get(serve_tile));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn serve_tile(/* params */) -> Json<serde_json::Value> {
    Json(json!({"status": "ok"}))
}
```

## Extending the Template

- Add tile serving endpoints for COG (Cloud Optimized GeoTIFF) data
- Implement OGC-compliant WMS/WFS/WMTS services
- Add authentication and rate limiting middleware
- Integrate with OxiGDAL cloud storage backends for remote data access
- Serve vector tiles from GeoParquet, FlatGeobuf, or Shapefile sources

## License

Apache-2.0

Part of the [OxiGDAL](https://github.com/cool-japan/oxigdal) project by COOLJAPAN OU.
