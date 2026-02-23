//! OxiGDAL Cluster - Distributed geospatial processing at scale.
//!
//! This crate provides a comprehensive distributed computing framework for geospatial
//! data processing, including:
//!
//! - **Distributed Scheduler**: Work-stealing scheduler with priority queues and dynamic load balancing
//! - **Task Graph Engine**: DAG-based task dependencies with parallel execution planning
//! - **Worker Pool Management**: Worker registration, health monitoring, and automatic failover
//! - **Data Locality Optimization**: Minimize data transfer through intelligent task placement
//! - **Fault Tolerance**: Task retry, speculative execution, and checkpointing
//! - **Distributed Cache**: Cache coherency with compression and distributed LRU
//! - **Data Replication**: Quorum-based reads/writes with automatic re-replication
//! - **Cluster Coordinator**: Raft-based consensus and leader election
//! - **Advanced Scheduling**: Gang scheduling, fair-share, deadline-based, and multi-queue scheduling
//! - **Resource Management**: Quota management, resource reservation, and usage accounting
//! - **Network Optimization**: Topology-aware scheduling, bandwidth tracking, and congestion control
//! - **Workflow Engine**: Complex workflow orchestration with conditional execution and loops
//! - **Autoscaling**: Dynamic cluster sizing based on load metrics and predictive scaling
//! - **Monitoring & Alerting**: Real-time metrics, custom alerts, and anomaly detection
//! - **Security & Access Control**: Authentication, RBAC, audit logging, and secret management
//!
//! # Examples
//!
//! ## Basic Cluster Setup
//!
//! ```rust,no_run
//! use oxigdal_cluster::{Cluster, ClusterBuilder, SchedulerConfig};
//!
//! #[tokio::main]
//! async fn main() -> oxigdal_cluster::Result<()> {
//!     // Create a cluster with default configuration
//!     let cluster = Cluster::new();
//!
//!     // Start the cluster
//!     cluster.start().await?;
//!
//!     // Get cluster statistics
//!     let stats = cluster.get_statistics();
//!     println!("Active workers: {}", stats.worker_pool.total_workers);
//!
//!     // Stop the cluster
//!     cluster.stop().await?;
//!     Ok(())
//! }
//! ```
//!
//! ## Custom Cluster Configuration
//!
//! ```rust,no_run
//! use oxigdal_cluster::{ClusterBuilder, SchedulerConfig, LoadBalanceStrategy};
//! use oxigdal_cluster::worker_pool::WorkerPoolConfig;
//! use std::time::Duration;
//!
//! #[tokio::main]
//! async fn main() -> oxigdal_cluster::Result<()> {
//!     // Configure the scheduler
//!     let scheduler_config = SchedulerConfig {
//!         max_queue_size: 10000,
//!         work_steal_threshold: 10,
//!         scheduling_interval: Duration::from_millis(100),
//!         load_balance_strategy: LoadBalanceStrategy::LeastLoaded,
//!         task_timeout: Duration::from_secs(300),
//!         enable_work_stealing: true,
//!         enable_backpressure: true,
//!         max_concurrent_tasks_per_worker: 1000,
//!     };
//!
//!     // Configure the worker pool
//!     let worker_config = WorkerPoolConfig {
//!         heartbeat_timeout: Duration::from_secs(30),
//!         health_check_interval: Duration::from_secs(10),
//!         max_unhealthy_duration: Duration::from_secs(120),
//!         min_workers: 1,
//!         max_workers: 100,
//!     };
//!
//!     // Build the cluster with custom configuration
//!     let cluster = ClusterBuilder::new()
//!         .with_scheduler_config(scheduler_config)
//!         .with_worker_pool_config(worker_config)
//!         .build();
//!
//!     cluster.start().await?;
//!
//!     // Your cluster operations here
//!
//!     cluster.stop().await?;
//!     Ok(())
//! }
//! ```

