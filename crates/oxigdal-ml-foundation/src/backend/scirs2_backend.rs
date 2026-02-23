//! # SciRS2 Backend Implementation
//!
//! This module implements the MLBackend trait using Pure Rust scirs2 ecosystem.
//! It provides actual training capabilities by building neural networks from
//! model configurations and executing forward/backward passes.
//!
//! NOTE: This module is a placeholder awaiting scirs2 API stabilization.

#![allow(dead_code, unused_variables, unused_imports)]

use super::layers::{
    ActivationType, ConvBlock, DenseBlock, MaxPool2D, Upsample2D, UpsampleMode, concat_channels,
};
use super::{BackendConfig, MLBackend};
use crate::error::{Error, Result};
use crate::models::resnet::ResNetConfig;
use crate::models::unet::UNetConfig;
use scirs2_core::ndarray::{Array1, Array4};
use std::collections::HashMap;
use std::path::Path;

/// Placeholder Variable type until scirs2-autograd API stabilizes
/// This is a minimal implementation to allow compilation
#[derive(Clone)]
struct Variable {
    data: Array4<f32>,
}

impl Variable {
    /// Create Variable from data
    fn from_data(data: Array4<f32>) -> Result<Self> {
        Ok(Self { data })
    }

    /// Get underlying data
    fn data(&self) -> &Array4<f32> {
        &self.data
    }

    /// Get shape
    fn shape(&self) -> &[usize] {
        self.data.shape()
    }

    /// Backward pass (placeholder)
    fn backward(&self) -> Result<()> {
        // Placeholder until scirs2-autograd is integrated
        Ok(())
    }
}

/// SciRS2 backend for UNet architecture
pub struct SciRS2UNetBackend {
    encoder_blocks: Vec<EncoderBlock>,
    bottleneck: ConvBlock,
    decoder_blocks: Vec<DecoderBlock>,
    final_conv: ConvBlock,
    config: UNetConfig,
    backend_config: BackendConfig,
    last_loss: Option<f32>,
    training: bool,
    optimizer: Option<Box<dyn OptimizerWrapper>>,
}

/// Encoder block (double conv + pooling)
struct EncoderBlock {
    conv1: ConvBlock,
    conv2: ConvBlock,
    pool: MaxPool2D,
}

impl EncoderBlock {
    fn forward(&mut self, input: &Variable) -> Result<(Variable, Variable)> {
        let x1 = self.conv1.forward(input)?;
        let x2 = self.conv2.forward(&x1)?;
        let pooled = self.pool.forward(&x2)?;
        Ok((x2, pooled)) // Return both for skip connection
    }

    fn num_parameters(&self) -> usize {
        self.conv1.num_parameters() + self.conv2.num_parameters()
    }
}

/// Decoder block (upsample + concat + double conv)
struct DecoderBlock {
    upsample: Upsample2D,
    conv1: ConvBlock,
    conv2: ConvBlock,
}

impl DecoderBlock {
    fn forward(&mut self, input: &Variable, skip: &Variable) -> Result<Variable> {
        let upsampled = self.upsample.forward(input)?;
        let concatenated = concat_channels(&[upsampled, skip.clone()])?;
        let x1 = self.conv1.forward(&concatenated)?;
        let x2 = self.conv2.forward(&x1)?;
        Ok(x2)
    }

    fn num_parameters(&self) -> usize {
        self.conv1.num_parameters() + self.conv2.num_parameters()
    }
}

impl SciRS2UNetBackend {
    /// Create new UNet backend from configuration
    pub fn new(config: &UNetConfig, backend_config: &BackendConfig) -> Result<Self> {
        tracing::info!(
            "Initializing SciRS2 UNet backend: depth={}, base_filters={}, device={}",
            config.depth,
            config.base_filters,
            backend_config.device
        );

        // Build encoder blocks
        let mut encoder_blocks = Vec::with_capacity(config.depth);
        for level in 0..config.depth {
            let in_channels = if level == 0 {
                config.in_channels
            } else {
                config.base_filters * (1 << (level - 1))
            };
            let out_channels = config.base_filters * (1 << level);

            let conv1 = ConvBlock::new(
                in_channels,
                out_channels,
                3,
                1,
                1,
                config.batch_norm,
                ActivationType::ReLU,
            )?;

            let conv2 = ConvBlock::new(
                out_channels,
                out_channels,
                3,
                1,
                1,
                config.batch_norm,
                ActivationType::ReLU,
            )?;

            let pool = MaxPool2D::new(2, 2, 0);

            encoder_blocks.push(EncoderBlock { conv1, conv2, pool });
        }

        // Build bottleneck
        let bottleneck_channels = config.base_filters * (1 << config.depth);
        let bottleneck = ConvBlock::new(
            bottleneck_channels / 2,
            bottleneck_channels,
            3,
            1,
            1,
            config.batch_norm,
            ActivationType::ReLU,
        )?;

        // Build decoder blocks
        let mut decoder_blocks = Vec::with_capacity(config.depth);
        for level in (0..config.depth).rev() {
            let in_channels = config.base_filters * (1 << (level + 1));
            let out_channels = config.base_filters * (1 << level);

            let upsample = Upsample2D::new(2, UpsampleMode::Bilinear);

            // After concat, channels double
            let conv1 = ConvBlock::new(
                in_channels,
                out_channels,
                3,
                1,
                1,
                config.batch_norm,
                ActivationType::ReLU,
            )?;

            let conv2 = ConvBlock::new(
                out_channels,
                out_channels,
                3,
                1,
                1,
                config.batch_norm,
                ActivationType::ReLU,
            )?;

            decoder_blocks.push(DecoderBlock {
                upsample,
                conv1,
                conv2,
            });
        }

        // Final 1x1 convolution
        let final_conv = ConvBlock::new(
            config.base_filters,
            config.out_channels,
            1,
            1,
            0,
            false,
            ActivationType::Sigmoid, // For segmentation output
        )?;

        let num_params =
            Self::count_parameters(&encoder_blocks, &decoder_blocks, &bottleneck, &final_conv);
        tracing::info!("UNet initialized with {} parameters", num_params);

        Ok(Self {
            encoder_blocks,
            bottleneck,
            decoder_blocks,
            final_conv,
            config: config.clone(),
            backend_config: backend_config.clone(),
            last_loss: None,
            training: true,
            optimizer: None,
        })
    }

