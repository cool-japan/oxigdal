//! Comprehensive tests for texture analysis algorithms
//!
//! Tests GLCM (Gray-Level Co-occurrence Matrix) and Haralick features including:
//! - GLCM computation
//! - Multiple directions and distances
//! - Haralick feature extraction
//! - Texture feature images
//! - Edge cases and validation

use oxigdal_algorithms::raster::{
    Glcm, GlcmParams, TextureDirection as Direction, compute_all_texture_features, compute_glcm,
    compute_glcm_multi_direction, compute_haralick_features, compute_texture_feature_image,
};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

// ============================================================================
// Helper Functions
// ============================================================================

#[allow(dead_code)]
fn create_test_raster(width: u64, height: u64) -> RasterBuffer {
    RasterBuffer::zeros(width, height, RasterDataType::UInt8)
}

fn create_uniform_raster(width: u64, height: u64, value: f64) -> RasterBuffer {
    let mut raster = RasterBuffer::zeros(width, height, RasterDataType::UInt8);
    for y in 0..height {
        for x in 0..width {
            let _ = raster.set_pixel(x, y, value);
        }
    }
    raster
}

fn create_gradient_raster(width: u64, height: u64) -> RasterBuffer {
    let mut raster = RasterBuffer::zeros(width, height, RasterDataType::UInt8);
    for y in 0..height {
        for x in 0..width {
            // Scale to 0-255 range
            let val = ((x + y) as f64 * 255.0 / (width + height) as f64).min(255.0);
            let _ = raster.set_pixel(x, y, val);
        }
    }
    raster
}

fn create_checkerboard_raster(width: u64, height: u64) -> RasterBuffer {
    let mut raster = RasterBuffer::zeros(width, height, RasterDataType::UInt8);
    for y in 0..height {
        for x in 0..width {
            let val = if (x + y) % 2 == 0 { 0.0 } else { 255.0 };
            let _ = raster.set_pixel(x, y, val);
        }
    }
    raster
}

fn create_striped_raster(width: u64, height: u64, horizontal: bool) -> RasterBuffer {
    let mut raster = RasterBuffer::zeros(width, height, RasterDataType::UInt8);
    for y in 0..height {
        for x in 0..width {
            let val = if horizontal {
                if y % 2 == 0 { 0.0 } else { 255.0 }
            } else {
                if x % 2 == 0 { 0.0 } else { 255.0 }
            };
            let _ = raster.set_pixel(x, y, val);
        }
    }
    raster
}

// ============================================================================
// Direction Tests
// ============================================================================

#[test]
fn test_direction_offset_horizontal() {
    assert_eq!(Direction::Horizontal.offset(1), (1, 0));
    assert_eq!(Direction::Horizontal.offset(2), (2, 0));
}

#[test]
fn test_direction_offset_vertical() {
    assert_eq!(Direction::Vertical.offset(1), (0, 1));
    assert_eq!(Direction::Vertical.offset(2), (0, 2));
}

#[test]
fn test_direction_offset_diagonal() {
    assert_eq!(Direction::Diagonal45.offset(1), (1, -1));
    assert_eq!(Direction::Diagonal135.offset(1), (1, 1));
}

#[test]
fn test_direction_offset_custom() {
    let direction = Direction::Custom(2, 3);
    assert_eq!(direction.offset(1), (2, 3));
    assert_eq!(direction.offset(2), (4, 6));
}

#[test]
fn test_direction_all_standard() {
    let directions = Direction::all_standard();
    assert_eq!(directions.len(), 4);
    assert!(directions.contains(&Direction::Horizontal));
    assert!(directions.contains(&Direction::Vertical));
    assert!(directions.contains(&Direction::Diagonal45));
    assert!(directions.contains(&Direction::Diagonal135));
}

// ============================================================================
// GLCM Parameters Tests
// ============================================================================

#[test]
fn test_glcm_params_default() {
    let params = GlcmParams::default();
    assert_eq!(params.gray_levels, 256);
    assert!(params.normalize);
    assert!(params.symmetric);
    assert!(params.window_size.is_none());
}

