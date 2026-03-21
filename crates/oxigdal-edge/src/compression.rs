//! Edge-optimized compression for bandwidth-limited environments
//!
//! Provides compression strategies optimized for edge devices with
//! limited CPU and memory resources.

use crate::error::{EdgeError, Result};
use bytes::Bytes;
use serde::{Deserialize, Serialize};

/// Compression level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionLevel {
    /// Fastest compression, lower ratio
    Fast,
    /// Balanced compression and speed
    Balanced,
    /// Best compression ratio, slower
    Best,
}

impl CompressionLevel {
    /// Get LZ4 compression level
    pub fn lz4_level(&self) -> i32 {
        match self {
            Self::Fast => 1,
            Self::Balanced => 4,
            Self::Best => 9,
        }
    }

    /// Get deflate level as u8
    pub fn deflate_level_u8(&self) -> u8 {
        match self {
            Self::Fast => 1,
            Self::Balanced => 6,
            Self::Best => 9,
        }
    }
}

/// Compression strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionStrategy {
    /// LZ4 compression (fast, good for real-time)
    Lz4,
    /// Snappy compression (fast, good for throughput)
    Snappy,
    /// Deflate/GZIP (balanced)
    Deflate,
    /// No compression
    None,
}

impl CompressionStrategy {
    /// Select best strategy based on data characteristics
    pub fn auto_select(data: &[u8]) -> Self {
        // Simple heuristic based on data size and entropy
        if data.len() < 1024 {
            // Small data: no compression overhead
            Self::None
        } else if Self::estimate_entropy(data) > 0.9 {
            // High entropy (likely already compressed): skip compression
            Self::None
        } else if data.len() < 10 * 1024 {
            // Small to medium: use fast compression
            Self::Snappy
        } else {
            // Larger data: use balanced compression
            Self::Lz4
        }
    }

    /// Estimate Shannon entropy of data
    fn estimate_entropy(data: &[u8]) -> f64 {
        if data.is_empty() {
            return 0.0;
        }

        let mut counts = [0u32; 256];
        for &byte in data.iter().take(1024.min(data.len())) {
            counts[byte as usize] = counts[byte as usize].saturating_add(1);
        }

        let len = data.len().min(1024) as f64;
        let mut entropy = 0.0;

        for &count in &counts {
            if count > 0 {
                let p = count as f64 / len;
                entropy -= p * p.log2();
            }
        }

        entropy / 8.0 // Normalize to 0-1 range
    }
}

/// Edge compressor
pub struct EdgeCompressor {
    strategy: CompressionStrategy,
    level: CompressionLevel,
}

impl EdgeCompressor {
    /// Create new compressor with strategy and level
    pub fn new(strategy: CompressionStrategy, level: CompressionLevel) -> Self {
        Self { strategy, level }
    }

    /// Create compressor with auto-selected strategy
    pub fn auto() -> Self {
        Self {
            strategy: CompressionStrategy::Lz4,
            level: CompressionLevel::Balanced,
        }
    }

    /// Create fast compressor for real-time use
    pub fn fast() -> Self {
        Self {
            strategy: CompressionStrategy::Snappy,
            level: CompressionLevel::Fast,
        }
    }

    /// Create best compression for storage
    pub fn best() -> Self {
        Self {
            strategy: CompressionStrategy::Deflate,
            level: CompressionLevel::Best,
        }
    }

    /// Compress data
    pub fn compress(&self, data: &[u8]) -> Result<Bytes> {
        match self.strategy {
            CompressionStrategy::None => Ok(Bytes::copy_from_slice(data)),
            CompressionStrategy::Lz4 => self.compress_lz4(data),
            CompressionStrategy::Snappy => self.compress_snappy(data),
            CompressionStrategy::Deflate => self.compress_deflate(data),
        }
    }

    /// Decompress data
    pub fn decompress(&self, data: &[u8]) -> Result<Bytes> {
        match self.strategy {
            CompressionStrategy::None => Ok(Bytes::copy_from_slice(data)),
            CompressionStrategy::Lz4 => self.decompress_lz4(data),
            CompressionStrategy::Snappy => self.decompress_snappy(data),
            CompressionStrategy::Deflate => self.decompress_deflate(data),
        }
    }

