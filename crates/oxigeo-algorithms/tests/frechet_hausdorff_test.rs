#![allow(missing_docs)]

use oxigeo_algorithms::{
    directed_hausdorff, discrete_frechet_distance, hausdorff_distance,
    hausdorff_distance_to_segments,
};

#[test]
fn test_frechet_identical_curves_returns_zero() {
    let curve: Vec<(f64, f64)> = (0..5).map(|i| (i as f64, (i as f64).powi(2))).collect();
    let result = discrete_frechet_distance(&curve, &curve);
    assert!(
        result.abs() < 1e-10,
        "identical curves: expected 0, got {result}"
    );
}

#[test]
fn test_frechet_two_shifted_lines() {
    let a = [(0.0_f64, 0.0_f64), (1.0, 0.0), (2.0, 0.0)];
    let b = [(0.0_f64, 1.0_f64), (1.0, 1.0), (2.0, 1.0)];
    let result = discrete_frechet_distance(&a, &b);
    assert!(
        (result - 1.0).abs() < 1e-10,
        "parallel lines 1 unit apart: expected 1.0, got {result}"
    );
}

#[test]
fn test_frechet_zigzag_vs_straight() {
    let a = [
        (0.0_f64, 0.0_f64),
        (1.0, 1.0),
        (2.0, 0.0),
        (3.0, 1.0),
        (4.0, 0.0),
    ];
    let b = [(0.0_f64, 0.0_f64), (4.0, 0.0)];
    let result = discrete_frechet_distance(&a, &b);
    assert!(
        result >= 1.0,
        "zigzag vs straight: leash must reach zigzag peaks (≥ 1.0), got {result}"
    );
}

#[test]
fn test_frechet_empty_input_returns_zero() {
    let empty: &[(f64, f64)] = &[];
    let nonempty = [(0.0_f64, 0.0_f64), (1.0, 0.0)];
    assert_eq!(discrete_frechet_distance(empty, empty), 0.0);
    assert_eq!(discrete_frechet_distance(empty, &nonempty), 0.0);
    assert_eq!(discrete_frechet_distance(&nonempty, empty), 0.0);
}

