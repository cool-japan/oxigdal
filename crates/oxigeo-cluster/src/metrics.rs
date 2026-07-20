//! Metrics collection and monitoring for the cluster.
//!
//! This module provides comprehensive metrics collection for cluster operations
//! including task throughput, latency, resource utilization, and worker efficiency.

use crate::error::Result;
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Metrics collector for the cluster.
#[derive(Clone)]
pub struct ClusterMetrics {
    inner: Arc<ClusterMetricsInner>,
}

struct ClusterMetricsInner {
    /// Task metrics
    tasks_submitted: AtomicU64,
    tasks_completed: AtomicU64,
    tasks_failed: AtomicU64,
    tasks_cancelled: AtomicU64,
    tasks_retried: AtomicU64,

    /// Latency metrics (in microseconds)
    total_scheduling_latency_us: AtomicU64,
    total_execution_latency_us: AtomicU64,
    total_queue_latency_us: AtomicU64,

    /// Resource metrics
    active_workers: AtomicUsize,
    total_cpu_cores: AtomicUsize,
    used_cpu_cores: AtomicUsize,
    total_memory_bytes: AtomicU64,
    used_memory_bytes: AtomicU64,

    /// Network metrics
    bytes_sent: AtomicU64,
    bytes_received: AtomicU64,
    network_errors: AtomicU64,

    /// Cache metrics
    cache_hits: AtomicU64,
    cache_misses: AtomicU64,
    cache_evictions: AtomicU64,

    /// Queue metrics
    queue_depth: AtomicUsize,
    max_queue_depth: AtomicUsize,

    /// Worker metrics
    worker_metrics: RwLock<HashMap<String, WorkerMetrics>>,

    /// Task type metrics
    task_type_metrics: RwLock<HashMap<String, TaskTypeMetrics>>,

    /// Start time
    start_time: Instant,
}

/// Metrics for individual workers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerMetrics {
    /// Worker ID
    pub worker_id: String,

    /// Tasks completed
    pub tasks_completed: u64,

    /// Tasks failed
    pub tasks_failed: u64,

    /// CPU utilization (0.0-1.0)
    pub cpu_utilization: f64,

    /// Memory utilization (0.0-1.0)
    pub memory_utilization: f64,

    /// Network sent (bytes)
    pub network_sent: u64,

    /// Network received (bytes)
    pub network_received: u64,

    /// Last heartbeat time
    pub last_heartbeat: DateTime<Utc>,

    /// Worker uptime
    pub uptime: Duration,
}

/// Metrics for task types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskTypeMetrics {
    /// Task type name
    pub task_type: String,

    /// Total submitted
    pub submitted: u64,

    /// Total completed
    pub completed: u64,

    /// Total failed
    pub failed: u64,

    /// Average execution time
    pub avg_execution_time_ms: f64,

    /// Min execution time
    pub min_execution_time_ms: f64,

    /// Max execution time
    pub max_execution_time_ms: f64,
}

impl Default for TaskTypeMetrics {
    fn default() -> Self {
        Self {
            task_type: String::new(),
            submitted: 0,
            completed: 0,
            failed: 0,
            avg_execution_time_ms: 0.0,
            min_execution_time_ms: f64::MAX,
            max_execution_time_ms: 0.0,
        }
    }
}

/// Snapshot of cluster metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    /// Tasks submitted
    pub tasks_submitted: u64,

    /// Tasks completed
    pub tasks_completed: u64,

    /// Tasks failed
    pub tasks_failed: u64,

    /// Tasks cancelled
    pub tasks_cancelled: u64,

    /// Tasks retried
    pub tasks_retried: u64,

    /// Average scheduling latency (ms)
    pub avg_scheduling_latency_ms: f64,

    /// Average execution latency (ms)
    pub avg_execution_latency_ms: f64,

    /// Average queue latency (ms)
    pub avg_queue_latency_ms: f64,

    /// Active workers
    pub active_workers: usize,

    /// Total CPU cores
    pub total_cpu_cores: usize,

    /// Used CPU cores
    pub used_cpu_cores: usize,

    /// CPU utilization (0.0-1.0)
    pub cpu_utilization: f64,

    /// Total memory (bytes)
    pub total_memory_bytes: u64,

    /// Used memory (bytes)
    pub used_memory_bytes: u64,

    /// Memory utilization (0.0-1.0)
    pub memory_utilization: f64,

    /// Bytes sent
    pub bytes_sent: u64,

    /// Bytes received
    pub bytes_received: u64,

    /// Network errors
    pub network_errors: u64,

    /// Cache hits
    pub cache_hits: u64,

    /// Cache misses
    pub cache_misses: u64,

    /// Cache hit rate (0.0-1.0)
    pub cache_hit_rate: f64,

    /// Cache evictions
    pub cache_evictions: u64,

    /// Current queue depth
    pub queue_depth: usize,

    /// Max queue depth
    pub max_queue_depth: usize,

    /// Task throughput (tasks/sec)
    pub task_throughput: f64,

    /// Uptime
    pub uptime: Duration,

    /// Worker metrics
    pub worker_metrics: Vec<WorkerMetrics>,

    /// Task type metrics
    pub task_type_metrics: Vec<TaskTypeMetrics>,
}

