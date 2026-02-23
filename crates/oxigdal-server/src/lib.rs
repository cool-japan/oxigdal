//! OxiGDAL Tile Server
//!
//! WMS/WMTS tile server for serving geospatial raster data over HTTP.
//!
//! # Features
//!
//! - **WMS 1.3.0**: Full Web Map Service support with GetCapabilities, GetMap, GetFeatureInfo
//! - **WMTS 1.0.0**: Web Map Tile Service with multiple tile matrix sets
//! - **XYZ Tiles**: Simple tile serving compatible with Leaflet, MapLibre, etc.
//! - **Multi-layer**: Serve multiple datasets simultaneously
//! - **Caching**: Multi-level caching (memory + optional disk) with LRU eviction
//! - **Performance**: Async/await with Tokio for high throughput
//! - **Pure Rust**: No C/C++ dependencies, built on OxiGDAL
//!
//! # Example Usage
//!
//! ```no_run
//! use oxigdal_server::{Config, TileServer};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Load configuration
//!     let config = Config::from_file("config.toml")?;
//!
//!     // Create and start server
//!     let server = TileServer::new(config)?;
//!     server.serve().await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! # Configuration
//!
//! Create a `config.toml` file:
//!
//! ```toml
//! [server]
//! host = "0.0.0.0"
//! port = 8080
//! workers = 4
//!
//! [cache]
//! memory_size_mb = 256
//! ttl_seconds = 3600
//!
//! [[layers]]
//! name = "landsat"
//! path = "/data/landsat.tif"
//! formats = ["png", "jpeg"]
//! tile_size = 256
//! ```
//!
//! # Endpoints
//!
//! ## WMS
//!
//! - GetCapabilities: `GET /wms?SERVICE=WMS&REQUEST=GetCapabilities`
//! - GetMap: `GET /wms?SERVICE=WMS&REQUEST=GetMap&LAYERS=...&BBOX=...`
//!
//! ## WMTS
//!
//! - GetCapabilities: `GET /wmts?SERVICE=WMTS&REQUEST=GetCapabilities`
//! - GetTile (RESTful): `GET /wmts/1.0.0/{layer}/{tileMatrixSet}/{z}/{x}/{y}.png`
//!
//! ## XYZ Tiles
//!
//! - Tiles: `GET /tiles/{layer}/{z}/{x}/{y}.png`
//! - TileJSON: `GET /tiles/{layer}/tilejson`

#![warn(missing_docs)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::panic)]

pub mod cache;
pub mod config;
pub mod dataset_registry;
pub mod handlers;
pub mod server;

// Re-export main types
pub use cache::{CacheError, CacheKey, CacheStats, TileCache, TileCacheConfig};
pub use config::{
    CacheConfig, Config, ConfigError, ImageFormat, LayerConfig, MetadataConfig, ServerConfig,
    StyleConfig,
};
pub use dataset_registry::{DatasetMetadata, DatasetRegistry, LayerInfo, RegistryError};
pub use server::{ServerError, TileServer};
