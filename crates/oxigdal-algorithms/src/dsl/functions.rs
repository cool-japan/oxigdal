//! Built-in functions for the Raster Algebra DSL
//!
//! This module provides a comprehensive library of built-in functions including:
//! - Mathematical functions (sin, cos, sqrt, etc.)
//! - Statistical functions (mean, median, percentile, etc.)
//! - Spatial functions (focal operations)
//! - Logical functions
//! - Type conversion functions

use super::variables::Value;
use crate::error::{AlgorithmError, Result};
use crate::raster::{gaussian_blur, median_filter};

#[cfg(not(feature = "std"))]
use alloc::{boxed::Box, string::String, vec::Vec};

/// Built-in function type
pub type BuiltinFn = fn(&[Value]) -> Result<Value>;

/// Registry of built-in functions
pub struct FunctionRegistry {
    functions: Vec<(&'static str, BuiltinFn, usize)>, // (name, function, arity)
}

impl Default for FunctionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl FunctionRegistry {
    /// Creates a new function registry with all built-in functions
    pub fn new() -> Self {
        let mut registry = Self {
            functions: Vec::new(),
        };

        // Mathematical functions (1 arg)
        registry.register("sqrt", fn_sqrt, 1);
        registry.register("abs", fn_abs, 1);
        registry.register("floor", fn_floor, 1);
        registry.register("ceil", fn_ceil, 1);
        registry.register("round", fn_round, 1);
        registry.register("log", fn_log, 1);
        registry.register("log10", fn_log10, 1);
        registry.register("log2", fn_log2, 1);
        registry.register("exp", fn_exp, 1);
        registry.register("sin", fn_sin, 1);
        registry.register("cos", fn_cos, 1);
        registry.register("tan", fn_tan, 1);
        registry.register("asin", fn_asin, 1);
        registry.register("acos", fn_acos, 1);
        registry.register("atan", fn_atan, 1);
        registry.register("sinh", fn_sinh, 1);
        registry.register("cosh", fn_cosh, 1);
        registry.register("tanh", fn_tanh, 1);

        // Mathematical functions (2 args)
        registry.register("atan2", fn_atan2, 2);
        registry.register("pow", fn_pow, 2);
        registry.register("hypot", fn_hypot, 2);

        // Min/Max (variable args)
        registry.register("min", fn_min, 0);
        registry.register("max", fn_max, 0);

        // Statistical functions (1 arg - raster)
        registry.register("mean", fn_mean, 1);
        registry.register("median", fn_median, 1);
        registry.register("mode", fn_mode, 1);
        registry.register("stddev", fn_stddev, 1);
        registry.register("variance", fn_variance, 1);
        registry.register("sum", fn_sum, 1);
        registry.register("product", fn_product, 1);

        // Percentile functions
        registry.register("percentile", fn_percentile, 2);

        // Spatial filters
        registry.register("gaussian", fn_gaussian, 2);
        registry.register("median_filter", fn_median_filt, 2);

        // Logical functions
        registry.register("and", fn_and, 2);
        registry.register("or", fn_or, 2);
        registry.register("not", fn_not, 1);
        registry.register("xor", fn_xor, 2);

        // Comparison functions
        registry.register("eq", fn_eq, 2);
        registry.register("ne", fn_ne, 2);
        registry.register("lt", fn_lt, 2);
        registry.register("le", fn_le, 2);
        registry.register("gt", fn_gt, 2);
        registry.register("ge", fn_ge, 2);

        // Type conversion
        registry.register("to_number", fn_to_number, 1);
        registry.register("to_bool", fn_to_bool, 1);

        // Utility functions
        registry.register("clamp", fn_clamp, 3);
        registry.register("select", fn_select, 3);

        registry
    }

    /// Registers a function
    pub fn register(&mut self, name: &'static str, func: BuiltinFn, arity: usize) {
        self.functions.push((name, func, arity));
    }

    /// Looks up a function by name
    pub fn lookup(&self, name: &str) -> Option<(BuiltinFn, usize)> {
        self.functions
            .iter()
            .find(|(n, _, _)| *n == name)
            .map(|(_, f, a)| (*f, *a))
    }

    /// Checks if a function exists
    pub fn exists(&self, name: &str) -> bool {
        self.functions.iter().any(|(n, _, _)| *n == name)
    }

    /// Gets all function names
    pub fn function_names(&self) -> Vec<&'static str> {
        self.functions.iter().map(|(n, _, _)| *n).collect()
    }
}

// Mathematical functions

