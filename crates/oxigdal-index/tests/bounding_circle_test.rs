//! Integration tests for `bounding_circle` — W5 of OxiGDAL Slice 18.

use std::f64::consts::SQRT_2;

use oxigdal_index::{
    Bbox2D, BoundingCircle, smallest_enclosing_circle, smallest_enclosing_circle_from_bboxes,
};

// ---------------------------------------------------------------------------
// Helper: Knuth MMIX LCG — mirrors the private function in bounding_circle.rs
// so that test 15 can generate reproducible "random" points independently.
// ---------------------------------------------------------------------------

/// Returns the next LCG state.
#[inline]
fn lcg_next(state: u64) -> u64 {
    state
        .wrapping_mul(6_364_136_223_846_793_005_u64)
        .wrapping_add(1_442_695_040_888_963_407_u64)
}

/// Produce `count` points in `[-range, range]²` using the Knuth MMIX LCG.
fn lcg_points(count: usize, seed: u64, range: f64) -> Vec<(f64, f64)> {
    let mut pts = Vec::with_capacity(count);
    let mut state = seed;
    for _ in 0..count {
        // X coordinate: map upper 32 bits to [0.0, 1.0), shift to [-range, range).
        state = lcg_next(state);
        let x = ((state >> 32) as f64 / u32::MAX as f64) * 2.0 * range - range;
        state = lcg_next(state);
        let y = ((state >> 32) as f64 / u32::MAX as f64) * 2.0 * range - range;
        pts.push((x, y));
    }
    pts
}

// ---------------------------------------------------------------------------
// 1. empty() has radius 0
// ---------------------------------------------------------------------------

#[test]
fn test_bounding_circle_empty_radius_zero() {
    let c = BoundingCircle::empty();
    assert_eq!(c.radius, 0.0, "empty circle must have radius 0");
    assert_eq!(c.center_x, 0.0);
    assert_eq!(c.center_y, 0.0);
}

// ---------------------------------------------------------------------------
// 2. from_point has radius 0 at the given point
// ---------------------------------------------------------------------------

#[test]
fn test_circle_from_point_radius_zero() {
    let p = (3.5, -7.2);
    let c = BoundingCircle::from_point(p);
    assert_eq!(c.radius, 0.0);
    assert!((c.center_x - p.0).abs() < f64::EPSILON);
    assert!((c.center_y - p.1).abs() < f64::EPSILON);
}

// ---------------------------------------------------------------------------
// 3. from_two: radius is half the distance, centre is midpoint
// ---------------------------------------------------------------------------

#[test]
fn test_circle_from_two_radius_is_half_distance() {
    let a = (0.0_f64, 0.0_f64);
    let b = (2.0, 0.0);
    let c = BoundingCircle::from_two(a, b);
    assert!(
        (c.radius - 1.0).abs() < 1e-12,
        "radius expected 1.0, got {}",
        c.radius
    );
    assert!((c.center_x - 1.0).abs() < 1e-12);
    assert!((c.center_y).abs() < 1e-12);
}

// ---------------------------------------------------------------------------
// 4. from_three: equilateral triangle circumcenter and circumradius
// ---------------------------------------------------------------------------

#[test]
fn test_circle_from_three_equilateral_circumcenter_at_centroid() {
    // Equilateral triangle of side 2, base on x-axis, apex above.
    let a = (0.0_f64, 0.0_f64);
    let b = (2.0, 0.0);
    let c_pt = (1.0, 3.0_f64.sqrt());

    let circle = BoundingCircle::from_three(a, b, c_pt)
        .expect("non-collinear equilateral triangle must have circumcircle");

    // Circumradius of equilateral triangle with side s is s / sqrt(3).
    let expected_r = 2.0_f64 / 3.0_f64.sqrt();
    assert!(
        (circle.radius - expected_r).abs() < 1e-9,
        "circumradius expected {expected_r}, got {}",
        circle.radius
    );

    // Circumcenter of equilateral triangle (0,0)-(2,0)-(1,√3) is at (1, 1/√3).
    let expected_cx = 1.0_f64;
    let expected_cy = 1.0_f64 / 3.0_f64.sqrt();
    assert!(
        (circle.center_x - expected_cx).abs() < 1e-9,
        "center_x expected {expected_cx}, got {}",
        circle.center_x
    );
    assert!(
        (circle.center_y - expected_cy).abs() < 1e-9,
        "center_y expected {expected_cy}, got {}",
        circle.center_y
    );
}

