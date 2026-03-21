//! Adaptive compression for cached data
//!
//! Automatically selects the best compression codec based on:
//! - Data type and characteristics
//! - Compression ratio vs speed tradeoff
//! - Historical performance metrics

use crate::error::{CacheError, Result};
use bytes::Bytes;
use std::collections::HashMap;

/// Compression codec type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum CompressionCodec {
    /// No compression
    None,
    /// LZ4 - very fast, moderate compression
    Lz4,
    /// Zstd - configurable speed/ratio tradeoff
    Zstd,
    /// Snappy - fast, moderate compression
    Snappy,
}

/// Compression level for codecs that support it
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionLevel {
    /// Fastest compression
    Fast,
    /// Balanced compression
    Default,
    /// Best compression ratio
    Best,
}

impl CompressionLevel {
    /// Convert to zstd level
    pub fn to_zstd_level(&self) -> i32 {
        match self {
            CompressionLevel::Fast => 1,
            CompressionLevel::Default => 3,
            CompressionLevel::Best => 19,
        }
    }
}

/// Statistics for a compression operation
#[derive(Debug, Clone)]
pub struct CompressionStats {
    /// Original size in bytes
    pub original_size: usize,
    /// Compressed size in bytes
    pub compressed_size: usize,
    /// Compression time in microseconds
    pub compression_time_us: u64,
    /// Decompression time in microseconds
    pub decompression_time_us: u64,
    /// Codec used
    pub codec: CompressionCodec,
}

impl CompressionStats {
    /// Calculate compression ratio
    pub fn compression_ratio(&self) -> f64 {
        if self.compressed_size > 0 {
            self.original_size as f64 / self.compressed_size as f64
        } else {
            1.0
        }
    }

    /// Calculate compression throughput in MB/s
    pub fn compression_throughput_mbps(&self) -> f64 {
        if self.compression_time_us > 0 {
            let mb = self.original_size as f64 / (1024.0 * 1024.0);
            let seconds = self.compression_time_us as f64 / 1_000_000.0;
            mb / seconds
        } else {
            0.0
        }
    }

    /// Calculate decompression throughput in MB/s
    pub fn decompression_throughput_mbps(&self) -> f64 {
        if self.decompression_time_us > 0 {
            let mb = self.compressed_size as f64 / (1024.0 * 1024.0);
            let seconds = self.decompression_time_us as f64 / 1_000_000.0;
            mb / seconds
        } else {
            0.0
        }
    }

    /// Calculate efficiency score (ratio * throughput)
    pub fn efficiency_score(&self) -> f64 {
        self.compression_ratio() * self.compression_throughput_mbps()
    }
}

/// Data type hints for compression selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DataType {
    /// Generic binary data
    Binary,
    /// Text/JSON/XML
    Text,
    /// Image data
    Image,
    /// Numerical/scientific data
    Numerical,
    /// Already compressed data
    Compressed,
}

/// Adaptive compressor that selects best codec
pub struct AdaptiveCompressor {
    /// Historical performance data per codec and data type
    performance_history: HashMap<(CompressionCodec, DataType), Vec<CompressionStats>>,
    /// Default compression level
    default_level: CompressionLevel,
    /// Minimum size threshold for compression (bytes)
    min_compress_size: usize,
    /// Maximum history entries per codec/datatype
    max_history: usize,
}

impl AdaptiveCompressor {
    /// Create new adaptive compressor
    pub fn new() -> Self {
        Self {
            performance_history: HashMap::new(),
            default_level: CompressionLevel::Default,
            min_compress_size: 1024, // 1KB
            max_history: 100,
        }
    }

    /// Set compression level
    pub fn with_level(mut self, level: CompressionLevel) -> Self {
        self.default_level = level;
        self
    }

    /// Set minimum compression size
    pub fn with_min_size(mut self, size: usize) -> Self {
        self.min_compress_size = size;
        self
    }

