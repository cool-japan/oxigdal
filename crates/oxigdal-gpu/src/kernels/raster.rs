//! GPU kernels for element-wise raster operations.
//!
//! This module provides GPU-accelerated element-wise operations on rasters,
//! including arithmetic, logical, and transformation operations.

use crate::buffer::GpuBuffer;
use crate::context::GpuContext;
use crate::error::{GpuError, GpuResult};
use crate::shaders::{
    ComputePipelineBuilder, WgslShader, create_compute_bind_group_layout, storage_buffer_layout,
};
use bytemuck::Pod;
use tracing::debug;
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, CommandEncoderDescriptor,
    ComputePassDescriptor, ComputePipeline,
};

/// Element-wise operation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementWiseOp {
    /// Addition: a + b
    Add,
    /// Subtraction: a - b
    Subtract,
    /// Multiplication: a * b
    Multiply,
    /// Division: a / b
    Divide,
    /// Power: a ^ b
    Power,
    /// Minimum: min(a, b)
    Min,
    /// Maximum: max(a, b)
    Max,
    /// Modulo: a % b
    Modulo,
}

impl ElementWiseOp {
    /// Get the WGSL shader source for this operation.
    fn shader_source(&self) -> &'static str {
        match self {
            Self::Add => include_str!("shaders/add.wgsl"),
            Self::Subtract => include_str!("shaders/subtract.wgsl"),
            Self::Multiply => include_str!("shaders/multiply.wgsl"),
            Self::Divide => include_str!("shaders/divide.wgsl"),
            Self::Power => include_str!("shaders/power.wgsl"),
            Self::Min => include_str!("shaders/min.wgsl"),
            Self::Max => include_str!("shaders/max.wgsl"),
            Self::Modulo => include_str!("shaders/modulo.wgsl"),
        }
    }

    /// Get the entry point name for this operation.
    fn entry_point(&self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Subtract => "subtract",
            Self::Multiply => "multiply",
            Self::Divide => "divide",
            Self::Power => "power",
            Self::Min => "min_op",
            Self::Max => "max_op",
            Self::Modulo => "modulo",
        }
    }

    /// Get a fallback inline shader if external shader not available.
    fn inline_shader(&self) -> String {
        let op_expr = match self {
            Self::Add => "input_a[idx] + input_b[idx]",
            Self::Subtract => "input_a[idx] - input_b[idx]",
            Self::Multiply => "input_a[idx] * input_b[idx]",
            Self::Divide => "safe_div(input_a[idx], input_b[idx])",
            Self::Power => "pow(input_a[idx], input_b[idx])",
            Self::Min => "min(input_a[idx], input_b[idx])",
            Self::Max => "max(input_a[idx], input_b[idx])",
            Self::Modulo => "input_a[idx] % input_b[idx]",
        };

        format!(
            r#"
@group(0) @binding(0) var<storage, read> input_a: array<f32>;
@group(0) @binding(1) var<storage, read> input_b: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;

fn safe_div(num: f32, denom: f32) -> f32 {{
    if (abs(denom) < 1e-10) {{
        return 0.0;
    }}
    return num / denom;
}}

@compute @workgroup_size(256)
fn {entry}(@builtin(global_invocation_id) global_id: vec3<u32>) {{
    let idx = global_id.x;
    if (idx >= arrayLength(&output)) {{
        return;
    }}
    output[idx] = {op_expr};
}}
"#,
            entry = self.entry_point(),
            op_expr = op_expr
        )
    }
}

/// GPU kernel for element-wise raster operations.
pub struct RasterKernel {
    context: GpuContext,
    pipeline: ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    workgroup_size: u32,
}

