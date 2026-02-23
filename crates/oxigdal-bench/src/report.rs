//! Benchmark report generation.
//!
//! This module provides utilities for generating benchmark reports in various formats:
//! - HTML reports with visualizations
//! - JSON reports for programmatic access
//! - CSV reports for data analysis
//! - Markdown reports for documentation

use crate::comparison::ComparisonSuite;
use crate::error::{BenchError, Result};
use crate::scenarios::ScenarioResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

/// Report format types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportFormat {
    /// HTML format with charts.
    Html,
    /// JSON format.
    Json,
    /// CSV format.
    Csv,
    /// Markdown format.
    Markdown,
}

/// Benchmark report containing all results and metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkReport {
    /// Report title.
    pub title: String,
    /// Report generation timestamp.
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// System information.
    pub system_info: SystemInfo,
    /// Scenario results.
    pub scenario_results: Vec<ScenarioResult>,
    /// Comparison suite (if any).
    pub comparison_suite: Option<ComparisonSuite>,
    /// Summary statistics.
    pub summary: ReportSummary,
}

/// System information for the benchmark environment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    /// Operating system.
    pub os: String,
    /// CPU model.
    pub cpu: String,
    /// Number of CPU cores.
    pub cores: usize,
    /// Total memory in bytes.
    pub total_memory: u64,
    /// Rust version.
    pub rust_version: String,
    /// Custom metadata.
    pub metadata: HashMap<String, String>,
}

impl SystemInfo {
    /// Collects current system information.
    pub fn collect() -> Self {
        use sysinfo::System;

        let mut sys = System::new_all();
        sys.refresh_all();

        Self {
            os: std::env::consts::OS.to_string(),
            cpu: sys
                .cpus()
                .first()
                .map(|cpu| cpu.brand().to_string())
                .unwrap_or_else(|| "Unknown".to_string()),
            cores: sys.cpus().len(),
            total_memory: sys.total_memory(),
            rust_version: rustc_version_runtime::version().to_string(),
            metadata: HashMap::new(),
        }
    }

    /// Adds custom metadata.
    pub fn with_metadata<K, V>(mut self, key: K, value: V) -> Self
    where
        K: Into<String>,
        V: Into<String>,
    {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

/// Summary statistics for the entire report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportSummary {
    /// Total number of benchmarks.
    pub total_benchmarks: usize,
    /// Number of successful benchmarks.
    pub successful_benchmarks: usize,
    /// Number of failed benchmarks.
    pub failed_benchmarks: usize,
    /// Total execution time.
    #[serde(with = "duration_serde")]
    pub total_duration: Duration,
    /// Average execution time per benchmark.
    #[serde(with = "duration_serde")]
    pub average_duration: Duration,
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

impl BenchmarkReport {
    /// Creates a new benchmark report.
    pub fn new<S: Into<String>>(title: S) -> Self {
        Self {
            title: title.into(),
            timestamp: chrono::Utc::now(),
            system_info: SystemInfo::collect(),
            scenario_results: Vec::new(),
            comparison_suite: None,
            summary: ReportSummary {
                total_benchmarks: 0,
                successful_benchmarks: 0,
                failed_benchmarks: 0,
                total_duration: Duration::ZERO,
                average_duration: Duration::ZERO,
            },
        }
    }

    /// Adds scenario results.
    pub fn add_scenario_results(&mut self, results: Vec<ScenarioResult>) {
        self.scenario_results.extend(results);
        self.update_summary();
    }

    /// Sets the comparison suite.
    pub fn set_comparison_suite(&mut self, suite: ComparisonSuite) {
        self.comparison_suite = Some(suite);
    }

    /// Updates the summary statistics.
    fn update_summary(&mut self) {
        let total = self.scenario_results.len();
        let successful = self.scenario_results.iter().filter(|r| r.success).count();
        let failed = total - successful;

        let total_duration: Duration = self.scenario_results.iter().map(|r| r.total_duration).sum();

        let average_duration = if total > 0 {
            total_duration / total as u32
        } else {
            Duration::ZERO
        };

        self.summary = ReportSummary {
            total_benchmarks: total,
            successful_benchmarks: successful,
            failed_benchmarks: failed,
            total_duration,
            average_duration,
        };
    }

    /// Generates a report in the specified format.
    pub fn generate<P: AsRef<Path>>(&self, path: P, format: ReportFormat) -> Result<()> {
        match format {
            ReportFormat::Html => self.generate_html(path),
            ReportFormat::Json => self.generate_json(path),
            ReportFormat::Csv => self.generate_csv(path),
            ReportFormat::Markdown => self.generate_markdown(path),
        }
    }

    /// Generates an HTML report.
    fn generate_html<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let html = self.render_html()?;
        let mut file = File::create(path)?;
        file.write_all(html.as_bytes())?;
        Ok(())
    }

    /// Renders HTML content.
    fn render_html(&self) -> Result<String> {
        let template = include_str!("templates/report.html");
        let mut tt = tinytemplate::TinyTemplate::new();
        tt.add_template("report", template)
            .map_err(|e| BenchError::report_generation("HTML", e.to_string()))?;

        // Prepare template context
        let context = self.prepare_template_context();

        tt.render("report", &context)
            .map_err(|e| BenchError::report_generation("HTML", e.to_string()))
    }

    /// Prepares template context.
    fn prepare_template_context(&self) -> HashMap<String, serde_json::Value> {
        let mut context = HashMap::new();
        context.insert("title".to_string(), serde_json::json!(self.title));
        context.insert(
            "timestamp".to_string(),
            serde_json::json!(self.timestamp.to_rfc3339()),
        );
        context.insert(
            "system_info".to_string(),
            serde_json::to_value(&self.system_info).unwrap_or_default(),
        );
        context.insert(
            "scenario_results".to_string(),
            serde_json::to_value(&self.scenario_results).unwrap_or_default(),
        );
        context.insert(
            "summary".to_string(),
            serde_json::to_value(&self.summary).unwrap_or_default(),
        );
        context
    }

    /// Generates a JSON report.
    fn generate_json<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let file = File::create(path)?;
        serde_json::to_writer_pretty(file, self)?;
        Ok(())
    }

