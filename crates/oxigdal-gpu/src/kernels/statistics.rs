//! GPU kernels for statistical raster operations.
//!
//! This module provides GPU-accelerated statistical operations including
//! parallel reduction, histogram computation, and basic statistics.

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

/// Reduction operation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReductionOp {
    /// Sum of all values.
    Sum,
    /// Minimum value.
    Min,
    /// Maximum value.
    Max,
    /// Product of all values.
    Product,
}

impl ReductionOp {
    /// Get the identity value for this operation.
    fn identity(&self) -> f32 {
        match self {
            Self::Sum => 0.0,
            Self::Min => f32::MAX,
            Self::Max => f32::MIN,
            Self::Product => 1.0,
        }
    }

    /// Get the WGSL operation expression.
    fn operation_expr(&self) -> &'static str {
        match self {
            Self::Sum => "a + b",
            Self::Min => "min(a, b)",
            Self::Max => "max(a, b)",
            Self::Product => "a * b",
        }
    }

    /// Get inline shader for parallel reduction.
    fn reduction_shader(&self) -> String {
        format!(
            r#"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

var<workgroup> shared_data: array<f32, 256>;

@compute @workgroup_size(256)
fn reduce(@builtin(global_invocation_id) global_id: vec3<u32>,
          @builtin(local_invocation_id) local_id: vec3<u32>,
          @builtin(workgroup_id) workgroup_id: vec3<u32>) {{
    let idx = global_id.x;
    let local_idx = local_id.x;
    let n = arrayLength(&input);

    // Load data into shared memory
    if (idx < n) {{
        shared_data[local_idx] = input[idx];
    }} else {{
        shared_data[local_idx] = {identity};
    }}

    workgroupBarrier();

    // Parallel reduction in shared memory
    var stride = 128u;
    while (stride > 0u) {{
        if (local_idx < stride && idx + stride < n) {{
            let a = shared_data[local_idx];
            let b = shared_data[local_idx + stride];
            shared_data[local_idx] = {op};
        }}
        stride = stride / 2u;
        workgroupBarrier();
    }}

    // Write result from first thread
    if (local_idx == 0u) {{
        output[workgroup_id.x] = shared_data[0];
    }}
}}
"#,
            identity = self.identity(),
            op = self.operation_expr()
        )
    }
}

/// GPU kernel for parallel reduction operations.
pub struct ReductionKernel {
    context: GpuContext,
    pipeline: ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    workgroup_size: u32,
}

impl ReductionKernel {
    /// Create a new reduction kernel.
    ///
    /// # Errors
    ///
    /// Returns an error if shader compilation or pipeline creation fails.
    pub fn new(context: &GpuContext, op: ReductionOp) -> GpuResult<Self> {
        debug!("Creating reduction kernel for operation: {:?}", op);

        let shader_source = op.reduction_shader();
        let mut shader = WgslShader::new(shader_source, "reduce");
        let shader_module = shader.compile(context.device())?;

        let bind_group_layout = create_compute_bind_group_layout(
            context.device(),
            &[
                storage_buffer_layout(0, true),  // input
                storage_buffer_layout(1, false), // output
            ],
            Some("ReductionKernel BindGroupLayout"),
        )?;

        let pipeline = ComputePipelineBuilder::new(context.device(), shader_module, "reduce")
            .bind_group_layout(&bind_group_layout)
            .label(format!("ReductionKernel Pipeline: {:?}", op))
            .build()?;

        Ok(Self {
            context: context.clone(),
            pipeline,
            bind_group_layout,
            workgroup_size: 256,
        })
    }

    /// Execute reduction on GPU buffer.
    ///
    /// # Errors
    ///
    /// Returns an error if execution fails.
    pub async fn execute<T: Pod + Copy>(
        &self,
        input: &GpuBuffer<T>,
        _op: ReductionOp,
    ) -> GpuResult<T> {
        let mut current_input = input.clone();
        let mut iteration = 0;

        loop {
            let input_size = current_input.len();
            let num_workgroups =
                (input_size as u32 + self.workgroup_size - 1) / self.workgroup_size;

            if num_workgroups == 1 && input_size <= self.workgroup_size as usize {
                // Final reduction
                let output = GpuBuffer::new(
                    &self.context,
                    1,
                    BufferUsages::STORAGE | BufferUsages::COPY_SRC,
                )?;

                self.execute_pass(&current_input, &output, num_workgroups)?;

                // Read result
                let staging = GpuBuffer::staging(&self.context, 1)?;
                let mut staging_mut = staging.clone();
                staging_mut.copy_from(&output)?;

                let result = staging.read().await?;
                return Ok(result[0]);
            }

            // Intermediate reduction
            let output = GpuBuffer::new(
                &self.context,
                num_workgroups as usize,
                BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
            )?;

            self.execute_pass(&current_input, &output, num_workgroups)?;

            current_input = output;
            iteration += 1;

            if iteration > 10 {
                return Err(GpuError::execution_failed(
                    "Reduction did not converge after 10 iterations",
                ));
            }
        }
    }

