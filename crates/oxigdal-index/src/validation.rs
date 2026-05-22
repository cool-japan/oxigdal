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
    /// The interiors of two parts of a multi-polygon overlap.
    PartsOverlapInterior {
        /// Index of the first part.
        part_a: usize,
        /// Index of the second part.
        part_b: usize,
    },
    /// Two parts share an edge with opposite orientations (i.e. they overlap on
    /// that edge in the topological sense rather than merely touching).
    SharedEdgeUsesOppositeOrientation {
        /// Index of the first part.
        part_a: usize,
        /// Index of the second part.
        part_b: usize,
        /// Start coordinate of the shared edge.
        edge_start: Coord,
        /// End coordinate of the shared edge.
        edge_end: Coord,
    },
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
// MultiPolygon
// ---------------------------------------------------------------------------

/// A multi-polygon: an ordered list of (possibly disjoint, possibly edge-sharing)
/// polygons.
///
/// In OGC Simple Features semantics the parts of a `MultiPolygon` may touch on
/// boundaries (shared edges or vertices) but must not overlap in their
/// interiors.  [`validate_multipolygon`] enforces both per-part validity and
/// these cross-part topological constraints.
#[derive(Debug, Clone)]
pub struct MultiPolygon {
    parts: Vec<Polygon>,
}

impl MultiPolygon {
    /// Create a multi-polygon from a list of polygons.
    pub fn new(parts: Vec<Polygon>) -> Self {
        Self { parts }
    }

    /// The parts of this multi-polygon.
    #[inline]
    pub fn parts(&self) -> &[Polygon] {
        &self.parts
    }

    /// Number of parts.
    #[inline]
    pub fn len(&self) -> usize {
        self.parts.len()
    }

    /// Whether this multi-polygon has no parts.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }
}

/// Epsilon used by the multi-polygon helpers for coordinate equality.
///
/// Slightly larger than the [`coord_eq`] epsilon to be more tolerant of
/// rounding when two parts have been authored against a shared boundary.
const MULTIPOLYGON_EPS: f64 = 1e-9;

/// Epsilon-equal comparison for two coordinates (multi-polygon helpers).
#[inline]
fn approx_coord_eq(a: Coord, b: Coord) -> bool {
    (a.x - b.x).abs() < MULTIPOLYGON_EPS && (a.y - b.y).abs() < MULTIPOLYGON_EPS
}

/// Validate a multi-polygon: each part valid, parts don't overlap interiors.
///
/// The checks performed are:
///
/// 1. Each part is validated individually with [`validate_polygon`]; any
///    issues are appended to the result.
/// 2. For every pair of parts `(i, j)` with `i < j`:
///    * If the parts' interiors overlap, a
///      [`ValidationIssue::PartsOverlapInterior`] is recorded.
///    * If the parts share an edge with the same direction (rather than the
///      opposite direction that is normal for touching coverage tiles) a
///      [`ValidationIssue::SharedEdgeUsesOppositeOrientation`] is recorded.
///
/// Shared edges between adjacent parts are valid topology (typical for
/// coverage maps).  The misleadingly named
/// `SharedEdgeUsesOppositeOrientation` variant fires when two adjacent
/// parts use the *same* directional traversal of a shared edge — which means
/// they are on the same side of the edge, indicating an overlap rather than
/// a clean shared boundary.
pub fn validate_multipolygon(mp: &MultiPolygon) -> ValidationResult {
    let mut result = ValidationResult::new();

    // Per-part validation
    for part in mp.parts.iter() {
        let part_result = validate_polygon(part);
        for issue in part_result.issues() {
            result.push(issue.clone());
        }
    }

    // Cross-part checks
    for i in 0..mp.parts.len() {
        for j in (i + 1)..mp.parts.len() {
            if polygon_interiors_overlap(&mp.parts[i], &mp.parts[j]) {
                result.push(ValidationIssue::PartsOverlapInterior {
                    part_a: i,
                    part_b: j,
                });
            }

            // Shared-edge orientation check.  A clean shared boundary between
            // two CCW exteriors traverses the edge in *opposite* directions;
            // if two CCW exteriors traverse the same edge in the *same*
            // direction, both polygons lie on the same side of that edge and
            // therefore overlap on it.
            for (edge_start, edge_end) in
                shared_edges_same_direction(&mp.parts[i].exterior, &mp.parts[j].exterior)
            {
                result.push(ValidationIssue::SharedEdgeUsesOppositeOrientation {
                    part_a: i,
                    part_b: j,
                    edge_start,
                    edge_end,
                });
            }
        }
    }

    result
}

