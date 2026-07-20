//! Integration tests for Universal Kriging with external drift (UKED).
//!
//! Covers the additions made for Slice 26 W2 — Wackernagel 2003 §16 and
//! Cressie 1993 §3.4.2. See `crates/oxigeo-analytics/src/interpolation/kriging.rs`.

use oxigeo_analytics::interpolation::{
    DriftBasis, KrigingInterpolator, KrigingType, UniversalKrigingOptions, Variogram,
    VariogramModel, universal_kriging_fit,
};
use scirs2_core::ndarray::{Array2, array};

/// Build a deterministic 5-point sample layout used by several tests.
fn sample_coords_values() -> (Array2<f64>, scirs2_core::ndarray::Array1<f64>) {
    let coords = array![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0], [0.5, 0.5],];
    let values = array![1.0, 2.0, 3.0, 4.0, 2.5];
    (coords, values)
}

#[test]
fn test_universal_kriging_constant_drift_equals_ordinary() {
    // Ordinary kriging is a degenerate case of UKED with a single constant
    // drift basis; predictions should be numerically identical (modulo the
    // tiny Tikhonov diagonal shared by both code paths).
    let (coords, values) = sample_coords_values();
    let targets = array![[0.25, 0.25], [0.75, 0.75]];

    let variogram = Variogram::new(VariogramModel::Spherical, 0.0, 1.0, 2.0);
    let interp = KrigingInterpolator::new(KrigingType::Ordinary, variogram);
    let ord = interp
        .interpolate(&coords, &values.view(), &targets)
        .expect("ordinary kriging should succeed for non-collinear inputs");

    let options = UniversalKrigingOptions {
        drift_bases: vec![DriftBasis::Constant],
        variogram,
        regularization: 0.0,
    };
    let uked = universal_kriging_fit(coords.view(), values.view(), &options, targets.view(), None)
        .expect("UKED with constant drift should succeed");

    assert_eq!(uked.predicted.len(), 2);
    for i in 0..2 {
        assert!(
            (uked.predicted[i] - ord.values[i]).abs() < 1e-8,
            "constant UKED diverged from ordinary kriging at index {}: uked={}, ord={}",
            i,
            uked.predicted[i],
            ord.values[i],
        );
    }
}

#[test]
fn test_universal_kriging_linear_drift_recovers_trend() {
    // Pure linear trend z(x, y) = 2x + 3y. With linear drift basis the
    // UKED predictor should reproduce the trend almost exactly.
    let coords = array![
        [0.0, 0.0],
        [1.0, 0.0],
        [2.0, 0.0],
        [0.0, 1.0],
        [1.0, 1.0],
        [2.0, 1.0],
        [0.0, 2.0],
        [1.0, 2.0],
        [2.0, 2.0],
    ];
    let mut vals_vec = Vec::with_capacity(coords.nrows());
    for i in 0..coords.nrows() {
        vals_vec.push(2.0 * coords[[i, 0]] + 3.0 * coords[[i, 1]]);
    }
    let values = scirs2_core::ndarray::Array1::from_vec(vals_vec);
    let targets = array![[0.5, 0.5], [1.5, 0.5], [0.5, 1.5], [1.25, 1.75]];

    let options = UniversalKrigingOptions {
        drift_bases: vec![DriftBasis::Linear],
        variogram: Variogram::new(VariogramModel::Exponential, 0.0, 1.0, 1.0),
        regularization: 1e-10,
    };

    let result =
        universal_kriging_fit(coords.view(), values.view(), &options, targets.view(), None)
            .expect("UKED with linear drift should succeed");

    for q in 0..targets.nrows() {
        let truth = 2.0 * targets[[q, 0]] + 3.0 * targets[[q, 1]];
        let rel_err = (result.predicted[q] - truth).abs() / truth.abs().max(1e-12);
        assert!(
            rel_err < 0.05,
            "linear trend recovery failed at query {}: predicted={}, truth={}, rel_err={}",
            q,
            result.predicted[q],
            truth,
            rel_err,
        );
    }
}

#[test]
fn test_universal_kriging_quadratic_drift_recovers_quadratic_surface() {
    // Quadratic surface z(x, y) = x² + y² sampled on a 4x4 grid. Quadratic
    // drift basis should reproduce the surface to within a few percent
    // even with a small nugget.
    let mut coord_rows: Vec<f64> = Vec::new();
    for i in 0..4 {
        for j in 0..4 {
            coord_rows.push(i as f64);
            coord_rows.push(j as f64);
        }
    }
    let coords = Array2::from_shape_vec((16, 2), coord_rows)
        .expect("16x2 reshape from 32-element vec must succeed");
    let mut vals_vec = Vec::with_capacity(16);
    for i in 0..16 {
        let x = coords[[i, 0]];
        let y = coords[[i, 1]];
        vals_vec.push(x * x + y * y);
    }
    let values = scirs2_core::ndarray::Array1::from_vec(vals_vec);
    let targets = array![[1.5, 1.5], [0.5, 2.5], [2.5, 0.5]];

    let options = UniversalKrigingOptions {
        drift_bases: vec![DriftBasis::Quadratic],
        variogram: Variogram::new(VariogramModel::Gaussian, 0.0, 0.5, 1.5),
        regularization: 1e-10,
    };

    let result =
        universal_kriging_fit(coords.view(), values.view(), &options, targets.view(), None)
            .expect("UKED with quadratic drift should succeed");

    for q in 0..targets.nrows() {
        let x = targets[[q, 0]];
        let y = targets[[q, 1]];
        let truth = x * x + y * y;
        let rel_err = (result.predicted[q] - truth).abs() / truth.abs().max(1e-12);
        assert!(
            rel_err < 0.05,
            "quadratic surface recovery failed at query {}: predicted={}, truth={}, rel_err={}",
            q,
            result.predicted[q],
            truth,
            rel_err,
        );
    }
}