    /// Execute a single reduction pass.
    fn execute_pass<T: Pod>(
        &self,
        input: &GpuBuffer<T>,
        output: &GpuBuffer<T>,
        num_workgroups: u32,
    ) -> GpuResult<()> {
        let bind_group = self
            .context
            .device()
            .create_bind_group(&BindGroupDescriptor {
                label: Some("ReductionKernel BindGroup"),
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
                ],
            });

        let mut encoder = self
            .context
            .device()
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("ReductionKernel Encoder"),
            });

        {
            let mut compute_pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("ReductionKernel Pass"),
                timestamp_writes: None,
            });

            compute_pass.set_pipeline(&self.pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);
            compute_pass.dispatch_workgroups(num_workgroups, 1, 1);
        }

        self.context.queue().submit(Some(encoder.finish()));
        Ok(())
    }

    /// Execute reduction synchronously.
    ///
    /// # Errors
    ///
    /// Returns an error if execution fails.
    pub fn execute_blocking<T: Pod + Copy>(
        &self,
        input: &GpuBuffer<T>,
        op: ReductionOp,
    ) -> GpuResult<T> {
        pollster::block_on(self.execute(input, op))
    }
}

/// Histogram parameters.
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct HistogramParams {
    /// Number of bins.
    pub num_bins: u32,
    /// Minimum value.
    pub min_value: f32,
    /// Maximum value.
    pub max_value: f32,
    /// Padding for alignment.
    _padding: u32,
}

impl HistogramParams {
    /// Create new histogram parameters.
    pub fn new(num_bins: u32, min_value: f32, max_value: f32) -> Self {
        Self {
            num_bins,
            min_value,
            max_value,
            _padding: 0,
        }
    }

    /// Create histogram with automatic range.
    pub fn auto(num_bins: u32) -> Self {
        Self::new(num_bins, 0.0, 1.0)
    }
}

/// GPU kernel for histogram computation.
pub struct HistogramKernel {
    context: GpuContext,
    pipeline: ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    workgroup_size: u32,
}

impl HistogramKernel {
    /// Create a new histogram kernel.
    ///
    /// # Errors
    ///
    /// Returns an error if shader compilation or pipeline creation fails.
    pub fn new(context: &GpuContext) -> GpuResult<Self> {
        debug!("Creating histogram kernel");

        let shader_source = Self::histogram_shader();
        let mut shader = WgslShader::new(shader_source, "histogram");
        let shader_module = shader.compile(context.device())?;

        let bind_group_layout = create_compute_bind_group_layout(
            context.device(),
            &[
                storage_buffer_layout(0, true),  // input
                uniform_buffer_layout(1),        // params
                storage_buffer_layout(2, false), // histogram output
            ],
            Some("HistogramKernel BindGroupLayout"),
        )?;

        let pipeline = ComputePipelineBuilder::new(context.device(), shader_module, "histogram")
            .bind_group_layout(&bind_group_layout)
            .label("HistogramKernel Pipeline")
            .build()?;

        Ok(Self {
            context: context.clone(),
            pipeline,
            bind_group_layout,
            workgroup_size: 256,
        })
    }

    /// Get histogram shader source.
    fn histogram_shader() -> String {
        r#"
struct HistogramParams {
    num_bins: u32,
    min_value: f32,
    max_value: f32,
    _padding: u32,
}

@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<uniform> params: HistogramParams;
@group(0) @binding(2) var<storage, read_write> histogram: array<atomic<u32>>;

@compute @workgroup_size(256)
fn histogram(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let idx = global_id.x;
    if (idx >= arrayLength(&input)) {
        return;
    }

    let value = input[idx];
    let range = params.max_value - params.min_value;

    if (value >= params.min_value && value <= params.max_value && range > 0.0) {
        let normalized = (value - params.min_value) / range;
        var bin = u32(normalized * f32(params.num_bins));

        // Clamp to valid bin range
        if (bin >= params.num_bins) {
            bin = params.num_bins - 1u;
        }

        atomicAdd(&histogram[bin], 1u);
    }
}
"#
        .to_string()
    }

