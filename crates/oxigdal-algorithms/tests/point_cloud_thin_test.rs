//! Integration tests for point-cloud thinning operators.

use oxigdal_algorithms::{
    ThinPoint3, ThinningMethod, ThinningStats, thin_grid, thin_poisson_disk, thin_random,
    thin_with_stats,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Euclidean distance in 3D between two `ThinPoint3`.
fn dist3(a: &ThinPoint3, b: &ThinPoint3) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    let dz = a.z - b.z;
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// Generate `n` points uniformly distributed in the unit cube `[0, 1)^3`
/// via a deterministic LCG (mirrors the LCG used in `point_cloud_thin.rs` so
/// the tests have no `rand` dependency).
fn unit_cube_points(n: usize, seed: u64) -> Vec<ThinPoint3> {
    let mut state = seed.wrapping_add(1);
    let mut step = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        // Take the top 53 bits and map into [0, 1).
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    (0..n)
        .map(|_| ThinPoint3::new(step(), step(), step()))
        .collect()
}

// ---------------------------------------------------------------------------
// Grid thinning
// ---------------------------------------------------------------------------

#[test]
fn test_thin_grid_empty_returns_empty() {
    let pts: Vec<ThinPoint3> = Vec::new();
    let out = thin_grid(&pts, 1.0);
    assert!(out.is_empty());
}

#[test]
fn test_thin_grid_keeps_one_per_voxel() {
    // Five points all in the unit voxel `[0, 1)^3`.
    let pts = vec![
        ThinPoint3::new(0.1, 0.1, 0.1),
        ThinPoint3::new(0.2, 0.2, 0.2),
        ThinPoint3::new(0.3, 0.3, 0.3),
        ThinPoint3::new(0.4, 0.4, 0.4),
        ThinPoint3::new(0.5, 0.5, 0.5),
    ];
    let out = thin_grid(&pts, 1.0);
    assert_eq!(out.len(), 1, "all five points share one voxel");
    // The first input point should be the survivor (input-order preference).
    assert_eq!(out[0], pts[0]);
}

#[test]
fn test_thin_grid_large_cell_keeps_one_total() {
    // All four points have non-negative coordinates well below 1e9, so they
    // collapse into the single voxel (0, 0, 0) under cell_size=1e9 and the
    // first input wins by input-order preference.
    let pts = vec![
        ThinPoint3::new(0.0, 0.0, 0.0),
        ThinPoint3::new(1.0, 2.0, 3.0),
        ThinPoint3::new(4.0, 5.0, 6.0),
        ThinPoint3::new(7.0, 8.0, 9.0),
    ];
    let out = thin_grid(&pts, 1e9);
    assert_eq!(out.len(), 1, "huge cell ⇒ single voxel ⇒ single survivor");
}

#[test]
fn test_thin_grid_distinct_voxels_keeps_all() {
    let pts = vec![
        ThinPoint3::new(0.0, 0.0, 0.0),
        ThinPoint3::new(10.0, 0.0, 0.0),
        ThinPoint3::new(0.0, 10.0, 0.0),
        ThinPoint3::new(0.0, 0.0, 10.0),
    ];
    let out = thin_grid(&pts, 1.0);
    assert_eq!(out.len(), 4, "four points in four distinct unit voxels");
}

// ---------------------------------------------------------------------------
// Random thinning
// ---------------------------------------------------------------------------

#[test]
fn test_thin_random_target_count_respected() {
    let pts: Vec<ThinPoint3> = (0..100)
        .map(|i| ThinPoint3::new(i as f64, 0.0, 0.0))
        .collect();
    let out = thin_random(&pts, 10, 12345);
    assert_eq!(out.len(), 10);
    // Each output point should be one of the originals (subset property).
    for p in &out {
        assert!(pts.contains(p));
    }
}

#[test]
fn test_thin_random_deterministic_same_seed() {
    let pts: Vec<ThinPoint3> = (0..100)
        .map(|i| ThinPoint3::new(i as f64, 0.0, 0.0))
        .collect();
    let a = thin_random(&pts, 20, 0xC0FFEE);
    let b = thin_random(&pts, 20, 0xC0FFEE);
    assert_eq!(a, b, "identical seed ⇒ identical output");
}

#[test]
fn test_thin_random_different_seed_different_result() {
    let pts: Vec<ThinPoint3> = (0..100)
        .map(|i| ThinPoint3::new(i as f64, 0.0, 0.0))
        .collect();
    let a = thin_random(&pts, 10, 1);
    let b = thin_random(&pts, 10, 2);
    // With 100 inputs and 10 outputs and well-spread seeds, the two outputs
    // are overwhelmingly unlikely to match. We assert at least one element
    // differs — that is what the spec demands.
    assert_ne!(a, b);
}

#[test]
fn test_thin_random_target_geq_input_returns_all() {
    let pts: Vec<ThinPoint3> = (0..100)
        .map(|i| ThinPoint3::new(i as f64, 0.0, 0.0))
        .collect();
    let out = thin_random(&pts, 200, 42);
    assert_eq!(out, pts);
}

#[test]
fn test_thin_random_zero_target_returns_empty() {
    let pts: Vec<ThinPoint3> = (0..10)
        .map(|i| ThinPoint3::new(i as f64, 0.0, 0.0))
        .collect();
    let out = thin_random(&pts, 0, 42);
    assert!(out.is_empty());
}

// ---------------------------------------------------------------------------
// Poisson-disk thinning
// ---------------------------------------------------------------------------

#[test]
fn test_thin_poisson_disk_all_kept_pairs_at_least_min_distance() {
    let pts = unit_cube_points(200, 1);
    let min_distance = 0.2;
    let out = thin_poisson_disk(&pts, min_distance, 0xBEEF);
    // Verify the Poisson-disk invariant: every pair of kept points is at
    // least `min_distance` apart in 3D Euclidean distance. Use a small
    // tolerance to absorb floating-point error in the comparison itself.
    let eps = 1e-12;
    for (i, p) in out.iter().enumerate() {
        for q in &out[i + 1..] {
            assert!(
                dist3(p, q) + eps >= min_distance,
                "kept pair too close: dist={} < min={}",
                dist3(p, q),
                min_distance
            );
        }
    }
}

#[test]
fn test_thin_poisson_disk_min_distance_zero_keeps_all() {
    let pts = vec![
        ThinPoint3::new(0.0, 0.0, 0.0),
        ThinPoint3::new(0.0, 0.0, 0.0),
        ThinPoint3::new(0.0, 0.0, 0.0),
    ];
    let out = thin_poisson_disk(&pts, 0.0, 1);
    assert_eq!(out, pts, "min_distance=0 disables the disk constraint");
}

#[test]
fn test_thin_poisson_disk_dense_input_thins_to_subset() {
    let pts = unit_cube_points(100, 7);
    let out = thin_poisson_disk(&pts, 0.3, 99);
    assert!(!out.is_empty(), "at least one point survives");
    assert!(out.len() < pts.len(), "dense input must be thinned");
}

#[test]
fn test_thin_poisson_disk_empty_returns_empty() {
    let pts: Vec<ThinPoint3> = Vec::new();
    let out = thin_poisson_disk(&pts, 1.0, 0);
    assert!(out.is_empty());
}

#[test]
fn test_thin_poisson_disk_deterministic_same_seed() {
    let pts = unit_cube_points(50, 3);
    let a = thin_poisson_disk(&pts, 0.25, 0xABCD);
    let b = thin_poisson_disk(&pts, 0.25, 0xABCD);
    assert_eq!(a, b, "identical seed ⇒ identical output");
}

// ---------------------------------------------------------------------------
// Stats / dispatcher
// ---------------------------------------------------------------------------

#[test]
fn test_thinning_stats_kept_count_correct() {
    let pts: Vec<ThinPoint3> = (0..1000)
        .map(|i| ThinPoint3::new(i as f64, 0.0, 0.0))
        .collect();
    let (out, stats) = thin_with_stats(
        &pts,
        ThinningMethod::Random {
            target_count: 100,
            seed: 11,
        },
    );
    assert_eq!(stats.input_count, 1000);
    assert_eq!(stats.kept_count, out.len());
    assert_eq!(stats.kept_count, 100);
    let expected = 1.0 - 100.0 / 1000.0;
    assert!(
        (stats.reduction_ratio - expected).abs() < 1e-12,
        "reduction_ratio={} expected={}",
        stats.reduction_ratio,
        expected
    );
}

#[test]
fn test_thinning_stats_zero_input_zero_ratio() {
    let pts: Vec<ThinPoint3> = Vec::new();
    let (_out, stats) = thin_with_stats(&pts, ThinningMethod::Grid { cell_size: 1.0 });
    assert_eq!(stats.input_count, 0);
    assert_eq!(stats.kept_count, 0);
    assert_eq!(stats.reduction_ratio, 0.0);
}

#[test]
fn test_thinning_stats_constructor_matches_manual() {
    let s = ThinningStats::new(200, 50);
    assert_eq!(s.input_count, 200);
    assert_eq!(s.kept_count, 50);
    assert!((s.reduction_ratio - 0.75).abs() < 1e-12);
}
