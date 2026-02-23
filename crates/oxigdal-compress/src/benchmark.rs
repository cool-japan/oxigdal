//! Built-in compression benchmarking
//!
//! Provides utilities for benchmarking compression codecs.

use crate::{codecs::CodecType, error::Result, metadata::CompressionMetadata};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Benchmark result for a single codec
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    /// Codec name
    pub codec: String,

    /// Original size
    pub original_size: usize,

    /// Compressed size
    pub compressed_size: usize,

    /// Compression ratio
    pub compression_ratio: f64,

    /// Space savings percentage
    pub space_savings: f64,

    /// Compression time
    pub compression_time: Duration,

    /// Decompression time
    pub decompression_time: Duration,

    /// Compression throughput (MB/s)
    pub compression_throughput: f64,

    /// Decompression throughput (MB/s)
    pub decompression_throughput: f64,
}

impl BenchmarkResult {
    /// Create from compression metadata and decompression time
    pub fn from_metadata(metadata: CompressionMetadata, decompression_time: Duration) -> Self {
        let decompression_throughput = if decompression_time.as_secs_f64() > 0.0 {
            (metadata.original_size as f64 / decompression_time.as_secs_f64()) / 1_048_576.0
        } else {
            0.0
        };

        Self {
            codec: metadata.codec,
            original_size: metadata.original_size,
            compressed_size: metadata.compressed_size,
            compression_ratio: metadata.compression_ratio,
            space_savings: metadata.space_savings,
            compression_time: metadata.duration.unwrap_or(Duration::ZERO),
            decompression_time,
            compression_throughput: metadata.throughput.unwrap_or(0.0),
            decompression_throughput,
        }
    }

    /// Format as human-readable string
    pub fn format_summary(&self) -> String {
        format!(
            "{}: {:.2}x ratio ({:.1}% savings), compress: {:.2} MB/s, decompress: {:.2} MB/s",
            self.codec,
            self.compression_ratio,
            self.space_savings,
            self.compression_throughput,
            self.decompression_throughput
        )
    }
}

/// Benchmark comparison report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkReport {
    /// All benchmark results
    pub results: Vec<BenchmarkResult>,

    /// Best codec for compression ratio
    pub best_ratio: String,

    /// Best codec for compression speed
    pub best_compression_speed: String,

    /// Best codec for decompression speed
    pub best_decompression_speed: String,

    /// Best codec for balanced performance
    pub best_balanced: String,
}

impl BenchmarkReport {
    /// Create from benchmark results
    pub fn new(results: Vec<BenchmarkResult>) -> Self {
        let best_ratio = results
            .iter()
            .max_by(|a, b| {
                a.compression_ratio
                    .partial_cmp(&b.compression_ratio)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|r| r.codec.clone())
            .unwrap_or_default();

        let best_compression_speed = results
            .iter()
            .max_by(|a, b| {
                a.compression_throughput
                    .partial_cmp(&b.compression_throughput)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|r| r.codec.clone())
            .unwrap_or_default();

        let best_decompression_speed = results
            .iter()
            .max_by(|a, b| {
                a.decompression_throughput
                    .partial_cmp(&b.decompression_throughput)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|r| r.codec.clone())
            .unwrap_or_default();

        // Balanced: compression_ratio * (compression_throughput + decompression_throughput)
        let best_balanced = results
            .iter()
            .max_by(|a, b| {
                let score_a =
                    a.compression_ratio * (a.compression_throughput + a.decompression_throughput);
                let score_b =
                    b.compression_ratio * (b.compression_throughput + b.decompression_throughput);
                score_a
                    .partial_cmp(&score_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|r| r.codec.clone())
            .unwrap_or_default();

        Self {
            results,
            best_ratio,
            best_compression_speed,
            best_decompression_speed,
            best_balanced,
        }
    }

    /// Format as human-readable report
    pub fn format_report(&self) -> String {
        let mut report = String::new();

        report.push_str("Compression Benchmark Report\n");
        report.push_str("=============================\n\n");

        for result in &self.results {
            report.push_str(&result.format_summary());
            report.push('\n');
        }

        report.push_str("\nSummary:\n");
        report.push_str(&format!("  Best Ratio: {}\n", self.best_ratio));
        report.push_str(&format!(
            "  Best Compression Speed: {}\n",
            self.best_compression_speed
        ));
        report.push_str(&format!(
            "  Best Decompression Speed: {}\n",
            self.best_decompression_speed
        ));
        report.push_str(&format!("  Best Balanced: {}\n", self.best_balanced));

        report
    }
}

/// Benchmark runner
pub struct Benchmarker {
    /// Number of iterations
    _iterations: usize,
}

impl Benchmarker {
    /// Create new benchmarker
    pub fn new(iterations: usize) -> Self {
        Self {
            _iterations: iterations,
        }
    }

    /// Run benchmark (placeholder - actual implementations would use specific codecs)
    pub fn benchmark(&self, _data: &[u8], _codecs: &[CodecType]) -> Result<BenchmarkReport> {
        // This is a placeholder. Real implementation would benchmark each codec
        Ok(BenchmarkReport {
            results: Vec::new(),
            best_ratio: String::new(),
            best_compression_speed: String::new(),
            best_decompression_speed: String::new(),
            best_balanced: String::new(),
        })
    }
}

impl Default for Benchmarker {
    fn default() -> Self {
        Self::new(3)
    }
}