impl ClusterMetrics {
    /// Create a new metrics collector.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(ClusterMetricsInner {
                tasks_submitted: AtomicU64::new(0),
                tasks_completed: AtomicU64::new(0),
                tasks_failed: AtomicU64::new(0),
                tasks_cancelled: AtomicU64::new(0),
                tasks_retried: AtomicU64::new(0),
                total_scheduling_latency_us: AtomicU64::new(0),
                total_execution_latency_us: AtomicU64::new(0),
                total_queue_latency_us: AtomicU64::new(0),
                active_workers: AtomicUsize::new(0),
                total_cpu_cores: AtomicUsize::new(0),
                used_cpu_cores: AtomicUsize::new(0),
                total_memory_bytes: AtomicU64::new(0),
                used_memory_bytes: AtomicU64::new(0),
                bytes_sent: AtomicU64::new(0),
                bytes_received: AtomicU64::new(0),
                network_errors: AtomicU64::new(0),
                cache_hits: AtomicU64::new(0),
                cache_misses: AtomicU64::new(0),
                cache_evictions: AtomicU64::new(0),
                queue_depth: AtomicUsize::new(0),
                max_queue_depth: AtomicUsize::new(0),
                worker_metrics: RwLock::new(HashMap::new()),
                task_type_metrics: RwLock::new(HashMap::new()),
                start_time: Instant::now(),
            }),
        }
    }

    /// Record a task submission.
    pub fn record_task_submitted(&self, task_type: &str) {
        self.inner.tasks_submitted.fetch_add(1, Ordering::Relaxed);
        let mut metrics = self.inner.task_type_metrics.write();
        let entry = metrics.entry(task_type.to_string()).or_default();
        entry.task_type = task_type.to_string();
        entry.submitted += 1;
    }

    /// Record a task completion.
    pub fn record_task_completed(&self, task_type: &str, execution_time: Duration) {
        self.inner.tasks_completed.fetch_add(1, Ordering::Relaxed);

        let exec_us = execution_time.as_micros() as u64;
        self.inner
            .total_execution_latency_us
            .fetch_add(exec_us, Ordering::Relaxed);

        let mut metrics = self.inner.task_type_metrics.write();
        let entry = metrics.entry(task_type.to_string()).or_default();
        entry.task_type = task_type.to_string();
        entry.completed += 1;

        let exec_ms = execution_time.as_secs_f64() * 1000.0;
        entry.min_execution_time_ms = entry.min_execution_time_ms.min(exec_ms);
        entry.max_execution_time_ms = entry.max_execution_time_ms.max(exec_ms);

        // Update running average
        let n = entry.completed as f64;
        entry.avg_execution_time_ms = (entry.avg_execution_time_ms * (n - 1.0) + exec_ms) / n;
    }

    /// Record a task failure.
    pub fn record_task_failed(&self, task_type: &str) {
        self.inner.tasks_failed.fetch_add(1, Ordering::Relaxed);
        let mut metrics = self.inner.task_type_metrics.write();
        let entry = metrics.entry(task_type.to_string()).or_default();
        entry.task_type = task_type.to_string();
        entry.failed += 1;
    }

    /// Record a task cancellation.
    pub fn record_task_cancelled(&self) {
        self.inner.tasks_cancelled.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a task retry.
    pub fn record_task_retried(&self) {
        self.inner.tasks_retried.fetch_add(1, Ordering::Relaxed);
    }

    /// Record scheduling latency.
    pub fn record_scheduling_latency(&self, latency: Duration) {
        let latency_us = latency.as_micros() as u64;
        self.inner
            .total_scheduling_latency_us
            .fetch_add(latency_us, Ordering::Relaxed);
    }

    /// Record queue latency.
    pub fn record_queue_latency(&self, latency: Duration) {
        let latency_us = latency.as_micros() as u64;
        self.inner
            .total_queue_latency_us
            .fetch_add(latency_us, Ordering::Relaxed);
    }

    /// Update queue depth.
    pub fn update_queue_depth(&self, depth: usize) {
        self.inner.queue_depth.store(depth, Ordering::Relaxed);

        // Update max queue depth
        let mut current_max = self.inner.max_queue_depth.load(Ordering::Relaxed);
        while depth > current_max {
            match self.inner.max_queue_depth.compare_exchange(
                current_max,
                depth,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(x) => current_max = x,
            }
        }
    }

    /// Update worker count.
    pub fn update_worker_count(&self, count: usize) {
        self.inner.active_workers.store(count, Ordering::Relaxed);
    }

    /// Update CPU cores.
    pub fn update_cpu_cores(&self, total: usize, used: usize) {
        self.inner.total_cpu_cores.store(total, Ordering::Relaxed);
        self.inner.used_cpu_cores.store(used, Ordering::Relaxed);
    }

    /// Update memory.
    pub fn update_memory(&self, total: u64, used: u64) {
        self.inner
            .total_memory_bytes
            .store(total, Ordering::Relaxed);
        self.inner.used_memory_bytes.store(used, Ordering::Relaxed);
    }

    /// Record network traffic.
    pub fn record_network_traffic(&self, sent: u64, received: u64) {
        self.inner.bytes_sent.fetch_add(sent, Ordering::Relaxed);
        self.inner
            .bytes_received
            .fetch_add(received, Ordering::Relaxed);
    }

    /// Record network error.
    pub fn record_network_error(&self) {
        self.inner.network_errors.fetch_add(1, Ordering::Relaxed);
    }

    /// Record cache hit.
    pub fn record_cache_hit(&self) {
        self.inner.cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Record cache miss.
    pub fn record_cache_miss(&self) {
        self.inner.cache_misses.fetch_add(1, Ordering::Relaxed);
    }

    /// Record cache eviction.
    pub fn record_cache_eviction(&self) {
        self.inner.cache_evictions.fetch_add(1, Ordering::Relaxed);
    }

    /// Update worker metrics.
    pub fn update_worker_metrics(&self, worker_id: String, metrics: WorkerMetrics) {
        let mut worker_metrics = self.inner.worker_metrics.write();
        worker_metrics.insert(worker_id, metrics);
    }

    /// Remove worker metrics.
    pub fn remove_worker_metrics(&self, worker_id: &str) {
        let mut worker_metrics = self.inner.worker_metrics.write();
        worker_metrics.remove(worker_id);
    }

    /// Get a snapshot of current metrics.
    pub fn snapshot(&self) -> MetricsSnapshot {
        let tasks_completed = self.inner.tasks_completed.load(Ordering::Relaxed);
        let tasks_submitted = self.inner.tasks_submitted.load(Ordering::Relaxed);

        let total_scheduling_latency_us = self
            .inner
            .total_scheduling_latency_us
            .load(Ordering::Relaxed);
        let total_execution_latency_us = self
            .inner
            .total_execution_latency_us
            .load(Ordering::Relaxed);
        let total_queue_latency_us = self.inner.total_queue_latency_us.load(Ordering::Relaxed);

        let avg_scheduling_latency_ms = if tasks_submitted > 0 {
            (total_scheduling_latency_us as f64 / tasks_submitted as f64) / 1000.0
        } else {
            0.0
        };

        let avg_execution_latency_ms = if tasks_completed > 0 {
            (total_execution_latency_us as f64 / tasks_completed as f64) / 1000.0
        } else {
            0.0
        };

        let avg_queue_latency_ms = if tasks_completed > 0 {
            (total_queue_latency_us as f64 / tasks_completed as f64) / 1000.0
        } else {
            0.0
        };

        let total_cpu_cores = self.inner.total_cpu_cores.load(Ordering::Relaxed);
        let used_cpu_cores = self.inner.used_cpu_cores.load(Ordering::Relaxed);
        let cpu_utilization = if total_cpu_cores > 0 {
            used_cpu_cores as f64 / total_cpu_cores as f64
        } else {
            0.0
        };

        let total_memory = self.inner.total_memory_bytes.load(Ordering::Relaxed);
        let used_memory = self.inner.used_memory_bytes.load(Ordering::Relaxed);
        let memory_utilization = if total_memory > 0 {
            used_memory as f64 / total_memory as f64
        } else {
            0.0
        };

        let cache_hits = self.inner.cache_hits.load(Ordering::Relaxed);
        let cache_misses = self.inner.cache_misses.load(Ordering::Relaxed);
        let cache_hit_rate = if cache_hits + cache_misses > 0 {
            cache_hits as f64 / (cache_hits + cache_misses) as f64
        } else {
            0.0
        };

        let uptime = self.inner.start_time.elapsed();
        let task_throughput = if uptime.as_secs() > 0 {
            tasks_completed as f64 / uptime.as_secs_f64()
        } else {
            0.0
        };

        let worker_metrics = self.inner.worker_metrics.read().values().cloned().collect();

        let task_type_metrics = self
            .inner
            .task_type_metrics
            .read()
            .values()
            .cloned()
            .collect();

        MetricsSnapshot {
            tasks_submitted,
            tasks_completed,
            tasks_failed: self.inner.tasks_failed.load(Ordering::Relaxed),
            tasks_cancelled: self.inner.tasks_cancelled.load(Ordering::Relaxed),
            tasks_retried: self.inner.tasks_retried.load(Ordering::Relaxed),
            avg_scheduling_latency_ms,
            avg_execution_latency_ms,
            avg_queue_latency_ms,
            active_workers: self.inner.active_workers.load(Ordering::Relaxed),
            total_cpu_cores,
            used_cpu_cores,
            cpu_utilization,
            total_memory_bytes: total_memory,
            used_memory_bytes: used_memory,
            memory_utilization,
            bytes_sent: self.inner.bytes_sent.load(Ordering::Relaxed),
            bytes_received: self.inner.bytes_received.load(Ordering::Relaxed),
            network_errors: self.inner.network_errors.load(Ordering::Relaxed),
            cache_hits,
            cache_misses,
            cache_hit_rate,
            cache_evictions: self.inner.cache_evictions.load(Ordering::Relaxed),
            queue_depth: self.inner.queue_depth.load(Ordering::Relaxed),
            max_queue_depth: self.inner.max_queue_depth.load(Ordering::Relaxed),
            task_throughput,
            uptime,
            worker_metrics,
            task_type_metrics,
        }
    }

    /// Reset all metrics.
    pub fn reset(&self) {
        self.inner.tasks_submitted.store(0, Ordering::Relaxed);
        self.inner.tasks_completed.store(0, Ordering::Relaxed);
        self.inner.tasks_failed.store(0, Ordering::Relaxed);
        self.inner.tasks_cancelled.store(0, Ordering::Relaxed);
        self.inner.tasks_retried.store(0, Ordering::Relaxed);
        self.inner
            .total_scheduling_latency_us
            .store(0, Ordering::Relaxed);
        self.inner
            .total_execution_latency_us
            .store(0, Ordering::Relaxed);
        self.inner
            .total_queue_latency_us
            .store(0, Ordering::Relaxed);
        self.inner.bytes_sent.store(0, Ordering::Relaxed);
        self.inner.bytes_received.store(0, Ordering::Relaxed);
        self.inner.network_errors.store(0, Ordering::Relaxed);
        self.inner.cache_hits.store(0, Ordering::Relaxed);
        self.inner.cache_misses.store(0, Ordering::Relaxed);
        self.inner.cache_evictions.store(0, Ordering::Relaxed);
        self.inner.queue_depth.store(0, Ordering::Relaxed);
        self.inner.max_queue_depth.store(0, Ordering::Relaxed);
        self.inner.worker_metrics.write().clear();
        self.inner.task_type_metrics.write().clear();
    }

    /// Export metrics as JSON.
    pub fn export_json(&self) -> Result<String> {
        let snapshot = self.snapshot();
        serde_json::to_string_pretty(&snapshot).map_err(|e| e.into())
    }
}

impl Default for ClusterMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_creation() {
        let metrics = ClusterMetrics::new();
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.tasks_submitted, 0);
        assert_eq!(snapshot.tasks_completed, 0);
    }

    #[test]
    fn test_task_metrics() {
        let metrics = ClusterMetrics::new();

        metrics.record_task_submitted("test_task");
        metrics.record_task_completed("test_task", Duration::from_millis(100));

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.tasks_submitted, 1);
        assert_eq!(snapshot.tasks_completed, 1);
        assert!(snapshot.avg_execution_latency_ms > 0.0);
    }

    #[test]
    fn test_cache_hit_rate() {
        let metrics = ClusterMetrics::new();

        metrics.record_cache_hit();
        metrics.record_cache_hit();
        metrics.record_cache_miss();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.cache_hits, 2);
        assert_eq!(snapshot.cache_misses, 1);
        assert!((snapshot.cache_hit_rate - 0.666).abs() < 0.01);
    }

    #[test]
    fn test_resource_utilization() {
        let metrics = ClusterMetrics::new();

        metrics.update_cpu_cores(8, 6);
        metrics.update_memory(16_000_000_000, 10_000_000_000);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.total_cpu_cores, 8);
        assert_eq!(snapshot.used_cpu_cores, 6);
        assert!((snapshot.cpu_utilization - 0.75).abs() < 0.01);
        assert!((snapshot.memory_utilization - 0.625).abs() < 0.01);
    }

    #[test]
    fn test_queue_depth() {
        let metrics = ClusterMetrics::new();

        metrics.update_queue_depth(10);
        metrics.update_queue_depth(20);
        metrics.update_queue_depth(15);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.queue_depth, 15);
        assert_eq!(snapshot.max_queue_depth, 20);
    }
}
