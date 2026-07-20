//! UNet architecture for semantic segmentation.
//!
//! UNet is a popular encoder-decoder architecture with skip connections,
//! widely used for image segmentation tasks in geospatial applications.

use crate::models::layers::{Conv2dConfig, ConvBlock};
use crate::models::{Activation, Model, ModelMetadata};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};

/// UNet model configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UNetConfig {
    /// Number of input channels (e.g., 3 for RGB, 4 for RGBA, 10+ for multispectral)
    pub in_channels: usize,
    /// Number of output classes
    pub num_classes: usize,
    /// Base number of filters (doubled at each encoder level)
    pub base_filters: usize,
    /// Number of encoder/decoder levels
    pub depth: usize,
    /// Activation function for hidden layers
    pub activation: Activation,
    /// Whether to use batch normalization
    pub use_batch_norm: bool,
    /// Whether to use dropout
    pub use_dropout: bool,
    /// Dropout probability
    pub dropout_p: f64,
}

impl Default for UNetConfig {
    fn default() -> Self {
        Self {
            in_channels: 3,
            num_classes: 2,
            base_filters: 64,
            depth: 4,
            activation: Activation::ReLU,
            use_batch_norm: true,
            use_dropout: false,
            dropout_p: 0.5,
        }
    }
}

impl UNetConfig {
    /// Creates a new UNet configuration.
    pub fn new(in_channels: usize, num_classes: usize) -> Self {
        Self {
            in_channels,
            num_classes,
            ..Self::default()
        }
    }

    /// Sets the base number of filters.
    pub fn with_base_filters(mut self, base_filters: usize) -> Self {
        self.base_filters = base_filters;
        self
    }

    /// Sets the depth (number of encoder/decoder levels).
    pub fn with_depth(mut self, depth: usize) -> Self {
        self.depth = depth;
        self
    }

    /// Enables dropout.
    pub fn with_dropout(mut self, dropout_p: f64) -> Self {
        self.use_dropout = true;
        self.dropout_p = dropout_p;
        self
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

        if self.depth == 0 || self.depth > 6 {
            return Err(Error::invalid_parameter(
                "depth",
                self.depth,
                "must be between 1 and 6",
            ));
        }

        if self.use_dropout && !(0.0..=1.0).contains(&self.dropout_p) {
            return Err(Error::invalid_parameter(
                "dropout_p",
                self.dropout_p,
                "must be in [0, 1]",
            ));
        }

        Ok(())
    }
}

/// UNet encoder block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncoderBlock {
    /// Input channels
    pub in_channels: usize,
    /// Output channels
    pub out_channels: usize,
    /// Convolutional blocks
    pub conv_blocks: Vec<ConvBlock>,
}

impl EncoderBlock {
    /// Creates a new encoder block.
    pub fn new(in_channels: usize, out_channels: usize, activation: Activation) -> Self {
        let conv_blocks = vec![
            ConvBlock::new(in_channels, out_channels, 3, activation),
            ConvBlock::new(out_channels, out_channels, 3, activation),
        ];

        Self {
            in_channels,
            out_channels,
            conv_blocks,
        }
    }

    /// Computes the number of parameters.
    pub fn num_parameters(&self) -> usize {
        self.conv_blocks.iter().map(|b| b.num_parameters()).sum()
    }
}

/// UNet decoder block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecoderBlock {
    /// Input channels from skip connection
    pub in_channels_skip: usize,
    /// Input channels from upsampled feature
    pub in_channels_up: usize,
    /// Output channels
    pub out_channels: usize,
    /// Upsampling configuration
    pub upsample_config: Conv2dConfig,
    /// Convolutional blocks
    pub conv_blocks: Vec<ConvBlock>,
}

