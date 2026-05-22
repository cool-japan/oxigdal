//! Geometry validity checking and winding-order correction for GeoJSON geometries.
//!
//! This module implements RFC 7946 validity rules for polygon rings:
//! - Exterior rings must be counter-clockwise (positive signed area).
//! - Hole rings must be clockwise (negative signed area).
//! - Rings must have at least 4 vertices (the closing vertex counts).
//! - The first and last vertex of each ring must be identical (within ε = 1e-10).
//! - No ring may self-intersect.

use crate::types::GeoJsonGeometry;

// ─── Winding order ────────────────────────────────────────────────────────────

/// The rotational direction of a polygon ring's vertices.
#[derive(Debug, Clone, PartialEq)]
pub enum WindingOrder {
    /// Vertices traverse the interior on the left (positive shoelace area).
    CounterClockwise,
    /// Vertices traverse the interior on the right (negative shoelace area).
    Clockwise,
    /// Ring has zero area (degenerate / collinear).
    Degenerate,
}

// ─── Validity issues ──────────────────────────────────────────────────────────

/// A single validity problem found in a polygon ring.
#[derive(Debug, Clone, PartialEq)]
pub enum GeometryValidityIssue {
    /// A ring intersects itself. `at` is the approximate crossing point.
    SelfIntersectingRing {
        /// Zero-based index of the ring (0 = exterior, 1+ = holes).
        ring_index: usize,
        /// Approximate 2-D coordinate of the self-intersection.
        at: [f64; 2],
    },
    /// A ring has the wrong winding direction.
    IncorrectWinding {
        /// Zero-based index of the ring.
        ring_index: usize,
        /// What the ring should be (CCW for exterior, CW for holes).
        expected: WindingOrder,
        /// What was actually detected.
        found: WindingOrder,
    },
    /// A ring contains two consecutive identical vertices.
    DuplicateVertex {
        /// Zero-based index of the ring.
        ring_index: usize,
        /// Index of the first of the two identical consecutive vertices.
        vertex_index: usize,
    },
    /// A ring has fewer than the minimum required vertices.
    FewerThanMinimumVertices {
        /// Zero-based index of the ring.
        ring_index: usize,
        /// Actual vertex count found.
        count: usize,
    },
    /// The ring is not closed (first vertex ≠ last vertex within 1e-10).
    UnclosedRing {
        /// Zero-based index of the ring.
        ring_index: usize,
    },
}

// ─── Report ───────────────────────────────────────────────────────────────────

/// Summary of all validity issues found in one geometry.
#[derive(Debug, Clone)]
pub struct GeometryValidityReport {
    /// All issues detected, in the order they were found.
    pub issues: Vec<GeometryValidityIssue>,
    /// `true` when `issues` is empty.
    pub is_valid: bool,
}

// ─── Core ring mathematics ────────────────────────────────────────────────────

/// Compute the signed area of a 2-D ring using the shoelace formula.
///
/// Positive area → counter-clockwise winding.
/// Negative area → clockwise winding.
/// Zero area → degenerate (collinear or insufficient vertices).
///
/// The closing vertex (last == first) is handled correctly by the modular
/// index arithmetic — including it causes one duplicate cross-product term
/// that cancels out, so the result is identical whether or not it is present.
#[must_use]
pub fn ring_signed_area(ring: &[[f64; 2]]) -> f64 {
    let n = ring.len();
    if n < 3 {
        return 0.0;
    }
    let mut sum = 0.0_f64;
    for i in 0..n {
        let [x0, y0] = ring[i];
        let [x1, y1] = ring[(i + 1) % n];
        sum += x0 * y1 - x1 * y0;
    }
    sum * 0.5
}

/// Determine the winding order of a ring from its signed area.
#[must_use]
pub fn ring_winding_order(ring: &[[f64; 2]]) -> WindingOrder {
    let area = ring_signed_area(ring);
    if area > 0.0 {
        WindingOrder::CounterClockwise
    } else if area < 0.0 {
        WindingOrder::Clockwise
    } else {
        WindingOrder::Degenerate
    }
}

// ─── Segment intersection ─────────────────────────────────────────────────────