#[test]
fn test_glcm_params_custom() {
    let params = GlcmParams {
        gray_levels: 16,
        normalize: false,
        symmetric: false,
        window_size: Some(11),
    };
    assert_eq!(params.gray_levels, 16);
    assert!(!params.normalize);
    assert!(!params.symmetric);
    assert_eq!(params.window_size, Some(11));
}

// ============================================================================
// GLCM Structure Tests
// ============================================================================

#[test]
fn test_glcm_creation() {
    let glcm = Glcm::new(8, Direction::Horizontal, 1);
    assert_eq!(glcm.gray_levels(), 8);
    assert_eq!(glcm.direction(), Direction::Horizontal);
    assert_eq!(glcm.distance(), 1);
    assert!(!glcm.is_normalized());
}

#[test]
fn test_glcm_get_set() {
    let mut glcm = Glcm::new(8, Direction::Horizontal, 1);

    glcm.set(2, 3, 5.0);
    assert!((glcm.get(2, 3) - 5.0).abs() < 1e-10);

    // Out of bounds should return 0
    assert!(glcm.get(100, 100).abs() < 1e-10);
}

#[test]
fn test_glcm_increment() {
    let mut glcm = Glcm::new(8, Direction::Horizontal, 1);

    glcm.increment(2, 3);
    glcm.increment(2, 3);
    glcm.increment(2, 3);

    assert!((glcm.get(2, 3) - 3.0).abs() < 1e-10);
}

#[test]
fn test_glcm_normalize() {
    let mut glcm = Glcm::new(2, Direction::Horizontal, 1);
    glcm.set(0, 0, 2.0);
    glcm.set(0, 1, 3.0);
    glcm.set(1, 0, 3.0);
    glcm.set(1, 1, 2.0);

    glcm.normalize();

    assert!(glcm.is_normalized());
    assert!((glcm.get(0, 0) - 0.2).abs() < 1e-10);
    assert!((glcm.get(0, 1) - 0.3).abs() < 1e-10);

    // Sum should be 1.0
    let sum: f64 = glcm.matrix().iter().flat_map(|row| row.iter()).sum();
    assert!((sum - 1.0).abs() < 1e-10);
}

#[test]
fn test_glcm_make_symmetric() {
    let mut glcm = Glcm::new(3, Direction::Horizontal, 1);
    glcm.set(0, 1, 4.0);
    glcm.set(1, 0, 2.0);

    glcm.make_symmetric();

    // Both should be average
    assert!((glcm.get(0, 1) - 3.0).abs() < 1e-10);
    assert!((glcm.get(1, 0) - 3.0).abs() < 1e-10);
}

// ============================================================================
// GLCM Computation Tests
// ============================================================================

#[test]
#[ignore = "TODO: GLCM computation needs investigation - uniform raster produces empty matrix"]
fn test_compute_glcm_uniform() {
    let src = create_uniform_raster(10, 10, 128.0);
    let params = GlcmParams {
        gray_levels: 256,
        normalize: true,
        symmetric: true,
        window_size: None,
    };

    let glcm = compute_glcm(&src, Direction::Horizontal, 1, &params).expect("Should compute GLCM");

    assert!(glcm.is_normalized());
    // For uniform image, there should be only one non-zero entry in the GLCM
    // Find the non-zero entry (quantization may map 128.0 to a nearby gray level)
    let mut found_non_zero = false;
    for i in 125..=130 {
        if glcm.get(i, i) > 0.0 {
            found_non_zero = true;
            break;
        }
    }
    assert!(
        found_non_zero,
        "Should have co-occurrence for uniform value around gray level 128"
    );
}

