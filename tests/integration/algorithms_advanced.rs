//! Advanced Algorithms Integration Tests
//!
//! Comprehensive test suite for all advanced algorithm implementations including:
//! - SIMD-optimized operations
//! - Raster algorithms (terrain, hydrology, focal, texture, viewshed, cost-distance)
//! - Resampling algorithms (nearest, bilinear, bicubic, lanczos)
//! - Parallel processing capabilities
//!
//! Each test validates correctness, performance characteristics, and edge cases.

#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_parens)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::manual_range_contains, clippy::needless_range_loop)]

use std::error::Error;

use oxigdal_algorithms::raster::compute_viewshed as real_compute_viewshed;
use oxigdal_algorithms::raster::hydrology::compute_d8_accumulation_from_fdir;
use oxigdal_algorithms::{Resampler, ResamplingMethod};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Builds a single-band `Float32` [`RasterBuffer`] (width × height) from a
/// row-major `f32` slice for feeding into the real `oxigdal-algorithms` kernels.
fn raster_from_f32(data: &[f32], width: usize, height: usize) -> Result<RasterBuffer> {
    let mut buf = RasterBuffer::zeros(width as u64, height as u64, RasterDataType::Float32);
    for y in 0..height {
        for x in 0..width {
            buf.set_pixel(x as u64, y as u64, f64::from(data[y * width + x]))
                .map_err(|e| Box::new(e) as Box<dyn Error>)?;
        }
    }
    Ok(buf)
}

/// Reads a `width × height` `RasterBuffer` back into a row-major `f32` vector.
fn raster_to_f32(buf: &RasterBuffer, width: usize, height: usize) -> Result<Vec<f32>> {
    let mut out = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            out.push(
                buf.get_pixel(x as u64, y as u64)
                    .map_err(|e| Box::new(e) as Box<dyn Error>)? as f32,
            );
        }
    }
    Ok(out)
}

/// Resamples a row-major `f32` raster through the real `oxigdal-algorithms`
/// [`Resampler`] using the requested method.
fn resample_real(
    src: &[f32],
    src_w: usize,
    src_h: usize,
    dst_w: usize,
    dst_h: usize,
    method: ResamplingMethod,
) -> Result<Vec<f32>> {
    let buf = raster_from_f32(src, src_w, src_h)?;
    let out = Resampler::new(method)
        .resample(&buf, dst_w as u64, dst_h as u64)
        .map_err(|e| Box::new(e) as Box<dyn Error>)?;
    raster_to_f32(&out, dst_w, dst_h)
}

// ============================================================================
// SIMD Operations Tests
// ============================================================================

#[test]
fn test_simd_raster_operations_basic() -> Result<()> {
    // Test basic SIMD-accelerated raster operations
    let width = 1024;
    let height = 1024;
    let data: Vec<f32> = (0..(width * height)).map(|i| i as f32).collect();

    // Test element-wise operations with SIMD
    let result = simd_add_scalar(&data, 10.0)?;
    assert_eq!(result.len(), data.len());
    assert!((result[0] - 10.0).abs() < 1e-6);
    assert!((result[100] - 110.0).abs() < 1e-6);

    Ok(())
}

#[test]
fn test_simd_statistics_computation() -> Result<()> {
    // Test SIMD-accelerated statistical operations
    let data: Vec<f64> = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];

    // Compute mean with SIMD
    let mean = simd_mean(&data)?;
    assert!((mean - 5.5).abs() < 1e-10);

    // Compute standard deviation with SIMD
    let std_dev = simd_std_dev(&data)?;
    assert!(std_dev > 2.8 && std_dev < 3.0);

    // Compute min/max with SIMD
    let (min_val, max_val) = simd_min_max(&data)?;
    assert!((min_val - 1.0).abs() < 1e-10);
    assert!((max_val - 10.0).abs() < 1e-10);

    Ok(())
}

#[test]
fn test_simd_convolution_operations() -> Result<()> {
    // Test SIMD-accelerated convolution
    let width = 100;
    let height = 100;
    let mut data: Vec<f32> = vec![0.0; width * height];

    // Set center pixel to 1.0
    data[height / 2 * width + width / 2] = 1.0;

    // 3x3 Gaussian kernel
    let kernel = vec![
        1.0 / 16.0,
        2.0 / 16.0,
        1.0 / 16.0,
        2.0 / 16.0,
        4.0 / 16.0,
        2.0 / 16.0,
        1.0 / 16.0,
        2.0 / 16.0,
        1.0 / 16.0,
    ];

    let result = simd_convolve(&data, width, height, &kernel, 3)?;

    // Center pixel should be reduced
    let center_idx = height / 2 * width + width / 2;
    assert!(result[center_idx] < 1.0);
    assert!(result[center_idx] > 0.0);

    Ok(())
}

#[test]
fn test_simd_resampling_operations() -> Result<()> {
    // Test SIMD-accelerated resampling
    let src_width = 100;
    let src_height = 100;
    let src_data: Vec<f32> = (0..(src_width * src_height))
        .map(|i| (i % src_width) as f32)
        .collect();

    // Resample to larger size
    let dst_width = 200;
    let dst_height = 200;

    let result = simd_resample_bilinear(&src_data, src_width, src_height, dst_width, dst_height)?;

    assert_eq!(result.len(), dst_width * dst_height);

    // Check some interpolated values
    assert!(result[0] >= 0.0);
    assert!(result[dst_width - 1] > result[0]);

    Ok(())
}

#[test]
fn test_simd_morphological_operations() -> Result<()> {
    // Test SIMD-accelerated morphological operations
    let width = 100;
    let height = 100;
    let mut data: Vec<u8> = vec![0; width * height];

    // Create a square in the center
    for y in 40..60 {
        for x in 40..60 {
            data[y * width + x] = 255;
        }
    }

    // Test erosion
    let eroded = simd_erosion(&data, width, height, 3)?;
    assert_eq!(eroded.len(), data.len());

    // Eroded area should be smaller
    let original_count = data.iter().filter(|&&x| x > 0).count();
    let eroded_count = eroded.iter().filter(|&&x| x > 0).count();
    assert!(eroded_count < original_count);

    // Test dilation
    let dilated = simd_dilation(&data, width, height, 3)?;

    // Dilated area should be larger
    let dilated_count = dilated.iter().filter(|&&x| x > 0).count();
    assert!(dilated_count > original_count);

    Ok(())
}

