//! Neural network model architectures.
//!
//! Provides common architectures for geospatial machine learning:
//! - UNet for segmentation
//! - ResNet for classification
//! - Common layers and building blocks

pub mod layers;
pub mod lstm;
pub mod resnet;
pub mod transformer;
pub mod unet;

use crate::Result;
use serde::{Deserialize, Serialize};

/// Model metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMetadata {
    /// Model name
    pub name: String,
    /// Model architecture type
    pub architecture: String,
    /// Number of input channels
    pub in_channels: usize,
    /// Number of output classes/channels
    pub out_channels: usize,
    /// Total number of parameters
    pub num_parameters: usize,
    /// Additional configuration (JSON string)
    pub config: Option<String>,
}

/// Trait for neural network models.
pub trait Model: Send + Sync {
    /// Returns the model name.
    fn name(&self) -> &str;

    /// Returns the model metadata.
    fn metadata(&self) -> ModelMetadata;

    /// Computes the total number of parameters in the model.
    fn num_parameters(&self) -> usize;

    /// Validates the model configuration.
    fn validate(&self) -> Result<()>;
}

/// Activation function types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Activation {
    /// Rectified Linear Unit
    ReLU,
    /// Leaky ReLU
    LeakyReLU,
    /// Exponential Linear Unit
    ELU,
    /// Gaussian Error Linear Unit
    GELU,
    /// Sigmoid
    Sigmoid,
    /// Hyperbolic tangent
    Tanh,
    /// Softmax (for output layers)
    Softmax,
    /// No activation (identity)
    Identity,
}

impl Activation {
    /// Applies the activation function to a value.
    pub fn apply(&self, x: f32) -> f32 {
        match self {
            Self::ReLU => x.max(0.0),
            Self::LeakyReLU => {
                if x > 0.0 {
                    x
                } else {
                    0.01 * x
                }
            }
            Self::ELU => {
                if x > 0.0 {
                    x
                } else {
                    x.exp() - 1.0
                }
            }
            Self::GELU => {
                // Approximate GELU
                0.5 * x
                    * (1.0
                        + ((2.0 / std::f32::consts::PI).sqrt() * (x + 0.044715 * x.powi(3))).tanh())
            }
            Self::Sigmoid => 1.0 / (1.0 + (-x).exp()),
            Self::Tanh => x.tanh(),
            Self::Softmax => {
                // Note: Softmax requires all values in the batch
                // This is a placeholder for single value
                x.exp()
            }
            Self::Identity => x,
        }
    }

    /// Returns the derivative of the activation function.
    pub fn derivative(&self, x: f32) -> f32 {
        match self {
            Self::ReLU => {
                if x > 0.0 {
                    1.0
                } else {
                    0.0
                }
            }
            Self::LeakyReLU => {
                if x > 0.0 {
                    1.0
                } else {
                    0.01
                }
            }
            Self::ELU => {
                if x > 0.0 {
                    1.0
                } else {
                    x.exp()
                }
            }
            Self::GELU => {
                // Approximate GELU derivative
                let tanh_arg = (2.0 / std::f32::consts::PI).sqrt() * (x + 0.044715 * x.powi(3));
                let tanh_val = tanh_arg.tanh();
                0.5 * (1.0
                    + tanh_val
                    + x * (1.0 - tanh_val.powi(2))
                        * (2.0 / std::f32::consts::PI).sqrt()
                        * (1.0 + 3.0 * 0.044715 * x.powi(2)))
            }
            Self::Sigmoid => {
                let sig = self.apply(x);
                sig * (1.0 - sig)
            }
            Self::Tanh => {
                let tanh = x.tanh();
                1.0 - tanh.powi(2)
            }
            Self::Softmax => {
                // Softmax derivative is complex, depends on context
                1.0
            }
            Self::Identity => 1.0,
        }
    }
}

/// Normalization layer types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Normalization {
    /// Batch normalization
    BatchNorm,
    /// Instance normalization
    InstanceNorm,
    /// Layer normalization
    LayerNorm,
    /// Group normalization
    GroupNorm,
    /// No normalization
    None,
}

/// Pooling types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Pooling {
    /// Max pooling
    Max,
    /// Average pooling
    Average,
    /// Adaptive average pooling
    AdaptiveAverage,
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_relu_activation() {
        let relu = Activation::ReLU;
        assert_eq!(relu.apply(5.0), 5.0);
        assert_eq!(relu.apply(-5.0), 0.0);
        assert_eq!(relu.derivative(5.0), 1.0);
        assert_eq!(relu.derivative(-5.0), 0.0);
    }

    #[test]
    fn test_sigmoid_activation() {
        let sigmoid = Activation::Sigmoid;
        let result = sigmoid.apply(0.0);
        assert_relative_eq!(result, 0.5, epsilon = 1e-6);

        let result = sigmoid.apply(100.0);
        assert!(result > 0.99);

        let result = sigmoid.apply(-100.0);
        assert!(result < 0.01);
    }

    #[test]
    fn test_tanh_activation() {
        let tanh = Activation::Tanh;
        assert_relative_eq!(tanh.apply(0.0), 0.0, epsilon = 1e-6);
        assert!(tanh.apply(10.0) > 0.99);
        assert!(tanh.apply(-10.0) < -0.99);
    }

    #[test]
    fn test_leaky_relu() {
        let leaky = Activation::LeakyReLU;
        assert_relative_eq!(leaky.apply(5.0), 5.0, epsilon = 1e-6);
        assert_relative_eq!(leaky.apply(-5.0), -0.05, epsilon = 1e-6);
    }

    #[test]
    fn test_gelu_activation() {
        let gelu = Activation::GELU;
        let result = gelu.apply(0.0);
        assert!(result.abs() < 0.1);

        let result = gelu.apply(1.0);
        assert!(result > 0.8 && result < 0.9);
    }
}