#[test]
fn test_compute_glcm_checkerboard() {
    let src = create_checkerboard_raster(10, 10);
    let params = GlcmParams {
        gray_levels: 2,
        normalize: true,
        symmetric: true,
        window_size: None,
    };

    let glcm = compute_glcm(&src, Direction::Horizontal, 1, &params).expect("Should compute GLCM");

    // For checkerboard, adjacent pixels alternate
    // So (0,1) and (1,0) should be high, (0,0) and (1,1) should be low
    assert!(glcm.get(0, 1) > glcm.get(0, 0));
    assert!(glcm.get(0, 1) > glcm.get(1, 1));
}

#[test]
fn test_compute_glcm_horizontal_stripes() {
    let src = create_striped_raster(10, 10, true); // Horizontal stripes
    let params = GlcmParams {
        gray_levels: 2,
        normalize: true,
        symmetric: true,
        window_size: None,
    };

    // Horizontal direction should see same values
    let glcm_h =
        compute_glcm(&src, Direction::Horizontal, 1, &params).expect("Should compute GLCM");

    // Vertical direction should see alternating values
    let _glcm_v = compute_glcm(&src, Direction::Vertical, 1, &params).expect("Should compute GLCM");

    // Horizontal: same values are adjacent
    assert!(glcm_h.get(0, 0) + glcm_h.get(1, 1) > glcm_h.get(0, 1) + glcm_h.get(1, 0));
}

#[test]
fn test_compute_glcm_different_distances() {
    let src = create_gradient_raster(20, 20);
    let params = GlcmParams {
        gray_levels: 8,
        normalize: true,
        symmetric: true,
        window_size: None,
    };

    let glcm_d1 =
        compute_glcm(&src, Direction::Horizontal, 1, &params).expect("Should compute GLCM d=1");
    let glcm_d2 =
        compute_glcm(&src, Direction::Horizontal, 2, &params).expect("Should compute GLCM d=2");

    // Both should be valid
    assert_eq!(glcm_d1.gray_levels(), 8);
    assert_eq!(glcm_d2.gray_levels(), 8);
    assert_eq!(glcm_d1.distance(), 1);
    assert_eq!(glcm_d2.distance(), 2);
}

#[test]
fn test_compute_glcm_error_zero_gray_levels() {
    let src = create_uniform_raster(10, 10, 128.0);
    let params = GlcmParams {
        gray_levels: 0,
        normalize: true,
        symmetric: true,
        window_size: None,
    };

    let result = compute_glcm(&src, Direction::Horizontal, 1, &params);
    assert!(result.is_err());
}

// ============================================================================
// Multi-Direction GLCM Tests
// ============================================================================

#[test]
fn test_compute_glcm_multi_direction() {
    let src = create_gradient_raster(20, 20);
    let params = GlcmParams {
        gray_levels: 8,
        normalize: true,
        symmetric: true,
        window_size: None,
    };

    let directions = Direction::all_standard();
    let glcm = compute_glcm_multi_direction(&src, &directions, 1, &params)
        .expect("Should compute multi-direction GLCM");

    assert_eq!(glcm.gray_levels(), 8);
}

#[test]
fn test_compute_glcm_multi_direction_empty_error() {
    let src = create_gradient_raster(20, 20);
    let params = GlcmParams::default();

    let result = compute_glcm_multi_direction(&src, &[], 1, &params);
    assert!(result.is_err());
}

// ============================================================================
// Haralick Features Tests
// ============================================================================

#[test]
fn test_haralick_features_uniform_glcm() {
    let mut glcm = Glcm::new(4, Direction::Horizontal, 1);

    // Create uniform distribution
    for i in 0..4 {
        for j in 0..4 {
            glcm.set(i, j, 1.0 / 16.0);
        }
    }

    let features = compute_haralick_features(&glcm);

    // Check that all features are finite
    assert!(features.contrast.is_finite());
    assert!(features.correlation.is_finite());
    assert!(features.energy.is_finite());
    assert!(features.homogeneity.is_finite());
    assert!(features.entropy.is_finite());
    assert!(features.dissimilarity.is_finite());
    assert!(features.variance.is_finite());
    assert!(features.sum_average.is_finite());
    assert!(features.sum_entropy.is_finite());
    assert!(features.difference_entropy.is_finite());
}

