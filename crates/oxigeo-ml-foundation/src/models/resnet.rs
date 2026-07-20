//! ResNet architecture for image classification.
//!
//! ResNet (Residual Network) is a deep convolutional neural network with
//! skip connections, enabling training of very deep networks.

use crate::models::layers::{Conv2dConfig, PoolingConfig, ResidualBlock};
use crate::models::{Activation, Model, ModelMetadata};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};

/// ResNet variant types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResNetVariant {
    /// ResNet-18 (2-2-2-2 blocks)
    ResNet18,
    /// ResNet-34 (3-4-6-3 blocks)
    ResNet34,
    /// ResNet-50 (3-4-6-3 blocks with bottleneck)
    ResNet50,
    /// ResNet-101 (3-4-23-3 blocks with bottleneck)
    ResNet101,
    /// ResNet-152 (3-8-36-3 blocks with bottleneck)
    ResNet152,
}

impl ResNetVariant {
    /// Returns the number of blocks for each stage.
    pub fn blocks_per_stage(&self) -> Vec<usize> {
        match self {
            Self::ResNet18 => vec![2, 2, 2, 2],
            Self::ResNet34 => vec![3, 4, 6, 3],
            Self::ResNet50 => vec![3, 4, 6, 3],
            Self::ResNet101 => vec![3, 4, 23, 3],
            Self::ResNet152 => vec![3, 8, 36, 3],
        }
    }

    /// Returns whether to use bottleneck blocks.
    pub fn use_bottleneck(&self) -> bool {
        matches!(self, Self::ResNet50 | Self::ResNet101 | Self::ResNet152)
    }

    /// Returns the name of the variant.
    pub fn name(&self) -> &str {
        match self {
            Self::ResNet18 => "ResNet-18",
            Self::ResNet34 => "ResNet-34",
            Self::ResNet50 => "ResNet-50",
            Self::ResNet101 => "ResNet-101",
            Self::ResNet152 => "ResNet-152",
        }
    }
}

/// ResNet model configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResNetConfig {
    /// Model variant
    pub variant: ResNetVariant,
    /// Number of input channels
    pub in_channels: usize,
    /// Number of output classes
    pub num_classes: usize,
    /// Base number of filters
    pub base_filters: usize,
    /// Activation function
    pub activation: Activation,
    /// Whether to use global average pooling before FC layer
    pub use_global_pool: bool,
}

impl Default for ResNetConfig {
    fn default() -> Self {
        Self {
            variant: ResNetVariant::ResNet18,
            in_channels: 3,
            num_classes: 1000,
            base_filters: 64,
            activation: Activation::ReLU,
            use_global_pool: true,
        }
    }
}

impl ResNetConfig {
    /// Creates a new ResNet configuration.
    pub fn new(variant: ResNetVariant, in_channels: usize, num_classes: usize) -> Self {
        Self {
            variant,
            in_channels,
            num_classes,
            ..Self::default()
        }
    }

    /// Validates the configuration.
    pub fn validate(&self) -> Result<()> {
        if self.in_channels == 0 {
            return Err(Error::invalid_parameter(
                "in_channels",
                self.in_channels,
                "must be positive",
            ));
        }

        if self.num_classes == 0 {
            return Err(Error::invalid_parameter(
                "num_classes",
                self.num_classes,
                "must be positive",
            ));
        }

        if self.base_filters == 0 {
            return Err(Error::invalid_parameter(
                "base_filters",
                self.base_filters,
                "must be positive",
            ));
        }

        Ok(())
    }
}

/// ResNet stage (group of residual blocks).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResNetStage {
    /// Stage index (0-3)
    pub stage_idx: usize,
    /// Number of blocks in this stage
    pub num_blocks: usize,
    /// Input channels
    pub in_channels: usize,
    /// Output channels
    pub out_channels: usize,
    /// Stride for the first block
    pub stride: usize,
    /// Residual blocks
    pub blocks: Vec<ResidualBlock>,
}

impl ResNetStage {
    /// Creates a new ResNet stage.
    pub fn new(
        stage_idx: usize,
        num_blocks: usize,
        in_channels: usize,
        out_channels: usize,
        stride: usize,
        bottleneck: bool,
    ) -> Self {
        let mut blocks = Vec::new();

        // First block may downsample
        if bottleneck {
            blocks.push(ResidualBlock::new_bottleneck(
                in_channels,
                out_channels,
                stride,
            ));
        } else {
            blocks.push(ResidualBlock::new(in_channels, out_channels, stride));
        }

        // Remaining blocks maintain spatial dimensions
        for _ in 1..num_blocks {
            if bottleneck {
                blocks.push(ResidualBlock::new_bottleneck(out_channels, out_channels, 1));
            } else {
                blocks.push(ResidualBlock::new(out_channels, out_channels, 1));
            }
        }

        Self {
            stage_idx,
            num_blocks,
            in_channels,
            out_channels,
            stride,
            blocks,
        }
    }

