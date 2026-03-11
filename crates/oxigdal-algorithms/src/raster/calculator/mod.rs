//! Raster calculator (map algebra) with expression parsing
//!
//! This module provides a comprehensive raster calculator with support for:
//! - Arithmetic operations: +, -, *, /, ^
//! - Math functions: sqrt, log, exp, sin, cos, tan, abs, floor, ceil, etc.
//! - Band algebra: (B1 - B2) / (B1 + B2) for NDVI and similar indices
//! - Conditional operations: if/then/else
//! - Multi-band operations
//! - Proper NoData handling

mod ast;
mod evaluator;
mod lexer;
mod ops;
mod optimizer;
mod parser;

pub use ops::{RasterCalculator, RasterExpression};

#[cfg(test)]
#[allow(clippy::panic, clippy::cloned_ref_to_slice_refs)]
mod tests {
    use super::*;
    use crate::error::AlgorithmError;
    use oxigdal_core::buffer::RasterBuffer;
    use oxigdal_core::types::RasterDataType;

    // ========== Basic Functionality Tests ==========

    #[test]
    fn test_simple_arithmetic() {
        let mut b1 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        let mut b2 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        for y in 0..5 {
            for x in 0..5 {
                b1.set_pixel(x, y, 10.0).ok();
                b2.set_pixel(x, y, 5.0).ok();
            }
        }

        let result = RasterCalculator::evaluate("B1 + B2", &[b1, b2]);
        assert!(result.is_ok());
        let r = result.expect("Result should be ok");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 15.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_ndvi() {
        let mut nir = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        let mut red = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        for y in 0..5 {
            for x in 0..5 {
                nir.set_pixel(x, y, 100.0).ok();
                red.set_pixel(x, y, 50.0).ok();
            }
        }

        let result = RasterCalculator::evaluate("(B1 - B2) / (B1 + B2)", &[nir, red]);
        assert!(result.is_ok());
        let r = result.expect("Result should be ok");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        let expected = (100.0 - 50.0) / (100.0 + 50.0);
        assert!((val - expected).abs() < 0.001);
    }

    #[test]
    fn test_math_functions() {
        let mut b1 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        for y in 0..5 {
            for x in 0..5 {
                b1.set_pixel(x, y, 16.0).ok();
            }
        }

        let result = RasterCalculator::evaluate("sqrt(B1)", &[b1]);
        assert!(result.is_ok());
        let r = result.expect("Result should be ok");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 4.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_conditional() {
        let mut b1 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        for y in 0..5 {
            for x in 0..5 {
                b1.set_pixel(x, y, (x * 10) as f64).ok();
            }
        }

        let result = RasterCalculator::evaluate("if B1 > 20 then 1 else 0", &[b1]);
        assert!(result.is_ok());
        let r = result.expect("Result should be ok");

        let val0 = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val0 - 0.0).abs() < f64::EPSILON);

        let val3 = r.get_pixel(3, 0).expect("Should get pixel");
        assert!((val3 - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_legacy_add() {
        let mut a = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        let mut b = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        a.set_pixel(0, 0, 10.0).ok();
        b.set_pixel(0, 0, 5.0).ok();

        let result = RasterCalculator::apply_binary(&a, &b, RasterExpression::Add);
        assert!(result.is_ok());
    }

    // ========== Edge Cases ==========

    #[test]
    fn test_empty_bands() {
        let result = RasterCalculator::evaluate("B1 + B2", &[]);
        assert!(result.is_err());
        if let Err(AlgorithmError::EmptyInput { .. }) = result {
            // Expected
        } else {
            panic!("Expected EmptyInput error");
        }
    }

    #[test]
    fn test_single_pixel() {
        let mut b1 = RasterBuffer::zeros(1, 1, RasterDataType::Float32);
        b1.set_pixel(0, 0, 42.0).ok();

        let result = RasterCalculator::evaluate("B1 * 2", &[b1]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 84.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_division_by_zero() {
        let mut b1 = RasterBuffer::zeros(3, 3, RasterDataType::Float32);
        let mut b2 = RasterBuffer::zeros(3, 3, RasterDataType::Float32);

        for y in 0..3 {
            for x in 0..3 {
                b1.set_pixel(x, y, 10.0).ok();
                b2.set_pixel(x, y, 0.0).ok();
            }
        }

        let result = RasterCalculator::evaluate("B1 / B2", &[b1, b2]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!(val.is_nan()); // Division by zero should give NaN
    }

    #[test]
    fn test_mismatched_dimensions() {
        let b1 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        let b2 = RasterBuffer::zeros(3, 3, RasterDataType::Float32);

        let result = RasterCalculator::evaluate("B1 + B2", &[b1, b2]);
        assert!(result.is_err());
        if let Err(AlgorithmError::InvalidDimensions { .. }) = result {
            // Expected
        } else {
            panic!("Expected InvalidDimensions error");
        }
    }

    // ========== Error Conditions ==========

    #[test]
    fn test_invalid_band_reference() {
        let b1 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        let result = RasterCalculator::evaluate("B5 + B1", &[b1]);
        assert!(result.is_err());
    }

    #[test]
    fn test_undefined_function() {
        let b1 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        let result = RasterCalculator::evaluate("undefined_func(B1)", &[b1]);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_expression() {
        let b1 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        let result = RasterCalculator::evaluate("B1 +", &[b1]);
        assert!(result.is_err());
    }

    #[test]
    fn test_mismatched_parentheses() {
        let b1 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        let result = RasterCalculator::evaluate("(B1 + 10", &[b1]);
        assert!(result.is_err());
    }

    #[test]
    fn test_wrong_function_arity() {
        let b1 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        let result = RasterCalculator::evaluate("sqrt(B1, B1)", &[b1]);
        assert!(result.is_err());
    }

    // ========== Complex Operations ==========

    #[test]
    fn test_nested_functions() {
        let mut b1 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        for y in 0..5 {
            for x in 0..5 {
                b1.set_pixel(x, y, 9.0).ok();
            }
        }

        let result = RasterCalculator::evaluate("sqrt(sqrt(B1))", &[b1]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        let expected = 9.0_f64.sqrt().sqrt();
        assert!((val - expected).abs() < 0.001);
    }

    #[test]
    fn test_complex_expression() {
        let mut b1 = RasterBuffer::zeros(3, 3, RasterDataType::Float32);
        let mut b2 = RasterBuffer::zeros(3, 3, RasterDataType::Float32);
        let mut b3 = RasterBuffer::zeros(3, 3, RasterDataType::Float32);

        for y in 0..3 {
            for x in 0..3 {
                b1.set_pixel(x, y, 10.0).ok();
                b2.set_pixel(x, y, 5.0).ok();
                b3.set_pixel(x, y, 2.0).ok();
            }
        }

        let result = RasterCalculator::evaluate("(B1 + B2) * B3 - sqrt(B1)", &[b1, b2, b3]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        let expected = (10.0 + 5.0) * 2.0 - 10.0_f64.sqrt();
        assert!((val - expected).abs() < 0.001);
    }

    #[test]
    fn test_all_math_functions() {
        let mut b1 = RasterBuffer::zeros(2, 2, RasterDataType::Float32);

        for y in 0..2 {
            for x in 0..2 {
                b1.set_pixel(x, y, 2.5).ok();
            }
        }

        // Test each function
        let functions = vec![
            ("abs(B1)", 2.5),
            ("floor(B1)", 2.0),
            ("ceil(B1)", 3.0),
            ("round(B1)", 3.0), // rounds to nearest (2.5 -> 3.0)
            ("exp(B1)", 2.5_f64.exp()),
            ("log(B1)", 2.5_f64.ln()),
            ("log10(B1)", 2.5_f64.log10()),
            ("sqrt(B1)", 2.5_f64.sqrt()),
            ("sin(B1)", 2.5_f64.sin()),
            ("cos(B1)", 2.5_f64.cos()),
            ("tan(B1)", 2.5_f64.tan()),
        ];

        for (expr, expected) in functions {
            let result = RasterCalculator::evaluate(expr, &[b1.clone()]);
            assert!(result.is_ok(), "Failed for expression: {}", expr);
            let r = result.expect("Should succeed");
            let val = r.get_pixel(0, 0).expect("Should get pixel");
            assert!(
                (val - expected).abs() < 0.001,
                "Expression {} expected {} but got {}",
                expr,
                expected,
                val
            );
        }
    }

    #[test]
    fn test_min_max_functions() {
        let mut b1 = RasterBuffer::zeros(3, 3, RasterDataType::Float32);
        let mut b2 = RasterBuffer::zeros(3, 3, RasterDataType::Float32);

        for y in 0..3 {
            for x in 0..3 {
                b1.set_pixel(x, y, 10.0).ok();
                b2.set_pixel(x, y, 20.0).ok();
            }
        }

        let result = RasterCalculator::evaluate("min(B1, B2)", &[b1.clone(), b2.clone()]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 10.0).abs() < f64::EPSILON);

        let result = RasterCalculator::evaluate("max(B1, B2)", &[b1, b2]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_power_operation() {
        let mut b1 = RasterBuffer::zeros(3, 3, RasterDataType::Float32);

        for y in 0..3 {
            for x in 0..3 {
                b1.set_pixel(x, y, 2.0).ok();
            }
        }

        let result = RasterCalculator::evaluate("B1 ^ 3", &[b1]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 8.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_comparison_operators() {
        let mut b1 = RasterBuffer::zeros(3, 3, RasterDataType::Float32);

        for y in 0..3 {
            for x in 0..3 {
                b1.set_pixel(x, y, 10.0).ok();
            }
        }

        let tests = vec![
            ("B1 > 5", 1.0),
            ("B1 < 5", 0.0),
            ("B1 >= 10", 1.0),
            ("B1 <= 10", 1.0),
            ("B1 == 10", 1.0),
            ("B1 != 5", 1.0),
        ];

        for (expr, expected) in tests {
            let result = RasterCalculator::evaluate(expr, &[b1.clone()]);
            assert!(result.is_ok(), "Failed for expression: {}", expr);
            let r = result.expect("Should succeed");
            let val = r.get_pixel(0, 0).expect("Should get pixel");
            assert!(
                (val - expected).abs() < f64::EPSILON,
                "Expression {} expected {} but got {}",
                expr,
                expected,
                val
            );
        }
    }

    #[test]
    fn test_logical_operators() {
        let mut b1 = RasterBuffer::zeros(3, 3, RasterDataType::Float32);

        for y in 0..3 {
            for x in 0..3 {
                b1.set_pixel(x, y, 10.0).ok();
            }
        }

        let result = RasterCalculator::evaluate("B1 > 5 and B1 < 15", &[b1.clone()]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 1.0).abs() < f64::EPSILON);

        let result = RasterCalculator::evaluate("B1 < 5 or B1 > 5", &[b1]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_nested_conditionals() {
        let mut b1 = RasterBuffer::zeros(3, 3, RasterDataType::Float32);

        for y in 0..3 {
            for x in 0..3 {
                b1.set_pixel(x, y, (x * 10) as f64).ok();
            }
        }

        // Nested conditionals using proper syntax
        let result =
            RasterCalculator::evaluate("if B1 > 15 then 3 else (if B1 > 5 then 2 else 1)", &[b1]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");

        let val0 = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val0 - 1.0).abs() < f64::EPSILON);

        let val1 = r.get_pixel(1, 0).expect("Should get pixel");
        assert!((val1 - 2.0).abs() < f64::EPSILON);

        let val2 = r.get_pixel(2, 0).expect("Should get pixel");
        assert!((val2 - 3.0).abs() < f64::EPSILON);
    }

    // ========== Legacy API Tests ==========

    #[test]
    fn test_legacy_operations() {
        let mut a = RasterBuffer::zeros(3, 3, RasterDataType::Float32);
        let mut b = RasterBuffer::zeros(3, 3, RasterDataType::Float32);

        for y in 0..3 {
            for x in 0..3 {
                a.set_pixel(x, y, 10.0).ok();
                b.set_pixel(x, y, 5.0).ok();
            }
        }

        let operations = vec![
            (RasterExpression::Add, 15.0),
            (RasterExpression::Subtract, 5.0),
            (RasterExpression::Multiply, 50.0),
            (RasterExpression::Divide, 2.0),
            (RasterExpression::Max, 10.0),
            (RasterExpression::Min, 5.0),
        ];

        for (op, expected) in operations {
            let result = RasterCalculator::apply_binary(&a, &b, op);
            assert!(result.is_ok());
            let r = result.expect("Should succeed");
            let val = r.get_pixel(0, 0).expect("Should get pixel");
            assert!(
                (val - expected).abs() < f64::EPSILON,
                "Operation {:?} expected {} but got {}",
                op,
                expected,
                val
            );
        }
    }

    #[test]
    fn test_legacy_unary() {
        let mut src = RasterBuffer::zeros(3, 3, RasterDataType::Float32);

        for y in 0..3 {
            for x in 0..3 {
                src.set_pixel(x, y, 5.0).ok();
            }
        }

        let result = RasterCalculator::apply_unary(&src, |x| x * 2.0);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_unary_negate() {
        let mut b1 = RasterBuffer::zeros(3, 3, RasterDataType::Float32);

        for y in 0..3 {
            for x in 0..3 {
                b1.set_pixel(x, y, 10.0).ok();
            }
        }

        let result = RasterCalculator::evaluate("-B1", &[b1]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val + 10.0).abs() < f64::EPSILON);
    }

    // ========== Optimizer Tests ==========

    #[test]
    fn test_optimizer_constant_folding() {
        let b1 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        // Constant expression should be pre-computed
        let result = RasterCalculator::evaluate("2 + 3", &[b1.clone()]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 5.0).abs() < f64::EPSILON);

        // Constant function call
        let result = RasterCalculator::evaluate("sqrt(16)", &[b1.clone()]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 4.0).abs() < f64::EPSILON);

        // Mixed constant and variable
        let mut b2 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        for y in 0..5 {
            for x in 0..5 {
                b2.set_pixel(x, y, 10.0).ok();
            }
        }
        let result = RasterCalculator::evaluate("B1 + (2 + 3)", &[b2]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 15.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_optimizer_algebraic_simplification() {
        let mut b1 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        for y in 0..5 {
            for x in 0..5 {
                b1.set_pixel(x, y, 10.0).ok();
            }
        }

        // x + 0 = x
        let result = RasterCalculator::evaluate("B1 + 0", &[b1.clone()]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 10.0).abs() < f64::EPSILON);

        // x * 1 = x
        let result = RasterCalculator::evaluate("B1 * 1", &[b1.clone()]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 10.0).abs() < f64::EPSILON);

        // x * 0 = 0
        let result = RasterCalculator::evaluate("B1 * 0", &[b1.clone()]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!(val.abs() < f64::EPSILON);

        // x ^ 1 = x
        let result = RasterCalculator::evaluate("B1 ^ 1", &[b1]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_optimizer_conditional_constant() {
        let mut b1 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        for y in 0..5 {
            for x in 0..5 {
                b1.set_pixel(x, y, 10.0).ok();
            }
        }

        // Constant true condition
        let result = RasterCalculator::evaluate("if 1 then B1 else B1 * 2", &[b1.clone()]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 10.0).abs() < f64::EPSILON);

        // Constant false condition
        let result = RasterCalculator::evaluate("if 0 then B1 else B1 * 2", &[b1]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");
        let val = r.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 20.0).abs() < f64::EPSILON);
    }

    // ========== Parallel Evaluation Tests ==========

    #[test]
    #[cfg(feature = "parallel")]
    fn test_parallel_evaluation() {
        let mut b1 = RasterBuffer::zeros(100, 100, RasterDataType::Float32);
        let mut b2 = RasterBuffer::zeros(100, 100, RasterDataType::Float32);

        for y in 0..100 {
            for x in 0..100 {
                b1.set_pixel(x, y, (x + y) as f64).ok();
                b2.set_pixel(x, y, (x * y) as f64).ok();
            }
        }

        let result = RasterCalculator::evaluate_parallel("B1 + B2", &[b1.clone(), b2.clone()]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");

        // Verify some values
        for y in 0..100 {
            for x in 0..100 {
                let val = r.get_pixel(x, y).expect("Should get pixel");
                let expected = (x + y) as f64 + (x * y) as f64;
                assert!(
                    (val - expected).abs() < f64::EPSILON,
                    "Mismatch at ({}, {}): expected {}, got {}",
                    x,
                    y,
                    expected,
                    val
                );
            }
        }
    }

    #[test]
    #[cfg(feature = "parallel")]
    fn test_parallel_complex_expression() {
        let mut nir = RasterBuffer::zeros(50, 50, RasterDataType::Float32);
        let mut red = RasterBuffer::zeros(50, 50, RasterDataType::Float32);

        for y in 0..50 {
            for x in 0..50 {
                nir.set_pixel(x, y, 100.0 + x as f64).ok();
                red.set_pixel(x, y, 50.0 + y as f64).ok();
            }
        }

        let result = RasterCalculator::evaluate_parallel(
            "(B1 - B2) / (B1 + B2)",
            &[nir.clone(), red.clone()],
        );
        assert!(result.is_ok());
        let r = result.expect("Should succeed");

        // Verify NDVI calculation
        for y in 0..50 {
            for x in 0..50 {
                let nir_val = 100.0 + x as f64;
                let red_val = 50.0 + y as f64;
                let expected = (nir_val - red_val) / (nir_val + red_val);
                let val = r.get_pixel(x, y).expect("Should get pixel");
                assert!(
                    (val - expected).abs() < 0.001,
                    "NDVI mismatch at ({}, {}): expected {}, got {}",
                    x,
                    y,
                    expected,
                    val
                );
            }
        }
    }

    #[test]
    #[cfg(feature = "parallel")]
    fn test_parallel_with_optimization() {
        let mut b1 = RasterBuffer::zeros(50, 50, RasterDataType::Float32);
        for y in 0..50 {
            for x in 0..50 {
                b1.set_pixel(x, y, x as f64).ok();
            }
        }

        // Expression with constants that should be optimized
        let result = RasterCalculator::evaluate_parallel("B1 * 1 + 0 + sqrt(16)", &[b1]);
        assert!(result.is_ok());
        let r = result.expect("Should succeed");

        for y in 0..50 {
            for x in 0..50 {
                let val = r.get_pixel(x, y).expect("Should get pixel");
                let expected = x as f64 + 4.0; // B1 + 4 (after optimization)
                assert!(
                    (val - expected).abs() < f64::EPSILON,
                    "Mismatch at ({}, {}): expected {}, got {}",
                    x,
                    y,
                    expected,
                    val
                );
            }
        }
    }
}
