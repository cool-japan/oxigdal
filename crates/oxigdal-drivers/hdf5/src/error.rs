//! HDF5-specific error types for OxiGDAL HDF5 driver.
//!
//! This module provides comprehensive error handling for HDF5 file operations,
//! following the COOLJAPAN No Unwrap policy.
//!
//! # Error Codes
//!
//! Each error variant has an associated error code (e.g., H001, H002) for easier
//! debugging and documentation. Error codes are stable across versions.
//!
//! # Helper Methods
//!
//! All error types provide:
//! - `code()` - Returns the error code
//! - `suggestion()` - Returns helpful hints for fixing the error
//! - `context()` - Returns additional context about the error (dataset/attribute names, etc.)

use std::io;
use thiserror::Error;

/// Result type for HDF5 operations
pub type Result<T> = std::result::Result<T, Hdf5Error>;

/// Comprehensive HDF5 error types
#[derive(Debug, Error)]
pub enum Hdf5Error {
    /// I/O error during file operations
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// Invalid HDF5 file format
    #[error("Invalid HDF5 format: {0}")]
    InvalidFormat(String),

    /// Invalid HDF5 signature (should be `\x89HDF\r\n\x1a\n`)
    #[error("Invalid HDF5 signature: expected \\x89HDF\\r\\n\\x1a\\n, got {0:?}")]
    InvalidSignature(Vec<u8>),

    /// Unsupported HDF5 version
    #[error("Unsupported HDF5 version: {0}.{1}")]
    UnsupportedVersion(u8, u8),

    /// Unsupported superblock version
    #[error("Unsupported superblock version: {0}")]
    UnsupportedSuperblockVersion(u8),

    /// Invalid object header
    #[error("Invalid object header: {0}")]
    InvalidObjectHeader(String),

    /// Dataset not found
    #[error("Dataset not found: {0}")]
    DatasetNotFound(String),

    /// Group not found
    #[error("Group not found: {0}")]
    GroupNotFound(String),

    /// Attribute not found
    #[error("Attribute not found: {0}")]
    AttributeNotFound(String),

    /// Invalid datatype
    #[error("Invalid datatype: {0}")]
    InvalidDatatype(String),

    /// Unsupported datatype
    #[error("Unsupported datatype: {0}")]
    UnsupportedDatatype(String),

    /// Type conversion error
    #[error("Type conversion error: cannot convert {from} to {to}")]
    TypeConversion {
        /// Source type
        from: String,
        /// Target type
        to: String,
    },

    /// Invalid dimensions
    #[error("Invalid dimensions: {0}")]
    InvalidDimensions(String),

    /// Invalid shape
    #[error("Invalid shape: expected {expected:?}, got {actual:?}")]
    InvalidShape {
        /// Expected shape
        expected: Vec<usize>,
        /// Actual shape
        actual: Vec<usize>,
    },

    /// Invalid chunk size
    #[error("Invalid chunk size: {0}")]
    InvalidChunkSize(String),

    /// Compression error
    #[error("Compression error: {0}")]
    Compression(String),

    /// Decompression error
    #[error("Decompression error: {0}")]
    Decompression(String),

    /// Unsupported compression filter
    #[error("Unsupported compression filter: {0}")]
    UnsupportedCompressionFilter(String),

    /// Checksum mismatch
    #[error("Checksum mismatch: expected {expected:#010x}, got {actual:#010x}")]
    ChecksumMismatch {
        /// Expected checksum value
        expected: u32,
        /// Actual computed checksum value
        actual: u32,
    },

    /// Invalid attribute value
    #[error("Invalid attribute value: {0}")]
    InvalidAttributeValue(String),

    /// Out of bounds access
    #[error("Out of bounds: index {index} is out of range for size {size}")]
    OutOfBounds {
        /// Requested index
        index: usize,
        /// Maximum size
        size: usize,
    },

    /// Invalid offset
    #[error("Invalid offset: {0}")]
    InvalidOffset(String),

    /// Invalid size
    #[error("Invalid size: {0}")]
    InvalidSize(String),

    /// Symbol table error
    #[error("Symbol table error: {0}")]
    SymbolTable(String),

    /// B-tree error
    #[error("B-tree error: {0}")]
    BTree(String),

    /// Heap error
    #[error("Heap error: {0}")]
    Heap(String),

    /// Message error
    #[error("Message error: {0}")]
    Message(String),

