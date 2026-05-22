//! # oxigdal-geojson-stream
//!
//! Pure-Rust streaming GeoJSON reader and writer for the OxiGDAL ecosystem.
//!
//! ## Quick start
//!
//! ```rust
//! use oxigdal_geojson_stream::{GeoJsonParser, GeoJsonWriter};
//!
//! let json = br#"{"type":"FeatureCollection","features":[]}"#;
//! let parser = GeoJsonParser::new();
//! let doc = parser.parse(json).expect("valid GeoJSON");
//!
//! let writer = GeoJsonWriter::compact();
//! println!("{}", writer.write_document(&doc));
//! ```
//!
//! ## Modules
//!
//! | Module | Contents |
//! |--------|----------|
//! | [`types`] | [`GeoJsonGeometry`], [`GeoJsonFeature`], [`GeoJsonCrs`], [`FeatureId`] |
//! | [`parser`] | [`GeoJsonParser`], [`GeoJsonDocument`], [`FeatureCollection`], [`StreamingFeatureReader`] |
//! | [`writer`] | [`GeoJsonWriter`], [`GeoJsonValidator`], [`ValidationIssue`], [`IssueSeverity`] |
//! | [`filter`] | [`FeatureFilter`], [`PropertyFilter`], [`FilterOp`], [`CompiledRegexFilter`] |
//! | [`error`] | [`GeoJsonError`] |

#![warn(clippy::all)]
#![deny(clippy::unwrap_used)]
#![allow(clippy::module_name_repetitions)]

pub mod clip;
pub mod diff;
pub mod dissolve;
pub mod error;
pub mod filter;
pub mod incremental;
#[cfg(feature = "parallel")]
pub mod parallel_parse;
pub mod parser;
#[cfg(feature = "reproject")]
pub mod reproject;
pub mod schema;
pub mod seq;
pub mod simplify;
pub mod sort;
pub mod topojson;
pub mod types;
pub mod validity;
pub mod wkt;
pub mod writer;

// ─── Re-exports ──────────────────────────────────────────────────────────────

pub use error::GeoJsonError;

pub use types::{FeatureId, GeoJsonCrs, GeoJsonFeature, GeoJsonGeometry};

pub use parser::{
    FeatureCollection, FeatureCollectionHeader, GeoJsonDocument, GeoJsonParser,
    StreamingFeatureReader,
};

pub use writer::{GeoJsonValidator, GeoJsonWriter, IssueSeverity, ValidationIssue};

pub use filter::{CompiledRegexFilter, FeatureFilter, FilterExpr, FilterOp, PropertyFilter};

pub use seq::{SeqReader, SeqWriter};

pub use incremental::IncrementalFeatureReader;

pub use simplify::simplify_dp;

pub use wkt::{geometry_from_wkt, geometry_to_wkt};

pub use topojson::{TopoOptions, feature_collection_to_topojson};

pub use clip::{ClipBox, clip_geometry, clip_linestring, clip_polygon};

pub use schema::{
    FeatureSchema, FieldSchema, InferredType, infer_schema, infer_schema_from_collection,
    infer_schema_slice,
};

pub use diff::{
    FeatureChangeDetail, FeatureDiff, GeoJsonDiff, PropertyChange, diff_feature_collections,
    diff_properties, feature_id_to_string, geometries_equal_within_eps,
};

pub use sort::{
    FeatureSortKey, SortOrder, feature_centroid, geohash_key, hilbert_key, sort_feature_collection,
    sort_features, sort_features_owned,
};

pub use dissolve::{
    DissolveOptions, DissolveStats, DissolveStrategy, PropertyAggregator,
    dissolve_feature_collection, dissolve_features,
};

pub use validity::{
    GeometryValidityIssue, GeometryValidityReport, WindingOrder, check_ring_self_intersection,
    fix_geometry_winding, fix_ring_winding, ring_signed_area, ring_winding_order,
    validate_geometry, validate_polygon_rings,
};

#[cfg(feature = "reproject")]
pub use reproject::{ReprojectOptions, Reprojector, extract_crs_from_geojson_value};

#[cfg(feature = "reproject")]
pub use parser::parse_feature_collection_with_reprojection;

#[cfg(feature = "reproject")]
pub use writer::write_feature_collection_with_reprojection;

#[cfg(feature = "parallel")]
pub use parallel_parse::{
    ParallelParseOptions, parse_features_parallel, parse_features_parallel_default,
};