    fn count_parameters(
        encoder_blocks: &[EncoderBlock],
        decoder_blocks: &[DecoderBlock],
        bottleneck: &ConvBlock,
        final_conv: &ConvBlock,
    ) -> usize {
        let encoder_params: usize = encoder_blocks.iter().map(|b| b.num_parameters()).sum();
        let decoder_params: usize = decoder_blocks.iter().map(|b| b.num_parameters()).sum();
        let bottleneck_params = bottleneck.num_parameters();
        let final_params = final_conv.num_parameters();

        encoder_params + decoder_params + bottleneck_params + final_params
    }

    /// Initialize optimizer
    pub fn init_optimizer(
        &mut self,
        learning_rate: f32,
        optimizer_type: OptimizerType,
    ) -> Result<()> {
        self.optimizer = Some(match optimizer_type {
            OptimizerType::Adam {
                beta1,
                beta2,
                epsilon,
            } => Box::new(AdamWrapper::new(learning_rate, beta1, beta2, epsilon)?),
            OptimizerType::SGD {
                momentum,
                weight_decay,
            } => Box::new(SGDWrapper::new(learning_rate, momentum, weight_decay)?),
        });
        Ok(())
    }
}

impl MLBackend for SciRS2UNetBackend {
    fn forward(&self, input: &[f32], input_shape: &[usize]) -> Result<Vec<f32>> {
        if input_shape.len() != 4 {
            return Err(Error::Backend(format!(
                "Expected 4D input (batch, channels, height, width), got {}D",
                input_shape.len()
            )));
        }

        // Convert input to Variable
        let input_array = Array4::from_shape_vec(
            (
                input_shape[0],
                input_shape[1],
                input_shape[2],
                input_shape[3],
            ),
            input.to_vec(),
        )
        .map_err(|e| Error::Backend(format!("Failed to create input array: {}", e)))?;

        let mut x = Variable::from_data(input_array)
            .map_err(|e| Error::Backend(format!("Failed to create input variable: {}", e)))?;

        // Encoder forward pass with skip connections
        let mut skip_connections = Vec::with_capacity(self.config.depth);
        for encoder in &self.encoder_blocks {
            let (skip, pooled) = encoder.forward(&x)?;
            skip_connections.push(skip);
            x = pooled;
        }

        // Bottleneck
        x = self.bottleneck.forward(&x)?;

        // Decoder forward pass with skip connections
        for (i, decoder) in self.decoder_blocks.iter().enumerate() {
            let skip_idx = self.config.depth - 1 - i;
            let skip = &skip_connections[skip_idx];
            x = decoder.forward(&x, skip)?;
        }

        // Final convolution
        x = self.final_conv.forward(&x)?;

        // Convert output to flat vector
        let output_data = x.data();
        Ok(output_data
            .as_slice()
            .ok_or_else(|| Error::Backend("Failed to get output slice".to_string()))?
            .to_vec())
    }

    fn backward(&self, grad_output: &[f32], grad_shape: &[usize]) -> Result<Vec<f32>> {
        // Create gradient variable
        let grad_array = Array4::from_shape_vec(
            (grad_shape[0], grad_shape[1], grad_shape[2], grad_shape[3]),
            grad_output.to_vec(),
        )
        .map_err(|e| Error::Backend(format!("Failed to create gradient array: {}", e)))?;

        let grad_var = Variable::from_data(grad_array)
            .map_err(|e| Error::Backend(format!("Failed to create gradient variable: {}", e)))?;

        // Backward pass through computational graph
        grad_var
            .backward()
            .map_err(|e| Error::Backend(format!("Backward pass failed: {}", e)))?;

        // Return input gradients (placeholder for now)
        // In full implementation, this would retrieve gradients from the computation graph
        Ok(vec![0.0; grad_output.len()])
    }

    fn optimizer_step(&mut self, learning_rate: f32) -> Result<()> {
        if let Some(ref mut optimizer) = self.optimizer {
            optimizer.step(learning_rate)?;
        } else {
            return Err(Error::Backend(
                "Optimizer not initialized. Call init_optimizer first.".to_string(),
            ));
        }
        Ok(())
    }

    fn zero_grad(&mut self) -> Result<()> {
        if let Some(ref mut optimizer) = self.optimizer {
            optimizer.zero_grad()?;
        }
        Ok(())
    }

