//! Workflow metrics collection and reporting.

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

/// Workflow metrics collector.
pub struct MetricsCollector {
    workflow_metrics: Arc<DashMap<String, WorkflowMetrics>>,
    global_metrics: Arc<GlobalMetrics>,
}

impl MetricsCollector {
    /// Create a new metrics collector.
    pub fn new() -> Self {
        Self {
            workflow_metrics: Arc::new(DashMap::new()),
            global_metrics: Arc::new(GlobalMetrics::new()),
        }
    }

    /// Record a workflow execution start.
    pub fn record_workflow_start(&self, workflow_id: &str) {
        let mut metrics = self
            .workflow_metrics
            .entry(workflow_id.to_string())
            .or_default();

        metrics.total_executions += 1;
        metrics.running_executions += 1;
        metrics.last_execution_start = Some(Utc::now());

        self.global_metrics.increment_total_executions();
        self.global_metrics.increment_running_executions();
    }

    /// Record a workflow execution completion.
    pub fn record_workflow_completion(&self, workflow_id: &str, duration: Duration, success: bool) {
        let mut metrics = self
            .workflow_metrics
            .entry(workflow_id.to_string())
            .or_default();

        metrics.running_executions = metrics.running_executions.saturating_sub(1);

        if success {
            metrics.successful_executions += 1;
            self.global_metrics.increment_successful_executions();
        } else {
            metrics.failed_executions += 1;
            self.global_metrics.increment_failed_executions();
        }

        metrics.total_execution_time_ms += duration.as_millis() as u64;
        metrics.last_execution_duration = Some(duration);
        metrics.last_execution_end = Some(Utc::now());

        // Update min/max duration
        if metrics.min_execution_duration.is_none()
            || Some(duration) < metrics.min_execution_duration
        {
            metrics.min_execution_duration = Some(duration);
        }

        if metrics.max_execution_duration.is_none()
            || Some(duration) > metrics.max_execution_duration
        {
            metrics.max_execution_duration = Some(duration);
        }

        self.global_metrics.decrement_running_executions();
        self.global_metrics
            .add_execution_time(duration.as_millis() as u64);
    }

    /// Record a task execution.
    pub fn record_task_execution(
        &self,
        workflow_id: &str,
        task_id: &str,
        duration: Duration,
        success: bool,
    ) {
        let mut metrics = self
            .workflow_metrics
            .entry(workflow_id.to_string())
            .or_default();

        metrics.total_tasks_executed += 1;

        if success {
            metrics.successful_tasks += 1;
        } else {
            metrics.failed_tasks += 1;
        }

        metrics.task_durations.insert(task_id.to_string(), duration);

        self.global_metrics.increment_total_tasks();
    }

    /// Record a task retry.
    pub fn record_task_retry(&self, workflow_id: &str) {
        let mut metrics = self
            .workflow_metrics
            .entry(workflow_id.to_string())
            .or_default();

        metrics.total_retries += 1;
        self.global_metrics.increment_retries();
    }

    /// Get metrics for a specific workflow.
    pub fn get_workflow_metrics(&self, workflow_id: &str) -> Option<WorkflowMetrics> {
        self.workflow_metrics
            .get(workflow_id)
            .map(|entry| entry.clone())
    }

    /// Get all workflow metrics.
    pub fn get_all_metrics(&self) -> HashMap<String, WorkflowMetrics> {
        self.workflow_metrics
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect()
    }

    /// Get global metrics.
    pub fn get_global_metrics(&self) -> GlobalMetricsSnapshot {
        self.global_metrics.snapshot()
    }

    /// Reset metrics for a workflow.
    pub fn reset_workflow_metrics(&self, workflow_id: &str) {
        self.workflow_metrics.remove(workflow_id);
    }

