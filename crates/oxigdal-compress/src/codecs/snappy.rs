//! Snappy compression codec
//!
//! Snappy is a compression algorithm developed by Google that aims for very high
//! speeds and reasonable compression. It does not aim for maximum compression or
//! compatibility with other compression libraries; instead, it aims for very high
//! speeds and reasonable compression.

use crate::error::{CompressionError, Result};
use snap::{
    raw::{Decoder, Encoder},
    read::FrameDecoder,
    write::FrameEncoder,
};
use std::io::{Read, Write};

/// Snappy codec configuration (Snappy has no compression levels)
#[derive(Debug, Clone, Default)]
pub struct SnappyConfig {
    /// Use framed format (with checksums and headers)
    pub framed: bool,
}

impl SnappyConfig {
    /// Create new configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable framed format
    pub fn with_framed(mut self, framed: bool) -> Self {
        self.framed = framed;
        self
    }
}

/// Snappy compression codec
pub struct SnappyCodec {
    /// Codec configuration (reserved for future use)
    _config: SnappyConfig,
}

impl SnappyCodec {
    /// Create a new Snappy codec with default configuration
    pub fn new() -> Self {
        Self {
            _config: SnappyConfig::default(),
        }
    }

    /// Create a new Snappy codec with custom configuration
    pub fn with_config(config: SnappyConfig) -> Self {
        Self { _config: config }
    }

    /// Compress data using Snappy raw format
    pub fn compress(&self, input: &[u8]) -> Result<Vec<u8>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        let mut encoder = Encoder::new();
        let compressed = encoder.compress_vec(input)?;

        Ok(compressed)
    }

    /// Decompress Snappy raw format data
    pub fn decompress(&self, input: &[u8]) -> Result<Vec<u8>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        let mut decoder = Decoder::new();
        let decompressed = decoder.decompress_vec(input)?;

        Ok(decompressed)
    }

    /// Compress data using Snappy framed format (with checksums)
    pub fn compress_framed<R: Read, W: Write>(&self, mut reader: R, writer: W) -> Result<usize> {
        let mut encoder = FrameEncoder::new(writer);

        let bytes_written = std::io::copy(&mut reader, &mut encoder)?;

        encoder
            .flush()
            .map_err(|e| CompressionError::SnappyError(e.to_string()))?;

        Ok(bytes_written as usize)
    }

    /// Decompress Snappy framed format data
    pub fn decompress_framed<R: Read, W: Write>(&self, reader: R, mut writer: W) -> Result<usize> {
        let mut decoder = FrameDecoder::new(reader);

        let bytes_written = std::io::copy(&mut decoder, &mut writer)?;

        Ok(bytes_written as usize)
    }

    /// Get the maximum compressed size for input of given size
    pub fn max_compressed_size(input_size: usize) -> usize {
        snap::raw::max_compress_len(input_size)
    }

    /// Get the decompressed size from compressed data (raw format only)
    pub fn get_decompressed_size(input: &[u8]) -> Result<usize> {
        if input.is_empty() {
            return Ok(0);
        }

        let size = snap::raw::decompress_len(input)
            .map_err(|e| CompressionError::SnappyError(e.to_string()))?;

        Ok(size)
    }
}

impl Default for SnappyCodec {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snappy_compress_decompress() {
        let codec = SnappyCodec::new();
        let data = b"Hello, world! This is a test of Snappy compression.".repeat(100);

        let compressed = codec.compress(&data).expect("Compression failed");
        assert!(compressed.len() < data.len());

        let decompressed = codec.decompress(&compressed).expect("Decompression failed");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_snappy_empty_data() {
        let codec = SnappyCodec::new();
        let data: &[u8] = b"";

        let compressed = codec.compress(data).expect("Compression failed");
        assert_eq!(compressed.len(), 0);

        let decompressed = codec.decompress(&compressed).expect("Decompression failed");
        assert_eq!(decompressed.len(), 0);
    }

    #[test]
    fn test_snappy_max_compressed_size() {
        let size = SnappyCodec::max_compressed_size(1024);
        assert!(size >= 1024);
    }

    #[test]
    fn test_snappy_get_decompressed_size() {
        let codec = SnappyCodec::new();
        let data = b"Hello, world!".repeat(10);

        let compressed = codec.compress(&data).expect("Compression failed");
        let size = SnappyCodec::get_decompressed_size(&compressed).expect("Failed to get size");

        assert_eq!(size, data.len());
    }
}