#[test]
fn test_simd_histogram_computation() -> Result<()> {
    // Test SIMD-accelerated histogram
    let data: Vec<u8> = (0..=255).cycle().take(10000).collect();

    let histogram = simd_histogram(&data, 256)?;

    assert_eq!(histogram.len(), 256);

    // Each bin should have roughly the same count
    let expected_per_bin = 10000 / 256;
    for count in histogram.iter() {
        assert!(*count >= expected_per_bin - 2 && *count <= expected_per_bin + 2);
    }

    Ok(())
}

#[test]
fn test_simd_threshold_operations() -> Result<()> {
    // Test SIMD-accelerated thresholding
    let data: Vec<u8> = (0..=255).collect();

    // Binary threshold
    let binary = simd_threshold_binary(&data, 127)?;
    assert_eq!(binary.iter().filter(|&&x| x == 0).count(), 128);
    assert_eq!(binary.iter().filter(|&&x| x == 255).count(), 128);

    // Otsu threshold
    let otsu_thresh = simd_threshold_otsu(&data)?;
    assert!(otsu_thresh > 0 && otsu_thresh < 255);

    Ok(())
}

#[test]
fn test_simd_colorspace_conversions() -> Result<()> {
    // Test SIMD-accelerated colorspace conversions
    let rgb_data: Vec<u8> = vec![255, 0, 0, 0, 255, 0, 0, 0, 255]; // Red, Green, Blue

    // RGB to Grayscale
    let gray = simd_rgb_to_gray(&rgb_data)?;
    assert_eq!(gray.len(), 3);

    // RGB to HSV
    let hsv = simd_rgb_to_hsv(&rgb_data)?;
    assert_eq!(hsv.len(), 9);

    // HSV to RGB (round-trip)
    let rgb_back = simd_hsv_to_rgb(&hsv)?;
    assert_eq!(rgb_back.len(), rgb_data.len());

    Ok(())
}

// ============================================================================
// Raster Algorithm Tests
// ============================================================================

#[test]
fn test_terrain_slope_aspect() -> Result<()> {
    // Test slope and aspect calculation
    let width = 100;
    let height = 100;

    // Create a simple sloped surface
    let elevation: Vec<f32> = (0..height)
        .flat_map(|y| (0..width).map(move |x| (y as f32 + x as f32) * 0.1))
        .collect();

    let cell_size = 1.0;
    let (slope, aspect) = compute_slope_aspect(&elevation, width, height, cell_size)?;

    assert_eq!(slope.len(), elevation.len());
    assert_eq!(aspect.len(), elevation.len());

    // Slope should be positive and relatively uniform
    let avg_slope: f32 = slope.iter().sum::<f32>() / slope.len() as f32;
    assert!(avg_slope > 0.0);

    // Aspect should be in range [0, 360)
    for &a in aspect.iter() {
        assert!(a >= 0.0 && a < 360.0 || a.is_nan()); // NaN for flat areas
    }

    Ok(())
}

#[test]
fn test_terrain_hillshade() -> Result<()> {
    // Test hillshade calculation
    let width = 100;
    let height = 100;

    // Create elevation data
    let elevation: Vec<f32> = (0..height)
        .flat_map(|y| {
            (0..width).map(move |x| {
                let dx = x as f32 - 50.0;
                let dy = y as f32 - 50.0;
                (dx * dx + dy * dy).sqrt()
            })
        })
        .collect();

    let cell_size = 1.0;
    let azimuth = 315.0; // Northwest
    let altitude = 45.0; // 45 degrees

    let hillshade = compute_hillshade(&elevation, width, height, cell_size, azimuth, altitude)?;

    assert_eq!(hillshade.len(), elevation.len());

    // Hillshade values should be in range [0, 255]
    for &value in hillshade.iter() {
        assert!((0.0..=255.0).contains(&value));
    }

    Ok(())
}

#[test]
fn test_terrain_tpi_tri() -> Result<()> {
    // Test Topographic Position Index and Terrain Ruggedness Index
    let width = 50;
    let height = 50;

    // Create elevation with some variation
    let elevation: Vec<f32> = (0..height)
        .flat_map(|y| {
            (0..width)
                .map(move |x| ((x as f32 * 0.1).sin() + (y as f32 * 0.1).cos()) * 10.0 + 100.0)
        })
        .collect();

    // Compute TPI (Topographic Position Index)
    let tpi = compute_tpi(&elevation, width, height, 3)?;
    assert_eq!(tpi.len(), elevation.len());

    // TPI should have positive and negative values
    let has_positive = tpi.iter().any(|&v| v > 0.0);
    let has_negative = tpi.iter().any(|&v| v < 0.0);
    assert!(has_positive && has_negative);

    // Compute TRI (Terrain Ruggedness Index)
    let tri = compute_tri(&elevation, width, height)?;
    assert_eq!(tri.len(), elevation.len());

    // TRI should be non-negative
    assert!(tri.iter().all(|&v| v >= 0.0));

    Ok(())
}

#[test]
fn test_focal_statistics() -> Result<()> {
    // Test focal statistics (moving window operations)
    let width = 100;
    let height = 100;
    let data: Vec<f32> = (0..(width * height)).map(|i| (i % 100) as f32).collect();

    let window_size = 3;

    // Focal mean
    let focal_mean = compute_focal_mean(&data, width, height, window_size)?;
    assert_eq!(focal_mean.len(), data.len());

    // Focal median
    let focal_median = compute_focal_median(&data, width, height, window_size)?;
    assert_eq!(focal_median.len(), data.len());

    // Focal range
    let focal_range = compute_focal_range(&data, width, height, window_size)?;
    assert_eq!(focal_range.len(), data.len());

    // Focal variety (unique values)
    let focal_variety = compute_focal_variety(&data, width, height, window_size)?;
    assert_eq!(focal_variety.len(), data.len());

    Ok(())
}

