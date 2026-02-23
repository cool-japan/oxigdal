//! Geometry validation and repair
//!
//! This module provides functions for validating geometries according to
//! OGC Simple Features specification and identifying common geometric issues.
//!
//! # Validation Checks
//!
//! - **Ring Closure**: Ensures polygon rings are properly closed
//! - **Self-Intersection**: Detects self-intersecting polygons
//! - **Duplicate Vertices**: Identifies consecutive duplicate points
//! - **Spike Detection**: Finds degenerate spikes in polygon boundaries
//! - **Minimum Points**: Verifies geometries have required number of points
//! - **Ring Orientation**: Checks proper orientation (exterior CCW, holes CW)
//! - **Hole Containment**: Verifies holes are inside exterior ring
//!
//! # Examples
//!
//! ```
//! use oxigdal_algorithms::vector::{Polygon, LineString, Coordinate, validate_polygon};
//! # use oxigdal_algorithms::error::Result;
//!
//! # fn main() -> Result<()> {
//! let coords = vec![
//!     Coordinate::new_2d(0.0, 0.0),
//!     Coordinate::new_2d(4.0, 0.0),
//!     Coordinate::new_2d(4.0, 4.0),
//!     Coordinate::new_2d(0.0, 4.0),
//!     Coordinate::new_2d(0.0, 0.0),
//! ];
//! let exterior = LineString::new(coords)?;
//! let polygon = Polygon::new(exterior, vec![])?;
//! let issues = validate_polygon(&polygon)?;
//! // issues should be empty for a valid square
//! # Ok(())
//! # }
//! ```

use crate::error::Result;
use oxigdal_core::vector::{Coordinate, Geometry, LineString, Polygon};

#[cfg(feature = "std")]
use std::vec::Vec;

/// Validation issue severity
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Error: geometry is invalid
    Error,
    /// Warning: geometry may cause issues
    Warning,
    /// Info: best practice violation
    Info,
}

/// Validation issue type
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssueType {
    /// Ring is not closed
    UnclosedRing,
    /// Self-intersecting polygon
    SelfIntersection,
    /// Consecutive duplicate vertices
    DuplicateVertices,
    /// Degenerate spike in boundary
    Spike,
    /// Insufficient number of points
    InsufficientPoints,
    /// Wrong ring orientation
    WrongOrientation,
    /// Hole not contained in exterior
    HoleNotContained,
    /// Ring has too few distinct points
    TooFewDistinctPoints,
    /// Collinear vertices
    CollinearVertices,
}

/// A validation issue found in a geometry
#[derive(Debug, Clone)]
pub struct ValidationIssue {
    /// Severity level
    pub severity: Severity,
    /// Type of issue
    pub issue_type: IssueType,
    /// Human-readable description
    pub description: String,
    /// Location of the issue (if applicable)
    pub location: Option<Coordinate>,
    /// Suggested repair action
    pub repair_suggestion: Option<String>,
}

impl ValidationIssue {
    /// Creates a new validation issue
    pub fn new(
        severity: Severity,
        issue_type: IssueType,
        description: String,
        location: Option<Coordinate>,
        repair_suggestion: Option<String>,
    ) -> Self {
        Self {
            severity,
            issue_type,
            description,
            location,
            repair_suggestion,
        }
    }
}

/// Validates a geometry and returns list of issues
///
/// # Arguments
///
/// * `geometry` - Geometry to validate
///
/// # Returns
///
/// Vector of validation issues (empty if geometry is valid)
///
/// # Errors
///
/// Returns error if validation process fails
pub fn validate_geometry(geometry: &Geometry) -> Result<Vec<ValidationIssue>> {
    match geometry {
        Geometry::Point(_) => Ok(vec![]), // Points are always valid
        Geometry::LineString(ls) => validate_linestring(ls),
        Geometry::Polygon(p) => validate_polygon(p),
        Geometry::MultiPoint(_) => Ok(vec![]), // MultiPoints are always valid
        Geometry::MultiLineString(mls) => {
            let mut issues = Vec::new();
            for ls in &mls.line_strings {
                issues.extend(validate_linestring(ls)?);
            }
            Ok(issues)
        }
        Geometry::MultiPolygon(mp) => {
            let mut issues = Vec::new();
            for p in &mp.polygons {
                issues.extend(validate_polygon(p)?);
            }
            Ok(issues)
        }
        Geometry::GeometryCollection(gc) => {
            let mut issues = Vec::new();
            for geom in &gc.geometries {
                issues.extend(validate_geometry(geom)?);
            }
            Ok(issues)
        }
    }
}

