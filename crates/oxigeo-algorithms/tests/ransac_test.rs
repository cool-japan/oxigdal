//! Integration tests for RANSAC robust line and plane fitting.

use oxigeo_algorithms::error::AlgorithmError;
use oxigeo_algorithms::vector::{
    Point, RansacLineModel, RansacOptions, RansacPlaneModel, ransac_fit_line, ransac_fit_plane,
};

/// Maximum residual we accept when checking that points lie "on" a recovered
/// model.
const RESIDUAL_TOL: f64 = 1e-6;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns true if two lines describe the same geometric line up to the
/// `(a, b, c)` vs `(-a, -b, -c)` sign ambiguity.
fn lines_equal_up_to_sign(m1: &RansacLineModel, m2: &RansacLineModel) -> bool {
    let same =
        (m1.a - m2.a).abs() < 1e-9 && (m1.b - m2.b).abs() < 1e-9 && (m1.c - m2.c).abs() < 1e-9;
    let flipped =
        (m1.a + m2.a).abs() < 1e-9 && (m1.b + m2.b).abs() < 1e-9 && (m1.c + m2.c).abs() < 1e-9;
    same || flipped
}

/// Returns true if two planes describe the same geometric plane up to sign.
fn planes_equal_up_to_sign(m1: &RansacPlaneModel, m2: &RansacPlaneModel) -> bool {
    let same = (m1.a - m2.a).abs() < 1e-9
        && (m1.b - m2.b).abs() < 1e-9
        && (m1.c - m2.c).abs() < 1e-9
        && (m1.d - m2.d).abs() < 1e-9;
    let flipped = (m1.a + m2.a).abs() < 1e-9
        && (m1.b + m2.b).abs() < 1e-9
        && (m1.c + m2.c).abs() < 1e-9
        && (m1.d + m2.d).abs() < 1e-9;
    same || flipped
}

// ---------------------------------------------------------------------------
// Line tests
// ---------------------------------------------------------------------------

#[test]
fn test_ransac_line_two_points_returns_those_two() {
    let points = [Point::new(0.0, 0.0), Point::new(1.0, 1.0)];
    let result =
        ransac_fit_line(&points, &RansacOptions::default()).expect("two points should fit a line");
    assert!(result.converged);
    // Both input points must be inliers of the recovered line.
    assert!(result.inliers.contains(&0));
    assert!(result.inliers.contains(&1));
    assert_eq!(result.inliers.len(), 2);
    // Residuals for both points are ~0.
    assert!(result.model.distance_to(points[0].clone()) < RESIDUAL_TOL);
    assert!(result.model.distance_to(points[1].clone()) < RESIDUAL_TOL);
}

#[test]
fn test_ransac_line_perfect_inliers_no_outliers_recovers_line() {
    // All points on y = 2x + 1, i.e. 2x - y + 1 = 0.
    let points: Vec<Point> = (0..20)
        .map(|i| {
            let x = i as f64;
            Point::new(x, 2.0 * x + 1.0)
        })
        .collect();
    let result =
        ransac_fit_line(&points, &RansacOptions::default()).expect("perfect data should fit");
    assert!(result.converged);
    assert_eq!(result.inliers.len(), points.len());
    // Every point has ~0 residual against the recovered line.
    for p in &points {
        assert!(
            result.model.distance_to(p.clone()) < RESIDUAL_TOL,
            "residual {} too large",
            result.model.distance_to(p.clone())
        );
    }
    // The recovered line must match the normalized 2x - y + 1 = 0 up to sign.
    let norm = (2.0_f64 * 2.0 + 1.0).sqrt();
    let expected = RansacLineModel {
        a: 2.0 / norm,
        b: -1.0 / norm,
        c: 1.0 / norm,
    };
    assert!(lines_equal_up_to_sign(&result.model, &expected));
}

#[test]
fn test_ransac_line_rejects_single_outlier() {
    // 10 points on y = x, plus one gross outlier.
    let mut points: Vec<Point> = (0..10).map(|i| Point::new(i as f64, i as f64)).collect();
    let outlier_index = points.len();
    points.push(Point::new(5.0, 500.0));

    let result = ransac_fit_line(&points, &RansacOptions::default()).expect("data should fit");
    assert!(result.converged);
    assert!(
        !result.inliers.contains(&outlier_index),
        "outlier {outlier_index} should be rejected"
    );
    assert_eq!(result.inliers.len(), 10);
}

