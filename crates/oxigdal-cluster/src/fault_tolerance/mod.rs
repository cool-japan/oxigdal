//! Advanced fault tolerance system for handling failures and recovery.
//!
//! This module implements comprehensive fault tolerance patterns including:
//! - **Circuit Breaker**: Prevent cascading failures by failing fast
//! - **Retry with Exponential Backoff**: Automatic retry with configurable backoff
//! - **Bulkhead Isolation**: Isolate components to prevent failure propagation
//! - **Timeout Management**: Configurable and adaptive timeouts
//! - **Health Checks**: Liveness and readiness checks with dependency health
//! - **Graceful Degradation**: Load shedding and feature flags for degraded operation
//! - **Task Retry**: Speculative execution and checkpointing for tasks
//!
//! # Example
//!
//! ```rust,ignore
//! use oxigdal_cluster::fault_tolerance::{
//!     CircuitBreaker, CircuitBreakerConfig,
//!     Bulkhead, BulkheadConfig,
//!     TimeoutManager, TimeoutConfig,
//!     HealthCheckManager,
//!     DegradationManager,
//! };
//!
//! // Create a circuit breaker
//! let circuit_breaker = CircuitBreaker::with_defaults();
//!
//! // Execute with circuit breaker protection
//! let result = circuit_breaker.call(async {
//!     // Your operation here
//!     Ok::<_, String>(42)
//! }).await;
//!
//! // Create a bulkhead for isolation
//! let bulkhead = Bulkhead::new(BulkheadConfig {
//!     max_concurrent: 10,
//!     ..Default::default()
//! });
//!
//! // Execute with bulkhead protection
//! let result = bulkhead.call(async {
//!     // Your operation here
//!     Ok::<_, String>(42)
//! }).await;
//! ```

pub mod bulkhead;
pub mod circuit_breaker;
pub mod degradation;
pub mod health_check;
pub mod timeout;

// Re-export main types from submodules
pub use bulkhead::{
    Bulkhead, BulkheadConfig, BulkheadPermit, BulkheadRegistry, BulkheadStats, ThreadPoolBulkhead,
    ThreadPoolBulkheadConfig,
};
pub use circuit_breaker::{
    CircuitBreaker, CircuitBreakerConfig, CircuitBreakerGuard, CircuitBreakerRegistry,
    CircuitBreakerStats, CircuitState,
};
pub use degradation::{
    ClassificationRule, DegradationConfig, DegradationLevel, DegradationManager, DegradationStats,
    FeatureFlag, LoadShedder, RequestClassifier, RequestPriority,
};
pub use health_check::{
    CompositeHealthCheck, FunctionHealthCheck, HealthCheck, HealthCheckConfig, HealthCheckManager,
    HealthCheckResult, HealthCheckStats, HealthStatus,
};
pub use timeout::{
    Deadline, TimeoutBudget, TimeoutConfig, TimeoutManager, TimeoutRegistry, TimeoutStats,
};

// Original fault_tolerance module content follows
// (Task retry, speculative execution, checkpointing)

use crate::error::{ClusterError, Result};
use crate::task_graph::TaskId;
use crate::worker_pool::WorkerId;
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::Notify;
use tracing::{debug, info};

/// Fault tolerance manager.
#[derive(Clone)]
pub struct FaultToleranceManager {
    inner: Arc<FaultToleranceInner>,
}

struct FaultToleranceInner {
    /// Configuration
    config: FaultToleranceConfig,

    /// Task retry state
    retry_state: DashMap<TaskId, RetryState>,

    /// Checkpoints
    checkpoints: DashMap<TaskId, Checkpoint>,

    /// Failure history
    failure_history: RwLock<VecDeque<FailureRecord>>,

    /// Speculative execution tracking
    speculative_tasks: DashMap<TaskId, Vec<SpeculativeExecution>>,

    /// Statistics
    stats: Arc<FaultToleranceStats>,

    /// Recovery notification
    #[allow(dead_code)]
    recovery_notify: Arc<Notify>,
}

/// Fault tolerance configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaultToleranceConfig {
    /// Maximum retry attempts
    pub max_retries: u32,

    /// Initial retry delay
    pub initial_retry_delay: Duration,

    /// Maximum retry delay
    pub max_retry_delay: Duration,

    /// Retry delay multiplier (for exponential backoff)
    pub retry_multiplier: f64,

    /// Enable speculative execution
    pub enable_speculative: bool,

    /// Straggler detection threshold (task running > threshold * median)
    pub straggler_threshold: f64,

    /// Minimum tasks for straggler detection
    pub min_tasks_for_straggler_detection: usize,

    /// Enable checkpointing
    pub enable_checkpointing: bool,

    /// Checkpoint interval
    pub checkpoint_interval: Duration,

    /// Maximum failure history size
    pub max_failure_history: usize,

    /// Worker failure threshold
    pub worker_failure_threshold: u32,

    /// Worker failure window
    pub worker_failure_window: Duration,
}

