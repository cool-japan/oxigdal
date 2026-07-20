//! Fine-tuning procedures and utilities.

use crate::transfer::{FineTuningConfig, TransferStrategy};
use crate::{Error, Result};

/// Fine-tuning scheduler for gradual unfreezing.
#[derive(Debug)]
pub struct FineTuningScheduler {
    /// Fine-tuning configuration
    config: FineTuningConfig,
    /// Current epoch
    current_epoch: usize,
    /// Total number of layers in the model
    total_layers: usize,
}

impl FineTuningScheduler {
    /// Creates a new fine-tuning scheduler.
    pub fn new(config: FineTuningConfig, total_layers: usize) -> Result<Self> {
        config.validate()?;

        if total_layers == 0 {
            return Err(Error::invalid_parameter(
                "total_layers",
                total_layers,
                "must be positive",
            ));
        }

        Ok(Self {
            config,
            current_epoch: 0,
            total_layers,
        })
    }

    /// Updates the scheduler for a new epoch.
    pub fn step(&mut self) {
        self.current_epoch += 1;
    }

    /// Gets the learning rate for a specific layer at the current epoch.
    pub fn get_layer_lr(&self, layer_idx: usize) -> f64 {
        match self.config.strategy {
            TransferStrategy::GradualUnfreezing => {
                let unfreeze_schedule = self.get_unfreeze_schedule();
                if layer_idx >= unfreeze_schedule {
                    self.config.base_learning_rate
                } else {
                    0.0
                }
            }
            _ => self
                .config
                .learning_rate_for_layer(layer_idx, self.total_layers),
        }
    }

    /// Gets the layer index to unfreeze at the current epoch for gradual unfreezing.
    fn get_unfreeze_schedule(&self) -> usize {
        if self.current_epoch < self.config.epochs_before_unfreeze {
            // Keep all layers frozen initially
            self.total_layers
        } else {
            // Gradually unfreeze from top to bottom
            let epochs_since_start = self.current_epoch - self.config.epochs_before_unfreeze;
            let layers_per_epoch = (self.total_layers as f64 / 10.0).ceil() as usize;
            let unfrozen_count = (epochs_since_start * layers_per_epoch).min(self.total_layers);
            self.total_layers.saturating_sub(unfrozen_count)
        }
    }

    /// Resets the scheduler to epoch 0.
    pub fn reset(&mut self) {
        self.current_epoch = 0;
    }

    /// Gets the current epoch.
    pub fn current_epoch(&self) -> usize {
        self.current_epoch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_finetuning_scheduler() {
        let config = FineTuningConfig::fine_tune_all(1e-3);
        let mut scheduler =
            FineTuningScheduler::new(config, 10).expect("Failed to create scheduler");

        assert_eq!(scheduler.current_epoch(), 0);
        assert_eq!(scheduler.get_layer_lr(0), 1e-3);
        assert_eq!(scheduler.get_layer_lr(9), 1e-3);

        scheduler.step();
        assert_eq!(scheduler.current_epoch(), 1);
    }

    #[test]
    fn test_gradual_unfreezing() {
        let config = FineTuningConfig {
            strategy: TransferStrategy::GradualUnfreezing,
            base_learning_rate: 1e-3,
            epochs_before_unfreeze: 5,
            ..Default::default()
        };

        let mut scheduler =
            FineTuningScheduler::new(config, 10).expect("Failed to create scheduler");

        // Before unfreezing starts
        for _ in 0..5 {
            assert_eq!(scheduler.get_layer_lr(0), 0.0);
            assert_eq!(scheduler.get_layer_lr(9), 0.0);
            scheduler.step();
        }

        // After unfreezing starts, top layers should have non-zero LR
        for _ in 0..5 {
            scheduler.step();
        }
        // At this point, some layers should be unfrozen
    }

    #[test]
    fn test_scheduler_reset() {
        let config = FineTuningConfig::default();
        let mut scheduler =
            FineTuningScheduler::new(config, 10).expect("Failed to create scheduler");

        scheduler.step();
        scheduler.step();
        assert_eq!(scheduler.current_epoch(), 2);

        scheduler.reset();
        assert_eq!(scheduler.current_epoch(), 0);
    }
}