#[test]
fn test_ransac_line_recovers_majority_when_30pct_outliers() {
    // 70 inliers on y = 0.5x + 3, 30 outliers far away.
    let mut points: Vec<Point> = Vec::new();
    for i in 0..70 {
        let x = i as f64;
        points.push(Point::new(x, 0.5 * x + 3.0));
    }
    // Outliers offset well beyond the threshold band.
    for i in 0..30 {
        let x = i as f64;
        points.push(Point::new(x, 0.5 * x + 3.0 + 100.0 + i as f64));
    }

    let options = RansacOptions {
        inlier_threshold: 1e-6,
        ..RansacOptions::default()
    };
    let result = ransac_fit_line(&points, &options).expect("data should fit");
    assert!(result.converged);
    // The majority (70 inliers) must be recovered.
    assert_eq!(result.inliers.len(), 70);
    // The recovered model matches y = 0.5x + 3 ⇒ 0.5x - y + 3 = 0.
    let norm = (0.5_f64 * 0.5 + 1.0).sqrt();
    let expected = RansacLineModel {
        a: 0.5 / norm,
        b: -1.0 / norm,
        c: 3.0 / norm,
    };
    assert!(lines_equal_up_to_sign(&result.model, &expected));
}

#[test]
fn test_ransac_line_too_few_points_returns_invalid_input() {
    let empty: [Point; 0] = [];
    assert!(matches!(
        ransac_fit_line(&empty, &RansacOptions::default()),
        Err(AlgorithmError::InvalidInput(_))
    ));

    let single = [Point::new(1.0, 2.0)];
    assert!(matches!(
        ransac_fit_line(&single, &RansacOptions::default()),
        Err(AlgorithmError::InvalidInput(_))
    ));
}

#[test]
fn test_ransac_line_collinear_three_points_recovered() {
    // Three collinear points on y = -x + 4 ⇒ x + y - 4 = 0.
    let points = [
        Point::new(0.0, 4.0),
        Point::new(1.0, 3.0),
        Point::new(2.0, 2.0),
    ];
    let result =
        ransac_fit_line(&points, &RansacOptions::default()).expect("collinear points should fit");
    assert!(result.converged);
    assert_eq!(result.inliers.len(), 3);
    for p in &points {
        assert!(result.model.distance_to(p.clone()) < RESIDUAL_TOL);
    }
}

// ---------------------------------------------------------------------------
// Plane tests
// ---------------------------------------------------------------------------

