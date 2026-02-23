//! Automatic codec selection
//!
//! This module provides intelligent codec selection based on data characteristics,
//! compression goals, and historical performance.

use crate::{codecs::CodecType, metadata::CompressionMetadata};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Data type classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DataType {
    /// Continuous floating-point data (elevation, temperature, etc.)
    ContinuousFloat,
    /// Integer coordinate data
    IntegerCoordinate,
    /// Categorical data (land cover, classification)
    Categorical,
    /// Image data (RGB, multispectral)
    Image,
    /// Time series data
    TimeSeries,
    /// Generic binary data
    Generic,
}

/// Compression goal
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionGoal {
    /// Prioritize compression speed
    Speed,
    /// Balance speed and ratio
    Balanced,
    /// Prioritize compression ratio
    Ratio,
    /// Minimize decompression time
    DecompressionSpeed,
}

/// Data characteristics
#[derive(Debug, Clone)]
pub struct DataCharacteristics {
    /// Data type
    pub data_type: DataType,

    /// Data size in bytes
    pub size: usize,

    /// Entropy estimate (0-1, higher = more random)
    pub entropy: f64,

    /// Number of unique values (for sampling)
    pub unique_count: Option<usize>,

    /// Value range (min, max)
    pub value_range: Option<(f64, f64)>,

    /// Run length ratio (for RLE suitability)
    pub run_length_ratio: Option<f64>,
}

impl DataCharacteristics {
    /// Analyze data sample
    pub fn analyze(data: &[u8], data_type: DataType) -> Self {
        let entropy = Self::estimate_entropy(data);
        let unique_count = Self::count_unique_bytes(data);
        let run_length_ratio = Self::compute_run_length_ratio(data);

        Self {
            data_type,
            size: data.len(),
            entropy,
            unique_count: Some(unique_count),
            value_range: None,
            run_length_ratio: Some(run_length_ratio),
        }
    }

    /// Estimate Shannon entropy
    fn estimate_entropy(data: &[u8]) -> f64 {
        if data.is_empty() {
            return 0.0;
        }

        let mut freq = [0u32; 256];

        for &byte in data {
            freq[byte as usize] += 1;
        }

        let len = data.len() as f64;
        let mut entropy = 0.0;

        for &count in &freq {
            if count > 0 {
                let p = count as f64 / len;
                entropy -= p * p.log2();
            }
        }

        entropy / 8.0 // Normalize to 0-1 range
    }

    /// Count unique bytes in sample
    fn count_unique_bytes(data: &[u8]) -> usize {
        let mut seen = [false; 256];
        let mut count = 0;

        for &byte in data {
            if !seen[byte as usize] {
                seen[byte as usize] = true;
                count += 1;
            }
        }

        count
    }

    /// Compute run-length ratio
    fn compute_run_length_ratio(data: &[u8]) -> f64 {
        if data.is_empty() {
            return 0.0;
        }

        let mut runs = 0;
        let mut i = 0;

        while i < data.len() {
            let value = data[i];
            let mut run_len = 1;

            while i + run_len < data.len() && data[i + run_len] == value {
                run_len += 1;
            }

            runs += 1;
            i += run_len;
        }

        data.len() as f64 / runs as f64
    }
}

/// Codec recommendation with score
#[derive(Debug, Clone)]
pub struct CodecRecommendation {
    /// Recommended codec
    pub codec: CodecType,

    /// Score (0-100, higher is better)
    pub score: f64,

    /// Estimated compression ratio
    pub estimated_ratio: f64,

    /// Estimated throughput (MB/s)
    pub estimated_throughput: f64,

    /// Reason for recommendation
    pub reason: String,
}

/// Historical performance tracker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceHistory {
    /// Per-codec performance records
    records: HashMap<CodecType, Vec<CompressionMetadata>>,
}

impl PerformanceHistory {
    /// Create new history tracker
    pub fn new() -> Self {
        Self {
            records: HashMap::new(),
        }
    }

