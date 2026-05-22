//! Integration tests for strip-based parallel raster functions.
//!
//! All tests are gated on the `parallel` feature and compare parallel
//! output against the corresponding scalar functions pixel-by-pixel.

#![cfg(feature = "parallel")]
#![allow(clippy::expect_used)]

use oxigdal_algorithms::error::Result as AlgResult;
use oxigdal_algorithms::parallel::{FocalOp, focal_parallel, hillshade_parallel, slope_parallel};
use oxigdal_algorithms::raster::{
    FocalBoundaryMode, HillshadeParams, WindowShape, focal_max, focal_mean, focal_median,
    focal_min, focal_range, focal_stddev, focal_sum, hillshade, slope,
};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

type FocalScalarFn = fn(&RasterBuffer, &WindowShape, &FocalBoundaryMode) -> AlgResult<RasterBuffer>;

/// Maximum allowed absolute difference between parallel and scalar outputs
/// (expressed as relative tolerance relative to the scalar value).
const REL_TOL: f64 = 1e-9;

/// Fills a raster with a deterministic gradient pattern so results are
/// non-trivial (avoids the degenerate all-zeros case).
fn make_gradient_dem(w: u64, h: u64) -> RasterBuffer {
    let mut buf = RasterBuffer::zeros(w, h, RasterDataType::Float32);
    for y in 0..h {
        for x in 0..w {
            let v = 100.0 * (x as f64 / w as f64)
                + 50.0 * (y as f64 / h as f64)
                + 10.0 * ((x as f64 * 0.3).sin() + (y as f64 * 0.2).cos());
            buf.set_pixel(x, y, v).expect("in-bounds");
        }
    }
    buf
}

/// Compares two rasters pixel-by-pixel and panics if any pair exceeds `rel_tol`.
fn assert_rasters_close(label: &str, scalar: &RasterBuffer, parallel: &RasterBuffer, rel_tol: f64) {
    assert_eq!(scalar.width(), parallel.width(), "{label}: width mismatch");
    assert_eq!(
        scalar.height(),
        parallel.height(),
        "{label}: height mismatch"
    );

    for y in 0..scalar.height() {
        for x in 0..scalar.width() {
            let sv = scalar.get_pixel(x, y).expect("in-bounds");
            let pv = parallel.get_pixel(x, y).expect("in-bounds");

            // Both zero: ok.
            if sv == 0.0 && pv == 0.0 {
                continue;
            }
            // Both NaN: ok.
            if sv.is_nan() && pv.is_nan() {
                continue;
            }

            let diff = (sv - pv).abs();
            let denom = sv.abs().max(1e-12);
            assert!(
                diff / denom <= rel_tol,
                "{label}: mismatch at ({x},{y}): scalar={sv}, parallel={pv}, rel_diff={}",
                diff / denom
            );
        }
    }
}

// ---------------------------------------------------------------------------
// hillshade_parallel
// ---------------------------------------------------------------------------

#[test]
fn test_hillshade_parallel_matches_scalar() {
    let dem = make_gradient_dem(64, 64);
    let params = HillshadeParams::standard();

    let scalar_out = hillshade(&dem, params).expect("scalar hillshade");
    let parallel_out = hillshade_parallel(&dem, params).expect("parallel hillshade");

    assert_rasters_close("hillshade 64×64", &scalar_out, &parallel_out, REL_TOL);
}

#[test]
fn test_hillshade_parallel_non_standard_params() {
    let dem = make_gradient_dem(64, 48);
    let params = HillshadeParams::new(225.0, 30.0)
        .with_z_factor(2.0)
        .with_pixel_size(30.0);

    let scalar_out = hillshade(&dem, params).expect("scalar hillshade");
    let parallel_out = hillshade_parallel(&dem, params).expect("parallel hillshade");

    assert_rasters_close(
        "hillshade custom params",
        &scalar_out,
        &parallel_out,
        REL_TOL,
    );
}

// ---------------------------------------------------------------------------
// slope_parallel
// ---------------------------------------------------------------------------

#[test]
fn test_slope_parallel_matches_scalar() {
    let dem = make_gradient_dem(64, 64);

    let scalar_out = slope(&dem, 1.0, 1.0).expect("scalar slope");
    let parallel_out = slope_parallel(&dem, 1.0, 1.0).expect("parallel slope");

    assert_rasters_close("slope 64×64", &scalar_out, &parallel_out, REL_TOL);
}

#[test]
fn test_slope_parallel_z_factor() {
    let dem = make_gradient_dem(80, 64);

    let scalar_out = slope(&dem, 30.0, 2.5).expect("scalar slope");
    let parallel_out = slope_parallel(&dem, 30.0, 2.5).expect("parallel slope");

    assert_rasters_close("slope z_factor=2.5", &scalar_out, &parallel_out, REL_TOL);
}