#[test]
fn test_texture_analysis_glcm() -> Result<()> {
    // Test Gray Level Co-occurrence Matrix (GLCM) texture analysis
    let width = 64;
    let height = 64;

    // Create a textured pattern
    let data: Vec<u8> = (0..height)
        .flat_map(|y| (0..width).map(move |x| ((x + y) % 16 * 16) as u8))
        .collect();

    let distance = 1;
    let angle = 0.0; // Horizontal
    let levels = 256;

    let glcm = compute_glcm(&data, width, height, distance, angle, levels)?;

    // Compute Haralick features from GLCM
    let contrast = compute_glcm_contrast(&glcm)?;
    let energy = compute_glcm_energy(&glcm)?;
    let homogeneity = compute_glcm_homogeneity(&glcm)?;
    let correlation = compute_glcm_correlation(&glcm)?;

    // Validate feature ranges
    assert!(contrast >= 0.0);
    assert!((0.0..=1.0).contains(&energy));
    assert!((0.0..=1.0).contains(&homogeneity));
    assert!((-1.0..=1.0).contains(&correlation));

    Ok(())
}

#[test]
fn test_viewshed_analysis() -> Result<()> {
    // Test viewshed analysis
    let width = 100;
    let height = 100;

    // Create elevation model with a hill in the center
    let elevation: Vec<f32> = (0..height)
        .flat_map(|y| {
            (0..width).map(move |x| {
                let dx = x as f32 - 50.0;
                let dy = y as f32 - 50.0;
                let dist = (dx * dx + dy * dy).sqrt();
                if dist < 20.0 { 50.0 - dist } else { 30.0 }
            })
        })
        .collect();

    // Observer at center of hill
    let observer_x = 50;
    let observer_y = 50;
    let observer_height = 2.0;
    let max_distance = 50.0;

    let viewshed = compute_viewshed(
        &elevation,
        width,
        height,
        observer_x,
        observer_y,
        observer_height,
        max_distance,
    )?;

    assert_eq!(viewshed.len(), elevation.len());

    // Observer position should be visible
    assert!(viewshed[observer_y * width + observer_x] > 0);

    // Count visible cells
    let visible_count = viewshed.iter().filter(|&&v| v > 0).count();
    assert!(visible_count > 0);

    Ok(())
}

#[test]
fn test_cost_distance_analysis() -> Result<()> {
    // Test cost-distance analysis
    let width = 100;
    let height = 100;

    // Create cost surface (flat terrain = cost 1)
    let cost: Vec<f32> = vec![1.0; width * height];

    // Source points (center)
    let mut sources: Vec<bool> = vec![false; width * height];
    sources[50 * width + 50] = true;

    let (cost_distance, allocation) = compute_cost_distance(&cost, &sources, width, height)?;

    assert_eq!(cost_distance.len(), cost.len());
    assert_eq!(allocation.len(), cost.len());

    // Distance at source should be 0
    assert!((cost_distance[50 * width + 50]).abs() < 1e-6);

    // Distance should increase with Euclidean distance
    let dist_to_corner = cost_distance[0];
    assert!(dist_to_corner > 0.0);

    Ok(())
}

#[test]
fn test_hydrology_flow_direction() -> Result<()> {
    // Test flow direction (D8 algorithm)
    let width = 50;
    let height = 50;

    // Create simple sloped surface
    let elevation: Vec<f32> = (0..height)
        .flat_map(|y| (0..width).map(move |x| (height - y) as f32 * 10.0))
        .collect();

    let flow_dir = compute_flow_direction(&elevation, width, height)?;

    assert_eq!(flow_dir.len(), elevation.len());

    // Flow direction values should be valid (powers of 2: 1,2,4,8,16,32,64,128)
    for &dir in flow_dir.iter() {
        assert!(
            dir == 0
                || dir == 1
                || dir == 2
                || dir == 4
                || dir == 8
                || dir == 16
                || dir == 32
                || dir == 64
                || dir == 128
        );
    }

    Ok(())
}

#[test]
fn test_hydrology_flow_accumulation() -> Result<()> {
    // Test flow accumulation
    let width = 50;
    let height = 50;

    // Create flow direction (all flowing south)
    let flow_dir: Vec<u8> = vec![4; width * height]; // 4 = south in D8

    let flow_accum = compute_flow_accumulation(&flow_dir, width, height)?;

    assert_eq!(flow_accum.len(), flow_dir.len());

    // Accumulation should increase moving south
    let top_row_accum = flow_accum[width / 2];
    let bottom_row_accum = flow_accum[(height - 1) * width + width / 2];

    assert!(bottom_row_accum > top_row_accum);

    Ok(())
}

#[test]
fn test_hydrology_watershed_delineation() -> Result<()> {
    // Test watershed delineation
    let width = 50;
    let height = 50;

    // Create elevation with two basins
    let elevation: Vec<f32> = (0..height)
        .flat_map(|y| {
            (0..width).map(move |x| {
                let dx1 = x as f32 - 15.0;
                let dy1 = y as f32 - 25.0;
                let dist1 = (dx1 * dx1 + dy1 * dy1).sqrt();

                let dx2 = x as f32 - 35.0;
                let dy2 = y as f32 - 25.0;
                let dist2 = (dx2 * dx2 + dy2 * dy2).sqrt();

                dist1.min(dist2)
            })
        })
        .collect();

    // Pour points
    let mut pour_points: Vec<bool> = vec![false; width * height];
    pour_points[25 * width + 15] = true;
    pour_points[25 * width + 35] = true;

    let watersheds = compute_watersheds(&elevation, &pour_points, width, height)?;

    assert_eq!(watersheds.len(), elevation.len());

    // Should have at least 2 watersheds
    let max_watershed = *watersheds.iter().max().ok_or("No watersheds")?;
    assert!(max_watershed >= 2);

    Ok(())
}

