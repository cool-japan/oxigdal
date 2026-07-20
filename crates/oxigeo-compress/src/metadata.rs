//! Compression metadata and statistics

use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime};

/// Compression metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionMetadata {
    /// Codec name
    pub codec: String,

    /// Original uncompressed size in bytes
    pub original_size: usize,

    /// Compressed size in bytes
    pub compressed_size: usize,

    /// Compression ratio (original/compressed)
    pub compression_ratio: f64,

    /// Space savings percentage
    pub space_savings: f64,

    /// Compression level used
    pub compression_level: Option<i32>,

    /// Compression parameters
    pub parameters: serde_json::Value,

    /// Compression timestamp
    pub timestamp: SystemTime,

    /// Compression duration
    pub duration: Option<Duration>,

    /// Throughput in MB/s
    pub throughput: Option<f64>,

    /// Checksum of original data (blake3)
    pub checksum_original: Option<Vec<u8>>,

    /// Checksum of compressed data (blake3)
    pub checksum_compressed: Option<Vec<u8>>,

    /// Additional metadata
    pub extra: serde_json::Value,
}

impl CompressionMetadata {
    /// Create new compression metadata
    pub fn new(codec: String, original_size: usize, compressed_size: usize) -> Self {
        let compression_ratio = if compressed_size > 0 {
            original_size as f64 / compressed_size as f64
        } else {
            0.0
        };

        let space_savings = if original_size > 0 {
            (1.0 - (compressed_size as f64 / original_size as f64)) * 100.0
        } else {
            0.0
        };

        Self {
            codec,
            original_size,
            compressed_size,
            compression_ratio,
            space_savings,
            compression_level: None,
            parameters: serde_json::Value::Null,
            timestamp: SystemTime::now(),
            duration: None,
            throughput: None,
            checksum_original: None,
            checksum_compressed: None,
            extra: serde_json::Value::Null,
        }
    }

    /// Set compression level
    pub fn with_level(mut self, level: i32) -> Self {
        self.compression_level = Some(level);
        self
    }

    /// Set parameters
    pub fn with_parameters(mut self, parameters: serde_json::Value) -> Self {
        self.parameters = parameters;
        self
    }

    /// Set duration
    pub fn with_duration(mut self, duration: Duration) -> Self {
        if self.original_size > 0 {
            let seconds = duration.as_secs_f64();
            if seconds > 0.0 {
                self.throughput = Some((self.original_size as f64 / seconds) / 1_048_576.0);
            }
        }
        self.duration = Some(duration);
        self
    }

    /// Set checksums
    pub fn with_checksums(mut self, original: Vec<u8>, compressed: Vec<u8>) -> Self {
        self.checksum_original = Some(original);
        self.checksum_compressed = Some(compressed);
        self
    }

    /// Set extra metadata
    pub fn with_extra(mut self, extra: serde_json::Value) -> Self {
        self.extra = extra;
        self
    }

    /// Format metadata as human-readable string
    pub fn format_summary(&self) -> String {
        format!(
            "Codec: {}, Size: {} -> {} bytes ({:.2}x ratio, {:.1}% savings), Throughput: {}",
            self.codec,
            self.original_size,
            self.compressed_size,
            self.compression_ratio,
            self.space_savings,
            self.throughput
                .map(|t| format!("{:.2} MB/s", t))
                .unwrap_or_else(|| "N/A".to_string())
        )
    }
}

/// Compression statistics aggregated over multiple operations
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompressionStats {
    /// Number of compression operations
    pub operations: usize,

    /// Total original size
    pub total_original_size: usize,

    /// Total compressed size
    pub total_compressed_size: usize,

    /// Average compression ratio
    pub avg_compression_ratio: f64,

    /// Average space savings
    pub avg_space_savings: f64,

    /// Average throughput
    pub avg_throughput: f64,

    /// Minimum compression ratio
    pub min_compression_ratio: f64,

    /// Maximum compression ratio
    pub max_compression_ratio: f64,

    /// Total duration
    pub total_duration: Duration,
}

impl CompressionStats {
    /// Create new empty statistics
    pub fn new() -> Self {
        Self {
            operations: 0,
            total_original_size: 0,
            total_compressed_size: 0,
            avg_compression_ratio: 0.0,
            avg_space_savings: 0.0,
            avg_throughput: 0.0,
            min_compression_ratio: f64::MAX,
            max_compression_ratio: 0.0,
            total_duration: Duration::ZERO,
        }
    }

    /// Add metadata to statistics
    pub fn add_metadata(&mut self, metadata: &CompressionMetadata) {
        self.operations += 1;
        self.total_original_size += metadata.original_size;
        self.total_compressed_size += metadata.compressed_size;

        // Update min/max ratio
        self.min_compression_ratio = self.min_compression_ratio.min(metadata.compression_ratio);
        self.max_compression_ratio = self.max_compression_ratio.max(metadata.compression_ratio);

        // Recalculate averages
        if self.total_compressed_size > 0 {
            self.avg_compression_ratio =
                self.total_original_size as f64 / self.total_compressed_size as f64;
        }

        if self.total_original_size > 0 {
            self.avg_space_savings = (1.0
                - (self.total_compressed_size as f64 / self.total_original_size as f64))
                * 100.0;
        }

        if let Some(duration) = metadata.duration {
            self.total_duration += duration;
            let seconds = self.total_duration.as_secs_f64();
            if seconds > 0.0 {
                self.avg_throughput = (self.total_original_size as f64 / seconds) / 1_048_576.0;
            }
        }
    }
}
