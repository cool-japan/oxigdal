//! Error types for oxigeo-gpkg

use thiserror::Error;

/// Errors that can occur when parsing GeoPackage / SQLite files.
#[derive(Debug, Error)]
pub enum GpkgError {
    /// The binary data does not conform to the expected format.
    #[error("Invalid format: {0}")]
    InvalidFormat(String),

    /// An I/O error occurred while reading.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// The GeoPackage geometry blob does not start with the expected magic bytes (0x47 0x50).
    #[error("Invalid GeoPackage geometry magic bytes")]
    InvalidGeometryMagic,

    /// A WKB geometry could not be parsed.
    #[error("WKB parse error: {0}")]
    WkbParseError(String),

    /// The WKB type code is not recognised.
    #[error("Unknown WKB geometry type: {0}")]
    UnknownWkbType(u32),

    /// A parse operation needed more bytes than were available.
    #[error("Insufficient data: needed {needed} bytes, available {available}")]
    InsufficientData {
        /// Number of bytes required by the operation.
        needed: usize,
        /// Number of bytes actually present in the buffer.
        available: usize,
    },

    /// An advisory file-locking operation failed.
    #[error("Locking error: {0}")]
    LockingError(String),

    /// The WAL file header begins with an unrecognised magic number.
    #[error("Invalid WAL magic: {0:#010x}")]
    InvalidWalMagic(u32),

    /// A WAL frame's cumulative checksum does not match the expected value.
    #[error("WAL checksum mismatch at frame for page {page}")]
    WalChecksumMismatch {
        /// The 1-indexed database page number of the offending frame.
        page: u32,
    },

    /// The WAL page size differs from the expected value.
    #[error("WAL page size mismatch: expected {expected}, actual {actual}")]
    WalSizeMismatch {
        /// Expected page size derived from the main database.
        expected: u32,
        /// Actual page size read from the WAL header.
        actual: u32,
    },

    /// The requested SQLite table was not found in the master page.
    #[error("Table not found: {0}")]
    TableNotFound(String),

    /// The requested tile set table was not found or has no tile data.
    #[error("Tile set not found: {0}")]
    TileSetNotFound(String),

    /// A single row's encoded size exceeds the maximum that fits in one leaf page.
    #[error("Row size {size} bytes exceeds single-leaf-page maximum {max} bytes")]
    RowOverflowsPage {
        /// Size of the row in bytes.
        size: usize,
        /// Maximum allowed row size in bytes.
        max: usize,
    },

    /// Geometry type has no corresponding representation in the target format.
    #[error("Unsupported geometry type for conversion: {kind}")]
    UnsupportedGeometry {
        /// Name of the geometry kind that cannot be converted.
        kind: String,
    },

    /// A string-typed metadata value could not be parsed into its typed form.
    #[error("Parse error: {0}")]
    ParseError(String),

    /// A cell value failed validation against a `gpkg_data_column_constraints`
    /// rule.  Reported by [`crate::schema_constraints::ConstraintValidator`].
    #[error("constraint violation for '{constraint_name}': {reason}")]
    ConstraintViolation {
        /// Name of the violated constraint (matches `constraint_name` column).
        constraint_name: String,
        /// Human-readable reason explaining why the value failed.
        reason: String,
    },

    /// A CRS reprojection operation failed.  Reported by
    /// `CrsReprojector` (feature-gated) when the underlying
    /// `oxigeo_proj::Transformer` returns an error.
    #[error("CRS reprojection failed: {0}")]
    ReprojectionError(String),

    /// The named gridded coverage table was not found in the GeoPackage.
    ///
    /// Reported when a caller explicitly requests a coverage by name that does
    /// not appear in `gpkg_2d_gridded_coverage_ancillary`.
    #[error("Gridded coverage not found: {0}")]
    CoverageNotFound(String),

    /// The `datatype` column in `gpkg_2d_gridded_coverage_ancillary` contains an
    /// unrecognised value (expected `"integer"` or `"float"`).
    #[error("Invalid coverage datatype: {0}")]
    InvalidCoverageDatatype(String),

    /// FlatGeoBuf export failed.
    #[error("flatgeobuf export error: {0}")]
    FlatGeoBufExportError(String),

    /// Feature with the given FID was not found in the snapshot.
    #[error("Feature not found: fid={0}")]
    FeatureNotFound(i64),

    /// A buffered-rewrite mutation operation failed.
    #[error("Mutation error: {0}")]
    MutationError(String),

    /// MBTiles export error
    #[error("MBTiles export error: {0}")]
    MbTilesExportError(String),

    /// Error from the change tracking subsystem.
    #[error("change tracking error: {0}")]
    ChangeTrackingError(String),
}
