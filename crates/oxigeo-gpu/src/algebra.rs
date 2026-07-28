//! GPU-accelerated raster algebra operations.
//!
//! Provides element-wise band math operations (`AlgebraOp`), a structured
//! expression tree (`BandExpression`) for composing multi-band formulas, and
//! the top-level `GpuAlgebra` driver. [`GpuAlgebra::execute`] runs a pure-Rust
//! CPU path; [`GpuAlgebra::execute_gpu`] dispatches the same operations to a
//! wgpu compute shader and is numerically equivalent within floating-point
//! tolerance.

use crate::buffer::GpuBuffer;
use crate::context::GpuContext;
use crate::error::GpuError;
use crate::shaders::{
    ComputePipelineBuilder, WgslShader, create_compute_bind_group_layout, storage_buffer_layout,
    uniform_buffer_layout,
};
use bytemuck::{Pod, Zeroable};
use wgpu::{
    BindGroupDescriptor, BindGroupEntry, BufferUsages, CommandEncoderDescriptor,
    ComputePassDescriptor,
};

/// WGSL source for the raster-algebra compute shader.
const RASTER_ALGEBRA_SHADER: &str = include_str!("shaders/raster_algebra.wgsl");

/// Host-side mirror of the `AlgebraParams` uniform in `raster_algebra.wgsl`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct AlgebraParamsGpu {
    width: u32,
    height: u32,
    operation: u32,
    use_nodata: u32,
    has_b: u32,
    _p0: u32,
    _p1: u32,
    _p2: u32,
    nodata_a: f32,
    nodata_b: f32,
    output_nodata: f32,
    scalar0: f32,
    scalar1: f32,
    scalar2: f32,
    scalar3: f32,
    _p3: f32,
}

/// Element-wise raster algebra operation.
#[derive(Debug, Clone, PartialEq)]
pub enum AlgebraOp {
    /// `A + B`
    Add,
    /// `A - B`
    Subtract,
    /// `A * B`
    Multiply,
    /// `A / B` (outputs nodata when `|B| < 1e-10`)
    Divide,
    /// `min(A, B)`
    Min,
    /// `max(A, B)`
    Max,
    /// `sqrt(max(0, A))`
    Sqrt,
    /// `|A|`
    Abs,
    /// `A ^ exp`
    Power(f32),
    /// `clamp(A, min, max)`
    Clamp { min: f32, max: f32 },
    /// Linear stretch: maps `[src_min, src_max]` → `[dst_min, dst_max]`
    Normalize {
        src_min: f32,
        src_max: f32,
        dst_min: f32,
        dst_max: f32,
    },
}

/// Pure-Rust raster algebra executor.
pub struct GpuAlgebra;

impl GpuAlgebra {
    /// Execute an algebra operation pixel-by-pixel (CPU fallback).
    ///
    /// `band_b` is required for binary operations (`Add`, `Subtract`,
    /// `Multiply`, `Divide`, `Min`, `Max`).  For unary operations it is
    /// ignored.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError::InvalidKernelParams`] if `band_a` is empty.
    pub fn execute(
        band_a: &[f32],
        band_b: Option<&[f32]>,
        op: AlgebraOp,
        nodata: Option<f32>,
    ) -> Result<Vec<f32>, GpuError> {
        if band_a.is_empty() {
            return Err(GpuError::invalid_kernel_params("band_a must not be empty"));
        }

        let nodata_val = nodata.unwrap_or(f32::NAN);
        let mut output = Vec::with_capacity(band_a.len());

        for (i, &a) in band_a.iter().enumerate() {
            // Nodata check for band A.
            if nodata.is_some() && Self::is_nodata(a, nodata_val) {
                output.push(nodata_val);
                continue;
            }

            let b = band_b.and_then(|bb| bb.get(i)).copied().unwrap_or(0.0_f32);

            // Nodata check for band B.
            if nodata.is_some() && band_b.is_some() && Self::is_nodata(b, nodata_val) {
                output.push(nodata_val);
                continue;
            }

            let result = match &op {
                AlgebraOp::Add => a + b,
                AlgebraOp::Subtract => a - b,
                AlgebraOp::Multiply => a * b,
                AlgebraOp::Divide => {
                    if b.abs() > 1e-10 {
                        a / b
                    } else {
                        nodata_val
                    }
                }
                AlgebraOp::Min => a.min(b),
                AlgebraOp::Max => a.max(b),
                AlgebraOp::Sqrt => a.max(0.0).sqrt(),
                AlgebraOp::Abs => a.abs(),
                AlgebraOp::Power(exp) => a.powf(*exp),
                AlgebraOp::Clamp { min, max } => a.clamp(*min, *max),
                AlgebraOp::Normalize {
                    src_min,
                    src_max,
                    dst_min,
                    dst_max,
                } => {
                    let range = src_max - src_min;
                    if range.abs() < 1e-10 {
                        *dst_min
                    } else {
                        (a - src_min) / range * (dst_max - dst_min) + dst_min
                    }
                }
            };

            output.push(result);
        }

        Ok(output)
    }

