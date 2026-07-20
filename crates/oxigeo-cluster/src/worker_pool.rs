//! Worker pool management for the cluster.
//!
//! This module manages worker nodes including registration, heartbeat monitoring,
//! capacity tracking, health checks, automatic failover, and worker pools by capability.

use crate::error::{ClusterError, Result};
use crate::metrics::WorkerMetrics;
use crate::task_graph::ResourceRequirements;
use chrono::Utc;
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Worker pool manager.
#[derive(Clone)]
pub struct WorkerPool {
    inner: Arc<WorkerPoolInner>,
}

struct WorkerPoolInner {
    /// All registered workers
    workers: DashMap<WorkerId, Arc<RwLock<Worker>>>,

    /// Worker capabilities index
    cpu_workers: RwLock<HashSet<WorkerId>>,
    gpu_workers: RwLock<HashSet<WorkerId>>,
    storage_workers: RwLock<HashSet<WorkerId>>,

    /// Configuration
    config: WorkerPoolConfig,
}

/// Worker pool configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerPoolConfig {
    /// Heartbeat timeout duration
    pub heartbeat_timeout: Duration,

    /// Health check interval
    pub health_check_interval: Duration,

    /// Maximum unhealthy duration before removal
    pub max_unhealthy_duration: Duration,

    /// Minimum workers required
    pub min_workers: usize,

    /// Maximum workers allowed
    pub max_workers: usize,
}

impl Default for WorkerPoolConfig {
    fn default() -> Self {
        Self {
            heartbeat_timeout: Duration::from_secs(30),
            health_check_interval: Duration::from_secs(10),
            max_unhealthy_duration: Duration::from_secs(120),
            min_workers: 1,
            max_workers: 1000,
        }
    }
}

/// Worker identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkerId(pub Uuid);

impl WorkerId {
    /// Create a new random worker ID.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create from UUID.
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl Default for WorkerId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for WorkerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Worker node information.
#[derive(Debug, Clone)]
pub struct Worker {
    /// Worker ID
    pub id: WorkerId,

    /// Worker name/hostname
    pub name: String,

    /// Network address
    pub address: String,

    /// Worker capabilities
    pub capabilities: WorkerCapabilities,

    /// Worker capacity
    pub capacity: WorkerCapacity,

    /// Current resource usage
    pub usage: WorkerUsage,

    /// Worker status
    pub status: WorkerStatus,

    /// Last heartbeat time
    pub last_heartbeat: Instant,

    /// Registration time
    pub registered_at: Instant,

    /// Last health check
    pub last_health_check: Option<Instant>,

    /// Health check failures
    pub health_check_failures: u32,

    /// Total tasks completed
    pub tasks_completed: u64,

    /// Total tasks failed
    pub tasks_failed: u64,

    /// Worker version
    pub version: String,

    /// Custom metadata
    pub metadata: HashMap<String, String>,
}

/// Worker capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerCapabilities {
    /// Has CPU processing
    pub cpu: bool,

    /// Has GPU processing
    pub gpu: bool,

    /// Has large storage
    pub storage: bool,

    /// Supported task types
    pub task_types: Vec<String>,

    /// Supported data formats
    pub data_formats: Vec<String>,
}

impl Default for WorkerCapabilities {
    fn default() -> Self {
        Self {
            cpu: true,
            gpu: false,
            storage: false,
            task_types: vec![],
            data_formats: vec![],
        }
    }
}

/// Worker capacity (total resources).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerCapacity {
    /// Total CPU cores
    pub cpu_cores: f64,

    /// Total memory (bytes)
    pub memory_bytes: u64,

    /// Total storage (bytes)
    pub storage_bytes: u64,

    /// Number of GPUs
    pub gpu_count: u32,

    /// Network bandwidth (bytes/sec)
    pub network_bandwidth: u64,
}

impl Default for WorkerCapacity {
    fn default() -> Self {
        Self {
            cpu_cores: 1.0,
            memory_bytes: 1024 * 1024 * 1024, // 1 GB
            storage_bytes: 0,
            gpu_count: 0,
            network_bandwidth: 0,
        }
    }
}

