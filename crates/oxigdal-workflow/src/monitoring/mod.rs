//! Workflow monitoring and observability.
//!
//! Provides comprehensive monitoring capabilities including:
//! - Real-time metrics collection
//! - Execution history tracking
//! - DAG visualization
//! - Performance profiling
//! - Bottleneck detection

pub mod debugging;
pub mod logging;
pub mod metrics;
pub mod visualization;

use crate::error::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

pub use debugging::{DebugInfo, DebugSession, Debugger};
pub use logging::{LogEntry, LogLevel, WorkflowLogger};
pub use metrics::{MetricsCollector, WorkflowMetrics};
pub use visualization::{DagVisualizer, GraphFormat, VisualizationConfig};

/// Execution history entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionHistory {
    /// Execution ID.
    pub execution_id: String,
    /// Workflow ID.
    pub workflow_id: String,
    /// Workflow name.
    pub workflow_name: String,
    /// Execution start time.
    pub start_time: DateTime<Utc>,
    /// Execution end time.
    pub end_time: Option<DateTime<Utc>>,
    /// Execution duration.
    pub duration: Option<Duration>,
    /// Execution status.
    pub status: ExecutionHistoryStatus,
    /// Task execution records.
    pub tasks: Vec<TaskExecutionRecord>,
    /// Total tasks count.
    pub total_tasks: usize,
    /// Completed tasks count.
    pub completed_tasks: usize,
    /// Failed tasks count.
    pub failed_tasks: usize,
    /// Execution metadata.
    pub metadata: HashMap<String, String>,
    /// Error message if failed.
    pub error_message: Option<String>,
}

/// Execution history status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionHistoryStatus {
    /// Execution is running.
    Running,
    /// Execution completed successfully.
    Success,
    /// Execution failed.
    Failed,
    /// Execution was cancelled.
    Cancelled,
    /// Execution timed out.
    TimedOut,
    /// Execution is paused.
    Paused,
}

/// Task execution record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskExecutionRecord {
    /// Task ID.
    pub task_id: String,
    /// Task name.
    pub task_name: String,
    /// Task start time.
    pub start_time: DateTime<Utc>,
    /// Task end time.
    pub end_time: Option<DateTime<Utc>>,
    /// Task duration.
    pub duration: Option<Duration>,
    /// Task status.
    pub status: TaskExecutionStatus,
    /// Retry count.
    pub retry_count: usize,
    /// Task output size in bytes.
    pub output_size_bytes: usize,
    /// Peak memory usage in bytes.
    pub peak_memory_bytes: Option<usize>,
    /// CPU time in milliseconds.
    pub cpu_time_ms: Option<u64>,
    /// Error message if failed.
    pub error_message: Option<String>,
}

/// Task execution status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskExecutionStatus {
    /// Task is pending.
    Pending,
    /// Task is running.
    Running,
    /// Task completed successfully.
    Success,
    /// Task failed.
    Failed,
    /// Task was skipped.
    Skipped,
    /// Task was cancelled.
    Cancelled,
}

/// Performance metrics for a workflow execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    /// Total execution time.
    pub total_duration: Duration,
    /// Time spent in task execution.
    pub task_execution_time: Duration,
    /// Time spent in scheduling/coordination.
    pub coordination_overhead: Duration,
    /// Average task duration.
    pub avg_task_duration: Duration,
    /// Longest task duration.
    pub longest_task_duration: Duration,
    /// Shortest task duration.
    pub shortest_task_duration: Duration,
    /// Parallelism factor (average concurrent tasks).
    pub parallelism_factor: f64,
    /// Throughput (tasks per second).
    pub throughput: f64,
    /// Critical path length.
    pub critical_path_length: Duration,
}

/// Bottleneck analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BottleneckAnalysis {
    /// Critical path tasks.
    pub critical_path: Vec<String>,
    /// Slowest tasks.
    pub slowest_tasks: Vec<(String, Duration)>,
    /// Tasks with high retry count.
    pub high_retry_tasks: Vec<(String, usize)>,
    /// Resource bottlenecks.
    pub resource_bottlenecks: Vec<ResourceBottleneck>,
    /// Suggestions for optimization.
    pub suggestions: Vec<String>,
}

/// Resource bottleneck information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceBottleneck {
    /// Resource type (CPU, memory, I/O).
    pub resource_type: String,
    /// Affected tasks.
    pub affected_tasks: Vec<String>,
    /// Severity (0.0 - 1.0).
    pub severity: f64,
    /// Description.
    pub description: String,
}

/// Monitoring service for workflow executions.
pub struct MonitoringService {
    metrics_collector: MetricsCollector,
    logger: WorkflowLogger,
    debugger: Debugger,
    visualizer: DagVisualizer,
}

impl MonitoringService {
    /// Create a new monitoring service.
    pub fn new() -> Self {
        Self {
            metrics_collector: MetricsCollector::new(),
            logger: WorkflowLogger::new(),
            debugger: Debugger::new(),
            visualizer: DagVisualizer::new(),
        }
    }

    /// Get the metrics collector.
    pub fn metrics(&self) -> &MetricsCollector {
        &self.metrics_collector
    }

    /// Get the logger.
    pub fn logger(&self) -> &WorkflowLogger {
        &self.logger
    }

    /// Get the debugger.
    pub fn debugger(&self) -> &Debugger {
        &self.debugger
    }

    /// Get the visualizer.
    pub fn visualizer(&self) -> &DagVisualizer {
        &self.visualizer
    }

