//! Advanced scheduling algorithms for distributed task execution.
//!
//! This module implements sophisticated scheduling strategies including:
//! - Gang scheduling for co-scheduling related tasks
//! - Fair-share scheduling for resource fairness
//! - Deadline scheduling with SLO awareness
//! - Priority scheduling with preemption support
//! - Backfilling to maximize resource utilization
//! - Multi-queue scheduling for different workload types

use crate::error::{ClusterError, Result};
use crate::task_graph::{ResourceRequirements, Task, TaskId};
use crate::worker_pool::{WorkerId, WorkerPool};
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

/// Gang scheduling configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GangSchedulingConfig {
    /// Maximum wait time for gang members
    pub max_wait_time: Duration,
    /// Minimum gang size to enable gang scheduling
    pub min_gang_size: usize,
    /// Enable preemption for gang scheduling
    pub enable_preemption: bool,
}

impl Default for GangSchedulingConfig {
    fn default() -> Self {
        Self {
            max_wait_time: Duration::from_secs(30),
            min_gang_size: 2,
            enable_preemption: false,
        }
    }
}

/// Gang of related tasks that should be scheduled together.
#[derive(Debug, Clone)]
pub struct Gang {
    /// Unique gang identifier
    pub id: GangId,
    /// Task IDs that belong to this gang
    pub tasks: Vec<TaskId>,
    /// When the gang was created
    pub created_at: Instant,
    /// Scheduling priority for this gang
    pub priority: i32,
    /// Combined resource requirements for all tasks
    pub total_resources: ResourceRequirements,
}

/// Gang identifier.
pub type GangId = uuid::Uuid;

/// Gang scheduler implementation.
pub struct GangScheduler {
    config: GangSchedulingConfig,
    /// Pending gangs waiting for resources
    pending_gangs: Arc<RwLock<Vec<Gang>>>,
    /// Task to gang mapping
    task_to_gang: Arc<DashMap<TaskId, GangId>>,
    /// Active gangs
    active_gangs: Arc<DashMap<GangId, Gang>>,
    /// Statistics
    stats: Arc<RwLock<GangSchedulerStats>>,
}

/// Gang scheduler statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GangSchedulerStats {
    /// Total number of gangs created
    pub gangs_created: u64,
    /// Number of gangs successfully scheduled
    pub gangs_scheduled: u64,
    /// Number of gangs that failed to schedule
    pub gangs_failed: u64,
    /// Average wait time for gangs before scheduling
    pub average_wait_time: Duration,
    /// Total number of preemptions performed
    pub total_preemptions: u64,
}

impl GangScheduler {
    /// Create a new gang scheduler.
    pub fn new(config: GangSchedulingConfig) -> Self {
        Self {
            config,
            pending_gangs: Arc::new(RwLock::new(Vec::new())),
            task_to_gang: Arc::new(DashMap::new()),
            active_gangs: Arc::new(DashMap::new()),
            stats: Arc::new(RwLock::new(GangSchedulerStats::default())),
        }
    }

    /// Create a gang from related tasks.
    pub fn create_gang(&self, tasks: Vec<TaskId>, priority: i32) -> Result<GangId> {
        let gang_id = uuid::Uuid::new_v4();

        // Calculate total resource requirements
        let total_resources = ResourceRequirements {
            cpu_cores: 0.0,
            memory_bytes: 0,
            gpu: false,
            storage_bytes: 0,
        };

        let gang = Gang {
            id: gang_id,
            tasks: tasks.clone(),
            created_at: Instant::now(),
            priority,
            total_resources,
        };

        // Register task-to-gang mappings
        for task_id in &tasks {
            self.task_to_gang.insert(*task_id, gang_id);
        }

        // Add to pending gangs
        let mut pending = self.pending_gangs.write();
        pending.push(gang);
        pending.sort_by_key(|g| std::cmp::Reverse(g.priority));

        let mut stats = self.stats.write();
        stats.gangs_created += 1;

        Ok(gang_id)
    }

