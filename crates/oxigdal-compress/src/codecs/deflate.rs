//! DEFLATE compression codec (gzip/zlib)
//!
//! DEFLATE is a lossless compression algorithm that uses a combination of LZ77
//! and Huffman coding. It is widely supported and used in formats like gzip and zlib.

use crate::error::{CompressionError, Result};
use flate2::{
    Compression,
    read::{GzDecoder, ZlibDecoder},
    write::{GzEncoder, ZlibEncoder},
};
use std::io::{Read, Write};

/// DEFLATE compression level (0-9, higher = better compression but slower)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeflateLevel(u32);

impl DeflateLevel {
    /// No compression
    pub const NONE: u32 = 0;

    /// Fast compression
    pub const FAST: u32 = 1;

    /// Default compression
    pub const DEFAULT: u32 = 6;

    /// Best compression
    pub const BEST: u32 = 9;

    /// Create a new DEFLATE compression level
    pub fn new(level: u32) -> Result<Self> {
        if level > 9 {
            return Err(CompressionError::InvalidCompressionLevel {
                level: level as i32,
                min: 0,
                max: 9,
            });
        }
        Ok(Self(level))
    }

    /// Get the level value
    pub fn value(&self) -> u32 {
        self.0
    }

    /// Convert to flate2 Compression
    fn to_compression(self) -> Compression {
        Compression::new(self.0)
    }
}

impl Default for DeflateLevel {
    fn default() -> Self {
        Self(Self::DEFAULT)
    }
}

/// DEFLATE format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeflateFormat {
    /// Raw DEFLATE (no wrapper)
    Raw,
    /// Zlib format (with zlib header and checksum)
    Zlib,
    /// Gzip format (with gzip header and checksum)
    Gzip,
}

/// DEFLATE codec configuration
#[derive(Debug, Clone)]
pub struct DeflateConfig {
    /// Compression level
    pub level: DeflateLevel,

    /// Format
    pub format: DeflateFormat,
}

impl Default for DeflateConfig {
    fn default() -> Self {
        Self {
            level: DeflateLevel::default(),
            format: DeflateFormat::Zlib,
        }
    }
}

impl DeflateConfig {
    /// Create new configuration with specified level
    pub fn with_level(level: u32) -> Result<Self> {
        Ok(Self {
            level: DeflateLevel::new(level)?,
            ..Default::default()
        })
    }

    /// Set format
    pub fn with_format(mut self, format: DeflateFormat) -> Self {
        self.format = format;
        self
    }
}

/// DEFLATE compression codec
pub struct DeflateCodec {
    config: DeflateConfig,
}

impl DeflateCodec {
    /// Create a new DEFLATE codec with default configuration
    pub fn new() -> Self {
        Self {
            config: DeflateConfig::default(),
        }
    }

    /// Create a new DEFLATE codec with custom configuration
    pub fn with_config(config: DeflateConfig) -> Self {
        Self { config }
    }

    /// Compress data using DEFLATE
    pub fn compress(&self, input: &[u8]) -> Result<Vec<u8>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        let mut output = Vec::new();

        match self.config.format {
            DeflateFormat::Gzip => {
                let mut encoder = GzEncoder::new(&mut output, self.config.level.to_compression());
                encoder.write_all(input)?;
                encoder.finish()?;
            }
            DeflateFormat::Zlib | DeflateFormat::Raw => {
                let mut encoder = ZlibEncoder::new(&mut output, self.config.level.to_compression());
                encoder.write_all(input)?;
                encoder.finish()?;
            }
        }

        Ok(output)
    }

    /// Decompress DEFLATE data
    pub fn decompress(&self, input: &[u8]) -> Result<Vec<u8>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        let mut output = Vec::new();

        match self.config.format {
            DeflateFormat::Gzip => {
                let mut decoder = GzDecoder::new(input);
                decoder.read_to_end(&mut output)?;
            }
            DeflateFormat::Zlib | DeflateFormat::Raw => {
                let mut decoder = ZlibDecoder::new(input);
                decoder.read_to_end(&mut output)?;
            }
        }

        Ok(output)
    }

    /// Compress data using DEFLATE stream
    pub fn compress_stream<R: Read, W: Write>(&self, mut reader: R, writer: W) -> Result<usize> {
        let bytes_written = match self.config.format {
            DeflateFormat::Gzip => {
                let mut encoder = GzEncoder::new(writer, self.config.level.to_compression());
                let n = std::io::copy(&mut reader, &mut encoder)?;
                encoder.finish()?;
                n
            }
            DeflateFormat::Zlib | DeflateFormat::Raw => {
                let mut encoder = ZlibEncoder::new(writer, self.config.level.to_compression());
                let n = std::io::copy(&mut reader, &mut encoder)?;
                encoder.finish()?;
                n
            }
        };

        Ok(bytes_written as usize)
    }

    /// Decompress DEFLATE stream
    pub fn decompress_stream<R: Read, W: Write>(&self, reader: R, mut writer: W) -> Result<usize> {
        let bytes_written = match self.config.format {
            DeflateFormat::Gzip => {
                let mut decoder = GzDecoder::new(reader);
                std::io::copy(&mut decoder, &mut writer)?
            }
            DeflateFormat::Zlib | DeflateFormat::Raw => {
                let mut decoder = ZlibDecoder::new(reader);
                std::io::copy(&mut decoder, &mut writer)?
            }
        };

        Ok(bytes_written as usize)
    }

    /// Get compression level
    pub fn level(&self) -> u32 {
        self.config.level.value()
    }
}

impl Default for DeflateCodec {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deflate_level_validation() {
        assert!(DeflateLevel::new(0).is_ok());
        assert!(DeflateLevel::new(9).is_ok());
        assert!(DeflateLevel::new(10).is_err());
    }

    #[test]
    fn test_deflate_compress_decompress() {
        let codec = DeflateCodec::new();
        let data = b"Hello, world! This is a test of DEFLATE compression.".repeat(100);

        let compressed = codec.compress(&data).expect("Compression failed");
        assert!(compressed.len() < data.len());

        let decompressed = codec.decompress(&compressed).expect("Decompression failed");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_deflate_gzip_format() {
        let config = DeflateConfig::with_level(6)
            .expect("Config creation failed")
            .with_format(DeflateFormat::Gzip);
        let codec = DeflateCodec::with_config(config);
        let data = b"Test data for gzip format";

        let compressed = codec.compress(data).expect("Compression failed");
        let decompressed = codec.decompress(&compressed).expect("Decompression failed");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_deflate_empty_data() {
        let codec = DeflateCodec::new();
        let data: &[u8] = b"";

        let compressed = codec.compress(data).expect("Compression failed");
        assert_eq!(compressed.len(), 0);

        let decompressed = codec.decompress(&compressed).expect("Decompression failed");
        assert_eq!(decompressed.len(), 0);
    }
}
