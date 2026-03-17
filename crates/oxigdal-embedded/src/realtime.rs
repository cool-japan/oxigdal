//! Real-time scheduling and deadline management
//!
//! Provides utilities for real-time constrained operations in embedded systems

use crate::error::{EmbeddedError, Result};
use crate::target;
use core::sync::atomic::{AtomicU64, Ordering};

/// Real-time priority levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Priority {
    /// Idle priority (lowest)
    Idle = 0,
    /// Low priority
    Low = 1,
    /// Normal priority
    Normal = 2,
    /// High priority
    High = 3,
    /// Critical priority (highest)
    Critical = 4,
}

impl Priority {
    /// Get priority from u8
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Idle),
            1 => Some(Self::Low),
            2 => Some(Self::Normal),
            3 => Some(Self::High),
            4 => Some(Self::Critical),
            _ => None,
        }
    }
}

/// Deadline specification
#[derive(Debug, Clone, Copy)]
pub struct Deadline {
    /// Deadline time in microseconds
    pub time_us: u64,
    /// Is this a hard deadline (must be met)?
    pub is_hard: bool,
}

impl Deadline {
    /// Create a new soft deadline
    pub const fn soft(time_us: u64) -> Self {
        Self {
            time_us,
            is_hard: false,
        }
    }

    /// Create a new hard deadline
    pub const fn hard(time_us: u64) -> Self {
        Self {
            time_us,
            is_hard: true,
        }
    }

    /// Check if deadline is expired
    pub fn is_expired(&self, current_us: u64) -> bool {
        current_us >= self.time_us
    }

    /// Get remaining time in microseconds
    pub fn remaining_us(&self, current_us: u64) -> u64 {
        self.time_us.saturating_sub(current_us)
    }
}

/// Real-time scheduler
pub struct RealtimeScheduler {
    start_cycles: AtomicU64,
    cycles_per_us: u64,
}

impl RealtimeScheduler {
    /// Create a new real-time scheduler
    ///
    /// # Arguments
    ///
    /// * `cpu_freq_mhz` - CPU frequency in MHz
    pub const fn new(cpu_freq_mhz: u64) -> Self {
        Self {
            start_cycles: AtomicU64::new(0),
            cycles_per_us: cpu_freq_mhz,
        }
    }

    /// Initialize the scheduler (record start time)
    pub fn init(&self) {
        if let Some(cycles) = target::cycle_count() {
            self.start_cycles.store(cycles, Ordering::Relaxed);
        }
    }

    /// Get elapsed time in microseconds since init
    pub fn elapsed_us(&self) -> u64 {
        match target::cycle_count() {
            Some(current) => {
                let start = self.start_cycles.load(Ordering::Relaxed);
                let elapsed_cycles = current.saturating_sub(start);
                elapsed_cycles / self.cycles_per_us
            }
            None => 0,
        }
    }

    /// Execute a function with a deadline
    ///
    /// # Errors
    ///
    /// Returns `DeadlineMissed` if the deadline is exceeded
    pub fn execute_with_deadline<F, T>(&self, deadline: Deadline, f: F) -> Result<T>
    where
        F: FnOnce() -> T,
    {
        let start_us = self.elapsed_us();
        let result = f();
        let end_us = self.elapsed_us();

        let elapsed = end_us.saturating_sub(start_us);

        if deadline.is_hard && elapsed > deadline.time_us {
            return Err(EmbeddedError::DeadlineMissed {
                actual_us: elapsed,
                deadline_us: deadline.time_us,
            });
        }

        Ok(result)
    }

    /// Check if deadline can be met
    pub fn can_meet_deadline(&self, deadline: &Deadline) -> bool {
        let current_us = self.elapsed_us();
        !deadline.is_expired(current_us)
    }

    /// Get time until deadline
    pub fn time_until_deadline(&self, deadline: &Deadline) -> u64 {
        let current_us = self.elapsed_us();
        deadline.remaining_us(current_us)
    }
}

