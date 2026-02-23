//! # Layer Wrappers for SciRS2
//!
//! This module provides thin wrappers around scirs2-neural layers to integrate
//! with the backend abstraction. These wrappers handle shape management,
//! parameter initialization, and gradient computation.

use crate::error::{Error, Result};
use scirs2_core::ndarray::{Array, IxDyn};
use scirs2_core::random::Rng;
use scirs2_neural::activations_minimal::{Activation, GELU, ReLU as NeuralReLU, Sigmoid, Tanh};
use scirs2_neural::layers::{BatchNorm, Conv2D, Dropout, Layer, LayerNorm, MaxPool2D};

/// Wrapper for convolutional layer with batch normalization and activation
pub struct ConvBlock<F>
where
    F: scirs2_core::numeric::Float
        + std::fmt::Debug
        + scirs2_core::ndarray::ScalarOperand
        + scirs2_core::numeric::NumAssign
        + Send
        + Sync
        + 'static,
{
    conv: Conv2D<F>,
    batch_norm: Option<BatchNorm<F>>,
    activation: ActivationType,
    output_shape: Vec<usize>,
}

impl<F> ConvBlock<F>
where
    F: scirs2_core::numeric::Float
        + std::fmt::Debug
        + scirs2_core::ndarray::ScalarOperand
        + scirs2_core::numeric::NumAssign
        + Send
        + Sync
        + 'static,
{
    /// Create new convolutional block
    ///
    /// # Arguments
    ///
    /// * `in_channels` - Number of input channels
    /// * `out_channels` - Number of output channels
    /// * `kernel_size` - Convolution kernel size
    /// * `stride` - Convolution stride
    /// * `use_batch_norm` - Whether to apply batch normalization
    /// * `activation` - Activation function type
    /// * `rng` - Random number generator for parameter initialization
    pub fn new<R: Rng>(
        in_channels: usize,
        out_channels: usize,
        kernel_size: usize,
        stride: usize,
        use_batch_norm: bool,
        activation: ActivationType,
        rng: &mut R,
    ) -> Result<Self> {
        let conv = Conv2D::new(
            in_channels,
            out_channels,
            (kernel_size, kernel_size),
            (stride, stride),
            Some("conv"),
        )
        .map_err(|e| Error::Backend(format!("Failed to create Conv2D: {}", e)))?;

        let batch_norm = if use_batch_norm {
            Some(
                BatchNorm::new(out_channels, 0.1, 1e-5, rng)
                    .map_err(|e| Error::Backend(format!("Failed to create BatchNorm: {}", e)))?,
            )
        } else {
            None
        };

        Ok(Self {
            conv,
            batch_norm,
            activation,
            output_shape: vec![],
        })
    }

    /// Forward pass
    pub fn forward(&mut self, input: &Array<F, IxDyn>) -> Result<Array<F, IxDyn>> {
        // Convolution
        let mut x = self
            .conv
            .forward(input)
            .map_err(|e| Error::Backend(format!("Conv2D forward failed: {}", e)))?;

        // Batch normalization
        if let Some(ref bn) = self.batch_norm {
            x = bn
                .forward(&x)
                .map_err(|e| Error::Backend(format!("BatchNorm forward failed: {}", e)))?;
        }

        // Activation
        let output = apply_activation(&x, self.activation)?;

        // Store output shape for backward pass
        self.output_shape = output.shape().to_vec();

        Ok(output)
    }

    /// Get number of parameters
    pub fn num_parameters(&self) -> usize {
        let conv_params = self.conv.parameter_count();
        let bn_params = self
            .batch_norm
            .as_ref()
            .map(|bn| bn.parameter_count())
            .unwrap_or(0);
        conv_params + bn_params
    }

    /// Set training mode
    pub fn set_training(&mut self, training: bool) {
        self.conv.set_training(training);
        if let Some(ref mut bn) = self.batch_norm {
            bn.set_training(training);
        }
    }
}

/// Max pooling layer wrapper
pub struct MaxPool2DBlock<F>
where
    F: scirs2_core::numeric::Float
        + std::fmt::Debug
        + scirs2_core::ndarray::ScalarOperand
        + num_traits::NumAssign
        + Send
        + Sync
        + 'static,
{
    pool: MaxPool2D<F>,
}

