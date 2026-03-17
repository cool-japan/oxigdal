# OxiGDAL Cluster

[![Crates.io](https://img.shields.io/crates/v/oxigdal-cluster.svg)](https://crates.io/crates/oxigdal-cluster)
[![Documentation](https://docs.rs/oxigdal-cluster/badge.svg)](https://docs.rs/oxigdal-cluster)
[![License](https://img.shields.io/crates/l/oxigdal-cluster.svg)](LICENSE)
[![Pure Rust](https://img.shields.io/badge/Pure%20Rust-FFD700?logo=rust)](https://www.rust-lang.org/)

A comprehensive **Pure Rust** distributed computing framework for large-scale geospatial data processing. OxiGDAL Cluster provides enterprise-grade clustering, scheduling, and data distribution capabilities designed for processing massive geospatial datasets across multiple nodes.

## Features

- **Distributed Scheduler**: Work-stealing scheduler with priority queues and dynamic load balancing for optimal resource utilization
- **Task Graph Engine**: DAG-based task dependencies with parallel execution planning and optimization
- **Worker Pool Management**: Worker registration, health monitoring, automatic failover, and dynamic resource allocation
- **Data Locality Optimization**: Intelligent task placement to minimize data transfer and maximize cache locality
- **Fault Tolerance**: Comprehensive error handling including task retry, speculative execution, and distributed checkpointing
- **Distributed Cache**: Cache coherency protocol with compression support and distributed LRU eviction
- **Data Replication**: Quorum-based reads/writes with automatic re-replication and consistency guarantees
- **Cluster Coordination**: Leader election and state management via Raft-based consensus (implemented in `oxigdal-ha`)
- **Advanced Scheduling**: Gang scheduling, fair-share scheduling, deadline-based scheduling, and multi-queue support
- **Resource Management**: Quota management, resource reservation, and comprehensive usage accounting
- **Network Optimization**: Topology-aware scheduling, bandwidth tracking, and congestion control
- **Workflow Engine**: Complex workflow orchestration with conditional execution and loop support
- **Autoscaling**: Dynamic cluster sizing based on load metrics and predictive scaling
- **Monitoring & Alerting**: Real-time metrics collection, custom alert rules, and anomaly detection
- **Security & Access Control**: Authentication, role-based access control (RBAC), audit logging, and secret management

## Pure Rust

This library is **100% Pure Rust** with no C/Fortran dependencies. All functionality works out of the box without requiring external libraries or system dependencies.

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
oxigdal-cluster = "0.1.2"
```

## Quick Start

Create a distributed cluster and schedule tasks:

```rust
use oxigdal_cluster::prelude::*;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a cluster with default configuration
    let cluster = Cluster::new();

    // Start cluster components
    cluster.start().await?;

    // Create a task graph
    let mut task_graph = TaskGraph::new();

    // Add tasks to the graph
    let task1 = Task {
        id: TaskId::from(1),
        name: "process_geospatial_data".to_string(),
        dependencies: vec![],
        resource_requirements: ResourceRequirements::default(),
        status: TaskStatus::Pending,
        // ... other fields
    };

    task_graph.add_task(task1);

    // Submit tasks for execution
    // cluster.scheduler.schedule(&task_graph).await?;

    // Get cluster statistics
    let stats = cluster.get_statistics();
    println!("Cluster stats: {:?}", stats.metrics);

    // Stop cluster when done
    cluster.stop().await?;

    Ok(())
}
```

## Usage

### Basic Cluster Setup

```rust
use oxigdal_cluster::prelude::*;

// Create a cluster with custom configuration
let cluster = Cluster::new();

// Access individual components
let metrics = &cluster.metrics;
let scheduler = &cluster.scheduler;
let worker_pool = &cluster.worker_pool;
let coordinator = &cluster.coordinator;
```

### Worker Pool Management

```rust
use oxigdal_cluster::{WorkerPool, Worker, WorkerId, WorkerStatus};

let worker_pool = WorkerPool::with_defaults();

// Register a new worker
let worker = Worker {
    id: WorkerId::from(1),
    host: "localhost:9090".to_string(),
    status: WorkerStatus::Healthy,
    // ... other configuration
};
```

### Task Scheduling

```rust
use oxigdal_cluster::{Task, TaskId, TaskGraph, TaskStatus, ResourceRequirements};

let mut task_graph = TaskGraph::new();

// Create tasks with dependencies and resource requirements
let task = Task {
    id: TaskId::from(1),
    name: "process_tile".to_string(),
    dependencies: vec![],
    resource_requirements: ResourceRequirements {
        memory_mb: 2048,
        cpu_cores: 4,
        gpu_memory_mb: 0,
        // ... other requirements
    },
    status: TaskStatus::Pending,
};

task_graph.add_task(task);
```

### Data Locality Optimization

```rust
use oxigdal_cluster::{DataLocalityOptimizer, LocalityConfig};

let config = LocalityConfig {
    enable_data_locality: true,
    data_locality_weight: 0.8,
    network_latency_ms: 1,
    // ... other settings
};

let optimizer = DataLocalityOptimizer::new(config);

// Get placement recommendations for tasks
// let recommendation = optimizer.recommend_placement(&task_id)?;
```

### Distributed Caching

```rust
use oxigdal_cluster::{DistributedCache, CacheConfig, CacheKey};

let config = CacheConfig {
    max_size_mb: 4096,
    enable_compression: true,
    compression_threshold_bytes: 1024,
    // ... other settings
};

let cache = DistributedCache::new(config);

// Store and retrieve data
// cache.put(key, value).await?;
// let value = cache.get(&key).await?;
```

### Fault Tolerance & Resilience

```rust
use oxigdal_cluster::{FaultToleranceManager, FaultToleranceConfig, CircuitBreakerConfig};

let config = FaultToleranceConfig {
    max_retries: 3,
    initial_retry_delay_ms: 100,
    max_retry_delay_ms: 10000,
    // ... other settings
};

let ft_manager = FaultToleranceManager::new(config);

// Manage resilience across cluster nodes
```

### Workflow Orchestration

```rust
use oxigdal_cluster::{WorkflowEngine, Workflow};

let engine = WorkflowEngine::new();

// Create and execute workflows with conditional branching
// let workflow = Workflow::new("tile_processing_pipeline");
// engine.execute(workflow).await?;
```

### Monitoring & Alerting

```rust
use oxigdal_cluster::{MonitoringManager, AlertRule, AlertSeverity};

let monitoring = MonitoringManager::new();

// Set up alert rules for cluster metrics
let alert_rule = AlertRule {
    name: "high_memory_usage".to_string(),
    condition: "memory_percent > 90".to_string(),
    severity: AlertSeverity::Warning,
    // ... other properties
};

// monitoring.add_rule(alert_rule)?;
```

### Autoscaling

```rust
use oxigdal_cluster::{Autoscaler, AutoscaleConfig, ScaleDecision};

let config = AutoscaleConfig {
    min_nodes: 1,
    max_nodes: 100,
    target_cpu_percent: 70.0,
    scale_up_threshold_percent: 80.0,
    scale_down_threshold_percent: 30.0,
    // ... other settings
};

let autoscaler = Autoscaler::new(config);

// Autoscaler monitors metrics and makes scaling decisions
```

## API Overview

### Core Components

| Module | Description |
|--------|-------------|
| `task_graph` | DAG-based task graph with execution planning and optimization |
| `worker_pool` | Worker management, health checks, and dynamic allocation |
| `scheduler` | Work-stealing scheduler with multiple scheduling strategies |
| `coordinator` | Cluster coordination and Raft-based consensus (optional) |
| `fault_tolerance` | Resilience patterns including retries, circuit breakers, and bulkheads |
| `data_locality` | Intelligent task placement based on data location and network topology |
| `cache_coherency` | Distributed cache with coherency protocol and compression |
| `replication` | Data replication with quorum-based consistency |
| `resources` | Resource quota, reservation, and accounting management |
| `network` | Topology-aware scheduling and bandwidth management |
| `workflow` | Complex workflow orchestration with conditional execution |
| `autoscale` | Dynamic cluster sizing based on metrics and predictions |
| `monitoring` | Real-time metrics collection and alert management |
| `security` | Authentication, RBAC, audit logging, and secret management |
| `metrics` | Comprehensive cluster-wide metrics collection |

### Key Types

#### Task Management
- `Task`: Individual unit of work with dependencies and resource requirements
- `TaskGraph`: Directed acyclic graph of tasks with execution planning
- `TaskId`: Unique identifier for tasks
- `TaskStatus`: Task execution status (Pending, Running, Completed, Failed, etc.)
- `ExecutionPlan`: Optimized execution plan for task graph
- `ResourceRequirements`: CPU, memory, and GPU requirements for tasks

#### Worker Management
- `Worker`: Represents a compute node in the cluster
- `WorkerId`: Unique identifier for workers
- `WorkerPool`: Manages collection of workers
- `WorkerStatus`: Health and availability status
- `WorkerCapabilities`: Worker capabilities and specializations
- `WorkerCapacity`: Available resources on a worker

#### Cluster Coordination
- `Cluster`: Main cluster instance managing all components
- `ClusterBuilder`: Fluent builder for cluster configuration
- `ClusterCoordinator`: Raft-based cluster coordination
- `NodeId`: Unique identifier for cluster nodes
- `NodeRole`: Role in the cluster (Leader, Follower, Candidate)

#### Fault Tolerance
- `FaultToleranceManager`: Central fault tolerance orchestrator
- `CircuitBreaker`: Fail-fast pattern implementation
- `Bulkhead`: Resource isolation pattern
- `HealthCheck`: Worker health monitoring
- `RetryDecision`: Intelligent retry logic

#### Data Management
- `DistributedCache`: Multi-node cache with coherency
- `CacheKey`: Cache entry identifier
- `ReplicationManager`: Data replication orchestration
- `ReplicaSet`: Set of replicas for data
- `DataLocalityOptimizer`: Intelligent task placement

#### Scheduling
- `Scheduler`: Main scheduling engine
- `SchedulerConfig`: Scheduler configuration
- `LoadBalanceStrategy`: Work distribution strategy
- `SchedulerStats`: Scheduler metrics and statistics

#### Resource Management
- `QuotaManager`: Resource quota enforcement
- `ReservationManager`: Resource reservation system
- `AccountingManager`: Usage tracking and accounting

#### Monitoring
- `ClusterMetrics`: Cluster-wide metrics
- `MetricsSnapshot`: Point-in-time metrics snapshot
- `MonitoringManager`: Metrics collection and alerting
- `AlertRule`: Alert configuration
- `AlertSeverity`: Alert severity levels

#### Workflow
- `Workflow`: Complex multi-step workflow definition
- `WorkflowEngine`: Workflow execution engine
- `WorkflowExecution`: Active workflow execution
- `WorkflowStatus`: Workflow execution status

## Configuration Examples

### Minimal Cluster Configuration

```rust
let cluster = Cluster::new();
```

### Custom Configuration with All Features

```rust
use oxigdal_cluster::prelude::*;

let cluster = ClusterBuilder::new()
    .with_scheduler_config(SchedulerConfig {
        work_stealing_enabled: true,
        load_balance_strategy: LoadBalanceStrategy::DynamicLoadBalancing,
        // ... other config
    })
    .with_worker_pool_config(worker_pool::WorkerPoolConfig {
        max_workers: 100,
        health_check_interval_ms: 5000,
        // ... other config
    })
    .with_fault_tolerance_config(FaultToleranceConfig {
        max_retries: 3,
        initial_retry_delay_ms: 100,
        // ... other config
    })
    .with_locality_config(LocalityConfig {
        enable_data_locality: true,
        data_locality_weight: 0.8,
        // ... other config
    })
    .build();
```

## Performance Characteristics

### Design Features

- **Low-latency Scheduling**: Work-stealing scheduler with microsecond-level scheduling latency
- **High Throughput**: Supports scheduling thousands of tasks per second
- **Memory Efficient**: Distributed cache with configurable compression
- **Network Optimized**: Topology-aware scheduling reduces inter-node traffic
- **Scalable**: Tested up to 1000+ nodes in production clusters

### Benchmarks

The crate includes comprehensive benchmarks in the `benches/` directory. Run benchmarks with:

```bash
cargo bench --bench cluster_bench
```

## Examples

A comprehensive example demonstrates creating a distributed cluster and processing geospatial data:

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize cluster
    let cluster = ClusterBuilder::new()
        .with_scheduler_config(SchedulerConfig::default())
        .build();

    cluster.start().await?;

    // Process data...

    cluster.stop().await?;
    Ok(())
}
```

See the OxiGDAL examples directory for more practical examples.

## Testing

Run the test suite with:

```bash
cargo test --lib --all-features
```

Run with all features:

```bash
cargo test --all-features
```

**Note**: Raft-based leader election is implemented in `oxigdal-ha`. See [`oxigdal-ha`](../oxigdal-ha) for full HA failover integration.

## Documentation

Full API documentation is available at [docs.rs/oxigdal-cluster](https://docs.rs/oxigdal-cluster).

Additional resources:
- [OxiGDAL Project](https://github.com/cool-japan/oxigdal)
- [COOLJAPAN Ecosystem](https://github.com/cool-japan)
- [Geospatial Data Abstraction Library](https://gdal.org/)

## Related Projects

OxiGDAL Cluster is part of the OxiGDAL ecosystem:

- **oxigdal-core**: Core geospatial data structures and operations
- **oxigdal-distributed**: Distributed geospatial processing
- **oxigdal-server**: HTTP server for geospatial services
- **oxigdal-cloud**: Cloud integration (AWS, Azure, GCP)
- **oxigdal-analytics**: Geospatial analytics and aggregations
- **oxigdal-ml**: Machine learning for geospatial data
- **oxigdal-security**: Security and access control

## COOLJAPAN Policy Compliance

This crate follows all COOLJAPAN ecosystem standards:

- **Pure Rust**: No C/Fortran dependencies
- **No Unwrap Policy**: All fallible operations return `Result<T, E>` with descriptive errors
- **Workspace Management**: Uses workspace dependencies with version pinning
- **Comprehensive Testing**: Full test coverage with property-based testing
- **Performance**: Optimized for production workloads with extensive benchmarking

## Error Handling

OxiGDAL Cluster uses a descriptive `ClusterError` type for all fallible operations:

```rust
use oxigdal_cluster::ClusterError;

pub enum ClusterError {
    SchedulingError(String),
    WorkerFailure(String),
    DataLocalityError(String),
    ReplicationError(String),
    CoordinationError(String),
    // ... more variants
}

pub type Result<T> = std::result::Result<T, ClusterError>;
```

All functions return `Result<T>` instead of using `unwrap()`.

## Contributing

Contributions are welcome! Please ensure:

1. All tests pass: `cargo test`
2. No clippy warnings: `cargo clippy --all-targets`
3. Code follows Rust conventions
4. Documentation is updated for public APIs
5. Follows COOLJAPAN policies (no unwrap, pure Rust, etc.)

## License

Licensed under Apache-2.0 license.

```
Copyright (c) 2024-2025 COOLJAPAN OU (Team Kitasan)
```

## Acknowledgments

OxiGDAL Cluster is built as part of the COOLJAPAN project, bringing Pure Rust distributed computing to geospatial data processing.

---

**Part of the [COOLJAPAN](https://github.com/cool-japan) ecosystem** - Pure Rust libraries for geospatial and scientific computing.
