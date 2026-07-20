//! Integration tests for the Bentley-Ottmann sweep-line intersection algorithm.
//!
//! These tests cover:
//! * Degenerate inputs (empty, single, parallel).
//! * Simple geometric cases (X-cross, T-junction).
//! * Special segment orientations (vertical, horizontal).
//! * Dense synthetic grids.
//! * Correctness properties (no duplicates, ordering guarantee).
//! * Agreement with O(n²) brute-force on a 50-segment stress case.

use oxigeo_index::{IntersectionPoint, Segment, find_all_intersections};

// ─────────────────────────────────────────────────────────────────────────────
// Helper: brute-force O(n²) reference implementation
// ─────────────────────────────────────────────────────────────────────────────

/// Classic parametric segment intersection (same maths as the internal helper
/// but fully self-contained so tests don't depend on private APIs).
fn brute_intersect(
    (ax, ay): (f64, f64),
    (bx, by): (f64, f64),
    (cx, cy): (f64, f64),
    (dx, dy): (f64, f64),
) -> Option<(f64, f64)> {
    let r_x = bx - ax;
    let r_y = by - ay;
    let s_x = dx - cx;
    let s_y = dy - cy;
    let det = r_x * s_y - r_y * s_x;
    if det.abs() < 1e-10 {
        return None;
    }
    let q_x = cx - ax;
    let q_y = cy - ay;
    let t = (q_x * s_y - q_y * s_x) / det;
    let u = (q_x * r_y - q_y * r_x) / det;
    let eps = 1e-10;
    if t >= -eps && t <= 1.0 + eps && u >= -eps && u <= 1.0 + eps {
        Some((ax + t * r_x, ay + t * r_y))
    } else {
        None
    }
}