    /// Analyze execution performance.
    pub fn analyze_performance(&self, history: &ExecutionHistory) -> Result<PerformanceMetrics> {
        let total_duration = history
            .duration
            .ok_or_else(|| crate::error::WorkflowError::monitoring("Duration not available"))?;

        let task_durations: Vec<Duration> =
            history.tasks.iter().filter_map(|t| t.duration).collect();

        if task_durations.is_empty() {
            return Err(crate::error::WorkflowError::monitoring(
                "No task durations available",
            ));
        }

        let task_execution_time: Duration = task_durations.iter().sum();
        let coordination_overhead = total_duration.saturating_sub(task_execution_time);

        let avg_task_duration = task_execution_time
            .checked_div(task_durations.len() as u32)
            .unwrap_or(Duration::ZERO);

        let longest_task_duration = task_durations
            .iter()
            .max()
            .copied()
            .unwrap_or(Duration::ZERO);

        let shortest_task_duration = task_durations
            .iter()
            .min()
            .copied()
            .unwrap_or(Duration::ZERO);

        let parallelism_factor = if total_duration.as_secs() > 0 {
            task_execution_time.as_secs_f64() / total_duration.as_secs_f64()
        } else {
            0.0
        };

        let throughput = if total_duration.as_secs_f64() > 0.0 {
            history.total_tasks as f64 / total_duration.as_secs_f64()
        } else {
            0.0
        };

        Ok(PerformanceMetrics {
            total_duration,
            task_execution_time,
            coordination_overhead,
            avg_task_duration,
            longest_task_duration,
            shortest_task_duration,
            parallelism_factor,
            throughput,
            critical_path_length: longest_task_duration, // Simplified
        })
    }

    /// Detect bottlenecks in execution.
    pub fn detect_bottlenecks(&self, history: &ExecutionHistory) -> Result<BottleneckAnalysis> {
        let mut slowest_tasks: Vec<(String, Duration)> = history
            .tasks
            .iter()
            .filter_map(|t| t.duration.map(|d| (t.task_id.clone(), d)))
            .collect();

        slowest_tasks.sort_by(|a, b| b.1.cmp(&a.1));
        slowest_tasks.truncate(5);

        let mut high_retry_tasks: Vec<(String, usize)> = history
            .tasks
            .iter()
            .filter(|t| t.retry_count > 0)
            .map(|t| (t.task_id.clone(), t.retry_count))
            .collect();

        high_retry_tasks.sort_by(|a, b| b.1.cmp(&a.1));
        high_retry_tasks.truncate(5);

        let mut suggestions = Vec::new();

        if !slowest_tasks.is_empty() {
            suggestions.push(format!(
                "Consider optimizing task '{}' which took {:?}",
                slowest_tasks[0].0, slowest_tasks[0].1
            ));
        }

        if !high_retry_tasks.is_empty() {
            suggestions.push(format!(
                "Task '{}' has {} retries, investigate failure causes",
                high_retry_tasks[0].0, high_retry_tasks[0].1
            ));
        }

        Ok(BottleneckAnalysis {
            critical_path: Vec::new(), // Would need DAG structure to compute
            slowest_tasks,
            high_retry_tasks,
            resource_bottlenecks: Vec::new(),
            suggestions,
        })
    }
}

impl Default for MonitoringService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monitoring_service_creation() {
        let service = MonitoringService::new();
        assert!(service.metrics().get_all_metrics().is_empty());
    }

    #[test]
    fn test_execution_history_status() {
        let status = ExecutionHistoryStatus::Running;
        assert_eq!(status, ExecutionHistoryStatus::Running);
    }

    #[test]
    fn test_performance_metrics() {
        let history = ExecutionHistory {
            execution_id: "exec1".to_string(),
            workflow_id: "wf1".to_string(),
            workflow_name: "Test Workflow".to_string(),
            start_time: Utc::now(),
            end_time: Some(Utc::now()),
            duration: Some(Duration::from_secs(100)),
            status: ExecutionHistoryStatus::Success,
            tasks: vec![
                TaskExecutionRecord {
                    task_id: "task1".to_string(),
                    task_name: "Task 1".to_string(),
                    start_time: Utc::now(),
                    end_time: Some(Utc::now()),
                    duration: Some(Duration::from_secs(30)),
                    status: TaskExecutionStatus::Success,
                    retry_count: 0,
                    output_size_bytes: 1024,
                    peak_memory_bytes: None,
                    cpu_time_ms: None,
                    error_message: None,
                },
                TaskExecutionRecord {
                    task_id: "task2".to_string(),
                    task_name: "Task 2".to_string(),
                    start_time: Utc::now(),
                    end_time: Some(Utc::now()),
                    duration: Some(Duration::from_secs(40)),
                    status: TaskExecutionStatus::Success,
                    retry_count: 0,
                    output_size_bytes: 2048,
                    peak_memory_bytes: None,
                    cpu_time_ms: None,
                    error_message: None,
                },
            ],
            total_tasks: 2,
            completed_tasks: 2,
            failed_tasks: 0,
            metadata: HashMap::new(),
            error_message: None,
        };

        let service = MonitoringService::new();
        let metrics = service
            .analyze_performance(&history)
            .expect("Analysis failed");

        assert_eq!(metrics.total_duration, Duration::from_secs(100));
        assert!(metrics.avg_task_duration.as_secs() > 0);
    }
}
