//! High-level raster calculator API: `RasterCalculator` and `RasterExpression`

use super::{evaluator::Evaluator, lexer::Lexer, optimizer::Optimizer, parser::Parser};
use crate::error::{AlgorithmError, Result};
use oxigdal_core::buffer::RasterBuffer;

#[cfg(feature = "parallel")]
use rayon::prelude::*;

/// Raster calculator for map algebra with expression parsing
pub struct RasterCalculator;

impl RasterCalculator {
    /// Evaluates a raster expression on one or more bands
    ///
    /// # Arguments
    ///
    /// * `expression` - The expression to evaluate (e.g., "(B1 - B2) / (B1 + B2)")
    /// * `bands` - Input bands (B1, B2, etc.)
    ///
    /// # Examples
    ///
    /// NDVI: `"(B1 - B2) / (B1 + B2)"`
    /// Conditional: `"if B1 > 100 then B1 * 2 else B1"`
    /// Math: `"sqrt(B1 ^ 2 + B2 ^ 2)"`
    ///
    /// # Errors
    ///
    /// Returns an error if the expression is invalid or evaluation fails
    pub fn evaluate(expression: &str, bands: &[RasterBuffer]) -> Result<RasterBuffer> {
        if bands.is_empty() {
            return Err(AlgorithmError::EmptyInput {
                operation: "evaluate",
            });
        }

        // Check all bands have same dimensions
        let width = bands[0].width();
        let height = bands[0].height();
        for (_i, band) in bands.iter().enumerate().skip(1) {
            if band.width() != width || band.height() != height {
                return Err(AlgorithmError::InvalidDimensions {
                    message: "All bands must have same dimensions",
                    actual: band.width() as usize,
                    expected: width as usize,
                });
            }
        }

        // Tokenize
        let mut lexer = Lexer::new(expression);
        let tokens = lexer.tokenize()?;

        // Parse
        let mut parser = Parser::new(tokens);
        let expr = parser.parse()?;

        // Optimize expression
        let expr = Optimizer::optimize(expr);

        // Evaluate
        let evaluator = Evaluator::new(bands);
        let mut result = RasterBuffer::zeros(width, height, bands[0].data_type());

        for y in 0..height {
            for x in 0..width {
                let value = evaluator.eval_pixel(&expr, x, y)?;
                result
                    .set_pixel(x, y, value)
                    .map_err(AlgorithmError::Core)?;
            }
        }

        Ok(result)
    }

    /// Evaluates a raster expression in parallel using rayon
    ///
    /// This method processes rows in parallel for improved performance on multi-core systems.
    /// Falls back to sequential evaluation if the parallel feature is not enabled.
    ///
    /// # Arguments
    ///
    /// * `expression` - The expression to evaluate (e.g., "(B1 - B2) / (B1 + B2)")
    /// * `bands` - Input bands (B1, B2, etc.)
    ///
    /// # Examples
    ///
    /// NDVI: `"(B1 - B2) / (B1 + B2)"`
    /// Conditional: `"if B1 > 100 then B1 * 2 else B1"`
    /// Math: `"sqrt(B1 ^ 2 + B2 ^ 2)"`
    ///
    /// # Errors
    ///
    /// Returns an error if the expression is invalid or evaluation fails
    #[cfg(feature = "parallel")]
    pub fn evaluate_parallel(expression: &str, bands: &[RasterBuffer]) -> Result<RasterBuffer> {
        if bands.is_empty() {
            return Err(AlgorithmError::EmptyInput {
                operation: "evaluate_parallel",
            });
        }

        // Check all bands have same dimensions
        let width = bands[0].width();
        let height = bands[0].height();
        for band in bands.iter().skip(1) {
            if band.width() != width || band.height() != height {
                return Err(AlgorithmError::InvalidDimensions {
                    message: "All bands must have same dimensions",
                    actual: band.width() as usize,
                    expected: width as usize,
                });
            }
        }

        // Tokenize
        let mut lexer = Lexer::new(expression);
        let tokens = lexer.tokenize()?;

        // Parse
        let mut parser = Parser::new(tokens);
        let expr = parser.parse()?;

        // Optimize expression
        let expr = Optimizer::optimize(expr);

        // Create evaluator
        let evaluator = Evaluator::new(bands);

        // Create result buffer
        let mut result = RasterBuffer::zeros(width, height, bands[0].data_type());

        // Process rows in parallel
        let row_data: Result<Vec<Vec<f64>>> = (0..height)
            .into_par_iter()
            .map(|y| {
                let mut row = Vec::with_capacity(width as usize);
                for x in 0..width {
                    let value = evaluator.eval_pixel(&expr, x, y)?;
                    row.push(value);
                }
                Ok(row)
            })
            .collect();

        let row_data = row_data?;

        // Write results back to buffer
        for (y, row) in row_data.iter().enumerate() {
            for (x, &value) in row.iter().enumerate() {
                result
                    .set_pixel(x as u64, y as u64, value)
                    .map_err(AlgorithmError::Core)?;
            }
        }

        Ok(result)
    }

    /// Applies a binary operation to two rasters (legacy API)
    pub fn apply_binary(
        a: &RasterBuffer,
        b: &RasterBuffer,
        op: RasterExpression,
    ) -> Result<RasterBuffer> {
        if a.width() != b.width() || a.height() != b.height() {
            return Err(AlgorithmError::InvalidDimensions {
                message: "Rasters must have same dimensions",
                actual: a.width() as usize,
                expected: b.width() as usize,
            });
        }

        let mut result = RasterBuffer::zeros(a.width(), a.height(), a.data_type());

        for y in 0..a.height() {
            for x in 0..a.width() {
                let val_a = a.get_pixel(x, y).map_err(AlgorithmError::Core)?;
                let val_b = b.get_pixel(x, y).map_err(AlgorithmError::Core)?;

                let val = match op {
                    RasterExpression::Add => val_a + val_b,
                    RasterExpression::Subtract => val_a - val_b,
                    RasterExpression::Multiply => val_a * val_b,
                    RasterExpression::Divide => {
                        if val_b.abs() < f64::EPSILON {
                            f64::NAN
                        } else {
                            val_a / val_b
                        }
                    }
                    RasterExpression::Max => val_a.max(val_b),
                    RasterExpression::Min => val_a.min(val_b),
                };

                result.set_pixel(x, y, val).map_err(AlgorithmError::Core)?;
            }
        }

        Ok(result)
    }

    /// Applies a unary function to a raster (legacy API)
    pub fn apply_unary<F>(src: &RasterBuffer, func: F) -> Result<RasterBuffer>
    where
        F: Fn(f64) -> f64,
    {
        let mut result = RasterBuffer::zeros(src.width(), src.height(), src.data_type());

        for y in 0..src.height() {
            for x in 0..src.width() {
                let val = src.get_pixel(x, y).map_err(AlgorithmError::Core)?;
                let new_val = func(val);
                result
                    .set_pixel(x, y, new_val)
                    .map_err(AlgorithmError::Core)?;
            }
        }

        Ok(result)
    }
}

/// Raster expression operations (legacy API)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RasterExpression {
    /// Add two rasters
    Add,
    /// Subtract rasters
    Subtract,
    /// Multiply rasters
    Multiply,
    /// Divide rasters
    Divide,
    /// Maximum of two rasters
    Max,
    /// Minimum of two rasters
    Min,
}