impl<F> MaxPool2DBlock<F>
where
    F: scirs2_core::numeric::Float
        + std::fmt::Debug
        + scirs2_core::ndarray::ScalarOperand
        + num_traits::NumAssign
        + Send
        + Sync
        + 'static,
{
    /// Create new max pooling layer
    pub fn new(kernel_size: usize, stride: usize) -> Result<Self> {
        let pool = MaxPool2D::new(
            (kernel_size, kernel_size),
            (stride, stride),
            Some("maxpool"),
        )
        .map_err(|e| Error::Backend(format!("Failed to create MaxPool2D: {}", e)))?;

        Ok(Self { pool })
    }

    /// Forward pass
    pub fn forward(&self, input: &Array<F, IxDyn>) -> Result<Array<F, IxDyn>> {
        self.pool
            .forward(input)
            .map_err(|e| Error::Backend(format!("MaxPool2D forward failed: {}", e)))
    }
}

/// Dropout layer wrapper
pub struct DropoutBlock<F>
where
    F: scirs2_core::numeric::Float
        + std::fmt::Debug
        + scirs2_core::ndarray::ScalarOperand
        + num_traits::NumAssign
        + Send
        + Sync
        + 'static,
{
    dropout: Dropout<F>,
}

impl<F> DropoutBlock<F>
where
    F: scirs2_core::numeric::Float
        + std::fmt::Debug
        + scirs2_core::ndarray::ScalarOperand
        + num_traits::NumAssign
        + Send
        + Sync
        + 'static,
{
    /// Create new dropout layer
    pub fn new<R: Rng + Clone + Send + Sync + 'static>(rate: f64, rng: &mut R) -> Result<Self> {
        let dropout = Dropout::new(rate, rng)
            .map_err(|e| Error::Backend(format!("Failed to create Dropout: {}", e)))?;

        Ok(Self { dropout })
    }

    /// Forward pass
    pub fn forward(&self, input: &Array<F, IxDyn>) -> Result<Array<F, IxDyn>> {
        self.dropout
            .forward(input)
            .map_err(|e| Error::Backend(format!("Dropout forward failed: {}", e)))
    }

    /// Set training mode
    pub fn set_training(&mut self, training: bool) {
        self.dropout.set_training(training);
    }
}

/// Layer normalization wrapper for f32
pub struct LayerNormBlockF32 {
    layer_norm: LayerNorm<f32>,
}

impl LayerNormBlockF32 {
    /// Create new layer normalization
    pub fn new<R: Rng>(normalized_shape: usize, rng: &mut R) -> Result<Self> {
        let layer_norm = LayerNorm::new(normalized_shape, 1e-5, rng)
            .map_err(|e| Error::Backend(format!("Failed to create LayerNorm: {}", e)))?;

        Ok(Self { layer_norm })
    }

    /// Forward pass
    pub fn forward(&self, input: &Array<f32, IxDyn>) -> Result<Array<f32, IxDyn>> {
        self.layer_norm
            .forward(input)
            .map_err(|e| Error::Backend(format!("LayerNorm forward failed: {}", e)))
    }
}

/// Layer normalization wrapper for f64
pub struct LayerNormBlockF64 {
    layer_norm: LayerNorm<f64>,
}

impl LayerNormBlockF64 {
    /// Create new layer normalization
    pub fn new<R: Rng>(normalized_shape: usize, rng: &mut R) -> Result<Self> {
        let layer_norm = LayerNorm::new(normalized_shape, 1e-5, rng)
            .map_err(|e| Error::Backend(format!("Failed to create LayerNorm: {}", e)))?;

        Ok(Self { layer_norm })
    }

    /// Forward pass
    pub fn forward(&self, input: &Array<f64, IxDyn>) -> Result<Array<f64, IxDyn>> {
        self.layer_norm
            .forward(input)
            .map_err(|e| Error::Backend(format!("LayerNorm forward failed: {}", e)))
    }
}

/// Activation function types
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub enum ActivationType {
    /// Rectified Linear Unit: f(x) = max(0, x)
    ReLU,
    /// Leaky ReLU: f(x) = x if x > 0, negative_slope * x otherwise
    LeakyReLU {
        /// Slope for negative values
        negative_slope: f64,
    },
    /// Sigmoid activation: f(x) = 1 / (1 + exp(-x))
    Sigmoid,
    /// Hyperbolic tangent: f(x) = tanh(x)
    Tanh,
    /// Gaussian Error Linear Unit
    GELU,
}

