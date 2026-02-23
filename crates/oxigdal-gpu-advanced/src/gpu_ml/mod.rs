//! GPU-accelerated ML inference for geospatial data.

mod compute;
mod neural;
#[cfg(test)]
#[allow(clippy::panic)]
mod tests;

use crate::error::{GpuAdvancedError, Result};
use oxigdal_gpu::GpuContext;
use std::sync::Arc;
use wgpu::util::DeviceExt;

/// GPU buffer handle for model data
#[derive(Debug)]
pub struct GpuBuffer {
    /// The wgpu buffer
    buffer: wgpu::Buffer,
    /// Size in bytes
    size: u64,
}

impl GpuBuffer {
    /// Get the underlying wgpu buffer
    pub fn buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }

    /// Get buffer size in bytes
    pub fn size(&self) -> u64 {
        self.size
    }
}

/// Model layer type
#[derive(Debug, Clone)]
pub enum LayerType {
    /// Dense (fully connected) layer
    Dense {
        /// Input features
        input_features: usize,
        /// Output features
        output_features: usize,
    },
    /// 2D Convolution layer
    Conv2d {
        /// Input channels
        input_channels: usize,
        /// Output channels
        output_channels: usize,
        /// Kernel size
        kernel_size: usize,
    },
    /// Batch normalization layer
    BatchNorm {
        /// Number of features
        num_features: usize,
        /// Epsilon for numerical stability
        epsilon: f32,
    },
    /// Pooling layer
    Pool2d {
        /// Pool type
        pool_type: PoolType,
        /// Pool size
        pool_size: usize,
        /// Stride
        stride: usize,
    },
    /// Activation layer
    Activation {
        /// Activation type
        activation: ActivationType,
    },
    /// Flatten layer
    Flatten,
    /// Dropout layer (inference mode - no-op)
    Dropout {
        /// Dropout rate (unused during inference)
        _rate: f32,
    },
}

/// GPU-resident model layer with weights
pub struct GpuLayer {
    /// Layer type
    pub(crate) layer_type: LayerType,
    /// Weight buffer (if applicable)
    pub(crate) weights: Option<GpuBuffer>,
    /// Bias buffer (if applicable)
    pub(crate) bias: Option<GpuBuffer>,
    /// Additional parameters (e.g., batch norm mean/var)
    pub(crate) extra_params: Vec<GpuBuffer>,
    /// Cached pipeline for this layer
    pub(crate) pipeline: Option<wgpu::ComputePipeline>,
    /// Bind group layout
    pub(crate) bind_group_layout: Option<wgpu::BindGroupLayout>,
}

impl GpuLayer {
    /// Get the layer type
    pub fn layer_type(&self) -> &LayerType {
        &self.layer_type
    }

    /// Check if this layer has trainable weights
    pub fn has_weights(&self) -> bool {
        self.weights.is_some()
    }

    /// Get the weights buffer if present
    pub fn weights(&self) -> Option<&GpuBuffer> {
        self.weights.as_ref()
    }

    /// Get the bias buffer if present
    pub fn bias(&self) -> Option<&GpuBuffer> {
        self.bias.as_ref()
    }

    /// Get extra parameters (e.g., batch norm mean/var/gamma/beta)
    ///
    /// These are additional GPU buffers needed by certain layer types
    /// like BatchNorm that require more than just weights and bias.
    pub fn extra_params(&self) -> &[GpuBuffer] {
        &self.extra_params
    }

    /// Get the cached compute pipeline if present
    ///
    /// Pipelines can be cached to avoid recreation on each inference call.
    /// This is used for performance optimization in repeated inference.
    pub fn pipeline(&self) -> Option<&wgpu::ComputePipeline> {
        self.pipeline.as_ref()
    }

    /// Get the cached bind group layout if present
    ///
    /// Bind group layouts define the structure of resources bound to shaders.
    /// Caching these avoids recreation overhead during inference.
    pub fn bind_group_layout(&self) -> Option<&wgpu::BindGroupLayout> {
        self.bind_group_layout.as_ref()
    }
}

/// GPU-resident model for inference
pub struct GpuModel {
    /// Model layers
    layers: Vec<GpuLayer>,
    /// GPU context
    context: Arc<GpuContext>,
    /// Model name
    name: String,
    /// Input shape (batch dimension excluded)
    input_shape: Vec<usize>,
    /// Output shape (batch dimension excluded)
    output_shape: Vec<usize>,
}

impl GpuModel {
    /// Create a new empty GPU model
    pub fn new(context: Arc<GpuContext>, name: impl Into<String>) -> Self {
        Self {
            layers: Vec::new(),
            context,
            name: name.into(),
            input_shape: Vec::new(),
            output_shape: Vec::new(),
        }
    }

    /// Set input shape
    pub fn with_input_shape(mut self, shape: Vec<usize>) -> Self {
        self.input_shape = shape;
        self
    }

    /// Set output shape
    pub fn with_output_shape(mut self, shape: Vec<usize>) -> Self {
        self.output_shape = shape;
        self
    }