    /// Generates a CSV report.
    fn generate_csv<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let mut file = File::create(path)?;

        // CSV header
        writeln!(
            file,
            "Benchmark Name,Setup Duration (s),Execution Duration (s),Teardown Duration (s),Total Duration (s),Success,Error"
        )?;

        // CSV rows
        for result in &self.scenario_results {
            writeln!(
                file,
                "{},{},{},{},{},{},{}",
                result.name,
                result.setup_duration.as_secs_f64(),
                result.execution_duration.as_secs_f64(),
                result.teardown_duration.as_secs_f64(),
                result.total_duration.as_secs_f64(),
                result.success,
                result.error_message.as_deref().unwrap_or("")
            )?;
        }

        Ok(())
    }

    /// Generates a Markdown report.
    fn generate_markdown<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let mut file = File::create(path)?;

        writeln!(file, "# {}\n", self.title)?;
        writeln!(file, "Generated: {}\n", self.timestamp.to_rfc3339())?;

        // System Information
        writeln!(file, "## System Information\n")?;
        writeln!(file, "- OS: {}", self.system_info.os)?;
        writeln!(file, "- CPU: {}", self.system_info.cpu)?;
        writeln!(file, "- Cores: {}", self.system_info.cores)?;
        writeln!(
            file,
            "- Memory: {} GB",
            self.system_info.total_memory / (1024 * 1024 * 1024)
        )?;
        writeln!(file, "- Rust: {}\n", self.system_info.rust_version)?;

        // Summary
        writeln!(file, "## Summary\n")?;
        writeln!(
            file,
            "- Total Benchmarks: {}",
            self.summary.total_benchmarks
        )?;
        writeln!(file, "- Successful: {}", self.summary.successful_benchmarks)?;
        writeln!(file, "- Failed: {}", self.summary.failed_benchmarks)?;
        writeln!(
            file,
            "- Total Duration: {:.3}s",
            self.summary.total_duration.as_secs_f64()
        )?;
        writeln!(
            file,
            "- Average Duration: {:.3}s\n",
            self.summary.average_duration.as_secs_f64()
        )?;

        // Results Table
        writeln!(file, "## Results\n")?;
        writeln!(file, "| Benchmark | Execution (s) | Total (s) | Status |")?;
        writeln!(file, "|-----------|---------------|-----------|--------|")?;

        for result in &self.scenario_results {
            let status = if result.success { "✓" } else { "✗" };
            writeln!(
                file,
                "| {} | {:.6} | {:.6} | {} |",
                result.name,
                result.execution_duration.as_secs_f64(),
                result.total_duration.as_secs_f64(),
                status
            )?;
        }

        // Failed Benchmarks
        let failed: Vec<_> = self
            .scenario_results
            .iter()
            .filter(|r| !r.success)
            .collect();

        if !failed.is_empty() {
            writeln!(file, "\n## Failed Benchmarks\n")?;
            for result in failed {
                writeln!(file, "### {}\n", result.name)?;
                if let Some(ref error) = result.error_message {
                    writeln!(file, "```\n{}\n```\n", error)?;
                }
            }
        }

        // Comparison Suite
        if let Some(ref suite) = self.comparison_suite {
            writeln!(file, "\n## Comparisons\n")?;
            for comparison in &suite.comparisons {
                writeln!(file, "### {}\n", comparison.benchmark_name)?;
                writeln!(file, "| Implementation | Duration (s) | Speedup |")?;
                writeln!(file, "|----------------|--------------|---------|")?;

                for result in &comparison.results {
                    let speedup = comparison
                        .speedup(&result.implementation.name)
                        .map(|s| format!("{:.2}x", s))
                        .unwrap_or_else(|| "-".to_string());

                    writeln!(
                        file,
                        "| {} | {:.6} | {} |",
                        result.implementation.name,
                        result.duration.as_secs_f64(),
                        speedup
                    )?;
                }

                writeln!(file)?;
            }
        }

        Ok(())
    }

    /// Loads a report from a JSON file.
    pub fn load_json<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path)?;
        let report = serde_json::from_reader(file)?;
        Ok(report)
    }
}

