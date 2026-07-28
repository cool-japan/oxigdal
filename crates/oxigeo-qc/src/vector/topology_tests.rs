//! Unit tests for the vector topology validation engine ([`super`]).
//!
//! Split out of topology.rs to keep that file under the workspace's
//! 2000-line-per-file refactoring policy.

use super::*;
use oxigeo_core::vector::{Feature, FeatureCollection, FeatureId};

// ── helpers ────────────────────────────────────────────────────────────────

fn ls(coords: &[(f64, f64)]) -> LineString {
    LineString {
        coords: coords
            .iter()
            .map(|(x, y)| Coordinate::new_2d(*x, *y))
            .collect(),
    }
}

/// Build a Polygon directly (bypasses Polygon::new validation, needed for
/// constructing intentionally invalid geometries in tests).
fn poly_raw(exterior_coords: &[(f64, f64)]) -> Polygon {
    Polygon {
        exterior: ls(exterior_coords),
        interiors: Vec::new(),
    }
}

/// Build a valid closed polygon (CCW square).
fn ccw_square(x0: f64, y0: f64, x1: f64, y1: f64) -> Polygon {
    poly_raw(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1), (x0, y0)])
}

fn fc_with_polygon(poly: Polygon) -> FeatureCollection {
    FeatureCollection::new(vec![Feature::new(Geometry::Polygon(poly))])
}

fn fc_with_linestring(ls_geom: LineString) -> FeatureCollection {
    FeatureCollection::new(vec![Feature::new(Geometry::LineString(ls_geom))])
}

// ── existing tests (unchanged) ─────────────────────────────────────────────

#[test]
fn test_topology_checker_creation() {
    let checker = TopologyChecker::new();
    assert!(checker.config.check_self_intersections);
}

#[test]
fn test_invalid_coordinate_detection() {
    let checker = TopologyChecker::new();
    let coord = Coordinate::new_2d(f64::NAN, 0.0);
    let errors = checker.validate_point(&coord, &None);

    assert!(errors.is_ok());
    let errors = errors.ok().unwrap_or_default();
    assert!(!errors.is_empty());
    assert_eq!(errors[0].error_type, TopologyErrorType::InvalidCoordinate);
}

#[test]
fn test_linestring_validation() {
    let checker = TopologyChecker::new();
    let linestring = LineString {
        coords: vec![Coordinate::new_2d(0.0, 0.0), Coordinate::new_2d(1.0, 1.0)],
    };

    let errors = checker.validate_linestring(&linestring, &None);
    assert!(errors.is_ok());
}

#[test]
fn test_coords_equal() {
    let checker = TopologyChecker::new();
    let c1 = Coordinate::new_2d(0.0, 0.0);
    let c2 = Coordinate::new_2d(0.0, 0.0);
    let c3 = Coordinate::new_2d(1.0, 1.0);

    assert!(checker.coords_equal(&c1, &c2));
    assert!(!checker.coords_equal(&c1, &c3));
}

// ── new topology engine tests ──────────────────────────────────────────────

/// X-shaped self-intersecting linestring: (0,0)→(2,2)→(0,2)→(2,0).
/// Segment 0 (0,0)→(2,2) crosses segment 2 (0,2)→(2,0) at (1,1).
#[test]
fn test_self_intersect_simple_x() {
    let ls_geom = ls(&[(0.0, 0.0), (2.0, 2.0), (0.0, 2.0), (2.0, 0.0)]);
    let result = has_self_intersection(&ls_geom);
    assert!(
        result.is_some(),
        "Expected self-intersection to be detected"
    );
    let pairs = result.unwrap_or_default();
    assert!(
        pairs.contains(&(0, 2)),
        "Expected pair (0, 2) in crossings, got: {:?}",
        pairs
    );
}

/// Straight line with 10 collinear points — no self-intersection.
#[test]
fn test_self_intersect_no_intersection() {
    let pts: Vec<(f64, f64)> = (0..10).map(|i| (i as f64, 0.0)).collect();
    let ls_geom = ls(&pts);
    let result = has_self_intersection(&ls_geom);
    assert!(result.is_none(), "Straight line must not self-intersect");
}