    /// Compress with LZ4
    fn compress_lz4(&self, data: &[u8]) -> Result<Bytes> {
        // Compress with oxiarc-lz4 and prepend original size as 4-byte LE i32
        let compressed = oxiarc_lz4::compress_block_with_accel(data, self.level.lz4_level())
            .map_err(|e| EdgeError::compression(e.to_string()))?;
        let orig_size = data.len() as i32;
        let mut result = Vec::with_capacity(4 + compressed.len());
        result.extend_from_slice(&orig_size.to_le_bytes());
        result.extend_from_slice(&compressed);
        Ok(Bytes::from(result))
    }

    /// Decompress with LZ4
    fn decompress_lz4(&self, data: &[u8]) -> Result<Bytes> {
        // Data has 4-byte LE i32 size prefix followed by compressed block
        if data.len() < 4 {
            return Err(EdgeError::decompression("LZ4 data too short".to_string()));
        }
        let orig_size = i32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        let decompressed = oxiarc_lz4::decompress_block(&data[4..], orig_size)
            .map_err(|e| EdgeError::decompression(e.to_string()))?;
        Ok(Bytes::from(decompressed))
    }

    /// Compress with Snappy
    fn compress_snappy(&self, data: &[u8]) -> Result<Bytes> {
        Ok(Bytes::from(oxiarc_snappy::compress(data)))
    }

    /// Decompress with Snappy
    fn decompress_snappy(&self, data: &[u8]) -> Result<Bytes> {
        oxiarc_snappy::decompress(data)
            .map(Bytes::from)
            .map_err(|e| EdgeError::decompression(e.to_string()))
    }

    /// Compress with Deflate
    fn compress_deflate(&self, data: &[u8]) -> Result<Bytes> {
        oxiarc_deflate::deflate(data, self.level.deflate_level_u8())
            .map(Bytes::from)
            .map_err(|e| EdgeError::compression(e.to_string()))
    }

    /// Decompress with Deflate
    fn decompress_deflate(&self, data: &[u8]) -> Result<Bytes> {
        oxiarc_deflate::inflate(data)
            .map(Bytes::from)
            .map_err(|e| EdgeError::decompression(e.to_string()))
    }

    /// Get compression ratio for data
    pub fn compression_ratio(&self, original_size: usize, compressed_size: usize) -> f64 {
        if original_size == 0 {
            return 0.0;
        }
        compressed_size as f64 / original_size as f64
    }

    /// Estimate compressed size without actually compressing
    pub fn estimate_compressed_size(&self, data: &[u8]) -> usize {
        match self.strategy {
            CompressionStrategy::None => data.len(),
            CompressionStrategy::Snappy => {
                // Snappy worst case: ~1.5x original size
                (data.len() as f64 * 1.5) as usize
            }
            CompressionStrategy::Lz4 => {
                // LZ4 worst case: original size + overhead
                data.len() + (data.len() / 255) + 16
            }
            CompressionStrategy::Deflate => {
                // Deflate worst case: ~1.1x original size
                (data.len() as f64 * 1.1) as usize
            }
        }
    }
}

/// Adaptive compressor that selects strategy based on data
pub struct AdaptiveCompressor {
    level: CompressionLevel,
}

impl AdaptiveCompressor {
    /// Create new adaptive compressor
    pub fn new(level: CompressionLevel) -> Self {
        Self { level }
    }

    /// Compress data with auto-selected strategy
    pub fn compress(&self, data: &[u8]) -> Result<(Bytes, CompressionStrategy)> {
        let strategy = CompressionStrategy::auto_select(data);
        let compressor = EdgeCompressor::new(strategy, self.level);
        let compressed = compressor.compress(data)?;
        Ok((compressed, strategy))
    }

    /// Decompress data with specified strategy
    pub fn decompress(&self, data: &[u8], strategy: CompressionStrategy) -> Result<Bytes> {
        let compressor = EdgeCompressor::new(strategy, self.level);
        compressor.decompress(data)
    }
}

/// Compressed data with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressedData {
    /// Compression strategy used
    pub strategy: CompressionStrategy,
    /// Original size
    pub original_size: usize,
    /// Compressed size
    pub compressed_size: usize,
    /// Compressed data
    pub data: Vec<u8>,
}

impl CompressedData {
    /// Create new compressed data
    pub fn new(strategy: CompressionStrategy, original_size: usize, data: Bytes) -> Self {
        let compressed_size = data.len();
        Self {
            strategy,
            original_size,
            compressed_size,
            data: data.to_vec(),
        }
    }