#![deny(missing_docs)]
#![warn(clippy::all)]

pub mod autoscale;
pub mod cache_coherency;
pub mod coordinator;
pub mod data_locality;
pub mod error;
pub mod fault_tolerance;
pub mod metrics;
pub mod monitoring;
pub mod network;
pub mod replication;
pub mod resources;
pub mod scheduler;
pub mod security;
pub mod task_graph;
pub mod worker_pool;
pub mod workflow;

// Re-export common types
pub use autoscale::{
    AutoscaleConfig, AutoscaleStats, Autoscaler, MetricsSnapshot as AutoscaleMetrics, ScaleDecision,
};
pub use cache_coherency::{CacheConfig, CacheKey, DistributedCache};
pub use coordinator::{ClusterCoordinator, CoordinatorConfig, NodeId, NodeRole};
pub use data_locality::{DataLocalityOptimizer, LocalityConfig, PlacementRecommendation};
pub use error::{ClusterError, Result};
pub use fault_tolerance::{
    // Bulkhead
    Bulkhead,
    BulkheadConfig,
    BulkheadRegistry,
    BulkheadStats,
    // Circuit breaker
    CircuitBreaker,
    CircuitBreakerConfig,
    CircuitBreakerRegistry,
    CircuitBreakerStats,
    CircuitState,
    // Timeout management
    Deadline,
    // Graceful degradation
    DegradationConfig,
    DegradationLevel,
    DegradationManager,
    DegradationStats,
    // Core fault tolerance
    FaultToleranceConfig,
    FaultToleranceManager,
    FaultToleranceStatistics,
    // Health checks
    HealthCheck,
    HealthCheckConfig,
    HealthCheckManager,
    HealthCheckResult,
    HealthCheckStats,
    HealthStatus,
    RequestPriority,
    RetryDecision,
    TimeoutBudget,
    TimeoutConfig,
    TimeoutManager,
    TimeoutRegistry,
    TimeoutStats,
};
pub use metrics::{ClusterMetrics, MetricsSnapshot, WorkerMetrics};
pub use monitoring::{Alert, AlertRule, AlertSeverity, MonitoringManager, MonitoringStats};
pub use network::{BandwidthTracker, CompressionManager, CongestionController, TopologyManager};
pub use replication::{ReplicaSet, ReplicationConfig, ReplicationManager};
pub use resources::{AccountingManager, QuotaManager, ReservationManager};
pub use scheduler::{LoadBalanceStrategy, Scheduler, SchedulerConfig, SchedulerStats};
pub use security::{Role, SecurityManager, SecurityStats, User};
pub use task_graph::{ExecutionPlan, ResourceRequirements, Task, TaskGraph, TaskId, TaskStatus};
pub use worker_pool::{
    SelectionStrategy, Worker, WorkerCapabilities, WorkerCapacity, WorkerId, WorkerPool,
    WorkerStatus, WorkerUsage,
};
pub use workflow::{Workflow, WorkflowEngine, WorkflowExecution, WorkflowStatus};

/// Cluster builder for easy setup.
///
/// The builder pattern allows you to customize various aspects of the cluster
/// before initialization. All configuration is optional; if not specified,
/// sensible defaults will be used.
///
/// # Examples
///
/// ```rust
/// use oxigdal_cluster::{ClusterBuilder, SchedulerConfig, LoadBalanceStrategy};
/// use std::time::Duration;
///
/// let scheduler_config = SchedulerConfig {
///     max_queue_size: 10000,
///     work_steal_threshold: 10,
///     scheduling_interval: Duration::from_millis(100),
///     load_balance_strategy: LoadBalanceStrategy::RoundRobin,
///     task_timeout: Duration::from_secs(600),
///     enable_work_stealing: true,
///     enable_backpressure: true,
///     max_concurrent_tasks_per_worker: 500,
/// };
///
/// let cluster = ClusterBuilder::new()
///     .with_scheduler_config(scheduler_config)
///     .build();
/// ```
pub struct ClusterBuilder {
    scheduler_config: Option<SchedulerConfig>,
    worker_pool_config: Option<worker_pool::WorkerPoolConfig>,
    coordinator_config: Option<CoordinatorConfig>,
    fault_tolerance_config: Option<FaultToleranceConfig>,
    locality_config: Option<LocalityConfig>,
    cache_config: Option<CacheConfig>,
    replication_config: Option<ReplicationConfig>,
}

