//! Comprehensive tests for focal statistics algorithms
//!
//! Tests focal/neighborhood operations including:
//! - Window shapes (rectangular, circular, custom)
//! - Focal operations (mean, median, range, variety, min, max, sum, stddev, majority)
//! - Boundary handling modes
//! - Edge cases and large datasets

use oxigdal_algorithms::raster::{
    FocalBoundaryMode, WindowShape, focal_convolve, focal_majority, focal_max, focal_mean,
    focal_mean_separable, focal_median, focal_min, focal_range, focal_stddev, focal_sum,
    focal_variety,
};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

// ============================================================================
// Window Shape Tests
// ============================================================================

#[test]
fn test_window_shape_rectangular_creation() {
    // Valid rectangular window
    let window = WindowShape::rectangular(3, 3);
    assert!(window.is_ok());
    let window = window.expect("Should create rectangular window");
    assert_eq!(window.dimensions(), (3, 3));
    assert_eq!(window.cell_count(), 9);

    // Larger rectangular window
    let window = WindowShape::rectangular(5, 7).expect("Should create 5x7 window");
    assert_eq!(window.dimensions(), (5, 7));
    assert_eq!(window.cell_count(), 35);
}

#[test]
fn test_window_shape_rectangular_errors() {
    // Even dimensions should fail
    let result = WindowShape::rectangular(4, 3);
    assert!(result.is_err());

    let result = WindowShape::rectangular(3, 4);
    assert!(result.is_err());

    // Zero dimensions should fail
    let result = WindowShape::rectangular(0, 3);
    assert!(result.is_err());

    let result = WindowShape::rectangular(3, 0);
    assert!(result.is_err());
}

#[test]
fn test_window_shape_circular_creation() {
    // Valid circular window
    let window = WindowShape::circular(1.5);
    assert!(window.is_ok());
    let window = window.expect("Should create circular window");

    // Check pixel inclusion
    assert!(window.includes(0, 0)); // Center
    assert!(window.includes(1, 0)); // Cardinal
    assert!(window.includes(0, 1)); // Cardinal
    assert!(window.includes(1, 1)); // Diagonal within radius
    assert!(!window.includes(2, 2)); // Outside radius
}

#[test]
fn test_window_shape_circular_radius_error() {
    // Zero radius should fail
    let result = WindowShape::circular(0.0);
    assert!(result.is_err());

    // Negative radius should fail
    let result = WindowShape::circular(-1.0);
    assert!(result.is_err());
}

#[test]
fn test_window_shape_custom_creation() {
    // Plus-shaped window (cross pattern)
    let mask = vec![false, true, false, true, true, true, false, true, false];
    let window = WindowShape::custom(mask.clone(), 3, 3);
    assert!(window.is_ok());
    let window = window.expect("Should create custom window");
    assert_eq!(window.cell_count(), 5);

    // Verify mask pattern
    assert!(!window.includes(-1, -1)); // Top-left
    assert!(window.includes(0, -1)); // Top-center
    assert!(!window.includes(1, -1)); // Top-right
    assert!(window.includes(-1, 0)); // Middle-left
    assert!(window.includes(0, 0)); // Center
    assert!(window.includes(1, 0)); // Middle-right
}

#[test]
fn test_window_shape_custom_errors() {
    // Mask size mismatch
    let mask = vec![true, true, true]; // 3 elements, not 9
    let result = WindowShape::custom(mask, 3, 3);
    assert!(result.is_err());

    // Even dimensions
    let mask = vec![true; 8];
    let result = WindowShape::custom(mask, 4, 2);
    assert!(result.is_err());
}

#[test]
fn test_window_offsets() {
    // 3x3 rectangular should have 9 offsets
    let window = WindowShape::rectangular(3, 3).expect("Should create window");
    let offsets = window.offsets();
    assert_eq!(offsets.len(), 9);

    // Check that all offsets are within expected range
    for (dx, dy) in &offsets {
        assert!(*dx >= -1 && *dx <= 1);
        assert!(*dy >= -1 && *dy <= 1);
    }
}

