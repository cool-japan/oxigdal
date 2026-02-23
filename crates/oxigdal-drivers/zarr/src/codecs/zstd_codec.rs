//! Zstd compression codec for Zarr
//!
//! This module provides Zstd (Zstandard) compression and decompression.

use super::Codec;
use crate::error::{CodecError, Result, ZarrError};

/// Zstd codec
#[derive(Debug, Clone)]
pub struct ZstdCodec {
    level: i32,
}

impl ZstdCodec {
    /// Creates a new Zstd codec with the specified compression level
    ///
    /// # Arguments
    /// * `level` - Compression level (1-22, where 1 is fastest and 22 is maximum compression)
    ///             Default is 3 for a good balance of speed and compression.
    ///
    /// # Errors
    /// Returns error if the level is invalid
    pub fn new(level: i32) -> Result<Self> {
        if !(1..=22).contains(&level) {
            return Err(ZarrError::Codec(CodecError::InvalidConfiguration {
                codec: "zstd".to_string(),
                message: format!("Invalid compression level: {level} (must be 1-22)"),
            }));
        }

        Ok(Self { level })
    }

    /// Creates a new Zstd codec with default compression level (3)
    #[must_use]
    pub fn default_level() -> Self {
        Self { level: 3 }
    }

    /// Creates a new Zstd codec with fast compression (level 1)
    #[must_use]
    pub fn fast() -> Self {
        Self { level: 1 }
    }

    /// Creates a new Zstd codec with best compression (level 22)
    #[must_use]
    pub fn best() -> Self {
        Self { level: 22 }
    }

    /// Returns the compression level
    #[must_use]
    pub const fn level(&self) -> i32 {
        self.level
    }
}

impl Default for ZstdCodec {
    fn default() -> Self {
        Self::default_level()
    }
}

impl Codec for ZstdCodec {
    fn id(&self) -> &str {
        "zstd"
    }

    fn encode(&self, data: &[u8]) -> Result<Vec<u8>> {
        zstd::encode_all(data, self.level).map_err(|e| {
            ZarrError::Codec(CodecError::CompressionFailed {
                message: format!("Zstd compression failed: {e}"),
            })
        })
    }

    fn decode(&self, data: &[u8]) -> Result<Vec<u8>> {
        zstd::decode_all(data).map_err(|e| {
            ZarrError::Codec(CodecError::DecompressionFailed {
                message: format!("Zstd decompression failed: {e}"),
            })
        })
    }

    fn max_encoded_size(&self, input_size: usize) -> usize {
        // Zstd max compressed size
        zstd::zstd_safe::compress_bound(input_size)
    }

    fn clone_box(&self) -> Box<dyn Codec> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zstd_codec_new() {
        let codec = ZstdCodec::new(3).expect("valid level");
        assert_eq!(codec.level(), 3);
        assert_eq!(codec.id(), "zstd");

        assert!(ZstdCodec::new(0).is_err());
        assert!(ZstdCodec::new(23).is_err());
    }

    #[test]
    fn test_zstd_codec_levels() {
        let fast = ZstdCodec::fast();
        assert_eq!(fast.level(), 1);

        let best = ZstdCodec::best();
        assert_eq!(best.level(), 22);

        let default = ZstdCodec::default();
        assert_eq!(default.level(), 3);
    }

    #[test]
    fn test_zstd_roundtrip() {
        let codec = ZstdCodec::new(3).expect("valid level");
        let data = b"Hello, Zarr! This is a test of Zstd compression. ".repeat(100);

        let compressed = codec.encode(&data).expect("compress");
        assert!(compressed.len() < data.len()); // Should be smaller

        let decompressed = codec.decode(&compressed).expect("decompress");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_zstd_empty_data() {
        let codec = ZstdCodec::new(3).expect("valid level");
        let data = b"";

        let compressed = codec.encode(data).expect("compress");
        let decompressed = codec.decode(&compressed).expect("decompress");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_zstd_single_byte() {
        let codec = ZstdCodec::new(3).expect("valid level");
        let data = b"x";

        let compressed = codec.encode(data).expect("compress");
        let decompressed = codec.decode(&compressed).expect("decompress");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_zstd_incompressible_data() {
        let codec = ZstdCodec::new(22).expect("valid level");
        // Random-ish data that won't compress well
        let data: Vec<u8> = (0..1000).map(|i| ((i * 31) % 256) as u8).collect();

        let compressed = codec.encode(&data).expect("compress");
        let decompressed = codec.decode(&compressed).expect("decompress");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_zstd_large_data() {
        let codec = ZstdCodec::new(3).expect("valid level");
        let data = vec![42u8; 1_000_000]; // 1 MB of the same byte

        let compressed = codec.encode(&data).expect("compress");
        assert!(compressed.len() < data.len() / 100); // Should compress very well

        let decompressed = codec.decode(&compressed).expect("decompress");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_zstd_compression_levels() {
        let data = b"The quick brown fox jumps over the lazy dog. ".repeat(100);

        let fast = ZstdCodec::fast();
        let default_codec = ZstdCodec::default();
        let best = ZstdCodec::best();

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
    fn test_zstd_max_encoded_size() {
        let codec = ZstdCodec::new(3).expect("valid level");

        let size_1kb = codec.max_encoded_size(1024);
        assert!(size_1kb > 1024);

        let size_1mb = codec.max_encoded_size(1024 * 1024);
        assert!(size_1mb > 1024 * 1024);
    }

    #[test]
    fn test_zstd_binary_data() {
        let codec = ZstdCodec::new(10).expect("valid level");
        let data: Vec<u8> = (0..=255).cycle().take(10000).collect();

        let compressed = codec.encode(&data).expect("compress");
        let decompressed = codec.decode(&compressed).expect("decompress");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_zstd_text_data() {
        let codec = ZstdCodec::new(5).expect("valid level");
        let text = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(50);
        let data = text.as_bytes();

        let compressed = codec.encode(data).expect("compress");
        assert!(compressed.len() < data.len() / 2); // Text compresses well

        let decompressed = codec.decode(&compressed).expect("decompress");
        assert_eq!(decompressed, data);
    }
}