    /// Evaluate a multi-band expression for every pixel.
    ///
    /// All bands in `bands` must have the same length.  Pixels where any
    /// band holds the nodata value are written as nodata without evaluating
    /// the expression.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError::InvalidKernelParams`] if no bands are provided.
    /// Propagates any error from `expression.evaluate`.
    pub fn evaluate_expression(
        bands: &[&[f32]],
        expression: &BandExpression,
        nodata: Option<f32>,
    ) -> Result<Vec<f32>, GpuError> {
        if bands.is_empty() {
            return Err(GpuError::invalid_kernel_params("no bands provided"));
        }

        let len = bands[0].len();
        let nodata_val = nodata.unwrap_or(f32::NAN);

        let mut output = Vec::with_capacity(len);
        for i in 0..len {
            // Check nodata across all bands.
            let has_nodata = nodata.is_some()
                && bands.iter().any(|b| {
                    b.get(i)
                        .map(|v| Self::is_nodata(*v, nodata_val))
                        .unwrap_or(false)
                });

            if has_nodata {
                output.push(nodata_val);
                continue;
            }

            let vals: Vec<f32> = bands
                .iter()
                .map(|b| b.get(i).copied().unwrap_or(0.0))
                .collect();
            output.push(expression.evaluate(&vals)?);
        }

        Ok(output)
    }

    #[inline]
    fn is_nodata(value: f32, nodata: f32) -> bool {
        (value - nodata).abs() < 1e-6
    }

    /// Encode an [`AlgebraOp`] into the GPU operation code, its "binary"
    /// flag (whether `band_b` is consumed), and up to four scalar operands.
    fn op_encoding(op: &AlgebraOp) -> (u32, bool, [f32; 4]) {
        match op {
            AlgebraOp::Add => (0, true, [0.0; 4]),
            AlgebraOp::Subtract => (1, true, [0.0; 4]),
            AlgebraOp::Multiply => (2, true, [0.0; 4]),
            AlgebraOp::Divide => (3, true, [0.0; 4]),
            AlgebraOp::Min => (4, true, [0.0; 4]),
            AlgebraOp::Max => (5, true, [0.0; 4]),
            AlgebraOp::Sqrt => (6, false, [0.0; 4]),
            AlgebraOp::Abs => (7, false, [0.0; 4]),
            AlgebraOp::Power(exp) => (8, false, [*exp, 0.0, 0.0, 0.0]),
            AlgebraOp::Clamp { min, max } => (9, false, [*min, *max, 0.0, 0.0]),
            AlgebraOp::Normalize {
                src_min,
                src_max,
                dst_min,
                dst_max,
            } => (10, false, [*src_min, *src_max, *dst_min, *dst_max]),
        }
    }