    /// Reset all metrics.
    pub fn reset_all_metrics(&self) {
        self.workflow_metrics.clear();
        self.global_metrics.reset();
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Metrics for a specific workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowMetrics {
    /// Total number of executions.
    pub total_executions: usize,
    /// Number of successful executions.
    pub successful_executions: usize,
    /// Number of failed executions.
    pub failed_executions: usize,
    /// Number of currently running executions.
    pub running_executions: usize,
    /// Total execution time in milliseconds.
    pub total_execution_time_ms: u64,
    /// Minimum execution duration.
    pub min_execution_duration: Option<Duration>,
    /// Maximum execution duration.
    pub max_execution_duration: Option<Duration>,
    /// Last execution duration.
    pub last_execution_duration: Option<Duration>,
    /// Last execution start time.
    pub last_execution_start: Option<DateTime<Utc>>,
    /// Last execution end time.
    pub last_execution_end: Option<DateTime<Utc>>,
    /// Total tasks executed.
    pub total_tasks_executed: usize,
    /// Successful tasks.
    pub successful_tasks: usize,
    /// Failed tasks.
    pub failed_tasks: usize,
    /// Total retries.
    pub total_retries: usize,
    /// Task duration map.
    pub task_durations: HashMap<String, Duration>,
}

impl WorkflowMetrics {
    /// Create new workflow metrics.
    pub fn new() -> Self {
        Self {
            total_executions: 0,
            successful_executions: 0,
            failed_executions: 0,
            running_executions: 0,
            total_execution_time_ms: 0,
            min_execution_duration: None,
            max_execution_duration: None,
            last_execution_duration: None,
            last_execution_start: None,
            last_execution_end: None,
            total_tasks_executed: 0,
            successful_tasks: 0,
            failed_tasks: 0,
            total_retries: 0,
            task_durations: HashMap::new(),
        }
    }

    /// Calculate average execution duration.
    pub fn average_execution_duration(&self) -> Option<Duration> {
        if self.total_executions > 0 {
            Some(Duration::from_millis(
                self.total_execution_time_ms / self.total_executions as u64,
            ))
        } else {
            None
        }
    }

    /// Calculate success rate (0.0 - 1.0).
    pub fn success_rate(&self) -> f64 {
        if self.total_executions > 0 {
            self.successful_executions as f64 / self.total_executions as f64
        } else {
            0.0
        }
    }

    /// Calculate task success rate (0.0 - 1.0).
    pub fn task_success_rate(&self) -> f64 {
        if self.total_tasks_executed > 0 {
            self.successful_tasks as f64 / self.total_tasks_executed as f64
        } else {
            0.0
        }
    }

    /// Get average task duration.
    pub fn average_task_duration(&self) -> Option<Duration> {
        if self.task_durations.is_empty() {
            return None;
        }

        let total: Duration = self.task_durations.values().sum();
        Some(total / self.task_durations.len() as u32)
    }
}

impl Default for WorkflowMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Global metrics across all workflows.
struct GlobalMetrics {
    total_executions: AtomicUsize,
    successful_executions: AtomicUsize,
    failed_executions: AtomicUsize,
    running_executions: AtomicUsize,
    total_execution_time_ms: AtomicU64,
    total_tasks: AtomicUsize,
    total_retries: AtomicUsize,
}

impl GlobalMetrics {
    fn new() -> Self {
        Self {
            total_executions: AtomicUsize::new(0),
            successful_executions: AtomicUsize::new(0),
            failed_executions: AtomicUsize::new(0),
            running_executions: AtomicUsize::new(0),
            total_execution_time_ms: AtomicU64::new(0),
            total_tasks: AtomicUsize::new(0),
            total_retries: AtomicUsize::new(0),
        }
    }

    fn increment_total_executions(&self) {
        self.total_executions.fetch_add(1, Ordering::Relaxed);
    }

    fn increment_successful_executions(&self) {
        self.successful_executions.fetch_add(1, Ordering::Relaxed);
    }

    fn increment_failed_executions(&self) {
        self.failed_executions.fetch_add(1, Ordering::Relaxed);
    }

    fn increment_running_executions(&self) {
        self.running_executions.fetch_add(1, Ordering::Relaxed);
    }

    fn decrement_running_executions(&self) {
        self.running_executions.fetch_sub(1, Ordering::Relaxed);
    }

    fn add_execution_time(&self, duration_ms: u64) {
        self.total_execution_time_ms
            .fetch_add(duration_ms, Ordering::Relaxed);
    }

    fn increment_total_tasks(&self) {
        self.total_tasks.fetch_add(1, Ordering::Relaxed);
    }

    fn increment_retries(&self) {
        self.total_retries.fetch_add(1, Ordering::Relaxed);
    }

    fn snapshot(&self) -> GlobalMetricsSnapshot {
        GlobalMetricsSnapshot {
            total_executions: self.total_executions.load(Ordering::Relaxed),
            successful_executions: self.successful_executions.load(Ordering::Relaxed),
            failed_executions: self.failed_executions.load(Ordering::Relaxed),
            running_executions: self.running_executions.load(Ordering::Relaxed),
            total_execution_time_ms: self.total_execution_time_ms.load(Ordering::Relaxed),
            total_tasks: self.total_tasks.load(Ordering::Relaxed),
            total_retries: self.total_retries.load(Ordering::Relaxed),
        }
    }

    fn reset(&self) {
        self.total_executions.store(0, Ordering::Relaxed);
        self.successful_executions.store(0, Ordering::Relaxed);
        self.failed_executions.store(0, Ordering::Relaxed);
        self.running_executions.store(0, Ordering::Relaxed);
        self.total_execution_time_ms.store(0, Ordering::Relaxed);
        self.total_tasks.store(0, Ordering::Relaxed);
        self.total_retries.store(0, Ordering::Relaxed);
    }
}

/// Snapshot of global metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalMetricsSnapshot {
    /// Total executions across all workflows.
    pub total_executions: usize,
    /// Successful executions.
    pub successful_executions: usize,
    /// Failed executions.
    pub failed_executions: usize,
    /// Currently running executions.
    pub running_executions: usize,
    /// Total execution time in milliseconds.
    pub total_execution_time_ms: u64,
    /// Total tasks executed.
    pub total_tasks: usize,
    /// Total retries.
    pub total_retries: usize,
}

impl GlobalMetricsSnapshot {
    /// Calculate global success rate.
    pub fn success_rate(&self) -> f64 {
        if self.total_executions > 0 {
            self.successful_executions as f64 / self.total_executions as f64
        } else {
            0.0
        }
    }

