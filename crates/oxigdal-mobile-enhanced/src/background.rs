//! Background task management for mobile platforms
//!
//! This module provides utilities for managing background processing tasks
//! on mobile devices, with awareness of platform limitations and battery state.
//!
//! # Key Features
//!
//! - Background task scheduling
//! - Platform-aware execution limits
//! - Battery-aware task throttling
//! - Task prioritization
//! - Progress tracking
//! - Automatic task suspension/resumption
//!
//! # Example
//!
//! ```rust,no_run
//! use oxigdal_mobile_enhanced::background::{BackgroundScheduler, TaskPriority};
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let scheduler = BackgroundScheduler::new();
//!
//! // Schedule a background task
//! let task_id = scheduler.schedule_task(
//!     "data_processing",
//!     TaskPriority::Normal,
//!     || async {
//!         // Process data in background
//!         Ok(())
//!     }
//! ).await?;
//!
//! // Check task status
//! let status = scheduler.task_status(task_id)?;
//! println!("Task status: {:?}", status);
//! # Ok(())
//! # }
//! ```

use crate::battery::{BatteryMonitor, ProcessingMode};
use crate::error::{MobileError, Result};
use futures::Future;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Task priority levels
///
/// Note: Variants are ordered from lowest to highest priority for correct `PartialOrd`/`Ord` comparison.
/// This ensures that `Critical > High > Normal > Low > Idle` as expected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TaskPriority {
    /// Background tasks that run only when idle
    Idle,
    /// Low priority tasks that can be deferred
    Low,
    /// Normal priority tasks (default)
    Normal,
    /// High priority tasks
    High,
    /// Critical tasks that must complete
    Critical,
}

impl TaskPriority {
    /// Get time budget for this priority (milliseconds)
    pub fn time_budget_ms(&self) -> u64 {
        match self {
            Self::Critical => 30_000, // 30 seconds
            Self::High => 10_000,     // 10 seconds
            Self::Normal => 5_000,    // 5 seconds
            Self::Low => 2_000,       // 2 seconds
            Self::Idle => 1_000,      // 1 second
        }
    }

    /// Check if this priority can run in power saver mode
    pub fn can_run_in_power_saver(&self) -> bool {
        matches!(self, Self::Critical)
    }

    /// Check if this priority requires WiFi
    pub fn requires_wifi(&self) -> bool {
        matches!(self, Self::Low | Self::Idle)
    }
}

/// Task execution status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// Task is queued and waiting to execute
    Queued,
    /// Task is currently running
    Running,
    /// Task completed successfully
    Completed,
    /// Task failed with error
    Failed,
    /// Task was cancelled
    Cancelled,
    /// Task is suspended and will resume later
    Suspended,
}

/// Background task metadata
#[derive(Debug, Clone)]
pub struct TaskInfo {
    /// Unique task identifier
    pub id: TaskId,
    /// Task name/description
    pub name: String,
    /// Task priority
    pub priority: TaskPriority,
    /// Current status
    pub status: TaskStatus,
    /// When the task was created
    pub created_at: Instant,
    /// When the task started executing
    pub started_at: Option<Instant>,
    /// When the task completed
    pub completed_at: Option<Instant>,
    /// Progress (0.0 - 1.0)
    pub progress: f32,
    /// Error message if failed
    pub error: Option<String>,
}

impl TaskInfo {
    /// Get task execution duration
    pub fn execution_duration(&self) -> Option<Duration> {
        match (self.started_at, self.completed_at) {
            (Some(start), Some(end)) => Some(end.duration_since(start)),
            (Some(start), None) if self.status == TaskStatus::Running => {
                Some(Instant::now().duration_since(start))
            }
            _ => None,
        }
    }

    /// Check if task is finished (completed, failed, or cancelled)
    pub fn is_finished(&self) -> bool {
        matches!(
            self.status,
            TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
        )
    }

    /// Check if task is active (queued, running, or suspended)
    pub fn is_active(&self) -> bool {
        !self.is_finished()
    }
}

/// Task identifier
pub type TaskId = u64;

/// Background task scheduler
pub struct BackgroundScheduler {
    tasks: Arc<RwLock<HashMap<TaskId, TaskInfo>>>,
    next_id: Arc<RwLock<TaskId>>,
    battery_monitor: BatteryMonitor,
    max_concurrent_tasks: usize,
}

