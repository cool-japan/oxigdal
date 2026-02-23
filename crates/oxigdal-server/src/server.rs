//! HTTP server implementation
//!
//! Sets up the Axum web server with all routes and middleware for serving tiles.

use crate::cache::{TileCache, TileCacheConfig};
use crate::config::Config;
use crate::dataset_registry::DatasetRegistry;
use crate::handlers::{
    TileState, WmsState, WmtsState, get_feature_info, get_map, get_tile, get_tile_kvp,
    get_tile_rest, get_tilejson, wms_get_capabilities, wmts_get_capabilities,
};
use axum::{
    Router,
    extract::{DefaultBodyLimit, Request},
    http::{Method, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tower::ServiceBuilder;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing::{error, info};

/// Server errors
#[derive(Debug, Error)]
pub enum ServerError {
    /// Configuration error
    #[error("Configuration error: {0}")]
    Config(String),

    /// Registry error
    #[error("Registry error: {0}")]
    Registry(#[from] crate::dataset_registry::RegistryError),

    /// HTTP server error
    #[error("HTTP server error: {0}")]
    Http(String),

    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Result type for server operations
pub type ServerResult<T> = Result<T, ServerError>;

/// Tile server instance
pub struct TileServer {
    /// Server configuration
    config: Config,

    /// Dataset registry
    registry: DatasetRegistry,

    /// Tile cache
    cache: TileCache,
}

impl TileServer {
    /// Create a new tile server
    pub fn new(config: Config) -> ServerResult<Self> {
        // Create dataset registry
        let registry = DatasetRegistry::new();

        // Register all configured layers
        registry
            .register_layers(config.layers.clone())
            .map_err(ServerError::Registry)?;

        info!("Registered {} layers", registry.layer_count());

        // Create tile cache
        let cache_config = TileCacheConfig {
            max_memory_bytes: config.cache.memory_size_mb * 1024 * 1024,
            disk_cache_dir: config.cache.disk_cache.clone(),
            ttl: Duration::from_secs(config.cache.ttl_seconds),
            enable_stats: config.cache.enable_stats,
            compression: config.cache.compression,
        };

        let cache = TileCache::new(cache_config);

        Ok(Self {
            config,
            registry,
            cache,
        })
    }

    /// Build the Axum router
    pub fn build_router(&self) -> Router {
        let service_url = format!(
            "http://{}:{}",
            self.config.server.host, self.config.server.port
        );

        // Create shared state for WMS
        let wms_state = Arc::new(WmsState {
            registry: self.registry.clone(),
            cache: self.cache.clone(),
            service_url: service_url.clone(),
            service_title: self.config.metadata.title.clone(),
            service_abstract: self.config.metadata.abstract_.clone(),
        });

        // Create shared state for WMTS
        let wmts_state = Arc::new(WmtsState {
            registry: self.registry.clone(),
            cache: self.cache.clone(),
            service_url: service_url.clone(),
            service_title: self.config.metadata.title.clone(),
            service_abstract: self.config.metadata.abstract_.clone(),
        });

        // Create shared state for XYZ tiles
        let tile_state = Arc::new(TileState {
            registry: self.registry.clone(),
            cache: self.cache.clone(),
        });

        // Build CORS layer
        let cors = if self.config.server.enable_cors {
            let mut cors = CorsLayer::new()
                .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
                .allow_headers([header::CONTENT_TYPE, header::ACCEPT]);

            cors = if self.config.server.cors_origins.is_empty() {
                cors.allow_origin(Any)
            } else {
                let origins: Vec<_> = self
                    .config
                    .server
                    .cors_origins
                    .iter()
                    .filter_map(|o| o.parse().ok())
                    .collect();
                cors.allow_origin(origins)
            };

            cors
        } else {
            CorsLayer::permissive()
        };

        // Build middleware stack
        let middleware = ServiceBuilder::new()
            .layer(TraceLayer::new_for_http())
            .layer(cors)
            .layer(DefaultBodyLimit::max(self.config.server.max_request_size));

        // Build routes with timeout middleware
        let timeout_duration = Duration::from_secs(self.config.server.timeout_seconds);

        Router::new()
            // Home/landing page
            .route("/", get(home_handler))
            // Health check
            .route("/health", get(health_handler))
            // Cache stats
            .route("/stats", get(stats_handler))
            // WMS endpoints
            .route("/wms", get(get_map).with_state(wms_state.clone()))
            .route(
                "/wms/capabilities",
                get(wms_get_capabilities).with_state(wms_state.clone()),
            )
            .route(
                "/wms/feature_info",
                get(get_feature_info).with_state(wms_state),
            )
            // WMTS endpoints
            .route("/wmts", get(get_tile_kvp).with_state(wmts_state.clone()))
            .route(
                "/wmts/capabilities",
                get(wmts_get_capabilities).with_state(wmts_state.clone()),
            )
            .route(
                "/wmts/1.0.0/:layer/:tile_matrix_set/:tile_matrix/:tile_row/:tile_col.png",
                get(get_tile_rest).with_state(wmts_state),
            )
            // XYZ tile endpoints
            .route(
                "/tiles/:layer/:z/:x/:y",
                get(get_tile).with_state(tile_state.clone()),
            )
            .route(
                "/tiles/:layer/tilejson",
                get(get_tilejson).with_state(tile_state),
            )
            .layer(middleware)
            .layer(middleware::from_fn(move |req, next| {
                timeout_middleware(req, next, timeout_duration)
            }))
    }

    /// Start the server
    pub async fn serve(self) -> ServerResult<()> {
        let bind_addr = self.config.bind_address();
        info!("Starting OxiGDAL tile server on {}", bind_addr);
        info!("Service URL: {}", self.get_service_url());
        info!("Workers: {}", self.config.server.workers);
        info!("Cache: {} MB memory", self.config.cache.memory_size_mb);

        if let Some(ref disk_cache) = self.config.cache.disk_cache {
            info!("Disk cache: {}", disk_cache.display());
        }

        // Build router
        let app = self.build_router();

        // Create TCP listener
        let listener = tokio::net::TcpListener::bind(&bind_addr)
            .await
            .map_err(|e| ServerError::Http(format!("Failed to bind to {}: {}", bind_addr, e)))?;

        info!("Server listening on {}", bind_addr);
        info!("Available endpoints:");
        info!("  - WMS:  http://{}/wms", bind_addr);
        info!("  - WMTS: http://{}/wmts", bind_addr);
        info!(
            "  - XYZ:  http://{}/tiles/{{layer}}/{{z}}/{{x}}/{{y}}.png",
            bind_addr
        );
        info!("  - Health: http://{}/health", bind_addr);
        info!("  - Stats: http://{}/stats", bind_addr);

        // Serve with Axum
        axum::serve(listener, app)
            .await
            .map_err(|e| ServerError::Http(e.to_string()))?;

        Ok(())
    }

    /// Get the service URL
    fn get_service_url(&self) -> String {
        format!(
            "http://{}:{}",
            self.config.server.host, self.config.server.port
        )
    }

    /// Get the dataset registry
    pub fn registry(&self) -> &DatasetRegistry {
        &self.registry
    }

    /// Get the tile cache
    pub fn cache(&self) -> &TileCache {
        &self.cache
    }

    /// Get the configuration
    pub fn config(&self) -> &Config {
        &self.config
    }
}

/// Timeout middleware that wraps requests with a timeout
async fn timeout_middleware(
    req: Request,
    next: Next,
    duration: Duration,
) -> Result<Response, StatusCode> {
    match tokio::time::timeout(duration, next.run(req)).await {
        Ok(response) => Ok(response),
        Err(_) => {
            error!("Request timeout after {:?}", duration);
            Err(StatusCode::GATEWAY_TIMEOUT)
        }
    }
}

/// Home page handler
async fn home_handler() -> Html<&'static str> {
    Html(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>OxiGDAL Tile Server</title>
    <style>
        body {
            font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif;
            max-width: 800px;
            margin: 50px auto;
            padding: 20px;
            line-height: 1.6;
        }
        h1 { color: #2c3e50; }
        h2 { color: #34495e; margin-top: 30px; }
        code {
            background: #f4f4f4;
            padding: 2px 6px;
            border-radius: 3px;
            font-family: 'Courier New', monospace;
        }
        .endpoint {
            background: #ecf0f1;
            padding: 10px;
            margin: 10px 0;
            border-left: 4px solid #3498db;
        }
        a { color: #3498db; text-decoration: none; }
        a:hover { text-decoration: underline; }
    </style>
</head>
<body>
    <h1>OxiGDAL Tile Server</h1>
    <p>WMS/WMTS tile server powered by OxiGDAL - Pure Rust geospatial data access library.</p>

    <h2>Available Endpoints</h2>

    <div class="endpoint">
        <h3>WMS (Web Map Service)</h3>
        <p><a href="/wms?SERVICE=WMS&REQUEST=GetCapabilities">GetCapabilities</a></p>
        <p>GetMap: <code>/wms?SERVICE=WMS&REQUEST=GetMap&LAYERS=layer&BBOX=...</code></p>
    </div>

    <div class="endpoint">
        <h3>WMTS (Web Map Tile Service)</h3>
        <p><a href="/wmts?SERVICE=WMTS&REQUEST=GetCapabilities">GetCapabilities</a></p>
        <p>GetTile: <code>/wmts/1.0.0/{layer}/{tileMatrixSet}/{z}/{x}/{y}.png</code></p>
    </div>

    <div class="endpoint">
        <h3>XYZ Tiles</h3>
        <p>Tiles: <code>/tiles/{layer}/{z}/{x}/{y}.png</code></p>
        <p>TileJSON: <code>/tiles/{layer}/tilejson</code></p>
    </div>

    <h2>Server Status</h2>
    <p><a href="/health">Health Check</a> | <a href="/stats">Cache Statistics</a></p>

    <h2>Documentation</h2>
    <p>For more information, visit the <a href="https://github.com/cool-japan/oxigdal">OxiGDAL repository</a>.</p>
</body>
</html>
"#,
    )
}

/// Health check handler
async fn health_handler() -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        r#"{"status":"healthy","service":"oxigdal-tile-server"}"#,
    )
        .into_response()
}

/// Cache statistics handler
async fn stats_handler() -> Response {
    // This is a placeholder - in a real implementation, we'd need to access the cache
    // through shared state
    let stats = serde_json::json!({
        "status": "ok",
        "message": "Cache statistics endpoint - requires state injection"
    });

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        serde_json::to_string_pretty(&stats).unwrap_or_default(),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_creation() {
        let config = Config::default_config();
        let result = TileServer::new(config);

        // Server creation should succeed with default config
        // (even though it has no layers)
        assert!(result.is_ok());
    }
}
