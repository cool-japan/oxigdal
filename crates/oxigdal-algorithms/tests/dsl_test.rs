//! Integration tests for the Raster Algebra DSL

#![cfg(feature = "dsl")]

use oxigdal_algorithms::dsl::{OptLevel, RasterDsl};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

/// Helper to create test bands
fn create_test_bands(width: u64, height: u64, num_bands: usize) -> Vec<RasterBuffer> {
    let mut bands = Vec::new();
    for i in 0..num_bands {
        let mut band = RasterBuffer::zeros(width, height, RasterDataType::Float32);
        for y in 0..height {
            for x in 0..width {
                let value = ((x + y + i as u64) % 100) as f64;
                let _ = band.set_pixel(x, y, value);
            }
        }
        bands.push(band);
    }
    bands
}

#[test]
fn test_simple_arithmetic() {
    let bands = create_test_bands(10, 10, 2);
    let dsl = RasterDsl::new();

    let result = dsl.execute("B1 + B2", &bands);
    assert!(result.is_ok());

    let result = dsl.execute("B1 - B2", &bands);
    assert!(result.is_ok());

    let result = dsl.execute("B1 * B2", &bands);
    assert!(result.is_ok());

    let result = dsl.execute("B1 / B2", &bands);
    assert!(result.is_ok());
}

#[test]
fn test_ndvi_calculation() {
    let bands = create_test_bands(10, 10, 2);
    let dsl = RasterDsl::new();

    let result = dsl.execute("(B1 - B2) / (B1 + B2)", &bands);
    assert!(result.is_ok());

    if let Ok(raster) = result {
        assert_eq!(raster.width(), 10);
        assert_eq!(raster.height(), 10);
    }
}

#[test]
fn test_mathematical_functions() {
    let bands = create_test_bands(10, 10, 1);
    let dsl = RasterDsl::new();

    // Square root
    let result = dsl.execute("sqrt(B1)", &bands);
    assert!(result.is_ok());

    // Trigonometric
    let result = dsl.execute("sin(B1)", &bands);
    assert!(result.is_ok());

    let result = dsl.execute("cos(B1)", &bands);
    assert!(result.is_ok());

    // Absolute value
    let result = dsl.execute("abs(B1 - 50)", &bands);
    assert!(result.is_ok());

    // Power
    let result = dsl.execute("pow(B1, 2)", &bands);
    assert!(result.is_ok());
}

#[test]
fn test_conditional_expressions() {
    let bands = create_test_bands(10, 10, 1);
    let dsl = RasterDsl::new();

    let result = dsl.execute("if B1 > 50 then 1 else 0", &bands);
    assert!(result.is_ok());

    let result = dsl.execute("if B1 > 75 then 1 else if B1 > 50 then 0.5 else 0", &bands);
    assert!(result.is_ok());
}

#[test]
fn test_logical_operations() {
    let bands = create_test_bands(10, 10, 2);
    let dsl = RasterDsl::new();

    let result = dsl.execute("if B1 > 50 && B2 > 50 then 1 else 0", &bands);
    assert!(result.is_ok());

    let result = dsl.execute("if B1 > 75 || B2 > 75 then 1 else 0", &bands);
    assert!(result.is_ok());
}

#[test]
fn test_complex_expression() {
    let bands = create_test_bands(10, 10, 3);
    let dsl = RasterDsl::new();

    let expr = r#"
        sqrt(B1 * B1 + B2 * B2 + B3 * B3)
    "#;

    let result = dsl.execute(expr, &bands);
    assert!(result.is_ok());
}

#[test]
fn test_variable_declarations() {
    let bands = create_test_bands(10, 10, 2);
    let dsl = RasterDsl::new();

    let program = r#"
        let ndvi = (B1 - B2) / (B1 + B2);
        ndvi;
    "#;

    let result = dsl.execute(program, &bands);
    assert!(result.is_ok());
}

#[test]
fn test_multiple_variables() {
    let bands = create_test_bands(10, 10, 4);
    let dsl = RasterDsl::new();

    let program = r#"
        let ndvi = (B1 - B2) / (B1 + B2);
        let evi = 2.5 * ((B1 - B2) / (B1 + 6*B2 - 7.5*B3 + 1));
        let avg = (ndvi + evi) / 2;
        avg;
    "#;

    let result = dsl.execute(program, &bands);
    assert!(result.is_ok());
}

#[test]
fn test_optimization_none() {
    let bands = create_test_bands(10, 10, 1);
    let mut dsl = RasterDsl::new();
    dsl.set_opt_level(OptLevel::None);

    let result = dsl.execute("B1 + 0", &bands);
    assert!(result.is_ok());
}

#[test]
fn test_optimization_basic() {
    let bands = create_test_bands(10, 10, 1);
    let mut dsl = RasterDsl::new();
    dsl.set_opt_level(OptLevel::Basic);

    let result = dsl.execute("2 + 3 * 4", &bands);
    assert!(result.is_ok());
}

#[test]
fn test_optimization_standard() {
    let bands = create_test_bands(10, 10, 1);
    let mut dsl = RasterDsl::new();
    dsl.set_opt_level(OptLevel::Standard);

    // Should optimize x + 0 to x
    let result = dsl.execute("B1 + 0", &bands);
    assert!(result.is_ok());

    // Should optimize x * 1 to x
    let result = dsl.execute("B1 * 1", &bands);
    assert!(result.is_ok());

    // Should optimize x * 0 to 0
    let result = dsl.execute("B1 * 0", &bands);
    assert!(result.is_ok());
}

#[test]
fn test_optimization_aggressive() {
    let bands = create_test_bands(10, 10, 1);
    let mut dsl = RasterDsl::new();
    dsl.set_opt_level(OptLevel::Aggressive);

    let result = dsl.execute("(B1 + B1) / 2", &bands);
    assert!(result.is_ok());
}

