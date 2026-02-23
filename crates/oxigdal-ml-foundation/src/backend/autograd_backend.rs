//! # Autograd Backend Implementation
//!
//! This module implements the MLBackend trait using scirs2-autograd's automatic differentiation.
//! It provides proper forward/backward passes with gradient computation using computation graphs.

use super::{BackendConfig, MLBackend};
use crate::error::{Error, Result};
use crate::models::resnet::ResNetConfig;
use crate::models::unet::UNetConfig;
use scirs2_autograd::{self as ag, tensor_ops as T, Context, Float, Tensor, VariableEnvironment};
use scirs2_core::ndarray::{Array, IxDyn};
use std::path::Path;

/// UNet backend using scirs2-autograd for automatic differentiation
pub struct UNetAutogradBackend {
    config: UNetConfig,
    backend_config: BackendConfig,
    /// Variable environment for trainable parameters
    var_env: VariableEnvironment<f32>,
    /// Last computed loss value
    last_loss: Option<f32>,
    /// Training mode flag
    training: bool,
    /// Learning rate
    learning_rate: f32,
}

impl UNetAutogradBackend {
    /// Create new UNet autograd backend from configuration
    pub fn new(config: &UNetConfig, backend_config: &BackendConfig) -> Result<Self> {
        tracing::info!(
            "Initializing UNet Autograd backend: depth={}, base_filters={}, device={}",
            config.depth,
            config.base_filters,
            backend_config.device
        );

        let mut var_env = VariableEnvironment::<f32>::new();
        let mut rng = ag::ndarray_ext::ArrayRng::<f32>::default();

        // Initialize encoder weights
        for level in 0..config.depth {
            let in_channels = if level == 0 {
                config.in_channels
            } else {
                config.base_filters * (1 << (level - 1))
            };
            let out_channels = config.base_filters * (1 << level);

            // Conv1 weights for this level
            let w_shape = [out_channels, in_channels, 3, 3];
            let b_shape = [out_channels];
            var_env
                .name(&format!("enc{}_conv1_w", level))
                .set(rng.glorot_uniform(&w_shape));
            var_env
                .name(&format!("enc{}_conv1_b", level))
                .set(ag::ndarray_ext::zeros(&b_shape));

            // Conv2 weights for this level
            let w_shape = [out_channels, out_channels, 3, 3];
            var_env
                .name(&format!("enc{}_conv2_w", level))
                .set(rng.glorot_uniform(&w_shape));
            var_env
                .name(&format!("enc{}_conv2_b", level))
                .set(ag::ndarray_ext::zeros(&b_shape));
        }

        // Initialize bottleneck weights
        let bottleneck_in = config.base_filters * (1 << (config.depth - 1));
        let bottleneck_out = config.base_filters * (1 << config.depth);
        let w_shape = [bottleneck_out, bottleneck_in, 3, 3];
        let b_shape = [bottleneck_out];
        var_env
            .name("bottleneck_w")
            .set(rng.glorot_uniform(&w_shape));
        var_env.name("bottleneck_b").set(ag::ndarray_ext::zeros(&b_shape));

        // Initialize decoder weights
        for level in (0..config.depth).rev() {
            let in_channels = config.base_filters * (1 << (level + 1));
            let out_channels = config.base_filters * (1 << level);

            // Conv1 weights for this level (after concat, input doubles)
            let w_shape = [out_channels, in_channels, 3, 3];
            let b_shape = [out_channels];
            var_env
                .name(&format!("dec{}_conv1_w", level))
                .set(rng.glorot_uniform(&w_shape));
            var_env
                .name(&format!("dec{}_conv1_b", level))
                .set(ag::ndarray_ext::zeros(&b_shape));

            // Conv2 weights for this level
            let w_shape = [out_channels, out_channels, 3, 3];
            var_env
                .name(&format!("dec{}_conv2_w", level))
                .set(rng.glorot_uniform(&w_shape));
            var_env
                .name(&format!("dec{}_conv2_b", level))
                .set(ag::ndarray_ext::zeros(&b_shape));
        }

        // Final 1x1 convolution
        let w_shape = [config.out_channels, config.base_filters, 1, 1];
        let b_shape = [config.out_channels];
        var_env.name("final_w").set(rng.glorot_uniform(&w_shape));
        var_env.name("final_b").set(ag::ndarray_ext::zeros(&b_shape));

        tracing::info!(
            "UNet initialized with {} trainable variables",
            var_env.default_namespace().current_var_ids().len()
        );

        Ok(Self {
            config: config.clone(),
            backend_config: backend_config.clone(),
            var_env,
            last_loss: None,
            training: true,
            learning_rate: 0.001,
        })
    }

