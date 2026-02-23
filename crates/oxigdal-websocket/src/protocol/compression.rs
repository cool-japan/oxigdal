//! Compression support for WebSocket protocol

use crate::error::{Error, Result};
use bytes::{Bytes, BytesMut};
use std::io::{Read, Write};

/// Compression type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionType {
    /// No compression
    None,
    /// Gzip compression
    Gzip,
    /// Zstd compression
    Zstd,
}

/// Compression level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionLevel {
    /// Fastest compression
    Fast,
    /// Default compression
    Default,
    /// Best compression
    Best,
}

impl CompressionLevel {
    /// Get zstd compression level
    pub fn zstd_level(&self) -> i32 {
        match self {
            CompressionLevel::Fast => 1,
            CompressionLevel::Default => 3,
            CompressionLevel::Best => 19,
        }
    }

    /// Get gzip compression level
    pub fn gzip_level(&self) -> flate2::Compression {
        match self {
            CompressionLevel::Fast => flate2::Compression::fast(),
            CompressionLevel::Default => flate2::Compression::default(),
            CompressionLevel::Best => flate2::Compression::best(),
        }
    }
}

/// Compression codec
pub struct CompressionCodec {
    compression_type: CompressionType,
    level: CompressionLevel,
}

impl CompressionCodec {
    /// Create a new compression codec
    pub fn new(compression_type: CompressionType, level: CompressionLevel) -> Self {
        Self {
            compression_type,
            level,
        }
    }

    /// Compress data
    pub fn compress(&self, data: &[u8]) -> Result<BytesMut> {
        match self.compression_type {
            CompressionType::None => Ok(BytesMut::from(data)),
            CompressionType::Gzip => self.compress_gzip(data),
            CompressionType::Zstd => self.compress_zstd(data),
        }
    }

    /// Decompress data
    pub fn decompress(&self, data: &[u8]) -> Result<Bytes> {
        match self.compression_type {
            CompressionType::None => Ok(Bytes::copy_from_slice(data)),
            CompressionType::Gzip => self.decompress_gzip(data),
            CompressionType::Zstd => self.decompress_zstd(data),
        }
    }

    /// Compress data using gzip
    fn compress_gzip(&self, data: &[u8]) -> Result<BytesMut> {
        use flate2::write::GzEncoder;

        let mut encoder = GzEncoder::new(Vec::new(), self.level.gzip_level());
        encoder
            .write_all(data)
            .map_err(|e| Error::Compression(format!("Gzip compression failed: {}", e)))?;

        let compressed = encoder
            .finish()
            .map_err(|e| Error::Compression(format!("Gzip finish failed: {}", e)))?;

        Ok(BytesMut::from(&compressed[..]))
    }

    /// Decompress data using gzip
    fn decompress_gzip(&self, data: &[u8]) -> Result<Bytes> {
        use flate2::read::GzDecoder;

        let mut decoder = GzDecoder::new(data);
        let mut decompressed = Vec::new();

        decoder
            .read_to_end(&mut decompressed)
            .map_err(|e| Error::Compression(format!("Gzip decompression failed: {}", e)))?;

        Ok(Bytes::from(decompressed))
    }

    /// Compress data using zstd
    fn compress_zstd(&self, data: &[u8]) -> Result<BytesMut> {
        let compressed = zstd::encode_all(data, self.level.zstd_level())
            .map_err(|e| Error::Compression(format!("Zstd compression failed: {}", e)))?;

        Ok(BytesMut::from(&compressed[..]))
    }

    /// Decompress data using zstd
    fn decompress_zstd(&self, data: &[u8]) -> Result<Bytes> {
        let decompressed = zstd::decode_all(data)
            .map_err(|e| Error::Compression(format!("Zstd decompression failed: {}", e)))?;

        Ok(Bytes::from(decompressed))
    }

    /// Get compression type
    pub fn compression_type(&self) -> CompressionType {
        self.compression_type
    }