    /// Compress data with specified codec
    pub fn compress(
        &mut self,
        data: &[u8],
        codec: CompressionCodec,
        data_type: DataType,
    ) -> Result<Bytes> {
        if data.len() < self.min_compress_size {
            return Ok(Bytes::copy_from_slice(data));
        }

        let start = std::time::Instant::now();

        let compressed = match codec {
            CompressionCodec::None => Bytes::copy_from_slice(data),
            CompressionCodec::Lz4 => self.compress_lz4(data)?,
            CompressionCodec::Zstd => self.compress_zstd(data)?,
            CompressionCodec::Snappy => self.compress_snappy(data)?,
        };

        let compression_time_us = start.elapsed().as_micros() as u64;

        // Record statistics
        let stats = CompressionStats {
            original_size: data.len(),
            compressed_size: compressed.len(),
            compression_time_us,
            decompression_time_us: 0,
            codec,
        };

        self.record_stats(data_type, stats);

        Ok(compressed)
    }

    /// Decompress data with specified codec
    pub fn decompress(&mut self, data: &[u8], codec: CompressionCodec) -> Result<Bytes> {
        let start = std::time::Instant::now();

        let decompressed = match codec {
            CompressionCodec::None => Bytes::copy_from_slice(data),
            CompressionCodec::Lz4 => self.decompress_lz4(data)?,
            CompressionCodec::Zstd => self.decompress_zstd(data)?,
            CompressionCodec::Snappy => self.decompress_snappy(data)?,
        };

        let _decompression_time_us = start.elapsed().as_micros() as u64;

        Ok(decompressed)
    }

    /// Select best codec for data type based on historical performance
    pub fn select_codec(&self, data_type: DataType) -> CompressionCodec {
        // Find codec with best efficiency score for this data type
        let mut best_codec = CompressionCodec::Lz4; // Default
        let mut best_score = 0.0;

        for codec in &[
            CompressionCodec::Lz4,
            CompressionCodec::Zstd,
            CompressionCodec::Snappy,
        ] {
            if let Some(stats_vec) = self.performance_history.get(&(*codec, data_type)) {
                if !stats_vec.is_empty() {
                    let avg_score: f64 =
                        stats_vec.iter().map(|s| s.efficiency_score()).sum::<f64>()
                            / stats_vec.len() as f64;

                    if avg_score > best_score {
                        best_score = avg_score;
                        best_codec = *codec;
                    }
                }
            }
        }

        // If no history, use heuristics
        if best_score == 0.0 {
            return self.heuristic_codec(data_type);
        }

        best_codec
    }

    /// Heuristic codec selection based on data type
    fn heuristic_codec(&self, data_type: DataType) -> CompressionCodec {
        match data_type {
            DataType::Binary => CompressionCodec::Lz4,
            DataType::Text => CompressionCodec::Zstd,
            DataType::Image => CompressionCodec::Lz4,
            DataType::Numerical => CompressionCodec::Zstd,
            DataType::Compressed => CompressionCodec::None,
        }
    }