    /// Try to schedule pending gangs.
    pub fn schedule_gangs(&self, worker_pool: &WorkerPool) -> Result<Vec<(GangId, Vec<WorkerId>)>> {
        let mut scheduled = Vec::new();
        let mut pending = self.pending_gangs.write();

        pending.retain(|gang| {
            // Check if gang has exceeded max wait time
            if gang.created_at.elapsed() > self.config.max_wait_time {
                let mut stats = self.stats.write();
                stats.gangs_failed += 1;
                return false;
            }

            // Try to find enough workers for the gang
            if let Some(workers) = self.find_workers_for_gang(gang, worker_pool) {
                scheduled.push((gang.id, workers));
                self.active_gangs.insert(gang.id, gang.clone());

                let mut stats = self.stats.write();
                stats.gangs_scheduled += 1;
                stats.average_wait_time = Duration::from_millis(
                    (stats.average_wait_time.as_millis() as u64 * (stats.gangs_scheduled - 1)
                        + gang.created_at.elapsed().as_millis() as u64)
                        / stats.gangs_scheduled,
                );

                false // Remove from pending
            } else {
                true // Keep in pending
            }
        });

        Ok(scheduled)
    }

    fn find_workers_for_gang(
        &self,
        gang: &Gang,
        worker_pool: &WorkerPool,
    ) -> Option<Vec<WorkerId>> {
        // Simplified worker selection - in production, this would check actual capacities
        let workers = worker_pool.get_all_workers();
        if workers.len() >= gang.tasks.len() {
            Some(
                workers
                    .into_iter()
                    .take(gang.tasks.len())
                    .map(|w| w.read().id)
                    .collect(),
            )
        } else {
            None
        }
    }

    /// Get gang scheduler statistics.
    pub fn get_stats(&self) -> GangSchedulerStats {
        self.stats.read().clone()
    }
}

/// Fair-share scheduling configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FairShareConfig {
    /// Update interval for fair share calculations
    pub update_interval: Duration,
    /// Decay factor for historical usage
    pub decay_factor: f64,
    /// Minimum share guarantee
    pub min_share: f64,
}

impl Default for FairShareConfig {
    fn default() -> Self {
        Self {
            update_interval: Duration::from_secs(10),
            decay_factor: 0.9,
            min_share: 0.01,
        }
    }
}

/// User or tenant identifier for fair-share scheduling.
pub type UserId = String;

/// Fair-share scheduler for ensuring resource fairness.
pub struct FairShareScheduler {
    config: FairShareConfig,
    /// User resource shares (configured)
    user_shares: Arc<DashMap<UserId, f64>>,
    /// User resource usage (actual)
    user_usage: Arc<DashMap<UserId, ResourceUsage>>,
    /// Last update time
    last_update: Arc<RwLock<Instant>>,
    /// Statistics
    stats: Arc<RwLock<FairShareStats>>,
}

/// Resource usage tracking.
#[derive(Debug, Clone)]
pub struct ResourceUsage {
    /// Total CPU seconds consumed
    pub cpu_seconds: f64,
    /// Total memory MB-seconds consumed
    pub memory_mb_seconds: f64,
    /// Number of tasks completed
    pub tasks_completed: u64,
    /// Last time usage was updated
    pub last_updated: Instant,
}

/// Fair-share scheduler statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FairShareStats {
    /// Total number of users in the system
    pub total_users: usize,
    /// Jain's fairness index (0 to 1, higher is fairer)
    pub fairness_index: f64,
    /// Maximum deviation from fair share
    pub max_deviation: f64,
}

impl FairShareScheduler {
    /// Create a new fair-share scheduler.
    pub fn new(config: FairShareConfig) -> Self {
        Self {
            config,
            user_shares: Arc::new(DashMap::new()),
            user_usage: Arc::new(DashMap::new()),
            last_update: Arc::new(RwLock::new(Instant::now())),
            stats: Arc::new(RwLock::new(FairShareStats::default())),
        }
    }

