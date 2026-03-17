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
//! | [`filter`] | [`FeatureFilter`], [`PropertyFilter`], [`FilterOp`] |
//! | [`error`] | [`GeoJsonError`] |

#![warn(clippy::all)]
#![deny(clippy::unwrap_used)]
#![allow(clippy::module_name_repetitions)]

pub mod error;
pub mod filter;
pub mod parser;
pub mod types;
pub mod writer;

// ─── Re-exports ──────────────────────────────────────────────────────────────

pub use error::GeoJsonError;

pub use types::{FeatureId, GeoJsonCrs, GeoJsonFeature, GeoJsonGeometry};

pub use parser::{
    FeatureCollection, FeatureCollectionHeader, GeoJsonDocument, GeoJsonParser,
    StreamingFeatureReader,
};

pub use writer::{GeoJsonValidator, GeoJsonWriter, IssueSeverity, ValidationIssue};

pub use filter::{FeatureFilter, FilterOp, PropertyFilter};
