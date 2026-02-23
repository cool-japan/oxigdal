//! Stream processing primitives for ETL pipelines
//!
//! This module provides async stream processing capabilities with backpressure,
//! state management, checkpointing, and parallel processing.

use crate::error::{Result, StreamError};
use async_trait::async_trait;
use dashmap::DashMap;
use futures::stream::{Stream, StreamExt};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, Semaphore, mpsc};
use tokio::time::timeout;

/// A data item flowing through the stream
pub type StreamItem = Vec<u8>;

/// Boxed async stream
pub type BoxStream<T> = Pin<Box<dyn Stream<Item = Result<T>> + Send + 'static>>;

/// Stream configuration
#[derive(Debug, Clone)]
pub struct StreamConfig {
    /// Buffer size for channels
    pub buffer_size: usize,
    /// Backpressure timeout
    pub backpressure_timeout: Duration,
    /// Enable checkpointing
    pub checkpointing: bool,
    /// Checkpoint interval (number of items)
    pub checkpoint_interval: usize,
    /// Maximum parallelism
    pub max_parallelism: usize,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            buffer_size: 1000,
            backpressure_timeout: Duration::from_secs(30),
            checkpointing: false,
            checkpoint_interval: 1000,
            max_parallelism: num_cpus(),
        }
    }
}

/// Stream processor trait
#[async_trait]
pub trait StreamProcessor: Send + Sync {
    /// Process a single item
    async fn process(&self, item: StreamItem) -> Result<StreamItem>;

    /// Called when checkpoint is triggered
    async fn checkpoint(&self) -> Result<Vec<u8>> {
        Ok(Vec::new())
    }

    /// Restore from checkpoint
    async fn restore(&self, _state: &[u8]) -> Result<()> {
        Ok(())
    }
}

/// Buffered stream with backpressure
pub struct BufferedStream {
    config: StreamConfig,
    sender: mpsc::Sender<StreamItem>,
    receiver: Arc<RwLock<mpsc::Receiver<StreamItem>>>,
    items_processed: Arc<RwLock<usize>>,
    semaphore: Arc<Semaphore>,
}

impl BufferedStream {
    /// Create a new buffered stream
    pub fn new(config: StreamConfig) -> Self {
        let (sender, receiver) = mpsc::channel(config.buffer_size);
        Self {
            semaphore: Arc::new(Semaphore::new(config.buffer_size)),
            config,
            sender,
            receiver: Arc::new(RwLock::new(receiver)),
            items_processed: Arc::new(RwLock::new(0)),
        }
    }

    /// Push an item to the stream with backpressure
    pub async fn push(&self, item: StreamItem) -> Result<()> {
        // Acquire permit with timeout
        let permit = timeout(self.config.backpressure_timeout, self.semaphore.acquire())
            .await
            .map_err(|_| StreamError::BackpressureTimeout {
                duration: self.config.backpressure_timeout,
            })?
            .map_err(|_| StreamError::ChannelClosed)?;

        // Send item
        self.sender
            .send(item)
            .await
            .map_err(|_| StreamError::ChannelClosed)?;

        // Release permit
        permit.forget();

        // Update counter
        let mut count = self.items_processed.write().await;
        *count += 1;

        Ok(())
    }

    /// Pull an item from the stream
    pub async fn pull(&self) -> Result<Option<StreamItem>> {
        let mut receiver = self.receiver.write().await;
        Ok(receiver.recv().await)
    }

    /// Get number of items processed
    pub async fn items_processed(&self) -> usize {
        *self.items_processed.read().await
    }

    /// Check if checkpoint is needed
    pub async fn needs_checkpoint(&self) -> bool {
        if !self.config.checkpointing {
            return false;
        }
        let count = self.items_processed().await;
        count > 0 && count % self.config.checkpoint_interval == 0
    }
}

/// State manager for stream processing
pub struct StateManager {
    state: DashMap<String, Vec<u8>>,
    checkpoint_dir: Option<std::path::PathBuf>,
}

impl StateManager {
    /// Create a new state manager
    pub fn new(checkpoint_dir: Option<std::path::PathBuf>) -> Self {
        Self {
            state: DashMap::new(),
            checkpoint_dir,
        }
    }

