//! Pipeline builder for constructing streaming pipelines.

use super::executor::PipelineExecutor;
use super::stage::PipelineStage;
use crate::error::{Result, StreamingError};
use std::sync::Arc;

/// Configuration for a pipeline.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Maximum number of concurrent stages
    pub max_concurrent_stages: usize,

    /// Buffer size between stages
    pub buffer_size: usize,

    /// Enable metrics collection
    pub enable_metrics: bool,

    /// Enable stage profiling
    pub enable_profiling: bool,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            max_concurrent_stages: num_cpus::get(),
            buffer_size: 100,
            enable_metrics: true,
            enable_profiling: false,
        }
    }
}

/// Builder for constructing streaming pipelines.
pub struct PipelineBuilder {
    config: PipelineConfig,
    stages: Vec<Arc<dyn PipelineStage>>,
}

impl PipelineBuilder {
    /// Create a new pipeline builder.
    pub fn new() -> Self {
        Self {
            config: PipelineConfig::default(),
            stages: Vec::new(),
        }
    }

    /// Create a new pipeline builder with config.
    pub fn with_config(config: PipelineConfig) -> Self {
        Self {
            config,
            stages: Vec::new(),
        }
    }

    /// Add a stage to the pipeline.
    pub fn add_stage(mut self, stage: Arc<dyn PipelineStage>) -> Self {
        self.stages.push(stage);
        self
    }

    /// Set the maximum number of concurrent stages.
    pub fn max_concurrent_stages(mut self, max: usize) -> Self {
        self.config.max_concurrent_stages = max;
        self
    }

    /// Set the buffer size between stages.
    pub fn buffer_size(mut self, size: usize) -> Self {
        self.config.buffer_size = size;
        self
    }

    /// Enable metrics collection.
    pub fn enable_metrics(mut self, enable: bool) -> Self {
        self.config.enable_metrics = enable;
        self
    }

    /// Enable stage profiling.
    pub fn enable_profiling(mut self, enable: bool) -> Self {
        self.config.enable_profiling = enable;
        self
    }

    /// Build the pipeline executor.
    pub async fn build(self) -> Result<PipelineExecutor> {
        if self.stages.is_empty() {
            return Err(StreamingError::ConfigError(
                "Pipeline must have at least one stage".to_string()
            ));
        }

        PipelineExecutor::new(self.config, self.stages).await
    }
}

impl Default for PipelineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::stage::TransformStage;

    #[test]
    fn test_pipeline_builder() {
        let builder = PipelineBuilder::new()
            .max_concurrent_stages(4)
            .buffer_size(50)
            .enable_metrics(true);

        assert_eq!(builder.config.max_concurrent_stages, 4);
        assert_eq!(builder.config.buffer_size, 50);
        assert_eq!(builder.config.enable_metrics, true);
    }

    #[tokio::test]
    async fn test_empty_pipeline() {
        let builder = PipelineBuilder::new();
        let result = builder.build().await;
        assert!(result.is_err());
    }
}
