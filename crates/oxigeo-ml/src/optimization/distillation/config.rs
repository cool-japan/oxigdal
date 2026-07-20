//! Configuration types for knowledge distillation

use crate::error::{MlError, Result};

/// Distillation loss function
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DistillationLoss {
    /// Kullback-Leibler divergence
    #[default]
    KLDivergence,
    /// Mean squared error
    MSE,
    /// Cross-entropy
    CrossEntropy,
    /// Custom weighted combination
    Weighted {
        /// Weight for distillation loss
        distill_weight: u8,
        /// Weight for ground truth loss
        ground_truth_weight: u8,
    },
}

/// Temperature for softening probability distributions
#[derive(Debug, Clone, Copy)]
pub struct Temperature(pub f32);

impl Default for Temperature {
    fn default() -> Self {
        Self(2.0) // Standard temperature for distillation
    }
}

impl Temperature {
    /// Creates a new temperature value
    #[must_use]
    pub fn new(value: f32) -> Self {
        Self(value.max(0.1)) // Minimum temperature to avoid numerical issues
    }

    /// Applies temperature scaling to logits
    #[must_use]
    pub fn scale_logits(&self, logits: &[f32]) -> Vec<f32> {
        logits.iter().map(|&x| x / self.0).collect()
    }

    /// Returns the temperature value
    #[must_use]
    pub fn value(&self) -> f32 {
        self.0
    }
}

/// Optimizer type for training
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OptimizerType {
    /// Stochastic Gradient Descent
    SGD,
    /// SGD with momentum
    SGDMomentum {
        /// Momentum coefficient (typically 0.9)
        momentum: u8,
    },
    /// Adam optimizer
    #[default]
    Adam,
    /// AdamW with weight decay
    AdamW {
        /// Weight decay coefficient (as percentage, e.g., 1 = 0.01)
        weight_decay: u8,
    },
}

/// Learning rate schedule
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum LearningRateSchedule {
    /// Constant learning rate
    #[default]
    Constant,
    /// Step decay
    StepDecay {
        /// Decay factor
        decay_factor: f32,
        /// Steps between decays
        step_size: usize,
    },
    /// Cosine annealing
    CosineAnnealing {
        /// Minimum learning rate
        min_lr: f32,
    },
    /// Warmup then decay
    WarmupDecay {
        /// Warmup epochs
        warmup_epochs: usize,
        /// Decay factor per epoch after warmup
        decay_factor: f32,
    },
}

/// Early stopping configuration
#[derive(Debug, Clone, Copy)]
pub struct EarlyStopping {
    /// Patience (epochs without improvement)
    pub patience: usize,
    /// Minimum delta for improvement
    pub min_delta: f32,
}

impl Default for EarlyStopping {
    fn default() -> Self {
        Self {
            patience: 10,
            min_delta: 0.001,
        }
    }
}

/// Knowledge distillation configuration
#[derive(Debug, Clone)]
pub struct DistillationConfig {
    /// Distillation loss function
    pub loss: DistillationLoss,
    /// Temperature for softening
    pub temperature: Temperature,
    /// Number of training epochs
    pub epochs: usize,
    /// Learning rate
    pub learning_rate: f32,
    /// Batch size
    pub batch_size: usize,
    /// Alpha weight for distillation loss (1 - alpha for hard label loss)
    pub alpha: f32,
    /// Optimizer type
    pub optimizer: OptimizerType,
    /// Learning rate schedule
    pub lr_schedule: LearningRateSchedule,
    /// Early stopping configuration
    pub early_stopping: Option<EarlyStopping>,
    /// Gradient clipping threshold (None = no clipping)
    pub gradient_clip: Option<f32>,
    /// Validation split ratio (0.0 to 0.3)
    pub validation_split: f32,
    /// Number of classes for classification
    pub num_classes: usize,
    /// Random seed for reproducibility
    pub seed: u64,
}

impl Default for DistillationConfig {
    fn default() -> Self {
        Self {
            loss: DistillationLoss::KLDivergence,
            temperature: Temperature::default(),
            epochs: 100,
            learning_rate: 0.001,
            batch_size: 32,
            alpha: 0.5,
            optimizer: OptimizerType::Adam,
            lr_schedule: LearningRateSchedule::Constant,
            early_stopping: Some(EarlyStopping::default()),
            gradient_clip: Some(1.0),
            validation_split: 0.1,
            num_classes: 10,
            seed: 42,
        }
    }
}

impl DistillationConfig {
    /// Creates a configuration builder
    #[must_use]
    pub fn builder() -> DistillationConfigBuilder {
        DistillationConfigBuilder::default()
    }

    /// Validates the configuration
    pub fn validate(&self) -> Result<()> {
        if self.alpha < 0.0 || self.alpha > 1.0 {
            return Err(MlError::InvalidConfig(format!(
                "Alpha must be between 0.0 and 1.0, got {}",
                self.alpha
            )));
        }
        if self.learning_rate <= 0.0 {
            return Err(MlError::InvalidConfig(format!(
                "Learning rate must be positive, got {}",
                self.learning_rate
            )));
        }
        if self.epochs == 0 {
            return Err(MlError::InvalidConfig(
                "Epochs must be at least 1".to_string(),
            ));
        }
        if self.batch_size == 0 {
            return Err(MlError::InvalidConfig(
                "Batch size must be at least 1".to_string(),
            ));
        }
        if self.validation_split < 0.0 || self.validation_split > 0.5 {
            return Err(MlError::InvalidConfig(format!(
                "Validation split must be between 0.0 and 0.5, got {}",
                self.validation_split
            )));
        }
        Ok(())
    }
}