impl Default for FaultToleranceConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_retry_delay: Duration::from_secs(1),
            max_retry_delay: Duration::from_secs(60),
            retry_multiplier: 2.0,
            enable_speculative: true,
            straggler_threshold: 2.0,
            min_tasks_for_straggler_detection: 10,
            enable_checkpointing: true,
            checkpoint_interval: Duration::from_secs(300),
            max_failure_history: 1000,
            worker_failure_threshold: 5,
            worker_failure_window: Duration::from_secs(300),
        }
    }
}

/// Retry state for a task.
#[derive(Debug, Clone)]
pub struct RetryState {
    /// Task ID
    pub task_id: TaskId,

    /// Current retry count
    pub retry_count: u32,

    /// Next retry time
    pub next_retry_at: Option<Instant>,

    /// Last failure reason
    pub last_failure: Option<String>,

    /// Last failed worker
    pub last_failed_worker: Option<WorkerId>,

    /// Retry history
    pub retry_history: Vec<RetryAttempt>,
}

/// Retry attempt record.
#[derive(Debug, Clone)]
pub struct RetryAttempt {
    /// Attempt number
    pub attempt: u32,

    /// Attempted at
    pub attempted_at: Instant,

    /// Worker ID
    pub worker_id: WorkerId,

    /// Failure reason
    pub failure_reason: Option<String>,

    /// Duration before failure
    pub duration: Duration,
}

/// Checkpoint for task state.
#[derive(Debug, Clone)]
pub struct Checkpoint {
    /// Task ID
    pub task_id: TaskId,

    /// Checkpoint data
    pub data: Vec<u8>,

    /// Checkpoint timestamp
    pub timestamp: Instant,

    /// Checkpoint version
    pub version: u64,
}

/// Failure record.
#[derive(Debug, Clone)]
pub struct FailureRecord {
    /// Task ID (if task failure)
    pub task_id: Option<TaskId>,

    /// Worker ID
    pub worker_id: Option<WorkerId>,

    /// Failure type
    pub failure_type: FailureType,

    /// Failure reason
    pub reason: String,

    /// Timestamp
    pub timestamp: Instant,

    /// Recovered
    pub recovered: bool,
}

/// Failure type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailureType {
    /// Task execution failure
    TaskFailure,

    /// Worker failure
    WorkerFailure,

    /// Network failure
    NetworkFailure,

    /// Timeout
    Timeout,

    /// Resource exhaustion
    ResourceExhaustion,

    /// Data unavailable
    DataUnavailable,
}

/// Speculative execution instance.
#[derive(Debug, Clone)]
pub struct SpeculativeExecution {
    /// Original task ID
    pub original_task_id: TaskId,

    /// Speculative task ID
    pub speculative_task_id: TaskId,

    /// Worker ID
    pub worker_id: WorkerId,

    /// Started at
    pub started_at: Instant,

    /// Completed
    pub completed: bool,
}

/// Fault tolerance statistics.
#[derive(Debug, Default)]
struct FaultToleranceStats {
    /// Total retries
    total_retries: AtomicU64,

    /// Successful recoveries
    successful_recoveries: AtomicU64,

    /// Permanent failures
    permanent_failures: AtomicU64,

    /// Speculative executions
    speculative_executions: AtomicU64,

    /// Speculative successes
    speculative_successes: AtomicU64,

    /// Checkpoints created
    checkpoints_created: AtomicU64,

    /// Checkpoints restored
    checkpoints_restored: AtomicU64,

    /// Worker failures
    worker_failures: AtomicU64,
}

impl FaultToleranceManager {
    /// Create a new fault tolerance manager.
    pub fn new(config: FaultToleranceConfig) -> Self {
        Self {
            inner: Arc::new(FaultToleranceInner {
                config,
                retry_state: DashMap::new(),
                checkpoints: DashMap::new(),
                failure_history: RwLock::new(VecDeque::new()),
                speculative_tasks: DashMap::new(),
                stats: Arc::new(FaultToleranceStats::default()),
                recovery_notify: Arc::new(Notify::new()),
            }),
        }
    }