    /// Computes the number of parameters.
    pub fn num_parameters(&self) -> usize {
        self.blocks.iter().map(|b| b.num_parameters()).sum()
    }
}

/// ResNet model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResNet {
    /// Model configuration
    pub config: ResNetConfig,
    /// Initial convolution
    pub conv1: Conv2dConfig,
    /// Initial pooling
    pub pool1: PoolingConfig,
    /// Stages (residual block groups)
    pub stages: Vec<ResNetStage>,
    /// Global pooling configuration
    pub global_pool: Option<PoolingConfig>,
    /// Fully connected layer input features
    pub fc_in_features: usize,
}

impl ResNet {
    /// Creates a new ResNet model.
    pub fn new(variant: ResNetVariant, in_channels: usize, num_classes: usize) -> Result<Self> {
        let config = ResNetConfig::new(variant, in_channels, num_classes);
        config.validate()?;

        Self::from_config(config)
    }

    /// Creates a ResNet from a configuration.
    pub fn from_config(config: ResNetConfig) -> Result<Self> {
        config.validate()?;

        // Initial 7x7 convolution with stride 2
        let conv1 = Conv2dConfig {
            in_channels: config.in_channels,
            out_channels: config.base_filters,
            kernel_size: (7, 7),
            stride: (2, 2),
            padding: (3, 3),
            dilation: (1, 1),
            bias: false,
        };

        // Max pooling with kernel 3x3, stride 2
        let pool1 = PoolingConfig::new(3).with_stride(2).with_padding(1);

        // Build stages
        let blocks_per_stage = config.variant.blocks_per_stage();
        let use_bottleneck = config.variant.use_bottleneck();
        let mut stages = Vec::new();
        let mut in_ch = config.base_filters;

        for (stage_idx, &num_blocks) in blocks_per_stage.iter().enumerate() {
            // For bottleneck blocks, output channels are 4x the base
            let base_ch = config.base_filters * (1 << stage_idx);
            let out_ch = if use_bottleneck { base_ch * 4 } else { base_ch };
            let stride = if stage_idx == 0 { 1 } else { 2 };

            stages.push(ResNetStage::new(
                stage_idx,
                num_blocks,
                in_ch,
                out_ch,
                stride,
                use_bottleneck,
            ));

            in_ch = out_ch;
        }

        // Global average pooling
        let global_pool = if config.use_global_pool {
            Some(PoolingConfig::new(1))
        } else {
            None
        };

        // FC layer input features
        let fc_in_features = in_ch;

        Ok(Self {
            config,
            conv1,
            pool1,
            stages,
            global_pool,
            fc_in_features,
        })
    }

    /// Creates ResNet-18.
    pub fn resnet18(in_channels: usize, num_classes: usize) -> Result<Self> {
        Self::new(ResNetVariant::ResNet18, in_channels, num_classes)
    }

    /// Creates ResNet-34.
    pub fn resnet34(in_channels: usize, num_classes: usize) -> Result<Self> {
        Self::new(ResNetVariant::ResNet34, in_channels, num_classes)
    }

    /// Creates ResNet-50.
    pub fn resnet50(in_channels: usize, num_classes: usize) -> Result<Self> {
        Self::new(ResNetVariant::ResNet50, in_channels, num_classes)
    }

    /// Creates ResNet-101.
    pub fn resnet101(in_channels: usize, num_classes: usize) -> Result<Self> {
        Self::new(ResNetVariant::ResNet101, in_channels, num_classes)
    }

    /// Creates ResNet-152.
    pub fn resnet152(in_channels: usize, num_classes: usize) -> Result<Self> {
        Self::new(ResNetVariant::ResNet152, in_channels, num_classes)
    }

    /// Computes the number of FC layer parameters.
    fn fc_num_parameters(&self) -> usize {
        self.fc_in_features * self.config.num_classes + self.config.num_classes
    }
}

impl Model for ResNet {
    fn name(&self) -> &str {
        self.config.variant.name()
    }

    fn metadata(&self) -> ModelMetadata {
        ModelMetadata {
            name: self.config.variant.name().to_string(),
            architecture: format!("ResNet-{}", self.name()),
            in_channels: self.config.in_channels,
            out_channels: self.config.num_classes,
            num_parameters: self.num_parameters(),
            config: serde_json::to_string(&self.config).ok(),
        }
    }

    fn num_parameters(&self) -> usize {
        let conv1_params = self.conv1.num_parameters();

        let stages_params: usize = self.stages.iter().map(|s| s.num_parameters()).sum();

        let fc_params = self.fc_num_parameters();

        conv1_params + stages_params + fc_params
    }

