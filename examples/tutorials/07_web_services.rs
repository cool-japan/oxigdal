//! Tutorial 07: Web Services (WMS/WMTS/WFS)
//!
//! This tutorial demonstrates setting up geospatial web services:
//! - WMS (Web Map Service)
//! - WMTS (Web Map Tile Service)
//! - WFS (Web Feature Service)
//! - OGC API - Features
//! - Performance optimization for tile serving
//!
//! Run with:
//! ```bash
//! cargo run --example 07_web_services --features server
//! ```

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{BoundingBox, GeoTransform, RasterDataType};
use oxigdal_server::handlers::tiles::{TileRequest, TileServer};
use oxigdal_server::handlers::wms::{GetCapabilities, GetMap, WmsHandler};
use oxigdal_server::handlers::wmts::{GetTile, WmtsHandler};
use oxigdal_server::server::{ServerConfig, ServerState};
use serde::{Deserialize, Serialize};
use std::env;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Tutorial 07: Web Services ===\n");

    // Initialize logging
    tracing_subscriber::fmt::init();

    let temp_dir = env::temp_dir();

    // Step 1: Server Configuration
    println!("Step 1: Server Configuration");
    println!("----------------------------");

    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 8080,
        max_connections: 1000,
        tile_cache_size_mb: 512,
        cors_enabled: true,
        compression_enabled: true,
    };

    println!("Server configuration:");
    println!("  Host: {}", config.host);
    println!("  Port: {}", config.port);
    println!("  Max connections: {}", config.max_connections);
    println!("  Tile cache: {} MB", config.tile_cache_size_mb);
    println!("  CORS: {}", config.cors_enabled);
    println!("  Compression: {}", config.compression_enabled);

    // Step 2: WMS (Web Map Service) Setup
    println!("\n\nStep 2: WMS (Web Map Service)");
    println!("------------------------------");

    println!("WMS enables rendering map images from geospatial data");

    // Create sample data
    println!("\nCreating sample raster data...");
    let sample_raster = create_sample_raster(1024, 1024)?;

    let bbox = BoundingBox::new(-180.0, -90.0, 180.0, 90.0)?;
    let geo_transform = GeoTransform::from_bounds(&bbox, 1024, 1024)?;

    println!("  Raster: 1024x1024");
    println!("  Bounds: [{}, {}, {}, {}]",
             bbox.min_x(), bbox.min_y(), bbox.max_x(), bbox.max_y());

    // WMS GetCapabilities
    println!("\nWMS GetCapabilities:");
    println!("  URL: http://{}:{}/wms?SERVICE=WMS&REQUEST=GetCapabilities&VERSION=1.3.0",
             config.host, config.port);

    let capabilities = generate_wms_capabilities();
    println!("\nCapabilities document includes:");
    println!("  - Service metadata");
    println!("  - Available layers");
    println!("  - Supported formats");
    println!("  - Coordinate systems");

    // WMS GetMap
    println!("\nWMS GetMap:");
    println!("  URL: http://{}:{}/wms?", config.host, config.port);
    println!("    SERVICE=WMS&");
    println!("    REQUEST=GetMap&");
    println!("    VERSION=1.3.0&");
    println!("    LAYERS=sample_layer&");
    println!("    BBOX=-180,-90,180,90&");
    println!("    WIDTH=512&");
    println!("    HEIGHT=512&");
    println!("    FORMAT=image/png&");
    println!("    CRS=EPSG:4326");

    println!("\nSupported formats:");
    println!("  - image/png");
    println!("  - image/jpeg");
    println!("  - image/webp");
    println!("  - image/tiff");

    // Step 3: WMTS (Web Map Tile Service)
    println!("\n\nStep 3: WMTS (Web Map Tile Service)");
    println!("------------------------------------");

    println!("WMTS provides pre-rendered map tiles for efficient delivery");

    // Tile matrix set
    println!("\nTile Matrix Set (WebMercatorQuad):");
    println!("  Zoom levels: 0-18");
    println!("  Tile size: 256x256 pixels");
    println!("  Origin: Top-left");

    // Calculate tiles at different zoom levels
    for zoom in 0..=4 {
        let tiles = 2_u32.pow(zoom);
        println!("  Level {}: {}x{} = {} tiles", zoom, tiles, tiles, tiles * tiles);
    }

    // WMTS GetCapabilities
    println!("\nWMTS GetCapabilities:");
    println!("  URL: http://{}:{}/wmts?SERVICE=WMTS&REQUEST=GetCapabilities",
             config.host, config.port);

    // WMTS GetTile
    println!("\nWMTS GetTile:");
    println!("  URL pattern: http://{}:{}/wmts/{{Layer}}/{{TileMatrixSet}}/{{TileMatrix}}/{{TileRow}}/{{TileCol}}.png",
             config.host, config.port);
    println!("  Example: http://{}:{}/wmts/sample_layer/WebMercatorQuad/5/10/15.png",
             config.host, config.port);

    // Tile caching
    println!("\nTile caching strategy:");
    println!("  - In-memory LRU cache");
    println!("  - Disk cache for persistence");
    println!("  - ETags for client-side caching");
    println!("  - Compression (gzip/brotli)");

    // Step 4: WFS (Web Feature Service)
    println!("\n\nStep 4: WFS (Web Feature Service)");
    println!("----------------------------------");

    println!("WFS provides access to vector features");

    // WFS GetCapabilities
    println!("\nWFS GetCapabilities:");
    println!("  URL: http://{}:{}/wfs?SERVICE=WFS&REQUEST=GetCapabilities&VERSION=2.0.0",
             config.host, config.port);

    // WFS GetFeature
    println!("\nWFS GetFeature:");
    println!("  URL: http://{}:{}/wfs?", config.host, config.port);
    println!("    SERVICE=WFS&");
    println!("    REQUEST=GetFeature&");
    println!("    VERSION=2.0.0&");
    println!("    TYPENAME=cities&");
    println!("    BBOX=-180,-90,180,90&");
    println!("    OUTPUTFORMAT=application/json");

    println!("\nSupported output formats:");
    println!("  - application/json (GeoJSON)");
    println!("  - application/gml+xml (GML)");
    println!("  - text/xml (GML 2.0)");

    // Filtering
    println!("\nFeature filtering:");
    println!("  - Spatial: BBOX, INTERSECTS, WITHIN");
    println!("  - Attribute: PropertyIsEqualTo, PropertyIsLike");
    println!("  - Temporal: During, Before, After");

    // Step 5: OGC API - Features
    println!("\n\nStep 5: OGC API - Features");
    println!("--------------------------");

    println!("Modern RESTful alternative to WFS");

    println!("\nEndpoints:");
    println!("  GET  /collections");
    println!("       List all feature collections");

    println!("\n  GET  /collections/{{collectionId}}");
    println!("       Collection metadata");

    println!("\n  GET  /collections/{{collectionId}}/items");
    println!("       Query features with parameters:");
    println!("         bbox=-180,-90,180,90");
    println!("         datetime=2023-01-01T00:00:00Z");
    println!("         limit=100");
    println!("         offset=0");

    println!("\n  GET  /collections/{{collectionId}}/items/{{featureId}}");
    println!("       Single feature by ID");

    // Example API calls
    println!("\nExample API calls:");
    println!("  http://{}:{}/collections", config.host, config.port);
    println!("  http://{}:{}/collections/cities/items?limit=10", config.host, config.port);
    println!("  http://{}:{}/collections/cities/items?bbox=-10,40,10,50", config.host, config.port);

    // Step 6: Performance Optimization
    println!("\n\nStep 6: Performance Optimization");
    println!("---------------------------------");

    println!("\n1. Tile Generation:");
    println!("   - Pre-generate tiles for common zoom levels");
    println!("   - Use tile pyramids (overviews)");
    println!("   - Parallel tile generation");
    println!("   - On-demand generation for rare tiles");

    println!("\n2. Caching Strategy:");
    println!("   - Multi-level cache (memory + disk)");
    println!("   - CDN integration");
    println!("   - Cache warming for popular areas");
    println!("   - Smart eviction policies");

    println!("\n3. Data Optimization:");
    println!("   - Use Cloud-Optimized formats (COG, FlatGeobuf)");
    println!("   - Spatial indexing (R-tree, Quadtree)");
    println!("   - Data partitioning by region/zoom");
    println!("   - Compression (LZW, WebP, Brotli)");

    println!("\n4. Server Optimization:");
    println!("   - Connection pooling");
    println!("   - Async I/O");
    println!("   - Load balancing");
    println!("   - Rate limiting");

    // Step 7: Building the Server
    println!("\n\nStep 7: Building the Server");
    println!("----------------------------");

    println!("Setting up Axum web server...");

    // Create shared state
    let state = Arc::new(ServerState {
        config: config.clone(),
        tile_cache: RwLock::new(std::collections::HashMap::new()),
        data_path: temp_dir.join("data"),
    });

    // Build router
    let app = Router::new()
        // WMS endpoints
        .route("/wms", get(wms_handler))
        // WMTS endpoints
        .route("/wmts", get(wmts_capabilities))
        .route("/wmts/:layer/:tilematrix/:z/:x/:y.png", get(wmts_tile_handler))
        // WFS endpoints
        .route("/wfs", get(wfs_handler))
        // OGC API - Features endpoints
        .route("/collections", get(collections_handler))
        .route("/collections/:collection_id", get(collection_handler))
        .route("/collections/:collection_id/items", get(items_handler))
        .route("/collections/:collection_id/items/:item_id", get(item_handler))
        // Health check
        .route("/health", get(health_handler))
        // State
        .with_state(state)
        // Middleware
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    println!("\nRoutes configured:");
    println!("  GET  /wms                                    - WMS endpoint");
    println!("  GET  /wmts                                   - WMTS capabilities");
    println!("  GET  /wmts/:layer/:matrix/:z/:x/:y.png      - WMTS tile");
    println!("  GET  /wfs                                    - WFS endpoint");
    println!("  GET  /collections                            - List collections");
    println!("  GET  /collections/:id/items                  - Query features");
    println!("  GET  /health                                 - Health check");

    // Step 8: Running the Server
    println!("\n\nStep 8: Running the Server");
    println!("--------------------------");

    let addr = SocketAddr::from(([127, 0, 0, 1], config.port));

    println!("\nServer ready to start at: http://{}", addr);
    println!("\nExample requests:");
    println!("  WMS:    curl 'http://{}/wms?SERVICE=WMS&REQUEST=GetCapabilities'", addr);
    println!("  WMTS:   curl 'http://{}/wmts?SERVICE=WMTS&REQUEST=GetCapabilities'", addr);
    println!("  Tile:   curl 'http://{}/wmts/layer/WebMercatorQuad/5/10/15.png' -o tile.png", addr);
    println!("  OGC:    curl 'http://{}/collections'", addr);

    println!("\nNote: In production, uncomment the server start code below");
    println!("      For this tutorial, we're demonstrating configuration only");

    // To actually run the server, uncomment:
    // println!("\nStarting server...");
    // axum::Server::bind(&addr)
    //     .serve(app.into_make_service())
    //     .await?;

    // Summary
    println!("\n\n=== Tutorial Complete! ===");
    println!("\nTopics Covered:");
    println!("  1. Server configuration");
    println!("  2. WMS (Web Map Service)");
    println!("  3. WMTS (Web Map Tile Service)");
    println!("  4. WFS (Web Feature Service)");
    println!("  5. OGC API - Features");
    println!("  6. Performance optimization");
    println!("  7. Building with Axum");
    println!("  8. Server deployment");

    println!("\nKey Points:");
    println!("  - OGC standards enable interoperability");
    println!("  - WMTS provides efficient tile delivery");
    println!("  - Caching is critical for performance");
    println!("  - OGC API - Features is the modern RESTful approach");
    println!("  - Proper indexing and optimization are essential");

    println!("\nNext Tutorial:");
    println!("  - Try tutorial 08 for performance optimization");

    Ok(())
}

