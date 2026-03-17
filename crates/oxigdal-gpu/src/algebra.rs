//! GPU-accelerated raster algebra operations.
//!
//! Provides element-wise band math operations (`AlgebraOp`), a structured
//! expression tree (`BandExpression`) for composing multi-band formulas, and
//! the top-level `GpuAlgebra` driver that dispatches to GPU (future) or CPU.

use crate::error::GpuError;

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
}