/// Test whether two line segments strictly intersect (endpoints excluded).
///
/// Uses parametric Cramér's rule:
///   P(t) = a1 + t*(a2 - a1)
///   Q(u) = b1 + u*(b2 - b1)
///
/// The segments intersect in their **open** interiors when both t and u are
/// strictly in (0, 1).  Returns the intersection point when they do, or `None`.
fn segment_intersect_exclusive(
    a1: [f64; 2],
    a2: [f64; 2],
    b1: [f64; 2],
    b2: [f64; 2],
) -> Option<[f64; 2]> {
    let dx_a = a2[0] - a1[0];
    let dy_a = a2[1] - a1[1];
    let dx_b = b2[0] - b1[0];
    let dy_b = b2[1] - b1[1];

    // 2×2 system determinant (cross product of direction vectors)
    let denom = dx_a * dy_b - dy_a * dx_b;

    // Parallel or collinear segments → treat as no strict intersection
    if denom.abs() < f64::EPSILON {
        return None;
    }

    let dx_start = b1[0] - a1[0];
    let dy_start = b1[1] - a1[1];

    let t = (dx_start * dy_b - dy_start * dx_b) / denom;
    let u = (dx_start * dy_a - dy_start * dx_a) / denom;

    // Strictly interior: both parameters must be in the open interval (0, 1)
    if t > 0.0 && t < 1.0 && u > 0.0 && u < 1.0 {
        let ix = a1[0] + t * dx_a;
        let iy = a1[1] + t * dy_a;
        Some([ix, iy])
    } else {
        None
    }
}

// ─── Ring self-intersection ────────────────────────────────────────────────────

/// Check whether a ring self-intersects and return the first crossing found.
///
/// The algorithm tests all O(n²) non-adjacent segment pairs.  Adjacent segments
/// share an endpoint and are excluded; the wrap-around pair `(0, n-1)` is also
/// excluded because it is adjacent by construction for a closed ring.
///
/// Returns `None` when the ring is simple (no self-intersections).
#[must_use]
pub fn check_ring_self_intersection(ring: &[[f64; 2]]) -> Option<[f64; 2]> {
    let n = ring.len();
    if n < 4 {
        return None;
    }

    for i in 0..(n - 1) {
        let a1 = ring[i];
        let a2 = ring[i + 1];

        // j must be at least i+2 to skip the adjacent segment.
        // For i == 0 we also skip j == n-1, which would be the wrap-around pair.
        let j_start = i + 2;
        let j_end = if i == 0 { n - 1 } else { n };

        for j in j_start..j_end {
            let b1 = ring[j];
            let b2 = ring[(j + 1) % n];

            if let Some(pt) = segment_intersect_exclusive(a1, a2, b1, b2) {
                return Some(pt);
            }
        }
    }

    None
}

// ─── Polygon ring validation ───────────────────────────────────────────────────

const CLOSE_EPS: f64 = 1e-10;
const MIN_RING_VERTICES: usize = 4;

/// Validate all rings of a polygon (exterior + holes) and collect issues.
///
/// Rules enforced:
/// - Ring 0 (exterior) must be CCW; rings 1+ (holes) must be CW.
/// - Every ring must have ≥ 4 vertices (including the closing duplicate).
/// - Every ring must be closed: `ring[0] ≈ ring[last]` within 1e-10.
/// - No ring may self-intersect.
#[must_use]
pub fn validate_polygon_rings(rings: &[Vec<[f64; 2]>]) -> Vec<GeometryValidityIssue> {
    let mut issues = Vec::new();

    for (ring_index, ring) in rings.iter().enumerate() {
        let count = ring.len();

        // ── Minimum vertex count ─────────────────────────────────────────────
        if count < MIN_RING_VERTICES {
            issues.push(GeometryValidityIssue::FewerThanMinimumVertices { ring_index, count });
            // Cannot meaningfully test other properties without enough vertices.
            continue;
        }

        // ── Ring must be closed ──────────────────────────────────────────────
        let first = ring[0];
        let last = ring[count - 1];
        let dx = first[0] - last[0];
        let dy = first[1] - last[1];
        if dx.abs() > CLOSE_EPS || dy.abs() > CLOSE_EPS {
            issues.push(GeometryValidityIssue::UnclosedRing { ring_index });
        }

        // ── Duplicate consecutive vertices ───────────────────────────────────
        // (Separate from the required close: we look at internal duplicates.)
        for vertex_index in 0..(count - 1) {
            let v0 = ring[vertex_index];
            let v1 = ring[vertex_index + 1];
            let ddx = v0[0] - v1[0];
            let ddy = v0[1] - v1[1];
            if ddx.abs() < CLOSE_EPS && ddy.abs() < CLOSE_EPS {
                // Only flag internal duplicates (skip the intentional closing pair)
                if vertex_index < count - 2 {
                    issues.push(GeometryValidityIssue::DuplicateVertex {
                        ring_index,
                        vertex_index,
                    });
                }
            }
        }

        // ── Winding order ────────────────────────────────────────────────────
        let winding = ring_winding_order(ring);
        let expected = if ring_index == 0 {
            WindingOrder::CounterClockwise
        } else {
            WindingOrder::Clockwise
        };
        if winding != WindingOrder::Degenerate && winding != expected {
            issues.push(GeometryValidityIssue::IncorrectWinding {
                ring_index,
                expected,
                found: winding,
            });
        }

        // ── Self-intersection ────────────────────────────────────────────────
        if let Some(at) = check_ring_self_intersection(ring) {
            issues.push(GeometryValidityIssue::SelfIntersectingRing { ring_index, at });
        }
    }

    issues
}

