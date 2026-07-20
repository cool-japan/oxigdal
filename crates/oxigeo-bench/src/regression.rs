//! Performance regression detection.
//!
//! This module provides tools for detecting performance regressions by comparing
//! current benchmark results against historical baselines.

use crate::error::Result;
use crate::scenarios::ScenarioResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Baseline data for a benchmark.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Baseline {
    /// Benchmark name.
    pub name: String,
    /// Baseline execution duration.
    #[serde(with = "duration_serde")]
    pub duration: Duration,
    /// Standard deviation of duration.
    #[serde(with = "duration_serde")]
    pub std_dev: Duration,
    /// Number of samples used to compute baseline.
    pub sample_count: usize,
    /// Timestamp when baseline was created.
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Git commit hash (if available).
    pub commit_hash: Option<String>,
    /// Additional metadata.
    pub metadata: HashMap<String, String>,
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

impl Baseline {
    /// Creates a new baseline from a scenario result.
    pub fn from_result(result: &ScenarioResult) -> Self {
        Self {
            name: result.name.clone(),
            duration: result.execution_duration,
            std_dev: Duration::ZERO,
            sample_count: 1,
            timestamp: chrono::Utc::now(),
            commit_hash: None,
            metadata: HashMap::new(),
        }
    }

    /// Creates a baseline from multiple results.
    pub fn from_results(name: String, results: &[ScenarioResult]) -> Option<Self> {
        if results.is_empty() {
            return None;
        }

        let durations: Vec<f64> = results
            .iter()
            .map(|r| r.execution_duration.as_secs_f64())
            .collect();

        let mean = durations.iter().sum::<f64>() / durations.len() as f64;

        let variance =
            durations.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / durations.len() as f64;

        let std_dev = variance.sqrt();

        Some(Self {
            name,
            duration: Duration::from_secs_f64(mean),
            std_dev: Duration::from_secs_f64(std_dev),
            sample_count: results.len(),
            timestamp: chrono::Utc::now(),
            commit_hash: None,
            metadata: HashMap::new(),
        })
    }

    /// Sets the git commit hash.
    pub fn with_commit_hash<S: Into<String>>(mut self, hash: S) -> Self {
        self.commit_hash = Some(hash.into());
        self
    }

    /// Adds metadata.
    pub fn with_metadata<K, V>(mut self, key: K, value: V) -> Self
    where
        K: Into<String>,
        V: Into<String>,
    {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

/// Collection of baselines.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineStore {
    /// Baselines indexed by benchmark name.
    pub baselines: HashMap<String, Baseline>,
    /// Store creation timestamp.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Last update timestamp.
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl BaselineStore {
    /// Creates a new baseline store.
    pub fn new() -> Self {
        let now = chrono::Utc::now();
        Self {
            baselines: HashMap::new(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Adds or updates a baseline.
    pub fn add_baseline(&mut self, baseline: Baseline) {
        self.baselines.insert(baseline.name.clone(), baseline);
        self.updated_at = chrono::Utc::now();
    }

    /// Gets a baseline by name.
    pub fn get_baseline(&self, name: &str) -> Option<&Baseline> {
        self.baselines.get(name)
    }

    /// Removes a baseline by name.
    pub fn remove_baseline(&mut self, name: &str) -> Option<Baseline> {
        let result = self.baselines.remove(name);
        if result.is_some() {
            self.updated_at = chrono::Utc::now();
        }
        result
    }

    /// Saves the baseline store to a JSON file.
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let file = File::create(path.as_ref())?;
        serde_json::to_writer_pretty(file, self)?;
        Ok(())
    }

    /// Loads a baseline store from a JSON file.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path.as_ref())?;
        let store = serde_json::from_reader(file)?;
        Ok(store)
    }

    /// Merges another baseline store into this one.
    pub fn merge(&mut self, other: BaselineStore) {
        for (name, baseline) in other.baselines {
            self.baselines.insert(name, baseline);
        }
        self.updated_at = chrono::Utc::now();
    }
}

impl Default for BaselineStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Regression detection configuration.
#[derive(Debug, Clone)]
pub struct RegressionConfig {
    /// Maximum allowed slowdown ratio (e.g., 1.1 for 10% slower).
    pub max_slowdown: f64,
    /// Number of standard deviations for statistical significance.
    pub std_dev_threshold: f64,
    /// Minimum number of samples required for statistical analysis.
    pub min_samples: usize,
}

impl Default for RegressionConfig {
    fn default() -> Self {
        Self {
            max_slowdown: 1.1,
            std_dev_threshold: 2.0,
            min_samples: 3,
        }
    }
}

/// Regression detection result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegressionResult {
    /// Benchmark name.
    pub benchmark_name: String,
    /// Whether a regression was detected.
    pub is_regression: bool,
    /// Current duration.
    #[serde(with = "duration_serde")]
    pub current_duration: Duration,
    /// Baseline duration.
    #[serde(with = "duration_serde")]
    pub baseline_duration: Duration,
    /// Slowdown ratio (current / baseline).
    pub slowdown_ratio: f64,
    /// Statistical significance (in standard deviations).
    pub significance: f64,
    /// Regression severity.
    pub severity: RegressionSeverity,
}

/// Regression severity levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegressionSeverity {
    /// No regression detected.
    None,
    /// Minor regression (within acceptable threshold).
    Minor,
    /// Moderate regression (exceeds threshold).
    Moderate,
    /// Severe regression (significantly exceeds threshold).
    Severe,
}

impl RegressionResult {
    /// Creates a new regression result.
    pub fn new(
        benchmark_name: String,
        current_duration: Duration,
        baseline: &Baseline,
        config: &RegressionConfig,
    ) -> Self {
        let slowdown_ratio = current_duration.as_secs_f64() / baseline.duration.as_secs_f64();

        let significance = if baseline.std_dev.as_secs_f64() > 0.0 {
            (current_duration.as_secs_f64() - baseline.duration.as_secs_f64())
                / baseline.std_dev.as_secs_f64()
        } else {
            0.0
        };

        let is_regression =
            slowdown_ratio > config.max_slowdown || significance > config.std_dev_threshold;

        let severity = if !is_regression {
            RegressionSeverity::None
        } else if slowdown_ratio < config.max_slowdown * 1.2 {
            RegressionSeverity::Minor
        } else if slowdown_ratio < config.max_slowdown * 1.5 {
            RegressionSeverity::Moderate
        } else {
            RegressionSeverity::Severe
        };

        Self {
            benchmark_name,
            is_regression,
            current_duration,
            baseline_duration: baseline.duration,
            slowdown_ratio,
            significance,
            severity,
        }
    }
}

/// Regression detector.
pub struct RegressionDetector {
    baseline_store: BaselineStore,
    config: RegressionConfig,
    baseline_path: PathBuf,
}

impl RegressionDetector {
    /// Creates a new regression detector.
    pub fn new<P: Into<PathBuf>>(baseline_path: P, config: RegressionConfig) -> Result<Self> {
        let baseline_path = baseline_path.into();

        let baseline_store = if baseline_path.exists() {
            BaselineStore::load(&baseline_path)?
        } else {
            BaselineStore::new()
        };

        Ok(Self {
            baseline_store,
            config,
            baseline_path,
        })
    }

    /// Creates a regression detector with default configuration.
    pub fn with_defaults<P: Into<PathBuf>>(baseline_path: P) -> Result<Self> {
        Self::new(baseline_path, RegressionConfig::default())
    }

    /// Detects regressions in scenario results.
    pub fn detect(&self, results: &[ScenarioResult]) -> Vec<RegressionResult> {
        let mut regression_results = Vec::new();

        for result in results {
            if !result.success {
                continue;
            }

            if let Some(baseline) = self.baseline_store.get_baseline(&result.name) {
                let regression = RegressionResult::new(
                    result.name.clone(),
                    result.execution_duration,
                    baseline,
                    &self.config,
                );

                regression_results.push(regression);
            }
        }

        regression_results
    }

    /// Updates baselines with new results.
    pub fn update_baselines(&mut self, results: &[ScenarioResult]) -> Result<()> {
        for result in results {
            if !result.success {
                continue;
            }

            let baseline = Baseline::from_result(result);
            self.baseline_store.add_baseline(baseline);
        }

        self.save_baselines()?;

        Ok(())
    }

    /// Saves the current baselines to disk.
    pub fn save_baselines(&self) -> Result<()> {
        if let Some(parent) = self.baseline_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        self.baseline_store.save(&self.baseline_path)?;

        Ok(())
    }

    /// Gets a reference to the baseline store.
    pub fn baseline_store(&self) -> &BaselineStore {
        &self.baseline_store
    }

    /// Gets a mutable reference to the baseline store.
    pub fn baseline_store_mut(&mut self) -> &mut BaselineStore {
        &mut self.baseline_store
    }
}

/// Regression report generator.
pub struct RegressionReport {
    results: Vec<RegressionResult>,
}

impl RegressionReport {
    /// Creates a new regression report.
    pub fn new(results: Vec<RegressionResult>) -> Self {
        Self { results }
    }

    /// Gets all regressions.
    pub fn regressions(&self) -> Vec<&RegressionResult> {
        self.results.iter().filter(|r| r.is_regression).collect()
    }

    /// Gets regressions by severity.
    pub fn regressions_by_severity(&self, severity: RegressionSeverity) -> Vec<&RegressionResult> {
        self.results
            .iter()
            .filter(|r| r.is_regression && r.severity == severity)
            .collect()
    }

    /// Checks if any regressions were detected.
    pub fn has_regressions(&self) -> bool {
        self.results.iter().any(|r| r.is_regression)
    }

    /// Generates a text summary.
    pub fn generate_summary(&self) -> String {
        let mut output = String::new();

        let regressions = self.regressions();

        if regressions.is_empty() {
            output.push_str("No performance regressions detected.\n");
        } else {
            output.push_str(&format!(
                "Detected {} performance regression(s):\n\n",
                regressions.len()
            ));

            for result in regressions {
                output.push_str(&format!(
                    "  {} [{:?}]:\n",
                    result.benchmark_name, result.severity
                ));
                output.push_str(&format!(
                    "    Current:  {:.6}s\n",
                    result.current_duration.as_secs_f64()
                ));
                output.push_str(&format!(
                    "    Baseline: {:.6}s\n",
                    result.baseline_duration.as_secs_f64()
                ));
                output.push_str(&format!(
                    "    Slowdown: {:.2}x ({:.1}% slower)\n",
                    result.slowdown_ratio,
                    (result.slowdown_ratio - 1.0) * 100.0
                ));
                output.push_str(&format!(
                    "    Significance: {:.2} std dev\n\n",
                    result.significance
                ));
            }
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_baseline_creation() {
        let result = ScenarioResult {
            name: "test_bench".to_string(),
            setup_duration: Duration::from_secs(1),
            execution_duration: Duration::from_secs(5),
            teardown_duration: Duration::from_secs(1),
            total_duration: Duration::from_secs(7),
            success: true,
            error_message: None,
            metrics: HashMap::new(),
        };

        let baseline = Baseline::from_result(&result);
        assert_eq!(baseline.name, "test_bench");
        assert_eq!(baseline.duration, Duration::from_secs(5));
    }

    #[test]
    fn test_baseline_store() {
        let mut store = BaselineStore::new();

        let baseline = Baseline {
            name: "bench1".to_string(),
            duration: Duration::from_secs(2),
            std_dev: Duration::from_millis(100),
            sample_count: 10,
            timestamp: chrono::Utc::now(),
            commit_hash: None,
            metadata: HashMap::new(),
        };

        store.add_baseline(baseline);

        assert!(store.get_baseline("bench1").is_some());
        assert!(store.get_baseline("bench2").is_none());
    }

    #[test]
    fn test_regression_detection() {
        let baseline = Baseline {
            name: "test_bench".to_string(),
            duration: Duration::from_secs(1),
            std_dev: Duration::from_millis(50),
            sample_count: 10,
            timestamp: chrono::Utc::now(),
            commit_hash: None,
            metadata: HashMap::new(),
        };

        let config = RegressionConfig::default();

        // No regression (same duration)
        let result1 = RegressionResult::new(
            "test_bench".to_string(),
            Duration::from_secs(1),
            &baseline,
            &config,
        );
        assert!(!result1.is_regression);

        // Regression (50% slower)
        let result2 = RegressionResult::new(
            "test_bench".to_string(),
            Duration::from_millis(1500),
            &baseline,
            &config,
        );
        assert!(result2.is_regression);
        assert_eq!(result2.severity, RegressionSeverity::Moderate);
    }

    #[test]
    fn test_regression_report() {
        let results = vec![
            RegressionResult {
                benchmark_name: "bench1".to_string(),
                is_regression: true,
                current_duration: Duration::from_secs(2),
                baseline_duration: Duration::from_secs(1),
                slowdown_ratio: 2.0,
                significance: 5.0,
                severity: RegressionSeverity::Severe,
            },
            RegressionResult {
                benchmark_name: "bench2".to_string(),
                is_regression: false,
                current_duration: Duration::from_secs(1),
                baseline_duration: Duration::from_secs(1),
                slowdown_ratio: 1.0,
                significance: 0.0,
                severity: RegressionSeverity::None,
            },
        ];

        let report = RegressionReport::new(results);
        assert!(report.has_regressions());
        assert_eq!(report.regressions().len(), 1);
    }
}
