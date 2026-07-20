//! Storage compression strategies for mobile devices

use crate::error::{MobileError, Result};
use bytes::Bytes;

/// Compression strategy for storage
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionStrategy {
    /// No compression - fastest but largest size
    None,
    /// Fast compression with moderate ratio
    Fast,
    /// Balanced compression and speed
    Balanced,
    /// Maximum compression, slower
    Maximum,
    /// Adaptive based on data characteristics
    Adaptive,
}

impl CompressionStrategy {
    /// Get compression level for zstd (1-22)
    pub fn zstd_level(&self) -> i32 {
        match self {
            Self::None => 0,
            Self::Fast => 3,
            Self::Balanced => 9,
            Self::Maximum => 19,
            Self::Adaptive => 9,
        }
    }

    /// Get compression level for deflate (0-9)
    pub fn deflate_level(&self) -> u32 {
        match self {
            Self::None => 0,
            Self::Fast => 3,
            Self::Balanced => 6,
            Self::Maximum => 9,
            Self::Adaptive => 6,
        }
    }

    /// Estimate compression time factor (higher = slower)
    pub fn time_factor(&self) -> f32 {
        match self {
            Self::None => 0.0,
            Self::Fast => 1.0,
            Self::Balanced => 2.5,
            Self::Maximum => 5.0,
            Self::Adaptive => 3.0,
        }
    }

    /// Estimate compression ratio (higher = better)
    pub fn expected_ratio(&self) -> f32 {
        match self {
            Self::None => 1.0,
            Self::Fast => 2.0,
            Self::Balanced => 3.0,
            Self::Maximum => 4.0,
            Self::Adaptive => 3.0,
        }
    }
}

/// Storage compressor with multiple compression algorithms
pub struct StorageCompressor {
    strategy: CompressionStrategy,
}

impl StorageCompressor {
    /// Create a new storage compressor
    pub fn new(strategy: CompressionStrategy) -> Self {
        Self { strategy }
    }

    /// Compress data using the configured strategy
    pub fn compress(&self, data: &[u8]) -> Result<CompressedData> {
        if matches!(self.strategy, CompressionStrategy::None) {
            return Ok(CompressedData {
                data: Bytes::copy_from_slice(data),
                original_size: data.len(),
                compressed_size: data.len(),
                algorithm: CompressionAlgorithm::None,
            });
        }

        // Choose algorithm based on data characteristics
        let algorithm = if matches!(self.strategy, CompressionStrategy::Adaptive) {
            self.choose_algorithm(data)
        } else {
            CompressionAlgorithm::Zstd
        };

        let compressed = match algorithm {
            CompressionAlgorithm::None => Bytes::copy_from_slice(data),
            CompressionAlgorithm::Zstd => self.compress_zstd(data)?,
            CompressionAlgorithm::Deflate => self.compress_deflate(data)?,
        };

        Ok(CompressedData {
            data: compressed.clone(),
            original_size: data.len(),
            compressed_size: compressed.len(),
            algorithm,
        })
    }

    /// Compress using Zstandard
    fn compress_zstd(&self, data: &[u8]) -> Result<Bytes> {
        let level = self.strategy.zstd_level();
        oxiarc_zstd::encode_all(data, level)
            .map(Bytes::from)
            .map_err(|e| MobileError::CompressionError(format!("Zstd compression failed: {}", e)))
    }

    /// Compress using Deflate (zlib)
    fn compress_deflate(&self, data: &[u8]) -> Result<Bytes> {
        let level = self.strategy.deflate_level() as u8;
        oxiarc_deflate::zlib_compress(data, level)
            .map(Bytes::from)
            .map_err(|e| {
                MobileError::CompressionError(format!("Deflate compression failed: {}", e))
            })
    }

    /// Choose best compression algorithm for data
    fn choose_algorithm(&self, data: &[u8]) -> CompressionAlgorithm {
        // Very small data - use fast deflate regardless of entropy
        // The overhead of analyzing and using zstd isn't worth it for tiny payloads
        if data.len() < 256 {
            return CompressionAlgorithm::Deflate;
        }

        // Calculate entropy (simplified)
        let mut byte_counts = [0u32; 256];
        for &byte in data {
            byte_counts[byte as usize] = byte_counts[byte as usize].saturating_add(1);
        }

        let unique_bytes = byte_counts.iter().filter(|&&count| count > 0).count();

        if unique_bytes < 16 {
            // Low entropy - highly compressible, use zstd
            CompressionAlgorithm::Zstd
        } else if unique_bytes > 200 {
            // High entropy - less compressible, use fast deflate
            CompressionAlgorithm::Deflate
        } else {
            // Medium entropy - use zstd for better ratio
            CompressionAlgorithm::Zstd
        }
    }

    /// Decompress data
    pub fn decompress(&self, compressed: &CompressedData) -> Result<Bytes> {
        match compressed.algorithm {
            CompressionAlgorithm::None => Ok(compressed.data.clone()),
            CompressionAlgorithm::Zstd => self.decompress_zstd(&compressed.data),
            CompressionAlgorithm::Deflate => self.decompress_deflate(&compressed.data),
        }
    }

    /// Decompress Zstandard data
    fn decompress_zstd(&self, data: &[u8]) -> Result<Bytes> {
        oxiarc_zstd::decode_all(data).map(Bytes::from).map_err(|e| {
            MobileError::DecompressionError(format!("Zstd decompression failed: {}", e))
        })
    }

    /// Decompress Deflate data
    fn decompress_deflate(&self, data: &[u8]) -> Result<Bytes> {
        oxiarc_deflate::zlib_decompress(data)
            .map(Bytes::from)
            .map_err(|e| {
                MobileError::DecompressionError(format!("Deflate decompression failed: {}", e))
            })
    }
}

