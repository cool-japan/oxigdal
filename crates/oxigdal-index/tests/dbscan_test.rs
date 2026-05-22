//! Integration tests for R-tree-accelerated DBSCAN clustering.
//!
//! These tests exercise the public API of `oxigdal_index::clustering::dbscan`
//! via the crate-root re-exports.

use oxigdal_index::{
    Bbox2D, DbscanOptions, NOISE, RTree, dbscan_rtree, dbscan_with_rtree, range_query_eps,
};

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn build_rtree(points: &[(f64, f64)]) -> RTree<usize> {
    let mut rtree: RTree<usize> = RTree::new();
    for (i, &(x, y)) in points.iter().enumerate() {
        rtree.insert(Bbox2D::point(x, y), i);
    }
    rtree
}

// ---------------------------------------------------------------------------
// Test 1 — empty input
// ---------------------------------------------------------------------------

#[test]
fn test_dbscan_empty_input_returns_empty_result() {
    let result = dbscan_rtree(&[], &DbscanOptions::default());
    assert!(result.labels.is_empty());
    assert_eq!(result.num_clusters, 0);
    assert_eq!(result.noise_count, 0);
}

// ---------------------------------------------------------------------------
// Test 2 — single dense cluster
// ---------------------------------------------------------------------------

#[test]
fn test_dbscan_single_dense_cluster_labels_all_zero() {
    // 5 tightly-packed points — all within eps=1.0 of each other.
    let points: Vec<(f64, f64)> =
        vec![(0.0, 0.0), (0.1, 0.0), (0.0, 0.1), (0.1, 0.1), (0.05, 0.05)];
    let result = dbscan_rtree(
        &points,
        &DbscanOptions {
            eps: 1.0,
            min_points: 3,
        },
    );
    assert_eq!(result.num_clusters, 1);
    assert_eq!(result.noise_count, 0);
    assert!(
        result.labels.iter().all(|&l| l == 0),
        "all labels must be 0, got: {:?}",
        result.labels
    );
}

// ---------------------------------------------------------------------------
// Test 3 — two separated clusters
// ---------------------------------------------------------------------------

#[test]
fn test_dbscan_two_separated_clusters_distinct_labels() {
    // Cluster A near origin; cluster B near (10, 10).
    let points: Vec<(f64, f64)> = vec![
        (0.0, 0.0),
        (0.1, 0.0),
        (0.0, 0.1), // cluster A
        (10.0, 10.0),
        (10.1, 10.0),
        (10.0, 10.1), // cluster B
    ];
    let result = dbscan_rtree(
        &points,
        &DbscanOptions {
            eps: 1.0,
            min_points: 3,
        },
    );
    assert_eq!(result.num_clusters, 2);
    assert_eq!(result.noise_count, 0);

    let label_a = result.labels[0];
    let label_b = result.labels[3];
    assert!(label_a >= 0, "cluster A label must be non-negative");
    assert!(label_b >= 0, "cluster B label must be non-negative");
    assert_ne!(label_a, label_b, "clusters A and B must have different ids");
}

// ---------------------------------------------------------------------------
// Test 4 — noise labelled -1
// ---------------------------------------------------------------------------

#[test]
fn test_dbscan_noise_points_labeled_minus_one() {
    // 3 dense points + 1 isolated point that cannot reach any cluster.
    let points: Vec<(f64, f64)> = vec![
        (0.0, 0.0),
        (0.1, 0.0),
        (0.0, 0.1),     // dense group → cluster 0
        (100.0, 100.0), // isolated → noise
    ];
    let result = dbscan_rtree(
        &points,
        &DbscanOptions {
            eps: 0.5,
            min_points: 3,
        },
    );
    assert_eq!(
        result.labels[3], NOISE,
        "isolated point at (100,100) must be labelled NOISE"
    );
    assert_eq!(result.noise_count, 1);
    assert_eq!(result.num_clusters, 1);
}

// ---------------------------------------------------------------------------
// Test 5 — min_points threshold makes both points noise
// ---------------------------------------------------------------------------

#[test]
fn test_dbscan_min_points_threshold_creates_noise() {
    // Only 2 nearby points but min_points = 5 → both are noise.
    let points: Vec<(f64, f64)> = vec![(0.0, 0.0), (0.1, 0.0)];
    let result = dbscan_rtree(
        &points,
        &DbscanOptions {
            eps: 1.0,
            min_points: 5,
        },
    );
    assert_eq!(result.num_clusters, 0);
    assert_eq!(result.noise_count, 2);
    assert!(
        result.labels.iter().all(|&l| l == NOISE),
        "all labels must be NOISE, got: {:?}",
        result.labels
    );
}

// ---------------------------------------------------------------------------
// Test 6 — eps too small → all noise
// ---------------------------------------------------------------------------