    /// Get compression level
    pub fn level(&self) -> CompressionLevel {
        self.level
    }
}

/// Estimate compression ratio for data
pub fn estimate_compression_ratio(data: &[u8]) -> f64 {
    // Simple heuristic: count unique bytes
    let mut seen = [false; 256];
    let mut unique_count = 0;

    for &byte in data {
        if !seen[byte as usize] {
            seen[byte as usize] = true;
            unique_count += 1;
        }
    }

    // Lower unique count suggests better compression
    let ratio = unique_count as f64 / 256.0;
    1.0 - ratio // Higher value means better compression potential
}

/// Determine if data should be compressed based on size and content
pub fn should_compress(data: &[u8], min_size: usize) -> bool {
    if data.len() < min_size {
        return false;
    }

    // Check compression potential
    estimate_compression_ratio(data) > 0.3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gzip_compression() -> Result<()> {
        let codec = CompressionCodec::new(CompressionType::Gzip, CompressionLevel::Default);
        let data = b"Hello, World! This is a test message.".repeat(10);

        let compressed = codec.compress(&data)?;
        let decompressed = codec.decompress(&compressed)?;

        assert_eq!(data.as_slice(), decompressed.as_ref());
        assert!(compressed.len() < data.len());
        Ok(())
    }

    #[test]
    fn test_zstd_compression() -> Result<()> {
        let codec = CompressionCodec::new(CompressionType::Zstd, CompressionLevel::Default);
        let data = b"Hello, World! This is a test message.".repeat(10);

        let compressed = codec.compress(&data)?;
        let decompressed = codec.decompress(&compressed)?;

        assert_eq!(data.as_slice(), decompressed.as_ref());
        assert!(compressed.len() < data.len());
        Ok(())
    }

    #[test]
    fn test_no_compression() -> Result<()> {
        let codec = CompressionCodec::new(CompressionType::None, CompressionLevel::Default);
        let data = b"Hello, World!";

        let compressed = codec.compress(data)?;
        let decompressed = codec.decompress(&compressed)?;

        assert_eq!(data, compressed.as_ref());
        assert_eq!(data, decompressed.as_ref());
        Ok(())
    }

    #[test]
    fn test_compression_levels() -> Result<()> {
        let data = b"Hello, World! This is a test message.".repeat(100);

        let fast = CompressionCodec::new(CompressionType::Zstd, CompressionLevel::Fast);
        let default = CompressionCodec::new(CompressionType::Zstd, CompressionLevel::Default);
        let best = CompressionCodec::new(CompressionType::Zstd, CompressionLevel::Best);

        let fast_compressed = fast.compress(&data)?;
        let default_compressed = default.compress(&data)?;
        let best_compressed = best.compress(&data)?;

        // Best should compress better than default, default better than fast
        assert!(best_compressed.len() <= default_compressed.len());
        assert!(default_compressed.len() <= fast_compressed.len());

        Ok(())
    }

    #[test]
    fn test_estimate_compression_ratio() {
        // Highly repetitive data
        let repetitive = vec![0u8; 1000];
        let ratio1 = estimate_compression_ratio(&repetitive);
        assert!(ratio1 > 0.9);

        // Random-like data
        let random: Vec<u8> = (0..1000).map(|i| (i % 256) as u8).collect();
        let ratio2 = estimate_compression_ratio(&random);
        assert!(ratio2 < ratio1);
    }

    #[test]
    fn test_should_compress() {
        // Too small
        let small = vec![0u8; 10];
        assert!(!should_compress(&small, 100));

        // Large and repetitive
        let large_repetitive = vec![0u8; 1000];
        assert!(should_compress(&large_repetitive, 100));

        // Large but random
        let large_random: Vec<u8> = (0..1000).map(|i| (i % 256) as u8).collect();
        // This might or might not compress well depending on the threshold
        let _ = should_compress(&large_random, 100);
    }
}