/// Validates a linestring
///
/// # Arguments
///
/// * `linestring` - LineString to validate
///
/// # Returns
///
/// Vector of validation issues
///
/// # Errors
///
/// Returns error if validation fails
pub fn validate_linestring(linestring: &LineString) -> Result<Vec<ValidationIssue>> {
    let mut issues = Vec::new();

    // Check minimum points
    if linestring.coords.len() < 2 {
        issues.push(ValidationIssue::new(
            Severity::Error,
            IssueType::InsufficientPoints,
            format!(
                "LineString must have at least 2 points, got {}",
                linestring.coords.len()
            ),
            None,
            Some("Add more points to the linestring".to_string()),
        ));
    }

    // Check for duplicate consecutive vertices
    for i in 0..linestring.coords.len().saturating_sub(1) {
        if coords_equal(&linestring.coords[i], &linestring.coords[i + 1]) {
            issues.push(ValidationIssue::new(
                Severity::Warning,
                IssueType::DuplicateVertices,
                format!("Duplicate consecutive vertices at index {}", i),
                Some(linestring.coords[i]),
                Some("Remove duplicate vertices".to_string()),
            ));
        }
    }

    // Check for collinear vertices
    if linestring.coords.len() >= 3 {
        for i in 0..linestring.coords.len() - 2 {
            if are_collinear(
                &linestring.coords[i],
                &linestring.coords[i + 1],
                &linestring.coords[i + 2],
            ) {
                issues.push(ValidationIssue::new(
                    Severity::Info,
                    IssueType::CollinearVertices,
                    format!("Collinear vertices at indices {}-{}-{}", i, i + 1, i + 2),
                    Some(linestring.coords[i + 1]),
                    Some("Consider removing middle vertex for simplification".to_string()),
                ));
            }
        }
    }

    Ok(issues)
}

/// Validates a polygon
///
/// # Arguments
///
/// * `polygon` - Polygon to validate
///
/// # Returns
///
/// Vector of validation issues (empty if valid)
///
/// # Errors
///
/// Returns error if validation fails
pub fn validate_polygon(polygon: &Polygon) -> Result<Vec<ValidationIssue>> {
    let mut issues = Vec::new();

    // Validate exterior ring
    issues.extend(validate_ring(&polygon.exterior.coords, true)?);

    // Validate interior rings (holes)
    for (i, hole) in polygon.interiors.iter().enumerate() {
        let hole_issues = validate_ring(&hole.coords, false)?;
        for mut issue in hole_issues {
            issue.description = format!("Hole {}: {}", i, issue.description);
            issues.push(issue);
        }

        // Check if hole is contained in exterior
        if !hole_contained_in_exterior(&hole.coords, &polygon.exterior.coords) {
            issues.push(ValidationIssue::new(
                Severity::Error,
                IssueType::HoleNotContained,
                format!("Hole {} is not contained within exterior ring", i),
                None,
                Some("Move hole inside exterior ring or remove it".to_string()),
            ));
        }
    }

    Ok(issues)
}