    /// Execute an algebra operation on the GPU via a wgpu compute dispatch.
    ///
    /// This mirrors [`execute`](Self::execute) exactly: the same operations,
    /// the same nodata masking (`1e-6` threshold), and the same divide-by-zero
    /// guard (`|b| <= 1e-10 → nodata`). Binary operations (`Add`, `Subtract`,
    /// `Multiply`, `Divide`, `Min`, `Max`) require `band_b`; unary operations
    /// ignore it.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError::InvalidKernelParams`] if `band_a` is empty, if a
    /// binary op is requested without a matching-length `band_b`, or if shader
    /// / pipeline creation, dispatch, or read-back fails.
    pub async fn execute_gpu(
        context: &GpuContext,
        band_a: &[f32],
        band_b: Option<&[f32]>,
        op: AlgebraOp,
        nodata: Option<f32>,
    ) -> Result<Vec<f32>, GpuError> {
        if band_a.is_empty() {
            return Err(GpuError::invalid_kernel_params("band_a must not be empty"));
        }

        let (operation, is_binary, scalars) = Self::op_encoding(&op);

        // Prepare band_b: real data for binary ops, a zeroed placeholder for
        // unary ops so the shader binding is always valid.
        let b_data: Vec<f32> = if is_binary {
            let b = band_b.ok_or_else(|| {
                GpuError::invalid_kernel_params(format!(
                    "operation {:?} is binary and requires band_b",
                    op
                ))
            })?;
            if b.len() != band_a.len() {
                return Err(GpuError::invalid_kernel_params(format!(
                    "band_b length {} does not match band_a length {}",
                    b.len(),
                    band_a.len()
                )));
            }
            b.to_vec()
        } else {
            vec![0.0_f32; band_a.len()]
        };

        let nodata_val = nodata.unwrap_or(f32::NAN);
        let params = AlgebraParamsGpu {
            width: band_a.len() as u32,
            height: 1,
            operation,
            use_nodata: nodata.is_some() as u32,
            has_b: is_binary as u32,
            _p0: 0,
            _p1: 0,
            _p2: 0,
            nodata_a: nodata_val,
            nodata_b: nodata_val,
            output_nodata: nodata_val,
            scalar0: scalars[0],
            scalar1: scalars[1],
            scalar2: scalars[2],
            scalar3: scalars[3],
            _p3: 0.0,
        };

        let a_buffer = GpuBuffer::from_data(
            context,
            band_a,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        )?;
        let b_buffer = GpuBuffer::from_data(
            context,
            &b_data,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        )?;
        let out_buffer = GpuBuffer::<f32>::new(
            context,
            band_a.len(),
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        )?;
        let params_buffer = GpuBuffer::from_data(
            context,
            &[params],
            BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        )?;

        let mut shader = WgslShader::new(RASTER_ALGEBRA_SHADER, "main");
        let module = shader.compile(context.device())?;

        let bind_group_layout = create_compute_bind_group_layout(
            context.device(),
            &[
                uniform_buffer_layout(0),
                storage_buffer_layout(1, true),
                storage_buffer_layout(2, true),
                storage_buffer_layout(3, false),
            ],
            Some("RasterAlgebra BindGroupLayout"),
        )?;

        let pipeline = ComputePipelineBuilder::new(context.device(), module, "main")
            .bind_group_layout(&bind_group_layout)
            .label("RasterAlgebra Pipeline")
            .build()?;

        let bind_group = context.device().create_bind_group(&BindGroupDescriptor {
            label: Some("RasterAlgebra BindGroup"),
            layout: &bind_group_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: params_buffer.buffer().as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: a_buffer.buffer().as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: b_buffer.buffer().as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: out_buffer.buffer().as_entire_binding(),
                },
            ],
        });

        let mut encoder = context
            .device()
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("RasterAlgebra Encoder"),
            });

        {
            let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("RasterAlgebra Pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            let groups = (band_a.len() as u32).div_ceil(256);
            pass.dispatch_workgroups(groups, 1, 1);
        }

        context.check_device_lost()?;
        context.queue().submit(Some(encoder.finish()));

        // `out_buffer` is a STORAGE buffer and cannot be host-mapped directly
        // (wgpu forbids `STORAGE | MAP_READ`). Copy it into a MAP_READ staging
        // buffer, then read that back to the host.
        let mut staging = GpuBuffer::<f32>::staging(context, band_a.len())?;
        staging.copy_from(&out_buffer)?;
        staging.read().await
    }
}

