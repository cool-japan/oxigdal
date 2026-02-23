//! Model checkpoint saving and loading.

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Checkpoint metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointMetadata {
    /// Epoch number when checkpoint was saved
    pub epoch: usize,
    /// Training loss at checkpoint
    pub train_loss: f64,
    /// Validation loss at checkpoint (if available)
    pub val_loss: Option<f64>,
    /// Training accuracy at checkpoint (if available)
    pub train_accuracy: Option<f64>,
    /// Validation accuracy at checkpoint (if available)
    pub val_accuracy: Option<f64>,
    /// Timestamp when checkpoint was created
    pub timestamp: String,
    /// Model configuration (JSON string)
    pub model_config: Option<String>,
}

impl CheckpointMetadata {
    /// Creates a new checkpoint metadata.
    pub fn new(epoch: usize, train_loss: f64) -> Self {
        Self {
            epoch,
            train_loss,
            val_loss: None,
            train_accuracy: None,
            val_accuracy: None,
            timestamp: chrono::Utc::now().to_rfc3339(),
            model_config: None,
        }
    }

    /// Sets validation metrics.
    pub fn with_val_metrics(mut self, val_loss: f64, val_accuracy: Option<f64>) -> Self {
        self.val_loss = Some(val_loss);
        self.val_accuracy = val_accuracy;
        self
    }

    /// Sets training accuracy.
    pub fn with_train_accuracy(mut self, train_accuracy: f64) -> Self {
        self.train_accuracy = Some(train_accuracy);
        self
    }

    /// Sets model configuration.
    pub fn with_model_config(mut self, config: String) -> Self {
        self.model_config = Some(config);
        self
    }
}

/// Checkpoint manager for saving and loading model checkpoints.
#[derive(Debug, Clone)]
pub struct CheckpointManager {
    /// Directory to save checkpoints
    pub checkpoint_dir: PathBuf,
    /// Maximum number of checkpoints to keep (None = keep all)
    pub max_to_keep: Option<usize>,
    /// Whether to save only the best checkpoint
    pub save_best_only: bool,
    /// Best validation loss seen so far
    best_val_loss: Option<f64>,
}

impl CheckpointManager {
    /// Creates a new checkpoint manager.
    ///
    /// # Arguments
    /// * `checkpoint_dir` - Directory to save checkpoints
    /// * `max_to_keep` - Maximum number of checkpoints to keep (None = keep all)
    /// * `save_best_only` - Whether to save only the best checkpoint
    pub fn new<P: AsRef<Path>>(
        checkpoint_dir: P,
        max_to_keep: Option<usize>,
        save_best_only: bool,
    ) -> Result<Self> {
        let checkpoint_dir = checkpoint_dir.as_ref().to_path_buf();

        // Create checkpoint directory if it doesn't exist
        if !checkpoint_dir.exists() {
            std::fs::create_dir_all(&checkpoint_dir).map_err(|e| {
                Error::Checkpoint(format!("Failed to create checkpoint directory: {}", e))
            })?;
        }

        Ok(Self {
            checkpoint_dir,
            max_to_keep,
            save_best_only,
            best_val_loss: None,
        })
    }

    /// Generates a checkpoint filename for a given epoch.
    pub fn checkpoint_filename(&self, epoch: usize) -> PathBuf {
        self.checkpoint_dir
            .join(format!("checkpoint_epoch_{:04}.json", epoch))
    }

    /// Generates a filename for the best checkpoint.
    pub fn best_checkpoint_filename(&self) -> PathBuf {
        self.checkpoint_dir.join("checkpoint_best.json")
    }

    /// Checks if a checkpoint should be saved based on the save strategy.
    ///
    /// # Arguments
    /// * `val_loss` - Current validation loss
    ///
    /// Returns `true` if the checkpoint should be saved.
    pub fn should_save(&mut self, val_loss: Option<f64>) -> bool {
        if !self.save_best_only {
            return true;
        }

        match (val_loss, self.best_val_loss) {
            (Some(current), Some(best)) => {
                if current < best {
                    self.best_val_loss = Some(current);
                    true
                } else {
                    false
                }
            }
            (Some(current), None) => {
                self.best_val_loss = Some(current);
                true
            }
            (None, _) => {
                // If no validation loss, always save
                true
            }
        }
    }

    /// Saves checkpoint metadata to a file.
    ///
    /// # Arguments
    /// * `metadata` - Checkpoint metadata to save
    pub fn save_metadata(&self, metadata: &CheckpointMetadata) -> Result<PathBuf> {
        let filename = if self.save_best_only {
            self.best_checkpoint_filename()
        } else {
            self.checkpoint_filename(metadata.epoch)
        };

        let json = serde_json::to_string_pretty(metadata).map_err(|e| {
            Error::Checkpoint(format!("Failed to serialize checkpoint metadata: {}", e))
        })?;

        std::fs::write(&filename, json)
            .map_err(|e| Error::Checkpoint(format!("Failed to write checkpoint file: {}", e)))?;

        // Clean up old checkpoints if max_to_keep is set
        if !self.save_best_only {
            if let Some(max_to_keep) = self.max_to_keep {
                self.cleanup_old_checkpoints(max_to_keep)?;
            }
        }

        Ok(filename)
    }