impl BackgroundScheduler {
    /// Create a new background scheduler
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            next_id: Arc::new(RwLock::new(0)),
            battery_monitor: BatteryMonitor::new().ok().unwrap_or_default(),
            max_concurrent_tasks: 3,
        }
    }

    /// Generate next task ID
    fn next_task_id(&self) -> TaskId {
        let mut id = self.next_id.write();
        let current = *id;
        *id = id.wrapping_add(1);
        current
    }

    /// Schedule a background task
    pub async fn schedule_task<F, Fut>(
        &self,
        name: &str,
        priority: TaskPriority,
        _task_fn: F,
    ) -> Result<TaskId>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        let task_id = self.next_task_id();

        let info = TaskInfo {
            id: task_id,
            name: name.to_string(),
            priority,
            status: TaskStatus::Queued,
            created_at: Instant::now(),
            started_at: None,
            completed_at: None,
            progress: 0.0,
            error: None,
        };

        self.tasks.write().insert(task_id, info);

        // Check if we should execute immediately
        if self.can_execute_task(priority) {
            // In a real implementation, we would spawn the task
            // For now, just mark as queued
        }

        Ok(task_id)
    }

    /// Check if a task can be executed given current conditions
    fn can_execute_task(&self, priority: TaskPriority) -> bool {
        // Check concurrent task limit
        let running_count = self
            .tasks
            .read()
            .values()
            .filter(|t| t.status == TaskStatus::Running)
            .count();

        if running_count >= self.max_concurrent_tasks {
            return false;
        }

        let mode = self.battery_monitor.recommended_processing_mode();

        match mode {
            ProcessingMode::PowerSaver => priority.can_run_in_power_saver(),
            ProcessingMode::Balanced => !matches!(priority, TaskPriority::Idle),
            ProcessingMode::HighPerformance => true,
        }
    }

    /// Get task status
    pub fn task_status(&self, task_id: TaskId) -> Result<TaskStatus> {
        let tasks = self.tasks.read();
        tasks
            .get(&task_id)
            .map(|info| info.status)
            .ok_or_else(|| MobileError::BackgroundTaskError(format!("Task {} not found", task_id)))
    }

    /// Get task info
    pub fn task_info(&self, task_id: TaskId) -> Result<TaskInfo> {
        let tasks = self.tasks.read();
        tasks
            .get(&task_id)
            .cloned()
            .ok_or_else(|| MobileError::BackgroundTaskError(format!("Task {} not found", task_id)))
    }

    /// Cancel a task
    pub fn cancel_task(&self, task_id: TaskId) -> Result<()> {
        let mut tasks = self.tasks.write();
        if let Some(info) = tasks.get_mut(&task_id) {
            if info.is_active() {
                info.status = TaskStatus::Cancelled;
                info.completed_at = Some(Instant::now());
                Ok(())
            } else {
                Err(MobileError::BackgroundTaskError(
                    "Task already finished".to_string(),
                ))
            }
        } else {
            Err(MobileError::BackgroundTaskError(format!(
                "Task {} not found",
                task_id
            )))
        }
    }

    /// Suspend all low-priority tasks
    pub fn suspend_low_priority_tasks(&self) -> Result<Vec<TaskId>> {
        let mut tasks = self.tasks.write();
        let mut suspended = Vec::new();

        for (id, info) in tasks.iter_mut() {
            if info.status == TaskStatus::Running
                && matches!(info.priority, TaskPriority::Low | TaskPriority::Idle)
            {
                info.status = TaskStatus::Suspended;
                suspended.push(*id);
            }
        }

        Ok(suspended)
    }

    /// Resume suspended tasks
    pub fn resume_suspended_tasks(&self) -> Result<Vec<TaskId>> {
        let mut tasks = self.tasks.write();
        let mut resumed = Vec::new();

        for (id, info) in tasks.iter_mut() {
            if info.status == TaskStatus::Suspended {
                info.status = TaskStatus::Queued;
                resumed.push(*id);
            }
        }

        Ok(resumed)
    }

    /// Get all active tasks
    pub fn active_tasks(&self) -> Vec<TaskInfo> {
        let tasks = self.tasks.read();
        tasks
            .values()
            .filter(|info| info.is_active())
            .cloned()
            .collect()
    }

    /// Get tasks by priority
    pub fn tasks_by_priority(&self, priority: TaskPriority) -> Vec<TaskInfo> {
        let tasks = self.tasks.read();
        tasks
            .values()
            .filter(|info| info.priority == priority)
            .cloned()
            .collect()
    }

    /// Clean up finished tasks older than duration
    pub fn cleanup_finished_tasks(&self, older_than: Duration) -> usize {
        let mut tasks = self.tasks.write();
        let now = Instant::now();
        let mut removed = 0;

        tasks.retain(|_, info| {
            if info.is_finished() {
                if let Some(completed_at) = info.completed_at {
                    if now.duration_since(completed_at) > older_than {
                        removed += 1;
                        return false;
                    }
                }
            }
            true
        });

        removed
    }

    /// Get task statistics
    pub fn statistics(&self) -> TaskStatistics {
        let tasks = self.tasks.read();

        let mut stats = TaskStatistics {
            total_tasks: tasks.len(),
            queued: 0,
            running: 0,
            completed: 0,
            failed: 0,
            cancelled: 0,
            suspended: 0,
        };

        for info in tasks.values() {
            match info.status {
                TaskStatus::Queued => stats.queued += 1,
                TaskStatus::Running => stats.running += 1,
                TaskStatus::Completed => stats.completed += 1,
                TaskStatus::Failed => stats.failed += 1,
                TaskStatus::Cancelled => stats.cancelled += 1,
                TaskStatus::Suspended => stats.suspended += 1,
            }
        }

        stats
    }
}

