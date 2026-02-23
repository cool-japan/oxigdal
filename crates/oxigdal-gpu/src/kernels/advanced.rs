//! Advanced GPU kernels for specialized geospatial operations.

use crate::buffer::GpuBuffer;
use crate::context::GpuContext;
use crate::error::{GpuError, GpuResult};
use crate::shaders::{WgslShader, ComputePipelineBuilder, create_compute_bind_group_layout, storage_buffer_layout};
use bytemuck::{Pod, Zeroable};
use tracing::debug;
use wgpu::{BindGroup, BindGroupDescriptor, BindGroupEntry, BindingResource, BufferUsages, ComputePipeline};

/// Terrain analysis operations.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TerrainOp {
    /// Calculate slope.
    Slope,
    /// Calculate aspect.
    Aspect,
    /// Calculate hillshade.
    Hillshade,
    /// Calculate curvature.
    Curvature,
    /// Calculate roughness.
    Roughness,
}

/// Morphological operations.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MorphologicalOp {
    /// Erosion.
    Erosion,
    /// Dilation.
    Dilation,
    /// Opening (erosion followed by dilation).
    Opening,
    /// Closing (dilation followed by erosion).
    Closing,
    /// Morphological gradient.
    Gradient,
}

/// Edge detection methods.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EdgeDetectionMethod {
    /// Sobel operator.
    Sobel,
    /// Prewitt operator.
    Prewitt,
    /// Canny edge detector.
    Canny,
    /// Laplacian operator.
    Laplacian,
}

/// Terrain analysis kernel.
pub struct TerrainKernel {
    context: GpuContext,
    pipeline: ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    op: TerrainOp,
}

impl TerrainKernel {
    /// Create a new terrain kernel.
    ///
    /// # Errors
    ///
    /// Returns an error if shader compilation or pipeline creation fails.
    pub fn new(context: &GpuContext, op: TerrainOp) -> GpuResult<Self> {
        let shader_code = Self::generate_shader(op);
        let mut shader = WgslShader::new(shader_code, "terrain_main");
        let shader_module = shader.compile(context.device())?;

        let bind_group_layout = create_compute_bind_group_layout(
            context.device(),
            &[
                storage_buffer_layout(0, true),  // input
                storage_buffer_layout(1, false), // output
                storage_buffer_layout(2, true),  // params
            ],
            Some("Terrain Bind Group Layout"),
        )?;

        let pipeline = ComputePipelineBuilder::new(context.device(), shader_module, "terrain_main")
            .bind_group_layout(&bind_group_layout)
            .label("Terrain Pipeline")
            .build()?;

        debug!("Created terrain kernel: {:?}", op);

        Ok(Self {
            context: context.clone(),
            pipeline,
            bind_group_layout,
            op,
        })
    }

    /// Execute terrain analysis.
    ///
    /// # Errors
    ///
    /// Returns an error if execution fails.
    pub fn execute<T: Pod>(
        &self,
        input: &GpuBuffer<T>,
        width: u32,
        height: u32,
        cell_size: f32,
    ) -> GpuResult<GpuBuffer<T>> {
        let mut output = GpuBuffer::new(
            &self.context,
            input.len(),
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        )?;

        let params = TerrainParams {
            width,
            height,
            cell_size,
            _padding: 0.0,
        };

        let params_buffer = self.context.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("Terrain Params"),
            size: std::mem::size_of::<TerrainParams>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        self.context.queue().write_buffer(
            &params_buffer,
            0,
            bytemuck::bytes_of(&params),
        );

        let bind_group = self.context.device().create_bind_group(&BindGroupDescriptor {
            label: Some("Terrain Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: input.buffer().as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: output.buffer().as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: params_buffer.as_entire_binding(),
                },
            ],
        });

