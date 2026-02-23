//! GPU compute operations: dense layer, matrix multiplication, softmax.

use super::{GpuLayer, GpuMlInference};
use crate::error::{GpuAdvancedError, Result};
use wgpu::util::DeviceExt;

impl GpuMlInference {
    /// Execute dense layer on GPU with batched input
    pub(super) async fn execute_dense_layer(
        &self,
        input: &[f32],
        layer: &GpuLayer,
        batch_size: usize,
        input_features: usize,
        output_features: usize,
    ) -> Result<Vec<f32>> {
        // Get weights and bias from layer
        let weights_buffer = layer.weights.as_ref().ok_or_else(|| {
            GpuAdvancedError::MlInferenceError("Dense layer missing weights".to_string())
        })?;
        let bias_buffer = layer.bias.as_ref().ok_or_else(|| {
            GpuAdvancedError::MlInferenceError("Dense layer missing bias".to_string())
        })?;

        let output_size = batch_size * output_features;

        let shader = r#"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read> weights: array<f32>;
@group(0) @binding(2) var<storage, read> bias: array<f32>;
@group(0) @binding(3) var<storage, read_write> output: array<f32>;
@group(0) @binding(4) var<uniform> params: Params;

struct Params {
    batch_size: u32,
    input_features: u32,
    output_features: u32,
    _padding: u32,
}

@compute @workgroup_size(64, 1, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let idx = global_id.x;
    let total_outputs = params.batch_size * params.output_features;

    if (idx >= total_outputs) {
        return;
    }

    let batch_idx = idx / params.output_features;
    let out_feature = idx % params.output_features;

    var sum = 0.0;
    for (var i = 0u; i < params.input_features; i = i + 1u) {
        let input_idx = batch_idx * params.input_features + i;
        let weight_idx = i * params.output_features + out_feature;
        sum = sum + input[input_idx] * weights[weight_idx];
    }

    sum = sum + bias[out_feature];
    output[idx] = sum;
}
        "#
        .to_string();

        let input_buffer =
            self.context
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Dense Input Buffer"),
                    contents: bytemuck::cast_slice(input),
                    usage: wgpu::BufferUsages::STORAGE,
                });

        let output_buffer = self
            .context
            .device()
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("Dense Output Buffer"),
                size: (output_size * std::mem::size_of::<f32>()) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });

        #[repr(C)]
        #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
        struct DenseParams {
            batch_size: u32,
            input_features: u32,
            output_features: u32,
            _padding: u32,
        }

        let params = DenseParams {
            batch_size: batch_size as u32,
            input_features: input_features as u32,
            output_features: output_features as u32,
            _padding: 0,
        };

        let params_buffer =
            self.context
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Dense Params Buffer"),
                    contents: bytemuck::bytes_of(&params),
                    usage: wgpu::BufferUsages::UNIFORM,
                });

        let shader_module =
            self.context
                .device()
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("Dense Shader"),
                    source: wgpu::ShaderSource::Wgsl(shader.into()),
                });

        let bind_group_layout =
            self.context
                .device()
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("Dense Bind Group Layout"),
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
                });

        let bind_group = self
            .context
            .device()
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Dense Bind Group"),
                layout: &bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: input_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: weights_buffer.buffer().as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: bias_buffer.buffer().as_entire_binding(),
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
                    label: Some("Dense Pipeline Layout"),
                    bind_group_layouts: &[&bind_group_layout],
                });

        let pipeline =
            self.context
                .device()
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("Dense Pipeline"),
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
                label: Some("Dense Staging Buffer"),
                size: (output_size * std::mem::size_of::<f32>()) as u64,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

        let mut encoder =
            self.context
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Dense Encoder"),
                });

        {
            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Dense Compute Pass"),
                timestamp_writes: None,
            });

            compute_pass.set_pipeline(&pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);

            let workgroup_count = (output_size as u32).div_ceil(64);
            compute_pass.dispatch_workgroups(workgroup_count, 1, 1);
        }

        encoder.copy_buffer_to_buffer(
            &output_buffer,
            0,
            &staging_buffer,
            0,
            (output_size * std::mem::size_of::<f32>()) as u64,
        );

        self.context.queue().submit(Some(encoder.finish()));

        let buffer_slice = staging_buffer.slice(..);
        let (sender, receiver) = futures::channel::oneshot::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
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

    /// Execute matrix multiplication on GPU (for external use)
    pub async fn matmul(
        &self,
        a: &[f32],
        b: &[f32],
        m: usize,
        k: usize,
        n: usize,
    ) -> Result<Vec<f32>> {
        let output_size = m * n;

        let shader = r#"
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> c: array<f32>;
@group(0) @binding(3) var<uniform> params: Params;

struct Params {
    m: u32,
    k: u32,
    n: u32,
    _padding: u32,
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let row = global_id.y;
    let col = global_id.x;

    if (row >= params.m || col >= params.n) {
        return;
    }

    var sum = 0.0;
    for (var i = 0u; i < params.k; i = i + 1u) {
        sum = sum + a[row * params.k + i] * b[i * params.n + col];
    }

    c[row * params.n + col] = sum;
}
        "#
        .to_string();

        let a_buffer =
            self.context
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("MatMul A Buffer"),
                    contents: bytemuck::cast_slice(a),
                    usage: wgpu::BufferUsages::STORAGE,
                });

        let b_buffer =
            self.context
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("MatMul B Buffer"),
                    contents: bytemuck::cast_slice(b),
                    usage: wgpu::BufferUsages::STORAGE,
                });

        let c_buffer = self
            .context
            .device()
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("MatMul C Buffer"),
                size: (output_size * std::mem::size_of::<f32>()) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });

        #[repr(C)]
        #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
        struct MatMulParams {
            m: u32,
            k: u32,
            n: u32,
            _padding: u32,
        }

        let params = MatMulParams {
            m: m as u32,
            k: k as u32,
            n: n as u32,
            _padding: 0,
        };

        let params_buffer =
            self.context
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("MatMul Params Buffer"),
                    contents: bytemuck::bytes_of(&params),
                    usage: wgpu::BufferUsages::UNIFORM,
                });

        let shader_module =
            self.context
                .device()
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("MatMul Shader"),
                    source: wgpu::ShaderSource::Wgsl(shader.into()),
                });

        let bind_group_layout =
            self.context
                .device()
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("MatMul Bind Group Layout"),
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
                                ty: wgpu::BufferBindingType::Storage { read_only: false },
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 3,
                            visibility: wgpu::ShaderStages::COMPUTE,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                    ],
                });

        let bind_group = self
            .context
            .device()
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("MatMul Bind Group"),
                layout: &bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: a_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: b_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: c_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: params_buffer.as_entire_binding(),
                    },
                ],
            });

        let pipeline_layout =
            self.context
                .device()
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    immediate_size: 0,
                    label: Some("MatMul Pipeline Layout"),
                    bind_group_layouts: &[&bind_group_layout],
                });

        let pipeline =
            self.context
                .device()
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("MatMul Pipeline"),
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
                label: Some("MatMul Staging Buffer"),
                size: (output_size * std::mem::size_of::<f32>()) as u64,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

        let mut encoder =
            self.context
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("MatMul Encoder"),
                });

        {
            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("MatMul Compute Pass"),
                timestamp_writes: None,
            });

            compute_pass.set_pipeline(&pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);

            let workgroup_count_x = (n as u32).div_ceil(8);
            let workgroup_count_y = (m as u32).div_ceil(8);
            compute_pass.dispatch_workgroups(workgroup_count_x, workgroup_count_y, 1);
        }

        encoder.copy_buffer_to_buffer(
            &c_buffer,
            0,
            &staging_buffer,
            0,
            (output_size * std::mem::size_of::<f32>()) as u64,
        );

        self.context.queue().submit(Some(encoder.finish()));

        let buffer_slice = staging_buffer.slice(..);
        let (sender, receiver) = futures::channel::oneshot::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
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

    /// Execute softmax on GPU
    pub async fn softmax(&self, input: &[f32], batch_size: usize) -> Result<Vec<f32>> {
        let features = input.len() / batch_size;

        let shader = r#"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;
@group(0) @binding(2) var<uniform> params: Params;

struct Params {
    batch_size: u32,
    features: u32,
}

@compute @workgroup_size(64, 1, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let batch_idx = global_id.x;

    if (batch_idx >= params.batch_size) {
        return;
    }

    let offset = batch_idx * params.features;

    var max_val = input[offset];
    for (var i = 1u; i < params.features; i = i + 1u) {
        max_val = max(max_val, input[offset + i]);
    }

    var sum = 0.0;
    for (var i = 0u; i < params.features; i = i + 1u) {
        let exp_val = exp(input[offset + i] - max_val);
        output[offset + i] = exp_val;
        sum = sum + exp_val;
    }

    for (var i = 0u; i < params.features; i = i + 1u) {
        output[offset + i] = output[offset + i] / sum;
    }
}
        "#
        .to_string();

        let input_buffer =
            self.context
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Softmax Input Buffer"),
                    contents: bytemuck::cast_slice(input),
                    usage: wgpu::BufferUsages::STORAGE,
                });

        let output_buffer = self
            .context
            .device()
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("Softmax Output Buffer"),
                size: std::mem::size_of_val(input) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });

        #[repr(C)]
        #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
        struct SoftmaxParams {
            batch_size: u32,
            features: u32,
        }

        let params = SoftmaxParams {
            batch_size: batch_size as u32,
            features: features as u32,
        };

        let params_buffer =
            self.context
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Softmax Params Buffer"),
                    contents: bytemuck::bytes_of(&params),
                    usage: wgpu::BufferUsages::UNIFORM,
                });

        let shader_module =
            self.context
                .device()
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("Softmax Shader"),
                    source: wgpu::ShaderSource::Wgsl(shader.into()),
                });

        let bind_group_layout =
            self.context
                .device()
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("Softmax Bind Group Layout"),
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
                });

        let bind_group = self
            .context
            .device()
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Softmax Bind Group"),
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
                    label: Some("Softmax Pipeline Layout"),
                    bind_group_layouts: &[&bind_group_layout],
                });

        let pipeline =
            self.context
                .device()
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("Softmax Pipeline"),
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
                label: Some("Softmax Staging Buffer"),
                size: std::mem::size_of_val(input) as u64,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

        let mut encoder =
            self.context
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Softmax Encoder"),
                });

        {
            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Softmax Compute Pass"),
                timestamp_writes: None,
            });

            compute_pass.set_pipeline(&pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);

            let workgroup_count = (batch_size as u32).div_ceil(64);
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

        let buffer_slice = staging_buffer.slice(..);
        let (sender, receiver) = futures::channel::oneshot::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
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
}