    /// Invalid message type
    #[error("Invalid message type: {0}")]
    InvalidMessageType(u8),

    /// Unsupported message type
    #[error("Unsupported message type: {0}")]
    UnsupportedMessageType(u8),

    /// Layout error
    #[error("Layout error: {0}")]
    Layout(String),

    /// Unsupported layout
    #[error("Unsupported layout: {0}")]
    UnsupportedLayout(String),

    /// Fill value error
    #[error("Fill value error: {0}")]
    FillValue(String),

    /// Filter pipeline error
    #[error("Filter pipeline error: {0}")]
    FilterPipeline(String),

    /// Invalid filter
    #[error("Invalid filter: {0}")]
    InvalidFilter(String),

    /// String encoding error
    #[error("String encoding error: {0}")]
    StringEncoding(#[from] std::string::FromUtf8Error),

    /// Invalid string padding
    #[error("Invalid string padding: {0}")]
    InvalidStringPadding(String),

    /// Reference error
    #[error("Reference error: {0}")]
    Reference(String),

    /// Invalid reference
    #[error("Invalid reference: {0}")]
    InvalidReference(String),

    /// Dataspace error
    #[error("Dataspace error: {0}")]
    Dataspace(String),

    /// Invalid dataspace
    #[error("Invalid dataspace: {0}")]
    InvalidDataspace(String),

    /// Unsupported dataspace
    #[error("Unsupported dataspace: {0}")]
    UnsupportedDataspace(String),

    /// Selection error
    #[error("Selection error: {0}")]
    Selection(String),

    /// Link error
    #[error("Link error: {0}")]
    Link(String),

    /// Invalid link
    #[error("Invalid link: {0}")]
    InvalidLink(String),

    /// Unsupported link type
    #[error("Unsupported link type: {0}")]
    UnsupportedLinkType(String),

    /// Object already exists
    #[error("Object already exists: {0}")]
    ObjectExists(String),

    /// Invalid object name
    #[error("Invalid object name: {0}")]
    InvalidObjectName(String),

    /// Path not found
    #[error("Path not found: {0}")]
    PathNotFound(String),

    /// Invalid path
    #[error("Invalid path: {0}")]
    InvalidPath(String),

    /// File is read-only
    #[error("File is read-only")]
    ReadOnly,

    /// File is write-only
    #[error("File is write-only")]
    WriteOnly,

    /// File already open
    #[error("File already open: {0}")]
    FileAlreadyOpen(String),

    /// File not open
    #[error("File not open")]
    FileNotOpen,

    /// Feature not available (Pure Rust limitations)
    #[error(
        "Feature not available in Pure Rust mode: {feature}. Enable 'hdf5_sys' feature for full support"
    )]
    FeatureNotAvailable {
        /// Feature name
        feature: String,
    },

    /// Internal error (should not happen)
    #[error("Internal error: {0}")]
    Internal(String),

    /// UTF-8 conversion error
    #[error("UTF-8 conversion error: {0}")]
    Utf8Error(#[from] std::str::Utf8Error),

    /// Integer conversion error
    #[error("Integer conversion error: {0}")]
    TryFromIntError(#[from] std::num::TryFromIntError),

    /// OxiGDAL core error
    #[error("OxiGDAL core error: {0}")]
    Core(#[from] oxigdal_core::error::OxiGdalError),
}

impl Hdf5Error {
    /// Create an invalid format error
    pub fn invalid_format(msg: impl Into<String>) -> Self {
        Self::InvalidFormat(msg.into())
    }

    /// Create an invalid datatype error
    pub fn invalid_datatype(msg: impl Into<String>) -> Self {
        Self::InvalidDatatype(msg.into())
    }

    /// Create an unsupported datatype error
    pub fn unsupported_datatype(msg: impl Into<String>) -> Self {
        Self::UnsupportedDatatype(msg.into())
    }

    /// Create a type conversion error
    pub fn type_conversion(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self::TypeConversion {
            from: from.into(),
            to: to.into(),
        }
    }

    /// Create an invalid dimensions error
    pub fn invalid_dimensions(msg: impl Into<String>) -> Self {
        Self::InvalidDimensions(msg.into())
    }

    /// Create an invalid shape error
    pub fn invalid_shape(expected: Vec<usize>, actual: Vec<usize>) -> Self {
        Self::InvalidShape { expected, actual }
    }

    /// Create a compression error
    pub fn compression(msg: impl Into<String>) -> Self {
        Self::Compression(msg.into())
    }

    /// Create a decompression error
    pub fn decompression(msg: impl Into<String>) -> Self {
        Self::Decompression(msg.into())
    }

    /// Create a dataset not found error
    pub fn dataset_not_found(name: impl Into<String>) -> Self {
        Self::DatasetNotFound(name.into())
    }

    /// Create a group not found error
    pub fn group_not_found(name: impl Into<String>) -> Self {
        Self::GroupNotFound(name.into())
    }

    /// Create an attribute not found error
    pub fn attribute_not_found(name: impl Into<String>) -> Self {
        Self::AttributeNotFound(name.into())
    }

    /// Create a feature not available error
    pub fn feature_not_available(feature: impl Into<String>) -> Self {
        Self::FeatureNotAvailable {
            feature: feature.into(),
        }
    }

    /// Create an internal error
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }

    /// Get the error code for this HDF5 error
    ///
    /// Error codes are stable across versions and can be used for documentation
    /// and error handling.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io(_) => "H001",
            Self::InvalidFormat(_) => "H002",
            Self::InvalidSignature(_) => "H003",
            Self::UnsupportedVersion(_, _) => "H004",
            Self::UnsupportedSuperblockVersion(_) => "H005",
            Self::InvalidObjectHeader(_) => "H006",
            Self::DatasetNotFound(_) => "H007",
            Self::GroupNotFound(_) => "H008",
            Self::AttributeNotFound(_) => "H009",
            Self::InvalidDatatype(_) => "H010",
            Self::UnsupportedDatatype(_) => "H011",
            Self::TypeConversion { .. } => "H012",
            Self::InvalidDimensions(_) => "H013",
            Self::InvalidShape { .. } => "H014",
            Self::InvalidChunkSize(_) => "H015",
            Self::Compression(_) => "H016",
            Self::Decompression(_) => "H017",
            Self::UnsupportedCompressionFilter(_) => "H018",
            Self::ChecksumMismatch { .. } => "H019",
            Self::InvalidAttributeValue(_) => "H020",
            Self::OutOfBounds { .. } => "H021",
            Self::InvalidOffset(_) => "H022",
            Self::InvalidSize(_) => "H023",
            Self::SymbolTable(_) => "H024",
            Self::BTree(_) => "H025",
            Self::Heap(_) => "H026",
            Self::Message(_) => "H027",
            Self::InvalidMessageType(_) => "H028",
            Self::UnsupportedMessageType(_) => "H029",
            Self::Layout(_) => "H030",
            Self::UnsupportedLayout(_) => "H031",
            Self::FillValue(_) => "H032",
            Self::FilterPipeline(_) => "H033",
            Self::InvalidFilter(_) => "H034",
            Self::StringEncoding(_) => "H035",
            Self::InvalidStringPadding(_) => "H036",
            Self::Reference(_) => "H037",
            Self::InvalidReference(_) => "H038",
            Self::Dataspace(_) => "H039",
            Self::InvalidDataspace(_) => "H040",
            Self::UnsupportedDataspace(_) => "H041",
            Self::Selection(_) => "H042",
            Self::Link(_) => "H043",
            Self::InvalidLink(_) => "H044",
            Self::UnsupportedLinkType(_) => "H045",
            Self::ObjectExists(_) => "H046",
            Self::InvalidObjectName(_) => "H047",
            Self::PathNotFound(_) => "H048",
            Self::InvalidPath(_) => "H049",
            Self::ReadOnly => "H050",
            Self::WriteOnly => "H051",
            Self::FileAlreadyOpen(_) => "H052",
            Self::FileNotOpen => "H053",
            Self::FeatureNotAvailable { .. } => "H054",
            Self::Internal(_) => "H055",
            Self::Utf8Error(_) => "H056",
            Self::TryFromIntError(_) => "H057",
            Self::Core(_) => "H058",
        }
    }

    /// Get a helpful suggestion for fixing this HDF5 error
    ///
    /// Returns a human-readable suggestion that can help users resolve the error.
    pub fn suggestion(&self) -> Option<&'static str> {
        match self {
            Self::Io(_) => Some("Check file permissions and disk space"),
            Self::InvalidFormat(_) => {
                Some("Verify the file is a valid HDF5 file. Try opening with h5dump or h5ls")
            }
            Self::InvalidSignature(_) => {
                Some("The file may not be an HDF5 file. Check the file format")
            }
            Self::UnsupportedVersion(_, _) => {
                Some("This HDF5 version is not supported. Try converting to a newer format")
            }
            Self::UnsupportedSuperblockVersion(_) => {
                Some("This superblock version is not supported. Use h5repack to convert")
            }
            Self::InvalidObjectHeader(_) => {
                Some("The file may be corrupted. Try recovering with h5recover")
            }
            Self::DatasetNotFound(_) => {
                Some("Check the dataset path. Use h5ls to list available datasets")
            }
            Self::GroupNotFound(_) => {
                Some("Check the group path. Use h5ls to list available groups")
            }
            Self::AttributeNotFound(_) => {
                Some("Check the attribute name. Use h5dump -A to list attributes")
            }
            Self::InvalidDatatype(_) => {
                Some("The datatype is not recognized. The file may be corrupted")
            }
            Self::UnsupportedDatatype(_) => {
                Some("This datatype is not yet supported in Pure Rust mode")
            }
            Self::TypeConversion { .. } => {
                Some("The type conversion is not supported. Use explicit casting")
            }
            Self::InvalidDimensions(_) => {
                Some("Check the dataset dimensions match the expected shape")
            }
            Self::InvalidShape { .. } => {
                Some("The data shape doesn't match the dataset shape. Verify dimensions")
            }
            Self::InvalidChunkSize(_) => {
                Some("Chunk size must be positive and less than dataset size")
            }
            Self::Compression(_) => {
                Some("Check compression settings. Try a different compression method")
            }
            Self::Decompression(_) => {
                Some("The compressed data may be corrupted. Try re-downloading the file")
            }
            Self::UnsupportedCompressionFilter(_) => Some(
                "This compression filter is not supported. Enable the corresponding feature or use a different filter",
            ),
            Self::ChecksumMismatch { .. } => {
                Some("Data corruption detected. The file may be damaged")
            }
            Self::InvalidAttributeValue(_) => Some("The attribute value format is invalid"),
            Self::OutOfBounds { .. } => Some("Check array indices are within the dataset bounds"),
            Self::InvalidOffset(_) => Some("The file offset is invalid. The file may be corrupted"),
            Self::InvalidSize(_) => Some("The size value is invalid. Check dataset metadata"),
            Self::SymbolTable(_) => Some("Symbol table error. The file structure may be corrupted"),
            Self::BTree(_) => Some("B-tree error. The file index may be corrupted"),
            Self::Heap(_) => Some("Heap error. The file heap may be corrupted"),
            Self::Message(_) => Some("Message parsing error. The file may be corrupted"),
            Self::InvalidMessageType(_) => {
                Some("Unknown message type. The file may be from a newer HDF5 version")
            }
            Self::UnsupportedMessageType(_) => Some("This message type is not yet supported"),
            Self::Layout(_) => Some("Dataset layout error. Check chunk settings"),
            Self::UnsupportedLayout(_) => Some("This dataset layout is not yet supported"),
            Self::FillValue(_) => Some("Fill value error. Check the dataset metadata"),
            Self::FilterPipeline(_) => Some("Filter pipeline error. Check compression settings"),
            Self::InvalidFilter(_) => Some("Invalid filter configuration"),
            Self::StringEncoding(_) => {
                Some("String encoding error. The file may contain invalid UTF-8")
            }
            Self::InvalidStringPadding(_) => Some("String padding is invalid"),
            Self::Reference(_) => Some("Object reference error"),
            Self::InvalidReference(_) => {
                Some("Invalid object reference. The referenced object may not exist")
            }
            Self::Dataspace(_) => Some("Dataspace error. Check dataset dimensions"),
            Self::InvalidDataspace(_) => Some("Invalid dataspace configuration"),
            Self::UnsupportedDataspace(_) => Some("This dataspace type is not yet supported"),
            Self::Selection(_) => Some("Hyperslab selection error. Check selection bounds"),
            Self::Link(_) => Some("Link error. The linked object may not exist"),
            Self::InvalidLink(_) => Some("Invalid link. Check the link path"),
            Self::UnsupportedLinkType(_) => Some("This link type is not yet supported"),
            Self::ObjectExists(_) => {
                Some("An object with this name already exists. Use a different name")
            }
            Self::InvalidObjectName(_) => {
                Some("Object names cannot contain '/' or NULL characters")
            }
            Self::PathNotFound(_) => Some("The path does not exist. Use h5ls to verify the path"),
            Self::InvalidPath(_) => Some("The path format is invalid. Paths must start with '/'"),
            Self::ReadOnly => Some("The file was opened in read-only mode. Open with write access"),
            Self::WriteOnly => {
                Some("The file was opened in write-only mode. Open with read access")
            }
            Self::FileAlreadyOpen(_) => Some("Close the file before opening it again"),
            Self::FileNotOpen => Some("Open the file before performing operations"),
            Self::FeatureNotAvailable { .. } => Some(
                "Enable the 'hdf5_sys' feature for full HDF5 support, or use Pure Rust compatible features",
            ),
            Self::Internal(_) => {
                Some("This is likely a bug. Please report it with steps to reproduce")
            }
            Self::Utf8Error(_) => Some("Invalid UTF-8 encoding in string data"),
            Self::TryFromIntError(_) => Some("Integer conversion overflow. The value is too large"),
            Self::Core(_) => Some("Check the underlying error message for details"),
        }
    }

    /// Get additional context about this HDF5 error
    ///
    /// Returns structured context information including dataset/attribute names and paths.
    pub fn context(&self) -> ErrorContext {
        match self {
            Self::Io(e) => ErrorContext::new("io_error").with_detail("message", e.to_string()),
            Self::InvalidFormat(msg) => {
                ErrorContext::new("invalid_format").with_detail("message", msg.clone())
            }
            Self::InvalidSignature(sig) => ErrorContext::new("invalid_signature")
                .with_detail("signature", format!("{:?}", sig)),
            Self::UnsupportedVersion(major, minor) => ErrorContext::new("unsupported_version")
                .with_detail("major", major.to_string())
                .with_detail("minor", minor.to_string()),
            Self::UnsupportedSuperblockVersion(version) => {
                ErrorContext::new("unsupported_superblock")
                    .with_detail("version", version.to_string())
            }
            Self::InvalidObjectHeader(msg) => {
                ErrorContext::new("invalid_object_header").with_detail("message", msg.clone())
            }
            Self::DatasetNotFound(name) => {
                ErrorContext::new("dataset_not_found").with_detail("dataset", name.clone())
            }
            Self::GroupNotFound(name) => {
                ErrorContext::new("group_not_found").with_detail("group", name.clone())
            }
            Self::AttributeNotFound(name) => {
                ErrorContext::new("attribute_not_found").with_detail("attribute", name.clone())
            }
            Self::InvalidDatatype(msg) => {
                ErrorContext::new("invalid_datatype").with_detail("message", msg.clone())
            }
            Self::UnsupportedDatatype(msg) => {
                ErrorContext::new("unsupported_datatype").with_detail("message", msg.clone())
            }
            Self::TypeConversion { from, to } => ErrorContext::new("type_conversion")
                .with_detail("from", from.clone())
                .with_detail("to", to.clone()),
            Self::InvalidDimensions(msg) => {
                ErrorContext::new("invalid_dimensions").with_detail("message", msg.clone())
            }
            Self::InvalidShape { expected, actual } => ErrorContext::new("invalid_shape")
                .with_detail("expected", format!("{:?}", expected))
                .with_detail("actual", format!("{:?}", actual)),
            Self::InvalidChunkSize(msg) => {
                ErrorContext::new("invalid_chunk_size").with_detail("message", msg.clone())
            }
            Self::Compression(msg) => {
                ErrorContext::new("compression").with_detail("message", msg.clone())
            }
            Self::Decompression(msg) => {
                ErrorContext::new("decompression").with_detail("message", msg.clone())
            }
            Self::UnsupportedCompressionFilter(msg) => {
                ErrorContext::new("unsupported_filter").with_detail("filter", msg.clone())
            }
            Self::ChecksumMismatch { expected, actual } => ErrorContext::new("checksum_mismatch")
                .with_detail("expected", format!("{:#010x}", expected))
                .with_detail("actual", format!("{:#010x}", actual)),
            Self::InvalidAttributeValue(msg) => {
                ErrorContext::new("invalid_attribute_value").with_detail("message", msg.clone())
            }
            Self::OutOfBounds { index, size } => ErrorContext::new("out_of_bounds")
                .with_detail("index", index.to_string())
                .with_detail("size", size.to_string()),
            Self::InvalidOffset(msg) => {
                ErrorContext::new("invalid_offset").with_detail("message", msg.clone())
            }
            Self::InvalidSize(msg) => {
                ErrorContext::new("invalid_size").with_detail("message", msg.clone())
            }
            Self::SymbolTable(msg) => {
                ErrorContext::new("symbol_table").with_detail("message", msg.clone())
            }
            Self::BTree(msg) => ErrorContext::new("btree").with_detail("message", msg.clone()),
            Self::Heap(msg) => ErrorContext::new("heap").with_detail("message", msg.clone()),
            Self::Message(msg) => ErrorContext::new("message").with_detail("message", msg.clone()),
            Self::InvalidMessageType(type_id) => ErrorContext::new("invalid_message_type")
                .with_detail("type_id", type_id.to_string()),
            Self::UnsupportedMessageType(type_id) => ErrorContext::new("unsupported_message_type")
                .with_detail("type_id", type_id.to_string()),
            Self::Layout(msg) => ErrorContext::new("layout").with_detail("message", msg.clone()),
            Self::UnsupportedLayout(msg) => {
                ErrorContext::new("unsupported_layout").with_detail("message", msg.clone())
            }
            Self::FillValue(msg) => {
                ErrorContext::new("fill_value").with_detail("message", msg.clone())
            }
            Self::FilterPipeline(msg) => {
                ErrorContext::new("filter_pipeline").with_detail("message", msg.clone())
            }
            Self::InvalidFilter(msg) => {
                ErrorContext::new("invalid_filter").with_detail("message", msg.clone())
            }
            Self::StringEncoding(e) => {
                ErrorContext::new("string_encoding").with_detail("error", e.to_string())
            }
            Self::InvalidStringPadding(msg) => {
                ErrorContext::new("invalid_string_padding").with_detail("message", msg.clone())
            }
            Self::Reference(msg) => {
                ErrorContext::new("reference").with_detail("message", msg.clone())
            }
            Self::InvalidReference(msg) => {
                ErrorContext::new("invalid_reference").with_detail("message", msg.clone())
            }
            Self::Dataspace(msg) => {
                ErrorContext::new("dataspace").with_detail("message", msg.clone())
            }
            Self::InvalidDataspace(msg) => {
                ErrorContext::new("invalid_dataspace").with_detail("message", msg.clone())
            }
            Self::UnsupportedDataspace(msg) => {
                ErrorContext::new("unsupported_dataspace").with_detail("message", msg.clone())
            }
            Self::Selection(msg) => {
                ErrorContext::new("selection").with_detail("message", msg.clone())
            }
            Self::Link(msg) => ErrorContext::new("link").with_detail("message", msg.clone()),
            Self::InvalidLink(msg) => {
                ErrorContext::new("invalid_link").with_detail("message", msg.clone())
            }
            Self::UnsupportedLinkType(msg) => {
                ErrorContext::new("unsupported_link_type").with_detail("message", msg.clone())
            }
            Self::ObjectExists(name) => {
                ErrorContext::new("object_exists").with_detail("name", name.clone())
            }
            Self::InvalidObjectName(name) => {
                ErrorContext::new("invalid_object_name").with_detail("name", name.clone())
            }
            Self::PathNotFound(path) => {
                ErrorContext::new("path_not_found").with_detail("path", path.clone())
            }
            Self::InvalidPath(path) => {
                ErrorContext::new("invalid_path").with_detail("path", path.clone())
            }
            Self::ReadOnly => ErrorContext::new("read_only"),
            Self::WriteOnly => ErrorContext::new("write_only"),
            Self::FileAlreadyOpen(path) => {
                ErrorContext::new("file_already_open").with_detail("path", path.clone())
            }
            Self::FileNotOpen => ErrorContext::new("file_not_open"),
            Self::FeatureNotAvailable { feature } => {
                ErrorContext::new("feature_not_available").with_detail("feature", feature.clone())
            }
            Self::Internal(msg) => {
                ErrorContext::new("internal").with_detail("message", msg.clone())
            }
            Self::Utf8Error(e) => {
                ErrorContext::new("utf8_error").with_detail("error", e.to_string())
            }
            Self::TryFromIntError(e) => {
                ErrorContext::new("int_conversion_error").with_detail("error", e.to_string())
            }
            Self::Core(e) => ErrorContext::new("core_error").with_detail("error", e.to_string()),
        }
    }
}

