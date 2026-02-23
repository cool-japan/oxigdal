//! Vector topology validation.
//!
//! This module provides quality control checks for vector topology,
//! including error detection, invalid geometry identification, and repair suggestions.

use crate::error::{QcIssue, QcResult, Severity};
use oxigdal_core::vector::{
    Coordinate, FeatureCollection, FeatureId, Geometry, LineString, Polygon,
};
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

    /// Checks if a linestring has self-intersections (simplified check).
    fn has_self_intersection(&self, _linestring: &LineString) -> bool {
        // Simplified implementation - full version would use sweep line algorithm
        false
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
    fn check_topology_rules(&self, _features: &FeatureCollection) -> QcResult<Vec<RuleViolation>> {
        let violations = Vec::new();

        // Simplified implementation - full version would implement each rule
        for rule in &self.config.topology_rules {
            match rule {
                TopologyRule::MustNotOverlap => {
                    // Check for overlaps
                }
                TopologyRule::MustNotHaveGaps => {
                    // Check for gaps
                }
                _ => {
                    // Other rules
                }
            }
        }

        Ok(violations)
    }
}

impl Default for TopologyChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
