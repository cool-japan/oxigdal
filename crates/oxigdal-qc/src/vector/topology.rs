//! Vector topology validation.
//!
//! This module provides quality control checks for vector topology,
//! including error detection, invalid geometry identification, and repair suggestions.

use crate::error::{QcIssue, QcResult, Severity};
use crate::vector::violations::{TopologyOptions, TopologyViolation};
use oxigdal_algorithms::vector::{
    SegmentIntersection, intersect_polygons, intersect_segment_segment,
};
use oxigdal_core::vector::{
    Coordinate, FeatureCollection, FeatureId, Geometry, LineString, Polygon,
};
use oxigdal_index::{Bbox2D, RTree};
use std::collections::HashMap;

/// Helper function to convert FeatureId to String
fn feature_id_to_string(id: &FeatureId) -> String {
    match id {
        FeatureId::Integer(i) => i.to_string(),
        FeatureId::String(s) => s.clone(),
    }
}

/// Result of topology validation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TopologyResult {
    /// Total number of features checked.
    pub feature_count: usize,

    /// Number of valid geometries.
    pub valid_geometries: usize,

    /// Number of invalid geometries.
    pub invalid_geometries: usize,

    /// Topology errors detected.
    pub topology_errors: Vec<TopologyError>,

    /// Sliver polygons detected.
    pub slivers: Vec<SliverPolygon>,

    /// Duplicate geometries detected.
    pub duplicates: Vec<DuplicateGroup>,

    /// Topology rule violations.
    pub rule_violations: Vec<RuleViolation>,

    /// Quality control issues found.
    pub issues: Vec<QcIssue>,
}

/// Topology error information.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TopologyError {
    /// Feature ID where error was found.
    pub feature_id: Option<String>,

    /// Type of topology error.
    pub error_type: TopologyErrorType,

    /// Location of the error.
    pub location: Coordinate,

    /// Severity of the error.
    pub severity: Severity,

    /// Description of the error.
    pub description: String,

    /// Suggested fix.
    pub fix_suggestion: Option<String>,
}

/// Types of topology errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TopologyErrorType {
    /// Dangling edge (line that doesn't connect).
    Dangle,

    /// Overshoot (line extends past intersection).
    Overshoot,

    /// Undershoot (line doesn't reach intersection).
    Undershoot,

    /// Self-intersection.
    SelfIntersection,

    /// Invalid ring (not closed).
    InvalidRing,

    /// Invalid polygon (less than 3 points).
    InvalidPolygon,

    /// Duplicate vertex.
    DuplicateVertex,

    /// Spike (extremely sharp angle).
    Spike,

    /// Invalid coordinate (NaN or infinite).
    InvalidCoordinate,
}

/// Sliver polygon information.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SliverPolygon {
    /// Feature ID of the sliver.
    pub feature_id: Option<String>,

    /// Area of the polygon.
    pub area: f64,

    /// Perimeter of the polygon.
    pub perimeter: f64,

    /// Compactness ratio (area / perimeter^2).
    pub compactness: f64,

    /// Width of the sliver.
    pub width: f64,

    /// Severity based on size.
    pub severity: Severity,
}

/// Duplicate geometry group.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DuplicateGroup {
    /// Feature IDs in the duplicate group.
    pub feature_ids: Vec<String>,

    /// Number of duplicates.
    pub count: usize,

    /// Geometry type.
    pub geometry_type: String,
}

/// Topology rule violation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RuleViolation {
    /// Rule that was violated.
    pub rule: TopologyRule,

    /// Feature IDs involved.
    pub feature_ids: Vec<String>,

    /// Location of the violation.
    pub location: Option<Coordinate>,

    /// Severity of the violation.
    pub severity: Severity,

    /// Description of the violation.
    pub description: String,
}

/// Topology rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TopologyRule {
    /// Polygons must not overlap.
    MustNotOverlap,

    /// Polygons must not have gaps.
    MustNotHaveGaps,

    /// Lines must not cross.
    MustNotCross,

    /// Lines must not self-overlap.
    MustNotSelfOverlap,

    /// Polygons must be covered by feature class.
    MustBeCoveredBy,

    /// Boundary must be covered by.
    BoundaryMustBeCoveredBy,

    /// Must be inside.
    MustBeInside,

    /// Points must be covered by line.
    PointsMustBeCoveredByLine,
}

/// Configuration for topology checks.
#[derive(Debug, Clone)]
pub struct TopologyConfig {
    /// Minimum area threshold for sliver detection.
    pub sliver_area_threshold: f64,

    /// Maximum compactness for sliver detection (0.0 - 1.0).
    pub sliver_compactness_threshold: f64,

    /// Tolerance for coordinate comparison (degrees or meters).
    pub coordinate_tolerance: f64,