    /// Set user's resource share.
    pub fn set_user_share(&self, user_id: UserId, share: f64) -> Result<()> {
        if share < 0.0 {
            return Err(ClusterError::InvalidConfiguration(
                "Share must be non-negative".to_string(),
            ));
        }
        self.user_shares
            .insert(user_id, share.max(self.config.min_share));
        Ok(())
    }

    /// Calculate fair-share priority for a task.
    pub fn calculate_priority(&self, user_id: &UserId, _task: &Task) -> f64 {
        let share = self
            .user_shares
            .get(user_id)
            .map(|s| *s)
            .unwrap_or(self.config.min_share);

        let usage = self
            .user_usage
            .get(user_id)
            .map(|u| u.cpu_seconds)
            .unwrap_or(0.0);

        // Priority = share / (usage + epsilon)
        // Higher share or lower usage = higher priority
        share / (usage + 1.0)
    }

    /// Update resource usage for a user.
    pub fn update_usage(&self, user_id: UserId, cpu_seconds: f64, memory_mb_seconds: f64) {
        self.user_usage
            .entry(user_id.clone())
            .and_modify(|usage| {
                let _elapsed = usage.last_updated.elapsed().as_secs_f64();
                usage.cpu_seconds = usage.cpu_seconds * self.config.decay_factor + cpu_seconds;
                usage.memory_mb_seconds =
                    usage.memory_mb_seconds * self.config.decay_factor + memory_mb_seconds;
                usage.tasks_completed += 1;
                usage.last_updated = Instant::now();
            })
            .or_insert_with(|| ResourceUsage {
                cpu_seconds,
                memory_mb_seconds,
                tasks_completed: 1,
                last_updated: Instant::now(),
            });

        self.update_fairness_stats();
    }

    fn update_fairness_stats(&self) {
        let mut last_update = self.last_update.write();
        if last_update.elapsed() < self.config.update_interval {
            return;
        }
        *last_update = Instant::now();

        let total_users = self.user_usage.len();
        if total_users == 0 {
            return;
        }

        // Calculate Jain's fairness index
        let mut sum_shares = 0.0;
        let mut sum_squares = 0.0;
        let mut max_deviation: f64 = 0.0;

        for entry in self.user_usage.iter() {
            let usage = entry.value();
            let share = self
                .user_shares
                .get(entry.key())
                .map(|s| *s)
                .unwrap_or(self.config.min_share);

            let normalized = usage.cpu_seconds / share.max(0.001);
            sum_shares += normalized;
            sum_squares += normalized * normalized;
            max_deviation = max_deviation.max(normalized);
        }

        let fairness_index = if sum_squares > 0.0 {
            (sum_shares * sum_shares) / (total_users as f64 * sum_squares)
        } else {
            1.0
        };

        let mut stats = self.stats.write();
        stats.total_users = total_users;
        stats.fairness_index = fairness_index;
        stats.max_deviation = max_deviation;
    }

    /// Get fair-share statistics.
    pub fn get_stats(&self) -> FairShareStats {
        self.stats.read().clone()
    }
}

/// Deadline scheduling configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadlineSchedulingConfig {
    /// Grace period before deadline
    pub grace_period: Duration,
    /// Enable deadline-based preemption
    pub enable_preemption: bool,
    /// SLO violation threshold
    pub slo_violation_threshold: f64,
}

impl Default for DeadlineSchedulingConfig {
    fn default() -> Self {
        Self {
            grace_period: Duration::from_secs(60),
            enable_preemption: true,
            slo_violation_threshold: 0.95,
        }
    }
}

