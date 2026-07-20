//! Common neural network layers and building blocks.

use crate::models::{Activation, Normalization};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};

/// Convolutional layer configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conv2dConfig {
    /// Number of input channels
    pub in_channels: usize,
    /// Number of output channels
    pub out_channels: usize,
    /// Kernel size (height, width)
    pub kernel_size: (usize, usize),
    /// Stride (height, width)
    pub stride: (usize, usize),
    /// Padding (height, width)
    pub padding: (usize, usize),
    /// Dilation (height, width)
    pub dilation: (usize, usize),
    /// Whether to use bias
    pub bias: bool,
}

impl Default for Conv2dConfig {
    fn default() -> Self {
        Self {
            in_channels: 3,
            out_channels: 64,
            kernel_size: (3, 3),
            stride: (1, 1),
            padding: (1, 1),
            dilation: (1, 1),
            bias: true,
        }
    }
}

impl Conv2dConfig {
    /// Creates a new Conv2d configuration.
    pub fn new(in_channels: usize, out_channels: usize, kernel_size: usize) -> Self {
        Self {
            in_channels,
            out_channels,
            kernel_size: (kernel_size, kernel_size),
            stride: (1, 1),
            padding: (kernel_size / 2, kernel_size / 2),
            dilation: (1, 1),
            bias: true,
        }
    }

    /// Sets the stride.
    pub fn with_stride(mut self, stride: usize) -> Self {
        self.stride = (stride, stride);
        self
    }

    /// Sets the padding.
    pub fn with_padding(mut self, padding: usize) -> Self {
        self.padding = (padding, padding);
        self
    }

    /// Disables bias.
    pub fn without_bias(mut self) -> Self {
        self.bias = false;
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

        if self.out_channels == 0 {
            return Err(Error::invalid_parameter(
                "out_channels",
                self.out_channels,
                "must be positive",
            ));
        }

        if self.kernel_size.0 == 0 || self.kernel_size.1 == 0 {
            return Err(Error::invalid_parameter(
                "kernel_size",
                format!("{:?}", self.kernel_size),
                "must be positive",
            ));
        }

        if self.stride.0 == 0 || self.stride.1 == 0 {
            return Err(Error::invalid_parameter(
                "stride",
                format!("{:?}", self.stride),
                "must be positive",
            ));
        }

        Ok(())
    }

    /// Computes the number of parameters.
    pub fn num_parameters(&self) -> usize {
        let weight_params =
            self.in_channels * self.out_channels * self.kernel_size.0 * self.kernel_size.1;
        let bias_params = if self.bias { self.out_channels } else { 0 };
        weight_params + bias_params
    }

    /// Computes the output spatial dimensions given input dimensions.
    pub fn output_size(&self, input_height: usize, input_width: usize) -> (usize, usize) {
        let out_h =
            (input_height + 2 * self.padding.0 - self.dilation.0 * (self.kernel_size.0 - 1) - 1)
                / self.stride.0
                + 1;

        let out_w =
            (input_width + 2 * self.padding.1 - self.dilation.1 * (self.kernel_size.1 - 1) - 1)
                / self.stride.1
                + 1;

        (out_h, out_w)
    }
}

/// Batch normalization layer configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchNormConfig {
    /// Number of features (channels)
    pub num_features: usize,
    /// Small constant for numerical stability
    pub eps: f64,
    /// Momentum for running mean/variance
    pub momentum: f64,
    /// Whether to use affine transformation (learnable scale and shift)
    pub affine: bool,
}

impl BatchNormConfig {
    /// Creates a new batch normalization configuration.
    pub fn new(num_features: usize) -> Self {
        Self {
            num_features,
            eps: 1e-5,
            momentum: 0.1,
            affine: true,
        }
    }

    /// Validates the configuration.
    pub fn validate(&self) -> Result<()> {
        if self.num_features == 0 {
            return Err(Error::invalid_parameter(
                "num_features",
                self.num_features,
                "must be positive",
            ));
        }

        if self.eps <= 0.0 {
            return Err(Error::invalid_parameter(
                "eps",
                self.eps,
                "must be positive",
            ));
        }

        if !(0.0..=1.0).contains(&self.momentum) {
            return Err(Error::invalid_parameter(
                "momentum",
                self.momentum,
                "must be in [0, 1]",
            ));
        }

        Ok(())
    }

    /// Computes the number of parameters.
    pub fn num_parameters(&self) -> usize {
        if self.affine {
            // Scale and shift parameters
            self.num_features * 2
        } else {
            0
        }
    }
}

