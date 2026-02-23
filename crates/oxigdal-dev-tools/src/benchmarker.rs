//! Quick benchmarking utilities
//!
//! This module provides simple benchmarking tools for OxiGDAL operations.

use crate::Result;
use chrono::{DateTime, Duration, Utc};
use colored::Colorize;
use comfy_table::{Cell, CellAlignment, Row, Table};
use serde::{Deserialize, Serialize};

/// Quick benchmarker
pub struct Benchmarker {
    /// Benchmark results
    results: Vec<BenchmarkResult>,
    /// Warmup iterations
    warmup_iterations: usize,
    /// Benchmark iterations
    iterations: usize,
}

/// Benchmark result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    /// Benchmark name
    pub name: String,
    /// Number of iterations
    pub iterations: usize,
    /// Total duration in milliseconds
    pub total_ms: i64,
    /// Average duration in milliseconds
    pub avg_ms: f64,
    /// Minimum duration in milliseconds
    pub min_ms: i64,
    /// Maximum duration in milliseconds
    pub max_ms: i64,
    /// Standard deviation in milliseconds
    pub std_dev_ms: f64,
    /// Operations per second
    pub ops_per_sec: f64,
}

impl Benchmarker {
    /// Create a new benchmarker
    pub fn new() -> Self {
        Self {
            results: Vec::new(),
            warmup_iterations: 3,
            iterations: 10,
        }
    }

    /// Set warmup iterations
    pub fn with_warmup(mut self, warmup: usize) -> Self {
        self.warmup_iterations = warmup;
        self
    }

    /// Set benchmark iterations
    pub fn with_iterations(mut self, iterations: usize) -> Self {
        self.iterations = iterations;
        self
    }

    /// Benchmark a function
    pub fn bench<F>(&mut self, name: impl Into<String>, mut f: F) -> Result<BenchmarkResult>
    where
        F: FnMut() -> Result<()>,
    {
        let name = name.into();

        // Warmup
        for _ in 0..self.warmup_iterations {
            f()?;
        }

        // Benchmark
        let mut durations = Vec::with_capacity(self.iterations);

        for _ in 0..self.iterations {
            let start = Utc::now();
            f()?;
            let end = Utc::now();

            let duration = end.signed_duration_since(start);
            durations.push(duration.num_milliseconds());
        }

        // Calculate statistics
        let total_ms: i64 = durations.iter().sum();
        let avg_ms = total_ms as f64 / self.iterations as f64;
        let min_ms = *durations.iter().min().unwrap_or(&0);
        let max_ms = *durations.iter().max().unwrap_or(&0);

        let variance = durations
            .iter()
            .map(|&d| {
                let diff = d as f64 - avg_ms;
                diff * diff
            })
            .sum::<f64>()
            / self.iterations as f64;
        let std_dev_ms = variance.sqrt();

        let ops_per_sec = if avg_ms > 0.0 {
            1000.0 / avg_ms
        } else {
            f64::INFINITY
        };

        let result = BenchmarkResult {
            name: name.clone(),
            iterations: self.iterations,
            total_ms,
            avg_ms,
            min_ms,
            max_ms,
            std_dev_ms,
            ops_per_sec,
        };

        self.results.push(result.clone());

        Ok(result)
    }

    /// Get all results
    pub fn results(&self) -> &[BenchmarkResult] {
        &self.results
    }

    /// Clear results
    pub fn clear(&mut self) {
        self.results.clear();
    }

    /// Generate report
    pub fn report(&self) -> String {
        let mut report = String::new();
        report.push_str(&format!("\n{}\n", "Benchmark Report".bold()));
        report.push_str(&format!("{}\n\n", "=".repeat(80)));

        if self.results.is_empty() {
            report.push_str("No benchmark results\n");
            return report;
        }

        let mut table = Table::new();
        table.set_header(Row::from(vec![
            Cell::new("Benchmark").set_alignment(CellAlignment::Left),
            Cell::new("Iterations").set_alignment(CellAlignment::Right),
            Cell::new("Avg (ms)").set_alignment(CellAlignment::Right),
            Cell::new("Min (ms)").set_alignment(CellAlignment::Right),
            Cell::new("Max (ms)").set_alignment(CellAlignment::Right),
            Cell::new("Std Dev").set_alignment(CellAlignment::Right),
            Cell::new("Ops/sec").set_alignment(CellAlignment::Right),
        ]));

        for result in &self.results {
            table.add_row(Row::from(vec![
                Cell::new(&result.name),
                Cell::new(format!("{}", result.iterations)),
                Cell::new(format!("{:.3}", result.avg_ms)),
                Cell::new(format!("{}", result.min_ms)),
                Cell::new(format!("{}", result.max_ms)),
                Cell::new(format!("{:.3}", result.std_dev_ms)),
                Cell::new(format!("{:.2}", result.ops_per_sec)),
            ]));
        }

        report.push_str(&table.to_string());
        report.push('\n');

        report
    }