    /// Add compression result
    pub fn add_record(&mut self, codec: CodecType, metadata: CompressionMetadata) {
        self.records.entry(codec).or_default().push(metadata);

        // Keep only recent records (last 100)
        if let Some(records) = self.records.get_mut(&codec) {
            if records.len() > 100 {
                records.drain(0..records.len() - 100);
            }
        }
    }

    /// Get average compression ratio for codec
    pub fn average_ratio(&self, codec: CodecType) -> Option<f64> {
        self.records.get(&codec).and_then(|records| {
            if records.is_empty() {
                None
            } else {
                let sum: f64 = records.iter().map(|r| r.compression_ratio).sum();
                Some(sum / records.len() as f64)
            }
        })
    }

    /// Get average throughput for codec
    pub fn average_throughput(&self, codec: CodecType) -> Option<f64> {
        self.records.get(&codec).and_then(|records| {
            let throughputs: Vec<f64> = records.iter().filter_map(|r| r.throughput).collect();

            if throughputs.is_empty() {
                None
            } else {
                Some(throughputs.iter().sum::<f64>() / throughputs.len() as f64)
            }
        })
    }
}

impl Default for PerformanceHistory {
    fn default() -> Self {
        Self::new()
    }
}

/// Auto-selection engine
pub struct AutoSelector {
    /// Performance history
    history: PerformanceHistory,

    /// Compression goal
    goal: CompressionGoal,
}

impl AutoSelector {
    /// Create new auto-selector
    pub fn new(goal: CompressionGoal) -> Self {
        Self {
            history: PerformanceHistory::new(),
            goal,
        }
    }

    /// Create with existing history
    pub fn with_history(goal: CompressionGoal, history: PerformanceHistory) -> Self {
        Self { history, goal }
    }

    /// Recommend codec based on data characteristics
    pub fn recommend(&self, characteristics: &DataCharacteristics) -> Vec<CodecRecommendation> {
        let mut recommendations = Vec::new();

        // Score each codec
        for &codec in &[
            CodecType::Zstd,
            CodecType::Lz4,
            CodecType::Snappy,
            CodecType::Brotli,
            CodecType::Deflate,
            CodecType::Delta,
            CodecType::Rle,
            CodecType::Dictionary,
        ] {
            if let Some(rec) = self.score_codec(codec, characteristics) {
                recommendations.push(rec);
            }
        }

        // Sort by score
        recommendations.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        recommendations
    }

    /// Score a specific codec
    fn score_codec(
        &self,
        codec: CodecType,
        characteristics: &DataCharacteristics,
    ) -> Option<CodecRecommendation> {
        let mut score = 50.0; // Base score

        // Data type specific scoring
        match (codec, characteristics.data_type) {
            (CodecType::Delta, DataType::IntegerCoordinate) => score += 30.0,
            (CodecType::Delta, DataType::TimeSeries) => score += 25.0,
            (CodecType::Rle, DataType::Categorical) => score += 35.0,
            (CodecType::Dictionary, DataType::Categorical) => score += 30.0,
            (CodecType::Zstd, DataType::ContinuousFloat) => score += 20.0,
            (CodecType::Lz4, DataType::Image) => score += 15.0,
            _ => {}
        }

        // Entropy-based scoring
        if characteristics.entropy < 0.3 {
            // Low entropy - good for RLE, Delta
            match codec {
                CodecType::Rle => score += 20.0,
                CodecType::Delta => score += 15.0,
                _ => {}
            }
        } else if characteristics.entropy > 0.7 {
            // High entropy - need strong compression
            match codec {
                CodecType::Zstd | CodecType::Brotli => score += 15.0,
                CodecType::Rle => score -= 20.0,
                _ => {}
            }
        }

        // Goal-based scoring
        match self.goal {
            CompressionGoal::Speed => {
                score += codec.speed_score() as f64 * 2.0;
            }
            CompressionGoal::Ratio => {
                score += codec.ratio_score() as f64 * 2.0;
            }
            CompressionGoal::Balanced => {
                score += (codec.speed_score() + codec.ratio_score()) as f64;
            }
            CompressionGoal::DecompressionSpeed => {
                score += codec.speed_score() as f64 * 1.5;
            }
        }

        // Historical performance adjustment
        if let Some(hist_ratio) = self.history.average_ratio(codec) {
            score += (hist_ratio - 1.0) * 10.0;
        }

        // Estimate metrics
        let estimated_ratio = self.estimate_ratio(codec, characteristics);
        let estimated_throughput = self.estimate_throughput(codec, characteristics);

        let reason = format!(
            "{} codec selected for {:?} data (entropy: {:.2})",
            codec.name(),
            characteristics.data_type,
            characteristics.entropy
        );

        Some(CodecRecommendation {
            codec,
            score: score.clamp(0.0, 100.0),
            estimated_ratio,
            estimated_throughput,
            reason,
        })
    }