    /// Calculate average execution time.
    pub fn average_execution_time(&self) -> Option<Duration> {
        if self.total_executions > 0 {
            Some(Duration::from_millis(
                self.total_execution_time_ms / self.total_executions as u64,
            ))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_collector() {
        let collector = MetricsCollector::new();

        collector.record_workflow_start("workflow1");
        collector.record_workflow_completion("workflow1", Duration::from_secs(10), true);

        let metrics = collector
            .get_workflow_metrics("workflow1")
            .expect("Metrics not found");

        assert_eq!(metrics.total_executions, 1);
        assert_eq!(metrics.successful_executions, 1);
        assert_eq!(metrics.running_executions, 0);
    }

    #[test]
    fn test_workflow_metrics_success_rate() {
        let mut metrics = WorkflowMetrics::new();
        metrics.total_executions = 10;
        metrics.successful_executions = 8;

        assert_eq!(metrics.success_rate(), 0.8);
    }

    #[test]
    fn test_global_metrics() {
        let collector = MetricsCollector::new();

        collector.record_workflow_start("workflow1");
        collector.record_workflow_completion("workflow1", Duration::from_secs(5), true);

        let global = collector.get_global_metrics();
        assert_eq!(global.total_executions, 1);
        assert_eq!(global.successful_executions, 1);
    }

    #[test]
    fn test_task_metrics() {
        let collector = MetricsCollector::new();

        collector.record_task_execution("workflow1", "task1", Duration::from_secs(1), true);
        collector.record_task_execution("workflow1", "task2", Duration::from_secs(2), true);

        let metrics = collector
            .get_workflow_metrics("workflow1")
            .expect("Metrics not found");

        assert_eq!(metrics.total_tasks_executed, 2);
        assert_eq!(metrics.successful_tasks, 2);
    }
}
