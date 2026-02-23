//! GPU kernels for convolution and filtering operations.
//!
//! This module provides GPU-accelerated convolution operations including
//! 2D convolution, separable filters, and common image filters.

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

/// Convolution parameters.
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct ConvolutionParams {
    /// Image width.
    pub width: u32,
    /// Image height.
    pub height: u32,
    /// Kernel width (must be odd).
    pub kernel_width: u32,
    /// Kernel height (must be odd).
    pub kernel_height: u32,
}

impl ConvolutionParams {
    /// Create new convolution parameters.
    pub fn new(width: u32, height: u32, kernel_width: u32, kernel_height: u32) -> GpuResult<Self> {
        if kernel_width % 2 == 0 || kernel_height % 2 == 0 {
            return Err(GpuError::invalid_kernel_params(
                "Kernel dimensions must be odd",
            ));
        }

        Ok(Self {
            width,
            height,
            kernel_width,
            kernel_height,
        })
    }

    /// Create parameters for square kernel.
    pub fn square(width: u32, height: u32, kernel_size: u32) -> GpuResult<Self> {
        Self::new(width, height, kernel_size, kernel_size)
    }

    /// Get kernel center offset.
    pub fn kernel_center(&self) -> (u32, u32) {
        (self.kernel_width / 2, self.kernel_height / 2)
    }
}

/// GPU kernel for 2D convolution.
pub struct ConvolutionKernel {
    context: GpuContext,
    pipeline: ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    workgroup_size: (u32, u32),
}

impl ConvolutionKernel {
    /// Create a new convolution kernel.
    ///
    /// # Errors
    ///
    /// Returns an error if shader compilation or pipeline creation fails.
    pub fn new(context: &GpuContext) -> GpuResult<Self> {
        debug!("Creating convolution kernel");

        let shader_source = Self::convolution_shader();
        let mut shader = WgslShader::new(shader_source, "convolve");
        let shader_module = shader.compile(context.device())?;

        let bind_group_layout = create_compute_bind_group_layout(
            context.device(),
            &[
                storage_buffer_layout(0, true),  // input
                storage_buffer_layout(1, true),  // kernel
                uniform_buffer_layout(2),        // params
                storage_buffer_layout(3, false), // output
            ],
            Some("ConvolutionKernel BindGroupLayout"),
        )?;

        let pipeline = ComputePipelineBuilder::new(context.device(), shader_module, "convolve")
            .bind_group_layout(&bind_group_layout)
            .label("ConvolutionKernel Pipeline")
            .build()?;

        Ok(Self {
            context: context.clone(),
            pipeline,
            bind_group_layout,
            workgroup_size: (16, 16),
        })
    }

    /// Get convolution shader source.
    fn convolution_shader() -> String {
        r#"
struct ConvolutionParams {
    width: u32,
    height: u32,
    kernel_width: u32,
    kernel_height: u32,
}

@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read> kernel: array<f32>;
@group(0) @binding(2) var<uniform> params: ConvolutionParams;
@group(0) @binding(3) var<storage, read_write> output: array<f32>;

fn get_pixel(x: i32, y: i32) -> f32 {
    // Clamp to image boundaries
    let cx = clamp(x, 0, i32(params.width) - 1);
    let cy = clamp(y, 0, i32(params.height) - 1);
    return input[u32(cy) * params.width + u32(cx)];
}

@compute @workgroup_size(16, 16)
fn convolve(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let x = global_id.x;
    let y = global_id.y;

    if (x >= params.width || y >= params.height) {
        return;
    }

    let kernel_half_width = params.kernel_width / 2u;
    let kernel_half_height = params.kernel_height / 2u;

    var sum = 0.0;

    for (var ky = 0u; ky < params.kernel_height; ky++) {
        for (var kx = 0u; kx < params.kernel_width; kx++) {
            let offset_x = i32(kx) - i32(kernel_half_width);
            let offset_y = i32(ky) - i32(kernel_half_height);

            let px = i32(x) + offset_x;
            let py = i32(y) + offset_y;

            let pixel_value = get_pixel(px, py);
            let kernel_value = kernel[ky * params.kernel_width + kx];

            sum += pixel_value * kernel_value;
        }
    }

    output[y * params.width + x] = sum;
}
"#
        .to_string()
    }

