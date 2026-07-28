//! Vector topology validation.
//!
//! This module provides quality control checks for vector topology,
//! including error detection, invalid geometry identification, and repair suggestions.

use crate::error::{QcError, QcIssue, QcResult, Severity};
use crate::vector::violations::{TopologyOptions, TopologyViolation};
use oxigeo_algorithms::vector::{
    SegmentIntersection, intersect_polygons, intersect_segment_segment,
};
use oxigeo_core::vector::{
    Coordinate, FeatureCollection, FeatureId, Geometry, LineString, Polygon,
};
use oxigeo_index::{Bbox2D, RTree};
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

        if let (Some(f), Some(l)) = (first, last)
            && !self.coords_equal(f, l)
        {
            errors.push(TopologyError {
                feature_id: feature_id.clone(),
                error_type: TopologyErrorType::InvalidRing,
                location: *f,
                severity: Severity::Critical,
                description: "Polygon ring is not closed".to_string(),
                fix_suggestion: Some("Close the ring by adding first point at end".to_string()),
            });
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

    /// Creates a canonical structural-equality key for a geometry, used to
    /// detect duplicate features regardless of vertex start point, ring
    /// winding direction, or line digitization direction.
    ///
    /// Previously this hashed `format!("{:?}", geometry)` (the `Debug`
    /// output), which is sensitive to exact vertex order: two features
    /// representing the identical polygon but whose rings started at a
    /// different vertex, or that differed only in winding direction (both
    /// common after round-tripping through different tools/CRS transforms),
    /// produced different strings and were never reported as duplicates.
    /// [`canonical_geometry_key`] normalizes all of that before hashing.
    fn hash_geometry(&self, geometry: &Geometry) -> QcResult<String> {
        Ok(canonical_geometry_key(
            geometry,
            self.config.coordinate_tolerance,
        ))
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

    /// Calculates polygon net area (shoelace formula on the exterior ring,
    /// minus the shoelace area of every interior ring/hole).
    ///
    /// Interior rings (holes) reduce the net area of the polygon; a thin
    /// annulus (large exterior, near-equal-size hole) must therefore score a
    /// small net area rather than the gross exterior-only area, so sliver
    /// detection (`check_sliver`) isn't fooled by donut-shaped features.
    /// The result is clamped at 0.0 to guard against malformed input where
    /// the holes' combined area exceeds the exterior's (e.g. self-overlapping
    /// or mis-wound rings).
    fn calculate_area(&self, polygon: &Polygon) -> f64 {
        let exterior_area = signed_area(&polygon.exterior.coords).abs();
        let holes_area: f64 = polygon
            .interiors
            .iter()
            .map(|ring| signed_area(&ring.coords).abs())
            .sum();

        (exterior_area - holes_area).max(0.0)
    }

    /// Calculates polygon perimeter.
    ///
    /// Intentionally exterior-ring-only (unlike `calculate_area`, which nets
    /// out interior rings): for the sliver-compactness ratio
    /// `area / perimeter^2` used by `check_sliver`, the boundary length that
    /// matters for "is this shape thin and elongated" is the outer boundary.
    /// Including hole perimeters would inflate the denominator for polygons
    /// that have holes for unrelated reasons (e.g. a donut with a large,
    /// well-formed hole) and bias them toward looking artificially more
    /// compact, masking genuine slivers elsewhere in the same feature.
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
    ///
    /// # Errors
    ///
    /// Returns [`QcError::InvalidConfiguration`] if `self.config.topology_rules`
    /// contains a [`TopologyRule`] variant this engine does not (yet) enforce
    /// ([`TopologyRule::MustBeCoveredBy`], [`TopologyRule::BoundaryMustBeCoveredBy`],
    /// [`TopologyRule::MustBeInside`], [`TopologyRule::PointsMustBeCoveredByLine`]).
    /// These four rules are inherently cross-feature-class checks (e.g. "this
    /// polygon layer must be covered by that boundary layer"), which the
    /// current single-`FeatureCollection` API has no way to express; rather
    /// than silently no-op validating a rule the caller explicitly asked
    /// for, this rejects the configuration up front so the gap is visible
    /// immediately instead of at audit time.
    fn check_topology_rules(&self, features: &FeatureCollection) -> QcResult<Vec<RuleViolation>> {
        if let Some(unsupported) = self
            .config
            .topology_rules
            .iter()
            .find(|rule| !is_topology_rule_enforced(rule))
        {
            return Err(QcError::InvalidConfiguration(format!(
                "TopologyRule::{unsupported:?} is not enforced by this engine (it requires \
                 cross-feature-class coverage/containment checks the current single-collection \
                 API cannot express); remove it from TopologyConfig::topology_rules or restrict \
                 to the supported rules (MustNotOverlap, MustNotHaveGaps, MustNotCross, \
                 MustNotSelfOverlap)"
            )));
        }

        let options = TopologyOptions {
            tolerance: self.config.coordinate_tolerance,
            detect_overlaps: self
                .config
                .topology_rules
                .contains(&TopologyRule::MustNotOverlap),
            detect_crossings: self
                .config
                .topology_rules
                .contains(&TopologyRule::MustNotCross),
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

/// Whether `rule` is actually enforced by [`check_topology_rules`] (the
/// free function) / [`TopologyChecker::check_topology_rules`] (the method).
///
/// See the method's doc comment for why the four coverage/containment rules
/// are not (yet) enforced.
const fn is_topology_rule_enforced(rule: &TopologyRule) -> bool {
    matches!(
        rule,
        TopologyRule::MustNotOverlap
            | TopologyRule::MustNotHaveGaps
            | TopologyRule::MustNotCross
            | TopologyRule::MustNotSelfOverlap
    )
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

/// Rounds a coordinate's X/Y components to a grid of size `tolerance` (or a
/// small fixed epsilon if `tolerance` is not a positive finite number), then
/// formats each component with enough decimal digits to be exact for that
/// grid. This absorbs floating-point noise well below `tolerance` so nearly
/// (but not bit-for-bit) identical coordinates hash identically.
fn canonical_coord(coord: &Coordinate, tolerance: f64) -> (String, String) {
    let grid = if tolerance.is_finite() && tolerance > 0.0 {
        tolerance
    } else {
        1e-9
    };
    // Round -0.0 to 0.0 so a coordinate that lands exactly on the snapping
    // grid at zero doesn't produce a different string than its positive
    // counterpart.
    let snap = |v: f64| -> f64 {
        let snapped = (v / grid).round() * grid;
        // Normalize -0.0 to 0.0 (bitwise, to avoid a float equality
        // comparison) so a coordinate landing exactly on the snapping grid
        // at zero doesn't hash differently from its positive counterpart.
        if snapped.to_bits() == (-0.0f64).to_bits() {
            0.0
        } else {
            snapped
        }
    };
    (
        format!("{:.12}", snap(coord.x)),
        format!("{:.12}", snap(coord.y)),
    )
}

/// Canonicalizes a (possibly closed) ring for duplicate-detection hashing:
/// drops the redundant closing point, forces a consistent (CCW) winding
/// direction, and rotates the vertex list to start at the
/// lexicographically-smallest rounded coordinate. Two rings that describe
/// the identical polygon but started at a different vertex, or differ only
/// in winding direction, canonicalize identically.
fn canonical_ring(coords: &[Coordinate], tolerance: f64) -> Vec<(String, String)> {
    if coords.is_empty() {
        return Vec::new();
    }

    let mut open: Vec<Coordinate> = coords.to_vec();
    if open.len() > 1 {
        let first = open[0];
        let last = open[open.len() - 1];
        let close_eps = if tolerance.is_finite() && tolerance > 0.0 {
            tolerance
        } else {
            1e-9
        };
        if (first.x - last.x).abs() < close_eps && (first.y - last.y).abs() < close_eps {
            open.pop();
        }
    }
    if open.is_empty() {
        return Vec::new();
    }

    // Force a consistent winding direction (CCW / positive signed area) so
    // exterior-vs-hole conventions and accidental reversals don't produce
    // different keys for the same shape.
    if signed_area(&open) < 0.0 {
        open.reverse();
    }

    let rounded: Vec<(String, String)> =
        open.iter().map(|c| canonical_coord(c, tolerance)).collect();
    let start = rounded
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| a.cmp(b))
        .map_or(0, |(i, _)| i);

    let mut canonical = Vec::with_capacity(rounded.len());
    canonical.extend_from_slice(&rounded[start..]);
    canonical.extend_from_slice(&rounded[..start]);
    canonical
}

/// Renders a canonicalized ring (see [`canonical_ring`]) as a single string.
fn ring_key(ring: &[(String, String)]) -> String {
    ring.iter()
        .map(|(x, y)| format!("{x} {y}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// Builds a canonical structural-equality key for any [`Geometry`], suitable
/// for duplicate-feature detection. See [`canonical_ring`] for the
/// polygon-ring normalization rules; `LineString`s are normalized so that
/// digitizing the same line in either direction produces the same key;
/// multi-geometries and geometry collections sort their normalized parts so
/// part ordering doesn't matter either.
fn canonical_geometry_key(geometry: &Geometry, tolerance: f64) -> String {
    match geometry {
        Geometry::Point(point) => {
            let (x, y) = canonical_coord(&point.coord, tolerance);
            format!("POINT({x} {y})")
        }
        Geometry::LineString(linestring) => canonical_linestring_key(linestring, tolerance),
        Geometry::Polygon(polygon) => canonical_polygon_key(polygon, tolerance),
        Geometry::MultiPoint(multipoint) => {
            let mut points: Vec<String> = multipoint
                .points
                .iter()
                .map(|p| {
                    let (x, y) = canonical_coord(&p.coord, tolerance);
                    format!("{x} {y}")
                })
                .collect();
            points.sort();
            format!("MULTIPOINT({})", points.join(","))
        }
        Geometry::MultiLineString(multilinestring) => {
            let mut lines: Vec<String> = multilinestring
                .line_strings
                .iter()
                .map(|ls| canonical_linestring_key(ls, tolerance))
                .collect();
            lines.sort();
            format!("MULTILINESTRING({})", lines.join(";"))
        }
        Geometry::MultiPolygon(multipolygon) => {
            let mut polys: Vec<String> = multipolygon
                .polygons
                .iter()
                .map(|p| canonical_polygon_key(p, tolerance))
                .collect();
            polys.sort();
            format!("MULTIPOLYGON({})", polys.join(";"))
        }
        Geometry::GeometryCollection(collection) => {
            let mut geoms: Vec<String> = collection
                .geometries
                .iter()
                .map(|g| canonical_geometry_key(g, tolerance))
                .collect();
            geoms.sort();
            format!("GEOMETRYCOLLECTION({})", geoms.join(";"))
        }
    }
}

/// Canonical key for a `LineString`: rounds coordinates, then picks
/// whichever of the forward/reversed vertex sequences sorts first, so the
/// same line digitized in either direction hashes identically.
fn canonical_linestring_key(linestring: &LineString, tolerance: f64) -> String {
    let forward: Vec<(String, String)> = linestring
        .coords
        .iter()
        .map(|c| canonical_coord(c, tolerance))
        .collect();
    let mut backward = forward.clone();
    backward.reverse();
    let chosen = if backward < forward {
        backward
    } else {
        forward
    };
    format!("LINESTRING({})", ring_key(&chosen))
}

/// Canonical key for a `Polygon`: canonicalizes the exterior ring and every
/// interior ring (see [`canonical_ring`]), then sorts the interior rings so
/// hole ordering doesn't affect the key.
fn canonical_polygon_key(polygon: &Polygon, tolerance: f64) -> String {
    let exterior = canonical_ring(&polygon.exterior.coords, tolerance);
    let mut interiors: Vec<String> = polygon
        .interiors
        .iter()
        .map(|ring| ring_key(&canonical_ring(&ring.coords, tolerance)))
        .collect();
    interiors.sort();
    format!("POLYGON({};[{}])", ring_key(&exterior), interiors.join(";"))
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

/// Returns the bounding box of a `LineString` as `Option<Bbox2D>`.
fn linestring_bbox(linestring: &LineString) -> Option<Bbox2D> {
    let coords = &linestring.coords;
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
/// - **R6** Overlap between polygons (STRtree pre-filter + boundary crossing check;
///   opt-out via `options.detect_overlaps`, default `true`)
/// - **R5** Gap detection (opt-in via `options.detect_gaps`)
/// - **R8** Crossing between distinct `LineString` features (STRtree
///   pre-filter + segment intersection check; opt-out via
///   `options.detect_crossings`, default `true`)
pub fn check_topology_rules(
    features: &FeatureCollection,
    options: &TopologyOptions,
) -> Vec<TopologyViolation> {
    let mut violations: Vec<TopologyViolation> = Vec::new();

    // Collect all polygons with their feature indices for spatial operations.
    // We use the feature's position in the collection as its numeric id when the
    // feature has no explicit integer id.
    let mut polygons_with_ids: Vec<(u64, &Polygon)> = Vec::new();
    // Collect all standalone LineString features for cross-feature crossing
    // detection (R8) -- distinct from the per-geometry self-intersection
    // check (R1) below, which only looks within a single LineString.
    let mut linestrings_with_ids: Vec<(u64, &LineString)> = Vec::new();

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
                linestrings_with_ids.push((feature_id, ls));
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

    // R6 — Overlap detection using STRtree spatial pre-filter (opt-out via
    // options.detect_overlaps, default true)
    if options.detect_overlaps {
        violations.extend(detect_overlaps(&polygons_with_ids));
    }

    // R8 — Crossing detection between distinct LineString features
    // (opt-out via options.detect_crossings, default true)
    if options.detect_crossings {
        violations.extend(detect_line_crossings(&linestrings_with_ids));
    }

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
    if poly_a.exterior.coords.len() >= 4
        && poly_b.exterior.coords.len() >= 4
        && let Ok(intersection_polys) = intersect_polygons(poly_a, poly_b)
    {
        let total: f64 = intersection_polys
            .iter()
            .map(|p| signed_area(&p.exterior.coords).abs())
            .sum();
        return total;
    }

    // Fallback: use bbox-intersection area as an estimate.
    let bbox_b = match polygon_bbox(poly_b) {
        Some(b) => b,
        None => return 0.0,
    };
    bbox_intersection_area(bbox_a, &bbox_b)
}

/// Detect crossing pairs of distinct `LineString` features (R8, the
/// [`TopologyRule::MustNotCross`] rule) using an R-tree spatial pre-filter,
/// mirroring [`detect_overlaps`]'s approach for polygons.
///
/// This is deliberately a separate check from R1 (`has_self_intersection`,
/// [`TopologyRule::MustNotSelfOverlap`]): R1 looks for a single geometry
/// crossing itself, while this looks for two *different* features' segments
/// crossing each other.
fn detect_line_crossings(linestrings: &[(u64, &LineString)]) -> Vec<TopologyViolation> {
    let mut violations = Vec::new();
    if linestrings.len() < 2 {
        return violations;
    }

    let mut tree: RTree<usize> = RTree::new();
    for (i, (_, ls)) in linestrings.iter().enumerate() {
        if let Some(bbox) = linestring_bbox(ls) {
            tree.insert(bbox, i);
        }
    }

    for (i, (id_a, ls_a)) in linestrings.iter().enumerate() {
        let bbox_a = match linestring_bbox(ls_a) {
            Some(b) => b,
            None => continue,
        };

        let candidates = tree.search(&bbox_a);
        for j_ref in candidates {
            let j = *j_ref;
            if j <= i {
                continue; // already processed or same linestring
            }
            let (id_b, ls_b) = &linestrings[j];

            let mut crosses = false;
            'segment_search: for w_a in ls_a.coords.windows(2) {
                for w_b in ls_b.coords.windows(2) {
                    if !matches!(
                        intersect_segment_segment(&w_a[0], &w_a[1], &w_b[0], &w_b[1]),
                        SegmentIntersection::None
                    ) {
                        crosses = true;
                        break 'segment_search;
                    }
                }
            }

            if crosses {
                violations.push(TopologyViolation::Crossing {
                    feature_a: *id_a,
                    feature_b: *id_b,
                });
            }
        }
    }

    violations
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
        if let Some(bbox) = polygon_bbox(poly)
            && let Some(expanded) = Bbox2D::new(
                bbox.min_x - tolerance,
                bbox.min_y - tolerance,
                bbox.max_x + tolerance,
                bbox.max_y + tolerance,
            )
        {
            tree.insert(expanded, i);
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
        TopologyViolation::Crossing {
            feature_a,
            feature_b,
        } => RuleViolation {
            rule: TopologyRule::MustNotCross,
            feature_ids: vec![feature_a.to_string(), feature_b.to_string()],
            location: None,
            severity: Severity::Major,
            description: format!("Features {} and {} cross each other", feature_a, feature_b),
        },
    }
}

#[cfg(test)]
#[path = "topology_tests.rs"]
mod tests;