/// Worker resource usage (current).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerUsage {
    /// Used CPU cores
    pub cpu_cores: f64,

    /// Used memory (bytes)
    pub memory_bytes: u64,

    /// Used storage (bytes)
    pub storage_bytes: u64,

    /// Active tasks
    pub active_tasks: u32,

    /// Network sent (bytes)
    pub network_sent: u64,

    /// Network received (bytes)
    pub network_received: u64,
}

impl Default for WorkerUsage {
    fn default() -> Self {
        Self {
            cpu_cores: 0.0,
            memory_bytes: 0,
            storage_bytes: 0,
            active_tasks: 0,
            network_sent: 0,
            network_received: 0,
        }
    }
}

/// Worker status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WorkerStatus {
    /// Worker is active and healthy
    Active,

    /// Worker is idle (no tasks)
    Idle,

    /// Worker is busy (at capacity)
    Busy,

    /// Worker is unhealthy
    Unhealthy,

    /// Worker is draining (no new tasks)
    Draining,

    /// Worker is offline
    Offline,
}

/// Worker selection strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionStrategy {
    /// Select least loaded worker
    LeastLoaded,

    /// Select worker with most available resources
    MostAvailable,

    /// Round-robin selection
    RoundRobin,

    /// Random selection
    Random,
}

impl WorkerPool {
    /// Create a new worker pool.
    pub fn new(config: WorkerPoolConfig) -> Self {
        Self {
            inner: Arc::new(WorkerPoolInner {
                workers: DashMap::new(),
                cpu_workers: RwLock::new(HashSet::new()),
                gpu_workers: RwLock::new(HashSet::new()),
                storage_workers: RwLock::new(HashSet::new()),
                config,
            }),
        }
    }

