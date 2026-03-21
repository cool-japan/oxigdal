//! GPU neural network layer operations: conv2d, activation, batch_norm, pool2d.

use super::{ActivationType, GpuMlInference, PoolType};
use crate::error::{GpuAdvancedError, Result};
use wgpu::util::DeviceExt;

impl GpuMlInference {
    /// Run convolutional layer on GPU
    #[allow(clippy::too_many_arguments)]
    pub async fn conv2d(
        &self,
        input: &[f32],
        weights: &[f32],
        bias: &[f32],
        input_channels: usize,
        output_channels: usize,
        kernel_size: usize,
        width: usize,
        height: usize,
    ) -> Result<Vec<f32>> {
        let output_width = width.saturating_sub(kernel_size - 1);
        let output_height = height.saturating_sub(kernel_size - 1);
        let output_size = output_channels * output_width * output_height;

        let shader = r#"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read> weights: array<f32>;
@group(0) @binding(2) var<storage, read> bias: array<f32>;
@group(0) @binding(3) var<storage, read_write> output: array<f32>;
@group(0) @binding(4) var<uniform> params: Params;

struct Params {
    input_channels: u32,
    output_channels: u32,
    kernel_size: u32,
    input_width: u32,
    input_height: u32,
    output_width: u32,
    output_height: u32,
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let out_x = global_id.x;
    let out_y = global_id.y;
    let out_c = global_id.z;

    if (out_x >= params.output_width || out_y >= params.output_height || out_c >= params.output_channels) {
        return;
    }

    var sum = 0.0;

    for (var in_c = 0u; in_c < params.input_channels; in_c = in_c + 1u) {
        for (var ky = 0u; ky < params.kernel_size; ky = ky + 1u) {
            for (var kx = 0u; kx < params.kernel_size; kx = kx + 1u) {
                let in_x = out_x + kx;
                let in_y = out_y + ky;

                let input_idx = in_c * params.input_height * params.input_width +
                                in_y * params.input_width + in_x;

                let weight_idx = out_c * params.input_channels * params.kernel_size * params.kernel_size +
                                 in_c * params.kernel_size * params.kernel_size +
                                 ky * params.kernel_size + kx;

                sum = sum + input[input_idx] * weights[weight_idx];
            }
        }
    }

    sum = sum + bias[out_c];

    let output_idx = out_c * params.output_height * params.output_width +
                     out_y * params.output_width + out_x;
    output[output_idx] = sum;
}
            "#.to_string();

        let input_buffer =
            self.context
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Conv2D Input Buffer"),
                    contents: bytemuck::cast_slice(input),
                    usage: wgpu::BufferUsages::STORAGE,
                });

        let weights_buffer =
            self.context
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Conv2D Weights Buffer"),
                    contents: bytemuck::cast_slice(weights),
                    usage: wgpu::BufferUsages::STORAGE,
                });

        let bias_buffer =
            self.context
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Conv2D Bias Buffer"),
                    contents: bytemuck::cast_slice(bias),
                    usage: wgpu::BufferUsages::STORAGE,
                });

        let output_buffer = self
            .context
            .device()
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("Conv2D Output Buffer"),
                size: (output_size * std::mem::size_of::<f32>()) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });

        #[repr(C)]
        #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
        struct ConvParams {
            input_channels: u32,
            output_channels: u32,
            kernel_size: u32,
            input_width: u32,
            input_height: u32,
            output_width: u32,
            output_height: u32,
            _padding: u32,
        }

        let params = ConvParams {
            input_channels: input_channels as u32,
            output_channels: output_channels as u32,
            kernel_size: kernel_size as u32,
            input_width: width as u32,
            input_height: height as u32,
            output_width: output_width as u32,
            output_height: output_height as u32,
            _padding: 0,
        };

        let params_buffer =
            self.context
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Conv2D Params Buffer"),
                    contents: bytemuck::bytes_of(&params),
                    usage: wgpu::BufferUsages::UNIFORM,
                });

        let shader_module =
            self.context
                .device()
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("Conv2D Shader"),
                    source: wgpu::ShaderSource::Wgsl(shader.into()),
                });

        let bind_group_layout = self.create_5_binding_layout("Conv2D")?;

        let bind_group = self
            .context
            .device()
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Conv2D Bind Group"),
                layout: &bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: input_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: weights_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: bias_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: output_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: params_buffer.as_entire_binding(),
                    },
                ],
            });

        let pipeline_layout =
            self.context
                .device()
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    immediate_size: 0,
                    label: Some("Conv2D Pipeline Layout"),
                    bind_group_layouts: &[Some(&bind_group_layout)],
                });

        let pipeline =
            self.context
                .device()
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("Conv2D Pipeline"),
                    layout: Some(&pipeline_layout),
                    module: &shader_module,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    cache: None,
                });

        let staging_buffer = self
            .context
            .device()
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("Conv2D Staging Buffer"),
                size: (output_size * std::mem::size_of::<f32>()) as u64,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

        let mut encoder =
            self.context
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Conv2D Encoder"),
                });

        {
            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Conv2D Compute Pass"),
                timestamp_writes: None,
            });

            compute_pass.set_pipeline(&pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);

            let workgroup_count_x = (output_width as u32).div_ceil(8);
            let workgroup_count_y = (output_height as u32).div_ceil(8);
            let workgroup_count_z = output_channels as u32;
            compute_pass.dispatch_workgroups(
                workgroup_count_x,
                workgroup_count_y,
                workgroup_count_z,
            );
        }

        encoder.copy_buffer_to_buffer(
            &output_buffer,
            0,
            &staging_buffer,
            0,
            (output_size * std::mem::size_of::<f32>()) as u64,
        );

        self.context.queue().submit(Some(encoder.finish()));

        self.read_staging_buffer(&staging_buffer).await
    }

    /// Apply activation function on GPU
    pub async fn activation(
        &self,
        input: &[f32],
        activation_type: ActivationType,
    ) -> Result<Vec<f32>> {
        let shader_source = match activation_type {
            ActivationType::ReLU => self.relu_shader().to_string(),
            ActivationType::Sigmoid => self.sigmoid_shader().to_string(),
            ActivationType::Tanh => self.tanh_shader().to_string(),
            ActivationType::LeakyReLU(alpha) => self.leaky_relu_shader(alpha),
        };

        let input_buffer =
            self.context
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Activation Input Buffer"),
                    contents: bytemuck::cast_slice(input),
                    usage: wgpu::BufferUsages::STORAGE,
                });

        let output_buffer = self
            .context
            .device()
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("Activation Output Buffer"),
                size: std::mem::size_of_val(input) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });

        let shader_module =
            self.context
                .device()
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("Activation Shader"),
                    source: wgpu::ShaderSource::Wgsl(shader_source.into()),
                });

        let bind_group_layout = self.create_2_binding_layout("Activation")?;

        let bind_group = self
            .context
            .device()
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Activation Bind Group"),
                layout: &bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: input_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: output_buffer.as_entire_binding(),
                    },
                ],
            });

        let pipeline_layout =
            self.context
                .device()
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    immediate_size: 0,
                    label: Some("Activation Pipeline Layout"),
                    bind_group_layouts: &[Some(&bind_group_layout)],
                });

        let pipeline =
            self.context
                .device()
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("Activation Pipeline"),
                    layout: Some(&pipeline_layout),
                    module: &shader_module,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    cache: None,
                });

        let staging_buffer = self
            .context
            .device()
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("Staging Buffer"),
                size: std::mem::size_of_val(input) as u64,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

        let mut encoder =
            self.context
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Activation Encoder"),
                });

        {
            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Activation Compute Pass"),
                timestamp_writes: None,
            });

            compute_pass.set_pipeline(&pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);

            let workgroup_count = (input.len() as u32).div_ceil(256);
            compute_pass.dispatch_workgroups(workgroup_count, 1, 1);
        }

        encoder.copy_buffer_to_buffer(
            &output_buffer,
            0,
            &staging_buffer,
            0,
            std::mem::size_of_val(input) as u64,
        );

        self.context.queue().submit(Some(encoder.finish()));

        self.read_staging_buffer(&staging_buffer).await
    }

    /// ReLU activation shader
    pub(super) fn relu_shader(&self) -> &str {
        r#"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(256, 1, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let idx = global_id.x;
    output[idx] = max(0.0, input[idx]);
}
        "#
    }

    /// Sigmoid activation shader
    pub(super) fn sigmoid_shader(&self) -> &str {
        r#"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(256, 1, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let idx = global_id.x;
    output[idx] = 1.0 / (1.0 + exp(-input[idx]));
}
        "#
    }

    /// Tanh activation shader
    pub(super) fn tanh_shader(&self) -> &str {
        r#"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(256, 1, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let idx = global_id.x;
    output[idx] = tanh(input[idx]);
}
        "#
    }

    /// Leaky ReLU activation shader
    pub(super) fn leaky_relu_shader(&self, alpha: f32) -> String {
        format!(
            r#"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(256, 1, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {{
    let idx = global_id.x;
    let x = input[idx];
    output[idx] = select({} * x, x, x > 0.0);
}}
        "#,
            alpha
        )
    }

    /// Batch normalization on GPU
    pub async fn batch_norm(
        &self,
        input: &[f32],
        mean: &[f32],
        variance: &[f32],
        gamma: &[f32],
        beta: &[f32],
        epsilon: f32,
    ) -> Result<Vec<f32>> {
        let shader = r#"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read> mean: array<f32>;
@group(0) @binding(2) var<storage, read> variance: array<f32>;
@group(0) @binding(3) var<storage, read> gamma: array<f32>;
@group(0) @binding(4) var<storage, read> beta: array<f32>;
@group(0) @binding(5) var<storage, read_write> output: array<f32>;
@group(0) @binding(6) var<uniform> params: Params;

struct Params {
    num_channels: u32,
    spatial_size: u32,
    epsilon: f32,
}

@compute @workgroup_size(256, 1, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let idx = global_id.x;
    if (idx >= params.num_channels * params.spatial_size) {
        return;
    }

    let channel = idx / params.spatial_size;

    let normalized = (input[idx] - mean[channel]) / sqrt(variance[channel] + params.epsilon);

    output[idx] = gamma[channel] * normalized + beta[channel];
}
            "#
        .to_string();

        let input_buffer =
            self.context
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("BatchNorm Input Buffer"),
                    contents: bytemuck::cast_slice(input),
                    usage: wgpu::BufferUsages::STORAGE,
                });

        let mean_buffer =
            self.context
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("BatchNorm Mean Buffer"),
                    contents: bytemuck::cast_slice(mean),
                    usage: wgpu::BufferUsages::STORAGE,
                });

        let variance_buffer =
            self.context
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("BatchNorm Variance Buffer"),
                    contents: bytemuck::cast_slice(variance),
                    usage: wgpu::BufferUsages::STORAGE,
                });

        let gamma_buffer =
            self.context
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("BatchNorm Gamma Buffer"),
                    contents: bytemuck::cast_slice(gamma),
                    usage: wgpu::BufferUsages::STORAGE,
                });

        let beta_buffer =
            self.context
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("BatchNorm Beta Buffer"),
                    contents: bytemuck::cast_slice(beta),
                    usage: wgpu::BufferUsages::STORAGE,
                });

        let output_buffer = self
            .context
            .device()
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("BatchNorm Output Buffer"),
                size: std::mem::size_of_val(input) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });

        #[repr(C)]
        #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
        struct BatchNormParams {
            num_channels: u32,
            spatial_size: u32,
            epsilon: f32,
            _padding: u32,
        }

        let spatial_size = input.len() / mean.len();
        let params = BatchNormParams {
            num_channels: mean.len() as u32,
            spatial_size: spatial_size as u32,
            epsilon,
            _padding: 0,
        };

        let params_buffer =
            self.context
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("BatchNorm Params Buffer"),
                    contents: bytemuck::bytes_of(&params),
                    usage: wgpu::BufferUsages::UNIFORM,
                });

        let shader_module =
            self.context
                .device()
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("BatchNorm Shader"),
                    source: wgpu::ShaderSource::Wgsl(shader.into()),
                });

        let bind_group_layout = self.create_batch_norm_layout()?;

        let bind_group = self
            .context
            .device()
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("BatchNorm Bind Group"),
                layout: &bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: input_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: mean_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: variance_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: gamma_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: beta_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: output_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: params_buffer.as_entire_binding(),
                    },
                ],
            });

        let pipeline_layout =
            self.context
                .device()
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    immediate_size: 0,
                    label: Some("BatchNorm Pipeline Layout"),
                    bind_group_layouts: &[Some(&bind_group_layout)],
                });

        let pipeline =
            self.context
                .device()
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("BatchNorm Pipeline"),
                    layout: Some(&pipeline_layout),
                    module: &shader_module,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    cache: None,
                });

        let staging_buffer = self
            .context
            .device()
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("BatchNorm Staging Buffer"),
                size: std::mem::size_of_val(input) as u64,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

        let mut encoder =
            self.context
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("BatchNorm Encoder"),
                });

        {
            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("BatchNorm Compute Pass"),
                timestamp_writes: None,
            });

            compute_pass.set_pipeline(&pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);

            let workgroup_count = (input.len() as u32).div_ceil(256);
            compute_pass.dispatch_workgroups(workgroup_count, 1, 1);
        }

        encoder.copy_buffer_to_buffer(
            &output_buffer,
            0,
            &staging_buffer,
            0,
            std::mem::size_of_val(input) as u64,
        );

        self.context.queue().submit(Some(encoder.finish()));

        self.read_staging_buffer(&staging_buffer).await
    }

    /// Pooling operation on GPU
    #[allow(clippy::too_many_arguments)]
    pub async fn pool2d(
        &self,
        input: &[f32],
        pool_type: PoolType,
        pool_size: usize,
        stride: usize,
        channels: usize,
        width: usize,
        height: usize,
    ) -> Result<Vec<f32>> {
        let output_width = (width - pool_size) / stride + 1;
        let output_height = (height - pool_size) / stride + 1;
        let output_size = channels * output_width * output_height;

        let pool_operation = match pool_type {
            PoolType::Max => {
                r#"
    var max_val = -3.402823e+38;
    for (var py = 0u; py < params.pool_size; py = py + 1u) {
        for (var px = 0u; px < params.pool_size; px = px + 1u) {
            let in_x = out_x * params.stride + px;
            let in_y = out_y * params.stride + py;
            let input_idx = c * params.input_height * params.input_width +
                            in_y * params.input_width + in_x;
            max_val = max(max_val, input[input_idx]);
        }
    }
    output[output_idx] = max_val;
                "#
            }
            PoolType::Average => {
                r#"
    var sum = 0.0;
    for (var py = 0u; py < params.pool_size; py = py + 1u) {
        for (var px = 0u; px < params.pool_size; px = px + 1u) {
            let in_x = out_x * params.stride + px;
            let in_y = out_y * params.stride + py;
            let input_idx = c * params.input_height * params.input_width +
                            in_y * params.input_width + in_x;
            sum = sum + input[input_idx];
        }
    }
    output[output_idx] = sum / f32(params.pool_size * params.pool_size);
                "#
            }
        };

        let shader = format!(
            r#"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;
@group(0) @binding(2) var<uniform> params: Params;

struct Params {{
    channels: u32,
    input_width: u32,
    input_height: u32,
    output_width: u32,
    output_height: u32,
    pool_size: u32,
    stride: u32,
}}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {{
    let out_x = global_id.x;
    let out_y = global_id.y;
    let c = global_id.z;

    if (out_x >= params.output_width || out_y >= params.output_height || c >= params.channels) {{
        return;
    }}

    let output_idx = c * params.output_height * params.output_width +
                     out_y * params.output_width + out_x;

{}
}}
            "#,
            pool_operation
        );

        let input_buffer =
            self.context
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Pool2D Input Buffer"),
                    contents: bytemuck::cast_slice(input),
                    usage: wgpu::BufferUsages::STORAGE,
                });

        let output_buffer = self
            .context
            .device()
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("Pool2D Output Buffer"),
                size: (output_size * std::mem::size_of::<f32>()) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });

        #[repr(C)]
        #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
        struct PoolParams {
            channels: u32,
            input_width: u32,
            input_height: u32,
            output_width: u32,
            output_height: u32,
            pool_size: u32,
            stride: u32,
            _padding: u32,
        }

        let params = PoolParams {
            channels: channels as u32,
            input_width: width as u32,
            input_height: height as u32,
            output_width: output_width as u32,
            output_height: output_height as u32,
            pool_size: pool_size as u32,
            stride: stride as u32,
            _padding: 0,
        };

        let params_buffer =
            self.context
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Pool2D Params Buffer"),
                    contents: bytemuck::bytes_of(&params),
                    usage: wgpu::BufferUsages::UNIFORM,
                });

        let shader_module =
            self.context
                .device()
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("Pool2D Shader"),
                    source: wgpu::ShaderSource::Wgsl(shader.into()),
                });

        let bind_group_layout = self.create_3_binding_layout("Pool2D")?;

        let bind_group = self
            .context
            .device()
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Pool2D Bind Group"),
                layout: &bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: input_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: output_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: params_buffer.as_entire_binding(),
                    },
                ],
            });

        let pipeline_layout =
            self.context
                .device()
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    immediate_size: 0,
                    label: Some("Pool2D Pipeline Layout"),
                    bind_group_layouts: &[Some(&bind_group_layout)],
                });

        let pipeline =
            self.context
                .device()
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("Pool2D Pipeline"),
                    layout: Some(&pipeline_layout),
                    module: &shader_module,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    cache: None,
                });

        let staging_buffer = self
            .context
            .device()
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("Pool2D Staging Buffer"),
                size: (output_size * std::mem::size_of::<f32>()) as u64,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

        let mut encoder =
            self.context
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Pool2D Encoder"),
                });

        {
            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Pool2D Compute Pass"),
                timestamp_writes: None,
            });

            compute_pass.set_pipeline(&pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);

            let workgroup_count_x = (output_width as u32).div_ceil(8);
            let workgroup_count_y = (output_height as u32).div_ceil(8);
            let workgroup_count_z = channels as u32;
            compute_pass.dispatch_workgroups(
                workgroup_count_x,
                workgroup_count_y,
                workgroup_count_z,
            );
        }

        encoder.copy_buffer_to_buffer(
            &output_buffer,
            0,
            &staging_buffer,
            0,
            (output_size * std::mem::size_of::<f32>()) as u64,
        );

        self.context.queue().submit(Some(encoder.finish()));

        self.read_staging_buffer(&staging_buffer).await
    }

    // ========================================================================
    // Helper methods for reducing bind group layout boilerplate
    // ========================================================================

    /// Reads results from a staging buffer back to CPU
    async fn read_staging_buffer(&self, staging_buffer: &wgpu::Buffer) -> Result<Vec<f32>> {
        let buffer_slice = staging_buffer.slice(..);
        let (sender, receiver) = futures::channel::oneshot::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).ok();
        });

        self.context.poll(true);
        receiver
            .await
            .map_err(|_| GpuAdvancedError::device_error("Failed to receive buffer mapping result"))?
            .map_err(|e| {
                GpuAdvancedError::device_error(format!("Buffer mapping failed: {:?}", e))
            })?;

        let data = buffer_slice.get_mapped_range();
        let result: Vec<f32> = bytemuck::cast_slice(&data).to_vec();

        drop(data);
        staging_buffer.unmap();

        Ok(result)
    }

    /// Creates a bind group layout with 2 bindings (input storage read, output storage read_write)
    fn create_2_binding_layout(&self, label: &str) -> Result<wgpu::BindGroupLayout> {
        Ok(self
            .context
            .device()
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(&format!("{} Bind Group Layout", label)),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            }))
    }

    /// Creates a bind group layout with 3 bindings (storage read, storage rw, uniform)
    fn create_3_binding_layout(&self, label: &str) -> Result<wgpu::BindGroupLayout> {
        Ok(self
            .context
            .device()
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(&format!("{} Bind Group Layout", label)),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            }))
    }

    /// Creates bind group layout for conv2d (3 read, 1 rw, 1 uniform)
    fn create_5_binding_layout(&self, label: &str) -> Result<wgpu::BindGroupLayout> {
        Ok(self
            .context
            .device()
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(&format!("{} Bind Group Layout", label)),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            }))
    }

    /// Creates bind group layout for batch norm (5 read, 1 rw, 1 uniform)
    fn create_batch_norm_layout(&self) -> Result<wgpu::BindGroupLayout> {
        Ok(self
            .context
            .device()
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("BatchNorm Bind Group Layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 5,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 6,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            }))
    }
}