/// 3-point L-bend: (0,0)→(1,0)→(1,1).  Adjacent segments share a point — not
/// a self-intersection (only 2 segments, cannot be non-adjacent).
#[test]
fn test_self_intersect_endpoint_shared_only() {
    let ls_geom = ls(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)]);
    // 3 coords → 2 segments → no non-adjacent pairs
    let result = has_self_intersection(&ls_geom);
    assert!(result.is_none(), "L-bend must not be flagged");
}

/// Collinear overlap: (0,0)→(2,0)→(1,0)→(3,0).
/// Segment 0 and segment 2 are collinear and overlap.
#[test]
fn test_self_intersect_collinear_overlap() {
    let ls_geom = ls(&[(0.0, 0.0), (2.0, 0.0), (1.0, 0.0), (3.0, 0.0)]);
    let result = has_self_intersection(&ls_geom);
    assert!(result.is_some(), "Collinear overlap should be detected");
}

/// Polygon with exterior ring in CW order — R2 violation expected.
#[test]
fn test_check_topology_rules_polygon_orientation_violation() {
    // CW square (reversed from CCW)
    let cw_poly = poly_raw(&[(0.0, 0.0), (0.0, 2.0), (2.0, 2.0), (2.0, 0.0), (0.0, 0.0)]);
    let options = TopologyOptions::default();
    let violations = check_topology_rules(&fc_with_polygon(cw_poly), &options);
    let has_orient = violations.iter().any(|v| {
        matches!(
            v,
            TopologyViolation::RingOrientation {
                ring_index: 0,
                expected_ccw: true,
                ..
            }
        )
    });
    assert!(
        has_orient,
        "Expected RingOrientation violation, got: {:?}",
        violations
    );
}

/// Polygon where last coord ≠ first — R3 violation.
#[test]
fn test_check_topology_rules_unclosed_ring() {
    let unclosed = poly_raw(&[(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)]);
    let options = TopologyOptions::default();
    let violations = check_topology_rules(&fc_with_polygon(unclosed), &options);
    let has_unclosed = violations
        .iter()
        .any(|v| matches!(v, TopologyViolation::UnclosedRing { ring_index: 0, .. }));
    assert!(
        has_unclosed,
        "Expected UnclosedRing violation, got: {:?}",
        violations
    );
}

/// Bowtie ring (self-intersecting exterior): R4 violation.
#[test]
fn test_check_topology_rules_polygon_self_intersect_ring() {
    // Bowtie: (0,0)→(2,2)→(0,2)→(2,0)→(0,0)
    let bowtie = poly_raw(&[(0.0, 0.0), (2.0, 2.0), (0.0, 2.0), (2.0, 0.0), (0.0, 0.0)]);
    let options = TopologyOptions::default();
    let violations = check_topology_rules(&fc_with_polygon(bowtie), &options);
    let has_self_intersect = violations
        .iter()
        .any(|v| matches!(v, TopologyViolation::SelfIntersection { .. }));
    assert!(
        has_self_intersect,
        "Expected SelfIntersection on bowtie ring, got: {:?}",
        violations
    );
}

/// Two overlapping squares: A (0,0)-(2,2) and B (1,1)-(3,3).  R6 violation.
#[test]
fn test_check_topology_rules_overlap_detection() {
    let poly_a = ccw_square(0.0, 0.0, 2.0, 2.0);
    let poly_b = ccw_square(1.0, 1.0, 3.0, 3.0);
    let fc = FeatureCollection::new(vec![
        Feature::with_id(FeatureId::Integer(1), Geometry::Polygon(poly_a)),
        Feature::with_id(FeatureId::Integer(2), Geometry::Polygon(poly_b)),
    ]);
    let options = TopologyOptions::default();
    let violations = check_topology_rules(&fc, &options);
    let overlap = violations
        .iter()
        .find(|v| matches!(v, TopologyViolation::Overlap { .. }));
    assert!(
        overlap.is_some(),
        "Expected Overlap violation, got: {:?}",
        violations
    );
    if let Some(TopologyViolation::Overlap { area, .. }) = overlap {
        assert!(*area > 0.0, "Expected positive overlap area, got {}", area);
    }
}

/// Well-formed CCW polygon — no violations expected.
#[test]
fn test_check_topology_rules_clean_data_returns_empty() {
    let clean = ccw_square(0.0, 0.0, 2.0, 2.0);
    let options = TopologyOptions::default();
    let violations = check_topology_rules(&fc_with_polygon(clean), &options);
    assert!(
        violations.is_empty(),
        "Expected no violations for clean polygon, got: {:?}",
        violations
    );
}

