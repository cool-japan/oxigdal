//! Cross-implementation comparison utilities.
//!
//! This module provides tools for comparing performance across different
//! implementations (e.g., OxiGDAL vs GDAL, different algorithms, etc.).

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Implementation identifier for comparison.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Implementation {
    /// Name of the implementation (e.g., "oxigdal", "gdal", "rasterio").
    pub name: String,
    /// Version of the implementation.
    pub version: String,
    /// Additional configuration or context.
    pub config: HashMap<String, String>,
}

impl Implementation {
    /// Creates a new implementation identifier.
    pub fn new<S1, S2>(name: S1, version: S2) -> Self
    where
        S1: Into<String>,
        S2: Into<String>,
    {
        Self {
            name: name.into(),
            version: version.into(),
            config: HashMap::new(),
        }
    }

    /// Adds a configuration entry.
    pub fn with_config<K, V>(mut self, key: K, value: V) -> Self
    where
        K: Into<String>,
        V: Into<String>,
    {
        self.config.insert(key.into(), value.into());
        self
    }
}

/// Benchmark result for a single implementation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    /// Implementation identifier.
    pub implementation: Implementation,
    /// Benchmark name.
    pub benchmark_name: String,
    /// Execution duration.
    #[serde(with = "duration_serde")]
    pub duration: Duration,
    /// Throughput (operations per second).
    pub throughput: Option<f64>,
    /// Memory usage in bytes.
    pub memory_usage: Option<u64>,
    /// Additional metrics.
    pub metrics: HashMap<String, f64>,
    /// Whether the benchmark succeeded.
    pub success: bool,
    /// Error message if failed.
    pub error_message: Option<String>,
}

mod duration_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        duration.as_secs_f64().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> std::result::Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = f64::deserialize(deserializer)?;
        Ok(Duration::from_secs_f64(secs))
    }
}

/// Comparison between multiple implementations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comparison {
    /// Benchmark name.
    pub benchmark_name: String,
    /// Results from different implementations.
    pub results: Vec<BenchmarkResult>,
    /// Baseline implementation (if any).
    pub baseline: Option<String>,
}

impl Comparison {
    /// Creates a new comparison.
    pub fn new<S: Into<String>>(benchmark_name: S) -> Self {
        Self {
            benchmark_name: benchmark_name.into(),
            results: Vec::new(),
            baseline: None,
        }
    }

    /// Adds a benchmark result.
    pub fn add_result(&mut self, result: BenchmarkResult) {
        self.results.push(result);
    }

    /// Sets the baseline implementation.
    pub fn set_baseline<S: Into<String>>(&mut self, name: S) {
        self.baseline = Some(name.into());
    }

    /// Gets the baseline result.
    pub fn baseline_result(&self) -> Option<&BenchmarkResult> {
        let baseline_name = self.baseline.as_ref()?;
        self.results
            .iter()
            .find(|r| &r.implementation.name == baseline_name)
    }

    /// Computes speedup relative to baseline.
    pub fn speedup(&self, implementation: &str) -> Option<f64> {
        let baseline = self.baseline_result()?;
        let target = self
            .results
            .iter()
            .find(|r| r.implementation.name == implementation)?;

        if !baseline.success || !target.success {
            return None;
        }

        Some(baseline.duration.as_secs_f64() / target.duration.as_secs_f64())
    }

    /// Computes speedup for all implementations relative to baseline.
    pub fn all_speedups(&self) -> HashMap<String, f64> {
        let mut speedups = HashMap::new();

        for result in &self.results {
            if let Some(speedup) = self.speedup(&result.implementation.name) {
                speedups.insert(result.implementation.name.clone(), speedup);
            }
        }

        speedups
    }

    /// Finds the fastest implementation.
    pub fn fastest(&self) -> Option<&BenchmarkResult> {
        self.results
            .iter()
            .filter(|r| r.success)
            .min_by(|a, b| a.duration.cmp(&b.duration))
    }

    /// Finds the slowest implementation.
    pub fn slowest(&self) -> Option<&BenchmarkResult> {
        self.results
            .iter()
            .filter(|r| r.success)
            .max_by(|a, b| a.duration.cmp(&b.duration))
    }

    /// Computes memory efficiency (throughput per byte).
    pub fn memory_efficiency(&self, implementation: &str) -> Option<f64> {
        let result = self
            .results
            .iter()
            .find(|r| r.implementation.name == implementation)?;

        let throughput = result.throughput?;
        let memory = result.memory_usage?;

        Some(throughput / memory as f64)
    }
}