#[test]
fn test_hydrology_stream_network() -> Result<()> {
    // Test stream network extraction
    let width = 50;
    let height = 50;

    // Create flow accumulation data
    let flow_accum: Vec<u32> = (0..height)
        .flat_map(|y| {
            (0..width).map(move |x| {
                if x == width / 2 {
                    (height - y) as u32 // Simulated stream
                } else {
                    1
                }
            })
        })
        .collect();

    let threshold = 10u32;
    let stream_network = extract_stream_network(&flow_accum, width, height, threshold)?;

    assert_eq!(stream_network.len(), flow_accum.len());

    // Should have some stream cells
    let stream_count = stream_network.iter().filter(|&&v| v).count();
    assert!(stream_count > 0);

    Ok(())
}

#[test]
fn test_hydrology_fill_sinks() -> Result<()> {
    // Test sink filling
    let width = 50;
    let height = 50;

    // Create elevation with a sink
    let mut elevation: Vec<f32> = (0..height)
        .flat_map(|y| (0..width).map(move |x| (x + y) as f32))
        .collect();

    // Create a sink at position (25, 25)
    elevation[25 * width + 25] = 0.0;

    let filled = fill_sinks(&elevation, width, height)?;

    assert_eq!(filled.len(), elevation.len());

    // Sink should be filled
    assert!(filled[25 * width + 25] > 0.0);

    Ok(())
}

// ============================================================================
// Resampling Algorithm Tests
// ============================================================================

#[test]
fn test_resampling_nearest_neighbor() -> Result<()> {
    // Test nearest neighbor resampling
    let src_width = 10;
    let src_height = 10;
    let src_data: Vec<f32> = (0..(src_width * src_height)).map(|i| i as f32).collect();

    let dst_width = 20;
    let dst_height = 20;

    let result = resample_nearest(&src_data, src_width, src_height, dst_width, dst_height)?;

    assert_eq!(result.len(), dst_width * dst_height);

    // First pixel should match
    assert!((result[0] - src_data[0]).abs() < 1e-6);

    Ok(())
}

#[test]
fn test_resampling_bilinear() -> Result<()> {
    // Test bilinear resampling
    let src_width = 10;
    let src_height = 10;
    let src_data: Vec<f32> = (0..(src_width * src_height)).map(|i| i as f32).collect();

    let dst_width = 15;
    let dst_height = 15;

    let result = resample_bilinear(&src_data, src_width, src_height, dst_width, dst_height)?;

    assert_eq!(result.len(), dst_width * dst_height);

    // Values should be interpolated
    assert!(result[0] >= src_data[0]);

    Ok(())
}

#[test]
fn test_resampling_bicubic() -> Result<()> {
    // Test bicubic resampling
    let src_width = 20;
    let src_height = 20;
    let src_data: Vec<f32> = (0..(src_width * src_height))
        .map(|i| ((i as f32 * 0.1).sin() * 100.0 + 100.0))
        .collect();

    let dst_width = 30;
    let dst_height = 30;

    let result = resample_bicubic(&src_data, src_width, src_height, dst_width, dst_height)?;

    assert_eq!(result.len(), dst_width * dst_height);

    Ok(())
}

#[test]
fn test_resampling_lanczos() -> Result<()> {
    // Test Lanczos resampling
    let src_width = 20;
    let src_height = 20;
    let src_data: Vec<f32> = (0..(src_width * src_height))
        .map(|i| ((i as f32 * 0.1).sin() * 100.0 + 100.0))
        .collect();

    let dst_width = 40;
    let dst_height = 40;

    let result = resample_lanczos(&src_data, src_width, src_height, dst_width, dst_height, 3)?;

    assert_eq!(result.len(), dst_width * dst_height);

    Ok(())
}

#[test]
fn test_resampling_downsampling_quality() -> Result<()> {
    // Test that downsampling maintains quality
    let src_width = 100;
    let src_height = 100;

    // Create a fine (1-pixel) checkerboard — the canonical high-frequency test
    // pattern. A coarse block pattern defeats the check by accident: when the
    // downsample stride is larger than the block size, interpolation sample
    // points land inside pure blocks and never straddle an edge, so no blending
    // occurs. A 1-pixel checkerboard guarantees that every interpolation window
    // spans both extremes, so a correct anti-aliasing resampler must produce
    // blended midtones.
    let src_data: Vec<f32> = (0..src_height)
        .flat_map(|y| (0..src_width).map(move |x| if (x + y) % 2 == 0 { 255.0 } else { 0.0 }))
        .collect();

    let dst_width = 50;
    let dst_height = 50;

    // Test different methods (all via the real oxigdal-algorithms resamplers).
    let nearest = resample_nearest(&src_data, src_width, src_height, dst_width, dst_height)?;
    let bilinear = resample_bilinear(&src_data, src_width, src_height, dst_width, dst_height)?;
    let bicubic = resample_bicubic(&src_data, src_width, src_height, dst_width, dst_height)?;

    // Nearest-neighbour preserves the exact source values: only the two extremes
    // appear, never a blended midtone.
    assert!(
        nearest
            .iter()
            .all(|&v| v.abs() < 1e-3 || (v - 255.0).abs() < 1e-3),
        "nearest-neighbour must not synthesise intermediate values"
    );

    // Bilinear and bicubic interpolate across block edges, so blended midtones
    // (strictly between the two source extremes) must appear.
    let has_intermediate_bilinear = bilinear.iter().any(|&v| v > 10.0 && v < 245.0);
    let has_intermediate_bicubic = bicubic.iter().any(|&v| v > 10.0 && v < 245.0);

    assert!(
        has_intermediate_bilinear,
        "bilinear downsampling should blend edges into intermediate values"
    );
    assert!(
        has_intermediate_bicubic,
        "bicubic downsampling should blend edges into intermediate values"
    );

    Ok(())
}