/// Apply activation function using scirs2-neural implementations
fn apply_activation<F>(
    input: &Array<F, IxDyn>,
    activation: ActivationType,
) -> Result<Array<F, IxDyn>>
where
    F: scirs2_core::numeric::Float
        + std::fmt::Debug
        + scirs2_core::ndarray::ScalarOperand
        + 'static,
{
    match activation {
        ActivationType::ReLU => {
            let relu = NeuralReLU::new();
            relu.forward(input)
                .map_err(|e| Error::Backend(format!("ReLU activation failed: {}", e)))
        }
        ActivationType::LeakyReLU { negative_slope } => {
            let leaky_relu = NeuralReLU::leaky(negative_slope);
            leaky_relu
                .forward(input)
                .map_err(|e| Error::Backend(format!("LeakyReLU activation failed: {}", e)))
        }
        ActivationType::Sigmoid => {
            let sigmoid = Sigmoid::new();
            sigmoid
                .forward(input)
                .map_err(|e| Error::Backend(format!("Sigmoid activation failed: {}", e)))
        }
        ActivationType::Tanh => {
            let tanh = Tanh::new();
            tanh.forward(input)
                .map_err(|e| Error::Backend(format!("Tanh activation failed: {}", e)))
        }
        ActivationType::GELU => {
            let gelu = GELU::new();
            gelu.forward(input)
                .map_err(|e| Error::Backend(format!("GELU activation failed: {}", e)))
        }
    }
}