/// Comparison suite for multiple benchmarks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonSuite {
    /// Suite name.
    pub name: String,
    /// Individual comparisons.
    pub comparisons: Vec<Comparison>,
}

impl ComparisonSuite {
    /// Creates a new comparison suite.
    pub fn new<S: Into<String>>(name: S) -> Self {
        Self {
            name: name.into(),
            comparisons: Vec::new(),
        }
    }

    /// Adds a comparison to the suite.
    pub fn add_comparison(&mut self, comparison: Comparison) {
        self.comparisons.push(comparison);
    }

    /// Gets a comparison by name.
    pub fn get_comparison(&self, name: &str) -> Option<&Comparison> {
        self.comparisons.iter().find(|c| c.benchmark_name == name)
    }

    /// Computes average speedup across all benchmarks for an implementation.
    pub fn average_speedup(&self, implementation: &str) -> Option<f64> {
        let speedups: Vec<f64> = self
            .comparisons
            .iter()
            .filter_map(|c| c.speedup(implementation))
            .collect();

        if speedups.is_empty() {
            None
        } else {
            Some(speedups.iter().sum::<f64>() / speedups.len() as f64)
        }
    }

    /// Computes geometric mean speedup across all benchmarks.
    pub fn geometric_mean_speedup(&self, implementation: &str) -> Option<f64> {
        let speedups: Vec<f64> = self
            .comparisons
            .iter()
            .filter_map(|c| c.speedup(implementation))
            .collect();

        if speedups.is_empty() {
            None
        } else {
            let product: f64 = speedups.iter().product();
            Some(product.powf(1.0 / speedups.len() as f64))
        }
    }

    /// Gets summary statistics for an implementation.
    pub fn summary_stats(&self, implementation: &str) -> SummaryStats {
        let durations: Vec<f64> = self
            .comparisons
            .iter()
            .filter_map(|c| {
                c.results
                    .iter()
                    .find(|r| r.implementation.name == implementation && r.success)
                    .map(|r| r.duration.as_secs_f64())
            })
            .collect();

        if durations.is_empty() {
            return SummaryStats::default();
        }

        let sum: f64 = durations.iter().sum();
        let mean = sum / durations.len() as f64;

        let variance =
            durations.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / durations.len() as f64;
        let std_dev = variance.sqrt();

        let mut sorted = durations.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let median = if sorted.len() % 2 == 0 {
            (sorted[sorted.len() / 2 - 1] + sorted[sorted.len() / 2]) / 2.0
        } else {
            sorted[sorted.len() / 2]
        };

        let min = sorted.first().copied().unwrap_or(0.0);
        let max = sorted.last().copied().unwrap_or(0.0);

        SummaryStats {
            count: durations.len(),
            mean,
            median,
            std_dev,
            min,
            max,
        }
    }

    /// Exports the comparison suite to JSON.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(|e| e.into())
    }

    /// Loads a comparison suite from JSON.
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).map_err(|e| e.into())
    }
}

/// Summary statistics for benchmark results.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SummaryStats {
    /// Number of results.
    pub count: usize,
    /// Mean duration in seconds.
    pub mean: f64,
    /// Median duration in seconds.
    pub median: f64,
    /// Standard deviation in seconds.
    pub std_dev: f64,
    /// Minimum duration in seconds.
    pub min: f64,
    /// Maximum duration in seconds.
    pub max: f64,
}

/// Comparison report generator.
pub struct ComparisonReport {
    suite: ComparisonSuite,
}

impl ComparisonReport {
    /// Creates a new comparison report.
    pub fn new(suite: ComparisonSuite) -> Self {
        Self { suite }
    }

    /// Generates a text summary.
    pub fn generate_text_summary(&self) -> String {
        let mut output = String::new();
        output.push_str(&format!("Comparison Suite: {}\n", self.suite.name));
        output.push_str(&format!(
            "Total Benchmarks: {}\n\n",
            self.suite.comparisons.len()
        ));

        for comparison in &self.suite.comparisons {
            output.push_str(&format!("Benchmark: {}\n", comparison.benchmark_name));
            output.push_str(&"-".repeat(60));
            output.push('\n');

            if let Some(fastest) = comparison.fastest() {
                output.push_str(&format!(
                    "Fastest: {} ({:.3}s)\n",
                    fastest.implementation.name,
                    fastest.duration.as_secs_f64()
                ));
            }

            if let Some(baseline) = comparison.baseline.as_ref() {
                output.push_str(&format!("Baseline: {}\n", baseline));

                let speedups = comparison.all_speedups();
                for (impl_name, speedup) in speedups {
                    if impl_name != *baseline {
                        output.push_str(&format!("  {} speedup: {:.2}x\n", impl_name, speedup));
                    }
                }
            }

            output.push('\n');
        }

        output
    }

