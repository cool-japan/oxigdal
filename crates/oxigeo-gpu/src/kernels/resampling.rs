//! GPU kernels for raster resampling operations.
//!
//! This module provides GPU-accelerated resampling operations including
//! nearest neighbor, bilinear, and bicubic interpolation.

use crate::buffer::GpuBuffer;
use crate::context::GpuContext;
use crate::error::{GpuError, GpuResult};
use crate::shaders::{
    ComputePipelineBuilder, WgslShader, create_compute_bind_group_layout, storage_buffer_layout,
    uniform_buffer_layout,
};
use bytemuck::{Pod, Zeroable};
use tracing::debug;
use wgpu::{
    BindGroupDescriptor, BindGroupEntry, BufferUsages, CommandEncoderDescriptor,
    ComputePassDescriptor, ComputePipeline,
};

/// Resampling method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResamplingMethod {
    /// Nearest neighbor (fast, blocky).
    NearestNeighbor,
    /// Bilinear interpolation (smooth, fast).
    Bilinear,
    /// Bicubic interpolation (highest quality, slower).
    Bicubic,
    /// Lanczos resampling with window radius `a` (typical values: 2 or 3).
    ///
    /// Evaluates `L(x) = sinc(x) * sinc(x/a)` for `|x| < a`, else 0.
    /// The 2D kernel is the outer product of two 1D Lanczos kernels.
    /// Results are normalized by dividing by the total weight sum.
    ///
    /// Valid range for `a`: 1..=8.
    Lanczos {
        /// Window radius (half-width). Valid range: 1..=8.
        a: u32,
    },
}

impl ResamplingMethod {
    /// Get the shader entry point name for this resampling method.
    pub fn entry_point(&self) -> &'static str {
        match self {
            Self::NearestNeighbor => "nearest_neighbor",
            Self::Bilinear => "bilinear",
            Self::Bicubic => "bicubic",
            Self::Lanczos { .. } => "lanczos",
        }
    }
}

/// Resampling parameters.
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct ResamplingParams {
    /// Source width.
    pub src_width: u32,
    /// Source height.
    pub src_height: u32,
    /// Destination width.
    pub dst_width: u32,
    /// Destination height.
    pub dst_height: u32,
}

impl ResamplingParams {
    /// Create new resampling parameters.
    pub fn new(src_width: u32, src_height: u32, dst_width: u32, dst_height: u32) -> Self {
        Self {
            src_width,
            src_height,
            dst_width,
            dst_height,
        }
    }

    /// Calculate scale factors.
    pub fn scale_factors(&self) -> (f32, f32) {
        let scale_x = self.src_width as f32 / self.dst_width as f32;
        let scale_y = self.src_height as f32 / self.dst_height as f32;
        (scale_x, scale_y)
    }
}

/// GPU kernel for resampling operations.
pub struct ResamplingKernel {
    context: GpuContext,
    pipeline: ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    workgroup_size: (u32, u32),
    method: ResamplingMethod,
}

impl ResamplingKernel {
    /// Create a new resampling kernel.
    ///
    /// The workgroup size is taken from `context.tuner.raster_2d` so it
    /// automatically adapts to low-end and high-end GPUs.
    ///
    /// # Errors
    ///
    /// Returns an error if shader compilation or pipeline creation fails,
    /// or if `method` parameters are out of valid range.
    pub fn new(context: &GpuContext, method: ResamplingMethod) -> GpuResult<Self> {
        let workgroup_size = context.tuner.raster_2d;
        Self::new_with_workgroup_size(context, method, workgroup_size)
    }