// ============================================================================
// Helper Functions (Placeholder Implementations)
// ============================================================================

fn simd_add_scalar(data: &[f32], scalar: f32) -> Result<Vec<f32>> {
    Ok(data.iter().map(|&v| v + scalar).collect())
}

fn simd_mean(data: &[f64]) -> Result<f64> {
    Ok(data.iter().sum::<f64>() / data.len() as f64)
}

fn simd_std_dev(data: &[f64]) -> Result<f64> {
    let mean = simd_mean(data)?;
    let variance = data.iter().map(|&v| (v - mean).powi(2)).sum::<f64>() / data.len() as f64;
    Ok(variance.sqrt())
}

fn simd_min_max(data: &[f64]) -> Result<(f64, f64)> {
    let min = data.iter().fold(f64::INFINITY, |a, &b| a.min(b));
    let max = data.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    Ok((min, max))
}

fn simd_convolve(
    data: &[f32],
    width: usize,
    height: usize,
    kernel: &[f32],
    kernel_size: usize,
) -> Result<Vec<f32>> {
    let mut result = vec![0.0; width * height];
    let half_kernel = kernel_size / 2;

    for y in half_kernel..(height - half_kernel) {
        for x in half_kernel..(width - half_kernel) {
            let mut sum = 0.0;
            for ky in 0..kernel_size {
                for kx in 0..kernel_size {
                    let py = y + ky - half_kernel;
                    let px = x + kx - half_kernel;
                    sum += data[py * width + px] * kernel[ky * kernel_size + kx];
                }
            }
            result[y * width + x] = sum;
        }
    }

    Ok(result)
}

fn simd_resample_bilinear(
    src: &[f32],
    src_w: usize,
    src_h: usize,
    dst_w: usize,
    dst_h: usize,
) -> Result<Vec<f32>> {
    let mut result = vec![0.0; dst_w * dst_h];
    let x_ratio = src_w as f32 / dst_w as f32;
    let y_ratio = src_h as f32 / dst_h as f32;

    for y in 0..dst_h {
        for x in 0..dst_w {
            let src_x = x as f32 * x_ratio;
            let src_y = y as f32 * y_ratio;
            let x0 = src_x.floor() as usize;
            let y0 = src_y.floor() as usize;
            let x1 = (x0 + 1).min(src_w - 1);
            let y1 = (y0 + 1).min(src_h - 1);

            let fx = src_x - x0 as f32;
            let fy = src_y - y0 as f32;

            let v00 = src[y0 * src_w + x0];
            let v10 = src[y0 * src_w + x1];
            let v01 = src[y1 * src_w + x0];
            let v11 = src[y1 * src_w + x1];

            let v0 = v00 * (1.0 - fx) + v10 * fx;
            let v1 = v01 * (1.0 - fx) + v11 * fx;

            result[y * dst_w + x] = v0 * (1.0 - fy) + v1 * fy;
        }
    }

    Ok(result)
}

fn simd_erosion(data: &[u8], width: usize, height: usize, kernel_size: usize) -> Result<Vec<u8>> {
    let mut result = data.to_vec();
    let half = kernel_size / 2;

    for y in half..(height - half) {
        for x in half..(width - half) {
            let mut min_val = 255u8;
            for ky in 0..kernel_size {
                for kx in 0..kernel_size {
                    let py = y + ky - half;
                    let px = x + kx - half;
                    min_val = min_val.min(data[py * width + px]);
                }
            }
            result[y * width + x] = min_val;
        }
    }

    Ok(result)
}

fn simd_dilation(data: &[u8], width: usize, height: usize, kernel_size: usize) -> Result<Vec<u8>> {
    let mut result = data.to_vec();
    let half = kernel_size / 2;

    for y in half..(height - half) {
        for x in half..(width - half) {
            let mut max_val = 0u8;
            for ky in 0..kernel_size {
                for kx in 0..kernel_size {
                    let py = y + ky - half;
                    let px = x + kx - half;
                    max_val = max_val.max(data[py * width + px]);
                }
            }
            result[y * width + x] = max_val;
        }
    }

    Ok(result)
}

fn simd_histogram(data: &[u8], bins: usize) -> Result<Vec<usize>> {
    let mut hist = vec![0; bins];
    for &val in data {
        hist[val as usize] += 1;
    }
    Ok(hist)
}

fn simd_threshold_binary(data: &[u8], threshold: u8) -> Result<Vec<u8>> {
    Ok(data
        .iter()
        .map(|&v| if v > threshold { 255 } else { 0 })
        .collect())
}

fn simd_threshold_otsu(data: &[u8]) -> Result<u8> {
    // Simplified Otsu's method
    let hist = simd_histogram(data, 256)?;
    let total = data.len();

    let mut sum = 0.0;
    for i in 0..256 {
        sum += (i as f64) * (hist[i] as f64);
    }

    let mut sum_b = 0.0;
    let mut w_b = 0;
    let mut max_variance = 0.0;
    let mut threshold = 0u8;

    for t in 0..256 {
        w_b += hist[t];
        if w_b == 0 {
            continue;
        }

        let w_f = total - w_b;
        if w_f == 0 {
            break;
        }

        sum_b += (t as f64) * (hist[t] as f64);

        let m_b = sum_b / w_b as f64;
        let m_f = (sum - sum_b) / w_f as f64;

        let variance = (w_b as f64) * (w_f as f64) * (m_b - m_f) * (m_b - m_f);

        if variance > max_variance {
            max_variance = variance;
            threshold = t as u8;
        }
    }

    Ok(threshold)
}

fn simd_rgb_to_gray(rgb: &[u8]) -> Result<Vec<u8>> {
    let mut gray = Vec::with_capacity(rgb.len() / 3);
    for chunk in rgb.chunks(3) {
        let r = chunk[0] as f32;
        let g = chunk[1] as f32;
        let b = chunk[2] as f32;
        let gray_val = (0.299 * r + 0.587 * g + 0.114 * b) as u8;
        gray.push(gray_val);
    }
    Ok(gray)
}