/// Additional context information for HDF5 errors
#[derive(Debug, Clone)]
pub struct ErrorContext {
    /// Error category for grouping similar errors
    pub category: &'static str,
    /// Additional details about the error (dataset/attribute names, paths, etc.)
    pub details: Vec<(String, String)>,
}

impl ErrorContext {
    /// Create a new error context
    pub fn new(category: &'static str) -> Self {
        Self {
            category,
            details: Vec::new(),
        }
    }

    /// Add a detail to the context
    pub fn with_detail(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.details.push((key.into(), value.into()));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = Hdf5Error::invalid_format("bad magic number");
        assert_eq!(err.to_string(), "Invalid HDF5 format: bad magic number");

        let err = Hdf5Error::dataset_not_found("temperature");
        assert_eq!(err.to_string(), "Dataset not found: temperature");

        let err = Hdf5Error::type_conversion("f32", "i32");
        assert_eq!(
            err.to_string(),
            "Type conversion error: cannot convert f32 to i32"
        );
    }

    #[test]
    fn test_invalid_signature() {
        let sig = vec![0x89, 0x48, 0x44, 0x46];
        let err = Hdf5Error::InvalidSignature(sig.clone());
        assert!(err.to_string().contains("Invalid HDF5 signature"));
    }

    #[test]
    fn test_unsupported_version() {
        let err = Hdf5Error::UnsupportedVersion(3, 0);
        assert_eq!(err.to_string(), "Unsupported HDF5 version: 3.0");
    }