// Handler functions (simplified for demonstration)

async fn wms_handler(Query(params): Query<std::collections::HashMap<String, String>>) -> Response {
    let request = params.get("REQUEST").map(|s| s.as_str()).unwrap_or("");

    match request {
        "GetCapabilities" => {
            let xml = generate_wms_capabilities();
            (StatusCode::OK, xml).into_response()
        }
        "GetMap" => {
            // Generate and return map image
            (StatusCode::OK, "PNG image data").into_response()
        }
        _ => (StatusCode::BAD_REQUEST, "Unknown request").into_response(),
    }
}

async fn wmts_capabilities() -> Response {
    let xml = generate_wmts_capabilities();
    (StatusCode::OK, xml).into_response()
}

async fn wmts_tile_handler(
    Path((layer, matrix, z, x, y)): Path<(String, String, u32, u32, String)>,
) -> Response {
    // Generate tile
    (StatusCode::OK, "PNG tile data").into_response()
}

async fn wfs_handler(Query(params): Query<std::collections::HashMap<String, String>>) -> Response {
    (StatusCode::OK, "GeoJSON features").into_response()
}

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

async fn items_handler(
    Path(collection_id): Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Response {
    (StatusCode::OK, "GeoJSON FeatureCollection").into_response()
}

async fn item_handler(Path((collection_id, item_id)): Path<(String, String)>) -> Response {
    (StatusCode::OK, "GeoJSON Feature").into_response()
}

async fn health_handler() -> Response {
    let health = serde_json::json!({
        "status": "healthy",
        "version": "0.1.0"
    });

    Json(health).into_response()
}

// Helper functions

fn create_sample_raster(width: u32, height: u32) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::UInt8);

    for y in 0..height {
        for x in 0..width {
            let value = ((x + y) % 256) as f64;
            buffer.set_pixel(x, y, value)?;
        }
    }

    Ok(buffer)
}

fn generate_wms_capabilities() -> String {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<WMS_Capabilities version="1.3.0">
  <Service>
    <Name>WMS</Name>
    <Title>OxiGDAL WMS Server</Title>
    <Abstract>Web Map Service powered by OxiGDAL</Abstract>
  </Service>
  <Capability>
    <Layer>
      <Name>sample_layer</Name>
      <Title>Sample Layer</Title>
      <CRS>EPSG:4326</CRS>
      <CRS>EPSG:3857</CRS>
    </Layer>
  </Capability>
</WMS_Capabilities>"#.to_string()
}

fn generate_wmts_capabilities() -> String {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<Capabilities version="1.0.0">
  <Contents>
    <Layer>
      <Title>Sample Layer</Title>
      <Identifier>sample_layer</Identifier>
      <TileMatrixSet>WebMercatorQuad</TileMatrixSet>
    </Layer>
  </Contents>
</Capabilities>"#.to_string()
}
