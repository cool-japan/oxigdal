//! Gzip compression codec for Zarr
//!
//! This module provides gzip compression and decompression using the flate2 crate.

use super::Codec;
use crate::error::{CodecError, Result, ZarrError};
use flate2::Compression;
use flate2::read::{GzDecoder, GzEncoder};
use std::io::Read;

/// Gzip codec
#[derive(Debug, Clone)]
pub struct GzipCodec {
    level: Compression,
}

impl GzipCodec {
    /// Creates a new Gzip codec with the specified compression level
    ///
    /// # Arguments
    /// * `level` - Compression level (0-9, where 0 is no compression and 9 is maximum)
    ///
    /// # Errors
    /// Returns error if the level is invalid
    pub fn new(level: u32) -> Result<Self> {
        if level > 9 {
            return Err(ZarrError::Codec(CodecError::InvalidConfiguration {
                codec: "gzip".to_string(),
                message: format!("Invalid compression level: {level} (must be 0-9)"),
            }));
        }

        Ok(Self {
            level: Compression::new(level),
        })
    }

    /// Creates a new Gzip codec with default compression level (6)
    #[must_use]
    pub fn default_level() -> Self {
        Self {
            level: Compression::default(),
        }
    }

    /// Creates a new Gzip codec with fast compression (level 1)
    #[must_use]
    pub fn fast() -> Self {
        Self {
            level: Compression::fast(),
        }
    }

    /// Creates a new Gzip codec with best compression (level 9)
    #[must_use]
    pub fn best() -> Self {
        Self {
            level: Compression::best(),
        }
    }

    /// Returns the compression level
    #[must_use]
    pub fn level(&self) -> u32 {
        self.level.level()
    }
}

impl Default for GzipCodec {
    fn default() -> Self {
        Self::default_level()
    }
}

impl Codec for GzipCodec {
    fn id(&self) -> &str {
        "gzip"
    }

    fn encode(&self, data: &[u8]) -> Result<Vec<u8>> {
        let mut encoder = GzEncoder::new(data, self.level);
        let mut compressed = Vec::new();

        encoder.read_to_end(&mut compressed).map_err(|e| {
            ZarrError::Codec(CodecError::CompressionFailed {
                message: format!("Gzip compression failed: {e}"),
            })
        })?;

        Ok(compressed)
    }

    fn decode(&self, data: &[u8]) -> Result<Vec<u8>> {
        let mut decoder = GzDecoder::new(data);
        let mut decompressed = Vec::new();

        decoder.read_to_end(&mut decompressed).map_err(|e| {
            ZarrError::Codec(CodecError::DecompressionFailed {
                message: format!("Gzip decompression failed: {e}"),
            })
        })?;

        Ok(decompressed)
    }

    fn max_encoded_size(&self, input_size: usize) -> usize {
        // Gzip worst case: input + 0.015% + 18 bytes (header) + 8 bytes (footer)
        input_size + (input_size / 6553) + 26
    }

    fn clone_box(&self) -> Box<dyn Codec> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gzip_codec_new() {
        let codec = GzipCodec::new(6).expect("valid level");
        assert_eq!(codec.level(), 6);
        assert_eq!(codec.id(), "gzip");

        assert!(GzipCodec::new(10).is_err());
    }

    #[test]
    fn test_gzip_codec_levels() {
        let fast = GzipCodec::fast();
        assert_eq!(fast.level(), 1);

        let best = GzipCodec::best();
        assert_eq!(best.level(), 9);

        let default = GzipCodec::default();
        assert_eq!(default.level(), 6);
    }

    #[test]
    fn test_gzip_roundtrip() {
        let codec = GzipCodec::new(6).expect("valid level");
        let data = b"Hello, Zarr! This is a test of gzip compression. ".repeat(100);

        let compressed = codec.encode(&data).expect("compress");
        assert!(compressed.len() < data.len()); // Should be smaller

        let decompressed = codec.decode(&compressed).expect("decompress");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_gzip_empty_data() {
        let codec = GzipCodec::new(6).expect("valid level");
        let data = b"";

        let compressed = codec.encode(data).expect("compress");
        let decompressed = codec.decode(&compressed).expect("decompress");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_gzip_single_byte() {
        let codec = GzipCodec::new(6).expect("valid level");
        let data = b"x";

        let compressed = codec.encode(data).expect("compress");
        let decompressed = codec.decode(&compressed).expect("decompress");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_gzip_incompressible_data() {
        let codec = GzipCodec::new(9).expect("valid level");
        // Random-ish data that won't compress well
        let data: Vec<u8> = (0..1000).map(|i| ((i * 31) % 256) as u8).collect();

        let compressed = codec.encode(&data).expect("compress");
        let decompressed = codec.decode(&compressed).expect("decompress");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_gzip_large_data() {
        let codec = GzipCodec::new(6).expect("valid level");
        let data = vec![42u8; 1_000_000]; // 1 MB of the same byte

        let compressed = codec.encode(&data).expect("compress");
        assert!(compressed.len() < data.len() / 100); // Should compress very well

        let decompressed = codec.decode(&compressed).expect("decompress");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_gzip_compression_levels() {
        let data = b"The quick brown fox jumps over the lazy dog. ".repeat(100);

        let fast = GzipCodec::fast();
        let default_codec = GzipCodec::default();
        let best = GzipCodec::best();

        let compressed_fast = fast.encode(&data).expect("compress");
        let compressed_default = default_codec.encode(&data).expect("compress");
        let compressed_best = best.encode(&data).expect("compress");

        // Best should produce smallest output
        assert!(compressed_best.len() <= compressed_default.len());
        assert!(compressed_default.len() <= compressed_fast.len());

        // All should decompress correctly
        assert_eq!(
            fast.decode(&compressed_fast).expect("decompress"),
            &data[..]
        );
        assert_eq!(
            default_codec
                .decode(&compressed_default)
                .expect("decompress"),
            &data[..]
        );
        assert_eq!(
            best.decode(&compressed_best).expect("decompress"),
            &data[..]
        );
    }

    #[test]
    fn test_gzip_max_encoded_size() {
        let codec = GzipCodec::new(6).expect("valid level");

        let size_1kb = codec.max_encoded_size(1024);
        assert!(size_1kb > 1024);

        let size_1mb = codec.max_encoded_size(1024 * 1024);
        assert!(size_1mb > 1024 * 1024);
    }
}