    fn validate(&self) -> Result<()> {
        self.config.validate()?;

        let expected_stages = self.config.variant.blocks_per_stage().len();
        if self.stages.len() != expected_stages {
            return Err(Error::ModelArchitecture(format!(
                "Expected {} stages, got {}",
                expected_stages,
                self.stages.len()
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resnet_variant() {
        assert_eq!(ResNetVariant::ResNet18.blocks_per_stage(), vec![2, 2, 2, 2]);
        assert_eq!(ResNetVariant::ResNet34.blocks_per_stage(), vec![3, 4, 6, 3]);
        assert_eq!(ResNetVariant::ResNet50.blocks_per_stage(), vec![3, 4, 6, 3]);

        assert!(!ResNetVariant::ResNet18.use_bottleneck());
        assert!(ResNetVariant::ResNet50.use_bottleneck());
    }

    #[test]
    fn test_resnet_config() {
        let config = ResNetConfig::new(ResNetVariant::ResNet18, 3, 10);
        assert_eq!(config.in_channels, 3);
        assert_eq!(config.num_classes, 10);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_resnet_stage() {
        let stage = ResNetStage::new(0, 2, 64, 64, 1, false);
        assert_eq!(stage.num_blocks, 2);
        assert_eq!(stage.blocks.len(), 2);
        assert!(stage.num_parameters() > 0);

        // Test bottleneck stage
        let bottleneck_stage = ResNetStage::new(0, 2, 64, 256, 1, true);
        assert_eq!(bottleneck_stage.num_blocks, 2);
        assert_eq!(bottleneck_stage.blocks.len(), 2);
        assert!(bottleneck_stage.num_parameters() > 0);
    }

    #[test]
    fn test_resnet18_creation() {
        let resnet = ResNet::resnet18(3, 10).expect("Failed to create ResNet-18");
        assert_eq!(resnet.config.variant, ResNetVariant::ResNet18);
        assert_eq!(resnet.config.in_channels, 3);
        assert_eq!(resnet.config.num_classes, 10);
        assert_eq!(resnet.stages.len(), 4);
    }

    #[test]
    fn test_resnet_validation() {
        let resnet = ResNet::resnet18(3, 10).expect("Failed to create ResNet");
        assert!(resnet.validate().is_ok());
    }

    #[test]
    fn test_resnet_num_parameters() {
        let resnet18 = ResNet::resnet18(3, 1000).expect("Failed to create ResNet-18");
        let resnet34 = ResNet::resnet34(3, 1000).expect("Failed to create ResNet-34");
        let resnet50 = ResNet::resnet50(3, 1000).expect("Failed to create ResNet-50");

        let params18 = resnet18.num_parameters();
        let params34 = resnet34.num_parameters();
        let params50 = resnet50.num_parameters();

        assert!(params18 > 0);
        assert!(params34 > params18);
        assert!(params50 > params34);

        // ResNet-18 should have approximately 11M parameters
        // (relaxed check for our implementation)
        assert!(params18 > 5_000_000);
        assert!(params18 < 20_000_000);
    }

    #[test]
    fn test_resnet_metadata() {
        let resnet = ResNet::resnet50(3, 1000).expect("Failed to create ResNet-50");
        let metadata = resnet.metadata();
        assert_eq!(metadata.name, "ResNet-50");
        assert_eq!(metadata.in_channels, 3);
        assert_eq!(metadata.out_channels, 1000);
        assert!(metadata.num_parameters > 0);
    }

    #[test]
    fn test_resnet_variants() {
        let r18 = ResNet::resnet18(3, 100).expect("Failed to create ResNet-18");
        let r34 = ResNet::resnet34(3, 100).expect("Failed to create ResNet-34");
        let r50 = ResNet::resnet50(3, 100).expect("Failed to create ResNet-50");
        let r101 = ResNet::resnet101(3, 100).expect("Failed to create ResNet-101");
        let r152 = ResNet::resnet152(3, 100).expect("Failed to create ResNet-152");

        // Verify that basic block models use basic blocks
        assert!(!r18.stages[0].blocks[0].bottleneck);
        assert!(!r34.stages[0].blocks[0].bottleneck);

        // Verify that bottleneck models use bottleneck blocks
        assert!(r50.stages[0].blocks[0].bottleneck);
        assert!(r101.stages[0].blocks[0].bottleneck);
        assert!(r152.stages[0].blocks[0].bottleneck);

        assert!(r18.num_parameters() < r34.num_parameters());
        assert!(r34.num_parameters() < r50.num_parameters());
        assert!(r50.num_parameters() < r101.num_parameters());
        assert!(r101.num_parameters() < r152.num_parameters());
    }
}
