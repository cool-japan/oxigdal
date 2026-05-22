//! High-level GeoPackage writer — composes SQLite pages into a valid `.gpkg` file.
//!
//! # Scope
//! This module supports writing GeoPackage files with:
//! - Required system tables (`gpkg_spatial_ref_sys`, `gpkg_contents`,
//!   `gpkg_geometry_columns`) populated with OGC-mandated default rows.
//! - Point feature tables (2D, single geometry column named `geom`).
//!
//! Interior B-tree page chains are not yet implemented; all tables must fit
//! on a single 4 096-byte SQLite leaf page.

pub mod builder;
pub mod feature_writer;
pub mod schema_emitter;

pub use builder::GeoPackageBuilder;
