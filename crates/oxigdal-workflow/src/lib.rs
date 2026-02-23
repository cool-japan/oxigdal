//! # oxigdal-workflow
//!
//! DAG-based workflow engine for complex geospatial processing pipelines.
//!
//! This crate provides a comprehensive workflow orchestration system with:
//! - DAG-based execution with automatic parallelization
//! - Flexible scheduling (cron, event-driven, interval-based)
//! - State persistence and recovery from failures
//! - Conditional execution and branching
//! - Workflow templates and versioning
//! - Comprehensive monitoring and debugging tools
//! - External integrations (Airflow, Prefect, Temporal)
//!
//! ## Example
//!
//! ```rust,no_run
//! use oxigdal_workflow::{
//!     WorkflowRuntime, TaskExecutor, ExecutorConfig, ExecutionContext, TaskOutput, TaskNode,
//!     Result,
//! };
//! use async_trait::async_trait;
//!
//! #[derive(Clone)]
//! struct MyExecutor;
//!
//! #[async_trait]
//! impl TaskExecutor for MyExecutor {
//!     async fn execute(&self, _task: &TaskNode, _ctx: &ExecutionContext) -> Result<TaskOutput> {
//!         Ok(TaskOutput { data: None, logs: vec![] })
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//!     // Create a workflow runtime with default config
//!     let runtime = WorkflowRuntime::new(ExecutorConfig::default(), MyExecutor);
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Features
//!
//! - `default`: Enables all features
//! - `full`: Enables all features
//! - `integrations`: External workflow system integrations
//! - `server`: HTTP server for workflow management

#![warn(missing_docs)]
#![deny(clippy::unwrap_used, clippy::panic)]

pub mod conditional;
pub mod dag;
pub mod engine;
pub mod error;
pub mod integrations;
pub mod monitoring;
pub mod scheduler;
pub mod templates;
pub mod versioning;

// Re-export commonly used types
pub use conditional::{ConditionalBranch, ConditionalEvaluator, ExecutionDecision, Expression};
pub use dag::{TaskNode, WorkflowDag};
pub use engine::{
    ExecutionContext, ExecutorConfig, TaskExecutor, TaskOutput, WorkflowDefinition,
    WorkflowExecutor, WorkflowRuntime,
};
pub use error::{DagError, Result, WorkflowError};
pub use monitoring::{ExecutionHistory, WorkflowMetrics};
pub use scheduler::{CronSchedule, EventTrigger, Scheduler, SchedulerConfig};
pub use templates::{WorkflowTemplate, WorkflowTemplateLibrary};
pub use versioning::{WorkflowVersion, WorkflowVersionManager};
