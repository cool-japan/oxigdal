//! STAC (SpatioTemporal Asset Catalog) support for OxiGDAL.
//!
//! This crate provides Pure Rust implementation of the STAC specification,
//! enabling cloud-native geospatial workflows with catalogs, collections,
//! and items.
//!
//! # Features
//!
//! - **STAC 1.0.0 Specification**: Full compliance with STAC 1.0.0
//! - **Core Models**: Catalog, Collection, Item, and Asset
//! - **Extensions**: EO (Electro-Optical) and Projection extensions
//! - **STAC API Client**: Async HTTP client for searching STAC APIs
//! - **Builder Patterns**: Fluent APIs for easy object creation
//! - **Validation**: Comprehensive validation of STAC objects
//! - **Pure Rust**: No C/Fortran dependencies
//!
//! # Example
//!
//! ```rust
//! use oxigdal_stac::{ItemBuilder, Asset};
//! use chrono::Utc;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a STAC Item
//! let item = ItemBuilder::new("my-item")
//!     .datetime(Utc::now())
//!     .bbox(-122.5, 37.5, -122.0, 38.0)
//!     .simple_asset("visual", "https://example.com/image.tif")
//!     .build()?;
//!
//! // Serialize to JSON
//! let json = serde_json::to_string_pretty(&item)?;
//! println!("{}", json);
//! # Ok(())
//! # }
//! ```
//!
//! # STAC API Search
//!
//! ```rust,no_run
//! # #[cfg(all(feature = "reqwest", feature = "async"))]
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! use oxigdal_stac::StacClient;
//!
//! // Create a STAC API client
//! let client = StacClient::new("https://earth-search.aws.element84.com/v1")?;
//!
//! // Search for items
//! let results = client.search()
//!     .collections(vec!["sentinel-2-l2a"])
//!     .bbox([-122.5, 37.5, -122.0, 38.0])
//!     .limit(10)
//!     .execute()
//!     .await?;
//!
//! println!("Found {} items", results.features.len());
//! # Ok(())
//! # }
//! ```

#![deny(missing_docs)]
#![cfg_attr(not(feature = "std"), no_std)]
// Allow collapsible matches for clear STAC field handling
#![allow(clippy::collapsible_match)]
#![allow(clippy::collapsible_if)]
// Allow dead code for future STAC extensions
#![allow(dead_code)]
// Allow stripping prefix manually for URL handling
#![allow(clippy::manual_strip)]
// Allow method name conflicts for builder patterns
#![allow(clippy::should_implement_trait)]

// Re-export common types
pub use chrono;
pub use geojson;
pub use serde_json;

// Core modules
pub mod aggregation;
pub mod api;
pub mod asset;
pub mod builder;
pub mod catalog;
pub mod collection;
pub mod collection_aggregation;
pub mod cql2;
pub mod error;
pub mod extensions;
pub mod item;
pub mod transaction;

#[cfg(feature = "reqwest")]
pub mod pagination;

#[cfg(feature = "reqwest")]
pub mod search;

// Public exports
pub use aggregation::{
    Aggregation, AggregationRequest, AggregationResponse, AggregationResult, Bucket,
};
pub use api::{
    CollectionSummary, CollectionsList, ConformanceDeclaration, FieldsSpec, ItemCollection,
    LandingPage, SearchContext as ApiSearchContext, SearchRequest as ApiSearchRequest,
    SortDirection as ApiSortDirection, SortField,
};
pub use asset::{Asset, media_types, roles};
pub use builder::{CatalogBuilder, CollectionBuilder, ItemBuilder};
pub use catalog::Catalog;
pub use collection::{Collection, Extent, Provider, SpatialExtent, TemporalExtent};
pub use collection_aggregation::{CollectionAggregator, CollectionStats, NumericStats};
pub use cql2::{Cql2Filter, Cql2Operand};
pub use error::{Result, StacError};
pub use extensions::{
    eo::{Band, CommonBandName, EoExtension},
    projection::{ProjectionExtension, epsg_codes},
    sar::{FrequencyBand, ObservationDirection, Polarization, SarExtension},
    scientific::{Publication, ScientificExtension},
    timestamps::TimestampsExtension,
    version::VersionExtension,
    view::ViewExtension,
};
pub use item::{Item, ItemProperties, Link, link_rel};
pub use transaction::{StacItemStore, TransactionOp, TransactionResult};

