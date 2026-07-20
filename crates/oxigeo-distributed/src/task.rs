//! Task definitions and management for distributed processing.
//!
//! This module defines the task types and execution logic for distributed
//! geospatial processing operations.

use crate::error::{DistributedError, Result};
use arrow::record_batch::RecordBatch;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;

/// Unique identifier for a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskId(pub u64);

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Task({})", self.0)
    }
}

/// Unique identifier for a partition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PartitionId(pub u64);

impl fmt::Display for PartitionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Partition({})", self.0)
    }
}

/// Status of a task execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    /// Task is pending execution.
    Pending,
    /// Task is currently being executed.
    Running,
    /// Task completed successfully.
    Completed,
    /// Task failed with an error.
    Failed,
    /// Task was cancelled.
    Cancelled,
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "Pending"),
            Self::Running => write!(f, "Running"),
            Self::Completed => write!(f, "Completed"),
            Self::Failed => write!(f, "Failed"),
            Self::Cancelled => write!(f, "Cancelled"),
        }
    }
}

/// Type of geospatial operation to perform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskOperation {
    /// Apply a filter to data.
    Filter {
        /// Filter expression.
        expression: String,
    },
    /// Calculate a raster index (NDVI, NDWI, etc.).
    CalculateIndex {
        /// Index type.
        index_type: String,
        /// Band indices for calculation.
        bands: Vec<usize>,
    },
    /// Reproject data to a different CRS.
    Reproject {
        /// Target EPSG code.
        target_epsg: i32,
    },
    /// Resample raster data.
    Resample {
        /// Target width.
        width: usize,
        /// Target height.
        height: usize,
        /// Resampling method.
        method: String,
    },
    /// Clip data to a bounding box.
    Clip {
        /// Minimum X coordinate.
        min_x: f64,
        /// Minimum Y coordinate.
        min_y: f64,
        /// Maximum X coordinate.
        max_x: f64,
        /// Maximum Y coordinate.
        max_y: f64,
    },
    /// Apply a convolution kernel.
    Convolve {
        /// Kernel values.
        kernel: Vec<f64>,
        /// Kernel width.
        kernel_width: usize,
        /// Kernel height.
        kernel_height: usize,
    },
    /// Custom user-defined operation.
    Custom {
        /// Operation name.
        name: String,
        /// JSON-serialized parameters.
        params: String,
    },
}

/// A task to be executed by a worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Unique task identifier.
    pub id: TaskId,
    /// Partition to process.
    pub partition_id: PartitionId,
    /// Operation to perform.
    pub operation: TaskOperation,
    /// Current status.
    pub status: TaskStatus,
    /// Worker ID assigned to this task (if any).
    pub worker_id: Option<String>,
    /// Number of retry attempts.
    pub retry_count: u32,
    /// Maximum number of retries allowed.
    pub max_retries: u32,
}

impl Task {
    /// Create a new task.
    pub fn new(id: TaskId, partition_id: PartitionId, operation: TaskOperation) -> Self {
        Self {
            id,
            partition_id,
            operation,
            status: TaskStatus::Pending,
            worker_id: None,
            retry_count: 0,
            max_retries: 3,
        }
    }

    /// Check if the task can be retried.
    pub fn can_retry(&self) -> bool {
        self.retry_count < self.max_retries
    }

    /// Mark the task as running on a specific worker.
    pub fn mark_running(&mut self, worker_id: String) {
        self.status = TaskStatus::Running;
        self.worker_id = Some(worker_id);
    }

    /// Mark the task as completed.
    pub fn mark_completed(&mut self) {
        self.status = TaskStatus::Completed;
    }

    /// Mark the task as failed and increment retry count.
    pub fn mark_failed(&mut self) {
        self.status = TaskStatus::Failed;
        self.retry_count += 1;
    }

    /// Mark the task as cancelled.
    pub fn mark_cancelled(&mut self) {
        self.status = TaskStatus::Cancelled;
    }

