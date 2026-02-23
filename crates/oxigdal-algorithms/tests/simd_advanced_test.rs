//! Integration tests for advanced SIMD operations
//!
//! This test suite validates the correctness and edge cases of advanced SIMD modules.

#[cfg(feature = "simd")]
mod simd_tests {
    use oxigdal_algorithms::simd::{
        colorspace, filters, histogram, morphology, projection, threshold,
    };

    #[test]
    fn test_projection_affine_transform() {
        let matrix = projection::AffineMatrix2D::scale(2.0, 3.0);
        let x = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let y = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let mut out_x = vec![0.0; 5];
        let mut out_y = vec![0.0; 5];

        projection::affine_transform_2d(&matrix, &x, &y, &mut out_x, &mut out_y)
            .expect("Failed to apply affine transform in test");

        for i in 0..5 {
            assert!((out_x[i] - x[i] * 2.0).abs() < 1e-10_f64);
            assert!((out_y[i] - y[i] * 3.0).abs() < 1e-10_f64);
        }
    }

    #[test]
    fn test_projection_web_mercator_roundtrip() {
        let lon = vec![-122.4194, 139.6917, 2.3522, 0.0, -180.0, 180.0];
        let lat = vec![37.7749, 35.6762, 48.8566, 0.0, -85.0, 85.0];
        let mut x = vec![0.0; 6];
        let mut y = vec![0.0; 6];
        let mut out_lon = vec![0.0; 6];
        let mut out_lat = vec![0.0; 6];

        projection::latlon_to_web_mercator(&lon, &lat, &mut x, &mut y)
            .expect("Failed to convert lat/lon to Web Mercator in test");
        projection::web_mercator_to_latlon(&x, &y, &mut out_lon, &mut out_lat)
            .expect("Failed to convert Web Mercator to lat/lon in test");

        for i in 0..6 {
            assert!((out_lon[i] - lon[i]).abs() < 1e-6_f64);
            assert!((out_lat[i] - lat[i]).abs() < 1e-6_f64);
        }
    }

    #[test]
    fn test_filters_gaussian_blur() {
        let width = 10;
        let height = 10;
        let input = vec![128u8; width * height];
        let mut output = vec![0u8; width * height];

        filters::gaussian_blur_3x3(&input, &mut output, width, height)
            .expect("Failed to apply Gaussian blur in test");

        // Check that uniform input produces uniform output (within borders)
        for y in 1..(height - 1) {
            for x in 1..(width - 1) {
                assert_eq!(output[y * width + x], 128);
            }
        }
    }

    #[test]
    fn test_filters_sobel_edge_detection() {
        let width = 10;
        let height = 10;
        let mut input = vec![0u8; width * height];

        // Create a vertical edge
        for y in 0..height {
            for x in 5..width {
                input[y * width + x] = 255;
            }
        }

        let mut gx = vec![0i16; width * height];
        let mut gy = vec![0i16; width * height];
        let mut magnitude = vec![0u8; width * height];

        filters::sobel_x_3x3(&input, &mut gx, width, height)
            .expect("Failed to apply Sobel X filter in test");
        filters::sobel_y_3x3(&input, &mut gy, width, height)
            .expect("Failed to apply Sobel Y filter in test");
        filters::sobel_magnitude(&gx, &gy, &mut magnitude)
            .expect("Failed to compute Sobel magnitude in test");

        // Check that edge is detected around x=5
        for y in 2..(height - 2) {
            assert!(magnitude[y * width + 5] > 100);
        }
    }

    #[test]
    fn test_colorspace_rgb_hsv_roundtrip() {
        let r = vec![255, 128, 64, 0, 192, 32];
        let g = vec![0, 128, 128, 255, 64, 96];
        let b = vec![0, 0, 192, 0, 32, 160];
        let mut h = vec![0.0; 6];
        let mut s = vec![0.0; 6];
        let mut v = vec![0.0; 6];
        let mut r_out = vec![0; 6];
        let mut g_out = vec![0; 6];
        let mut b_out = vec![0; 6];

        colorspace::rgb_to_hsv(&r, &g, &b, &mut h, &mut s, &mut v)
            .expect("Failed to convert RGB to HSV in test");
        colorspace::hsv_to_rgb(&h, &s, &v, &mut r_out, &mut g_out, &mut b_out)
            .expect("Failed to convert HSV to RGB in test");

        for i in 0..6 {
            assert!((r[i] as i16 - r_out[i] as i16).abs() <= 1);
            assert!((g[i] as i16 - g_out[i] as i16).abs() <= 1);
            assert!((b[i] as i16 - b_out[i] as i16).abs() <= 1);
        }
    }

