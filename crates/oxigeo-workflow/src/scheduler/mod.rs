//! Workflow scheduler for managing workflow executions.
//!
//! Provides multiple scheduling strategies:
//! - Cron-based scheduling
//! - Event-driven triggers
//! - Interval-based scheduling
//! - Cross-workflow dependencies

pub mod cron;
pub mod dependency;
pub mod event;
pub mod interval;

use crate::engine::WorkflowDefinition;
use crate::error::{Result, WorkflowError};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration as StdDuration;
use tokio::sync::{RwLock, Semaphore};
use uuid::Uuid;

pub use self::cron::{CronSchedule, CronScheduler};
pub use self::dependency::{
    DependencyRule, DependencyScheduler, DependencyStrategy, WorkflowDependency,
};
pub use self::event::{EventScheduler, EventTrigger, WorkflowEvent};
pub use self::interval::{IntervalSchedule, IntervalScheduler};

/// Trait implemented by callers to actually run a workflow when the scheduler
/// determines a schedule is due.
///
/// The scheduler is intentionally decoupled from any particular
/// [`crate::engine::WorkflowExecutor`]/[`crate::engine::TaskExecutor`] setup: it
/// decides *when* a workflow should run and delegates *how* to run it to this
/// trait. Implementations typically forward the workflow's DAG to a
/// `WorkflowExecutor`.
#[async_trait]
pub trait WorkflowRunner: Send + Sync + 'static {
    /// Execute the given workflow definition to completion.
    ///
    /// Returning `Err` marks the scheduled execution as failed; the error
    /// message is recorded in the schedule's execution history.
    async fn run(&self, workflow: &WorkflowDefinition) -> Result<()>;
}

/// Scheduler configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerConfig {
    /// Maximum number of concurrent workflow executions.
    pub max_concurrent_executions: usize,
    /// Enable missed execution handling.
    pub handle_missed_executions: bool,
    /// Maximum number of missed executions to handle.
    pub max_missed_executions: usize,
    /// Execution timeout in seconds.
    pub execution_timeout_secs: u64,
    /// Enable scheduler state persistence.
    pub enable_persistence: bool,
    /// Persistence directory path.
    pub persistence_path: Option<String>,
    /// Scheduler tick interval in milliseconds.
    pub tick_interval_ms: u64,
    /// Time zone for scheduling (IANA timezone name).
    pub timezone: String,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            max_concurrent_executions: 100,
            handle_missed_executions: true,
            max_missed_executions: 10,
            execution_timeout_secs: 3600,
            enable_persistence: true,
            persistence_path: None,
            tick_interval_ms: 100,
            timezone: "UTC".to_string(),
        }
    }
}

/// Scheduled workflow entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledWorkflow {
    /// Unique schedule ID.
    pub schedule_id: String,
    /// Workflow definition.
    pub workflow: WorkflowDefinition,
    /// Schedule type.
    pub schedule_type: ScheduleType,
    /// Whether the schedule is enabled.
    pub enabled: bool,
    /// Last execution time.
    pub last_execution: Option<DateTime<Utc>>,
    /// Next scheduled execution time.
    pub next_execution: Option<DateTime<Utc>>,
    /// Execution history (last N executions).
    pub execution_history: Vec<ScheduleExecution>,
    /// Maximum number of history entries to keep.
    pub max_history: usize,
    /// Schedule metadata.
    pub metadata: ScheduleMetadata,
}

/// Schedule type enumeration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ScheduleType {
    /// Cron-based scheduling.
    Cron {
        /// Cron expression.
        expression: String,
    },
    /// Interval-based scheduling.
    Interval {
        /// Interval in seconds.
        interval_secs: u64,
    },
    /// Event-driven trigger.
    Event {
        /// Event pattern to match.
        event_pattern: String,
    },
    /// Manual trigger only.
    Manual,
    /// Dependency-based trigger.
    Dependency {
        /// Workflow dependencies.
        dependencies: Vec<String>,
    },
}

/// Schedule execution record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleExecution {
    /// Execution ID.
    pub execution_id: String,
    /// Execution start time.
    pub start_time: DateTime<Utc>,
    /// Execution end time.
    pub end_time: Option<DateTime<Utc>>,
    /// Execution status.
    pub status: ExecutionStatus,
    /// Error message if failed.
    pub error_message: Option<String>,
}

