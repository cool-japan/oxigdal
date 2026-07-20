//! Integration tests for Geographically Weighted Regression (GWR).

use oxigeo_analytics::{GwrBandwidth, GwrKernel, GwrOptions, gwr_fit};

/// Build a regular grid of `(x, y)` coordinates with `side * side` points.
fn grid_coords(side: usize, spacing: f64) -> Vec<(f64, f64)> {
    let mut coords = Vec::with_capacity(side * side);
    for r in 0..side {
        for c in 0..side {
            coords.push((c as f64 * spacing, r as f64 * spacing));
        }
    }
    coords
}

#[test]
fn test_gwr_constant_data_returns_global_intercept() {
    // Constant response with a single (varying) predictor: the intercept
    // should equal the constant and the slope should be ~0 everywhere.
    let coords = grid_coords(5, 1.0);
    let n = coords.len();
    let x: Vec<Vec<f64>> = (0..n).map(|i| vec![i as f64]).collect();
    let y = vec![7.0; n];

    let options = GwrOptions {
        kernel: GwrKernel::Gaussian,
        bandwidth: GwrBandwidth::Fixed(100.0),
        optimize_bandwidth: false,
    };
    let result = gwr_fit(&coords, &x, &y, &options).expect("GWR should fit constant data");

    assert_eq!(result.coefficients.len(), n);
    for beta in &result.coefficients {
        assert_eq!(beta.len(), 2);
        assert!((beta[0] - 7.0).abs() < 1e-6, "intercept should be 7.0");
        assert!(beta[1].abs() < 1e-6, "slope should be ~0 for constant y");
    }
    for &p in &result.predicted {
        assert!((p - 7.0).abs() < 1e-6);
    }
}

#[test]
fn test_gwr_recovers_global_ols_with_huge_bandwidth() {
    // With an enormous bandwidth every observation is weighted near-equally,
    // so the local fits collapse to the global OLS solution. Generate data
    // from y = 2 + 3*x1 - 1*x2 exactly.
    let coords = grid_coords(6, 1.0);
    let n = coords.len();
    let x: Vec<Vec<f64>> = (0..n).map(|i| vec![i as f64, (i % 5) as f64]).collect();
    let y: Vec<f64> = x
        .iter()
        .map(|row| 2.0 + 3.0 * row[0] - 1.0 * row[1])
        .collect();

    let options = GwrOptions {
        kernel: GwrKernel::Gaussian,
        bandwidth: GwrBandwidth::Fixed(1.0e6),
        optimize_bandwidth: false,
    };
    let result = gwr_fit(&coords, &x, &y, &options).expect("GWR should fit");

    for beta in &result.coefficients {
        assert!(
            (beta[0] - 2.0).abs() < 1e-4,
            "intercept ~2, got {}",
            beta[0]
        );
        assert!((beta[1] - 3.0).abs() < 1e-4, "x1 coef ~3, got {}", beta[1]);
        assert!((beta[2] + 1.0).abs() < 1e-4, "x2 coef ~-1, got {}", beta[2]);
    }
}

#[test]
fn test_gwr_bisquare_kernel_zero_beyond_bandwidth() {
    // Bisquare kernel has compact support; weight is exactly zero at and beyond
    // the bandwidth and positive inside it.
    assert_eq!(GwrKernel::Bisquare.weight(2.0, 1.0), 0.0);
    assert_eq!(GwrKernel::Bisquare.weight(1.0, 1.0), 0.0);
    let inside = GwrKernel::Bisquare.weight(0.5, 1.0);
    assert!(inside > 0.0 && inside < 1.0);
    // At distance 0 the weight is maximal (1.0).
    assert!((GwrKernel::Bisquare.weight(0.0, 1.0) - 1.0).abs() < 1e-12);
}

#[test]
fn test_gwr_gaussian_weights_decrease_with_distance() {
    // Gaussian kernel weights are strictly decreasing with distance and never
    // reach zero.
    let b = 2.0;
    let w0 = GwrKernel::Gaussian.weight(0.0, b);
    let w1 = GwrKernel::Gaussian.weight(1.0, b);
    let w2 = GwrKernel::Gaussian.weight(3.0, b);
    assert!((w0 - 1.0).abs() < 1e-12);
    assert!(w0 > w1 && w1 > w2);
    assert!(w2 > 0.0, "Gaussian weight is always positive");

    // The exponential kernel shares the monotone-decay property.
    let e0 = GwrKernel::Exponential.weight(0.0, b);
    let e1 = GwrKernel::Exponential.weight(1.0, b);
    assert!((e0 - 1.0).abs() < 1e-12);
    assert!(e0 > e1 && e1 > 0.0);
}

