//! Real-time scheduling and deadline management
//!
//! Provides utilities for real-time constrained operations in embedded systems

use crate::error::{EmbeddedError, Result};
use crate::target;
use core::sync::atomic::Ordering;
// `portable-atomic` provides `AtomicU64` on 32-bit bare-metal targets
// (thumbv7em, riscv32imac, …) where `core::sync::atomic::AtomicU64` is absent.
use portable_atomic::AtomicU64;

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

    /// Read the raw monotonic tick counter, expressed in CPU cycles.
    ///
    /// On bare-metal targets this is the hardware cycle counter
    /// ([`target::cycle_count`]). On hosted (`std`) builds — where no cycle
    /// counter feature is enabled and `cycle_count()` returns `None` — a real
    /// monotonic clock ([`target::host_now_us`]) is used instead, scaled by
    /// `cycles_per_us` so that the same [`elapsed_us`](Self::elapsed_us)
    /// conversion (`ticks / cycles_per_us`) yields wall-clock microseconds.
    ///
    /// Returns `None` only on a `no_std` build with no cycle-counter feature
    /// (`arm`/`riscv`/`esp32`) enabled — i.e. a target with genuinely no time
    /// source, where elapsed time cannot be measured.
    #[inline]
    fn raw_ticks(&self) -> Option<u64> {
        #[cfg(feature = "std")]
        {
            // Prefer a hardware counter when a target feature exposes one,
            // otherwise fall back to the host monotonic clock.
            match target::cycle_count() {
                Some(cycles) => Some(cycles),
                None => Some(target::host_now_us().saturating_mul(self.cycles_per_us.max(1))),
            }
        }
        #[cfg(not(feature = "std"))]
        {
            target::cycle_count()
        }
    }

    /// Initialize the scheduler (record start time)
    pub fn init(&self) {
        if let Some(ticks) = self.raw_ticks() {
            self.start_cycles.store(ticks, Ordering::Relaxed);
        }
    }

    /// Get elapsed time in microseconds since init
    ///
    /// Returns `0` only when there is no time source at all (a `no_std` build
    /// with none of the `arm`/`riscv`/`esp32` cycle-counter features enabled);
    /// in that configuration hard-deadline enforcement cannot fire because
    /// elapsed time is unmeasurable. Hosted (`std`) builds and real embedded
    /// targets both return true elapsed microseconds.
    pub fn elapsed_us(&self) -> u64 {
        match self.raw_ticks() {
            Some(current) => {
                let start = self.start_cycles.load(Ordering::Relaxed);
                let elapsed_cycles = current.saturating_sub(start);
                elapsed_cycles / self.cycles_per_us.max(1)
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
///
/// A task may carry a `run` function pointer that is invoked by
/// [`RateMonotonicScheduler::schedule`] when the task becomes ready. Tasks
/// created without a runner (via [`PeriodicTask::new`]) only advance their
/// period bookkeeping when scheduled; use [`PeriodicTask::with_runner`] or
/// [`RateMonotonicScheduler::schedule_with`] to run real work.
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
    /// Optional work executed when the task is ready.
    ///
    /// A plain `fn()` pointer keeps `PeriodicTask` usable in `no_std` targets
    /// without an allocator. Stateful tasks should use
    /// [`RateMonotonicScheduler::schedule_with`] instead.
    run: Option<fn()>,
}

impl PeriodicTask {
    /// Create a new periodic task without an attached runner
    ///
    /// When scheduled, such a task only advances its period bookkeeping and
    /// records a zero-duration execution. Attach work with
    /// [`PeriodicTask::with_runner`] or drive execution through
    /// [`RateMonotonicScheduler::schedule_with`].
    pub const fn new(period_us: u64, budget_us: u64, priority: Priority) -> Self {
        Self {
            period_us,
            budget_us,
            priority,
            last_exec_us: None,
            run: None,
        }
    }

    /// Create a new periodic task with an attached runner function
    ///
    /// The `run` function is executed by [`RateMonotonicScheduler::schedule`]
    /// each time the task becomes ready, and its wall-clock duration is
    /// recorded in the task statistics (including deadline-miss detection
    /// against `budget_us`).
    pub const fn with_runner(
        period_us: u64,
        budget_us: u64,
        priority: Priority,
        run: fn(),
    ) -> Self {
        Self {
            period_us,
            budget_us,
            priority,
            last_exec_us: None,
            run: Some(run),
        }
    }

    /// Get the attached runner, if any
    pub fn runner(&self) -> Option<fn()> {
        self.run
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
    ///
    /// For each ready task the attached runner (see
    /// [`PeriodicTask::with_runner`]) is invoked and its wall-clock duration is
    /// measured against the task budget. Tasks without a runner advance their
    /// period bookkeeping and record a zero-duration execution. Returns the
    /// number of tasks that became ready this tick.
    pub fn schedule(&mut self) -> Result<usize> {
        let current_us = self.scheduler.elapsed_us();
        let mut executed: usize = 0;

        for i in 0..self.tasks.len() {
            if !self.tasks[i].is_ready(current_us) {
                continue;
            }

            let start_us = self.scheduler.elapsed_us();
            if let Some(run) = self.tasks[i].run {
                // Actually execute the task body.
                run();
            }
            let end_us = self.scheduler.elapsed_us();
            let exec_us = end_us.saturating_sub(start_us);

            let missed = exec_us > self.tasks[i].budget_us;
            self.stats[i].record_execution(exec_us, missed);

            self.tasks[i].mark_executed(current_us);
            executed = executed.saturating_add(1);
        }

        Ok(executed)
    }

    /// Schedule ready tasks, executing arbitrary (possibly stateful) work
    ///
    /// For each ready task the `dispatch` closure is invoked with the task's
    /// index; its wall-clock duration is measured against the task budget and
    /// recorded in the task statistics. This is the preferred entry point for
    /// tasks that need to capture state, since [`PeriodicTask`] can only store a
    /// bare `fn()` runner.
    ///
    /// Returns the number of tasks that became ready this tick.
    pub fn schedule_with<F>(&mut self, mut dispatch: F) -> Result<usize>
    where
        F: FnMut(usize),
    {
        let current_us = self.scheduler.elapsed_us();
        let mut executed: usize = 0;

        for i in 0..self.tasks.len() {
            if !self.tasks[i].is_ready(current_us) {
                continue;
            }

            let start_us = self.scheduler.elapsed_us();
            dispatch(i);
            let end_us = self.scheduler.elapsed_us();
            let exec_us = end_us.saturating_sub(start_us);

            let missed = exec_us > self.tasks[i].budget_us;
            self.stats[i].record_execution(exec_us, missed);

            self.tasks[i].mark_executed(current_us);
            executed = executed.saturating_add(1);
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
    fn test_schedule_runs_attached_runner() {
        use core::sync::atomic::AtomicUsize;

        static RUN_COUNT: AtomicUsize = AtomicUsize::new(0);
        fn body() {
            RUN_COUNT.fetch_add(1, Ordering::Relaxed);
        }

        let mut scheduler = RateMonotonicScheduler::<4>::new(1);
        scheduler.init();
        scheduler
            .add_task(PeriodicTask::with_runner(1000, 100, Priority::Normal, body))
            .expect("add_task failed");

        let executed = scheduler.schedule().expect("schedule failed");
        assert_eq!(executed, 1, "ready task should be scheduled");
        assert_eq!(
            RUN_COUNT.load(Ordering::Relaxed),
            1,
            "attached runner must actually execute"
        );

        let stats = scheduler.get_stats(0).expect("stats should exist");
        assert_eq!(stats.executions, 1);
    }

    #[test]
    fn test_schedule_with_closure_executes() {
        let mut scheduler = RateMonotonicScheduler::<4>::new(1);
        scheduler.init();
        scheduler
            .add_task(PeriodicTask::new(1000, 100, Priority::Normal))
            .expect("add_task failed");

        let mut counter = 0usize;
        let executed = scheduler
            .schedule_with(|_idx| {
                counter += 1;
            })
            .expect("schedule_with failed");

        assert_eq!(executed, 1);
        assert_eq!(counter, 1, "dispatch closure must run for the ready task");
    }

    /// Regression test for the previously-silent deadline no-op: on a hosted
    /// (`std`) build the scheduler now uses a real monotonic clock, so a task
    /// that runs past a hard deadline must be reported as `DeadlineMissed`
    /// rather than always succeeding with a zero-elapsed measurement.
    #[cfg(feature = "std")]
    #[test]
    fn test_execute_with_deadline_detects_hard_miss() {
        let scheduler = RealtimeScheduler::new(1);
        scheduler.init();

        let deadline = Deadline::hard(500); // 500 microseconds
        let result = scheduler.execute_with_deadline(deadline, || {
            // Sleep well past the 500us hard deadline.
            std::thread::sleep(core::time::Duration::from_millis(5));
            42u32
        });

        match result {
            Err(EmbeddedError::DeadlineMissed {
                actual_us,
                deadline_us,
            }) => {
                assert_eq!(deadline_us, 500);
                assert!(
                    actual_us > 500,
                    "measured elapsed {actual_us}us should exceed the 500us deadline"
                );
            }
            other => panic!("expected DeadlineMissed, got {other:?}"),
        }
    }

    /// A fast task under a generous hard deadline must still succeed and return
    /// its value on a hosted build (guards against the clock over-reporting).
    #[cfg(feature = "std")]
    #[test]
    fn test_execute_with_deadline_allows_fast_task() {
        let scheduler = RealtimeScheduler::new(1);
        scheduler.init();

        let deadline = Deadline::hard(1_000_000); // 1 second
        let value = scheduler
            .execute_with_deadline(deadline, || 1u32 + 1)
            .expect("fast task well under a 1s deadline must not miss");
        assert_eq!(value, 2);
    }

    /// A soft deadline must never turn a slow task into an error, even when the
    /// host clock reports the overrun.
    #[cfg(feature = "std")]
    #[test]
    fn test_execute_with_soft_deadline_never_errors() {
        let scheduler = RealtimeScheduler::new(1);
        scheduler.init();

        let deadline = Deadline::soft(1); // 1us soft deadline, trivially exceeded
        let value = scheduler
            .execute_with_deadline(deadline, || {
                std::thread::sleep(core::time::Duration::from_millis(2));
                7u32
            })
            .expect("soft deadlines never produce an error");
        assert_eq!(value, 7);
    }

    /// The host clock must advance between two `elapsed_us` reads.
    #[cfg(feature = "std")]
    #[test]
    fn test_elapsed_us_advances_on_host() {
        let scheduler = RealtimeScheduler::new(1);
        scheduler.init();

        let first = scheduler.elapsed_us();
        std::thread::sleep(core::time::Duration::from_millis(2));
        let second = scheduler.elapsed_us();
        assert!(
            second > first,
            "elapsed_us must advance on a hosted build (first={first}, second={second})"
        );
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
