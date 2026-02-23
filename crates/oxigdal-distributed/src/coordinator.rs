//! Coordinator for managing distributed task execution.
//!
//! This module implements the coordinator that schedules tasks across worker nodes,
//! monitors progress, and aggregates results.

use crate::error::{DistributedError, Result};
use crate::task::{PartitionId, Task, TaskId, TaskOperation, TaskResult, TaskScheduler};
use crate::worker::WorkerStatus;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// Coordinator configuration.
#[derive(Debug, Clone)]
pub struct CoordinatorConfig {
    /// Listen address for Flight server.
    pub listen_addr: String,
    /// Maximum task retry attempts.
    pub max_retries: u32,
    /// Task timeout in seconds.
    pub task_timeout_secs: u64,
    /// Worker heartbeat timeout in seconds.
    pub worker_timeout_secs: u64,
    /// Result buffer size.
    pub result_buffer_size: usize,
}

impl CoordinatorConfig {
    /// Create a new coordinator configuration.
    pub fn new(listen_addr: String) -> Self {
        Self {
            listen_addr,
            max_retries: 3,
            task_timeout_secs: 300, // 5 minutes
            worker_timeout_secs: 60,
            result_buffer_size: 1000,
        }
    }

    /// Set the maximum retry attempts.
    pub fn with_max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

    /// Set the task timeout.
    pub fn with_task_timeout(mut self, timeout_secs: u64) -> Self {
        self.task_timeout_secs = timeout_secs;
        self
    }
}

/// Information about a connected worker.
#[derive(Debug, Clone)]
pub struct WorkerInfo {
    /// Worker identifier.
    pub worker_id: String,
    /// Worker address.
    pub address: String,
    /// Current status.
    pub status: WorkerStatus,
    /// Last heartbeat timestamp.
    pub last_heartbeat: Instant,
    /// Number of active tasks.
    pub active_tasks: usize,
    /// Total tasks completed.
    pub completed_tasks: u64,
    /// Total tasks failed.
    pub failed_tasks: u64,
}

impl WorkerInfo {
    /// Create new worker info.
    pub fn new(worker_id: String, address: String) -> Self {
        Self {
            worker_id,
            address,
            status: WorkerStatus::Idle,
            last_heartbeat: Instant::now(),
            active_tasks: 0,
            completed_tasks: 0,
            failed_tasks: 0,
        }
    }

    /// Update heartbeat timestamp.
    pub fn update_heartbeat(&mut self) {
        self.last_heartbeat = Instant::now();
    }

    /// Check if the worker has timed out.
    pub fn is_timed_out(&self, timeout: Duration) -> bool {
        self.last_heartbeat.elapsed() > timeout
    }

    /// Get the success rate.
    pub fn success_rate(&self) -> f64 {
        let total = self.completed_tasks + self.failed_tasks;
        if total == 0 {
            1.0
        } else {
            self.completed_tasks as f64 / total as f64
        }
    }
}

/// Coordinator for distributed task execution.
pub struct Coordinator {
    /// Coordinator configuration.
    config: CoordinatorConfig,
    /// Task scheduler.
    scheduler: Arc<RwLock<TaskScheduler>>,
    /// Connected workers.
    workers: Arc<RwLock<HashMap<String, WorkerInfo>>>,
    /// Task assignments (task_id -> worker_id).
    assignments: Arc<RwLock<HashMap<TaskId, String>>>,
    /// Task results.
    results: Arc<RwLock<HashMap<TaskId, TaskResult>>>,
    /// Task counter for generating unique IDs.
    next_task_id: Arc<RwLock<u64>>,
}

impl Coordinator {
    /// Create a new coordinator.
    pub fn new(config: CoordinatorConfig) -> Self {
        Self {
            config,
            scheduler: Arc::new(RwLock::new(TaskScheduler::new())),
            workers: Arc::new(RwLock::new(HashMap::new())),
            assignments: Arc::new(RwLock::new(HashMap::new())),
            results: Arc::new(RwLock::new(HashMap::new())),
            next_task_id: Arc::new(RwLock::new(0)),
        }
    }