/// Periodic task specification
#[derive(Debug, Clone)]
pub struct PeriodicTask {
    /// Period in microseconds
    pub period_us: u64,
    /// Execution time budget in microseconds
    pub budget_us: u64,
    /// Priority
    pub priority: Priority,
    /// Last execution time (None if never executed)
    last_exec_us: Option<u64>,
}

impl PeriodicTask {
    /// Create a new periodic task
    pub const fn new(period_us: u64, budget_us: u64, priority: Priority) -> Self {
        Self {
            period_us,
            budget_us,
            priority,
            last_exec_us: None,
        }
    }

    /// Check if task is ready to execute
    pub fn is_ready(&self, current_us: u64) -> bool {
        match self.last_exec_us {
            // First execution: task is always ready
            None => true,
            // Subsequent executions: check if period has elapsed
            Some(last) => current_us.saturating_sub(last) >= self.period_us,
        }
    }

    /// Mark task as executed
    pub fn mark_executed(&mut self, current_us: u64) {
        self.last_exec_us = Some(current_us);
    }

    /// Get deadline for next execution
    pub fn next_deadline(&self) -> Deadline {
        let last = self.last_exec_us.unwrap_or(0);
        Deadline::hard(last + self.period_us + self.budget_us)
    }
}

/// Task timing statistics
#[derive(Debug, Clone, Copy, Default)]
pub struct TaskStats {
    /// Total executions
    pub executions: u64,
    /// Minimum execution time (microseconds)
    pub min_exec_us: u64,
    /// Maximum execution time (microseconds)
    pub max_exec_us: u64,
    /// Total execution time (microseconds)
    pub total_exec_us: u64,
    /// Number of deadline misses
    pub deadline_misses: u64,
}

impl TaskStats {
    /// Create new task statistics
    pub const fn new() -> Self {
        Self {
            executions: 0,
            min_exec_us: u64::MAX,
            max_exec_us: 0,
            total_exec_us: 0,
            deadline_misses: 0,
        }
    }

    /// Record an execution
    pub fn record_execution(&mut self, exec_us: u64, missed_deadline: bool) {
        self.executions = self.executions.saturating_add(1);
        self.total_exec_us = self.total_exec_us.saturating_add(exec_us);

        if exec_us < self.min_exec_us {
            self.min_exec_us = exec_us;
        }

        if exec_us > self.max_exec_us {
            self.max_exec_us = exec_us;
        }

        if missed_deadline {
            self.deadline_misses = self.deadline_misses.saturating_add(1);
        }
    }

    /// Get average execution time
    pub fn avg_exec_us(&self) -> u64 {
        self.total_exec_us.checked_div(self.executions).unwrap_or(0)
    }

    /// Get deadline miss rate
    pub fn miss_rate(&self) -> f32 {
        if self.executions == 0 {
            0.0
        } else {
            self.deadline_misses as f32 / self.executions as f32
        }
    }
}

/// Rate monotonic scheduler
///
/// Tasks are assigned priorities based on their periods (shorter period = higher priority)
pub struct RateMonotonicScheduler<const MAX_TASKS: usize> {
    tasks: heapless::Vec<PeriodicTask, MAX_TASKS>,
    stats: heapless::Vec<TaskStats, MAX_TASKS>,
    scheduler: RealtimeScheduler,
}

impl<const MAX_TASKS: usize> RateMonotonicScheduler<MAX_TASKS> {
    /// Create a new rate monotonic scheduler
    pub const fn new(cpu_freq_mhz: u64) -> Self {
        Self {
            tasks: heapless::Vec::new(),
            stats: heapless::Vec::new(),
            scheduler: RealtimeScheduler::new(cpu_freq_mhz),
        }
    }

    /// Initialize the scheduler
    pub fn init(&mut self) {
        self.scheduler.init();
    }

    /// Add a periodic task
    ///
    /// # Errors
    ///
    /// Returns error if maximum tasks reached
    pub fn add_task(&mut self, task: PeriodicTask) -> Result<()> {
        self.tasks
            .push(task)
            .map_err(|_| EmbeddedError::BufferTooSmall {
                required: 1,
                available: 0,
            })?;

        self.stats
            .push(TaskStats::new())
            .map_err(|_| EmbeddedError::BufferTooSmall {
                required: 1,
                available: 0,
            })?;

        // Sort tasks by period (rate monotonic scheduling)
        self.sort_tasks();

        Ok(())
    }

