//! Integration tests for GPU expansion features:
//! - GpuReprojector (CPU fallback path)
//! - GpuAlgebra / AlgebraOp / BandExpression
//! - ShaderRegistry
//! - GpuCapabilities

use oxigdal_gpu::{
    algebra::{AlgebraOp, BandExpression, GpuAlgebra},
    reprojection::{GpuReprojector, ReprojectionConfig, ResampleMethod},
    webgpu_compat::{GpuCapabilities, ShaderRegistry},
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a square ReprojectionConfig with a simple 1:1 identity transform.
fn identity_config(size: u32) -> ReprojectionConfig {
    ReprojectionConfig {
        src_width: size,
        src_height: size,
        dst_width: size,
        dst_height: size,
        // src_gt: pixel → geo: x_geo = 0 + col*1 + row*0, y_geo = 0 + col*0 + row*1
        src_geotransform: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        // dst_inv_gt: geo = 0 + dst_x*1 + dst_y*0 etc (identity)
        dst_inv_geotransform: [0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        resample_method: ResampleMethod::NearestNeighbor,
        nodata: None,
    }
}

fn make_ramp(n: usize) -> Vec<f32> {
    (0..n).map(|i| i as f32).collect()
}

// ---------------------------------------------------------------------------
// GpuReprojector construction
// ---------------------------------------------------------------------------

#[test]
fn test_reprojector_new() {
    let cfg = identity_config(8);
    let r = GpuReprojector::new(cfg);
    assert_eq!(r.config().src_width, 8);
    assert_eq!(r.config().src_height, 8);
}

#[test]
fn test_reprojector_config_accessor() {
    let cfg = identity_config(16);
    let r = GpuReprojector::new(cfg);
    assert_eq!(r.config().dst_width, 16);
    assert_eq!(r.config().dst_height, 16);
    assert_eq!(r.config().resample_method, ResampleMethod::NearestNeighbor);
}

#[test]
fn test_reprojector_nodata_none() {
    let cfg = identity_config(4);
    assert!(cfg.nodata.is_none());
}

// ---------------------------------------------------------------------------
// reproject_cpu — identity (same CRS)
// ---------------------------------------------------------------------------

#[test]
fn test_reproject_cpu_identity_size() {
    let size = 4u32;
    let src = make_ramp((size * size) as usize);
    let r = GpuReprojector::new(identity_config(size));
    let dst = r.reproject_cpu(&src).expect("reproject_cpu should succeed");
    assert_eq!(dst.len(), (size * size) as usize);
}

#[test]
fn test_reproject_cpu_identity_values() {
    // With the identity geotransform the output should contain the source values.
    let size = 4u32;
    let src: Vec<f32> = (0..(size * size)).map(|i| i as f32 * 2.0).collect();
    let r = GpuReprojector::new(identity_config(size));
    let dst = r.reproject_cpu(&src).expect("reproject_cpu failed");
    // Check a sample of pixels
    assert!((dst[0] - src[0]).abs() < 1.0);
    assert!((dst[5] - src[5]).abs() < 1.0);
}

// ---------------------------------------------------------------------------
// reproject_cpu — 2× downscale
// ---------------------------------------------------------------------------

#[test]
fn test_reproject_cpu_downscale_output_size() {
    let src_size = 8u32;
    let dst_size = 4u32;
    let src = make_ramp((src_size * src_size) as usize);
    let cfg = ReprojectionConfig {
        src_width: src_size,
        src_height: src_size,
        dst_width: dst_size,
        dst_height: dst_size,
        src_geotransform: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        dst_inv_geotransform: [0.0, 2.0, 0.0, 0.0, 0.0, 2.0],
        resample_method: ResampleMethod::NearestNeighbor,
        nodata: None,
    };
    let r = GpuReprojector::new(cfg);
    let dst = r.reproject_cpu(&src).expect("downscale failed");
    assert_eq!(dst.len(), (dst_size * dst_size) as usize);
}

#[test]
fn test_reproject_cpu_wrong_src_len_errors() {
    let cfg = identity_config(4);
    let r = GpuReprojector::new(cfg);
    assert!(r.reproject_cpu(&[1.0_f32, 2.0]).is_err());
}

#[test]
fn test_reproject_cpu_bilinear_identity() {
    let size = 4u32;
    let src = make_ramp((size * size) as usize);
    let mut cfg = identity_config(size);
    cfg.resample_method = ResampleMethod::Bilinear;
    let r = GpuReprojector::new(cfg);
    let dst = r.reproject_cpu(&src).expect("bilinear failed");
    assert_eq!(dst.len(), (size * size) as usize);
}

// ---------------------------------------------------------------------------
// AlgebraOp via GpuAlgebra::execute
// ---------------------------------------------------------------------------

#[test]
fn test_algebra_add() {
    let a = vec![1.0_f32, 2.0, 3.0];
    let b = vec![4.0_f32, 5.0, 6.0];
    let out = GpuAlgebra::execute(&a, Some(&b), AlgebraOp::Add, None).expect("add failed");
    assert_eq!(out, vec![5.0, 7.0, 9.0]);
}

#[test]
fn test_algebra_subtract() {
    let a = vec![10.0_f32, 20.0];
    let b = vec![3.0_f32, 7.0];
    let out = GpuAlgebra::execute(&a, Some(&b), AlgebraOp::Subtract, None).expect("sub failed");
    assert_eq!(out, vec![7.0, 13.0]);
}

#[test]
fn test_algebra_multiply() {
    let a = vec![2.0_f32, 3.0];
    let b = vec![5.0_f32, 4.0];
    let out = GpuAlgebra::execute(&a, Some(&b), AlgebraOp::Multiply, None).expect("mul failed");
    assert_eq!(out, vec![10.0, 12.0]);
}

#[test]
fn test_algebra_divide() {
    let a = vec![10.0_f32, 9.0];
    let b = vec![2.0_f32, 3.0];
    let out = GpuAlgebra::execute(&a, Some(&b), AlgebraOp::Divide, None).expect("div failed");
    assert!((out[0] - 5.0).abs() < 1e-5);
    assert!((out[1] - 3.0).abs() < 1e-5);
}

#[test]
fn test_algebra_divide_by_zero_outputs_nodata() {
    let a = vec![1.0_f32];
    let b = vec![0.0_f32];
    let nodata = -9999.0;
    let out = GpuAlgebra::execute(&a, Some(&b), AlgebraOp::Divide, Some(nodata))
        .expect("div-by-zero failed");
    assert!((out[0] - nodata).abs() < 1e-5);
}

#[test]
fn test_algebra_min() {
    let a = vec![3.0_f32, 1.0];
    let b = vec![1.0_f32, 5.0];
    let out = GpuAlgebra::execute(&a, Some(&b), AlgebraOp::Min, None).expect("min failed");
    assert_eq!(out, vec![1.0, 1.0]);
}

#[test]
fn test_algebra_max() {
    let a = vec![3.0_f32, 1.0];
    let b = vec![1.0_f32, 5.0];
    let out = GpuAlgebra::execute(&a, Some(&b), AlgebraOp::Max, None).expect("max failed");
    assert_eq!(out, vec![3.0, 5.0]);
}

#[test]
fn test_algebra_sqrt() {
    let a = vec![4.0_f32, 9.0, 0.0];
    let out = GpuAlgebra::execute(&a, None, AlgebraOp::Sqrt, None).expect("sqrt failed");
    assert!((out[0] - 2.0).abs() < 1e-5);
    assert!((out[1] - 3.0).abs() < 1e-5);
    assert!((out[2] - 0.0).abs() < 1e-5);
}

#[test]
fn test_algebra_sqrt_negative_clamped_to_zero() {
    let a = vec![-4.0_f32];
    let out = GpuAlgebra::execute(&a, None, AlgebraOp::Sqrt, None).expect("sqrt neg failed");
    assert!((out[0] - 0.0).abs() < 1e-5);
}

#[test]
fn test_algebra_abs() {
    let a = vec![-3.0_f32, 5.0, -0.5];
    let out = GpuAlgebra::execute(&a, None, AlgebraOp::Abs, None).expect("abs failed");
    assert_eq!(out, vec![3.0, 5.0, 0.5]);
}

#[test]
fn test_algebra_power() {
    let a = vec![2.0_f32, 3.0];
    let out = GpuAlgebra::execute(&a, None, AlgebraOp::Power(3.0), None).expect("pow failed");
    assert!((out[0] - 8.0).abs() < 1e-4);
    assert!((out[1] - 27.0).abs() < 1e-4);
}

#[test]
fn test_algebra_clamp() {
    let a = vec![-1.0_f32, 0.5, 2.0];
    let out = GpuAlgebra::execute(&a, None, AlgebraOp::Clamp { min: 0.0, max: 1.0 }, None)
        .expect("clamp failed");
    assert_eq!(out, vec![0.0, 0.5, 1.0]);
}

#[test]
fn test_algebra_normalize() {
    let a = vec![0.0_f32, 50.0, 100.0];
    let op = AlgebraOp::Normalize {
        src_min: 0.0,
        src_max: 100.0,
        dst_min: 0.0,
        dst_max: 1.0,
    };
    let out = GpuAlgebra::execute(&a, None, op, None).expect("normalize failed");
    assert!((out[0] - 0.0).abs() < 1e-5);
    assert!((out[1] - 0.5).abs() < 1e-5);
    assert!((out[2] - 1.0).abs() < 1e-5);
}

// ---------------------------------------------------------------------------
// Nodata passthrough in algebra
// ---------------------------------------------------------------------------

#[test]
fn test_algebra_nodata_passthrough_band_a() {
    let nodata = -9999.0_f32;
    let a = vec![nodata, 2.0, 3.0];
    let b = vec![1.0_f32, 1.0, 1.0];
    let out = GpuAlgebra::execute(&a, Some(&b), AlgebraOp::Add, Some(nodata))
        .expect("nodata passthrough failed");
    assert!((out[0] - nodata).abs() < 1e-5);
    assert!((out[1] - 3.0).abs() < 1e-5);
}

#[test]
fn test_algebra_nodata_passthrough_band_b() {
    let nodata = -9999.0_f32;
    let a = vec![1.0_f32, 2.0];
    let b = vec![nodata, 5.0];
    let out = GpuAlgebra::execute(&a, Some(&b), AlgebraOp::Multiply, Some(nodata))
        .expect("nodata passthrough band b failed");
    assert!((out[0] - nodata).abs() < 1e-5);
    assert!((out[1] - 10.0).abs() < 1e-5);
}

#[test]
fn test_algebra_empty_band_errors() {
    let result = GpuAlgebra::execute(&[], None, AlgebraOp::Abs, None);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// BandExpression evaluation
// ---------------------------------------------------------------------------

#[test]
fn test_expr_band() {
    let expr = BandExpression::Band(0);
    assert!((expr.evaluate(&[42.0]).expect("band eval") - 42.0).abs() < 1e-5);
}

#[test]
#[allow(clippy::approx_constant)]
fn test_expr_constant() {
    let expr = BandExpression::Constant(3.14);
    assert!((expr.evaluate(&[]).expect("constant eval") - 3.14).abs() < 1e-5);
}

#[test]
fn test_expr_add() {
    let expr = BandExpression::Add(
        Box::new(BandExpression::Band(0)),
        Box::new(BandExpression::Band(1)),
    );
    assert!((expr.evaluate(&[3.0, 4.0]).expect("add eval") - 7.0).abs() < 1e-5);
}

#[test]
fn test_expr_sub() {
    let expr = BandExpression::Sub(
        Box::new(BandExpression::Band(0)),
        Box::new(BandExpression::Constant(1.0)),
    );
    assert!((expr.evaluate(&[5.0]).expect("sub eval") - 4.0).abs() < 1e-5);
}

#[test]
fn test_expr_mul() {
    let expr = BandExpression::Mul(
        Box::new(BandExpression::Constant(2.0)),
        Box::new(BandExpression::Band(0)),
    );
    assert!((expr.evaluate(&[7.0]).expect("mul eval") - 14.0).abs() < 1e-5);
}

#[test]
fn test_expr_div() {
    let expr = BandExpression::Div(
        Box::new(BandExpression::Band(0)),
        Box::new(BandExpression::Band(1)),
    );
    assert!((expr.evaluate(&[10.0, 4.0]).expect("div eval") - 2.5).abs() < 1e-5);
}

#[test]
fn test_expr_sqrt() {
    let expr = BandExpression::Sqrt(Box::new(BandExpression::Band(0)));
    assert!((expr.evaluate(&[16.0]).expect("sqrt eval") - 4.0).abs() < 1e-5);
}

#[test]
fn test_expr_abs() {
    let expr = BandExpression::Abs(Box::new(BandExpression::Band(0)));
    assert!((expr.evaluate(&[-7.0]).expect("abs eval") - 7.0).abs() < 1e-5);
}

#[test]
fn test_expr_neg() {
    let expr = BandExpression::Neg(Box::new(BandExpression::Band(0)));
    assert!((expr.evaluate(&[5.0]).expect("neg eval") - (-5.0)).abs() < 1e-5);
}

// ---------------------------------------------------------------------------
// Nested expression
// ---------------------------------------------------------------------------

#[test]
fn test_expr_nested() {
    // 2 * B(0) + B(1)
    let expr = BandExpression::Add(
        Box::new(BandExpression::Mul(
            Box::new(BandExpression::Constant(2.0)),
            Box::new(BandExpression::Band(0)),
        )),
        Box::new(BandExpression::Band(1)),
    );
    // 2 * 3 + 4 = 10
    assert!((expr.evaluate(&[3.0, 4.0]).expect("nested eval") - 10.0).abs() < 1e-5);
}

#[test]
fn test_expr_deeply_nested() {
    // sqrt(abs(neg(B(0)))) = sqrt(abs(-9)) = sqrt(9) = 3
    let expr = BandExpression::Sqrt(Box::new(BandExpression::Abs(Box::new(
        BandExpression::Neg(Box::new(BandExpression::Band(0))),
    ))));
    assert!((expr.evaluate(&[9.0]).expect("deep nested") - 3.0).abs() < 1e-5);
}

// ---------------------------------------------------------------------------
// Error cases
// ---------------------------------------------------------------------------

#[test]
fn test_expr_band_out_of_range() {
    let expr = BandExpression::Band(3);
    assert!(expr.evaluate(&[1.0, 2.0]).is_err());
}

#[test]
fn test_expr_div_by_zero_errors() {
    let expr = BandExpression::Div(
        Box::new(BandExpression::Constant(5.0)),
        Box::new(BandExpression::Constant(0.0)),
    );
    assert!(expr.evaluate(&[]).is_err());
}

// ---------------------------------------------------------------------------
// evaluate_expression — multi-band
// ---------------------------------------------------------------------------

#[test]
fn test_evaluate_expression_simple() {
    let red = vec![0.2_f32, 0.4];
    let nir = vec![0.6_f32, 0.8];
    // NDVI = (NIR - Red) / (NIR + Red)
    let ndvi_expr = BandExpression::Div(
        Box::new(BandExpression::Sub(
            Box::new(BandExpression::Band(1)),
            Box::new(BandExpression::Band(0)),
        )),
        Box::new(BandExpression::Add(
            Box::new(BandExpression::Band(1)),
            Box::new(BandExpression::Band(0)),
        )),
    );
    let bands: &[&[f32]] = &[&red, &nir];
    let result = GpuAlgebra::evaluate_expression(bands, &ndvi_expr, None).expect("ndvi failed");
    // Pixel 0: (0.6-0.2)/(0.6+0.2) = 0.4/0.8 = 0.5
    assert!((result[0] - 0.5).abs() < 1e-5);
    // Pixel 1: (0.8-0.4)/(0.8+0.4) = 0.4/1.2 ≈ 0.333
    assert!((result[1] - (0.4 / 1.2)).abs() < 1e-5);
}

#[test]
fn test_evaluate_expression_no_bands_errors() {
    let expr = BandExpression::Constant(1.0);
    assert!(GpuAlgebra::evaluate_expression(&[], &expr, None).is_err());
}

#[test]
fn test_evaluate_expression_nodata_propagation() {
    let nodata = -9999.0_f32;
    let a = vec![nodata, 2.0];
    let b = vec![1.0_f32, 3.0];
    let expr = BandExpression::Add(
        Box::new(BandExpression::Band(0)),
        Box::new(BandExpression::Band(1)),
    );
    let bands: &[&[f32]] = &[&a, &b];
    let result = GpuAlgebra::evaluate_expression(bands, &expr, Some(nodata))
        .expect("nodata propagation failed");
    assert!((result[0] - nodata).abs() < 1e-5);
    assert!((result[1] - 5.0).abs() < 1e-5);
}

#[test]
fn test_ndvi_band_expression() {
    // Separate NDVI test with known values
    let red = vec![100.0_f32, 50.0];
    let nir = vec![200.0_f32, 150.0];
    let ndvi_expr = BandExpression::Div(
        Box::new(BandExpression::Sub(
            Box::new(BandExpression::Band(1)), // NIR
            Box::new(BandExpression::Band(0)), // Red
        )),
        Box::new(BandExpression::Add(
            Box::new(BandExpression::Band(1)),
            Box::new(BandExpression::Band(0)),
        )),
    );
    let bands: &[&[f32]] = &[&red, &nir];
    let result = GpuAlgebra::evaluate_expression(bands, &ndvi_expr, None).expect("ndvi band expr");
    // (200-100)/(200+100) = 100/300 = 0.333...
    assert!((result[0] - (100.0 / 300.0)).abs() < 1e-5);
    // (150-50)/(150+50) = 100/200 = 0.5
    assert!((result[1] - 0.5).abs() < 1e-5);
}

// ---------------------------------------------------------------------------
// ShaderRegistry
// ---------------------------------------------------------------------------

#[test]
fn test_shader_registry_reproject_some() {
    assert!(ShaderRegistry::get("reproject").is_some());
}

#[test]
fn test_shader_registry_raster_algebra_some() {
    assert!(ShaderRegistry::get("raster_algebra").is_some());
}

#[test]
fn test_shader_registry_hillshade_some() {
    assert!(ShaderRegistry::get("hillshade").is_some());
}

#[test]
fn test_shader_registry_unknown_none() {
    assert!(ShaderRegistry::get("does_not_exist").is_none());
    assert!(ShaderRegistry::get("").is_none());
}

#[test]
fn test_shader_registry_list_contains_all() {
    let list = ShaderRegistry::list();
    assert!(list.contains(&"reproject"));
    assert!(list.contains(&"raster_algebra"));
    assert!(list.contains(&"hillshade"));
}

#[test]
fn test_shader_registry_list_length() {
    assert_eq!(ShaderRegistry::list().len(), 3);
}

#[test]
fn test_shader_content_reproject_wgsl() {
    let src = ShaderRegistry::get("reproject").expect("reproject shader missing");
    assert!(src.contains("@compute"));
    assert!(src.contains("ReprojParams"));
}

#[test]
fn test_shader_content_hillshade_wgsl() {
    let src = ShaderRegistry::get("hillshade").expect("hillshade shader missing");
    assert!(src.contains("HillshadeParams"));
    assert!(src.contains("get_elev"));
}

#[test]
fn test_shader_content_algebra_wgsl() {
    let src = ShaderRegistry::get("raster_algebra").expect("algebra shader missing");
    assert!(src.contains("AlgebraParams"));
    assert!(src.contains("band_a"));
}

// ---------------------------------------------------------------------------
// GpuCapabilities
// ---------------------------------------------------------------------------

#[test]
fn test_capabilities_default_has_compute() {
    let caps = GpuCapabilities::default();
    assert!(caps.has_compute);
}

#[test]
fn test_capabilities_default_has_texture_float() {
    let caps = GpuCapabilities::default();
    assert!(caps.has_texture_float);
}

#[test]
fn test_capabilities_default_workgroup_size() {
    let caps = GpuCapabilities::default();
    assert_eq!(caps.max_workgroup_size, 256);
}

#[test]
fn test_capabilities_default_buffer_size() {
    let caps = GpuCapabilities::default();
    assert_eq!(caps.max_buffer_size, 256 * 1024 * 1024);
}

#[test]
fn test_capabilities_webgpu_buffer_size() {
    let caps = GpuCapabilities::webgpu_conservative();
    assert_eq!(caps.max_buffer_size, 128 * 1024 * 1024);
}

#[test]
fn test_capabilities_webgpu_workgroup_size() {
    let caps = GpuCapabilities::webgpu_conservative();
    assert_eq!(caps.max_workgroup_size, 256);
}

#[test]
fn test_validate_buffer_size_within_limit() {
    let caps = GpuCapabilities::default();
    assert!(caps.validate_buffer_size(1024 * 1024));
}

#[test]
fn test_validate_buffer_size_exactly_at_limit() {
    let caps = GpuCapabilities::default();
    assert!(caps.validate_buffer_size(caps.max_buffer_size));
}

#[test]
fn test_validate_buffer_size_exceeds_limit() {
    let caps = GpuCapabilities::default();
    assert!(!caps.validate_buffer_size(caps.max_buffer_size + 1));
}

#[test]
fn test_validate_workgroup_within_limit() {
    let caps = GpuCapabilities::default();
    assert!(caps.validate_workgroup(64));
}

#[test]
fn test_validate_workgroup_exactly_at_limit() {
    let caps = GpuCapabilities::default();
    assert!(caps.validate_workgroup(caps.max_workgroup_size));
}

#[test]
fn test_validate_workgroup_exceeds_limit() {
    let caps = GpuCapabilities::default();
    assert!(!caps.validate_workgroup(caps.max_workgroup_size + 1));
}

#[test]
fn test_webgpu_validate_buffer_size_128mb() {
    let caps = GpuCapabilities::webgpu_conservative();
    assert!(caps.validate_buffer_size(128 * 1024 * 1024));
    assert!(!caps.validate_buffer_size(128 * 1024 * 1024 + 1));
}
