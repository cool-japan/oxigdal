//! Training loop implementation with SciRS2 backend.
//!
//! This module provides actual training capabilities using Pure Rust scirs2 ecosystem.
//! Full functionality requires the `ml` feature to be enabled.

use crate::error::{Error, Result};
use crate::training::{TrainingConfig, TrainingHistory};

#[cfg(feature = "ml")]
use crate::backend::{BackendConfig, BackendFactory, MLBackend};
#[cfg(feature = "ml")]
use crate::models::resnet::ResNetConfig;
#[cfg(feature = "ml")]
use crate::models::unet::UNetConfig;
#[cfg(feature = "ml")]
use crate::training::EpochStats;
#[cfg(feature = "ml")]
use crate::training::checkpointing::CheckpointManager;
#[cfg(feature = "ml")]
use crate::training::early_stopping::EarlyStopping;
#[cfg(feature = "ml")]
use crate::training::losses::LossFunctionType;
#[cfg(feature = "ml")]
use crate::training::schedulers::LRScheduler;
#[cfg(feature = "ml")]
use std::time::Instant;

/// Dataset trait for training data
#[cfg(feature = "ml")]
pub trait Dataset: Send + Sync {
    /// Get the number of samples in the dataset
    fn len(&self) -> usize;

    /// Check if the dataset is empty
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get a batch of samples
    ///
    /// # Arguments
    ///
    /// * `indices` - Indices of samples to retrieve
    ///
    /// # Returns
    ///
    /// (inputs, targets) where inputs and targets are flat vectors
    fn get_batch(&self, indices: &[usize]) -> Result<(Vec<f32>, Vec<f32>)>;

    /// Get input and output shapes
    fn shapes(&self) -> (Vec<usize>, Vec<usize>);
}

/// Trainer for neural network models using SciRS2 backend.
///
/// The trainer manages the training process, including forward/backward passes,
/// optimization, validation, checkpointing, and early stopping.
pub struct Trainer {
    /// Training configuration
    pub config: TrainingConfig,

    #[cfg(feature = "ml")]
    /// ML backend (UNet or ResNet)
    backend: Option<Box<dyn MLBackend>>,

    #[cfg(feature = "ml")]
    /// Early stopping handler
    early_stopping: Option<EarlyStopping>,

    #[cfg(feature = "ml")]
    /// Checkpoint manager
    checkpoint_mgr: Option<CheckpointManager>,

    #[cfg(feature = "ml")]
    /// Learning rate scheduler
    lr_scheduler: Option<Box<dyn LRScheduler>>,

    #[cfg(feature = "ml")]
    /// Loss function
    loss_fn: LossFunctionType,
}

impl Trainer {
    /// Creates a new trainer with the given configuration.
    ///
    /// # Arguments
    /// * `config` - Training configuration
    pub fn new(config: TrainingConfig) -> Result<Self> {
        config.validate()?;

        #[cfg(not(feature = "ml"))]
        {
            Err(Error::feature_not_available("Training loop", "ml"))
        }

        #[cfg(feature = "ml")]
        {
            Ok(Self {
                config,
                backend: None,
                early_stopping: None,
                checkpoint_mgr: None,
                lr_scheduler: None,
                loss_fn: LossFunctionType::CrossEntropy,
            })
        }
    }

    /// Initialize trainer with UNet backend
    #[cfg(feature = "ml")]
    pub fn with_unet(
        mut self,
        unet_config: &UNetConfig,
        backend_config: &BackendConfig,
    ) -> Result<Self> {
        let backend = BackendFactory::create_unet(unet_config, backend_config)?;
        tracing::info!(
            "Initialized UNet with {} parameters",
            backend.num_parameters()
        );
        self.backend = Some(backend);
        Ok(self)
    }

    /// Initialize trainer with ResNet backend
    #[cfg(feature = "ml")]
    pub fn with_resnet(
        mut self,
        resnet_config: &ResNetConfig,
        backend_config: &BackendConfig,
    ) -> Result<Self> {
        let backend = BackendFactory::create_resnet(resnet_config, backend_config)?;
        tracing::info!(
            "Initialized ResNet with {} parameters",
            backend.num_parameters()
        );
        self.backend = Some(backend);
        Ok(self)
    }

    /// Set loss function
    #[cfg(feature = "ml")]
    pub fn with_loss(mut self, loss_fn: LossFunctionType) -> Self {
        self.loss_fn = loss_fn;
        self
    }

