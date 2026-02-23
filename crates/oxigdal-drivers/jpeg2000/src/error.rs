//! Error types for JPEG2000 driver

use thiserror::Error;

/// JPEG2000 specific errors
#[derive(Error, Debug)]
pub enum Jpeg2000Error {
    /// Invalid JP2 signature
    #[error("Invalid JP2 signature: expected JP2 magic bytes")]
    InvalidSignature,

    /// Invalid box type
    #[error("Invalid or unsupported box type: {0}")]
    InvalidBoxType(String),

    /// Box parsing error
    #[error("Failed to parse box {box_type}: {reason}")]
    BoxParseError {
        /// Box type that failed to parse
        box_type: String,
        /// Reason for failure
        reason: String,
    },

    /// Invalid codestream marker
    #[error("Invalid JPEG2000 codestream marker: 0x{0:04X}")]
    InvalidMarker(u16),

    /// Codestream parsing error
    #[error("Failed to parse codestream: {0}")]
    CodestreamError(String),

    /// Unsupported feature
    #[error("Unsupported JPEG2000 feature: {0}")]
    UnsupportedFeature(String),

    /// Invalid image header
    #[error("Invalid image header: {0}")]
    InvalidImageHeader(String),

    /// Invalid tile
    #[error("Invalid tile parameters: {0}")]
    InvalidTile(String),

    /// Wavelet transform error
    #[error("Wavelet transform error: {0}")]
    WaveletError(String),

    /// Tier-1 decoding error
    #[error("Tier-1 (EBCOT) decoding error: {0}")]
    Tier1Error(String),

    /// Tier-2 decoding error
    #[error("Tier-2 (packet) decoding error: {0}")]
    Tier2Error(String),

    /// Color space conversion error
    #[error("Color space conversion error: {0}")]
    ColorError(String),

    /// Invalid metadata
    #[error("Invalid metadata: {0}")]
    InvalidMetadata(String),

    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Insufficient data
    #[error("Insufficient data: expected {expected} bytes, got {actual}")]
    InsufficientData {
        /// Expected number of bytes
        expected: usize,
        /// Actual number of bytes available
        actual: usize,
    },

    /// Invalid dimension
    #[error("Invalid dimension: {0}")]
    InvalidDimension(String),

    /// Memory allocation error
    #[error("Failed to allocate memory: {0}")]
    AllocationError(String),

    /// Generic error
    #[error("{0}")]
    Other(String),
}

/// Result type for JPEG2000 operations
pub type Result<T> = std::result::Result<T, Jpeg2000Error>;

/// Error resilience mode for JPEG2000 decoding
///
/// Determines how the decoder handles corrupted or invalid data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResilienceMode {
    /// No error resilience - fail on any error
    #[default]
    None,
    /// Basic error resilience - skip corrupted packets, use error concealment
    Basic,
    /// Full error resilience - aggressive error recovery and concealment
    Full,
}

impl ResilienceMode {
    /// Check if error resilience is enabled
    pub fn is_enabled(&self) -> bool {
        !matches!(self, Self::None)
    }

    /// Check if this is full resilience mode
    pub fn is_full(&self) -> bool {
        matches!(self, Self::Full)
    }

    /// Check if this is basic or full resilience mode
    pub fn is_basic_or_higher(&self) -> bool {
        matches!(self, Self::Basic | Self::Full)
    }
}
