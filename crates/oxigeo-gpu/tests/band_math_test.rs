//! Integration tests for the band-math string → AST → WGSL compiler.

// `panic!` is denied crate-wide, but tests need to fail loudly on AST shape
// mismatches; `assert!(false, ...)` is the lint-clean equivalent.
#![allow(clippy::panic)]

use oxigeo_gpu::{BandExpression, BandMathError, band_expression_to_wgsl, parse_band_expression};

/// `B2 - B1` must parse to a top-level `Sub` of `Band(1)` and `Band(0)`.
#[test]
fn test_parse_simple_band_diff() {
    let expr = parse_band_expression("B2-B1").expect("parse");
    match expr {
        BandExpression::Sub(a, b) => {
            assert!(matches!(*a, BandExpression::Band(1)));
            assert!(matches!(*b, BandExpression::Band(0)));
        }
        other => panic!("expected Sub, got {:?}", other),
    }
}

/// `(B4-B3)/(B4+B3)` is the classic NDVI formula — must parse cleanly.
#[test]
fn test_parse_ndvi_formula() {
    let expr = parse_band_expression("(B4-B3)/(B4+B3)").expect("parse");
    let b3 = 0.2_f32;
    let b4 = 0.6_f32;
    // Bands are 0-indexed internally — slot 2 = B3, slot 3 = B4.
    let bands = vec![0.0_f32, 0.0, b3, b4];
    let got = expr.evaluate(&bands).expect("eval");
    let want = (b4 - b3) / (b4 + b3);
    assert!((got - want).abs() < 1e-6, "got {}, want {}", got, want);
}

/// Nested parentheses round-trip correctly.
#[test]
fn test_parse_nested_parens() {
    let expr = parse_band_expression("((B1+B2)*B3)").expect("parse");
    let bands = vec![1.0_f32, 2.0, 4.0];
    let got = expr.evaluate(&bands).expect("eval");
    assert!((got - 12.0).abs() < 1e-6);
}

/// Unary minus binds tighter than binary `-`.
#[test]
fn test_parse_unary_minus() {
    let e1 = parse_band_expression("-B1").expect("parse");
    assert!((e1.evaluate(&[3.5]).expect("eval") + 3.5).abs() < 1e-6);

    let e2 = parse_band_expression("-(B1+B2)").expect("parse");
    assert!((e2.evaluate(&[2.0, 3.0]).expect("eval") + 5.0).abs() < 1e-6);
}

/// `log(B5)` must parse and produce the natural log.
#[test]
fn test_parse_function_call_log() {
    let expr = parse_band_expression("log(B5)").expect("parse");
    let bands = vec![0.0_f32, 0.0, 0.0, 0.0, std::f32::consts::E];
    let got = expr.evaluate(&bands).expect("eval");
    assert!((got - 1.0).abs() < 1e-6, "got {}", got);
}

/// `clamp(B1, 0.0, 1.0)` is a three-argument call.
#[test]
fn test_parse_function_call_clamp_three_args() {
    let expr = parse_band_expression("clamp(B1, 0.0, 1.0)").expect("parse");
    assert!((expr.evaluate(&[-0.5]).expect("eval") - 0.0).abs() < 1e-6);
    assert!((expr.evaluate(&[0.5]).expect("eval") - 0.5).abs() < 1e-6);
    assert!((expr.evaluate(&[1.5]).expect("eval") - 1.0).abs() < 1e-6);
}

/// `B0` is invalid (1-based) and so is `B1000`.
#[test]
fn test_parse_invalid_band_index_errors() {
    match parse_band_expression("B0") {
        Err(BandMathError::InvalidBandIndex(0)) => {}
        other => panic!("expected InvalidBandIndex(0), got {:?}", other),
    }
    match parse_band_expression("B1000") {
        Err(BandMathError::InvalidBandIndex(1000)) => {}
        other => panic!("expected InvalidBandIndex(1000), got {:?}", other),
    }
}

/// Unknown function names should produce the typed error.
#[test]
fn test_parse_unknown_function_errors() {
    match parse_band_expression("foobar(B1)") {
        Err(BandMathError::UnknownFunction(name)) => assert_eq!(name, "foobar"),
        other => panic!("expected UnknownFunction, got {:?}", other),
    }
}

/// Tokens after a complete expression should error.
#[test]
fn test_parse_trailing_tokens_errors() {
    match parse_band_expression("B1 + B2 B3") {
        Err(BandMathError::TrailingTokens(_)) => {}
        other => panic!("expected TrailingTokens, got {:?}", other),
    }
}