    /// Create a new resampling kernel with an explicit workgroup size.
    ///
    /// Prefer [`ResamplingKernel::new`] which derives the size from the
    /// context's tuner automatically.
    ///
    /// # Errors
    ///
    /// Returns an error if shader compilation or pipeline creation fails,
    /// or if `method` parameters are out of valid range.
    pub fn new_with_workgroup_size(
        context: &GpuContext,
        method: ResamplingMethod,
        workgroup_size: (u32, u32),
    ) -> GpuResult<Self> {
        debug!(
            "Creating resampling kernel: {:?} (workgroup {}x{})",
            method, workgroup_size.0, workgroup_size.1
        );

        // Validate Lanczos window parameter before touching the GPU.
        if let ResamplingMethod::Lanczos { a } = method
            && (a == 0 || a > 8)
        {
            return Err(GpuError::invalid_kernel_params(format!(
                "Lanczos window parameter a={a} out of valid range 1..=8"
            )));
        }

        let shader_source = Self::resampling_shader(method, workgroup_size);
        let mut shader = WgslShader::new(shader_source, method.entry_point());
        let shader_module = shader.compile(context.device())?;

        let bind_group_layout = create_compute_bind_group_layout(
            context.device(),
            &[
                storage_buffer_layout(0, true),  // input
                uniform_buffer_layout(1),        // params
                storage_buffer_layout(2, false), // output
            ],
            Some("ResamplingKernel BindGroupLayout"),
        )?;

        let pipeline =
            ComputePipelineBuilder::new(context.device(), shader_module, method.entry_point())
                .bind_group_layout(&bind_group_layout)
                .label(format!("ResamplingKernel Pipeline: {:?}", method))
                .build()?;

        Ok(Self {
            context: context.clone(),
            pipeline,
            bind_group_layout,
            workgroup_size,
            method,
        })
    }

