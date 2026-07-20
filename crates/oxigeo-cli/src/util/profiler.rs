//! Performance profiler for geospatial operations.
//!
//! Provides `Profiler` (start/stop timing), `Operation` (file-dispatched actions),
//! and `execute_operation` (run N iterations and return durations) for the
//! `oxigeo profile` CLI subcommand.

use anyhow::{Context, Result};
use serde::Serialize;
use std::str::FromStr;
use std::time::{Duration, Instant};

// ─── Profiler ─────────────────────────────────────────────────────────────────

/// Accumulates wall-clock measurements for repeated operations.
///
/// Typical use:
/// ```rust,ignore
/// let mut p = Profiler::new("open");
/// p.start();
/// // ... do work ...
/// p.stop();
/// println!("{}", p.report());
/// ```
pub struct Profiler {
    name: String,
    measurements: Vec<Duration>,
    current_start: Option<Instant>,
}

impl Profiler {
    /// Create a new profiler with the given name.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            measurements: Vec::new(),
            current_start: None,
        }
    }

    /// Start a timing lap.
    ///
    /// Calling `start` when a lap is already running resets the start time.
    pub fn start(&mut self) {
        self.current_start = Some(Instant::now());
    }

    /// Stop the current timing lap and record the measurement.
    ///
    /// If `start` was never called, this is a no-op.
    pub fn stop(&mut self) {
        if let Some(start) = self.current_start.take() {
            self.measurements.push(start.elapsed());
        }
    }

    /// Returns a human-readable statistics table.
    ///
    /// Columns: count | min ms | mean ms | median ms | p95 ms | p99 ms | max ms
    #[must_use]
    pub fn report(&self) -> String {
        let n = self.measurements.len();
        if n == 0 {
            return format!("=== {} ===\nNo measurements recorded.\n", self.name);
        }

        let stats = compute_stats(&self.measurements);

        let header = format!("=== {} ({} iterations) ===", self.name, n);
        let row = format!(
            "count={n}  min={:.3}ms  mean={:.3}ms  median={:.3}ms  p95={:.3}ms  p99={:.3}ms  max={:.3}ms",
            stats.min_ms, stats.mean_ms, stats.median_ms, stats.p95_ms, stats.p99_ms, stats.max_ms,
        );
        format!("{header}\n{row}\n")
    }

    /// Serialise the statistics + raw measurements to a pretty-printed JSON string.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialisation fails (should not happen in practice).
    pub fn export_json(&self) -> Result<String> {
        let n = self.measurements.len();
        let measurements_ms: Vec<f64> = self.measurements.iter().map(duration_to_ms).collect();

        let payload = if n == 0 {
            ProfilerJson {
                name: self.name.clone(),
                count: 0,
                min_ms: 0.0,
                mean_ms: 0.0,
                median_ms: 0.0,
                p95_ms: 0.0,
                p99_ms: 0.0,
                max_ms: 0.0,
                measurements_ms,
            }
        } else {
            let stats = compute_stats(&self.measurements);
            ProfilerJson {
                name: self.name.clone(),
                count: n,
                min_ms: stats.min_ms,
                mean_ms: stats.mean_ms,
                median_ms: stats.median_ms,
                p95_ms: stats.p95_ms,
                p99_ms: stats.p99_ms,
                max_ms: stats.max_ms,
                measurements_ms,
            }
        };

        serde_json::to_string_pretty(&payload)
            .context("Failed to serialise profiler report to JSON")
    }
}

// ─── JSON export shape ─────────────────────────────────────────────────────────

#[derive(Serialize)]
struct ProfilerJson {
    name: String,
    count: usize,
    min_ms: f64,
    mean_ms: f64,
    median_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    max_ms: f64,
    measurements_ms: Vec<f64>,
}

// ─── Internal statistics ───────────────────────────────────────────────────────

struct Stats {
    min_ms: f64,
    mean_ms: f64,
    median_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    max_ms: f64,
}