    /// Estimate compression ratio
    fn estimate_ratio(&self, codec: CodecType, characteristics: &DataCharacteristics) -> f64 {
        // Use historical data if available
        if let Some(hist_ratio) = self.history.average_ratio(codec) {
            return hist_ratio;
        }

        // Otherwise use heuristics
        let base_ratio = match codec {
            CodecType::Zstd => 3.0,
            CodecType::Brotli => 3.5,
            CodecType::Deflate => 2.5,
            CodecType::Lz4 => 2.0,
            CodecType::Snappy => 1.8,
            CodecType::Delta => 2.5,
            CodecType::Rle => characteristics.run_length_ratio.unwrap_or(2.0).max(1.0),
            CodecType::Dictionary => {
                let unique_ratio = characteristics
                    .unique_count
                    .map(|u| (characteristics.size / u.max(1)) as f64)
                    .unwrap_or(2.0);
                unique_ratio.max(1.5)
            }
        };

        // Adjust for entropy
        base_ratio * (1.0 - characteristics.entropy * 0.5).max(0.5)
    }

    /// Estimate throughput
    fn estimate_throughput(&self, codec: CodecType, _characteristics: &DataCharacteristics) -> f64 {
        // Use historical data if available
        if let Some(hist_throughput) = self.history.average_throughput(codec) {
            return hist_throughput;
        }

        // Otherwise use typical speeds (MB/s)
        match codec {
            CodecType::Snappy => 500.0,
            CodecType::Lz4 => 450.0,
            CodecType::Zstd => 400.0,
            CodecType::Deflate => 200.0,
            CodecType::Brotli => 100.0,
            CodecType::Delta => 600.0,
            CodecType::Rle => 550.0,
            CodecType::Dictionary => 300.0,
        }
    }

    /// Record compression result for learning
    pub fn record_result(&mut self, codec: CodecType, metadata: CompressionMetadata) {
        self.history.add_record(codec, metadata);
    }

    /// Get performance history
    pub fn history(&self) -> &PerformanceHistory {
        &self.history
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_characteristics() {
        let data = vec![1u8; 1000]; // Low entropy
        let chars = DataCharacteristics::analyze(&data, DataType::Categorical);

        assert!(chars.entropy < 0.1);
        assert!(chars.run_length_ratio.is_some());
    }

    #[test]
    fn test_auto_selector() {
        let selector = AutoSelector::new(CompressionGoal::Balanced);
        let chars = DataCharacteristics {
            data_type: DataType::Categorical,
            size: 10000,
            entropy: 0.2,
            unique_count: Some(10),
            value_range: None,
            run_length_ratio: Some(100.0),
        };

        let recommendations = selector.recommend(&chars);

        assert!(!recommendations.is_empty());
        // RLE should be highly recommended for categorical data with low entropy
        assert!(
            recommendations[0].codec == CodecType::Rle
                || recommendations[1].codec == CodecType::Rle
        );
    }
}
