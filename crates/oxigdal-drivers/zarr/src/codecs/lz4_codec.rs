//! LZ4 compression codec for Zarr
//!
//! This module provides LZ4 compression and decompression.

use super::Codec;
use crate::error::{CodecError, Result, ZarrError};

/// LZ4 codec
#[derive(Debug, Clone)]
pub struct Lz4Codec {
    acceleration: i32,
}

impl Lz4Codec {
    /// Creates a new LZ4 codec with the specified acceleration factor
    ///
    /// # Arguments
    /// * `acceleration` - Acceleration factor (>= 1, higher = faster but less compression)
    ///                    None uses the default (1).
    ///
    /// # Errors
    /// Returns error if the acceleration factor is invalid
    pub fn new(acceleration: Option<i32>) -> Result<Self> {
        let accel = acceleration.unwrap_or(1);

        if accel < 1 {
            return Err(ZarrError::Codec(CodecError::InvalidConfiguration {
                codec: "lz4".to_string(),
                message: format!("Invalid acceleration factor: {accel} (must be >= 1)"),
            }));
        }

        Ok(Self {
            acceleration: accel,
        })
    }

    /// Creates a new LZ4 codec with default settings
    #[must_use]
    pub fn default_acceleration() -> Self {
        Self { acceleration: 1 }
    }

    /// Creates a new LZ4 codec with fast compression
    #[must_use]
    pub fn fast() -> Self {
        Self { acceleration: 10 }
    }

    /// Returns the acceleration factor
    #[must_use]
    pub const fn acceleration(&self) -> i32 {
        self.acceleration
    }
}

impl Default for Lz4Codec {
    fn default() -> Self {
        Self::default_acceleration()
    }
}

impl Codec for Lz4Codec {
    fn id(&self) -> &str {
        "lz4"
    }

    fn encode(&self, data: &[u8]) -> Result<Vec<u8>> {
        // Use prepend_size=true to include the uncompressed size in the output
        // This allows decompression without needing to know the size in advance
        lz4::block::compress(
            data,
            Some(lz4::block::CompressionMode::FAST(self.acceleration)),
            true,
        )
        .map_err(|e| {
            ZarrError::Codec(CodecError::CompressionFailed {
                message: format!("LZ4 compression failed: {e}"),
            })
        })
    }

    fn decode(&self, data: &[u8]) -> Result<Vec<u8>> {
        // The compressed data includes the uncompressed size prefix (from prepend_size=true)
        lz4::block::decompress(data, None).map_err(|e| {
            ZarrError::Codec(CodecError::DecompressionFailed {
                message: format!("LZ4 decompression failed: {e}"),
            })
        })
    }

    fn max_encoded_size(&self, input_size: usize) -> usize {
        // LZ4 max compressed size
        lz4::block::compress_bound(input_size).unwrap_or(input_size + (input_size / 10) + 1024)
    }

    fn clone_box(&self) -> Box<dyn Codec> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lz4_codec_new() {
        let codec = Lz4Codec::new(Some(1)).expect("valid acceleration");
        assert_eq!(codec.acceleration(), 1);
        assert_eq!(codec.id(), "lz4");

        let codec2 = Lz4Codec::new(None).expect("default");
        assert_eq!(codec2.acceleration(), 1);

        assert!(Lz4Codec::new(Some(0)).is_err());
        assert!(Lz4Codec::new(Some(-1)).is_err());
    }

    #[test]
    fn test_lz4_codec_levels() {
        let default = Lz4Codec::default();
        assert_eq!(default.acceleration(), 1);

        let fast = Lz4Codec::fast();
        assert_eq!(fast.acceleration(), 10);
    }

    #[test]
    fn test_lz4_roundtrip() {
        let codec = Lz4Codec::new(Some(1)).expect("valid acceleration");
        let data = b"Hello, Zarr! This is a test of LZ4 compression. ".repeat(100);

        let compressed = codec.encode(&data).expect("compress");
        assert!(compressed.len() < data.len()); // Should be smaller

        let decompressed = codec.decode(&compressed).expect("decompress");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_lz4_empty_data() {
        let codec = Lz4Codec::new(Some(1)).expect("valid acceleration");
        let data = b"";

        let compressed = codec.encode(data).expect("compress");
        let decompressed = codec.decode(&compressed).expect("decompress");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_lz4_single_byte() {
        let codec = Lz4Codec::new(Some(1)).expect("valid acceleration");
        let data = b"x";

        let compressed = codec.encode(data).expect("compress");
        let decompressed = codec.decode(&compressed).expect("decompress");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_lz4_incompressible_data() {
        let codec = Lz4Codec::new(Some(1)).expect("valid acceleration");
        // Random-ish data that won't compress well
        let data: Vec<u8> = (0..1000).map(|i| ((i * 31) % 256) as u8).collect();

        let compressed = codec.encode(&data).expect("compress");
        let decompressed = codec.decode(&compressed).expect("decompress");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_lz4_large_data() {
        let codec = Lz4Codec::new(Some(1)).expect("valid acceleration");
        let data = vec![42u8; 1_000_000]; // 1 MB of the same byte

        let compressed = codec.encode(&data).expect("compress");
        assert!(compressed.len() < data.len() / 100); // Should compress very well

        let decompressed = codec.decode(&compressed).expect("decompress");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_lz4_acceleration_levels() {
        let data = b"The quick brown fox jumps over the lazy dog. ".repeat(100);

        let slow = Lz4Codec::new(Some(1)).expect("valid");
        let fast = Lz4Codec::fast();

        let compressed_slow = slow.encode(&data).expect("compress");
        let compressed_fast = fast.encode(&data).expect("compress");

        // Slower compression should produce smaller or equal output
        assert!(compressed_slow.len() <= compressed_fast.len() + 100); // Allow some variance

        // Both should decompress correctly
        assert_eq!(
            slow.decode(&compressed_slow).expect("decompress"),
            &data[..]
        );
        assert_eq!(
            fast.decode(&compressed_fast).expect("decompress"),
            &data[..]
        );
    }

    #[test]
    fn test_lz4_max_encoded_size() {
        let codec = Lz4Codec::new(Some(1)).expect("valid acceleration");

        let size_1kb = codec.max_encoded_size(1024);
        assert!(size_1kb > 1024);

        let size_1mb = codec.max_encoded_size(1024 * 1024);
        assert!(size_1mb > 1024 * 1024);
    }

    #[test]
    fn test_lz4_binary_data() {
        let codec = Lz4Codec::new(Some(1)).expect("valid acceleration");
        let data: Vec<u8> = (0..=255).cycle().take(10000).collect();

        let compressed = codec.encode(&data).expect("compress");
        let decompressed = codec.decode(&compressed).expect("decompress");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_lz4_text_data() {
        let codec = Lz4Codec::new(Some(1)).expect("valid acceleration");
        let text = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(50);
        let data = text.as_bytes();

        let compressed = codec.encode(data).expect("compress");
        assert!(compressed.len() < data.len() / 2); // Text compresses well

        let decompressed = codec.decode(&compressed).expect("decompress");
        assert_eq!(decompressed, data);
    }
}