    fn num_parameters(&self) -> usize {
        Self::count_parameters(
            &self.encoder_blocks,
            &self.decoder_blocks,
            &self.bottleneck,
            &self.final_conv,
        )
    }

    fn last_loss(&self) -> Option<f32> {
        self.last_loss
    }

    fn set_train_mode(&mut self, train: bool) {
        self.training = train;
        // Set training mode for all batch norm layers
        // (Would need to propagate through all blocks in full implementation)
    }

    fn save_weights(&self, path: &Path) -> Result<()> {
        tracing::info!("Saving UNet weights to {:?}", path);
        // Implement weight serialization
        // For now, placeholder
        Ok(())
    }

    fn load_weights(&mut self, path: &Path) -> Result<()> {
        tracing::info!("Loading UNet weights from {:?}", path);
        // Implement weight deserialization
        // For now, placeholder
        Ok(())
    }

    #[cfg(feature = "onnx")]
    fn export_onnx(&self, path: &Path) -> Result<()> {
        tracing::info!("Exporting UNet to ONNX: {:?}", path);
        // Implement ONNX export
        Err(Error::Backend(
            "ONNX export not yet implemented".to_string(),
        ))
    }
}

/// SciRS2 backend for ResNet architecture
pub struct SciRS2ResNetBackend {
    initial_conv: ConvBlock,
    residual_blocks: Vec<ResidualBlock>,
    global_pool: GlobalAvgPool,
    fc: DenseBlock,
    config: ResNetConfig,
    backend_config: BackendConfig,
    last_loss: Option<f32>,
    training: bool,
}

struct ResidualBlock {
    conv1: ConvBlock,
    conv2: ConvBlock,
    shortcut: Option<ConvBlock>,
}

impl ResidualBlock {
    fn forward(&mut self, input: &Variable) -> Result<Variable> {
        let identity = input.clone();

        let x1 = self.conv1.forward(input)?;
        let x2 = self.conv2.forward(&x1)?;

        let shortcut = if let Some(ref mut sc) = self.shortcut {
            sc.forward(&identity)?
        } else {
            identity
        };

        // Residual connection: x2 + shortcut
        let output = &x2 + &shortcut;
        Ok(output)
    }

    fn num_parameters(&self) -> usize {
        let conv_params = self.conv1.num_parameters() + self.conv2.num_parameters();
        let shortcut_params = self
            .shortcut
            .as_ref()
            .map(|s| s.num_parameters())
            .unwrap_or(0);
        conv_params + shortcut_params
    }
}

/// Tensor layout format for 4D tensors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TensorLayout {
    /// Batch, Channels, Height, Width (PyTorch default)
    NCHW,
    /// Batch, Height, Width, Channels (TensorFlow default)
    NHWC,
}

impl Default for TensorLayout {
    fn default() -> Self {
        TensorLayout::NCHW
    }
}

/// Global Average Pooling layer
///
/// Reduces spatial dimensions (H, W) to single values per channel by computing
/// the mean across all spatial positions. This is essential for classification
/// networks like ResNet where the final features need to be reduced before
/// the fully connected layer.
///
/// Output shape:
/// - NCHW input [batch, channels, height, width] -> [batch, channels, 1, 1]
/// - NHWC input [batch, height, width, channels] -> [batch, 1, 1, channels]
struct GlobalAvgPool {
    /// Tensor layout format
    layout: TensorLayout,
}

impl GlobalAvgPool {
    /// Create a new global average pooling layer
    ///
    /// # Arguments
    ///
    /// * `layout` - The tensor layout format (NCHW or NHWC)
    fn new(layout: TensorLayout) -> Self {
        Self { layout }
    }

    /// Forward pass: compute global average pooling
    ///
    /// For NCHW format:
    ///   - Input shape: [batch, channels, height, width]
    ///   - Output shape: [batch, channels, 1, 1]
    ///
    /// For NHWC format:
    ///   - Input shape: [batch, height, width, channels]
    ///   - Output shape: [batch, 1, 1, channels]
    ///
    /// # Arguments
    ///
    /// * `input` - Input tensor (4D)
    ///
    /// # Returns
    ///
    /// Pooled tensor with spatial dimensions reduced to 1
    fn forward(&self, input: &Variable) -> Result<Variable> {
        let shape = input.shape();

        // Validate input is 4D
        if shape.len() != 4 {
            return Err(Error::Backend(format!(
                "GlobalAvgPool expects 4D input, got {}D tensor with shape {:?}",
                shape.len(),
                shape
            )));
        }

        match self.layout {
            TensorLayout::NCHW => self.forward_nchw(input),
            TensorLayout::NHWC => self.forward_nhwc(input),
        }
    }

    /// Forward pass for NCHW layout
    ///
    /// Input shape: [batch, channels, height, width]
    /// Output shape: [batch, channels, 1, 1]
    fn forward_nchw(&self, input: &Variable) -> Result<Variable> {
        let data = input.data();
        let shape = data.shape();

        let batch_size = shape[0];
        let num_channels = shape[1];
        let height = shape[2];
        let width = shape[3];
        let spatial_size = height * width;

        // Validate spatial dimensions are non-zero
        if spatial_size == 0 {
            return Err(Error::Backend(
                "GlobalAvgPool: spatial dimensions (height, width) cannot be zero".to_string()
            ));
        }

        // Create output array with shape [batch, channels, 1, 1]
        let mut output = Array4::<f32>::zeros((batch_size, num_channels, 1, 1));

        // Compute mean over spatial dimensions for each (batch, channel)
        for b in 0..batch_size {
            for c in 0..num_channels {
                let mut sum = 0.0f32;
                for h in 0..height {
                    for w in 0..width {
                        sum += data[[b, c, h, w]];
                    }
                }
                let mean = sum / (spatial_size as f32);
                output[[b, c, 0, 0]] = mean;
            }
        }

        Variable::from_data(output)
    }