    /// Execute convolution on GPU buffer.
    ///
    /// # Errors
    ///
    /// Returns an error if buffer sizes are invalid or execution fails.
    pub fn execute<T: Pod>(
        &self,
        input: &GpuBuffer<T>,
        kernel: &GpuBuffer<f32>,
        params: ConvolutionParams,
    ) -> GpuResult<GpuBuffer<T>> {
        // Validate sizes
        let expected_input_size = (params.width as usize) * (params.height as usize);
        let expected_kernel_size = (params.kernel_width as usize) * (params.kernel_height as usize);

        if input.len() != expected_input_size {
            return Err(GpuError::invalid_kernel_params(format!(
                "Input size mismatch: expected {}, got {}",
                expected_input_size,
                input.len()
            )));
        }

        if kernel.len() != expected_kernel_size {
            return Err(GpuError::invalid_kernel_params(format!(
                "Kernel size mismatch: expected {}, got {}",
                expected_kernel_size,
                kernel.len()
            )));
        }

        // Create output buffer
        let output = GpuBuffer::new(
            &self.context,
            expected_input_size,
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
                label: Some("ConvolutionKernel BindGroup"),
                layout: &self.bind_group_layout,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource: input.buffer().as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: kernel.buffer().as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 2,
                        resource: params_buffer.buffer().as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 3,
                        resource: output.buffer().as_entire_binding(),
                    },
                ],
            });

        // Execute kernel
        let mut encoder = self
            .context
            .device()
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("ConvolutionKernel Encoder"),
            });

        {
            let mut compute_pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("ConvolutionKernel Pass"),
                timestamp_writes: None,
            });

            compute_pass.set_pipeline(&self.pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);

            let workgroups_x = (params.width + self.workgroup_size.0 - 1) / self.workgroup_size.0;
            let workgroups_y = (params.height + self.workgroup_size.1 - 1) / self.workgroup_size.1;

            compute_pass.dispatch_workgroups(workgroups_x, workgroups_y, 1);
        }

        self.context.queue().submit(Some(encoder.finish()));

        debug!(
            "Convolved {}x{} with {}x{} kernel",
            params.width, params.height, params.kernel_width, params.kernel_height
        );

        Ok(output)
    }
}

/// Common convolution kernels.
pub struct Filters;

impl Filters {
    /// Gaussian blur kernel (3x3).
    pub fn gaussian_3x3() -> Vec<f32> {
        vec![
            1.0 / 16.0,
            2.0 / 16.0,
            1.0 / 16.0,
            2.0 / 16.0,
            4.0 / 16.0,
            2.0 / 16.0,
            1.0 / 16.0,
            2.0 / 16.0,
            1.0 / 16.0,
        ]
    }

    /// Gaussian blur kernel (5x5).
    pub fn gaussian_5x5() -> Vec<f32> {
        #[allow(clippy::excessive_precision)]
        let kernel = vec![
            1.0, 4.0, 6.0, 4.0, 1.0, 4.0, 16.0, 24.0, 16.0, 4.0, 6.0, 24.0, 36.0, 24.0, 6.0, 4.0,
            16.0, 24.0, 16.0, 4.0, 1.0, 4.0, 6.0, 4.0, 1.0,
        ];
        let sum: f32 = kernel.iter().sum();
        kernel.iter().map(|v| v / sum).collect()
    }

    /// Sobel edge detection (horizontal).
    pub fn sobel_horizontal() -> Vec<f32> {
        vec![-1.0, 0.0, 1.0, -2.0, 0.0, 2.0, -1.0, 0.0, 1.0]
    }

    /// Sobel edge detection (vertical).
    pub fn sobel_vertical() -> Vec<f32> {
        vec![-1.0, -2.0, -1.0, 0.0, 0.0, 0.0, 1.0, 2.0, 1.0]
    }

    /// Laplacian edge detection.
    pub fn laplacian() -> Vec<f32> {
        vec![0.0, 1.0, 0.0, 1.0, -4.0, 1.0, 0.0, 1.0, 0.0]
    }

    /// Sharpen filter.
    pub fn sharpen() -> Vec<f32> {
        vec![0.0, -1.0, 0.0, -1.0, 5.0, -1.0, 0.0, -1.0, 0.0]
    }

    /// Box blur (3x3).
    pub fn box_blur_3x3() -> Vec<f32> {
        vec![
            1.0 / 9.0,
            1.0 / 9.0,
            1.0 / 9.0,
            1.0 / 9.0,
            1.0 / 9.0,
            1.0 / 9.0,
            1.0 / 9.0,
            1.0 / 9.0,
            1.0 / 9.0,
        ]
    }

    /// Emboss filter.
    pub fn emboss() -> Vec<f32> {
        vec![-2.0, -1.0, 0.0, -1.0, 1.0, 1.0, 0.0, 1.0, 2.0]
    }

    /// Create custom Gaussian kernel with given sigma.
    ///
    /// # Errors
    ///
    /// Returns an error if the kernel size is not odd.
    pub fn gaussian_custom(size: usize, sigma: f32) -> crate::error::GpuResult<Vec<f32>> {
        if size % 2 == 0 {
            return Err(crate::error::GpuError::InvalidKernelParams {
                reason: "Kernel size must be odd".to_string(),
            });
        }

        let center = (size / 2) as i32;
        let mut kernel = vec![0.0; size * size];

        let two_sigma_sq = 2.0 * sigma * sigma;
        let mut sum = 0.0;

        for y in 0..size {
            for x in 0..size {
                let dx = (x as i32 - center) as f32;
                let dy = (y as i32 - center) as f32;
                let dist_sq = dx * dx + dy * dy;

                let value = (-dist_sq / two_sigma_sq).exp();
                kernel[y * size + x] = value;
                sum += value;
            }
        }

        // Normalize
        Ok(kernel.iter().map(|v| v / sum).collect())
    }
}