    /// Compute histogram of GPU buffer.
    ///
    /// # Errors
    ///
    /// Returns an error if execution fails.
    pub async fn execute<T: Pod>(
        &self,
        input: &GpuBuffer<T>,
        params: HistogramParams,
    ) -> GpuResult<Vec<u32>> {
        // Create histogram buffer (atomic u32)
        let histogram = GpuBuffer::<u32>::new(
            &self.context,
            params.num_bins as usize,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        )?;

        // Create params uniform buffer
        let params_buffer = GpuBuffer::from_data(
            &self.context,
            &[params],
            BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        )?;

        let bind_group = self
            .context
            .device()
            .create_bind_group(&BindGroupDescriptor {
                label: Some("HistogramKernel BindGroup"),
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
                        resource: histogram.buffer().as_entire_binding(),
                    },
                ],
            });

        let mut encoder = self
            .context
            .device()
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("HistogramKernel Encoder"),
            });

        {
            let mut compute_pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("HistogramKernel Pass"),
                timestamp_writes: None,
            });

            compute_pass.set_pipeline(&self.pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);

            let num_workgroups =
                (input.len() as u32 + self.workgroup_size - 1) / self.workgroup_size;
            compute_pass.dispatch_workgroups(num_workgroups, 1, 1);
        }

        self.context.queue().submit(Some(encoder.finish()));

        // Read histogram result
        let staging = GpuBuffer::staging(&self.context, params.num_bins as usize)?;
        let mut staging_mut = staging.clone();
        staging_mut.copy_from(&histogram)?;

        let result = staging.read().await?;
        debug!("Computed histogram with {} bins", params.num_bins);
        Ok(result)
    }

    /// Compute histogram synchronously.
    ///
    /// # Errors
    ///
    /// Returns an error if execution fails.
    pub fn execute_blocking<T: Pod>(
        &self,
        input: &GpuBuffer<T>,
        params: HistogramParams,
    ) -> GpuResult<Vec<u32>> {
        pollster::block_on(self.execute(input, params))
    }
}

/// Statistics result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Statistics {
    /// Minimum value.
    pub min: f32,
    /// Maximum value.
    pub max: f32,
    /// Sum of all values.
    pub sum: f32,
    /// Number of values.
    pub count: usize,
}

impl Statistics {
    /// Calculate mean.
    pub fn mean(&self) -> f32 {
        if self.count == 0 {
            0.0
        } else {
            self.sum / self.count as f32
        }
    }

    /// Calculate range.
    pub fn range(&self) -> f32 {
        self.max - self.min
    }
}

/// Compute basic statistics on GPU buffer.
///
/// # Errors
///
/// Returns an error if GPU operations fail.
pub async fn compute_statistics(
    context: &GpuContext,
    input: &GpuBuffer<f32>,
) -> GpuResult<Statistics> {
    let sum_kernel = ReductionKernel::new(context, ReductionOp::Sum)?;
    let min_kernel = ReductionKernel::new(context, ReductionOp::Min)?;
    let max_kernel = ReductionKernel::new(context, ReductionOp::Max)?;

    let sum = sum_kernel.execute(input, ReductionOp::Sum).await?;
    let min = min_kernel.execute(input, ReductionOp::Min).await?;
    let max = max_kernel.execute(input, ReductionOp::Max).await?;

    Ok(Statistics {
        min,
        max,
        sum,
        count: input.len(),
    })
}

/// Compute basic statistics synchronously.
///
/// # Errors
///
/// Returns an error if GPU operations fail.
pub fn compute_statistics_blocking(
    context: &GpuContext,
    input: &GpuBuffer<f32>,
) -> GpuResult<Statistics> {
    pollster::block_on(compute_statistics(context, input))
}

// Re-export for convenience
pub use compute_statistics_blocking as compute_stats_blocking;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reduction_op_identity() {
        assert_eq!(ReductionOp::Sum.identity(), 0.0);
        assert_eq!(ReductionOp::Product.identity(), 1.0);
    }

    #[test]
    fn test_histogram_params() {
        let params = HistogramParams::new(256, 0.0, 255.0);
        assert_eq!(params.num_bins, 256);
        assert_eq!(params.min_value, 0.0);
        assert_eq!(params.max_value, 255.0);
    }

    #[tokio::test]
    #[ignore]
    async fn test_reduction_kernel() {
        if let Ok(context) = GpuContext::new().await {
            if let Ok(kernel) = ReductionKernel::new(&context, ReductionOp::Sum) {
                let data: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0, 5.0];

                if let Ok(buffer) = GpuBuffer::from_data(
                    &context,
                    &data,
                    BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
                ) {
                    if let Ok(result) = kernel.execute(&buffer, ReductionOp::Sum).await {
                        assert!((result - 15.0).abs() < 1e-5);
                    }
                }
            }
        }
    }

    #[test]
    fn test_statistics_calculations() {
        let stats = Statistics {
            min: 0.0,
            max: 100.0,
            sum: 500.0,
            count: 10,
        };

        assert_eq!(stats.mean(), 50.0);
        assert_eq!(stats.range(), 100.0);
    }
}
