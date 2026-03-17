//! OxiGDAL Services - OGC Web Services Implementation
//!
//! Provides OGC-compliant web service implementations for geospatial data access and processing:
//!
//! - **WFS (Web Feature Service) 2.0/3.0**: Vector data access with filtering and transactions
//! - **WCS (Web Coverage Service) 2.0**: Raster data access with subsetting and format conversion
//! - **WPS (Web Processing Service) 2.0**: Geospatial processing with built-in algorithms
//! - **CSW (Catalog Service for the Web) 2.0.2**: Metadata catalog search and retrieval
//! - **OGC API - Tiles**: TileMatrixSet, TileSetMetadata, tile coordinate utilities
//! - **MVT (Mapbox Vector Tile)**: Protobuf encoding for vector tile generation
//!
//! # Features
//!
//! - OGC-compliant implementations following official standards
//! - XML/JSON output formats with proper schema validation
//! - CRS support and coordinate transformation
//! - Built-in WPS processes (buffer, clip, union, etc.)
//! - Async request handling with Axum
//! - Pure Rust implementation (no C/C++ dependencies)
//! - Hand-rolled MVT protobuf encoder (no protobuf crate dependency)
//!
//! # COOLJAPAN Policies
//!
//! - **Pure Rust**: No C/C++ dependencies
//! - **No unwrap()**: Proper error handling throughout
//! - **Workspace**: Uses workspace dependencies
//! - **Files < 2000 lines**: Modular code organization

#![warn(missing_docs)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::panic)]

pub mod cache_headers;
pub mod csw;
pub mod error;
pub mod mvt;
pub mod ogc_features;
pub mod ogc_tiles;
pub mod style;
pub mod tile_cache;
pub mod wcs;
pub mod wfs;
pub mod wps;

// Re-export main types
pub use csw::{CswState, MetadataRecord};
pub use error::{ServiceError, ServiceResult};
pub use mvt::{
    MvtFeature, MvtGeometryType, MvtLayer, MvtLayerBuilder, MvtTile, MvtValue, close_path,
    decode_zigzag, delta_encode, encode_varint, encode_zigzag, line_to, linestring_geometry,
    move_to, point_geometry, polygon_ring_geometry, scale_to_tile,
};
pub use ogc_tiles::{
    ConformanceDeclaration, CornerOfOrigin, GeographicBoundingBox, TileDataType, TileLink,
    TileMatrix, TileMatrixSet, TileSetMetadata, lonlat_to_tile, tile_children, tile_parent,
    tile_to_bbox, tile_to_pixel_bounds, tiles_in_bbox, validate_tile_coords,
};
pub use wcs::{CoverageInfo, CoverageSource, WcsState};
pub use wfs::{FeatureSource, FeatureTypeInfo, WfsState};
pub use wps::{Process, ProcessInputs, ProcessOutputs, WpsState};

/// Library version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }
}