/// Collect all `(edge_start, edge_end)` pairs for which segments of `ring_a`
/// and `ring_b` coincide and traverse the edge in the same direction.
///
/// Each match indicates an edge that both rings walk identically, which — for
/// CCW exteriors — implies the polygons overlap on that edge.
fn shared_edges_same_direction(ring_a: &Ring, ring_b: &Ring) -> Vec<(Coord, Coord)> {
    let mut out = Vec::new();
    let segs_a = ring_segments(ring_a);
    let segs_b = ring_segments(ring_b);
    for (a0, a1) in &segs_a {
        for (b0, b1) in &segs_b {
            if approx_coord_eq(*a0, *b0) && approx_coord_eq(*a1, *b1) {
                out.push((*a0, *a1));
            }
        }
    }
    out
}

/// True if two rings share any segment (direction-insensitive).
///
/// Exposed at crate visibility for unit tests; not yet used by the public
/// API of [`validate_multipolygon`] (which relies on the directional variant
/// [`shared_edges_same_direction`] for its overlap heuristic).
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn rings_share_edge(ring_a: &Ring, ring_b: &Ring) -> bool {
    let segs_a = ring_segments(ring_a);
    let segs_b = ring_segments(ring_b);

    for (a0, a1) in &segs_a {
        for (b0, b1) in &segs_b {
            let same_direction = approx_coord_eq(*a0, *b0) && approx_coord_eq(*a1, *b1);
            let opposite_direction = approx_coord_eq(*a0, *b1) && approx_coord_eq(*a1, *b0);
            if same_direction || opposite_direction {
                return true;
            }
        }
    }
    false
}

/// True if the interiors of `p1` and `p2` overlap.
///
/// Detection strategy:
///
/// 1. Any pair of exterior segments that *properly* cross (excluding shared
///    edges and endpoint-only touches) implies an interior overlap.
/// 2. Otherwise, if one polygon's exterior centroid lies strictly inside the
///    other polygon's exterior (and not inside any of its holes), the
///    interiors overlap.
///
/// Exposed at crate visibility for unit tests in the integration test crate.
pub(crate) fn polygon_interiors_overlap(p1: &Polygon, p2: &Polygon) -> bool {
    let e1 = &p1.exterior;
    let e2 = &p2.exterior;

    let segs1 = ring_segments(e1);
    let segs2 = ring_segments(e2);
    for (a0, a1) in &segs1 {
        for (b0, b1) in &segs2 {
            if segments_share_endpoint(*a0, *a1, *b0, *b1) {
                continue;
            }
            if segments_intersect(*a0, *a1, *b0, *b1) {
                return true;
            }
        }
    }

    // Centroid-inside-other test (both directions).
    if let Some(c1) = ring_centroid(e1)
        && ring_contains_point(e2, c1)
        && !p2.holes.iter().any(|h| ring_contains_point(h, c1))
    {
        return true;
    }
    if let Some(c2) = ring_centroid(e2)
        && ring_contains_point(e1, c2)
        && !p1.holes.iter().any(|h| ring_contains_point(h, c2))
    {
        return true;
    }

    false
}

/// Collect segments of a ring as `(start, end)` pairs.
fn ring_segments(ring: &Ring) -> Vec<(Coord, Coord)> {
    ring.coords().windows(2).map(|w| (w[0], w[1])).collect()
}