    /// Forward pass for NHWC layout
    ///
    /// Input shape: [batch, height, width, channels]
    /// Output shape: [batch, 1, 1, channels]
    fn forward_nhwc(&self, input: &Variable) -> Result<Variable> {
        let data = input.data();
        let shape = data.shape();

        let batch_size = shape[0];
        let height = shape[1];
        let width = shape[2];
        let num_channels = shape[3];
        let spatial_size = height * width;

        // Validate spatial dimensions are non-zero
        if spatial_size == 0 {
            return Err(Error::Backend(
                "GlobalAvgPool: spatial dimensions (height, width) cannot be zero".to_string()
            ));
        }

        // Create output array with shape [batch, 1, 1, channels]
        let mut output = Array4::<f32>::zeros((batch_size, 1, 1, num_channels));

        // Compute mean over spatial dimensions for each (batch, channel)
        for b in 0..batch_size {
            for c in 0..num_channels {
                let mut sum = 0.0f32;
                for h in 0..height {
                    for w in 0..width {
                        sum += data[[b, h, w, c]];
                    }
                }
                let mean = sum / (spatial_size as f32);
                output[[b, 0, 0, c]] = mean;
            }
        }

        Variable::from_data(output)
    }
}

impl SciRS2ResNetBackend {
    /// Create new ResNet backend from configuration
    pub fn new(config: &ResNetConfig, backend_config: &BackendConfig) -> Result<Self> {
        tracing::info!(
            "Initializing SciRS2 ResNet backend: variant={:?}, num_classes={}",
            config.variant,
            config.num_classes
        );

        // Initial 7x7 convolution
        let initial_conv =
            ConvBlock::new(config.in_channels, 64, 7, 2, 3, true, ActivationType::ReLU)?;

        // Build residual blocks based on variant
        let layer_config = match config.variant {
            crate::models::resnet::ResNetVariant::ResNet18 => vec![2, 2, 2, 2],
            crate::models::resnet::ResNetVariant::ResNet34 => vec![3, 4, 6, 3],
            crate::models::resnet::ResNetVariant::ResNet50 => vec![3, 4, 6, 3],
            crate::models::resnet::ResNetVariant::ResNet101 => vec![3, 4, 23, 3],
            crate::models::resnet::ResNetVariant::ResNet152 => vec![3, 8, 36, 3],
        };

        let mut residual_blocks = Vec::new();
        let mut in_channels = 64;

        for (stage_idx, &num_blocks) in layer_config.iter().enumerate() {
            let out_channels = 64 * (1 << stage_idx);
            let stride = if stage_idx == 0 { 1 } else { 2 };

            for block_idx in 0..num_blocks {
                let block_stride = if block_idx == 0 { stride } else { 1 };
                let block_in = if block_idx == 0 {
                    in_channels
                } else {
                    out_channels
                };

                let conv1 = ConvBlock::new(
                    block_in,
                    out_channels,
                    3,
                    block_stride,
                    1,
                    true,
                    ActivationType::ReLU,
                )?;

                let conv2 = ConvBlock::new(
                    out_channels,
                    out_channels,
                    3,
                    1,
                    1,
                    true,
                    ActivationType::ReLU,
                )?;

                let shortcut = if block_in != out_channels || block_stride != 1 {
                    Some(ConvBlock::new(
                        block_in,
                        out_channels,
                        1,
                        block_stride,
                        0,
                        true,
                        ActivationType::ReLU,
                    )?)
                } else {
                    None
                };

                residual_blocks.push(ResidualBlock {
                    conv1,
                    conv2,
                    shortcut,
                });
            }

            in_channels = out_channels;
        }

        // Global average pooling
        let global_pool = GlobalAvgPool;

        // Fully connected layer
        let fc_in = 64 * (1 << (layer_config.len() - 1));
        let fc = DenseBlock::new(fc_in, config.num_classes, None, config.dropout)?;

        tracing::info!(
            "ResNet initialized with {} residual blocks",
            residual_blocks.len()
        );

        Ok(Self {
            initial_conv,
            residual_blocks,
            global_pool,
            fc,
            config: config.clone(),
            backend_config: backend_config.clone(),
            last_loss: None,
            training: true,
        })
    }
}

impl MLBackend for SciRS2ResNetBackend {
    fn forward(&self, input: &[f32], input_shape: &[usize]) -> Result<Vec<f32>> {
        // Convert input to Variable
        let input_array = Array4::from_shape_vec(
            (
                input_shape[0],
                input_shape[1],
                input_shape[2],
                input_shape[3],
            ),
            input.to_vec(),
        )
        .map_err(|e| Error::Backend(format!("Failed to create input array: {}", e)))?;

        let mut x = Variable::from_data(input_array)
            .map_err(|e| Error::Backend(format!("Failed to create input variable: {}", e)))?;

        // Initial convolution
        x = self.initial_conv.forward(&x)?;

        // Residual blocks
        for block in &self.residual_blocks {
            x = block.forward(&x)?;
        }

        // Global average pooling
        x = self.global_pool.forward(&x)?;

        // Fully connected
        x = self.fc.forward(&x, self.training)?;

        // Convert to output vector
        let output_data = x.data();
        Ok(output_data
            .as_slice()
            .ok_or_else(|| Error::Backend("Failed to get output slice".to_string()))?
            .to_vec())
    }

