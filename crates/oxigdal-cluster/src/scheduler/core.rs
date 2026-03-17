//! Distributed task scheduler with work-stealing and priority queues.
//!
//! This module implements a sophisticated distributed scheduler with features including:
//! - Work-stealing scheduler with priority queues
//! - Dynamic load balancing across workers
//! - Backpressure handling
//! - Task dependency resolution
//! - Scheduler metrics and monitoring

use crate::error::{ClusterError, Result};
use crate::metrics::ClusterMetrics;
use crate::task_graph::{Task, TaskGraph, TaskId, TaskStatus};
use crate::worker_pool::{SelectionStrategy, WorkerId, WorkerPool};
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering};
use std::time::{Duration, Instant};
use tokio::sync::{Notify, Semaphore};
use tracing::{debug, error, info, warn};

/// Distributed task scheduler.
#[derive(Clone)]
pub struct Scheduler {
    inner: Arc<SchedulerInner>,
}

struct SchedulerInner {
    /// Task graph
    task_graph: Arc<TaskGraph>,

    /// Worker pool
    worker_pool: Arc<WorkerPool>,

    /// Cluster metrics
    metrics: Arc<ClusterMetrics>,

    /// Global priority queue
    global_queue: Arc<RwLock<BinaryHeap<PriorityTask>>>,

    /// Per-worker queues (work-stealing)
    worker_queues: Arc<DashMap<WorkerId, Arc<RwLock<VecDeque<TaskId>>>>>,

    /// Task assignments (task -> worker)
    task_assignments: Arc<DashMap<TaskId, WorkerId>>,

    /// Worker task counts (for load balancing)
    worker_task_counts: Arc<DashMap<WorkerId, AtomicU64>>,

    /// Configuration
    config: SchedulerConfig,

    /// Running flag
    running: AtomicBool,

    /// Backpressure semaphore
    backpressure: Arc<Semaphore>,

    /// Scheduling notification
    schedule_notify: Arc<Notify>,

    /// Scheduler statistics
    stats: Arc<SchedulerStatistics>,
}

/// Scheduler configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerConfig {
    /// Maximum queue size (for backpressure)
    pub max_queue_size: usize,

    /// Work-stealing threshold (steal if queue > threshold)
    pub work_steal_threshold: usize,

    /// Scheduling interval
    pub scheduling_interval: Duration,

    /// Load balancing strategy
    pub load_balance_strategy: LoadBalanceStrategy,

    /// Task timeout
    pub task_timeout: Duration,

    /// Enable work stealing
    pub enable_work_stealing: bool,

    /// Enable backpressure
    pub enable_backpressure: bool,

    /// Maximum concurrent tasks per worker
    pub max_concurrent_tasks_per_worker: usize,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            max_queue_size: 10000,
            work_steal_threshold: 10,
            scheduling_interval: Duration::from_millis(100),
            load_balance_strategy: LoadBalanceStrategy::Adaptive,
            task_timeout: Duration::from_secs(3600),
            enable_work_stealing: true,
            enable_backpressure: true,
            max_concurrent_tasks_per_worker: 100,
        }
    }
}

/// Load balancing strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoadBalanceStrategy {
    /// Static assignment (no rebalancing)
    Static,

    /// Round-robin assignment
    RoundRobin,

    /// Least loaded worker
    LeastLoaded,

    /// Adaptive based on worker metrics
    Adaptive,
}

/// Task with priority for priority queue.
#[derive(Debug, Clone)]
struct PriorityTask {
    task_id: TaskId,
    priority: i32,
    queue_time: Instant,
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
        // Higher priority first, then older tasks
        self.priority
            .cmp(&other.priority)
            .then_with(|| other.queue_time.cmp(&self.queue_time))
    }
}

/// Scheduler statistics.
#[derive(Debug, Default)]
struct SchedulerStatistics {
    /// Tasks scheduled
    tasks_scheduled: AtomicU64,

    /// Tasks completed
    tasks_completed: AtomicU64,

    /// Tasks failed
    tasks_failed: AtomicU64,

    /// Work steals performed
    work_steals: AtomicU64,

    /// Load balancing operations
    load_balances: AtomicU64,

    /// Backpressure activations
    backpressure_activations: AtomicU64,
}

/// Task execution request.
#[derive(Debug, Clone)]
pub struct TaskExecution {
    /// Task ID
    pub task_id: TaskId,

    /// Worker ID
    pub worker_id: WorkerId,

    /// Scheduled time
    pub scheduled_at: Instant,

    /// Execution timeout
    pub timeout: Duration,
}

