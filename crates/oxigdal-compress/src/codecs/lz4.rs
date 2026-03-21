//! LZ4 compression codec
//!
//! LZ4 is a lossless compression algorithm that provides extremely fast compression
//! and decompression speeds. It is particularly well-suited for scenarios where
//! speed is more important than compression ratio.

use crate::error::{CompressionError, Result};
use std::io::{Read, Write};

/// LZ4 compression level (1-12, higher = better compression but slower)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lz4Level(i32);

impl Lz4Level {
    /// Minimum compression level (fastest)
    pub const MIN: i32 = 1;

    /// Maximum compression level (best compression)
    pub const MAX: i32 = 12;

    /// Default compression level (balanced)
    pub const DEFAULT: i32 = 4;

    /// Create a new LZ4 compression level
    pub fn new(level: i32) -> Result<Self> {
        if !(Self::MIN..=Self::MAX).contains(&level) {
            return Err(CompressionError::InvalidCompressionLevel {
                level,
                min: Self::MIN,
                max: Self::MAX,
            });
        }
        Ok(Self(level))
    }

    /// Get the level value
    pub fn value(&self) -> i32 {
        self.0
    }
}

impl Default for Lz4Level {
    fn default() -> Self {
        Self(Self::DEFAULT)
    }
}

/// LZ4 codec configuration
#[derive(Debug, Clone)]
pub struct Lz4Config {
    /// Compression level
    pub level: Lz4Level,

    /// Block independence (true = each block can be decompressed independently)
    pub block_independent: bool,

    /// Block size in bytes
    pub block_size: usize,

    /// Enable content checksum
    pub checksum: bool,
}

impl Default for Lz4Config {
    fn default() -> Self {
        Self {
            level: Lz4Level::default(),
            block_independent: true,
            block_size: 4 * 1024 * 1024, // 4 MB
            checksum: true,
        }
    }
}

impl Lz4Config {
    /// Create new configuration with specified level
    pub fn with_level(level: i32) -> Result<Self> {
        Ok(Self {
            level: Lz4Level::new(level)?,
            ..Default::default()
        })
    }

    /// Set block independence
    pub fn with_block_independent(mut self, independent: bool) -> Self {
        self.block_independent = independent;
        self
    }

    /// Set block size
    pub fn with_block_size(mut self, size: usize) -> Self {
        self.block_size = size;
        self
    }

    /// Enable/disable checksum
    pub fn with_checksum(mut self, checksum: bool) -> Self {
        self.checksum = checksum;
        self
    }
}

/// LZ4 compression codec
pub struct Lz4Codec {
    config: Lz4Config,
}

impl Lz4Codec {
    /// Create a new LZ4 codec with default configuration
    pub fn new() -> Self {
        Self {
            config: Lz4Config::default(),
        }
    }

    /// Create a new LZ4 codec with custom configuration
    pub fn with_config(config: Lz4Config) -> Self {
        Self { config }
    }

    /// Compress data using LZ4
    pub fn compress(&self, input: &[u8]) -> Result<Vec<u8>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        // Use LZ4 block compression (oxiarc-lz4 Pure Rust)
        let compressed = oxiarc_lz4::compress_block_hc(input, self.config.level.value())
            .map_err(|e| CompressionError::Lz4Error(e.to_string()))?;

        Ok(compressed)
    }

    /// Decompress LZ4 data
    pub fn decompress(&self, input: &[u8], decompressed_size: Option<usize>) -> Result<Vec<u8>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        let size = decompressed_size.unwrap_or(input.len() * 4);
        let decompressed = oxiarc_lz4::decompress_block(input, size)
            .map_err(|e| CompressionError::Lz4Error(e.to_string()))?;

        Ok(decompressed)
    }

    /// Compress data using LZ4 frame format (with headers)
    pub fn compress_frame<R: Read, W: Write>(&self, mut reader: R, mut writer: W) -> Result<usize> {
        let mut input = Vec::new();
        reader
            .read_to_end(&mut input)
            .map_err(CompressionError::Io)?;

        let desc = oxiarc_lz4::FrameDescriptor::new()
            .with_content_size(input.len() as u64)
            .with_content_checksum(self.config.checksum);

        let compressed = oxiarc_lz4::compress_with_options(&input, desc)
            .map_err(|e| CompressionError::Lz4Error(e.to_string()))?;

        writer
            .write_all(&compressed)
            .map_err(CompressionError::Io)?;
        Ok(input.len())
    }

    /// Decompress LZ4 frame format
    pub fn decompress_frame<R: Read, W: Write>(
        &self,
        mut reader: R,
        mut writer: W,
    ) -> Result<usize> {
        let mut input = Vec::new();
        reader
            .read_to_end(&mut input)
            .map_err(CompressionError::Io)?;

        // Use a generous max output size for frame decompression
        let max_output = input.len() * 10;
        let decompressed = oxiarc_lz4::decompress(&input, max_output)
            .map_err(|e| CompressionError::Lz4Error(e.to_string()))?;

        let len = decompressed.len();
        writer
            .write_all(&decompressed)
            .map_err(CompressionError::Io)?;
        Ok(len)
    }

    /// Get the maximum compressed size for input of given size
    pub fn max_compressed_size(input_size: usize) -> usize {
        // LZ4 worst-case bound: input_size + (input_size / 255) + 16
        input_size + (input_size / 255) + 16
    }
}

impl Default for Lz4Codec {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lz4_level_validation() {
        assert!(Lz4Level::new(0).is_err());
        assert!(Lz4Level::new(1).is_ok());
        assert!(Lz4Level::new(12).is_ok());
        assert!(Lz4Level::new(13).is_err());
    }

    #[test]
    fn test_lz4_compress_decompress() {
        let codec = Lz4Codec::new();
        let data = b"Hello, world! This is a test of LZ4 compression.".repeat(100);

        let compressed = codec.compress(&data).expect("Compression failed");
        assert!(compressed.len() < data.len());

        let decompressed = codec
            .decompress(&compressed, Some(data.len()))
            .expect("Decompression failed");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_lz4_empty_data() {
        let codec = Lz4Codec::new();
        let data: &[u8] = b"";

        let compressed = codec.compress(data).expect("Compression failed");
        assert_eq!(compressed.len(), 0);

        let decompressed = codec
            .decompress(&compressed, Some(0))
            .expect("Decompression failed");
        assert_eq!(decompressed.len(), 0);
    }

    #[test]
    fn test_lz4_config() {
        let config = Lz4Config::with_level(9)
            .expect("Config creation failed")
            .with_checksum(true)
            .with_block_size(8 * 1024 * 1024);

        assert_eq!(config.level.value(), 9);
        assert!(config.checksum);
        assert_eq!(config.block_size, 8 * 1024 * 1024);
    }
}