impl RasterKernel {
    /// Create a new raster kernel for the specified operation.
    ///
    /// # Errors
    ///
    /// Returns an error if shader compilation or pipeline creation fails.
    pub fn new(context: &GpuContext, op: ElementWiseOp) -> GpuResult<Self> {
        debug!("Creating raster kernel for operation: {:?}", op);

        // Create shader - use inline shader as fallback
        let shader_source = op.inline_shader();
        let mut shader = WgslShader::new(shader_source, op.entry_point());
        let shader_module = shader.compile(context.device())?;

        // Create bind group layout
        let bind_group_layout = create_compute_bind_group_layout(
            context.device(),
            &[
                storage_buffer_layout(0, true),  // input_a (read-only)
                storage_buffer_layout(1, true),  // input_b (read-only)
                storage_buffer_layout(2, false), // output (read-write)
            ],
            Some("RasterKernel BindGroupLayout"),
        )?;

        // Create pipeline
        let pipeline =
            ComputePipelineBuilder::new(context.device(), shader_module, op.entry_point())
                .bind_group_layout(&bind_group_layout)
                .label(format!("RasterKernel Pipeline: {:?}", op))
                .build()?;

        Ok(Self {
            context: context.clone(),
            pipeline,
            bind_group_layout,
            workgroup_size: 256,
        })
    }

    /// Execute the kernel on GPU buffers.
    ///
    /// # Errors
    ///
    /// Returns an error if buffer sizes don't match or execution fails.
    pub fn execute<T: Pod>(
        &self,
        input_a: &GpuBuffer<T>,
        input_b: &GpuBuffer<T>,
        output: &mut GpuBuffer<T>,
    ) -> GpuResult<()> {
        // Validate buffer sizes
        if input_a.len() != input_b.len() || input_a.len() != output.len() {
            return Err(GpuError::invalid_kernel_params(format!(
                "Buffer size mismatch: {} != {} != {}",
                input_a.len(),
                input_b.len(),
                output.len()
            )));
        }

        let num_elements = input_a.len();

        // Create bind group
        let bind_group = self.create_bind_group(input_a, input_b, output)?;

        // Create command encoder
        let mut encoder = self
            .context
            .device()
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("RasterKernel Encoder"),
            });

        // Compute pass
        {
            let mut compute_pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("RasterKernel Pass"),
                timestamp_writes: None,
            });

            compute_pass.set_pipeline(&self.pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);

            let workgroup_count =
                (num_elements as u32 + self.workgroup_size - 1) / self.workgroup_size;
            compute_pass.dispatch_workgroups(workgroup_count, 1, 1);
        }

        // Submit commands
        self.context.queue().submit(Some(encoder.finish()));

        debug!("Executed raster kernel on {} elements", num_elements);
        Ok(())
    }

    /// Create bind group for kernel execution.
    fn create_bind_group<T: Pod>(
        &self,
        input_a: &GpuBuffer<T>,
        input_b: &GpuBuffer<T>,
        output: &GpuBuffer<T>,
    ) -> GpuResult<BindGroup> {
        let bind_group = self
            .context
            .device()
            .create_bind_group(&BindGroupDescriptor {
                label: Some("RasterKernel BindGroup"),
                layout: &self.bind_group_layout,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource: input_a.buffer().as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: input_b.buffer().as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 2,
                        resource: output.buffer().as_entire_binding(),
                    },
                ],
            });

        Ok(bind_group)
    }
}

/// Unary operation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    /// Negate: -a
    Negate,
    /// Absolute value: |a|
    Abs,
    /// Square root: √a
    Sqrt,
    /// Square: a²
    Square,
    /// Natural logarithm: ln(a)
    Log,
    /// Exponential: e^a
    Exp,
    /// Sine: sin(a)
    Sin,
    /// Cosine: cos(a)
    Cos,
    /// Tangent: tan(a)
    Tan,
}

impl UnaryOp {
    /// Get inline shader for this operation.
    fn inline_shader(&self) -> String {
        let op_expr = match self {
            Self::Negate => "-input[idx]",
            Self::Abs => "abs(input[idx])",
            Self::Sqrt => "sqrt(max(input[idx], 0.0))",
            Self::Square => "input[idx] * input[idx]",
            Self::Log => "log(max(input[idx], 1e-10))",
            Self::Exp => "exp(input[idx])",
            Self::Sin => "sin(input[idx])",
            Self::Cos => "cos(input[idx])",
            Self::Tan => "tan(input[idx])",
        };

        format!(
            r#"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(256)
fn unary_op(@builtin(global_invocation_id) global_id: vec3<u32>) {{
    let idx = global_id.x;
    if (idx >= arrayLength(&output)) {{
        return;
    }}
    output[idx] = {op_expr};
}}
"#,
            op_expr = op_expr
        )
    }
}

