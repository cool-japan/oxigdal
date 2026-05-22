//! Integration tests for the DEM ray-marching kernel.
//!
//! Pure-Rust tests (normalize3, bilinear sampling, CPU ray-march, config
//! validation, and shader source inspection) run unconditionally.  The GPU
//! integration test gracefully skips when no wgpu backend is available,
//! following the `try_gpu_context` pattern used elsewhere in this test suite.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::float_cmp,
    missing_docs
)]

use oxigdal_gpu::{
    DemRayMarcher, RayMarchConfig, make_ray_march_shader_source, normalize3, ray_march_cpu,
    sample_dem_bilinear,
};

// ─────────────────────────────────────────────────────────────────────────────
// Helper: try to create a GPU context without panicking.
// ─────────────────────────────────────────────────────────────────────────────

fn try_gpu_context() -> Option<std::sync::Arc<oxigdal_gpu::GpuContext>> {
    use std::panic::AssertUnwindSafe;
    std::panic::catch_unwind(AssertUnwindSafe(|| {
        pollster::block_on(oxigdal_gpu::GpuContext::new()).ok()
    }))
    .ok()
    .flatten()
    .map(std::sync::Arc::new)
}

// ─────────────────────────────────────────────────────────────────────────────
// normalize3 tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_normalize3_unit_vector_unchanged() {
    let v = normalize3([1.0, 0.0, 0.0]);
    assert!(
        (v[0] - 1.0).abs() < 1e-6,
        "v[0] should be 1.0, got {}",
        v[0]
    );
    assert!(v[1].abs() < 1e-6, "v[1] should be 0.0, got {}", v[1]);
    assert!(v[2].abs() < 1e-6, "v[2] should be 0.0, got {}", v[2]);
}

#[test]
fn test_normalize3_axis_aligned() {
    let v = normalize3([3.0, 0.0, 0.0]);
    assert!(
        (v[0] - 1.0).abs() < 1e-6,
        "v[0] should be 1.0, got {}",
        v[0]
    );
    assert!(v[1].abs() < 1e-6, "v[1] should be 0.0, got {}", v[1]);
    assert!(v[2].abs() < 1e-6, "v[2] should be 0.0, got {}", v[2]);
}

#[test]
fn test_normalize3_zero_returns_zero() {
    let v = normalize3([0.0, 0.0, 0.0]);
    assert_eq!(v, [0.0, 0.0, 0.0]);
}

// ─────────────────────────────────────────────────────────────────────────────
// sample_dem_bilinear tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_sample_dem_bilinear_corner() {
    // 2×2 raster: [1, 2; 3, 4] (row-major)
    let dem = [1.0_f32, 2.0, 3.0, 4.0];
    let v = sample_dem_bilinear(&dem, 2, 2, 0.0, 0.0);
    assert!(
        (v - 1.0).abs() < 1e-6,
        "top-left corner should be 1.0, got {}",
        v
    );
}

#[test]
fn test_sample_dem_bilinear_center() {
    // 2×2 raster: [0, 0; 0, 4] — only bottom-right is non-zero
    let dem = [0.0_f32, 0.0, 0.0, 4.0];
    let v = sample_dem_bilinear(&dem, 2, 2, 1.0, 1.0);
    assert!(
        (v - 4.0).abs() < 1e-6,
        "bottom-right corner should be 4.0, got {}",
        v
    );
}