// ============================================================================
// Helper Functions for Test Data Creation
// ============================================================================

fn create_test_raster(width: u64, height: u64) -> RasterBuffer {
    RasterBuffer::zeros(width, height, RasterDataType::Float32)
}

fn create_gradient_raster(width: u64, height: u64) -> RasterBuffer {
    let mut raster = RasterBuffer::zeros(width, height, RasterDataType::Float32);
    for y in 0..height {
        for x in 0..width {
            let _ = raster.set_pixel(x, y, (x + y) as f64);
        }
    }
    raster
}

fn create_uniform_raster(width: u64, height: u64, value: f64) -> RasterBuffer {
    let mut raster = RasterBuffer::zeros(width, height, RasterDataType::Float32);
    for y in 0..height {
        for x in 0..width {
            let _ = raster.set_pixel(x, y, value);
        }
    }
    raster
}

fn create_checkerboard_raster(width: u64, height: u64, val1: f64, val2: f64) -> RasterBuffer {
    let mut raster = RasterBuffer::zeros(width, height, RasterDataType::Float32);
    for y in 0..height {
        for x in 0..width {
            let val = if (x + y) % 2 == 0 { val1 } else { val2 };
            let _ = raster.set_pixel(x, y, val);
        }
    }
    raster
}

// ============================================================================
// Focal Mean Tests
// ============================================================================

#[test]
fn test_focal_mean_uniform_input() {
    let src = create_uniform_raster(10, 10, 5.0);
    let window = WindowShape::rectangular(3, 3).expect("Should create window");

    let result =
        focal_mean(&src, &window, &FocalBoundaryMode::Edge).expect("Should compute focal mean");

    // Mean of uniform values should be the same value
    let center = result.get_pixel(5, 5).expect("Should get pixel");
    assert!((center - 5.0).abs() < 1e-6);
}

#[test]
fn test_focal_mean_single_point() {
    let mut src = create_test_raster(5, 5);
    let _ = src.set_pixel(2, 2, 9.0);

    let window = WindowShape::rectangular(3, 3).expect("Should create window");
    let result =
        focal_mean(&src, &window, &FocalBoundaryMode::Edge).expect("Should compute focal mean");

    // Center should be 9/9 = 1.0 (one 9 and eight 0s)
    let center = result.get_pixel(2, 2).expect("Should get pixel");
    assert!((center - 1.0).abs() < 0.01);
}

#[test]
fn test_focal_mean_gradient() {
    let src = create_gradient_raster(10, 10);
    let window = WindowShape::rectangular(3, 3).expect("Should create window");

    let result =
        focal_mean(&src, &window, &FocalBoundaryMode::Edge).expect("Should compute focal mean");

    // Result should be smoother than input but preserve general gradient
    assert!(result.get_pixel(5, 5).is_ok());
}

#[test]
fn test_focal_mean_large_window() {
    let src = create_gradient_raster(20, 20);
    let window = WindowShape::rectangular(5, 5).expect("Should create window");

    let result =
        focal_mean(&src, &window, &FocalBoundaryMode::Edge).expect("Should compute focal mean");

    // Should produce valid output
    let val = result.get_pixel(10, 10).expect("Should get pixel");
    assert!(val.is_finite());
}

// ============================================================================
// Focal Median Tests
// ============================================================================

#[test]
fn test_focal_median_odd_values() {
    let mut src = create_test_raster(5, 5);
    // Fill center with values 1-9
    let values = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0];
    let mut idx = 0;
    for y in 1..4 {
        for x in 1..4 {
            let _ = src.set_pixel(x, y, values[idx]);
            idx += 1;
        }
    }

    let window = WindowShape::rectangular(3, 3).expect("Should create window");
    let result = focal_median(&src, &window, &FocalBoundaryMode::Ignore)
        .expect("Should compute focal median");

    // Median of 1-9 should be 5.0
    let center = result.get_pixel(2, 2).expect("Should get pixel");
    assert!((center - 5.0).abs() < 0.01);
}

