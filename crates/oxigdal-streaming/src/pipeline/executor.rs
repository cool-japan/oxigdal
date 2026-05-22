//! Pipeline executor for running streaming pipelines.

use super::builder::PipelineConfig;
use super::stage::{PipelineStage, StageResult};
use crate::error::{Result, StreamingError};
use crate::pipeline::zerocopy::ZeroCopyBuffer;
use bytes::Bytes;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info};

/// Statistics about a completed pipeline execution.
#[derive(Debug, Clone)]
pub struct ExecutionStats {
    /// Total number of items processed
    pub items_processed: usize,

    /// Total bytes processed
    pub bytes_processed: usize,

    /// Total execution time in milliseconds
    pub total_time_ms: u64,

    /// Average time per item in milliseconds
    pub avg_time_per_item_ms: f64,
}

/// Executes a pipeline of stages on a stream of data.
pub struct PipelineExecutor {
    config: PipelineConfig,
    stages: Vec<Arc<dyn PipelineStage>>,
}

impl PipelineExecutor {
    /// Create a new pipeline executor.
    pub async fn new(config: PipelineConfig, stages: Vec<Arc<dyn PipelineStage>>) -> Result<Self> {
        if stages.is_empty() {
            return Err(StreamingError::ConfigError(
                "Pipeline must have at least one stage".to_string(),
            ));
        }

        // Initialize all stages
        for stage in &stages {
            stage.initialize().await?;
        }

        info!("Created pipeline executor with {} stages", stages.len());

        Ok(Self { config, stages })
    }

    /// Process a single item through all pipeline stages.
    pub async fn process(&self, data: Bytes) -> Result<StageResult> {
        let start = Instant::now();
        let mut current = ZeroCopyBuffer::new(data);

        for stage in &self.stages {
            debug!("Executing stage: {}", stage.name());
            let result = stage.process(current).await?;
            current = result.data;
        }

        let elapsed_ms = start.elapsed().as_millis() as u64;
        Ok(StageResult::new(current, elapsed_ms))
    }

    /// Process multiple items and return aggregated statistics.
    pub async fn process_batch(&self, items: Vec<Bytes>) -> Result<ExecutionStats> {
        let start = Instant::now();
        let mut total_bytes = 0usize;
        let item_count = items.len();

        for item in items {
            let byte_len = item.len();
            self.process(item).await?;
            total_bytes += byte_len;
        }

        let total_ms = start.elapsed().as_millis() as u64;
        let avg_ms = if item_count > 0 {
            total_ms as f64 / item_count as f64
        } else {
            0.0
        };

        let stats = ExecutionStats {
            items_processed: item_count,
            bytes_processed: total_bytes,
            total_time_ms: total_ms,
            avg_time_per_item_ms: avg_ms,
        };

        info!(
            "Processed {} items ({} bytes) in {}ms",
            stats.items_processed, stats.bytes_processed, stats.total_time_ms
        );

        Ok(stats)
    }

    /// Finalize and close the pipeline.
    pub async fn finalize(self) -> Result<()> {
        for stage in &self.stages {
            stage.finalize().await?;
        }
        info!("Pipeline finalized");
        Ok(())
    }

    /// Get the pipeline configuration.
    pub fn config(&self) -> &PipelineConfig {
        &self.config
    }

    /// Get the number of stages.
    pub fn stage_count(&self) -> usize {
        self.stages.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::stage::TransformStage;
    use bytes::Bytes;

    #[tokio::test]
    async fn test_pipeline_executor() {
        let config = PipelineConfig::default();
        // TransformStage: Fn(&[u8]) -> Result<Vec<u8>>
        let stage = Arc::new(TransformStage::new(
            "identity".to_string(),
            |data: &[u8]| Ok(data.to_vec()),
        ));

        let executor = PipelineExecutor::new(config, vec![stage])
            .await
            .expect("executor should be created");

        let data = Bytes::from(vec![1u8, 2, 3, 4, 5]);
        let result = executor
            .process(data)
            .await
            .expect("process should succeed");

        assert_eq!(result.bytes_processed, 5);
    }
}
