//! Pipeline stage definitions.

use super::zerocopy::ZeroCopyBuffer;
use crate::error::{Result, StreamingError};
use async_trait::async_trait;
use std::time::Instant;

/// Result of a pipeline stage execution.
#[derive(Debug)]
pub struct StageResult {
    /// The output data
    pub data: ZeroCopyBuffer,

    /// Time taken to execute in milliseconds
    pub execution_time_ms: u64,

    /// Number of bytes processed
    pub bytes_processed: usize,

    /// Stage-specific metadata
    pub metadata: std::collections::HashMap<String, String>,
}

impl StageResult {
    /// Create a new stage result.
    pub fn new(data: ZeroCopyBuffer, execution_time_ms: u64) -> Self {
        let bytes_processed = data.len();
        Self {
            data,
            execution_time_ms,
            bytes_processed,
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Add metadata to the result.
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

/// A stage in a streaming pipeline.
#[async_trait]
pub trait PipelineStage: Send + Sync {
    /// Get the name of this stage.
    fn name(&self) -> &str;

    /// Process data through this stage.
    async fn process(&self, input: ZeroCopyBuffer) -> Result<StageResult>;

    /// Initialize the stage (optional).
    async fn initialize(&self) -> Result<()> {
        Ok(())
    }

    /// Finalize the stage (optional).
    async fn finalize(&self) -> Result<()> {
        Ok(())
    }
}

/// A simple transformation stage.
pub struct TransformStage<F>
where
    F: Fn(&[u8]) -> Result<Vec<u8>> + Send + Sync,
{
    name: String,
    transform_fn: F,
}

impl<F> TransformStage<F>
where
    F: Fn(&[u8]) -> Result<Vec<u8>> + Send + Sync,
{
    /// Create a new transform stage.
    pub fn new(name: String, transform_fn: F) -> Self {
        Self {
            name,
            transform_fn,
        }
    }
}

#[async_trait]
impl<F> PipelineStage for TransformStage<F>
where
    F: Fn(&[u8]) -> Result<Vec<u8>> + Send + Sync,
{
    fn name(&self) -> &str {
        &self.name
    }

    async fn process(&self, input: ZeroCopyBuffer) -> Result<StageResult> {
        let start = Instant::now();

        let output = (self.transform_fn)(input.as_ref())?;
        let output_buffer = ZeroCopyBuffer::new(bytes::Bytes::from(output));

        let elapsed = start.elapsed().as_millis() as u64;

        Ok(StageResult::new(output_buffer, elapsed))
    }
}

/// A filter stage that selectively passes data.
pub struct FilterStage<F>
where
    F: Fn(&[u8]) -> bool + Send + Sync,
{
    name: String,
    filter_fn: F,
}

impl<F> FilterStage<F>
where
    F: Fn(&[u8]) -> bool + Send + Sync,
{
    /// Create a new filter stage.
    pub fn new(name: String, filter_fn: F) -> Self {
        Self { name, filter_fn }
    }
}

#[async_trait]
impl<F> PipelineStage for FilterStage<F>
where
    F: Fn(&[u8]) -> bool + Send + Sync,
{
    fn name(&self) -> &str {
        &self.name
    }

    async fn process(&self, input: ZeroCopyBuffer) -> Result<StageResult> {
        let start = Instant::now();

        if (self.filter_fn)(input.as_ref()) {
            let elapsed = start.elapsed().as_millis() as u64;
            Ok(StageResult::new(input, elapsed))
        } else {
            Err(StreamingError::Other("Filtered out".to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    #[tokio::test]
    async fn test_transform_stage() {
        let stage = TransformStage::new(
            "double".to_string(),
            |data| Ok(data.iter().map(|&x| x * 2).collect()),
        );

        let input = ZeroCopyBuffer::new(Bytes::from(vec![1, 2, 3]));
        let result = stage.process(input).await.ok();

        assert!(result.is_some());
        if let Some(result) = result {
            assert_eq!(result.data.as_ref(), &[2, 4, 6]);
        }
    }

    #[tokio::test]
    async fn test_filter_stage() {
        let stage = FilterStage::new(
            "non_empty".to_string(),
            |data| !data.is_empty(),
        );

        let input = ZeroCopyBuffer::new(Bytes::from(vec![1, 2, 3]));
        let result = stage.process(input).await;
        assert!(result.is_ok());

        let empty = ZeroCopyBuffer::new(Bytes::new());
        let result = stage.process(empty).await;
        assert!(result.is_err());
    }
}