#[test]
fn test_focal_median_removes_outliers() {
    let mut src = create_uniform_raster(5, 5, 10.0);
    // Add outlier
    let _ = src.set_pixel(2, 2, 1000.0);

    let window = WindowShape::rectangular(3, 3).expect("Should create window");
    let result =
        focal_median(&src, &window, &FocalBoundaryMode::Edge).expect("Should compute focal median");

    // Median should be 10.0 (the outlier doesn't affect median)
    let neighbor = result.get_pixel(2, 1).expect("Should get pixel");
    assert!((neighbor - 10.0).abs() < 0.01);
}

// ============================================================================
// Focal Range Tests
// ============================================================================

#[test]
fn test_focal_range_uniform() {
    let src = create_uniform_raster(10, 10, 5.0);
    let window = WindowShape::rectangular(3, 3).expect("Should create window");

    let result =
        focal_range(&src, &window, &FocalBoundaryMode::Edge).expect("Should compute focal range");

    // Range of uniform values should be 0
    let center = result.get_pixel(5, 5).expect("Should get pixel");
    assert!(center.abs() < 1e-6);
}

#[test]
fn test_focal_range_with_variation() {
    let mut src = create_test_raster(5, 5);
    let _ = src.set_pixel(2, 2, 10.0);
    let _ = src.set_pixel(2, 3, 5.0);

    let window = WindowShape::rectangular(3, 3).expect("Should create window");
    let result =
        focal_range(&src, &window, &FocalBoundaryMode::Edge).expect("Should compute focal range");

    // Range should be max - min = 10 - 0 = 10
    let center = result.get_pixel(2, 2).expect("Should get pixel");
    assert!((center - 10.0).abs() < 0.01);
}

// ============================================================================
// Focal Variety Tests
// ============================================================================

#[test]
fn test_focal_variety_uniform() {
    let src = create_uniform_raster(10, 10, 5.0);
    let window = WindowShape::rectangular(3, 3).expect("Should create window");

    let result = focal_variety(&src, &window, &FocalBoundaryMode::Edge)
        .expect("Should compute focal variety");

    // Variety of uniform values should be 1
    let center = result.get_pixel(5, 5).expect("Should get pixel");
    assert!((center - 1.0).abs() < 0.01);
}

#[test]
fn test_focal_variety_all_unique() {
    let mut src = create_test_raster(5, 5);
    // Fill with unique values
    for y in 0..5 {
        for x in 0..5 {
            let _ = src.set_pixel(x, y, (x * 5 + y) as f64);
        }
    }

    let window = WindowShape::rectangular(3, 3).expect("Should create window");
    let result = focal_variety(&src, &window, &FocalBoundaryMode::Ignore)
        .expect("Should compute focal variety");

    // Interior cell should have 9 unique values
    let center = result.get_pixel(2, 2).expect("Should get pixel");
    assert!((center - 9.0).abs() < 0.01);
}

// ============================================================================
// Focal Min/Max Tests
// ============================================================================

#[test]
fn test_focal_min() {
    let mut src = create_uniform_raster(5, 5, 10.0);
    let _ = src.set_pixel(2, 2, 1.0);

    let window = WindowShape::rectangular(3, 3).expect("Should create window");
    let result =
        focal_min(&src, &window, &FocalBoundaryMode::Edge).expect("Should compute focal min");

    // Min should propagate to neighbors
    let neighbor = result.get_pixel(2, 1).expect("Should get pixel");
    assert!((neighbor - 1.0).abs() < 0.01);
}