impl DecoderBlock {
    /// Creates a new decoder block.
    pub fn new(
        in_channels_skip: usize,
        in_channels_up: usize,
        out_channels: usize,
        activation: Activation,
    ) -> Self {
        // Transposed convolution for upsampling
        let upsample_config = Conv2dConfig {
            in_channels: in_channels_up,
            out_channels,
            kernel_size: (2, 2),
            stride: (2, 2),
            padding: (0, 0),
            dilation: (1, 1),
            bias: true,
        };

        // After concatenation with skip connection
        let concat_channels = in_channels_skip + out_channels;

        let conv_blocks = vec![
            ConvBlock::new(concat_channels, out_channels, 3, activation),
            ConvBlock::new(out_channels, out_channels, 3, activation),
        ];

        Self {
            in_channels_skip,
            in_channels_up,
            out_channels,
            upsample_config,
            conv_blocks,
        }
    }

    /// Computes the number of parameters.
    pub fn num_parameters(&self) -> usize {
        let upsample_params = self.upsample_config.num_parameters();
        let conv_params: usize = self.conv_blocks.iter().map(|b| b.num_parameters()).sum();
        upsample_params + conv_params
    }
}

/// UNet model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UNet {
    /// Model configuration
    pub config: UNetConfig,
    /// Encoder blocks
    pub encoder_blocks: Vec<EncoderBlock>,
    /// Bottleneck block
    pub bottleneck: EncoderBlock,
    /// Decoder blocks
    pub decoder_blocks: Vec<DecoderBlock>,
    /// Output convolution
    pub output_conv: Conv2dConfig,
}

impl UNet {
    /// Creates a new UNet model.
    pub fn new(in_channels: usize, num_classes: usize, depth: usize) -> Result<Self> {
        let config = UNetConfig::new(in_channels, num_classes).with_depth(depth);
        config.validate()?;

        Self::from_config(config)
    }

    /// Creates a UNet from a configuration.
    pub fn from_config(config: UNetConfig) -> Result<Self> {
        config.validate()?;

        let mut encoder_blocks = Vec::new();
        let mut in_ch = config.in_channels;

        // Build encoder
        for i in 0..config.depth {
            let out_ch = config.base_filters * (1 << i);
            encoder_blocks.push(EncoderBlock::new(in_ch, out_ch, config.activation));
            in_ch = out_ch;
        }

        // Bottleneck
        let bottleneck_ch = config.base_filters * (1 << config.depth);
        let bottleneck = EncoderBlock::new(in_ch, bottleneck_ch, config.activation);

        // Build decoder
        let mut decoder_blocks = Vec::new();
        in_ch = bottleneck_ch;

        for i in (0..config.depth).rev() {
            let out_ch = config.base_filters * (1 << i);
            let skip_ch = out_ch; // From corresponding encoder
            decoder_blocks.push(DecoderBlock::new(skip_ch, in_ch, out_ch, config.activation));
            in_ch = out_ch;
        }

        // Output convolution (1x1 conv to num_classes)
        let output_conv = Conv2dConfig {
            in_channels: in_ch,
            out_channels: config.num_classes,
            kernel_size: (1, 1),
            stride: (1, 1),
            padding: (0, 0),
            dilation: (1, 1),
            bias: true,
        };

        Ok(Self {
            config,
            encoder_blocks,
            bottleneck,
            decoder_blocks,
            output_conv,
        })
    }

    /// Creates a small UNet for testing.
    pub fn small(in_channels: usize, num_classes: usize) -> Result<Self> {
        Self::new(in_channels, num_classes, 2)
    }

    /// Creates a standard UNet.
    pub fn standard(in_channels: usize, num_classes: usize) -> Result<Self> {
        Self::new(in_channels, num_classes, 4)
    }

    /// Creates a deep UNet.
    pub fn deep(in_channels: usize, num_classes: usize) -> Result<Self> {
        Self::new(in_channels, num_classes, 5)
    }
}

impl Model for UNet {
    fn name(&self) -> &str {
        "UNet"
    }