impl Scheduler {
    /// Create a new scheduler.
    pub fn new(
        task_graph: Arc<TaskGraph>,
        worker_pool: Arc<WorkerPool>,
        metrics: Arc<ClusterMetrics>,
        config: SchedulerConfig,
    ) -> Self {
        let backpressure_permits = if config.enable_backpressure {
            config.max_queue_size
        } else {
            usize::MAX
        };

        Self {
            inner: Arc::new(SchedulerInner {
                task_graph,
                worker_pool,
                metrics,
                global_queue: Arc::new(RwLock::new(BinaryHeap::new())),
                worker_queues: Arc::new(DashMap::new()),
                task_assignments: Arc::new(DashMap::new()),
                worker_task_counts: Arc::new(DashMap::new()),
                config,
                running: AtomicBool::new(false),
                backpressure: Arc::new(Semaphore::new(backpressure_permits)),
                schedule_notify: Arc::new(Notify::new()),
                stats: Arc::new(SchedulerStatistics::default()),
            }),
        }
    }

    /// Create with default configuration.
    pub fn with_defaults(
        task_graph: Arc<TaskGraph>,
        worker_pool: Arc<WorkerPool>,
        metrics: Arc<ClusterMetrics>,
    ) -> Self {
        Self::new(task_graph, worker_pool, metrics, SchedulerConfig::default())
    }

    /// Start the scheduler.
    pub async fn start(&self) -> Result<()> {
        if self.inner.running.swap(true, AtomicOrdering::SeqCst) {
            return Err(ClusterError::InvalidState(
                "Scheduler already running".to_string(),
            ));
        }

        info!("Starting distributed scheduler");

        // Spawn scheduler loop
        let scheduler = self.clone();
        tokio::spawn(async move {
            scheduler.run_scheduler_loop().await;
        });

        Ok(())
    }

    /// Stop the scheduler.
    pub async fn stop(&self) -> Result<()> {
        info!("Stopping scheduler");
        self.inner.running.store(false, AtomicOrdering::SeqCst);
        self.inner.schedule_notify.notify_waiters();
        Ok(())
    }

    /// Submit a task for scheduling.
    pub async fn submit_task(&self, task: Task) -> Result<TaskId> {
        let task_id = task.id;
        let task_type = task.task_type.clone();

        // Acquire backpressure permit
        if self.inner.config.enable_backpressure {
            let _permit =
                self.inner.backpressure.acquire().await.map_err(|e| {
                    ClusterError::SchedulerError(format!("Backpressure error: {}", e))
                })?;

            self.inner
                .stats
                .backpressure_activations
                .fetch_add(1, AtomicOrdering::Relaxed);
        }

        // Add to task graph
        self.inner.task_graph.add_task(task.clone())?;

        // Record submission
        self.inner.metrics.record_task_submitted(&task_type);

        // Add to queue if ready
        if task.status == TaskStatus::Ready {
            self.enqueue_task(task_id, task.priority)?;
        }

        // Notify scheduler
        self.inner.schedule_notify.notify_one();

        debug!("Task {} submitted", task_id);

        Ok(task_id)
    }

    /// Enqueue a task in the priority queue.
    fn enqueue_task(&self, task_id: TaskId, priority: i32) -> Result<()> {
        let priority_task = PriorityTask {
            task_id,
            priority,
            queue_time: Instant::now(),
        };

        self.inner.global_queue.write().push(priority_task);

        // Update queue depth metric
        let queue_depth = self.inner.global_queue.read().len();
        self.inner.metrics.update_queue_depth(queue_depth);

        Ok(())
    }