    /// Sort tasks by period (shortest period first)
    fn sort_tasks(&mut self) {
        let len = self.tasks.len();

        for i in 0..len {
            for j in (i + 1)..len {
                if self.tasks[j].period_us < self.tasks[i].period_us {
                    self.tasks.swap(i, j);
                    self.stats.swap(i, j);
                }
            }
        }
    }

    /// Schedule and execute ready tasks
    pub fn schedule(&mut self) -> Result<usize> {
        let current_us = self.scheduler.elapsed_us();
        let mut executed: usize = 0;

        for (i, task) in self.tasks.iter_mut().enumerate() {
            if task.is_ready(current_us) {
                let start_us = self.scheduler.elapsed_us();

                // Task would be executed here
                // For now, just mark as executed

                let end_us = self.scheduler.elapsed_us();
                let exec_us = end_us.saturating_sub(start_us);

                let missed = exec_us > task.budget_us;
                self.stats[i].record_execution(exec_us, missed);

                task.mark_executed(current_us);
                executed = executed.saturating_add(1);
            }
        }

        Ok(executed)
    }

    /// Get statistics for a task
    pub fn get_stats(&self, task_index: usize) -> Option<&TaskStats> {
        self.stats.get(task_index)
    }

    /// Get number of tasks
    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }
}

/// Watchdog timer for deadline monitoring
pub struct Watchdog {
    timeout_us: u64,
    last_feed_us: AtomicU64,
}

impl Watchdog {
    /// Create a new watchdog with timeout
    pub const fn new(timeout_us: u64) -> Self {
        Self {
            timeout_us,
            last_feed_us: AtomicU64::new(0),
        }
    }

    /// Feed the watchdog (reset timer)
    pub fn feed(&self, current_us: u64) {
        self.last_feed_us.store(current_us, Ordering::Release);
    }

    /// Check if watchdog has expired
    pub fn is_expired(&self, current_us: u64) -> bool {
        let last_feed = self.last_feed_us.load(Ordering::Acquire);
        current_us.saturating_sub(last_feed) >= self.timeout_us
    }

    /// Get time until expiry
    pub fn time_until_expiry(&self, current_us: u64) -> u64 {
        let last_feed = self.last_feed_us.load(Ordering::Acquire);
        let elapsed = current_us.saturating_sub(last_feed);
        self.timeout_us.saturating_sub(elapsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_ordering() {
        assert!(Priority::Idle < Priority::Low);
        assert!(Priority::Low < Priority::Normal);
        assert!(Priority::Normal < Priority::High);
        assert!(Priority::High < Priority::Critical);
    }

    #[test]
    fn test_deadline() {
        let deadline = Deadline::hard(1000);
        assert!(!deadline.is_expired(500));
        assert!(deadline.is_expired(1000));
        assert_eq!(deadline.remaining_us(500), 500);
    }

    #[test]
    fn test_periodic_task() {
        let mut task = PeriodicTask::new(1000, 100, Priority::Normal);
        assert!(task.is_ready(0));
        task.mark_executed(0);
        assert!(!task.is_ready(500));
        assert!(task.is_ready(1000));
    }

    #[test]
    fn test_task_stats() {
        let mut stats = TaskStats::new();
        stats.record_execution(100, false);
        stats.record_execution(200, false);
        stats.record_execution(150, true);

        assert_eq!(stats.executions, 3);
        assert_eq!(stats.min_exec_us, 100);
        assert_eq!(stats.max_exec_us, 200);
        assert_eq!(stats.avg_exec_us(), 150);
        assert_eq!(stats.deadline_misses, 1);
    }

    #[test]
    fn test_watchdog() {
        let watchdog = Watchdog::new(1000);
        watchdog.feed(0);

        assert!(!watchdog.is_expired(500));
        assert!(watchdog.is_expired(1000));
        assert_eq!(watchdog.time_until_expiry(500), 500);
    }
}
