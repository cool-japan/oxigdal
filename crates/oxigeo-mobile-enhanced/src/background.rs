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
//! use oxigeo_mobile_enhanced::background::{BackgroundScheduler, TaskPriority};
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
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// A type-erased, boxed unit of work submitted to the scheduler.
///
/// Boxing erases the caller's concrete `F`/`Fut` types so the scheduler can
/// store heterogeneous tasks in a single map and execute them on a plain
/// worker thread via [`futures::executor::block_on`] -- this requires no
/// async runtime (`tokio`) to be linked, so it works regardless of whether
/// the optional `background-tasks` feature (which pulls in `tokio`) is
/// enabled.
type BoxedTask = Pin<Box<dyn Future<Output = Result<()>> + Send>>;

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
///
/// Submitted work is genuinely executed: [`schedule_task`](Self::schedule_task)
/// boxes the caller's future and stores it in a `pending` map keyed by
/// [`TaskId`]; a dedicated background poller thread (spawned in [`new`](Self::new)
/// and stopped in [`Drop`]) together with the immediate dispatch attempt made
/// by `schedule_task` itself pulls futures out of that map and drives them to
/// completion on a plain OS worker thread via [`futures::executor::block_on`]
/// -- no async runtime is required, so this works whether or not the optional
/// `background-tasks` (`tokio`) feature is enabled. [`TaskInfo::status`]
/// transitions `Queued -> Running -> Completed`/`Failed` as execution
/// actually happens.
pub struct BackgroundScheduler {
    tasks: Arc<RwLock<HashMap<TaskId, TaskInfo>>>,
    /// Boxed futures for tasks that are `Queued` but have not yet been
    /// dispatched to a worker thread. A task's future is removed from this
    /// map the moment it starts running (or is cancelled).
    pending: Arc<Mutex<HashMap<TaskId, BoxedTask>>>,
    next_id: Arc<RwLock<TaskId>>,
    battery_monitor: BatteryMonitor,
    max_concurrent_tasks: usize,
    /// Signals the background poller thread (see [`new`](Self::new)) to stop.
    shutdown: Arc<AtomicBool>,
}

/// Attempts to dispatch as many `Queued` tasks (that still have a stored
/// future in `pending`) as the concurrency limit and battery-aware
/// processing mode currently allow. Each dispatched task is moved to
/// `Running` and driven to completion on its own OS thread. Returns the
/// number of tasks dispatched by this call.
///
/// This is a free function (rather than a `&self` method) so it can be
/// shared between `schedule_task`'s immediate-dispatch attempt and the
/// scheduler's background poller thread without requiring `BatteryMonitor`
/// (or all of `BackgroundScheduler`) to be `Sync`: each caller supplies its
/// own `BatteryMonitor` reference.
fn dispatch_ready_tasks(
    tasks: &Arc<RwLock<HashMap<TaskId, TaskInfo>>>,
    pending: &Arc<Mutex<HashMap<TaskId, BoxedTask>>>,
    max_concurrent_tasks: usize,
    battery_monitor: &BatteryMonitor,
) -> usize {
    let mode = battery_monitor.recommended_processing_mode();
    let mut dispatched = 0usize;

    loop {
        let running_count = tasks
            .read()
            .values()
            .filter(|t| t.status == TaskStatus::Running)
            .count();
        if running_count >= max_concurrent_tasks {
            break;
        }

        // Pick the highest-priority queued task that is both runnable under
        // the current battery mode and still has a future waiting to run.
        let next_id = {
            let pending_guard = pending.lock();
            let tasks_guard = tasks.read();
            tasks_guard
                .values()
                .filter(|info| {
                    info.status == TaskStatus::Queued && pending_guard.contains_key(&info.id)
                })
                .filter(|info| match mode {
                    ProcessingMode::PowerSaver => info.priority.can_run_in_power_saver(),
                    ProcessingMode::Balanced => !matches!(info.priority, TaskPriority::Idle),
                    ProcessingMode::HighPerformance => true,
                })
                .max_by_key(|info| info.priority)
                .map(|info| info.id)
        };

        let Some(task_id) = next_id else {
            break;
        };

        let future = {
            let mut pending_guard = pending.lock();
            match pending_guard.remove(&task_id) {
                Some(f) => f,
                // Raced with another dispatch call; nothing left to run.
                None => break,
            }
        };

        {
            let mut tasks_guard = tasks.write();
            if let Some(info) = tasks_guard.get_mut(&task_id) {
                info.status = TaskStatus::Running;
                info.started_at = Some(Instant::now());
            }
        }

        let tasks_for_thread = Arc::clone(tasks);
        std::thread::spawn(move || {
            let result = futures::executor::block_on(future);
            let mut tasks_guard = tasks_for_thread.write();
            if let Some(info) = tasks_guard.get_mut(&task_id) {
                // The task may have been cancelled/suspended concurrently;
                // only a still-`Running` task transitions to a terminal state
                // here.
                if info.status == TaskStatus::Running {
                    info.completed_at = Some(Instant::now());
                    info.progress = 1.0;
                    match result {
                        Ok(()) => info.status = TaskStatus::Completed,
                        Err(e) => {
                            info.status = TaskStatus::Failed;
                            info.error = Some(e.to_string());
                        }
                    }
                }
            }
        });

        dispatched += 1;
    }

    dispatched
}