    /// Create with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(FaultToleranceConfig::default())
    }

    /// Handle task failure.
    pub async fn handle_task_failure(
        &self,
        task_id: TaskId,
        worker_id: WorkerId,
        error: String,
    ) -> Result<RetryDecision> {
        let mut retry_state = self
            .inner
            .retry_state
            .entry(task_id)
            .or_insert_with(|| RetryState {
                task_id,
                retry_count: 0,
                next_retry_at: None,
                last_failure: None,
                last_failed_worker: None,
                retry_history: Vec::new(),
            });

        retry_state.retry_count += 1;
        retry_state.last_failure = Some(error.clone());
        retry_state.last_failed_worker = Some(worker_id);

        // Record failure
        self.record_failure(FailureRecord {
            task_id: Some(task_id),
            worker_id: Some(worker_id),
            failure_type: FailureType::TaskFailure,
            reason: error.clone(),
            timestamp: Instant::now(),
            recovered: false,
        });

        // Check if we should retry
        if retry_state.retry_count >= self.inner.config.max_retries {
            self.inner
                .stats
                .permanent_failures
                .fetch_add(1, Ordering::Relaxed);

            return Ok(RetryDecision::GiveUp {
                reason: "Maximum retries exceeded".to_string(),
            });
        }

        // Calculate retry delay with exponential backoff
        let delay = self.calculate_retry_delay(retry_state.retry_count);
        let next_retry_at = Instant::now() + delay;
        retry_state.next_retry_at = Some(next_retry_at);

        self.inner
            .stats
            .total_retries
            .fetch_add(1, Ordering::Relaxed);

        Ok(RetryDecision::Retry {
            delay,
            attempt: retry_state.retry_count,
            avoid_worker: Some(worker_id),
        })
    }

    /// Calculate retry delay with exponential backoff.
    fn calculate_retry_delay(&self, attempt: u32) -> Duration {
        let delay_secs = self.inner.config.initial_retry_delay.as_secs_f64()
            * self.inner.config.retry_multiplier.powi(attempt as i32 - 1);

        let delay = Duration::from_secs_f64(delay_secs);

        delay.min(self.inner.config.max_retry_delay)
    }

    /// Check if task is ready for retry.
    pub fn is_ready_for_retry(&self, task_id: TaskId) -> bool {
        if let Some(retry_state) = self.inner.retry_state.get(&task_id) {
            if let Some(next_retry_at) = retry_state.next_retry_at {
                return Instant::now() >= next_retry_at;
            }
        }
        false
    }

    /// Get tasks ready for retry.
    pub fn get_tasks_ready_for_retry(&self) -> Vec<TaskId> {
        let now = Instant::now();
        self.inner
            .retry_state
            .iter()
            .filter(|entry| {
                entry
                    .value()
                    .next_retry_at
                    .map(|t| now >= t)
                    .unwrap_or(false)
            })
            .map(|entry| *entry.key())
            .collect()
    }

    /// Record successful task completion (clear retry state).
    pub fn record_success(&self, task_id: TaskId) {
        self.inner.retry_state.remove(&task_id);

        self.inner
            .stats
            .successful_recoveries
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Create checkpoint for task.
    pub fn create_checkpoint(&self, task_id: TaskId, data: Vec<u8>) -> Result<()> {
        if !self.inner.config.enable_checkpointing {
            return Ok(());
        }

        let version = self
            .inner
            .checkpoints
            .get(&task_id)
            .map(|cp| cp.version + 1)
            .unwrap_or(1);

        let checkpoint = Checkpoint {
            task_id,
            data,
            timestamp: Instant::now(),
            version,
        };

        self.inner.checkpoints.insert(task_id, checkpoint);

        self.inner
            .stats
            .checkpoints_created
            .fetch_add(1, Ordering::Relaxed);

        debug!(
            "Created checkpoint for task {} (version {})",
            task_id, version
        );

        Ok(())
    }

    /// Restore task from checkpoint.
    pub fn restore_checkpoint(&self, task_id: TaskId) -> Result<Option<Vec<u8>>> {
        if let Some(checkpoint) = self.inner.checkpoints.get(&task_id) {
            self.inner
                .stats
                .checkpoints_restored
                .fetch_add(1, Ordering::Relaxed);

            info!(
                "Restored checkpoint for task {} (version {})",
                task_id, checkpoint.version
            );

            Ok(Some(checkpoint.data.clone()))
        } else {
            Ok(None)
        }
    }

    /// Remove checkpoint.
    pub fn remove_checkpoint(&self, task_id: TaskId) {
        self.inner.checkpoints.remove(&task_id);
    }

    /// Start speculative execution for straggler task.
    pub fn start_speculative_execution(
        &self,
        original_task_id: TaskId,
        speculative_task_id: TaskId,
        worker_id: WorkerId,
    ) -> Result<()> {
        if !self.inner.config.enable_speculative {
            return Err(ClusterError::InvalidState(
                "Speculative execution not enabled".to_string(),
            ));
        }

        let spec_exec = SpeculativeExecution {
            original_task_id,
            speculative_task_id,
            worker_id,
            started_at: Instant::now(),
            completed: false,
        };

        self.inner
            .speculative_tasks
            .entry(original_task_id)
            .or_default()
            .push(spec_exec);

        self.inner
            .stats
            .speculative_executions
            .fetch_add(1, Ordering::Relaxed);

        info!(
            "Started speculative execution for task {}",
            original_task_id
        );

        Ok(())
    }

    /// Complete speculative execution.
    pub fn complete_speculative_execution(
        &self,
        original_task_id: TaskId,
        speculative_task_id: TaskId,
    ) -> Result<bool> {
        if let Some(mut spec_list) = self.inner.speculative_tasks.get_mut(&original_task_id) {
            for spec in spec_list.iter_mut() {
                if spec.speculative_task_id == speculative_task_id {
                    spec.completed = true;

                    self.inner
                        .stats
                        .speculative_successes
                        .fetch_add(1, Ordering::Relaxed);

                    info!(
                        "Completed speculative execution for task {}",
                        original_task_id
                    );

                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    /// Cancel speculative executions for a task.
    pub fn cancel_speculative_executions(&self, original_task_id: TaskId) -> Vec<TaskId> {
        self.inner
            .speculative_tasks
            .remove(&original_task_id)
            .map(|(_, specs)| {
                specs
                    .into_iter()
                    .filter(|s| !s.completed)
                    .map(|s| s.speculative_task_id)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Record failure.
    fn record_failure(&self, record: FailureRecord) {
        let mut history = self.inner.failure_history.write();

        // Check failure type before moving record
        let is_worker_failure = matches!(record.failure_type, FailureType::WorkerFailure);

        history.push_back(record);

        // Trim history if too large
        while history.len() > self.inner.config.max_failure_history {
            history.pop_front();
        }

        // Update worker failure stats
        if is_worker_failure {
            self.inner
                .stats
                .worker_failures
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Get worker failure rate.
    pub fn get_worker_failure_rate(&self, worker_id: WorkerId) -> f64 {
        let history = self.inner.failure_history.read();
        let window_start = Instant::now() - self.inner.config.worker_failure_window;

        let total_failures = history
            .iter()
            .filter(|r| r.worker_id == Some(worker_id) && r.timestamp >= window_start)
            .count();

        let total_tasks = history
            .iter()
            .filter(|r| r.worker_id == Some(worker_id) && r.timestamp >= window_start)
            .count();

        if total_tasks == 0 {
            0.0
        } else {
            total_failures as f64 / total_tasks as f64
        }
    }

    /// Check if worker should be quarantined.
    pub fn should_quarantine_worker(&self, worker_id: WorkerId) -> bool {
        let history = self.inner.failure_history.read();
        let window_start = Instant::now() - self.inner.config.worker_failure_window;

        let recent_failures = history
            .iter()
            .filter(|r| {
                r.worker_id == Some(worker_id)
                    && r.timestamp >= window_start
                    && matches!(r.failure_type, FailureType::WorkerFailure)
            })
            .count();

        recent_failures as u32 >= self.inner.config.worker_failure_threshold
    }

    /// Get failure history.
    pub fn get_failure_history(&self) -> Vec<FailureRecord> {
        self.inner.failure_history.read().iter().cloned().collect()
    }

    /// Get statistics.
    pub fn get_statistics(&self) -> FaultToleranceStatistics {
        let total_retries = self.inner.stats.total_retries.load(Ordering::Relaxed);
        let successful = self
            .inner
            .stats
            .successful_recoveries
            .load(Ordering::Relaxed);
        let permanent = self.inner.stats.permanent_failures.load(Ordering::Relaxed);

        let recovery_rate = if total_retries > 0 {
            successful as f64 / total_retries as f64
        } else {
            0.0
        };

        let spec_executions = self
            .inner
            .stats
            .speculative_executions
            .load(Ordering::Relaxed);
        let spec_successes = self
            .inner
            .stats
            .speculative_successes
            .load(Ordering::Relaxed);

        let speculative_success_rate = if spec_executions > 0 {
            spec_successes as f64 / spec_executions as f64
        } else {
            0.0
        };

        FaultToleranceStatistics {
            total_retries,
            successful_recoveries: successful,
            permanent_failures: permanent,
            recovery_rate,
            speculative_executions: spec_executions,
            speculative_successes: spec_successes,
            speculative_success_rate,
            checkpoints_created: self.inner.stats.checkpoints_created.load(Ordering::Relaxed),
            checkpoints_restored: self
                .inner
                .stats
                .checkpoints_restored
                .load(Ordering::Relaxed),
            worker_failures: self.inner.stats.worker_failures.load(Ordering::Relaxed),
            active_retry_states: self.inner.retry_state.len(),
            active_checkpoints: self.inner.checkpoints.len(),
        }
    }
}

/// Retry decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RetryDecision {
    /// Retry the task
    Retry {
        /// Delay before retry
        delay: Duration,
        /// Retry attempt number
        attempt: u32,
        /// Worker to avoid
        avoid_worker: Option<WorkerId>,
    },
    /// Give up on the task
    GiveUp {
        /// Reason for giving up
        reason: String,
    },
}

/// Fault tolerance statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaultToleranceStatistics {
    /// Total retry attempts
    pub total_retries: u64,

    /// Successful recoveries
    pub successful_recoveries: u64,

    /// Permanent failures
    pub permanent_failures: u64,

    /// Recovery rate (0.0-1.0)
    pub recovery_rate: f64,

    /// Speculative executions started
    pub speculative_executions: u64,

    /// Speculative executions that succeeded
    pub speculative_successes: u64,

    /// Speculative success rate (0.0-1.0)
    pub speculative_success_rate: f64,

    /// Checkpoints created
    pub checkpoints_created: u64,

    /// Checkpoints restored
    pub checkpoints_restored: u64,

    /// Worker failures
    pub worker_failures: u64,

    /// Active retry states
    pub active_retry_states: usize,

    /// Active checkpoints
    pub active_checkpoints: usize,
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fault_tolerance_creation() {
        let ft = FaultToleranceManager::with_defaults();
        let stats = ft.get_statistics();
        assert_eq!(stats.total_retries, 0);
    }

    #[tokio::test]
    async fn test_task_retry() {
        let ft = FaultToleranceManager::with_defaults();
        let task_id = TaskId::new();
        let worker_id = WorkerId::new();

        let decision = ft
            .handle_task_failure(task_id, worker_id, "Test error".to_string())
            .await;

        assert!(decision.is_ok());
        if let Ok(RetryDecision::Retry { attempt, .. }) = decision {
            assert_eq!(attempt, 1);
        }
    }

    #[tokio::test]
    async fn test_max_retries() {
        let config = FaultToleranceConfig {
            max_retries: 2,
            ..Default::default()
        };

        let ft = FaultToleranceManager::new(config);
        let task_id = TaskId::new();
        let worker_id = WorkerId::new();

        // First retry
        ft.handle_task_failure(task_id, worker_id, "Error 1".to_string())
            .await
            .ok();

        // Second retry
        ft.handle_task_failure(task_id, worker_id, "Error 2".to_string())
            .await
            .ok();

        // Third attempt should give up
        let decision = ft
            .handle_task_failure(task_id, worker_id, "Error 3".to_string())
            .await;

        assert!(matches!(decision, Ok(RetryDecision::GiveUp { .. })));
    }

    #[test]
    fn test_checkpoint() {
        let ft = FaultToleranceManager::with_defaults();
        let task_id = TaskId::new();
        let data = vec![1, 2, 3, 4, 5];

        let result = ft.create_checkpoint(task_id, data.clone());
        assert!(result.is_ok());

        let restored = ft.restore_checkpoint(task_id);
        assert!(restored.is_ok());
        if let Ok(Some(restored_data)) = restored {
            assert_eq!(restored_data, data);
        }
    }

    #[test]
    fn test_worker_quarantine() {
        let ft = FaultToleranceManager::with_defaults();
        let worker_id = WorkerId::new();

        // Record multiple failures
        for i in 0..10 {
            ft.record_failure(FailureRecord {
                task_id: None,
                worker_id: Some(worker_id),
                failure_type: FailureType::WorkerFailure,
                reason: format!("Failure {}", i),
                timestamp: Instant::now(),
                recovered: false,
            });
        }

        assert!(ft.should_quarantine_worker(worker_id));
    }
}