    /// Add a dense layer with weights loaded to GPU
    pub fn add_dense_layer(
        &mut self,
        input_features: usize,
        output_features: usize,
        weights: &[f32],
        bias: &[f32],
    ) -> Result<()> {
        // Validate dimensions
        let expected_weights = input_features * output_features;
        if weights.len() != expected_weights {
            return Err(GpuAdvancedError::invalid_parameter(format!(
                "Dense layer weight size mismatch: expected {}, got {}",
                expected_weights,
                weights.len()
            )));
        }
        if bias.len() != output_features {
            return Err(GpuAdvancedError::invalid_parameter(format!(
                "Dense layer bias size mismatch: expected {}, got {}",
                output_features,
                bias.len()
            )));
        }

        // Create GPU buffers for weights and bias
        let weights_buffer =
            self.context
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Dense Weights Buffer"),
                    contents: bytemuck::cast_slice(weights),
                    usage: wgpu::BufferUsages::STORAGE,
                });

        let bias_buffer =
            self.context
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Dense Bias Buffer"),
                    contents: bytemuck::cast_slice(bias),
                    usage: wgpu::BufferUsages::STORAGE,
                });

        let layer = GpuLayer {
            layer_type: LayerType::Dense {
                input_features,
                output_features,
            },
            weights: Some(GpuBuffer {
                buffer: weights_buffer,
                size: std::mem::size_of_val(weights) as u64,
            }),
            bias: Some(GpuBuffer {
                buffer: bias_buffer,
                size: std::mem::size_of_val(bias) as u64,
            }),
            extra_params: Vec::new(),
            pipeline: None,
            bind_group_layout: None,
        };

        self.layers.push(layer);
        Ok(())
    }

    /// Add an activation layer
    pub fn add_activation_layer(&mut self, activation: ActivationType) {
        let layer = GpuLayer {
            layer_type: LayerType::Activation { activation },
            weights: None,
            bias: None,
            extra_params: Vec::new(),
            pipeline: None,
            bind_group_layout: None,
        };
        self.layers.push(layer);
    }

    /// Add a flatten layer
    pub fn add_flatten_layer(&mut self) {
        let layer = GpuLayer {
            layer_type: LayerType::Flatten,
            weights: None,
            bias: None,
            extra_params: Vec::new(),
            pipeline: None,
            bind_group_layout: None,
        };
        self.layers.push(layer);
    }

    /// Get number of layers
    pub fn num_layers(&self) -> usize {
        self.layers.len()
    }

    /// Get model name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get input shape
    pub fn input_shape(&self) -> &[usize] {
        &self.input_shape
    }

    /// Get output shape
    pub fn output_shape(&self) -> &[usize] {
        &self.output_shape
    }

    /// Get GPU context
    pub fn context(&self) -> &Arc<GpuContext> {
        &self.context
    }

    /// Get layers
    pub fn layers(&self) -> &[GpuLayer] {
        &self.layers
    }
}

/// GPU ML inference engine
pub struct GpuMlInference {
    /// GPU context
    context: Arc<GpuContext>,
    /// Batch size
    batch_size: usize,
    /// Use mixed precision (FP16/FP32)
    mixed_precision: bool,
    /// Loaded model (optional)
    model: Option<GpuModel>,
}

impl GpuMlInference {
    /// Create a new GPU ML inference engine
    pub fn new(context: Arc<GpuContext>, batch_size: usize) -> Self {
        Self {
            context,
            batch_size,
            mixed_precision: false,
            model: None,
        }
    }

    /// Enable mixed precision inference
    pub fn with_mixed_precision(mut self, enabled: bool) -> Self {
        self.mixed_precision = enabled;
        self
    }

    /// Load a model for inference
    pub fn load_model(&mut self, model: GpuModel) {
        self.model = Some(model);
    }

    /// Create and load a simple feedforward model
    pub fn create_feedforward_model(
        &mut self,
        name: &str,
        layer_sizes: &[usize],
        weights: &[Vec<f32>],
        biases: &[Vec<f32>],
        activations: &[ActivationType],
    ) -> Result<()> {
        if layer_sizes.len() < 2 {
            return Err(GpuAdvancedError::invalid_parameter(
                "Model must have at least input and output layer",
            ));
        }

        let num_layers = layer_sizes.len() - 1;
        if weights.len() != num_layers || biases.len() != num_layers {
            return Err(GpuAdvancedError::invalid_parameter(
                "Number of weight/bias arrays must match number of layers",
            ));
        }

        let mut model = GpuModel::new(Arc::clone(&self.context), name)
            .with_input_shape(vec![layer_sizes[0]])
            .with_output_shape(vec![layer_sizes[layer_sizes.len() - 1]]);

        for i in 0..num_layers {
            model.add_dense_layer(layer_sizes[i], layer_sizes[i + 1], &weights[i], &biases[i])?;

            // Add activation if provided
            if i < activations.len() {
                model.add_activation_layer(activations[i]);
            }
        }

        self.model = Some(model);
        Ok(())
    }