    /// Enable early stopping
    #[cfg(feature = "ml")]
    pub fn with_early_stopping(mut self, patience: usize, min_delta: f64) -> Result<Self> {
        self.early_stopping = Some(
            EarlyStopping::for_loss(patience, min_delta)
                .map_err(|e| Error::Training(format!("Failed to create early stopping: {}", e)))?,
        );
        Ok(self)
    }

    /// Enable checkpointing
    #[cfg(feature = "ml")]
    pub fn with_checkpointing(mut self, checkpoint_mgr: CheckpointManager) -> Self {
        self.checkpoint_mgr = Some(checkpoint_mgr);
        self
    }

    /// Set learning rate scheduler
    #[cfg(feature = "ml")]
    pub fn with_lr_scheduler(mut self, scheduler: Box<dyn LRScheduler>) -> Self {
        self.lr_scheduler = Some(scheduler);
        self
    }

    /// Runs the training loop with SciRS2 backend.
    ///
    /// # Arguments
    ///
    /// * `train_dataset` - Training dataset
    /// * `val_dataset` - Optional validation dataset
    ///
    /// # Returns
    ///
    /// Training history with losses and metrics per epoch
    #[cfg(feature = "ml")]
    pub fn train<D: Dataset>(
        &mut self,
        train_dataset: &D,
        val_dataset: Option<&D>,
    ) -> Result<TrainingHistory> {
        let backend = self.backend.as_mut().ok_or_else(|| {
            Error::Training(
                "Backend not initialized. Call with_unet() or with_resnet()".to_string(),
            )
        })?;

        let mut history = TrainingHistory::new();
        let batch_size = self.config.batch_size;
        let num_batches = train_dataset.len().div_ceil(batch_size);

        tracing::info!(
            "Starting training: {} epochs, {} batches per epoch, batch size {}",
            self.config.num_epochs,
            num_batches,
            batch_size
        );

        for epoch in 0..self.config.num_epochs {
            let epoch_start = Instant::now();

            // Set training mode
            backend.set_train_mode(true);

            // Train for one epoch
            let (train_loss, train_accuracy) =
                Self::train_epoch_impl(backend, train_dataset, epoch, &self.config, &self.loss_fn)?;

            // Validation
            let (val_loss, val_accuracy) = if let Some(val_ds) = val_dataset {
                backend.set_train_mode(false);
                let (loss, acc) =
                    Self::validate_epoch_impl(&**backend, val_ds, &self.config, &self.loss_fn)?;
                (Some(loss), acc)
            } else {
                (None, None)
            };

            let epoch_time = epoch_start.elapsed().as_secs_f64();

            // Update learning rate
            let current_lr = if let Some(ref scheduler) = self.lr_scheduler {
                scheduler.get_lr(epoch, self.config.learning_rate)
            } else {
                self.config.learning_rate
            };

            // Record epoch stats
            let stats = EpochStats {
                epoch,
                train_loss,
                val_loss,
                train_accuracy,
                val_accuracy,
                learning_rate: current_lr,
                epoch_time,
            };

            history.add_epoch(stats.clone());

            let train_acc_str = train_accuracy
                .map(|a| format!("{:.4}", a))
                .unwrap_or_else(|| "N/A".to_string());
            let val_loss_str = val_loss
                .map(|v| format!("{:.4}", v))
                .unwrap_or_else(|| "N/A".to_string());
            let val_acc_str = val_accuracy
                .map(|a| format!("{:.4}", a))
                .unwrap_or_else(|| "N/A".to_string());

            tracing::info!(
                "Epoch {}/{}: train_loss={:.4}, train_acc={}, val_loss={}, val_acc={}, lr={:.6}, time={:.2}s",
                epoch + 1,
                self.config.num_epochs,
                train_loss,
                train_acc_str,
                val_loss_str,
                val_acc_str,
                current_lr,
                epoch_time
            );

            // Checkpointing
            if let Some(ref mut checkpoint_mgr) = self.checkpoint_mgr {
                if checkpoint_mgr.should_save(val_loss) {
                    let checkpoint_path = checkpoint_mgr
                        .checkpoint_dir
                        .join(format!("epoch_{:04}.ckpt", epoch));
                    backend.save_weights(&checkpoint_path)?;
                    tracing::info!("Saved checkpoint to {:?}", checkpoint_path);
                }
            }

            // Early stopping check
            if let Some(ref mut early_stop) = self.early_stopping {
                if let Some(v_loss) = val_loss {
                    if !early_stop.update(v_loss) {
                        let best = early_stop.best_value().unwrap_or(v_loss);
                        tracing::info!(
                            "Early stopping triggered at epoch {} (best: {:.4})",
                            epoch + 1,
                            best
                        );
                        break;
                    }
                }
            }
        }

        Ok(history)
    }