/// R5 gap detection is opt-in.  With `detect_gaps: false`, no Gap violations.
/// With `detect_gaps: true` and two side-by-side non-overlapping polygons, a Gap
/// may be detected (proximity-based heuristic).
#[test]
fn test_check_topology_rules_gap_detection_optional() {
    let poly_a = ccw_square(0.0, 0.0, 1.0, 1.0);
    let poly_b = ccw_square(1.5, 0.0, 2.5, 1.0); // 0.5 gap on X axis
    let fc = FeatureCollection::new(vec![
        Feature::new(Geometry::Polygon(poly_a)),
        Feature::new(Geometry::Polygon(poly_b)),
    ]);

    // Default options (detect_gaps = false) — no R5 violations
    let options_off = TopologyOptions::default();
    let violations_off = check_topology_rules(&fc, &options_off);
    let has_gap_off = violations_off
        .iter()
        .any(|v| matches!(v, TopologyViolation::Gap { .. }));
    assert!(
        !has_gap_off,
        "Should not detect gaps when detect_gaps=false"
    );

    // With detect_gaps = true — proximity heuristic may fire
    let options_on = TopologyOptions {
        detect_gaps: true,
        ..TopologyOptions::default()
    };
    let violations_on = check_topology_rules(&fc, &options_on);
    // We don't assert it MUST find a gap (heuristic), but we verify the code runs
    let _ = violations_on;
}

/// 1000 non-overlapping polygons in a grid — no overlap violations expected.
/// This is a performance smoke test: we only assert correctness, not timing.
#[test]
fn test_check_topology_rules_1000_polygons_perf_smoke() {
    let mut features = Vec::with_capacity(1000);
    for row in 0..25 {
        for col in 0..40 {
            let x0 = col as f64 * 2.0;
            let y0 = row as f64 * 2.0;
            let poly = ccw_square(x0, y0, x0 + 1.0, y0 + 1.0);
            features.push(Feature::new(Geometry::Polygon(poly)));
        }
    }
    let fc = FeatureCollection::new(features);
    let options = TopologyOptions::default();
    let violations = check_topology_rules(&fc, &options);
    // No overlaps expected among grid cells with 1-unit gaps between them
    let overlap_count = violations
        .iter()
        .filter(|v| matches!(v, TopologyViolation::Overlap { .. }))
        .count();
    assert!(
        overlap_count == 0,
        "Non-overlapping grid should produce 0 Overlap violations, got {}",
        overlap_count
    );
    // Total violations count is an upper bound sanity check (only orientation/closure
    // violations if any raw struct construction produced bad geometry, which it shouldn't)
    assert!(
        violations.len() < 10,
        "Expected < 10 violations for clean grid, got {}",
        violations.len()
    );
}

/// A self-intersecting LineString geometry inside a FeatureCollection — R1 violation.
#[test]
fn test_check_topology_rules_linestring_self_intersect() {
    let ls_geom = ls(&[(0.0, 0.0), (2.0, 2.0), (0.0, 2.0), (2.0, 0.0)]);
    let fc = fc_with_linestring(ls_geom);
    let options = TopologyOptions::default();
    let violations = check_topology_rules(&fc, &options);
    let has_si = violations
        .iter()
        .any(|v| matches!(v, TopologyViolation::SelfIntersection { .. }));
    assert!(
        has_si,
        "Expected SelfIntersection for X linestring, got: {:?}",
        violations
    );
}

// ── sliver area/perimeter tests (holes must net out of area) ──────────────

/// A square with a hole cut out must report the net area (exterior minus
/// hole), not the gross exterior area.
#[test]
fn test_calculate_area_subtracts_holes() {
    let checker = TopologyChecker::new();
    let exterior = ls(&[
        (0.0, 0.0),
        (10.0, 0.0),
        (10.0, 10.0),
        (0.0, 10.0),
        (0.0, 0.0),
    ]);
    let hole = ls(&[(1.0, 1.0), (9.0, 1.0), (9.0, 9.0), (1.0, 9.0), (1.0, 1.0)]);
    let polygon = Polygon {
        exterior,
        interiors: vec![hole],
    };

    // Exterior area = 100.0, hole area = 64.0, net area = 36.0.
    let area = checker.calculate_area(&polygon);
    assert!(
        (area - 36.0).abs() < 1e-9,
        "expected net area 36.0, got {area}"
    );
}