// ---------------------------------------------------------------------------
// 5. from_three: collinear returns None
// ---------------------------------------------------------------------------

#[test]
fn test_circle_from_three_collinear_returns_none() {
    let result = BoundingCircle::from_three((0.0, 0.0), (1.0, 0.0), (2.0, 0.0));
    assert!(
        result.is_none(),
        "collinear points must yield None, not {:?}",
        result
    );
}

// ---------------------------------------------------------------------------
// 6. Any circle contains its own centre
// ---------------------------------------------------------------------------

#[test]
fn test_circle_contains_center() {
    let c = BoundingCircle {
        center_x: 5.0,
        center_y: -3.0,
        radius: 7.5,
    };
    // Both the epsilon-tolerant and the strict forms must pass for the centre.
    assert!(c.contains_point((c.center_x, c.center_y)));
    assert!(c.contains_point_strict((c.center_x, c.center_y)));
}

// ---------------------------------------------------------------------------
// 7. A point exactly at radius is inside (with epsilon)
// ---------------------------------------------------------------------------

#[test]
fn test_circle_contains_boundary_point_within_epsilon() {
    let c = BoundingCircle {
        center_x: 0.0,
        center_y: 0.0,
        radius: 1.0,
    };
    // Exactly on the boundary — must be included by contains_point.
    assert!(c.contains_point((1.0, 0.0)));
    assert!(c.contains_point((0.0, 1.0)));
    assert!(c.contains_point((-1.0, 0.0)));
    assert!(c.contains_point((0.0, -1.0)));
}

// ---------------------------------------------------------------------------
// 8. A point clearly outside is not contained
// ---------------------------------------------------------------------------

#[test]
fn test_circle_does_not_contain_far_point() {
    let c = BoundingCircle {
        center_x: 0.0,
        center_y: 0.0,
        radius: 1.0,
    };
    // Well beyond radius + epsilon.
    assert!(!c.contains_point((2.0, 0.0)));
    assert!(!c.contains_point((0.0, 100.0)));
}

// ---------------------------------------------------------------------------
// 9. Circle intersects overlapping bbox
// ---------------------------------------------------------------------------

#[test]
fn test_circle_intersects_bbox_when_overlapping() {
    let c = BoundingCircle {
        center_x: 0.0,
        center_y: 0.0,
        radius: 2.0,
    };
    // Bbox overlaps the circle.
    let bbox = Bbox2D::new(1.0, 1.0, 3.0, 3.0).unwrap();
    assert!(c.intersects_bbox(&bbox));

    // Centre inside the bbox.
    let bbox_wrap = Bbox2D::new(-1.0, -1.0, 1.0, 1.0).unwrap();
    assert!(c.intersects_bbox(&bbox_wrap));
}

// ---------------------------------------------------------------------------
// 10. Circle does NOT intersect disjoint bbox
// ---------------------------------------------------------------------------

#[test]
fn test_circle_no_intersect_bbox_when_disjoint() {
    let c = BoundingCircle {
        center_x: 0.0,
        center_y: 0.0,
        radius: 1.0,
    };
    // Bbox whose nearest corner is at distance 5 (>> radius).
    let bbox = Bbox2D::new(6.0, 6.0, 8.0, 8.0).unwrap();
    assert!(!c.intersects_bbox(&bbox));

    // Bbox along the x-axis, far away.
    let bbox2 = Bbox2D::new(2.0, -0.5, 5.0, 0.5).unwrap();
    assert!(!c.intersects_bbox(&bbox2));
}

// ---------------------------------------------------------------------------
// 11. smallest_enclosing_circle on empty input → empty circle
// ---------------------------------------------------------------------------

#[test]
fn test_smallest_enclosing_circle_empty_input() {
    let c = smallest_enclosing_circle(&[]);
    assert_eq!(c.radius, 0.0);
    assert_eq!(c.center_x, 0.0);
    assert_eq!(c.center_y, 0.0);
}