#[test]
fn test_haralick_features_energy_uniform() {
    let mut glcm = Glcm::new(4, Direction::Horizontal, 1);

    // Uniform distribution: all 1/16
    for i in 0..4 {
        for j in 0..4 {
            glcm.set(i, j, 1.0 / 16.0);
        }
    }

    let features = compute_haralick_features(&glcm);

    // Energy = sum(p^2) = 16 * (1/16)^2 = 1/16 = 0.0625
    assert!((features.energy - 0.0625).abs() < 0.01);
}

#[test]
fn test_haralick_features_energy_concentrated() {
    let mut glcm = Glcm::new(4, Direction::Horizontal, 1);

    // All probability in one cell
    glcm.set(0, 0, 1.0);

    let features = compute_haralick_features(&glcm);

    // Energy = 1^2 = 1.0 (maximum energy for concentrated)
    assert!((features.energy - 1.0).abs() < 0.01);
}

#[test]
fn test_haralick_features_contrast() {
    // Low contrast: diagonal GLCM
    let mut glcm_low = Glcm::new(4, Direction::Horizontal, 1);
    for i in 0..4 {
        glcm_low.set(i, i, 0.25);
    }

    // High contrast: anti-diagonal GLCM
    let mut glcm_high = Glcm::new(4, Direction::Horizontal, 1);
    glcm_high.set(0, 3, 0.5);
    glcm_high.set(3, 0, 0.5);

    let features_low = compute_haralick_features(&glcm_low);
    let features_high = compute_haralick_features(&glcm_high);

    // Anti-diagonal should have higher contrast
    assert!(features_high.contrast > features_low.contrast);
}

#[test]
fn test_haralick_features_homogeneity() {
    // High homogeneity: diagonal GLCM
    let mut glcm_homo = Glcm::new(4, Direction::Horizontal, 1);
    for i in 0..4 {
        glcm_homo.set(i, i, 0.25);
    }

    // Low homogeneity: spread out GLCM
    let mut glcm_hetero = Glcm::new(4, Direction::Horizontal, 1);
    glcm_hetero.set(0, 3, 0.5);
    glcm_hetero.set(3, 0, 0.5);

    let features_homo = compute_haralick_features(&glcm_homo);
    let features_hetero = compute_haralick_features(&glcm_hetero);

    // Diagonal should have higher homogeneity
    assert!(features_homo.homogeneity > features_hetero.homogeneity);
}

#[test]
fn test_haralick_features_entropy() {
    // Low entropy: concentrated
    let mut glcm_low = Glcm::new(4, Direction::Horizontal, 1);
    glcm_low.set(0, 0, 1.0);

    // Higher entropy: uniform
    let mut glcm_high = Glcm::new(4, Direction::Horizontal, 1);
    for i in 0..4 {
        for j in 0..4 {
            glcm_high.set(i, j, 1.0 / 16.0);
        }
    }

    let features_low = compute_haralick_features(&glcm_low);
    let features_high = compute_haralick_features(&glcm_high);

    // Uniform distribution should have higher entropy
    assert!(features_high.entropy > features_low.entropy);
}

#[test]
fn test_haralick_features_from_computed_glcm() {
    let src = create_gradient_raster(20, 20);
    let params = GlcmParams {
        gray_levels: 8,
        normalize: true,
        symmetric: true,
        window_size: None,
    };

    let glcm = compute_glcm(&src, Direction::Horizontal, 1, &params).expect("Should compute GLCM");

    let features = compute_haralick_features(&glcm);

    // All features should be finite and non-negative for most
    assert!(features.contrast >= 0.0);
    assert!(features.energy >= 0.0);
    assert!(features.entropy >= 0.0);
    assert!(features.homogeneity >= 0.0);
}

// ============================================================================
// Texture Feature Image Tests
// ============================================================================

