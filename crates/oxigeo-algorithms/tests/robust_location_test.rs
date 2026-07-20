//! Integration tests for robust location estimators (W6, Slice 18).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use oxigeo_algorithms::{
    RobustLocationOptions, geometric_median, geometric_median_3d, geometric_median_with_options,
    l1_median, spatial_mean, weighted_geometric_median,
};

// ---------------------------------------------------------------------------
// geometric_median
// ---------------------------------------------------------------------------

#[test]
fn test_geometric_median_empty_returns_none() {
    assert!(geometric_median(&[]).is_none());
}

#[test]
fn test_geometric_median_single_point_returns_that_point() {
    let p = geometric_median(&[(3.0, 7.0)]).unwrap();
    assert!((p.0 - 3.0).abs() < 1e-9 && (p.1 - 7.0).abs() < 1e-9);
}

#[test]
fn test_geometric_median_two_points_returns_midpoint() {
    let p = geometric_median(&[(0.0, 0.0), (4.0, 0.0)]).unwrap();
    // For two equidistant points the geometric median is any point on the segment;
    // Weiszfeld converges to the midpoint from the spatial-mean initial guess.
    assert!((p.0 - 2.0).abs() < 1e-6);
}

#[test]
fn test_geometric_median_unit_equilateral_triangle_at_fermat_point() {
    // Equilateral triangle vertices.  The geometric median (= Fermat point)
    // equals the centroid: (0, h/3) for the standard configuration.
    let h = 3_f64.sqrt() / 2.0;
    let pts = [(-0.5, 0.0), (0.5, 0.0), (0.0, h)];
    let centroid = (0.0, h / 3.0);
    let p = geometric_median(&pts).unwrap();
    assert!(
        (p.0 - centroid.0).abs() < 1e-6 && (p.1 - centroid.1).abs() < 1e-6,
        "expected centroid ({:.8}, {:.8}), got ({:.8}, {:.8})",
        centroid.0,
        centroid.1,
        p.0,
        p.1
    );
}

#[test]
fn test_geometric_median_unit_square_near_centroid() {
    // For a symmetric square the geometric median == centroid.
    let pts = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
    let p = geometric_median(&pts).unwrap();
    assert!(
        (p.0 - 0.5).abs() < 1e-6 && (p.1 - 0.5).abs() < 1e-6,
        "expected (0.5, 0.5), got ({:.8}, {:.8})",
        p.0,
        p.1
    );
}

#[test]
fn test_geometric_median_robust_to_single_outlier() {
    // 5 points near (0, 0), 1 outlier at (1000, 1000)
    let mut pts = vec![
        (0.1, 0.0),
        (0.0, 0.1),
        (-0.1, 0.0),
        (0.0, -0.1),
        (0.05, 0.05),
    ];
    pts.push((1000.0, 1000.0));
    let p = geometric_median(&pts).unwrap();
    let mean = spatial_mean(&pts).unwrap();
    // Median should be much closer to origin than the arithmetic mean.
    let dist_median = (p.0 * p.0 + p.1 * p.1).sqrt();
    let dist_mean = (mean.0 * mean.0 + mean.1 * mean.1).sqrt();
    assert!(
        dist_median < dist_mean,
        "geometric median (dist {:.4}) should be closer to cluster than mean (dist {:.4})",
        dist_median,
        dist_mean
    );
}

// ---------------------------------------------------------------------------
// weighted_geometric_median
// ---------------------------------------------------------------------------

#[test]
fn test_weighted_geometric_median_pulls_toward_heavier_weight() {
    let pts = [(0.0, 0.0), (10.0, 0.0)];
    // Weight the right point much more heavily.
    let opts = RobustLocationOptions::default();
    let p = weighted_geometric_median(&pts, &[1.0, 100.0], &opts).unwrap();
    assert!(
        p.0 > 5.0,
        "weighted median x ({:.4}) should be closer to the heavier point at x=10",
        p.0
    );
}