/// Pooling layer configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolingConfig {
    /// Kernel size
    pub kernel_size: usize,
    /// Stride
    pub stride: usize,
    /// Padding
    pub padding: usize,
}

impl PoolingConfig {
    /// Creates a new pooling configuration.
    pub fn new(kernel_size: usize) -> Self {
        Self {
            kernel_size,
            stride: kernel_size,
            padding: 0,
        }
    }

    /// Sets the stride.
    pub fn with_stride(mut self, stride: usize) -> Self {
        self.stride = stride;
        self
    }

    /// Sets the padding.
    pub fn with_padding(mut self, padding: usize) -> Self {
        self.padding = padding;
        self
    }

    /// Computes the output spatial dimensions given input dimensions.
    pub fn output_size(&self, input_height: usize, input_width: usize) -> (usize, usize) {
        let out_h = (input_height + 2 * self.padding - self.kernel_size) / self.stride + 1;
        let out_w = (input_width + 2 * self.padding - self.kernel_size) / self.stride + 1;
        (out_h, out_w)
    }
}

/// Convolutional block (Conv + BatchNorm + Activation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvBlock {
    /// Convolution configuration
    pub conv: Conv2dConfig,
    /// Normalization type
    pub norm: Normalization,
    /// Activation function
    pub activation: Activation,
}

impl ConvBlock {
    /// Creates a new convolutional block.
    pub fn new(
        in_channels: usize,
        out_channels: usize,
        kernel_size: usize,
        activation: Activation,
    ) -> Self {
        Self {
            conv: Conv2dConfig::new(in_channels, out_channels, kernel_size),
            norm: Normalization::BatchNorm,
            activation,
        }
    }

    /// Creates a conv block with stride.
    pub fn with_stride(
        in_channels: usize,
        out_channels: usize,
        kernel_size: usize,
        stride: usize,
        activation: Activation,
    ) -> Self {
        Self {
            conv: Conv2dConfig::new(in_channels, out_channels, kernel_size).with_stride(stride),
            norm: Normalization::BatchNorm,
            activation,
        }
    }

    /// Validates the block configuration.
    pub fn validate(&self) -> Result<()> {
        self.conv.validate()
    }

    /// Computes the number of parameters.
    pub fn num_parameters(&self) -> usize {
        let conv_params = self.conv.num_parameters();
        let norm_params = match self.norm {
            Normalization::BatchNorm => {
                BatchNormConfig::new(self.conv.out_channels).num_parameters()
            }
            _ => 0,
        };
        conv_params + norm_params
    }
}

/// Residual block for ResNet-style architectures.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResidualBlock {
    /// Number of input channels
    pub in_channels: usize,
    /// Number of output channels
    pub out_channels: usize,
    /// Stride
    pub stride: usize,
    /// Activation function
    pub activation: Activation,
    /// Whether to use downsample
    pub downsample: bool,
    /// Whether to use bottleneck architecture (1x1 -> 3x3 -> 1x1)
    pub bottleneck: bool,
}

impl ResidualBlock {
    /// Creates a new residual block (basic block by default).
    pub fn new(in_channels: usize, out_channels: usize, stride: usize) -> Self {
        let downsample = stride != 1 || in_channels != out_channels;
        Self {
            in_channels,
            out_channels,
            stride,
            activation: Activation::ReLU,
            downsample,
            bottleneck: false,
        }
    }

    /// Creates a new residual block with bottleneck architecture.
    pub fn new_bottleneck(in_channels: usize, out_channels: usize, stride: usize) -> Self {
        let downsample = stride != 1 || in_channels != out_channels;
        Self {
            in_channels,
            out_channels,
            stride,
            activation: Activation::ReLU,
            downsample,
            bottleneck: true,
        }
    }

    /// Validates the block configuration.
    pub fn validate(&self) -> Result<()> {
        if self.in_channels == 0 {
            return Err(Error::invalid_parameter(
                "in_channels",
                self.in_channels,
                "must be positive",
            ));
        }

        if self.out_channels == 0 {
            return Err(Error::invalid_parameter(
                "out_channels",
                self.out_channels,
                "must be positive",
            ));
        }

        if self.stride == 0 {
            return Err(Error::invalid_parameter(
                "stride",
                self.stride,
                "must be positive",
            ));
        }

        Ok(())
    }