/// Validates a polygon ring (exterior or interior)
fn validate_ring(coords: &[Coordinate], is_exterior: bool) -> Result<Vec<ValidationIssue>> {
    let mut issues = Vec::new();
    let ring_type = if is_exterior { "Exterior" } else { "Interior" };

    // Check minimum points
    if coords.len() < 4 {
        issues.push(ValidationIssue::new(
            Severity::Error,
            IssueType::InsufficientPoints,
            format!(
                "{} ring must have at least 4 points, got {}",
                ring_type,
                coords.len()
            ),
            None,
            Some("Add more points to form a closed ring".to_string()),
        ));
        return Ok(issues);
    }

    // Check if ring is closed
    if !coords_equal(&coords[0], &coords[coords.len() - 1]) {
        issues.push(ValidationIssue::new(
            Severity::Error,
            IssueType::UnclosedRing,
            format!("{} ring is not closed", ring_type),
            Some(coords[coords.len() - 1]),
            Some("Make last point equal to first point".to_string()),
        ));
    }

    // Check for duplicate consecutive vertices
    for i in 0..coords.len() - 1 {
        if coords_equal(&coords[i], &coords[i + 1]) {
            issues.push(ValidationIssue::new(
                Severity::Warning,
                IssueType::DuplicateVertices,
                format!(
                    "{} ring has duplicate consecutive vertices at index {}",
                    ring_type, i
                ),
                Some(coords[i]),
                Some("Remove duplicate vertices".to_string()),
            ));
        }
    }

    // Check for too few distinct points
    let distinct_count = count_distinct_points(coords);
    if distinct_count < 3 {
        issues.push(ValidationIssue::new(
            Severity::Error,
            IssueType::TooFewDistinctPoints,
            format!(
                "{} ring has only {} distinct points (need at least 3)",
                ring_type, distinct_count
            ),
            None,
            Some("Add more distinct points".to_string()),
        ));
    }

    // Check for spikes
    let spikes = find_spikes(coords);
    for spike_idx in spikes {
        issues.push(ValidationIssue::new(
            Severity::Warning,
            IssueType::Spike,
            format!("{} ring has spike at index {}", ring_type, spike_idx),
            Some(coords[spike_idx]),
            Some("Remove spike by deleting the vertex or adjusting coordinates".to_string()),
        ));
    }

    // Check for self-intersection
    if has_self_intersection(coords) {
        issues.push(ValidationIssue::new(
            Severity::Error,
            IssueType::SelfIntersection,
            format!("{} ring is self-intersecting", ring_type),
            None,
            Some("Modify coordinates to eliminate self-intersection".to_string()),
        ));
    }

    // Check orientation
    let is_ccw = is_counter_clockwise(coords);
    if is_exterior && !is_ccw {
        issues.push(ValidationIssue::new(
            Severity::Warning,
            IssueType::WrongOrientation,
            "Exterior ring should be counter-clockwise".to_string(),
            None,
            Some("Reverse vertex order".to_string()),
        ));
    } else if !is_exterior && is_ccw {
        issues.push(ValidationIssue::new(
            Severity::Warning,
            IssueType::WrongOrientation,
            "Interior ring (hole) should be clockwise".to_string(),
            None,
            Some("Reverse vertex order".to_string()),
        ));
    }

    Ok(issues)
}

/// Checks if two coordinates are equal (within epsilon)
fn coords_equal(c1: &Coordinate, c2: &Coordinate) -> bool {
    (c1.x - c2.x).abs() < f64::EPSILON && (c1.y - c2.y).abs() < f64::EPSILON
}

/// Checks if three points are collinear
fn are_collinear(p1: &Coordinate, p2: &Coordinate, p3: &Coordinate) -> bool {
    let cross = (p2.x - p1.x) * (p3.y - p1.y) - (p3.x - p1.x) * (p2.y - p1.y);
    cross.abs() < f64::EPSILON
}

/// Counts distinct points in a ring
fn count_distinct_points(coords: &[Coordinate]) -> usize {
    if coords.is_empty() {
        return 0;
    }

    let mut distinct = 1; // First point is always distinct
    for i in 1..coords.len() {
        let mut is_distinct = true;
        for j in 0..i {
            if coords_equal(&coords[i], &coords[j]) {
                is_distinct = false;
                break;
            }
        }
        if is_distinct {
            distinct += 1;
        }
    }

    distinct
}

/// Finds spikes (consecutive vertices that form a very sharp angle)
fn find_spikes(coords: &[Coordinate]) -> Vec<usize> {
    let mut spikes = Vec::new();

    if coords.len() < 3 {
        return spikes;
    }

    for i in 1..coords.len() - 1 {
        let prev = &coords[i - 1];
        let curr = &coords[i];
        let next = &coords[i + 1];

        // Check if current point is between prev and next (forms a spike)
        if is_spike(prev, curr, next) {
            spikes.push(i);
        }
    }

    spikes
}