    /// Compress with LZ4
    fn compress_lz4(&self, data: &[u8]) -> Result<Bytes> {
        // Compress with oxiarc-lz4 and prepend original size as 4-byte LE i32
        let compressed =
            oxiarc_lz4::compress_block(data).map_err(|e| CacheError::Compression(e.to_string()))?;
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
            return Err(CacheError::Decompression("LZ4 data too short".to_string()));
        }
        let orig_size = i32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        let decompressed = oxiarc_lz4::decompress_block(&data[4..], orig_size)
            .map_err(|e| CacheError::Decompression(e.to_string()))?;
        Ok(Bytes::from(decompressed))
    }

    /// Compress with Zstd
    fn compress_zstd(&self, data: &[u8]) -> Result<Bytes> {
        let level = self.default_level.to_zstd_level();
        oxiarc_zstd::encode_all(data, level)
            .map(Bytes::from)
            .map_err(|e| CacheError::Compression(e.to_string()))
    }

    /// Decompress with Zstd
    fn decompress_zstd(&self, data: &[u8]) -> Result<Bytes> {
        oxiarc_zstd::decode_all(data)
            .map(Bytes::from)
            .map_err(|e| CacheError::Decompression(e.to_string()))
    }

    /// Compress with Snappy
    fn compress_snappy(&self, data: &[u8]) -> Result<Bytes> {
        Ok(Bytes::from(oxiarc_snappy::compress(data)))
    }

    /// Decompress with Snappy
    fn decompress_snappy(&self, data: &[u8]) -> Result<Bytes> {
        oxiarc_snappy::decompress(data)
            .map(Bytes::from)
            .map_err(|e| CacheError::Decompression(e.to_string()))
    }

    /// Record compression statistics
    fn record_stats(&mut self, data_type: DataType, stats: CompressionStats) {
        let key = (stats.codec, data_type);
        let history = self.performance_history.entry(key).or_default();

        history.push(stats);

        // Limit history size
        if history.len() > self.max_history {
            history.remove(0);
        }
    }

    /// Get average compression ratio for a codec and data type
    pub fn avg_compression_ratio(
        &self,
        codec: CompressionCodec,
        data_type: DataType,
    ) -> Option<f64> {
        self.performance_history
            .get(&(codec, data_type))
            .and_then(|stats_vec| {
                if stats_vec.is_empty() {
                    None
                } else {
                    let avg = stats_vec.iter().map(|s| s.compression_ratio()).sum::<f64>()
                        / stats_vec.len() as f64;
                    Some(avg)
                }
            })
    }

    /// Get performance statistics for all codecs
    pub fn get_performance_stats(
        &self,
    ) -> HashMap<(CompressionCodec, DataType), PerformanceMetrics> {
        let mut result = HashMap::new();

        for (key, stats_vec) in &self.performance_history {
            if stats_vec.is_empty() {
                continue;
            }

            let avg_ratio = stats_vec.iter().map(|s| s.compression_ratio()).sum::<f64>()
                / stats_vec.len() as f64;

            let avg_comp_throughput = stats_vec
                .iter()
                .map(|s| s.compression_throughput_mbps())
                .sum::<f64>()
                / stats_vec.len() as f64;

            let avg_decomp_throughput = stats_vec
                .iter()
                .filter(|s| s.decompression_time_us > 0)
                .map(|s| s.decompression_throughput_mbps())
                .sum::<f64>()
                / stats_vec.len() as f64;

            result.insert(
                *key,
                PerformanceMetrics {
                    avg_compression_ratio: avg_ratio,
                    avg_compression_throughput_mbps: avg_comp_throughput,
                    avg_decompression_throughput_mbps: avg_decomp_throughput,
                    sample_count: stats_vec.len(),
                },
            );
        }

        result
    }

    /// Clear all performance history
    pub fn clear_history(&mut self) {
        self.performance_history.clear();
    }
}

impl Default for AdaptiveCompressor {
    fn default() -> Self {
        Self::new()
    }
}

/// Performance metrics summary
#[derive(Debug, Clone)]
pub struct PerformanceMetrics {
    /// Average compression ratio
    pub avg_compression_ratio: f64,
    /// Average compression throughput in MB/s
    pub avg_compression_throughput_mbps: f64,
    /// Average decompression throughput in MB/s
    pub avg_decompression_throughput_mbps: f64,
    /// Number of samples
    pub sample_count: usize,
}

/// Compressed data container
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompressedData {
    /// Compressed bytes
    #[serde(with = "serde_bytes")]
    pub data: Vec<u8>,
    /// Codec used
    pub codec: CompressionCodec,
    /// Original size (for validation)
    pub original_size: usize,
}

impl CompressedData {
    /// Create new compressed data
    pub fn new(data: Vec<u8>, codec: CompressionCodec, original_size: usize) -> Self {
        Self {
            data,
            codec,
            original_size,
        }
    }

    /// Decompress the data
    pub fn decompress(&self, compressor: &mut AdaptiveCompressor) -> Result<Bytes> {
        let decompressed = compressor.decompress(&self.data, self.codec)?;

        // Validate size
        if decompressed.len() != self.original_size {
            return Err(CacheError::Decompression(format!(
                "Size mismatch: expected {}, got {}",
                self.original_size,
                decompressed.len()
            )));
        }

        Ok(decompressed)
    }

