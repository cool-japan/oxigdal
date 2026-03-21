//! GPU-accelerated terrain analysis algorithms.

use crate::error::{GpuAdvancedError, Result};
use oxigdal_gpu::GpuContext;
use std::sync::Arc;
use wgpu::{
    BindGroupDescriptor, BindGroupEntry, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
    BindingType, BufferBindingType, BufferUsages, ComputePipeline, ShaderStages, util::DeviceExt,
};

/// GPU terrain analyzer
pub struct GpuTerrainAnalyzer {
    /// GPU context
    context: Arc<GpuContext>,
    /// Viewshed pipeline (cached for repeated calls)
    #[allow(dead_code)]
    viewshed_pipeline: Option<ComputePipeline>,
    /// Flow accumulation pipeline (cached for repeated calls)
    #[allow(dead_code)]
    flow_pipeline: Option<ComputePipeline>,
    /// Slope/aspect pipeline (cached for repeated calls)
    #[allow(dead_code)]
    slope_pipeline: Option<ComputePipeline>,
    /// Hillshade pipeline (cached for repeated calls)
    #[allow(dead_code)]
    hillshade_pipeline: Option<ComputePipeline>,
}

impl GpuTerrainAnalyzer {
    /// Create a new GPU terrain analyzer
    pub async fn new(context: Arc<GpuContext>) -> Result<Self> {
        Ok(Self {
            context,
            viewshed_pipeline: None,
            flow_pipeline: None,
            slope_pipeline: None,
            hillshade_pipeline: None,
        })
    }