/// Helper to apply a unary function to either a scalar or raster
fn apply_unary_fn<F>(value: &Value, f: F) -> Result<Value>
where
    F: Fn(f64) -> f64,
{
    match value {
        Value::Number(x) => Ok(Value::Number(f(*x))),
        Value::Raster(raster) => {
            use oxigdal_core::types::RasterDataType;
            let width = raster.width();
            let height = raster.height();
            let mut result =
                oxigdal_core::buffer::RasterBuffer::zeros(width, height, RasterDataType::Float32);

            for y in 0..height {
                for x in 0..width {
                    let pixel = raster
                        .get_pixel(x, y)
                        .map_err(crate::error::AlgorithmError::Core)?;
                    let new_val = f(pixel);
                    result
                        .set_pixel(x, y, new_val)
                        .map_err(crate::error::AlgorithmError::Core)?;
                }
            }

            Ok(Value::Raster(Box::new(result)))
        }
        _ => Err(AlgorithmError::InvalidParameter {
            parameter: "value",
            message: "Expected number or raster".to_string(),
        }),
    }
}

/// Helper to apply a binary function to scalars or rasters
fn apply_binary_fn<F>(left: &Value, right: &Value, f: F) -> Result<Value>
where
    F: Fn(f64, f64) -> f64,
{
    match (left, right) {
        (Value::Number(l), Value::Number(r)) => Ok(Value::Number(f(*l, *r))),
        (Value::Raster(raster), Value::Number(scalar))
        | (Value::Number(scalar), Value::Raster(raster)) => {
            use oxigdal_core::types::RasterDataType;
            let width = raster.width();
            let height = raster.height();
            let mut result =
                oxigdal_core::buffer::RasterBuffer::zeros(width, height, RasterDataType::Float32);

            for y in 0..height {
                for x in 0..width {
                    let pixel = raster
                        .get_pixel(x, y)
                        .map_err(crate::error::AlgorithmError::Core)?;
                    let new_val = f(pixel, *scalar);
                    result
                        .set_pixel(x, y, new_val)
                        .map_err(crate::error::AlgorithmError::Core)?;
                }
            }

            Ok(Value::Raster(Box::new(result)))
        }
        (Value::Raster(left_raster), Value::Raster(right_raster)) => {
            use oxigdal_core::types::RasterDataType;
            let width = left_raster.width();
            let height = left_raster.height();

            if right_raster.width() != width || right_raster.height() != height {
                return Err(AlgorithmError::InvalidDimensions {
                    message: "Rasters must have same dimensions",
                    actual: right_raster.width() as usize,
                    expected: width as usize,
                });
            }

            let mut result =
                oxigdal_core::buffer::RasterBuffer::zeros(width, height, RasterDataType::Float32);

            for y in 0..height {
                for x in 0..width {
                    let left_pixel = left_raster
                        .get_pixel(x, y)
                        .map_err(crate::error::AlgorithmError::Core)?;
                    let right_pixel = right_raster
                        .get_pixel(x, y)
                        .map_err(crate::error::AlgorithmError::Core)?;
                    let new_val = f(left_pixel, right_pixel);
                    result
                        .set_pixel(x, y, new_val)
                        .map_err(crate::error::AlgorithmError::Core)?;
                }
            }

            Ok(Value::Raster(Box::new(result)))
        }
        _ => Err(AlgorithmError::InvalidParameter {
            parameter: "value",
            message: "Expected number or raster".to_string(),
        }),
    }
}

fn fn_sqrt(args: &[Value]) -> Result<Value> {
    apply_unary_fn(&args[0], |x| x.sqrt())
}

fn fn_abs(args: &[Value]) -> Result<Value> {
    apply_unary_fn(&args[0], |x| x.abs())
}

fn fn_floor(args: &[Value]) -> Result<Value> {
    apply_unary_fn(&args[0], |x| x.floor())
}

fn fn_ceil(args: &[Value]) -> Result<Value> {
    apply_unary_fn(&args[0], |x| x.ceil())
}

fn fn_round(args: &[Value]) -> Result<Value> {
    apply_unary_fn(&args[0], |x| x.round())
}

fn fn_log(args: &[Value]) -> Result<Value> {
    apply_unary_fn(&args[0], |x| x.ln())
}

fn fn_log10(args: &[Value]) -> Result<Value> {
    apply_unary_fn(&args[0], |x| x.log10())
}

fn fn_log2(args: &[Value]) -> Result<Value> {
    apply_unary_fn(&args[0], |x| x.log2())
}

fn fn_exp(args: &[Value]) -> Result<Value> {
    apply_unary_fn(&args[0], |x| x.exp())
}