// ---------------------------------------------------------------------------
// 12. Single point → radius 0 at that point
// ---------------------------------------------------------------------------

#[test]
fn test_smallest_enclosing_circle_single_point() {
    let p = (-4.0, 9.0);
    let c = smallest_enclosing_circle(&[p]);
    assert_eq!(c.radius, 0.0);
    assert!((c.center_x - p.0).abs() < f64::EPSILON);
    assert!((c.center_y - p.1).abs() < f64::EPSILON);
}

// ---------------------------------------------------------------------------
// 13. Two points → same as from_two
// ---------------------------------------------------------------------------

#[test]
fn test_smallest_enclosing_circle_two_points() {
    let a = (-1.0, 0.0);
    let b = (1.0, 0.0);
    let c = smallest_enclosing_circle(&[a, b]);
    let expected = BoundingCircle::from_two(a, b);
    assert!(
        (c.radius - expected.radius).abs() < 1e-10,
        "radius mismatch: {} vs {}",
        c.radius,
        expected.radius
    );
    assert!((c.center_x - expected.center_x).abs() < 1e-10);
    assert!((c.center_y - expected.center_y).abs() < 1e-10);
}

// ---------------------------------------------------------------------------
// 14. Unit square corners → radius √2 ≈ 1.4142…
// ---------------------------------------------------------------------------

#[test]
fn test_smallest_enclosing_circle_unit_square_corners() {
    // Corners at (±1, ±1); the enclosing circle has centre (0,0) and radius √2.
    let pts = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
    let c = smallest_enclosing_circle(&pts);

    assert!(
        (c.radius - SQRT_2).abs() < 1e-9,
        "expected radius √2 ≈ {SQRT_2}, got {}",
        c.radius
    );
    assert!(
        c.center_x.abs() < 1e-9,
        "center_x should be 0, got {}",
        c.center_x
    );
    assert!(
        c.center_y.abs() < 1e-9,
        "center_y should be 0, got {}",
        c.center_y
    );
}

// ---------------------------------------------------------------------------
// 15. 100 LCG-generated points — all must be within radius + 1e-9
// ---------------------------------------------------------------------------

#[test]
fn test_smallest_enclosing_circle_contains_all_100_random_points() {
    let pts = lcg_points(100, 0xDEAD_BEEF_1234_5678_u64, 50.0);
    let c = smallest_enclosing_circle(&pts);

    let tol = 1e-9;
    for (i, &p) in pts.iter().enumerate() {
        let dx = p.0 - c.center_x;
        let dy = p.1 - c.center_y;
        let dist = (dx * dx + dy * dy).sqrt();
        assert!(
            dist <= c.radius + tol,
            "point {i} ({}, {}) is outside the enclosing circle \
             (dist={dist}, radius={}, excess={})",
            p.0,
            p.1,
            c.radius,
            dist - c.radius
        );
    }
}

// ---------------------------------------------------------------------------
// Bonus: smallest_enclosing_circle_from_bboxes
// ---------------------------------------------------------------------------

#[test]
fn test_smallest_enclosing_circle_from_bboxes_contains_all_corners() {
    let bboxes = vec![
        Bbox2D::new(0.0, 0.0, 2.0, 2.0).unwrap(),
        Bbox2D::new(-3.0, -1.0, -1.0, 1.0).unwrap(),
        Bbox2D::new(1.0, -4.0, 3.0, -2.0).unwrap(),
    ];

    let c = smallest_enclosing_circle_from_bboxes(&bboxes);
    let tol = 1e-9;

    for bb in &bboxes {
        for &corner in &[
            (bb.min_x, bb.min_y),
            (bb.max_x, bb.min_y),
            (bb.max_x, bb.max_y),
            (bb.min_x, bb.max_y),
        ] {
            let dx = corner.0 - c.center_x;
            let dy = corner.1 - c.center_y;
            let dist = (dx * dx + dy * dy).sqrt();
            assert!(
                dist <= c.radius + tol,
                "corner {:?} is outside the enclosing circle \
                 (dist={dist}, radius={}, excess={})",
                corner,
                c.radius,
                dist - c.radius
            );
        }
    }
}
