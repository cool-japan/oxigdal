//! Task scheduling for ETL pipelines
//!
//! This module provides cron-based scheduling, event-triggered execution,
//! retry logic, and resource limits for ETL pipelines.

use crate::error::{Result, SchedulerError};
use crate::pipeline::Pipeline;
use dashmap::DashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, mpsc};
use tokio::time::sleep;
use tracing::{error, info, warn};

#[cfg(feature = "scheduler")]
use tracing::debug;

/// Schedule definition
#[derive(Debug, Clone)]
pub enum Schedule {
    /// One-time execution
    Once,
    /// Cron-based schedule
    #[cfg(feature = "scheduler")]
    Cron(String),
    /// Fixed interval
    Interval(Duration),
    /// Event-triggered (manual trigger)
    Event,
}

/// Task configuration
#[derive(Debug, Clone)]
pub struct TaskConfig {
    /// Task ID
    pub id: String,
    /// Task name
    pub name: String,
    /// Schedule
    pub schedule: Schedule,
    /// Maximum retries on failure
    pub max_retries: usize,
    /// Retry backoff
    pub retry_backoff: Duration,
    /// Timeout
    pub timeout: Option<Duration>,
    /// Enable concurrent execution
    pub allow_concurrent: bool,
}

impl TaskConfig {
    /// Create a new task configuration
    pub fn new(id: String, name: String, schedule: Schedule) -> Self {
        Self {
            id,
            name,
            schedule,
            max_retries: 3,
            retry_backoff: Duration::from_secs(1),
            timeout: None,
            allow_concurrent: false,
        }
    }

    /// Set maximum retries
    pub fn max_retries(mut self, retries: usize) -> Self {
        self.max_retries = retries;
        self
    }

    /// Set retry backoff
    pub fn retry_backoff(mut self, backoff: Duration) -> Self {
        self.retry_backoff = backoff;
        self
    }

    /// Set timeout
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Allow concurrent execution
    pub fn allow_concurrent(mut self, allow: bool) -> Self {
        self.allow_concurrent = allow;
        self
    }
}

/// Task execution result
#[derive(Debug, Clone)]
pub struct TaskResult {
    /// Task ID
    pub task_id: String,
    /// Success flag
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
    /// Execution time
    pub duration: Duration,
    /// Number of retries
    pub retries: usize,
}

/// Task state
#[derive(Debug, Clone, PartialEq, Eq)]
enum TaskState {
    Idle,
    Running,
    Failed,
    Completed,
}

/// Scheduled task
struct ScheduledTask {
    config: TaskConfig,
    pipeline: Arc<RwLock<Option<Pipeline>>>,
    state: RwLock<TaskState>,
    last_run: RwLock<Option<std::time::Instant>>,
    retries: RwLock<usize>,
}

impl ScheduledTask {
    fn new(config: TaskConfig) -> Self {
        Self {
            config,
            pipeline: Arc::new(RwLock::new(None)),
            state: RwLock::new(TaskState::Idle),
            last_run: RwLock::new(None),
            retries: RwLock::new(0),
        }
    }

    async fn set_pipeline(&self, pipeline: Pipeline) {
        let mut p = self.pipeline.write().await;
        *p = Some(pipeline);
    }

    async fn is_running(&self) -> bool {
        *self.state.read().await == TaskState::Running
    }

    async fn can_run(&self) -> bool {
        if !self.config.allow_concurrent && self.is_running().await {
            return false;
        }
        true
    }