fn fn_sin(args: &[Value]) -> Result<Value> {
    apply_unary_fn(&args[0], |x| x.sin())
}

fn fn_cos(args: &[Value]) -> Result<Value> {
    apply_unary_fn(&args[0], |x| x.cos())
}

fn fn_tan(args: &[Value]) -> Result<Value> {
    apply_unary_fn(&args[0], |x| x.tan())
}

fn fn_asin(args: &[Value]) -> Result<Value> {
    apply_unary_fn(&args[0], |x| x.asin())
}

fn fn_acos(args: &[Value]) -> Result<Value> {
    apply_unary_fn(&args[0], |x| x.acos())
}

fn fn_atan(args: &[Value]) -> Result<Value> {
    apply_unary_fn(&args[0], |x| x.atan())
}

fn fn_sinh(args: &[Value]) -> Result<Value> {
    apply_unary_fn(&args[0], |x| x.sinh())
}

fn fn_cosh(args: &[Value]) -> Result<Value> {
    apply_unary_fn(&args[0], |x| x.cosh())
}

fn fn_tanh(args: &[Value]) -> Result<Value> {
    apply_unary_fn(&args[0], |x| x.tanh())
}

fn fn_atan2(args: &[Value]) -> Result<Value> {
    apply_binary_fn(&args[0], &args[1], |y, x| y.atan2(x))
}

fn fn_pow(args: &[Value]) -> Result<Value> {
    apply_binary_fn(&args[0], &args[1], |base, exp| base.powf(exp))
}

fn fn_hypot(args: &[Value]) -> Result<Value> {
    apply_binary_fn(&args[0], &args[1], |x, y| x.hypot(y))
}

fn fn_min(args: &[Value]) -> Result<Value> {
    if args.is_empty() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "min",
            message: "Expected at least 1 argument".to_string(),
        });
    }

    let mut min_val = args[0].as_number()?;
    for arg in &args[1..] {
        let val = arg.as_number()?;
        if val < min_val {
            min_val = val;
        }
    }
    Ok(Value::Number(min_val))
}

fn fn_max(args: &[Value]) -> Result<Value> {
    if args.is_empty() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "max",
            message: "Expected at least 1 argument".to_string(),
        });
    }

    let mut max_val = args[0].as_number()?;
    for arg in &args[1..] {
        let val = arg.as_number()?;
        if val > max_val {
            max_val = val;
        }
    }
    Ok(Value::Number(max_val))
}

// Statistical functions

fn fn_mean(args: &[Value]) -> Result<Value> {
    let raster = args[0].as_raster()?;
    let mut sum = 0.0;
    let mut count = 0u64;

    for y in 0..raster.height() {
        for x in 0..raster.width() {
            if let Ok(val) = raster.get_pixel(x, y) {
                if val.is_finite() {
                    sum += val;
                    count += 1;
                }
            }
        }
    }

    if count == 0 {
        return Err(AlgorithmError::EmptyInput { operation: "mean" });
    }

    Ok(Value::Number(sum / count as f64))
}

fn fn_median(args: &[Value]) -> Result<Value> {
    let raster = args[0].as_raster()?;
    let mut values: Vec<f64> = Vec::with_capacity((raster.width() * raster.height()) as usize);

    for y in 0..raster.height() {
        for x in 0..raster.width() {
            if let Ok(val) = raster.get_pixel(x, y) {
                if val.is_finite() {
                    values.push(val);
                }
            }
        }
    }

    if values.is_empty() {
        return Err(AlgorithmError::EmptyInput {
            operation: "median",
        });
    }

    // Sort using total ordering on finite f64 values (all NaN/inf already excluded above)
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let mid = values.len() / 2;
    let median = if values.len() % 2 == 0 {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    };

    Ok(Value::Number(median))
}

fn fn_mode(args: &[Value]) -> Result<Value> {
    let raster = args[0].as_raster()?;

    // Use a frequency map keyed by integer bit pattern for exact equality
    // (suitable for raster data that is typically quantized)
    use std::collections::HashMap;
    let mut freq: HashMap<u64, (f64, u64)> = HashMap::new();

    for y in 0..raster.height() {
        for x in 0..raster.width() {
            if let Ok(val) = raster.get_pixel(x, y) {
                if val.is_finite() {
                    let key = val.to_bits();
                    let entry = freq.entry(key).or_insert((val, 0));
                    entry.1 += 1;
                }
            }
        }
    }

    if freq.is_empty() {
        return Err(AlgorithmError::EmptyInput { operation: "mode" });
    }

    // Find the value with the highest frequency; break ties by smallest value
    let mode = freq
        .values()
        .max_by(|a, b| {
            a.1.cmp(&b.1)
                .then_with(|| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal))
        })
        .map(|(val, _)| *val)
        .ok_or(AlgorithmError::EmptyInput { operation: "mode" })?;

    Ok(Value::Number(mode))
}