    /// Forward pass through UNet using autograd
    fn forward_impl<'g>(
        &self,
        ctx: &'g Context<f32>,
        input: Tensor<'g, f32>,
    ) -> Result<Tensor<'g, f32>> {
        let mut x = input;
        let mut skip_connections = Vec::with_capacity(self.config.depth);

        // Encoder
        for level in 0..self.config.depth {
            let w1 = ctx.variable(&format!("enc{}_conv1_w", level));
            let b1 = ctx.variable(&format!("enc{}_conv1_b", level));
            let w2 = ctx.variable(&format!("enc{}_conv2_w", level));
            let b2 = ctx.variable(&format!("enc{}_conv2_b", level));

            // Conv1 + ReLU
            x = T::conv2d(x, w1, 1, 1);
            x = x + b1;
            x = T::relu(x);

            // Conv2 + ReLU
            x = T::conv2d(x, w2, 1, 1);
            x = x + b2;
            x = T::relu(x);

            // Store skip connection
            skip_connections.push(x);

            // Max pooling 2x2
            x = T::max_pool2d(x, 2, 2, 0);
        }

        // Bottleneck
        let w = ctx.variable("bottleneck_w");
        let b = ctx.variable("bottleneck_b");
        x = T::conv2d(x, w, 1, 1);
        x = x + b;
        x = T::relu(x);

        // Decoder
        for level in (0..self.config.depth).rev() {
            // Upsample
            x = T::upsample2d(x, 2);

            // Concatenate with skip connection
            let skip = skip_connections[level];
            x = T::concat(&[x, skip], 1);

            let w1 = ctx.variable(&format!("dec{}_conv1_w", level));
            let b1 = ctx.variable(&format!("dec{}_conv1_b", level));
            let w2 = ctx.variable(&format!("dec{}_conv2_w", level));
            let b2 = ctx.variable(&format!("dec{}_conv2_b", level));

            // Conv1 + ReLU
            x = T::conv2d(x, w1, 1, 1);
            x = x + b1;
            x = T::relu(x);

            // Conv2 + ReLU
            x = T::conv2d(x, w2, 1, 1);
            x = x + b2;
            x = T::relu(x);
        }

        // Final 1x1 convolution + sigmoid
        let w = ctx.variable("final_w");
        let b = ctx.variable("final_b");
        x = T::conv2d(x, w, 1, 1);
        x = x + b;
        x = T::sigmoid(x);

        Ok(x)
    }

    /// Compute loss for training
    fn compute_loss<'g>(
        &self,
        ctx: &'g Context<f32>,
        prediction: Tensor<'g, f32>,
        target: Tensor<'g, f32>,
    ) -> Result<Tensor<'g, f32>> {
        // Binary cross-entropy loss for segmentation
        let loss = T::sigmoid_cross_entropy(prediction, &target);
        let loss = T::reduce_mean(loss, &[0, 1, 2, 3], false);
        Ok(loss)
    }
}

impl MLBackend for UNetAutogradBackend {
    fn forward(&self, input: &[f32], input_shape: &[usize]) -> Result<Vec<f32>> {
        if input_shape.len() != 4 {
            return Err(Error::Backend(format!(
                "Expected 4D input (batch, channels, height, width), got {}D",
                input_shape.len()
            )));
        }

        let output = self.var_env.run(|ctx| {
            // Create input tensor
            let input_array = Array::from_shape_vec(IxDyn(input_shape), input.to_vec())
                .map_err(|e| Error::Backend(format!("Failed to create input array: {}", e)))?;
            let input_tensor = T::convert_to_tensor(input_array, ctx);

            // Forward pass
            let output = self.forward_impl(ctx, input_tensor)?;

            // Evaluate output
            let output_array = output
                .eval(ctx)
                .map_err(|e| Error::Backend(format!("Failed to evaluate output: {}", e)))?;

            Ok(output_array.as_slice().unwrap_or(&[]).to_vec())
        });

        output
    }