/// GPU kernel for unary raster operations.
pub struct UnaryKernel {
    context: GpuContext,
    pipeline: ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    workgroup_size: u32,
}

impl UnaryKernel {
    /// Create a new unary kernel for the specified operation.
    ///
    /// # Errors
    ///
    /// Returns an error if shader compilation or pipeline creation fails.
    pub fn new(context: &GpuContext, op: UnaryOp) -> GpuResult<Self> {
        debug!("Creating unary kernel for operation: {:?}", op);

        let shader_source = op.inline_shader();
        let mut shader = WgslShader::new(shader_source, "unary_op");
        let shader_module = shader.compile(context.device())?;

        let bind_group_layout = create_compute_bind_group_layout(
            context.device(),
            &[
                storage_buffer_layout(0, true),  // input (read-only)
                storage_buffer_layout(1, false), // output (read-write)
            ],
            Some("UnaryKernel BindGroupLayout"),
        )?;

        let pipeline = ComputePipelineBuilder::new(context.device(), shader_module, "unary_op")
            .bind_group_layout(&bind_group_layout)
            .label(format!("UnaryKernel Pipeline: {:?}", op))
            .build()?;

        Ok(Self {
            context: context.clone(),
            pipeline,
            bind_group_layout,
            workgroup_size: 256,
        })
    }

    /// Execute the kernel on GPU buffer.
    ///
    /// # Errors
    ///
    /// Returns an error if buffer sizes don't match or execution fails.
    pub fn execute<T: Pod>(
        &self,
        input: &GpuBuffer<T>,
        output: &mut GpuBuffer<T>,
    ) -> GpuResult<()> {
        if input.len() != output.len() {
            return Err(GpuError::invalid_kernel_params(format!(
                "Buffer size mismatch: {} != {}",
                input.len(),
                output.len()
            )));
        }

        let num_elements = input.len();

        let bind_group = self
            .context
            .device()
            .create_bind_group(&BindGroupDescriptor {
                label: Some("UnaryKernel BindGroup"),
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
                label: Some("UnaryKernel Encoder"),
            });

        {
            let mut compute_pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("UnaryKernel Pass"),
                timestamp_writes: None,
            });

            compute_pass.set_pipeline(&self.pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);

            let workgroup_count =
                (num_elements as u32 + self.workgroup_size - 1) / self.workgroup_size;
            compute_pass.dispatch_workgroups(workgroup_count, 1, 1);
        }

        self.context.queue().submit(Some(encoder.finish()));

        debug!("Executed unary kernel on {} elements", num_elements);
        Ok(())
    }
}

/// Scalar operation type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScalarOp {
    /// Add scalar: a + c
    Add(f32),
    /// Multiply by scalar: a * c
    Multiply(f32),
    /// Clamp to range: clamp(a, min, max)
    Clamp { min: f32, max: f32 },
    /// Threshold: a > threshold ? above : below
    Threshold {
        threshold: f32,
        above: f32,
        below: f32,
    },
}

impl ScalarOp {
    /// Get inline shader for this operation.
    fn inline_shader(&self) -> String {
        match self {
            Self::Add(value) => format!(
                r#"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(256)
fn scalar_op(@builtin(global_invocation_id) global_id: vec3<u32>) {{
    let idx = global_id.x;
    if (idx >= arrayLength(&output)) {{
        return;
    }}
    output[idx] = input[idx] + {value};
}}
"#,
                value = value
            ),
            Self::Multiply(value) => format!(
                r#"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(256)
fn scalar_op(@builtin(global_invocation_id) global_id: vec3<u32>) {{
    let idx = global_id.x;
    if (idx >= arrayLength(&output)) {{
        return;
    }}
    output[idx] = input[idx] * {value};
}}
"#,
                value = value
            ),
            Self::Clamp { min, max } => format!(
                r#"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(256)
fn scalar_op(@builtin(global_invocation_id) global_id: vec3<u32>) {{
    let idx = global_id.x;
    if (idx >= arrayLength(&output)) {{
        return;
    }}
    output[idx] = clamp(input[idx], {min}, {max});
}}
"#,
                min = min,
                max = max
            ),
            Self::Threshold {
                threshold,
                above,
                below,
            } => format!(
                r#"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(256)
fn scalar_op(@builtin(global_invocation_id) global_id: vec3<u32>) {{
    let idx = global_id.x;
    if (idx >= arrayLength(&output)) {{
        return;
    }}
    if (input[idx] > {threshold}) {{
        output[idx] = {above};
    }} else {{
        output[idx] = {below};
    }}
}}
"#,
                threshold = threshold,
                above = above,
                below = below
            ),
        }
    }
}

