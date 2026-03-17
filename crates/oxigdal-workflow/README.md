# oxigdal-workflow

[![Crates.io](https://img.shields.io/crates/v/oxigdal-workflow.svg)](https://crates.io/crates/oxigdal-workflow)
[![Documentation](https://docs.rs/oxigdal-workflow/badge.svg)](https://docs.rs/oxigdal-workflow)
[![License](https://img.shields.io/crates/l/oxigdal-workflow.svg)](LICENSE)
[![Pure Rust](https://img.shields.io/badge/language-Pure%20Rust-ce3262)](https://www.rust-lang.org/)

A DAG-based workflow engine for complex geospatial processing pipelines. Orchestrate multi-stage geospatial workflows with automatic parallelization, flexible scheduling, state persistence, and comprehensive monitoring.

## Overview

`oxigdal-workflow` provides a production-grade workflow orchestration system designed for geospatial data processing. Build complex pipelines with directed acyclic graphs (DAGs), execute tasks in parallel, handle failures gracefully, and monitor execution with detailed metrics.

## Features

- **DAG-Based Execution**: Define workflows as directed acyclic graphs for automatic dependency resolution and parallelization
- **Flexible Scheduling**: Cron-based, interval-based, and event-driven task scheduling with timezone support
- **Automatic Parallelization**: Intelligent task scheduling maximizes parallelism based on task dependencies
- **Retry Logic & Resilience**: Configurable retry policies with exponential backoff and failure recovery
- **State Persistence**: Save and restore workflow execution state for fault tolerance and recovery
- **Conditional Execution**: Dynamic branching and conditional task execution based on previous results
- **Workflow Templates**: Reusable workflow templates with parameterization and versioning
- **Comprehensive Monitoring**: Detailed execution metrics, logging, and debugging capabilities
- **Resource Management**: Resource requirement specification and tracking with pool-based scheduling
- **External Integrations**: Adapters for Airflow, Prefect, Temporal, and HTTP webhooks (with `integrations` feature)
- **HTTP Server**: RESTful API for workflow management and monitoring (with `server` feature)
- **Pure Rust**: 100% Pure Rust implementation with no C/Fortran dependencies

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
oxigdal-workflow = "0.1.2"

# For integrations (HTTP, Kafka)
oxigdal-workflow = { version = "0.1.2", features = ["integrations"] }

# For HTTP server support
oxigdal-workflow = { version = "0.1.2", features = ["server"] }

# For all features
oxigdal-workflow = { version = "0.1.2", features = ["full"] }
```

## Quick Start

### Basic Workflow Definition

```rust
use oxigdal_workflow::{
    WorkflowDefinition,
    dag::{TaskNode, WorkflowDag, RetryPolicy, ResourceRequirements},
};
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a new DAG
    let mut dag = WorkflowDag::new();

    // Define tasks
    let task1 = TaskNode {
        id: "fetch_data".to_string(),
        name: "Fetch Data".to_string(),
        description: Some("Fetch input data".to_string()),
        config: serde_json::json!({"source": "S3"}),
        retry: RetryPolicy::default(),
        timeout_secs: Some(300),
        resources: ResourceRequirements::default(),
        metadata: HashMap::new(),
    };

    let task2 = TaskNode {
        id: "process_data".to_string(),
        name: "Process Data".to_string(),
        description: Some("Process and transform data".to_string()),
        config: serde_json::json!({"algorithm": "NDVI"}),
        retry: RetryPolicy::default(),
        timeout_secs: Some(600),
        resources: ResourceRequirements::default(),
        metadata: HashMap::new(),
    };

    // Add tasks to DAG
    dag.add_task(task1)?;
    dag.add_task(task2)?;

    // Define dependencies
    dag.add_dependency("fetch_data", "process_data", Default::default())?;

    // Create workflow definition
    let workflow = WorkflowDefinition {
        id: "geospatial-pipeline".to_string(),
        name: "Geospatial Processing Pipeline".to_string(),
        description: Some("Process satellite imagery".to_string()),
        version: "1.0.0".to_string(),
        dag,
    };

    println!("Workflow defined: {}", workflow.name);
    Ok(())
}
```

### Workflow Scheduling

```rust
use oxigdal_workflow::{
    WorkflowDefinition,
    scheduler::{Scheduler, ScheduleType, SchedulerConfig},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create scheduler with custom configuration
    let config = SchedulerConfig {
        max_concurrent_executions: 5,
        handle_missed_executions: true,
        max_missed_executions: 3,
        execution_timeout_secs: 3600,
        enable_persistence: true,
        persistence_path: Some("./workflow_state".to_string()),
        tick_interval_ms: 100,
        timezone: "UTC".to_string(),
    };

    let scheduler = Scheduler::new(config);

    // Schedule workflow to run daily at midnight
    let schedule_id = scheduler
        .add_schedule(
            workflow,
            ScheduleType::Cron {
                expression: "0 0 * * *".to_string(),
            },
        )
        .await?;

    println!("Workflow scheduled with ID: {}", schedule_id);

    // Or schedule with fixed interval (every hour)
    let schedule_id = scheduler
        .add_schedule(
            workflow,
            ScheduleType::Interval {
                interval_secs: 3600,
            },
        )
        .await?;

    println!("Workflow scheduled with ID: {}", schedule_id);

    Ok(())
}
```

### Parallel Execution Planning

```rust
use oxigdal_workflow::dag::{WorkflowDag, create_execution_plan};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dag = WorkflowDag::new();
    // ... add tasks and dependencies ...

    // Create execution plan with parallelization levels
    let execution_plan = create_execution_plan(&dag)?;

    // Tasks in same level can execute in parallel
    for (level_idx, level) in execution_plan.iter().enumerate() {
        println!("Level {}: {:?} (can run in parallel)", level_idx, level);
    }

    Ok(())
}
```

## Usage Examples

### Example 1: Batch Processing Workflow

```rust
use oxigdal_workflow::{
    WorkflowDefinition,
    dag::{TaskNode, WorkflowDag, TaskEdge},
    monitoring::MonitoringService,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut dag = WorkflowDag::new();

    // Source task
    dag.add_task(TaskNode {
        id: "list_files".to_string(),
        name: "List Input Files".to_string(),
        ..Default::default()
    })?;

    // Parallel processing tasks
    for i in 0..8 {
        dag.add_task(TaskNode {
            id: format!("process_batch_{}", i),
            name: format!("Process Batch {}", i),
            ..Default::default()
        })?;
    }

    // Merge task
    dag.add_task(TaskNode {
        id: "merge_results".to_string(),
        name: "Merge Results".to_string(),
        ..Default::default()
    })?;

    // Create dependencies for parallel execution
    for i in 0..8 {
        dag.add_dependency("list_files", &format!("process_batch_{}", i), TaskEdge::default())?;
        dag.add_dependency(&format!("process_batch_{}", i), "merge_results", TaskEdge::default())?;
    }

    // Create workflow
    let workflow = WorkflowDefinition {
        id: "batch-processing".to_string(),
        name: "Batch Processing Workflow".to_string(),
        description: Some("Process multiple files in parallel".to_string()),
        version: "1.0.0".to_string(),
        dag,
    };

    // Set up monitoring
    let monitoring = MonitoringService::new();
    println!("Monitoring initialized");

    Ok(())
}
```

### Example 2: Satellite Imagery Processing

```rust
use oxigdal_workflow::{
    WorkflowDefinition,
    dag::{TaskNode, WorkflowDag},
    scheduler::{Scheduler, ScheduleType},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut dag = WorkflowDag::new();

    // Define processing pipeline
    dag.add_task(TaskNode {
        id: "download_imagery".to_string(),
        name: "Download Imagery".to_string(),
        ..Default::default()
    })?;

    dag.add_task(TaskNode {
        id: "atmospheric_correction".to_string(),
        name: "Atmospheric Correction".to_string(),
        ..Default::default()
    })?;

    dag.add_task(TaskNode {
        id: "cloud_masking".to_string(),
        name: "Cloud Masking".to_string(),
        ..Default::default()
    })?;

    dag.add_task(TaskNode {
        id: "calculate_ndvi".to_string(),
        name: "Calculate NDVI".to_string(),
        ..Default::default()
    })?;

    dag.add_task(TaskNode {
        id: "export_results".to_string(),
        name: "Export Results".to_string(),
        ..Default::default()
    })?;

    // Create linear pipeline
    let steps = vec![
        ("download_imagery", "atmospheric_correction"),
        ("atmospheric_correction", "cloud_masking"),
        ("cloud_masking", "calculate_ndvi"),
        ("calculate_ndvi", "export_results"),
    ];

    for (from, to) in steps {
        dag.add_dependency(from, to, Default::default())?;
    }

    let workflow = WorkflowDefinition {
        id: "satellite-processing".to_string(),
        name: "Satellite Imagery Processing".to_string(),
        description: Some("Process Sentinel-2 satellite imagery".to_string()),
        version: "1.0.0".to_string(),
        dag,
    };

    // Schedule for daily execution
    let scheduler = Scheduler::with_defaults();
    let schedule_id = scheduler
        .add_schedule(
            workflow,
            ScheduleType::Cron {
                expression: "0 0 * * *".to_string(), // Daily at midnight
            },
        )
        .await?;

    println!("Scheduled workflow with ID: {}", schedule_id);
    Ok(())
}
```

## API Overview

### Core Modules

| Module | Description |
|--------|-------------|
| `engine` | Workflow runtime, execution, and state management |
| `dag` | DAG construction, validation, and execution planning |
| `scheduler` | Cron and interval-based scheduling system |
| `monitoring` | Execution metrics, history, and observability |
| `conditional` | Conditional branching and dynamic execution |
| `templates` | Workflow templates and reusable patterns |
| `versioning` | Version management and backward compatibility |
| `integrations` | External system adapters (Airflow, Prefect, Kafka) |
| `error` | Error types and result handling |

### Key Types

#### WorkflowDefinition
```rust
pub struct WorkflowDefinition {
    pub id: String,                  // Unique workflow identifier
    pub name: String,                // Human-readable name
    pub description: Option<String>, // Detailed description
    pub version: String,             // Semantic version
    pub dag: WorkflowDag,           // Task dependency graph
}
```

#### TaskNode
```rust
pub struct TaskNode {
    pub id: String,                           // Task identifier
    pub name: String,                         // Human-readable name
    pub description: Option<String>,          // Task description
    pub config: serde_json::Value,           // Task configuration
    pub retry: RetryPolicy,                  // Retry behavior
    pub timeout_secs: Option<u64>,           // Execution timeout
    pub resources: ResourceRequirements,      // Resource requirements
    pub metadata: HashMap<String, String>,   // Custom metadata
}
```

#### WorkflowDag
```rust
impl WorkflowDag {
    pub fn new() -> Self;
    pub fn add_task(&mut self, task: TaskNode) -> Result<()>;
    pub fn add_dependency(&mut self, from: &str, to: &str, edge: TaskEdge) -> Result<()>;
    pub fn validate(&self) -> Result<()>;
    pub fn tasks(&self) -> Vec<&TaskNode>;
}
```

#### Scheduler
```rust
impl Scheduler {
    pub fn new(config: SchedulerConfig) -> Self;
    pub async fn add_schedule(&self, workflow: WorkflowDefinition, schedule: ScheduleType) -> Result<String>;
    pub fn get_schedules(&self) -> Vec<ScheduleInfo>;
    pub async fn remove_schedule(&self, id: &str) -> Result<()>;
}
```

#### MonitoringService
```rust
impl MonitoringService {
    pub fn new() -> Self;
    pub fn record_task_execution(&self, task_id: &str, status: TaskStatus, duration: Duration);
    pub fn get_metrics(&self) -> WorkflowMetrics;
    pub fn get_execution_history(&self, workflow_id: &str) -> Vec<ExecutionHistory>;
}
```

## Advanced Features

### Conditional Execution

```rust
use oxigdal_workflow::conditional::{ConditionalBranch, Expression, ExecutionDecision};

let condition = ConditionalBranch {
    id: "check_data_quality".to_string(),
    expression: Expression::GreaterThan("quality_score".to_string(), 0.8),
    true_task: "process_high_quality".to_string(),
    false_task: Some("process_low_quality".to_string()),
};
```

### Retry Policies

```rust
use oxigdal_workflow::dag::RetryPolicy;

let retry_policy = RetryPolicy {
    max_attempts: 3,
    initial_delay_ms: 1000,
    max_delay_ms: 30000,
    backoff_multiplier: 2.0,
    jitter_factor: 0.1,
};
```

### Resource Requirements

```rust
use oxigdal_workflow::dag::ResourceRequirements;

let resources = ResourceRequirements {
    cpu_cores: Some(4),
    memory_mb: Some(8192),
    gpu_count: Some(1),
    disk_gb: Some(50),
};
```

### Workflow Templates

```rust
use oxigdal_workflow::templates::{WorkflowTemplate, WorkflowTemplateLibrary};

let template = WorkflowTemplate {
    id: "geospatial_pipeline".to_string(),
    name: "Geospatial Pipeline Template".to_string(),
    description: Some("Reusable template for geospatial processing".to_string()),
    version: "1.0.0".to_string(),
    parameters: vec![
        ("source_url".to_string(), "string".to_string()),
        ("algorithm".to_string(), "enum(NDVI,EVI)".to_string()),
    ],
    workflow_definition: workflow,
};

let mut library = WorkflowTemplateLibrary::new();
library.add_template(template)?;
```

## Performance Characteristics

- **Task Execution Overhead**: <1ms per task (in-process scheduling)
- **DAG Validation**: O(V + E) where V=vertices, E=edges
- **Execution Planning**: O(V + E) topological sort
- **Parallel Scheduling**: Optimal for DAGs with high parallelism factor
- **State Persistence**: Configurable with optional disk-based durability

### Benchmark Results

| Operation | Time (avg) |
|-----------|-----------|
| DAG creation (100 tasks) | ~5ms |
| Execution plan generation | ~2ms |
| Task scheduling | <0.5ms |
| Metrics aggregation | ~1ms |

## Error Handling

All fallible operations return `Result<T, WorkflowError>` with comprehensive error types:

```rust
pub enum WorkflowError {
    DagError(String),              // DAG validation errors
    ScheduleError(String),         // Scheduling errors
    ExecutionError(String),        // Execution errors
    StateError(String),            // State management errors
    ConfigurationError(String),    // Configuration errors
    IntegrationError(String),      // Integration errors
}
```

## Examples

Complete working examples are available in the [examples](examples/) directory:

- **[batch_processing_workflow.rs](examples/batch_processing_workflow.rs)**: Multi-task batch processing with parallelization
- **[satellite_workflow.rs](examples/satellite_workflow.rs)**: Satellite imagery processing pipeline
- **[change_detection_workflow.rs](examples/change_detection_workflow.rs)**: Change detection between image sets

Run examples with:

```bash
cargo run --example batch_processing_workflow
cargo run --example satellite_workflow
cargo run --example change_detection_workflow
```

## Configuration

### SchedulerConfig

```rust
pub struct SchedulerConfig {
    pub max_concurrent_executions: usize,  // Max parallel workflow executions
    pub handle_missed_executions: bool,    // Handle missed cron executions
    pub max_missed_executions: usize,      // Max allowed missed runs
    pub execution_timeout_secs: u64,       // Global execution timeout
    pub enable_persistence: bool,          // Enable state persistence
    pub persistence_path: Option<String>,  // State file location
    pub tick_interval_ms: u64,             // Scheduler tick interval
    pub timezone: String,                  // Cron timezone
}
```

## Integration with OxiGDAL Ecosystem

`oxigdal-workflow` is part of the OxiGDAL ecosystem for geospatial data processing:

- **[oxigdal-core](https://github.com/cool-japan/oxigdal)**: Core geospatial types and operations
- **[oxigdal-raster](https://github.com/cool-japan/oxigdal)**: Raster data processing
- **[oxigdal-vector](https://github.com/cool-japan/oxigdal)**: Vector data processing
- **[oxigdal-io](https://github.com/cool-japan/oxigdal)**: Input/output operations

## Pure Rust

This library is **100% Pure Rust** with no C/Fortran dependencies. All functionality works out of the box without external binaries or system libraries. Optional features may integrate with external systems via HTTP or message queues, but core functionality remains pure Rust.

## Documentation

- **[API Documentation](https://docs.rs/oxigdal-workflow/)**: Full API reference with examples
- **[COOLJAPAN Guide](https://github.com/cool-japan/oxigdal)**: Geospatial processing guide
- **[Examples](examples/)**: Working examples for common use cases

## Contributing

Contributions are welcome! This project follows COOLJAPAN ecosystem standards:

- Pure Rust implementation required
- No unwrap() calls in production code
- Comprehensive error handling
- Full documentation with examples
- Tests for all public APIs

## License

Licensed under the Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0).

## Related Projects

- [OxiGDAL Core](https://github.com/cool-japan/oxigdal) - Core geospatial types and operations
- [OxiBLAS](https://github.com/cool-japan/oxiblas) - Pure Rust BLAS operations
- [OxiCode](https://github.com/cool-japan/oxicode) - Pure Rust serialization
- [SciRS2](https://github.com/cool-japan/scirs) - Scientific computing ecosystem

---

**Part of the [COOLJAPAN](https://github.com/cool-japan) Ecosystem - Pure Rust Geospatial Processing**
