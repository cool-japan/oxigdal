//! DEFLATE compression codec (gzip/zlib)
//!
//! DEFLATE is a lossless compression algorithm that uses a combination of LZ77
//! and Huffman coding. It is widely supported and used in formats like gzip and zlib.

use crate::error::{CompressionError, Result};
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

    /// Convert to u8 level for oxiarc
    fn to_level_u8(self) -> u8 {
        self.0.clamp(0, 9) as u8
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

        let level = self.config.level.to_level_u8();

        match self.config.format {
            DeflateFormat::Gzip => oxiarc_archive::gzip::compress(input, level)
                .map_err(|e| CompressionError::Io(std::io::Error::other(e.to_string()))),
            DeflateFormat::Zlib | DeflateFormat::Raw => oxiarc_deflate::zlib_compress(input, level)
                .map_err(|e| CompressionError::Io(std::io::Error::other(e.to_string()))),
        }
    }

    /// Decompress DEFLATE data
    pub fn decompress(&self, input: &[u8]) -> Result<Vec<u8>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        match self.config.format {
            DeflateFormat::Gzip => {
                let mut reader = std::io::Cursor::new(input);
                oxiarc_archive::gzip::decompress(&mut reader)
                    .map_err(|e| CompressionError::Io(std::io::Error::other(e.to_string())))
            }
            DeflateFormat::Zlib | DeflateFormat::Raw => oxiarc_deflate::zlib_decompress(input)
                .map_err(|e| CompressionError::Io(std::io::Error::other(e.to_string()))),
        }
    }

    /// Compress data using DEFLATE stream (reads all, compresses, writes)
    pub fn compress_stream<R: Read, W: Write>(
        &self,
        mut reader: R,
        mut writer: W,
    ) -> Result<usize> {
        let mut input = Vec::new();
        reader.read_to_end(&mut input)?;
        let compressed = self.compress(&input)?;
        let n = compressed.len();
        writer.write_all(&compressed)?;
        Ok(n)
    }

    /// Decompress DEFLATE stream (reads all, decompresses, writes)
    pub fn decompress_stream<R: Read, W: Write>(
        &self,
        mut reader: R,
        mut writer: W,
    ) -> Result<usize> {
        let mut input = Vec::new();
        reader.read_to_end(&mut input)?;
        let decompressed = self.decompress(&input)?;
        let n = decompressed.len();
        writer.write_all(&decompressed)?;
        Ok(n)
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
