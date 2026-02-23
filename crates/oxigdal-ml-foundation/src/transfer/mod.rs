//! Transfer learning and fine-tuning capabilities.
//!
//! Provides tools for loading pre-trained models, freezing layers, and fine-tuning.

pub mod feature_extraction;
pub mod finetuning;
pub mod freezing;
pub mod pretrained;

use crate::{Error, Result};
use serde::{Deserialize, Serialize};

/// Transfer learning strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferStrategy {
    /// Feature extraction: freeze all pre-trained layers
    FeatureExtraction,
    /// Fine-tune all layers with small learning rate
    FineTuneAll,
    /// Fine-tune only top N layers
    FineTuneTop(usize),
    /// Gradual unfreezing: unfreeze layers progressively during training
    GradualUnfreezing,
    /// Discriminative fine-tuning: different learning rates for different layers
    DiscriminativeLearning,
}

/// Layer freezing configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreezingConfig {
    /// Indices of layers to freeze (None = freeze all)
    pub frozen_layers: Option<Vec<usize>>,
    /// Freeze batch normalization statistics
    pub freeze_bn: bool,
    /// Freeze dropout layers
    pub freeze_dropout: bool,
}

impl Default for FreezingConfig {
    fn default() -> Self {
        Self {
            frozen_layers: None,
            freeze_bn: true,
            freeze_dropout: false,
        }
    }
}

impl FreezingConfig {
    /// Creates a new freezing configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Freezes all layers.
    pub fn freeze_all() -> Self {
        Self {
            frozen_layers: None,
            freeze_bn: true,
            freeze_dropout: true,
        }
    }

    /// Freezes specific layers by index.
    pub fn freeze_layers(indices: Vec<usize>) -> Self {
        Self {
            frozen_layers: Some(indices),
            freeze_bn: true,
            freeze_dropout: false,
        }
    }

    /// Freezes the first N layers.
    pub fn freeze_first_n(n: usize) -> Self {
        Self {
            frozen_layers: Some((0..n).collect()),
            freeze_bn: true,
            freeze_dropout: false,
        }
    }