#[test]
fn test_compile_and_execute() {
    let bands = create_test_bands(10, 10, 2);
    let dsl = RasterDsl::new();

    let compiled = dsl.compile("(B1 - B2) / (B1 + B2)");
    assert!(compiled.is_ok());

    if let Ok(program) = compiled {
        let result = program.execute(&bands);
        assert!(result.is_ok());

        // Execute again with different data
        let bands2 = create_test_bands(10, 10, 2);
        let result2 = program.execute(&bands2);
        assert!(result2.is_ok());
    }
}

#[test]
fn test_function_list() {
    let dsl = RasterDsl::new();
    let functions = dsl.list_functions();

    assert!(!functions.is_empty());
    assert!(functions.contains(&"sqrt"));
    assert!(functions.contains(&"sin"));
    assert!(functions.contains(&"cos"));
    assert!(functions.contains(&"mean"));
    assert!(functions.contains(&"max"));
    assert!(functions.contains(&"min"));
}

#[test]
fn test_comparison_operators() {
    let bands = create_test_bands(10, 10, 2);
    let dsl = RasterDsl::new();

    let result = dsl.execute("if B1 > B2 then 1 else 0", &bands);
    assert!(result.is_ok());

    let result = dsl.execute("if B1 >= B2 then 1 else 0", &bands);
    assert!(result.is_ok());

    let result = dsl.execute("if B1 < B2 then 1 else 0", &bands);
    assert!(result.is_ok());

    let result = dsl.execute("if B1 <= B2 then 1 else 0", &bands);
    assert!(result.is_ok());

    let result = dsl.execute("if B1 == B2 then 1 else 0", &bands);
    assert!(result.is_ok());

    let result = dsl.execute("if B1 != B2 then 1 else 0", &bands);
    assert!(result.is_ok());
}

#[test]
fn test_parentheses_precedence() {
    let bands = create_test_bands(10, 10, 2);
    let dsl = RasterDsl::new();

    let result1 = dsl.execute("(B1 + B2) * 2", &bands);
    assert!(result1.is_ok());

    let result2 = dsl.execute("B1 + B2 * 2", &bands);
    assert!(result2.is_ok());

    // Results should be different due to precedence
}

#[test]
fn test_min_max_functions() {
    let bands = create_test_bands(10, 10, 3);
    let dsl = RasterDsl::new();

    let result = dsl.execute("max(50, 75, 25)", &bands);
    assert!(result.is_ok());

    let result = dsl.execute("min(50, 75, 25)", &bands);
    assert!(result.is_ok());
}

#[test]
fn test_clamp_function() {
    let bands = create_test_bands(10, 10, 1);
    let dsl = RasterDsl::new();

    let result = dsl.execute("clamp(150, 0, 100)", &bands);
    assert!(result.is_ok());
}

#[test]
fn test_nested_conditionals() {
    let bands = create_test_bands(10, 10, 1);
    let dsl = RasterDsl::new();

    let expr = r#"
        if B1 > 80 then
            100
        else if B1 > 60 then
            75
        else if B1 > 40 then
            50
        else if B1 > 20 then
            25
        else
            0
    "#;

    let result = dsl.execute(expr, &bands);
    assert!(result.is_ok());
}

#[test]
fn test_complex_vegetation_analysis() {
    let bands = create_test_bands(100, 100, 8);
    let dsl = RasterDsl::new();

    let program = r#"
        let ndvi = (B8 - B4) / (B8 + B4);
        let evi = 2.5 * ((B8 - B4) / (B8 + 6*B4 - 7.5*B2 + 1));

        if ndvi > 0.6 && evi > 0.5 then
            1.0
        else if ndvi > 0.3 then
            0.5
        else
            0.0;
    "#;

    let result = dsl.execute(program, &bands);
    assert!(result.is_ok());
}

#[test]
fn test_error_undefined_band() {
    let bands = create_test_bands(10, 10, 2);
    let dsl = RasterDsl::new();

    let result = dsl.execute("B5", &bands);
    assert!(result.is_err());
}

#[test]
fn test_error_invalid_syntax() {
    let bands = create_test_bands(10, 10, 2);
    let dsl = RasterDsl::new();

    // Test truly invalid syntax - incomplete expression
    let result = dsl.execute("B1 +", &bands);
    assert!(result.is_err());

    // Test invalid operator sequence
    let result = dsl.execute("B1 * / B2", &bands);
    assert!(result.is_err());
}

#[test]
fn test_error_type_mismatch() {
    let bands = create_test_bands(10, 10, 2);
    let dsl = RasterDsl::new();

    // This should work as numbers can be compared
    let result = dsl.execute("if 5 > 3 then B1 else B2", &bands);
    assert!(result.is_ok());
}

#[test]
fn test_power_operator() {
    let bands = create_test_bands(10, 10, 1);
    let dsl = RasterDsl::new();

    let result = dsl.execute("B1 ^ 2", &bands);
    assert!(result.is_ok());

    let result = dsl.execute("pow(B1, 2)", &bands);
    assert!(result.is_ok());
}

#[test]
fn test_modulo_operator() {
    let bands = create_test_bands(10, 10, 1);
    let dsl = RasterDsl::new();

    let result = dsl.execute("B1 % 10", &bands);
    assert!(result.is_ok());
}

#[test]
fn test_unary_operators() {
    let bands = create_test_bands(10, 10, 1);
    let dsl = RasterDsl::new();

    let result = dsl.execute("-B1", &bands);
    assert!(result.is_ok());

    let result = dsl.execute("+B1", &bands);
    assert!(result.is_ok());

    let result = dsl.execute("--B1", &bands);
    assert!(result.is_ok());
}