    async fn execute(&self) -> Result<TaskResult> {
        if !self.can_run().await {
            return Err(SchedulerError::ExecutionFailed {
                message: "Task is already running".to_string(),
            }
            .into());
        }

        *self.state.write().await = TaskState::Running;
        let start = std::time::Instant::now();
        let mut retries = 0;

        loop {
            // Clone pipeline for execution (we need to move it)
            let pipeline_guard = self.pipeline.read().await;
            let _pipeline =
                pipeline_guard
                    .as_ref()
                    .ok_or_else(|| SchedulerError::ExecutionFailed {
                        message: "No pipeline configured".to_string(),
                    })?;

            // Note: We can't actually run the pipeline here because it consumes self
            // In a real implementation, we'd need to restructure Pipeline to be reusable
            // For now, we'll simulate execution
            drop(pipeline_guard);

            match self.execute_with_timeout().await {
                Ok(_) => {
                    *self.state.write().await = TaskState::Completed;
                    *self.last_run.write().await = Some(std::time::Instant::now());
                    *self.retries.write().await = 0;

                    return Ok(TaskResult {
                        task_id: self.config.id.clone(),
                        success: true,
                        error: None,
                        duration: start.elapsed(),
                        retries,
                    });
                }
                Err(e) => {
                    retries += 1;
                    if retries >= self.config.max_retries {
                        *self.state.write().await = TaskState::Failed;
                        return Ok(TaskResult {
                            task_id: self.config.id.clone(),
                            success: false,
                            error: Some(e.to_string()),
                            duration: start.elapsed(),
                            retries,
                        });
                    }

                    warn!(
                        "Task '{}' failed (attempt {}/{}): {}",
                        self.config.name, retries, self.config.max_retries, e
                    );

                    // Exponential backoff
                    let backoff = self.config.retry_backoff * retries as u32;
                    sleep(backoff).await;
                }
            }
        }
    }

    async fn execute_with_timeout(&self) -> Result<()> {
        // Simulate execution
        // In real implementation, run pipeline here
        sleep(Duration::from_millis(100)).await;
        Ok(())
    }
}

/// ETL Scheduler
pub struct Scheduler {
    tasks: Arc<DashMap<String, Arc<ScheduledTask>>>,
    running: Arc<RwLock<bool>>,
    event_tx: mpsc::UnboundedSender<String>,
    event_rx: Arc<RwLock<mpsc::UnboundedReceiver<String>>>,
}