#[test]
fn test_ransac_plane_three_points_returns_those_three() {
    let points = [(0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (0.0, 1.0, 0.0)];
    let result = ransac_fit_plane(&points, &RansacOptions::default())
        .expect("three points should fit a plane");
    assert!(result.converged);
    assert_eq!(result.inliers.len(), 3);
    for p in &points {
        assert!(result.model.distance_to(*p) < RESIDUAL_TOL);
    }
    // The points lie on z = 0 ⇒ normal is ±(0, 0, 1), d = 0.
    let expected = RansacPlaneModel {
        a: 0.0,
        b: 0.0,
        c: 1.0,
        d: 0.0,
    };
    assert!(planes_equal_up_to_sign(&result.model, &expected));
}

#[test]
fn test_ransac_plane_perfect_inliers_recovers_plane() {
    // All points on z = x + 2y + 3 ⇒ x + 2y - z + 3 = 0.
    let mut points: Vec<(f64, f64, f64)> = Vec::new();
    for i in 0..6 {
        for j in 0..6 {
            let x = i as f64;
            let y = j as f64;
            points.push((x, y, x + 2.0 * y + 3.0));
        }
    }
    let result =
        ransac_fit_plane(&points, &RansacOptions::default()).expect("perfect plane should fit");
    assert!(result.converged);
    assert_eq!(result.inliers.len(), points.len());
    for p in &points {
        assert!(
            result.model.distance_to(*p) < RESIDUAL_TOL,
            "residual {} too large",
            result.model.distance_to(*p)
        );
    }
    let norm = (1.0_f64 + 4.0 + 1.0).sqrt();
    let expected = RansacPlaneModel {
        a: 1.0 / norm,
        b: 2.0 / norm,
        c: -1.0 / norm,
        d: 3.0 / norm,
    };
    assert!(planes_equal_up_to_sign(&result.model, &expected));
}

#[test]
fn test_ransac_plane_rejects_outliers_below_threshold() {
    // Inliers on z = 0 plus outliers well above it.
    let mut points: Vec<(f64, f64, f64)> = Vec::new();
    for i in 0..6 {
        for j in 0..6 {
            points.push((i as f64, j as f64, 0.0));
        }
    }
    let inlier_count = points.len();
    // Outliers far off the z = 0 plane.
    points.push((0.0, 0.0, 50.0));
    points.push((1.0, 1.0, 75.0));
    points.push((2.0, 3.0, 100.0));

    let options = RansacOptions {
        inlier_threshold: 1e-6,
        ..RansacOptions::default()
    };
    let result = ransac_fit_plane(&points, &options).expect("data should fit");
    assert!(result.converged);
    assert_eq!(result.inliers.len(), inlier_count);
    // The three outliers (last three indices) must be rejected.
    assert!(!result.inliers.contains(&inlier_count));
    assert!(!result.inliers.contains(&(inlier_count + 1)));
    assert!(!result.inliers.contains(&(inlier_count + 2)));
}

#[test]
fn test_ransac_plane_too_few_points_returns_invalid_input() {
    let empty: [(f64, f64, f64); 0] = [];
    assert!(matches!(
        ransac_fit_plane(&empty, &RansacOptions::default()),
        Err(AlgorithmError::InvalidInput(_))
    ));

    let two = [(0.0, 0.0, 0.0), (1.0, 1.0, 1.0)];
    assert!(matches!(
        ransac_fit_plane(&two, &RansacOptions::default()),
        Err(AlgorithmError::InvalidInput(_))
    ));
}

// ---------------------------------------------------------------------------
// Options / determinism / adaptive-count tests
// ---------------------------------------------------------------------------

#[test]
fn test_ransac_options_default_max_iterations_1000() {
    let options = RansacOptions::default();
    assert_eq!(options.max_iterations, 1000);
    assert_eq!(options.min_inlier_count, 2);
    assert_eq!(options.seed, 0xDEAD_BEEF);
    assert!((options.inlier_threshold - 1e-3).abs() < f64::EPSILON);
    assert!((options.confidence - 0.99).abs() < f64::EPSILON);
}

#[test]
fn test_ransac_seed_reproducibility_same_seed_same_result() {
    // Ambiguous-ish data with noise so the sampling order matters.
    let mut points: Vec<Point> = Vec::new();
    for i in 0..30 {
        let x = i as f64;
        points.push(Point::new(x, 1.5 * x + 2.0));
    }
    for i in 0..15 {
        let x = i as f64;
        points.push(Point::new(x, 1.5 * x + 2.0 + 200.0 + i as f64));
    }

    let options = RansacOptions {
        seed: 0x1234_5678,
        inlier_threshold: 1e-6,
        ..RansacOptions::default()
    };
    let r1 = ransac_fit_line(&points, &options).expect("fit");
    let r2 = ransac_fit_line(&points, &options).expect("fit");

    // Identical seed ⇒ identical full inlier vectors, iteration counts and model.
    assert_eq!(r1.inliers, r2.inliers);
    assert_eq!(r1.iterations, r2.iterations);
    assert_eq!(r1.converged, r2.converged);
    assert!(lines_equal_up_to_sign(&r1.model, &r2.model));
}

#[test]
fn test_ransac_seed_different_seeds_different_inlier_sets_for_ambiguous_input() {
    // Two parallel lines of equal support form an ambiguous configuration: each
    // is an equally-good 40-point consensus set. Which one a seeded RANSAC run
    // latches onto depends on its sampling order, so the inlier set is
    // genuinely seed-dependent. We assert that scanning several seeds yields at
    // least two distinct outcomes (the meaningful, deterministic property).
    let mut points: Vec<Point> = Vec::new();
    // Line A: y = 2x (40 points).
    for i in 0..40 {
        let x = i as f64;
        points.push(Point::new(x, 2.0 * x));
    }
    // Line B: y = 2x + 1000 (parallel, far away ⇒ disjoint inlier sets).
    for i in 0..40 {
        let x = i as f64;
        points.push(Point::new(x, 2.0 * x + 1000.0));
    }

    let mut distinct_outcomes: Vec<Vec<usize>> = Vec::new();
    for seed in [1u64, 2, 3, 7, 42, 0xABCD, 0xFFFF_0000, 0xDEAD_BEEF] {
        let options = RansacOptions {
            seed,
            inlier_threshold: 1e-9,
            ..RansacOptions::default()
        };
        let result = ransac_fit_line(&points, &options).expect("fit");
        assert!(result.converged);
        // Each run recovers exactly one of the two 40-point lines.
        assert_eq!(result.inliers.len(), 40);
        if !distinct_outcomes.contains(&result.inliers) {
            distinct_outcomes.push(result.inliers.clone());
        }
    }

    assert!(
        distinct_outcomes.len() >= 2,
        "expected seed-dependent inlier sets on ambiguous input, but all {} seeds agreed",
        8
    );
}

#[test]
fn test_ransac_adaptive_iteration_count_high_inlier_ratio_terminates_early() {
    // Clean data (inlier ratio ~ 1) ⇒ the adaptive criterion should stop after
    // very few iterations, well under the 1000 budget.
    let points: Vec<Point> = (0..50)
        .map(|i| {
            let x = i as f64;
            Point::new(x, 3.0 * x - 7.0)
        })
        .collect();
    let result =
        ransac_fit_line(&points, &RansacOptions::default()).expect("clean data should fit");
    assert!(result.converged);
    assert!(
        result.iterations < 50,
        "expected early termination, but ran {} iterations",
        result.iterations
    );
}
