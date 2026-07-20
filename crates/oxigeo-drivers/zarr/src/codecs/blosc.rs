//! Blosc codec implementation for Zarr arrays
//!
//! # Status: NOT IMPLEMENTED (C Dependency)
//!
//! Blosc is a high-performance compressor optimized for binary data,
//! commonly used in Zarr arrays. However, it requires C/Fortran dependencies
//! which violates the COOLJAPAN Pure Rust Policy.
//!
//! ## Alternatives
//!
//! For pure Rust compression in Zarr arrays, use:
//!
//! - **ZSTD** - Excellent compression ratio and speed, similar to Blosc
//! - **LZ4** - Very fast compression, good for real-time use cases
//! - **Gzip** - Widely compatible, moderate speed
//!
//! These codecs provide comparable or better performance while maintaining
//! 100% Pure Rust compatibility.
//!
//! ## Future Work
//!
//! A pure Rust Blosc implementation would require:
//! - Pure Rust implementation of the Blosc format
//! - Pure Rust versions of internal compressors (LZ4, Zstd, etc.)
//! - Bit/byte shuffle algorithms in Rust
//!
//! This is a significant undertaking and may be addressed in future versions
//! if there is sufficient demand.

use super::Codec;
use crate::error::{CodecError, Result, ZarrError};

/// Blosc codec configuration
///
/// **Note**: This codec is not currently implemented due to C library dependencies.
/// Use ZSTD, LZ4, or Gzip as Pure Rust alternatives.
#[derive(Debug, Clone)]
pub struct BloscCodec {
    /// Compression level (0-9)
    pub clevel: u8,
    /// Shuffle mode
    pub shuffle: ShuffleMode,
    /// Block size for compression
    pub blocksize: usize,
    /// Compression algorithm
    pub cname: BloscCompressor,
}

/// Blosc shuffle modes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShuffleMode {
    /// No shuffle
    NoShuffle,
    /// Byte shuffle
    ByteShuffle,
    /// Bit shuffle
    BitShuffle,
}

/// Blosc compression algorithms
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BloscCompressor {
    /// LZ4 (default)
    Lz4,
    /// LZ4HC (high compression)
    Lz4hc,
    /// Zstd
    Zstd,
    /// Zlib
    Zlib,
    /// Snappy
    Snappy,
}

impl BloscCodec {
    /// Creates a new Blosc codec
    ///
    /// # Arguments
    /// * `cname` - Compression algorithm name
    /// * `clevel` - Compression level (0-9)
    /// * `shuffle` - Shuffle mode (0=no shuffle, 1=byte shuffle, 2=bit shuffle)
    /// * `blocksize` - Block size (0 for auto)
    ///
    /// # Errors
    /// Returns error if configuration is invalid
    pub fn new(
        cname: impl Into<String>,
        clevel: u8,
        shuffle: u8,
        blocksize: Option<usize>,
    ) -> Result<Self> {
        let cname_str = cname.into();
        let compressor = match cname_str.as_str() {
            "lz4" => BloscCompressor::Lz4,
            "lz4hc" => BloscCompressor::Lz4hc,
            "zstd" => BloscCompressor::Zstd,
            "zlib" => BloscCompressor::Zlib,
            "snappy" => BloscCompressor::Snappy,
            _ => {
                return Err(ZarrError::Codec(CodecError::InvalidConfiguration {
                    codec: "blosc".to_string(),
                    message: format!("Unknown compressor: {cname_str}"),
                }));
            }
        };

        let shuffle_mode = match shuffle {
            0 => ShuffleMode::NoShuffle,
            1 => ShuffleMode::ByteShuffle,
            2 => ShuffleMode::BitShuffle,
            _ => {
                return Err(ZarrError::Codec(CodecError::InvalidConfiguration {
                    codec: "blosc".to_string(),
                    message: format!("Invalid shuffle mode: {shuffle}"),
                }));
            }
        };

        if clevel > 9 {
            return Err(ZarrError::Codec(CodecError::InvalidConfiguration {
                codec: "blosc".to_string(),
                message: format!("Invalid compression level: {clevel} (must be 0-9)"),
            }));
        }

        Ok(Self {
            clevel,
            shuffle: shuffle_mode,
            blocksize: blocksize.unwrap_or(0),
            cname: compressor,
        })
    }
}

impl Default for BloscCodec {
    fn default() -> Self {
        Self {
            clevel: 5,
            shuffle: ShuffleMode::ByteShuffle,
            blocksize: 0,
            cname: BloscCompressor::Lz4,
        }
    }
}

impl Codec for BloscCodec {
    fn id(&self) -> &str {
        "blosc"
    }

    fn encode(&self, _data: &[u8]) -> Result<Vec<u8>> {
        Err(ZarrError::Codec(CodecError::CodecNotAvailable {
            codec: "blosc (requires C dependencies - use zstd, lz4, or gzip instead)".to_string(),
        }))
    }

    fn decode(&self, _data: &[u8]) -> Result<Vec<u8>> {
        Err(ZarrError::Codec(CodecError::CodecNotAvailable {
            codec: "blosc (requires C dependencies - use zstd, lz4, or gzip instead)".to_string(),
        }))
    }

    fn clone_box(&self) -> Box<dyn Codec> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blosc_default() {
        let codec = BloscCodec::default();
        assert_eq!(codec.clevel, 5);
        assert_eq!(codec.shuffle, ShuffleMode::ByteShuffle);
        assert_eq!(codec.id(), "blosc");
    }

    #[test]
    fn test_blosc_encode_not_available() {
        let codec = BloscCodec::default();
        let data = b"test data";

        let result = codec.encode(data);
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(ZarrError::Codec(CodecError::CodecNotAvailable { .. }))
        ));
    }

    #[test]
    fn test_blosc_decode_not_available() {
        let codec = BloscCodec::default();
        let data = b"compressed data";

        let result = codec.decode(data);
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(ZarrError::Codec(CodecError::CodecNotAvailable { .. }))
        ));
    }

    #[test]
    fn test_shuffle_modes() {
        assert_ne!(ShuffleMode::NoShuffle, ShuffleMode::ByteShuffle);
        assert_ne!(ShuffleMode::ByteShuffle, ShuffleMode::BitShuffle);
        assert_ne!(ShuffleMode::NoShuffle, ShuffleMode::BitShuffle);
    }

    #[test]
    fn test_blosc_compressors() {
        assert_ne!(BloscCompressor::Lz4, BloscCompressor::Lz4hc);
        assert_ne!(BloscCompressor::Lz4, BloscCompressor::Zstd);
        assert_ne!(BloscCompressor::Zstd, BloscCompressor::Zlib);
    }
}
