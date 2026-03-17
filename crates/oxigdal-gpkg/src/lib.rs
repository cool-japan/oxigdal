//! Pure Rust GeoPackage (GPKG) reader.
//!
//! Implements a minimal SQLite binary format parser ([`sqlite_reader`]) and a
//! GeoPackage schema layer ([`gpkg`]) without any C / FFI dependencies.

pub mod error;
pub mod gpkg;
pub mod sqlite_reader;
pub mod tile_matrix;
pub mod vector;

pub use error::GpkgError;
pub use gpkg::{GeoPackage, GpkgContents, GpkgDataType, GpkgGeometryColumn, GpkgSrs};
pub use sqlite_reader::{SqliteHeader, SqliteReader, TextEncoding};
pub use tile_matrix::TileMatrix;
pub use vector::{
    FeatureRow, FeatureTable, FieldDefinition, FieldType, FieldValue, GpkgBinaryParser,
    GpkgGeometry, SrsInfo,
};