    /// Loads checkpoint metadata from a file.
    ///
    /// # Arguments
    /// * `filename` - Path to the checkpoint file
    pub fn load_metadata<P: AsRef<Path>>(filename: P) -> Result<CheckpointMetadata> {
        let json = std::fs::read_to_string(filename.as_ref())
            .map_err(|e| Error::Checkpoint(format!("Failed to read checkpoint file: {}", e)))?;

        let metadata: CheckpointMetadata = serde_json::from_str(&json).map_err(|e| {
            Error::Checkpoint(format!("Failed to deserialize checkpoint metadata: {}", e))
        })?;

        Ok(metadata)
    }

    /// Lists all checkpoint files in the checkpoint directory.
    pub fn list_checkpoints(&self) -> Result<Vec<PathBuf>> {
        let mut checkpoints = Vec::new();

        let entries = std::fs::read_dir(&self.checkpoint_dir).map_err(|e| {
            Error::Checkpoint(format!("Failed to read checkpoint directory: {}", e))
        })?;

        for entry in entries {
            let entry = entry
                .map_err(|e| Error::Checkpoint(format!("Failed to read directory entry: {}", e)))?;

            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                checkpoints.push(path);
            }
        }

        // Sort by modification time
        checkpoints.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());

        Ok(checkpoints)
    }

    /// Finds the latest checkpoint.
    pub fn latest_checkpoint(&self) -> Result<Option<PathBuf>> {
        let checkpoints = self.list_checkpoints()?;
        Ok(checkpoints.last().cloned())
    }

    /// Cleans up old checkpoints, keeping only the most recent N.
    fn cleanup_old_checkpoints(&self, max_to_keep: usize) -> Result<()> {
        let mut checkpoints = self.list_checkpoints()?;

        // Remove "checkpoint_best.json" from the list
        checkpoints
            .retain(|p| p.file_name().and_then(|n| n.to_str()) != Some("checkpoint_best.json"));

        if checkpoints.len() <= max_to_keep {
            return Ok(());
        }

        // Remove oldest checkpoints
        let to_remove = checkpoints.len() - max_to_keep;
        for checkpoint in checkpoints.iter().take(to_remove) {
            std::fs::remove_file(checkpoint).map_err(|e| {
                Error::Checkpoint(format!("Failed to remove old checkpoint: {}", e))
            })?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_checkpoint_metadata() {
        let metadata = CheckpointMetadata::new(5, 0.5)
            .with_val_metrics(0.6, Some(0.85))
            .with_train_accuracy(0.9)
            .with_model_config("{}".to_string());

        assert_eq!(metadata.epoch, 5);
        assert_eq!(metadata.train_loss, 0.5);
        assert_eq!(metadata.val_loss, Some(0.6));
        assert_eq!(metadata.val_accuracy, Some(0.85));
        assert_eq!(metadata.train_accuracy, Some(0.9));
        assert!(metadata.model_config.is_some());
    }

    #[test]
    fn test_checkpoint_manager_creation() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manager = CheckpointManager::new(temp_dir.path(), Some(3), false)
            .expect("Failed to create checkpoint manager");

        assert!(manager.checkpoint_dir.exists());
        assert_eq!(manager.max_to_keep, Some(3));
        assert!(!manager.save_best_only);
    }

    #[test]
    fn test_checkpoint_save_and_load() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manager = CheckpointManager::new(temp_dir.path(), None, false)
            .expect("Failed to create checkpoint manager");

        let metadata = CheckpointMetadata::new(1, 0.5).with_val_metrics(0.6, Some(0.85));

        let filename = manager
            .save_metadata(&metadata)
            .expect("Failed to save checkpoint");

        let loaded =
            CheckpointManager::load_metadata(&filename).expect("Failed to load checkpoint");

        assert_eq!(loaded.epoch, metadata.epoch);
        assert_eq!(loaded.train_loss, metadata.train_loss);
        assert_eq!(loaded.val_loss, metadata.val_loss);
    }

    #[test]
    fn test_should_save_best_only() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let mut manager = CheckpointManager::new(temp_dir.path(), None, true)
            .expect("Failed to create checkpoint manager");

        assert!(manager.should_save(Some(1.0)));
        assert!(manager.should_save(Some(0.8))); // Better
        assert!(!manager.should_save(Some(0.9))); // Worse
        assert!(manager.should_save(Some(0.7))); // Better again
    }

    #[test]
    fn test_list_checkpoints() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manager = CheckpointManager::new(temp_dir.path(), None, false)
            .expect("Failed to create checkpoint manager");

        // Save multiple checkpoints
        for epoch in 0..5 {
            let metadata = CheckpointMetadata::new(epoch, 0.5 - epoch as f64 * 0.05);
            manager
                .save_metadata(&metadata)
                .expect("Failed to save checkpoint");
        }

        let checkpoints = manager
            .list_checkpoints()
            .expect("Failed to list checkpoints");
        assert_eq!(checkpoints.len(), 5);
    }

    #[test]
    fn test_latest_checkpoint() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manager = CheckpointManager::new(temp_dir.path(), None, false)
            .expect("Failed to create checkpoint manager");

        let latest = manager
            .latest_checkpoint()
            .expect("Failed to get latest checkpoint");
        assert!(latest.is_none());

        let metadata = CheckpointMetadata::new(1, 0.5);
        manager
            .save_metadata(&metadata)
            .expect("Failed to save checkpoint");

        let latest = manager
            .latest_checkpoint()
            .expect("Failed to get latest checkpoint");
        assert!(latest.is_some());
    }
}