impl Default for BackgroundScheduler {
    fn default() -> Self {
        Self::new()
    }
}

/// Task execution statistics
#[derive(Debug, Clone)]
pub struct TaskStatistics {
    /// Total number of tasks
    pub total_tasks: usize,
    /// Number of queued tasks
    pub queued: usize,
    /// Number of running tasks
    pub running: usize,
    /// Number of completed tasks
    pub completed: usize,
    /// Number of failed tasks
    pub failed: usize,
    /// Number of cancelled tasks
    pub cancelled: usize,
    /// Number of suspended tasks
    pub suspended: usize,
}

impl TaskStatistics {
    /// Get success rate (0.0 - 1.0)
    pub fn success_rate(&self) -> f64 {
        let finished = self.completed + self.failed + self.cancelled;
        if finished == 0 {
            return 0.0;
        }
        self.completed as f64 / finished as f64
    }

    /// Get active task count
    pub fn active_count(&self) -> usize {
        self.queued + self.running + self.suspended
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_priority_ordering() {
        assert!(TaskPriority::Critical > TaskPriority::High);
        assert!(TaskPriority::High > TaskPriority::Normal);
        assert!(TaskPriority::Normal > TaskPriority::Low);
        assert!(TaskPriority::Low > TaskPriority::Idle);
    }

    #[test]
    fn test_task_priority_properties() {
        assert!(TaskPriority::Critical.can_run_in_power_saver());
        assert!(!TaskPriority::Normal.can_run_in_power_saver());
        assert!(TaskPriority::Low.requires_wifi());
        assert!(!TaskPriority::Critical.requires_wifi());
    }

    #[test]
    fn test_task_info() {
        let info = TaskInfo {
            id: 0,
            name: "test".to_string(),
            priority: TaskPriority::Normal,
            status: TaskStatus::Completed,
            created_at: Instant::now(),
            started_at: Some(Instant::now()),
            completed_at: Some(Instant::now()),
            progress: 1.0,
            error: None,
        };

        assert!(info.is_finished());
        assert!(!info.is_active());
        assert!(info.execution_duration().is_some());
    }

    #[test]
    fn test_task_statistics() {
        let stats = TaskStatistics {
            total_tasks: 100,
            queued: 10,
            running: 5,
            completed: 70,
            failed: 10,
            cancelled: 5,
            suspended: 0,
        };

        assert_eq!(stats.success_rate(), 70.0 / 85.0);
        assert_eq!(stats.active_count(), 15);
    }

    #[tokio::test]
    async fn test_background_scheduler() {
        let scheduler = BackgroundScheduler::new();

        let task_id = scheduler
            .schedule_task("test_task", TaskPriority::Normal, || async { Ok(()) })
            .await
            .expect("Failed to schedule task");

        let status = scheduler
            .task_status(task_id)
            .expect("Failed to get status");
        assert_eq!(status, TaskStatus::Queued);

        let info = scheduler.task_info(task_id).expect("Failed to get info");
        assert_eq!(info.name, "test_task");
        assert_eq!(info.priority, TaskPriority::Normal);
    }

    #[test]
    fn test_scheduler_cancel_task() {
        let scheduler = BackgroundScheduler::new();

        // Manually add a task
        let task_id = scheduler.next_task_id();
        let info = TaskInfo {
            id: task_id,
            name: "test".to_string(),
            priority: TaskPriority::Normal,
            status: TaskStatus::Running,
            created_at: Instant::now(),
            started_at: Some(Instant::now()),
            completed_at: None,
            progress: 0.5,
            error: None,
        };
        scheduler.tasks.write().insert(task_id, info);

        // Cancel task
        scheduler.cancel_task(task_id).expect("Failed to cancel");
        let status = scheduler
            .task_status(task_id)
            .expect("Failed to get status");
        assert_eq!(status, TaskStatus::Cancelled);
    }
}