    /// Set state for a key
    pub fn set(&self, key: String, value: Vec<u8>) {
        self.state.insert(key, value);
    }

    /// Get state for a key
    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        self.state.get(key).map(|v| v.clone())
    }

    /// Save checkpoint to disk
    pub async fn save_checkpoint(&self, pipeline_id: &str) -> Result<()> {
        let checkpoint_dir =
            self.checkpoint_dir
                .as_ref()
                .ok_or_else(|| StreamError::StateFailed {
                    message: "No checkpoint directory configured".to_string(),
                })?;

        tokio::fs::create_dir_all(checkpoint_dir).await?;

        let checkpoint_file = checkpoint_dir.join(format!("{}.checkpoint", pipeline_id));
        let mut data = Vec::new();

        for entry in self.state.iter() {
            let key_bytes = entry.key().as_bytes();
            data.extend_from_slice(&(key_bytes.len() as u32).to_le_bytes());
            data.extend_from_slice(key_bytes);
            data.extend_from_slice(&(entry.value().len() as u32).to_le_bytes());
            data.extend_from_slice(entry.value());
        }

        tokio::fs::write(checkpoint_file, data).await?;
        Ok(())
    }

    /// Load checkpoint from disk
    pub async fn load_checkpoint(&self, pipeline_id: &str) -> Result<()> {
        let checkpoint_dir =
            self.checkpoint_dir
                .as_ref()
                .ok_or_else(|| StreamError::StateFailed {
                    message: "No checkpoint directory configured".to_string(),
                })?;

        let checkpoint_file = checkpoint_dir.join(format!("{}.checkpoint", pipeline_id));
        if !checkpoint_file.exists() {
            return Ok(());
        }

        let data = tokio::fs::read(checkpoint_file).await?;
        let mut offset = 0;

        while offset < data.len() {
            if offset + 4 > data.len() {
                break;
            }

            let key_len = u32::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]) as usize;
            offset += 4;

            if offset + key_len > data.len() {
                break;
            }

            let key = String::from_utf8_lossy(&data[offset..offset + key_len]).to_string();
            offset += key_len;

            if offset + 4 > data.len() {
                break;
            }

            let value_len = u32::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]) as usize;
            offset += 4;

            if offset + value_len > data.len() {
                break;
            }

            let value = data[offset..offset + value_len].to_vec();
            offset += value_len;

            self.state.insert(key, value);
        }

        Ok(())
    }

    /// Clear all state
    pub fn clear(&self) {
        self.state.clear();
    }
}

/// Parallel stream processor
pub struct ParallelProcessor {
    config: StreamConfig,
    processor: Arc<dyn StreamProcessor>,
    state_manager: Arc<StateManager>,
    /// Pipeline identifier for checkpointing
    pipeline_id: String,
}

impl ParallelProcessor {
    /// Create a new parallel processor
    pub fn new(
        config: StreamConfig,
        processor: Arc<dyn StreamProcessor>,
        state_manager: Arc<StateManager>,
    ) -> Self {
        Self {
            config,
            processor,
            state_manager,
            pipeline_id: "default".to_string(),
        }
    }

    /// Create a new parallel processor with a specific pipeline ID
    pub fn with_pipeline_id(
        config: StreamConfig,
        processor: Arc<dyn StreamProcessor>,
        state_manager: Arc<StateManager>,
        pipeline_id: String,
    ) -> Self {
        Self {
            config,
            processor,
            state_manager,
            pipeline_id,
        }
    }

    /// Get the state manager for external access
    pub fn state_manager(&self) -> &Arc<StateManager> {
        &self.state_manager
    }

    /// Save checkpoint using the state manager
    pub async fn save_checkpoint(&self) -> Result<()> {
        // Get checkpoint data from processor
        let checkpoint_data = self.processor.checkpoint().await?;

        // Store in state manager
        self.state_manager
            .set(format!("processor_{}", self.pipeline_id), checkpoint_data);

        // Persist to disk if configured
        self.state_manager.save_checkpoint(&self.pipeline_id).await
    }

    /// Restore from checkpoint
    pub async fn restore_checkpoint(&self) -> Result<()> {
        // Load checkpoint from disk
        self.state_manager
            .load_checkpoint(&self.pipeline_id)
            .await?;

        // Restore processor state if available
        if let Some(state) = self
            .state_manager
            .get(&format!("processor_{}", self.pipeline_id))
        {
            self.processor.restore(&state).await?;
        }

        Ok(())
    }