#[test]
fn test_weighted_geometric_median_uniform_weights_matches_unweighted() {
    let pts = [(1.0, 2.0), (3.0, 4.0), (5.0, 6.0)];
    let opts = RobustLocationOptions::default();
    let unweighted = geometric_median(&pts).unwrap();
    let weighted = weighted_geometric_median(&pts, &[1.0, 1.0, 1.0], &opts).unwrap();
    assert!(
        (unweighted.0 - weighted.0).abs() < 1e-6,
        "x mismatch: unweighted={:.8}, weighted={:.8}",
        unweighted.0,
        weighted.0
    );
    assert!(
        (unweighted.1 - weighted.1).abs() < 1e-6,
        "y mismatch: unweighted={:.8}, weighted={:.8}",
        unweighted.1,
        weighted.1
    );
}

#[test]
fn test_weighted_geometric_median_mismatched_lengths_returns_none() {
    let opts = RobustLocationOptions::default();
    let result = weighted_geometric_median(&[(0.0, 0.0)], &[1.0, 2.0], &opts);
    assert!(result.is_none());
}

// ---------------------------------------------------------------------------
// l1_median
// ---------------------------------------------------------------------------

#[test]
fn test_l1_median_odd_count() {
    // x-values: 1, 5, 9  → median x = 5
    // y-values: 2, 4, 6  → median y = 4
    let pts = [(1.0, 2.0), (9.0, 6.0), (5.0, 4.0)];
    let p = l1_median(&pts).unwrap();
    assert!(
        (p.0 - 5.0).abs() < 1e-9 && (p.1 - 4.0).abs() < 1e-9,
        "expected (5, 4), got ({:.8}, {:.8})",
        p.0,
        p.1
    );
}

#[test]
fn test_l1_median_even_count() {
    // x: 1, 3  → mean of middle two = 2
    // y: 2, 4  → mean of middle two = 3
    let pts = [(1.0, 2.0), (3.0, 4.0)];
    let p = l1_median(&pts).unwrap();
    assert!(
        (p.0 - 2.0).abs() < 1e-9 && (p.1 - 3.0).abs() < 1e-9,
        "expected (2, 3), got ({:.8}, {:.8})",
        p.0,
        p.1
    );
}

// ---------------------------------------------------------------------------
// spatial_mean
// ---------------------------------------------------------------------------

#[test]
fn test_spatial_mean_matches_centroid() {
    let pts = [(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)];
    let p = spatial_mean(&pts).unwrap();
    assert!(
        (p.0 - 1.0).abs() < 1e-9 && (p.1 - 1.0).abs() < 1e-9,
        "expected (1, 1), got ({:.8}, {:.8})",
        p.0,
        p.1
    );
}

// ---------------------------------------------------------------------------
// geometric_median_3d
// ---------------------------------------------------------------------------

#[test]
fn test_geometric_median_3d_octahedron_at_origin() {
    // ±x, ±y, ±z unit vectors → symmetric → geometric median at origin.
    let pts = [
        (1.0, 0.0, 0.0),
        (-1.0, 0.0, 0.0),
        (0.0, 1.0, 0.0),
        (0.0, -1.0, 0.0),
        (0.0, 0.0, 1.0),
        (0.0, 0.0, -1.0),
    ];
    let opts = RobustLocationOptions::default();
    let p = geometric_median_3d(&pts, &opts).unwrap();
    assert!(
        p.0.abs() < 1e-6 && p.1.abs() < 1e-6 && p.2.abs() < 1e-6,
        "expected origin, got ({:.8}, {:.8}, {:.8})",
        p.0,
        p.1,
        p.2
    );
}

// ---------------------------------------------------------------------------
// geometric_median_with_options — edge-case options
// ---------------------------------------------------------------------------

#[test]
fn test_geometric_median_options_max_iter_zero_returns_initial_guess() {
    let pts = [(0.0, 0.0), (10.0, 0.0)];
    let opts = RobustLocationOptions::default().with_max_iter(0);
    let p = geometric_median_with_options(&pts, &opts).unwrap();
    // 0 iterations → returns initial guess = weighted mean = spatial mean = (5, 0)
    assert!(
        (p.0 - 5.0).abs() < 1e-9 && p.1.abs() < 1e-9,
        "expected initial guess (5, 0), got ({:.8}, {:.8})",
        p.0,
        p.1
    );
}