/// Execution status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionStatus {
    /// Execution is pending.
    Pending,
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
}

/// Schedule metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleMetadata {
    /// Schedule creation time.
    pub created_at: DateTime<Utc>,
    /// Schedule last update time.
    pub updated_at: DateTime<Utc>,
    /// Schedule creator.
    pub created_by: String,
    /// Schedule description.
    pub description: Option<String>,
    /// Schedule tags.
    pub tags: Vec<String>,
}

/// Main workflow scheduler.
pub struct Scheduler {
    config: SchedulerConfig,
    schedules: Arc<DashMap<String, ScheduledWorkflow>>,
    cron_scheduler: Arc<RwLock<CronScheduler>>,
    interval_scheduler: Arc<RwLock<IntervalScheduler>>,
    event_scheduler: Arc<RwLock<EventScheduler>>,
    dependency_scheduler: Arc<RwLock<DependencyScheduler>>,
    running: Arc<RwLock<bool>>,
    /// Optional workflow runner used to dispatch due executions. When `None`,
    /// [`Scheduler::start`] returns an error rather than silently running a
    /// scheduler that can never execute anything.
    runner: Option<Arc<dyn WorkflowRunner>>,
}

impl Scheduler {
    /// Create a new scheduler with the given configuration (no runner attached).
    ///
    /// Schedules can be added/removed and inspected, but [`Scheduler::start`]
    /// will error until a runner is attached via [`Scheduler::with_runner`].
    pub fn new(config: SchedulerConfig) -> Self {
        Self::build(config, None)
    }

    /// Create a new scheduler with a workflow runner attached.
    pub fn with_runner(config: SchedulerConfig, runner: Arc<dyn WorkflowRunner>) -> Self {
        Self::build(config, Some(runner))
    }

    fn build(config: SchedulerConfig, runner: Option<Arc<dyn WorkflowRunner>>) -> Self {
        Self {
            config: config.clone(),
            schedules: Arc::new(DashMap::new()),
            cron_scheduler: Arc::new(RwLock::new(CronScheduler::new(config.clone()))),
            interval_scheduler: Arc::new(RwLock::new(IntervalScheduler::new(config.clone()))),
            event_scheduler: Arc::new(RwLock::new(EventScheduler::new(config.clone()))),
            dependency_scheduler: Arc::new(RwLock::new(DependencyScheduler::new(config.clone()))),
            running: Arc::new(RwLock::new(false)),
            runner,
        }
    }

    /// Attach (or replace) the workflow runner used to dispatch executions.
    pub fn set_runner(&mut self, runner: Arc<dyn WorkflowRunner>) {
        self.runner = Some(runner);
    }