/// Concatenate tensors along channel dimension (axis 1 for NCHW format)
pub fn concat_channels<F>(tensors: &[Array<F, IxDyn>]) -> Result<Array<F, IxDyn>>
where
    F: scirs2_core::numeric::Float
        + std::fmt::Debug
        + scirs2_core::ndarray::ScalarOperand
        + Clone
        + 'static,
{
    use scirs2_core::ndarray::Axis;

    if tensors.is_empty() {
        return Err(Error::Backend(
            "Cannot concatenate empty tensor list".to_string(),
        ));
    }

    if tensors.len() == 1 {
        return Ok(tensors[0].clone());
    }

    // Verify all tensors have the same shape except for the channel dimension (axis 1)
    let ref_shape = tensors[0].shape();
    if ref_shape.len() != 4 {
        return Err(Error::Backend(format!(
            "Expected 4D tensors (NCHW format), got {}D",
            ref_shape.len()
        )));
    }

    for tensor in tensors.iter().skip(1) {
        let shape = tensor.shape();
        if shape.len() != 4 {
            return Err(Error::Backend(format!(
                "All tensors must have same number of dimensions, got {}D and {}D",
                ref_shape.len(),
                shape.len()
            )));
        }
        if shape[0] != ref_shape[0] || shape[2] != ref_shape[2] || shape[3] != ref_shape[3] {
            return Err(Error::Backend(format!(
                "Tensor shapes must match except for channel dimension, got {:?} and {:?}",
                ref_shape, shape
            )));
        }
    }

    // Convert to ndarray views and concatenate along axis 1 (channels)
    let views: Vec<_> = tensors.iter().map(|t| t.view()).collect();
    let concatenated = scirs2_core::ndarray::concatenate(Axis(1), &views)
        .map_err(|e| Error::Backend(format!("Failed to concatenate tensors: {}", e)))?;

    Ok(concatenated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use scirs2_core::ndarray::Array4;
    use scirs2_core::random::Random;

    #[test]
    fn test_conv_block_creation() {
        let mut rng = Random::seed(42);
        let result = ConvBlock::<f64>::new(3, 64, 3, 1, true, ActivationType::ReLU, &mut rng);
        assert!(result.is_ok());

        let block = result.expect("ConvBlock creation failed");
        assert!(block.num_parameters() > 0);
    }

    #[test]
    fn test_conv_block_forward() {
        let mut rng = Random::seed(42);
        let mut block = ConvBlock::<f64>::new(3, 64, 3, 1, true, ActivationType::ReLU, &mut rng)
            .expect("ConvBlock creation failed");

        // Create input: (batch=2, channels=3, height=32, width=32)
        let input = Array4::<f64>::zeros((2, 3, 32, 32)).into_dyn();
        let result = block.forward(&input);
        assert!(result.is_ok());

        let output = result.expect("Forward pass failed");
        assert_eq!(output.shape()[0], 2); // batch size
        assert_eq!(output.shape()[1], 64); // output channels
    }

    #[test]
    fn test_maxpool_creation() {
        let result = MaxPool2DBlock::<f64>::new(2, 2);
        assert!(result.is_ok());
    }

    #[test]
    fn test_maxpool_forward() {
        let pool = MaxPool2DBlock::<f64>::new(2, 2).expect("MaxPool2D creation failed");

        // Create input: (batch=2, channels=64, height=32, width=32)
        let input = Array4::<f64>::ones((2, 64, 32, 32)).into_dyn();
        let result = pool.forward(&input);
        assert!(result.is_ok());

        let output = result.expect("Forward pass failed");
        assert_eq!(output.shape()[0], 2); // batch size
        assert_eq!(output.shape()[1], 64); // channels unchanged
        assert_eq!(output.shape()[2], 16); // height halved
        assert_eq!(output.shape()[3], 16); // width halved
    }

    #[test]
    fn test_dropout_creation() {
        let mut rng = Random::seed(42);
        let result = DropoutBlock::<f64>::new(0.5, &mut rng);
        assert!(result.is_ok());
    }

    #[test]
    fn test_dropout_forward_inference() {
        let mut rng = Random::seed(42);
        let mut dropout = DropoutBlock::<f64>::new(0.5, &mut rng).expect("Dropout creation failed");
        dropout.set_training(false); // Inference mode

        let input = Array4::<f64>::ones((2, 64, 32, 32)).into_dyn();
        let result = dropout.forward(&input);
        assert!(result.is_ok());

        let output = result.expect("Forward pass failed");
        assert_eq!(output.shape(), input.shape());
    }

    #[test]
    fn test_layer_norm_creation() {
        let mut rng = Random::seed(42);
        let result = LayerNormBlockF64::new(512, &mut rng);
        assert!(result.is_ok());
    }

    #[test]
    fn test_activation_relu() {
        use scirs2_core::ndarray::Array2;

        // Create input with some negative values
        let input =
            Array2::<f64>::from_shape_vec((2, 4), vec![-1.0, 2.0, -3.0, 4.0, 5.0, -6.0, 7.0, -8.0])
                .expect("Array creation failed")
                .into_dyn();

        let result = apply_activation(&input, ActivationType::ReLU);
        assert!(result.is_ok());

        let output = result.expect("ReLU activation failed");
        assert_eq!(output.shape(), input.shape());

        // Verify ReLU behavior: negative values become zero
        let output_slice = output.as_slice().expect("Failed to get slice");
        assert_eq!(output_slice[0], 0.0); // -1.0 -> 0.0
        assert_eq!(output_slice[1], 2.0); // 2.0 -> 2.0
        assert_eq!(output_slice[2], 0.0); // -3.0 -> 0.0
        assert_eq!(output_slice[3], 4.0); // 4.0 -> 4.0
    }

    #[test]
    #[allow(clippy::panic)]
    fn test_activation_types() {
        // Test activation type serialization
        let act = ActivationType::LeakyReLU {
            negative_slope: 0.01,
        };
        let json = serde_json::to_string(&act).expect("serialization failed");
        let deserialized: ActivationType =
            serde_json::from_str(&json).expect("deserialization failed");

        match deserialized {
            ActivationType::LeakyReLU { negative_slope } => {
                assert!((negative_slope - 0.01).abs() < 1e-6);
            }
            other => panic!("Wrong activation type: expected LeakyReLU, got {:?}", other),
        }
    }

    #[test]
    fn test_concat_channels() {
        // Create two tensors with different channel counts
        let tensor1 = Array4::<f64>::ones((2, 32, 16, 16)).into_dyn();
        let tensor2 = Array4::<f64>::ones((2, 64, 16, 16)).into_dyn();

        let result = concat_channels(&[tensor1, tensor2]);
        assert!(result.is_ok());

        let output = result.expect("Concatenation failed");
        assert_eq!(output.shape()[0], 2); // batch size
        assert_eq!(output.shape()[1], 96); // 32 + 64 channels
        assert_eq!(output.shape()[2], 16); // height unchanged
        assert_eq!(output.shape()[3], 16); // width unchanged
    }

    #[test]
    fn test_concat_channels_empty() {
        let result = concat_channels::<f64>(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_concat_channels_single() {
        let tensor = Array4::<f64>::ones((2, 32, 16, 16)).into_dyn();
        let result = concat_channels(std::slice::from_ref(&tensor));
        assert!(result.is_ok());

        let output = result.expect("Concatenation failed");
        assert_eq!(output.shape(), tensor.shape());
    }

    #[test]
    fn test_concat_channels_shape_mismatch() {
        // Tensors with different spatial dimensions should fail
        let tensor1 = Array4::<f64>::ones((2, 32, 16, 16)).into_dyn();
        let tensor2 = Array4::<f64>::ones((2, 64, 32, 32)).into_dyn();

        let result = concat_channels(&[tensor1, tensor2]);
        assert!(result.is_err());
    }
}
