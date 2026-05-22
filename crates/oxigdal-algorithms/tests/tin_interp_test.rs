//! Integration tests for TIN (Triangulated Irregular Network) interpolation.

use oxigdal_algorithms::{
    Tin, TinInterpMethod, TinPoint, build_tin, interpolate_idw_tin, interpolate_natural_neighbor,
    rasterize_tin,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A simple non-degenerate triangle covering `[0, 1] x [0, 1]`.
fn simple_triangle_points() -> Vec<TinPoint> {
    vec![
        TinPoint::new(0.0, 0.0, 0.0),
        TinPoint::new(1.0, 0.0, 10.0),
        TinPoint::new(0.5, 1.0, 5.0),
    ]
}

/// A four-point square split into two triangles, with corner heights that
/// give us a wider zoo of interior queries to play with.
fn quad_points() -> Vec<TinPoint> {
    vec![
        TinPoint::new(0.0, 0.0, 1.0),
        TinPoint::new(1.0, 0.0, 2.0),
        TinPoint::new(1.0, 1.0, 3.0),
        TinPoint::new(0.0, 1.0, 4.0),
    ]
}

/// Return `(min_z, max_z)` of a TIN's input samples.
fn z_range(tin: &Tin) -> (f64, f64) {
    let mut mn = f64::INFINITY;
    let mut mx = f64::NEG_INFINITY;
    for p in &tin.points {
        if p.z < mn {
            mn = p.z;
        }
        if p.z > mx {
            mx = p.z;
        }
    }
    (mn, mx)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_build_tin_three_points_one_triangle() {
    let tin = build_tin(&simple_triangle_points()).expect("build TIN");
    assert_eq!(
        tin.triangle_count(),
        1,
        "three CCW points must form 1 triangle"
    );
    assert_eq!(tin.point_count(), 3);
}

#[test]
fn test_build_tin_too_few_points_returns_empty() {
    let empty = build_tin(&[]).expect("empty input ok");
    assert_eq!(empty.triangle_count(), 0);
    assert_eq!(empty.point_count(), 0);

    let single = build_tin(&[TinPoint::new(0.0, 0.0, 1.0)]).expect("single point ok");
    assert_eq!(single.triangle_count(), 0);
    assert_eq!(single.point_count(), 1);

    let pair = build_tin(&[TinPoint::new(0.0, 0.0, 1.0), TinPoint::new(1.0, 0.0, 2.0)])
        .expect("two points ok");
    assert_eq!(pair.triangle_count(), 0);
    assert_eq!(pair.point_count(), 2);
}

#[test]
fn test_idw_at_vertex_returns_vertex_z() {
    let tin = build_tin(&simple_triangle_points()).expect("build TIN");

    // Each input vertex must reproduce its own z.
    for p in &tin.points {
        let z = interpolate_idw_tin(&tin, p.x, p.y, 2.0).expect("vertex must lie inside hull");
        assert!(
            (z - p.z).abs() < 1e-9,
            "IDW at vertex ({}, {}) expected {} got {}",
            p.x,
            p.y,
            p.z,
            z
        );
    }
}

#[test]
fn test_idw_inside_triangle_returns_finite_within_range() {
    let tin = build_tin(&simple_triangle_points()).expect("build TIN");
    let (mn, mx) = z_range(&tin);

    // A safely interior query (centroid).
    let qx = (0.0 + 1.0 + 0.5) / 3.0;
    let qy = (0.0 + 0.0 + 1.0) / 3.0;
    let z = interpolate_idw_tin(&tin, qx, qy, 2.0).expect("centroid inside hull");

    assert!(z.is_finite(), "IDW must return a finite value");
    assert!(
        z >= mn - 1e-9 && z <= mx + 1e-9,
        "IDW result {} must lie within [{mn}, {mx}]",
        z
    );
}

#[test]
fn test_idw_outside_hull_returns_none() {
    let tin = build_tin(&simple_triangle_points()).expect("build TIN");
    assert!(interpolate_idw_tin(&tin, 100.0, 100.0, 2.0).is_none());
    assert!(interpolate_idw_tin(&tin, -50.0, -50.0, 2.0).is_none());
}

#[test]
fn test_idw_power_2_falls_off_quadratically() {
    // Triangle with one vertex high and the other two low; queries near the
    // high vertex should be more strongly attracted to it under higher
    // `power` exponents.
    let pts = vec![
        TinPoint::new(0.0, 0.0, 100.0), // dominant high vertex
        TinPoint::new(10.0, 0.0, 0.0),
        TinPoint::new(5.0, 10.0, 0.0),
    ];
    let tin = build_tin(&pts).expect("build TIN");

    // Query close to the high vertex.
    let qx = 0.5;
    let qy = 0.5;
    let z_p1 = interpolate_idw_tin(&tin, qx, qy, 1.0).expect("inside hull");
    let z_p2 = interpolate_idw_tin(&tin, qx, qy, 2.0).expect("inside hull");
    let z_p4 = interpolate_idw_tin(&tin, qx, qy, 4.0).expect("inside hull");

    // Stronger falloff (larger power) should drag the result closer to 100.
    assert!(
        z_p4 > z_p2,
        "power=4 ({z_p4}) should give more weight to nearest vertex than power=2 ({z_p2})"
    );
    assert!(
        z_p2 > z_p1,
        "power=2 ({z_p2}) should give more weight to nearest vertex than power=1 ({z_p1})"
    );
    // All values are bounded above by the dominant vertex's z.
    assert!(z_p4 < 100.0);
}

#[test]
fn test_natural_neighbor_at_vertex_returns_vertex_z() {
    let tin = build_tin(&simple_triangle_points()).expect("build TIN");
    for p in &tin.points {
        let z = interpolate_natural_neighbor(&tin, p.x, p.y).expect("vertex must lie inside hull");
        assert!(
            (z - p.z).abs() < 1e-9,
            "NN at vertex ({}, {}) expected {} got {}",
            p.x,
            p.y,
            p.z,
            z
        );
    }
}

#[test]
fn test_natural_neighbor_inside_triangle_returns_finite() {
    let tin = build_tin(&simple_triangle_points()).expect("build TIN");
    let (mn, mx) = z_range(&tin);

    let qx = (0.0 + 1.0 + 0.5) / 3.0;
    let qy = (0.0 + 0.0 + 1.0) / 3.0;
    let z = interpolate_natural_neighbor(&tin, qx, qy).expect("centroid inside hull");

    assert!(z.is_finite(), "NN must return a finite value");
    assert!(
        z >= mn - 1e-9 && z <= mx + 1e-9,
        "NN result {} must lie within [{mn}, {mx}]",
        z
    );
}

#[test]
fn test_rasterize_tin_idw_returns_correct_dims() {
    let tin = build_tin(&quad_points()).expect("build TIN");
    let width = 16;
    let height = 9;
    let grid = rasterize_tin(
        &tin,
        0.0,
        0.0,
        1.0,
        1.0,
        width,
        height,
        TinInterpMethod::Idw { power: 2.0 },
    );
    assert_eq!(grid.len(), width * height);

    // At least one interior pixel should produce a finite value.
    assert!(grid.iter().any(|v| v.is_finite()));
}

#[test]
fn test_rasterize_tin_natural_neighbor_returns_correct_dims() {
    let tin = build_tin(&quad_points()).expect("build TIN");
    let width = 12;
    let height = 7;
    let grid = rasterize_tin(
        &tin,
        0.0,
        0.0,
        1.0,
        1.0,
        width,
        height,
        TinInterpMethod::NaturalNeighbor,
    );
    assert_eq!(grid.len(), width * height);
    assert!(grid.iter().any(|v| v.is_finite()));
}

#[test]
fn test_tin_interp_method_dispatch_returns_finite() {
    let tin = build_tin(&quad_points()).expect("build TIN");
    let bbox = tin.bounding_box().expect("non-empty");

    // Same grid, both methods — both must produce some finite values
    // somewhere in the buffer.
    let g_idw = rasterize_tin(
        &tin,
        bbox.0,
        bbox.1,
        bbox.2,
        bbox.3,
        8,
        8,
        TinInterpMethod::Idw { power: 2.0 },
    );
    let g_nn = rasterize_tin(
        &tin,
        bbox.0,
        bbox.1,
        bbox.2,
        bbox.3,
        8,
        8,
        TinInterpMethod::NaturalNeighbor,
    );
    assert_eq!(g_idw.len(), 64);
    assert_eq!(g_nn.len(), 64);
    assert!(g_idw.iter().any(|v| v.is_finite()));
    assert!(g_nn.iter().any(|v| v.is_finite()));
}

#[test]
fn test_tin_planar_input_constant_z_returns_constant_everywhere() {
    // Every input has z = 42; a TIN-interpolated query inside the hull must
    // also return 42 within numerical tolerance, regardless of method.
    let pts = vec![
        TinPoint::new(0.0, 0.0, 42.0),
        TinPoint::new(2.0, 0.0, 42.0),
        TinPoint::new(2.0, 2.0, 42.0),
        TinPoint::new(0.0, 2.0, 42.0),
        TinPoint::new(1.0, 1.0, 42.0),
    ];
    let tin = build_tin(&pts).expect("build TIN");

    // A scatter of interior queries.
    let queries = [
        (0.25, 0.25),
        (1.5, 0.5),
        (0.5, 1.5),
        (1.0, 1.0),
        (0.75, 0.75),
        (1.25, 1.25),
    ];

    let eps = 1e-6;
    for (qx, qy) in queries {
        let z_nn = interpolate_natural_neighbor(&tin, qx, qy).expect("query inside hull");
        assert!(
            (z_nn - 42.0).abs() < eps,
            "NN at ({qx}, {qy}) expected 42 got {z_nn}"
        );

        let z_idw = interpolate_idw_tin(&tin, qx, qy, 2.0).expect("query inside hull");
        assert!(
            (z_idw - 42.0).abs() < eps,
            "IDW at ({qx}, {qy}) expected 42 got {z_idw}"
        );
    }

    // Rasterised values must also be 42 wherever finite.
    let grid = rasterize_tin(
        &tin,
        0.0,
        0.0,
        2.0,
        2.0,
        8,
        8,
        TinInterpMethod::NaturalNeighbor,
    );
    for v in &grid {
        if v.is_finite() {
            assert!(
                ((*v as f64) - 42.0).abs() < 1e-5,
                "rasterised value {v} must equal 42 for planar input"
            );
        }
    }
}