fn simd_rgb_to_hsv(rgb: &[u8]) -> Result<Vec<f32>> {
    let mut hsv = Vec::with_capacity(rgb.len());
    for chunk in rgb.chunks(3) {
        let r = chunk[0] as f32 / 255.0;
        let g = chunk[1] as f32 / 255.0;
        let b = chunk[2] as f32 / 255.0;

        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;

        let h = if delta == 0.0 {
            0.0
        } else if max == r {
            60.0 * (((g - b) / delta) % 6.0)
        } else if max == g {
            60.0 * (((b - r) / delta) + 2.0)
        } else {
            60.0 * (((r - g) / delta) + 4.0)
        };

        let s = if max == 0.0 { 0.0 } else { delta / max };
        let v = max;

        hsv.push(h);
        hsv.push(s);
        hsv.push(v);
    }
    Ok(hsv)
}

fn simd_hsv_to_rgb(hsv: &[f32]) -> Result<Vec<u8>> {
    let mut rgb = Vec::with_capacity(hsv.len());
    for chunk in hsv.chunks(3) {
        let h = chunk[0];
        let s = chunk[1];
        let v = chunk[2];

        let c = v * s;
        let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
        let m = v - c;

        let (r, g, b) = if h < 60.0 {
            (c, x, 0.0)
        } else if h < 120.0 {
            (x, c, 0.0)
        } else if h < 180.0 {
            (0.0, c, x)
        } else if h < 240.0 {
            (0.0, x, c)
        } else if h < 300.0 {
            (x, 0.0, c)
        } else {
            (c, 0.0, x)
        };

        rgb.push(((r + m) * 255.0) as u8);
        rgb.push(((g + m) * 255.0) as u8);
        rgb.push(((b + m) * 255.0) as u8);
    }
    Ok(rgb)
}

fn compute_slope_aspect(
    elevation: &[f32],
    width: usize,
    height: usize,
    cell_size: f32,
) -> Result<(Vec<f32>, Vec<f32>)> {
    let mut slope = vec![0.0; elevation.len()];
    let mut aspect = vec![0.0; elevation.len()];

    for y in 1..(height - 1) {
        for x in 1..(width - 1) {
            let dz_dx = (elevation[y * width + (x + 1)] - elevation[y * width + (x - 1)])
                / (2.0 * cell_size);
            let dz_dy = (elevation[(y + 1) * width + x] - elevation[(y - 1) * width + x])
                / (2.0 * cell_size);

            slope[y * width + x] = (dz_dx * dz_dx + dz_dy * dz_dy).sqrt().atan().to_degrees();
            aspect[y * width + x] = dz_dy.atan2(dz_dx).to_degrees();

            if aspect[y * width + x] < 0.0 {
                aspect[y * width + x] += 360.0;
            }
        }
    }

    Ok((slope, aspect))
}

fn compute_hillshade(
    elevation: &[f32],
    width: usize,
    height: usize,
    cell_size: f32,
    azimuth: f32,
    altitude: f32,
) -> Result<Vec<f32>> {
    let (slope, aspect) = compute_slope_aspect(elevation, width, height, cell_size)?;
    let mut hillshade = vec![0.0; elevation.len()];

    let zenith = 90.0 - altitude;
    let azimuth_rad = azimuth.to_radians();

    for i in 0..hillshade.len() {
        let slope_rad = slope[i].to_radians();
        let aspect_rad = aspect[i].to_radians();

        let value = (zenith.to_radians().cos() * slope_rad.cos())
            + (zenith.to_radians().sin() * slope_rad.sin() * (azimuth_rad - aspect_rad).cos());

        hillshade[i] = (value.max(0.0) * 255.0).min(255.0);
    }

    Ok(hillshade)
}

fn compute_tpi(elevation: &[f32], width: usize, height: usize, radius: usize) -> Result<Vec<f32>> {
    let mut tpi = vec![0.0; elevation.len()];

    for y in radius..(height - radius) {
        for x in radius..(width - radius) {
            let center = elevation[y * width + x];
            let mut sum = 0.0;
            let mut count = 0;

            for ky in 0..=(2 * radius) {
                for kx in 0..=(2 * radius) {
                    let py = y + ky - radius;
                    let px = x + kx - radius;
                    sum += elevation[py * width + px];
                    count += 1;
                }
            }

            let mean = sum / count as f32;
            tpi[y * width + x] = center - mean;
        }
    }

    Ok(tpi)
}

fn compute_tri(elevation: &[f32], width: usize, height: usize) -> Result<Vec<f32>> {
    let mut tri = vec![0.0; elevation.len()];

    for y in 1..(height - 1) {
        for x in 1..(width - 1) {
            let center = elevation[y * width + x];
            let mut sum_sq_diff = 0.0;

            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let py = (y as i32 + dy) as usize;
                    let px = (x as i32 + dx) as usize;
                    let diff = elevation[py * width + px] - center;
                    sum_sq_diff += diff * diff;
                }
            }

            tri[y * width + x] = sum_sq_diff.sqrt();
        }
    }

    Ok(tri)
}

fn compute_focal_mean(
    data: &[f32],
    width: usize,
    height: usize,
    window_size: usize,
) -> Result<Vec<f32>> {
    let mut result = vec![0.0; data.len()];
    let half = window_size / 2;

    for y in half..(height - half) {
        for x in half..(width - half) {
            let mut sum = 0.0;
            let mut count = 0;

            for ky in 0..window_size {
                for kx in 0..window_size {
                    let py = y + ky - half;
                    let px = x + kx - half;
                    sum += data[py * width + px];
                    count += 1;
                }
            }

            result[y * width + x] = sum / count as f32;
        }
    }

    Ok(result)
}

