//! Pipeline builder for constructing ETL workflows
//!
//! This module provides a fluent API for building ETL pipelines with sources,
//! transformations, and sinks.

use crate::error::{PipelineError, Result};
use crate::sink::Sink;
use crate::source::Source;
use crate::stream::{StateManager, StreamConfig};
use crate::transform::Transform;
use futures::StreamExt;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Pipeline execution mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    /// Batch mode (process all data and stop)
    Batch,
    /// Streaming mode (continuous processing)
    Streaming,
}

/// Pipeline configuration
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Pipeline ID for tracking and checkpointing
    pub id: String,
    /// Stream configuration
    pub stream: StreamConfig,
    /// Execution mode
    pub mode: ExecutionMode,
    /// Enable error recovery
    pub error_recovery: bool,
    /// Maximum retries on failure
    pub max_retries: usize,
    /// Checkpoint directory
    pub checkpoint_dir: Option<PathBuf>,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            stream: StreamConfig::default(),
            mode: ExecutionMode::Batch,
            error_recovery: false,
            max_retries: 3,
            checkpoint_dir: None,
        }
    }
}

/// Pipeline builder
pub struct PipelineBuilder {
    config: PipelineConfig,
    source: Option<Box<dyn Source>>,
    transforms: Vec<Box<dyn Transform>>,
    sink: Option<Box<dyn Sink>>,
}

impl PipelineBuilder {
    /// Create a new pipeline builder
    pub fn new() -> Self {
        Self {
            config: PipelineConfig::default(),
            source: None,
            transforms: Vec::new(),
            sink: None,
        }
    }

    /// Set pipeline ID
    pub fn id(mut self, id: String) -> Self {
        self.config.id = id;
        self
    }

    /// Set stream configuration
    pub fn stream_config(mut self, config: StreamConfig) -> Self {
        self.config.stream = config;
        self
    }

    /// Set execution mode
    pub fn mode(mut self, mode: ExecutionMode) -> Self {
        self.config.mode = mode;
        self
    }

    /// Enable checkpointing
    pub fn with_checkpointing(mut self) -> Self {
        self.config.stream.checkpointing = true;
        self.config.checkpoint_dir = Some(PathBuf::from("/tmp/oxigdal-checkpoints"));
        self
    }

    /// Set checkpoint directory
    pub fn checkpoint_dir(mut self, dir: PathBuf) -> Self {
        self.config.checkpoint_dir = Some(dir);
        self.config.stream.checkpointing = true;
        self
    }

    /// Enable error recovery
    pub fn with_error_recovery(mut self, max_retries: usize) -> Self {
        self.config.error_recovery = true;
        self.config.max_retries = max_retries;
        self
    }

    /// Set buffer size
    pub fn buffer_size(mut self, size: usize) -> Self {
        self.config.stream.buffer_size = size;
        self
    }

    /// Set maximum parallelism
    pub fn max_parallelism(mut self, parallelism: usize) -> Self {
        self.config.stream.max_parallelism = parallelism;
        self
    }

    /// Set data source
    pub fn source(mut self, source: Box<dyn Source>) -> Self {
        self.source = Some(source);
        self
    }

    /// Add a transformation
    pub fn transform(mut self, transform: Box<dyn Transform>) -> Self {
        self.transforms.push(transform);
        self
    }

    /// Add a map transformation
    pub fn map<F>(self, name: String, f: F) -> Self
    where
        F: Fn(Vec<u8>) -> std::pin::Pin<Box<dyn futures::Future<Output = Result<Vec<u8>>> + Send>>
            + Send
            + Sync
            + 'static,
    {
        use crate::transform::MapTransform;
        self.transform(Box::new(MapTransform::new(name, f)))
    }

    /// Add a filter transformation
    pub fn filter<F>(self, name: String, f: F) -> Self
    where
        F: Fn(&Vec<u8>) -> std::pin::Pin<Box<dyn futures::Future<Output = Result<bool>> + Send>>
            + Send
            + Sync
            + 'static,
    {
        use crate::transform::FilterTransform;
        self.transform(Box::new(FilterTransform::new(name, f)))
    }