    /// Validates the configuration.
    pub fn validate(&self) -> Result<()> {
        if let Some(ref layers) = self.frozen_layers {
            if layers.is_empty() {
                return Err(Error::TransferLearning(
                    "frozen_layers cannot be empty (use None for all layers)".to_string(),
                ));
            }

            // Check for duplicates
            let mut sorted = layers.clone();
            sorted.sort_unstable();
            sorted.dedup();
            if sorted.len() != layers.len() {
                return Err(Error::TransferLearning(
                    "frozen_layers contains duplicates".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Checks if a layer should be frozen.
    pub fn is_frozen(&self, layer_idx: usize) -> bool {
        match &self.frozen_layers {
            None => true, // All layers frozen
            Some(indices) => indices.contains(&layer_idx),
        }
    }
}

/// Fine-tuning configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FineTuningConfig {
    /// Transfer learning strategy
    pub strategy: TransferStrategy,
    /// Base learning rate for fine-tuning
    pub base_learning_rate: f64,
    /// Learning rate scaling factor for frozen->unfrozen transition
    pub lr_scale_factor: f64,
    /// Number of epochs before unfreezing (for gradual unfreezing)
    pub epochs_before_unfreeze: usize,
    /// Layer freezing configuration
    pub freezing: FreezingConfig,
}

impl Default for FineTuningConfig {
    fn default() -> Self {
        Self {
            strategy: TransferStrategy::FineTuneAll,
            base_learning_rate: 1e-4,
            lr_scale_factor: 0.1,
            epochs_before_unfreeze: 5,
            freezing: FreezingConfig::default(),
        }
    }
}

impl FineTuningConfig {
    /// Creates a new fine-tuning configuration.
    pub fn new(strategy: TransferStrategy) -> Self {
        Self {
            strategy,
            ..Default::default()
        }
    }

    /// Creates a configuration for feature extraction.
    pub fn feature_extraction() -> Self {
        Self {
            strategy: TransferStrategy::FeatureExtraction,
            freezing: FreezingConfig::freeze_all(),
            ..Default::default()
        }
    }

    /// Creates a configuration for fine-tuning all layers.
    pub fn fine_tune_all(learning_rate: f64) -> Self {
        Self {
            strategy: TransferStrategy::FineTuneAll,
            base_learning_rate: learning_rate,
            freezing: FreezingConfig::default(),
            ..Default::default()
        }
    }

    /// Creates a configuration for fine-tuning top N layers.
    pub fn fine_tune_top(n: usize, learning_rate: f64) -> Self {
        Self {
            strategy: TransferStrategy::FineTuneTop(n),
            base_learning_rate: learning_rate,
            freezing: FreezingConfig::default(),
            ..Default::default()
        }
    }

    /// Validates the configuration.
    pub fn validate(&self) -> Result<()> {
        if self.base_learning_rate <= 0.0 {
            return Err(Error::invalid_parameter(
                "base_learning_rate",
                self.base_learning_rate,
                "must be positive",
            ));
        }

        if self.lr_scale_factor <= 0.0 || self.lr_scale_factor > 1.0 {
            return Err(Error::invalid_parameter(
                "lr_scale_factor",
                self.lr_scale_factor,
                "must be in (0, 1]",
            ));
        }

        self.freezing.validate()?;

        Ok(())
    }

    /// Gets the learning rate for a specific layer.
    pub fn learning_rate_for_layer(&self, layer_idx: usize, total_layers: usize) -> f64 {
        match self.strategy {
            TransferStrategy::FeatureExtraction => {
                // Frozen layers don't have a learning rate
                if self.freezing.is_frozen(layer_idx) {
                    0.0
                } else {
                    self.base_learning_rate
                }
            }
            TransferStrategy::FineTuneAll => self.base_learning_rate,
            TransferStrategy::FineTuneTop(n) => {
                if layer_idx >= total_layers.saturating_sub(n) {
                    self.base_learning_rate
                } else {
                    0.0
                }
            }
            TransferStrategy::GradualUnfreezing => {
                // Gradually unfreeze from top to bottom
                self.base_learning_rate
            }
            TransferStrategy::DiscriminativeLearning => {
                // Lower learning rates for earlier layers
                let layer_factor = (layer_idx as f64 + 1.0) / (total_layers as f64);
                self.base_learning_rate * self.lr_scale_factor * layer_factor
            }
        }
    }
}

/// Pre-trained model source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PretrainedSource {
    /// ImageNet pre-trained weights
    ImageNet,
    /// COCO pre-trained weights
    COCO,
    /// Custom pre-trained weights from file
    Custom(String),
}

/// Pre-trained model information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PretrainedModel {
    /// Model name
    pub name: String,
    /// Source of pre-trained weights
    pub source: PretrainedSource,
    /// Input size (height, width)
    pub input_size: (usize, usize),
    /// Number of classes in the pre-trained model
    pub num_classes: usize,
    /// Mean values for normalization (per channel)
    pub mean: Vec<f32>,
    /// Std values for normalization (per channel)
    pub std: Vec<f32>,
}

impl PretrainedModel {
    /// Creates ImageNet pre-trained ResNet-18.
    pub fn resnet18_imagenet() -> Self {
        Self {
            name: "ResNet-18".to_string(),
            source: PretrainedSource::ImageNet,
            input_size: (224, 224),
            num_classes: 1000,
            mean: vec![0.485, 0.456, 0.406],
            std: vec![0.229, 0.224, 0.225],
        }
    }

    /// Creates ImageNet pre-trained ResNet-50.
    pub fn resnet50_imagenet() -> Self {
        Self {
            name: "ResNet-50".to_string(),
            source: PretrainedSource::ImageNet,
            input_size: (224, 224),
            num_classes: 1000,
            mean: vec![0.485, 0.456, 0.406],
            std: vec![0.229, 0.224, 0.225],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_freezing_config() {
        let config = FreezingConfig::freeze_all();
        assert!(config.freeze_bn);
        assert!(config.freeze_dropout);

        let config = FreezingConfig::freeze_first_n(5);
        assert!(config.is_frozen(0));
        assert!(config.is_frozen(4));
        assert!(!config.is_frozen(5));

        let config = FreezingConfig::freeze_layers(vec![0, 2, 4]);
        assert!(config.is_frozen(0));
        assert!(!config.is_frozen(1));
        assert!(config.is_frozen(2));
    }

    #[test]
    fn test_freezing_config_validation() {
        let config = FreezingConfig::freeze_layers(vec![]);
        assert!(config.validate().is_err());

        let config = FreezingConfig::freeze_layers(vec![0, 1, 1, 2]);
        assert!(config.validate().is_err());

        let config = FreezingConfig::freeze_first_n(3);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_finetuning_config() {
        let config = FineTuningConfig::feature_extraction();
        assert_eq!(config.strategy, TransferStrategy::FeatureExtraction);

        let config = FineTuningConfig::fine_tune_all(1e-3);
        assert_eq!(config.base_learning_rate, 1e-3);

        let config = FineTuningConfig::fine_tune_top(5, 1e-4);
        assert_eq!(config.strategy, TransferStrategy::FineTuneTop(5));
    }

    #[test]
    fn test_finetuning_config_validation() {
        let mut config = FineTuningConfig::default();
        assert!(config.validate().is_ok());

        config.base_learning_rate = -0.001;
        assert!(config.validate().is_err());

        config.base_learning_rate = 0.001;
        config.lr_scale_factor = 1.5;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_learning_rate_for_layer() {
        let config = FineTuningConfig::fine_tune_all(1e-3);
        assert_eq!(config.learning_rate_for_layer(0, 10), 1e-3);
        assert_eq!(config.learning_rate_for_layer(9, 10), 1e-3);

        let config = FineTuningConfig::fine_tune_top(3, 1e-3);
        assert_eq!(config.learning_rate_for_layer(6, 10), 0.0);
        assert_eq!(config.learning_rate_for_layer(7, 10), 1e-3);
        assert_eq!(config.learning_rate_for_layer(9, 10), 1e-3);
    }

    #[test]
    fn test_pretrained_models() {
        let resnet18 = PretrainedModel::resnet18_imagenet();
        assert_eq!(resnet18.name, "ResNet-18");
        assert_eq!(resnet18.source, PretrainedSource::ImageNet);
        assert_eq!(resnet18.input_size, (224, 224));
        assert_eq!(resnet18.num_classes, 1000);
        assert_eq!(resnet18.mean.len(), 3);
        assert_eq!(resnet18.std.len(), 3);
    }
}
