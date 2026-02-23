//! Memory-efficient streaming pipelines.
//!
//! This module provides zero-copy and memory-efficient streaming pipelines
//! for processing large geospatial datasets.

pub mod builder;
pub mod executor;
pub mod stage;
pub mod zerocopy;

pub use builder::{PipelineBuilder, PipelineConfig};
pub use executor::{PipelineExecutor, ExecutionStats};
pub use stage::{PipelineStage, StageResult};
pub use zerocopy::{ZeroCopyBuffer, SharedBuffer};