/// Builder for distillation configuration
#[derive(Debug, Default)]
pub struct DistillationConfigBuilder {
    loss: Option<DistillationLoss>,
    temperature: Option<f32>,
    epochs: Option<usize>,
    learning_rate: Option<f32>,
    batch_size: Option<usize>,
    alpha: Option<f32>,
    optimizer: Option<OptimizerType>,
    lr_schedule: Option<LearningRateSchedule>,
    early_stopping: Option<Option<EarlyStopping>>,
    gradient_clip: Option<Option<f32>>,
    validation_split: Option<f32>,
    num_classes: Option<usize>,
    seed: Option<u64>,
}

impl DistillationConfigBuilder {
    /// Sets the distillation loss
    #[must_use]
    pub fn loss(mut self, loss: DistillationLoss) -> Self {
        self.loss = Some(loss);
        self
    }

    /// Sets the temperature
    #[must_use]
    pub fn temperature(mut self, temp: f32) -> Self {
        self.temperature = Some(temp);
        self
    }

    /// Sets the number of epochs
    #[must_use]
    pub fn epochs(mut self, epochs: usize) -> Self {
        self.epochs = Some(epochs);
        self
    }

    /// Sets the learning rate
    #[must_use]
    pub fn learning_rate(mut self, lr: f32) -> Self {
        self.learning_rate = Some(lr);
        self
    }

    /// Sets the batch size
    #[must_use]
    pub fn batch_size(mut self, size: usize) -> Self {
        self.batch_size = Some(size);
        self
    }

    /// Sets the alpha weight for distillation loss
    #[must_use]
    pub fn alpha(mut self, alpha: f32) -> Self {
        self.alpha = Some(alpha.clamp(0.0, 1.0));
        self
    }

    /// Sets the optimizer type
    #[must_use]
    pub fn optimizer(mut self, optimizer: OptimizerType) -> Self {
        self.optimizer = Some(optimizer);
        self
    }

    /// Sets the learning rate schedule
    #[must_use]
    pub fn lr_schedule(mut self, schedule: LearningRateSchedule) -> Self {
        self.lr_schedule = Some(schedule);
        self
    }

    /// Sets early stopping configuration
    #[must_use]
    pub fn early_stopping(mut self, early_stopping: Option<EarlyStopping>) -> Self {
        self.early_stopping = Some(early_stopping);
        self
    }

    /// Sets gradient clipping threshold
    #[must_use]
    pub fn gradient_clip(mut self, clip: Option<f32>) -> Self {
        self.gradient_clip = Some(clip);
        self
    }

    /// Sets validation split ratio
    #[must_use]
    pub fn validation_split(mut self, split: f32) -> Self {
        self.validation_split = Some(split.clamp(0.0, 0.5));
        self
    }

    /// Sets number of classes
    #[must_use]
    pub fn num_classes(mut self, num: usize) -> Self {
        self.num_classes = Some(num);
        self
    }

    /// Sets random seed
    #[must_use]
    pub fn seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Builds the configuration
    #[must_use]
    pub fn build(self) -> DistillationConfig {
        DistillationConfig {
            loss: self.loss.unwrap_or(DistillationLoss::KLDivergence),
            temperature: Temperature::new(self.temperature.unwrap_or(2.0)),
            epochs: self.epochs.unwrap_or(100),
            learning_rate: self.learning_rate.unwrap_or(0.001),
            batch_size: self.batch_size.unwrap_or(32),
            alpha: self.alpha.unwrap_or(0.5),
            optimizer: self.optimizer.unwrap_or(OptimizerType::Adam),
            lr_schedule: self.lr_schedule.unwrap_or(LearningRateSchedule::Constant),
            early_stopping: self
                .early_stopping
                .unwrap_or(Some(EarlyStopping::default())),
            gradient_clip: self.gradient_clip.unwrap_or(Some(1.0)),
            validation_split: self.validation_split.unwrap_or(0.1),
            num_classes: self.num_classes.unwrap_or(10),
            seed: self.seed.unwrap_or(42),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_distillation_config_builder() {
        let config = DistillationConfig::builder()
            .loss(DistillationLoss::MSE)
            .temperature(3.0)
            .epochs(50)
            .learning_rate(0.01)
            .batch_size(64)
            .alpha(0.7)
            .build();

        assert_eq!(config.loss, DistillationLoss::MSE);
        assert!((config.temperature.0 - 3.0).abs() < 1e-6);
        assert_eq!(config.epochs, 50);
        assert!((config.learning_rate - 0.01).abs() < 1e-6);
        assert_eq!(config.batch_size, 64);
        assert!((config.alpha - 0.7).abs() < 1e-6);
    }

    #[test]
    fn test_config_validation() {
        let valid_config = DistillationConfig::default();
        assert!(valid_config.validate().is_ok());

        let invalid_alpha = DistillationConfig {
            alpha: 1.5,
            ..Default::default()
        };
        assert!(invalid_alpha.validate().is_err());

        let invalid_lr = DistillationConfig {
            learning_rate: -0.1,
            ..Default::default()
        };
        assert!(invalid_lr.validate().is_err());
    }

    #[test]
    fn test_temperature_scaling() {
        let temp = Temperature::new(2.0);
        let logits = vec![1.0, 2.0, 3.0];
        let scaled = temp.scale_logits(&logits);

        assert!((scaled[0] - 0.5).abs() < 1e-6);
        assert!((scaled[1] - 1.0).abs() < 1e-6);
        assert!((scaled[2] - 1.5).abs() < 1e-6);
    }

    #[test]
    fn test_temperature_minimum() {
        let temp = Temperature::new(0.01);
        assert!(temp.0 >= 0.1);
    }
}