#[test]
fn test_dbscan_eps_too_small_all_noise() {
    let points: Vec<(f64, f64)> = vec![(0.0, 0.0), (5.0, 0.0), (10.0, 0.0)];
    let result = dbscan_rtree(
        &points,
        &DbscanOptions {
            eps: 0.001,
            min_points: 2,
        },
    );
    assert_eq!(result.num_clusters, 0);
    assert_eq!(result.noise_count, 3);
    assert!(result.labels.iter().all(|&l| l == NOISE));
}

// ---------------------------------------------------------------------------
// Test 7 — range_query_eps excludes point outside radius
// ---------------------------------------------------------------------------

#[test]
fn test_range_query_eps_excludes_outside_radius() {
    // points[0] = (0,0), points[1] = (0.5,0), points[2] = (2.0,0)
    // Query centre=(0,0), eps=1.0 → 0 and 1 in, 2 out.
    let points: Vec<(f64, f64)> = vec![(0.0, 0.0), (0.5, 0.0), (2.0, 0.0)];
    let rtree = build_rtree(&points);
    let neighbors = range_query_eps(&rtree, &points, 0.0, 0.0, 1.0);
    assert!(
        neighbors.contains(&0),
        "origin must be in its own neighborhood"
    );
    assert!(
        neighbors.contains(&1),
        "point at distance 0.5 must be included"
    );
    assert!(
        !neighbors.contains(&2),
        "point at distance 2.0 must be excluded"
    );
}

// ---------------------------------------------------------------------------
// Test 8 — dbscan_with_rtree uses caller-provided tree
// ---------------------------------------------------------------------------

#[test]
fn test_dbscan_with_rtree_uses_provided_tree() {
    let points: Vec<(f64, f64)> = vec![
        (0.0, 0.0),
        (0.1, 0.0),
        (0.0, 0.1),   // dense group
        (50.0, 50.0), // isolated noise
    ];
    let rtree = build_rtree(&points);
    let opts = DbscanOptions {
        eps: 0.5,
        min_points: 3,
    };
    let result = dbscan_with_rtree(&rtree, &points, &opts);
    assert_eq!(result.num_clusters, 1);
    assert_eq!(result.noise_count, 1);
    assert_eq!(result.labels[3], NOISE);
    // All three dense points belong to the same cluster.
    assert_eq!(result.labels[0], result.labels[1]);
    assert_eq!(result.labels[1], result.labels[2]);
}

// ---------------------------------------------------------------------------
// Additional robustness tests
// ---------------------------------------------------------------------------

#[test]
fn test_dbscan_single_point_min_points_one() {
    // With min_points = 1 a lone point is its own core.
    let points: Vec<(f64, f64)> = vec![(42.0, -7.0)];
    let result = dbscan_rtree(
        &points,
        &DbscanOptions {
            eps: 0.5,
            min_points: 1,
        },
    );
    assert_eq!(result.num_clusters, 1);
    assert_eq!(result.noise_count, 0);
    assert_eq!(result.labels[0], 0);
}

#[test]
fn test_dbscan_border_point_absorbed_into_cluster() {
    // core group: (0,0),(0.1,0),(0,0.1) — all within eps=0.5 of each other
    // border:     (0.4,0) — within eps of (0,0) and (0.1,0) but its own
    //             neighborhood has only 2 points → not a core, but reachable.
    let points: Vec<(f64, f64)> = vec![
        (0.0, 0.0),
        (0.1, 0.0),
        (0.0, 0.1), // core group
        (0.4, 0.0), // border
    ];
    let result = dbscan_rtree(
        &points,
        &DbscanOptions {
            eps: 0.5,
            min_points: 3,
        },
    );
    assert_eq!(
        result.num_clusters, 1,
        "border point should not create a second cluster"
    );
    assert_ne!(
        result.labels[3], NOISE,
        "border point must be absorbed into the cluster"
    );
    assert_eq!(result.noise_count, 0);
}

#[test]
fn test_dbscan_labels_length_matches_points() {
    let points: Vec<(f64, f64)> = vec![(1.0, 2.0), (3.0, 4.0), (5.0, 6.0), (7.0, 8.0), (9.0, 10.0)];
    let result = dbscan_rtree(
        &points,
        &DbscanOptions {
            eps: 0.5,
            min_points: 2,
        },
    );
    assert_eq!(
        result.labels.len(),
        points.len(),
        "labels vector must have same length as points"
    );
}

#[test]
fn test_dbscan_noise_count_consistent_with_labels() {
    let points: Vec<(f64, f64)> = vec![
        (0.0, 0.0),
        (0.1, 0.0),
        (0.0, 0.1),
        (50.0, 50.0),
        (51.0, 51.0),
        (200.0, 200.0),
    ];
    let result = dbscan_rtree(
        &points,
        &DbscanOptions {
            eps: 2.0,
            min_points: 3,
        },
    );
    let manual_noise = result.labels.iter().filter(|&&l| l == NOISE).count();
    assert_eq!(
        result.noise_count, manual_noise,
        "noise_count field must equal the count of NOISE labels"
    );
}