    /// Main scheduler loop.
    async fn run_scheduler_loop(&self) {
        let mut interval = tokio::time::interval(self.inner.config.scheduling_interval);

        while self.inner.running.load(AtomicOrdering::SeqCst) {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(e) = self.schedule_ready_tasks().await {
                        error!("Scheduling error: {}", e);
                    }

                    if self.inner.config.enable_work_stealing {
                        if let Err(e) = self.perform_work_stealing().await {
                            error!("Work stealing error: {}", e);
                        }
                    }
                }
                _ = self.inner.schedule_notify.notified() => {
                    if let Err(e) = self.schedule_ready_tasks().await {
                        error!("Scheduling error: {}", e);
                    }
                }
            }
        }

        info!("Scheduler loop stopped");
    }

    /// Schedule ready tasks to workers.
    async fn schedule_ready_tasks(&self) -> Result<()> {
        let _scheduling_start = Instant::now();

        loop {
            let priority_task = {
                let mut queue = self.inner.global_queue.write();
                match queue.pop() {
                    Some(task) => task,
                    None => break,
                }
            };
            let task_id = priority_task.task_id;

            // Get task from graph
            let task = match self.inner.task_graph.get_task(task_id) {
                Ok(task) => task,
                Err(_) => continue, // Task may have been cancelled
            };

            // Extract needed data while holding lock, then drop immediately
            let (task_status, locality_hints, resources, task_id_str, task_name) = {
                let task = task.read();
                (
                    task.status,
                    task.locality_hints.clone(),
                    task.resources.clone(),
                    task.id,
                    task.name.clone(),
                )
            }; // Lock is dropped here

            // Check if task is still ready
            if task_status != TaskStatus::Ready {
                continue;
            }

            // Create temporary task view for selection (no lock held)
            let task_view = Task {
                id: task_id_str,
                name: task_name,
                task_type: String::new(),
                priority: 0,
                payload: Vec::new(),
                dependencies: Vec::new(),
                estimated_duration: None,
                resources,
                locality_hints,
                created_at: Instant::now(),
                scheduled_at: None,
                started_at: None,
                completed_at: None,
                status: task_status,
                result: None,
                error: None,
                retry_count: 0,
                checkpoint: None,
            };

            // Select worker based on strategy
            let worker_id = match self.select_worker_for_task(&task_view).await {
                Ok(id) => id,
                Err(e) => {
                    warn!("Failed to select worker for task {}: {}", task_id, e);
                    // Re-enqueue task
                    self.enqueue_task(task_id, priority_task.priority)?;
                    break;
                }
            };

            // Assign task to worker
            self.assign_task_to_worker(task_id, worker_id).await?;

            // Record scheduling latency
            let scheduling_latency = priority_task.queue_time.elapsed();
            self.inner
                .metrics
                .record_scheduling_latency(scheduling_latency);

            self.inner
                .stats
                .tasks_scheduled
                .fetch_add(1, AtomicOrdering::Relaxed);
        }

        // Update queue depth
        let queue_depth = self.inner.global_queue.read().len();
        self.inner.metrics.update_queue_depth(queue_depth);

        Ok(())
    }

    /// Select a worker for a task.
    async fn select_worker_for_task(&self, task: &Task) -> Result<WorkerId> {
        let strategy = match self.inner.config.load_balance_strategy {
            LoadBalanceStrategy::Static => SelectionStrategy::Random,
            LoadBalanceStrategy::RoundRobin => SelectionStrategy::RoundRobin,
            LoadBalanceStrategy::LeastLoaded => SelectionStrategy::LeastLoaded,
            LoadBalanceStrategy::Adaptive => {
                // Use task characteristics to select strategy
                if !task.locality_hints.is_empty() {
                    // Try to use locality hints first
                    return self.select_worker_with_locality(task);
                }
                SelectionStrategy::LeastLoaded
            }
        };

        self.inner
            .worker_pool
            .select_worker(&task.resources, strategy)
    }

    /// Select worker with data locality preference.
    fn select_worker_with_locality(&self, task: &Task) -> Result<WorkerId> {
        // Try to find a worker from locality hints that's available
        for hint in &task.locality_hints {
            if let Ok(uuid) = hint.parse::<uuid::Uuid>() {
                let worker_id = WorkerId::from_uuid(uuid);
                if self.inner.worker_pool.get_worker(worker_id).is_ok() {
                    // Check if worker has capacity
                    let worker = self.inner.worker_pool.get_worker(worker_id)?;
                    let worker = worker.read();

                    let available_cpu = worker.capacity.cpu_cores - worker.usage.cpu_cores;
                    let available_memory = worker.capacity.memory_bytes - worker.usage.memory_bytes;

                    if available_cpu >= task.resources.cpu_cores
                        && available_memory >= task.resources.memory_bytes
                    {
                        return Ok(worker_id);
                    }
                }
            }
        }

        // Fallback to least loaded
        self.inner
            .worker_pool
            .select_worker(&task.resources, SelectionStrategy::LeastLoaded)
    }

    /// Assign a task to a worker.
    async fn assign_task_to_worker(&self, task_id: TaskId, worker_id: WorkerId) -> Result<()> {
        // Update task status
        self.inner
            .task_graph
            .update_task_status(task_id, TaskStatus::Scheduled)?;

        // Record assignment
        self.inner.task_assignments.insert(task_id, worker_id);

        // Add to worker queue
        let queue = self
            .inner
            .worker_queues
            .entry(worker_id)
            .or_insert_with(|| Arc::new(RwLock::new(VecDeque::new())));

        queue.write().push_back(task_id);

        // Update worker task count
        self.inner
            .worker_task_counts
            .entry(worker_id)
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, AtomicOrdering::Relaxed);

        debug!("Assigned task {} to worker {}", task_id, worker_id);

        Ok(())
    }

    /// Perform work stealing to balance load.
    async fn perform_work_stealing(&self) -> Result<()> {
        // Find overloaded and underloaded workers
        let mut overloaded = Vec::new();
        let mut underloaded = Vec::new();

        for entry in self.inner.worker_task_counts.iter() {
            let worker_id = *entry.key();
            let task_count = entry.value().load(AtomicOrdering::Relaxed);

            if task_count > self.inner.config.work_steal_threshold as u64 {
                overloaded.push((worker_id, task_count));
            } else if task_count < (self.inner.config.work_steal_threshold / 2) as u64 {
                underloaded.push(worker_id);
            }
        }

        if overloaded.is_empty() || underloaded.is_empty() {
            return Ok(());
        }

        // Sort overloaded workers by task count (descending)
        overloaded.sort_by_key(|x| std::cmp::Reverse(x.1));

        // Steal tasks from overloaded to underloaded
        for (victim_id, _) in overloaded {
            if underloaded.is_empty() {
                break;
            }

            let thief_id = underloaded[0];

            // Try to steal a task
            if let Some(queue) = self.inner.worker_queues.get(&victim_id) {
                let stolen_task = queue.write().pop_front();

                if let Some(task_id) = stolen_task {
                    // Move task to thief's queue
                    let thief_queue = self
                        .inner
                        .worker_queues
                        .entry(thief_id)
                        .or_insert_with(|| Arc::new(RwLock::new(VecDeque::new())));

                    thief_queue.write().push_back(task_id);

                    // Update assignments
                    self.inner.task_assignments.insert(task_id, thief_id);

                    // Update task counts
                    self.inner
                        .worker_task_counts
                        .get(&victim_id)
                        .map(|c| c.fetch_sub(1, AtomicOrdering::Relaxed));

                    self.inner
                        .worker_task_counts
                        .entry(thief_id)
                        .or_insert_with(|| AtomicU64::new(0))
                        .fetch_add(1, AtomicOrdering::Relaxed);

                    self.inner
                        .stats
                        .work_steals
                        .fetch_add(1, AtomicOrdering::Relaxed);

                    debug!(
                        "Stole task {} from worker {} to worker {}",
                        task_id, victim_id, thief_id
                    );

                    // Remove thief from underloaded if it has enough tasks now
                    let thief_count = self
                        .inner
                        .worker_task_counts
                        .get(&thief_id)
                        .map(|c| c.load(AtomicOrdering::Relaxed))
                        .unwrap_or(0);

                    if thief_count >= (self.inner.config.work_steal_threshold / 2) as u64 {
                        underloaded.remove(0);
                    }
                }
            }
        }

        Ok(())
    }

    /// Mark a task as completed.
    pub async fn complete_task(&self, task_id: TaskId) -> Result<()> {
        // Update task status
        self.inner
            .task_graph
            .update_task_status(task_id, TaskStatus::Completed)?;

        // Remove from worker queue and assignment
        if let Some((_, worker_id)) = self.inner.task_assignments.remove(&task_id) {
            self.inner
                .worker_task_counts
                .get(&worker_id)
                .map(|c| c.fetch_sub(1, AtomicOrdering::Relaxed));
        }

        // Release backpressure permit
        if self.inner.config.enable_backpressure {
            self.inner.backpressure.add_permits(1);
        }

        self.inner
            .stats
            .tasks_completed
            .fetch_add(1, AtomicOrdering::Relaxed);

        debug!("Task {} completed", task_id);

        Ok(())
    }

    /// Mark a task as failed.
    pub async fn fail_task(&self, task_id: TaskId, error: String) -> Result<()> {
        // Update task status
        self.inner.task_graph.set_task_error(task_id, error)?;

        // Remove from worker queue and assignment
        if let Some((_, worker_id)) = self.inner.task_assignments.remove(&task_id) {
            self.inner
                .worker_task_counts
                .get(&worker_id)
                .map(|c| c.fetch_sub(1, AtomicOrdering::Relaxed));
        }

        // Release backpressure permit
        if self.inner.config.enable_backpressure {
            self.inner.backpressure.add_permits(1);
        }

        self.inner
            .stats
            .tasks_failed
            .fetch_add(1, AtomicOrdering::Relaxed);

        warn!("Task {} failed", task_id);

        Ok(())
    }

    /// Cancel a task.
    pub async fn cancel_task(&self, task_id: TaskId) -> Result<()> {
        // Update task status
        self.inner
            .task_graph
            .update_task_status(task_id, TaskStatus::Cancelled)?;

        // Remove from queues
        if let Some((_, worker_id)) = self.inner.task_assignments.remove(&task_id) {
            if let Some(queue) = self.inner.worker_queues.get(&worker_id) {
                queue.write().retain(|&id| id != task_id);
            }

            self.inner
                .worker_task_counts
                .get(&worker_id)
                .map(|c| c.fetch_sub(1, AtomicOrdering::Relaxed));
        }

        // Release backpressure permit
        if self.inner.config.enable_backpressure {
            self.inner.backpressure.add_permits(1);
        }

        debug!("Task {} cancelled", task_id);

        Ok(())
    }

    /// Get scheduler statistics.
    pub fn get_statistics(&self) -> SchedulerStats {
        SchedulerStats {
            tasks_scheduled: self
                .inner
                .stats
                .tasks_scheduled
                .load(AtomicOrdering::Relaxed),
            tasks_completed: self
                .inner
                .stats
                .tasks_completed
                .load(AtomicOrdering::Relaxed),
            tasks_failed: self.inner.stats.tasks_failed.load(AtomicOrdering::Relaxed),
            work_steals: self.inner.stats.work_steals.load(AtomicOrdering::Relaxed),
            load_balances: self.inner.stats.load_balances.load(AtomicOrdering::Relaxed),
            backpressure_activations: self
                .inner
                .stats
                .backpressure_activations
                .load(AtomicOrdering::Relaxed),
            queue_depth: self.inner.global_queue.read().len(),
            worker_queues: self.inner.worker_queues.len(),
        }
    }
}