impl ClusterBuilder {
    /// Create a new cluster builder.
    pub fn new() -> Self {
        Self {
            scheduler_config: None,
            worker_pool_config: None,
            coordinator_config: None,
            fault_tolerance_config: None,
            locality_config: None,
            cache_config: None,
            replication_config: None,
        }
    }

    /// Set scheduler configuration.
    pub fn with_scheduler_config(mut self, config: SchedulerConfig) -> Self {
        self.scheduler_config = Some(config);
        self
    }

    /// Set worker pool configuration.
    pub fn with_worker_pool_config(mut self, config: worker_pool::WorkerPoolConfig) -> Self {
        self.worker_pool_config = Some(config);
        self
    }

    /// Set coordinator configuration.
    pub fn with_coordinator_config(mut self, config: CoordinatorConfig) -> Self {
        self.coordinator_config = Some(config);
        self
    }

    /// Set fault tolerance configuration.
    pub fn with_fault_tolerance_config(mut self, config: FaultToleranceConfig) -> Self {
        self.fault_tolerance_config = Some(config);
        self
    }

    /// Set locality optimizer configuration.
    pub fn with_locality_config(mut self, config: LocalityConfig) -> Self {
        self.locality_config = Some(config);
        self
    }

    /// Set cache configuration.
    pub fn with_cache_config(mut self, config: CacheConfig) -> Self {
        self.cache_config = Some(config);
        self
    }

    /// Set replication configuration.
    pub fn with_replication_config(mut self, config: ReplicationConfig) -> Self {
        self.replication_config = Some(config);
        self
    }

    /// Build the cluster.
    pub fn build(self) -> Cluster {
        use std::sync::Arc;

        let task_graph = Arc::new(TaskGraph::new());
        let metrics = Arc::new(ClusterMetrics::new());

        let worker_pool = Arc::new(if let Some(config) = self.worker_pool_config {
            WorkerPool::new(config)
        } else {
            WorkerPool::with_defaults()
        });

        let scheduler = Arc::new(if let Some(config) = self.scheduler_config {
            Scheduler::new(
                Arc::clone(&task_graph),
                Arc::clone(&worker_pool),
                Arc::clone(&metrics),
                config,
            )
        } else {
            Scheduler::with_defaults(
                Arc::clone(&task_graph),
                Arc::clone(&worker_pool),
                Arc::clone(&metrics),
            )
        });

        let coordinator = Arc::new(if let Some(config) = self.coordinator_config {
            ClusterCoordinator::new(config)
        } else {
            ClusterCoordinator::with_defaults()
        });

        let fault_tolerance = Arc::new(if let Some(config) = self.fault_tolerance_config {
            FaultToleranceManager::new(config)
        } else {
            FaultToleranceManager::with_defaults()
        });

        let locality_optimizer = Arc::new(if let Some(config) = self.locality_config {
            DataLocalityOptimizer::new(config)
        } else {
            DataLocalityOptimizer::with_defaults()
        });

        let cache = Arc::new(if let Some(config) = self.cache_config {
            DistributedCache::new(config)
        } else {
            DistributedCache::with_defaults()
        });

        let replication = Arc::new(if let Some(config) = self.replication_config {
            ReplicationManager::new(config)
        } else {
            ReplicationManager::with_defaults()
        });

        Cluster {
            task_graph,
            worker_pool,
            scheduler,
            coordinator,
            fault_tolerance,
            locality_optimizer,
            cache,
            replication,
            metrics,
        }
    }
}