    /// Tolerance for dangle detection.
    pub dangle_tolerance: f64,

    /// Whether to check for self-intersections.
    pub check_self_intersections: bool,

    /// Whether to check for duplicates.
    pub check_duplicates: bool,

    /// Whether to check for slivers.
    pub check_slivers: bool,

    /// Topology rules to enforce.
    pub topology_rules: Vec<TopologyRule>,
}

impl Default for TopologyConfig {
    fn default() -> Self {
        Self {
            sliver_area_threshold: 1.0,
            sliver_compactness_threshold: 0.01,
            coordinate_tolerance: 1e-9,
            dangle_tolerance: 1e-6,
            check_self_intersections: true,
            check_duplicates: true,
            check_slivers: true,
            topology_rules: vec![TopologyRule::MustNotOverlap],
        }
    }
}

/// Topology checker.
pub struct TopologyChecker {
    config: TopologyConfig,
}

impl TopologyChecker {
    /// Creates a new topology checker with default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: TopologyConfig::default(),
        }
    }

    /// Creates a new topology checker with custom configuration.
    #[must_use]
    pub fn with_config(config: TopologyConfig) -> Self {
        Self { config }
    }

    /// Validates topology of a feature collection.
    ///
    /// # Errors
    ///
    /// Returns an error if validation fails.
    pub fn validate(&self, features: &FeatureCollection) -> QcResult<TopologyResult> {
        let mut issues = Vec::new();
        let mut topology_errors = Vec::new();
        let mut valid_geometries = 0;
        let mut invalid_geometries = 0;

        // Validate individual geometries
        for feature in &features.features {
            let feature_id_str = feature.id.as_ref().map(feature_id_to_string);
            if let Some(ref geometry) = feature.geometry {
                match self.validate_geometry(geometry, &feature_id_str) {
                    Ok(errors) => {
                        if errors.is_empty() {
                            valid_geometries += 1;
                        } else {
                            invalid_geometries += 1;
                            topology_errors.extend(errors);
                        }
                    }
                    Err(e) => {
                        invalid_geometries += 1;
                        issues.push(QcIssue::new(
                            Severity::Major,
                            "topology",
                            "Geometry validation error",
                            format!("Failed to validate geometry: {}", e),
                        ));
                    }
                }
            } else {
                // Feature has no geometry
                valid_geometries += 1;
            }
        }

        // Check for duplicates
        let duplicates = if self.config.check_duplicates {
            self.find_duplicates(features)?
        } else {
            Vec::new()
        };

        for dup in &duplicates {
            issues.push(
                QcIssue::new(
                    Severity::Warning,
                    "topology",
                    "Duplicate geometries detected",
                    format!(
                        "{} duplicate {} geometries found",
                        dup.count, dup.geometry_type
                    ),
                )
                .with_suggestion("Remove or merge duplicate features"),
            );
        }

        // Check for slivers
        let slivers = if self.config.check_slivers {
            self.find_slivers(features)?
        } else {
            Vec::new()
        };

        for sliver in &slivers {
            if sliver.severity >= Severity::Minor {
                issues.push(
                    QcIssue::new(
                        sliver.severity,
                        "topology",
                        "Sliver polygon detected",
                        format!(
                            "Sliver with area {} and compactness {}",
                            sliver.area, sliver.compactness
                        ),
                    )
                    .with_location(sliver.feature_id.clone().unwrap_or_default())
                    .with_suggestion("Remove or merge sliver polygon"),
                );
            }
        }

        // Check topology rules
        let rule_violations = self.check_topology_rules(features)?;

        for violation in &rule_violations {
            issues.push(
                QcIssue::new(
                    violation.severity,
                    "topology",
                    format!("Topology rule violation: {:?}", violation.rule),
                    violation.description.clone(),
                )
                .with_suggestion("Fix geometry to comply with topology rule"),
            );
        }

        // Add issues for topology errors
        for error in &topology_errors {
            if error.severity >= Severity::Minor {
                let mut issue = QcIssue::new(
                    error.severity,
                    "topology",
                    format!("Topology error: {:?}", error.error_type),
                    error.description.clone(),
                )
                .with_location(format!("({}, {})", error.location.x, error.location.y));

                if let Some(ref fix) = error.fix_suggestion {
                    issue = issue.with_suggestion(fix.clone());
                }

                issues.push(issue);
            }
        }

        Ok(TopologyResult {
            feature_count: features.features.len(),
            valid_geometries,
            invalid_geometries,
            topology_errors,
            slivers,
            duplicates,
            rule_violations,
            issues,
        })
    }

    /// Validates a single geometry.
    fn validate_geometry(
        &self,
        geometry: &Geometry,
        feature_id: &Option<String>,
    ) -> QcResult<Vec<TopologyError>> {
        let mut errors = Vec::new();

        match geometry {
            Geometry::Point(point) => {
                errors.extend(self.validate_point(&point.coord, feature_id)?);
            }
            Geometry::LineString(linestring) => {
                errors.extend(self.validate_linestring(linestring, feature_id)?);
            }
            Geometry::Polygon(polygon) => {
                errors.extend(self.validate_polygon(polygon, feature_id)?);
            }
            Geometry::MultiPolygon(multipolygon) => {
                for polygon in &multipolygon.polygons {
                    errors.extend(self.validate_polygon(polygon, feature_id)?);
                }
            }
            Geometry::MultiLineString(multilinestring) => {
                for linestring in &multilinestring.line_strings {
                    errors.extend(self.validate_linestring(linestring, feature_id)?);
                }
            }
            _ => {
                // Other geometry types
            }
        }

        Ok(errors)
    }

    /// Validates a point coordinate.
    fn validate_point(
        &self,
        coord: &Coordinate,
        feature_id: &Option<String>,
    ) -> QcResult<Vec<TopologyError>> {
        let mut errors = Vec::new();

        if coord.x.is_nan() || coord.y.is_nan() || coord.x.is_infinite() || coord.y.is_infinite() {
            errors.push(TopologyError {
                feature_id: feature_id.clone(),
                error_type: TopologyErrorType::InvalidCoordinate,
                location: *coord,
                severity: Severity::Critical,
                description: "Invalid coordinate (NaN or infinite)".to_string(),
                fix_suggestion: Some("Remove or fix invalid coordinate".to_string()),
            });
        }

        Ok(errors)
    }

    /// Validates a linestring.
    fn validate_linestring(
        &self,
        linestring: &LineString,
        feature_id: &Option<String>,
    ) -> QcResult<Vec<TopologyError>> {
        let mut errors = Vec::new();

        if linestring.coords.len() < 2 {
            errors.push(TopologyError {
                feature_id: feature_id.clone(),
                error_type: TopologyErrorType::InvalidRing,
                location: linestring
                    .coords
                    .first()
                    .copied()
                    .unwrap_or(Coordinate::new_2d(0.0, 0.0)),
                severity: Severity::Critical,
                description: "LineString has less than 2 points".to_string(),
                fix_suggestion: Some("Add more points or remove invalid linestring".to_string()),
            });
            return Ok(errors);
        }

        // Check for invalid coordinates
        for coord in &linestring.coords {
            errors.extend(self.validate_point(coord, feature_id)?);
        }

        // Check for duplicate consecutive vertices
        for i in 0..linestring.coords.len() - 1 {
            let c1 = &linestring.coords[i];
            let c2 = &linestring.coords[i + 1];

            if self.coords_equal(c1, c2) {
                errors.push(TopologyError {
                    feature_id: feature_id.clone(),
                    error_type: TopologyErrorType::DuplicateVertex,
                    location: *c1,
                    severity: Severity::Minor,
                    description: format!("Duplicate vertex at index {}", i),
                    fix_suggestion: Some("Remove duplicate vertex".to_string()),
                });
            }
        }

        // Check for self-intersections
        if self.config.check_self_intersections && self.has_self_intersection(linestring) {
            errors.push(TopologyError {
                feature_id: feature_id.clone(),
                error_type: TopologyErrorType::SelfIntersection,
                location: linestring.coords[0],
                severity: Severity::Major,
                description: "LineString has self-intersection".to_string(),
                fix_suggestion: Some("Remove or fix self-intersection".to_string()),
            });
        }

        Ok(errors)
    }

    /// Validates a polygon.
    fn validate_polygon(
        &self,
        polygon: &Polygon,
        feature_id: &Option<String>,
    ) -> QcResult<Vec<TopologyError>> {
        let mut errors = Vec::new();

        // Validate exterior ring
        if polygon.exterior.coords.len() < 4 {
            errors.push(TopologyError {
                feature_id: feature_id.clone(),
                error_type: TopologyErrorType::InvalidPolygon,
                location: polygon
                    .exterior
                    .coords
                    .first()
                    .copied()
                    .unwrap_or(Coordinate::new_2d(0.0, 0.0)),
                severity: Severity::Critical,
                description: "Polygon has less than 4 points".to_string(),
                fix_suggestion: Some("Add more points or remove invalid polygon".to_string()),
            });
            return Ok(errors);
        }

        // Check if ring is closed
        let first = polygon.exterior.coords.first();
        let last = polygon.exterior.coords.last();

        if let (Some(f), Some(l)) = (first, last) {
            if !self.coords_equal(f, l) {
                errors.push(TopologyError {
                    feature_id: feature_id.clone(),
                    error_type: TopologyErrorType::InvalidRing,
                    location: *f,
                    severity: Severity::Critical,
                    description: "Polygon ring is not closed".to_string(),
                    fix_suggestion: Some("Close the ring by adding first point at end".to_string()),
                });
            }
        }

        // Validate exterior ring as linestring
        errors.extend(self.validate_linestring(&polygon.exterior, feature_id)?);

        // Validate interior rings
        for interior in &polygon.interiors {
            errors.extend(self.validate_linestring(interior, feature_id)?);
        }

        Ok(errors)
    }

    /// Checks if two coordinates are equal within tolerance.
    fn coords_equal(&self, c1: &Coordinate, c2: &Coordinate) -> bool {
        (c1.x - c2.x).abs() < self.config.coordinate_tolerance
            && (c1.y - c2.y).abs() < self.config.coordinate_tolerance
    }

    /// Checks if a linestring has self-intersections.
    ///
    /// Uses a pairwise segment test, filtering out adjacent segment pairs that
    /// share exactly one endpoint (which is expected and valid).
    ///
    /// Returns `None` when no violations are found; `Some(pairs)` otherwise where
    /// each pair `(i, j)` identifies the non-adjacent segment indices that cross.
    fn has_self_intersection(&self, linestring: &LineString) -> bool {
        has_self_intersection(linestring).is_some()
    }

    /// Finds duplicate geometries.
    fn find_duplicates(&self, features: &FeatureCollection) -> QcResult<Vec<DuplicateGroup>> {
        let mut geometry_map: HashMap<String, Vec<String>> = HashMap::new();

        for feature in &features.features {
            if let Some(ref geometry) = feature.geometry {
                let geom_hash = self.hash_geometry(geometry)?;
                let feature_id = feature
                    .id
                    .as_ref()
                    .map_or_else(|| "unknown".to_string(), feature_id_to_string);

                geometry_map.entry(geom_hash).or_default().push(feature_id);
            }
        }

        let duplicates: Vec<DuplicateGroup> = geometry_map
            .into_iter()
            .filter(|(_, ids)| ids.len() > 1)
            .map(|(_, ids)| DuplicateGroup {
                count: ids.len(),
                feature_ids: ids,
                geometry_type: "Unknown".to_string(),
            })
            .collect();

        Ok(duplicates)
    }

    /// Creates a hash representation of a geometry (simplified).
    fn hash_geometry(&self, geometry: &Geometry) -> QcResult<String> {
        // Simplified implementation - real version would use proper geometry hashing
        Ok(format!("{:?}", geometry))
    }

    /// Finds sliver polygons.
    fn find_slivers(&self, features: &FeatureCollection) -> QcResult<Vec<SliverPolygon>> {
        let mut slivers = Vec::new();

        for feature in &features.features {
            let feature_id_str = feature.id.as_ref().map(feature_id_to_string);
            if let Some(ref geometry) = feature.geometry {
                match geometry {
                    Geometry::Polygon(polygon) => {
                        if let Some(sliver) = self.check_sliver(polygon, &feature_id_str)? {
                            slivers.push(sliver);
                        }
                    }
                    Geometry::MultiPolygon(multipolygon) => {
                        for polygon in &multipolygon.polygons {
                            if let Some(sliver) = self.check_sliver(polygon, &feature_id_str)? {
                                slivers.push(sliver);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        Ok(slivers)
    }

    /// Checks if a polygon is a sliver.
    fn check_sliver(
        &self,
        polygon: &Polygon,
        feature_id: &Option<String>,
    ) -> QcResult<Option<SliverPolygon>> {
        let area = self.calculate_area(polygon);
        let perimeter = self.calculate_perimeter(polygon);

        if area < self.config.sliver_area_threshold {
            let compactness = if perimeter > 0.0 {
                area / (perimeter * perimeter)
            } else {
                0.0
            };

            if compactness < self.config.sliver_compactness_threshold {
                let width = if perimeter > 0.0 {
                    area / perimeter
                } else {
                    0.0
                };

                let severity = if area < 0.1 {
                    Severity::Major
                } else if area < 0.5 {
                    Severity::Minor
                } else {
                    Severity::Warning
                };

                return Ok(Some(SliverPolygon {
                    feature_id: feature_id.clone(),
                    area,
                    perimeter,
                    compactness,
                    width,
                    severity,
                }));
            }
        }

        Ok(None)
    }

    /// Calculates polygon area (simplified shoelace formula).
    fn calculate_area(&self, polygon: &Polygon) -> f64 {
        let coords = &polygon.exterior.coords;
        if coords.len() < 3 {
            return 0.0;
        }

        let mut area = 0.0;
        for i in 0..coords.len() - 1 {
            area += coords[i].x * coords[i + 1].y;
            area -= coords[i + 1].x * coords[i].y;
        }

        (area / 2.0).abs()
    }

    /// Calculates polygon perimeter.
    fn calculate_perimeter(&self, polygon: &Polygon) -> f64 {
        let coords = &polygon.exterior.coords;
        if coords.len() < 2 {
            return 0.0;
        }

        let mut perimeter = 0.0;
        for i in 0..coords.len() - 1 {
            let dx = coords[i + 1].x - coords[i].x;
            let dy = coords[i + 1].y - coords[i].y;
            perimeter += (dx * dx + dy * dy).sqrt();
        }

        perimeter
    }

    /// Checks topology rules.
    fn check_topology_rules(&self, features: &FeatureCollection) -> QcResult<Vec<RuleViolation>> {
        let options = TopologyOptions {
            tolerance: self.config.coordinate_tolerance,
            detect_gaps: self
                .config
                .topology_rules
                .contains(&TopologyRule::MustNotHaveGaps),
            detect_dangles: false,
        };

        let topology_violations = check_topology_rules(features, &options);
        let mut rule_violations = Vec::new();

        for viol in topology_violations {
            let rv = topology_violation_to_rule_violation(viol);
            rule_violations.push(rv);
        }

        Ok(rule_violations)
    }
}

impl Default for TopologyChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Free-standing topology engine functions (public API)
// ─────────────────────────────────────────────────────────────────────────────

/// Computes the signed area of a closed ring using the shoelace formula.
///
/// Positive result = CCW winding (standard exterior ring).
/// Negative result = CW winding (standard hole ring).
fn signed_area(coords: &[Coordinate]) -> f64 {
    let n = coords.len();
    if n < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in 0..n {
        let j = (i + 1) % n;
        sum += coords[i].x * coords[j].y - coords[j].x * coords[i].y;
    }
    sum / 2.0
}

/// Returns the bounding box of a polygon exterior as `Option<Bbox2D>`.
fn polygon_bbox(polygon: &Polygon) -> Option<Bbox2D> {
    let coords = &polygon.exterior.coords;
    if coords.is_empty() {
        return None;
    }
    let mut min_x = coords[0].x;
    let mut min_y = coords[0].y;
    let mut max_x = coords[0].x;
    let mut max_y = coords[0].y;
    for c in coords.iter().skip(1) {
        if c.x < min_x {
            min_x = c.x;
        }
        if c.y < min_y {
            min_y = c.y;
        }
        if c.x > max_x {
            max_x = c.x;
        }
        if c.y > max_y {
            max_y = c.y;
        }
    }
    Bbox2D::new(min_x, min_y, max_x, max_y)
}

/// Computes the intersection area of two bounding boxes, returning 0.0 if they
/// do not overlap.
fn bbox_intersection_area(a: &Bbox2D, b: &Bbox2D) -> f64 {
    let ix_min = a.min_x.max(b.min_x);
    let iy_min = a.min_y.max(b.min_y);
    let ix_max = a.max_x.min(b.max_x);
    let iy_max = a.max_y.min(b.max_y);
    if ix_max <= ix_min || iy_max <= iy_min {
        0.0
    } else {
        (ix_max - ix_min) * (iy_max - iy_min)
    }
}

/// Checks whether two non-adjacent line segments (i, j) of a `LineString` cross.
///
/// Adjacent segments share an endpoint; those are **not** reported.
///
/// For closed rings (first coord == last coord within f64 epsilon), the pair
/// `(0, seg_count-1)` is also skipped because the last segment connects back to
/// the first coordinate — they share one endpoint.  For open linestrings that guard
/// does NOT apply.
///
/// Returns `None` if no self-intersection is found, or `Some(pairs)` where each
/// tuple `(i, j)` identifies the crossing segment indices.
pub fn has_self_intersection(linestring: &LineString) -> Option<Vec<(usize, usize)>> {
    let coords = &linestring.coords;
    let n = coords.len();
    if n < 4 {
        // Need at least 4 points to have two non-adjacent segments
        return None;
    }

    // A linestring is a ring when its first and last coordinates coincide.
    let is_ring = {
        let first = &coords[0];
        let last = &coords[n - 1];
        (first.x - last.x).abs() < f64::EPSILON && (first.y - last.y).abs() < f64::EPSILON
    };

    let seg_count = n - 1;
    let mut crossings: Vec<(usize, usize)> = Vec::new();

    for i in 0..seg_count {
        let p1 = &coords[i];
        let p2 = &coords[i + 1];

        // j must be at least i+2 to be non-adjacent
        for j in (i + 2)..seg_count {
            // For a closed ring the last segment shares the start point of segment 0
            // (j = seg_count-1, i = 0).  Skip that pair for rings only.
            if is_ring && i == 0 && j == seg_count - 1 {
                continue;
            }

            let p3 = &coords[j];
            let p4 = &coords[j + 1];

            match intersect_segment_segment(p1, p2, p3, p4) {
                SegmentIntersection::None => {}
                _ => {
                    crossings.push((i, j));
                }
            }
        }
    }

    if crossings.is_empty() {
        None
    } else {
        Some(crossings)
    }
}

/// Runs the full suite of topology rules against a `FeatureCollection`.
///
/// Rules implemented:
/// - **R1** `LineString` self-intersection
/// - **R2** Polygon ring orientation (shoelace signed-area)
/// - **R3** Ring closure (first == last within tolerance)
/// - **R4** Polygon ring self-intersection
/// - **R6** Overlap between polygons (STRtree pre-filter + boundary crossing check)
/// - **R5** Gap detection (opt-in via `options.detect_gaps`)
pub fn check_topology_rules(
    features: &FeatureCollection,
    options: &TopologyOptions,
) -> Vec<TopologyViolation> {
    let mut violations: Vec<TopologyViolation> = Vec::new();

    // Collect all polygons with their feature indices for spatial operations.
    // We use the feature's position in the collection as its numeric id when the
    // feature has no explicit integer id.
    let mut polygons_with_ids: Vec<(u64, &Polygon)> = Vec::new();

    for (idx, feature) in features.features.iter().enumerate() {
        let feature_id = feature_id_as_u64(&feature.id, idx);

        let Some(ref geometry) = feature.geometry else {
            continue;
        };

        match geometry {
            Geometry::LineString(ls) => {
                // R1 — LineString self-intersection
                if let Some(segs) = has_self_intersection(ls) {
                    violations.push(TopologyViolation::SelfIntersection {
                        feature_id,
                        segments: segs,
                    });
                }
            }
            Geometry::Polygon(polygon) => {
                // R3 — exterior ring closure
                check_ring_closure(
                    &polygon.exterior.coords,
                    feature_id,
                    0,
                    options.tolerance,
                    &mut violations,
                );
                // R2 — exterior ring must be CCW (positive signed area)
                let area = signed_area(&polygon.exterior.coords);
                if area < 0.0 {
                    violations.push(TopologyViolation::RingOrientation {
                        feature_id,
                        ring_index: 0,
                        expected_ccw: true,
                    });
                }
                // R4 — exterior ring self-intersection
                if let Some(segs) = has_self_intersection(&polygon.exterior) {
                    violations.push(TopologyViolation::SelfIntersection {
                        feature_id,
                        segments: segs,
                    });
                }
                // Interior rings (holes)
                for (hole_idx, hole) in polygon.interiors.iter().enumerate() {
                    let ring_index = hole_idx + 1;
                    // R3 — hole closure
                    check_ring_closure(
                        &hole.coords,
                        feature_id,
                        ring_index,
                        options.tolerance,
                        &mut violations,
                    );
                    // R2 — hole must be CW (negative signed area)
                    let hole_area = signed_area(&hole.coords);
                    if hole_area > 0.0 {
                        violations.push(TopologyViolation::RingOrientation {
                            feature_id,
                            ring_index,
                            expected_ccw: false,
                        });
                    }
                    // R4 — hole self-intersection
                    if let Some(segs) = has_self_intersection(hole) {
                        violations.push(TopologyViolation::SelfIntersection {
                            feature_id,
                            segments: segs,
                        });
                    }
                }
                polygons_with_ids.push((feature_id, polygon));
            }
            Geometry::MultiPolygon(mp) => {
                for polygon in &mp.polygons {
                    // Same per-polygon checks
                    check_ring_closure(
                        &polygon.exterior.coords,
                        feature_id,
                        0,
                        options.tolerance,
                        &mut violations,
                    );
                    let area = signed_area(&polygon.exterior.coords);
                    if area < 0.0 {
                        violations.push(TopologyViolation::RingOrientation {
                            feature_id,
                            ring_index: 0,
                            expected_ccw: true,
                        });
                    }
                    if let Some(segs) = has_self_intersection(&polygon.exterior) {
                        violations.push(TopologyViolation::SelfIntersection {
                            feature_id,
                            segments: segs,
                        });
                    }
                    for (hole_idx, hole) in polygon.interiors.iter().enumerate() {
                        let ring_index = hole_idx + 1;
                        check_ring_closure(
                            &hole.coords,
                            feature_id,
                            ring_index,
                            options.tolerance,
                            &mut violations,
                        );
                        let hole_area = signed_area(&hole.coords);
                        if hole_area > 0.0 {
                            violations.push(TopologyViolation::RingOrientation {
                                feature_id,
                                ring_index,
                                expected_ccw: false,
                            });
                        }
                        if let Some(segs) = has_self_intersection(hole) {
                            violations.push(TopologyViolation::SelfIntersection {
                                feature_id,
                                segments: segs,
                            });
                        }
                    }
                    polygons_with_ids.push((feature_id, polygon));
                }
            }
            _ => {}
        }
    }

    // R6 — Overlap detection using STRtree spatial pre-filter
    violations.extend(detect_overlaps(&polygons_with_ids));

    // R5 — Gap detection (opt-in, expensive)
    if options.detect_gaps {
        violations.extend(detect_gaps(&polygons_with_ids, options.tolerance));
    }

    violations
}

/// Helper: check ring closure and push an `UnclosedRing` violation if needed.
fn check_ring_closure(
    coords: &[Coordinate],
    feature_id: u64,
    ring_index: usize,
    tolerance: f64,
    violations: &mut Vec<TopologyViolation>,
) {
    if coords.len() < 2 {
        violations.push(TopologyViolation::UnclosedRing {
            feature_id,
            ring_index,
        });
        return;
    }
    let first = &coords[0];
    let last = &coords[coords.len() - 1];
    if (first.x - last.x).abs() >= tolerance || (first.y - last.y).abs() >= tolerance {
        violations.push(TopologyViolation::UnclosedRing {
            feature_id,
            ring_index,
        });
    }
}

/// Converts a `FeatureId` to a stable `u64` using the feature's collection index
/// as a fallback.
fn feature_id_as_u64(id: &Option<FeatureId>, fallback_idx: usize) -> u64 {
    match id {
        Some(FeatureId::Integer(i)) => *i as u64,
        _ => fallback_idx as u64,
    }
}

/// Detect overlapping polygon pairs using an R-tree spatial pre-filter.
///
/// For each pair whose bboxes intersect we perform a more precise test:
/// 1. Check if any boundary segments cross.
/// 2. If boundaries don't cross, check if a vertex of A is strictly inside B (one
///    polygon contained in the other).
fn detect_overlaps(polygons: &[(u64, &Polygon)]) -> Vec<TopologyViolation> {
    let mut violations = Vec::new();
    if polygons.len() < 2 {
        return violations;
    }

    // Build an R-tree indexed by (bbox → index into `polygons`).
    let mut tree: RTree<usize> = RTree::new();
    for (i, (_, poly)) in polygons.iter().enumerate() {
        if let Some(bbox) = polygon_bbox(poly) {
            tree.insert(bbox, i);
        }
    }

    // For each polygon, query the tree for candidates and check each pair once
    // (use i < j to avoid duplicates).
    for (i, (id_a, poly_a)) in polygons.iter().enumerate() {
        let bbox_a = match polygon_bbox(poly_a) {
            Some(b) => b,
            None => continue,
        };

        let candidates = tree.search(&bbox_a);
        for j_ref in candidates {
            let j = *j_ref;
            if j <= i {
                continue; // already processed or same polygon
            }
            let (id_b, poly_b) = &polygons[j];

            // Precise overlap test: try polygon intersection
            let overlap_area = compute_overlap_area(poly_a, poly_b, &bbox_a);
            if overlap_area > 0.0 {
                violations.push(TopologyViolation::Overlap {
                    feature_a: *id_a,
                    feature_b: *id_b,
                    area: overlap_area,
                });
            }
        }
    }

    violations
}

/// Computes the overlap area between two polygons.
///
/// Uses `intersect_polygons` if both polygons have valid rings (≥4 coords), then
/// accumulates the absolute area of the result polygons.  Falls back to the
/// bbox-intersection area as a lower bound when `intersect_polygons` cannot be
/// called or returns an error.
fn compute_overlap_area(poly_a: &Polygon, poly_b: &Polygon, bbox_a: &Bbox2D) -> f64 {
    // Only attempt full intersection when both exteriors have the minimum 4 coords
    // required by `intersect_polygons`.
    if poly_a.exterior.coords.len() >= 4 && poly_b.exterior.coords.len() >= 4 {
        if let Ok(intersection_polys) = intersect_polygons(poly_a, poly_b) {
            let total: f64 = intersection_polys
                .iter()
                .map(|p| signed_area(&p.exterior.coords).abs())
                .sum();
            return total;
        }
    }

    // Fallback: use bbox-intersection area as an estimate.
    let bbox_b = match polygon_bbox(poly_b) {
        Some(b) => b,
        None => return 0.0,
    };
    bbox_intersection_area(bbox_a, &bbox_b)
}

/// Approximate gap detection.
///
/// For each pair of polygons whose bboxes are nearly-touching (within `tolerance`),
/// we check whether any boundary segment of one polygon's exterior is very close to
/// the other.  If their bboxes touch but no boundary points overlap, we assume a gap.
///
/// This is intentionally approximate — it emits `Gap { area: 0.0 }` for detected
/// candidates.  Full gap geometry is too expensive without a polygon clipping engine.
fn detect_gaps(polygons: &[(u64, &Polygon)], tolerance: f64) -> Vec<TopologyViolation> {
    let mut violations = Vec::new();
    if polygons.len() < 2 {
        return violations;
    }

    // Build R-tree with bboxes expanded by tolerance so near-touching bboxes
    // register as intersecting.
    let mut tree: RTree<usize> = RTree::new();
    for (i, (_, poly)) in polygons.iter().enumerate() {
        if let Some(bbox) = polygon_bbox(poly) {
            if let Some(expanded) = Bbox2D::new(
                bbox.min_x - tolerance,
                bbox.min_y - tolerance,
                bbox.max_x + tolerance,
                bbox.max_y + tolerance,
            ) {
                tree.insert(expanded, i);
            }
        }
    }

    for (i, (id_a, poly_a)) in polygons.iter().enumerate() {
        let bbox_a = match polygon_bbox(poly_a) {
            Some(b) => b,
            None => continue,
        };
        let query = match Bbox2D::new(
            bbox_a.min_x - tolerance,
            bbox_a.min_y - tolerance,
            bbox_a.max_x + tolerance,
            bbox_a.max_y + tolerance,
        ) {
            Some(b) => b,
            None => continue,
        };

        let candidates = tree.search(&query);
        for j_ref in candidates {
            let j = *j_ref;
            if j <= i {
                continue;
            }
            let (id_b, poly_b) = &polygons[j];
            let bbox_b = match polygon_bbox(poly_b) {
                Some(b) => b,
                None => continue,
            };

            // Compute actual overlap (not expanded).
            let actual_overlap = bbox_intersection_area(&bbox_a, &bbox_b);

            // Polygons are "near" but don't overlap — potential gap.
            if actual_overlap <= 0.0 {
                // Check if they are genuinely adjacent (bboxes close within tolerance).
                let gap_x = (bbox_a.min_x - bbox_b.max_x)
                    .abs()
                    .min((bbox_b.min_x - bbox_a.max_x).abs());
                let gap_y = (bbox_a.min_y - bbox_b.max_y)
                    .abs()
                    .min((bbox_b.min_y - bbox_a.max_y).abs());

                if gap_x <= tolerance * 100.0 || gap_y <= tolerance * 100.0 {
                    violations.push(TopologyViolation::Gap {
                        feature_a: *id_a,
                        feature_b: *id_b,
                        area: 0.0,
                    });
                }
            }
        }
    }

    violations
}

/// Converts a [`TopologyViolation`] into a [`RuleViolation`] for use in
/// `TopologyResult`.
fn topology_violation_to_rule_violation(viol: TopologyViolation) -> RuleViolation {
    match viol {
        TopologyViolation::SelfIntersection {
            feature_id,
            segments,
        } => RuleViolation {
            rule: TopologyRule::MustNotSelfOverlap,
            feature_ids: vec![feature_id.to_string()],
            location: None,
            severity: Severity::Major,
            description: format!(
                "Feature {} has {} self-intersecting segment pair(s)",
                feature_id,
                segments.len()
            ),
        },
        TopologyViolation::RingOrientation {
            feature_id,
            ring_index,
            expected_ccw,
        } => RuleViolation {
            rule: TopologyRule::MustNotSelfOverlap,
            feature_ids: vec![feature_id.to_string()],
            location: None,
            severity: Severity::Major,
            description: format!(
                "Feature {} ring {} has wrong orientation (expected {})",
                feature_id,
                ring_index,
                if expected_ccw { "CCW" } else { "CW" }
            ),
        },
        TopologyViolation::UnclosedRing {
            feature_id,
            ring_index,
        } => RuleViolation {
            rule: TopologyRule::MustNotSelfOverlap,
            feature_ids: vec![feature_id.to_string()],
            location: None,
            severity: Severity::Major,
            description: format!("Feature {} ring {} is not closed", feature_id, ring_index),
        },
        TopologyViolation::Overlap {
            feature_a,
            feature_b,
            area,
        } => RuleViolation {
            rule: TopologyRule::MustNotOverlap,
            feature_ids: vec![feature_a.to_string(), feature_b.to_string()],
            location: None,
            severity: Severity::Major,
            description: format!(
                "Features {} and {} overlap by approximately {:.6} area units",
                feature_a, feature_b, area
            ),
        },
        TopologyViolation::Gap {
            feature_a,
            feature_b,
            area,
        } => RuleViolation {
            rule: TopologyRule::MustNotHaveGaps,
            feature_ids: vec![feature_a.to_string(), feature_b.to_string()],
            location: None,
            severity: Severity::Minor,
            description: format!(
                "Possible gap between features {} and {} (area ≈ {:.6})",
                feature_a, feature_b, area
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigdal_core::vector::{Feature, FeatureCollection, FeatureId};

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
}