    /// Set data sink
    pub fn sink(mut self, sink: Box<dyn Sink>) -> Self {
        self.sink = Some(sink);
        self
    }

    /// Validate pipeline configuration
    fn validate(&self) -> Result<()> {
        if self.source.is_none() {
            return Err(PipelineError::NoSource.into());
        }

        if self.sink.is_none() {
            return Err(PipelineError::NoSink.into());
        }

        Ok(())
    }

    /// Build the pipeline
    pub fn build(self) -> Result<Pipeline> {
        self.validate()?;

        Ok(Pipeline {
            config: self.config,
            source: self.source.ok_or(PipelineError::NoSource)?,
            transforms: self.transforms,
            sink: self.sink.ok_or(PipelineError::NoSink)?,
        })
    }
}

impl Default for PipelineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// ETL Pipeline
pub struct Pipeline {
    config: PipelineConfig,
    source: Box<dyn Source>,
    transforms: Vec<Box<dyn Transform>>,
    sink: Box<dyn Sink>,
}

impl Pipeline {
    /// Create a new pipeline builder
    pub fn builder() -> PipelineBuilder {
        PipelineBuilder::new()
    }

    /// Run the pipeline in batch mode
    pub async fn run(self) -> Result<PipelineStats> {
        info!(
            "Starting pipeline '{}' in {:?} mode",
            self.config.id, self.config.mode
        );

        let stats = PipelineStats::new();
        let state_manager = Arc::new(StateManager::new(self.config.checkpoint_dir.clone()));

        // Load checkpoint if enabled
        if self.config.stream.checkpointing {
            info!("Loading checkpoint for pipeline '{}'", self.config.id);
            state_manager
                .load_checkpoint(&self.config.id)
                .await
                .map_err(|e| {
                    warn!("Failed to load checkpoint: {}", e);
                    e
                })
                .ok();
        }

        // Create stream from source
        let mut stream = self.source.stream().await?;
        debug!("Source stream created: {}", self.source.name());

        // Process items
        let mut items_processed = 0usize;
        let mut items_filtered = 0usize;

        while let Some(item_result) = stream.next().await {
            let item = match item_result {
                Ok(item) => item,
                Err(e) => {
                    error!("Source error: {}", e);
                    if self.config.error_recovery {
                        stats.record_error();
                        continue;
                    } else {
                        return Err(e);
                    }
                }
            };

            // Apply transformations
            let mut items = vec![item];
            for transform in &self.transforms {
                let mut new_items = Vec::new();
                for item in items {
                    match transform.transform(item).await {
                        Ok(results) => new_items.extend(results),
                        Err(e) => {
                            error!("Transform error in '{}': {}", transform.name(), e);
                            if self.config.error_recovery {
                                stats.record_error();
                                continue;
                            } else {
                                return Err(e);
                            }
                        }
                    }
                }

                items = new_items;
                if items.is_empty() {
                    items_filtered += 1;
                    break;
                }
            }

            // Write to sink
            for item in items {
                if let Err(e) = self.sink.write(item).await {
                    error!("Sink error: {}", e);
                    if self.config.error_recovery {
                        stats.record_error();
                        continue;
                    } else {
                        return Err(e);
                    }
                }

                items_processed += 1;
                stats.record_item();

                // Checkpoint if needed
                if self.config.stream.checkpointing
                    && items_processed % self.config.stream.checkpoint_interval == 0
                {
                    debug!("Creating checkpoint at {} items", items_processed);
                    state_manager
                        .save_checkpoint(&self.config.id)
                        .await
                        .map_err(|e| {
                            warn!("Failed to save checkpoint: {}", e);
                            e
                        })
                        .ok();
                }
            }
        }

        // Flush sink
        self.sink.flush().await?;

        // Final checkpoint
        if self.config.stream.checkpointing {
            state_manager.save_checkpoint(&self.config.id).await?;
        }

        info!(
            "Pipeline '{}' completed: {} items processed, {} filtered, {} errors",
            self.config.id,
            items_processed,
            items_filtered,
            stats.errors()
        );

        Ok(stats)
    }