fn fn_stddev(args: &[Value]) -> Result<Value> {
    let raster = args[0].as_raster()?;
    let mut sum = 0.0;
    let mut sum_sq = 0.0;
    let mut count = 0u64;

    for y in 0..raster.height() {
        for x in 0..raster.width() {
            if let Ok(val) = raster.get_pixel(x, y) {
                if val.is_finite() {
                    sum += val;
                    sum_sq += val * val;
                    count += 1;
                }
            }
        }
    }

    if count == 0 {
        return Err(AlgorithmError::EmptyInput {
            operation: "stddev",
        });
    }

    let mean = sum / count as f64;
    let variance = (sum_sq / count as f64) - (mean * mean);
    Ok(Value::Number(variance.sqrt()))
}

fn fn_variance(args: &[Value]) -> Result<Value> {
    let raster = args[0].as_raster()?;
    let mut sum = 0.0;
    let mut sum_sq = 0.0;
    let mut count = 0u64;

    for y in 0..raster.height() {
        for x in 0..raster.width() {
            if let Ok(val) = raster.get_pixel(x, y) {
                if val.is_finite() {
                    sum += val;
                    sum_sq += val * val;
                    count += 1;
                }
            }
        }
    }

    if count == 0 {
        return Err(AlgorithmError::EmptyInput {
            operation: "variance",
        });
    }

    let mean = sum / count as f64;
    let variance = (sum_sq / count as f64) - (mean * mean);
    Ok(Value::Number(variance))
}

fn fn_sum(args: &[Value]) -> Result<Value> {
    let raster = args[0].as_raster()?;
    let mut sum = 0.0;

    for y in 0..raster.height() {
        for x in 0..raster.width() {
            if let Ok(val) = raster.get_pixel(x, y) {
                if val.is_finite() {
                    sum += val;
                }
            }
        }
    }

    Ok(Value::Number(sum))
}

fn fn_product(args: &[Value]) -> Result<Value> {
    let raster = args[0].as_raster()?;
    let mut product = 1.0;

    for y in 0..raster.height() {
        for x in 0..raster.width() {
            if let Ok(val) = raster.get_pixel(x, y) {
                if val.is_finite() {
                    product *= val;
                }
            }
        }
    }

    Ok(Value::Number(product))
}

fn fn_percentile(args: &[Value]) -> Result<Value> {
    let raster = args[0].as_raster()?;
    let p = args[1].as_number()?;

    if !(0.0..=100.0).contains(&p) {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "percentile",
            message: format!("Percentile must be in [0, 100], got {p}"),
        });
    }

    let mut values: Vec<f64> = Vec::with_capacity((raster.width() * raster.height()) as usize);

    for y in 0..raster.height() {
        for x in 0..raster.width() {
            if let Ok(val) = raster.get_pixel(x, y) {
                if val.is_finite() {
                    values.push(val);
                }
            }
        }
    }

    if values.is_empty() {
        return Err(AlgorithmError::EmptyInput {
            operation: "percentile",
        });
    }

    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let n = values.len();
    if n == 1 {
        return Ok(Value::Number(values[0]));
    }

    // Linear interpolation method (same as numpy's default)
    let rank = p / 100.0 * (n - 1) as f64;
    let lower = rank.floor() as usize;
    let upper = (lower + 1).min(n - 1);
    let frac = rank - lower as f64;
    let result = values[lower] + frac * (values[upper] - values[lower]);

    Ok(Value::Number(result))
}

// Spatial filters

fn fn_gaussian(args: &[Value]) -> Result<Value> {
    let raster = args[0].as_raster()?;
    let sigma = args[1].as_number()?;

    let result = gaussian_blur(raster, sigma, None)?;
    Ok(Value::Raster(Box::new(result)))
}

fn fn_median_filt(args: &[Value]) -> Result<Value> {
    let raster = args[0].as_raster()?;
    let radius = args[1].as_number()? as usize;

    let result = median_filter(raster, radius)?;
    Ok(Value::Raster(Box::new(result)))
}

// Logical functions

fn fn_and(args: &[Value]) -> Result<Value> {
    let a = args[0].as_bool()?;
    let b = args[1].as_bool()?;
    Ok(Value::Bool(a && b))
}

fn fn_or(args: &[Value]) -> Result<Value> {
    let a = args[0].as_bool()?;
    let b = args[1].as_bool()?;
    Ok(Value::Bool(a || b))
}