fn duration_to_ms(d: &Duration) -> f64 {
    d.as_secs_f64() * 1_000.0
}

fn compute_stats(measurements: &[Duration]) -> Stats {
    let n = measurements.len();
    debug_assert!(n > 0, "compute_stats called with empty slice");

    let mut sorted: Vec<f64> = measurements.iter().map(duration_to_ms).collect();
    sorted.sort_by(|a, b| a.total_cmp(b));

    let min_ms = sorted[0];
    let max_ms = sorted[n - 1];
    let mean_ms = sorted.iter().sum::<f64>() / n as f64;
    let median_ms = percentile_from_sorted(&sorted, 50.0);
    let p95_ms = percentile_from_sorted(&sorted, 95.0);
    let p99_ms = percentile_from_sorted(&sorted, 99.0);

    Stats {
        min_ms,
        mean_ms,
        median_ms,
        p95_ms,
        p99_ms,
        max_ms,
    }
}

/// Compute the p-th percentile from a pre-sorted slice using linear interpolation.
fn percentile_from_sorted(sorted: &[f64], p: f64) -> f64 {
    let n = sorted.len();
    if n == 1 {
        return sorted[0];
    }
    // rank in [0, n-1] via the "index = p/100 * (n-1)" formula
    let rank = p / 100.0 * (n - 1) as f64;
    let lower = rank.floor() as usize;
    let upper = rank.ceil() as usize;
    if lower == upper {
        sorted[lower]
    } else {
        let frac = rank - lower as f64;
        sorted[lower] * (1.0 - frac) + sorted[upper] * frac
    }
}

// ─── Operation ────────────────────────────────────────────────────────────────

/// A geospatial operation that the profiler can benchmark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    /// Open the dataset (parse headers / verify magic bytes / read metadata).
    Open,
    /// Read all features from a vector dataset.
    ReadFeatures,
    /// Read raster band data (band 0 at overview level 0).
    ReadBands,
    /// Compute basic statistics by reading all data.
    Stats,
}

impl Operation {
    /// Execute the operation once against `input`.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened or read.
    pub fn execute(&self, input: &str) -> Result<()> {
        let path = std::path::Path::new(input);
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();

        match self {
            Self::Open => execute_open(input, &ext),
            Self::ReadFeatures => execute_read_features(input, &ext),
            Self::ReadBands => execute_read_bands(input, &ext),
            Self::Stats => execute_stats(input, &ext),
        }
    }
}

impl FromStr for Operation {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "open" | "open-dataset" | "opendataset" => Ok(Self::Open),
            "read-features" | "readfeatures" | "features" => Ok(Self::ReadFeatures),
            "read-bands" | "readbands" | "bands" => Ok(Self::ReadBands),
            "stats" | "compute-stats" | "computestats" => Ok(Self::Stats),
            other => anyhow::bail!(
                "Unknown operation: '{other}'. Valid options: open, read-features, read-bands, stats"
            ),
        }
    }
}

impl std::fmt::Display for Operation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Open => "open",
            Self::ReadFeatures => "read-features",
            Self::ReadBands => "read-bands",
            Self::Stats => "stats",
        };
        write!(f, "{name}")
    }
}

// ─── execute_operation ────────────────────────────────────────────────────────

/// Run `op` `iterations` times against `input` and return the per-iteration durations.
///
/// Superseded by `commands::profile::run_profiler` (which additionally feeds a
/// [`Profiler`] for report/JSON export), but kept as a lighter-weight public
/// helper for callers that only need raw per-iteration [`Duration`]s.
///
/// # Errors
///
/// Propagates any error that occurs during the first failing iteration.
#[allow(dead_code)]
pub fn execute_operation(op: &Operation, input: &str, iterations: usize) -> Result<Vec<Duration>> {
    let mut durations = Vec::with_capacity(iterations);
    for i in 0..iterations {
        let start = Instant::now();
        op.execute(input)
            .with_context(|| format!("Iteration {i} of operation '{op}' failed"))?;
        durations.push(start.elapsed());
    }
    Ok(durations)
}