/// Scheduler statistics snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerStats {
    /// Tasks scheduled
    pub tasks_scheduled: u64,

    /// Tasks completed
    pub tasks_completed: u64,

    /// Tasks failed
    pub tasks_failed: u64,

    /// Work steals performed
    pub work_steals: u64,

    /// Load balancing operations
    pub load_balances: u64,

    /// Backpressure activations
    pub backpressure_activations: u64,

    /// Current queue depth
    pub queue_depth: usize,

    /// Number of worker queues
    pub worker_queues: usize,
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::task_graph::{ResourceRequirements, Task};
    use crate::worker_pool::{
        Worker, WorkerCapabilities, WorkerCapacity, WorkerStatus, WorkerUsage,
    };
    use std::collections::HashMap;

    fn create_test_task(name: &str) -> Task {
        Task {
            id: TaskId::new(),
            name: name.to_string(),
            task_type: "test".to_string(),
            priority: 0,
            payload: vec![],
            dependencies: vec![],
            estimated_duration: Some(Duration::from_secs(1)),
            resources: ResourceRequirements::default(),
            locality_hints: vec![],
            created_at: Instant::now(),
            scheduled_at: None,
            started_at: None,
            completed_at: None,
            status: TaskStatus::Ready,
            result: None,
            error: None,
            retry_count: 0,
            checkpoint: None,
        }
    }

    fn create_test_worker() -> Worker {
        Worker {
            id: WorkerId::new(),
            name: "test_worker".to_string(),
            address: "localhost:8080".to_string(),
            capabilities: WorkerCapabilities::default(),
            capacity: WorkerCapacity {
                cpu_cores: 8.0,
                memory_bytes: 16_000_000_000,
                storage_bytes: 0,
                gpu_count: 0,
                network_bandwidth: 0,
            },
            usage: WorkerUsage::default(),
            status: WorkerStatus::Active,
            last_heartbeat: Instant::now(),
            registered_at: Instant::now(),
            last_health_check: None,
            health_check_failures: 0,
            tasks_completed: 0,
            tasks_failed: 0,
            version: "1.0.0".to_string(),
            metadata: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn test_scheduler_creation() {
        let task_graph = Arc::new(TaskGraph::new());
        let worker_pool = Arc::new(WorkerPool::with_defaults());
        let metrics = Arc::new(ClusterMetrics::new());

        let scheduler = Scheduler::with_defaults(task_graph, worker_pool, metrics);
        let stats = scheduler.get_statistics();

        assert_eq!(stats.tasks_scheduled, 0);
    }

    #[tokio::test]
    async fn test_submit_task() {
        let task_graph = Arc::new(TaskGraph::new());
        let worker_pool = Arc::new(WorkerPool::with_defaults());
        let metrics = Arc::new(ClusterMetrics::new());

        // Register a worker
        let worker = create_test_worker();
        worker_pool.register_worker(worker).ok();

        let scheduler = Scheduler::with_defaults(task_graph, worker_pool, metrics);

        let task = create_test_task("test_task");
        let result = scheduler.submit_task(task).await;

        assert!(result.is_ok());
    }
}