    /// Add a worker to the coordinator.
    pub fn add_worker(&self, worker_id: String, address: String) -> Result<()> {
        info!("Adding worker: {} at {}", worker_id, address);

        let worker_info = WorkerInfo::new(worker_id.clone(), address);

        let mut workers = self
            .workers
            .write()
            .map_err(|_| DistributedError::coordinator("Failed to acquire workers lock"))?;

        if workers.contains_key(&worker_id) {
            return Err(DistributedError::coordinator(format!(
                "Worker {} already exists",
                worker_id
            )));
        }

        workers.insert(worker_id, worker_info);
        Ok(())
    }

    /// Remove a worker from the coordinator.
    pub fn remove_worker(&self, worker_id: &str) -> Result<()> {
        info!("Removing worker: {}", worker_id);

        let mut workers = self
            .workers
            .write()
            .map_err(|_| DistributedError::coordinator("Failed to acquire workers lock"))?;

        workers.remove(worker_id);

        // Reassign tasks from this worker
        self.reassign_worker_tasks(worker_id)?;

        Ok(())
    }

    /// Update worker heartbeat.
    pub fn update_worker_heartbeat(&self, worker_id: &str) -> Result<()> {
        let mut workers = self
            .workers
            .write()
            .map_err(|_| DistributedError::coordinator("Failed to acquire workers lock"))?;

        if let Some(worker) = workers.get_mut(worker_id) {
            worker.update_heartbeat();
            debug!("Updated heartbeat for worker {}", worker_id);
            Ok(())
        } else {
            Err(DistributedError::coordinator(format!(
                "Worker {} not found",
                worker_id
            )))
        }
    }

    /// Check for timed-out workers and reassign their tasks.
    pub fn check_worker_timeouts(&self) -> Result<Vec<String>> {
        let timeout = Duration::from_secs(self.config.worker_timeout_secs);
        let mut timed_out = Vec::new();

        let workers = self
            .workers
            .read()
            .map_err(|_| DistributedError::coordinator("Failed to acquire workers lock"))?;

        for (worker_id, worker) in workers.iter() {
            if worker.is_timed_out(timeout) {
                warn!("Worker {} has timed out", worker_id);
                timed_out.push(worker_id.clone());
            }
        }

        drop(workers);

        // Reassign tasks from timed-out workers
        for worker_id in &timed_out {
            self.reassign_worker_tasks(worker_id)?;
            self.remove_worker(worker_id)?;
        }

        Ok(timed_out)
    }

    /// Submit a task for execution.
    pub fn submit_task(
        &self,
        partition_id: PartitionId,
        operation: TaskOperation,
    ) -> Result<TaskId> {
        let task_id = self.generate_task_id()?;
        let mut task = Task::new(task_id, partition_id, operation);
        task.max_retries = self.config.max_retries;

        let mut scheduler = self
            .scheduler
            .write()
            .map_err(|_| DistributedError::coordinator("Failed to acquire scheduler lock"))?;

        scheduler.add_task(task);
        debug!("Submitted task {}", task_id);

        Ok(task_id)
    }

    /// Get the next task to execute.
    pub fn next_task(&self) -> Result<Option<Task>> {
        let mut scheduler = self
            .scheduler
            .write()
            .map_err(|_| DistributedError::coordinator("Failed to acquire scheduler lock"))?;

        Ok(scheduler.next_task())
    }

    /// Assign a task to a worker.
    pub fn assign_task(&self, task: Task, worker_id: String) -> Result<()> {
        // Mark task as running
        let mut scheduler = self
            .scheduler
            .write()
            .map_err(|_| DistributedError::coordinator("Failed to acquire scheduler lock"))?;
        scheduler.mark_running(task.clone(), worker_id.clone());
        drop(scheduler);

        // Record assignment
        let mut assignments = self
            .assignments
            .write()
            .map_err(|_| DistributedError::coordinator("Failed to acquire assignments lock"))?;
        assignments.insert(task.id, worker_id.clone());

        // Update worker info
        let mut workers = self
            .workers
            .write()
            .map_err(|_| DistributedError::coordinator("Failed to acquire workers lock"))?;
        if let Some(worker) = workers.get_mut(&worker_id) {
            worker.active_tasks += 1;
            worker.status = WorkerStatus::Busy;
        }

        info!("Assigned task {} to worker {}", task.id, worker_id);
        Ok(())
    }