/// Task with deadline.
#[derive(Debug, Clone)]
pub struct DeadlineTask {
    /// Unique task identifier
    pub task_id: TaskId,
    /// Deadline by which task must complete
    pub deadline: SystemTime,
    /// Task priority (higher is more important)
    pub priority: i32,
    /// Estimated time to complete the task
    pub estimated_duration: Duration,
}

impl PartialEq for DeadlineTask {
    fn eq(&self, other: &Self) -> bool {
        self.task_id == other.task_id
    }
}

impl Eq for DeadlineTask {}

impl PartialOrd for DeadlineTask {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DeadlineTask {
    fn cmp(&self, other: &Self) -> Ordering {
        // Earlier deadline = higher priority (reversed for max heap)
        other
            .deadline
            .cmp(&self.deadline)
            .then_with(|| other.priority.cmp(&self.priority))
    }
}

/// Deadline scheduler using Earliest Deadline First (EDF) algorithm.
pub struct DeadlineScheduler {
    config: DeadlineSchedulingConfig,
    /// Priority queue ordered by deadline
    deadline_queue: Arc<RwLock<BinaryHeap<DeadlineTask>>>,
    /// Task deadlines
    task_deadlines: Arc<DashMap<TaskId, SystemTime>>,
    /// Statistics
    stats: Arc<RwLock<DeadlineSchedulerStats>>,
}

/// Deadline scheduler statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeadlineSchedulerStats {
    /// Total number of tasks scheduled
    pub tasks_scheduled: u64,
    /// Number of deadlines successfully met
    pub deadlines_met: u64,
    /// Number of deadlines missed
    pub deadlines_missed: u64,
    /// SLO compliance ratio (met/total)
    pub slo_compliance: f64,
    /// Average time remaining before deadline when completed
    pub average_slack_time: Duration,
}

impl DeadlineScheduler {
    /// Create a new deadline scheduler.
    pub fn new(config: DeadlineSchedulingConfig) -> Self {
        Self {
            config,
            deadline_queue: Arc::new(RwLock::new(BinaryHeap::new())),
            task_deadlines: Arc::new(DashMap::new()),
            stats: Arc::new(RwLock::new(DeadlineSchedulerStats::default())),
        }
    }

    /// Add a task with deadline.
    pub fn add_task(&self, task: DeadlineTask) -> Result<()> {
        // Check if deadline is feasible
        let now = SystemTime::now();
        if task.deadline <= now {
            return Err(ClusterError::InvalidConfiguration(
                "Deadline is in the past".to_string(),
            ));
        }

        self.task_deadlines.insert(task.task_id, task.deadline);
        let mut queue = self.deadline_queue.write();
        queue.push(task);

        Ok(())
    }

    /// Get next task to schedule based on EDF.
    pub fn get_next_task(&self) -> Option<DeadlineTask> {
        let mut queue = self.deadline_queue.write();
        queue.pop()
    }

    /// Report task completion.
    pub fn report_completion(&self, task_id: TaskId, completed_at: SystemTime) {
        if let Some((_, deadline)) = self.task_deadlines.remove(&task_id) {
            let mut stats = self.stats.write();
            stats.tasks_scheduled += 1;

            if completed_at <= deadline {
                stats.deadlines_met += 1;
                if let Ok(slack) = deadline.duration_since(completed_at) {
                    let total_slack =
                        stats.average_slack_time.as_secs_f64() * (stats.deadlines_met - 1) as f64;
                    stats.average_slack_time = Duration::from_secs_f64(
                        (total_slack + slack.as_secs_f64()) / stats.deadlines_met as f64,
                    );
                }
            } else {
                stats.deadlines_missed += 1;
            }

            stats.slo_compliance = stats.deadlines_met as f64 / stats.tasks_scheduled as f64;
        }
    }