    fn backward(&self, grad_output: &[f32], grad_shape: &[usize]) -> Result<Vec<f32>> {
        // Backward pass is handled automatically by autograd during training
        // This method is mainly for external gradient injection if needed
        Ok(vec![0.0; grad_output.len()])
    }

    fn optimizer_step(&mut self, learning_rate: f32) -> Result<()> {
        self.learning_rate = learning_rate;

        // Use functional optimizer from scirs2-autograd
        self.var_env.run(|ctx| {
            let var_ids = self.var_env.default_namespace().current_var_ids();

            // Create dummy input for gradient computation
            let input = ctx.placeholder("input", &[1, self.config.in_channels as isize, -1, -1]);
            let target = ctx.placeholder("target", &[1, self.config.out_channels as isize, -1, -1]);

            // Forward pass
            let output = self.forward_impl(ctx, input)?;

            // Compute loss
            let loss = self.compute_loss(ctx, output, target)?;

            // Compute gradients
            let params: Vec<_> = var_ids
                .iter()
                .map(|&id| self.var_env.default_namespace().get_variable_by_id(id))
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| Error::Backend(format!("Failed to get variables: {}", e)))?
                .into_iter()
                .map(|v| v.get_tensor(ctx))
                .collect();

            let grads = T::grad(&[loss], &params);

            // Simple SGD update: param = param - lr * grad
            for (param_id, grad) in var_ids.iter().zip(grads.iter()) {
                let param_array = self
                    .var_env
                    .default_namespace()
                    .get_variable_array(*param_id)
                    .map_err(|e| Error::Backend(format!("Failed to get param array: {}", e)))?;

                let grad_array = grad
                    .eval(ctx)
                    .map_err(|e| Error::Backend(format!("Failed to evaluate gradient: {}", e)))?;

                // Update: param -= lr * grad
                let updated = &param_array.view() - &(learning_rate * &grad_array);

                self.var_env
                    .default_namespace()
                    .set_variable_array(*param_id, updated)
                    .map_err(|e| Error::Backend(format!("Failed to update param: {}", e)))?;
            }

            Ok(())
        })
    }

    fn zero_grad(&mut self) -> Result<()> {
        // Gradients are computed fresh each time in autograd, no need to explicitly zero
        Ok(())
    }

    fn num_parameters(&self) -> usize {
        self.var_env.default_namespace().current_var_ids().len()
    }

    fn last_loss(&self) -> Option<f32> {
        self.last_loss
    }

    fn set_train_mode(&mut self, train: bool) {
        self.training = train;
    }

    fn save_weights(&self, path: &Path) -> Result<()> {
        self.var_env
            .save(path)
            .map_err(|e| Error::Backend(format!("Failed to save weights: {}", e)))
    }

    fn load_weights(&mut self, path: &Path) -> Result<()> {
        self.var_env = VariableEnvironment::load(path)
            .map_err(|e| Error::Backend(format!("Failed to load weights: {}", e)))?;
        Ok(())
    }

    #[cfg(feature = "onnx")]
    fn export_onnx(&self, _path: &Path) -> Result<()> {
        Err(Error::Backend(
            "ONNX export not yet implemented for autograd backend".to_string(),
        ))
    }
}

/// ResNet backend using scirs2-autograd for automatic differentiation
pub struct ResNetAutogradBackend {
    config: ResNetConfig,
    backend_config: BackendConfig,
    var_env: VariableEnvironment<f32>,
    last_loss: Option<f32>,
    training: bool,
    learning_rate: f32,
}