    /// Generates a markdown table.
    pub fn generate_markdown_table(&self) -> String {
        let mut output = String::new();
        output.push_str(&format!("# {}\n\n", self.suite.name));

        for comparison in &self.suite.comparisons {
            output.push_str(&format!("## {}\n\n", comparison.benchmark_name));

            // Table header
            output.push_str("| Implementation | Duration (s) | Speedup | Status |\n");
            output.push_str("|----------------|--------------|---------|--------|\n");

            for result in &comparison.results {
                let duration = format!("{:.6}", result.duration.as_secs_f64());
                let speedup = comparison
                    .speedup(&result.implementation.name)
                    .map(|s| format!("{:.2}x", s))
                    .unwrap_or_else(|| "-".to_string());
                let status = if result.success { "✓" } else { "✗" };

                output.push_str(&format!(
                    "| {} | {} | {} | {} |\n",
                    result.implementation.name, duration, speedup, status
                ));
            }

            output.push('\n');
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_implementation_creation() {
        let impl_id = Implementation::new("oxigdal", "0.1.0").with_config("compression", "lzw");

        assert_eq!(impl_id.name, "oxigdal");
        assert_eq!(impl_id.version, "0.1.0");
        assert_eq!(impl_id.config.get("compression"), Some(&"lzw".to_string()));
    }

    #[test]
    fn test_comparison_speedup() {
        let mut comparison = Comparison::new("test_benchmark");
        comparison.set_baseline("baseline");

        let baseline_result = BenchmarkResult {
            implementation: Implementation::new("baseline", "1.0"),
            benchmark_name: "test_benchmark".to_string(),
            duration: Duration::from_secs(2),
            throughput: None,
            memory_usage: None,
            metrics: HashMap::new(),
            success: true,
            error_message: None,
        };

        let fast_result = BenchmarkResult {
            implementation: Implementation::new("fast", "1.0"),
            benchmark_name: "test_benchmark".to_string(),
            duration: Duration::from_secs(1),
            throughput: None,
            memory_usage: None,
            metrics: HashMap::new(),
            success: true,
            error_message: None,
        };

        comparison.add_result(baseline_result);
        comparison.add_result(fast_result);

        let speedup = comparison.speedup("fast");
        assert!(speedup.is_some());
        assert_eq!(speedup.expect("speedup"), 2.0);
    }

    #[test]
    fn test_comparison_suite() {
        let mut suite = ComparisonSuite::new("Test Suite");

        let mut comparison = Comparison::new("benchmark1");
        comparison.add_result(BenchmarkResult {
            implementation: Implementation::new("impl1", "1.0"),
            benchmark_name: "benchmark1".to_string(),
            duration: Duration::from_secs(1),
            throughput: None,
            memory_usage: None,
            metrics: HashMap::new(),
            success: true,
            error_message: None,
        });

        suite.add_comparison(comparison);

        assert_eq!(suite.comparisons.len(), 1);
        assert!(suite.get_comparison("benchmark1").is_some());
    }

    #[test]
    fn test_summary_stats() {
        let mut suite = ComparisonSuite::new("Test Suite");

        for i in 1..=5 {
            let mut comparison = Comparison::new(format!("bench{}", i));
            comparison.add_result(BenchmarkResult {
                implementation: Implementation::new("test", "1.0"),
                benchmark_name: format!("bench{}", i),
                duration: Duration::from_secs(i),
                throughput: None,
                memory_usage: None,
                metrics: HashMap::new(),
                success: true,
                error_message: None,
            });
            suite.add_comparison(comparison);
        }

        let stats = suite.summary_stats("test");
        assert_eq!(stats.count, 5);
        assert_eq!(stats.mean, 3.0);
        assert_eq!(stats.min, 1.0);
        assert_eq!(stats.max, 5.0);
    }
}