    /// Reset the task for retry.
    pub fn reset_for_retry(&mut self) {
        self.status = TaskStatus::Pending;
        self.worker_id = None;
    }
}

/// Result of a task execution.
#[derive(Debug, Clone)]
pub struct TaskResult {
    /// Task identifier.
    pub task_id: TaskId,
    /// Resulting data as Arrow RecordBatch.
    pub data: Option<Arc<RecordBatch>>,
    /// Execution time in milliseconds.
    pub execution_time_ms: u64,
    /// Error message if task failed.
    pub error: Option<String>,
}

impl TaskResult {
    /// Create a successful task result.
    pub fn success(task_id: TaskId, data: Arc<RecordBatch>, execution_time_ms: u64) -> Self {
        Self {
            task_id,
            data: Some(data),
            execution_time_ms,
            error: None,
        }
    }

    /// Create a failed task result.
    pub fn failure(task_id: TaskId, error: String, execution_time_ms: u64) -> Self {
        Self {
            task_id,
            data: None,
            execution_time_ms,
            error: Some(error),
        }
    }

    /// Check if the result indicates success.
    pub fn is_success(&self) -> bool {
        self.error.is_none()
    }

    /// Check if the result indicates failure.
    pub fn is_failure(&self) -> bool {
        self.error.is_some()
    }
}

/// Task execution context with metadata.
#[derive(Debug, Clone)]
pub struct TaskContext {
    /// Task identifier.
    pub task_id: TaskId,
    /// Worker identifier executing this task.
    pub worker_id: String,
    /// Total memory available (bytes).
    pub memory_limit: u64,
    /// Number of CPU cores available.
    pub num_cores: usize,
}

impl TaskContext {
    /// Create a new task context.
    pub fn new(task_id: TaskId, worker_id: String) -> Self {
        Self {
            task_id,
            worker_id,
            memory_limit: 1024 * 1024 * 1024, // 1 GB default
            num_cores: num_cpus(),
        }
    }

    /// Set the memory limit.
    pub fn with_memory_limit(mut self, limit: u64) -> Self {
        self.memory_limit = limit;
        self
    }

    /// Set the number of cores.
    pub fn with_num_cores(mut self, cores: usize) -> Self {
        self.num_cores = cores;
        self
    }
}

/// Get the number of available CPU cores.
fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

/// Task scheduler for managing task execution order.
#[derive(Debug)]
pub struct TaskScheduler {
    /// Queue of pending tasks.
    pending: Vec<Task>,
    /// Currently running tasks.
    running: Vec<Task>,
    /// Completed tasks.
    completed: Vec<Task>,
    /// Failed tasks.
    failed: Vec<Task>,
}

impl TaskScheduler {
    /// Create a new task scheduler.
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
            running: Vec::new(),
            completed: Vec::new(),
            failed: Vec::new(),
        }
    }

    /// Add a task to the scheduler.
    pub fn add_task(&mut self, task: Task) {
        self.pending.push(task);
    }

    /// Get the next pending task.
    pub fn next_task(&mut self) -> Option<Task> {
        self.pending.pop()
    }

    /// Mark a task as running.
    pub fn mark_running(&mut self, mut task: Task, worker_id: String) {
        task.mark_running(worker_id);
        self.running.push(task);
    }

    /// Mark a task as completed.
    pub fn mark_completed(&mut self, task_id: TaskId) -> Result<()> {
        if let Some(pos) = self.running.iter().position(|t| t.id == task_id) {
            let mut task = self.running.remove(pos);
            task.mark_completed();
            self.completed.push(task);
            Ok(())
        } else {
            Err(DistributedError::coordinator(format!(
                "Task {} not found in running tasks",
                task_id
            )))
        }
    }

    /// Mark a task as failed and potentially retry.
    pub fn mark_failed(&mut self, task_id: TaskId) -> Result<()> {
        if let Some(pos) = self.running.iter().position(|t| t.id == task_id) {
            let mut task = self.running.remove(pos);
            task.mark_failed();

            if task.can_retry() {
                task.reset_for_retry();
                self.pending.push(task);
            } else {
                self.failed.push(task);
            }
            Ok(())
        } else {
            Err(DistributedError::coordinator(format!(
                "Task {} not found in running tasks",
                task_id
            )))
        }
    }

    /// Get the number of pending tasks.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Get the number of running tasks.
    pub fn running_count(&self) -> usize {
        self.running.len()
    }

    /// Get the number of completed tasks.
    pub fn completed_count(&self) -> usize {
        self.completed.len()
    }

    /// Get the number of failed tasks.
    pub fn failed_count(&self) -> usize {
        self.failed.len()
    }

    /// Check if all tasks are complete.
    pub fn is_complete(&self) -> bool {
        self.pending.is_empty() && self.running.is_empty()
    }
}