// ─── Dispatch helpers ─────────────────────────────────────────────────────────

fn execute_open(input: &str, ext: &str) -> Result<()> {
    match ext {
        "tif" | "tiff" => {
            use oxigeo_core::io::FileDataSource;
            use oxigeo_geotiff::GeoTiffReader;
            let source = FileDataSource::open(input)
                .with_context(|| format!("Failed to open GeoTIFF: {input}"))?;
            let _reader = GeoTiffReader::open(source)
                .with_context(|| format!("Failed to parse GeoTIFF header: {input}"))?;
            Ok(())
        }
        "geojson" | "json" => {
            use oxigeo_geojson::GeoJsonReader;
            use std::fs::File;
            use std::io::BufReader;
            let file =
                File::open(input).with_context(|| format!("Failed to open GeoJSON: {input}"))?;
            let _reader = GeoJsonReader::new(BufReader::new(file));
            Ok(())
        }
        "fgb" => {
            use oxigeo_flatgeobuf::FlatGeobufReader;
            use std::fs::File;
            let file =
                File::open(input).with_context(|| format!("Failed to open FlatGeobuf: {input}"))?;
            let _reader = FlatGeobufReader::new(file)
                .with_context(|| format!("Failed to parse FlatGeobuf header: {input}"))?;
            Ok(())
        }
        other => anyhow::bail!(
            "Unsupported file extension for 'open' operation: '{other}'. \
             Supported: tif, tiff, geojson, json, fgb"
        ),
    }
}

fn execute_read_features(input: &str, ext: &str) -> Result<()> {
    match ext {
        "geojson" | "json" => {
            use oxigeo_geojson::GeoJsonReader;
            use std::fs::File;
            use std::io::BufReader;
            let file =
                File::open(input).with_context(|| format!("Failed to open GeoJSON: {input}"))?;
            let mut reader = GeoJsonReader::new(BufReader::new(file));
            let _fc = reader
                .read_feature_collection()
                .with_context(|| format!("Failed to read features from {input}"))?;
            Ok(())
        }
        "fgb" => {
            use oxigeo_flatgeobuf::FlatGeobufReader;
            use std::fs::File;
            let file =
                File::open(input).with_context(|| format!("Failed to open FlatGeobuf: {input}"))?;
            let mut reader = FlatGeobufReader::new(file)
                .with_context(|| format!("Failed to parse FlatGeobuf: {input}"))?;
            let mut iter = reader
                .features()
                .with_context(|| format!("Failed to iterate features from {input}"))?;
            while iter.next().is_some() {}
            Ok(())
        }
        other => anyhow::bail!(
            "Unsupported file extension for 'read-features' operation: '{other}'. \
             Supported: geojson, json, fgb"
        ),
    }
}

fn execute_read_bands(input: &str, ext: &str) -> Result<()> {
    match ext {
        "tif" | "tiff" => {
            use oxigeo_core::io::FileDataSource;
            use oxigeo_geotiff::GeoTiffReader;
            let source = FileDataSource::open(input)
                .with_context(|| format!("Failed to open GeoTIFF: {input}"))?;
            let reader = GeoTiffReader::open(source)
                .with_context(|| format!("Failed to parse GeoTIFF: {input}"))?;
            let _data = reader
                .read_band(0, 0)
                .with_context(|| format!("Failed to read band from {input}"))?;
            Ok(())
        }
        other => anyhow::bail!(
            "Unsupported file extension for 'read-bands' operation: '{other}'. \
             Supported: tif, tiff"
        ),
    }
}