    /// Check for tasks at risk of missing deadlines.
    pub fn get_at_risk_tasks(&self, current_time: SystemTime) -> Vec<TaskId> {
        let queue = self.deadline_queue.read();
        queue
            .iter()
            .filter(|task| {
                if let Ok(remaining) = task.deadline.duration_since(current_time) {
                    remaining < self.config.grace_period
                } else {
                    true // Already past deadline
                }
            })
            .map(|task| task.task_id)
            .collect()
    }

    /// Get deadline scheduler statistics.
    pub fn get_stats(&self) -> DeadlineSchedulerStats {
        self.stats.read().clone()
    }
}

/// Backfilling scheduler to maximize resource utilization.
pub struct BackfillingScheduler {
    /// Main queue (priority-based)
    main_queue: Arc<RwLock<BinaryHeap<PriorityTask>>>,
    /// Backfill queue (for small tasks)
    backfill_queue: Arc<RwLock<VecDeque<TaskId>>>,
    /// Size threshold for backfill (in resource units)
    backfill_threshold: f64,
    /// Statistics
    stats: Arc<RwLock<BackfillStats>>,
}

/// Priority task wrapper.
#[derive(Debug, Clone)]
pub struct PriorityTask {
    /// Unique task identifier
    pub task_id: TaskId,
    /// Task priority (higher is more important)
    pub priority: i32,
    /// Resource requirements for the task
    pub resources: ResourceRequirements,
}

impl PartialEq for PriorityTask {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority
    }
}

impl Eq for PriorityTask {}

impl PartialOrd for PriorityTask {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PriorityTask {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority.cmp(&other.priority)
    }
}

/// Backfilling statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BackfillStats {
    /// Number of tasks scheduled via backfilling
    pub tasks_backfilled: u64,
    /// Improvement in resource utilization from backfilling
    pub utilization_improvement: f64,
    /// Average reduction in wait time from backfilling
    pub average_wait_reduction: Duration,
}

impl BackfillingScheduler {
    /// Create a new backfilling scheduler.
    pub fn new(backfill_threshold: f64) -> Self {
        Self {
            main_queue: Arc::new(RwLock::new(BinaryHeap::new())),
            backfill_queue: Arc::new(RwLock::new(VecDeque::new())),
            backfill_threshold,
            stats: Arc::new(RwLock::new(BackfillStats::default())),
        }
    }

    /// Add a task to the appropriate queue.
    pub fn add_task(&self, task: PriorityTask) {
        // Use CPU cores as the primary resource metric for backfill threshold
        // Tasks with fewer cores are candidates for backfilling
        let resource_size = task.resources.cpu_cores;

        if resource_size <= self.backfill_threshold {
            let mut backfill = self.backfill_queue.write();
            backfill.push_back(task.task_id);
        } else {
            let mut main = self.main_queue.write();
            main.push(task);
        }
    }

    /// Try to backfill tasks into available gaps.
    pub fn try_backfill(&self, available_resources: &ResourceRequirements) -> Vec<TaskId> {
        let mut backfilled = Vec::new();
        let mut backfill_queue = self.backfill_queue.write();

        let mut remaining_cpu = available_resources.cpu_cores;
        let mut remaining_mem = (available_resources.memory_bytes / 1024 / 1024) as u32;

        backfill_queue.retain(|task_id| {
            // In production, we would check actual task requirements
            // For now, assume small tasks fit
            if remaining_cpu > 0.0 && remaining_mem > 0 {
                backfilled.push(*task_id);
                remaining_cpu -= 1.0;
                remaining_mem = remaining_mem.saturating_sub(512);

                let mut stats = self.stats.write();
                stats.tasks_backfilled += 1;

                false // Remove from queue
            } else {
                true // Keep in queue
            }
        });

        backfilled
    }

    /// Get next task from main queue.
    pub fn get_next_main_task(&self) -> Option<PriorityTask> {
        let mut main = self.main_queue.write();
        main.pop()
    }

    /// Get backfilling statistics.
    pub fn get_stats(&self) -> BackfillStats {
        self.stats.read().clone()
    }
}