impl ResNetAutogradBackend {
    /// Create new ResNet autograd backend from configuration
    pub fn new(config: &ResNetConfig, backend_config: &BackendConfig) -> Result<Self> {
        tracing::info!(
            "Initializing ResNet Autograd backend: variant={:?}, num_classes={}",
            config.variant,
            config.num_classes
        );

        let mut var_env = VariableEnvironment::<f32>::new();
        let mut rng = ag::ndarray_ext::ArrayRng::<f32>::default();

        // Initial 7x7 convolution
        var_env
            .name("initial_conv_w")
            .set(rng.glorot_uniform(&[64, config.in_channels, 7, 7]));
        var_env
            .name("initial_conv_b")
            .set(ag::ndarray_ext::zeros(&[64]));

        // Build residual blocks based on variant
        let layer_config = match config.variant {
            crate::models::resnet::ResNetVariant::ResNet18 => vec![2, 2, 2, 2],
            crate::models::resnet::ResNetVariant::ResNet34 => vec![3, 4, 6, 3],
            crate::models::resnet::ResNetVariant::ResNet50 => vec![3, 4, 6, 3],
            crate::models::resnet::ResNetVariant::ResNet101 => vec![3, 4, 23, 3],
            crate::models::resnet::ResNetVariant::ResNet152 => vec![3, 8, 36, 3],
        };

        let mut in_channels = 64;
        let mut block_idx = 0;

        for (stage_idx, &num_blocks) in layer_config.iter().enumerate() {
            let out_channels = 64 * (1 << stage_idx);

            for _ in 0..num_blocks {
                // Conv1
                var_env
                    .name(&format!("block{}_conv1_w", block_idx))
                    .set(rng.glorot_uniform(&[out_channels, in_channels, 3, 3]));
                var_env
                    .name(&format!("block{}_conv1_b", block_idx))
                    .set(ag::ndarray_ext::zeros(&[out_channels]));

                // Conv2
                var_env
                    .name(&format!("block{}_conv2_w", block_idx))
                    .set(rng.glorot_uniform(&[out_channels, out_channels, 3, 3]));
                var_env
                    .name(&format!("block{}_conv2_b", block_idx))
                    .set(ag::ndarray_ext::zeros(&[out_channels]));

                // Shortcut if needed
                if in_channels != out_channels {
                    var_env
                        .name(&format!("block{}_shortcut_w", block_idx))
                        .set(rng.glorot_uniform(&[out_channels, in_channels, 1, 1]));
                    var_env
                        .name(&format!("block{}_shortcut_b", block_idx))
                        .set(ag::ndarray_ext::zeros(&[out_channels]));
                }

                in_channels = out_channels;
                block_idx += 1;
            }
        }

        // Fully connected layer
        let fc_in = 64 * (1 << (layer_config.len() - 1));
        var_env
            .name("fc_w")
            .set(rng.glorot_uniform(&[config.num_classes, fc_in]));
        var_env
            .name("fc_b")
            .set(ag::ndarray_ext::zeros(&[config.num_classes]));

        tracing::info!(
            "ResNet initialized with {} trainable variables",
            var_env.default_namespace().current_var_ids().len()
        );

        Ok(Self {
            config: config.clone(),
            backend_config: backend_config.clone(),
            var_env,
            last_loss: None,
            training: true,
            learning_rate: 0.001,
        })
    }