/// A composable expression tree for multi-band raster math.
///
/// Leaf nodes are either a `Band` index or a scalar `Constant`.
/// Interior nodes are arithmetic operators.
#[derive(Debug, Clone)]
pub enum BandExpression {
    /// Reference to band at the given index.
    Band(usize),
    /// Scalar constant.
    Constant(f32),
    /// Addition: `A + B`
    Add(Box<BandExpression>, Box<BandExpression>),
    /// Subtraction: `A - B`
    Sub(Box<BandExpression>, Box<BandExpression>),
    /// Multiplication: `A * B`
    Mul(Box<BandExpression>, Box<BandExpression>),
    /// Division: `A / B` (errors on divide-by-zero)
    Div(Box<BandExpression>, Box<BandExpression>),
    /// Square root: `sqrt(max(0, A))`
    Sqrt(Box<BandExpression>),
    /// Absolute value: `|A|`
    Abs(Box<BandExpression>),
    /// Negation: `-A`
    Neg(Box<BandExpression>),
    /// Element-wise minimum: `min(A, B)`
    Min(Box<BandExpression>, Box<BandExpression>),
    /// Element-wise maximum: `max(A, B)`
    Max(Box<BandExpression>, Box<BandExpression>),
    /// Power: `A ^ B` (uses `pow(A, B)` in WGSL).
    Pow(Box<BandExpression>, Box<BandExpression>),
    /// Natural logarithm: `ln(A)`.
    Log(Box<BandExpression>),
    /// Exponential: `exp(A)`.
    Exp(Box<BandExpression>),
    /// Clamp: `clamp(A, lo, hi)` — equivalent to `min(max(A, lo), hi)`.
    Clamp {
        value: Box<BandExpression>,
        lo: Box<BandExpression>,
        hi: Box<BandExpression>,
    },
}