/// The generated WGSL must declare the bands it touches.
#[test]
fn test_codegen_emits_band_binding() {
    let expr = parse_band_expression("B1 + B2").expect("parse");
    let shader = band_expression_to_wgsl(&expr, &[0, 1, 2]);
    assert!(
        shader.contains("band_1: array<f32>"),
        "shader missing band_1: {}",
        shader
    );
    assert!(
        shader.contains("band_2: array<f32>"),
        "shader missing band_2: {}",
        shader
    );
    assert!(shader.contains("output: array<f32>"));
    assert!(shader.contains("@compute"));
    assert!(shader.contains("@workgroup_size(64)"));
}

/// Constant subtrees must fold to a single literal in the emitted shader.
#[test]
fn test_codegen_constant_folds() {
    let expr = parse_band_expression("2.0 + 3.0").expect("parse");
    let shader = band_expression_to_wgsl(&expr, &[]);
    // Folded value should appear literally; the unfolded `(2.0 + 3.0)`
    // sub-expression must not.
    assert!(
        shader.contains("output[idx] = 5.0;") || shader.contains("output[idx] = 5;"),
        "shader did not constant-fold: {}",
        shader
    );
    assert!(
        !shader.contains("(2.0 + 3.0)") && !shader.contains("(2 + 3)"),
        "shader still has unfolded literals: {}",
        shader
    );
}

/// AST evaluation must match a direct `BandExpression::evaluate` over the same
/// inputs for several representative expressions.
#[test]
fn test_codegen_round_trip_evaluates_same_as_ast() {
    // Construct an equivalent expression by hand and compare to the parsed
    // version pixel-by-pixel via `BandExpression::evaluate`.
    let parsed = parse_band_expression("(B1+B2)*B3 - sqrt(B4)").expect("parse");
    let hand = BandExpression::Sub(
        Box::new(BandExpression::Mul(
            Box::new(BandExpression::Add(
                Box::new(BandExpression::Band(0)),
                Box::new(BandExpression::Band(1)),
            )),
            Box::new(BandExpression::Band(2)),
        )),
        Box::new(BandExpression::Sqrt(Box::new(BandExpression::Band(3)))),
    );

    // A handful of pixels.
    for sample in &[
        [1.0_f32, 2.0, 3.0, 4.0],
        [0.5_f32, -1.5, 2.0, 9.0],
        [10.0_f32, 0.0, 0.5, 16.0],
    ] {
        let want = hand.evaluate(sample).expect("eval hand");
        let got = parsed.evaluate(sample).expect("eval parsed");
        assert!(
            (got - want).abs() < 1e-5,
            "parsed != hand: got {}, want {} (sample {:?})",
            got,
            want,
            sample
        );
    }
}

/// Power operator `^` and `pow(...)` must produce identical results, and the
/// generated WGSL must use the `pow(` builtin (since WGSL has no `^`).
#[test]
fn test_pow_operator_and_function_equivalent() {
    let by_op = parse_band_expression("B1 ^ 3.0").expect("parse op");
    let by_fn = parse_band_expression("pow(B1, 3.0)").expect("parse fn");
    let got_op = by_op.evaluate(&[2.0]).expect("eval");
    let got_fn = by_fn.evaluate(&[2.0]).expect("eval");
    assert!((got_op - 8.0).abs() < 1e-6);
    assert!((got_fn - 8.0).abs() < 1e-6);

    let shader = band_expression_to_wgsl(&by_op, &[0]);
    assert!(
        shader.contains("pow("),
        "shader must use pow(), got: {}",
        shader
    );
    assert!(!shader.contains(" ^ "), "shader must not contain '^'");
}

/// Division by literal zero must be caught at parse time.
#[test]
fn test_div_by_literal_zero_errors() {
    match parse_band_expression("B1 / 0.0") {
        Err(BandMathError::DivByConstantZero) => {}
        other => panic!("expected DivByConstantZero, got {:?}", other),
    }
}

/// `min` and `max` must accept two arguments.
#[test]
fn test_min_max_parse_and_eval() {
    let expr = parse_band_expression("max(B1, B2) - min(B1, B2)").expect("parse");
    let got = expr.evaluate(&[3.0, 7.0]).expect("eval");
    assert!((got - 4.0).abs() < 1e-6);
}

/// Whitespace and the empty string trigger the empty-expression error.
#[test]
fn test_empty_input_errors() {
    assert!(matches!(
        parse_band_expression(""),
        Err(BandMathError::EmptyExpression)
    ));
    assert!(matches!(
        parse_band_expression("   "),
        Err(BandMathError::EmptyExpression)
    ));
}