/// Report builder for easier report construction.
pub struct ReportBuilder {
    report: BenchmarkReport,
}

impl ReportBuilder {
    /// Creates a new report builder.
    pub fn new<S: Into<String>>(title: S) -> Self {
        Self {
            report: BenchmarkReport::new(title),
        }
    }

    /// Adds scenario results.
    pub fn with_scenario_results(mut self, results: Vec<ScenarioResult>) -> Self {
        self.report.add_scenario_results(results);
        self
    }

    /// Adds a comparison suite.
    pub fn with_comparison_suite(mut self, suite: ComparisonSuite) -> Self {
        self.report.set_comparison_suite(suite);
        self
    }

    /// Adds system metadata.
    pub fn with_system_metadata<K, V>(mut self, key: K, value: V) -> Self
    where
        K: Into<String>,
        V: Into<String>,
    {
        self.report.system_info = self.report.system_info.with_metadata(key, value);
        self
    }

    /// Builds the report.
    pub fn build(self) -> BenchmarkReport {
        self.report
    }
}

// Dummy rustc version runtime helper (should be in Cargo.toml as dependency in real impl)
mod rustc_version_runtime {
    pub fn version() -> String {
        std::env::var("RUSTC_VERSION").unwrap_or_else(|_| "unknown".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_report_creation() {
        let report = BenchmarkReport::new("Test Report");
        assert_eq!(report.title, "Test Report");
        assert_eq!(report.summary.total_benchmarks, 0);
    }

    #[test]
    fn test_report_builder() {
        let report = ReportBuilder::new("Test Report")
            .with_system_metadata("custom_key", "custom_value")
            .build();

        assert_eq!(report.title, "Test Report");
        assert_eq!(
            report.system_info.metadata.get("custom_key"),
            Some(&"custom_value".to_string())
        );
    }

    #[test]
    fn test_summary_update() {
        let mut report = BenchmarkReport::new("Test");

        let result = ScenarioResult {
            name: "test1".to_string(),
            setup_duration: Duration::from_secs(1),
            execution_duration: Duration::from_secs(2),
            teardown_duration: Duration::from_secs(1),
            total_duration: Duration::from_secs(4),
            success: true,
            error_message: None,
            metrics: HashMap::new(),
        };

        report.add_scenario_results(vec![result]);

        assert_eq!(report.summary.total_benchmarks, 1);
        assert_eq!(report.summary.successful_benchmarks, 1);
        assert_eq!(report.summary.failed_benchmarks, 0);
    }
}
