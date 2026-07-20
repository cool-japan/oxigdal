//! Pure-Rust SQLite binary writer for GeoPackage output.
//!
//! Generates byte-perfect SQLite files from scratch without any C/FFI
//! dependencies.  Only single-leaf-page B-trees are supported; interior
//! page chains are not yet implemented.
//!
//! Reference: <https://www.sqlite.org/fileformat.html>

/// SQLite page size used by the GeoPackage writer.
pub const PAGE_SIZE: usize = 4096;

/// SQLite magic string (16 bytes, NUL-terminated).
pub const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

/// Application-ID written into the SQLite header to identify a GeoPackage file.
/// ASCII "GPKG" = `0x4750_4B47`.
pub const APP_ID_GPKG: u32 = 0x4750_4B47;

/// GeoPackage specification version encoded as `user_version`.
/// Value `10_300` = GeoPackage 1.3.0 (100 * major + minor).
pub const USER_VERSION_130: u32 = 10_300;

pub mod btree_writer;
pub mod file_header;
pub mod page_allocator;
pub mod record;
