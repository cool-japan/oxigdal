//! Performance profiling and monitoring.

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

/// Performance profiler for tracking operation durations.
pub struct Profiler {
    profiles: Arc<RwLock<HashMap<String, Vec<ProfileSample>>>>,
}

/// Profile sample with timing information.
#[derive(Debug, Clone)]
pub struct ProfileSample {
    /// Name of the profiled operation.
    pub operation: String,
    /// Duration of the operation in milliseconds.
    pub duration_ms: f64,
    /// Timestamp when the sample was recorded.
    pub timestamp: DateTime<Utc>,
    /// Additional metadata associated with the sample.
    pub metadata: HashMap<String, String>,
}

impl Profiler {
    /// Create a new profiler.
    pub fn new() -> Self {
        Self {
            profiles: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Start profiling an operation.
    pub fn start(&self, operation: impl Into<String>) -> ProfileGuard {
        ProfileGuard {
            operation: operation.into(),
            start: Instant::now(),
            profiler: self.profiles.clone(),
            metadata: HashMap::new(),
        }
    }

    /// Get profile samples for an operation.
    pub fn get_samples(&self, operation: &str) -> Vec<ProfileSample> {
        self.profiles
            .read()
            .get(operation)
            .cloned()
            .unwrap_or_default()
    }

    /// Get statistics for an operation.
    pub fn get_stats(&self, operation: &str) -> Option<ProfileStats> {
        let samples = self.get_samples(operation);
        if samples.is_empty() {
            return None;
        }

        let durations: Vec<f64> = samples.iter().map(|s| s.duration_ms).collect();
        let count = durations.len();
        let sum: f64 = durations.iter().sum();
        let mean = sum / count as f64;

        let variance: f64 =
            durations.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / count as f64;

        let std_dev = variance.sqrt();

        let mut sorted = durations.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let min = sorted[0];
        let max = sorted[count - 1];
        let p50 = sorted[count / 2];
        let p95 = sorted[(count * 95) / 100];
        let p99 = sorted[(count * 99) / 100];

        Some(ProfileStats {
            operation: operation.to_string(),
            count,
            mean,
            std_dev,
            min,
            max,
            p50,
            p95,
            p99,
        })
    }

    /// Clear all profile data.
    pub fn clear(&self) {
        self.profiles.write().clear();
    }
}

impl Default for Profiler {
    fn default() -> Self {
        Self::new()
    }
}

/// Guard for automatic profiling.
pub struct ProfileGuard {
    operation: String,
    start: Instant,
    profiler: Arc<RwLock<HashMap<String, Vec<ProfileSample>>>>,
    metadata: HashMap<String, String>,
}

impl ProfileGuard {
    /// Add metadata to the profile sample.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

impl Drop for ProfileGuard {
    fn drop(&mut self) {
        let duration = self.start.elapsed();
        let sample = ProfileSample {
            operation: self.operation.clone(),
            duration_ms: duration.as_secs_f64() * 1000.0,
            timestamp: Utc::now(),
            metadata: self.metadata.clone(),
        };

        let mut profiles = self.profiler.write();
        profiles
            .entry(self.operation.clone())
            .or_default()
            .push(sample);
    }
}

/// Profile statistics.
#[derive(Debug, Clone)]
pub struct ProfileStats {
    /// Name of the profiled operation.
    pub operation: String,
    /// Number of samples collected.
    pub count: usize,
    /// Mean duration in milliseconds.
    pub mean: f64,
    /// Standard deviation of durations.
    pub std_dev: f64,
    /// Minimum duration in milliseconds.
    pub min: f64,
    /// Maximum duration in milliseconds.
    pub max: f64,
    /// 50th percentile (median) duration.
    pub p50: f64,
    /// 95th percentile duration.
    pub p95: f64,
    /// 99th percentile duration.
    pub p99: f64,
}

impl std::fmt::Display for ProfileStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Operation: {}\n\
             Count: {}\n\
             Mean: {:.2}ms\n\
             StdDev: {:.2}ms\n\
             Min: {:.2}ms\n\
             Max: {:.2}ms\n\
             P50: {:.2}ms\n\
             P95: {:.2}ms\n\
             P99: {:.2}ms",
            self.operation,
            self.count,
            self.mean,
            self.std_dev,
            self.min,
            self.max,
            self.p50,
            self.p95,
            self.p99
        )
    }
}