    fn metadata(&self) -> ModelMetadata {
        ModelMetadata {
            name: "UNet".to_string(),
            architecture: format!("UNet-D{}", self.config.depth),
            in_channels: self.config.in_channels,
            out_channels: self.config.num_classes,
            num_parameters: self.num_parameters(),
            config: serde_json::to_string(&self.config).ok(),
        }
    }

    fn num_parameters(&self) -> usize {
        let encoder_params: usize = self.encoder_blocks.iter().map(|b| b.num_parameters()).sum();

        let bottleneck_params = self.bottleneck.num_parameters();

        let decoder_params: usize = self.decoder_blocks.iter().map(|b| b.num_parameters()).sum();

        let output_params = self.output_conv.num_parameters();

        encoder_params + bottleneck_params + decoder_params + output_params
    }

    fn validate(&self) -> Result<()> {
        self.config.validate()?;

        if self.encoder_blocks.len() != self.config.depth {
            return Err(Error::ModelArchitecture(format!(
                "Expected {} encoder blocks, got {}",
                self.config.depth,
                self.encoder_blocks.len()
            )));
        }

        if self.decoder_blocks.len() != self.config.depth {
            return Err(Error::ModelArchitecture(format!(
                "Expected {} decoder blocks, got {}",
                self.config.depth,
                self.decoder_blocks.len()
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unet_config_validation() {
        let config = UNetConfig::new(3, 10);
        assert!(config.validate().is_ok());

        let invalid_config = UNetConfig {
            in_channels: 0,
            ..Default::default()
        };
        assert!(invalid_config.validate().is_err());

        let invalid_depth = UNetConfig {
            depth: 10,
            ..Default::default()
        };
        assert!(invalid_depth.validate().is_err());
    }

    #[test]
    fn test_encoder_block() {
        let block = EncoderBlock::new(64, 128, Activation::ReLU);
        assert_eq!(block.in_channels, 64);
        assert_eq!(block.out_channels, 128);
        assert_eq!(block.conv_blocks.len(), 2);
        assert!(block.num_parameters() > 0);
    }

    #[test]
    fn test_decoder_block() {
        let block = DecoderBlock::new(128, 256, 128, Activation::ReLU);
        assert_eq!(block.in_channels_skip, 128);
        assert_eq!(block.in_channels_up, 256);
        assert_eq!(block.out_channels, 128);
        assert!(block.num_parameters() > 0);
    }

    #[test]
    fn test_unet_creation() {
        let unet = UNet::new(3, 10, 4).expect("Failed to create UNet");
        assert_eq!(unet.config.in_channels, 3);
        assert_eq!(unet.config.num_classes, 10);
        assert_eq!(unet.config.depth, 4);
        assert_eq!(unet.encoder_blocks.len(), 4);
        assert_eq!(unet.decoder_blocks.len(), 4);
    }

    #[test]
    fn test_unet_validation() {
        let unet = UNet::standard(3, 10).expect("Failed to create UNet");
        assert!(unet.validate().is_ok());
    }

    #[test]
    fn test_unet_num_parameters() {
        let unet = UNet::small(3, 2).expect("Failed to create UNet");
        let num_params = unet.num_parameters();
        assert!(num_params > 0);
        assert!(num_params < 10_000_000); // Reasonable range for small model
    }

    #[test]
    fn test_unet_metadata() {
        let unet = UNet::standard(3, 10).expect("Failed to create UNet");
        let metadata = unet.metadata();
        assert_eq!(metadata.name, "UNet");
        assert_eq!(metadata.in_channels, 3);
        assert_eq!(metadata.out_channels, 10);
        assert!(metadata.num_parameters > 0);
    }

    #[test]
    fn test_unet_variants() {
        let small = UNet::small(3, 5).expect("Failed to create small UNet");
        let standard = UNet::standard(3, 5).expect("Failed to create standard UNet");
        let deep = UNet::deep(3, 5).expect("Failed to create deep UNet");

        assert!(small.num_parameters() < standard.num_parameters());
        assert!(standard.num_parameters() < deep.num_parameters());
    }
}