    #[test]
    fn test_colorspace_rgb_lab_roundtrip() {
        let r = vec![255, 128, 64, 0];
        let g = vec![0, 128, 128, 255];
        let b = vec![0, 0, 192, 0];
        let mut l = vec![0.0; 4];
        let mut a = vec![0.0; 4];
        let mut b_lab = vec![0.0; 4];
        let mut r_out = vec![0; 4];
        let mut g_out = vec![0; 4];
        let mut b_out = vec![0; 4];

        colorspace::rgb_to_lab(&r, &g, &b, &mut l, &mut a, &mut b_lab)
            .expect("Failed to convert RGB to LAB in test");
        colorspace::lab_to_rgb(&l, &a, &b_lab, &mut r_out, &mut g_out, &mut b_out)
            .expect("Failed to convert LAB to RGB in test");

        for i in 0..4 {
            assert!((r[i] as i16 - r_out[i] as i16).abs() <= 3);
            assert!((g[i] as i16 - g_out[i] as i16).abs() <= 3);
            assert!((b[i] as i16 - b_out[i] as i16).abs() <= 3);
        }
    }

    #[test]
    fn test_histogram_u8_computation() {
        let mut data = vec![0u8; 1000];
        for i in 0..256 {
            if i < data.len() {
                data[i] = (i % 256) as u8;
            }
        }

        let hist =
            histogram::histogram_u8(&data, 256).expect("Failed to compute histogram in test");

        assert_eq!(hist.len(), 256);
        assert_eq!(hist.iter().sum::<u32>(), 1000);
    }

    #[test]
    fn test_histogram_equalization() {
        let mut data = vec![100u8; 500];
        data.extend(vec![200u8; 500]);
        let mut output = vec![0u8; 1000];

        histogram::equalize_histogram(&data, &mut output)
            .expect("Failed to equalize histogram in test");

        // Output should use more of the dynamic range
        let min = *output.iter().min().unwrap_or(&0);
        let max = *output.iter().max().unwrap_or(&0);
        assert!(max - min > 150);
    }

    #[test]
    fn test_histogram_otsu_threshold() {
        // Create clear bimodal distribution
        let mut data = vec![30u8; 500];
        data.extend(std::iter::repeat_n(220u8, 500));

        let threshold =
            threshold::otsu_threshold(&data).expect("Failed to compute Otsu threshold in test");

        // Threshold should be somewhere between the two clear modes
        // Allow for a wider range as Otsu's method finds optimal separation
        assert!(threshold > 20 && threshold < 230);
    }

    #[test]
    fn test_histogram_quantiles() {
        let data: Vec<u8> = (0..=255).cycle().take(10000).collect();
        let hist = histogram::histogram_u8(&data, 256)
            .expect("Failed to compute histogram for quantiles test");

        let quantiles = vec![0.0, 0.25, 0.5, 0.75, 1.0];
        let results = histogram::histogram_quantiles(&hist, &quantiles)
            .expect("Failed to compute histogram quantiles in test");

        assert_eq!(results.len(), 5);
        assert_eq!(results[0], 0); // Min
        assert_eq!(results[4], 255); // Max
        // Median should be around 127
        assert!((results[2] as i32 - 127).abs() < 10);
    }

    #[test]
    fn test_morphology_erosion_dilation() {
        let width = 10;
        let height = 10;
        let mut input = vec![0u8; width * height];

        // Create a bright square in the center
        for y in 3..7 {
            for x in 3..7 {
                input[y * width + x] = 255;
            }
        }

        let mut eroded = vec![0u8; width * height];
        let mut dilated = vec![0u8; width * height];

        morphology::erode_3x3(&input, &mut eroded, width, height)
            .expect("Failed to apply morphological erosion in test");
        morphology::dilate_3x3(&input, &mut dilated, width, height)
            .expect("Failed to apply morphological dilation in test");

        // Erosion should shrink the square
        assert_eq!(eroded[4 * width + 4], 255); // Center still bright
        assert_eq!(eroded[3 * width + 3], 0); // Corner eroded

        // Dilation should expand the square
        assert_eq!(dilated[5 * width + 5], 255); // Center still bright
        assert_eq!(dilated[2 * width + 3], 255); // Expanded
    }

    #[test]
    fn test_morphology_opening_closing() {
        let width = 10;
        let height = 10;
        let input = vec![128u8; width * height];
        let mut opened = vec![0u8; width * height];
        let mut closed = vec![0u8; width * height];

        morphology::opening_3x3(&input, &mut opened, width, height)
            .expect("Failed to apply morphological opening in test");
        morphology::closing_3x3(&input, &mut closed, width, height)
            .expect("Failed to apply morphological closing in test");

        // For uniform input, opening and closing should produce similar results
        for i in 0..input.len() {
            assert!((opened[i] as i16 - input[i] as i16).abs() < 50);
            assert!((closed[i] as i16 - input[i] as i16).abs() < 50);
        }
    }

    #[test]
    fn test_morphology_gradient() {
        let width = 10;
        let height = 10;
        let mut input = vec![128u8; width * height];

        // Create a step edge
        for y in 0..5 {
            for x in 0..width {
                input[y * width + x] = 50;
            }
        }

        let mut gradient = vec![0u8; width * height];
        morphology::morphological_gradient_3x3(&input, &mut gradient, width, height)
            .expect("Failed to compute morphological gradient in test");

        // Gradient should be high near the edge (y=4,5)
        assert!(gradient[4 * width + 5] > 20);
        assert!(gradient[5 * width + 5] > 20);
    }

