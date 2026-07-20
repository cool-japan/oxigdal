//! Run-Length Encoding (RLE) codec
//!
//! RLE is effective for data with long runs of repeated values, such as
//! categorical raster data with large homogeneous regions.

use crate::error::{CompressionError, Result};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::Cursor;

/// RLE codec configuration
#[derive(Debug, Clone)]
pub struct RleConfig {
    /// Maximum run length before splitting
    pub max_run_length: usize,
}

impl Default for RleConfig {
    fn default() -> Self {
        Self {
            max_run_length: 65535, // u16::MAX
        }
    }
}

/// RLE compression codec
pub struct RleCodec {
    config: RleConfig,
}

impl RleCodec {
    /// Create a new RLE codec with default configuration
    pub fn new() -> Self {
        Self {
            config: RleConfig::default(),
        }
    }

    /// Create a new RLE codec with custom configuration.
    ///
    /// # Errors
    ///
    /// Returns [`CompressionError::ConfigurationError`] if
    /// `config.max_run_length` is `0` or exceeds `u16::MAX`, since the
    /// on-disk run-length field is a little-endian `u16` and any larger
    /// value would silently truncate (`run_length as u16`), corrupting
    /// compressed output.
    pub fn with_config(config: RleConfig) -> Result<Self> {
        if config.max_run_length == 0 || config.max_run_length > u16::MAX as usize {
            return Err(CompressionError::ConfigurationError(format!(
                "RleConfig::max_run_length must be in 1..={}, got {}",
                u16::MAX,
                config.max_run_length
            )));
        }
        Ok(Self { config })
    }

    /// Compress data using run-length encoding
    pub fn compress(&self, input: &[u8]) -> Result<Vec<u8>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        let mut output = Vec::new();
        let mut i = 0;

        while i < input.len() {
            let value = input[i];
            let mut run_length = 1;

            // Count consecutive equal values
            while i + run_length < input.len()
                && input[i + run_length] == value
                && run_length < self.config.max_run_length
            {
                run_length += 1;
            }

            // Write run: length (u16) + value (u8)
            output.write_u16::<LittleEndian>(run_length as u16)?;
            output.push(value);

            i += run_length;
        }

        Ok(output)
    }

    /// Decompress RLE data
    pub fn decompress(&self, input: &[u8]) -> Result<Vec<u8>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        let mut output = Vec::new();
        let mut cursor = Cursor::new(input);

        while cursor.position() < input.len() as u64 {
            let run_length = cursor.read_u16::<LittleEndian>()? as usize;
            let value = cursor.read_u8()?;

            output.extend(std::iter::repeat_n(value, run_length));
        }

        Ok(output)
    }

    /// Compress data with byte-level RLE (more efficient for small runs)
    pub fn compress_byte(&self, input: &[u8]) -> Result<Vec<u8>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        let mut output = Vec::new();
        let mut i = 0;

        while i < input.len() {
            let value = input[i];
            let mut run_length = 1;

            // Count consecutive equal values (max 255 for byte encoding)
            while i + run_length < input.len() && input[i + run_length] == value && run_length < 255
            {
                run_length += 1;
            }

            // Write run: length (u8) + value (u8)
            output.push(run_length as u8);
            output.push(value);

            i += run_length;
        }

        Ok(output)
    }

    /// Decompress byte-level RLE data
    pub fn decompress_byte(&self, input: &[u8]) -> Result<Vec<u8>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        if !input.len().is_multiple_of(2) {
            return Err(CompressionError::RleError(
                "Invalid RLE data: odd length".to_string(),
            ));
        }

        let mut output = Vec::new();

        for chunk in input.chunks_exact(2) {
            let run_length = chunk[0] as usize;
            let value = chunk[1];

            output.extend(std::iter::repeat_n(value, run_length));
        }

        Ok(output)
    }

    /// Estimate compression ratio for the given data
    pub fn estimate_ratio(input: &[u8]) -> f64 {
        if input.is_empty() {
            return 1.0;
        }

        let mut runs = 0;
        let mut i = 0;

        while i < input.len() {
            let value = input[i];
            let mut run_length = 1;

            while i + run_length < input.len() && input[i + run_length] == value {
                run_length += 1;
            }

            runs += 1;
            i += run_length;
        }

        // Each run takes 3 bytes (u16 length + u8 value)
        let compressed_size = runs * 3;
        input.len() as f64 / compressed_size as f64
    }
}

impl Default for RleCodec {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rle_compress_decompress() {
        let codec = RleCodec::new();
        let data = vec![1u8; 100];

        let compressed = codec.compress(&data).expect("Compression failed");
        assert!(compressed.len() < data.len());

        let decompressed = codec.decompress(&compressed).expect("Decompression failed");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_rle_byte_compress_decompress() {
        let codec = RleCodec::new();
        let data = vec![5u8; 50];

        let compressed = codec.compress_byte(&data).expect("Compression failed");
        assert!(compressed.len() < data.len());

        let decompressed = codec
            .decompress_byte(&compressed)
            .expect("Decompression failed");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_rle_mixed_data() {
        let codec = RleCodec::new();
        let mut data = Vec::new();
        data.extend(vec![1u8; 50]);
        data.extend(vec![2u8; 30]);
        data.extend(vec![3u8; 20]);

        let compressed = codec.compress(&data).expect("Compression failed");
        let decompressed = codec.decompress(&compressed).expect("Decompression failed");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_rle_empty_data() {
        let codec = RleCodec::new();
        let data: &[u8] = b"";

        let compressed = codec.compress(data).expect("Compression failed");
        assert_eq!(compressed.len(), 0);

        let decompressed = codec.decompress(&compressed).expect("Decompression failed");
        assert_eq!(decompressed.len(), 0);
    }

    #[test]
    fn test_rle_estimate_ratio() {
        let data = vec![1u8; 1000];
        let ratio = RleCodec::estimate_ratio(&data);
        assert!(ratio > 100.0); // Should compress very well
    }

    #[test]
    fn test_rle_with_config_rejects_max_run_length_above_u16_max() {
        let config = RleConfig {
            max_run_length: 100_000, // > u16::MAX, would silently truncate
        };
        let result = RleCodec::with_config(config);
        assert!(result.is_err());
    }

    #[test]
    fn test_rle_with_config_rejects_zero_max_run_length() {
        let config = RleConfig { max_run_length: 0 };
        let result = RleCodec::with_config(config);
        assert!(result.is_err());
    }

    #[test]
    fn test_rle_with_config_accepts_u16_max() {
        let config = RleConfig {
            max_run_length: u16::MAX as usize,
        };
        let codec = RleCodec::with_config(config).expect("valid config should be accepted");
        let data = vec![7u8; u16::MAX as usize];
        let compressed = codec.compress(&data).expect("compression failed");
        let decompressed = codec.decompress(&compressed).expect("decompression failed");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_rle_long_run_does_not_truncate_run_length() {
        // Regression test: with an unvalidated max_run_length previously it was
        // possible to construct a codec where a run of 100_000 identical bytes
        // would truncate to 100_000 % 65536 = 34464 on the wire. With a valid
        // (clamped) config, a run longer than max_run_length must be split into
        // multiple runs rather than truncated, and must round-trip exactly.
        let codec = RleCodec::new(); // default max_run_length = 65535
        let data = vec![9u8; 100_000];

        let compressed = codec.compress(&data).expect("compression failed");
        let decompressed = codec.decompress(&compressed).expect("decompression failed");
        assert_eq!(decompressed, data);
        assert_eq!(decompressed.len(), 100_000);
    }
}
