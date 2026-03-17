//! Geometry validation for polygons and rings.
//!
//! Provides [`validate_polygon`] to check a [`Polygon`] for a variety of
//! structural issues such as unclosed rings, self-intersections, invalid hole
//! orientation, and zero-area rings.

#[cfg(not(feature = "std"))]
use alloc::{vec, vec::Vec};

// ---------------------------------------------------------------------------
// Coordinate types
// ---------------------------------------------------------------------------

/// A 2-D coordinate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coord {
    /// X (easting / longitude) component.
    pub x: f64,
    /// Y (northing / latitude) component.
    pub y: f64,
}

impl Coord {
    /// Create a new coordinate.
    #[inline]
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// A linear ring — a closed sequence of [`Coord`]s.
///
/// By convention the last coordinate must equal the first.
#[derive(Debug, Clone, PartialEq)]
pub struct Ring {
    coords: Vec<Coord>,
}

impl Ring {
    /// Construct a ring from a vector of coordinates.
    pub fn new(coords: Vec<Coord>) -> Self {
        Self { coords }
    }

    /// The underlying coordinate slice.
    #[inline]
    pub fn coords(&self) -> &[Coord] {
        &self.coords
    }

    /// Number of coordinates in the ring.
    #[inline]
    pub fn len(&self) -> usize {
        self.coords.len()
    }

    /// Whether the ring has no coordinates.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.coords.is_empty()
    }
}

/// A polygon with an exterior ring and zero or more interior rings (holes).
#[derive(Debug, Clone, PartialEq)]
pub struct Polygon {
    /// Exterior ring (should be counter-clockwise).
    pub exterior: Ring,
    /// Interior rings / holes (should be clockwise).
    pub holes: Vec<Ring>,
}

impl Polygon {
    /// Create a polygon from an exterior ring and optional holes.
    pub fn new(exterior: Ring, holes: Vec<Ring>) -> Self {
        Self { exterior, holes }
    }