    /// Process a stream in parallel
    pub async fn process_stream<S>(&self, mut stream: S) -> Result<Vec<StreamItem>>
    where
        S: Stream<Item = Result<StreamItem>> + Unpin + Send,
    {
        let mut results = Vec::new();
        let semaphore = Arc::new(Semaphore::new(self.config.max_parallelism));
        let mut handles = Vec::new();

        while let Some(item_result) = stream.next().await {
            let item = item_result?;

            let processor = Arc::clone(&self.processor);
            let semaphore = Arc::clone(&semaphore);

            let handle = tokio::spawn(async move {
                let _permit =
                    semaphore
                        .acquire()
                        .await
                        .map_err(|_| StreamError::ParallelFailed {
                            message: "Failed to acquire semaphore".to_string(),
                        })?;
                processor.process(item).await
            });

            handles.push(handle);
        }

        // Collect results
        for handle in handles {
            let result = handle.await.map_err(|e| StreamError::ParallelFailed {
                message: format!("Task join error: {}", e),
            })??;
            results.push(result);
        }

        Ok(results)
    }

    /// Process a batch of items
    pub async fn process_batch(&self, items: Vec<StreamItem>) -> Result<Vec<StreamItem>> {
        let mut results = Vec::new();
        let semaphore = Arc::new(Semaphore::new(self.config.max_parallelism));
        let mut handles = Vec::new();

        for item in items {
            let processor = Arc::clone(&self.processor);
            let semaphore = Arc::clone(&semaphore);

            let handle = tokio::spawn(async move {
                let _permit =
                    semaphore
                        .acquire()
                        .await
                        .map_err(|_| StreamError::ParallelFailed {
                            message: "Failed to acquire semaphore".to_string(),
                        })?;
                processor.process(item).await
            });

            handles.push(handle);
        }

        // Collect results
        for handle in handles {
            let result = handle.await.map_err(|e| StreamError::ParallelFailed {
                message: format!("Task join error: {}", e),
            })??;
            results.push(result);
        }

        Ok(results)
    }
}

// Helper to get number of CPUs
#[allow(clippy::unnecessary_wraps)]
fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestProcessor;

    #[async_trait]
    impl StreamProcessor for TestProcessor {
        async fn process(&self, item: StreamItem) -> Result<StreamItem> {
            Ok(item)
        }
    }

    #[tokio::test]
    async fn test_buffered_stream() {
        let config = StreamConfig::default();
        let stream = BufferedStream::new(config);

        let item = vec![1, 2, 3, 4];
        stream.push(item.clone()).await.expect("Failed to push");

        let pulled = stream.pull().await.expect("Failed to pull");
        assert_eq!(pulled, Some(item));

        assert_eq!(stream.items_processed().await, 1);
    }

    #[tokio::test]
    async fn test_state_manager() {
        let manager = StateManager::new(None);

        manager.set("test_key".to_string(), vec![1, 2, 3]);
        let value = manager.get("test_key");
        assert_eq!(value, Some(vec![1, 2, 3]));

        manager.clear();
        let value = manager.get("test_key");
        assert_eq!(value, None);
    }

    #[tokio::test]
    async fn test_parallel_processor() {
        let config = StreamConfig::default();
        let processor = Arc::new(TestProcessor);
        let state_manager = Arc::new(StateManager::new(None));

        let parallel = ParallelProcessor::new(config, processor, state_manager);

        let items = vec![vec![1, 2], vec![3, 4], vec![5, 6]];
        let results = parallel
            .process_batch(items.clone())
            .await
            .expect("Failed to process");

        assert_eq!(results.len(), 3);
    }

    #[tokio::test]
    async fn test_checkpoint_needed() {
        let config = StreamConfig {
            checkpointing: true,
            checkpoint_interval: 2,
            ..Default::default()
        };

        let stream = BufferedStream::new(config);

        stream.push(vec![1]).await.expect("Failed to push");
        assert!(!stream.needs_checkpoint().await);

        stream.push(vec![2]).await.expect("Failed to push");
        assert!(stream.needs_checkpoint().await);
    }
}