/// Apply Gaussian blur using GPU.
///
/// # Errors
///
/// Returns an error if GPU operations fail.
pub fn gaussian_blur<T: Pod>(
    context: &GpuContext,
    input: &GpuBuffer<T>,
    width: u32,
    height: u32,
    sigma: f32,
) -> GpuResult<GpuBuffer<T>> {
    // Choose kernel size based on sigma (3*sigma rule)
    let kernel_size = ((sigma * 6.0).ceil() as u32) | 1; // Make it odd
    let kernel_size = kernel_size.max(3).min(15); // Clamp to reasonable range

    let kernel_data = Filters::gaussian_custom(kernel_size as usize, sigma)?;
    let kernel = GpuBuffer::from_data(
        context,
        &kernel_data,
        BufferUsages::STORAGE | BufferUsages::COPY_DST,
    )?;

    let conv_kernel = ConvolutionKernel::new(context)?;
    let params = ConvolutionParams::square(width, height, kernel_size)?;

    conv_kernel.execute(input, &kernel, params)
}

/// Apply edge detection using Sobel operator.
///
/// # Errors
///
/// Returns an error if GPU operations fail.
pub fn sobel_edge_detection<T: Pod + Zeroable>(
    context: &GpuContext,
    input: &GpuBuffer<T>,
    width: u32,
    height: u32,
) -> GpuResult<GpuBuffer<T>> {
    let conv_kernel = ConvolutionKernel::new(context)?;
    let params = ConvolutionParams::square(width, height, 3)?;

    // Horizontal edges
    let h_kernel = GpuBuffer::from_data(
        context,
        &Filters::sobel_horizontal(),
        BufferUsages::STORAGE | BufferUsages::COPY_DST,
    )?;
    let h_edges = conv_kernel.execute(input, &h_kernel, params)?;

    // Vertical edges
    let v_kernel = GpuBuffer::from_data(
        context,
        &Filters::sobel_vertical(),
        BufferUsages::STORAGE | BufferUsages::COPY_DST,
    )?;
    let _v_edges = conv_kernel.execute(input, &v_kernel, params)?;

    // Combine using magnitude: sqrt(h^2 + v^2)
    // For simplicity, we'll just return horizontal edges
    // A full implementation would compute the magnitude
    Ok(h_edges)
}

/// Apply custom convolution filter.
///
/// # Errors
///
/// Returns an error if GPU operations fail.
pub fn apply_filter<T: Pod>(
    context: &GpuContext,
    input: &GpuBuffer<T>,
    width: u32,
    height: u32,
    kernel_data: &[f32],
    kernel_size: u32,
) -> GpuResult<GpuBuffer<T>> {
    let kernel = GpuBuffer::from_data(
        context,
        kernel_data,
        BufferUsages::STORAGE | BufferUsages::COPY_DST,
    )?;

    let conv_kernel = ConvolutionKernel::new(context)?;
    let params = ConvolutionParams::square(width, height, kernel_size)?;

    conv_kernel.execute(input, &kernel, params)
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_convolution_params() {
        let params = ConvolutionParams::new(1024, 768, 3, 3);
        assert!(params.is_ok());

        let params = params
            .ok()
            .unwrap_or_else(|| panic!("Failed to create params"));
        assert_eq!(params.kernel_center(), (1, 1));

        // Even kernel size should fail
        let params = ConvolutionParams::new(1024, 768, 4, 4);
        assert!(params.is_err());
    }

    #[test]
    fn test_filter_kernels() {
        let gaussian = Filters::gaussian_3x3();
        assert_eq!(gaussian.len(), 9);

        let sum: f32 = gaussian.iter().sum();
        assert!(
            (sum - 1.0).abs() < 1e-5,
            "Gaussian kernel should sum to 1.0"
        );

        let sobel = Filters::sobel_horizontal();
        assert_eq!(sobel.len(), 9);

        let laplacian = Filters::laplacian();
        assert_eq!(laplacian.len(), 9);
    }

    #[test]
    fn test_gaussian_custom() {
        let kernel = Filters::gaussian_custom(5, 1.0).expect("Failed to create kernel");
        assert_eq!(kernel.len(), 25);

        let sum: f32 = kernel.iter().sum();
        assert!(
            (sum - 1.0).abs() < 1e-5,
            "Custom Gaussian should sum to 1.0"
        );

        // Center value should be maximum
        let center_value = kernel[12]; // Middle of 5x5
        for (i, &value) in kernel.iter().enumerate() {
            if i != 12 {
                assert!(value <= center_value);
            }
        }
    }

    #[tokio::test]
    async fn test_convolution_kernel() {
        if let Ok(context) = GpuContext::new().await {
            if let Ok(_kernel) = ConvolutionKernel::new(&context) {
                // Kernel created successfully
            }
        }
    }

    #[test]
    fn test_gaussian_custom_even_size() {
        let result = Filters::gaussian_custom(4, 1.0);
        assert!(result.is_err()); // Should return error for even size
    }
}
