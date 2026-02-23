//! Worker node implementation for distributed processing.
//!
//! This module implements worker nodes that execute geospatial processing tasks
//! assigned by the coordinator.

use crate::error::{DistributedError, Result};
use crate::task::{Task, TaskContext, TaskId, TaskOperation, TaskResult};
use arrow::record_batch::RecordBatch;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// Worker node configuration.
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// Unique worker identifier.
    pub worker_id: String,
    /// Maximum number of concurrent tasks.
    pub max_concurrent_tasks: usize,
    /// Memory limit in bytes.
    pub memory_limit: u64,
    /// Number of CPU cores available.
    pub num_cores: usize,
    /// Heartbeat interval in seconds.
    pub heartbeat_interval_secs: u64,
}

impl WorkerConfig {
    /// Create a new worker configuration.
    pub fn new(worker_id: String) -> Self {
        let num_cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);

        Self {
            worker_id,
            max_concurrent_tasks: num_cores,
            memory_limit: 4 * 1024 * 1024 * 1024, // 4 GB default
            num_cores,
            heartbeat_interval_secs: 30,
        }
    }

    /// Set the maximum number of concurrent tasks.
    pub fn with_max_concurrent_tasks(mut self, max: usize) -> Self {
        self.max_concurrent_tasks = max;
        self
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

/// Worker node status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerStatus {
    /// Worker is idle and ready for tasks.
    Idle,
    /// Worker is executing tasks.
    Busy,
    /// Worker is shutting down.
    ShuttingDown,
    /// Worker is offline.
    Offline,
}

/// Worker resource metrics.
#[derive(Debug, Clone, Default)]
pub struct WorkerMetrics {
    /// Total tasks executed.
    pub tasks_executed: u64,
    /// Total tasks succeeded.
    pub tasks_succeeded: u64,
    /// Total tasks failed.
    pub tasks_failed: u64,
    /// Total execution time in milliseconds.
    pub total_execution_time_ms: u64,
    /// Current memory usage in bytes.
    pub memory_usage: u64,
    /// Number of active tasks.
    pub active_tasks: u64,
}

impl WorkerMetrics {
    /// Record a successful task execution.
    pub fn record_success(&mut self, execution_time_ms: u64) {
        self.tasks_executed += 1;
        self.tasks_succeeded += 1;
        self.total_execution_time_ms += execution_time_ms;
    }

    /// Record a failed task execution.
    pub fn record_failure(&mut self, execution_time_ms: u64) {
        self.tasks_executed += 1;
        self.tasks_failed += 1;
        self.total_execution_time_ms += execution_time_ms;
    }

    /// Get the success rate.
    pub fn success_rate(&self) -> f64 {
        if self.tasks_executed == 0 {
            0.0
        } else {
            self.tasks_succeeded as f64 / self.tasks_executed as f64
        }
    }

    /// Get the average execution time.
    pub fn avg_execution_time_ms(&self) -> f64 {
        if self.tasks_executed == 0 {
            0.0
        } else {
            self.total_execution_time_ms as f64 / self.tasks_executed as f64
        }
    }
}

/// Worker node for executing distributed tasks.
pub struct Worker {
    /// Worker configuration.
    config: WorkerConfig,
    /// Current status.
    status: Arc<RwLock<WorkerStatus>>,
    /// Worker metrics.
    metrics: Arc<RwLock<WorkerMetrics>>,
    /// Currently running tasks.
    running_tasks: Arc<RwLock<HashMap<TaskId, Instant>>>,
    /// Shutdown signal.
    shutdown: Arc<AtomicBool>,
}