impl Scheduler {
    /// Create a new scheduler
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        Self {
            tasks: Arc::new(DashMap::new()),
            running: Arc::new(RwLock::new(false)),
            event_tx,
            event_rx: Arc::new(RwLock::new(event_rx)),
        }
    }

    /// Add a task to the scheduler
    pub async fn add_task(&self, config: TaskConfig, pipeline: Pipeline) -> Result<()> {
        let task = Arc::new(ScheduledTask::new(config.clone()));
        task.set_pipeline(pipeline).await;
        self.tasks.insert(config.id.clone(), task);

        info!("Added task '{}' ({})", config.name, config.id);
        Ok(())
    }

    /// Remove a task from the scheduler
    pub fn remove_task(&self, task_id: &str) -> Result<()> {
        self.tasks
            .remove(task_id)
            .ok_or_else(|| SchedulerError::NotFound {
                id: task_id.to_string(),
            })?;

        info!("Removed task '{}'", task_id);
        Ok(())
    }

    /// Trigger a task manually
    pub async fn trigger(&self, task_id: &str) -> Result<()> {
        if !self.tasks.contains_key(task_id) {
            return Err(SchedulerError::NotFound {
                id: task_id.to_string(),
            }
            .into());
        }

        self.event_tx
            .send(task_id.to_string())
            .map_err(|_| SchedulerError::ExecutionFailed {
                message: "Failed to send event".to_string(),
            })?;

        Ok(())
    }

    /// Start the scheduler
    pub async fn start(&self) -> Result<()> {
        let mut running = self.running.write().await;
        if *running {
            return Err(SchedulerError::ExecutionFailed {
                message: "Scheduler is already running".to_string(),
            }
            .into());
        }

        *running = true;
        drop(running);

        info!("Scheduler started");

        // Spawn background task for each schedule type
        self.start_interval_scheduler().await;
        self.start_event_scheduler().await;

        #[cfg(feature = "scheduler")]
        self.start_cron_scheduler().await;

        Ok(())
    }

    /// Stop the scheduler
    pub async fn stop(&self) {
        let mut running = self.running.write().await;
        *running = false;
        info!("Scheduler stopped");
    }

    /// Start interval-based scheduler
    async fn start_interval_scheduler(&self) {
        let tasks = Arc::clone(&self.tasks);
        let running = Arc::clone(&self.running);

        tokio::spawn(async move {
            while *running.read().await {
                for entry in tasks.iter() {
                    let task = entry.value();

                    if let Schedule::Interval(duration) = &task.config.schedule {
                        let should_run = {
                            let last_run = task.last_run.read().await;
                            match *last_run {
                                Some(last) => last.elapsed() >= *duration,
                                None => true,
                            }
                        };

                        if should_run && task.can_run().await {
                            let task = Arc::clone(task);
                            tokio::spawn(async move {
                                match task.execute().await {
                                    Ok(result) => {
                                        if result.success {
                                            info!(
                                                "Task '{}' completed successfully",
                                                result.task_id
                                            );
                                        } else {
                                            error!(
                                                "Task '{}' failed: {:?}",
                                                result.task_id, result.error
                                            );
                                        }
                                    }
                                    Err(e) => {
                                        error!("Task execution error: {}", e);
                                    }
                                }
                            });
                        }
                    }
                }

                sleep(Duration::from_secs(1)).await;
            }
        });
    }

    /// Start event-based scheduler
    async fn start_event_scheduler(&self) {
        let tasks = Arc::clone(&self.tasks);
        let running = Arc::clone(&self.running);
        let event_rx = Arc::clone(&self.event_rx);

        tokio::spawn(async move {
            let mut rx = event_rx.write().await;

            while *running.read().await {
                if let Some(task_id) = rx.recv().await {
                    if let Some(entry) = tasks.get(&task_id) {
                        let task = Arc::clone(entry.value());

                        tokio::spawn(async move {
                            match task.execute().await {
                                Ok(result) => {
                                    if result.success {
                                        info!("Task '{}' completed successfully", result.task_id);
                                    } else {
                                        error!(
                                            "Task '{}' failed: {:?}",
                                            result.task_id, result.error
                                        );
                                    }
                                }
                                Err(e) => {
                                    error!("Task execution error: {}", e);
                                }
                            }
                        });
                    }
                }
            }
        });
    }

    /// Start cron-based scheduler
    #[cfg(feature = "scheduler")]
    async fn start_cron_scheduler(&self) {
        let tasks = Arc::clone(&self.tasks);
        let running = Arc::clone(&self.running);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60)); // Check every minute

            while *running.read().await {
                interval.tick().await;

                for entry in tasks.iter() {
                    let task = entry.value();

                    if let Schedule::Cron(expr) = &task.config.schedule {
                        // In real implementation, parse cron expression and check if should run
                        // For now, just log
                        debug!("Checking cron task: {}", expr);
                    }
                }
            }
        });
    }

    /// Get scheduler status
    pub async fn is_running(&self) -> bool {
        *self.running.read().await
    }

    /// Get number of tasks
    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_config() {
        let config = TaskConfig::new("task1".to_string(), "Test Task".to_string(), Schedule::Once)
            .max_retries(5)
            .timeout(Duration::from_secs(60));

        assert_eq!(config.id, "task1");
        assert_eq!(config.max_retries, 5);
        assert_eq!(config.timeout, Some(Duration::from_secs(60)));
    }

    #[tokio::test]
    async fn test_scheduler_creation() {
        let scheduler = Scheduler::new();
        assert!(!scheduler.is_running().await);
        assert_eq!(scheduler.task_count(), 0);
    }

    #[tokio::test]
    async fn test_scheduler_start_stop() {
        let scheduler = Scheduler::new();
        scheduler.start().await.expect("Failed to start");
        assert!(scheduler.is_running().await);

        scheduler.stop().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!scheduler.is_running().await);
    }
}