fn execute_stats(input: &str, ext: &str) -> Result<()> {
    match ext {
        "tif" | "tiff" => execute_read_bands(input, ext),
        "geojson" | "json" | "fgb" => execute_read_features(input, ext),
        other => anyhow::bail!(
            "Unsupported file extension for 'stats' operation: '{other}'. \
             Supported: tif, tiff, geojson, json, fgb"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profiler_no_measurements() {
        let p = Profiler::new("test");
        let report = p.report();
        assert!(
            report.contains("No measurements"),
            "empty profiler should say no measurements"
        );
    }

    #[test]
    fn test_profiler_single_measurement() {
        let mut p = Profiler::new("single");
        p.start();
        std::thread::sleep(Duration::from_millis(5));
        p.stop();
        let report = p.report();
        assert!(report.contains("count=1"), "report should show count=1");
        assert!(
            report.contains("single"),
            "report should include profiler name"
        );
    }

    #[test]
    fn test_profiler_export_json_empty() {
        let p = Profiler::new("empty");
        let json = p.export_json().expect("export_json should not fail");
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(v["name"], "empty");
        assert_eq!(v["count"], 0);
        assert!(
            v["measurements_ms"]
                .as_array()
                .map(|a| a.is_empty())
                .unwrap_or(false),
            "measurements_ms should be empty"
        );
    }

    #[test]
    fn test_profiler_export_json_with_data() {
        let mut p = Profiler::new("json_test");
        for _ in 0..3 {
            p.start();
            std::thread::sleep(Duration::from_millis(2));
            p.stop();
        }
        let json = p.export_json().expect("export_json should not fail");
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(v["count"], 3);
        assert_eq!(
            v["measurements_ms"]
                .as_array()
                .map(|a| a.len())
                .unwrap_or(0),
            3
        );
        assert!(v["min_ms"].as_f64().unwrap_or(0.0) > 0.0);
        assert!(v["max_ms"].as_f64().unwrap_or(0.0) >= v["min_ms"].as_f64().unwrap_or(0.0));
    }

    #[test]
    fn test_operation_from_str_valid() {
        assert_eq!("open".parse::<Operation>().ok(), Some(Operation::Open));
        assert_eq!(
            "read-features".parse::<Operation>().ok(),
            Some(Operation::ReadFeatures)
        );
        assert_eq!(
            "read-bands".parse::<Operation>().ok(),
            Some(Operation::ReadBands)
        );
        assert_eq!("stats".parse::<Operation>().ok(), Some(Operation::Stats));
        // Aliases
        assert_eq!("OPEN".parse::<Operation>().ok(), Some(Operation::Open));
        assert_eq!(
            "features".parse::<Operation>().ok(),
            Some(Operation::ReadFeatures)
        );
    }

    #[test]
    fn test_operation_from_str_invalid() {
        let result = "unknown_op".parse::<Operation>();
        assert!(result.is_err(), "unknown operation should return Err");
        let err_msg = result
            .expect_err("parsing an unknown operation must fail")
            .to_string();
        assert!(
            err_msg.contains("Unknown operation"),
            "error should mention 'Unknown operation'"
        );
    }

    #[test]
    fn test_percentile_from_sorted_single() {
        let data = vec![42.0f64];
        assert!((percentile_from_sorted(&data, 50.0) - 42.0).abs() < 1e-10);
        assert!((percentile_from_sorted(&data, 95.0) - 42.0).abs() < 1e-10);
    }

    #[test]
    fn test_percentile_from_sorted_multiple() {
        let data: Vec<f64> = (1..=10).map(|x| x as f64).collect();
        let median = percentile_from_sorted(&data, 50.0);
        // median of [1..10] should be 5.5
        assert!(
            (median - 5.5).abs() < 1e-10,
            "median should be 5.5, got {median}"
        );
        let min = percentile_from_sorted(&data, 0.0);
        assert!((min - 1.0).abs() < 1e-10);
        let max = percentile_from_sorted(&data, 100.0);
        assert!((max - 10.0).abs() < 1e-10);
    }

    #[test]
    fn test_stop_without_start_is_noop() {
        let mut p = Profiler::new("noop");
        p.stop(); // should not panic
        assert_eq!(p.measurements.len(), 0);
    }
}
