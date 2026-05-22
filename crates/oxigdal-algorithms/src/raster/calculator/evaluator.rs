//! Pixel-level expression evaluator for raster bands

use super::ast::{BinaryOp, Expr, UnaryOp};
use crate::error::{AlgorithmError, Result};
use oxigdal_core::buffer::RasterBuffer;

#[cfg(not(feature = "std"))]
use alloc::{string::String, vec::Vec};

/// Evaluator for raster expressions.
///
/// `cache_slots` holds the original (pre-CSE-rewrite) expression for each CSE
/// slot.  When `eval_pixel` encounters a `CacheRef(i)` it evaluates
/// `cache_slots[i]` lazily (computing it once per pixel) and stores the result
/// in the caller-supplied `pixel_cache` vector.  For non-CSE expressions
/// `cache_slots` may be an empty slice and `pixel_cache` may be an empty vec.
pub(super) struct Evaluator<'a> {
    bands: &'a [RasterBuffer],
    cache_slots: &'a [Expr],
}

impl<'a> Evaluator<'a> {
    pub(super) fn new(bands: &'a [RasterBuffer], cache_slots: &'a [Expr]) -> Self {
        Self { bands, cache_slots }
    }

    /// Evaluate `expr` at pixel `(x, y)`.
    ///
    /// `pixel_cache` is a per-pixel memoisation table with one `Option<f64>`
    /// entry per CSE slot.  It must be reset to all-`None` before each pixel.
    /// When there are no CSE slots an empty `&mut Vec` may be passed.
    pub(super) fn eval_pixel(
        &self,
        expr: &Expr,
        x: u64,
        y: u64,
        pixel_cache: &mut Vec<Option<f64>>,
    ) -> Result<f64> {
        match expr {
            Expr::Number(n) => Ok(*n),
            Expr::Band(b) => {
                if *b == 0 || *b > self.bands.len() {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "band",
                        message: format!("Band {} out of range (1-{})", b, self.bands.len()),
                    });
                }
                self.bands[b - 1]
                    .get_pixel(x, y)
                    .map_err(AlgorithmError::Core)
            }
            Expr::CacheRef(slot_id) => {
                let idx = *slot_id as usize;
                // Return cached value if already computed for this pixel
                if let Some(cached) = pixel_cache.get(idx).and_then(|v| *v) {
                    return Ok(cached);
                }
                // Clone the slot expression so we can recurse without borrowing self
                let slot_expr = self.cache_slots[idx].clone();
                let value = self.eval_pixel(&slot_expr, x, y, pixel_cache)?;
                // Store into cache — the vec is always pre-allocated to the right length
                if idx < pixel_cache.len() {
                    pixel_cache[idx] = Some(value);
                }
                Ok(value)
            }
            Expr::BinaryOp { left, op, right } => {
                let lval = self.eval_pixel(left, x, y, pixel_cache)?;
                let rval = self.eval_pixel(right, x, y, pixel_cache)?;

                let result = match op {
                    BinaryOp::Add => lval + rval,
                    BinaryOp::Subtract => lval - rval,
                    BinaryOp::Multiply => lval * rval,
                    BinaryOp::Divide => {
                        if rval.abs() < f64::EPSILON {
                            f64::NAN
                        } else {
                            lval / rval
                        }
                    }
                    BinaryOp::Power => lval.powf(rval),
                    BinaryOp::Greater => {
                        if lval > rval {
                            1.0
                        } else {
                            0.0
                        }
                    }
                    BinaryOp::Less => {
                        if lval < rval {
                            1.0
                        } else {
                            0.0
                        }
                    }
                    BinaryOp::GreaterEqual => {
                        if lval >= rval {
                            1.0
                        } else {
                            0.0
                        }
                    }
                    BinaryOp::LessEqual => {
                        if lval <= rval {
                            1.0
                        } else {
                            0.0
                        }
                    }
                    BinaryOp::Equal => {
                        if (lval - rval).abs() < f64::EPSILON {
                            1.0
                        } else {
                            0.0
                        }
                    }
                    BinaryOp::NotEqual => {
                        if (lval - rval).abs() >= f64::EPSILON {
                            1.0
                        } else {
                            0.0
                        }
                    }
                    BinaryOp::And => {
                        if lval != 0.0 && rval != 0.0 {
                            1.0
                        } else {
                            0.0
                        }
                    }
                    BinaryOp::Or => {
                        if lval != 0.0 || rval != 0.0 {
                            1.0
                        } else {
                            0.0
                        }
                    }
                };

                Ok(result)
            }
            Expr::UnaryOp { op, expr } => {
                let val = self.eval_pixel(expr, x, y, pixel_cache)?;
                match op {
                    UnaryOp::Negate => Ok(-val),
                }
            }
            Expr::Function { name, args } => self.eval_function(name, args, x, y, pixel_cache),
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                let cond_val = self.eval_pixel(condition, x, y, pixel_cache)?;
                if cond_val != 0.0 {
                    self.eval_pixel(then_expr, x, y, pixel_cache)
                } else {
                    self.eval_pixel(else_expr, x, y, pixel_cache)
                }
            }
        }
    }

    pub(super) fn eval_function(
        &self,
        name: &str,
        args: &[Expr],
        x: u64,
        y: u64,
        pixel_cache: &mut Vec<Option<f64>>,
    ) -> Result<f64> {
        let arg_vals: Result<Vec<f64>> = args
            .iter()
            .map(|arg| self.eval_pixel(arg, x, y, pixel_cache))
            .collect();
        let arg_vals = arg_vals?;

        let result = match name {
            "sqrt" => {
                if arg_vals.len() != 1 {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "sqrt",
                        message: "Expected 1 argument".to_string(),
                    });
                }
                arg_vals[0].sqrt()
            }
            "abs" => {
                if arg_vals.len() != 1 {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "abs",
                        message: "Expected 1 argument".to_string(),
                    });
                }
                arg_vals[0].abs()
            }
            "log" => {
                if arg_vals.len() != 1 {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "log",
                        message: "Expected 1 argument".to_string(),
                    });
                }
                arg_vals[0].ln()
            }
            "log10" => {
                if arg_vals.len() != 1 {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "log10",
                        message: "Expected 1 argument".to_string(),
                    });
                }
                arg_vals[0].log10()
            }
            "exp" => {
                if arg_vals.len() != 1 {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "exp",
                        message: "Expected 1 argument".to_string(),
                    });
                }
                arg_vals[0].exp()
            }
            "sin" => {
                if arg_vals.len() != 1 {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "sin",
                        message: "Expected 1 argument".to_string(),
                    });
                }
                arg_vals[0].sin()
            }
            "cos" => {
                if arg_vals.len() != 1 {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "cos",
                        message: "Expected 1 argument".to_string(),
                    });
                }
                arg_vals[0].cos()
            }
            "tan" => {
                if arg_vals.len() != 1 {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "tan",
                        message: "Expected 1 argument".to_string(),
                    });
                }
                arg_vals[0].tan()
            }
            "floor" => {
                if arg_vals.len() != 1 {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "floor",
                        message: "Expected 1 argument".to_string(),
                    });
                }
                arg_vals[0].floor()
            }
            "ceil" => {
                if arg_vals.len() != 1 {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "ceil",
                        message: "Expected 1 argument".to_string(),
                    });
                }
                arg_vals[0].ceil()
            }
            "round" => {
                if arg_vals.len() != 1 {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "round",
                        message: "Expected 1 argument".to_string(),
                    });
                }
                arg_vals[0].round()
            }
            "min" => {
                if arg_vals.is_empty() {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "min",
                        message: "Expected at least 1 argument".to_string(),
                    });
                }
                arg_vals.iter().copied().fold(f64::INFINITY, f64::min)
            }
            "max" => {
                if arg_vals.is_empty() {
                    return Err(AlgorithmError::InvalidParameter {
                        parameter: "max",
                        message: "Expected at least 1 argument".to_string(),
                    });
                }
                arg_vals.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            }
            _ => {
                return Err(AlgorithmError::InvalidParameter {
                    parameter: "function",
                    message: format!("Unknown function: {name}"),
                });
            }
        };

        Ok(result)
    }
}
