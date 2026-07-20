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
    /// Wall-clock creation time, used as the anchor for the first cron evaluation.
    #[cfg(feature = "scheduler")]
    created_at: chrono::DateTime<chrono::Utc>,
    /// Wall-clock time of the last cron-triggered fire, used to decide the next due time.
    #[cfg(feature = "scheduler")]
    last_cron_fire: RwLock<Option<chrono::DateTime<chrono::Utc>>>,
}

impl ScheduledTask {
    fn new(config: TaskConfig) -> Self {
        Self {
            config,
            pipeline: Arc::new(RwLock::new(None)),
            state: RwLock::new(TaskState::Idle),
            last_run: RwLock::new(None),
            retries: RwLock::new(0),
            #[cfg(feature = "scheduler")]
            created_at: chrono::Utc::now(),
            #[cfg(feature = "scheduler")]
            last_cron_fire: RwLock::new(None),
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

        // Fail fast on misconfiguration rather than burning the whole retry budget on a task
        // that can never succeed.
        if self.pipeline.read().await.is_none() {
            return Err(SchedulerError::ExecutionFailed {
                message: "No pipeline configured".to_string(),
            }
            .into());
        }

        *self.state.write().await = TaskState::Running;
        let start = std::time::Instant::now();
        let mut retries = 0;

        loop {
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

    /// Actually execute the configured pipeline, honoring `config.timeout`.
    ///
    /// The pipeline is run in place through [`Pipeline::run_ref`] (no `self`-consuming clone
    /// needed), and its real `Result` is propagated so a failing pipeline surfaces as a failed
    /// task rather than a fabricated success. When a timeout is configured, exceeding it maps to
    /// a [`SchedulerError::ExecutionFailed`] that feeds the retry loop.
    async fn execute_with_timeout(&self) -> Result<()> {
        let pipeline_guard = self.pipeline.read().await;
        let pipeline = pipeline_guard
            .as_ref()
            .ok_or_else(|| SchedulerError::ExecutionFailed {
                message: "No pipeline configured".to_string(),
            })?;

        match self.config.timeout {
            Some(timeout) => match tokio::time::timeout(timeout, pipeline.run_ref()).await {
                Ok(result) => {
                    result?;
                    Ok(())
                }
                Err(_) => Err(SchedulerError::ExecutionFailed {
                    message: format!("Pipeline execution timed out after {:?}", timeout),
                }
                .into()),
            },
            None => {
                pipeline.run_ref().await?;
                Ok(())
            }
        }
    }
}

/// Returns `true` if a cron `schedule` has a firing instant in the window `(anchor, now]`.
///
/// The next occurrence strictly after `anchor` is computed; the task is due when that instant is
/// at or before `now`. Anchoring on the last fire (or task creation) guarantees each scheduled
/// instant is triggered exactly once regardless of the poll cadence.
#[cfg(feature = "scheduler")]
fn cron_is_due(
    schedule: &cron::Schedule,
    anchor: chrono::DateTime<chrono::Utc>,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    schedule
        .after(&anchor)
        .next()
        .is_some_and(|next| next <= now)
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
                if let Some(task_id) = rx.recv().await
                    && let Some(entry) = tasks.get(&task_id)
                {
                    let task = Arc::clone(entry.value());

                    tokio::spawn(async move {
                        match task.execute().await {
                            Ok(result) => {
                                if result.success {
                                    info!("Task '{}' completed successfully", result.task_id);
                                } else {
                                    error!("Task '{}' failed: {:?}", result.task_id, result.error);
                                }
                            }
                            Err(e) => {
                                error!("Task execution error: {}", e);
                            }
                        }
                    });
                }
            }
        });
    }

    /// Start cron-based scheduler
    #[cfg(feature = "scheduler")]
    async fn start_cron_scheduler(&self) {
        use std::str::FromStr;

        let tasks = Arc::clone(&self.tasks);
        let running = Arc::clone(&self.running);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60)); // Check every minute

            while *running.read().await {
                interval.tick().await;

                for entry in tasks.iter() {
                    let task = entry.value();

                    let Schedule::Cron(expr) = &task.config.schedule else {
                        continue;
                    };

                    // Parse the cron expression. An invalid expression is logged and skipped
                    // rather than aborting the whole cron loop for every task.
                    let schedule = match cron::Schedule::from_str(expr) {
                        Ok(schedule) => schedule,
                        Err(e) => {
                            error!("Invalid cron expression '{}': {}", expr, e);
                            continue;
                        }
                    };

                    let now = chrono::Utc::now();
                    // Anchor the "next occurrence" search at the last fire (or task creation
                    // for the first evaluation) so each scheduled instant fires exactly once.
                    let anchor = task.last_cron_fire.read().await.unwrap_or(task.created_at);
                    let due = cron_is_due(&schedule, anchor, now);

                    debug!(
                        "Cron task '{}' expr='{}' anchor={} due={}",
                        task.config.name, expr, anchor, due
                    );

                    if due && task.can_run().await {
                        *task.last_cron_fire.write().await = Some(now);

                        let task = Arc::clone(task);
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

    // --- Real-pipeline execution regression tests -------------------------------------------

    use crate::sink::FileSink;
    use crate::source::{FileSource, Source};
    use crate::stream::{BoxStream, StreamItem};
    use std::io::Write;

    fn unique_temp_path(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("oxigeo-etl-sched-{}-{}", tag, nanos))
    }

    #[tokio::test]
    async fn test_execute_reports_success_for_valid_pipeline() {
        // A well-formed pipeline over an existing input file must report success:true and
        // actually move the bytes to the sink (proving the pipeline really ran).
        let input_path = unique_temp_path("ok-in");
        let output_path = unique_temp_path("ok-out");
        {
            let mut f = std::fs::File::create(&input_path).expect("create input");
            f.write_all(b"hello etl scheduler").expect("write input");
        }

        let pipeline = Pipeline::builder()
            .source(Box::new(FileSource::new(input_path.clone())))
            .sink(Box::new(FileSink::new(output_path.clone())))
            .build()
            .expect("build pipeline");

        let config = TaskConfig::new("ok".to_string(), "OK Task".to_string(), Schedule::Once);
        let task = ScheduledTask::new(config);
        task.set_pipeline(pipeline).await;

        let result = task.execute().await.expect("execute returns Ok");
        assert!(result.success, "valid pipeline should report success");
        assert!(result.error.is_none());

        let written = std::fs::read(&output_path).expect("read output");
        assert_eq!(written, b"hello etl scheduler");

        let _ = std::fs::remove_file(&input_path);
        let _ = std::fs::remove_file(&output_path);
    }

    #[tokio::test]
    async fn test_execute_reports_failure_when_pipeline_errors() {
        // Source file does not exist -> Pipeline::run_ref returns Err -> the task must report
        // success:false (NOT a fabricated success), after exhausting retries.
        let missing_input = unique_temp_path("missing-in");
        let output_path = unique_temp_path("fail-out");

        let pipeline = Pipeline::builder()
            .source(Box::new(FileSource::new(missing_input)))
            .sink(Box::new(FileSink::new(output_path.clone())))
            .build()
            .expect("build pipeline");

        let config = TaskConfig::new("fail".to_string(), "Fail Task".to_string(), Schedule::Once)
            .max_retries(2)
            .retry_backoff(Duration::from_millis(1));
        let task = ScheduledTask::new(config);
        task.set_pipeline(pipeline).await;

        let result = task
            .execute()
            .await
            .expect("execute returns Ok(TaskResult)");
        assert!(
            !result.success,
            "a failing pipeline must report success:false"
        );
        assert!(
            result.error.is_some(),
            "failure must carry an error message"
        );
        assert_eq!(result.retries, 2, "should have exhausted the retry budget");

        let _ = std::fs::remove_file(&output_path);
    }

    #[tokio::test]
    async fn test_execute_without_pipeline_errors() {
        // No pipeline configured -> execute must surface an error, never a success.
        let config = TaskConfig::new("nopipe".to_string(), "No Pipe".to_string(), Schedule::Once);
        let task = ScheduledTask::new(config);
        let result = task.execute().await;
        assert!(result.is_err(), "missing pipeline must be an error");
    }

    #[cfg(feature = "scheduler")]
    #[test]
    fn test_cron_is_due() {
        use std::str::FromStr;

        // Every day at 00:00.
        let schedule = cron::Schedule::from_str("0 0 0 * * * *").expect("valid cron");

        let anchor = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .expect("anchor")
            .with_timezone(&chrono::Utc);

        // `now` still on the same day before the next midnight -> not due yet.
        let before = chrono::DateTime::parse_from_rfc3339("2026-01-01T12:00:00Z")
            .expect("before")
            .with_timezone(&chrono::Utc);
        assert!(
            !cron_is_due(&schedule, anchor, before),
            "no midnight has elapsed since anchor"
        );

        // `now` past the next midnight -> due.
        let after = chrono::DateTime::parse_from_rfc3339("2026-01-02T00:00:01Z")
            .expect("after")
            .with_timezone(&chrono::Utc);
        assert!(
            cron_is_due(&schedule, anchor, after),
            "the 2026-01-02 midnight occurrence is due"
        );
    }

    /// A source that blocks far longer than the task timeout, to exercise timeout handling.
    struct SlowSource;

    #[async_trait::async_trait]
    impl Source for SlowSource {
        async fn stream(&self) -> Result<BoxStream<StreamItem>> {
            tokio::time::sleep(Duration::from_secs(30)).await;
            Ok(Box::pin(futures::stream::empty()))
        }

        fn name(&self) -> &str {
            "SlowSource"
        }

        async fn is_available(&self) -> bool {
            true
        }
    }

    #[tokio::test]
    async fn test_execute_honors_timeout() {
        // The pipeline would take 30s; a 50ms timeout must abort it and, after retries, report
        // failure with a timeout error rather than hanging or fabricating success.
        let output_path = unique_temp_path("timeout-out");

        let pipeline = Pipeline::builder()
            .source(Box::new(SlowSource))
            .sink(Box::new(FileSink::new(output_path.clone())))
            .build()
            .expect("build pipeline");

        let config = TaskConfig::new(
            "timeout".to_string(),
            "Timeout Task".to_string(),
            Schedule::Once,
        )
        .max_retries(2)
        .retry_backoff(Duration::from_millis(1))
        .timeout(Duration::from_millis(50));
        let task = ScheduledTask::new(config);
        task.set_pipeline(pipeline).await;

        let result = task
            .execute()
            .await
            .expect("execute returns Ok(TaskResult)");
        assert!(!result.success, "timed-out pipeline must report failure");
        let error = result.error.expect("timeout must carry an error");
        assert!(
            error.contains("timed out"),
            "error should mention the timeout, got: {}",
            error
        );

        let _ = std::fs::remove_file(&output_path);
    }
}