    #[test]
    fn test_invalid_shape() {
        let err = Hdf5Error::invalid_shape(vec![10, 20], vec![10, 30]);
        assert!(err.to_string().contains("Invalid shape"));
        assert!(err.to_string().contains("10, 20"));
        assert!(err.to_string().contains("10, 30"));
    }

    #[test]
    fn test_feature_not_available() {
        let err = Hdf5Error::feature_not_available("SZIP compression");
        assert!(
            err.to_string()
                .contains("Feature not available in Pure Rust mode")
        );
        assert!(err.to_string().contains("SZIP compression"));
    }

    #[test]
    fn test_error_codes() {
        let err = Hdf5Error::DatasetNotFound("temperature".to_string());
        assert_eq!(err.code(), "H007");

        let err = Hdf5Error::AttributeNotFound("units".to_string());
        assert_eq!(err.code(), "H009");

        let err = Hdf5Error::ChecksumMismatch {
            expected: 0x12345678,
            actual: 0x87654321,
        };
        assert_eq!(err.code(), "H019");
    }

    #[test]
    fn test_error_suggestions() {
        let err = Hdf5Error::DatasetNotFound("temperature".to_string());
        assert!(err.suggestion().is_some());
        assert!(err.suggestion().is_some_and(|s| s.contains("h5ls")));

        let err = Hdf5Error::AttributeNotFound("units".to_string());
        assert!(err.suggestion().is_some());
        assert!(err.suggestion().is_some_and(|s| s.contains("h5dump")));
    }