    /// Create a new scheduler with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(SchedulerConfig::default())
    }

    /// Add a scheduled workflow.
    pub async fn add_schedule(
        &self,
        workflow: WorkflowDefinition,
        schedule_type: ScheduleType,
    ) -> Result<String> {
        let schedule_id = Uuid::new_v4().to_string();
        let now = Utc::now();

        let next_execution = match &schedule_type {
            ScheduleType::Cron { expression } => {
                let scheduler = self.cron_scheduler.write().await;
                scheduler.calculate_next_execution(expression, now)?
            }
            ScheduleType::Interval { interval_secs } => Some(
                now + Duration::try_seconds(*interval_secs as i64)
                    .ok_or_else(|| WorkflowError::scheduling("Invalid interval"))?,
            ),
            ScheduleType::Event { .. } | ScheduleType::Dependency { .. } => None,
            ScheduleType::Manual => None,
        };

        // Wire up the type-specific sub-schedulers so event- and dependency-based
        // schedules can actually be triggered (otherwise they would be inert).
        match &schedule_type {
            ScheduleType::Event { event_pattern } => {
                // Register a trigger keyed by the schedule id; it fires whenever an
                // event whose `event_type` equals `event_pattern` is published via
                // `Scheduler::publish_event`.
                let trigger = EventTrigger::exact(event_pattern.clone(), String::new());
                self.event_scheduler
                    .read()
                    .await
                    .register_trigger(schedule_id.clone(), trigger)
                    .await?;
            }
            ScheduleType::Dependency { dependencies } => {
                let rules = dependencies
                    .iter()
                    .map(|dep| DependencyRule {
                        workflow_id: dep.clone(),
                        required_status: ExecutionStatus::Success,
                        time_window_secs: None,
                        version_requirement: None,
                    })
                    .collect();
                let workflow_dependency = WorkflowDependency {
                    workflow_id: workflow.id.clone(),
                    dependencies: rules,
                    strategy: DependencyStrategy::All,
                    description: None,
                };
                self.dependency_scheduler
                    .read()
                    .await
                    .add_dependency(workflow_dependency)?;
            }
            _ => {}
        }

        let scheduled = ScheduledWorkflow {
            schedule_id: schedule_id.clone(),
            workflow,
            schedule_type,
            enabled: true,
            last_execution: None,
            next_execution,
            execution_history: Vec::new(),
            max_history: 100,
            metadata: ScheduleMetadata {
                created_at: now,
                updated_at: now,
                created_by: "system".to_string(),
                description: None,
                tags: Vec::new(),
            },
        };

        self.schedules.insert(schedule_id.clone(), scheduled);

        if self.config.enable_persistence {
            self.persist_state().await?;
        }

        Ok(schedule_id)
    }

    /// Remove a scheduled workflow.
    pub async fn remove_schedule(&self, schedule_id: &str) -> Result<()> {
        let (_, removed) = self
            .schedules
            .remove(schedule_id)
            .ok_or_else(|| WorkflowError::not_found(schedule_id))?;

        // Tear down any type-specific sub-scheduler registration created in
        // `add_schedule` so the scheduler does not leak triggers/dependencies.
        match &removed.schedule_type {
            ScheduleType::Event { .. } => {
                // Ignore "not found" if the trigger was never registered.
                let _ = self
                    .event_scheduler
                    .read()
                    .await
                    .unregister_trigger(schedule_id)
                    .await;
            }
            ScheduleType::Dependency { .. } => {
                let _ = self
                    .dependency_scheduler
                    .read()
                    .await
                    .remove_dependency(&removed.workflow.id);
            }
            _ => {}
        }

        if self.config.enable_persistence {
            self.persist_state().await?;
        }

        Ok(())
    }

    /// Enable a schedule.
    pub async fn enable_schedule(&self, schedule_id: &str) -> Result<()> {
        let mut schedule = self
            .schedules
            .get_mut(schedule_id)
            .ok_or_else(|| WorkflowError::not_found(schedule_id))?;
        schedule.enabled = true;
        schedule.metadata.updated_at = Utc::now();
        Ok(())
    }

    /// Disable a schedule.
    pub async fn disable_schedule(&self, schedule_id: &str) -> Result<()> {
        let mut schedule = self
            .schedules
            .get_mut(schedule_id)
            .ok_or_else(|| WorkflowError::not_found(schedule_id))?;
        schedule.enabled = false;
        schedule.metadata.updated_at = Utc::now();
        Ok(())
    }

    /// Start the scheduler.
    ///
    /// Spawns a background tick loop (respecting `config.tick_interval_ms`) that,
    /// on every tick, finds due time-based (cron/interval) and satisfied
    /// dependency schedules and dispatches them through the attached
    /// [`WorkflowRunner`]. Event-based schedules are dispatched separately via
    /// [`Scheduler::publish_event`].
    ///
    /// Returns an error if the scheduler is already running or if no runner has
    /// been attached (dispatch would otherwise be impossible).
    pub async fn start(&self) -> Result<()> {
        let runner = self.runner.clone().ok_or_else(|| {
            WorkflowError::scheduling(
                "No workflow runner configured; construct the scheduler with Scheduler::with_runner (or call set_runner) before starting",
            )
        })?;

        let mut running = self.running.write().await;
        if *running {
            return Err(WorkflowError::scheduling("Scheduler already running"));
        }
        *running = true;
        drop(running);

        let schedules = self.schedules.clone();
        let running_flag = self.running.clone();
        let cron_scheduler = self.cron_scheduler.clone();
        let interval_scheduler = self.interval_scheduler.clone();
        let dependency_scheduler = self.dependency_scheduler.clone();
        let config = self.config.clone();
        let semaphore = Arc::new(Semaphore::new(self.config.max_concurrent_executions.max(1)));

        tokio::spawn(async move {
            let tick = StdDuration::from_millis(config.tick_interval_ms.max(1));

            loop {
                if !*running_flag.read().await {
                    break;
                }

                let now = Utc::now();

                // Phase 1: collect candidate schedule ids without holding a
                // DashMap reference across any `.await`.
                let mut candidates: Vec<(String, ScheduleType, String, bool)> = Vec::new();
                for entry in schedules.iter() {
                    let s = entry.value();
                    if !s.enabled {
                        continue;
                    }
                    let time_due = matches!(
                        s.schedule_type,
                        ScheduleType::Cron { .. } | ScheduleType::Interval { .. }
                    ) && s.next_execution.map(|n| n <= now).unwrap_or(false);
                    let dep_candidate = matches!(s.schedule_type, ScheduleType::Dependency { .. })
                        && s.last_execution.is_none();
                    if time_due || dep_candidate {
                        candidates.push((
                            entry.key().clone(),
                            s.schedule_type.clone(),
                            s.workflow.id.clone(),
                            dep_candidate,
                        ));
                    }
                }

                // Phase 2: for each candidate, confirm it should run, claim it
                // (advance bookkeeping synchronously so it is not re-dispatched),
                // then spawn the actual run under a concurrency permit.
                for (schedule_id, schedule_type, workflow_id, dep_candidate) in candidates {
                    if dep_candidate {
                        let satisfied = dependency_scheduler
                            .read()
                            .await
                            .are_dependencies_satisfied(&workflow_id)
                            .unwrap_or(false);
                        if !satisfied {
                            continue;
                        }
                    }

                    let next_execution = match Self::compute_next_execution(
                        &cron_scheduler,
                        &interval_scheduler,
                        &schedule_type,
                        now,
                    )
                    .await
                    {
                        Ok(next) => next,
                        Err(e) => {
                            tracing::warn!(
                                "scheduler: failed to compute next execution for {}: {}",
                                schedule_id,
                                e
                            );
                            continue;
                        }
                    };

                    // Claim the execution: record a Running entry and advance
                    // last/next execution so the next tick does not re-fire it.
                    let execution_id = Uuid::new_v4().to_string();
                    let workflow = {
                        let mut s = match schedules.get_mut(&schedule_id) {
                            Some(s) => s,
                            None => continue,
                        };
                        s.last_execution = Some(now);
                        s.next_execution = next_execution;
                        s.execution_history.push(ScheduleExecution {
                            execution_id: execution_id.clone(),
                            start_time: now,
                            end_time: None,
                            status: ExecutionStatus::Running,
                            error_message: None,
                        });
                        let max_history = s.max_history;
                        if s.execution_history.len() > max_history {
                            s.execution_history.remove(0);
                        }
                        s.workflow.clone()
                    };

                    let permit = match Arc::clone(&semaphore).acquire_owned().await {
                        Ok(permit) => permit,
                        Err(_) => break,
                    };

                    let schedules_task = schedules.clone();
                    let dependency_task = dependency_scheduler.clone();
                    let runner_task = runner.clone();
                    let config_task = config.clone();
                    tokio::spawn(async move {
                        let _permit = permit;
                        Self::finish_execution(
                            schedules_task,
                            dependency_task,
                            runner_task,
                            config_task,
                            schedule_id,
                            execution_id,
                            workflow,
                        )
                        .await;
                    });
                }

                tokio::time::sleep(tick).await;
            }
        });

        Ok(())
    }

    /// Publish an event that may trigger event-based schedules.
    ///
    /// Forwards the event to the internal event scheduler, then dispatches every
    /// enabled `Event` schedule whose registered trigger matches. Returns the
    /// list of schedule ids that were dispatched.
    pub async fn publish_event(&self, event: WorkflowEvent) -> Result<Vec<String>> {
        let runner = self.runner.clone().ok_or_else(|| {
            WorkflowError::scheduling(
                "No workflow runner configured; cannot dispatch event-triggered schedules",
            )
        })?;

        let matched = self
            .event_scheduler
            .read()
            .await
            .publish_event(event)
            .await?;

        let now = Utc::now();
        let mut dispatched = Vec::new();

        for schedule_id in matched {
            // Only dispatch enabled schedules that still exist.
            let workflow = {
                let mut s = match self.schedules.get_mut(&schedule_id) {
                    Some(s) => s,
                    None => continue,
                };
                if !s.enabled {
                    continue;
                }
                s.last_execution = Some(now);
                s.workflow.clone()
            };

            let execution_id = Uuid::new_v4().to_string();
            {
                let mut s = match self.schedules.get_mut(&schedule_id) {
                    Some(s) => s,
                    None => continue,
                };
                s.execution_history.push(ScheduleExecution {
                    execution_id: execution_id.clone(),
                    start_time: now,
                    end_time: None,
                    status: ExecutionStatus::Running,
                    error_message: None,
                });
                let max_history = s.max_history;
                if s.execution_history.len() > max_history {
                    s.execution_history.remove(0);
                }
            }

            Self::finish_execution(
                self.schedules.clone(),
                self.dependency_scheduler.clone(),
                runner.clone(),
                self.config.clone(),
                schedule_id.clone(),
                execution_id,
                workflow,
            )
            .await;

            dispatched.push(schedule_id);
        }

        Ok(dispatched)
    }

    /// Compute the next execution time for a schedule type after `now`.
    async fn compute_next_execution(
        cron_scheduler: &Arc<RwLock<CronScheduler>>,
        interval_scheduler: &Arc<RwLock<IntervalScheduler>>,
        schedule_type: &ScheduleType,
        now: DateTime<Utc>,
    ) -> Result<Option<DateTime<Utc>>> {
        match schedule_type {
            ScheduleType::Cron { expression } => cron_scheduler
                .read()
                .await
                .calculate_next_execution(expression, now),
            ScheduleType::Interval { interval_secs } => interval_scheduler
                .read()
                .await
                .calculate_next_execution(*interval_secs, Some(now))
                .map(Some),
            ScheduleType::Event { .. } | ScheduleType::Dependency { .. } | ScheduleType::Manual => {
                Ok(None)
            }
        }
    }

    /// Run a claimed execution to completion and record the result.
    ///
    /// The corresponding `Running` execution record (identified by
    /// `execution_id`) must already be present in the schedule's history.
    async fn finish_execution(
        schedules: Arc<DashMap<String, ScheduledWorkflow>>,
        dependency_scheduler: Arc<RwLock<DependencyScheduler>>,
        runner: Arc<dyn WorkflowRunner>,
        config: SchedulerConfig,
        schedule_id: String,
        execution_id: String,
        workflow: WorkflowDefinition,
    ) {
        let run_result = runner.run(&workflow).await;

        // Record the outcome on the execution record.
        if let Some(mut s) = schedules.get_mut(&schedule_id)
            && let Some(record) = s
                .execution_history
                .iter_mut()
                .find(|e| e.execution_id == execution_id)
        {
            record.end_time = Some(Utc::now());
            match &run_result {
                Ok(()) => record.status = ExecutionStatus::Success,
                Err(e) => {
                    record.status = ExecutionStatus::Failed;
                    record.error_message = Some(e.to_string());
                }
            }
        }

        // On success, publish the completion so downstream dependency schedules
        // can observe it.
        if run_result.is_ok() {
            dependency_scheduler
                .read()
                .await
                .update_status(workflow.id.clone(), ExecutionStatus::Success);
        } else if let Err(ref e) = run_result {
            tracing::warn!(
                "scheduler: workflow '{}' (schedule {}) failed: {}",
                workflow.id,
                schedule_id,
                e
            );
        }

        if config.enable_persistence
            && let Err(e) = Self::persist_snapshot(&schedules, &config).await
        {
            tracing::warn!("scheduler: failed to persist state after execution: {}", e);
        }
    }

    /// Stop the scheduler.
    pub async fn stop(&self) -> Result<()> {
        let mut running = self.running.write().await;
        if !*running {
            return Err(WorkflowError::scheduling("Scheduler not running"));
        }
        *running = false;
        Ok(())
    }

    /// Check if the scheduler is running.
    pub async fn is_running(&self) -> bool {
        *self.running.read().await
    }

    /// Get all schedules.
    pub fn get_schedules(&self) -> Vec<ScheduledWorkflow> {
        self.schedules
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Get a specific schedule.
    pub fn get_schedule(&self, schedule_id: &str) -> Option<ScheduledWorkflow> {
        self.schedules.get(schedule_id).map(|entry| entry.clone())
    }

    /// Trigger a manual execution.
    pub async fn trigger_manual(&self, schedule_id: &str) -> Result<String> {
        let schedule = self
            .schedules
            .get(schedule_id)
            .ok_or_else(|| WorkflowError::not_found(schedule_id))?;

        if !schedule.enabled {
            return Err(WorkflowError::scheduling("Schedule is disabled"));
        }

        let execution_id = Uuid::new_v4().to_string();

        // Record execution start
        let execution = ScheduleExecution {
            execution_id: execution_id.clone(),
            start_time: Utc::now(),
            end_time: None,
            status: ExecutionStatus::Pending,
            error_message: None,
        };

        drop(schedule);

        let mut schedule_mut = self
            .schedules
            .get_mut(schedule_id)
            .ok_or_else(|| WorkflowError::not_found(schedule_id))?;
        schedule_mut.execution_history.push(execution);
        if schedule_mut.execution_history.len() > schedule_mut.max_history {
            schedule_mut.execution_history.remove(0);
        }

        Ok(execution_id)
    }

    /// Update execution status.
    pub async fn update_execution_status(
        &self,
        schedule_id: &str,
        execution_id: &str,
        status: ExecutionStatus,
        error_message: Option<String>,
    ) -> Result<()> {
        let mut schedule = self
            .schedules
            .get_mut(schedule_id)
            .ok_or_else(|| WorkflowError::not_found(schedule_id))?;

        if let Some(execution) = schedule
            .execution_history
            .iter_mut()
            .find(|e| e.execution_id == execution_id)
        {
            execution.status = status;
            execution.error_message = error_message;
            if matches!(
                status,
                ExecutionStatus::Success
                    | ExecutionStatus::Failed
                    | ExecutionStatus::Cancelled
                    | ExecutionStatus::TimedOut
            ) {
                execution.end_time = Some(Utc::now());
            }
        }

        // Release the DashMap lock before async I/O.
        drop(schedule);

        if self.config.enable_persistence
            && let Err(e) = self.persist_state().await
        {
            tracing::warn!(
                "scheduler: failed to persist state after status update: {}",
                e
            );
        }

        Ok(())
    }

    /// Persist scheduler state atomically using write-to-tmp-then-rename.
    ///
    /// Each `ScheduledWorkflow` is serialized as a single JSON line (JSON-Lines format).
    /// The write is crash-safe: data is first written to a `.tmp` sibling file, then
    /// atomically renamed into place, so a mid-write crash never leaves a corrupt file.
    async fn persist_state(&self) -> Result<()> {
        Self::persist_snapshot(&self.schedules, &self.config).await
    }

    /// Persist a snapshot of the given schedules using the same crash-safe
    /// write-to-tmp-then-rename strategy as [`Scheduler::persist_state`].
    ///
    /// Factored out so the background dispatch loop (which owns only cloned
    /// `Arc`s, not `&self`) can persist after recording an execution.
    async fn persist_snapshot(
        schedules: &DashMap<String, ScheduledWorkflow>,
        config: &SchedulerConfig,
    ) -> Result<()> {
        let path_str = match &config.persistence_path {
            Some(p) => p.clone(),
            None => return Ok(()),
        };

        let path = std::path::PathBuf::from(&path_str);

        // Ensure the parent directory exists.
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Snapshot the DashMap into a sorted Vec for deterministic output.
        let mut snapshot: Vec<ScheduledWorkflow> = schedules
            .iter()
            .map(|entry| entry.value().clone())
            .collect();
        snapshot.sort_by(|a, b| a.schedule_id.cmp(&b.schedule_id));

        // Build JSON-Lines payload: one compact JSON object per line.
        let mut payload = String::with_capacity(snapshot.len() * 256);
        for workflow in &snapshot {
            let line = serde_json::to_string(workflow)?;
            payload.push_str(&line);
            payload.push('\n');
        }

        // Atomic write: write to a `.tmp` sibling, then rename into place.
        let tmp_path = {
            let mut p = path.clone();
            let ext = match p.extension() {
                Some(e) => format!("{}.tmp", e.to_string_lossy()),
                None => "tmp".to_string(),
            };
            p.set_extension(ext);
            p
        };

        tokio::fs::write(&tmp_path, payload.as_bytes()).await?;
        tokio::fs::rename(&tmp_path, &path).await?;

        tracing::debug!(
            "scheduler::persist_state: wrote {} schedules to {}",
            snapshot.len(),
            path.display()
        );

        Ok(())
    }

    /// Load scheduler state from the persistence file.
    ///
    /// Reads the JSON-Lines file written by `persist_state`.  Corrupt or
    /// unparseable lines are skipped with a `warn!` log; the method still
    /// returns `Ok(())` so that a partially-corrupt file does not prevent
    /// the scheduler from starting.
    pub async fn load_state(&self) -> Result<()> {
        let path_str = match &self.config.persistence_path {
            Some(p) => p.clone(),
            None => return Ok(()),
        };

        let path = std::path::PathBuf::from(&path_str);

        // First startup — no persistence file exists yet.
        if !path.exists() {
            return Ok(());
        }

        let content = tokio::fs::read_to_string(&path).await?;
        let mut loaded_count = 0usize;
        let mut failed_count = 0usize;

        for (line_no, raw_line) in content.lines().enumerate() {
            let line = raw_line.trim();
            if line.is_empty() {
                continue;
            }

            match serde_json::from_str::<ScheduledWorkflow>(line) {
                Ok(workflow) => {
                    self.schedules
                        .insert(workflow.schedule_id.clone(), workflow);
                    loaded_count += 1;
                }
                Err(e) => {
                    tracing::warn!(
                        "scheduler::load_state: skipping corrupt line {} in {}: {}",
                        line_no + 1,
                        path.display(),
                        e
                    );
                    failed_count += 1;
                }
            }
        }

        if failed_count > 0 {
            tracing::warn!(
                "scheduler::load_state: {}/{} lines failed to parse in {}",
                failed_count,
                loaded_count + failed_count,
                path.display()
            );
        }

        tracing::debug!(
            "scheduler::load_state: loaded {} schedules from {}",
            loaded_count,
            path.display()
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::WorkflowDag;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Runner that counts how many workflows it ran (and can be made to fail).
    struct CountingRunner {
        count: Arc<AtomicUsize>,
        fail: bool,
    }

    #[async_trait]
    impl WorkflowRunner for CountingRunner {
        async fn run(&self, _workflow: &WorkflowDefinition) -> Result<()> {
            self.count.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                Err(WorkflowError::execution("intentional test failure"))
            } else {
                Ok(())
            }
        }
    }

    fn test_workflow(id: &str) -> WorkflowDefinition {
        WorkflowDefinition {
            id: id.to_string(),
            name: id.to_string(),
            description: None,
            version: "1.0.0".to_string(),
            dag: WorkflowDag::new(),
        }
    }

    fn no_persist_config() -> SchedulerConfig {
        SchedulerConfig {
            enable_persistence: false,
            tick_interval_ms: 40,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_scheduler_creation() {
        let scheduler = Scheduler::with_defaults();
        assert!(!scheduler.is_running().await);
    }

    #[tokio::test]
    async fn test_start_requires_runner() {
        let scheduler = Scheduler::new(no_persist_config());
        let result = scheduler.start().await;
        assert!(result.is_err(), "start without a runner must error");
        assert!(!scheduler.is_running().await);
    }

    #[tokio::test]
    async fn test_event_schedule_dispatches() {
        let count = Arc::new(AtomicUsize::new(0));
        let runner = Arc::new(CountingRunner {
            count: Arc::clone(&count),
            fail: false,
        });
        let scheduler = Scheduler::with_runner(no_persist_config(), runner);

        let schedule_id = scheduler
            .add_schedule(
                test_workflow("wf-event"),
                ScheduleType::Event {
                    event_pattern: "data.arrived".to_string(),
                },
            )
            .await
            .expect("add event schedule");

        let event = WorkflowEvent::new("data.arrived", serde_json::json!({"n": 1}));
        let dispatched = scheduler.publish_event(event).await.expect("publish event");

        assert_eq!(dispatched, vec![schedule_id.clone()]);
        assert_eq!(count.load(Ordering::SeqCst), 1);

        let sched = scheduler
            .get_schedule(&schedule_id)
            .expect("schedule exists");
        assert_eq!(sched.execution_history.len(), 1);
        assert_eq!(sched.execution_history[0].status, ExecutionStatus::Success);
        assert!(sched.execution_history[0].end_time.is_some());
    }

    #[tokio::test]
    async fn test_event_non_matching_does_not_dispatch() {
        let count = Arc::new(AtomicUsize::new(0));
        let runner = Arc::new(CountingRunner {
            count: Arc::clone(&count),
            fail: false,
        });
        let scheduler = Scheduler::with_runner(no_persist_config(), runner);

        scheduler
            .add_schedule(
                test_workflow("wf-event"),
                ScheduleType::Event {
                    event_pattern: "data.arrived".to_string(),
                },
            )
            .await
            .expect("add event schedule");

        let event = WorkflowEvent::new("something.else", serde_json::json!({}));
        let dispatched = scheduler.publish_event(event).await.expect("publish");
        assert!(dispatched.is_empty());
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn test_interval_schedule_executes() {
        let count = Arc::new(AtomicUsize::new(0));
        let runner = Arc::new(CountingRunner {
            count: Arc::clone(&count),
            fail: false,
        });
        let scheduler = Arc::new(Scheduler::with_runner(no_persist_config(), runner));

        let schedule_id = scheduler
            .add_schedule(
                test_workflow("wf-interval"),
                ScheduleType::Interval { interval_secs: 1 },
            )
            .await
            .expect("add interval schedule");

        scheduler.start().await.expect("start scheduler");

        // Poll (bounded) for the first successful execution.
        let mut succeeded = false;
        for _ in 0..100 {
            tokio::time::sleep(StdDuration::from_millis(50)).await;
            if let Some(s) = scheduler.get_schedule(&schedule_id)
                && s.execution_history
                    .iter()
                    .any(|e| e.status == ExecutionStatus::Success)
            {
                succeeded = true;
                break;
            }
        }

        scheduler.stop().await.ok();

        assert!(succeeded, "interval schedule never executed");
        assert!(count.load(Ordering::SeqCst) >= 1);
    }

    #[tokio::test]
    async fn test_failed_execution_recorded() {
        let count = Arc::new(AtomicUsize::new(0));
        let runner = Arc::new(CountingRunner {
            count: Arc::clone(&count),
            fail: true,
        });
        let scheduler = Scheduler::with_runner(no_persist_config(), runner);

        let schedule_id = scheduler
            .add_schedule(
                test_workflow("wf-fail"),
                ScheduleType::Event {
                    event_pattern: "boom".to_string(),
                },
            )
            .await
            .expect("add event schedule");

        let event = WorkflowEvent::new("boom", serde_json::json!({}));
        scheduler.publish_event(event).await.expect("publish");

        let sched = scheduler
            .get_schedule(&schedule_id)
            .expect("schedule exists");
        assert_eq!(sched.execution_history.len(), 1);
        assert_eq!(sched.execution_history[0].status, ExecutionStatus::Failed);
        assert!(sched.execution_history[0].error_message.is_some());
    }

    #[tokio::test]
    async fn test_add_remove_schedule() {
        let scheduler = Scheduler::with_defaults();
        let workflow = WorkflowDefinition {
            id: "test-workflow".to_string(),
            name: "Test Workflow".to_string(),
            description: None,
            version: "1.0.0".to_string(),
            dag: WorkflowDag::new(),
        };

        let schedule_id = scheduler
            .add_schedule(workflow, ScheduleType::Manual)
            .await
            .expect("Failed to add schedule");

        assert!(scheduler.get_schedule(&schedule_id).is_some());

        scheduler
            .remove_schedule(&schedule_id)
            .await
            .expect("Failed to remove schedule");

        assert!(scheduler.get_schedule(&schedule_id).is_none());
    }

    #[tokio::test]
    async fn test_enable_disable_schedule() {
        let scheduler = Scheduler::with_defaults();
        let workflow = WorkflowDefinition {
            id: "test-workflow".to_string(),
            name: "Test Workflow".to_string(),
            description: None,
            version: "1.0.0".to_string(),
            dag: WorkflowDag::new(),
        };

        let schedule_id = scheduler
            .add_schedule(workflow, ScheduleType::Manual)
            .await
            .expect("Failed to add schedule");

        scheduler
            .disable_schedule(&schedule_id)
            .await
            .expect("Failed to disable");
        assert!(
            !scheduler
                .get_schedule(&schedule_id)
                .is_some_and(|s| s.enabled)
        );

        scheduler
            .enable_schedule(&schedule_id)
            .await
            .expect("Failed to enable");
        assert!(
            scheduler
                .get_schedule(&schedule_id)
                .is_some_and(|s| s.enabled)
        );
    }
}