#[test]
fn test_gwr_local_coefficients_vary_with_spatial_trend() {
    // Construct a spatially varying relationship: the slope on x1 increases
    // with the x-coordinate. GWR should recover locally different slopes.
    let side = 9;
    let coords = grid_coords(side, 1.0);
    let n = coords.len();
    // Predictor independent of position so the slope is identifiable.
    let x: Vec<Vec<f64>> = (0..n).map(|i| vec![((i * 7) % 11) as f64]).collect();
    // True local slope grows with x-coordinate.
    let y: Vec<f64> = coords
        .iter()
        .zip(x.iter())
        .map(|(&(cx, _), row)| 1.0 + (0.5 + cx) * row[0])
        .collect();

    let options = GwrOptions {
        kernel: GwrKernel::Bisquare,
        bandwidth: GwrBandwidth::AdaptiveKnn(12),
        optimize_bandwidth: false,
    };
    let result = gwr_fit(&coords, &x, &y, &options).expect("GWR should fit varying trend");

    // Slope (coefficient index 1) at low-x locations should be smaller than at
    // high-x locations.
    let mut low_x_slope = 0.0;
    let mut high_x_slope = 0.0;
    let mut low_count = 0.0;
    let mut high_count = 0.0;
    for (idx, &(cx, _)) in coords.iter().enumerate() {
        let slope = result.coefficients[idx][1];
        if cx <= 1.0 {
            low_x_slope += slope;
            low_count += 1.0;
        } else if cx >= (side as f64 - 2.0) {
            high_x_slope += slope;
            high_count += 1.0;
        }
    }
    low_x_slope /= low_count;
    high_x_slope /= high_count;
    assert!(
        high_x_slope > low_x_slope + 1.0,
        "high-x slope ({high_x_slope}) should exceed low-x slope ({low_x_slope})"
    );
}

#[test]
fn test_gwr_adaptive_knn_bandwidth() {
    // The adaptive-knn bandwidth should fit successfully and report the
    // neighbour count as the reported bandwidth.
    let coords = grid_coords(6, 1.0);
    let n = coords.len();
    let x: Vec<Vec<f64>> = (0..n).map(|i| vec![(i % 4) as f64]).collect();
    let y: Vec<f64> = x.iter().map(|row| 5.0 + 2.0 * row[0]).collect();

    let k = 10;
    let options = GwrOptions {
        kernel: GwrKernel::Bisquare,
        bandwidth: GwrBandwidth::AdaptiveKnn(k),
        optimize_bandwidth: false,
    };
    let result = gwr_fit(&coords, &x, &y, &options).expect("adaptive knn GWR should fit");
    assert_eq!(result.coefficients.len(), n);
    assert!((result.bandwidth - k as f64).abs() < 1e-9);
    // The exact linear relationship is recovered locally.
    for beta in &result.coefficients {
        assert!((beta[0] - 5.0).abs() < 1e-3);
        assert!((beta[1] - 2.0).abs() < 1e-3);
    }
}

#[test]
fn test_gwr_aicc_bandwidth_optimization_selects_reasonable() {
    // With optimisation enabled, AICc-driven golden-section search should pick
    // a finite bandwidth and produce a finite AICc.
    let side = 8;
    let coords = grid_coords(side, 1.0);
    let n = coords.len();
    let x: Vec<Vec<f64>> = (0..n).map(|i| vec![((i * 3) % 7) as f64]).collect();
    let y: Vec<f64> = coords
        .iter()
        .zip(x.iter())
        .map(|(&(cx, cy), row)| 1.0 + 0.3 * cx + 0.2 * cy + 1.5 * row[0])
        .collect();

    let options = GwrOptions {
        kernel: GwrKernel::Gaussian,
        bandwidth: GwrBandwidth::Fixed(0.0), // placeholder; optimisation overrides
        optimize_bandwidth: true,
    };
    let result = gwr_fit(&coords, &x, &y, &options).expect("optimised GWR should fit");
    assert!(result.bandwidth.is_finite() && result.bandwidth > 0.0);
    assert!(result.aicc.is_finite());

    // Adaptive optimisation should pick a neighbour count within bounds.
    let options_knn = GwrOptions {
        kernel: GwrKernel::Bisquare,
        bandwidth: GwrBandwidth::AdaptiveKnn(0),
        optimize_bandwidth: true,
    };
    let result_knn = gwr_fit(&coords, &x, &y, &options_knn).expect("optimised adaptive GWR fits");
    let k = result_knn.bandwidth;
    assert!(
        k >= 2.0 && k <= (n - 1) as f64,
        "k={k} should be in [2, n-1]"
    );
}

