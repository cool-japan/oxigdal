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
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

pub use self::cron::{CronSchedule, CronScheduler};
pub use self::dependency::{DependencyScheduler, WorkflowDependency};
pub use self::event::{EventScheduler, EventTrigger};
pub use self::interval::{IntervalSchedule, IntervalScheduler};

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
}

impl Scheduler {
    /// Create a new scheduler with the given configuration.
    pub fn new(config: SchedulerConfig) -> Self {
        Self {
            config: config.clone(),
            schedules: Arc::new(DashMap::new()),
            cron_scheduler: Arc::new(RwLock::new(CronScheduler::new(config.clone()))),
            interval_scheduler: Arc::new(RwLock::new(IntervalScheduler::new(config.clone()))),
            event_scheduler: Arc::new(RwLock::new(EventScheduler::new(config.clone()))),
            dependency_scheduler: Arc::new(RwLock::new(DependencyScheduler::new(config.clone()))),
            running: Arc::new(RwLock::new(false)),
        }
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
                now + chrono::Duration::try_seconds(*interval_secs as i64)
                    .ok_or_else(|| WorkflowError::scheduling("Invalid interval"))?,
            ),
            ScheduleType::Event { .. } | ScheduleType::Dependency { .. } => None,
            ScheduleType::Manual => None,
        };

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
        self.schedules
            .remove(schedule_id)
            .ok_or_else(|| WorkflowError::not_found(schedule_id))?;

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
    pub async fn start(&self) -> Result<()> {
        let mut running = self.running.write().await;
        if *running {
            return Err(WorkflowError::scheduling("Scheduler already running"));
        }
        *running = true;
        drop(running);

        // Start all sub-schedulers
        let cron_scheduler = self.cron_scheduler.clone();
        let interval_scheduler = self.interval_scheduler.clone();
        let event_scheduler = self.event_scheduler.clone();
        let dependency_scheduler = self.dependency_scheduler.clone();

        tokio::spawn(async move {
            let _ = cron_scheduler.write().await;
            // Scheduler loop would go here
        });

        tokio::spawn(async move {
            let _ = interval_scheduler.write().await;
            // Scheduler loop would go here
        });

        tokio::spawn(async move {
            let _ = event_scheduler.write().await;
            // Scheduler loop would go here
        });

        tokio::spawn(async move {
            let _ = dependency_scheduler.write().await;
            // Scheduler loop would go here
        });

        Ok(())
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

        Ok(())
    }

    /// Persist scheduler state.
    async fn persist_state(&self) -> Result<()> {
        if let Some(_path) = &self.config.persistence_path {
            // Persistence implementation would go here
            // For now, this is a placeholder
        }
        Ok(())
    }

    /// Load scheduler state from persistence.
    pub async fn load_state(&self) -> Result<()> {
        if let Some(_path) = &self.config.persistence_path {
            // Load implementation would go here
            // For now, this is a placeholder
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::WorkflowDag;

    #[tokio::test]
    async fn test_scheduler_creation() {
        let scheduler = Scheduler::with_defaults();
        assert!(!scheduler.is_running().await);
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