    /// Create a simple polygon (no holes) from an exterior ring.
    pub fn simple(exterior: Ring) -> Self {
        Self {
            exterior,
            holes: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Validation types
// ---------------------------------------------------------------------------

/// A single issue detected during polygon validation.
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationIssue {
    /// The first and last coordinate of a ring are not equal.
    UnclosedRing,
    /// A closed ring must have at least 4 coordinates (3 distinct + closing).
    TooFewPoints,
    /// Two non-adjacent segments of a ring cross each other.
    SelfIntersection {
        /// Index of the first segment's start coordinate.
        segment1: usize,
        /// Index of the second segment's start coordinate.
        segment2: usize,
    },
    /// Two consecutive coordinates are identical.
    DuplicateConsecutivePoints {
        /// Index of the first of the two duplicate coordinates.
        index: usize,
    },
    /// A hole is wound in the same direction as the exterior ring.
    InvalidHoleOrientation,
    /// A hole's centroid lies outside the exterior ring.
    HoleOutsideExterior,
    /// The ring has zero signed area (all points are collinear).
    ZeroAreaRing,
}

/// Result of validating a polygon: a collection of zero or more issues.
#[derive(Debug, Clone, Default)]
pub struct ValidationResult {
    issues: Vec<ValidationIssue>,
}

impl ValidationResult {
    /// Create an empty (valid) result.
    pub fn new() -> Self {
        Self { issues: Vec::new() }
    }

    /// Add an issue.
    pub fn push(&mut self, issue: ValidationIssue) {
        self.issues.push(issue);
    }

    /// Whether no issues were found.
    pub fn is_valid(&self) -> bool {
        self.issues.is_empty()
    }

    /// The collected issues.
    pub fn issues(&self) -> &[ValidationIssue] {
        &self.issues
    }

    /// Number of issues.
    pub fn len(&self) -> usize {
        self.issues.len()
    }

    /// Whether there are zero issues.
    pub fn is_empty(&self) -> bool {
        self.issues.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Signed area
// ---------------------------------------------------------------------------

/// Compute the signed area of a ring using the shoelace formula.
///
/// A positive value indicates counter-clockwise winding; negative indicates
/// clockwise.  The ring need not be closed (the closing edge from last to
/// first is included automatically).
pub fn signed_area(ring: &Ring) -> f64 {
    let coords = ring.coords();
    if coords.len() < 3 {
        return 0.0;
    }
    let mut sum = 0.0_f64;
    let n = coords.len();
    for i in 0..n {
        let j = (i + 1) % n;
        sum += coords[i].x * coords[j].y;
        sum -= coords[j].x * coords[i].y;
    }
    sum * 0.5
}

// ---------------------------------------------------------------------------
// Ring closure
// ---------------------------------------------------------------------------

/// Check whether a ring is properly closed (first == last coordinate).
///
/// Returns `Some(UnclosedRing)` if the ring has fewer than 2 points or the
/// first and last coordinates differ.
pub fn validate_ring_closure(ring: &Ring) -> Option<ValidationIssue> {
    let coords = ring.coords();
    if coords.len() < 2 {
        return Some(ValidationIssue::UnclosedRing);
    }
    let first = coords[0];
    let last = coords[coords.len() - 1];
    if !coord_eq(first, last) {
        Some(ValidationIssue::UnclosedRing)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Ring orientation
// ---------------------------------------------------------------------------

/// Return `true` if the ring is wound counter-clockwise (positive signed area).
///
/// Rings with zero area return `false`.
pub fn validate_ring_orientation(ring: &Ring) -> bool {
    signed_area(ring) > 0.0
}

// ---------------------------------------------------------------------------
// Segment intersection
// ---------------------------------------------------------------------------

/// Proper intersection test for two segments `(p1→p2)` and `(p3→p4)`.
///
/// Returns `true` when the segments cross each other (overlap / shared
/// endpoints are **not** treated as intersections).
pub fn segments_intersect(p1: Coord, p2: Coord, p3: Coord, p4: Coord) -> bool {
    let d1 = cross_product_sign(p3, p4, p1);
    let d2 = cross_product_sign(p3, p4, p2);
    let d3 = cross_product_sign(p1, p2, p3);
    let d4 = cross_product_sign(p1, p2, p4);

    // Proper crossing: the endpoints of each segment lie on opposite sides of
    // the line through the other segment.
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    {
        return true;
    }

    false
}

/// Cross-product of vectors `(b - a)` and `(c - a)`.  The sign encodes the
/// orientation: positive ⇒ `c` is left of `a→b`; negative ⇒ right.
#[inline]
fn cross_product_sign(a: Coord, b: Coord, c: Coord) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

// ---------------------------------------------------------------------------
// Self-intersection
// ---------------------------------------------------------------------------

/// Find all pairs of non-adjacent segments in a ring that properly intersect.
pub fn validate_no_self_intersection(ring: &Ring) -> Vec<ValidationIssue> {
    let coords = ring.coords();
    let n = coords.len();
    if n < 4 {
        return Vec::new();
    }

    let mut issues = Vec::new();
    let seg_count = n - 1; // number of segments

    for i in 0..seg_count {
        // Start j from i+2 to skip adjacent segments.  Also skip the pair
        // (0, seg_count-1) because they share the closing vertex.
        for j in (i + 2)..seg_count {
            if i == 0 && j == seg_count - 1 {
                continue; // adjacent via ring closure
            }
            if segments_intersect(coords[i], coords[i + 1], coords[j], coords[j + 1]) {
                issues.push(ValidationIssue::SelfIntersection {
                    segment1: i,
                    segment2: j,
                });
            }
        }
    }
    issues
}

// ---------------------------------------------------------------------------
// Duplicate consecutive points
// ---------------------------------------------------------------------------

/// Find all consecutive duplicate coordinates in a ring.
fn validate_no_duplicate_consecutive(ring: &Ring) -> Vec<ValidationIssue> {
    let coords = ring.coords();
    let mut issues = Vec::new();
    for i in 0..coords.len().saturating_sub(1) {
        if coord_eq(coords[i], coords[i + 1]) {
            // Skip the closing pair (first == last is expected).
            if i == coords.len() - 2 {
                continue;
            }
            issues.push(ValidationIssue::DuplicateConsecutivePoints { index: i });
        }
    }
    issues
}

// ---------------------------------------------------------------------------
// Point-in-ring (ray casting) — used by hole-outside-exterior check
// ---------------------------------------------------------------------------

/// Ray-casting point-in-ring test.  Returns `true` if `point` lies inside
/// the ring (boundary is indeterminate).
fn point_in_ring(point: &Coord, ring: &Ring) -> bool {
    let coords = ring.coords();
    let n = coords.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let ci = &coords[i];
        let cj = &coords[j];
        if ((ci.y > point.y) != (cj.y > point.y))
            && (point.x < (cj.x - ci.x) * (point.y - ci.y) / (cj.y - ci.y) + ci.x)
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

// ---------------------------------------------------------------------------
// Polygon validation
// ---------------------------------------------------------------------------

/// Validate a polygon, returning all detected issues.
///
/// Checks performed (in order):
/// 1. Unclosed rings (exterior + holes)
/// 2. Too-few-points (min 4 for a closed ring)
/// 3. Zero-area rings
/// 4. Duplicate consecutive points
/// 5. Self-intersections
/// 6. Hole orientation (holes must be CW, i.e. opposite to exterior)
/// 7. Hole outside exterior (centroid test)
pub fn validate_polygon(polygon: &Polygon) -> ValidationResult {
    let mut result = ValidationResult::new();

    // --- Exterior ring ---
    validate_single_ring(&polygon.exterior, &mut result);

    // --- Holes ---
    let ext_is_ccw = validate_ring_orientation(&polygon.exterior);
    for hole in &polygon.holes {
        validate_single_ring(hole, &mut result);

        // Hole must have opposite orientation to exterior.
        let hole_is_ccw = validate_ring_orientation(hole);
        if ext_is_ccw == hole_is_ccw {
            result.push(ValidationIssue::InvalidHoleOrientation);
        }

        // Hole centroid should lie inside the exterior ring.
        if !hole_centroid_inside_exterior(hole, &polygon.exterior) {
            result.push(ValidationIssue::HoleOutsideExterior);
        }
    }

    result
}

/// Validate a single ring (shared logic for exterior + holes).
fn validate_single_ring(ring: &Ring, result: &mut ValidationResult) {
    // Closure
    if let Some(issue) = validate_ring_closure(ring) {
        result.push(issue);
    }

    // Too few points
    if ring.len() < 4 {
        result.push(ValidationIssue::TooFewPoints);
    }

    // Zero area
    if signed_area(ring).abs() < 1e-10 {
        result.push(ValidationIssue::ZeroAreaRing);
    }

    // Duplicate consecutive
    for issue in validate_no_duplicate_consecutive(ring) {
        result.push(issue);
    }

    // Self-intersection
    for issue in validate_no_self_intersection(ring) {
        result.push(issue);
    }
}

/// Check whether the centroid of a hole lies inside the exterior ring.
fn hole_centroid_inside_exterior(hole: &Ring, exterior: &Ring) -> bool {
    let coords = hole.coords();
    if coords.is_empty() {
        return false;
    }
    let n = coords.len() as f64;
    let cx = coords.iter().map(|c| c.x).sum::<f64>() / n;
    let cy = coords.iter().map(|c| c.y).sum::<f64>() / n;
    point_in_ring(&Coord::new(cx, cy), exterior)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Epsilon-equal comparison for two coordinates.
#[inline]
fn coord_eq(a: Coord, b: Coord) -> bool {
    (a.x - b.x).abs() < 1e-10 && (a.y - b.y).abs() < 1e-10
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn square_ring() -> Ring {
        Ring::new(vec![
            Coord::new(0.0, 0.0),
            Coord::new(1.0, 0.0),
            Coord::new(1.0, 1.0),
            Coord::new(0.0, 1.0),
            Coord::new(0.0, 0.0),
        ])
    }

    #[test]
    fn signed_area_ccw_square() {
        let area = signed_area(&square_ring());
        assert!((area - 1.0).abs() < 1e-10);
    }

    #[test]
    fn signed_area_cw_square() {
        let ring = Ring::new(vec![
            Coord::new(0.0, 0.0),
            Coord::new(0.0, 1.0),
            Coord::new(1.0, 1.0),
            Coord::new(1.0, 0.0),
            Coord::new(0.0, 0.0),
        ]);
        assert!((signed_area(&ring) + 1.0).abs() < 1e-10);
    }

    #[test]
    fn ring_closure_valid() {
        assert!(validate_ring_closure(&square_ring()).is_none());
    }

    #[test]
    fn ring_closure_invalid() {
        let ring = Ring::new(vec![
            Coord::new(0.0, 0.0),
            Coord::new(1.0, 0.0),
            Coord::new(1.0, 1.0),
        ]);
        assert_eq!(
            validate_ring_closure(&ring),
            Some(ValidationIssue::UnclosedRing)
        );
    }

    #[test]
    fn orientation_ccw() {
        assert!(validate_ring_orientation(&square_ring()));
    }

    #[test]
    fn segments_cross() {
        assert!(segments_intersect(
            Coord::new(0.0, 0.0),
            Coord::new(2.0, 2.0),
            Coord::new(0.0, 2.0),
            Coord::new(2.0, 0.0),
        ));
    }

    #[test]
    fn segments_parallel_no_cross() {
        assert!(!segments_intersect(
            Coord::new(0.0, 0.0),
            Coord::new(1.0, 0.0),
            Coord::new(0.0, 1.0),
            Coord::new(1.0, 1.0),
        ));
    }

    #[test]
    fn valid_square_polygon() {
        let poly = Polygon::simple(square_ring());
        let res = validate_polygon(&poly);
        assert!(res.is_valid(), "issues: {:?}", res.issues());
    }

    #[test]
    fn figure_eight_self_intersection() {
        // A figure-8 ring: edges cross in the middle.
        let ring = Ring::new(vec![
            Coord::new(0.0, 0.0),
            Coord::new(2.0, 2.0),
            Coord::new(2.0, 0.0),
            Coord::new(0.0, 2.0),
            Coord::new(0.0, 0.0),
        ]);
        let issues = validate_no_self_intersection(&ring);
        assert!(!issues.is_empty());
    }

    #[test]
    fn zero_area_collinear() {
        let ring = Ring::new(vec![
            Coord::new(0.0, 0.0),
            Coord::new(1.0, 0.0),
            Coord::new(2.0, 0.0),
            Coord::new(0.0, 0.0),
        ]);
        let poly = Polygon::simple(ring);
        let res = validate_polygon(&poly);
        assert!(res.issues().contains(&ValidationIssue::ZeroAreaRing));
    }

    #[test]
    fn too_few_points() {
        let ring = Ring::new(vec![
            Coord::new(0.0, 0.0),
            Coord::new(1.0, 0.0),
            Coord::new(0.0, 0.0),
        ]);
        let poly = Polygon::simple(ring);
        let res = validate_polygon(&poly);
        assert!(res.issues().contains(&ValidationIssue::TooFewPoints));
    }
}
