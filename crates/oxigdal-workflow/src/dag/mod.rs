//! DAG (Directed Acyclic Graph) infrastructure for workflow execution.
//!
//! This module provides comprehensive DAG construction, validation, and execution planning
//! capabilities for the workflow engine.

pub mod graph;
pub mod parallelism;
pub mod topological_sort;

pub use graph::{
    DagSummary, EdgeType, ResourceRequirements, RetryPolicy, TaskEdge, TaskNode, WorkflowDag,
};
pub use parallelism::{
    AvailableResources, ExecutionWave, ParallelSchedule, ResourcePool, ResourceUtilization,
    create_parallel_schedule, optimize_schedule,
};
pub use topological_sort::{
    ExecutionLevel, ExecutionPlan, TopologicalOrder, calculate_depths, create_execution_plan,
    critical_path, find_all_paths, topological_sort,
};