    fn backward(&self, grad_output: &[f32], grad_shape: &[usize]) -> Result<Vec<f32>> {
        // Similar to UNet backward
        Ok(vec![0.0; grad_output.len()])
    }

    fn optimizer_step(&mut self, _learning_rate: f32) -> Result<()> {
        // Implement optimizer step
        Ok(())
    }

    fn zero_grad(&mut self) -> Result<()> {
        Ok(())
    }

    fn num_parameters(&self) -> usize {
        let initial_params = self.initial_conv.num_parameters();
        let residual_params: usize = self
            .residual_blocks
            .iter()
            .map(|b| b.num_parameters())
            .sum();
        let fc_params = self.fc.num_parameters();
        initial_params + residual_params + fc_params
    }

    fn last_loss(&self) -> Option<f32> {
        self.last_loss
    }

    fn set_train_mode(&mut self, train: bool) {
        self.training = train;
    }

    fn save_weights(&self, _path: &Path) -> Result<()> {
        Ok(())
    }

    fn load_weights(&mut self, _path: &Path) -> Result<()> {
        Ok(())
    }

    #[cfg(feature = "onnx")]
    fn export_onnx(&self, _path: &Path) -> Result<()> {
        Err(Error::Backend(
            "ONNX export not yet implemented".to_string(),
        ))
    }
}

/// Optimizer type enumeration
pub enum OptimizerType {
    Adam {
        beta1: f32,
        beta2: f32,
        epsilon: f32,
    },
    SGD {
        momentum: f32,
        weight_decay: f32,
    },
}

/// Trait for optimizer wrappers
trait OptimizerWrapper: Send + Sync {
    fn step(&mut self, learning_rate: f32) -> Result<()>;
    fn zero_grad(&mut self) -> Result<()>;
}

/// Adam optimizer wrapper with momentum and adaptive learning rate
struct AdamWrapper {
    learning_rate: f32,
    beta1: f32,
    beta2: f32,
    epsilon: f32,
    /// First moment estimates (momentum) for each parameter
    m: HashMap<String, Array1<f32>>,
    /// Second moment estimates (adaptive learning rate) for each parameter
    v: HashMap<String, Array1<f32>>,
    /// Time step counter for bias correction
    t: usize,
    /// Parameter storage (parameter name -> values)
    parameters: HashMap<String, Array1<f32>>,
    /// Gradient storage (parameter name -> gradients)
    gradients: HashMap<String, Array1<f32>>,
}

impl AdamWrapper {
    fn new(learning_rate: f32, beta1: f32, beta2: f32, epsilon: f32) -> Result<Self> {
        tracing::debug!(
            "Initializing Adam optimizer: lr={}, beta1={}, beta2={}, epsilon={}",
            learning_rate,
            beta1,
            beta2,
            epsilon
        );

        Ok(Self {
            learning_rate,
            beta1,
            beta2,
            epsilon,
            m: HashMap::new(),
            v: HashMap::new(),
            t: 0,
            parameters: HashMap::new(),
            gradients: HashMap::new(),
        })
    }

    /// Register a parameter for optimization
    fn register_parameter(&mut self, name: String, initial_value: Array1<f32>) {
        let size = initial_value.len();
        self.parameters.insert(name.clone(), initial_value);
        self.m.insert(name.clone(), Array1::zeros(size));
        self.v.insert(name.clone(), Array1::zeros(size));
        self.gradients.insert(name, Array1::zeros(size));
    }
}

impl OptimizerWrapper for AdamWrapper {
    fn step(&mut self, learning_rate: f32) -> Result<()> {
        // Increment time step
        self.t += 1;

        // Use provided learning rate or default
        let lr = if learning_rate > 0.0 {
            learning_rate
        } else {
            self.learning_rate
        };

        // Update each parameter using Adam algorithm
        for (param_name, param_values) in self.parameters.iter_mut() {
            // Get gradient for this parameter
            let grad = self.gradients.get(param_name).ok_or_else(|| {
                Error::Backend(format!("No gradient found for parameter: {}", param_name))
            })?;

            // Get or initialize moment estimates
            let m = self.m.get_mut(param_name).ok_or_else(|| {
                Error::Backend(format!("No first moment for parameter: {}", param_name))
            })?;
            let v = self.v.get_mut(param_name).ok_or_else(|| {
                Error::Backend(format!("No second moment for parameter: {}", param_name))
            })?;

            // Update biased first moment estimate: m_t = beta1 * m_{t-1} + (1 - beta1) * g_t
            for i in 0..m.len() {
                m[i] = self.beta1 * m[i] + (1.0 - self.beta1) * grad[i];
            }

            // Update biased second raw moment estimate: v_t = beta2 * v_{t-1} + (1 - beta2) * g_t^2
            for i in 0..v.len() {
                v[i] = self.beta2 * v[i] + (1.0 - self.beta2) * grad[i] * grad[i];
            }

            // Compute bias-corrected first moment estimate
            let bias_correction1 = 1.0 - self.beta1.powi(self.t as i32);
            let bias_correction2 = 1.0 - self.beta2.powi(self.t as i32);

            // Update parameters: theta_t = theta_{t-1} - lr * m_hat / (sqrt(v_hat) + epsilon)
            for i in 0..param_values.len() {
                let m_hat = m[i] / bias_correction1;
                let v_hat = v[i] / bias_correction2;
                param_values[i] -= lr * m_hat / (v_hat.sqrt() + self.epsilon);
            }
        }

        tracing::trace!(
            "Adam step completed: t={}, lr={}, parameters={}",
            self.t,
            lr,
            self.parameters.len()
        );

        Ok(())
    }