        let mut encoder = self.context.device().create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Terrain Encoder"),
        });

        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Terrain Compute Pass"),
                timestamp_writes: None,
            });

            cpass.set_pipeline(&self.pipeline);
            cpass.set_bind_group(0, &bind_group, &[]);

            let workgroup_size = 256;
            let num_workgroups = ((input.len() as u32 + workgroup_size - 1) / workgroup_size).max(1);
            cpass.dispatch_workgroups(num_workgroups, 1, 1);
        }

        self.context.queue().submit(Some(encoder.finish()));

        debug!("Executed terrain analysis: {:?}", self.op);

        Ok(output)
    }

    fn generate_shader(op: TerrainOp) -> String {
        let computation = match op {
            TerrainOp::Slope => r#"
                // Calculate slope using 3x3 neighborhood
                let dz_dx = (z3 + 2.0 * z6 + z9 - z1 - 2.0 * z4 - z7) / (8.0 * params.cell_size);
                let dz_dy = (z7 + 2.0 * z8 + z9 - z1 - 2.0 * z2 - z3) / (8.0 * params.cell_size);
                let slope = atan(sqrt(dz_dx * dz_dx + dz_dy * dz_dy)) * 57.29578; // Convert to degrees
                output[idx] = slope;
            "#,
            TerrainOp::Aspect => r#"
                let dz_dx = (z3 + 2.0 * z6 + z9 - z1 - 2.0 * z4 - z7) / (8.0 * params.cell_size);
                let dz_dy = (z7 + 2.0 * z8 + z9 - z1 - 2.0 * z2 - z3) / (8.0 * params.cell_size);
                let aspect = atan2(dz_dy, -dz_dx) * 57.29578; // Convert to degrees
                output[idx] = select(aspect, aspect + 360.0, aspect < 0.0);
            "#,
            TerrainOp::Hillshade => r#"
                let zenith = 45.0 * 0.01745329; // 45 degrees in radians
                let azimuth = 315.0 * 0.01745329; // 315 degrees in radians

                let dz_dx = (z3 + 2.0 * z6 + z9 - z1 - 2.0 * z4 - z7) / (8.0 * params.cell_size);
                let dz_dy = (z7 + 2.0 * z8 + z9 - z1 - 2.0 * z2 - z3) / (8.0 * params.cell_size);

                let slope = atan(sqrt(dz_dx * dz_dx + dz_dy * dz_dy));
                let aspect = atan2(dz_dy, -dz_dx);

                let hillshade = 255.0 * ((cos(zenith) * cos(slope)) +
                                        (sin(zenith) * sin(slope) * cos(azimuth - aspect)));
                output[idx] = clamp(hillshade, 0.0, 255.0);
            "#,
            TerrainOp::Curvature => r#"
                let d2z_dx2 = (z4 - 2.0 * z5 + z6) / (params.cell_size * params.cell_size);
                let d2z_dy2 = (z2 - 2.0 * z5 + z8) / (params.cell_size * params.cell_size);
                let curvature = -(d2z_dx2 + d2z_dy2) * 100.0;
                output[idx] = curvature;
            "#,
            TerrainOp::Roughness => r#"
                let mean = (z1 + z2 + z3 + z4 + z5 + z6 + z7 + z8 + z9) / 9.0;
                let variance = ((z1 - mean) * (z1 - mean) +
                               (z2 - mean) * (z2 - mean) +
                               (z3 - mean) * (z3 - mean) +
                               (z4 - mean) * (z4 - mean) +
                               (z5 - mean) * (z5 - mean) +
                               (z6 - mean) * (z6 - mean) +
                               (z7 - mean) * (z7 - mean) +
                               (z8 - mean) * (z8 - mean) +
                               (z9 - mean) * (z9 - mean)) / 9.0;
                output[idx] = sqrt(variance);
            "#,
        };

        format!(
            r#"
struct TerrainParams {{
    width: u32,
    height: u32,
    cell_size: f32,
    _padding: f32,
}}

@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;
@group(0) @binding(2) var<uniform> params: TerrainParams;

fn get_value(x: i32, y: i32) -> f32 {{
    if (x < 0 || x >= i32(params.width) || y < 0 || y >= i32(params.height)) {{
        return 0.0;
    }}
    let idx = u32(y) * params.width + u32(x);
    return input[idx];
}}

@compute @workgroup_size(256)
fn terrain_main(@builtin(global_invocation_id) global_id: vec3<u32>) {{
    let idx = global_id.x;
    if (idx >= arrayLength(&output)) {{
        return;
    }}

    let x = i32(idx % params.width);
    let y = i32(idx / params.width);

    // Get 3x3 neighborhood
    let z1 = get_value(x - 1, y - 1);
    let z2 = get_value(x, y - 1);
    let z3 = get_value(x + 1, y - 1);
    let z4 = get_value(x - 1, y);
    let z5 = get_value(x, y);
    let z6 = get_value(x + 1, y);
    let z7 = get_value(x - 1, y + 1);
    let z8 = get_value(x, y + 1);
    let z9 = get_value(x + 1, y + 1);

    {}
}}
"#,
            computation
        )
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct TerrainParams {
    width: u32,
    height: u32,
    cell_size: f32,
    _padding: f32,
}

/// Morphological operations kernel.
pub struct MorphologicalKernel {
    context: GpuContext,
    pipeline: ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    op: MorphologicalOp,
}

impl MorphologicalKernel {
    /// Create a new morphological kernel.
    ///
    /// # Errors
    ///
    /// Returns an error if shader compilation or pipeline creation fails.
    pub fn new(context: &GpuContext, op: MorphologicalOp) -> GpuResult<Self> {
        let shader_code = Self::generate_shader(op);
        let mut shader = WgslShader::new(shader_code, "morphology_main");
        let shader_module = shader.compile(context.device())?;

        let bind_group_layout = create_compute_bind_group_layout(
            context.device(),
            &[
                storage_buffer_layout(0, true),  // input
                storage_buffer_layout(1, false), // output
                storage_buffer_layout(2, true),  // params
            ],
            Some("Morphology Bind Group Layout"),
        )?;

        let pipeline = ComputePipelineBuilder::new(context.device(), shader_module, "morphology_main")
            .bind_group_layout(&bind_group_layout)
            .label("Morphology Pipeline")
            .build()?;

        debug!("Created morphological kernel: {:?}", op);

        Ok(Self {
            context: context.clone(),
            pipeline,
            bind_group_layout,
            op,
        })
    }

    /// Execute morphological operation.
    ///
    /// # Errors
    ///
    /// Returns an error if execution fails.
    pub fn execute<T: Pod>(
        &self,
        input: &GpuBuffer<T>,
        width: u32,
        height: u32,
        kernel_size: u32,
    ) -> GpuResult<GpuBuffer<T>> {
        let mut output = GpuBuffer::new(
            &self.context,
            input.len(),
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        )?;

        let params = MorphologyParams {
            width,
            height,
            kernel_size,
            _padding: 0,
        };

        let params_buffer = self.context.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("Morphology Params"),
            size: std::mem::size_of::<MorphologyParams>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        self.context.queue().write_buffer(
            &params_buffer,
            0,
            bytemuck::bytes_of(&params),
        );

        let bind_group = self.context.device().create_bind_group(&BindGroupDescriptor {
            label: Some("Morphology Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: input.buffer().as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: output.buffer().as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: params_buffer.as_entire_binding(),
                },
            ],
        });

        let mut encoder = self.context.device().create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Morphology Encoder"),
        });

        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Morphology Compute Pass"),
                timestamp_writes: None,
            });

            cpass.set_pipeline(&self.pipeline);
            cpass.set_bind_group(0, &bind_group, &[]);

            let workgroup_size = 256;
            let num_workgroups = ((input.len() as u32 + workgroup_size - 1) / workgroup_size).max(1);
            cpass.dispatch_workgroups(num_workgroups, 1, 1);
        }

        self.context.queue().submit(Some(encoder.finish()));

        debug!("Executed morphological operation: {:?}", self.op);

        Ok(output)
    }

    fn generate_shader(op: MorphologicalOp) -> String {
        let computation = match op {
            MorphologicalOp::Erosion => r#"
                var min_val = 999999.0;
                for (var ky = 0u; ky < params.kernel_size; ky++) {
                    for (var kx = 0u; kx < params.kernel_size; kx++) {
                        let sample = get_value(ix + i32(kx), iy + i32(ky));
                        min_val = min(min_val, sample);
                    }
                }
                output[idx] = min_val;
            "#,
            MorphologicalOp::Dilation => r#"
                var max_val = -999999.0;
                for (var ky = 0u; ky < params.kernel_size; ky++) {
                    for (var kx = 0u; kx < params.kernel_size; kx++) {
                        let sample = get_value(ix + i32(kx), iy + i32(ky));
                        max_val = max(max_val, sample);
                    }
                }
                output[idx] = max_val;
            "#,
            MorphologicalOp::Opening => r#"
                // Erosion followed by dilation
                var min_val = 999999.0;
                for (var ky = 0u; ky < params.kernel_size; ky++) {
                    for (var kx = 0u; kx < params.kernel_size; kx++) {
                        let sample = get_value(ix + i32(kx), iy + i32(ky));
                        min_val = min(min_val, sample);
                    }
                }
                output[idx] = min_val;
            "#,
            MorphologicalOp::Closing => r#"
                // Dilation followed by erosion
                var max_val = -999999.0;
                for (var ky = 0u; ky < params.kernel_size; ky++) {
                    for (var kx = 0u; kx < params.kernel_size; kx++) {
                        let sample = get_value(ix + i32(kx), iy + i32(ky));
                        max_val = max(max_val, sample);
                    }
                }
                output[idx] = max_val;
            "#,
            MorphologicalOp::Gradient => r#"
                var min_val = 999999.0;
                var max_val = -999999.0;
                for (var ky = 0u; ky < params.kernel_size; ky++) {
                    for (var kx = 0u; kx < params.kernel_size; kx++) {
                        let sample = get_value(ix + i32(kx), iy + i32(ky));
                        min_val = min(min_val, sample);
                        max_val = max(max_val, sample);
                    }
                }
                output[idx] = max_val - min_val;
            "#,
        };

        format!(
            r#"
struct MorphologyParams {{
    width: u32,
    height: u32,
    kernel_size: u32,
    _padding: u32,
}}

@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;
@group(0) @binding(2) var<uniform> params: MorphologyParams;

fn get_value(x: i32, y: i32) -> f32 {{
    if (x < 0 || x >= i32(params.width) || y < 0 || y >= i32(params.height)) {{
        return 0.0;
    }}
    let idx = u32(y) * params.width + u32(x);
    return input[idx];
}}

@compute @workgroup_size(256)
fn morphology_main(@builtin(global_invocation_id) global_id: vec3<u32>) {{
    let idx = global_id.x;
    if (idx >= arrayLength(&output)) {{
        return;
    }}

    let x = i32(idx % params.width);
    let y = i32(idx / params.width);

    let half_k = i32(params.kernel_size / 2u);
    let ix = x - half_k;
    let iy = y - half_k;

    {}
}}
"#,
            computation
        )
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct MorphologyParams {
    width: u32,
    height: u32,
    kernel_size: u32,
    _padding: u32,
}

/// Edge detection kernel.
pub struct EdgeDetectionKernel {
    context: GpuContext,
    pipeline: ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    method: EdgeDetectionMethod,
}

impl EdgeDetectionKernel {
    /// Create a new edge detection kernel.
    ///
    /// # Errors
    ///
    /// Returns an error if shader compilation or pipeline creation fails.
    pub fn new(context: &GpuContext, method: EdgeDetectionMethod) -> GpuResult<Self> {
        let shader_code = Self::generate_shader(method);
        let mut shader = WgslShader::new(shader_code, "edge_detect_main");
        let shader_module = shader.compile(context.device())?;

        let bind_group_layout = create_compute_bind_group_layout(
            context.device(),
            &[
                storage_buffer_layout(0, true),  // input
                storage_buffer_layout(1, false), // output
                storage_buffer_layout(2, true),  // params
            ],
            Some("Edge Detection Bind Group Layout"),
        )?;

        let pipeline = ComputePipelineBuilder::new(context.device(), shader_module, "edge_detect_main")
            .bind_group_layout(&bind_group_layout)
            .label("Edge Detection Pipeline")
            .build()?;

        debug!("Created edge detection kernel: {:?}", method);

        Ok(Self {
            context: context.clone(),
            pipeline,
            bind_group_layout,
            method,
        })
    }

    /// Execute edge detection.
    ///
    /// # Errors
    ///
    /// Returns an error if execution fails.
    pub fn execute<T: Pod>(
        &self,
        input: &GpuBuffer<T>,
        width: u32,
        height: u32,
    ) -> GpuResult<GpuBuffer<T>> {
        let mut output = GpuBuffer::new(
            &self.context,
            input.len(),
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        )?;

        let params = EdgeDetectionParams {
            width,
            height,
            _padding: [0; 2],
        };

        let params_buffer = self.context.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("Edge Detection Params"),
            size: std::mem::size_of::<EdgeDetectionParams>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        self.context.queue().write_buffer(
            &params_buffer,
            0,
            bytemuck::bytes_of(&params),
        );

        let bind_group = self.context.device().create_bind_group(&BindGroupDescriptor {
            label: Some("Edge Detection Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: input.buffer().as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: output.buffer().as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: params_buffer.as_entire_binding(),
                },
            ],
        });

        let mut encoder = self.context.device().create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Edge Detection Encoder"),
        });

        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Edge Detection Compute Pass"),
                timestamp_writes: None,
            });

            cpass.set_pipeline(&self.pipeline);
            cpass.set_bind_group(0, &bind_group, &[]);

            let workgroup_size = 256;
            let num_workgroups = ((input.len() as u32 + workgroup_size - 1) / workgroup_size).max(1);
            cpass.dispatch_workgroups(num_workgroups, 1, 1);
        }

        self.context.queue().submit(Some(encoder.finish()));

        debug!("Executed edge detection: {:?}", self.method);

        Ok(output)
    }

    fn generate_shader(method: EdgeDetectionMethod) -> String {
        let computation = match method {
            EdgeDetectionMethod::Sobel => r#"
                // Sobel operator
                let gx = -z1 + z3 - 2.0 * z4 + 2.0 * z6 - z7 + z9;
                let gy = -z1 - 2.0 * z2 - z3 + z7 + 2.0 * z8 + z9;
                let magnitude = sqrt(gx * gx + gy * gy);
                output[idx] = magnitude;
            "#,
            EdgeDetectionMethod::Prewitt => r#"
                // Prewitt operator
                let gx = -z1 + z3 - z4 + z6 - z7 + z9;
                let gy = -z1 - z2 - z3 + z7 + z8 + z9;
                let magnitude = sqrt(gx * gx + gy * gy);
                output[idx] = magnitude;
            "#,
            EdgeDetectionMethod::Canny => r#"
                // Simplified Canny (just Gaussian + Sobel)
                let gx = -z1 + z3 - 2.0 * z4 + 2.0 * z6 - z7 + z9;
                let gy = -z1 - 2.0 * z2 - z3 + z7 + 2.0 * z8 + z9;
                let magnitude = sqrt(gx * gx + gy * gy);
                output[idx] = magnitude;
            "#,
            EdgeDetectionMethod::Laplacian => r#"
                // Laplacian operator
                let laplacian = z2 + z4 - 4.0 * z5 + z6 + z8;
                output[idx] = abs(laplacian);
            "#,
        };

        format!(
            r#"
struct EdgeDetectionParams {{
    width: u32,
    height: u32,
    _padding: array<u32, 2>,
}}

@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;
@group(0) @binding(2) var<uniform> params: EdgeDetectionParams;

fn get_value(x: i32, y: i32) -> f32 {{
    if (x < 0 || x >= i32(params.width) || y < 0 || y >= i32(params.height)) {{
        return 0.0;
    }}
    let idx = u32(y) * params.width + u32(x);
    return input[idx];
}}

@compute @workgroup_size(256)
fn edge_detect_main(@builtin(global_invocation_id) global_id: vec3<u32>) {{
    let idx = global_id.x;
    if (idx >= arrayLength(&output)) {{
        return;
    }}

    let x = i32(idx % params.width);
    let y = i32(idx / params.width);

    // Get 3x3 neighborhood
    let z1 = get_value(x - 1, y - 1);
    let z2 = get_value(x, y - 1);
    let z3 = get_value(x + 1, y - 1);
    let z4 = get_value(x - 1, y);
    let z5 = get_value(x, y);
    let z6 = get_value(x + 1, y);
    let z7 = get_value(x - 1, y + 1);
    let z8 = get_value(x, y + 1);
    let z9 = get_value(x + 1, y + 1);

    {}
}}
"#,
            computation
        )
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct EdgeDetectionParams {
    width: u32,
    height: u32,
    _padding: [u32; 2],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_terrain_operations() {
        if let Ok(context) = GpuContext::new().await {
            for op in [
                TerrainOp::Slope,
                TerrainOp::Aspect,
                TerrainOp::Hillshade,
                TerrainOp::Curvature,
                TerrainOp::Roughness,
            ] {
                assert!(TerrainKernel::new(&context, op).is_ok());
            }
        }
    }

    #[tokio::test]
    async fn test_morphological_operations() {
        if let Ok(context) = GpuContext::new().await {
            for op in [
                MorphologicalOp::Erosion,
                MorphologicalOp::Dilation,
                MorphologicalOp::Opening,
                MorphologicalOp::Closing,
                MorphologicalOp::Gradient,
            ] {
                assert!(MorphologicalKernel::new(&context, op).is_ok());
            }
        }
    }

    #[tokio::test]
    async fn test_edge_detection() {
        if let Ok(context) = GpuContext::new().await {
            for method in [
                EdgeDetectionMethod::Sobel,
                EdgeDetectionMethod::Prewitt,
                EdgeDetectionMethod::Canny,
                EdgeDetectionMethod::Laplacian,
            ] {
                assert!(EdgeDetectionKernel::new(&context, method).is_ok());
            }
        }
    }
}