#[test]
fn test_universal_kriging_external_drift_basis_elevation() {
    // Composite signal: linear trend in (x, y) plus an elevation-driven
    // contribution. UKED with Linear + External(elevation) should track
    // the composite truth far better than constant drift alone.
    let coords = array![
        [0.0, 0.0],
        [2.0, 0.0],
        [0.0, 2.0],
        [2.0, 2.0],
        [1.0, 1.0],
        [3.0, 1.0],
        [1.0, 3.0],
    ];
    let elevations: Vec<f64> = (0..coords.nrows())
        .map(|i| {
            let x: f64 = coords[[i, 0]];
            let y: f64 = coords[[i, 1]];
            10.0_f64 + x.sin().abs() + y.cos().abs()
        })
        .collect();
    let mut vals_vec = Vec::with_capacity(coords.nrows());
    for i in 0..coords.nrows() {
        let x = coords[[i, 0]];
        let y = coords[[i, 1]];
        vals_vec.push(1.0 + 0.5 * x + 0.25 * y + 0.75 * elevations[i]);
    }
    let values = scirs2_core::ndarray::Array1::from_vec(vals_vec);

    let targets = array![[0.5, 0.5], [1.5, 1.5], [2.5, 0.5]];
    let mut query_elev = Vec::with_capacity(targets.nrows());
    for q in 0..targets.nrows() {
        let x: f64 = targets[[q, 0]];
        let y: f64 = targets[[q, 1]];
        query_elev.push(10.0_f64 + x.sin().abs() + y.cos().abs());
    }
    let query_external = Array2::from_shape_vec((targets.nrows(), 1), query_elev.clone())
        .expect("query external drift reshape must succeed");

    let options = UniversalKrigingOptions {
        drift_bases: vec![DriftBasis::Linear, DriftBasis::External(elevations.clone())],
        variogram: Variogram::new(VariogramModel::Spherical, 0.0, 0.25, 2.5),
        regularization: 1e-10,
    };

    let result = universal_kriging_fit(
        coords.view(),
        values.view(),
        &options,
        targets.view(),
        Some(query_external.view()),
    )
    .expect("UKED with external drift should succeed");

    for q in 0..targets.nrows() {
        let x = targets[[q, 0]];
        let y = targets[[q, 1]];
        let truth = 1.0 + 0.5 * x + 0.25 * y + 0.75 * query_elev[q];
        let rel_err = (result.predicted[q] - truth).abs() / truth.abs().max(1e-12);
        assert!(
            rel_err < 0.10,
            "external-drift recovery failed at query {}: predicted={}, truth={}, rel_err={}",
            q,
            result.predicted[q],
            truth,
            rel_err,
        );
    }
}

#[test]
fn test_universal_kriging_singular_design_matrix_errors() {
    // Three collinear samples cannot identify the 6 quadratic basis
    // coefficients — the augmented matrix is rank-deficient.
    let coords = array![[0.0, 0.0], [1.0, 1.0], [2.0, 2.0]];
    let values = array![1.0, 2.0, 3.0];
    let targets = array![[0.5, 0.5]];

    let options = UniversalKrigingOptions {
        drift_bases: vec![DriftBasis::Quadratic],
        variogram: Variogram::new(VariogramModel::Spherical, 0.0, 1.0, 5.0),
        regularization: 0.0,
    };

    let result =
        universal_kriging_fit(coords.view(), values.view(), &options, targets.view(), None);
    assert!(
        result.is_err(),
        "expected Err for rank-deficient quadratic drift on 3 collinear points, got {:?}",
        result.as_ref().map(|r| &r.predicted),
    );
}

#[test]
fn test_universal_kriging_at_sample_point_returns_observation_within_eps() {
    // Querying exactly at a known sample point should produce the
    // observed value (within numerical tolerance) thanks to the
    // exact-interpolation property of kriging when nugget=0.
    let (coords, values) = sample_coords_values();
    let targets = array![[1.0, 1.0]]; // matches coords[3], value 4.0

    let options = UniversalKrigingOptions {
        drift_bases: vec![DriftBasis::Linear],
        variogram: Variogram::new(VariogramModel::Spherical, 0.0, 1.0, 3.0),
        regularization: 1e-12,
    };

    let result =
        universal_kriging_fit(coords.view(), values.view(), &options, targets.view(), None)
            .expect("UKED at sample point should succeed");

    assert!(
        (result.predicted[0] - 4.0).abs() < 1e-6,
        "exact interpolation broken: predicted={}, expected 4.0",
        result.predicted[0],
    );
}