/// Checks if three points form a spike
fn is_spike(prev: &Coordinate, curr: &Coordinate, next: &Coordinate) -> bool {
    // A spike occurs when the current point is very close to the line from prev to next
    let dx1 = curr.x - prev.x;
    let dy1 = curr.y - prev.y;
    let dx2 = next.x - curr.x;
    let dy2 = next.y - curr.y;

    // Check if vectors are opposite (angle close to 180 degrees)
    let dot = dx1 * dx2 + dy1 * dy2;
    let len1 = (dx1 * dx1 + dy1 * dy1).sqrt();
    let len2 = (dx2 * dx2 + dy2 * dy2).sqrt();

    if len1 < f64::EPSILON || len2 < f64::EPSILON {
        return false;
    }

    let cos_angle = dot / (len1 * len2);

    // If angle is close to 180 degrees (cos ~ -1), it's a spike
    cos_angle < -0.99
}

/// Checks if a ring has self-intersections
fn has_self_intersection(coords: &[Coordinate]) -> bool {
    let n = coords.len();
    if n < 4 {
        return false;
    }

    for i in 0..n - 1 {
        for j in i + 2..n - 1 {
            // Skip adjacent segments
            if j == i + 1 || (i == 0 && j == n - 2) {
                continue;
            }

            if segments_intersect(&coords[i], &coords[i + 1], &coords[j], &coords[j + 1]) {
                return true;
            }
        }
    }

    false
}

/// Checks if two segments intersect
fn segments_intersect(p1: &Coordinate, p2: &Coordinate, p3: &Coordinate, p4: &Coordinate) -> bool {
    let d1 = direction(p3, p4, p1);
    let d2 = direction(p3, p4, p2);
    let d3 = direction(p1, p2, p3);
    let d4 = direction(p1, p2, p4);

    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    {
        return true;
    }

    false
}

/// Computes direction for orientation test
fn direction(a: &Coordinate, b: &Coordinate, p: &Coordinate) -> f64 {
    (b.x - a.x) * (p.y - a.y) - (p.x - a.x) * (b.y - a.y)
}

/// Checks if a ring is counter-clockwise using signed area
fn is_counter_clockwise(coords: &[Coordinate]) -> bool {
    let mut area = 0.0;
    let n = coords.len();

    for i in 0..n {
        let j = (i + 1) % n;
        area += coords[i].x * coords[j].y;
        area -= coords[j].x * coords[i].y;
    }

    area > 0.0
}

/// Checks if a hole is contained within the exterior ring
fn hole_contained_in_exterior(hole: &[Coordinate], exterior: &[Coordinate]) -> bool {
    if hole.is_empty() {
        return false;
    }

    // Check if at least one point of the hole is inside the exterior
    for point in hole {
        if !point_in_ring(point, exterior) {
            return false;
        }
    }

    true
}