#[test]
fn test_sample_dem_bilinear_out_of_bounds_clamps() {
    // Any out-of-bounds sample should clamp to the nearest valid pixel.
    let dem = [7.0_f32, 8.0, 9.0, 10.0];
    let v_neg = sample_dem_bilinear(&dem, 2, 2, -1.0, -1.0);
    // Clamped to (0,0) → dem[0] = 7.0
    assert!(
        (v_neg - 7.0).abs() < 1e-6,
        "out-of-bounds (-1,-1) should clamp to 7.0, got {}",
        v_neg
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// ray_march_cpu tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_ray_march_cpu_flat_dem_no_shadow() {
    // A perfectly flat DEM at elevation 0.  A vertical light (light_dir z > 0)
    // will never hit the terrain from below, so every pixel must be lit.
    let width = 8_u32;
    let height = 8_u32;
    let dem = vec![0.0_f32; (width * height) as usize];

    let config = RayMarchConfig {
        // Light pointing straight up-ish so ray_z increases each step → no shadow
        light_dir: [0.0, 0.0, 1.0],
        step_size: 1.0,
        max_steps: 32,
        vertical_exaggeration: 1.0,
        ..Default::default()
    };

    let result = ray_march_cpu(&dem, width, height, &config);

    assert_eq!(result.width, width);
    assert_eq!(result.height, height);
    assert_eq!(result.shaded.len(), (width * height) as usize);

    for (i, &s) in result.shaded.iter().enumerate() {
        assert!(
            (s - 1.0).abs() < 1e-6,
            "pixel {} should be lit (1.0), got {}",
            i,
            s
        );
    }
}

#[test]
fn test_ray_march_cpu_step_canyon_casts_shadow() {
    // A 4×2 DEM where the right half has a tall ridge (elevation 100).
    // With a light coming from the right (+X direction), pixels to the left
    // of the ridge must be shadowed.
    //
    // DEM layout (row-major, 4 cols × 2 rows):
    //   [ 0,  0, 100, 100 ]
    //   [ 0,  0, 100, 100 ]
    //
    // Light direction: (+1, 0, 0) — light comes from the east (right).
    // Step_size = 1.0, max_steps = 50.
    //
    // For pixel (0, 0) at elevation 0:
    //   Step 1: sample at x=1 → elev=0,    ray_z=0+0*1*1=0  → 0 <= 0 ok
    //   Step 2: sample at x=2 → elev=100,  ray_z=0           → 100 > 0 SHADOW!
    let width = 4_u32;
    let height = 2_u32;
    let dem = vec![0.0_f32, 0.0, 100.0, 100.0, 0.0, 0.0, 100.0, 100.0];

    let config = RayMarchConfig {
        // Light from the right (+x), no vertical component so ray_z stays at start_z
        light_dir: [1.0, 0.0, 0.0],
        step_size: 1.0,
        max_steps: 50,
        vertical_exaggeration: 1.0,
        ..Default::default()
    };

    let result = ray_march_cpu(&dem, width, height, &config);

    // Pixels at x=0 and x=1 should be shadowed by the ridge at x=2.
    for row in 0..height {
        let px0 = result.shaded[(row * width) as usize];
        let px1 = result.shaded[(row * width + 1) as usize];
        assert!(
            px0 < 0.5,
            "pixel ({},row={}) should be shadowed, got shaded={}",
            0,
            row,
            px0
        );
        assert!(
            px1 < 0.5,
            "pixel ({},row={}) should be shadowed, got shaded={}",
            1,
            row,
            px1
        );
        // Ridge pixels themselves are at max elevation, not shadowed by anything higher.
        let px2 = result.shaded[(row * width + 2) as usize];
        let px3 = result.shaded[(row * width + 3) as usize];
        assert!(
            px2 > 0.5 || px3 > 0.5,
            "at least one ridge pixel should be lit (row={}): px2={}, px3={}",
            row,
            px2,
            px3
        );
    }
}

#[test]
fn test_ray_march_cpu_depth_monotonic_with_height() {
    // A 3×1 DEM with increasing elevation: [0, 5, 10].
    // With light_dir=(1,0,0) and vertical_exaggeration=1:
    //   - px=0 (elev=0): ray will be blocked by px=1 (elev=5) very quickly → small depth
    //   - px=1 (elev=5): ray blocked by px=2 (elev=10) less quickly
    //   - px=2 (elev=10): no obstacle ahead → depth=1.0
    let width = 3_u32;
    let height = 1_u32;
    let dem = [0.0_f32, 5.0, 10.0];

    let config = RayMarchConfig {
        light_dir: [1.0, 0.0, 0.0],
        step_size: 0.5,
        max_steps: 100,
        vertical_exaggeration: 1.0,
        ..Default::default()
    };

    let result = ray_march_cpu(&dem, width, height, &config);

    // px=2 must be unblocked (depth = 1.0)
    assert!(
        (result.depth[2] - 1.0).abs() < 1e-6,
        "px=2 should have depth=1.0, got {}",
        result.depth[2]
    );

    // px=0 must be blocked before px=1 or at same step → depth[0] <= depth[1]
    // (both may be blocked, but px=0 sees the obstruction first or together)
    assert!(
        result.depth[0] <= result.depth[1] + 1e-6,
        "px=0 depth ({}) should be <= px=1 depth ({})",
        result.depth[0],
        result.depth[1]
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// RayMarchConfig::validate tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_ray_march_config_validate_rejects_zero_step() {
    let config = RayMarchConfig {
        step_size: 0.0,
        ..Default::default()
    };
    let result = config.validate(64, 64);
    assert!(result.is_err(), "step_size=0 should be rejected");
}

#[test]
fn test_ray_march_config_validate_rejects_zero_max_steps() {
    let config = RayMarchConfig {
        max_steps: 0,
        ..Default::default()
    };
    let result = config.validate(64, 64);
    assert!(result.is_err(), "max_steps=0 should be rejected");
}

// ─────────────────────────────────────────────────────────────────────────────
// Shader source inspection tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_ray_march_shader_source_contains_required_bindings() {
    let src = make_ray_march_shader_source();

    assert!(
        src.contains("@binding(0)"),
        "shader must have @binding(0); source:\n{}",
        &src[..src.len().min(500)]
    );
    assert!(
        src.contains("@compute"),
        "shader must contain @compute directive"
    );
    assert!(
        src.contains("dem"),
        "shader must reference the 'dem' buffer"
    );
    // Also verify the other required bindings are present.
    assert!(src.contains("@binding(1)"), "shader must have @binding(1)");
    assert!(src.contains("@binding(2)"), "shader must have @binding(2)");
    assert!(src.contains("@binding(3)"), "shader must have @binding(3)");
    assert!(
        src.contains("@workgroup_size(16, 16, 1)"),
        "shader must use workgroup_size(16,16,1)"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// GPU integration test (skipped if no GPU backend is available)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_ray_march_gpu_matches_cpu_when_backend_present() {
    let Some(ctx) = try_gpu_context() else {
        println!("No GPU backend available — skipping GPU integration test.");
        return;
    };

    let width = 16_u32;
    let height = 16_u32;
    let n = (width * height) as usize;

    // Build a simple DEM: zero except for a ridge in the middle column.
    let mut dem = vec![0.0_f32; n];
    for row in 0..height {
        dem[(row * width + width / 2) as usize] = 20.0;
    }

    let config = RayMarchConfig {
        light_dir: normalize3([1.0, 0.0, 0.0]),
        step_size: 0.5,
        max_steps: 64,
        vertical_exaggeration: 1.0,
        ..Default::default()
    };

    eprintln!("PHASE A: about to call ray_march_cpu");
    // CPU reference
    let cpu_result = ray_march_cpu(&dem, width, height, &config);
    eprintln!("PHASE B: ray_march_cpu returned, about to DemRayMarcher::new");

    // GPU result
    let marcher = DemRayMarcher::new(ctx).expect("DemRayMarcher::new should succeed");
    eprintln!("PHASE C: DemRayMarcher::new returned, about to march");
    let gpu_result = marcher
        .march(&dem, width, height, &config)
        .expect("DemRayMarcher::march should succeed");
    eprintln!("PHASE D: march returned");

    assert_eq!(gpu_result.width, width);
    assert_eq!(gpu_result.height, height);
    assert_eq!(gpu_result.shaded.len(), n);
    assert_eq!(gpu_result.depth.len(), n);

    // GPU shaded values must match CPU reference within float tolerance.
    let mut max_diff = 0.0_f32;
    for i in 0..n {
        let diff = (gpu_result.shaded[i] - cpu_result.shaded[i]).abs();
        if diff > max_diff {
            max_diff = diff;
        }
    }
    assert!(
        max_diff < 0.01,
        "GPU and CPU shaded buffers differ by {max_diff} (max allowed 0.01)"
    );
}