impl Default for TaskScheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_creation() {
        let task = Task::new(
            TaskId(1),
            PartitionId(0),
            TaskOperation::Filter {
                expression: "value > 10".to_string(),
            },
        );

        assert_eq!(task.id, TaskId(1));
        assert_eq!(task.partition_id, PartitionId(0));
        assert_eq!(task.status, TaskStatus::Pending);
        assert!(task.worker_id.is_none());
    }

    #[test]
    fn test_task_lifecycle() {
        let mut task = Task::new(
            TaskId(1),
            PartitionId(0),
            TaskOperation::Filter {
                expression: "value > 10".to_string(),
            },
        );

        task.mark_running("worker-1".to_string());
        assert_eq!(task.status, TaskStatus::Running);
        assert_eq!(task.worker_id, Some("worker-1".to_string()));

        task.mark_completed();
        assert_eq!(task.status, TaskStatus::Completed);
    }

    #[test]
    fn test_task_retry() {
        let mut task = Task::new(
            TaskId(1),
            PartitionId(0),
            TaskOperation::Filter {
                expression: "value > 10".to_string(),
            },
        );

        task.max_retries = 2;

        assert!(task.can_retry());
        task.mark_failed();
        assert_eq!(task.retry_count, 1);
        assert!(task.can_retry());

        task.mark_failed();
        assert_eq!(task.retry_count, 2);
        assert!(!task.can_retry());
    }

    #[test]
    fn test_task_scheduler() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let mut scheduler = TaskScheduler::new();

        let task1 = Task::new(
            TaskId(1),
            PartitionId(0),
            TaskOperation::Filter {
                expression: "value > 10".to_string(),
            },
        );
        let task2 = Task::new(
            TaskId(2),
            PartitionId(1),
            TaskOperation::Filter {
                expression: "value < 100".to_string(),
            },
        );

        scheduler.add_task(task1);
        scheduler.add_task(task2);

        assert_eq!(scheduler.pending_count(), 2);
        assert_eq!(scheduler.running_count(), 0);

        let task = scheduler
            .next_task()
            .ok_or_else(|| Box::<dyn std::error::Error>::from("should have task"))?;
        scheduler.mark_running(task, "worker-1".to_string());

        assert_eq!(scheduler.pending_count(), 1);
        assert_eq!(scheduler.running_count(), 1);

        scheduler.mark_completed(TaskId(2))?;

        assert_eq!(scheduler.running_count(), 0);
        assert_eq!(scheduler.completed_count(), 1);
        Ok(())
    }

    #[test]
    fn test_task_context() {
        let ctx = TaskContext::new(TaskId(1), "worker-1".to_string())
            .with_memory_limit(2 * 1024 * 1024 * 1024)
            .with_num_cores(4);

        assert_eq!(ctx.task_id, TaskId(1));
        assert_eq!(ctx.worker_id, "worker-1");
        assert_eq!(ctx.memory_limit, 2 * 1024 * 1024 * 1024);
        assert_eq!(ctx.num_cores, 4);
    }
}