/// True if the two segments share at least one endpoint coordinate.
fn segments_share_endpoint(a0: Coord, a1: Coord, b0: Coord, b1: Coord) -> bool {
    approx_coord_eq(a0, b0)
        || approx_coord_eq(a0, b1)
        || approx_coord_eq(a1, b0)
        || approx_coord_eq(a1, b1)
}

/// Arithmetic-mean centroid of a ring's coordinates (returns `None` for rings
/// with fewer than three points).
fn ring_centroid(ring: &Ring) -> Option<Coord> {
    let coords = ring.coords();
    if coords.len() < 3 {
        return None;
    }
    let mut x = 0.0_f64;
    let mut y = 0.0_f64;
    for c in coords {
        x += c.x;
        y += c.y;
    }
    let n = coords.len() as f64;
    Some(Coord::new(x / n, y / n))
}

/// Thin wrapper around the internal `point_in_ring` so callers can pass a
/// value rather than a reference.
#[inline]
fn ring_contains_point(ring: &Ring, p: Coord) -> bool {
    point_in_ring(&p, ring)
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

    // ----- multi-polygon crate-private helpers --------------------------

    fn unit_square_at(ox: f64, oy: f64) -> Polygon {
        let r = Ring::new(vec![
            Coord::new(ox, oy),
            Coord::new(ox + 1.0, oy),
            Coord::new(ox + 1.0, oy + 1.0),
            Coord::new(ox, oy + 1.0),
            Coord::new(ox, oy),
        ]);
        Polygon::simple(r)
    }

    #[test]
    fn test_multipolygon_rings_share_edge_helper_detects_same_direction() {
        // Two CCW unit squares that share the edge x = 1 (between them).
        let a = unit_square_at(0.0, 0.0);
        let b = unit_square_at(1.0, 0.0);
        // Direction-insensitive: a's right edge is (1,0)→(1,1), b's left edge
        // is (1,1)→(1,0).  rings_share_edge should detect this in either
        // orientation.
        assert!(rings_share_edge(&a.exterior, &b.exterior));

        // Same-direction shared edge: construct a deliberately bad pair where
        // both rings traverse the seam (1,0)→(1,1).
        let bad = Polygon::simple(Ring::new(vec![
            Coord::new(1.0, 0.0),
            Coord::new(1.0, 1.0),
            Coord::new(2.0, 1.0),
            Coord::new(2.0, 0.0),
            Coord::new(1.0, 0.0),
        ]));
        // a contains (1,0)→(1,1)?  a's right edge in CCW order is
        // (1,0)→(1,1).  bad's first segment is (1,0)→(1,1).  Therefore
        // shared_edges_same_direction must find this.
        let same_dir = shared_edges_same_direction(&a.exterior, &bad.exterior);
        assert!(
            !same_dir.is_empty(),
            "expected a same-direction shared edge between a and bad"
        );
    }

    #[test]
    fn test_multipolygon_polygon_interiors_overlap_detects_centroid_inside() {
        // Big square (0,0)-(4,4) and a small square (1,1)-(2,2) fully inside.
        // Centroid of the inner square lies inside the outer; overlap detected.
        let outer = Polygon::simple(Ring::new(vec![
            Coord::new(0.0, 0.0),
            Coord::new(4.0, 0.0),
            Coord::new(4.0, 4.0),
            Coord::new(0.0, 4.0),
            Coord::new(0.0, 0.0),
        ]));
        let inner = unit_square_at(1.0, 1.0);
        assert!(polygon_interiors_overlap(&outer, &inner));
        assert!(polygon_interiors_overlap(&inner, &outer));

        // Two disjoint squares should not overlap.
        let a = unit_square_at(0.0, 0.0);
        let b = unit_square_at(5.0, 5.0);
        assert!(!polygon_interiors_overlap(&a, &b));
    }
}