/// All pairwise intersecting pairs using brute-force O(n²).
/// Returns a set of `(min_idx, max_idx)` pairs (using the `idx` field).
fn brute_force_pairs(segments: &[Segment]) -> std::collections::HashSet<(usize, usize)> {
    let mut pairs = std::collections::HashSet::new();
    for i in 0..segments.len() {
        for j in (i + 1)..segments.len() {
            let sa = &segments[i];
            let sb = &segments[j];
            if brute_intersect(sa.p0, sa.p1, sb.p0, sb.p1).is_some() {
                let a = sa.idx.min(sb.idx);
                let b = sa.idx.max(sb.idx);
                pairs.insert((a, b));
            }
        }
    }
    pairs
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper: deterministic pseudo-random number generator (linear congruential)
// ─────────────────────────────────────────────────────────────────────────────

/// Generate the n-th value from a simple LCG with Knuth multiplier.
/// Returns a value in [0, 1).
fn lcg_f64(n: u64) -> f64 {
    // Standard Knuth LCG parameters (64-bit):
    //   a = 6364136223846793005, c = 1442695040888963407
    let mut x: u64 = 0;
    for _ in 0..=n {
        x = x
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
    }
    x as f64 / u64::MAX as f64
}

/// Build a vector of `count` pseudo-random segments whose coordinates lie in
/// [0, scale) using the LCG above.  Each segment gets `idx = i`.
fn random_segments(count: usize, scale: f64, seed_offset: u64) -> Vec<Segment> {
    let mut segs = Vec::with_capacity(count);
    for i in 0..count {
        let base = seed_offset + (i as u64) * 4;
        let x0 = lcg_f64(base) * scale;
        let y0 = lcg_f64(base + 1) * scale;
        let x1 = lcg_f64(base + 2) * scale;
        let y1 = lcg_f64(base + 3) * scale;
        segs.push(Segment::new(i, (x0, y0), (x1, y1)));
    }
    segs
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_sweep_empty_input_returns_empty() {
    let result = find_all_intersections(&[]);
    assert!(
        result.is_empty(),
        "Expected no intersections for empty input, got {result:?}"
    );
}

#[test]
fn test_sweep_two_parallel_lines_no_intersection() {
    // Two horizontal lines at y=0 and y=1, both spanning x=[0,10].
    let segments = vec![
        Segment::new(0, (0.0, 0.0), (10.0, 0.0)),
        Segment::new(1, (0.0, 1.0), (10.0, 1.0)),
    ];
    let result = find_all_intersections(&segments);
    assert!(
        result.is_empty(),
        "Parallel lines should not intersect, got: {result:?}"
    );
}

#[test]
fn test_sweep_two_crossing_lines_finds_one_point() {
    // X shape: diagonal segments crossing at (0, 0).
    // Segment 0: from (-1, -1) to (1, 1).
    // Segment 1: from (-1,  1) to (1, -1).
    let segments = vec![
        Segment::new(0, (-1.0, -1.0), (1.0, 1.0)),
        Segment::new(1, (-1.0, 1.0), (1.0, -1.0)),
    ];
    let result = find_all_intersections(&segments);
    assert_eq!(
        result.len(),
        1,
        "Expected exactly 1 intersection, got {result:?}"
    );

    let ip = &result[0];
    assert_eq!(ip.seg_a, 0, "seg_a should be 0 (smaller index)");
    assert_eq!(ip.seg_b, 1, "seg_b should be 1 (larger index)");
    assert!(
        ip.x.abs() < 1e-9 && ip.y.abs() < 1e-9,
        "Intersection should be near (0,0), got ({}, {})",
        ip.x,
        ip.y
    );
}

#[test]
fn test_sweep_t_junction_finds_endpoint_intersection() {
    // Segment 0: horizontal from (-2, 0) to (2, 0).
    // Segment 1: vertical from (0, 0) to (0, 2) — endpoint touches segment 0.
    let segments = vec![
        Segment::new(0, (-2.0, 0.0), (2.0, 0.0)),
        Segment::new(1, (0.0, 0.0), (0.0, 2.0)),
    ];
    let result = find_all_intersections(&segments);
    assert!(
        !result.is_empty(),
        "T-junction endpoint touch should be detected"
    );
    // There should be exactly one intersection.
    assert_eq!(
        result.len(),
        1,
        "Expected exactly 1 intersection for T-junction, got {result:?}"
    );
    let ip = &result[0];
    assert!(
        ip.x.abs() < 1e-9 && ip.y.abs() < 1e-9,
        "Intersection should be near (0,0)"
    );
}

#[test]
fn test_sweep_vertical_segments_handled() {
    // Vertical segment crossing a horizontal segment — must not panic.
    // Segment 0: horizontal from (0, 5) to (10, 5).
    // Segment 1: vertical   from (5, 0) to (5, 10).
    let segments = vec![
        Segment::new(0, (0.0, 5.0), (10.0, 5.0)),
        Segment::new(1, (5.0, 0.0), (5.0, 10.0)),
    ];
    let result = find_all_intersections(&segments);
    assert_eq!(
        result.len(),
        1,
        "Vertical ∩ horizontal should produce 1 intersection, got {result:?}"
    );
    let ip = &result[0];
    assert!(
        (ip.x - 5.0).abs() < 1e-9 && (ip.y - 5.0).abs() < 1e-9,
        "Intersection should be near (5, 5), got ({}, {})",
        ip.x,
        ip.y
    );
}

#[test]
fn test_sweep_horizontal_segments_handled() {
    // Two horizontal segments at the same y but non-overlapping — no intersection.
    let segments = vec![
        Segment::new(0, (0.0, 0.0), (3.0, 0.0)),
        Segment::new(1, (5.0, 0.0), (8.0, 0.0)),
    ];
    let result = find_all_intersections(&segments);
    assert!(
        result.is_empty(),
        "Non-overlapping collinear segments should not intersect, got {result:?}"
    );
}

#[test]
fn test_sweep_dense_grid_finds_intersections() {
    // 4 horizontal segments × 4 vertical segments → 16 intersections.
    //
    // Horizontal: y = 1, 2, 3, 4 spanning x = [0, 5].
    // Vertical:   x = 1, 2, 3, 4 spanning y = [0, 5].
    let mut segments: Vec<Segment> = Vec::new();

    // Horizontal segments: idx 0..3.
    for (i, &y) in [1.0f64, 2.0, 3.0, 4.0].iter().enumerate() {
        segments.push(Segment::new(i, (0.0, y), (5.0, y)));
    }
    // Vertical segments: idx 4..7.
    for (j, &x) in [1.0f64, 2.0, 3.0, 4.0].iter().enumerate() {
        segments.push(Segment::new(j + 4, (x, 0.0), (x, 5.0)));
    }

    let result = find_all_intersections(&segments);

    assert_eq!(
        result.len(),
        16,
        "4×4 grid should produce 16 intersections, got {}: {result:?}",
        result.len()
    );
}

#[test]
fn test_sweep_reports_each_pair_exactly_once_with_a_lt_b() {
    // Use the 4×4 grid and verify every result has seg_a < seg_b and no dups.
    let mut segments: Vec<Segment> = Vec::new();
    for (i, &y) in [1.0f64, 2.0, 3.0, 4.0].iter().enumerate() {
        segments.push(Segment::new(i, (0.0, y), (5.0, y)));
    }
    for (j, &x) in [1.0f64, 2.0, 3.0, 4.0].iter().enumerate() {
        segments.push(Segment::new(j + 4, (x, 0.0), (x, 5.0)));
    }

    let result = find_all_intersections(&segments);

    // Collect all reported pairs.
    let mut pairs = std::collections::HashSet::new();
    for ip in &result {
        assert!(
            ip.seg_a < ip.seg_b,
            "seg_a ({}) must be strictly less than seg_b ({})",
            ip.seg_a,
            ip.seg_b
        );
        let inserted = pairs.insert((ip.seg_a, ip.seg_b));
        assert!(
            inserted,
            "Duplicate intersection pair ({}, {}) reported",
            ip.seg_a, ip.seg_b
        );
    }
}

#[test]
fn test_sweep_matches_brute_force_on_random_50_segments() {
    // Generate 50 deterministic segments via a simple LCG.
    // Compare the set of intersecting pairs produced by sweep vs brute-force.
    let segments = random_segments(50, 1.0, 0);

    let sweep_result = find_all_intersections(&segments);
    let brute_result = brute_force_pairs(&segments);

    // Build set of pairs reported by sweep.
    let sweep_pairs: std::collections::HashSet<(usize, usize)> = sweep_result
        .iter()
        .map(|ip: &IntersectionPoint| (ip.seg_a, ip.seg_b))
        .collect();

    // Every brute-force pair must appear in sweep output.
    for &(a, b) in &brute_result {
        assert!(
            sweep_pairs.contains(&(a, b)),
            "Sweep missed intersection pair ({a}, {b}) found by brute-force"
        );
    }

    // Every sweep pair must appear in brute-force output.
    for &(a, b) in &sweep_pairs {
        assert!(
            brute_result.contains(&(a, b)),
            "Sweep reported spurious intersection pair ({a}, {b}) not found by brute-force"
        );
    }
}