    /// Export results as JSON
    pub fn export_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(&self.results)?)
    }

    /// Compare two benchmarks
    pub fn compare(&self, name1: &str, name2: &str) -> Option<Comparison> {
        let result1 = self.results.iter().find(|r| r.name == name1)?;
        let result2 = self.results.iter().find(|r| r.name == name2)?;

        Some(Comparison {
            name1: result1.name.clone(),
            name2: result2.name.clone(),
            speedup: result2.avg_ms / result1.avg_ms,
            diff_ms: result1.avg_ms - result2.avg_ms,
            faster: if result1.avg_ms < result2.avg_ms {
                result1.name.clone()
            } else {
                result2.name.clone()
            },
        })
    }
}

impl Default for Benchmarker {
    fn default() -> Self {
        Self::new()
    }
}

/// Benchmark comparison
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comparison {
    /// First benchmark name
    pub name1: String,
    /// Second benchmark name
    pub name2: String,
    /// Speedup factor
    pub speedup: f64,
    /// Difference in milliseconds
    pub diff_ms: f64,
    /// Which is faster
    pub faster: String,
}

impl Comparison {
    /// Format comparison
    pub fn format(&self) -> String {
        format!(
            "{} is {:.2}x {} than {} ({:.3} ms difference)",
            self.faster,
            self.speedup.abs(),
            if self.speedup > 1.0 {
                "faster"
            } else {
                "slower"
            },
            if self.faster == self.name1 {
                &self.name2
            } else {
                &self.name1
            },
            self.diff_ms.abs()
        )
    }
}

/// Simple timer for quick measurements
pub struct Timer {
    /// Timer name
    name: String,
    /// Start time
    start: DateTime<Utc>,
}

impl Timer {
    /// Create and start a new timer
    pub fn start(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            start: Utc::now(),
        }
    }

    /// Stop timer and return elapsed duration
    pub fn stop(self) -> TimerResult {
        let elapsed = Utc::now().signed_duration_since(self.start);
        TimerResult {
            name: self.name,
            duration: elapsed,
        }
    }

    /// Get elapsed duration without stopping
    pub fn elapsed(&self) -> Duration {
        Utc::now().signed_duration_since(self.start)
    }
}

/// Timer result
pub struct TimerResult {
    /// Timer name
    name: String,
    /// Duration
    duration: Duration,
}

impl TimerResult {
    /// Get duration in milliseconds
    pub fn milliseconds(&self) -> i64 {
        self.duration.num_milliseconds()
    }

    /// Get duration in microseconds
    pub fn microseconds(&self) -> i64 {
        self.duration.num_microseconds().unwrap_or(0)
    }

    /// Format result
    pub fn format(&self) -> String {
        let ms = self.milliseconds();
        if ms > 1000 {
            format!("{}: {:.2} s", self.name, ms as f64 / 1000.0)
        } else if ms > 0 {
            format!("{}: {} ms", self.name, ms)
        } else {
            format!("{}: {} µs", self.name, self.microseconds())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration as StdDuration;

    #[test]
    fn test_benchmarker_creation() {
        let bench = Benchmarker::new();
        assert_eq!(bench.iterations, 10);
        assert_eq!(bench.warmup_iterations, 3);
    }

    #[test]
    fn test_benchmarker_config() {
        let bench = Benchmarker::new().with_warmup(5).with_iterations(20);
        assert_eq!(bench.warmup_iterations, 5);
        assert_eq!(bench.iterations, 20);
    }

    #[test]
    fn test_benchmark_simple() -> Result<()> {
        let mut bench = Benchmarker::new().with_warmup(1).with_iterations(5);

        let result = bench.bench("test", || {
            thread::sleep(StdDuration::from_millis(10));
            Ok(())
        })?;

        assert_eq!(result.name, "test");
        assert_eq!(result.iterations, 5);
        assert!(result.avg_ms >= 10.0);

        Ok(())
    }

    #[test]
    fn test_benchmarker_results() -> Result<()> {
        let mut bench = Benchmarker::new().with_iterations(5);

        bench.bench("test1", || Ok(()))?;
        bench.bench("test2", || Ok(()))?;

        assert_eq!(bench.results().len(), 2);

        Ok(())
    }

    #[test]
    fn test_timer() {
        let timer = Timer::start("test");
        thread::sleep(StdDuration::from_millis(50));
        let result = timer.stop();

        assert!(result.milliseconds() >= 50);
    }

    #[test]
    fn test_timer_elapsed() {
        let timer = Timer::start("test");
        thread::sleep(StdDuration::from_millis(10));
        let elapsed = timer.elapsed();

        assert!(elapsed.num_milliseconds() >= 10);
    }

    #[test]
    fn test_comparison() -> Result<()> {
        let mut bench = Benchmarker::new().with_warmup(1).with_iterations(3);

        bench.bench("fast", || {
            thread::sleep(StdDuration::from_millis(10));
            Ok(())
        })?;

        bench.bench("slow", || {
            thread::sleep(StdDuration::from_millis(20));
            Ok(())
        })?;

        let comp = bench.compare("fast", "slow");
        assert!(comp.is_some());

        if let Some(c) = comp {
            assert_eq!(c.faster, "fast");
        }

        Ok(())
    }

    #[test]
    fn test_export_json() -> Result<()> {
        let mut bench = Benchmarker::new().with_iterations(2);
        bench.bench("test", || Ok(()))?;

        let json = bench.export_json()?;
        assert!(json.contains("test"));
        assert!(json.contains("iterations"));

        Ok(())
    }
}