    /// Train for a single epoch
    #[cfg(feature = "ml")]
    fn train_epoch_impl<D: Dataset>(
        backend: &mut Box<dyn MLBackend>,
        dataset: &D,
        epoch: usize,
        config: &TrainingConfig,
        loss_fn: &LossFunctionType,
    ) -> Result<(f64, Option<f64>)> {
        let batch_size = config.batch_size;
        let num_samples = dataset.len();
        let num_batches = num_samples.div_ceil(batch_size);

        let mut epoch_loss = 0.0;
        let mut correct = 0;
        let mut total = 0;
        let (input_shape, target_shape) = dataset.shapes();

        for batch_idx in 0..num_batches {
            let start_idx = batch_idx * batch_size;
            let end_idx = (start_idx + batch_size).min(num_samples);
            let indices: Vec<usize> = (start_idx..end_idx).collect();

            // Get batch data
            let (inputs, targets) = dataset.get_batch(&indices)?;

            // Forward pass
            let outputs = backend.forward(&inputs, &input_shape)?;

            // Compute loss
            let loss = loss_fn.compute(&outputs, &targets, &target_shape)?;
            epoch_loss += loss;

            // Compute accuracy for classification tasks
            if target_shape.len() == 2 && target_shape[1] > 1 {
                let batch_correct = compute_accuracy(&outputs, &targets, &target_shape)?;
                correct += batch_correct;
                total += end_idx - start_idx;
            }

            // Backward pass
            let grad_output = loss_fn.backward(&outputs, &targets, &target_shape)?;
            backend.backward(&grad_output, &target_shape)?;

            // Optimizer step
            backend.optimizer_step(config.learning_rate as f32)?;
            backend.zero_grad()?;

            // Logging
            if (batch_idx + 1) % config.log_interval == 0 {
                tracing::debug!(
                    "Epoch {}, Batch {}/{}: loss={:.4}",
                    epoch + 1,
                    batch_idx + 1,
                    num_batches,
                    loss / (end_idx - start_idx) as f64
                );
            }
        }

        let avg_loss = epoch_loss / num_samples as f64;
        let accuracy = if total > 0 {
            Some(correct as f64 / total as f64)
        } else {
            None
        };

        Ok((avg_loss, accuracy))
    }

    /// Validate for one epoch
    #[cfg(feature = "ml")]
    fn validate_epoch_impl<D: Dataset>(
        backend: &dyn MLBackend,
        dataset: &D,
        config: &TrainingConfig,
        loss_fn: &LossFunctionType,
    ) -> Result<(f64, Option<f64>)> {
        let batch_size = config.batch_size;
        let num_samples = dataset.len();
        let num_batches = num_samples.div_ceil(batch_size);

        let mut total_loss = 0.0;
        let mut correct = 0;
        let mut total = 0;

        let (input_shape, target_shape) = dataset.shapes();

        for batch_idx in 0..num_batches {
            let start_idx = batch_idx * batch_size;
            let end_idx = (start_idx + batch_size).min(num_samples);
            let indices: Vec<usize> = (start_idx..end_idx).collect();

            // Get batch data
            let (inputs, targets) = dataset.get_batch(&indices)?;

            // Forward pass (no gradient computation)
            let outputs = backend.forward(&inputs, &input_shape)?;

            // Compute loss
            let loss = loss_fn.compute(&outputs, &targets, &target_shape)?;
            total_loss += loss;

            // Compute accuracy for classification tasks
            if target_shape.len() == 2 && target_shape[1] > 1 {
                // Multi-class classification
                let batch_correct = compute_accuracy(&outputs, &targets, &target_shape)?;
                correct += batch_correct;
                total += end_idx - start_idx;
            }
        }

        let avg_loss = total_loss / num_samples as f64;
        let accuracy = if total > 0 {
            Some(correct as f64 / total as f64)
        } else {
            None
        };

        Ok((avg_loss, accuracy))
    }