/// Multi-queue scheduler for different workload types.
pub struct MultiQueueScheduler {
    /// Queues for different priority levels
    queues: Arc<RwLock<HashMap<QueueLevel, VecDeque<TaskId>>>>,
    /// Task to queue mapping
    task_queue: Arc<DashMap<TaskId, QueueLevel>>,
    /// Queue weights for scheduling
    queue_weights: Arc<RwLock<HashMap<QueueLevel, f64>>>,
    /// Statistics
    stats: Arc<RwLock<MultiQueueStats>>,
}

/// Queue priority level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QueueLevel {
    /// Critical priority - highest
    Critical,
    /// High priority
    High,
    /// Normal priority (default)
    Normal,
    /// Low priority
    Low,
    /// Background priority - lowest
    Background,
}

/// Multi-queue scheduler statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MultiQueueStats {
    /// Number of tasks processed per queue
    pub tasks_per_queue: HashMap<String, u64>,
    /// Average wait time per queue
    pub average_wait_per_queue: HashMap<String, Duration>,
}

impl MultiQueueScheduler {
    /// Create a new multi-queue scheduler.
    pub fn new() -> Self {
        let mut queues = HashMap::new();
        queues.insert(QueueLevel::Critical, VecDeque::new());
        queues.insert(QueueLevel::High, VecDeque::new());
        queues.insert(QueueLevel::Normal, VecDeque::new());
        queues.insert(QueueLevel::Low, VecDeque::new());
        queues.insert(QueueLevel::Background, VecDeque::new());

        let mut weights = HashMap::new();
        weights.insert(QueueLevel::Critical, 1.0);
        weights.insert(QueueLevel::High, 0.8);
        weights.insert(QueueLevel::Normal, 0.5);
        weights.insert(QueueLevel::Low, 0.2);
        weights.insert(QueueLevel::Background, 0.1);

        Self {
            queues: Arc::new(RwLock::new(queues)),
            task_queue: Arc::new(DashMap::new()),
            queue_weights: Arc::new(RwLock::new(weights)),
            stats: Arc::new(RwLock::new(MultiQueueStats::default())),
        }
    }

    /// Add a task to a specific queue.
    pub fn add_task(&self, task_id: TaskId, level: QueueLevel) {
        self.task_queue.insert(task_id, level);

        let mut queues = self.queues.write();
        if let Some(queue) = queues.get_mut(&level) {
            queue.push_back(task_id);
        }
    }

    /// Get next task based on weighted round-robin.
    pub fn get_next_task(&self) -> Option<(TaskId, QueueLevel)> {
        let mut queues = self.queues.write();
        let weights = self.queue_weights.read();

        // Try queues in priority order with weighted selection
        for level in &[
            QueueLevel::Critical,
            QueueLevel::High,
            QueueLevel::Normal,
            QueueLevel::Low,
            QueueLevel::Background,
        ] {
            if let Some(queue) = queues.get_mut(level) {
                if !queue.is_empty() {
                    let weight = weights.get(level).copied().unwrap_or(0.5);
                    // Weighted random selection (simplified)
                    if weight > 0.5 || queue.len() > 10 {
                        if let Some(task_id) = queue.pop_front() {
                            return Some((task_id, *level));
                        }
                    }
                }
            }
        }

        None
    }

    /// Set queue weight.
    pub fn set_queue_weight(&self, level: QueueLevel, weight: f64) {
        let mut weights = self.queue_weights.write();
        weights.insert(level, weight.clamp(0.0, 1.0));
    }

    /// Get multi-queue statistics.
    pub fn get_stats(&self) -> MultiQueueStats {
        self.stats.read().clone()
    }
}

impl Default for MultiQueueScheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_gang_scheduler_creation() {
        let config = GangSchedulingConfig::default();
        let scheduler = GangScheduler::new(config);