// ─── Geometry-level validation ────────────────────────────────────────────────

/// Validate a [`GeoJsonGeometry`] and return a comprehensive report.
///
/// - `Point`, `MultiPoint`, `LineString`, `MultiLineString`, and their Z
///   variants are always considered valid (they carry no ring structure).
/// - `Polygon` and `PolygonZ` rings are validated via [`validate_polygon_rings`].
/// - `MultiPolygon` and `MultiPolygonZ` validate each constituent polygon.
/// - `GeometryCollection` recurses into each member geometry.
/// - `Null` is valid (no coordinates).
#[must_use]
pub fn validate_geometry(geom: &GeoJsonGeometry) -> GeometryValidityReport {
    let issues = collect_geometry_issues(geom);
    let is_valid = issues.is_empty();
    GeometryValidityReport { issues, is_valid }
}

/// Internal recursive helper that collects issues without building a report at
/// each recursion level.
fn collect_geometry_issues(geom: &GeoJsonGeometry) -> Vec<GeometryValidityIssue> {
    match geom {
        // ── Always valid (no ring structure) ─────────────────────────────────
        GeoJsonGeometry::Point(_)
        | GeoJsonGeometry::PointZ(_)
        | GeoJsonGeometry::LineString(_)
        | GeoJsonGeometry::LineStringZ(_)
        | GeoJsonGeometry::MultiPoint(_)
        | GeoJsonGeometry::MultiPointZ(_)
        | GeoJsonGeometry::MultiLineString(_)
        | GeoJsonGeometry::MultiLineStringZ(_)
        | GeoJsonGeometry::Null => Vec::new(),

        // ── Polygon ──────────────────────────────────────────────────────────
        GeoJsonGeometry::Polygon(rings) => validate_polygon_rings(rings),

        // ── PolygonZ: drop Z, then validate 2-D projection ───────────────────
        GeoJsonGeometry::PolygonZ(rings) => {
            let rings_2d: Vec<Vec<[f64; 2]>> = rings
                .iter()
                .map(|r| r.iter().map(|[x, y, _]| [*x, *y]).collect())
                .collect();
            validate_polygon_rings(&rings_2d)
        }

        // ── MultiPolygon ─────────────────────────────────────────────────────
        GeoJsonGeometry::MultiPolygon(polys) => polys
            .iter()
            .flat_map(|rings| validate_polygon_rings(rings))
            .collect(),

        // ── MultiPolygonZ ────────────────────────────────────────────────────
        GeoJsonGeometry::MultiPolygonZ(polys) => polys
            .iter()
            .flat_map(|rings| {
                let rings_2d: Vec<Vec<[f64; 2]>> = rings
                    .iter()
                    .map(|r| r.iter().map(|[x, y, _]| [*x, *y]).collect())
                    .collect();
                validate_polygon_rings(&rings_2d)
            })
            .collect(),

        // ── GeometryCollection: recurse ───────────────────────────────────────
        GeoJsonGeometry::GeometryCollection(geoms) => {
            geoms.iter().flat_map(collect_geometry_issues).collect()
        }
    }
}

// ─── Winding correction ────────────────────────────────────────────────────────

/// Reverse the ring's vertex order in-place when it does not match `target`.
///
/// Degenerate rings (zero area) are left unchanged.
pub fn fix_ring_winding(ring: &mut [[f64; 2]], target: WindingOrder) {
    let current = ring_winding_order(ring);
    if current == WindingOrder::Degenerate {
        return;
    }
    if current != target {
        ring.reverse();
    }
}

