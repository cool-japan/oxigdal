//! Training infrastructure for deep learning models.
//!
//! Provides training loops, optimizers, schedulers, loss functions, and utilities.

use crate::{Error, Result};
use serde::{Deserialize, Serialize};

pub mod checkpointing;
pub mod early_stopping;
pub mod losses;
pub mod optimizers;
pub mod schedulers;
pub mod training_loop;

/// Training configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingConfig {
    /// Learning rate
    pub learning_rate: f64,
    /// Batch size
    pub batch_size: usize,
    /// Number of training epochs
    pub num_epochs: usize,
    /// Weight decay for L2 regularization
    pub weight_decay: f64,
    /// Gradient clipping threshold (None = no clipping)
    pub gradient_clip: Option<f64>,
    /// Number of accumulation steps for gradient accumulation
    pub accumulation_steps: usize,
    /// Enable mixed precision training
    pub mixed_precision: bool,
    /// Validation frequency (every N epochs)
    pub validation_frequency: usize,
    /// Checkpoint save frequency (every N epochs)
    pub checkpoint_frequency: usize,
    /// Device to use for training ("cpu", "cuda:0", etc.)
    pub device: String,
    /// Random seed for reproducibility
    pub seed: Option<u64>,
    /// Number of data loader workers
    pub num_workers: usize,
    /// Enable data shuffling
    pub shuffle: bool,
    /// Log interval (every N batches)
    pub log_interval: usize,
}

impl Default for TrainingConfig {
    fn default() -> Self {
        Self {
            learning_rate: 0.001,
            batch_size: 32,
            num_epochs: 100,
            weight_decay: 0.0,
            gradient_clip: Some(1.0),
            accumulation_steps: 1,
            mixed_precision: false,
            validation_frequency: 1,
            checkpoint_frequency: 10,
            device: "cpu".to_string(),
            seed: Some(42),
            num_workers: 4,
            shuffle: true,
            log_interval: 10,
        }
    }
}

impl TrainingConfig {
    /// Validates the training configuration.
    pub fn validate(&self) -> Result<()> {
        if self.learning_rate <= 0.0 {
            return Err(Error::invalid_parameter(
                "learning_rate",
                self.learning_rate,
                "must be positive",
            ));
        }

        if self.batch_size == 0 {
            return Err(Error::invalid_parameter(
                "batch_size",
                self.batch_size,
                "must be positive",
            ));
        }

        if self.num_epochs == 0 {
            return Err(Error::invalid_parameter(
                "num_epochs",
                self.num_epochs,
                "must be positive",
            ));
        }

        if self.weight_decay < 0.0 {
            return Err(Error::invalid_parameter(
                "weight_decay",
                self.weight_decay,
                "must be non-negative",
            ));
        }

        if let Some(clip) = self.gradient_clip {
            if clip <= 0.0 {
                return Err(Error::invalid_parameter(
                    "gradient_clip",
                    clip,
                    "must be positive if specified",
                ));
            }
        }

        if self.accumulation_steps == 0 {
            return Err(Error::invalid_parameter(
                "accumulation_steps",
                self.accumulation_steps,
                "must be positive",
            ));
        }

        if self.validation_frequency == 0 {
            return Err(Error::invalid_parameter(
                "validation_frequency",
                self.validation_frequency,
                "must be positive",
            ));
        }

        if self.checkpoint_frequency == 0 {
            return Err(Error::invalid_parameter(
                "checkpoint_frequency",
                self.checkpoint_frequency,
                "must be positive",
            ));
        }

        Ok(())
    }

    /// Creates a configuration for quick testing with small settings.
    pub fn for_testing() -> Self {
        Self {
            learning_rate: 0.01,
            batch_size: 4,
            num_epochs: 2,
            weight_decay: 0.0,
            gradient_clip: None,
            accumulation_steps: 1,
            mixed_precision: false,
            validation_frequency: 1,
            checkpoint_frequency: 1,
            device: "cpu".to_string(),
            seed: Some(42),
            num_workers: 1,
            shuffle: false,
            log_interval: 5,
        }
    }
}

