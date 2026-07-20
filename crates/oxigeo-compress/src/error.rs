//! Error types for compression operations

use thiserror::Error;

/// Result type for compression operations
pub type Result<T> = std::result::Result<T, CompressionError>;

/// Errors that can occur during compression operations
#[derive(Error, Debug)]
pub enum CompressionError {
    /// Codec not found or not supported
    #[error("Codec not supported: {0}")]
    CodecNotSupported(String),

    /// Compression failed
    #[error("Compression failed: {0}")]
    CompressionFailed(String),

    /// Decompression failed
    #[error("Decompression failed: {0}")]
    DecompressionFailed(String),

    /// Invalid compression level
    #[error("Invalid compression level: {level}, expected range {min}..={max}")]
    InvalidCompressionLevel {
        /// Provided compression level
        level: i32,
        /// Minimum acceptable level
        min: i32,
        /// Maximum acceptable level
        max: i32,
    },

    /// Invalid buffer size
    #[error("Invalid buffer size: {0}")]
    InvalidBufferSize(String),

    /// Checksum mismatch
    #[error("Checksum mismatch: expected {expected:x}, got {actual:x}")]
    ChecksumMismatch {
        /// Expected checksum
        expected: u64,
        /// Actual checksum
        actual: u64,
    },

    /// Integrity check failed
    #[error("Integrity check failed: {0}")]
    IntegrityCheckFailed(String),

    /// Invalid metadata
    #[error("Invalid metadata: {0}")]
    InvalidMetadata(String),

    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Invalid parameter
    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    /// LZ4 codec error
    #[error("LZ4 error: {0}")]
    Lz4Error(String),

    /// Zstd codec error
    #[error("Zstd error: {0}")]
    ZstdError(String),

    /// Brotli codec error
    #[error("Brotli error: {0}")]
    BrotliError(String),

    /// Snappy codec error
    #[error("Snappy codec error: {0}")]
    SnappyError(String),

    /// Deflate codec error
    #[error("Deflate error: {0}")]
    DeflateError(String),

    /// Delta codec error
    #[error("Delta codec error: {0}")]
    DeltaError(String),

    /// RLE codec error
    #[error("RLE error: {0}")]
    RleError(String),

    /// Dictionary codec error
    #[error("Dictionary error: {0}")]
    DictionaryError(String),

    /// Floating-point compression error
    #[error("Floating-point compression error: {0}")]
    FloatingPointError(String),

    /// Auto-selection error
    #[error("Auto-selection error: {0}")]
    AutoSelectionError(String),

    /// Parallel processing error
    #[error("Parallel processing error: {0}")]
    ParallelError(String),

    /// Streaming error
    #[error("Streaming error: {0}")]
    StreamingError(String),

    /// Unsupported data type
    #[error("Unsupported data type: {0}")]
    UnsupportedDataType(String),

    /// Buffer too small
    #[error("Buffer too small: required {required}, available {available}")]
    BufferTooSmall {
        /// Required buffer size
        required: usize,
        /// Available buffer size
        available: usize,
    },

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigurationError(String),
}

// Note: lz4 0.1 removed CompressError and DecompressError types
// Errors are now returned as strings
// Users should handle lz4 errors inline where they occur

impl From<oxiarc_snappy::SnappyError> for CompressionError {
    fn from(err: oxiarc_snappy::SnappyError) -> Self {
        CompressionError::SnappyError(err.to_string())
    }
}