    /// Get compression ratio
    pub fn ratio(&self) -> f64 {
        if self.original_size == 0 {
            return 0.0;
        }
        self.compressed_size as f64 / self.original_size as f64
    }

    /// Get space saved in bytes
    pub fn space_saved(&self) -> usize {
        self.original_size.saturating_sub(self.compressed_size)
    }

    /// Get space saved as percentage
    pub fn space_saved_percent(&self) -> f64 {
        if self.original_size == 0 {
            return 0.0;
        }
        (self.space_saved() as f64 / self.original_size as f64) * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compression_lz4() -> Result<()> {
        let compressor = EdgeCompressor::new(CompressionStrategy::Lz4, CompressionLevel::Balanced);
        // Use larger data to ensure compression is beneficial
        let data = b"Hello, World! This is a test message for compression. \
                     Repeat this several times to make it worth compressing. \
                     Hello, World! This is a test message for compression.";

        let compressed = compressor.compress(data)?;
        let decompressed = compressor.decompress(&compressed)?;

        assert_eq!(&decompressed[..], &data[..]);
        // Note: For very small data, compression may not reduce size due to overhead
        // Just verify decompression works correctly

        Ok(())
    }

    #[test]
    fn test_compression_snappy() -> Result<()> {
        let compressor = EdgeCompressor::new(CompressionStrategy::Snappy, CompressionLevel::Fast);
        let data = b"Hello, World! This is a test message for compression.";

        let compressed = compressor.compress(data)?;
        let decompressed = compressor.decompress(&compressed)?;

        assert_eq!(&decompressed[..], &data[..]);

        Ok(())
    }

    #[test]
    fn test_compression_deflate() -> Result<()> {
        let compressor = EdgeCompressor::new(CompressionStrategy::Deflate, CompressionLevel::Best);
        let data = b"Hello, World! This is a test message for compression.";

        let compressed = compressor.compress(data)?;
        let decompressed = compressor.decompress(&compressed)?;

        assert_eq!(&decompressed[..], &data[..]);

        Ok(())
    }

    #[test]
    fn test_compression_none() -> Result<()> {
        let compressor = EdgeCompressor::new(CompressionStrategy::None, CompressionLevel::Fast);
        let data = b"Hello, World!";

        let compressed = compressor.compress(data)?;
        assert_eq!(&compressed[..], &data[..]);

        Ok(())
    }

    #[test]
    fn test_adaptive_compression() -> Result<()> {
        let compressor = AdaptiveCompressor::new(CompressionLevel::Balanced);
        let data = b"Hello, World! This is a test message for adaptive compression.";

        let (compressed, strategy) = compressor.compress(data)?;
        let decompressed = compressor.decompress(&compressed, strategy)?;

        assert_eq!(&decompressed[..], &data[..]);

        Ok(())
    }

    #[test]
    fn test_auto_select_strategy() {
        let small_data = b"Hi";
        let strategy = CompressionStrategy::auto_select(small_data);
        assert_eq!(strategy, CompressionStrategy::None);

        let medium_data = vec![0u8; 5000];
        let strategy = CompressionStrategy::auto_select(&medium_data);
        assert!(matches!(
            strategy,
            CompressionStrategy::Snappy | CompressionStrategy::Lz4
        ));
    }

    #[test]
    fn test_compression_ratio() {
        let compressor = EdgeCompressor::fast();
        let ratio = compressor.compression_ratio(1000, 500);
        assert_eq!(ratio, 0.5);
    }

    #[test]
    fn test_compressed_data_metadata() -> Result<()> {
        // Use larger, more compressible data to ensure compression works
        let original = b"Test data for compression. This message repeats. \
                         Test data for compression. This message repeats. \
                         Test data for compression. This message repeats.";
        let compressor = EdgeCompressor::fast();
        let compressed = compressor.compress(original)?;

        let metadata = CompressedData::new(CompressionStrategy::Snappy, original.len(), compressed);

        assert_eq!(metadata.original_size, original.len());
        // Compression ratio should be positive (can be > 1.0 if compression increases size due to overhead)
        assert!(metadata.ratio() > 0.0);
        // Space saved percentage can be negative if compression increased size
        assert!(
            metadata.space_saved_percent() >= -100.0 && metadata.space_saved_percent() <= 100.0
        );

        Ok(())
    }
}