    #[test]
    fn test_error_context() {
        let err = Hdf5Error::DatasetNotFound("temperature".to_string());
        let ctx = err.context();
        assert_eq!(ctx.category, "dataset_not_found");
        assert!(
            ctx.details
                .iter()
                .any(|(k, v)| k == "dataset" && v == "temperature")
        );

        let err = Hdf5Error::AttributeNotFound("units".to_string());
        let ctx = err.context();
        assert_eq!(ctx.category, "attribute_not_found");
        assert!(
            ctx.details
                .iter()
                .any(|(k, v)| k == "attribute" && v == "units")
        );

        let err = Hdf5Error::TypeConversion {
            from: "f32".to_string(),
            to: "i64".to_string(),
        };
        let ctx = err.context();
        assert_eq!(ctx.category, "type_conversion");
        assert!(ctx.details.iter().any(|(k, v)| k == "from" && v == "f32"));
        assert!(ctx.details.iter().any(|(k, v)| k == "to" && v == "i64"));
    }

    #[test]
    fn test_error_context_with_shape() {
        let err = Hdf5Error::InvalidShape {
            expected: vec![10, 20, 30],
            actual: vec![10, 20, 40],
        };
        let ctx = err.context();
        assert_eq!(ctx.category, "invalid_shape");
        assert!(!ctx.details.is_empty());
    }
}