#[test]
fn test_hausdorff_identical_sets_returns_zero() {
    let pts = [(0.0_f64, 0.0_f64), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
    let result = hausdorff_distance(&pts, &pts);
    assert!(
        result.abs() < 1e-10,
        "identical sets: expected 0, got {result}"
    );
}

#[test]
fn test_hausdorff_point_and_unit_square() {
    let center = [(0.5_f64, 0.5_f64)];
    let square = [(0.0_f64, 0.0_f64), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
    // directed from center to square: min distance from (0.5,0.5) to nearest corner
    // = sqrt((0.5)^2 + (0.5)^2) = sqrt(0.5)
    let expected = (0.25_f64 + 0.25).sqrt();
    let result = directed_hausdorff(&center, &square);
    assert!(
        (result - expected).abs() < 1e-9,
        "center to unit-square corners: expected {expected:.10}, got {result:.10}"
    );
}

#[test]
fn test_hausdorff_asymmetry_of_directed() {
    let a = [(0.0_f64, 0.0_f64)];
    let b = [(0.0_f64, 0.0_f64), (1.0, 0.0), (2.0, 0.0)];

    let d_ab = directed_hausdorff(&a, &b);
    // A's single point sits at B's first vertex — nearest distance = 0
    assert!(d_ab.abs() < 1e-10, "directed A→B: expected 0.0, got {d_ab}");

    let d_ba = directed_hausdorff(&b, &a);
    // B's last point (2,0) is 2 units from A's only point (0,0)
    assert!(
        (d_ba - 2.0).abs() < 1e-10,
        "directed B→A: expected 2.0, got {d_ba}"
    );

    let h = hausdorff_distance(&a, &b);
    assert!(
        (h - 2.0).abs() < 1e-10,
        "symmetric Hausdorff: expected 2.0, got {h}"
    );
}

#[test]
fn test_hausdorff_to_segments_closer_than_vertex_hausdorff() {
    // A = short horizontal line centred on x-axis, B = single long segment at y=1.
    //
    // A = [(0,0),(0.01,0)]  — effectively a point near origin.
    // B = [(-1,1),(1,1)]    — horizontal segment at y=1.
    //
    // Vertex Hausdorff (A↔B):
    //   directed A→B: min distances from A vertices to B vertices:
    //     (0,0)    → nearest B vertex (-1,1) or (1,1): both distance sqrt(2) ≈ 1.414
    //     (0.01,0) → nearest B vertex: sqrt((1.01)^2+1) ≈ 1.422 or sqrt(0.99^2+1) ≈ 1.408
    //   directed A→B max = sqrt(2) ≈ 1.414
    //   directed B→A: B vertices to A vertices:
    //     (-1,1) → nearest A vertex (0,0): sqrt(2) or (0.01,0): sqrt(1.01^2+1) ≈ 1.420
    //     (1,1)  → nearest A vertex (0,0): sqrt(2) or (0.01,0): sqrt(0.99^2+1) ≈ 1.408
    //   both ≈ sqrt(2), so symmetric vertex Hausdorff ≈ sqrt(2) ≈ 1.414
    //
    // Segment Hausdorff (A↔B):
    //   directed A→B (A points to segments of B):
    //     The single segment of B is [(-1,1),(1,1)].
    //     (0,0)    → segment perpendicular projection → (0,1), dist = 1.0
    //     (0.01,0) → projection → (0.01,1), dist = 1.0
    //     max = 1.0
    //   directed B→A (B points to segments of A):
    //     The single segment of A is [(0,0),(0.01,0)].
    //     (-1,1) → nearest point on A segment: t=clamp((-1*0.01+1*0)/(0.0001),0,1)=0
    //              → (0,0), dist = sqrt(2) ≈ 1.414
    //     (1,1)  → t=clamp((1*0.01)/(0.0001),0,1)=1 → (0.01,0), dist = sqrt(0.99^2+1) ≈ 1.408
    //   max of B→A ≈ sqrt(2)
    //
    //   symmetric segment Hausdorff = max(1.0, sqrt(2)) ≈ sqrt(2)
    //
    // This shows directed A→B with segments < directed A→B with vertices.
    // Use directed Hausdorff to isolate the one direction where segment wins.
    let a = [(0.0_f64, 0.0_f64), (0.01_f64, 0.0_f64)];
    let b = [(-1.0_f64, 1.0_f64), (1.0_f64, 1.0_f64)];

    // directed vertex: A's points to B's nearest vertex
    let _dir_vertex = directed_hausdorff(&a, &b);

    // directed segment: A's points to B's nearest segment point
    // Use the helper indirectly: symmetric minus the reverse direction
    // Instead, verify the symmetric variant inequality in a scenario where
    // A has dense points along x=0 and B has a sparse long edge.
    //
    // Rebuild the example so A contains the midpoint of B's span and the advantage
    // is clear in the symmetric result.  A = 5 evenly-spaced points on y=0 from -1 to 1.
    // B = [(-1,1),(1,1)].  Every point of A projects perpendicularly onto B at y=1.
    let a2: Vec<(f64, f64)> = (0..5).map(|i| (-1.0 + i as f64 * 0.5, 0.0)).collect();
    let b2 = [(-1.0_f64, 1.0_f64), (1.0_f64, 1.0_f64)];

    // Vertex Hausdorff for a2 vs b2:
    // directed a2→b2: every point of a2 is ≤ sqrt(1+1)=sqrt(2) from a vertex of b2
    //   inner points of a2 have both vertices of b2 at the same or greater distance
    //   max over a2: point (-1,0) → nearest vertex (-1,1) distance = 1.0;
    //                point (0,0)  → nearest vertex (-1,1) or (1,1) distance = sqrt(2)
    //   so directed a2→b2 (vertex) = sqrt(2)
    // directed b2→a2: (-1,1)→nearest a2 point(-1,0) dist=1.0; (1,1)→(1,0) dist=1.0
    //   max = 1.0
    // symmetric vertex = sqrt(2)
    let vertex_h2 = hausdorff_distance(&a2, &b2);
    let expected_vertex_h2 = 2.0_f64.sqrt();
    assert!(
        (vertex_h2 - expected_vertex_h2).abs() < 1e-9,
        "vertex Hausdorff expected {expected_vertex_h2:.10}, got {vertex_h2:.10}"
    );

    // Segment Hausdorff for a2 vs b2:
    // directed a2→b2: every point of a2 projects onto the segment at y=1, distance = 1.0
    //   max = 1.0
    // directed b2→a2: (-1,1)→nearest pt on a2 segment → (-1,0) dist=1.0; (1,1)→(1,0) dist=1.0
    //   max = 1.0
    // symmetric segment = 1.0
    let seg_h2 = hausdorff_distance_to_segments(&a2, &b2);
    assert!(
        (seg_h2 - 1.0).abs() < 1e-9,
        "segment Hausdorff expected 1.0, got {seg_h2:.10}"
    );

    assert!(
        seg_h2 < vertex_h2,
        "segment Hausdorff ({seg_h2}) must be less than vertex Hausdorff ({vertex_h2})"
    );

    // Also verify the one-sided directed comparison for the simple single-point case
    let a1 = [(0.0_f64, 0.0_f64)];
    // directed A1→B (vertex): min dist from (0,0) to {(-1,1),(1,1)} = sqrt(2)
    let dv = directed_hausdorff(&a1, &b2);
    assert!(
        (dv - 2.0_f64.sqrt()).abs() < 1e-9,
        "directed vertex: expected sqrt(2), got {dv}"
    );
    // directed A1→B (segment): proj of (0,0) onto [(-1,1),(1,1)] = (0,1), dist = 1.0
    // No public directed_to_segs, check via symmetric where b has 1 point so both sides equal
    // Use b = [(0,1)] (single point) so symmetric = 1.0
    let b_single = [(0.0_f64, 1.0_f64)];
    let seg_single = hausdorff_distance_to_segments(&a1, &b_single);
    let vert_single = hausdorff_distance(&a1, &b_single);
    assert!(
        (seg_single - 1.0).abs() < 1e-9,
        "point-to-point seg: {seg_single}"
    );
    assert!(
        (vert_single - 1.0).abs() < 1e-9,
        "point-to-point vert: {vert_single}"
    );
}