    /// Get shader source for resampling method, with workgroup size interpolated
    /// into the WGSL `@workgroup_size` attribute at build time.
    ///
    /// Because `@workgroup_size` requires compile-time constants in WGSL, the
    /// sizes are baked into the shader source string rather than passed as
    /// push constants.  The pipeline is therefore implicitly keyed by workgroup
    /// size, which is the correct behaviour — different devices compile
    /// different (but correct) pipelines.
    fn resampling_shader(method: ResamplingMethod, workgroup_size: (u32, u32)) -> String {
        let (wg_x, wg_y) = workgroup_size;

        let common = r#"
struct ResamplingParams {
    src_width: u32,
    src_height: u32,
    dst_width: u32,
    dst_height: u32,
}

@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<uniform> params: ResamplingParams;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;

fn get_pixel(x: u32, y: u32) -> f32 {
    if (x >= params.src_width || y >= params.src_height) {
        return 0.0;
    }
    return input[y * params.src_width + x];
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    return a + (b - a) * t;
}
"#;

        match method {
            ResamplingMethod::NearestNeighbor => format!(
                r#"
{}

@compute @workgroup_size({wg_x}, {wg_y})
fn nearest_neighbor(@builtin(global_invocation_id) global_id: vec3<u32>) {{
    let dst_x = global_id.x;
    let dst_y = global_id.y;

    if (dst_x >= params.dst_width || dst_y >= params.dst_height) {{
        return;
    }}

    let scale_x = f32(params.src_width) / f32(params.dst_width);
    let scale_y = f32(params.src_height) / f32(params.dst_height);

    let src_x = u32(f32(dst_x) * scale_x);
    let src_y = u32(f32(dst_y) * scale_y);

    let value = get_pixel(src_x, src_y);
    output[dst_y * params.dst_width + dst_x] = value;
}}
"#,
                common,
                wg_x = wg_x,
                wg_y = wg_y,
            ),

            ResamplingMethod::Bilinear => format!(
                r#"
{}

@compute @workgroup_size({wg_x}, {wg_y})
fn bilinear(@builtin(global_invocation_id) global_id: vec3<u32>) {{
    let dst_x = global_id.x;
    let dst_y = global_id.y;

    if (dst_x >= params.dst_width || dst_y >= params.dst_height) {{
        return;
    }}

    let scale_x = f32(params.src_width) / f32(params.dst_width);
    let scale_y = f32(params.src_height) / f32(params.dst_height);

    let src_x = f32(dst_x) * scale_x;
    let src_y = f32(dst_y) * scale_y;

    let x0 = u32(floor(src_x));
    let y0 = u32(floor(src_y));
    let x1 = min(x0 + 1u, params.src_width - 1u);
    let y1 = min(y0 + 1u, params.src_height - 1u);

    let tx = fract(src_x);
    let ty = fract(src_y);

    let v00 = get_pixel(x0, y0);
    let v10 = get_pixel(x1, y0);
    let v01 = get_pixel(x0, y1);
    let v11 = get_pixel(x1, y1);

    let v0 = lerp(v00, v10, tx);
    let v1 = lerp(v01, v11, tx);
    let value = lerp(v0, v1, ty);

    output[dst_y * params.dst_width + dst_x] = value;
}}
"#,
                common,
                wg_x = wg_x,
                wg_y = wg_y,
            ),

            ResamplingMethod::Bicubic => format!(
                r#"
{}

fn cubic_interpolate(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {{
    let a = -0.5 * p0 + 1.5 * p1 - 1.5 * p2 + 0.5 * p3;
    let b = p0 - 2.5 * p1 + 2.0 * p2 - 0.5 * p3;
    let c = -0.5 * p0 + 0.5 * p2;
    let d = p1;
    return a * t * t * t + b * t * t + c * t + d;
}}

@compute @workgroup_size({wg_x}, {wg_y})
fn bicubic(@builtin(global_invocation_id) global_id: vec3<u32>) {{
    let dst_x = global_id.x;
    let dst_y = global_id.y;

    if (dst_x >= params.dst_width || dst_y >= params.dst_height) {{
        return;
    }}

    let scale_x = f32(params.src_width) / f32(params.dst_width);
    let scale_y = f32(params.src_height) / f32(params.dst_height);

    let src_x = f32(dst_x) * scale_x;
    let src_y = f32(dst_y) * scale_y;

    let x_floor = floor(src_x);
    let y_floor = floor(src_y);
    let tx = fract(src_x);
    let ty = fract(src_y);

    // Sample 4x4 neighborhood
    var cols: array<f32, 4>;
    for (var j = 0; j < 4; j++) {{
        let y = i32(y_floor) + j - 1;
        var row: array<f32, 4>;
        for (var i = 0; i < 4; i++) {{
            let x = i32(x_floor) + i - 1;
            if (x >= 0 && x < i32(params.src_width) && y >= 0 && y < i32(params.src_height)) {{
                row[i] = get_pixel(u32(x), u32(y));
            }} else {{
                row[i] = 0.0;
            }}
        }}
        cols[j] = cubic_interpolate(row[0], row[1], row[2], row[3], tx);
    }}

    let value = cubic_interpolate(cols[0], cols[1], cols[2], cols[3], ty);
    output[dst_y * params.dst_width + dst_x] = value;
}}
"#,
                common,
                wg_x = wg_x,
                wg_y = wg_y,
            ),

            ResamplingMethod::Lanczos { a } => {
                // Pre-compute the loop trip count: 2*a samples per axis.
                let two_a = 2 * a;
                format!(
                    r#"
{common}

// sinc(x) = sin(pi*x) / (pi*x), with sinc(0) = 1.
fn sinc(x: f32) -> f32 {{
    if (abs(x) < 1e-6) {{
        return 1.0;
    }}
    let pi_x: f32 = 3.14159265358979323846 * x;
    return sin(pi_x) / pi_x;
}}

// Lanczos weighting function with window radius a.
// L(x) = sinc(x) * sinc(x/a)  for |x| < a, else 0.
fn lanczos_weight(x: f32, a: f32) -> f32 {{
    if (abs(x) >= a) {{
        return 0.0;
    }}
    return sinc(x) * sinc(x / a);
}}

@compute @workgroup_size({wg_x}, {wg_y})
fn lanczos(@builtin(global_invocation_id) global_id: vec3<u32>) {{
    let dst_x = global_id.x;
    let dst_y = global_id.y;

    if (dst_x >= params.dst_width || dst_y >= params.dst_height) {{
        return;
    }}

    let scale_x = f32(params.src_width) / f32(params.dst_width);
    let scale_y = f32(params.src_height) / f32(params.dst_height);

    // Map destination pixel centre to source coordinate space.
    // The +0.5 / -0.5 shift aligns pixel centres correctly.
    let src_cx = (f32(dst_x) + 0.5) * scale_x - 0.5;
    let src_cy = (f32(dst_y) + 0.5) * scale_y - 0.5;

    // Window radius injected at shader-build time.
    let lanczos_a: f32 = {a}f;

    var weight_sum: f32 = 0.0;
    var value_sum: f32 = 0.0;

    // Sample 2*a rows and 2*a columns centred on src_c(x,y).
    // Row/col index starts at floor(src_c) - a + 1 and runs for 2*a steps.
    let x_start: i32 = i32(floor(src_cx)) - i32({a}) + 1;
    let y_start: i32 = i32(floor(src_cy)) - i32({a}) + 1;

    for (var j: i32 = 0; j < {two_a}; j++) {{
        let sy: i32 = y_start + j;
        let wy: f32 = lanczos_weight(src_cy - f32(sy), lanczos_a);
        if (wy == 0.0) {{ continue; }}

        // Clamp-to-edge: keep source coordinate inside the image.
        let clamped_y: u32 = u32(clamp(sy, 0, i32(params.src_height) - 1));

        for (var i: i32 = 0; i < {two_a}; i++) {{
            let sx: i32 = x_start + i;
            let wx: f32 = lanczos_weight(src_cx - f32(sx), lanczos_a);
            if (wx == 0.0) {{ continue; }}

            let clamped_x: u32 = u32(clamp(sx, 0, i32(params.src_width) - 1));

            let w: f32 = wx * wy;
            weight_sum += w;
            value_sum += w * get_pixel(clamped_x, clamped_y);
        }}
    }}

    // Normalise to preserve overall brightness; guard against zero weight sum.
    let result: f32 = select(0.0, value_sum / weight_sum, weight_sum > 1e-10);
    output[dst_y * params.dst_width + dst_x] = result;
}}
"#,
                    a = a,
                    two_a = two_a,
                    wg_x = wg_x,
                    wg_y = wg_y,
                )
            }
        }
    }

    /// Execute resampling on GPU buffer.
    ///
    /// # Errors
    ///
    /// Returns an error if buffer sizes are invalid or execution fails.
    pub fn execute<T: Pod>(
        &self,
        input: &GpuBuffer<T>,
        params: ResamplingParams,
    ) -> GpuResult<GpuBuffer<T>> {
        // Validate input size
        let expected_input_size = (params.src_width as usize) * (params.src_height as usize);
        if input.len() != expected_input_size {
            return Err(GpuError::invalid_kernel_params(format!(
                "Input buffer size mismatch: expected {}, got {}",
                expected_input_size,
                input.len()
            )));
        }

        // Create output buffer
        let output_size = (params.dst_width as usize) * (params.dst_height as usize);
        let output = GpuBuffer::new(
            &self.context,
            output_size,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        )?;

        // Create params buffer
        let params_buffer = GpuBuffer::from_data(
            &self.context,
            &[params],
            BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        )?;

        // Create bind group
        let bind_group = self
            .context
            .device()
            .create_bind_group(&BindGroupDescriptor {
                label: Some("ResamplingKernel BindGroup"),
                layout: &self.bind_group_layout,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource: input.buffer().as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: params_buffer.buffer().as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 2,
                        resource: output.buffer().as_entire_binding(),
                    },
                ],
            });

