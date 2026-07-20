//! Tutorial 07: Web Services (WMS/WMTS/WFS)
//!
//! This tutorial demonstrates setting up geospatial web services with the
//! real `oxigdal-server` `TileServer`, plus a small illustrative OGC API -
//! Features style router built directly with `axum`.
//!
//! Run with:
//! ```bash
//! cargo run --example tutorial_07_web_services
//! ```

use axum::{
    Json, Router,
    extract::Path,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use oxigdal_server::config::Config;
use oxigdal_server::server::TileServer;
use std::env;
use std::net::SocketAddr;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Tutorial 07: Web Services ===\n");

    let temp_dir = env::temp_dir();

    // Step 1: Server Configuration
    println!("Step 1: Server Configuration");
    println!("----------------------------");

    let config = Config::default_config();

    println!("Server configuration:");
    println!("  Host: {}", config.server.host);
    println!("  Port: {}", config.server.port);
    println!("  Workers: {}", config.server.workers);
    println!("  CORS enabled: {}", config.server.enable_cors);
    println!("  Cache memory size: {} MB", config.cache.memory_size_mb);
    println!("  Layers configured: {}", config.layers.len());

    // Step 2: Building the real TileServer
    println!("\n\nStep 2: Building the TileServer");
    println!("--------------------------------");

    let tile_server = TileServer::new(config.clone())?;
    let tile_router = tile_server.build_router();

    let layer_count = tile_server.registry().list_layers()?.len();
    println!(
        "TileServer created with {} registered layer(s)",
        layer_count
    );
    println!("Router exposes WMS/WMTS/tile endpoints as configured in `Config::layers`");

    // Step 3: A minimal OGC API - Features style router
    println!("\n\nStep 3: OGC API - Features (illustrative)");
    println!("-------------------------------------------");

    println!("Modern RESTful alternative to WFS");
    println!("\nEndpoints:");
    println!("  GET  /collections                          - List collections");
    println!("  GET  /collections/{{collectionId}}           - Collection metadata");
    println!("  GET  /collections/{{collectionId}}/items      - Query features");
    println!("  GET  /health                                - Health check");

    let features_router = Router::new()
        .route("/collections", get(collections_handler))
        .route("/collections/{collection_id}", get(collection_handler))
        .route("/collections/{collection_id}/items", get(items_handler))
        .route("/health", get(health_handler));

    // Step 4: Merging routers and adding middleware
    println!("\n\nStep 4: Assembling the Application");
    println!("------------------------------------");

    let app = Router::new()
        .merge(tile_router)
        .merge(features_router)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    println!("Routes configured (tiles + OGC API - Features + health)");

    // Step 5: Running the Server
    println!("\n\nStep 5: Running the Server");
    println!("--------------------------");

    let addr = SocketAddr::from(([127, 0, 0, 1], config.server.port));

    println!("\nServer ready to bind at: http://{}", addr);
    println!("\nExample requests:");
    println!("  curl 'http://{}/collections'", addr);
    println!("  curl 'http://{}/health'", addr);

    println!("\nNote: This tutorial builds and validates the router but does not");
    println!("      block on `axum::serve` so the example can exit cleanly.");
    println!(
        "      Data directory for tiles: {:?}",
        temp_dir.join("data")
    );

    // To actually run the server:
    // let listener = tokio::net::TcpListener::bind(addr).await?;
    // axum::serve(listener, app).await?;
    let _ = app; // keep the assembled router alive for the type-check above

    // Summary
    println!("\n\n=== Tutorial Complete! ===");
    println!("\nTopics Covered:");
    println!("  1. Server configuration (`oxigdal_server::config::Config`)");
    println!("  2. Building a real `TileServer` and its `axum::Router`");
    println!("  3. OGC API - Features style endpoints");
    println!("  4. Merging routers and adding CORS/tracing middleware");
    println!("  5. Server startup pattern");

    println!("\nKey Points:");
    println!("  - `TileServer::build_router()` returns a ready-to-serve `axum::Router`");
    println!("  - Custom routers can be `.merge()`d onto the tile server's router");
    println!("  - Caching and layer configuration live in `Config`");

    println!("\nNext Tutorial:");
    println!("  - Try tutorial 08 for performance optimization");

    Ok(())
}

// Handler functions (illustrative OGC API - Features style)

async fn collections_handler() -> Response {
    let collections = serde_json::json!({
        "collections": [
            {
                "id": "cities",
                "title": "World Cities",
                "extent": {
                    "spatial": {
                        "bbox": [[-180, -90, 180, 90]]
                    }
                }
            }
        ]
    });

    Json(collections).into_response()
}

async fn collection_handler(Path(collection_id): Path<String>) -> Response {
    (StatusCode::OK, format!("Collection: {}", collection_id)).into_response()
}

async fn items_handler(Path(collection_id): Path<String>) -> Response {
    (
        StatusCode::OK,
        format!("GeoJSON FeatureCollection for: {}", collection_id),
    )
        .into_response()
}

async fn health_handler() -> Response {
    let health = serde_json::json!({
        "status": "healthy",
        "version": env!("CARGO_PKG_VERSION"),
    });

    Json(health).into_response()
}
