//! Brotli compression codec
//!
//! Brotli is a compression algorithm developed by Google that provides excellent
//! compression ratios, particularly for text and web content. It offers good
//! decompression speeds and multiple quality levels.

use crate::error::{CompressionError, Result};
use oxiarc_brotli::{BrotliCompressor, BrotliDecompressor, BrotliParams};
use std::io::{Read, Write};

/// Brotli compression quality (0-11, higher = better compression but slower)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrotliQuality(u32);

impl BrotliQuality {
    /// Minimum compression quality (fastest)
    pub const MIN: u32 = 0;

    /// Maximum compression quality (best compression)
    pub const MAX: u32 = 11;

    /// Default compression quality (balanced)
    pub const DEFAULT: u32 = 6;

    /// Create a new Brotli compression quality
    pub fn new(quality: u32) -> Result<Self> {
        if quality > Self::MAX {
            return Err(CompressionError::InvalidCompressionLevel {
                level: quality as i32,
                min: Self::MIN as i32,
                max: Self::MAX as i32,
            });
        }
        Ok(Self(quality))
    }

    /// Get the quality value
    pub fn value(&self) -> u32 {
        self.0
    }
}

impl Default for BrotliQuality {
    fn default() -> Self {
        Self(Self::DEFAULT)
    }
}

/// Brotli codec configuration
#[derive(Debug, Clone)]
pub struct BrotliConfig {
    /// Compression quality
    pub quality: BrotliQuality,

    /// Window size (10-24)
    pub window_size: u32,

    /// Block size (16-24)
    pub block_size: u32,
}

impl Default for BrotliConfig {
    fn default() -> Self {
        Self {
            quality: BrotliQuality::default(),
            window_size: 22,
            block_size: 20,
        }
    }
}

impl BrotliConfig {
    /// Create new configuration with specified quality
    pub fn with_quality(quality: u32) -> Result<Self> {
        Ok(Self {
            quality: BrotliQuality::new(quality)?,
            ..Default::default()
        })
    }

    /// Set window size
    pub fn with_window_size(mut self, size: u32) -> Self {
        self.window_size = size;
        self
    }

    /// Set block size
    pub fn with_block_size(mut self, size: u32) -> Self {
        self.block_size = size;
        self
    }
}

/// Brotli compression codec
pub struct BrotliCodec {
    config: BrotliConfig,
}

impl BrotliCodec {
    /// Create a new Brotli codec with default configuration
    pub fn new() -> Self {
        Self {
            config: BrotliConfig::default(),
        }
    }

    /// Create a new Brotli codec with custom configuration
    pub fn with_config(config: BrotliConfig) -> Self {
        Self { config }
    }

    /// Compress data using Brotli
    pub fn compress(&self, input: &[u8]) -> Result<Vec<u8>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        let mut output = Vec::new();
        let params = self.create_encoder_params();

        let mut compressor = BrotliCompressor::new(&mut output, params);

        compressor
            .write_all(input)
            .map_err(|e| CompressionError::BrotliError(e.to_string()))?;

        compressor
            .finish()
            .map_err(|e| CompressionError::BrotliError(e.to_string()))?;

        Ok(output)
    }

    /// Decompress Brotli data
    pub fn decompress(&self, input: &[u8]) -> Result<Vec<u8>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        let mut output = Vec::new();
        let mut decompressor = BrotliDecompressor::new(input);

        decompressor
            .read_to_end(&mut output)
            .map_err(|e| CompressionError::BrotliError(e.to_string()))?;

        Ok(output)
    }

    /// Compress data using Brotli stream
    pub fn compress_stream<R: Read, W: Write>(&self, mut reader: R, writer: W) -> Result<usize> {
        let params = self.create_encoder_params();
        let mut compressor = BrotliCompressor::new(writer, params);

        let bytes_written = std::io::copy(&mut reader, &mut compressor)?;

        compressor
            .finish()
            .map_err(|e| CompressionError::BrotliError(e.to_string()))?;

        Ok(bytes_written as usize)
    }

    /// Decompress Brotli stream
    pub fn decompress_stream<R: Read, W: Write>(&self, reader: R, mut writer: W) -> Result<usize> {
        let mut decompressor = BrotliDecompressor::new(reader);

        let bytes_written = std::io::copy(&mut decompressor, &mut writer)?;

        Ok(bytes_written as usize)
    }

    /// Create encoder parameters
    fn create_encoder_params(&self) -> BrotliParams {
        BrotliParams {
            quality: self.config.quality.value(),
            lgwin: self.config.window_size,
            lgblock: self.config.block_size,
        }
    }

    /// Get compression quality
    pub fn quality(&self) -> u32 {
        self.config.quality.value()
    }
}

impl Default for BrotliCodec {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_brotli_quality_validation() {
        assert!(BrotliQuality::new(0).is_ok());
        assert!(BrotliQuality::new(11).is_ok());
        assert!(BrotliQuality::new(12).is_err());
    }

    #[test]
    fn test_brotli_compress_decompress() {
        let codec = BrotliCodec::new();
        let data = b"Hello, world! This is a test of Brotli compression.".repeat(100);

        let compressed = codec.compress(&data).expect("Compression failed");
        assert!(compressed.len() < data.len());

        let decompressed = codec.decompress(&compressed).expect("Decompression failed");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_brotli_empty_data() {
        let codec = BrotliCodec::new();
        let data: &[u8] = b"";

        let compressed = codec.compress(data).expect("Compression failed");
        assert_eq!(compressed.len(), 0);

        let decompressed = codec.decompress(&compressed).expect("Decompression failed");
        assert_eq!(decompressed.len(), 0);
    }

    #[test]
    fn test_brotli_config() {
        let config = BrotliConfig::with_quality(9)
            .expect("Config creation failed")
            .with_window_size(20)
            .with_block_size(18);

        assert_eq!(config.quality.value(), 9);
        assert_eq!(config.window_size, 20);
        assert_eq!(config.block_size, 18);
    }
}
