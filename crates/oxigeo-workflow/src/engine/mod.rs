//! Workflow engine core components.
//!
//! This module provides the core workflow execution engine, including:
//! - Workflow state management and persistence
//! - Task execution with retry logic
//! - Workflow runtime for managing multiple executions

pub mod executor;
pub mod runtime;
pub mod state;

pub use executor::{ExecutionContext, ExecutorConfig, TaskExecutor, TaskOutput, WorkflowExecutor};
pub use runtime::{WorkflowDefinition, WorkflowRuntime};
pub use state::{
    ExecutionContext as StateExecutionContext, StatePersistence, TaskState, TaskStatus,
    WorkflowMetadata, WorkflowState, WorkflowStatus,
};
