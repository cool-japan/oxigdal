//! Profiling utilities for CPU and memory profiling.
//!
//! This module provides comprehensive profiling capabilities including:
//! - CPU profiling with flamegraph generation
//! - Memory profiling and leak detection
//! - Performance metrics collection
//! - System resource monitoring

use crate::error::{BenchError, Result};
use pprof::ProfilerGuard;
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use sysinfo::System;

/// CPU profiler configuration.
#[derive(Debug, Clone)]
pub struct CpuProfilerConfig {
    /// Sampling frequency (samples per second).
    pub frequency: i32,
    /// Whether to generate flamegraph.
    pub generate_flamegraph: bool,
    /// Output directory for profiling data.
    pub output_dir: PathBuf,
    /// Flamegraph filename.
    pub flamegraph_name: String,
}

impl Default for CpuProfilerConfig {
    fn default() -> Self {
        Self {
            frequency: 100,
            generate_flamegraph: true,
            output_dir: PathBuf::from("target/profiling"),
            flamegraph_name: String::from("flamegraph.svg"),
        }
    }
}

/// CPU profiler with flamegraph generation support.
pub struct CpuProfiler {
    config: CpuProfilerConfig,
    guard: Option<ProfilerGuard<'static>>,
    start_time: Option<Instant>,
}

impl CpuProfiler {
    /// Creates a new CPU profiler with the given configuration.
    pub fn new(config: CpuProfilerConfig) -> Self {
        Self {
            config,
            guard: None,
            start_time: None,
        }
    }

    /// Creates a new CPU profiler with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(CpuProfilerConfig::default())
    }

    /// Starts CPU profiling.
    pub fn start(&mut self) -> Result<()> {
        if self.guard.is_some() {
            return Err(BenchError::profiler_start("Profiler already running"));
        }

        let guard = ProfilerGuard::new(self.config.frequency)
            .map_err(|e| BenchError::profiler_start(format!("Failed to start profiler: {e}")))?;

        self.guard = Some(guard);
        self.start_time = Some(Instant::now());

        Ok(())
    }

    /// Stops CPU profiling and generates flamegraph if configured.
    pub fn stop(&mut self) -> Result<CpuProfilerReport> {
        let guard = self
            .guard
            .take()
            .ok_or_else(|| BenchError::profiler_stop("Profiler not running"))?;

        let duration = self
            .start_time
            .take()
            .map(|start| start.elapsed())
            .ok_or_else(|| BenchError::profiler_stop("Start time not recorded"))?;

        let report = guard.report().build().map_err(|e| {
            BenchError::profiler_stop(format!("Failed to build profiling report: {e}"))
        })?;

        let mut flamegraph_path = None;

        if self.config.generate_flamegraph {
            std::fs::create_dir_all(&self.config.output_dir).map_err(|e| {
                BenchError::Flamegraph(format!("Failed to create output directory: {e}"))
            })?;

            let path = self.config.output_dir.join(&self.config.flamegraph_name);
            let file = File::create(&path)
                .map_err(|e| BenchError::Flamegraph(format!("Failed to create file: {e}")))?;

            report.flamegraph(file).map_err(|e| {
                BenchError::Flamegraph(format!("Failed to generate flamegraph: {e}"))
            })?;

            flamegraph_path = Some(path);
        }

        Ok(CpuProfilerReport {
            duration,
            flamegraph_path,
            sample_count: report.data.len(),
        })
    }
}

/// CPU profiler report.
#[derive(Debug, Clone)]
pub struct CpuProfilerReport {
    /// Total profiling duration.
    pub duration: Duration,
    /// Path to generated flamegraph (if any).
    pub flamegraph_path: Option<PathBuf>,
    /// Number of samples collected.
    pub sample_count: usize,
}

/// Memory profiler configuration.
#[derive(Debug, Clone)]
pub struct MemoryProfilerConfig {
    /// Sample interval in milliseconds.
    pub sample_interval_ms: u64,
    /// Whether to track allocations.
    pub track_allocations: bool,
    /// Output directory for profiling data.
    pub output_dir: PathBuf,
}

impl Default for MemoryProfilerConfig {
    fn default() -> Self {
        Self {
            sample_interval_ms: 100,
            track_allocations: true,
            output_dir: PathBuf::from("target/profiling"),
        }
    }
}

/// Memory usage snapshot.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemorySnapshot {
    /// Timestamp of the snapshot.
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Physical memory usage in bytes.
    pub physical_memory: u64,
    /// Virtual memory usage in bytes.
    pub virtual_memory: u64,
    /// Resident set size in bytes.
    pub rss: u64,
}