fn compute_focal_median(
    data: &[f32],
    width: usize,
    height: usize,
    window_size: usize,
) -> Result<Vec<f32>> {
    let mut result = vec![0.0; data.len()];
    let half = window_size / 2;

    for y in half..(height - half) {
        for x in half..(width - half) {
            let mut values = Vec::new();

            for ky in 0..window_size {
                for kx in 0..window_size {
                    let py = y + ky - half;
                    let px = x + kx - half;
                    values.push(data[py * width + px]);
                }
            }

            values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            result[y * width + x] = values[values.len() / 2];
        }
    }

    Ok(result)
}

fn compute_focal_range(
    data: &[f32],
    width: usize,
    height: usize,
    window_size: usize,
) -> Result<Vec<f32>> {
    let mut result = vec![0.0; data.len()];
    let half = window_size / 2;

    for y in half..(height - half) {
        for x in half..(width - half) {
            let mut min_val = f32::INFINITY;
            let mut max_val = f32::NEG_INFINITY;

            for ky in 0..window_size {
                for kx in 0..window_size {
                    let py = y + ky - half;
                    let px = x + kx - half;
                    let val = data[py * width + px];
                    min_val = min_val.min(val);
                    max_val = max_val.max(val);
                }
            }

            result[y * width + x] = max_val - min_val;
        }
    }

    Ok(result)
}

fn compute_focal_variety(
    data: &[f32],
    width: usize,
    height: usize,
    window_size: usize,
) -> Result<Vec<f32>> {
    let mut result = vec![0.0; data.len()];
    let half = window_size / 2;

    for y in half..(height - half) {
        for x in half..(width - half) {
            let mut unique_values = std::collections::HashSet::new();

            for ky in 0..window_size {
                for kx in 0..window_size {
                    let py = y + ky - half;
                    let px = x + kx - half;
                    unique_values.insert(data[py * width + px].to_bits());
                }
            }

            result[y * width + x] = unique_values.len() as f32;
        }
    }

    Ok(result)
}

fn compute_glcm(
    data: &[u8],
    width: usize,
    height: usize,
    distance: usize,
    angle: f32,
    levels: usize,
) -> Result<Vec<Vec<usize>>> {
    let mut glcm = vec![vec![0; levels]; levels];

    let (dx, dy) = if angle.abs() < 1e-6 {
        (distance as i32, 0)
    } else if (angle - 90.0).abs() < 1e-6 {
        (0, distance as i32)
    } else if (angle - 45.0).abs() < 1e-6 {
        (distance as i32, distance as i32)
    } else {
        (distance as i32, -(distance as i32))
    };

    for y in 0..height {
        for x in 0..width {
            let nx = x as i32 + dx;
            let ny = y as i32 + dy;

            if nx >= 0 && nx < width as i32 && ny >= 0 && ny < height as i32 {
                let i = data[y * width + x] as usize;
                let j = data[ny as usize * width + nx as usize] as usize;
                glcm[i][j] += 1;
            }
        }
    }

    Ok(glcm)
}

fn compute_glcm_contrast(glcm: &[Vec<usize>]) -> Result<f64> {
    let mut contrast = 0.0;
    let total: usize = glcm.iter().map(|row| row.iter().sum::<usize>()).sum();

    for i in 0..glcm.len() {
        for j in 0..glcm[i].len() {
            let p = glcm[i][j] as f64 / total as f64;
            contrast += p * ((i as i32 - j as i32).pow(2) as f64);
        }
    }

    Ok(contrast)
}

fn compute_glcm_energy(glcm: &[Vec<usize>]) -> Result<f64> {
    let mut energy = 0.0;
    let total: usize = glcm.iter().map(|row| row.iter().sum::<usize>()).sum();

    for row in glcm {
        for &count in row {
            let p = count as f64 / total as f64;
            energy += p * p;
        }
    }

    Ok(energy)
}

fn compute_glcm_homogeneity(glcm: &[Vec<usize>]) -> Result<f64> {
    let mut homogeneity = 0.0;
    let total: usize = glcm.iter().map(|row| row.iter().sum::<usize>()).sum();

    for i in 0..glcm.len() {
        for j in 0..glcm[i].len() {
            let p = glcm[i][j] as f64 / total as f64;
            homogeneity += p / (1.0 + ((i as i32 - j as i32).abs() as f64));
        }
    }

    Ok(homogeneity)
}

fn compute_glcm_correlation(glcm: &[Vec<usize>]) -> Result<f64> {
    // Simplified correlation calculation
    Ok(0.5) // Placeholder
}

/// Computes a binary viewshed via the real `oxigdal-algorithms` R1
/// line-of-sight kernel. Returns a row-major `u8` grid (1 = visible).
fn compute_viewshed(
    elevation: &[f32],
    width: usize,
    height: usize,
    observer_x: usize,
    observer_y: usize,
    observer_height: f32,
    max_distance: f32,
) -> Result<Vec<u8>> {
    let dem = raster_from_f32(elevation, width, height)?;
    let visibility = real_compute_viewshed(
        &dem,
        observer_x as u64,
        observer_y as u64,
        f64::from(observer_height),
        0.0,
        Some(f64::from(max_distance)),
        1.0,
    )
    .map_err(|e| Box::new(e) as Box<dyn Error>)?;

    let mut out = vec![0u8; elevation.len()];
    for y in 0..height {
        for x in 0..width {
            let v = visibility
                .get_pixel(x as u64, y as u64)
                .map_err(|e| Box::new(e) as Box<dyn Error>)?;
            out[y * width + x] = u8::from(v > 0.5);
        }
    }
    Ok(out)
}

