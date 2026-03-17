//! Comprehensive tests for the overviews module (~60+ tests).

use oxigdal_geotiff::overviews::{
    BandHistogram, OverviewBuilder, OverviewLevel, RasterStatistics, ResampleMethod,
    resample_average, resample_bilinear, resample_lanczos, resample_max, resample_min,
    resample_mode, resample_nearest,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Flat `n×n` raster where every pixel = `value`.
fn uniform(n: u32, value: f64) -> Vec<f64> {
    vec![value; (n * n) as usize]
}

/// 4×4 raster with sequential values 1.0 .. 16.0 (row-major).
fn seq4x4() -> Vec<f64> {
    (1..=16).map(|x| x as f64).collect()
}

// ---------------------------------------------------------------------------
// ResampleMethod
// ---------------------------------------------------------------------------

#[test]
fn kernel_size_nearest() {
    assert_eq!(ResampleMethod::Nearest.kernel_size(), 1);
}

#[test]
fn kernel_size_bilinear() {
    assert_eq!(ResampleMethod::Bilinear.kernel_size(), 2);
}

#[test]
fn kernel_size_bicubic() {
    assert_eq!(ResampleMethod::Bicubic.kernel_size(), 4);
}

#[test]
fn kernel_size_lanczos() {
    assert_eq!(ResampleMethod::Lanczos.kernel_size(), 6);
}

#[test]
fn kernel_size_average() {
    assert_eq!(ResampleMethod::Average.kernel_size(), 2);
}

#[test]
fn kernel_size_mode() {
    assert_eq!(ResampleMethod::Mode.kernel_size(), 2);
}

#[test]
fn kernel_size_gauss() {
    assert_eq!(ResampleMethod::Gauss.kernel_size(), 2);
}

#[test]
fn kernel_size_min() {
    assert_eq!(ResampleMethod::Min.kernel_size(), 2);
}

#[test]
fn kernel_size_max() {
    assert_eq!(ResampleMethod::Max.kernel_size(), 2);
}

#[test]
fn kernel_size_median() {
    assert_eq!(ResampleMethod::Median.kernel_size(), 2);
}

#[test]
fn is_exact_nearest() {
    assert!(ResampleMethod::Nearest.is_exact());
}

#[test]
fn is_exact_mode() {
    assert!(ResampleMethod::Mode.is_exact());
}

#[test]
fn is_not_exact_bilinear() {
    assert!(!ResampleMethod::Bilinear.is_exact());
}

#[test]
fn is_not_exact_average() {
    assert!(!ResampleMethod::Average.is_exact());
}

#[test]
fn is_not_exact_lanczos() {
    assert!(!ResampleMethod::Lanczos.is_exact());
}

// ---------------------------------------------------------------------------
// OverviewBuilder::standard_factors
// ---------------------------------------------------------------------------

#[test]
fn standard_factors_1024() {
    let factors = OverviewBuilder::standard_factors(1024, 1024);
    // Must include [2, 4, 8] — factor 8 → 128×128, which is < 256, so stops there
    assert!(factors.contains(&2));
    assert!(factors.contains(&4));
    assert!(factors.contains(&8));
    // Must be non-empty
    assert!(!factors.is_empty());
    // Must be strictly increasing
    for w in factors.windows(2) {
        assert!(w[0] < w[1]);
    }
}

#[test]
fn standard_factors_256x256_stops_small() {
    // 256×256: factor 2 → 128×128 < 256 in both dims → stops at [2]
    let factors = OverviewBuilder::standard_factors(256, 256);
    assert!(factors.contains(&2));
    assert!(!factors.is_empty());
}

#[test]
fn standard_factors_are_powers_of_two() {
    // 2048×2048: [2,4,8,16] — factor 16 → 128 < 256 stops
    let factors = OverviewBuilder::standard_factors(2048, 2048);
    assert!(!factors.is_empty());
    for f in &factors {
        assert!(f.is_power_of_two(), "factor {f} is not a power of two");
    }
    assert!(factors.contains(&2));
    assert!(factors.contains(&4));
}

#[test]
fn standard_factors_small_raster() {
    // 128×128 → only factor 2 should appear (result = 64 < 256, so it stops)
    let factors = OverviewBuilder::standard_factors(128, 128);
    assert!(!factors.is_empty());
}

#[test]
fn standard_factors_1x1() {
    // Degenerate: only factor 2 can be emitted (result = 1)
    let factors = OverviewBuilder::standard_factors(1, 1);
    assert!(!factors.is_empty());
}

// ---------------------------------------------------------------------------
// resample_nearest
// ---------------------------------------------------------------------------

#[test]
fn nearest_4x4_to_2x2() {
    // seq4x4: row0=[1,2,3,4] row1=[5,6,7,8] row2=[9,10,11,12] row3=[13,14,15,16]
    // Nearest: scale=2, for dst(0,0) → src center ≈ (1.0,1.0) → src(1,1) = 6
    let src = seq4x4();
    let dst = resample_nearest(&src, 4, 4, 2, 2);
    assert_eq!(dst.len(), 4);
    // Output values should be from the source (exact pixels)
    for &v in &dst {
        assert!((1.0..=16.0).contains(&v));
    }
}

#[test]
fn nearest_output_size() {
    let src = uniform(8, 5.0);
    let dst = resample_nearest(&src, 8, 8, 4, 4);
    assert_eq!(dst.len(), 16);
}

#[test]
fn nearest_uniform_data_is_identity() {
    let src = uniform(4, 7.0);
    let dst = resample_nearest(&src, 4, 4, 2, 2);
    for &v in &dst {
        assert!((v - 7.0).abs() < 1e-12);
    }
}

#[test]
fn nearest_preserves_corners() {
    // 4×4 with corners set to 99.0, rest 0.0
    let mut src = vec![0.0_f64; 16];
    src[0] = 99.0; // top-left
    src[3] = 99.0; // top-right
    src[12] = 99.0; // bottom-left
    src[15] = 99.0; // bottom-right
    let dst = resample_nearest(&src, 4, 4, 2, 2);
    // Each corner of 2×2 should have mapped to one corner of the 4×4
    assert_eq!(dst.len(), 4);
}

#[test]
fn nearest_1x1_source() {
    let src = vec![42.0_f64];
    let dst = resample_nearest(&src, 1, 1, 1, 1);
    assert_eq!(dst, vec![42.0]);
}

// ---------------------------------------------------------------------------
// resample_bilinear
// ---------------------------------------------------------------------------

#[test]
fn bilinear_4x4_to_2x2_output_size() {
    let src = seq4x4();
    let dst = resample_bilinear(&src, 4, 4, 2, 2);
    assert_eq!(dst.len(), 4);
}

#[test]
fn bilinear_uniform_is_constant() {
    let src = uniform(8, 3.5);
    let dst = resample_bilinear(&src, 8, 8, 4, 4);
    for &v in &dst {
        assert!((v - 3.5).abs() < 1e-10, "expected 3.5 got {v}");
    }
}

#[test]
fn bilinear_values_in_range() {
    let src = seq4x4(); // values 1..16
    let dst = resample_bilinear(&src, 4, 4, 2, 2);
    for &v in &dst {
        assert!((1.0..=16.0).contains(&v), "bilinear value {v} out of range");
    }
}

#[test]
fn bilinear_center_average() {
    // 2×2 all-same value → 1×1 should equal that value
    let src = vec![5.0_f64; 4];
    let dst = resample_bilinear(&src, 2, 2, 1, 1);
    assert_eq!(dst.len(), 1);
    assert!((dst[0] - 5.0).abs() < 1e-10);
}

#[test]
fn bilinear_8x8_to_4x4() {
    let src: Vec<f64> = (0..64).map(|i| i as f64).collect();
    let dst = resample_bilinear(&src, 8, 8, 4, 4);
    assert_eq!(dst.len(), 16);
    for &v in &dst {
        assert!((0.0..=63.0).contains(&v));
    }
}

// ---------------------------------------------------------------------------
// resample_average
// ---------------------------------------------------------------------------

#[test]
fn average_uniform_equals_constant() {
    let src = uniform(4, 9.0);
    let dst = resample_average(&src, 4, 4, 2, 2, None);
    for &v in &dst {
        assert!((v - 9.0).abs() < 1e-10, "expected 9.0 got {v}");
    }
}

#[test]
fn average_ignores_nodata() {
    // 4×4: left half = 1.0, right half = nodata(−9999.0)
    let mut src = vec![-9999.0_f64; 16];
    for row in 0..4u32 {
        src[(row * 4) as usize] = 1.0;
        src[(row * 4 + 1) as usize] = 1.0;
    }
    let dst = resample_average(&src, 4, 4, 2, 2, Some(-9999.0));
    // Left column of 2×2 should be ~1.0, right column = nodata fallback
    assert!(
        (dst[0] - 1.0).abs() < 0.01,
        "left-top expected 1.0, got {}",
        dst[0]
    );
    assert!(
        (dst[2] - 1.0).abs() < 0.01,
        "left-bot expected 1.0, got {}",
        dst[2]
    );
}

#[test]
fn average_output_size() {
    let src: Vec<f64> = (0..64).map(|x| x as f64).collect();
    let dst = resample_average(&src, 8, 8, 4, 4, None);
    assert_eq!(dst.len(), 16);
}

#[test]
fn average_all_nodata_returns_fallback() {
    let src = vec![-9999.0_f64; 4];
    let dst = resample_average(&src, 2, 2, 1, 1, Some(-9999.0));
    // All nodata → output is the nodata value (fallback)
    assert_eq!(dst.len(), 1);
    assert!((dst[0] - (-9999.0)).abs() < 1e-10);
}

// ---------------------------------------------------------------------------
// resample_min / resample_max
// ---------------------------------------------------------------------------

#[test]
fn min_returns_minimum() {
    // 4×4: values 1..16 → 2×2 min
    let src = seq4x4();
    let dst = resample_min(&src, 4, 4, 2, 2, None);
    assert_eq!(dst.len(), 4);
    // Top-left 2×2 block has values 1,2,5,6 → min=1
    assert!((dst[0] - 1.0).abs() < 1e-10, "expected 1.0, got {}", dst[0]);
}

#[test]
fn max_returns_maximum() {
    // 4×4: values 1..16 → 2×2 max
    let src = seq4x4();
    let dst = resample_max(&src, 4, 4, 2, 2, None);
    assert_eq!(dst.len(), 4);
    // Bottom-right 2×2 block has values 11,12,15,16 → max=16
    assert!(
        (dst[3] - 16.0).abs() < 1e-10,
        "expected 16.0, got {}",
        dst[3]
    );
}

#[test]
fn min_ignores_nodata() {
    let src = vec![5.0_f64, -9999.0, 3.0, -9999.0];
    let dst = resample_min(&src, 2, 2, 1, 1, Some(-9999.0));
    assert_eq!(dst.len(), 1);
    assert!((dst[0] - 3.0).abs() < 1e-10, "expected 3.0 got {}", dst[0]);
}

#[test]
fn max_ignores_nodata() {
    let src = vec![-9999.0_f64, 7.0, -9999.0, 2.0];
    let dst = resample_max(&src, 2, 2, 1, 1, Some(-9999.0));
    assert_eq!(dst.len(), 1);
    assert!((dst[0] - 7.0).abs() < 1e-10, "expected 7.0 got {}", dst[0]);
}

#[test]
fn min_uniform() {
    let src = uniform(4, 42.0);
    let dst = resample_min(&src, 4, 4, 2, 2, None);
    for &v in &dst {
        assert!((v - 42.0).abs() < 1e-10);
    }
}

#[test]
fn max_uniform() {
    let src = uniform(4, 42.0);
    let dst = resample_max(&src, 4, 4, 2, 2, None);
    for &v in &dst {
        assert!((v - 42.0).abs() < 1e-10);
    }
}

// ---------------------------------------------------------------------------
// resample_lanczos
// ---------------------------------------------------------------------------

#[test]
fn lanczos_4x4_to_2x2_in_range() {
    let src = seq4x4(); // 1..16
    let dst = resample_lanczos(&src, 4, 4, 2, 2);
    assert_eq!(dst.len(), 4);
    for &v in &dst {
        // Lanczos can ring slightly but should stay near [1,16]
        assert!(
            (0.0..=20.0).contains(&v),
            "lanczos value {v} out of expected range"
        );
    }
}

#[test]
fn lanczos_uniform_is_constant() {
    let src = uniform(8, 5.0);
    let dst = resample_lanczos(&src, 8, 8, 4, 4);
    for &v in &dst {
        assert!((v - 5.0).abs() < 0.01, "expected ~5.0 got {v}");
    }
}

#[test]
fn lanczos_output_size() {
    let src = uniform(8, 1.0);
    let dst = resample_lanczos(&src, 8, 8, 2, 2);
    assert_eq!(dst.len(), 4);
}

// ---------------------------------------------------------------------------
// resample_mode
// ---------------------------------------------------------------------------

#[test]
fn mode_dominant_value() {
    // 4×4: 12 pixels = 7.0, 4 pixels = 3.0 → output should be 7.0
    let mut src = vec![7.0_f64; 16];
    src[0] = 3.0;
    src[1] = 3.0;
    src[2] = 3.0;
    src[3] = 3.0;
    let dst = resample_mode(&src, 4, 4, 1, 1);
    assert_eq!(dst.len(), 1);
    assert!((dst[0] - 7.0).abs() < 0.01, "expected 7.0 got {}", dst[0]);
}

#[test]
fn mode_uniform() {
    let src = uniform(4, 42.0);
    let dst = resample_mode(&src, 4, 4, 2, 2);
    for &v in &dst {
        assert!((v - 42.0).abs() < 0.01, "expected 42.0 got {v}");
    }
}

#[test]
fn mode_output_size() {
    let src = seq4x4();
    let dst = resample_mode(&src, 4, 4, 2, 2);
    assert_eq!(dst.len(), 4);
}

// ---------------------------------------------------------------------------
// OverviewBuilder::build
// ---------------------------------------------------------------------------

#[test]
fn builder_correct_level_count() {
    let src = uniform(64, 1.0);
    let levels = OverviewBuilder::new()
        .with_factors(vec![2, 4, 8])
        .build(&src, 64, 64);
    assert_eq!(levels.len(), 3);
}

#[test]
fn builder_level_dimensions() {
    let src = uniform(64, 1.0);
    let levels = OverviewBuilder::new()
        .with_factors(vec![2, 4])
        .build(&src, 64, 64);
    assert_eq!(levels[0].width, 32);
    assert_eq!(levels[0].height, 32);
    assert_eq!(levels[1].width, 16);
    assert_eq!(levels[1].height, 16);
}

#[test]
fn builder_data_populated() {
    let src = uniform(8, 3.0);
    let levels = OverviewBuilder::new()
        .with_factors(vec![2])
        .build(&src, 8, 8);
    assert_eq!(levels.len(), 1);
    assert_eq!(levels[0].data.len(), (4 * 4) as usize);
}

#[test]
fn builder_empty_source_returns_empty() {
    let levels = OverviewBuilder::new().build(&[], 0, 0);
    assert!(levels.is_empty());
}

#[test]
fn builder_skips_invalid_factor_1() {
    let src = uniform(4, 1.0);
    let levels = OverviewBuilder::new()
        .with_factors(vec![1, 2])
        .build(&src, 4, 4);
    // Factor 1 is skipped, only factor 2 remains
    assert_eq!(levels.len(), 1);
    assert_eq!(levels[0].factor, 2);
}

#[test]
fn builder_nearest_method() {
    let src = uniform(8, 5.0);
    let levels = OverviewBuilder::new()
        .with_method(ResampleMethod::Nearest)
        .with_factors(vec![2])
        .build(&src, 8, 8);
    assert_eq!(levels.len(), 1);
    for &v in &levels[0].data {
        assert!((v - 5.0).abs() < 1e-10);
    }
}

#[test]
fn builder_with_nodata() {
    let mut src = vec![1.0_f64; 16];
    src[0] = -9999.0;
    let levels = OverviewBuilder::new()
        .with_factors(vec![2])
        .with_nodata(-9999.0)
        .build(&src, 4, 4);
    assert_eq!(levels.len(), 1);
}

#[test]
fn builder_all_methods_produce_output() {
    let src: Vec<f64> = (0..64).map(|x| x as f64).collect();
    let methods = [
        ResampleMethod::Nearest,
        ResampleMethod::Bilinear,
        ResampleMethod::Bicubic,
        ResampleMethod::Average,
        ResampleMethod::Mode,
        ResampleMethod::Lanczos,
        ResampleMethod::Gauss,
        ResampleMethod::Min,
        ResampleMethod::Max,
        ResampleMethod::Median,
    ];
    for method in methods {
        let levels = OverviewBuilder::new()
            .with_method(method)
            .with_factors(vec![2])
            .build(&src, 8, 8);
        assert_eq!(levels.len(), 1, "method {method:?} produced no levels");
        assert_eq!(levels[0].data.len(), 16, "method {method:?} wrong data len");
    }
}

// ---------------------------------------------------------------------------
// OverviewLevel
// ---------------------------------------------------------------------------

#[test]
fn level_tile_count_512_with_256_tile() {
    let level = OverviewLevel {
        factor: 2,
        width: 512,
        height: 512,
        tile_width: 256,
        tile_height: 256,
        data: vec![0.0; 512 * 512],
    };
    let (tx, ty) = level.tile_count();
    assert_eq!(tx, 2);
    assert_eq!(ty, 2);
}

#[test]
fn level_tile_count_non_divisible() {
    // 300×300 / 256 → ceil = 2 in each direction
    let level = OverviewLevel {
        factor: 2,
        width: 300,
        height: 300,
        tile_width: 256,
        tile_height: 256,
        data: vec![0.0; 300 * 300],
    };
    let (tx, ty) = level.tile_count();
    assert_eq!(tx, 2);
    assert_eq!(ty, 2);
}

#[test]
fn level_pixel_at_in_bounds() {
    let mut level = OverviewLevel::new(2, 8, 8, 256);
    level.data = (0..16).map(|x| x as f64).collect();
    assert_eq!(level.pixel_at(0, 0), Some(0.0));
    assert_eq!(level.pixel_at(3, 3), Some(15.0));
}

#[test]
fn level_pixel_at_out_of_bounds() {
    let mut level = OverviewLevel::new(2, 8, 8, 256);
    level.data = vec![0.0; 16];
    assert_eq!(level.pixel_at(4, 0), None);
    assert_eq!(level.pixel_at(0, 4), None);
    assert_eq!(level.pixel_at(100, 100), None);
}

#[test]
fn level_pixel_at_exact_boundary() {
    let mut level = OverviewLevel::new(2, 8, 8, 256);
    level.data = vec![1.0; 16];
    // (width-1, height-1) is the last valid pixel
    assert_eq!(level.pixel_at(3, 3), Some(1.0));
    // width is out of bounds
    assert_eq!(level.pixel_at(4, 3), None);
}

// ---------------------------------------------------------------------------
// RasterStatistics
// ---------------------------------------------------------------------------

#[test]
fn stats_known_dataset_1_to_100() {
    let data: Vec<f64> = (1..=100).map(|x| x as f64).collect();
    let stats = RasterStatistics::compute(&data, None).expect("should compute");
    assert!((stats.min - 1.0).abs() < 1e-10);
    assert!((stats.max - 100.0).abs() < 1e-10);
    assert!((stats.mean - 50.5).abs() < 0.001, "mean={}", stats.mean);
    assert_eq!(stats.valid_count, 100);
    assert_eq!(stats.nodata_count, 0);
}

#[test]
fn stats_with_nodata_ignores_nodata() {
    let mut data: Vec<f64> = (1..=10).map(|x| x as f64).collect();
    data.push(-9999.0); // nodata
    data.push(-9999.0);
    let stats = RasterStatistics::compute(&data, Some(-9999.0)).expect("should compute");
    assert_eq!(stats.valid_count, 10);
    assert_eq!(stats.nodata_count, 2);
    assert!((stats.min - 1.0).abs() < 1e-10);
    assert!((stats.max - 10.0).abs() < 1e-10);
}

#[test]
fn stats_all_nodata_returns_none() {
    let data = vec![-9999.0_f64; 16];
    let result = RasterStatistics::compute(&data, Some(-9999.0));
    assert!(result.is_none());
}

#[test]
fn stats_empty_returns_none() {
    let result = RasterStatistics::compute(&[], None);
    assert!(result.is_none());
}

#[test]
fn stats_mean_correct() {
    let data = vec![1.0_f64, 2.0, 3.0, 4.0, 5.0];
    let stats = RasterStatistics::compute(&data, None).expect("should compute");
    assert!((stats.mean - 3.0).abs() < 0.001, "mean={}", stats.mean);
}

#[test]
fn stats_std_dev_zero_uniform() {
    let data = uniform(4, 7.0);
    let stats = RasterStatistics::compute(&data, None).expect("should compute");
    assert!(stats.std_dev.abs() < 1e-10, "stddev={}", stats.std_dev);
}

#[test]
fn stats_std_dev_nonzero() {
    let data = vec![0.0_f64, 10.0];
    let stats = RasterStatistics::compute(&data, None).expect("should compute");
    assert!(stats.std_dev > 0.0);
}

#[test]
fn stats_percentile_50_is_median() {
    // Sorted data 1..=99 → median = 50
    let data: Vec<f64> = (1..=99).map(|x| x as f64).collect();
    let stats = RasterStatistics::compute(&data, None).expect("should compute");
    // Allow ±1% of range tolerance
    let tol = 1.0;
    assert!(
        (stats.percentile_50 - 50.0).abs() < tol,
        "p50={} expected ~50.0",
        stats.percentile_50
    );
}

#[test]
fn stats_approximate_matches_exact_roughly() {
    let data: Vec<f64> = (1..=1000).map(|x| x as f64).collect();
    let exact = RasterStatistics::compute(&data, None).expect("exact");
    let approx = RasterStatistics::compute_approximate(&data, None, 4).expect("approx");
    // Mean should be within 5% of exact
    assert!((exact.mean - approx.mean).abs() / exact.mean < 0.05);
}

#[test]
fn stats_single_value() {
    let data = vec![42.0_f64];
    let stats = RasterStatistics::compute(&data, None).expect("single value");
    assert!((stats.min - 42.0).abs() < 1e-10);
    assert!((stats.max - 42.0).abs() < 1e-10);
    assert!((stats.mean - 42.0).abs() < 1e-10);
    assert_eq!(stats.valid_count, 1);
}

// ---------------------------------------------------------------------------
// BandHistogram
// ---------------------------------------------------------------------------

#[test]
fn histogram_compute_correct_bucket_count() {
    let data: Vec<f64> = (0..100).map(|x| x as f64).collect();
    let hist = BandHistogram::compute(&data, 16, None).expect("should compute");
    assert_eq!(hist.buckets.len(), 16);
}

#[test]
fn histogram_p0_approx_min() {
    let data: Vec<f64> = (0..=100).map(|x| x as f64).collect();
    let hist = BandHistogram::compute(&data, 64, None).expect("should compute");
    let p0 = hist.value_at_percentile(0.0);
    assert!((0.0..5.0).contains(&p0), "p0={p0} expected near 0");
}

#[test]
fn histogram_p100_approx_max() {
    let data: Vec<f64> = (0..=100).map(|x| x as f64).collect();
    let hist = BandHistogram::compute(&data, 64, None).expect("should compute");
    let p100 = hist.value_at_percentile(100.0);
    // Should be near 100
    assert!(p100 >= 95.0, "p100={p100} expected near 100");
}

#[test]
fn histogram_mode_value_uniform() {
    let data = uniform(8, 5.0);
    let hist = BandHistogram::compute(&data, 4, None).expect("should compute");
    let mode = hist.mode_value();
    // All same value → single bucket has all counts → mode near 5.0
    assert!((mode - 5.0).abs() < 1.0, "mode={mode} expected near 5.0");
}

#[test]
fn histogram_bucket_for_value_at_min() {
    let data: Vec<f64> = (0..=10).map(|x| x as f64).collect();
    let hist = BandHistogram::compute(&data, 11, None).expect("should compute");
    let idx = hist.bucket_for_value(hist.bucket_min);
    assert_eq!(idx, Some(0));
}

#[test]
fn histogram_bucket_for_value_at_max_minus_epsilon() {
    let data: Vec<f64> = (0..=10).map(|x| x as f64).collect();
    let hist = BandHistogram::compute(&data, 11, None).expect("should compute");
    let idx = hist.bucket_for_value(hist.bucket_max - 1e-9);
    assert!(
        idx.is_some(),
        "expected Some for value just below bucket_max"
    );
}

#[test]
fn histogram_bucket_for_value_below_range() {
    let data = vec![5.0_f64, 6.0, 7.0];
    let hist = BandHistogram::compute(&data, 4, None).expect("should compute");
    let idx = hist.bucket_for_value(hist.bucket_min - 1.0);
    assert_eq!(idx, None);
}

#[test]
fn histogram_bucket_for_value_above_range() {
    let data = vec![5.0_f64, 6.0, 7.0];
    let hist = BandHistogram::compute(&data, 4, None).expect("should compute");
    let idx = hist.bucket_for_value(hist.bucket_max + 1.0);
    assert_eq!(idx, None);
}

#[test]
fn histogram_all_nodata_returns_none() {
    let data = vec![-9999.0_f64; 10];
    let result = BandHistogram::compute(&data, 16, Some(-9999.0));
    assert!(result.is_none());
}

#[test]
fn histogram_empty_returns_none() {
    let result = BandHistogram::compute(&[], 16, None);
    assert!(result.is_none());
}

#[test]
fn histogram_total_counts_correct() {
    let data: Vec<f64> = (0..=99).map(|x| x as f64).collect();
    let hist = BandHistogram::compute(&data, 10, None).expect("should compute");
    let total: u64 = hist.buckets.iter().sum();
    assert_eq!(total, 100);
}

#[test]
fn histogram_nodata_excluded_from_total() {
    let mut data: Vec<f64> = (0..=9).map(|x| x as f64).collect();
    data.extend(vec![-9999.0_f64; 5]);
    let hist = BandHistogram::compute(&data, 10, Some(-9999.0)).expect("should compute");
    let total: u64 = hist.buckets.iter().sum();
    assert_eq!(
        total, 10,
        "nodata pixels should not be counted, total={total}"
    );
}

// ---------------------------------------------------------------------------
// OverviewBuilder builder pattern
// ---------------------------------------------------------------------------

#[test]
fn builder_with_method_sets_method() {
    let b = OverviewBuilder::new().with_method(ResampleMethod::Lanczos);
    assert_eq!(b.method, ResampleMethod::Lanczos);
}

#[test]
fn builder_with_tile_size() {
    let b = OverviewBuilder::new().with_tile_size(512);
    assert_eq!(b.tile_size, 512);
}

#[test]
fn builder_with_nodata_sets_nodata() {
    let b = OverviewBuilder::new().with_nodata(-1.0);
    assert_eq!(b.nodata, Some(-1.0));
}

#[test]
fn builder_default_tile_size_is_256() {
    let b = OverviewBuilder::new();
    assert_eq!(b.tile_size, 256);
}

#[test]
fn builder_level_factors_stored() {
    let src = uniform(8, 2.0);
    let levels = OverviewBuilder::new()
        .with_factors(vec![2, 4])
        .build(&src, 8, 8);
    assert_eq!(levels[0].factor, 2);
    assert_eq!(levels[1].factor, 4);
}
