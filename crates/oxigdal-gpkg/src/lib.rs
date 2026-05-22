//! Pure Rust GeoPackage (GPKG) reader and writer.
//!
//! Implements a minimal SQLite binary format parser ([`sqlite_reader`]) and a
//! GeoPackage schema layer ([`gpkg`]) without any C / FFI dependencies.
//! Also includes a pure-Rust writer ([`writer`]) that can produce valid
//! GeoPackage files from scratch.

pub mod btree;
#[cfg(feature = "change-tracking")]
pub mod change_tracking;
pub mod coverage;
pub mod data_columns;
pub mod error;
pub mod extensions;
pub mod filter;
#[cfg(feature = "flatgeobuf-export")]
pub mod flatgeobuf_export;
pub mod gpkg;
pub mod incremental_mutation;
pub mod integrity;
/// POSIX advisory file locking for concurrent-read / exclusive-write
/// coordination on `.gpkg` files. Only available on unix targets with the
/// `file-locking` feature enabled.
#[cfg(all(feature = "file-locking", unix))]
pub mod locking;

#[cfg(all(feature = "file-locking", not(unix)))]
compile_error!("file-locking is only supported on unix targets");
#[cfg(feature = "mbtiles-export")]
pub mod mbtiles_export;
pub mod metadata;
pub mod multi_geom;
pub mod related_tables;
#[cfg(feature = "reproject")]
pub mod reproject;
pub mod rtree;
pub mod schema_constraints;
pub mod sqlite_reader;
pub mod sqlite_writer;
pub mod tile_matrix;
pub mod tile_pyramid;
pub mod vector;
pub mod wal;
pub mod writer;

pub use btree::{
    CellValue, MasterEntry, count_table_rows, scan_sqlite_master, scan_table, scan_table_paginated,
};
#[cfg(feature = "change-tracking")]
pub use change_tracking::{ChangeLogEntry, ChangeOperation, ChangeTracker};
pub use coverage::{
    CoverageDatatype, GridCellEncoding, GriddedCoverage, TileGriddedAncillary,
    load_gridded_coverages, load_gridded_tile_ancillary, unscale_tile_buffer_i16,
    unscale_tile_buffer_u16, unscale_value,
};
pub use data_columns::{DataColumn, DataColumnsCatalog, read_data_columns_rows};
pub use error::GpkgError;
pub use extensions::{ExtensionScope, GpkgExtension};
pub use filter::{Comparator, FilterExpr, FilterOperand, Predicate, evaluate as evaluate_filter};
#[cfg(feature = "flatgeobuf-export")]
pub use flatgeobuf_export::FlatGeoBufExporter;
pub use gpkg::{GeoPackage, GpkgContents, GpkgDataType, GpkgGeometryColumn, GpkgSrs};
pub use incremental_mutation::{GeoPackageEditor, MutationStats};
pub use integrity::{
    GPKG_APP_ID, IntegrityIssue, IntegrityReport, MIN_USER_VERSION, REQUIRED_SRS, check_integrity,
    check_integrity_strict,
};
#[cfg(all(feature = "file-locking", unix))]
pub use locking::{GpkgFileLock, LockMode, lock_for_read, lock_for_write};
#[cfg(feature = "mbtiles-export")]
pub use mbtiles_export::{GpkgMbTilesExporter, GpkgMbTilesStats};
pub use metadata::{GpkgMetadata, GpkgMetadataReference, MetadataScope, ReferenceScope};
pub use multi_geom::{
    GeometryColumnDef, MultiGeomColumnSet, has_multiple_geometry_columns,
    load_all_geometry_columns, load_geometry_columns_for_table,
};
pub use related_tables::{GpkgRelation, MappingRow, RelationType};
#[cfg(feature = "reproject")]
pub use reproject::CrsReprojector;
pub use rtree::{GpkgRTreeReader, RTreeEntry};
pub use schema_constraints::{
    ConstraintType, ConstraintValidator, ConstraintViolation, DataColumnConstraint, glob_matches,
    load_data_column_constraints,
};
pub use sqlite_reader::{SqliteHeader, SqliteReader, TextEncoding};
pub use tile_matrix::TileMatrix;
pub use tile_pyramid::TilePyramidReader;
pub use vector::{
    CoordDim, FeatureRow, FeatureTable, FieldDefinition, FieldType, FieldValue, GpkgBinaryParser,
    GpkgGeometry, Point4D, SrsInfo, WKB_EWKB_M_HIGHBIT, WKB_EWKB_ZM_HIGHBIT, WKB_POINT_M,
    WKB_POINT_ZM,
};
pub use vector::{
    feature_table_from_geojson, feature_table_to_geojson, geojson_geom_to_gpkg,
    gpkg_geom_to_geojson,
};
pub use wal::{WalReader, overlay_wal};
pub use writer::GeoPackageBuilder;