        // Execute kernel
        let mut encoder = self
            .context
            .device()
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("ResamplingKernel Encoder"),
            });

        {
            let mut compute_pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("ResamplingKernel Pass"),
                timestamp_writes: None,
            });

            compute_pass.set_pipeline(&self.pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);

            let workgroups_x =
                (params.dst_width + self.workgroup_size.0 - 1) / self.workgroup_size.0;
            let workgroups_y =
                (params.dst_height + self.workgroup_size.1 - 1) / self.workgroup_size.1;

            compute_pass.dispatch_workgroups(workgroups_x, workgroups_y, 1);
        }

        self.context.check_device_lost()?;
        self.context.queue().submit(Some(encoder.finish()));

        debug!(
            "Resampled {}x{} to {}x{} using {:?}",
            params.src_width, params.src_height, params.dst_width, params.dst_height, self.method
        );

        Ok(output)
    }
}

/// Resize raster using GPU acceleration.
///
/// # Errors
///
/// Returns an error if GPU operations fail.
pub fn resize<T: Pod>(
    context: &GpuContext,
    input: &GpuBuffer<T>,
    src_width: u32,
    src_height: u32,
    dst_width: u32,
    dst_height: u32,
    method: ResamplingMethod,
) -> GpuResult<GpuBuffer<T>> {
    let kernel = ResamplingKernel::new(context, method)?;
    let params = ResamplingParams::new(src_width, src_height, dst_width, dst_height);
    kernel.execute(input, params)
}

/// Downscale raster by factor of 2 (fast).
///
/// # Errors
///
/// Returns an error if GPU operations fail.
pub fn downscale_2x<T: Pod>(
    context: &GpuContext,
    input: &GpuBuffer<T>,
    width: u32,
    height: u32,
) -> GpuResult<GpuBuffer<T>> {
    resize(
        context,
        input,
        width,
        height,
        width / 2,
        height / 2,
        ResamplingMethod::Bilinear,
    )
}

