//! GeoPackage vector feature table types.
//!
//! Provides pure-Rust data structures for GeoPackage geometry (GeoPackageBinary /
//! WKB), field definitions, feature rows, and feature tables.  No SQLite
//! library is required — this module is a binary parser.

pub mod feature;
#[cfg(feature = "geojson-convert")]
pub mod geojson_convert;
pub mod types;
pub mod wkb;

pub use feature::{FeatureRow, FeatureTable, SrsInfo};
#[cfg(feature = "geojson-convert")]
pub use geojson_convert::{
    feature_table_from_geojson, feature_table_to_geojson, geojson_geom_to_gpkg,
    gpkg_geom_to_geojson,
};
pub use types::{CoordDim, FieldDefinition, FieldType, FieldValue, GpkgGeometry, Point4D};
pub use wkb::{
    GpkgBinaryParser, WKB_EWKB_M_HIGHBIT, WKB_EWKB_ZM_HIGHBIT, WKB_POINT_M, WKB_POINT_ZM,
};