#[test]
fn test_focal_max() {
    let mut src = create_uniform_raster(5, 5, 1.0);
    let _ = src.set_pixel(2, 2, 100.0);

    let window = WindowShape::rectangular(3, 3).expect("Should create window");
    let result =
        focal_max(&src, &window, &FocalBoundaryMode::Edge).expect("Should compute focal max");

    // Max should propagate to neighbors
    let neighbor = result.get_pixel(2, 1).expect("Should get pixel");
    assert!((neighbor - 100.0).abs() < 0.01);
}

// ============================================================================
// Focal Sum Tests
// ============================================================================

#[test]
fn test_focal_sum_uniform() {
    let src = create_uniform_raster(10, 10, 1.0);
    let window = WindowShape::rectangular(3, 3).expect("Should create window");

    let result =
        focal_sum(&src, &window, &FocalBoundaryMode::Edge).expect("Should compute focal sum");

    // Sum of 9 cells with value 1 = 9
    let center = result.get_pixel(5, 5).expect("Should get pixel");
    assert!((center - 9.0).abs() < 0.01);
}

#[test]
fn test_focal_sum_single_nonzero() {
    let mut src = create_test_raster(5, 5);
    let _ = src.set_pixel(2, 2, 10.0);

    let window = WindowShape::rectangular(3, 3).expect("Should create window");
    let result =
        focal_sum(&src, &window, &FocalBoundaryMode::Ignore).expect("Should compute focal sum");

    // Center sum should be 10
    let center = result.get_pixel(2, 2).expect("Should get pixel");
    assert!((center - 10.0).abs() < 0.01);
}

// ============================================================================
// Focal Standard Deviation Tests
// ============================================================================

#[test]
fn test_focal_stddev_uniform() {
    let src = create_uniform_raster(10, 10, 5.0);
    let window = WindowShape::rectangular(3, 3).expect("Should create window");

    let result =
        focal_stddev(&src, &window, &FocalBoundaryMode::Edge).expect("Should compute focal stddev");

    // StdDev of uniform values should be 0
    let center = result.get_pixel(5, 5).expect("Should get pixel");
    assert!(center.abs() < 1e-6);
}

#[test]
fn test_focal_stddev_variation() {
    let src = create_gradient_raster(10, 10);
    let window = WindowShape::rectangular(3, 3).expect("Should create window");

    let result =
        focal_stddev(&src, &window, &FocalBoundaryMode::Edge).expect("Should compute focal stddev");

    // StdDev should be positive for varying data
    let center = result.get_pixel(5, 5).expect("Should get pixel");
    assert!(center > 0.0);
}

// ============================================================================
// Focal Majority Tests
// ============================================================================

#[test]
fn test_focal_majority_clear_majority() {
    let mut src = create_uniform_raster(5, 5, 5.0);
    // Change a few values
    let _ = src.set_pixel(2, 2, 10.0);

    let window = WindowShape::rectangular(3, 3).expect("Should create window");
    let result = focal_majority(&src, &window, &FocalBoundaryMode::Edge)
        .expect("Should compute focal majority");

    // Majority should be 5 (8 fives vs 1 ten)
    let center = result.get_pixel(2, 2).expect("Should get pixel");
    assert!((center - 5.0).abs() < 0.01);
}

// ============================================================================
// Boundary Mode Tests
// ============================================================================

#[test]
fn test_boundary_mode_ignore() {
    let src = create_gradient_raster(5, 5);
    let window = WindowShape::rectangular(3, 3).expect("Should create window");

    let result = focal_mean(&src, &window, &FocalBoundaryMode::Ignore);
    assert!(result.is_ok());
}

#[test]
fn test_boundary_mode_constant() {
    let src = create_gradient_raster(5, 5);
    let window = WindowShape::rectangular(3, 3).expect("Should create window");

    let result = focal_mean(&src, &window, &FocalBoundaryMode::Constant(0));
    assert!(result.is_ok());
}

#[test]
fn test_boundary_mode_reflect() {
    let src = create_gradient_raster(5, 5);
    let window = WindowShape::rectangular(3, 3).expect("Should create window");

    let result = focal_mean(&src, &window, &FocalBoundaryMode::Reflect);
    assert!(result.is_ok());
}