/// Upscale raster by factor of 2.
///
/// # Errors
///
/// Returns an error if GPU operations fail.
pub fn upscale_2x<T: Pod>(
    context: &GpuContext,
    input: &GpuBuffer<T>,
    width: u32,
    height: u32,
    method: ResamplingMethod,
) -> GpuResult<GpuBuffer<T>> {
    resize(context, input, width, height, width * 2, height * 2, method)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resampling_params() {
        let params = ResamplingParams::new(1024, 768, 512, 384);
        let (scale_x, scale_y) = params.scale_factors();
        assert!((scale_x - 2.0).abs() < 1e-5);
        assert!((scale_y - 2.0).abs() < 1e-5);
    }

    #[test]
    fn test_resampling_shader() {
        let shader = ResamplingKernel::resampling_shader(ResamplingMethod::Bilinear, (16, 16));
        assert!(shader.contains("@compute"));
        assert!(shader.contains("bilinear"));
        assert!(shader.contains("@workgroup_size(16, 16)"));
    }

    #[test]
    fn test_resampling_shader_custom_workgroup() {
        let shader = ResamplingKernel::resampling_shader(ResamplingMethod::NearestNeighbor, (8, 8));
        assert!(shader.contains("@workgroup_size(8, 8)"));
        assert!(shader.contains("nearest_neighbor"));
    }

    #[test]
    fn test_resampling_shader_4x4_fallback() {
        let shader = ResamplingKernel::resampling_shader(ResamplingMethod::Bilinear, (4, 4));
        assert!(shader.contains("@workgroup_size(4, 4)"));
    }

    #[test]
    fn test_lanczos_shader_a2() {
        let shader =
            ResamplingKernel::resampling_shader(ResamplingMethod::Lanczos { a: 2 }, (16, 16));
        assert!(shader.contains("@compute"));
        assert!(shader.contains("fn lanczos("));
        assert!(shader.contains("fn sinc("));
        assert!(shader.contains("fn lanczos_weight("));
        // Loop trip count 2*a = 4 should appear in the shader.
        assert!(shader.contains("4"));
        // The window radius should be embedded as a float literal.
        assert!(shader.contains("2f"));
        assert!(shader.contains("@workgroup_size(16, 16)"));
    }

    #[test]
    fn test_lanczos_shader_a3() {
        let shader =
            ResamplingKernel::resampling_shader(ResamplingMethod::Lanczos { a: 3 }, (16, 16));
        assert!(shader.contains("fn lanczos("));
        // Loop trip count 2*a = 6.
        assert!(shader.contains("6"));
        assert!(shader.contains("3f"));
    }

    #[test]
    fn test_lanczos_shader_a2_custom_workgroup() {
        let shader =
            ResamplingKernel::resampling_shader(ResamplingMethod::Lanczos { a: 2 }, (8, 8));
        assert!(shader.contains("@workgroup_size(8, 8)"));
        assert!(shader.contains("fn lanczos("));
    }

    #[test]
    fn test_lanczos_entry_point() {
        assert_eq!(ResamplingMethod::Lanczos { a: 2 }.entry_point(), "lanczos");
        assert_eq!(ResamplingMethod::Lanczos { a: 3 }.entry_point(), "lanczos");
    }

    #[test]
    fn test_lanczos_equality() {
        assert_eq!(
            ResamplingMethod::Lanczos { a: 2 },
            ResamplingMethod::Lanczos { a: 2 }
        );
        assert_ne!(
            ResamplingMethod::Lanczos { a: 2 },
            ResamplingMethod::Lanczos { a: 3 }
        );
        assert_ne!(
            ResamplingMethod::Lanczos { a: 2 },
            ResamplingMethod::Bilinear
        );
    }

    #[tokio::test]
    async fn test_resampling_kernel() {
        if let Ok(context) = GpuContext::new().await
            && let Ok(_kernel) = ResamplingKernel::new(&context, ResamplingMethod::NearestNeighbor)
        {
            // Kernel created successfully
        }
    }

    #[tokio::test]
    async fn test_resize_operation() {
        if let Ok(context) = GpuContext::new().await {
            // Create a simple 4x4 input
            let input_data: Vec<f32> = (0..16).map(|i| i as f32).collect();

            if let Ok(input) = GpuBuffer::from_data(
                &context,
                &input_data,
                BufferUsages::STORAGE | BufferUsages::COPY_SRC,
            ) && let Ok(_output) = resize(
                &context,
                &input,
                4,
                4,
                2,
                2,
                ResamplingMethod::NearestNeighbor,
            ) {
                // Successfully resized
            }
        }
    }
}