impl Default for StorageCompressor {
    fn default() -> Self {
        Self::new(CompressionStrategy::Balanced)
    }
}

/// Compression algorithm used
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionAlgorithm {
    /// No compression
    None,
    /// Zstandard compression
    Zstd,
    /// Deflate (zlib) compression
    Deflate,
}

/// Compressed data with metadata
#[derive(Debug, Clone)]
pub struct CompressedData {
    /// Compressed data bytes
    pub data: Bytes,
    /// Original uncompressed size
    pub original_size: usize,
    /// Compressed size
    pub compressed_size: usize,
    /// Algorithm used
    pub algorithm: CompressionAlgorithm,
}

impl CompressedData {
    /// Get compression ratio
    pub fn compression_ratio(&self) -> f64 {
        if self.compressed_size == 0 {
            return 1.0;
        }
        self.original_size as f64 / self.compressed_size as f64
    }

    /// Get space saved in bytes
    pub fn space_saved(&self) -> usize {
        self.original_size.saturating_sub(self.compressed_size)
    }

    /// Get space saved percentage (0.0 - 100.0)
    pub fn space_saved_percentage(&self) -> f64 {
        if self.original_size == 0 {
            return 0.0;
        }
        (self.space_saved() as f64 / self.original_size as f64) * 100.0
    }

    /// Check if compression was effective (saved > 20%)
    pub fn is_effective(&self) -> bool {
        self.space_saved_percentage() > 20.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compression_strategy_levels() {
        assert_eq!(CompressionStrategy::None.zstd_level(), 0);
        assert_eq!(CompressionStrategy::Fast.zstd_level(), 3);
        assert_eq!(CompressionStrategy::Balanced.zstd_level(), 9);
        assert_eq!(CompressionStrategy::Maximum.zstd_level(), 19);

        assert_eq!(CompressionStrategy::None.deflate_level(), 0);
        assert_eq!(CompressionStrategy::Fast.deflate_level(), 3);
        assert_eq!(CompressionStrategy::Balanced.deflate_level(), 6);
        assert_eq!(CompressionStrategy::Maximum.deflate_level(), 9);
    }

    #[test]
    fn test_storage_compressor_none() {
        let compressor = StorageCompressor::new(CompressionStrategy::None);
        let data = b"Hello, World!";

        let compressed = compressor.compress(data).expect("Compression failed");
        assert_eq!(compressed.algorithm, CompressionAlgorithm::None);
        assert_eq!(compressed.original_size, compressed.compressed_size);
        assert_eq!(compressed.compression_ratio(), 1.0);

        let decompressed = compressor
            .decompress(&compressed)
            .expect("Decompression failed");
        assert_eq!(&decompressed[..], data);
    }

    #[test]
    fn test_storage_compressor_zstd() {
        let compressor = StorageCompressor::new(CompressionStrategy::Balanced);
        let data = b"Hello, World! This is a test. ".repeat(10);

        let compressed = compressor.compress(&data).expect("Compression failed");
        assert!(compressed.compressed_size < compressed.original_size);

        let decompressed = compressor
            .decompress(&compressed)
            .expect("Decompression failed");
        assert_eq!(&decompressed[..], &data[..]);
    }

    #[test]
    fn test_storage_compressor_deflate() {
        let compressor = StorageCompressor::new(CompressionStrategy::Fast);
        let data = b"Repeating data. ".repeat(20);

        let compressed = compressor
            .compress_deflate(&data)
            .expect("Compression failed");
        assert!(compressed.len() < data.len());

        let decompressed = compressor
            .decompress_deflate(&compressed)
            .expect("Decompression failed");
        assert_eq!(&decompressed[..], &data[..]);
    }

    #[test]
    fn test_compressed_data_metrics() {
        let compressed = CompressedData {
            data: Bytes::from(vec![0u8; 50]),
            original_size: 100,
            compressed_size: 50,
            algorithm: CompressionAlgorithm::Zstd,
        };

        assert_eq!(compressed.compression_ratio(), 2.0);
        assert_eq!(compressed.space_saved(), 50);
        assert_eq!(compressed.space_saved_percentage(), 50.0);
        assert!(compressed.is_effective());
    }

    #[test]
    fn test_adaptive_compression() {
        let compressor = StorageCompressor::new(CompressionStrategy::Adaptive);

        // Small data
        let small_data = b"Small";
        let algorithm = compressor.choose_algorithm(small_data);
        assert_eq!(algorithm, CompressionAlgorithm::Deflate);

        // Highly compressible data (low entropy)
        let repeated_data = vec![0u8; 1000];
        let algorithm = compressor.choose_algorithm(&repeated_data);
        assert_eq!(algorithm, CompressionAlgorithm::Zstd);

        // Random-ish data (high entropy)
        let random_data: Vec<u8> = (0..=255).cycle().take(1000).collect();
        let algorithm = compressor.choose_algorithm(&random_data);
        // Should choose based on entropy analysis
        assert!(matches!(
            algorithm,
            CompressionAlgorithm::Deflate | CompressionAlgorithm::Zstd
        ));
    }

    #[test]
    fn test_compression_round_trip() {
        let strategies = [
            CompressionStrategy::Fast,
            CompressionStrategy::Balanced,
            CompressionStrategy::Maximum,
        ];

        let test_data = b"The quick brown fox jumps over the lazy dog. ".repeat(50);

        for strategy in &strategies {
            let compressor = StorageCompressor::new(*strategy);
            let compressed = compressor.compress(&test_data).expect("Compression failed");
            let decompressed = compressor
                .decompress(&compressed)
                .expect("Decompression failed");

            assert_eq!(&decompressed[..], &test_data[..]);
            assert!(compressed.is_effective());
        }
    }
}