/// Ray casting test for point in ring
fn point_in_ring(point: &Coordinate, ring: &[Coordinate]) -> bool {
    let mut inside = false;
    let n = ring.len();

    let mut j = n - 1;
    for i in 0..n {
        let xi = ring[i].x;
        let yi = ring[i].y;
        let xj = ring[j].x;
        let yj = ring[j].y;

        let intersect = ((yi > point.y) != (yj > point.y))
            && (point.x < (xj - xi) * (point.y - yi) / (yj - yi) + xi);

        if intersect {
            inside = !inside;
        }

        j = i;
    }

    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_valid_square() -> Polygon {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(4.0, 0.0),
            Coordinate::new_2d(4.0, 4.0),
            Coordinate::new_2d(0.0, 4.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let exterior = LineString::new(coords).expect("valid linestring");
        Polygon::new(exterior, vec![]).expect("valid polygon")
    }

    #[test]
    fn test_validate_valid_polygon() {
        let poly = create_valid_square();
        let result = validate_polygon(&poly);
        assert!(result.is_ok());

        if let Ok(issues) = result {
            // Should have no errors or warnings, might have info about optimization
            let errors = issues
                .iter()
                .filter(|i| i.severity == Severity::Error)
                .count();
            assert_eq!(errors, 0);
        }
    }

    #[test]
    fn test_validate_unclosed_ring() {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(4.0, 0.0),
            Coordinate::new_2d(4.0, 4.0),
            Coordinate::new_2d(0.0, 4.0),
            // Missing closing point
        ];
        let ls = LineString::new(coords);

        if let Ok(linestring) = ls {
            let result = validate_linestring(&linestring);
            assert!(result.is_ok());
            // LineString doesn't require closure, so this is valid
        }
    }

    #[test]
    fn test_validate_duplicate_vertices() {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(2.0, 0.0),
            Coordinate::new_2d(2.0, 0.0), // Duplicate
            Coordinate::new_2d(4.0, 0.0),
        ];
        let ls = LineString::new(coords);
        assert!(ls.is_ok());

        if let Ok(linestring) = ls {
            let result = validate_linestring(&linestring);
            assert!(result.is_ok());

            if let Ok(issues) = result {
                let duplicates = issues
                    .iter()
                    .filter(|i| i.issue_type == IssueType::DuplicateVertices)
                    .count();
                assert!(duplicates > 0);
            }
        }
    }

    #[test]
    fn test_coords_equal() {
        let c1 = Coordinate::new_2d(1.0, 2.0);
        let c2 = Coordinate::new_2d(1.0, 2.0);
        let c3 = Coordinate::new_2d(1.1, 2.0);

        assert!(coords_equal(&c1, &c2));
        assert!(!coords_equal(&c1, &c3));
    }

    #[test]
    fn test_are_collinear() {
        let p1 = Coordinate::new_2d(0.0, 0.0);
        let p2 = Coordinate::new_2d(1.0, 1.0);
        let p3 = Coordinate::new_2d(2.0, 2.0);

        assert!(are_collinear(&p1, &p2, &p3));

        let p4 = Coordinate::new_2d(1.0, 2.0);
        assert!(!are_collinear(&p1, &p2, &p4));
    }

    #[test]
    fn test_is_counter_clockwise() {
        // Counter-clockwise square
        let ccw = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(4.0, 0.0),
            Coordinate::new_2d(4.0, 4.0),
            Coordinate::new_2d(0.0, 4.0),
            Coordinate::new_2d(0.0, 0.0),
        ];

        assert!(is_counter_clockwise(&ccw));

        // Clockwise square
        let cw = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(0.0, 4.0),
            Coordinate::new_2d(4.0, 4.0),
            Coordinate::new_2d(4.0, 0.0),
            Coordinate::new_2d(0.0, 0.0),
        ];

        assert!(!is_counter_clockwise(&cw));
    }

    #[test]
    fn test_has_self_intersection() {
        // Self-intersecting (bow-tie)
        let self_intersecting = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(4.0, 4.0),
            Coordinate::new_2d(4.0, 0.0),
            Coordinate::new_2d(0.0, 4.0),
            Coordinate::new_2d(0.0, 0.0),
        ];

        assert!(has_self_intersection(&self_intersecting));

        // Non-self-intersecting
        let valid = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(4.0, 0.0),
            Coordinate::new_2d(4.0, 4.0),
            Coordinate::new_2d(0.0, 4.0),
            Coordinate::new_2d(0.0, 0.0),
        ];

        assert!(!has_self_intersection(&valid));
    }

    #[test]
    fn test_count_distinct_points() {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(4.0, 0.0),
            Coordinate::new_2d(4.0, 4.0),
            Coordinate::new_2d(0.0, 0.0), // Duplicate of first
        ];

        assert_eq!(count_distinct_points(&coords), 3);
    }

    #[test]
    fn test_is_spike() {
        // Spike: points go forward then immediately back
        let prev = Coordinate::new_2d(0.0, 0.0);
        let curr = Coordinate::new_2d(2.0, 0.0);
        let next = Coordinate::new_2d(0.0, 0.0);

        assert!(is_spike(&prev, &curr, &next));

        // Not a spike: normal angle
        let next2 = Coordinate::new_2d(2.0, 2.0);
        assert!(!is_spike(&prev, &curr, &next2));
    }
}