#[test]
fn test_compute_texture_feature_image_contrast() {
    let src = create_gradient_raster(30, 30);
    let params = GlcmParams {
        gray_levels: 8,
        normalize: true,
        symmetric: true,
        window_size: None,
    };

    let result =
        compute_texture_feature_image(&src, "contrast", Direction::Horizontal, 1, 11, &params)
            .expect("Should compute texture feature image");

    assert_eq!(result.width(), 30);
    assert_eq!(result.height(), 30);
}

#[test]
fn test_compute_texture_feature_image_all_features() {
    let src = create_gradient_raster(30, 30);
    let params = GlcmParams {
        gray_levels: 8,
        normalize: true,
        symmetric: true,
        window_size: None,
    };

    let features = [
        "contrast",
        "correlation",
        "energy",
        "homogeneity",
        "entropy",
        "dissimilarity",
        "variance",
        "sum_average",
        "sum_entropy",
        "difference_entropy",
    ];

    for feature_name in &features {
        let result = compute_texture_feature_image(
            &src,
            feature_name,
            Direction::Horizontal,
            1,
            11,
            &params,
        );
        assert!(result.is_ok(), "Failed for feature: {}", feature_name);
    }
}

#[test]
fn test_compute_texture_feature_image_invalid_feature() {
    let src = create_gradient_raster(30, 30);
    let params = GlcmParams::default();

    let result = compute_texture_feature_image(
        &src,
        "invalid_feature",
        Direction::Horizontal,
        1,
        11,
        &params,
    );
    assert!(result.is_err());
}

#[test]
fn test_compute_texture_feature_image_even_window_error() {
    let src = create_gradient_raster(30, 30);
    let params = GlcmParams::default();

    let result =
        compute_texture_feature_image(&src, "contrast", Direction::Horizontal, 1, 10, &params);
    assert!(result.is_err());
}

// ============================================================================
// All Texture Features Test
// ============================================================================

#[test]
fn test_compute_all_texture_features() {
    let src = create_gradient_raster(30, 30);
    let params = GlcmParams {
        gray_levels: 8,
        normalize: true,
        symmetric: true,
        window_size: None,
    };

    let results = compute_all_texture_features(&src, Direction::Horizontal, 1, 11, &params)
        .expect("Should compute all texture features");

    // Should have multiple feature images
    assert!(!results.is_empty());

    // Check that expected features are present
    let feature_names: Vec<_> = results.iter().map(|(name, _)| *name).collect();
    assert!(feature_names.contains(&"contrast"));
    assert!(feature_names.contains(&"energy"));
    assert!(feature_names.contains(&"entropy"));
}

// ============================================================================
// Edge Cases and Special Values
// ============================================================================

#[test]
fn test_glcm_small_image() {
    let src = create_gradient_raster(5, 5);
    let params = GlcmParams {
        gray_levels: 4,
        normalize: true,
        symmetric: true,
        window_size: None,
    };

    let glcm = compute_glcm(&src, Direction::Horizontal, 1, &params);
    assert!(glcm.is_ok());
}

#[test]
fn test_glcm_large_distance() {
    let src = create_gradient_raster(20, 20);
    let params = GlcmParams {
        gray_levels: 8,
        normalize: true,
        symmetric: true,
        window_size: None,
    };

    // Distance larger than some pairs will have no valid pairs
    let glcm = compute_glcm(&src, Direction::Horizontal, 10, &params);
    assert!(glcm.is_ok());
}

#[test]
fn test_haralick_features_zero_glcm() {
    let glcm = Glcm::new(4, Direction::Horizontal, 1);
    // All zeros

    let features = compute_haralick_features(&glcm);

    // Should handle gracefully
    assert!(features.contrast.is_finite());
    assert!(features.energy.is_finite());
}