#[test]
fn test_gwr_rank_deficient_returns_error() {
    // Two perfectly collinear predictors make every local design matrix
    // rank-deficient, so the solver must return an error rather than panic.
    let coords = grid_coords(5, 1.0);
    let n = coords.len();
    // x2 = 2 * x1 exactly -> collinear with each other (and producing a
    // singular Xᵀ W X together with the intercept).
    let x: Vec<Vec<f64>> = (0..n)
        .map(|i| {
            let v = i as f64;
            vec![v, 2.0 * v]
        })
        .collect();
    let y: Vec<f64> = (0..n).map(|i| i as f64).collect();

    let options = GwrOptions {
        kernel: GwrKernel::Gaussian,
        bandwidth: GwrBandwidth::Fixed(100.0),
        optimize_bandwidth: false,
    };
    let result = gwr_fit(&coords, &x, &y, &options);
    assert!(result.is_err(), "rank-deficient system should error");
}

#[test]
fn test_gwr_residuals_plus_predicted_equals_y() {
    // By definition residual_i = y_i - predicted_i, so reconstructing y from
    // predicted + residual must be exact.
    let coords = grid_coords(6, 1.0);
    let n = coords.len();
    let x: Vec<Vec<f64>> = (0..n)
        .map(|i| vec![(i % 5) as f64, (i % 3) as f64])
        .collect();
    let y: Vec<f64> = coords
        .iter()
        .map(|&(cx, cy)| 0.5 + 1.1 * cx - 0.7 * cy)
        .collect();

    let options = GwrOptions {
        kernel: GwrKernel::Bisquare,
        bandwidth: GwrBandwidth::AdaptiveKnn(15),
        optimize_bandwidth: false,
    };
    let result = gwr_fit(&coords, &x, &y, &options).expect("GWR should fit");
    assert_eq!(result.predicted.len(), n);
    assert_eq!(result.residuals.len(), n);
    for (i, &yi) in y.iter().enumerate() {
        let reconstructed = result.predicted[i] + result.residuals[i];
        assert!(
            (reconstructed - yi).abs() < 1e-9,
            "predicted + residual must equal y at {i}"
        );
    }
    // Local R² values are within [0, 1].
    for &r2 in &result.local_r2 {
        assert!((0.0..=1.0).contains(&r2), "local R² out of range: {r2}");
    }
}

#[test]
fn test_gwr_single_predictor_slope_recovery() {
    // Simple bivariate relationship y = 4 + 2.5*x. With a large bandwidth the
    // recovered slope and intercept match the generating values closely.
    let coords = grid_coords(5, 2.0);
    let n = coords.len();
    let x: Vec<Vec<f64>> = (0..n).map(|i| vec![(i as f64) * 0.5]).collect();
    let y: Vec<f64> = x.iter().map(|row| 4.0 + 2.5 * row[0]).collect();

    let options = GwrOptions {
        kernel: GwrKernel::Gaussian,
        bandwidth: GwrBandwidth::Fixed(1.0e5),
        optimize_bandwidth: false,
    };
    let result = gwr_fit(&coords, &x, &y, &options).expect("single-predictor GWR should fit");
    for beta in &result.coefficients {
        assert!(
            (beta[0] - 4.0).abs() < 1e-3,
            "intercept ~4, got {}",
            beta[0]
        );
        assert!((beta[1] - 2.5).abs() < 1e-3, "slope ~2.5, got {}", beta[1]);
    }
}