/// Memory profiler for tracking memory usage over time.
pub struct MemoryProfiler {
    config: MemoryProfilerConfig,
    snapshots: Vec<MemorySnapshot>,
    start_time: Option<Instant>,
    system: System,
}

impl MemoryProfiler {
    /// Creates a new memory profiler with the given configuration.
    pub fn new(config: MemoryProfilerConfig) -> Self {
        Self {
            config,
            snapshots: Vec::new(),
            start_time: None,
            system: System::new_all(),
        }
    }

    /// Creates a new memory profiler with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(MemoryProfilerConfig::default())
    }

    /// Takes a memory snapshot.
    pub fn snapshot(&mut self) -> Result<MemorySnapshot> {
        self.system.refresh_memory_specifics(Default::default());

        // Refresh all processes
        self.system.refresh_all();

        let pid = sysinfo::get_current_pid()
            .map_err(|e| BenchError::MemoryProfiling(format!("Failed to get current PID: {e}")))?;

        let process = self.system.process(pid).ok_or_else(|| {
            BenchError::MemoryProfiling("Failed to get process information".to_string())
        })?;

        let snapshot = MemorySnapshot {
            timestamp: chrono::Utc::now(),
            physical_memory: process.memory(),
            virtual_memory: process.virtual_memory(),
            rss: process.memory(),
        };

        self.snapshots.push(snapshot.clone());

        Ok(snapshot)
    }

    /// Starts memory profiling.
    pub fn start(&mut self) -> Result<()> {
        if self.start_time.is_some() {
            return Err(BenchError::MemoryProfiling(
                "Profiler already running".to_string(),
            ));
        }

        self.snapshots.clear();
        self.start_time = Some(Instant::now());
        self.snapshot()?;

        Ok(())
    }

    /// Stops memory profiling and returns a report.
    pub fn stop(&mut self) -> Result<MemoryProfilerReport> {
        let start_time = self
            .start_time
            .take()
            .ok_or_else(|| BenchError::MemoryProfiling("Profiler not running".to_string()))?;

        self.snapshot()?;

        let duration = start_time.elapsed();
        let snapshots = std::mem::take(&mut self.snapshots);

        let peak_memory = snapshots
            .iter()
            .map(|s| s.physical_memory)
            .max()
            .unwrap_or(0);

        let avg_memory = if snapshots.is_empty() {
            0
        } else {
            snapshots.iter().map(|s| s.physical_memory).sum::<u64>() / snapshots.len() as u64
        };

        let memory_growth = if snapshots.len() >= 2 {
            let first = &snapshots[0];
            let last = &snapshots[snapshots.len() - 1];
            last.physical_memory.saturating_sub(first.physical_memory)
        } else {
            0
        };

        Ok(MemoryProfilerReport {
            duration,
            peak_memory,
            avg_memory,
            memory_growth,
            snapshots,
        })
    }

    /// Gets all collected snapshots.
    pub fn snapshots(&self) -> &[MemorySnapshot] {
        &self.snapshots
    }

    /// Gets the profiler configuration.
    pub fn config(&self) -> &MemoryProfilerConfig {
        &self.config
    }
}

/// Memory profiler report.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryProfilerReport {
    /// Total profiling duration.
    #[serde(with = "duration_serde")]
    pub duration: Duration,
    /// Peak memory usage in bytes.
    pub peak_memory: u64,
    /// Average memory usage in bytes.
    pub avg_memory: u64,
    /// Memory growth during profiling in bytes.
    pub memory_growth: u64,
    /// All collected snapshots.
    pub snapshots: Vec<MemorySnapshot>,
}

/// Serde serialization helpers for `std::time::Duration`.
///
/// This module provides custom serialization and deserialization for `Duration`
/// values, converting them to/from floating-point seconds for JSON compatibility.
pub mod duration_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    /// Serializes a `Duration` as a floating-point number of seconds.
    ///
    /// This allows `Duration` values to be serialized as simple numbers in JSON
    /// rather than as complex structures.
    pub fn serialize<S>(duration: &Duration, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        duration.as_secs_f64().serialize(serializer)
    }

    /// Deserializes a floating-point number of seconds into a `Duration`.
    ///
    /// This is the counterpart to `serialize`, reconstructing a `Duration`
    /// from a floating-point seconds value.
    pub fn deserialize<'de, D>(deserializer: D) -> std::result::Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = f64::deserialize(deserializer)?;
        Ok(Duration::from_secs_f64(secs))
    }
}