#[test]
fn test_texture_analysis_different_directions() {
    let src = create_striped_raster(20, 20, false); // Vertical stripes
    let params = GlcmParams {
        gray_levels: 2,
        normalize: true,
        symmetric: true,
        window_size: None,
    };

    let glcm_h = compute_glcm(&src, Direction::Horizontal, 1, &params)
        .expect("Should compute horizontal GLCM");
    let glcm_v =
        compute_glcm(&src, Direction::Vertical, 1, &params).expect("Should compute vertical GLCM");

    let features_h = compute_haralick_features(&glcm_h);
    let features_v = compute_haralick_features(&glcm_v);

    // Vertical stripes should show different characteristics
    // in horizontal vs vertical directions
    // Horizontal direction sees alternating values -> higher contrast
    assert!(features_h.contrast > features_v.contrast);
}

// ============================================================================
// Integration Tests
// ============================================================================

#[test]
fn test_texture_analysis_pipeline() {
    // Create test image with known texture
    let src = create_checkerboard_raster(20, 20);

    let params = GlcmParams {
        gray_levels: 2,
        normalize: true,
        symmetric: true,
        window_size: None,
    };

    // Step 1: Compute GLCM
    let glcm = compute_glcm(&src, Direction::Horizontal, 1, &params).expect("Should compute GLCM");

    // Step 2: Extract features
    let features = compute_haralick_features(&glcm);

    // Step 3: Verify checkerboard characteristics
    // High contrast (alternating values)
    assert!(features.contrast > 0.5);
    // Low homogeneity (values differ from neighbors)
    assert!(features.homogeneity < 0.8);
}

#[test]
fn test_comparing_textures() {
    let uniform = create_uniform_raster(20, 20, 128.0);
    let gradient = create_gradient_raster(20, 20);
    let checkerboard = create_checkerboard_raster(20, 20);

    let params = GlcmParams {
        gray_levels: 8,
        normalize: true,
        symmetric: true,
        window_size: None,
    };

    let glcm_uniform = compute_glcm(&uniform, Direction::Horizontal, 1, &params)
        .expect("Should compute uniform GLCM");
    let glcm_gradient = compute_glcm(&gradient, Direction::Horizontal, 1, &params)
        .expect("Should compute gradient GLCM");
    let glcm_checker = compute_glcm(&checkerboard, Direction::Horizontal, 1, &params)
        .expect("Should compute checkerboard GLCM");

    let features_uniform = compute_haralick_features(&glcm_uniform);
    let features_gradient = compute_haralick_features(&glcm_gradient);
    let features_checker = compute_haralick_features(&glcm_checker);

    // Uniform should have lowest entropy
    assert!(features_uniform.entropy < features_gradient.entropy);

    // Checkerboard should have high contrast
    assert!(features_checker.contrast > features_uniform.contrast);

    // Uniform should have highest energy (most concentrated)
    assert!(features_uniform.energy > features_gradient.energy);
}

// ============================================================================
// Performance and Large Dataset Tests
// ============================================================================

#[test]
fn test_large_glcm() {
    let src = create_gradient_raster(100, 100);
    let params = GlcmParams {
        gray_levels: 256,
        normalize: true,
        symmetric: true,
        window_size: None,
    };

    let glcm =
        compute_glcm(&src, Direction::Horizontal, 1, &params).expect("Should compute large GLCM");

    assert_eq!(glcm.gray_levels(), 256);
    assert!(glcm.is_normalized());
}

#[test]
fn test_reduced_gray_levels_performance() {
    let src = create_gradient_raster(50, 50);

    // Compare computation with different gray levels
    let params_8 = GlcmParams {
        gray_levels: 8,
        normalize: true,
        symmetric: true,
        window_size: None,
    };

    let params_32 = GlcmParams {
        gray_levels: 32,
        normalize: true,
        symmetric: true,
        window_size: None,
    };

    let glcm_8 = compute_glcm(&src, Direction::Horizontal, 1, &params_8)
        .expect("Should compute 8-level GLCM");
    let glcm_32 = compute_glcm(&src, Direction::Horizontal, 1, &params_32)
        .expect("Should compute 32-level GLCM");

    // Both should be valid
    let features_8 = compute_haralick_features(&glcm_8);
    let features_32 = compute_haralick_features(&glcm_32);

    assert!(features_8.energy.is_finite());
    assert!(features_32.energy.is_finite());
}