impl Default for ClusterBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Complete cluster instance.
///
/// The `Cluster` struct provides access to all cluster components including
/// the task graph, worker pool, scheduler, coordinator, fault tolerance,
/// data locality optimizer, distributed cache, and replication manager.
///
/// # Examples
///
/// ```rust,no_run
/// use oxigdal_cluster::Cluster;
///
/// #[tokio::main]
/// async fn main() -> oxigdal_cluster::Result<()> {
///     // Create and start a cluster
///     let cluster = Cluster::new();
///     cluster.start().await?;
///
///     // Access cluster components
///     let metrics = cluster.metrics.snapshot();
///     println!("Tasks completed: {}", metrics.tasks_completed);
///
///     // Get cluster statistics
///     let stats = cluster.get_statistics();
///     println!("Total workers: {}", stats.worker_pool.total_workers);
///
///     // Stop the cluster
///     cluster.stop().await?;
///     Ok(())
/// }
/// ```
pub struct Cluster {
    /// Task graph
    pub task_graph: std::sync::Arc<TaskGraph>,

    /// Worker pool
    pub worker_pool: std::sync::Arc<WorkerPool>,

    /// Scheduler
    pub scheduler: std::sync::Arc<Scheduler>,

    /// Coordinator
    pub coordinator: std::sync::Arc<ClusterCoordinator>,

    /// Fault tolerance manager
    pub fault_tolerance: std::sync::Arc<FaultToleranceManager>,

    /// Data locality optimizer
    pub locality_optimizer: std::sync::Arc<DataLocalityOptimizer>,

    /// Distributed cache
    pub cache: std::sync::Arc<DistributedCache>,

    /// Replication manager
    pub replication: std::sync::Arc<ReplicationManager>,

    /// Cluster metrics
    pub metrics: std::sync::Arc<ClusterMetrics>,
}

impl Cluster {
    /// Create a new cluster with default configuration.
    pub fn new() -> Self {
        ClusterBuilder::new().build()
    }

    /// Start all cluster components.
    pub async fn start(&self) -> Result<()> {
        self.coordinator.start().await?;
        self.scheduler.start().await?;
        Ok(())
    }

    /// Stop all cluster components.
    pub async fn stop(&self) -> Result<()> {
        self.scheduler.stop().await?;
        self.coordinator.stop().await?;
        Ok(())
    }

    /// Get cluster-wide statistics.
    pub fn get_statistics(&self) -> ClusterStatistics {
        ClusterStatistics {
            metrics: self.metrics.snapshot(),
            scheduler: self.scheduler.get_statistics(),
            worker_pool: self.worker_pool.get_statistics(),
            coordinator: self.coordinator.get_statistics(),
            fault_tolerance: self.fault_tolerance.get_statistics(),
            locality: self.locality_optimizer.get_statistics(),
            cache: self.cache.get_statistics(),
            replication: self.replication.get_statistics(),
        }
    }
}

impl Default for Cluster {
    fn default() -> Self {
        Self::new()
    }
}

/// Cluster-wide statistics.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ClusterStatistics {
    /// Metrics snapshot
    pub metrics: MetricsSnapshot,

    /// Scheduler statistics
    pub scheduler: SchedulerStats,

    /// Worker pool statistics
    pub worker_pool: worker_pool::WorkerPoolStatistics,

    /// Coordinator statistics
    pub coordinator: coordinator::CoordinatorStatistics,

    /// Fault tolerance statistics
    pub fault_tolerance: fault_tolerance::FaultToleranceStatistics,

    /// Locality statistics
    pub locality: data_locality::LocalityStats,

    /// Cache statistics
    pub cache: cache_coherency::CacheStats,

    /// Replication statistics
    pub replication: replication::ReplicationStatistics,
}