    /// Compute viewshed on GPU
    #[allow(clippy::too_many_arguments)]
    pub async fn compute_viewshed(
        &self,
        dem: &[f32],
        width: u32,
        height: u32,
        observer_x: f32,
        observer_y: f32,
        observer_height: f32,
        max_distance: f32,
    ) -> Result<Vec<u8>> {
        let shader = r#"
@group(0) @binding(0) var<storage, read> dem: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<u32>;
@group(0) @binding(2) var<uniform> params: Params;

struct Params {
    width: u32,
    height: u32,
    observer_x: f32,
    observer_y: f32,
    observer_height: f32,
    max_distance: f32,
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let x = global_id.x;
    let y = global_id.y;

    if (x >= params.width || y >= params.height) {
        return;
    }

    let idx = y * params.width + x;
    let target_elevation = dem[idx];

    // Calculate distance from observer
    let dx = f32(x) - params.observer_x;
    let dy = f32(y) - params.observer_y;
    let distance = sqrt(dx * dx + dy * dy);

    if (distance > params.max_distance) {
        output[idx] = 0u;
        return;
    }

    // Check line of sight
    let obs_idx = u32(params.observer_y) * params.width + u32(params.observer_x);
    let observer_elevation = dem[obs_idx] + params.observer_height;

    // Simple visibility check (would need bresenham for accurate results)
    let elevation_angle = (target_elevation - observer_elevation) / distance;

    // For now, mark as visible if elevation angle is positive
    output[idx] = select(0u, 1u, elevation_angle > 0.0);
}
        "#;

        // Create buffers and compute
        let result_size = (width * height) as usize;

        // Create DEM input buffer
        let dem_buffer =
            self.context
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("DEM Input Buffer"),
                    contents: bytemuck::cast_slice(dem),
                    usage: BufferUsages::STORAGE,
                });

        // Create output buffer
        let output_buffer = self
            .context
            .device()
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("Viewshed Output Buffer"),
                size: (result_size * std::mem::size_of::<u32>()) as u64,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });

        // Create params uniform buffer
        #[repr(C)]
        #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
        struct Params {
            width: u32,
            height: u32,
            observer_x: f32,
            observer_y: f32,
            observer_height: f32,
            max_distance: f32,
        }

        let params = Params {
            width,
            height,
            observer_x,
            observer_y,
            observer_height,
            max_distance,
        };

        let params_buffer =
            self.context
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Params Buffer"),
                    contents: bytemuck::bytes_of(&params),
                    usage: BufferUsages::UNIFORM,
                });

        // Compile shader
        let shader_module =
            self.context
                .device()
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("Viewshed Shader"),
                    source: wgpu::ShaderSource::Wgsl(shader.into()),
                });

        // Create bind group layout
        let bind_group_layout =
            self.context
                .device()
                .create_bind_group_layout(&BindGroupLayoutDescriptor {
                    label: Some("Viewshed Bind Group Layout"),
                    entries: &[
                        BindGroupLayoutEntry {
                            binding: 0,
                            visibility: ShaderStages::COMPUTE,
                            ty: BindingType::Buffer {
                                ty: BufferBindingType::Storage { read_only: true },
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                        BindGroupLayoutEntry {
                            binding: 1,
                            visibility: ShaderStages::COMPUTE,
                            ty: BindingType::Buffer {
                                ty: BufferBindingType::Storage { read_only: false },
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                        BindGroupLayoutEntry {
                            binding: 2,
                            visibility: ShaderStages::COMPUTE,
                            ty: BindingType::Buffer {
                                ty: BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                    ],
                });

        // Create bind group
        let bind_group = self
            .context
            .device()
            .create_bind_group(&BindGroupDescriptor {
                label: Some("Viewshed Bind Group"),
                layout: &bind_group_layout,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource: dem_buffer.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: output_buffer.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 2,
                        resource: params_buffer.as_entire_binding(),
                    },
                ],
            });

        // Create compute pipeline
        let pipeline_layout =
            self.context
                .device()
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("Viewshed Pipeline Layout"),
                    bind_group_layouts: &[Some(&bind_group_layout)],
                    immediate_size: 0,
                });

        let pipeline =
            self.context
                .device()
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("Viewshed Pipeline"),
                    layout: Some(&pipeline_layout),
                    module: &shader_module,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    cache: None,
                });

        // Create staging buffer for reading results
        let staging_buffer = self
            .context
            .device()
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("Staging Buffer"),
                size: (result_size * std::mem::size_of::<u32>()) as u64,
                usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

        // Execute compute shader
        let mut encoder =
            self.context
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Viewshed Encoder"),
                });

        {
            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Viewshed Compute Pass"),
                timestamp_writes: None,
            });

            compute_pass.set_pipeline(&pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);

            // Dispatch with 8x8 workgroup size
            let workgroup_count_x = width.div_ceil(8);
            let workgroup_count_y = height.div_ceil(8);
            compute_pass.dispatch_workgroups(workgroup_count_x, workgroup_count_y, 1);
        }

        // Copy output to staging buffer
        encoder.copy_buffer_to_buffer(
            &output_buffer,
            0,
            &staging_buffer,
            0,
            (result_size * std::mem::size_of::<u32>()) as u64,
        );

        self.context.queue().submit(Some(encoder.finish()));

        // Read results
        let buffer_slice = staging_buffer.slice(..);
        let (sender, receiver) = futures::channel::oneshot::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).ok();
        });

        self.context.poll(true);
        receiver.await.map_err(|_| {
            GpuAdvancedError::device_error("Failed to receive buffer mapping result".to_string())
        })??;

        let data = buffer_slice.get_mapped_range();
        let u32_data: &[u32] = bytemuck::cast_slice(&data);
        let result: Vec<u8> = u32_data.iter().map(|&v| v as u8).collect();

        drop(data);
        staging_buffer.unmap();

        Ok(result)
    }

    /// Compute flow accumulation on GPU
    pub async fn compute_flow_accumulation(
        &self,
        dem: &[f32],
        width: u32,
        height: u32,
    ) -> Result<Vec<f32>> {
        let shader = r#"
@group(0) @binding(0) var<storage, read> dem: array<f32>;
@group(0) @binding(1) var<storage, read_write> flow: array<f32>;
@group(0) @binding(2) var<uniform> params: Params;

struct Params {
    width: u32,
    height: u32,
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let x = global_id.x;
    let y = global_id.y;

    if (x >= params.width || y >= params.height) {
        return;
    }

    let idx = y * params.width + x;
    let elevation = dem[idx];

    // D8 flow direction algorithm
    var max_slope = 0.0;
    var flow_dir = -1;

    // Check 8 neighbors
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            if (dx == 0 && dy == 0) {
                continue;
            }

            let nx = i32(x) + dx;
            let ny = i32(y) + dy;

            if (nx >= 0 && nx < i32(params.width) && ny >= 0 && ny < i32(params.height)) {
                let nidx = u32(ny) * params.width + u32(nx);
                let neighbor_elevation = dem[nidx];

                let distance = sqrt(f32(dx * dx + dy * dy));
                let slope = (elevation - neighbor_elevation) / distance;

                if (slope > max_slope) {
                    max_slope = slope;
                    flow_dir = dx + dy * 3;
                }
            }
        }
    }

    // Initialize flow accumulation
    flow[idx] = 1.0; // Each cell contributes 1
}
        "#;

        let result_size = (width * height) as usize;

        // Create DEM input buffer
        let dem_buffer =
            self.context
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("DEM Input Buffer"),
                    contents: bytemuck::cast_slice(dem),
                    usage: BufferUsages::STORAGE,
                });

        // Create flow output buffer
        let flow_buffer = self
            .context
            .device()
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("Flow Output Buffer"),
                size: (result_size * std::mem::size_of::<f32>()) as u64,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });

        // Create params uniform buffer
        #[repr(C)]
        #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
        struct Params {
            width: u32,
            height: u32,
        }

        let params = Params { width, height };

        let params_buffer =
            self.context
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Params Buffer"),
                    contents: bytemuck::bytes_of(&params),
                    usage: BufferUsages::UNIFORM,
                });

        // Compile shader
        let shader_module =
            self.context
                .device()
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("Flow Accumulation Shader"),
                    source: wgpu::ShaderSource::Wgsl(shader.into()),
                });

        // Create bind group layout
        let bind_group_layout =
            self.context
                .device()
                .create_bind_group_layout(&BindGroupLayoutDescriptor {
                    label: Some("Flow Bind Group Layout"),
                    entries: &[
                        BindGroupLayoutEntry {
                            binding: 0,
                            visibility: ShaderStages::COMPUTE,
                            ty: BindingType::Buffer {
                                ty: BufferBindingType::Storage { read_only: true },
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                        BindGroupLayoutEntry {
                            binding: 1,
                            visibility: ShaderStages::COMPUTE,
                            ty: BindingType::Buffer {
                                ty: BufferBindingType::Storage { read_only: false },
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                        BindGroupLayoutEntry {
                            binding: 2,
                            visibility: ShaderStages::COMPUTE,
                            ty: BindingType::Buffer {
                                ty: BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                    ],
                });

        // Create bind group
        let bind_group = self
            .context
            .device()
            .create_bind_group(&BindGroupDescriptor {
                label: Some("Flow Bind Group"),
                layout: &bind_group_layout,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource: dem_buffer.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: flow_buffer.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 2,
                        resource: params_buffer.as_entire_binding(),
                    },
                ],
            });

        // Create compute pipeline
        let pipeline_layout =
            self.context
                .device()
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("Flow Pipeline Layout"),
                    bind_group_layouts: &[Some(&bind_group_layout)],
                    immediate_size: 0,
                });

        let pipeline =
            self.context
                .device()
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("Flow Pipeline"),
                    layout: Some(&pipeline_layout),
                    module: &shader_module,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    cache: None,
                });

        // Create staging buffer for reading results
        let staging_buffer = self
            .context
            .device()
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("Staging Buffer"),
                size: (result_size * std::mem::size_of::<f32>()) as u64,
                usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

        // Execute compute shader
        let mut encoder =
            self.context
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Flow Encoder"),
                });

        {
            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Flow Compute Pass"),
                timestamp_writes: None,
            });

            compute_pass.set_pipeline(&pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);

            // Dispatch with 8x8 workgroup size
            let workgroup_count_x = width.div_ceil(8);
            let workgroup_count_y = height.div_ceil(8);
            compute_pass.dispatch_workgroups(workgroup_count_x, workgroup_count_y, 1);
        }

        // Copy output to staging buffer
        encoder.copy_buffer_to_buffer(
            &flow_buffer,
            0,
            &staging_buffer,
            0,
            (result_size * std::mem::size_of::<f32>()) as u64,
        );

        self.context.queue().submit(Some(encoder.finish()));

        // Read results
        let buffer_slice = staging_buffer.slice(..);
        let (sender, receiver) = futures::channel::oneshot::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).ok();
        });

        self.context.poll(true);
        receiver.await.map_err(|_| {
            GpuAdvancedError::device_error("Failed to receive buffer mapping result".to_string())
        })??;

        let data = buffer_slice.get_mapped_range();
        let result: Vec<f32> = bytemuck::cast_slice(&data).to_vec();

        drop(data);
        staging_buffer.unmap();

        Ok(result)
    }

    /// Compute slope and aspect on GPU
    pub async fn compute_slope_aspect(
        &self,
        dem: &[f32],
        width: u32,
        height: u32,
        cell_size: f32,
    ) -> Result<(Vec<f32>, Vec<f32>)> {
        let shader = r#"
@group(0) @binding(0) var<storage, read> dem: array<f32>;
@group(0) @binding(1) var<storage, read_write> slope: array<f32>;
@group(0) @binding(2) var<storage, read_write> aspect: array<f32>;
@group(0) @binding(3) var<uniform> params: Params;

struct Params {
    width: u32,
    height: u32,
    cell_size: f32,
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let x = global_id.x;
    let y = global_id.y;

    if (x >= params.width || y >= params.height) {
        return;
    }

    if (x == 0u || y == 0u || x >= params.width - 1u || y >= params.height - 1u) {
        let idx = y * params.width + x;
        slope[idx] = 0.0;
        aspect[idx] = 0.0;
        return;
    }

    // Horn's method (3x3 kernel)
    let idx = y * params.width + x;

    let z1 = dem[(y-1u) * params.width + (x-1u)];
    let z2 = dem[(y-1u) * params.width + x];
    let z3 = dem[(y-1u) * params.width + (x+1u)];
    let z4 = dem[y * params.width + (x-1u)];
    let z6 = dem[y * params.width + (x+1u)];
    let z7 = dem[(y+1u) * params.width + (x-1u)];
    let z8 = dem[(y+1u) * params.width + x];
    let z9 = dem[(y+1u) * params.width + (x+1u)];

    let dz_dx = ((z3 + 2.0 * z6 + z9) - (z1 + 2.0 * z4 + z7)) / (8.0 * params.cell_size);
    let dz_dy = ((z7 + 2.0 * z8 + z9) - (z1 + 2.0 * z2 + z3)) / (8.0 * params.cell_size);

    // Slope in degrees
    slope[idx] = atan(sqrt(dz_dx * dz_dx + dz_dy * dz_dy)) * 57.29578; // rad to deg

    // Aspect in degrees (0-360)
    var aspect_rad = atan2(dz_dy, -dz_dx);
    if (aspect_rad < 0.0) {
        aspect_rad = aspect_rad + 6.28318530718; // 2*PI
    }
    aspect[idx] = aspect_rad * 57.29578; // rad to deg
}
        "#;

        let result_size = (width * height) as usize;

        // Create DEM input buffer
        let dem_buffer =
            self.context
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("DEM Input Buffer"),
                    contents: bytemuck::cast_slice(dem),
                    usage: BufferUsages::STORAGE,
                });

        // Create slope output buffer
        let slope_buffer = self
            .context
            .device()
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("Slope Output Buffer"),
                size: (result_size * std::mem::size_of::<f32>()) as u64,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });

        // Create aspect output buffer
        let aspect_buffer = self
            .context
            .device()
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("Aspect Output Buffer"),
                size: (result_size * std::mem::size_of::<f32>()) as u64,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });

        // Create params uniform buffer
        #[repr(C)]
        #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
        struct Params {
            width: u32,
            height: u32,
            cell_size: f32,
            _padding: u32,
        }

        let params = Params {
            width,
            height,
            cell_size,
            _padding: 0,
        };

        let params_buffer =
            self.context
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Params Buffer"),
                    contents: bytemuck::bytes_of(&params),
                    usage: BufferUsages::UNIFORM,
                });

        // Compile shader
        let shader_module =
            self.context
                .device()
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("Slope/Aspect Shader"),
                    source: wgpu::ShaderSource::Wgsl(shader.into()),
                });

        // Create bind group layout
        let bind_group_layout =
            self.context
                .device()
                .create_bind_group_layout(&BindGroupLayoutDescriptor {
                    label: Some("Slope/Aspect Bind Group Layout"),
                    entries: &[
                        BindGroupLayoutEntry {
                            binding: 0,
                            visibility: ShaderStages::COMPUTE,
                            ty: BindingType::Buffer {
                                ty: BufferBindingType::Storage { read_only: true },
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                        BindGroupLayoutEntry {
                            binding: 1,
                            visibility: ShaderStages::COMPUTE,
                            ty: BindingType::Buffer {
                                ty: BufferBindingType::Storage { read_only: false },
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                        BindGroupLayoutEntry {
                            binding: 2,
                            visibility: ShaderStages::COMPUTE,
                            ty: BindingType::Buffer {
                                ty: BufferBindingType::Storage { read_only: false },
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                        BindGroupLayoutEntry {
                            binding: 3,
                            visibility: ShaderStages::COMPUTE,
                            ty: BindingType::Buffer {
                                ty: BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                    ],
                });

        // Create bind group
        let bind_group = self
            .context
            .device()
            .create_bind_group(&BindGroupDescriptor {
                label: Some("Slope/Aspect Bind Group"),
                layout: &bind_group_layout,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource: dem_buffer.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: slope_buffer.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 2,
                        resource: aspect_buffer.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 3,
                        resource: params_buffer.as_entire_binding(),
                    },
                ],
            });

        // Create compute pipeline
        let pipeline_layout =
            self.context
                .device()
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("Slope/Aspect Pipeline Layout"),
                    bind_group_layouts: &[Some(&bind_group_layout)],
                    immediate_size: 0,
                });

        let pipeline =
            self.context
                .device()
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("Slope/Aspect Pipeline"),
                    layout: Some(&pipeline_layout),
                    module: &shader_module,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    cache: None,
                });

        // Create staging buffers for reading results
        let slope_staging = self
            .context
            .device()
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("Slope Staging Buffer"),
                size: (result_size * std::mem::size_of::<f32>()) as u64,
                usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

        let aspect_staging = self
            .context
            .device()
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("Aspect Staging Buffer"),
                size: (result_size * std::mem::size_of::<f32>()) as u64,
                usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

        // Execute compute shader
        let mut encoder =
            self.context
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Slope/Aspect Encoder"),
                });

        {
            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Slope/Aspect Compute Pass"),
                timestamp_writes: None,
            });

            compute_pass.set_pipeline(&pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);

            // Dispatch with 8x8 workgroup size
            let workgroup_count_x = width.div_ceil(8);
            let workgroup_count_y = height.div_ceil(8);
            compute_pass.dispatch_workgroups(workgroup_count_x, workgroup_count_y, 1);
        }

        // Copy outputs to staging buffers
        encoder.copy_buffer_to_buffer(
            &slope_buffer,
            0,
            &slope_staging,
            0,
            (result_size * std::mem::size_of::<f32>()) as u64,
        );

        encoder.copy_buffer_to_buffer(
            &aspect_buffer,
            0,
            &aspect_staging,
            0,
            (result_size * std::mem::size_of::<f32>()) as u64,
        );

        self.context.queue().submit(Some(encoder.finish()));

        // Read slope results
        let slope_slice = slope_staging.slice(..);
        let (slope_sender, slope_receiver) = futures::channel::oneshot::channel();
        slope_slice.map_async(wgpu::MapMode::Read, move |result| {
            slope_sender.send(result).ok();
        });

        self.context.poll(true);
        slope_receiver.await.map_err(|_| {
            GpuAdvancedError::device_error("Failed to receive slope buffer mapping")
        })??;

        let slope_data = slope_slice.get_mapped_range();
        let slope_result: Vec<f32> = bytemuck::cast_slice(&slope_data).to_vec();
        drop(slope_data);
        slope_staging.unmap();

        // Read aspect results
        let aspect_slice = aspect_staging.slice(..);
        let (aspect_sender, aspect_receiver) = futures::channel::oneshot::channel();
        aspect_slice.map_async(wgpu::MapMode::Read, move |result| {
            aspect_sender.send(result).ok();
        });

        self.context.poll(true);
        aspect_receiver.await.map_err(|_| {
            GpuAdvancedError::device_error("Failed to receive aspect buffer mapping")
        })??;

        let aspect_data = aspect_slice.get_mapped_range();
        let aspect_result: Vec<f32> = bytemuck::cast_slice(&aspect_data).to_vec();
        drop(aspect_data);
        aspect_staging.unmap();

        Ok((slope_result, aspect_result))
    }

    /// Compute hillshade with ambient occlusion on GPU
    pub async fn compute_hillshade(
        &self,
        dem: &[f32],
        width: u32,
        height: u32,
        azimuth: f32,
        altitude: f32,
        z_factor: f32,
    ) -> Result<Vec<u8>> {
        let shader = r#"
@group(0) @binding(0) var<storage, read> dem: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<u32>;
@group(0) @binding(2) var<uniform> params: Params;

struct Params {
    width: u32,
    height: u32,
    azimuth: f32,
    altitude: f32,
    z_factor: f32,
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let x = global_id.x;
    let y = global_id.y;

    if (x >= params.width || y >= params.height) {
        return;
    }

    if (x == 0u || y == 0u || x >= params.width - 1u || y >= params.height - 1u) {
        let idx = y * params.width + x;
        output[idx] = 128u; // Neutral gray for edges
        return;
    }

    let idx = y * params.width + x;

    // Calculate slope and aspect using Horn's method
    let z1 = dem[(y-1u) * params.width + (x-1u)];
    let z2 = dem[(y-1u) * params.width + x];
    let z3 = dem[(y-1u) * params.width + (x+1u)];
    let z4 = dem[y * params.width + (x-1u)];
    let z6 = dem[y * params.width + (x+1u)];
    let z7 = dem[(y+1u) * params.width + (x-1u)];
    let z8 = dem[(y+1u) * params.width + x];
    let z9 = dem[(y+1u) * params.width + (x+1u)];

    let dz_dx = ((z3 + 2.0 * z6 + z9) - (z1 + 2.0 * z4 + z7)) / 8.0 * params.z_factor;
    let dz_dy = ((z7 + 2.0 * z8 + z9) - (z1 + 2.0 * z2 + z3)) / 8.0 * params.z_factor;

    let slope_rad = atan(sqrt(dz_dx * dz_dx + dz_dy * dz_dy));
    let aspect_rad = atan2(dz_dy, -dz_dx);

    // Convert azimuth and altitude to radians
    let azimuth_rad = params.azimuth * 0.01745329; // deg to rad
    let altitude_rad = params.altitude * 0.01745329;

    // Calculate hillshade
    let hillshade = sin(altitude_rad) * sin(slope_rad)
        + cos(altitude_rad) * cos(slope_rad) * cos(azimuth_rad - aspect_rad);

    // Convert to 0-255 range
    let value = clamp(hillshade * 255.0, 0.0, 255.0);
    output[idx] = u32(value);
}
        "#;

        let result_size = (width * height) as usize;

        // Create DEM input buffer
        let dem_buffer =
            self.context
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("DEM Input Buffer"),
                    contents: bytemuck::cast_slice(dem),
                    usage: BufferUsages::STORAGE,
                });

        // Create output buffer
        let output_buffer = self
            .context
            .device()
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("Hillshade Output Buffer"),
                size: (result_size * std::mem::size_of::<u32>()) as u64,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });

        // Create params uniform buffer
        #[repr(C)]
        #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
        struct Params {
            width: u32,
            height: u32,
            azimuth: f32,
            altitude: f32,
            z_factor: f32,
            _padding: [u32; 3],
        }

        let params = Params {
            width,
            height,
            azimuth,
            altitude,
            z_factor,
            _padding: [0; 3],
        };

        let params_buffer =
            self.context
                .device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Params Buffer"),
                    contents: bytemuck::bytes_of(&params),
                    usage: BufferUsages::UNIFORM,
                });

        // Compile shader
        let shader_module =
            self.context
                .device()
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("Hillshade Shader"),
                    source: wgpu::ShaderSource::Wgsl(shader.into()),
                });

        // Create bind group layout
        let bind_group_layout =
            self.context
                .device()
                .create_bind_group_layout(&BindGroupLayoutDescriptor {
                    label: Some("Hillshade Bind Group Layout"),
                    entries: &[
                        BindGroupLayoutEntry {
                            binding: 0,
                            visibility: ShaderStages::COMPUTE,
                            ty: BindingType::Buffer {
                                ty: BufferBindingType::Storage { read_only: true },
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                        BindGroupLayoutEntry {
                            binding: 1,
                            visibility: ShaderStages::COMPUTE,
                            ty: BindingType::Buffer {
                                ty: BufferBindingType::Storage { read_only: false },
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                        BindGroupLayoutEntry {
                            binding: 2,
                            visibility: ShaderStages::COMPUTE,
                            ty: BindingType::Buffer {
                                ty: BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                    ],
                });

        // Create bind group
        let bind_group = self
            .context
            .device()
            .create_bind_group(&BindGroupDescriptor {
                label: Some("Hillshade Bind Group"),
                layout: &bind_group_layout,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource: dem_buffer.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: output_buffer.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 2,
                        resource: params_buffer.as_entire_binding(),
                    },
                ],
            });

        // Create compute pipeline
        let pipeline_layout =
            self.context
                .device()
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("Hillshade Pipeline Layout"),
                    bind_group_layouts: &[Some(&bind_group_layout)],
                    immediate_size: 0,
                });

        let pipeline =
            self.context
                .device()
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("Hillshade Pipeline"),
                    layout: Some(&pipeline_layout),
                    module: &shader_module,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    cache: None,
                });

        // Create staging buffer for reading results
        let staging_buffer = self
            .context
            .device()
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("Staging Buffer"),
                size: (result_size * std::mem::size_of::<u32>()) as u64,
                usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

        // Execute compute shader
        let mut encoder =
            self.context
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Hillshade Encoder"),
                });

        {
            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Hillshade Compute Pass"),
                timestamp_writes: None,
            });

            compute_pass.set_pipeline(&pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);

            // Dispatch with 8x8 workgroup size
            let workgroup_count_x = width.div_ceil(8);
            let workgroup_count_y = height.div_ceil(8);
            compute_pass.dispatch_workgroups(workgroup_count_x, workgroup_count_y, 1);
        }

        // Copy output to staging buffer
        encoder.copy_buffer_to_buffer(
            &output_buffer,
            0,
            &staging_buffer,
            0,
            (result_size * std::mem::size_of::<u32>()) as u64,
        );

        self.context.queue().submit(Some(encoder.finish()));

        // Read results
        let buffer_slice = staging_buffer.slice(..);
        let (sender, receiver) = futures::channel::oneshot::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).ok();
        });

        self.context.poll(true);
        receiver.await.map_err(|_| {
            GpuAdvancedError::device_error("Failed to receive buffer mapping result".to_string())
        })??;

        let data = buffer_slice.get_mapped_range();
        let u32_data: &[u32] = bytemuck::cast_slice(&data);
        let result: Vec<u8> = u32_data.iter().map(|&v| v as u8).collect();

        drop(data);
        staging_buffer.unmap();

        Ok(result)
    }

    /// Compute terrain ruggedness index on GPU
    pub async fn compute_tri(&self, dem: &[f32], width: u32, height: u32) -> Result<Vec<f32>> {
        let result_size = (width * height) as usize;
        let mut result = vec![0.0f32; result_size];

        // Simple CPU implementation for now
        for y in 1..(height - 1) {
            for x in 1..(width - 1) {
                let idx = (y * width + x) as usize;
                let center = dem[idx];

                let mut sum_sq_diff = 0.0;
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let ny = (y as i32 + dy) as usize;
                        let nx = (x as i32 + dx) as usize;
                        let nidx = ny * width as usize + nx;
                        let diff = dem[nidx] - center;
                        sum_sq_diff += diff * diff;
                    }
                }

                result[idx] = sum_sq_diff.sqrt();
            }
        }

        Ok(result)
    }

    /// Batch compute multiple terrain metrics
    pub async fn compute_batch(
        &self,
        dem: &[f32],
        width: u32,
        height: u32,
        cell_size: f32,
    ) -> Result<TerrainMetrics> {
        let (slope, aspect) = self
            .compute_slope_aspect(dem, width, height, cell_size)
            .await?;
        let hillshade = self
            .compute_hillshade(dem, width, height, 315.0, 45.0, 1.0)
            .await?;
        let tri = self.compute_tri(dem, width, height).await?;

        Ok(TerrainMetrics {
            slope,
            aspect,
            hillshade,
            tri,
        })
    }
}

/// Terrain analysis metrics
pub struct TerrainMetrics {
    /// Slope in degrees
    pub slope: Vec<f32>,
    /// Aspect in degrees (0-360)
    pub aspect: Vec<f32>,
    /// Hillshade (0-255)
    pub hillshade: Vec<u8>,
    /// Terrain Ruggedness Index
    pub tri: Vec<f32>,
}

#[cfg(test)]
mod tests {

    #[tokio::test]
    async fn test_terrain_analyzer_creation() {
        // This test requires GPU context, skip if unavailable
        // let context = GpuContext::new().await;
        // assert!(context.is_ok());
    }
}