/// A polygon with no holes keeps its full exterior area (regression guard
/// for the holes-subtraction change above).
#[test]
fn test_calculate_area_no_holes_unchanged() {
    let checker = TopologyChecker::new();
    let exterior = ls(&[
        (0.0, 0.0),
        (10.0, 0.0),
        (10.0, 10.0),
        (0.0, 10.0),
        (0.0, 0.0),
    ]);
    let polygon = Polygon {
        exterior,
        interiors: vec![],
    };

    let area = checker.calculate_area(&polygon);
    assert!(
        (area - 100.0).abs() < 1e-9,
        "expected area 100.0, got {area}"
    );
}

/// A thin annulus (large exterior, near-equal-size hole) is a textbook
/// sliver: its true (net) area and compactness are tiny even though the
/// gross exterior area is large. Before the holes-subtraction fix,
/// `check_sliver` scored this using the gross exterior area (100.0),
/// which is far above `sliver_area_threshold` (1.0 by default), so the
/// sliver was silently missed. After the fix, the net area (~0.8) is
/// below the threshold and the compactness ratio is small enough to be
/// flagged.
#[test]
fn test_check_sliver_detects_thin_annulus() {
    let checker = TopologyChecker::new();
    let exterior = ls(&[
        (0.0, 0.0),
        (10.0, 0.0),
        (10.0, 10.0),
        (0.0, 10.0),
        (0.0, 0.0),
    ]);
    // Ring thickness of 0.02 on all sides -> hole area = 9.96^2 = 99.2016,
    // net area = 100.0 - 99.2016 = 0.7984.
    let hole = ls(&[
        (0.02, 0.02),
        (9.98, 0.02),
        (9.98, 9.98),
        (0.02, 9.98),
        (0.02, 0.02),
    ]);
    let polygon = Polygon {
        exterior,
        interiors: vec![hole],
    };

    let sliver = checker
        .check_sliver(&polygon, &None)
        .expect("check_sliver should succeed")
        .expect("thin annulus should be flagged as a sliver");
    assert!(
        sliver.area < 1.0,
        "expected net sliver area < 1.0, got {}",
        sliver.area
    );
}

/// A solid square (no holes) with the same exterior as the annulus test
/// above is well above the default area threshold and must NOT be
/// flagged as a sliver.
#[test]
fn test_check_sliver_ignores_solid_square() {
    let checker = TopologyChecker::new();
    let exterior = ls(&[
        (0.0, 0.0),
        (10.0, 0.0),
        (10.0, 10.0),
        (0.0, 10.0),
        (0.0, 0.0),
    ]);
    let polygon = Polygon {
        exterior,
        interiors: vec![],
    };

    let sliver = checker
        .check_sliver(&polygon, &None)
        .expect("check_sliver should succeed");
    assert!(sliver.is_none(), "solid square should not be a sliver");
}

// ── TopologyRule enforcement / gating tests ────────────────────────────────

/// Two crossing diagonal linestrings (an X shape split across two
/// features) — R8 `Crossing` violation, mapped to `MustNotCross`.
#[test]
fn test_detect_line_crossings_finds_real_crossing() {
    let line_a = ls(&[(0.0, 0.0), (2.0, 2.0)]);
    let line_b = ls(&[(0.0, 2.0), (2.0, 0.0)]);
    let fc = FeatureCollection::new(vec![
        Feature::with_id(FeatureId::Integer(1), Geometry::LineString(line_a)),
        Feature::with_id(FeatureId::Integer(2), Geometry::LineString(line_b)),
    ]);

    let options = TopologyOptions::default();
    let violations = check_topology_rules(&fc, &options);
    assert!(
        violations
            .iter()
            .any(|v| matches!(v, TopologyViolation::Crossing { .. })),
        "two genuinely crossing linestrings must produce a Crossing violation, got: {violations:?}"
    );

    let rule_violations: Vec<RuleViolation> = violations
        .into_iter()
        .map(topology_violation_to_rule_violation)
        .collect();
    assert!(
        rule_violations
            .iter()
            .any(|rv| rv.rule == TopologyRule::MustNotCross),
        "Crossing violations must map to TopologyRule::MustNotCross"
    );
}