impl BackgroundScheduler {
    /// Create a new background scheduler.
    ///
    /// Spawns a background poller thread that periodically retries
    /// dispatching queued tasks (e.g. once a running task frees a concurrency
    /// slot, or the battery-aware processing mode improves). The thread is
    /// stopped when this scheduler is dropped.
    pub fn new() -> Self {
        let tasks: Arc<RwLock<HashMap<TaskId, TaskInfo>>> = Arc::new(RwLock::new(HashMap::new()));
        let pending: Arc<Mutex<HashMap<TaskId, BoxedTask>>> = Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let max_concurrent_tasks = 3;

        {
            let tasks = Arc::clone(&tasks);
            let pending = Arc::clone(&pending);
            let shutdown = Arc::clone(&shutdown);
            std::thread::spawn(move || {
                // The poller uses its own `BatteryMonitor` instance so this
                // thread never shares mutable battery-monitor state with the
                // scheduler's own monitor.
                let poll_monitor = BatteryMonitor::new().ok().unwrap_or_default();
                while !shutdown.load(Ordering::Relaxed) {
                    dispatch_ready_tasks(&tasks, &pending, max_concurrent_tasks, &poll_monitor);
                    std::thread::sleep(Duration::from_millis(50));
                }
            });
        }

        Self {
            tasks,
            pending,
            next_id: Arc::new(RwLock::new(0)),
            battery_monitor: BatteryMonitor::new().ok().unwrap_or_default(),
            max_concurrent_tasks,
            shutdown,
        }
    }

    /// Generate next task ID
    fn next_task_id(&self) -> TaskId {
        let mut id = self.next_id.write();
        let current = *id;
        *id = id.wrapping_add(1);
        current
    }