/// Training statistics for a single epoch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpochStats {
    /// Epoch number (0-indexed)
    pub epoch: usize,
    /// Training loss
    pub train_loss: f64,
    /// Validation loss (if available)
    pub val_loss: Option<f64>,
    /// Training accuracy (if available)
    pub train_accuracy: Option<f64>,
    /// Validation accuracy (if available)
    pub val_accuracy: Option<f64>,
    /// Learning rate used in this epoch
    pub learning_rate: f64,
    /// Time taken for this epoch (seconds)
    pub epoch_time: f64,
}

/// Training history containing statistics for all epochs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrainingHistory {
    /// Statistics for each epoch
    pub epochs: Vec<EpochStats>,
}

impl TrainingHistory {
    /// Creates a new empty training history.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds epoch statistics to the history.
    pub fn add_epoch(&mut self, stats: EpochStats) {
        self.epochs.push(stats);
    }

    /// Gets the best validation loss and corresponding epoch.
    pub fn best_val_loss(&self) -> Option<(usize, f64)> {
        self.epochs
            .iter()
            .filter_map(|s| s.val_loss.map(|loss| (s.epoch, loss)))
            .min_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// Gets the best validation accuracy and corresponding epoch.
    pub fn best_val_accuracy(&self) -> Option<(usize, f64)> {
        self.epochs
            .iter()
            .filter_map(|s| s.val_accuracy.map(|acc| (s.epoch, acc)))
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// Checks if validation loss is improving.
    pub fn is_improving(&self, patience: usize) -> bool {
        if self.epochs.len() < patience + 1 {
            return true;
        }

        if let Some((best_epoch, _)) = self.best_val_loss() {
            let epochs_since_best = self.epochs.len() - 1 - best_epoch;
            epochs_since_best < patience
        } else {
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_validation() {
        let mut config = TrainingConfig::default();
        assert!(config.validate().is_ok());

        config.learning_rate = -0.001;
        assert!(config.validate().is_err());

        config.learning_rate = 0.001;
        config.batch_size = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_for_testing() {
        let config = TrainingConfig::for_testing();
        assert!(config.validate().is_ok());
        assert_eq!(config.num_epochs, 2);
        assert_eq!(config.batch_size, 4);
    }

    #[test]
    fn test_training_history() {
        let mut history = TrainingHistory::new();

        history.add_epoch(EpochStats {
            epoch: 0,
            train_loss: 1.0,
            val_loss: Some(0.9),
            train_accuracy: Some(0.7),
            val_accuracy: Some(0.75),
            learning_rate: 0.001,
            epoch_time: 10.0,
        });

        history.add_epoch(EpochStats {
            epoch: 1,
            train_loss: 0.8,
            val_loss: Some(0.85),
            train_accuracy: Some(0.75),
            val_accuracy: Some(0.8),
            learning_rate: 0.001,
            epoch_time: 10.0,
        });

        assert_eq!(history.epochs.len(), 2);

        let (best_epoch, best_loss) = history.best_val_loss().expect("Should have best val loss");
        assert_eq!(best_epoch, 1);
        assert_eq!(best_loss, 0.85);

        let (best_epoch, best_acc) = history
            .best_val_accuracy()
            .expect("Should have best val accuracy");
        assert_eq!(best_epoch, 1);
        assert_eq!(best_acc, 0.8);
    }

    #[test]
    fn test_is_improving() {
        let mut history = TrainingHistory::new();

        // Add improving epochs
        for i in 0..5 {
            history.add_epoch(EpochStats {
                epoch: i,
                train_loss: 1.0 - (i as f64 * 0.1),
                val_loss: Some(1.0 - (i as f64 * 0.1)),
                train_accuracy: None,
                val_accuracy: None,
                learning_rate: 0.001,
                epoch_time: 10.0,
            });
        }

        assert!(history.is_improving(3));

        // Add non-improving epochs
        for i in 5..8 {
            history.add_epoch(EpochStats {
                epoch: i,
                train_loss: 0.6,
                val_loss: Some(0.6),
                train_accuracy: None,
                val_accuracy: None,
                learning_rate: 0.001,
                epoch_time: 10.0,
            });
        }

        assert!(!history.is_improving(2));
    }
}