/// Memory profiler for tracking allocations.
pub struct MemoryProfiler {
    snapshots: Arc<RwLock<Vec<MemorySnapshot>>>,
}

/// Memory snapshot.
#[derive(Debug, Clone)]
pub struct MemorySnapshot {
    /// Timestamp when the snapshot was taken.
    pub timestamp: DateTime<Utc>,
    /// Total bytes allocated at this point.
    pub allocated_bytes: u64,
    /// Total bytes deallocated at this point.
    pub deallocated_bytes: u64,
    /// Number of active allocations.
    pub active_allocations: u64,
    /// Operation associated with this snapshot.
    pub operation: String,
}

impl MemoryProfiler {
    /// Create a new memory profiler.
    pub fn new() -> Self {
        Self {
            snapshots: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Take a memory snapshot.
    pub fn snapshot(&self, operation: impl Into<String>) {
        // In production, this would use actual memory allocation tracking
        let snapshot = MemorySnapshot {
            timestamp: Utc::now(),
            allocated_bytes: 0,
            deallocated_bytes: 0,
            active_allocations: 0,
            operation: operation.into(),
        };

        self.snapshots.write().push(snapshot);
    }

    /// Get all snapshots.
    pub fn get_snapshots(&self) -> Vec<MemorySnapshot> {
        self.snapshots.read().clone()
    }

    /// Clear all snapshots.
    pub fn clear(&self) {
        self.snapshots.write().clear();
    }
}

impl Default for MemoryProfiler {
    fn default() -> Self {
        Self::new()
    }
}

/// CPU profiler for tracking CPU usage.
pub struct CpuProfiler {
    samples: Arc<RwLock<Vec<CpuSample>>>,
}

/// CPU usage sample.
#[derive(Debug, Clone)]
pub struct CpuSample {
    /// Timestamp when the sample was taken.
    pub timestamp: DateTime<Utc>,
    /// CPU usage percentage (0.0 to 100.0).
    pub cpu_percent: f64,
    /// Operation associated with this sample.
    pub operation: String,
}

impl CpuProfiler {
    /// Create a new CPU profiler.
    pub fn new() -> Self {
        Self {
            samples: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Record CPU usage.
    pub fn record(&self, operation: impl Into<String>, cpu_percent: f64) {
        let sample = CpuSample {
            timestamp: Utc::now(),
            cpu_percent,
            operation: operation.into(),
        };

        self.samples.write().push(sample);
    }

    /// Get all samples.
    pub fn get_samples(&self) -> Vec<CpuSample> {
        self.samples.read().clone()
    }

    /// Get average CPU usage for an operation.
    pub fn get_average(&self, operation: &str) -> Option<f64> {
        let samples: Vec<CpuSample> = self
            .samples
            .read()
            .iter()
            .filter(|s| s.operation == operation)
            .cloned()
            .collect();

        if samples.is_empty() {
            return None;
        }

        let sum: f64 = samples.iter().map(|s| s.cpu_percent).sum();
        Some(sum / samples.len() as f64)
    }
}

impl Default for CpuProfiler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_profiler() {
        let profiler = Profiler::new();

        {
            let _guard = profiler.start("test_operation");
            thread::sleep(Duration::from_millis(10));
        }

        let samples = profiler.get_samples("test_operation");
        assert_eq!(samples.len(), 1);
        assert!(samples[0].duration_ms >= 10.0);
    }

    #[test]
    fn test_profiler_stats() {
        let profiler = Profiler::new();

        for _ in 0..10 {
            let _guard = profiler.start("test_op");
            thread::sleep(Duration::from_millis(1));
        }

        let stats = profiler.get_stats("test_op");
        assert!(stats.is_some());

        let stats = stats.expect("No stats");
        assert_eq!(stats.count, 10);
        assert!(stats.mean > 0.0);
    }

    #[test]
    fn test_memory_profiler() {
        let profiler = MemoryProfiler::new();
        profiler.snapshot("test_operation");

        assert_eq!(profiler.get_snapshots().len(), 1);
    }

    #[test]
    fn test_cpu_profiler() {
        let profiler = CpuProfiler::new();
        profiler.record("test_op", 50.0);
        profiler.record("test_op", 60.0);

        let avg = profiler.get_average("test_op");
        assert_eq!(avg, Some(55.0));
    }
}