impl Worker {
    /// Create a new worker.
    pub fn new(config: WorkerConfig) -> Self {
        Self {
            config,
            status: Arc::new(RwLock::new(WorkerStatus::Idle)),
            metrics: Arc::new(RwLock::new(WorkerMetrics::default())),
            running_tasks: Arc::new(RwLock::new(HashMap::new())),
            shutdown: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Get the worker ID.
    pub fn worker_id(&self) -> &str {
        &self.config.worker_id
    }

    /// Get the current status.
    pub fn status(&self) -> WorkerStatus {
        self.status.read().map_or(WorkerStatus::Offline, |s| *s)
    }

    /// Get the current metrics.
    pub fn metrics(&self) -> WorkerMetrics {
        self.metrics
            .read()
            .map_or_else(|_| WorkerMetrics::default(), |m| m.clone())
    }

    /// Check if the worker is available for new tasks.
    pub fn is_available(&self) -> bool {
        let running_count = self.running_tasks.read().map_or(0, |r| r.len());
        running_count < self.config.max_concurrent_tasks
            && self.status() == WorkerStatus::Idle
            && !self.shutdown.load(Ordering::SeqCst)
    }

    /// Execute a task.
    pub async fn execute_task(&self, task: Task, data: Arc<RecordBatch>) -> Result<TaskResult> {
        // Check if shutdown was requested
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(DistributedError::worker_task_failure(
                "Worker is shutting down",
            ));
        }

        // Update status
        {
            let mut status = self.status.write().map_err(|_| {
                DistributedError::worker_task_failure("Failed to acquire status lock")
            })?;
            *status = WorkerStatus::Busy;
        }

        // Record task start
        {
            let mut running = self.running_tasks.write().map_err(|_| {
                DistributedError::worker_task_failure("Failed to acquire running tasks lock")
            })?;
            running.insert(task.id, Instant::now());
        }

        // Create task context
        let context = TaskContext::new(task.id, self.config.worker_id.clone())
            .with_memory_limit(self.config.memory_limit)
            .with_num_cores(self.config.num_cores);

        info!(
            "Worker {} executing task {:?}",
            self.config.worker_id, task.id
        );

        let start = Instant::now();

        // Execute the task operation
        let result = self
            .execute_operation(&task.operation, data, &context)
            .await;

        let execution_time_ms = start.elapsed().as_millis() as u64;

        // Remove from running tasks
        {
            let mut running = self.running_tasks.write().map_err(|_| {
                DistributedError::worker_task_failure("Failed to acquire running tasks lock")
            })?;
            running.remove(&task.id);
        }

        // Update metrics and status
        {
            let mut metrics = self.metrics.write().map_err(|_| {
                DistributedError::worker_task_failure("Failed to acquire metrics lock")
            })?;

            match &result {
                Ok(batch) => {
                    metrics.record_success(execution_time_ms);
                    info!(
                        "Worker {} completed task {:?} in {}ms",
                        self.config.worker_id, task.id, execution_time_ms
                    );

                    let task_result =
                        TaskResult::success(task.id, batch.clone(), execution_time_ms);

                    // Update status back to idle if no more tasks
                    if self.running_tasks.read().map_or(true, |r| r.is_empty()) {
                        if let Ok(mut status) = self.status.write() {
                            *status = WorkerStatus::Idle;
                        }
                    }

                    Ok(task_result)
                }
                Err(e) => {
                    metrics.record_failure(execution_time_ms);
                    error!(
                        "Worker {} failed task {:?}: {}",
                        self.config.worker_id, task.id, e
                    );

                    let task_result =
                        TaskResult::failure(task.id, e.to_string(), execution_time_ms);

                    // Update status back to idle if no more tasks
                    if self.running_tasks.read().map_or(true, |r| r.is_empty()) {
                        if let Ok(mut status) = self.status.write() {
                            *status = WorkerStatus::Idle;
                        }
                    }

                    Ok(task_result)
                }
            }
        }
    }

    /// Execute a specific operation.
    async fn execute_operation(
        &self,
        operation: &TaskOperation,
        data: Arc<RecordBatch>,
        _context: &TaskContext,
    ) -> Result<Arc<RecordBatch>> {
        match operation {
            TaskOperation::Filter { expression } => {
                debug!("Applying filter: {}", expression);
                // Placeholder: In real implementation, apply filter using Arrow compute
                Ok(data)
            }
            TaskOperation::CalculateIndex { index_type, bands } => {
                debug!("Calculating index: {} with bands {:?}", index_type, bands);
                // Placeholder: In real implementation, calculate the index
                Ok(data)
            }
            TaskOperation::Reproject { target_epsg } => {
                debug!("Reprojecting to EPSG:{}", target_epsg);
                // Placeholder: In real implementation, reproject using oxigdal-proj
                Ok(data)
            }
            TaskOperation::Resample {
                width,
                height,
                method,
            } => {
                debug!("Resampling to {}x{} using {}", width, height, method);
                // Placeholder: In real implementation, resample the raster
                Ok(data)
            }
            TaskOperation::Clip {
                min_x,
                min_y,
                max_x,
                max_y,
            } => {
                debug!(
                    "Clipping to bbox: [{}, {}, {}, {}]",
                    min_x, min_y, max_x, max_y
                );
                // Placeholder: In real implementation, clip to bbox
                Ok(data)
            }
            TaskOperation::Convolve {
                kernel,
                kernel_width,
                kernel_height,
            } => {
                debug!(
                    "Applying convolution with {}x{} kernel",
                    kernel_width, kernel_height
                );
                // Placeholder: In real implementation, apply convolution
                let _ = kernel; // Suppress unused warning
                Ok(data)
            }
            TaskOperation::Custom { name, params } => {
                debug!(
                    "Executing custom operation: {} with params: {}",
                    name, params
                );
                // Placeholder: In real implementation, execute custom operation
                Ok(data)
            }
        }
    }

    /// Start the worker's heartbeat loop.
    pub async fn start_heartbeat(&self, heartbeat_tx: mpsc::Sender<String>) -> Result<()> {
        let worker_id = self.config.worker_id.clone();
        let interval = self.config.heartbeat_interval_secs;
        let shutdown = self.shutdown.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(interval));

            loop {
                interval.tick().await;

                if shutdown.load(Ordering::SeqCst) {
                    debug!("Worker {} heartbeat loop shutting down", worker_id);
                    break;
                }

                if let Err(e) = heartbeat_tx.send(worker_id.clone()).await {
                    warn!("Failed to send heartbeat for worker {}: {}", worker_id, e);
                    break;
                }

                debug!("Worker {} sent heartbeat", worker_id);
            }
        });

        Ok(())
    }

    /// Initiate graceful shutdown.
    pub async fn shutdown(&self) -> Result<()> {
        info!("Worker {} initiating shutdown", self.config.worker_id);

        self.shutdown.store(true, Ordering::SeqCst);

        // Update status
        {
            let mut status = self.status.write().map_err(|_| {
                DistributedError::worker_task_failure("Failed to acquire status lock")
            })?;
            *status = WorkerStatus::ShuttingDown;
        }

        // Wait for running tasks to complete (with timeout)
        let timeout = tokio::time::Duration::from_secs(30);
        let start = Instant::now();

        while start.elapsed() < timeout {
            let running_count = self.running_tasks.read().map_or(0, |r| r.len());
            if running_count == 0 {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        // Final status update
        {
            let mut status = self.status.write().map_err(|_| {
                DistributedError::worker_task_failure("Failed to acquire status lock")
            })?;
            *status = WorkerStatus::Offline;
        }

        info!("Worker {} shutdown complete", self.config.worker_id);
        Ok(())
    }

    /// Get health check information.
    pub fn health_check(&self) -> WorkerHealthCheck {
        let metrics = self.metrics();
        let status = self.status();
        let running_count = self.running_tasks.read().map_or(0, |r| r.len());

        WorkerHealthCheck {
            worker_id: self.config.worker_id.clone(),
            status,
            is_healthy: status != WorkerStatus::Offline,
            active_tasks: running_count,
            total_tasks_executed: metrics.tasks_executed,
            success_rate: metrics.success_rate(),
            avg_execution_time_ms: metrics.avg_execution_time_ms(),
            memory_usage: metrics.memory_usage,
        }
    }
}

/// Health check information for a worker.
#[derive(Debug, Clone)]
pub struct WorkerHealthCheck {
    /// Worker identifier.
    pub worker_id: String,
    /// Current status.
    pub status: WorkerStatus,
    /// Whether the worker is healthy.
    pub is_healthy: bool,
    /// Number of active tasks.
    pub active_tasks: usize,
    /// Total tasks executed.
    pub total_tasks_executed: u64,
    /// Success rate (0.0 to 1.0).
    pub success_rate: f64,
    /// Average execution time in milliseconds.
    pub avg_execution_time_ms: f64,
    /// Current memory usage in bytes.
    pub memory_usage: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::PartitionId;
    use arrow::array::Int32Array;
    use arrow::datatypes::{DataType, Field, Schema};

    fn create_test_batch() -> std::result::Result<Arc<RecordBatch>, Box<dyn std::error::Error>> {
        let schema = Arc::new(Schema::new(vec![Field::new(
            "value",
            DataType::Int32,
            false,
        )]));

        let array = Int32Array::from(vec![1, 2, 3, 4, 5]);

        Ok(Arc::new(RecordBatch::try_new(
            schema,
            vec![Arc::new(array)],
        )?))
    }

    #[test]
    fn test_worker_config() {
        let config = WorkerConfig::new("worker-1".to_string())
            .with_max_concurrent_tasks(8)
            .with_memory_limit(8 * 1024 * 1024 * 1024);

        assert_eq!(config.worker_id, "worker-1");
        assert_eq!(config.max_concurrent_tasks, 8);
        assert_eq!(config.memory_limit, 8 * 1024 * 1024 * 1024);
    }

    #[test]
    fn test_worker_metrics() {
        let mut metrics = WorkerMetrics::default();

        metrics.record_success(100);
        metrics.record_success(200);
        metrics.record_failure(150);

        assert_eq!(metrics.tasks_executed, 3);
        assert_eq!(metrics.tasks_succeeded, 2);
        assert_eq!(metrics.tasks_failed, 1);
        assert_eq!(metrics.total_execution_time_ms, 450);
        assert_eq!(metrics.success_rate(), 2.0 / 3.0);
        assert_eq!(metrics.avg_execution_time_ms(), 150.0);
    }

    #[tokio::test]
    async fn test_worker_creation() {
        let config = WorkerConfig::new("worker-test".to_string());
        let worker = Worker::new(config);

        assert_eq!(worker.worker_id(), "worker-test");
        assert_eq!(worker.status(), WorkerStatus::Idle);
        assert!(worker.is_available());
    }

    #[tokio::test]
    async fn test_worker_execute_task() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let config = WorkerConfig::new("worker-test".to_string());
        let worker = Worker::new(config);

        let task = Task::new(
            TaskId(1),
            PartitionId(0),
            TaskOperation::Filter {
                expression: "value > 2".to_string(),
            },
        );

        let data = create_test_batch()?;
        let result = worker.execute_task(task, data).await;

        assert!(result.is_ok());
        let task_result = result?;
        assert!(task_result.is_success());
        Ok(())
    }

    #[tokio::test]
    async fn test_worker_health_check() {
        let config = WorkerConfig::new("worker-test".to_string());
        let worker = Worker::new(config);

        let health = worker.health_check();

        assert_eq!(health.worker_id, "worker-test");
        assert!(health.is_healthy);
        assert_eq!(health.active_tasks, 0);
        assert_eq!(health.total_tasks_executed, 0);
    }
}