    /// Run batch inference
    pub async fn infer_batch(&self, inputs: &[Vec<f32>]) -> Result<Vec<Vec<f32>>> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }

        let mut results = Vec::with_capacity(inputs.len());

        // Process in batches
        for chunk in inputs.chunks(self.batch_size) {
            let batch_results = self.process_batch(chunk).await?;
            results.extend(batch_results);
        }

        Ok(results)
    }

    /// Process a single batch through the loaded model
    async fn process_batch(&self, batch: &[Vec<f32>]) -> Result<Vec<Vec<f32>>> {
        // Check if model is loaded
        let model = self.model.as_ref().ok_or_else(|| {
            GpuAdvancedError::MlInferenceError("No model loaded for inference".to_string())
        })?;

        if batch.is_empty() {
            return Ok(Vec::new());
        }

        // Validate input dimensions
        let input_size = model.input_shape().iter().product::<usize>();
        for (idx, input) in batch.iter().enumerate() {
            if input.len() != input_size {
                return Err(GpuAdvancedError::invalid_parameter(format!(
                    "Input {} has wrong size: expected {}, got {}",
                    idx,
                    input_size,
                    input.len()
                )));
            }
        }

        let batch_size = batch.len();

        // Flatten batch into contiguous buffer for GPU upload
        let mut flat_input: Vec<f32> = Vec::with_capacity(batch_size * input_size);
        for input in batch {
            flat_input.extend_from_slice(input);
        }

        // Process through each layer
        let mut current_data = flat_input;
        let mut current_feature_size = input_size;

        for layer in model.layers() {
            match layer.layer_type() {
                LayerType::Dense {
                    input_features,
                    output_features,
                } => {
                    // Execute dense layer on GPU
                    current_data = self
                        .execute_dense_layer(
                            &current_data,
                            layer,
                            batch_size,
                            *input_features,
                            *output_features,
                        )
                        .await?;
                    current_feature_size = *output_features;
                }
                LayerType::Activation { activation } => {
                    // Execute activation on GPU
                    current_data = self.activation(&current_data, *activation).await?;
                }
                LayerType::Flatten => {
                    // Flatten is a no-op when data is already flattened
                    continue;
                }
                LayerType::Dropout { .. } => {
                    // Dropout is a no-op during inference
                    continue;
                }
                _ => {
                    // Other layer types would be handled here
                    return Err(GpuAdvancedError::NotImplemented(format!(
                        "Layer type {:?} not yet supported in batched inference",
                        layer.layer_type()
                    )));
                }
            }
        }

        // Split results back into individual outputs
        let output_size = current_feature_size;
        let mut results = Vec::with_capacity(batch_size);
        for i in 0..batch_size {
            let start = i * output_size;
            let end = start + output_size;
            results.push(current_data[start..end].to_vec());
        }

        Ok(results)
    }

    /// Dynamic batching for variable-sized inputs
    pub async fn dynamic_batch(&self, inputs: Vec<Vec<f32>>) -> Result<Vec<Vec<f32>>> {
        // Group inputs by size
        let mut size_groups: std::collections::HashMap<usize, Vec<Vec<f32>>> =
            std::collections::HashMap::new();

        for input in inputs {
            size_groups.entry(input.len()).or_default().push(input);
        }

        let mut all_results = Vec::new();

        // Process each size group
        for (_size, group) in size_groups {
            let results = self.infer_batch(&group).await?;
            all_results.extend(results);
        }

        Ok(all_results)
    }

    /// Get the loaded model (if any)
    pub fn model(&self) -> Option<&GpuModel> {
        self.model.as_ref()
    }

    /// Check if a model is loaded
    pub fn has_model(&self) -> bool {
        self.model.is_some()
    }

    /// Get batch size
    pub fn batch_size(&self) -> usize {
        self.batch_size
    }

    /// Check if mixed precision is enabled
    pub fn is_mixed_precision(&self) -> bool {
        self.mixed_precision
    }
}

/// Activation function types
#[derive(Debug, Clone, Copy)]
pub enum ActivationType {
    /// ReLU activation
    ReLU,
    /// Sigmoid activation
    Sigmoid,
    /// Tanh activation
    Tanh,
    /// Leaky ReLU with alpha parameter
    LeakyReLU(f32),
}

/// Pooling types
#[derive(Debug, Clone, Copy)]
pub enum PoolType {
    /// Max pooling
    Max,
    /// Average pooling
    Average,
}

/// Inference statistics
#[derive(Debug, Clone, Default)]
pub struct InferenceStats {
    /// Total inferences
    pub total_inferences: u64,
    /// Total batches
    pub total_batches: u64,
    /// Average batch size
    pub avg_batch_size: f64,
    /// Total inference time (microseconds)
    pub total_time_us: u64,
    /// Average inference time per sample (microseconds)
    pub avg_time_per_sample_us: f64,
}

impl InferenceStats {
    /// Print statistics
    pub fn print(&self) {
        println!("\nML Inference Statistics:");
        println!("  Total inferences: {}", self.total_inferences);
        println!("  Total batches: {}", self.total_batches);
        println!("  Average batch size: {:.1}", self.avg_batch_size);
        println!("  Total time: {} ms", self.total_time_us / 1000);
        println!(
            "  Avg time per sample: {:.2} us",
            self.avg_time_per_sample_us
        );
    }
}