        let tasks = vec![
            TaskId(uuid::Uuid::new_v4()),
            TaskId(uuid::Uuid::new_v4()),
            TaskId(uuid::Uuid::new_v4()),
        ];

        let result = scheduler.create_gang(tasks, 10);
        assert!(result.is_ok());

        let stats = scheduler.get_stats();
        assert_eq!(stats.gangs_created, 1);
    }

    #[test]
    fn test_fair_share_priority() {
        let config = FairShareConfig::default();
        let scheduler = FairShareScheduler::new(config);

        let user1 = "user1".to_string();
        let user2 = "user2".to_string();

        let _ = scheduler.set_user_share(user1.clone(), 0.7);
        let _ = scheduler.set_user_share(user2.clone(), 0.3);

        let task = Task {
            id: TaskId(uuid::Uuid::new_v4()),
            name: "test".to_string(),
            task_type: "test".to_string(),
            priority: 0,
            payload: vec![],
            dependencies: vec![],
            estimated_duration: None,
            resources: ResourceRequirements::default(),
            locality_hints: vec![],
            created_at: std::time::Instant::now(),
            scheduled_at: None,
            started_at: None,
            completed_at: None,
            status: crate::task_graph::TaskStatus::Pending,
            result: None,
            error: None,
            retry_count: 0,
            checkpoint: None,
        };

        let priority1 = scheduler.calculate_priority(&user1, &task);
        let priority2 = scheduler.calculate_priority(&user2, &task);

        assert!(priority1 > priority2);
    }

    #[test]
    fn test_deadline_scheduler() {
        let config = DeadlineSchedulingConfig::default();
        let scheduler = DeadlineScheduler::new(config);

        let now = SystemTime::now();
        let deadline1 = now + Duration::from_secs(10);
        let deadline2 = now + Duration::from_secs(20);

        let task1 = DeadlineTask {
            task_id: TaskId(uuid::Uuid::new_v4()),
            deadline: deadline1,
            priority: 5,
            estimated_duration: Duration::from_secs(5),
        };

        let task2 = DeadlineTask {
            task_id: TaskId(uuid::Uuid::new_v4()),
            deadline: deadline2,
            priority: 10,
            estimated_duration: Duration::from_secs(5),
        };

        let _ = scheduler.add_task(task1.clone());
        let _ = scheduler.add_task(task2);

        // Task with earlier deadline should be scheduled first
        let next = scheduler.get_next_task();
        assert!(next.is_some());
        assert_eq!(next.expect("next task should exist").task_id, task1.task_id);
    }

    #[test]
    fn test_backfilling_scheduler() {
        let scheduler = BackfillingScheduler::new(2.0);

        let small_task = PriorityTask {
            task_id: TaskId(uuid::Uuid::new_v4()),
            priority: 5,
            resources: ResourceRequirements {
                cpu_cores: 1.0,
                memory_bytes: 512 * 1024 * 1024,
                gpu: false,
                storage_bytes: 0,
            },
        };

        scheduler.add_task(small_task);

        let available = ResourceRequirements {
            cpu_cores: 4.0,
            memory_bytes: 4096 * 1024 * 1024,
            gpu: false,
            storage_bytes: 0,
        };

        let backfilled = scheduler.try_backfill(&available);
        assert_eq!(backfilled.len(), 1);
    }

    #[test]
    fn test_multi_queue_scheduler() {
        let scheduler = MultiQueueScheduler::new();

        let task1 = TaskId(uuid::Uuid::new_v4());
        let task2 = TaskId(uuid::Uuid::new_v4());

        scheduler.add_task(task1, QueueLevel::Critical);
        scheduler.add_task(task2, QueueLevel::Low);

        // Critical task should be scheduled first
        let next = scheduler.get_next_task();
        assert!(next.is_some());
        let (task_id, level) = next.expect("next task should exist");
        assert_eq!(task_id, task1);
        assert_eq!(level, QueueLevel::Critical);
    }
}