    /// Record task completion.
    pub fn complete_task(&self, task_id: TaskId, result: TaskResult) -> Result<()> {
        let worker_id = {
            let assignments = self
                .assignments
                .read()
                .map_err(|_| DistributedError::coordinator("Failed to acquire assignments lock"))?;
            assignments.get(&task_id).cloned()
        };

        // Update scheduler
        let mut scheduler = self
            .scheduler
            .write()
            .map_err(|_| DistributedError::coordinator("Failed to acquire scheduler lock"))?;

        if result.is_success() {
            scheduler.mark_completed(task_id)?;
        } else {
            scheduler.mark_failed(task_id)?;
        }
        drop(scheduler);

        // Update worker info
        if let Some(worker_id) = worker_id {
            let mut workers = self
                .workers
                .write()
                .map_err(|_| DistributedError::coordinator("Failed to acquire workers lock"))?;

            if let Some(worker) = workers.get_mut(&worker_id) {
                if worker.active_tasks > 0 {
                    worker.active_tasks -= 1;
                }
                if result.is_success() {
                    worker.completed_tasks += 1;
                } else {
                    worker.failed_tasks += 1;
                }
                if worker.active_tasks == 0 {
                    worker.status = WorkerStatus::Idle;
                }
            }
        }

        // Store result
        let mut results = self
            .results
            .write()
            .map_err(|_| DistributedError::coordinator("Failed to acquire results lock"))?;
        results.insert(task_id, result);

        info!("Task {} completed", task_id);
        Ok(())
    }

    /// Get the best available worker for a task.
    pub fn get_available_worker(&self) -> Result<Option<String>> {
        let workers = self
            .workers
            .read()
            .map_err(|_| DistributedError::coordinator("Failed to acquire workers lock"))?;

        // Find idle worker with best success rate
        let best_worker = workers
            .values()
            .filter(|w| w.status == WorkerStatus::Idle)
            .max_by(|a, b| {
                a.success_rate()
                    .partial_cmp(&b.success_rate())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|w| w.worker_id.clone());

        Ok(best_worker)
    }

    /// Get execution progress.
    pub fn get_progress(&self) -> Result<CoordinatorProgress> {
        let scheduler = self
            .scheduler
            .read()
            .map_err(|_| DistributedError::coordinator("Failed to acquire scheduler lock"))?;

        let workers = self
            .workers
            .read()
            .map_err(|_| DistributedError::coordinator("Failed to acquire workers lock"))?;

        Ok(CoordinatorProgress {
            pending_tasks: scheduler.pending_count(),
            running_tasks: scheduler.running_count(),
            completed_tasks: scheduler.completed_count(),
            failed_tasks: scheduler.failed_count(),
            active_workers: workers.len(),
            idle_workers: workers
                .values()
                .filter(|w| w.status == WorkerStatus::Idle)
                .count(),
        })
    }

    /// Collect all task results.
    pub fn collect_results(&self) -> Result<Vec<TaskResult>> {
        let results = self
            .results
            .read()
            .map_err(|_| DistributedError::coordinator("Failed to acquire results lock"))?;

        Ok(results.values().cloned().collect())
    }

    /// Check if all tasks are complete.
    pub fn is_complete(&self) -> bool {
        self.scheduler
            .read()
            .map(|s| s.is_complete())
            .unwrap_or(false)
    }

    /// Generate a unique task ID.
    fn generate_task_id(&self) -> Result<TaskId> {
        let mut next_id = self
            .next_task_id
            .write()
            .map_err(|_| DistributedError::coordinator("Failed to acquire task ID lock"))?;
        let id = *next_id;
        *next_id += 1;
        Ok(TaskId(id))
    }

    /// Reassign tasks from a specific worker.
    fn reassign_worker_tasks(&self, worker_id: &str) -> Result<()> {
        let mut scheduler = self
            .scheduler
            .write()
            .map_err(|_| DistributedError::coordinator("Failed to acquire scheduler lock"))?;

        let mut assignments = self
            .assignments
            .write()
            .map_err(|_| DistributedError::coordinator("Failed to acquire assignments lock"))?;

        // Find tasks assigned to this worker
        let task_ids: Vec<TaskId> = assignments
            .iter()
            .filter(|(_, wid)| *wid == worker_id)
            .map(|(tid, _)| *tid)
            .collect();

        // Mark them as failed (will be retried if possible)
        for task_id in task_ids {
            let _ = scheduler.mark_failed(task_id);
            assignments.remove(&task_id);
        }

        Ok(())
    }

    /// Get list of all workers.
    pub fn list_workers(&self) -> Result<Vec<WorkerInfo>> {
        let workers = self
            .workers
            .read()
            .map_err(|_| DistributedError::coordinator("Failed to acquire workers lock"))?;

        Ok(workers.values().cloned().collect())
    }

    /// Start monitoring loop for worker health.
    pub async fn start_monitoring(
        self: Arc<Self>,
        mut shutdown_rx: mpsc::Receiver<()>,
    ) -> Result<()> {
        info!("Starting coordinator monitoring loop");

        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(10));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(e) = self.check_worker_timeouts() {
                        error!("Error checking worker timeouts: {}", e);
                    }

                    let progress = self.get_progress().unwrap_or_default();
                    debug!("Progress: {:?}", progress);
                }
                _ = shutdown_rx.recv() => {
                    info!("Coordinator monitoring loop shutting down");
                    break;
                }
            }
        }

        Ok(())
    }
}