    /// Stub implementation for when ml feature is disabled
    #[cfg(not(feature = "ml"))]
    #[allow(unused_variables)]
    pub fn train<D>(
        &mut self,
        train_dataset: &D,
        val_dataset: Option<&D>,
    ) -> Result<TrainingHistory> {
        Err(Error::feature_not_available("Training execution", "ml"))
    }
}

/// Compute accuracy for classification
#[cfg(feature = "ml")]
fn compute_accuracy(outputs: &[f32], targets: &[f32], target_shape: &[usize]) -> Result<usize> {
    if target_shape.len() != 2 {
        return Ok(0);
    }

    let batch_size = target_shape[0];
    let num_classes = target_shape[1];

    let mut correct = 0;

    for i in 0..batch_size {
        let output_start = i * num_classes;
        let output_end = output_start + num_classes;
        let output_slice = &outputs[output_start..output_end];

        let target_start = i * num_classes;
        let target_end = target_start + num_classes;
        let target_slice = &targets[target_start..target_end];

        // Find predicted class (argmax)
        let pred_class = output_slice
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(idx, _)| idx)
            .unwrap_or(0);

        // Find true class (argmax for one-hot encoded targets)
        let true_class = target_slice
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(idx, _)| idx)
            .unwrap_or(0);

        if pred_class == true_class {
            correct += 1;
        }
    }

    Ok(correct)
}

/// Utility functions for training loops.
pub mod utils {
    /// Computes gradient norm for gradient clipping.
    ///
    /// # Arguments
    /// * `gradients` - List of gradient tensors
    ///
    /// Returns the L2 norm of the concatenated gradients.
    pub fn compute_gradient_norm(gradients: &[ndarray::Array2<f32>]) -> f32 {
        let mut total_norm_sq = 0.0f32;

        for grad in gradients {
            total_norm_sq += grad.iter().map(|&g| g * g).sum::<f32>();
        }

        total_norm_sq.sqrt()
    }

    /// Clips gradients by global norm.
    ///
    /// # Arguments
    /// * `gradients` - Mutable list of gradient tensors
    /// * `max_norm` - Maximum allowed gradient norm
    pub fn clip_gradients_by_norm(gradients: &mut [ndarray::Array2<f32>], max_norm: f32) {
        let total_norm = compute_gradient_norm(gradients);

        if total_norm > max_norm {
            let scale = max_norm / total_norm;
            for grad in gradients.iter_mut() {
                grad.mapv_inplace(|g| g * scale);
            }
        }
    }

    /// Computes moving average for smoothing metrics.
    ///
    /// # Arguments
    /// * `current_avg` - Current moving average
    /// * `new_value` - New value to incorporate
    /// * `momentum` - Momentum factor (0-1)
    pub fn moving_average(current_avg: f64, new_value: f64, momentum: f64) -> f64 {
        momentum * current_avg + (1.0 - momentum) * new_value
    }
}

#[cfg(test)]
mod tests {
    use super::utils::*;
    use super::*;
    use approx::assert_relative_eq;
    use ndarray::arr2;

    #[test]
    fn test_trainer_creation() {
        let config = TrainingConfig::for_testing();

        #[cfg(not(feature = "ml"))]
        {
            let result = Trainer::new(config);
            assert!(result.is_err());
        }

        #[cfg(feature = "ml")]
        {
            let result = Trainer::new(config);
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_compute_gradient_norm() {
        let grad1 = arr2(&[[3.0, 4.0]]);
        let grad2 = arr2(&[[0.0, 0.0]]);

        let norm = compute_gradient_norm(&[grad1, grad2]);
        assert_relative_eq!(norm, 5.0, epsilon = 1e-5);
    }

    #[test]
    fn test_clip_gradients_by_norm() {
        let grad1 = arr2(&[[3.0, 4.0]]);
        let grad2 = arr2(&[[0.0, 0.0]]);

        let gradients = &mut [grad1.clone(), grad2.clone()];
        clip_gradients_by_norm(gradients, 2.5);

        let new_norm = compute_gradient_norm(gradients);
        assert_relative_eq!(new_norm, 2.5, epsilon = 1e-5);
    }

    #[test]
    fn test_moving_average() {
        let current = 10.0;
        let new_value = 5.0;
        let momentum = 0.9;

        let result = moving_average(current, new_value, momentum);
        assert_relative_eq!(result, 9.5, epsilon = 1e-10);
    }
}