    /// Run the pipeline in streaming mode (continuous)
    pub async fn run_streaming(self) -> Result<()> {
        info!("Starting pipeline '{}' in streaming mode", self.config.id);

        loop {
            match self.run_once().await {
                Ok(_) => {
                    if self.config.mode == ExecutionMode::Batch {
                        break;
                    }
                }
                Err(e) => {
                    error!("Pipeline error: {}", e);
                    if !self.config.error_recovery {
                        return Err(e);
                    }
                }
            }

            // Small delay before restarting
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }

        Ok(())
    }

    /// Run the pipeline once
    async fn run_once(&self) -> Result<()> {
        let mut stream = self.source.stream().await?;

        while let Some(item_result) = stream.next().await {
            let item = item_result?;

            let mut items = vec![item];
            for transform in &self.transforms {
                let mut new_items = Vec::new();
                for item in items {
                    let results = transform.transform(item).await?;
                    new_items.extend(results);
                }
                items = new_items;
            }

            for item in items {
                self.sink.write(item).await?;
            }
        }

        self.sink.flush().await?;
        Ok(())
    }
}

/// Pipeline execution statistics
#[derive(Debug, Clone)]
pub struct PipelineStats {
    items_processed: Arc<std::sync::atomic::AtomicUsize>,
    errors: Arc<std::sync::atomic::AtomicUsize>,
    start_time: std::time::Instant,
}

impl PipelineStats {
    fn new() -> Self {
        Self {
            items_processed: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            errors: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            start_time: std::time::Instant::now(),
        }
    }

    fn record_item(&self) {
        self.items_processed
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    fn record_error(&self) {
        self.errors
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// Get number of items processed
    pub fn items_processed(&self) -> usize {
        self.items_processed
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Get number of errors
    pub fn errors(&self) -> usize {
        self.errors.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Get elapsed time
    pub fn elapsed(&self) -> std::time::Duration {
        self.start_time.elapsed()
    }

    /// Get throughput (items per second)
    pub fn throughput(&self) -> f64 {
        let elapsed_secs = self.elapsed().as_secs_f64();
        if elapsed_secs > 0.0 {
            self.items_processed() as f64 / elapsed_secs
        } else {
            0.0
        }
    }
}

// Helper to generate UUIDs
mod uuid {
    pub struct Uuid;

    impl Uuid {
        pub fn new_v4() -> Self {
            Self
        }
    }

    impl std::fmt::Display for Uuid {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            // Simple UUID generation for testing
            use std::time::{SystemTime, UNIX_EPOCH};
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            write!(f, "pipeline-{:x}", nanos)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sink::FileSink;
    use crate::source::FileSource;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_pipeline_builder() {
        let mut temp_input = NamedTempFile::new().expect("Failed to create temp file");
        write!(temp_input, "test data").expect("Failed to write");
        let input_path = temp_input.path().to_path_buf();

        let temp_output = NamedTempFile::new().expect("Failed to create temp file");
        let output_path = temp_output.path().to_path_buf();

        let pipeline = Pipeline::builder()
            .source(Box::new(FileSource::new(input_path)))
            .sink(Box::new(FileSink::new(output_path.clone())))
            .build();

        assert!(pipeline.is_ok());
    }

    #[tokio::test]
    async fn test_pipeline_validation() {
        let result = Pipeline::builder().build();
        assert!(result.is_err()); // No source or sink
    }

    #[tokio::test]
    async fn test_pipeline_stats() {
        let stats = PipelineStats::new();
        assert_eq!(stats.items_processed(), 0);
        assert_eq!(stats.errors(), 0);

        stats.record_item();
        stats.record_item();
        assert_eq!(stats.items_processed(), 2);

        stats.record_error();
        assert_eq!(stats.errors(), 1);
    }
}