    /// Schedule a background task.
    ///
    /// The submitted `task_fn` is invoked to produce a future, which is boxed
    /// and stored until it can actually run. If a concurrency slot is
    /// immediately available (and the current battery-aware processing mode
    /// permits this task's priority), it starts executing on a dedicated
    /// worker thread right away; otherwise it remains queued and the
    /// scheduler's background poller thread (see [`new`](Self::new)) will
    /// start it as soon as capacity frees up.
    pub async fn schedule_task<F, Fut>(
        &self,
        name: &str,
        priority: TaskPriority,
        task_fn: F,
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

        let future: BoxedTask = Box::pin(task_fn());
        self.pending.lock().insert(task_id, future);

        // Attempt to run it right away if capacity/battery policy allow.
        dispatch_ready_tasks(
            &self.tasks,
            &self.pending,
            self.max_concurrent_tasks,
            &self.battery_monitor,
        );

        Ok(task_id)
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

    /// Cancel a task.
    ///
    /// If the task's future has not yet started running, it is dropped
    /// immediately (removed from the pending queue) and will never execute.
    /// A task that is already `Running` on a worker thread is marked
    /// `Cancelled` here, but -- since the underlying [`Future`] has no
    /// cooperative cancellation hook -- the in-flight work still runs to
    /// completion on its thread; its result is simply discarded (the
    /// dispatcher only promotes a task to `Completed`/`Failed` while it is
    /// still `Running`, so the `Cancelled` status set here is preserved).
    pub fn cancel_task(&self, task_id: TaskId) -> Result<()> {
        let mut tasks = self.tasks.write();
        if let Some(info) = tasks.get_mut(&task_id) {
            if info.is_active() {
                info.status = TaskStatus::Cancelled;
                info.completed_at = Some(Instant::now());
                drop(tasks);
                // Drop the future for a not-yet-started task so it never runs.
                self.pending.lock().remove(&task_id);
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

    /// Resume suspended tasks.
    ///
    /// Note: this restores bookkeeping status (`Suspended` -> `Queued`) for
    /// tasks whose future had not yet started when they were suspended, and
    /// the background poller will dispatch them once resumed. A task that
    /// was suspended *after* its future had already started running on a
    /// worker thread (see [`cancel_task`](Self::cancel_task) for the same
    /// caveat about in-flight work) has no future left to re-dispatch, since
    /// genuine pause/resume of an already-running [`Future`] would require
    /// cooperative cancellation support that the submitted task closure does
    /// not provide.
    pub fn resume_suspended_tasks(&self) -> Result<Vec<TaskId>> {
        let mut tasks = self.tasks.write();
        let mut resumed = Vec::new();

        for (id, info) in tasks.iter_mut() {
            if info.status == TaskStatus::Suspended {
                info.status = TaskStatus::Queued;
                resumed.push(*id);
            }
        }
        drop(tasks);

        if !resumed.is_empty() {
            dispatch_ready_tasks(
                &self.tasks,
                &self.pending,
                self.max_concurrent_tasks,
                &self.battery_monitor,
            );
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
            if info.is_finished()
                && let Some(completed_at) = info.completed_at
                && now.duration_since(completed_at) > older_than
            {
                removed += 1;
                return false;
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

impl Drop for BackgroundScheduler {
    fn drop(&mut self) {
        // Signal the background poller thread (spawned in `new`) to stop;
        // it observes this within one poll interval and exits on its own.
        self.shutdown.store(true, Ordering::Relaxed);
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

        // Normal priority with an empty scheduler dispatches immediately, so
        // by the time `schedule_task` returns the task has already been
        // moved out of `Queued` (into `Running`, and typically `Completed`
        // almost immediately after since the body is trivial).
        wait_for_finished(&scheduler, task_id);

        let info = scheduler.task_info(task_id).expect("Failed to get info");
        assert_eq!(info.name, "test_task");
        assert_eq!(info.priority, TaskPriority::Normal);
        assert_eq!(info.status, TaskStatus::Completed);
    }

    /// Polls `task_status` until the task leaves `Queued`/`Running`, or a
    /// generous timeout elapses. Used to deterministically wait for the
    /// scheduler's worker/poller threads without a fixed sleep.
    fn wait_for_finished(scheduler: &BackgroundScheduler, task_id: TaskId) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match scheduler.task_status(task_id) {
                Ok(TaskStatus::Queued | TaskStatus::Running) => {
                    if Instant::now() >= deadline {
                        panic!("task {} did not finish executing in time", task_id);
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                _ => return,
            }
        }
    }

    /// Verifies the critical fix: `schedule_task` actually executes the
    /// submitted closure (a real side effect is observed), rather than only
    /// recording bookkeeping and silently dropping the work.
    #[tokio::test]
    async fn test_schedule_task_actually_executes_closure() {
        let scheduler = BackgroundScheduler::new();

        let executed = Arc::new(AtomicBool::new(false));
        let executed_for_task = Arc::clone(&executed);

        let task_id = scheduler
            .schedule_task("side_effect_task", TaskPriority::Normal, move || {
                let executed = Arc::clone(&executed_for_task);
                async move {
                    executed.store(true, Ordering::SeqCst);
                    Ok(())
                }
            })
            .await
            .expect("Failed to schedule task");

        wait_for_finished(&scheduler, task_id);

        assert!(
            executed.load(Ordering::SeqCst),
            "schedule_task must actually run the submitted closure, not just record it as queued"
        );

        let info = scheduler.task_info(task_id).expect("Failed to get info");
        assert_eq!(info.status, TaskStatus::Completed);
        assert!(info.started_at.is_some());
        assert!(info.completed_at.is_some());
    }

    /// A task whose closure returns `Err` must surface as `Failed` with the
    /// error message recorded, never silently reported as success.
    #[tokio::test]
    async fn test_schedule_task_propagates_failure() {
        let scheduler = BackgroundScheduler::new();

        let task_id = scheduler
            .schedule_task("failing_task", TaskPriority::Normal, || async {
                Err(MobileError::BackgroundTaskError("boom".to_string()))
            })
            .await
            .expect("Failed to schedule task");

        wait_for_finished(&scheduler, task_id);

        let info = scheduler.task_info(task_id).expect("Failed to get info");
        assert_eq!(info.status, TaskStatus::Failed);
        assert!(info.error.is_some_and(|e| e.contains("boom")));
    }

    /// `Idle` priority tasks must queue (not fabricate immediate success)
    /// once the concurrency limit is saturated by long-running tasks, and
    /// must actually run once a slot frees up (background poller behavior).
    #[tokio::test]
    async fn test_schedule_task_queues_when_saturated_then_runs() {
        let scheduler = BackgroundScheduler::new();

        // Saturate all 3 concurrency slots with slow tasks.
        let release = Arc::new(AtomicBool::new(false));
        for i in 0..3 {
            let release = Arc::clone(&release);
            scheduler
                .schedule_task(&format!("blocker_{i}"), TaskPriority::Critical, move || {
                    let release = Arc::clone(&release);
                    async move {
                        while !release.load(Ordering::SeqCst) {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Ok(())
                    }
                })
                .await
                .expect("Failed to schedule blocker");
        }

        let executed = Arc::new(AtomicBool::new(false));
        let executed_for_task = Arc::clone(&executed);
        let queued_id = scheduler
            .schedule_task("queued_task", TaskPriority::Normal, move || {
                let executed = Arc::clone(&executed_for_task);
                async move {
                    executed.store(true, Ordering::SeqCst);
                    Ok(())
                }
            })
            .await
            .expect("Failed to schedule queued task");

        // While all slots are saturated, the new task must remain queued
        // rather than fabricating a false "ran" status.
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(
            scheduler.task_status(queued_id).expect("status must exist"),
            TaskStatus::Queued
        );
        assert!(!executed.load(Ordering::SeqCst));

        // Free up the blockers; the poller thread must then actually run
        // the queued task.
        release.store(true, Ordering::SeqCst);
        wait_for_finished(&scheduler, queued_id);

        assert!(
            executed.load(Ordering::SeqCst),
            "queued task must eventually run once capacity frees up"
        );
        assert_eq!(
            scheduler.task_status(queued_id).expect("status must exist"),
            TaskStatus::Completed
        );
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