/// Fix the winding order of all rings in a [`GeoJsonGeometry`] in-place.
///
/// - Exterior ring (index 0) → [`WindingOrder::CounterClockwise`].
/// - Hole rings (index 1+) → [`WindingOrder::Clockwise`].
/// - For `MultiPolygon`, each constituent polygon is corrected independently.
/// - All other geometry types are left unchanged.
pub fn fix_geometry_winding(geom: &mut GeoJsonGeometry) {
    match geom {
        GeoJsonGeometry::Polygon(rings) => fix_polygon_rings_winding(rings),
        GeoJsonGeometry::MultiPolygon(polys) => {
            for rings in polys.iter_mut() {
                fix_polygon_rings_winding(rings);
            }
        }
        // Z variants: correct the 2-D winding of the XY projection.
        GeoJsonGeometry::PolygonZ(rings) => fix_polygon_rings_winding_z(rings),
        GeoJsonGeometry::MultiPolygonZ(polys) => {
            for rings in polys.iter_mut() {
                fix_polygon_rings_winding_z(rings);
            }
        }
        GeoJsonGeometry::GeometryCollection(geoms) => {
            for g in geoms.iter_mut() {
                fix_geometry_winding(g);
            }
        }
        // Point, LineString, MultiPoint, MultiLineString, Null → no-op
        _ => {}
    }
}

/// Fix winding for a slice of 2-D rings (exterior + holes).
fn fix_polygon_rings_winding(rings: &mut [Vec<[f64; 2]>]) {
    for (idx, ring) in rings.iter_mut().enumerate() {
        let target = if idx == 0 {
            WindingOrder::CounterClockwise
        } else {
            WindingOrder::Clockwise
        };
        fix_ring_winding(ring, target);
    }
}