    fn zero_grad(&mut self) -> Result<()> {
        // Zero out all gradients
        for (_param_name, grad) in self.gradients.iter_mut() {
            grad.fill(0.0);
        }

        tracing::trace!("Gradients zeroed for {} parameters", self.gradients.len());
        Ok(())
    }
}

/// SGD optimizer wrapper with momentum and weight decay
struct SGDWrapper {
    learning_rate: f32,
    momentum: f32,
    weight_decay: f32,
    /// Velocity (momentum buffer) for each parameter
    velocity: HashMap<String, Array1<f32>>,
    /// Parameter storage (parameter name -> values)
    parameters: HashMap<String, Array1<f32>>,
    /// Gradient storage (parameter name -> gradients)
    gradients: HashMap<String, Array1<f32>>,
}

impl SGDWrapper {
    fn new(learning_rate: f32, momentum: f32, weight_decay: f32) -> Result<Self> {
        tracing::debug!(
            "Initializing SGD optimizer: lr={}, momentum={}, weight_decay={}",
            learning_rate,
            momentum,
            weight_decay
        );

        Ok(Self {
            learning_rate,
            momentum,
            weight_decay,
            velocity: HashMap::new(),
            parameters: HashMap::new(),
            gradients: HashMap::new(),
        })
    }

    /// Register a parameter for optimization
    fn register_parameter(&mut self, name: String, initial_value: Array1<f32>) {
        let size = initial_value.len();
        self.parameters.insert(name.clone(), initial_value);
        self.velocity.insert(name.clone(), Array1::zeros(size));
        self.gradients.insert(name, Array1::zeros(size));
    }
}

impl OptimizerWrapper for SGDWrapper {
    fn step(&mut self, learning_rate: f32) -> Result<()> {
        // Use provided learning rate or default
        let lr = if learning_rate > 0.0 {
            learning_rate
        } else {
            self.learning_rate
        };

        // Update each parameter using SGD with momentum
        for (param_name, param_values) in self.parameters.iter_mut() {
            // Get gradient for this parameter
            let grad = self.gradients.get(param_name).ok_or_else(|| {
                Error::Backend(format!("No gradient found for parameter: {}", param_name))
            })?;

            // Get velocity buffer (momentum)
            let vel = self.velocity.get_mut(param_name).ok_or_else(|| {
                Error::Backend(format!("No velocity buffer for parameter: {}", param_name))
            })?;

            // Apply weight decay if configured: grad = grad + weight_decay * param
            // (L2 regularization)
            for i in 0..param_values.len() {
                let mut effective_grad = grad[i];
                if self.weight_decay > 0.0 {
                    effective_grad += self.weight_decay * param_values[i];
                }

                // Update velocity with momentum: v_t = momentum * v_{t-1} + grad
                if self.momentum > 0.0 {
                    vel[i] = self.momentum * vel[i] + effective_grad;
                    // Update parameter: theta_t = theta_{t-1} - lr * v_t
                    param_values[i] -= lr * vel[i];
                } else {
                    // Pure SGD without momentum: theta_t = theta_{t-1} - lr * grad
                    param_values[i] -= lr * effective_grad;
                }
            }
        }

        tracing::trace!(
            "SGD step completed: lr={}, parameters={}",
            lr,
            self.parameters.len()
        );

        Ok(())
    }

    fn zero_grad(&mut self) -> Result<()> {
        // Zero out all gradients
        for (_param_name, grad) in self.gradients.iter_mut() {
            grad.fill(0.0);
        }

        tracing::trace!("Gradients zeroed for {} parameters", self.gradients.len());
        Ok(())
    }
}

#[cfg(test)]
mod optimizer_tests {
    use super::*;

    #[test]
    fn test_adam_optimizer_initialization() {
        let adam = AdamWrapper::new(0.001, 0.9, 0.999, 1e-8);
        assert!(adam.is_ok());

        let optimizer = adam.expect("Failed to create Adam optimizer");
        assert!((optimizer.learning_rate - 0.001).abs() < 1e-9);
        assert!((optimizer.beta1 - 0.9).abs() < 1e-9);
        assert!((optimizer.beta2 - 0.999).abs() < 1e-9);
        assert_eq!(optimizer.t, 0);
        assert!(optimizer.parameters.is_empty());
        assert!(optimizer.gradients.is_empty());
    }