    /// Computes the number of parameters.
    pub fn num_parameters(&self) -> usize {
        if self.bottleneck {
            // Bottleneck block: 1x1 -> 3x3 -> 1x1
            let mid_channels = self.out_channels / 4;

            let conv1_params = Conv2dConfig::new(self.in_channels, mid_channels, 1)
                .without_bias()
                .num_parameters();
            let bn1_params = BatchNormConfig::new(mid_channels).num_parameters();

            let conv2_params = Conv2dConfig::new(mid_channels, mid_channels, 3)
                .without_bias()
                .num_parameters();
            let bn2_params = BatchNormConfig::new(mid_channels).num_parameters();

            let conv3_params = Conv2dConfig::new(mid_channels, self.out_channels, 1)
                .without_bias()
                .num_parameters();
            let bn3_params = BatchNormConfig::new(self.out_channels).num_parameters();

            let downsample_params = if self.downsample {
                let ds_conv = Conv2dConfig::new(self.in_channels, self.out_channels, 1)
                    .without_bias()
                    .num_parameters();
                let ds_bn = BatchNormConfig::new(self.out_channels).num_parameters();
                ds_conv + ds_bn
            } else {
                0
            };

            conv1_params
                + bn1_params
                + conv2_params
                + bn2_params
                + conv3_params
                + bn3_params
                + downsample_params
        } else {
            // Basic block: 3x3 -> 3x3
            let conv1_params = Conv2dConfig::new(self.in_channels, self.out_channels, 3)
                .without_bias()
                .num_parameters();
            let bn1_params = BatchNormConfig::new(self.out_channels).num_parameters();

            let conv2_params = Conv2dConfig::new(self.out_channels, self.out_channels, 3)
                .without_bias()
                .num_parameters();
            let bn2_params = BatchNormConfig::new(self.out_channels).num_parameters();

            let downsample_params = if self.downsample {
                let ds_conv = Conv2dConfig::new(self.in_channels, self.out_channels, 1)
                    .without_bias()
                    .num_parameters();
                let ds_bn = BatchNormConfig::new(self.out_channels).num_parameters();
                ds_conv + ds_bn
            } else {
                0
            };

            conv1_params + bn1_params + conv2_params + bn2_params + downsample_params
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conv2d_config() {
        let config = Conv2dConfig::new(3, 64, 3);
        assert_eq!(config.in_channels, 3);
        assert_eq!(config.out_channels, 64);
        assert_eq!(config.kernel_size, (3, 3));
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_conv2d_num_parameters() {
        let config = Conv2dConfig::new(3, 64, 3);
        let params = config.num_parameters();
        // 3 * 64 * 3 * 3 + 64 (bias) = 1792
        assert_eq!(params, 1792);

        let config_no_bias = config.without_bias();
        let params_no_bias = config_no_bias.num_parameters();
        // 3 * 64 * 3 * 3 = 1728
        assert_eq!(params_no_bias, 1728);
    }

    #[test]
    fn test_conv2d_output_size() {
        let config = Conv2dConfig::new(3, 64, 3).with_stride(1).with_padding(1);
        let (out_h, out_w) = config.output_size(224, 224);
        assert_eq!(out_h, 224);
        assert_eq!(out_w, 224);

        let config_stride2 = config.with_stride(2);
        let (out_h, out_w) = config_stride2.output_size(224, 224);
        assert_eq!(out_h, 112);
        assert_eq!(out_w, 112);
    }

    #[test]
    fn test_batchnorm_config() {
        let config = BatchNormConfig::new(64);
        assert_eq!(config.num_features, 64);
        assert!(config.validate().is_ok());

        let params = config.num_parameters();
        // 64 scale + 64 shift = 128
        assert_eq!(params, 128);
    }

    #[test]
    fn test_pooling_config() {
        let config = PoolingConfig::new(2);
        assert_eq!(config.kernel_size, 2);
        assert_eq!(config.stride, 2);

        let (out_h, out_w) = config.output_size(224, 224);
        assert_eq!(out_h, 112);
        assert_eq!(out_w, 112);
    }

    #[test]
    fn test_conv_block() {
        let block = ConvBlock::new(3, 64, 3, Activation::ReLU);
        assert!(block.validate().is_ok());

        let params = block.num_parameters();
        // Conv: 3*64*3*3 + 64 = 1792, BN: 64*2 = 128
        assert_eq!(params, 1920);
    }

    #[test]
    fn test_residual_block() {
        let block = ResidualBlock::new(64, 128, 2);
        assert!(block.validate().is_ok());
        assert!(block.downsample);

        let params = block.num_parameters();
        assert!(params > 0);
    }
}
