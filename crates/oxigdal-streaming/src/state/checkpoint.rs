//! Checkpointing for fault tolerance.

use crate::error::{Result, StreamingError};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;

/// Checkpoint metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointMetadata {
    /// Checkpoint ID
    pub id: u64,

    /// Checkpoint timestamp
    pub timestamp: DateTime<Utc>,

    /// Checkpoint size in bytes
    pub size_bytes: usize,

    /// State of operators
    pub operator_states: HashMap<String, Vec<u8>>,

    /// Success status
    pub success: bool,

    /// Duration to complete
    pub duration: Duration,
}

/// Checkpoint barrier.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CheckpointBarrier {
    /// Checkpoint ID
    pub id: u64,

    /// Timestamp
    pub timestamp: DateTime<Utc>,
}

impl CheckpointBarrier {
    /// Create a new checkpoint barrier.
    pub fn new(id: u64) -> Self {
        Self {
            id,
            timestamp: Utc::now(),
        }
    }
}

/// Checkpoint configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointConfig {
    /// Checkpoint interval
    pub interval: Duration,

    /// Minimum pause between checkpoints
    pub min_pause: Duration,

    /// Maximum concurrent checkpoints
    pub max_concurrent: usize,

    /// Enable unaligned checkpoints
    pub unaligned: bool,

    /// Checkpoint timeout
    pub timeout: Duration,

    /// Storage path
    pub storage_path: Option<PathBuf>,
}

impl Default for CheckpointConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(60),
            min_pause: Duration::from_secs(10),
            max_concurrent: 1,
            unaligned: false,
            timeout: Duration::from_secs(300),
            storage_path: None,
        }
    }
}

/// Checkpoint storage.
pub trait CheckpointStorage: Send + Sync {
    /// Store a checkpoint.
    fn store(&self, checkpoint: &Checkpoint) -> Result<()>;

    /// Load a checkpoint.
    fn load(&self, checkpoint_id: u64) -> Result<Option<Checkpoint>>;

    /// Delete a checkpoint.
    fn delete(&self, checkpoint_id: u64) -> Result<()>;

    /// List all checkpoints.
    fn list(&self) -> Result<Vec<u64>>;

    /// Get the latest checkpoint ID.
    fn latest(&self) -> Result<Option<u64>>;
}

/// In-memory checkpoint implementation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    /// Metadata
    pub metadata: CheckpointMetadata,

    /// Actual checkpoint data
    pub data: Vec<u8>,
}

impl Checkpoint {
    /// Create a new checkpoint.
    pub fn new(id: u64, data: Vec<u8>) -> Self {
        let size_bytes = data.len();
        Self {
            metadata: CheckpointMetadata {
                id,
                timestamp: Utc::now(),
                size_bytes,
                operator_states: HashMap::new(),
                success: true,
                duration: Duration::ZERO,
            },
            data,
        }
    }

    /// Get the checkpoint ID.
    pub fn id(&self) -> u64 {
        self.metadata.id
    }

    /// Get the checkpoint size.
    pub fn size(&self) -> usize {
        self.metadata.size_bytes
    }
}

/// Checkpoint coordinator.
pub struct CheckpointCoordinator {
    config: CheckpointConfig,
    next_checkpoint_id: Arc<RwLock<u64>>,
    active_checkpoints: Arc<RwLock<HashMap<u64, CheckpointMetadata>>>,
    completed_checkpoints: Arc<RwLock<Vec<u64>>>,
    last_checkpoint_time: Arc<RwLock<Option<DateTime<Utc>>>>,
}

impl CheckpointCoordinator {
    /// Create a new checkpoint coordinator.
    pub fn new(config: CheckpointConfig) -> Self {
        Self {
            config,
            next_checkpoint_id: Arc::new(RwLock::new(0)),
            active_checkpoints: Arc::new(RwLock::new(HashMap::new())),
            completed_checkpoints: Arc::new(RwLock::new(Vec::new())),
            last_checkpoint_time: Arc::new(RwLock::new(None)),
        }
    }

    /// Trigger a new checkpoint.
    pub async fn trigger_checkpoint(&self) -> Result<u64> {
        let now = Utc::now();
        let last_time = *self.last_checkpoint_time.read().await;

        if let Some(last) = last_time {
            let min_pause_chrono = match chrono::Duration::from_std(self.config.min_pause) {
                Ok(duration) => duration,
                Err(_) => chrono::Duration::zero(),
            };

            if now - last < min_pause_chrono {
                return Err(StreamingError::CheckpointError(
                    "Minimum pause not elapsed".to_string(),
                ));
            }
        }

        let active_count = self.active_checkpoints.read().await.len();
        if active_count >= self.config.max_concurrent {
            return Err(StreamingError::CheckpointError(
                "Too many concurrent checkpoints".to_string(),
            ));
        }

        let mut next_id = self.next_checkpoint_id.write().await;
        let checkpoint_id = *next_id;
        *next_id += 1;

        let metadata = CheckpointMetadata {
            id: checkpoint_id,
            timestamp: now,
            size_bytes: 0,
            operator_states: HashMap::new(),
            success: false,
            duration: Duration::ZERO,
        };

        self.active_checkpoints
            .write()
            .await
            .insert(checkpoint_id, metadata);

        *self.last_checkpoint_time.write().await = Some(now);

        Ok(checkpoint_id)
    }