    #[test]
    fn test_adam_parameter_registration() {
        let mut adam = AdamWrapper::new(0.001, 0.9, 0.999, 1e-8).expect("Failed to create Adam");

        let param = Array1::from_vec(vec![1.0, 2.0, 3.0]);
        adam.register_parameter("weights".to_string(), param.clone());

        assert_eq!(adam.parameters.len(), 1);
        assert_eq!(adam.gradients.len(), 1);
        assert_eq!(adam.m.len(), 1);
        assert_eq!(adam.v.len(), 1);

        let stored_param = adam.parameters.get("weights").expect("Parameter not found");
        assert_eq!(stored_param.len(), 3);
    }

    #[test]
    fn test_adam_zero_grad() {
        let mut adam = AdamWrapper::new(0.001, 0.9, 0.999, 1e-8).expect("Failed to create Adam");

        let param = Array1::from_vec(vec![1.0, 2.0, 3.0]);
        adam.register_parameter("weights".to_string(), param);

        // Set some gradients
        if let Some(grad) = adam.gradients.get_mut("weights") {
            grad[0] = 1.5;
            grad[1] = 2.5;
            grad[2] = 3.5;
        }

        // Zero gradients
        let result = adam.zero_grad();
        assert!(result.is_ok());

        // Verify all gradients are zero
        let grad = adam.gradients.get("weights").expect("Gradient not found");
        assert_eq!(grad[0], 0.0);
        assert_eq!(grad[1], 0.0);
        assert_eq!(grad[2], 0.0);
    }

    #[test]
    fn test_adam_optimizer_step() {
        let mut adam = AdamWrapper::new(0.1, 0.9, 0.999, 1e-8).expect("Failed to create Adam");

        // Register a simple parameter
        let initial_param = Array1::from_vec(vec![10.0, 20.0, 30.0]);
        adam.register_parameter("weights".to_string(), initial_param.clone());

        // Set gradients (simulating backward pass)
        if let Some(grad) = adam.gradients.get_mut("weights") {
            grad[0] = 1.0; // Positive gradient -> parameter should decrease
            grad[1] = -1.0; // Negative gradient -> parameter should increase
            grad[2] = 0.0; // Zero gradient -> parameter should stay similar
        }

        // Perform optimizer step
        let result = adam.step(0.1);
        assert!(result.is_ok());

        // Verify parameters changed
        let updated_param = adam.parameters.get("weights").expect("Parameter not found");

        // With Adam, parameters should move in opposite direction of gradients
        assert!(updated_param[0] < initial_param[0], "Parameter with positive gradient should decrease");
        assert!(updated_param[1] > initial_param[1], "Parameter with negative gradient should increase");

        // Time step should increment
        assert_eq!(adam.t, 1);
    }

    #[test]
    fn test_adam_multiple_steps() {
        let mut adam = AdamWrapper::new(0.01, 0.9, 0.999, 1e-8).expect("Failed to create Adam");

        let initial_param = Array1::from_vec(vec![1.0]);
        adam.register_parameter("w".to_string(), initial_param.clone());

        // Simulate multiple optimization steps with constant gradient
        for _ in 0..10 {
            if let Some(grad) = adam.gradients.get_mut("w") {
                grad[0] = 1.0; // Constant positive gradient
            }
            let result = adam.step(0.1);
            assert!(result.is_ok());
        }

        // Parameter should have decreased significantly
        let final_param = adam.parameters.get("w").expect("Parameter not found");
        assert!(final_param[0] < initial_param[0]);
        assert_eq!(adam.t, 10);
    }

    #[test]
    fn test_sgd_optimizer_initialization() {
        let sgd = SGDWrapper::new(0.01, 0.9, 0.0001);
        assert!(sgd.is_ok());

        let optimizer = sgd.expect("Failed to create SGD optimizer");
        assert!((optimizer.learning_rate - 0.01).abs() < 1e-9);
        assert!((optimizer.momentum - 0.9).abs() < 1e-9);
        assert!((optimizer.weight_decay - 0.0001).abs() < 1e-9);
        assert!(optimizer.parameters.is_empty());
        assert!(optimizer.gradients.is_empty());
    }

    #[test]
    fn test_sgd_parameter_registration() {
        let mut sgd = SGDWrapper::new(0.01, 0.9, 0.0).expect("Failed to create SGD");

        let param = Array1::from_vec(vec![1.0, 2.0, 3.0, 4.0]);
        sgd.register_parameter("bias".to_string(), param.clone());

        assert_eq!(sgd.parameters.len(), 1);
        assert_eq!(sgd.gradients.len(), 1);
        assert_eq!(sgd.velocity.len(), 1);

        let stored_param = sgd.parameters.get("bias").expect("Parameter not found");
        assert_eq!(stored_param.len(), 4);
    }

    #[test]
    fn test_sgd_zero_grad() {
        let mut sgd = SGDWrapper::new(0.01, 0.0, 0.0).expect("Failed to create SGD");

        let param = Array1::from_vec(vec![1.0, 2.0]);
        sgd.register_parameter("w".to_string(), param);

        // Set some gradients
        if let Some(grad) = sgd.gradients.get_mut("w") {
            grad[0] = 5.5;
            grad[1] = 6.5;
        }

        // Zero gradients
        let result = sgd.zero_grad();
        assert!(result.is_ok());

        // Verify all gradients are zero
        let grad = sgd.gradients.get("w").expect("Gradient not found");
        assert_eq!(grad[0], 0.0);
        assert_eq!(grad[1], 0.0);
    }