fn fn_not(args: &[Value]) -> Result<Value> {
    let a = args[0].as_bool()?;
    Ok(Value::Bool(!a))
}

fn fn_xor(args: &[Value]) -> Result<Value> {
    let a = args[0].as_bool()?;
    let b = args[1].as_bool()?;
    Ok(Value::Bool(a ^ b))
}

// Comparison functions

fn fn_eq(args: &[Value]) -> Result<Value> {
    let a = args[0].as_number()?;
    let b = args[1].as_number()?;
    Ok(Value::Bool((a - b).abs() < f64::EPSILON))
}

fn fn_ne(args: &[Value]) -> Result<Value> {
    let a = args[0].as_number()?;
    let b = args[1].as_number()?;
    Ok(Value::Bool((a - b).abs() >= f64::EPSILON))
}

fn fn_lt(args: &[Value]) -> Result<Value> {
    let a = args[0].as_number()?;
    let b = args[1].as_number()?;
    Ok(Value::Bool(a < b))
}

fn fn_le(args: &[Value]) -> Result<Value> {
    let a = args[0].as_number()?;
    let b = args[1].as_number()?;
    Ok(Value::Bool(a <= b))
}

fn fn_gt(args: &[Value]) -> Result<Value> {
    let a = args[0].as_number()?;
    let b = args[1].as_number()?;
    Ok(Value::Bool(a > b))
}

fn fn_ge(args: &[Value]) -> Result<Value> {
    let a = args[0].as_number()?;
    let b = args[1].as_number()?;
    Ok(Value::Bool(a >= b))
}

// Type conversion

fn fn_to_number(args: &[Value]) -> Result<Value> {
    args[0].as_number().map(Value::Number)
}

fn fn_to_bool(args: &[Value]) -> Result<Value> {
    args[0].as_bool().map(Value::Bool)
}

// Utility functions

fn fn_clamp(args: &[Value]) -> Result<Value> {
    let value = args[0].as_number()?;
    let min = args[1].as_number()?;
    let max = args[2].as_number()?;

    let clamped = if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    };

    Ok(Value::Number(clamped))
}

fn fn_select(args: &[Value]) -> Result<Value> {
    let cond = args[0].as_bool()?;
    if cond {
        Ok(args[1].clone())
    } else {
        Ok(args[2].clone())
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use oxigdal_core::buffer::RasterBuffer;
    use oxigdal_core::types::RasterDataType;

    #[test]
    fn test_function_registry() {
        let registry = FunctionRegistry::new();
        assert!(registry.exists("sqrt"));
        assert!(registry.exists("sin"));
        assert!(registry.exists("mean"));
        assert!(!registry.exists("nonexistent"));
    }

    #[test]
    fn test_math_functions() {
        let args = vec![Value::Number(16.0)];
        let result = fn_sqrt(&args).expect("Should work");
        if let Value::Number(n) = result {
            assert!((n - 4.0).abs() < 1e-10);
        } else {
            panic!("Expected number");
        }
    }

    #[test]
    fn test_min_max() {
        let args = vec![
            Value::Number(3.0),
            Value::Number(1.0),
            Value::Number(4.0),
            Value::Number(1.0),
            Value::Number(5.0),
        ];

        let min_result = fn_min(&args).expect("Should work");
        if let Value::Number(n) = min_result {
            assert!((n - 1.0).abs() < 1e-10);
        }

        let max_result = fn_max(&args).expect("Should work");
        if let Value::Number(n) = max_result {
            assert!((n - 5.0).abs() < 1e-10);
        }
    }

    #[test]
    fn test_mean() {
        let mut raster = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        for y in 0..10 {
            for x in 0..10 {
                let _ = raster.set_pixel(x, y, (x + y) as f64);
            }
        }

        let args = vec![Value::Raster(Box::new(raster))];
        let result = fn_mean(&args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_logical_functions() {
        let args_true = vec![Value::Bool(true), Value::Bool(true)];
        let result = fn_and(&args_true).expect("Should work");
        assert!(matches!(result, Value::Bool(true)));

        let args_false = vec![Value::Bool(true), Value::Bool(false)];
        let result = fn_and(&args_false).expect("Should work");
        assert!(matches!(result, Value::Bool(false)));
    }

    #[test]
    fn test_clamp() {
        let args = vec![Value::Number(15.0), Value::Number(0.0), Value::Number(10.0)];
        let result = fn_clamp(&args).expect("Should work");
        if let Value::Number(n) = result {
            assert!((n - 10.0).abs() < 1e-10);
        }
    }
}