    /// Complete a checkpoint.
    pub async fn complete_checkpoint(&self, checkpoint_id: u64, success: bool) -> Result<()> {
        let mut active = self.active_checkpoints.write().await;

        if let Some(mut metadata) = active.remove(&checkpoint_id) {
            metadata.success = success;
            metadata.duration = match (Utc::now() - metadata.timestamp).to_std() {
                Ok(duration) => duration,
                Err(_) => Duration::ZERO,
            };

            if success {
                self.completed_checkpoints.write().await.push(checkpoint_id);
            }

            Ok(())
        } else {
            Err(StreamingError::CheckpointError(format!(
                "Checkpoint {} not found",
                checkpoint_id
            )))
        }
    }

    /// Get active checkpoint count.
    pub async fn active_count(&self) -> usize {
        self.active_checkpoints.read().await.len()
    }

    /// Get completed checkpoint count.
    pub async fn completed_count(&self) -> usize {
        self.completed_checkpoints.read().await.len()
    }

    /// Get the latest completed checkpoint ID.
    pub async fn latest_checkpoint(&self) -> Option<u64> {
        self.completed_checkpoints.read().await.last().copied()
    }

    /// Clear old checkpoints.
    pub async fn clear_old_checkpoints(&self, keep_count: usize) {
        let mut completed = self.completed_checkpoints.write().await;

        if completed.len() > keep_count {
            let to_remove = completed.len() - keep_count;
            completed.drain(0..to_remove);
        }
    }

    /// Start periodic checkpointing.
    pub async fn start_periodic_checkpointing(self: Arc<Self>) {
        let interval = self.config.interval;

        tokio::spawn(async move {
            loop {
                sleep(interval).await;

                match self.trigger_checkpoint().await {
                    Ok(id) => {
                        tracing::info!("Triggered checkpoint {}", id);

                        tokio::spawn({
                            let coordinator = self.clone();
                            async move {
                                sleep(Duration::from_secs(1)).await;
                                if let Err(e) = coordinator.complete_checkpoint(id, true).await {
                                    tracing::error!("Failed to complete checkpoint {}: {}", id, e);
                                }
                            }
                        });
                    }
                    Err(e) => {
                        tracing::warn!("Failed to trigger checkpoint: {}", e);
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_checkpoint_creation() {
        let data = vec![1, 2, 3, 4];
        let checkpoint = Checkpoint::new(1, data.clone());

        assert_eq!(checkpoint.id(), 1);
        assert_eq!(checkpoint.size(), 4);
        assert_eq!(checkpoint.data, data);
    }

    #[tokio::test]
    async fn test_checkpoint_barrier() {
        let barrier = CheckpointBarrier::new(1);
        assert_eq!(barrier.id, 1);
    }

    #[tokio::test]
    async fn test_checkpoint_coordinator() {
        let config = CheckpointConfig {
            min_pause: Duration::ZERO, // Allow immediate consecutive checkpoints
            max_concurrent: 2,         // Allow 2 concurrent checkpoints
            ..Default::default()
        };
        let coordinator = CheckpointCoordinator::new(config);

        let id1 = coordinator
            .trigger_checkpoint()
            .await
            .expect("First checkpoint trigger should succeed");
        assert_eq!(id1, 0);

        let id2 = coordinator
            .trigger_checkpoint()
            .await
            .expect("Second checkpoint trigger should succeed");
        assert_eq!(id2, 1);

        assert_eq!(coordinator.active_count().await, 2);

        coordinator
            .complete_checkpoint(id1, true)
            .await
            .expect("Checkpoint completion should succeed");
        assert_eq!(coordinator.active_count().await, 1);
        assert_eq!(coordinator.completed_count().await, 1);
    }

    #[tokio::test]
    async fn test_checkpoint_min_pause() {
        let config = CheckpointConfig {
            min_pause: Duration::from_secs(60),
            ..Default::default()
        };

        let coordinator = CheckpointCoordinator::new(config);

        coordinator
            .trigger_checkpoint()
            .await
            .expect("First checkpoint should trigger successfully");
        let result = coordinator.trigger_checkpoint().await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_clear_old_checkpoints() {
        let config = CheckpointConfig {
            min_pause: Duration::ZERO, // Allow rapid consecutive checkpoints
            ..Default::default()
        };
        let coordinator = CheckpointCoordinator::new(config);

        for _ in 0..5 {
            let id = coordinator
                .trigger_checkpoint()
                .await
                .expect("Checkpoint trigger should succeed in loop");
            coordinator
                .complete_checkpoint(id, true)
                .await
                .expect("Checkpoint completion should succeed in loop");
        }

        assert_eq!(coordinator.completed_count().await, 5);

        coordinator.clear_old_checkpoints(2).await;
        assert_eq!(coordinator.completed_count().await, 2);
    }
}