/// Two parallel, non-touching linestrings must not be flagged.
#[test]
fn test_detect_line_crossings_ignores_parallel_lines() {
    let line_a = ls(&[(0.0, 0.0), (2.0, 0.0)]);
    let line_b = ls(&[(0.0, 1.0), (2.0, 1.0)]);
    let fc = FeatureCollection::new(vec![
        Feature::new(Geometry::LineString(line_a)),
        Feature::new(Geometry::LineString(line_b)),
    ]);

    let options = TopologyOptions::default();
    let violations = check_topology_rules(&fc, &options);
    assert!(
        !violations
            .iter()
            .any(|v| matches!(v, TopologyViolation::Crossing { .. })),
        "parallel non-touching lines must not be flagged as crossing"
    );
}

/// `detect_crossings = false` must suppress R8 even for genuinely
/// crossing lines.
#[test]
fn test_detect_line_crossings_opt_out() {
    let line_a = ls(&[(0.0, 0.0), (2.0, 2.0)]);
    let line_b = ls(&[(0.0, 2.0), (2.0, 0.0)]);
    let fc = FeatureCollection::new(vec![
        Feature::new(Geometry::LineString(line_a)),
        Feature::new(Geometry::LineString(line_b)),
    ]);

    let options = TopologyOptions {
        detect_crossings: false,
        ..TopologyOptions::default()
    };
    let violations = check_topology_rules(&fc, &options);
    assert!(
        !violations
            .iter()
            .any(|v| matches!(v, TopologyViolation::Crossing { .. })),
        "detect_crossings=false must suppress crossing detection"
    );
}

/// `TopologyChecker::validate` must not report Overlap violations when
/// `topology_rules` does not include `MustNotOverlap` -- previously
/// `detect_overlaps` ran unconditionally regardless of configuration.
#[test]
fn test_topology_checker_respects_disabled_overlap_rule() {
    let poly_a = ccw_square(0.0, 0.0, 2.0, 2.0);
    let poly_b = ccw_square(1.0, 1.0, 3.0, 3.0);
    let fc = FeatureCollection::new(vec![
        Feature::with_id(FeatureId::Integer(1), Geometry::Polygon(poly_a)),
        Feature::with_id(FeatureId::Integer(2), Geometry::Polygon(poly_b)),
    ]);

    // Disable MustNotOverlap entirely; only MustNotHaveGaps stays configured.
    let config = TopologyConfig {
        topology_rules: vec![TopologyRule::MustNotHaveGaps],
        ..TopologyConfig::default()
    };
    let checker = TopologyChecker::with_config(config);
    let result = checker.validate(&fc).expect("validate should succeed");

    assert!(
        !result
            .rule_violations
            .iter()
            .any(|rv| rv.rule == TopologyRule::MustNotOverlap),
        "disabling MustNotOverlap must actually suppress overlap violations, not silently \
             report them anyway"
    );
}

/// `TopologyChecker::validate` must reject (not silently no-op) a
/// `topology_rules` configuration containing a rule this engine cannot
/// enforce (cross-feature-class coverage/containment rules).
#[test]
fn test_topology_checker_rejects_unsupported_rule() {
    for unsupported in [
        TopologyRule::MustBeCoveredBy,
        TopologyRule::BoundaryMustBeCoveredBy,
        TopologyRule::MustBeInside,
        TopologyRule::PointsMustBeCoveredByLine,
    ] {
        let config = TopologyConfig {
            topology_rules: vec![unsupported],
            ..TopologyConfig::default()
        };
        let checker = TopologyChecker::with_config(config);
        let clean = ccw_square(0.0, 0.0, 2.0, 2.0);
        let result = checker.validate(&fc_with_polygon(clean));
        assert!(
            result.is_err(),
            "{unsupported:?} is not enforced by this engine and must be rejected instead \
                 of silently validating as if it were checked"
        );
    }
}

/// Sanity check that the four genuinely-supported rules are NOT rejected.
#[test]
fn test_topology_checker_accepts_all_supported_rules() {
    let config = TopologyConfig {
        topology_rules: vec![
            TopologyRule::MustNotOverlap,
            TopologyRule::MustNotHaveGaps,
            TopologyRule::MustNotCross,
            TopologyRule::MustNotSelfOverlap,
        ],
        ..TopologyConfig::default()
    };
    let checker = TopologyChecker::with_config(config);
    let clean = ccw_square(0.0, 0.0, 2.0, 2.0);
    let result = checker.validate(&fc_with_polygon(clean));
    assert!(result.is_ok(), "supported rules must not be rejected");
}