/// Progress information for the coordinator.
#[derive(Debug, Clone, Default)]
pub struct CoordinatorProgress {
    /// Number of pending tasks.
    pub pending_tasks: usize,
    /// Number of running tasks.
    pub running_tasks: usize,
    /// Number of completed tasks.
    pub completed_tasks: usize,
    /// Number of failed tasks.
    pub failed_tasks: usize,
    /// Number of active workers.
    pub active_workers: usize,
    /// Number of idle workers.
    pub idle_workers: usize,
}

impl CoordinatorProgress {
    /// Get the total number of tasks.
    pub fn total_tasks(&self) -> usize {
        self.pending_tasks + self.running_tasks + self.completed_tasks + self.failed_tasks
    }

    /// Get the completion percentage.
    pub fn completion_percentage(&self) -> f64 {
        let total = self.total_tasks();
        if total == 0 {
            0.0
        } else {
            (self.completed_tasks as f64 / total as f64) * 100.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coordinator_config() {
        let config = CoordinatorConfig::new("localhost:50051".to_string())
            .with_max_retries(5)
            .with_task_timeout(600);

        assert_eq!(config.listen_addr, "localhost:50051");
        assert_eq!(config.max_retries, 5);
        assert_eq!(config.task_timeout_secs, 600);
    }

    #[test]
    fn test_worker_info() {
        let mut info = WorkerInfo::new("worker-1".to_string(), "localhost:50052".to_string());

        info.completed_tasks = 8;
        info.failed_tasks = 2;

        assert_eq!(info.success_rate(), 0.8);
        assert!(!info.is_timed_out(Duration::from_secs(60)));
    }

    #[test]
    fn test_coordinator_creation() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let config = CoordinatorConfig::new("localhost:50051".to_string());
        let coordinator = Coordinator::new(config);

        let progress = coordinator.get_progress()?;
        assert_eq!(progress.total_tasks(), 0);
        assert_eq!(progress.active_workers, 0);
        Ok(())
    }

    #[test]
    fn test_add_worker() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let config = CoordinatorConfig::new("localhost:50051".to_string());
        let coordinator = Coordinator::new(config);

        coordinator.add_worker("worker-1".to_string(), "localhost:50052".to_string())?;

        let workers = coordinator.list_workers()?;
        assert_eq!(workers.len(), 1);
        assert_eq!(workers[0].worker_id, "worker-1");
        Ok(())
    }

    #[test]
    fn test_submit_task() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let config = CoordinatorConfig::new("localhost:50051".to_string());
        let coordinator = Coordinator::new(config);

        let task_id = coordinator.submit_task(
            PartitionId(0),
            TaskOperation::Filter {
                expression: "value > 10".to_string(),
            },
        )?;

        assert_eq!(task_id, TaskId(0));

        let progress = coordinator.get_progress()?;
        assert_eq!(progress.pending_tasks, 1);
        Ok(())
    }

    #[test]
    fn test_progress() {
        let progress = CoordinatorProgress {
            pending_tasks: 10,
            running_tasks: 5,
            completed_tasks: 30,
            failed_tasks: 5,
            active_workers: 4,
            idle_workers: 2,
        };

        assert_eq!(progress.total_tasks(), 50);
        assert_eq!(progress.completion_percentage(), 60.0);
    }
}