impl MemoryProfilerReport {
    /// Saves the report to a JSON file.
    pub fn save_json<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let file = File::create(path.as_ref())?;
        serde_json::to_writer_pretty(file, self)?;
        Ok(())
    }

    /// Loads a report from a JSON file.
    pub fn load_json<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path.as_ref())?;
        let report = serde_json::from_reader(file)?;
        Ok(report)
    }
}

/// System resource monitor for tracking CPU, memory, and other system metrics.
pub struct SystemMonitor {
    system: System,
    metrics: Vec<SystemMetrics>,
}

/// System metrics snapshot.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SystemMetrics {
    /// Timestamp of the snapshot.
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Total CPU usage percentage (0-100 * number of cores).
    pub cpu_usage: f32,
    /// Total memory in bytes.
    pub total_memory: u64,
    /// Used memory in bytes.
    pub used_memory: u64,
    /// Available memory in bytes.
    pub available_memory: u64,
    /// Number of processes.
    pub process_count: usize,
    /// Additional custom metrics.
    pub custom_metrics: HashMap<String, f64>,
}

impl SystemMonitor {
    /// Creates a new system monitor.
    pub fn new() -> Self {
        Self {
            system: System::new_all(),
            metrics: Vec::new(),
        }
    }

    /// Takes a system metrics snapshot.
    pub fn snapshot(&mut self) -> SystemMetrics {
        self.system.refresh_all();

        let metrics = SystemMetrics {
            timestamp: chrono::Utc::now(),
            cpu_usage: self.system.global_cpu_usage(),
            total_memory: self.system.total_memory(),
            used_memory: self.system.used_memory(),
            available_memory: self.system.available_memory(),
            process_count: self.system.processes().len(),
            custom_metrics: HashMap::new(),
        };

        self.metrics.push(metrics.clone());
        metrics
    }

    /// Gets all collected metrics.
    pub fn metrics(&self) -> &[SystemMetrics] {
        &self.metrics
    }

    /// Clears all collected metrics.
    pub fn clear(&mut self) {
        self.metrics.clear();
    }
}

impl Default for SystemMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Profile a function with CPU profiling.
pub fn profile_cpu<F, R>(func: F, config: CpuProfilerConfig) -> Result<(R, CpuProfilerReport)>
where
    F: FnOnce() -> R,
{
    let mut profiler = CpuProfiler::new(config);
    profiler.start()?;
    let result = func();
    let report = profiler.stop()?;
    Ok((result, report))
}

/// Profile a function with memory profiling.
pub fn profile_memory<F, R>(
    func: F,
    config: MemoryProfilerConfig,
) -> Result<(R, MemoryProfilerReport)>
where
    F: FnOnce() -> R,
{
    let mut profiler = MemoryProfiler::new(config);
    profiler.start()?;
    let result = func();
    let report = profiler.stop()?;
    Ok((result, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_profiler_config_default() {
        let config = CpuProfilerConfig::default();
        assert_eq!(config.frequency, 100);
        assert!(config.generate_flamegraph);
    }

    #[test]
    fn test_memory_profiler_snapshot() {
        let mut profiler = MemoryProfiler::with_defaults();
        let snapshot = profiler.snapshot();
        assert!(snapshot.is_ok());
        let snapshot = snapshot.expect("Failed to take snapshot");
        assert!(snapshot.physical_memory > 0);
    }

    #[test]
    fn test_system_monitor() {
        let mut monitor = SystemMonitor::new();
        let metrics = monitor.snapshot();
        assert!(metrics.total_memory > 0);
        assert!(metrics.process_count > 0);
    }

    #[test]
    fn test_memory_profiler_report_serialization() {
        let report = MemoryProfilerReport {
            duration: Duration::from_secs(5),
            peak_memory: 1024 * 1024,
            avg_memory: 512 * 1024,
            memory_growth: 256 * 1024,
            snapshots: vec![],
        };

        let json = serde_json::to_string(&report);
        assert!(json.is_ok());

        let deserialized: std::result::Result<MemoryProfilerReport, _> =
            serde_json::from_str(&json.expect("Failed to serialize"));
        assert!(deserialized.is_ok());
    }

    #[test]
    fn test_profile_cpu() {
        let config = CpuProfilerConfig {
            generate_flamegraph: false,
            ..Default::default()
        };

        let result = profile_cpu(|| 42, config);
        assert!(result.is_ok());
        let (value, _report) = result.expect("Failed to profile");
        assert_eq!(value, 42);
    }

    #[test]
    fn test_profile_memory() {
        let config = MemoryProfilerConfig::default();
        let result = profile_memory(|| 42, config);
        assert!(result.is_ok());
        let (value, _report) = result.expect("Failed to profile");
        assert_eq!(value, 42);
    }
}