    /// Create with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(WorkerPoolConfig::default())
    }

    /// Register a new worker.
    pub fn register_worker(&self, worker: Worker) -> Result<WorkerId> {
        let worker_id = worker.id;

        // Check if we're at capacity
        if self.inner.workers.len() >= self.inner.config.max_workers {
            return Err(ClusterError::CapacityExceeded(
                "Worker pool at maximum capacity".to_string(),
            ));
        }

        // Update capability indices
        if worker.capabilities.cpu {
            self.inner.cpu_workers.write().insert(worker_id);
        }
        if worker.capabilities.gpu {
            self.inner.gpu_workers.write().insert(worker_id);
        }
        if worker.capabilities.storage {
            self.inner.storage_workers.write().insert(worker_id);
        }

        // Store worker
        self.inner
            .workers
            .insert(worker_id, Arc::new(RwLock::new(worker)));

        Ok(worker_id)
    }

    /// Unregister a worker.
    pub fn unregister_worker(&self, worker_id: WorkerId) -> Result<()> {
        // Remove from capability indices
        self.inner.cpu_workers.write().remove(&worker_id);
        self.inner.gpu_workers.write().remove(&worker_id);
        self.inner.storage_workers.write().remove(&worker_id);

        // Remove worker
        self.inner.workers.remove(&worker_id);

        Ok(())
    }

    /// Get a worker by ID.
    pub fn get_worker(&self, worker_id: WorkerId) -> Result<Arc<RwLock<Worker>>> {
        self.inner
            .workers
            .get(&worker_id)
            .map(|entry| Arc::clone(entry.value()))
            .ok_or_else(|| ClusterError::WorkerNotFound(worker_id.to_string()))
    }

    /// Get all workers.
    pub fn get_all_workers(&self) -> Vec<Arc<RwLock<Worker>>> {
        self.inner
            .workers
            .iter()
            .map(|entry| Arc::clone(entry.value()))
            .collect()
    }

    /// Get workers by status.
    pub fn get_workers_by_status(&self, status: WorkerStatus) -> Vec<Arc<RwLock<Worker>>> {
        self.inner
            .workers
            .iter()
            .filter(|entry| entry.value().read().status == status)
            .map(|entry| Arc::clone(entry.value()))
            .collect()
    }

    /// Update worker heartbeat.
    pub fn heartbeat(&self, worker_id: WorkerId) -> Result<()> {
        let worker = self.get_worker(worker_id)?;
        let mut worker = worker.write();

        worker.last_heartbeat = Instant::now();

        // If worker was unhealthy, mark as active
        if worker.status == WorkerStatus::Unhealthy {
            worker.status = WorkerStatus::Active;
            worker.health_check_failures = 0;
        }

        Ok(())
    }

    /// Update worker resource usage.
    pub fn update_worker_usage(&self, worker_id: WorkerId, usage: WorkerUsage) -> Result<()> {
        let worker = self.get_worker(worker_id)?;
        let mut worker = worker.write();

        // Calculate utilizationsbefore moving usage
        let cpu_utilization = usage.cpu_cores / worker.capacity.cpu_cores;
        let memory_utilization = usage.memory_bytes as f64 / worker.capacity.memory_bytes as f64;
        let active_tasks = usage.active_tasks;

        worker.usage = usage;

        // Update status based on usage
        if active_tasks == 0 {
            worker.status = WorkerStatus::Idle;
        } else {
            if cpu_utilization >= 0.9 || memory_utilization >= 0.9 {
                worker.status = WorkerStatus::Busy;
            } else {
                worker.status = WorkerStatus::Active;
            }
        }

        Ok(())
    }

    /// Check worker health.
    pub fn check_worker_health(&self, worker_id: WorkerId) -> Result<bool> {
        let worker = self.get_worker(worker_id)?;
        let mut worker = worker.write();

        let now = Instant::now();
        worker.last_health_check = Some(now);

        // Check heartbeat timeout
        let heartbeat_age = now.duration_since(worker.last_heartbeat);
        if heartbeat_age > self.inner.config.heartbeat_timeout {
            worker.health_check_failures += 1;
            worker.status = WorkerStatus::Unhealthy;

            // Check if worker should be removed
            if heartbeat_age > self.inner.config.max_unhealthy_duration {
                worker.status = WorkerStatus::Offline;
                return Ok(false);
            }
        } else {
            worker.health_check_failures = 0;
            if worker.status == WorkerStatus::Unhealthy {
                worker.status = WorkerStatus::Active;
            }
        }

        Ok(worker.status != WorkerStatus::Offline)
    }

    /// Run health checks on all workers.
    pub fn check_all_workers(&self) -> Result<Vec<WorkerId>> {
        let mut failed_workers = Vec::new();

        for entry in self.inner.workers.iter() {
            let worker_id = *entry.key();
            let is_healthy = self.check_worker_health(worker_id)?;

            if !is_healthy {
                failed_workers.push(worker_id);
            }
        }

        // Remove offline workers
        for worker_id in &failed_workers {
            self.unregister_worker(*worker_id)?;
        }

        Ok(failed_workers)
    }

    /// Select a worker for task execution.
    pub fn select_worker(
        &self,
        requirements: &ResourceRequirements,
        strategy: SelectionStrategy,
    ) -> Result<WorkerId> {
        // Get candidate workers based on requirements
        let candidates = self.get_candidate_workers(requirements)?;

        if candidates.is_empty() {
            return Err(ClusterError::WorkerPoolError(
                "No available workers matching requirements".to_string(),
            ));
        }

        // Select worker based on strategy
        let selected = match strategy {
            SelectionStrategy::LeastLoaded => self.select_least_loaded(&candidates)?,
            SelectionStrategy::MostAvailable => self.select_most_available(&candidates)?,
            SelectionStrategy::RoundRobin => self.select_round_robin(&candidates)?,
            SelectionStrategy::Random => self.select_random(&candidates)?,
        };

        Ok(selected)
    }

    /// Get candidate workers matching requirements.
    fn get_candidate_workers(&self, requirements: &ResourceRequirements) -> Result<Vec<WorkerId>> {
        let mut candidates = Vec::new();

        // Filter by capability
        let capability_workers = if requirements.gpu {
            self.inner.gpu_workers.read().clone()
        } else {
            self.inner.cpu_workers.read().clone()
        };

        for worker_id in capability_workers {
            if let Ok(worker) = self.get_worker(worker_id) {
                let worker = worker.read();

                // Check status
                if !matches!(worker.status, WorkerStatus::Active | WorkerStatus::Idle) {
                    continue;
                }

                // Check resource availability
                let available_cpu = worker.capacity.cpu_cores - worker.usage.cpu_cores;
                let available_memory = worker.capacity.memory_bytes - worker.usage.memory_bytes;

                if available_cpu >= requirements.cpu_cores
                    && available_memory >= requirements.memory_bytes
                {
                    candidates.push(worker_id);
                }
            }
        }

        Ok(candidates)
    }

    /// Select least loaded worker.
    fn select_least_loaded(&self, candidates: &[WorkerId]) -> Result<WorkerId> {
        candidates
            .iter()
            .min_by_key(|worker_id| {
                self.get_worker(**worker_id)
                    .map(|w| w.read().usage.active_tasks)
                    .unwrap_or(u32::MAX)
            })
            .copied()
            .ok_or_else(|| ClusterError::WorkerPoolError("No workers available".to_string()))
    }

    /// Select worker with most available resources.
    fn select_most_available(&self, candidates: &[WorkerId]) -> Result<WorkerId> {
        candidates
            .iter()
            .max_by_key(|worker_id| {
                self.get_worker(**worker_id)
                    .map(|w| {
                        let worker = w.read();
                        let available_cpu = worker.capacity.cpu_cores - worker.usage.cpu_cores;
                        let available_memory =
                            worker.capacity.memory_bytes - worker.usage.memory_bytes;
                        (available_cpu * 1000.0) as u64 + available_memory / 1_000_000
                    })
                    .unwrap_or(0)
            })
            .copied()
            .ok_or_else(|| ClusterError::WorkerPoolError("No workers available".to_string()))
    }

    /// Select worker using round-robin.
    fn select_round_robin(&self, candidates: &[WorkerId]) -> Result<WorkerId> {
        // Simple round-robin: select first candidate
        // In production, maintain a counter for true round-robin
        candidates
            .first()
            .copied()
            .ok_or_else(|| ClusterError::WorkerPoolError("No workers available".to_string()))
    }

    /// Select random worker.
    fn select_random(&self, candidates: &[WorkerId]) -> Result<WorkerId> {
        use std::collections::hash_map::RandomState;
        use std::hash::BuildHasher;

        let state = RandomState::new();
        let index = (state.hash_one(Instant::now()) as usize) % candidates.len();

        candidates
            .get(index)
            .copied()
            .ok_or_else(|| ClusterError::WorkerPoolError("No workers available".to_string()))
    }

    /// Get worker metrics.
    pub fn get_worker_metrics(&self, worker_id: WorkerId) -> Result<WorkerMetrics> {
        let worker = self.get_worker(worker_id)?;
        let worker = worker.read();

        let cpu_utilization = worker.usage.cpu_cores / worker.capacity.cpu_cores;
        let memory_utilization =
            worker.usage.memory_bytes as f64 / worker.capacity.memory_bytes as f64;

        let uptime = worker.registered_at.elapsed();

        Ok(WorkerMetrics {
            worker_id: worker_id.to_string(),
            tasks_completed: worker.tasks_completed,
            tasks_failed: worker.tasks_failed,
            cpu_utilization,
            memory_utilization,
            network_sent: worker.usage.network_sent,
            network_received: worker.usage.network_received,
            last_heartbeat: Utc::now(),
            uptime,
        })
    }

    /// Get pool statistics.
    pub fn get_statistics(&self) -> WorkerPoolStatistics {
        let total_workers = self.inner.workers.len();
        let mut status_counts = HashMap::new();

        let mut total_capacity = WorkerCapacity::default();
        let mut total_usage = WorkerUsage::default();

        for entry in self.inner.workers.iter() {
            let worker = entry.value().read();

            *status_counts.entry(worker.status).or_insert(0) += 1;

            total_capacity.cpu_cores += worker.capacity.cpu_cores;
            total_capacity.memory_bytes += worker.capacity.memory_bytes;
            total_capacity.storage_bytes += worker.capacity.storage_bytes;
            total_capacity.gpu_count += worker.capacity.gpu_count;

            total_usage.cpu_cores += worker.usage.cpu_cores;
            total_usage.memory_bytes += worker.usage.memory_bytes;
            total_usage.storage_bytes += worker.usage.storage_bytes;
            total_usage.active_tasks += worker.usage.active_tasks;
        }

        WorkerPoolStatistics {
            total_workers,
            status_counts,
            total_capacity,
            total_usage,
            cpu_workers: self.inner.cpu_workers.read().len(),
            gpu_workers: self.inner.gpu_workers.read().len(),
            storage_workers: self.inner.storage_workers.read().len(),
        }
    }

    /// Drain a worker (no new tasks).
    pub fn drain_worker(&self, worker_id: WorkerId) -> Result<()> {
        let worker = self.get_worker(worker_id)?;
        let mut worker = worker.write();

        worker.status = WorkerStatus::Draining;

        Ok(())
    }

    /// Resume a drained worker.
    pub fn resume_worker(&self, worker_id: WorkerId) -> Result<()> {
        let worker = self.get_worker(worker_id)?;
        let mut worker = worker.write();

        if worker.status == WorkerStatus::Draining {
            worker.status = WorkerStatus::Active;
        }

        Ok(())
    }

    /// Get the current number of workers in the pool.
    pub fn get_worker_count(&self) -> usize {
        self.inner.workers.len()
    }
}