#[cfg(feature = "reqwest")]
pub use pagination::{CursorPagination, PagePagination, Paginator, TokenPagination};

#[cfg(feature = "reqwest")]
pub use search::{SearchContext, SearchParams, SearchResults, SortBy, SortDirection, StacClient};

/// STAC version supported by this crate.
pub const STAC_VERSION: &str = "1.0.0";

/// Helper function to create a bounding box from coordinates.
///
/// # Arguments
///
/// * `west` - Western longitude
/// * `south` - Southern latitude
/// * `east` - Eastern longitude
/// * `north` - Northern latitude
///
/// # Returns
///
/// Bounding box vector [west, south, east, north]
///
/// # Example
///
/// ```
/// use oxigdal_stac::bbox;
///
/// let bbox = bbox(-122.5, 37.5, -122.0, 38.0);
/// assert_eq!(bbox, vec![-122.5, 37.5, -122.0, 38.0]);
/// ```
pub fn bbox(west: f64, south: f64, east: f64, north: f64) -> Vec<f64> {
    vec![west, south, east, north]
}

/// Helper function to create a GeoJSON Point geometry.
///
/// # Arguments
///
/// * `lon` - Longitude
/// * `lat` - Latitude
///
/// # Returns
///
/// GeoJSON Point geometry
///
/// # Example
///
/// ```
/// use oxigdal_stac::point_geometry;
///
/// let geometry = point_geometry(-122.0, 37.0);
/// assert_eq!(geometry.value, geojson::Value::Point(vec![-122.0, 37.0]));
/// ```
pub fn point_geometry(lon: f64, lat: f64) -> geojson::Geometry {
    geojson::Geometry::new(geojson::Value::Point(vec![lon, lat]))
}

/// Helper function to create a GeoJSON Polygon geometry from a bounding box.
///
/// # Arguments
///
/// * `west` - Western longitude
/// * `south` - Southern latitude
/// * `east` - Eastern longitude
/// * `north` - Northern latitude
///
/// # Returns
///
/// GeoJSON Polygon geometry
///
/// # Example
///
/// ```
/// use oxigdal_stac::bbox_to_polygon;
///
/// let geometry = bbox_to_polygon(-122.5, 37.5, -122.0, 38.0);
/// match geometry.value {
///     geojson::Value::Polygon(_) => (),
///     _ => panic!("Expected Polygon"),
/// }
/// ```
pub fn bbox_to_polygon(west: f64, south: f64, east: f64, north: f64) -> geojson::Geometry {
    let polygon = vec![vec![
        vec![west, south],
        vec![east, south],
        vec![east, north],
        vec![west, north],
        vec![west, south], // Close the ring
    ]];
    geojson::Geometry::new(geojson::Value::Polygon(polygon))
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_bbox_helper() {
        let bbox = bbox(-122.5, 37.5, -122.0, 38.0);
        assert_eq!(bbox, vec![-122.5, 37.5, -122.0, 38.0]);
    }

    #[test]
    fn test_point_geometry() {
        let geometry = point_geometry(-122.0, 37.0);
        assert_eq!(geometry.value, geojson::Value::Point(vec![-122.0, 37.0]));
    }

    #[test]
    fn test_bbox_to_polygon() {
        let geometry = bbox_to_polygon(-122.5, 37.5, -122.0, 38.0);
        match geometry.value {
            geojson::Value::Polygon(coords) => {
                assert_eq!(coords.len(), 1);
                assert_eq!(coords[0].len(), 5); // 4 corners + close
            }
            _ => panic!("Expected Polygon"),
        }
    }

    #[test]
    fn test_stac_version() {
        assert_eq!(STAC_VERSION, "1.0.0");
    }
}