/// GPU kernel for scalar raster operations.
pub struct ScalarKernel {
    context: GpuContext,
    pipeline: ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    workgroup_size: u32,
}

impl ScalarKernel {
    /// Create a new scalar kernel for the specified operation.
    ///
    /// # Errors
    ///
    /// Returns an error if shader compilation or pipeline creation fails.
    pub fn new(context: &GpuContext, op: ScalarOp) -> GpuResult<Self> {
        debug!("Creating scalar kernel for operation: {:?}", op);

        let shader_source = op.inline_shader();
        let mut shader = WgslShader::new(shader_source, "scalar_op");
        let shader_module = shader.compile(context.device())?;

        let bind_group_layout = create_compute_bind_group_layout(
            context.device(),
            &[
                storage_buffer_layout(0, true),  // input (read-only)
                storage_buffer_layout(1, false), // output (read-write)
            ],
            Some("ScalarKernel BindGroupLayout"),
        )?;

        let pipeline = ComputePipelineBuilder::new(context.device(), shader_module, "scalar_op")
            .bind_group_layout(&bind_group_layout)
            .label(format!("ScalarKernel Pipeline: {:?}", op))
            .build()?;

        Ok(Self {
            context: context.clone(),
            pipeline,
            bind_group_layout,
            workgroup_size: 256,
        })
    }

    /// Execute the kernel on GPU buffer.
    ///
    /// # Errors
    ///
    /// Returns an error if execution fails.
    pub fn execute<T: Pod>(
        &self,
        input: &GpuBuffer<T>,
        output: &mut GpuBuffer<T>,
    ) -> GpuResult<()> {
        if input.len() != output.len() {
            return Err(GpuError::invalid_kernel_params(format!(
                "Buffer size mismatch: {} != {}",
                input.len(),
                output.len()
            )));
        }

        let num_elements = input.len();

        let bind_group = self
            .context
            .device()
            .create_bind_group(&BindGroupDescriptor {
                label: Some("ScalarKernel BindGroup"),
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
                label: Some("ScalarKernel Encoder"),
            });

        {
            let mut compute_pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("ScalarKernel Pass"),
                timestamp_writes: None,
            });

            compute_pass.set_pipeline(&self.pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);

            let workgroup_count =
                (num_elements as u32 + self.workgroup_size - 1) / self.workgroup_size;
            compute_pass.dispatch_workgroups(workgroup_count, 1, 1);
        }

        self.context.queue().submit(Some(encoder.finish()));

        debug!("Executed scalar kernel on {} elements", num_elements);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_element_wise_op_shader() {
        let op = ElementWiseOp::Add;
        let shader = op.inline_shader();
        assert!(shader.contains("@compute"));
        assert!(shader.contains("add"));
    }

    #[test]
    fn test_unary_op_shader() {
        let op = UnaryOp::Sqrt;
        let shader = op.inline_shader();
        assert!(shader.contains("sqrt"));
    }

    #[test]
    fn test_scalar_op_shader() {
        let op = ScalarOp::Add(5.0);
        let shader = op.inline_shader();
        assert!(shader.contains("5"));
    }

    #[tokio::test]
    async fn test_raster_kernel_execution() {
        if let Ok(context) = GpuContext::new().await {
            let kernel = RasterKernel::new(&context, ElementWiseOp::Add);
            if let Ok(_kernel) = kernel {
                // Kernel created successfully
            }
        }
    }
}