/// Worker pool statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerPoolStatistics {
    /// Total number of workers
    pub total_workers: usize,

    /// Worker counts by status
    pub status_counts: HashMap<WorkerStatus, usize>,

    /// Total capacity across all workers
    pub total_capacity: WorkerCapacity,

    /// Total usage across all workers
    pub total_usage: WorkerUsage,

    /// Number of CPU workers
    pub cpu_workers: usize,

    /// Number of GPU workers
    pub gpu_workers: usize,

    /// Number of storage workers
    pub storage_workers: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_worker(name: &str) -> Worker {
        Worker {
            id: WorkerId::new(),
            name: name.to_string(),
            address: "localhost:8080".to_string(),
            capabilities: WorkerCapabilities::default(),
            capacity: WorkerCapacity::default(),
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

    #[test]
    fn test_worker_pool_creation() {
        let pool = WorkerPool::with_defaults();
        let stats = pool.get_statistics();
        assert_eq!(stats.total_workers, 0);
    }

    #[test]
    fn test_register_worker() {
        let pool = WorkerPool::with_defaults();
        let worker = create_test_worker("worker1");

        let result = pool.register_worker(worker);
        assert!(result.is_ok());

        let stats = pool.get_statistics();
        assert_eq!(stats.total_workers, 1);
    }

    #[test]
    fn test_heartbeat() {
        let pool = WorkerPool::with_defaults();
        let worker = create_test_worker("worker1");
        let worker_id = pool.register_worker(worker).ok().unwrap_or_default();

        let result = pool.heartbeat(worker_id);
        assert!(result.is_ok());
    }

    #[test]
    fn test_worker_selection() {
        let pool = WorkerPool::with_defaults();

        let mut worker = create_test_worker("worker1");
        worker.capacity.cpu_cores = 8.0;
        worker.capacity.memory_bytes = 16_000_000_000;

        pool.register_worker(worker).ok();

        let requirements = ResourceRequirements {
            cpu_cores: 2.0,
            memory_bytes: 4_000_000_000,
            gpu: false,
            storage_bytes: 0,
        };

        let result = pool.select_worker(&requirements, SelectionStrategy::LeastLoaded);
        assert!(result.is_ok());
    }
}