// ---------------------------------------------------------------------------
// focal_parallel
// ---------------------------------------------------------------------------

#[test]
fn test_focal_parallel_mean_matches_scalar() {
    let src = make_gradient_dem(32, 32);
    let window = WindowShape::rectangular(3, 3).expect("valid window");
    let boundary = FocalBoundaryMode::Reflect;

    let scalar_out = focal_mean(&src, &window, &boundary).expect("scalar focal_mean");
    let parallel_out =
        focal_parallel(&src, &window, FocalOp::Mean, &boundary).expect("parallel focal_mean");

    assert_rasters_close("focal_mean 32×32 3×3", &scalar_out, &parallel_out, REL_TOL);
}

#[test]
fn test_focal_parallel_all_ops_match_scalar() {
    let src = make_gradient_dem(32, 32);
    let window = WindowShape::rectangular(3, 3).expect("valid window");
    let boundary = FocalBoundaryMode::Ignore;

    let cases: &[(&str, FocalOp, FocalScalarFn)] = &[
        ("mean", FocalOp::Mean, focal_mean),
        ("median", FocalOp::Median, focal_median),
        ("min", FocalOp::Min, focal_min),
        ("max", FocalOp::Max, focal_max),
        ("sum", FocalOp::Sum, focal_sum),
        ("range", FocalOp::Range, focal_range),
        ("stddev", FocalOp::StdDev, focal_stddev),
    ];

    for (name, op, scalar_fn) in cases {
        let label = format!("focal_{name} 32×32 3×3");
        let scalar_out = scalar_fn(&src, &window, &boundary).expect(&label);
        let parallel_out = focal_parallel(&src, &window, *op, &boundary).expect(&label);
        assert_rasters_close(&label, &scalar_out, &parallel_out, REL_TOL);
    }
}

#[test]
fn test_focal_parallel_large_window_halo_correctness() {
    // 5×5 window — halo = 2 rows.  Verify no boundary artifacts vs scalar.
    let src = make_gradient_dem(64, 64);
    let window = WindowShape::rectangular(5, 5).expect("valid window");
    let boundary = FocalBoundaryMode::Edge;

    let scalar_out = focal_mean(&src, &window, &boundary).expect("scalar focal_mean 5×5");
    let parallel_out =
        focal_parallel(&src, &window, FocalOp::Mean, &boundary).expect("parallel focal_mean 5×5");

    assert_rasters_close("focal_mean 64×64 5×5", &scalar_out, &parallel_out, REL_TOL);
}

// ---------------------------------------------------------------------------
// Single-thread pool correctness
// ---------------------------------------------------------------------------

#[test]
fn test_parallel_with_single_thread_matches_serial() {
    let dem = make_gradient_dem(64, 64);
    let params = HillshadeParams::standard();
    let scalar_out = hillshade(&dem, params).expect("scalar hillshade");

    // Build a private 1-thread pool and run hillshade_parallel inside it.
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .expect("build pool");

    let parallel_out = pool
        .install(|| hillshade_parallel(&dem, params))
        .expect("single-thread parallel hillshade");

    assert_rasters_close(
        "hillshade single-thread pool",
        &scalar_out,
        &parallel_out,
        REL_TOL,
    );
}

// ---------------------------------------------------------------------------
// Small raster (below one strip height)
// ---------------------------------------------------------------------------

#[test]
fn test_parallel_handles_small_raster_below_chunk_size() {
    // 4×4 is much smaller than DEFAULT_STRIP_HEIGHT (128).
    // hillshade and slope require ≥ 3×3 so this is valid.
    let dem = make_gradient_dem(4, 4);
    let params = HillshadeParams::standard();
    let window = WindowShape::rectangular(3, 3).expect("valid window");
    let boundary = FocalBoundaryMode::Reflect;

    let hs_scalar = hillshade(&dem, params).expect("scalar hillshade 4×4");
    let hs_parallel = hillshade_parallel(&dem, params).expect("parallel hillshade 4×4");
    assert_rasters_close("hillshade 4×4", &hs_scalar, &hs_parallel, REL_TOL);

    let sl_scalar = slope(&dem, 1.0, 1.0).expect("scalar slope 4×4");
    let sl_parallel = slope_parallel(&dem, 1.0, 1.0).expect("parallel slope 4×4");
    assert_rasters_close("slope 4×4", &sl_scalar, &sl_parallel, REL_TOL);

    let fm_scalar = focal_mean(&dem, &window, &boundary).expect("scalar focal_mean 4×4");
    let fm_parallel =
        focal_parallel(&dem, &window, FocalOp::Mean, &boundary).expect("parallel focal_mean 4×4");
    assert_rasters_close("focal_mean 4×4", &fm_scalar, &fm_parallel, REL_TOL);
}