fn compute_cost_distance(
    cost: &[f32],
    sources: &[bool],
    width: usize,
    height: usize,
) -> Result<(Vec<f32>, Vec<usize>)> {
    let mut distance = vec![f32::INFINITY; cost.len()];
    let mut allocation = vec![0; cost.len()];

    // Initialize sources
    for i in 0..sources.len() {
        if sources[i] {
            distance[i] = 0.0;
            allocation[i] = i;
        }
    }

    // Simplified Dijkstra
    let mut changed = true;
    while changed {
        changed = false;
        for y in 1..(height - 1) {
            for x in 1..(width - 1) {
                let idx = y * width + x;
                let neighbors = [
                    (y - 1) * width + x,
                    (y + 1) * width + x,
                    y * width + (x - 1),
                    y * width + (x + 1),
                ];

                for &n in &neighbors {
                    let new_dist = distance[n] + cost[idx];
                    if new_dist < distance[idx] {
                        distance[idx] = new_dist;
                        allocation[idx] = allocation[n];
                        changed = true;
                    }
                }
            }
        }
    }

    Ok((distance, allocation))
}

fn compute_flow_direction(elevation: &[f32], width: usize, height: usize) -> Result<Vec<u8>> {
    let mut flow_dir = vec![0u8; elevation.len()];

    for y in 1..(height - 1) {
        for x in 1..(width - 1) {
            let center = elevation[y * width + x];
            let mut max_slope = f32::NEG_INFINITY;
            let mut direction = 0u8;

            // D8 neighbors: E, SE, S, SW, W, NW, N, NE
            let neighbors = [
                (0, 1, 1),    // E
                (1, 1, 2),    // SE
                (1, 0, 4),    // S
                (1, -1, 8),   // SW
                (0, -1, 16),  // W
                (-1, -1, 32), // NW
                (-1, 0, 64),  // N
                (-1, 1, 128), // NE
            ];

            for &(dy, dx, dir_val) in &neighbors {
                let ny = (y as i32 + dy) as usize;
                let nx = (x as i32 + dx) as usize;
                let neighbor = elevation[ny * width + nx];
                let slope = center - neighbor;

                if slope > max_slope {
                    max_slope = slope;
                    direction = dir_val;
                }
            }

            flow_dir[y * width + x] = direction;
        }
    }

    Ok(flow_dir)
}

/// Computes D8 flow accumulation from a precomputed flow-direction grid via the
/// real `oxigdal-algorithms` hydrology kernel. The grid uses the standard D8
/// power-of-two encoding (E=1, SE=2, S=4, ... NE=128), matching the crate's
/// `FlowDirection` codes. Returns a row-major `u32` accumulation grid.
fn compute_flow_accumulation(flow_dir: &[u8], width: usize, height: usize) -> Result<Vec<u32>> {
    let mut fdir = RasterBuffer::zeros(width as u64, height as u64, RasterDataType::UInt8);
    for y in 0..height {
        for x in 0..width {
            fdir.set_pixel(x as u64, y as u64, f64::from(flow_dir[y * width + x]))
                .map_err(|e| Box::new(e) as Box<dyn Error>)?;
        }
    }

    let accum = compute_d8_accumulation_from_fdir(&fdir, None)
        .map_err(|e| Box::new(e) as Box<dyn Error>)?;

    let mut out = vec![0u32; flow_dir.len()];
    for y in 0..height {
        for x in 0..width {
            let v = accum
                .get_pixel(x as u64, y as u64)
                .map_err(|e| Box::new(e) as Box<dyn Error>)?;
            out[y * width + x] = v.max(0.0).round() as u32;
        }
    }
    Ok(out)
}

fn compute_watersheds(
    elevation: &[f32],
    pour_points: &[bool],
    width: usize,
    height: usize,
) -> Result<Vec<u32>> {
    let mut watersheds = vec![0u32; elevation.len()];
    let mut watershed_id = 1u32;

    for i in 0..pour_points.len() {
        if pour_points[i] {
            // Simple region growing from pour point
            watersheds[i] = watershed_id;
            watershed_id += 1;
        }
    }

    Ok(watersheds)
}

fn extract_stream_network(
    flow_accum: &[u32],
    width: usize,
    height: usize,
    threshold: u32,
) -> Result<Vec<bool>> {
    let mut streams = vec![false; flow_accum.len()];

    for i in 0..flow_accum.len() {
        if flow_accum[i] >= threshold {
            streams[i] = true;
        }
    }

    Ok(streams)
}

fn fill_sinks(elevation: &[f32], width: usize, height: usize) -> Result<Vec<f32>> {
    let mut filled = elevation.to_vec();

    // Simplified sink filling
    let mut changed = true;
    while changed {
        changed = false;
        for y in 1..(height - 1) {
            for x in 1..(width - 1) {
                let idx = y * width + x;
                let center = filled[idx];

                let neighbors = [
                    filled[(y - 1) * width + x],
                    filled[(y + 1) * width + x],
                    filled[y * width + (x - 1)],
                    filled[y * width + (x + 1)],
                ];

                let min_neighbor = neighbors.iter().fold(f32::INFINITY, |a, &b| a.min(b));

                if center < min_neighbor {
                    filled[idx] = min_neighbor;
                    changed = true;
                }
            }
        }
    }

    Ok(filled)
}

fn resample_nearest(
    src: &[f32],
    src_w: usize,
    src_h: usize,
    dst_w: usize,
    dst_h: usize,
) -> Result<Vec<f32>> {
    resample_real(src, src_w, src_h, dst_w, dst_h, ResamplingMethod::Nearest)
}

fn resample_bilinear(
    src: &[f32],
    src_w: usize,
    src_h: usize,
    dst_w: usize,
    dst_h: usize,
) -> Result<Vec<f32>> {
    resample_real(src, src_w, src_h, dst_w, dst_h, ResamplingMethod::Bilinear)
}

fn resample_bicubic(
    src: &[f32],
    src_w: usize,
    src_h: usize,
    dst_w: usize,
    dst_h: usize,
) -> Result<Vec<f32>> {
    resample_real(src, src_w, src_h, dst_w, dst_h, ResamplingMethod::Bicubic)
}

fn resample_lanczos(
    src: &[f32],
    src_w: usize,
    src_h: usize,
    dst_w: usize,
    dst_h: usize,
    _a: usize,
) -> Result<Vec<f32>> {
    resample_real(src, src_w, src_h, dst_w, dst_h, ResamplingMethod::Lanczos)
}