#[test]
fn test_boundary_mode_wrap() {
    let src = create_gradient_raster(5, 5);
    let window = WindowShape::rectangular(3, 3).expect("Should create window");

    let result = focal_mean(&src, &window, &FocalBoundaryMode::Wrap);
    assert!(result.is_ok());
}

#[test]
fn test_boundary_mode_edge() {
    let src = create_gradient_raster(5, 5);
    let window = WindowShape::rectangular(3, 3).expect("Should create window");

    let result = focal_mean(&src, &window, &FocalBoundaryMode::Edge);
    assert!(result.is_ok());
}

// ============================================================================
// Separable Mean Tests
// ============================================================================

#[test]
fn test_focal_mean_separable_vs_generic() {
    let src = create_gradient_raster(20, 20);

    // Generic method
    let window = WindowShape::rectangular(3, 3).expect("Should create window");
    let generic_result =
        focal_mean(&src, &window, &FocalBoundaryMode::Edge).expect("Should compute generic mean");

    // Separable method
    let separable_result = focal_mean_separable(&src, 3, 3).expect("Should compute separable mean");

    // Results should be similar in interior
    for y in 2..18 {
        for x in 2..18 {
            let generic_val = generic_result.get_pixel(x, y).expect("Should get pixel");
            let separable_val = separable_result.get_pixel(x, y).expect("Should get pixel");
            assert!(
                (generic_val - separable_val).abs() < 0.5,
                "Values differ at ({}, {}): {} vs {}",
                x,
                y,
                generic_val,
                separable_val
            );
        }
    }
}

#[test]
fn test_focal_mean_separable_even_error() {
    let src = create_gradient_raster(10, 10);

    // Even dimensions should fail
    let result = focal_mean_separable(&src, 4, 3);
    assert!(result.is_err());

    let result = focal_mean_separable(&src, 3, 4);
    assert!(result.is_err());
}

// ============================================================================
// Convolution Tests
// ============================================================================

#[test]
fn test_focal_convolve_identity() {
    let src = create_gradient_raster(10, 10);

    // Identity kernel
    let kernel = vec![0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0];

    let result = focal_convolve(&src, &kernel, 3, 3, false).expect("Should compute convolution");

    // Result should be similar to input in interior
    for y in 1..9 {
        for x in 1..9 {
            let src_val = src.get_pixel(x, y).expect("Should get pixel");
            let result_val = result.get_pixel(x, y).expect("Should get pixel");
            assert!(
                (src_val - result_val).abs() < 0.01,
                "Values differ at ({}, {})",
                x,
                y
            );
        }
    }
}

#[test]
fn test_focal_convolve_box_blur() {
    let src = create_gradient_raster(10, 10);

    // Box blur kernel (all 1s, normalized)
    let kernel = vec![1.0; 9];

    let result = focal_convolve(&src, &kernel, 3, 3, true).expect("Should compute convolution");

    // Should produce smoothed output
    assert!(result.get_pixel(5, 5).is_ok());
}

#[test]
fn test_focal_convolve_size_mismatch_error() {
    let src = create_gradient_raster(10, 10);

    // Kernel size doesn't match dimensions
    let kernel = vec![1.0; 5];

    let result = focal_convolve(&src, &kernel, 3, 3, false);
    assert!(result.is_err());
}

// ============================================================================
// Circular Window Tests
// ============================================================================

#[test]
fn test_focal_mean_circular_window() {
    let src = create_gradient_raster(10, 10);
    let window = WindowShape::circular(1.5).expect("Should create circular window");

    let result =
        focal_mean(&src, &window, &FocalBoundaryMode::Edge).expect("Should compute focal mean");

    // Should produce valid output
    assert!(result.get_pixel(5, 5).is_ok());
}

// ============================================================================
// Edge Cases and Large Dataset Tests
// ============================================================================