    #[test]
    fn test_sgd_step_without_momentum() {
        let mut sgd = SGDWrapper::new(0.1, 0.0, 0.0).expect("Failed to create SGD");

        let initial_param = Array1::from_vec(vec![10.0, 20.0]);
        sgd.register_parameter("w".to_string(), initial_param.clone());

        // Set gradients
        if let Some(grad) = sgd.gradients.get_mut("w") {
            grad[0] = 2.0; // Positive gradient
            grad[1] = -3.0; // Negative gradient
        }

        // Perform SGD step
        let result = sgd.step(0.1);
        assert!(result.is_ok());

        // Verify parameter updates: theta = theta - lr * grad
        let updated_param = sgd.parameters.get("w").expect("Parameter not found");

        // theta[0] = 10.0 - 0.1 * 2.0 = 9.8
        assert!((updated_param[0] - 9.8).abs() < 1e-6, "Expected 9.8, got {}", updated_param[0]);

        // theta[1] = 20.0 - 0.1 * (-3.0) = 20.3
        assert!((updated_param[1] - 20.3).abs() < 1e-6, "Expected 20.3, got {}", updated_param[1]);
    }

    #[test]
    fn test_sgd_step_with_momentum() {
        let mut sgd = SGDWrapper::new(0.1, 0.9, 0.0).expect("Failed to create SGD");

        let initial_param = Array1::from_vec(vec![5.0]);
        sgd.register_parameter("w".to_string(), initial_param.clone());

        // First step
        if let Some(grad) = sgd.gradients.get_mut("w") {
            grad[0] = 1.0;
        }
        let result = sgd.step(0.1);
        assert!(result.is_ok());

        let param_after_step1 = sgd.parameters.get("w").expect("Parameter not found");
        // v = 0.9 * 0 + 1.0 = 1.0
        // theta = 5.0 - 0.1 * 1.0 = 4.9
        assert!((param_after_step1[0] - 4.9).abs() < 1e-6);

        // Second step with same gradient
        if let Some(grad) = sgd.gradients.get_mut("w") {
            grad[0] = 1.0;
        }
        let result = sgd.step(0.1);
        assert!(result.is_ok());

        let param_after_step2 = sgd.parameters.get("w").expect("Parameter not found");
        // v = 0.9 * 1.0 + 1.0 = 1.9
        // theta = 4.9 - 0.1 * 1.9 = 4.71
        assert!((param_after_step2[0] - 4.71).abs() < 1e-6);
    }

    #[test]
    fn test_sgd_weight_decay() {
        let mut sgd = SGDWrapper::new(0.1, 0.0, 0.01).expect("Failed to create SGD");

        let initial_param = Array1::from_vec(vec![10.0]);
        sgd.register_parameter("w".to_string(), initial_param.clone());

        // Set gradient to zero to isolate weight decay effect
        if let Some(grad) = sgd.gradients.get_mut("w") {
            grad[0] = 0.0;
        }

        // Perform SGD step
        let result = sgd.step(0.1);
        assert!(result.is_ok());

        // With weight decay: effective_grad = 0.0 + 0.01 * 10.0 = 0.1
        // theta = 10.0 - 0.1 * 0.1 = 9.99
        let updated_param = sgd.parameters.get("w").expect("Parameter not found");
        assert!((updated_param[0] - 9.99).abs() < 1e-6);
    }

    #[test]
    fn test_optimizer_type_enum() {
        let adam_type = OptimizerType::Adam {
            beta1: 0.9,
            beta2: 0.999,
            epsilon: 1e-8,
        };

        let sgd_type = OptimizerType::SGD {
            momentum: 0.9,
            weight_decay: 0.0001,
        };

        // Just verify enum variants exist and can be matched
        match adam_type {
            OptimizerType::Adam { beta1, beta2, epsilon } => {
                assert!((beta1 - 0.9).abs() < 1e-9);
                assert!((beta2 - 0.999).abs() < 1e-9);
                assert!((epsilon - 1e-8).abs() < 1e-16);
            }
            _ => panic!("Wrong optimizer type"),
        }

        match sgd_type {
            OptimizerType::SGD { momentum, weight_decay } => {
                assert!((momentum - 0.9).abs() < 1e-9);
                assert!((weight_decay - 0.0001).abs() < 1e-9);
            }
            _ => panic!("Wrong optimizer type"),
        }
    }

    #[test]
    fn test_convergence_simple_quadratic() {
        // Test that Adam can minimize a simple quadratic function: f(x) = x^2
        // Gradient: df/dx = 2x
        let mut adam = AdamWrapper::new(0.1, 0.9, 0.999, 1e-8).expect("Failed to create Adam");

        let initial_param = Array1::from_vec(vec![10.0]); // Start far from optimum
        adam.register_parameter("x".to_string(), initial_param.clone());

        // Run optimization for several steps
        for _ in 0..50 {
            // Compute gradient: df/dx = 2x
            let current_x = adam.parameters.get("x").expect("Parameter not found")[0];
            if let Some(grad) = adam.gradients.get_mut("x") {
                grad[0] = 2.0 * current_x;
            }

            let result = adam.step(0.1);
            assert!(result.is_ok());
        }

        // Parameter should converge close to 0 (the minimum of x^2)
        let final_x = adam.parameters.get("x").expect("Parameter not found")[0];
        assert!(final_x.abs() < 1.0, "Adam should converge x^2 to near 0, got {}", final_x);
    }
}