/// Fix winding for a slice of 3-D rings by operating on the XY projection.
///
/// The Z values of each vertex are preserved; only the vertex order may change.
fn fix_polygon_rings_winding_z(rings: &mut [Vec<[f64; 3]>]) {
    for (idx, ring) in rings.iter_mut().enumerate() {
        let target = if idx == 0 {
            WindingOrder::CounterClockwise
        } else {
            WindingOrder::Clockwise
        };
        // Project to 2-D to determine and correct winding.
        let mut ring_2d: Vec<[f64; 2]> = ring.iter().map(|[x, y, _]| [*x, *y]).collect();
        let current = ring_winding_order(&ring_2d);
        if current == WindingOrder::Degenerate {
            continue;
        }
        if current != target {
            ring_2d.reverse();
            // Apply the same reversal to the 3-D ring to keep Z in sync.
            ring.reverse();
        }
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Fixtures ────────────────────────────────────────────────────────────

    fn ccw_square() -> Vec<[f64; 2]> {
        vec![[0., 0.], [1., 0.], [1., 1.], [0., 1.], [0., 0.]]
    }

    fn cw_square() -> Vec<[f64; 2]> {
        vec![[0., 0.], [0., 1.], [1., 1.], [1., 0.], [0., 0.]]
    }

    fn collinear_ring() -> Vec<[f64; 2]> {
        vec![[0., 0.], [1., 0.], [2., 0.], [3., 0.], [0., 0.]]
    }

    // ── ring_winding_order ───────────────────────────────────────────────────

    #[test]
    fn test_ring_winding_ccw() {
        assert_eq!(
            ring_winding_order(&ccw_square()),
            WindingOrder::CounterClockwise
        );
    }

    #[test]
    fn test_ring_winding_cw() {
        assert_eq!(ring_winding_order(&cw_square()), WindingOrder::Clockwise);
    }

    #[test]
    fn test_ring_winding_degenerate_collinear() {
        assert_eq!(
            ring_winding_order(&collinear_ring()),
            WindingOrder::Degenerate
        );
    }

    // ── ring_signed_area ─────────────────────────────────────────────────────

    #[test]
    fn test_ring_signed_area_unit_square_ccw() {
        let area = ring_signed_area(&ccw_square());
        assert!(
            (area - 1.0).abs() < 1e-10,
            "expected area ≈ 1.0, got {area}"
        );
    }

    #[test]
    fn test_ring_signed_area_unit_square_cw() {
        let area = ring_signed_area(&cw_square());
        assert!(
            (area + 1.0).abs() < 1e-10,
            "expected area ≈ -1.0, got {area}"
        );
    }

    // ── check_ring_self_intersection ─────────────────────────────────────────

    #[test]
    fn test_self_intersection_simple_square_none() {
        assert!(check_ring_self_intersection(&ccw_square()).is_none());
    }

    #[test]
    fn test_self_intersection_bowtie_detected() {
        // Bowtie: (0,0)->(1,1)->(1,0)->(0,1)->(0,0) — the diagonal segments cross
        let bowtie = vec![[0., 0.], [1., 1.], [1., 0.], [0., 1.], [0., 0.]];
        assert!(check_ring_self_intersection(&bowtie).is_some());
    }

    // ── validate_polygon_rings ────────────────────────────────────────────────

    #[test]
    fn test_validate_exterior_ccw_no_issues() {
        let issues = validate_polygon_rings(&[ccw_square()]);
        assert!(issues.is_empty(), "unexpected issues: {issues:?}");
    }

    #[test]
    fn test_validate_exterior_cw_reports_incorrect_winding() {
        let issues = validate_polygon_rings(&[cw_square()]);
        assert!(
            issues
                .iter()
                .any(|i| matches!(i, GeometryValidityIssue::IncorrectWinding { .. })),
            "expected IncorrectWinding, got: {issues:?}"
        );
    }

    #[test]
    fn test_validate_hole_ccw_reports_incorrect_winding() {
        // ring 0 = exterior (CCW), ring 1 = hole that is also CCW (wrong)
        let issues = validate_polygon_rings(&[ccw_square(), ccw_square()]);
        assert!(
            issues.iter().any(|i| matches!(
                i,
                GeometryValidityIssue::IncorrectWinding { ring_index: 1, .. }
            )),
            "expected IncorrectWinding for hole, got: {issues:?}"
        );
    }

    #[test]
    fn test_validate_unclosed_ring() {
        let mut r = ccw_square();
        r.pop(); // remove closing vertex
        let issues = validate_polygon_rings(&[r]);
        assert!(
            issues
                .iter()
                .any(|i| matches!(i, GeometryValidityIssue::UnclosedRing { .. })),
            "expected UnclosedRing, got: {issues:?}"
        );
    }

    #[test]
    fn test_validate_too_few_vertices() {
        let r = vec![[0., 0.], [1., 1.], [0., 0.]]; // only 3 vertices
        let issues = validate_polygon_rings(&[r]);
        assert!(
            issues
                .iter()
                .any(|i| matches!(i, GeometryValidityIssue::FewerThanMinimumVertices { .. })),
            "expected FewerThanMinimumVertices, got: {issues:?}"
        );
    }

    // ── validate_geometry ─────────────────────────────────────────────────────

    #[test]
    fn test_validate_geometry_point_always_valid() {
        let geom = GeoJsonGeometry::Point([1.0, 2.0]);
        assert!(validate_geometry(&geom).is_valid);
    }

    #[test]
    fn test_validate_geometry_null_valid() {
        assert!(validate_geometry(&GeoJsonGeometry::Null).is_valid);
    }

    #[test]
    fn test_validate_geometry_valid_polygon() {
        let geom = GeoJsonGeometry::Polygon(vec![ccw_square()]);
        assert!(validate_geometry(&geom).is_valid);
    }

    #[test]
    fn test_validate_geometry_invalid_polygon() {
        let geom = GeoJsonGeometry::Polygon(vec![cw_square()]);
        let report = validate_geometry(&geom);
        assert!(!report.is_valid);
    }

    // ── fix_ring_winding ──────────────────────────────────────────────────────

    #[test]
    fn test_fix_ring_winding_reverses_cw_to_ccw() {
        let mut r = cw_square();
        fix_ring_winding(&mut r, WindingOrder::CounterClockwise);
        assert_eq!(ring_winding_order(&r), WindingOrder::CounterClockwise);
    }

    #[test]
    fn test_fix_ring_winding_no_op_when_correct() {
        let original = ccw_square();
        let mut r = original.clone();
        fix_ring_winding(&mut r, WindingOrder::CounterClockwise);
        assert_eq!(r, original, "ring should be unchanged");
    }

    // ── fix_geometry_winding ───────────────────────────────────────────────────

    #[test]
    fn test_fix_geometry_winding_corrects_polygon_with_hole() {
        // CW exterior + CCW hole (both wrong)
        let exterior = cw_square();
        let hole = ccw_square();
        let mut geom = GeoJsonGeometry::Polygon(vec![exterior, hole]);
        fix_geometry_winding(&mut geom);
        let GeoJsonGeometry::Polygon(rings) = &geom else {
            unreachable!("we constructed a Polygon, so this branch is never reached");
        };
        assert_eq!(
            ring_winding_order(&rings[0]),
            WindingOrder::CounterClockwise
        );
        assert_eq!(ring_winding_order(&rings[1]), WindingOrder::Clockwise);
    }

    #[test]
    fn test_fix_geometry_winding_multipolygon() {
        let poly1 = vec![cw_square()];
        let poly2 = vec![cw_square()];
        let mut geom = GeoJsonGeometry::MultiPolygon(vec![poly1, poly2]);
        fix_geometry_winding(&mut geom);
        let GeoJsonGeometry::MultiPolygon(polys) = &geom else {
            unreachable!("we constructed a MultiPolygon, so this branch is never reached");
        };
        for poly in polys {
            assert_eq!(ring_winding_order(&poly[0]), WindingOrder::CounterClockwise);
        }
    }
}