#[test]
fn test_focal_operations_on_small_raster() {
    let src = create_gradient_raster(3, 3);
    let window = WindowShape::rectangular(3, 3).expect("Should create window");

    // All operations should work on minimum-size raster
    assert!(focal_mean(&src, &window, &FocalBoundaryMode::Edge).is_ok());
    assert!(focal_median(&src, &window, &FocalBoundaryMode::Edge).is_ok());
    assert!(focal_range(&src, &window, &FocalBoundaryMode::Edge).is_ok());
    assert!(focal_min(&src, &window, &FocalBoundaryMode::Edge).is_ok());
    assert!(focal_max(&src, &window, &FocalBoundaryMode::Edge).is_ok());
    assert!(focal_sum(&src, &window, &FocalBoundaryMode::Edge).is_ok());
}

#[test]
fn test_focal_operations_preserve_dimensions() {
    let src = create_gradient_raster(50, 30);
    let window = WindowShape::rectangular(5, 5).expect("Should create window");

    let result =
        focal_mean(&src, &window, &FocalBoundaryMode::Edge).expect("Should compute focal mean");

    assert_eq!(result.width(), 50);
    assert_eq!(result.height(), 30);
}

#[test]
fn test_focal_mean_large_dataset() {
    let src = create_uniform_raster(100, 100, 1.0);
    let window = WindowShape::rectangular(7, 7).expect("Should create window");

    let result =
        focal_mean(&src, &window, &FocalBoundaryMode::Edge).expect("Should compute focal mean");

    // All values should be 1.0 for uniform input
    let val = result.get_pixel(50, 50).expect("Should get pixel");
    assert!((val - 1.0).abs() < 1e-6);
}

#[test]
fn test_checkerboard_pattern_processing() {
    let src = create_checkerboard_raster(10, 10, 0.0, 10.0);
    let window = WindowShape::rectangular(3, 3).expect("Should create window");

    let mean_result =
        focal_mean(&src, &window, &FocalBoundaryMode::Edge).expect("Should compute focal mean");
    let range_result =
        focal_range(&src, &window, &FocalBoundaryMode::Edge).expect("Should compute focal range");

    // Mean should be around 5 for checkerboard
    let mean_val = mean_result.get_pixel(5, 5).expect("Should get pixel");
    assert!(mean_val > 3.0 && mean_val < 7.0);

    // Range should be 10
    let range_val = range_result.get_pixel(5, 5).expect("Should get pixel");
    assert!((range_val - 10.0).abs() < 0.01);
}

// ============================================================================
// NaN and Special Value Handling
// ============================================================================

#[test]
fn test_focal_mean_with_zeros() {
    let src = create_test_raster(10, 10);
    let window = WindowShape::rectangular(3, 3).expect("Should create window");

    let result =
        focal_mean(&src, &window, &FocalBoundaryMode::Edge).expect("Should compute focal mean");

    // Mean of zeros should be zero
    let center = result.get_pixel(5, 5).expect("Should get pixel");
    assert!(center.abs() < 1e-6);
}

// ============================================================================
// Integration Tests
// ============================================================================

#[test]
fn test_chained_focal_operations() {
    let src = create_gradient_raster(20, 20);
    let window = WindowShape::rectangular(3, 3).expect("Should create window");

    // Apply mean then range
    let smoothed =
        focal_mean(&src, &window, &FocalBoundaryMode::Edge).expect("Should compute focal mean");
    let range_of_smoothed = focal_range(&smoothed, &window, &FocalBoundaryMode::Edge)
        .expect("Should compute focal range");

    // Range should be smaller after smoothing
    let original_range =
        focal_range(&src, &window, &FocalBoundaryMode::Edge).expect("Should compute focal range");

    let orig_center = original_range.get_pixel(10, 10).expect("Should get pixel");
    let smooth_center = range_of_smoothed
        .get_pixel(10, 10)
        .expect("Should get pixel");

    assert!(smooth_center <= orig_center);
}