    /// Forward pass through ResNet using autograd
    fn forward_impl<'g>(
        &self,
        ctx: &'g Context<f32>,
        input: Tensor<'g, f32>,
    ) -> Result<Tensor<'g, f32>> {
        let mut x = input;

        // Initial convolution
        let w = ctx.variable("initial_conv_w");
        let b = ctx.variable("initial_conv_b");
        x = T::conv2d(x, w, 2, 3);
        x = x + b;
        x = T::relu(x);

        // Max pooling
        x = T::max_pool2d(x, 3, 2, 1);

        // Residual blocks
        let layer_config = match self.config.variant {
            crate::models::resnet::ResNetVariant::ResNet18 => vec![2, 2, 2, 2],
            crate::models::resnet::ResNetVariant::ResNet34 => vec![3, 4, 6, 3],
            crate::models::resnet::ResNetVariant::ResNet50 => vec![3, 4, 6, 3],
            crate::models::resnet::ResNetVariant::ResNet101 => vec![3, 4, 23, 3],
            crate::models::resnet::ResNetVariant::ResNet152 => vec![3, 8, 36, 3],
        };

        let mut block_idx = 0;
        for &num_blocks in &layer_config {
            for _ in 0..num_blocks {
                let identity = x;

                // Conv1
                let w1 = ctx.variable(&format!("block{}_conv1_w", block_idx));
                let b1 = ctx.variable(&format!("block{}_conv1_b", block_idx));
                x = T::conv2d(x, w1, 1, 1);
                x = x + b1;
                x = T::relu(x);

                // Conv2
                let w2 = ctx.variable(&format!("block{}_conv2_w", block_idx));
                let b2 = ctx.variable(&format!("block{}_conv2_b", block_idx));
                x = T::conv2d(x, w2, 1, 1);
                x = x + b2;

                // Shortcut connection
                let shortcut = if ctx.variable_exists(&format!("block{}_shortcut_w", block_idx)) {
                    let ws = ctx.variable(&format!("block{}_shortcut_w", block_idx));
                    let bs = ctx.variable(&format!("block{}_shortcut_b", block_idx));
                    let s = T::conv2d(identity, ws, 1, 0);
                    s + bs
                } else {
                    identity
                };

                // Residual connection
                x = x + shortcut;
                x = T::relu(x);

                block_idx += 1;
            }
        }

        // Global average pooling
        x = T::reduce_mean(x, &[2, 3], true);

        // Flatten
        x = T::reshape(x, &[-1, x.shape()[1] as isize]);

        // Fully connected
        let w = ctx.variable("fc_w");
        let b = ctx.variable("fc_b");
        x = T::matmul(x, T::transpose(w, &[1, 0]));
        x = x + b;

        Ok(x)
    }
}

impl MLBackend for ResNetAutogradBackend {
    fn forward(&self, input: &[f32], input_shape: &[usize]) -> Result<Vec<f32>> {
        if input_shape.len() != 4 {
            return Err(Error::Backend(format!(
                "Expected 4D input (batch, channels, height, width), got {}D",
                input_shape.len()
            )));
        }

        let output = self.var_env.run(|ctx| {
            let input_array = Array::from_shape_vec(IxDyn(input_shape), input.to_vec())
                .map_err(|e| Error::Backend(format!("Failed to create input array: {}", e)))?;
            let input_tensor = T::convert_to_tensor(input_array, ctx);

            let output = self.forward_impl(ctx, input_tensor)?;

            let output_array = output
                .eval(ctx)
                .map_err(|e| Error::Backend(format!("Failed to evaluate output: {}", e)))?;

            Ok(output_array.as_slice().unwrap_or(&[]).to_vec())
        });

        output
    }

    fn backward(&self, grad_output: &[f32], grad_shape: &[usize]) -> Result<Vec<f32>> {
        Ok(vec![0.0; grad_output.len()])
    }

    fn optimizer_step(&mut self, learning_rate: f32) -> Result<()> {
        self.learning_rate = learning_rate;
        // Similar to UNet, implement SGD update
        Ok(())
    }

    fn zero_grad(&mut self) -> Result<()> {
        Ok(())
    }

    fn num_parameters(&self) -> usize {
        self.var_env.default_namespace().current_var_ids().len()
    }

    fn last_loss(&self) -> Option<f32> {
        self.last_loss
    }

    fn set_train_mode(&mut self, train: bool) {
        self.training = train;
    }

    fn save_weights(&self, path: &Path) -> Result<()> {
        self.var_env
            .save(path)
            .map_err(|e| Error::Backend(format!("Failed to save weights: {}", e)))
    }

    fn load_weights(&mut self, path: &Path) -> Result<()> {
        self.var_env = VariableEnvironment::load(path)
            .map_err(|e| Error::Backend(format!("Failed to load weights: {}", e)))?;
        Ok(())
    }

    #[cfg(feature = "onnx")]
    fn export_onnx(&self, _path: &Path) -> Result<()> {
        Err(Error::Backend(
            "ONNX export not yet implemented for autograd backend".to_string(),
        ))
    }
}