impl BandExpression {
    /// Evaluate the expression for one pixel given per-band values.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError::InvalidKernelParams`] when a `Band` index is out
    /// of range or a `Div` node encounters a zero denominator.
    pub fn evaluate(&self, bands: &[f32]) -> Result<f32, GpuError> {
        match self {
            BandExpression::Band(idx) => bands.get(*idx).copied().ok_or_else(|| {
                GpuError::invalid_kernel_params(format!(
                    "band index {} out of range (have {} bands)",
                    idx,
                    bands.len()
                ))
            }),
            BandExpression::Constant(v) => Ok(*v),
            BandExpression::Add(a, b) => Ok(a.evaluate(bands)? + b.evaluate(bands)?),
            BandExpression::Sub(a, b) => Ok(a.evaluate(bands)? - b.evaluate(bands)?),
            BandExpression::Mul(a, b) => Ok(a.evaluate(bands)? * b.evaluate(bands)?),
            BandExpression::Div(a, b) => {
                let denom = b.evaluate(bands)?;
                if denom.abs() < 1e-10 {
                    Err(GpuError::invalid_kernel_params(
                        "division by zero in BandExpression",
                    ))
                } else {
                    Ok(a.evaluate(bands)? / denom)
                }
            }
            BandExpression::Sqrt(a) => Ok(a.evaluate(bands)?.max(0.0).sqrt()),
            BandExpression::Abs(a) => Ok(a.evaluate(bands)?.abs()),
            BandExpression::Neg(a) => Ok(-a.evaluate(bands)?),
            BandExpression::Min(a, b) => Ok(a.evaluate(bands)?.min(b.evaluate(bands)?)),
            BandExpression::Max(a, b) => Ok(a.evaluate(bands)?.max(b.evaluate(bands)?)),
            BandExpression::Pow(a, b) => Ok(a.evaluate(bands)?.powf(b.evaluate(bands)?)),
            BandExpression::Log(a) => Ok(a.evaluate(bands)?.ln()),
            BandExpression::Exp(a) => Ok(a.evaluate(bands)?.exp()),
            BandExpression::Clamp { value, lo, hi } => {
                let v = value.evaluate(bands)?;
                let l = lo.evaluate(bands)?;
                let h = hi.evaluate(bands)?;
                Ok(v.clamp(l, h))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute_empty_band_a() {
        let result = GpuAlgebra::execute(&[], None, AlgebraOp::Add, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_add() {
        let a = vec![1.0_f32, 2.0, 3.0];
        let b = vec![4.0_f32, 5.0, 6.0];
        let out = GpuAlgebra::execute(&a, Some(&b), AlgebraOp::Add, None).expect("execute failed");
        assert_eq!(out, vec![5.0, 7.0, 9.0]);
    }

    #[test]
    fn test_expression_band_out_of_range() {
        let expr = BandExpression::Band(5);
        assert!(expr.evaluate(&[1.0, 2.0]).is_err());
    }

    #[test]
    fn test_expression_div_by_zero() {
        let expr = BandExpression::Div(
            Box::new(BandExpression::Band(0)),
            Box::new(BandExpression::Constant(0.0)),
        );
        assert!(expr.evaluate(&[1.0]).is_err());
    }

    #[test]
    fn test_op_encoding_covers_all_ops() {
        // Every AlgebraOp must map to a distinct, defined GPU opcode (0..=10),
        // so no operation can fall through to the shader's NaN sentinel.
        let ops = [
            AlgebraOp::Add,
            AlgebraOp::Subtract,
            AlgebraOp::Multiply,
            AlgebraOp::Divide,
            AlgebraOp::Min,
            AlgebraOp::Max,
            AlgebraOp::Sqrt,
            AlgebraOp::Abs,
            AlgebraOp::Power(2.0),
            AlgebraOp::Clamp { min: 0.0, max: 1.0 },
            AlgebraOp::Normalize {
                src_min: 0.0,
                src_max: 1.0,
                dst_min: 0.0,
                dst_max: 255.0,
            },
        ];
        for op in ops {
            let (code, is_binary, _) = GpuAlgebra::op_encoding(&op);
            assert!(
                code <= 10,
                "opcode {code} out of supported range for {op:?}"
            );
            // Binary ops are exactly the first six.
            let expect_binary = matches!(
                op,
                AlgebraOp::Add
                    | AlgebraOp::Subtract
                    | AlgebraOp::Multiply
                    | AlgebraOp::Divide
                    | AlgebraOp::Min
                    | AlgebraOp::Max
            );
            assert_eq!(is_binary, expect_binary, "binary flag wrong for {op:?}");
        }
    }

    #[test]
    fn test_algebra_params_layout() {
        assert_eq!(std::mem::size_of::<AlgebraParamsGpu>(), 64);
    }

    #[tokio::test]
    async fn test_execute_gpu_matches_cpu() {
        // Environment guard: only the GPU-context creation is optional. Once a
        // context exists, GPU results must match the CPU reference for every op.
        if let Ok(context) = GpuContext::new().await {
            let a: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
            let b: Vec<f32> = vec![8.0, 7.0, 6.0, 5.0, 4.0, 3.0, 2.0, 1.0];

            let cases: Vec<(AlgebraOp, Option<&[f32]>)> = vec![
                (AlgebraOp::Add, Some(b.as_slice())),
                (AlgebraOp::Subtract, Some(b.as_slice())),
                (AlgebraOp::Multiply, Some(b.as_slice())),
                (AlgebraOp::Divide, Some(b.as_slice())),
                (AlgebraOp::Min, Some(b.as_slice())),
                (AlgebraOp::Max, Some(b.as_slice())),
                (AlgebraOp::Sqrt, None),
                (AlgebraOp::Abs, None),
                (AlgebraOp::Power(2.0), None),
                (AlgebraOp::Clamp { min: 2.0, max: 6.0 }, None),
                (
                    AlgebraOp::Normalize {
                        src_min: 1.0,
                        src_max: 8.0,
                        dst_min: 0.0,
                        dst_max: 100.0,
                    },
                    None,
                ),
            ];

            for (op, bb) in cases {
                let cpu = GpuAlgebra::execute(&a, bb, op.clone(), None)
                    .expect("cpu execute must succeed");
                let gpu = GpuAlgebra::execute_gpu(&context, &a, bb, op.clone(), None)
                    .await
                    .expect("gpu execute must succeed");
                assert_eq!(cpu.len(), gpu.len());
                for (i, (c, g)) in cpu.iter().zip(gpu.iter()).enumerate() {
                    assert!((c - g).abs() < 1e-3, "op {op:?} pixel {i}: cpu={c} gpu={g}");
                }
            }
        }
    }

    #[tokio::test]
    async fn test_execute_gpu_binary_without_band_b_errors() {
        if let Ok(context) = GpuContext::new().await {
            let a: Vec<f32> = vec![1.0, 2.0, 3.0];
            let result = GpuAlgebra::execute_gpu(&context, &a, None, AlgebraOp::Add, None).await;
            assert!(
                result.is_err(),
                "a binary op without band_b must return a typed error"
            );
        }
    }
}