#[test]
fn test_universal_kriging_variance_nonnegative_at_query() {
    let (coords, values) = sample_coords_values();
    let targets = array![[0.25, 0.25], [0.5, 0.5], [10.0, 10.0]];

    let options = UniversalKrigingOptions {
        drift_bases: vec![DriftBasis::Linear],
        variogram: Variogram::new(VariogramModel::Exponential, 0.05, 1.0, 1.5),
        regularization: 1e-10,
    };

    let result =
        universal_kriging_fit(coords.view(), values.view(), &options, targets.view(), None)
            .expect("UKED variance test should succeed");

    for (q, v) in result.variance.iter().enumerate() {
        assert!(
            *v >= 0.0,
            "kriging variance at query {} was negative: {}",
            q,
            v,
        );
        assert!(v.is_finite(), "variance must be finite, got {} at {}", v, q);
    }
}

#[test]
fn test_universal_kriging_drift_coefficients_returned() {
    // Drift coefficient count must equal the total column count p across
    // every basis. Use Linear + External(elev) — 3 + 1 = 4 distinct
    // (non-collinear) drift columns to avoid an inherently singular design.
    let coords = array![
        [0.0, 0.0],
        [1.0, 0.0],
        [2.0, 0.0],
        [0.0, 1.0],
        [1.0, 1.0],
        [2.0, 1.0],
        [0.0, 2.0],
        [1.0, 2.0],
        [2.0, 2.0],
        [0.5, 1.5],
    ];
    let elevations: Vec<f64> = (0..coords.nrows())
        .map(|i| {
            let x: f64 = coords[[i, 0]];
            let y: f64 = coords[[i, 1]];
            // Non-polynomial signal independent of (1, x, y).
            5.0_f64 + (x + y).sin()
        })
        .collect();
    let values = scirs2_core::ndarray::Array1::from_vec(
        (0..coords.nrows())
            .map(|i| coords[[i, 0]] + 2.0 * coords[[i, 1]] + 0.1 * elevations[i])
            .collect::<Vec<_>>(),
    );
    let targets = array![[0.5, 0.5]];
    let target_elev: Vec<f64> = (0..targets.nrows())
        .map(|q| {
            let x: f64 = targets[[q, 0]];
            let y: f64 = targets[[q, 1]];
            5.0_f64 + (x + y).sin()
        })
        .collect();
    let query_external = Array2::from_shape_vec((targets.nrows(), 1), target_elev)
        .expect("query external reshape must succeed");

    let options = UniversalKrigingOptions {
        drift_bases: vec![DriftBasis::Linear, DriftBasis::External(elevations)],
        variogram: Variogram::new(VariogramModel::Spherical, 0.0, 1.0, 3.0),
        regularization: 1e-9,
    };

    let result = universal_kriging_fit(
        coords.view(),
        values.view(),
        &options,
        targets.view(),
        Some(query_external.view()),
    )
    .expect("UKED with Linear+External drift should succeed on 10 non-collinear samples");

    let expected_p = 3 + 1;
    assert_eq!(
        result.drift_coefficients.len(),
        expected_p,
        "drift_coefficients length must equal total p={} columns, got {}",
        expected_p,
        result.drift_coefficients.len(),
    );
}

#[test]
fn test_universal_kriging_mismatched_external_drift_length_errors() {
    // External(vec![1.0, 2.0]) has length 2 but coords has 5 rows — must
    // error out before any solve attempt.
    let (coords, values) = sample_coords_values();
    let targets = array![[0.25, 0.25]];

    let options = UniversalKrigingOptions {
        drift_bases: vec![DriftBasis::External(vec![1.0, 2.0])],
        variogram: Variogram::new(VariogramModel::Spherical, 0.0, 1.0, 1.0),
        regularization: 1e-10,
    };
    // Query external drift must match basis count (=1).
    let query_external =
        Array2::from_shape_vec((1, 1), vec![1.0]).expect("query external reshape must succeed");

    let result = universal_kriging_fit(
        coords.view(),
        values.view(),
        &options,
        targets.view(),
        Some(query_external.view()),
    );
    assert!(
        result.is_err(),
        "expected Err for External drift length 2 vs n=5, got {:?}",
        result.as_ref().map(|r| &r.predicted),
    );
}

#[test]
fn test_universal_kriging_default_options_constant_basis() {
    let opts = UniversalKrigingOptions::default();
    assert_eq!(
        opts.drift_bases.len(),
        1,
        "default drift_bases must contain exactly one basis",
    );
    assert!(
        matches!(opts.drift_bases[0], DriftBasis::Constant),
        "default basis must be DriftBasis::Constant, got {:?}",
        opts.drift_bases[0],
    );
}