    /// Get compressed size
    pub fn compressed_size(&self) -> usize {
        self.data.len()
    }

    /// Calculate compression ratio
    pub fn compression_ratio(&self) -> f64 {
        if !self.data.is_empty() {
            self.original_size as f64 / self.data.len() as f64
        } else {
            1.0
        }
    }
}

mod serde_bytes {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(bytes: &Vec<u8>, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        bytes.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> std::result::Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Vec::<u8>::deserialize(deserializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lz4_compression() {
        let mut compressor = AdaptiveCompressor::new();
        let data = b"Hello, World! ".repeat(100);

        let compressed = compressor
            .compress(&data, CompressionCodec::Lz4, DataType::Text)
            .expect("compression failed");

        assert!(compressed.len() < data.len());

        let decompressed = compressor
            .decompress(&compressed, CompressionCodec::Lz4)
            .expect("decompression failed");

        assert_eq!(decompressed.as_ref(), &data[..]);
    }

    #[test]
    fn test_zstd_compression() {
        let mut compressor = AdaptiveCompressor::new();
        let data = b"Test data for compression ".repeat(50);

        let compressed = compressor
            .compress(&data, CompressionCodec::Zstd, DataType::Text)
            .expect("compression failed");

        assert!(compressed.len() < data.len());

        let decompressed = compressor
            .decompress(&compressed, CompressionCodec::Zstd)
            .expect("decompression failed");

        assert_eq!(decompressed.as_ref(), &data[..]);
    }

    #[test]
    fn test_snappy_compression() {
        let mut compressor = AdaptiveCompressor::new();
        let data = b"Snappy compression test ".repeat(50);

        let compressed = compressor
            .compress(&data, CompressionCodec::Snappy, DataType::Binary)
            .expect("compression failed");

        assert!(compressed.len() < data.len());

        let decompressed = compressor
            .decompress(&compressed, CompressionCodec::Snappy)
            .expect("decompression failed");

        assert_eq!(decompressed.as_ref(), &data[..]);
    }

    #[test]
    fn test_codec_selection() {
        let compressor = AdaptiveCompressor::new();

        // Initially uses heuristics
        assert_eq!(
            compressor.select_codec(DataType::Text),
            CompressionCodec::Zstd
        );
        assert_eq!(
            compressor.select_codec(DataType::Binary),
            CompressionCodec::Lz4
        );
        assert_eq!(
            compressor.select_codec(DataType::Compressed),
            CompressionCodec::None
        );
    }

    #[test]
    fn test_min_compress_size() {
        let mut compressor = AdaptiveCompressor::new().with_min_size(1000);
        let small_data = b"small";

        let result = compressor
            .compress(small_data, CompressionCodec::Lz4, DataType::Binary)
            .expect("compression failed");

        // Should not compress small data
        assert_eq!(result.len(), small_data.len());
    }

    #[test]
    fn test_compression_stats() {
        let stats = CompressionStats {
            original_size: 1000,
            compressed_size: 500,
            compression_time_us: 1000,
            decompression_time_us: 500,
            codec: CompressionCodec::Lz4,
        };

        approx::assert_relative_eq!(stats.compression_ratio(), 2.0, epsilon = 0.01);
        assert!(stats.compression_throughput_mbps() > 0.0);
        assert!(stats.decompression_throughput_mbps() > 0.0);
    }

    #[test]
    fn test_compressed_data() {
        let mut compressor = AdaptiveCompressor::new();
        // Use enough repeated data to exceed min_compress_size (1024 bytes)
        // and achieve a compression ratio > 1.0
        let original = b"Test data for compression ratio validation! ".repeat(100);

        let compressed_bytes = compressor
            .compress(&original, CompressionCodec::Zstd, DataType::Binary)
            .expect("compression failed");

        let compressed_data = CompressedData::new(
            compressed_bytes.to_vec(),
            CompressionCodec::Zstd,
            original.len(),
        );

        assert!(compressed_data.compression_ratio() > 1.0);

        let decompressed = compressed_data
            .decompress(&mut compressor)
            .expect("decompression failed");

        assert_eq!(decompressed.as_ref(), &original[..]);
    }
}