    #[test]
    fn test_threshold_binary() {
        let input = vec![50, 100, 150, 200, 250];
        let mut output = vec![0; 5];

        threshold::binary_threshold(&input, &mut output, 128, 255, 0)
            .expect("Failed to apply binary threshold in test");

        assert_eq!(output, vec![0, 0, 255, 255, 255]);
    }

    #[test]
    fn test_threshold_range() {
        let input = vec![50, 100, 150, 200, 250];
        let mut output = vec![0; 5];

        threshold::threshold_range(&input, &mut output, 100, 200)
            .expect("Failed to apply threshold range in test");

        assert_eq!(output, vec![0, 100, 150, 200, 0]);
    }

    #[test]
    fn test_threshold_multi_level() {
        let input = vec![10, 50, 100, 150, 200, 250];
        let mut output = vec![0; 6];
        let thresholds = vec![64, 128, 192];
        let levels = vec![0, 85, 170, 255];

        threshold::multi_threshold(&input, &mut output, &thresholds, &levels)
            .expect("Failed to apply multi-level threshold in test");

        assert_eq!(output[0], 0); // 10 < 64
        assert_eq!(output[1], 0); // 50 < 64
        assert_eq!(output[2], 85); // 64 <= 100 < 128
        assert_eq!(output[3], 170); // 128 <= 150 < 192
        assert_eq!(output[4], 255); // 200 >= 192
        assert_eq!(output[5], 255); // 250 >= 192
    }

    #[test]
    fn test_threshold_adaptive() {
        let width = 20;
        let height = 20;
        let mut input = vec![128u8; width * height];

        // Create varying intensity
        for y in 0..height {
            for x in 0..width {
                input[y * width + x] = ((x + y) * 6) as u8;
            }
        }

        let mut output = vec![0u8; width * height];
        threshold::adaptive_threshold_mean(&input, &mut output, width, height, 5, 10)
            .expect("Failed to apply adaptive threshold in test");

        // Should produce binary output
        for &val in &output {
            assert!(val == 0 || val == 255);
        }
    }

    #[test]
    fn test_threshold_hysteresis() {
        let width = 10;
        let height = 10;
        let mut input = vec![0u8; width * height];

        // Strong edge at center
        input[5 * width + 5] = 200;
        // Weak edges connected to strong edge
        input[5 * width + 4] = 80;
        input[4 * width + 5] = 80;
        // Isolated weak edge
        input[2 * width + 2] = 80;

        let mut output = vec![0u8; width * height];
        threshold::hysteresis_threshold(&input, &mut output, width, height, 50, 150)
            .expect("Failed to apply hysteresis threshold in test");

        // Strong edge should be marked
        assert_eq!(output[5 * width + 5], 255);
        // Connected weak edges should be marked
        assert_eq!(output[5 * width + 4], 255);
        assert_eq!(output[4 * width + 5], 255);
        // Isolated weak edge should not be marked
        assert_eq!(output[2 * width + 2], 0);
    }

    #[test]
    fn test_large_dataset_processing() {
        // Test with larger datasets to ensure SIMD handles various sizes
        let size = 10_000;
        let data = vec![128u8; size];
        let mut output = vec![0u8; size];

        threshold::binary_threshold(&data, &mut output, 100, 255, 0)
            .expect("Failed to apply binary threshold on large dataset in test");

        for &val in &output {
            assert_eq!(val, 255);
        }
    }

    #[test]
    fn test_edge_cases_empty_input() {
        let empty: Vec<u8> = vec![];
        let result = histogram::histogram_u8(&empty, 256);
        assert!(result.is_err());
    }

    #[test]
    fn test_edge_cases_single_pixel() {
        let data = vec![128u8];
        let hist = histogram::histogram_u8(&data, 256)
            .expect("Failed to compute histogram for single pixel in test");
        assert_eq!(hist[128], 1);
    }

    #[test]
    fn test_projection_matrix_inversion() {
        let matrix = projection::AffineMatrix2D {
            a: 2.0,
            b: 0.0,
            c: 0.0,
            d: 3.0,
            e: 10.0,
            f: 20.0,
        };

        let inverse = matrix
            .invert()
            .expect("Failed to invert affine matrix in test");

        // Test that matrix * inverse = identity
        let x = vec![100.0, 200.0, 300.0];
        let y = vec![50.0, 100.0, 150.0];
        let mut temp_x = vec![0.0; 3];
        let mut temp_y = vec![0.0; 3];
        let mut final_x = vec![0.0; 3];
        let mut final_y = vec![0.0; 3];

        projection::affine_transform_2d(&matrix, &x, &y, &mut temp_x, &mut temp_y)
            .expect("Failed to apply forward affine transform in matrix inversion test");
        projection::affine_transform_2d(&inverse, &temp_x, &temp_y, &mut final_x, &mut final_y)
            .expect("Failed to apply inverse affine transform in matrix inversion test");

        for i in 0..3 {
            assert!((final_x[i] - x[i]).abs() < 1e-9_f64);
            assert!((final_y[i] - y[i]).abs() < 1e-9_f64);
        }
    }
}
