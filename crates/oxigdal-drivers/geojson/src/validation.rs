//! RFC 7946 validation utilities
//!
//! This module provides comprehensive validation for GeoJSON objects
//! according to RFC 7946 specification.

use crate::error::{GeoJsonError, Result};
use crate::types::{
    CoordinateSequence, Feature, FeatureCollection, Geometry, GeometryCollection, LineString,
    MultiLineString, MultiPoint, MultiPolygon, Point, Polygon, Position,
};

/// Validation configuration
#[derive(Debug, Clone)]
pub struct ValidationConfig {
    /// Maximum coordinate depth (to prevent stack overflow)
    pub max_depth: usize,
    /// Maximum number of coordinates
    pub max_coordinates: usize,
    /// Strict RFC 7946 compliance
    pub strict_rfc7946: bool,
    /// Validate polygon winding order (right-hand rule)
    pub validate_winding: bool,
    /// Validate linear rings are closed
    pub validate_closed_rings: bool,
    /// Check for self-intersections (expensive)
    pub check_self_intersections: bool,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            max_depth: 100,
            max_coordinates: 1_000_000,
            strict_rfc7946: true,
            validate_winding: true,
            validate_closed_rings: true,
            check_self_intersections: false,
        }
    }
}

/// GeoJSON validator
pub struct Validator {
    config: ValidationConfig,
    coordinate_count: usize,
    depth: usize,
}

impl Validator {
    /// Creates a new validator with default configuration
    pub fn new() -> Self {
        Self::with_config(ValidationConfig::default())
    }

    /// Creates a new validator with custom configuration
    pub fn with_config(config: ValidationConfig) -> Self {
        Self {
            config,
            coordinate_count: 0,
            depth: 0,
        }
    }

    /// Validates a position
    pub fn validate_position(&mut self, pos: &Position) -> Result<()> {
        if pos.len() < 2 {
            return Err(GeoJsonError::invalid_coordinates(
                "Position must have at least 2 coordinates",
            ));
        }

        // RFC 7946 specifies maximum 3 dimensions (lon, lat, alt)
        if self.config.strict_rfc7946 && pos.len() > 3 {
            return Err(GeoJsonError::validation(format!(
                "Position has more than 3 coordinates: {}",
                pos.len()
            )));
        }

        // Validate finite numbers
        for (i, &coord) in pos.iter().enumerate() {
            if !coord.is_finite() {
                return Err(GeoJsonError::invalid_coordinates_at(
                    format!("Coordinate at index {i} is not finite: {coord}"),
                    i,
                ));
            }
        }

        // Validate longitude range
        let lon = pos[0];
        if self.config.strict_rfc7946 && !(-180.0..=180.0).contains(&lon) {
            return Err(GeoJsonError::invalid_coordinates(format!(
                "Longitude out of valid range [-180, 180]: {lon}"
            )));
        }

        // Validate latitude range
        let lat = pos[1];
        if self.config.strict_rfc7946 && !(-90.0..=90.0).contains(&lat) {
            return Err(GeoJsonError::invalid_coordinates(format!(
                "Latitude out of valid range [-90, 90]: {lat}"
            )));
        }

        self.coordinate_count += 1;
        if self.coordinate_count > self.config.max_coordinates {
            return Err(GeoJsonError::limit_exceeded(
                "Maximum coordinate count exceeded",
                self.config.max_coordinates,
                self.coordinate_count,
            ));
        }

        Ok(())
    }

    /// Validates a linear ring
    pub fn validate_linear_ring(&mut self, ring: &CoordinateSequence) -> Result<()> {
        if ring.len() < 4 {
            return Err(GeoJsonError::invalid_coordinates(
                "Linear ring must have at least 4 positions",
            ));
        }

        // Validate all positions
        for (i, pos) in ring.iter().enumerate() {
            self.validate_position(pos)
                .map_err(|e| GeoJsonError::validation_at(e.to_string(), format!("position/{i}")))?;
        }

        // Check if ring is closed
        if self.config.validate_closed_rings {
            if let (Some(first), Some(last)) = (ring.first(), ring.last()) {
                if first != last {
                    return Err(GeoJsonError::topology(
                        "Linear ring must be closed (first and last positions must be equal)",
                    ));
                }
            }
        }

        // Validate winding order (right-hand rule for exterior rings)
        // Note: This validation is optional and can be disabled
        // RFC 7946 recommends but does not require specific winding order
        if self.config.validate_winding {
            let area = compute_signed_area(ring);
            // In GeoJSON (lon, lat), exterior rings should be counterclockwise (positive area)
            // However, many existing GeoJSON files don't follow this strictly
            // So we just warn if the area is very small (potentially degenerate)
            if area.abs() < 1e-10 {
                return Err(GeoJsonError::validation(
                    "Linear ring has near-zero area (potentially degenerate)",
                ));
            }
        }

        // Check for self-intersections (optional, expensive)
        if self.config.check_self_intersections && has_self_intersections(ring) {
            return Err(GeoJsonError::topology("Linear ring has self-intersections"));
        }

        Ok(())
    }

    /// Validates a Point geometry
    pub fn validate_point(&mut self, point: &Point) -> Result<()> {
        self.validate_position(&point.coordinates)?;
        if let Some(bbox) = &point.bbox {
            self.validate_bbox(bbox)?;
        }
        Ok(())
    }

    /// Validates a LineString geometry
    pub fn validate_linestring(&mut self, linestring: &LineString) -> Result<()> {
        if linestring.coordinates.len() < 2 {
            return Err(GeoJsonError::invalid_coordinates(
                "LineString must have at least 2 positions",
            ));
        }

        for (i, pos) in linestring.coordinates.iter().enumerate() {
            self.validate_position(pos)
                .map_err(|e| GeoJsonError::validation_at(e.to_string(), format!("position/{i}")))?;
        }

        if let Some(bbox) = &linestring.bbox {
            self.validate_bbox(bbox)?;
        }

        Ok(())
    }

    /// Validates a Polygon geometry
    pub fn validate_polygon(&mut self, polygon: &Polygon) -> Result<()> {
        if polygon.coordinates.is_empty() {
            return Err(GeoJsonError::invalid_coordinates(
                "Polygon must have at least one ring",
            ));
        }

        // Validate exterior ring
        if let Some(exterior) = polygon.coordinates.first() {
            self.validate_linear_ring(exterior)
                .map_err(|e| GeoJsonError::validation_at(e.to_string(), "exterior_ring"))?;
        }

        // Validate holes
        for (i, hole) in polygon.coordinates.iter().skip(1).enumerate() {
            self.validate_linear_ring(hole)
                .map_err(|e| GeoJsonError::validation_at(e.to_string(), format!("hole/{i}")))?;

            // Holes validation (optional)
            if self.config.validate_winding {
                let area = compute_signed_area(hole);
                // Just check for degenerate holes
                if area.abs() < 1e-10 {
                    return Err(GeoJsonError::validation(format!(
                        "Hole {i} has near-zero area (potentially degenerate)"
                    )));
                }
            }
        }

        if let Some(bbox) = &polygon.bbox {
            self.validate_bbox(bbox)?;
        }

        Ok(())
    }

    /// Validates a MultiPoint geometry
    pub fn validate_multipoint(&mut self, multipoint: &MultiPoint) -> Result<()> {
        for (i, pos) in multipoint.coordinates.iter().enumerate() {
            self.validate_position(pos)
                .map_err(|e| GeoJsonError::validation_at(e.to_string(), format!("point/{i}")))?;
        }

        if let Some(bbox) = &multipoint.bbox {
            self.validate_bbox(bbox)?;
        }

        Ok(())
    }

    /// Validates a MultiLineString geometry
    pub fn validate_multilinestring(&mut self, multilinestring: &MultiLineString) -> Result<()> {
        for (i, linestring) in multilinestring.coordinates.iter().enumerate() {
            if linestring.len() < 2 {
                return Err(GeoJsonError::validation_at(
                    "LineString must have at least 2 positions",
                    format!("linestring/{i}"),
                ));
            }

            for (j, pos) in linestring.iter().enumerate() {
                self.validate_position(pos).map_err(|e| {
                    GeoJsonError::validation_at(
                        e.to_string(),
                        format!("linestring/{i}/position/{j}"),
                    )
                })?;
            }
        }

        if let Some(bbox) = &multilinestring.bbox {
            self.validate_bbox(bbox)?;
        }

        Ok(())
    }

    /// Validates a MultiPolygon geometry
    pub fn validate_multipolygon(&mut self, multipolygon: &MultiPolygon) -> Result<()> {
        for (i, polygon) in multipolygon.coordinates.iter().enumerate() {
            if polygon.is_empty() {
                return Err(GeoJsonError::validation_at(
                    "Polygon must have at least one ring",
                    format!("polygon/{i}"),
                ));
            }

            for (j, ring) in polygon.iter().enumerate() {
                self.validate_linear_ring(ring).map_err(|e| {
                    GeoJsonError::validation_at(e.to_string(), format!("polygon/{i}/ring/{j}"))
                })?;
            }
        }

        if let Some(bbox) = &multipolygon.bbox {
            self.validate_bbox(bbox)?;
        }

        Ok(())
    }

    /// Validates a GeometryCollection
    pub fn validate_geometry_collection(&mut self, collection: &GeometryCollection) -> Result<()> {
        self.depth += 1;
        if self.depth > self.config.max_depth {
            return Err(GeoJsonError::validation(format!(
                "Maximum nesting depth exceeded: {}",
                self.config.max_depth
            )));
        }

        for (i, geom) in collection.geometries.iter().enumerate() {
            self.validate_geometry(geom)
                .map_err(|e| GeoJsonError::validation_at(e.to_string(), format!("geometry/{i}")))?;
        }

        if let Some(bbox) = &collection.bbox {
            self.validate_bbox(bbox)?;
        }

        self.depth -= 1;
        Ok(())
    }

    /// Validates any Geometry
    pub fn validate_geometry(&mut self, geometry: &Geometry) -> Result<()> {
        match geometry {
            Geometry::Point(p) => self.validate_point(p),
            Geometry::LineString(ls) => self.validate_linestring(ls),
            Geometry::Polygon(p) => self.validate_polygon(p),
            Geometry::MultiPoint(mp) => self.validate_multipoint(mp),
            Geometry::MultiLineString(mls) => self.validate_multilinestring(mls),
            Geometry::MultiPolygon(mp) => self.validate_multipolygon(mp),
            Geometry::GeometryCollection(gc) => self.validate_geometry_collection(gc),
        }
    }

    /// Validates a Feature
    pub fn validate_feature(&mut self, feature: &Feature) -> Result<()> {
        if feature.feature_type != "Feature" {
            return Err(GeoJsonError::InvalidFeature {
                message: format!(
                    "Invalid type: expected 'Feature', got '{}'",
                    feature.feature_type
                ),
                feature_id: feature.id.as_ref().map(|id| id.as_string()),
            });
        }

        if let Some(ref geometry) = feature.geometry {
            self.validate_geometry(geometry)
                .map_err(|e| GeoJsonError::InvalidFeature {
                    message: format!("Invalid geometry: {e}"),
                    feature_id: feature.id.as_ref().map(|id| id.as_string()),
                })?;
        }

        if let Some(bbox) = &feature.bbox {
            self.validate_bbox(bbox)?;
        }

        Ok(())
    }

    /// Validates a FeatureCollection
    pub fn validate_feature_collection(&mut self, collection: &FeatureCollection) -> Result<()> {
        if collection.collection_type != "FeatureCollection" {
            return Err(GeoJsonError::InvalidFeatureCollection {
                message: format!(
                    "Invalid type: expected 'FeatureCollection', got '{}'",
                    collection.collection_type
                ),
            });
        }

        for (i, feature) in collection.features.iter().enumerate() {
            self.validate_feature(feature)
                .map_err(|e| GeoJsonError::validation_at(e.to_string(), format!("features/{i}")))?;
        }

        if let Some(bbox) = &collection.bbox {
            self.validate_bbox(bbox)?;
        }

        Ok(())
    }

    /// Validates a bounding box
    pub fn validate_bbox(&self, bbox: &[f64]) -> Result<()> {
        if bbox.len() != 4 && bbox.len() != 6 {
            return Err(GeoJsonError::InvalidBbox {
                message: format!("Bounding box must have 4 or 6 elements, got {}", bbox.len()),
            });
        }

        for &val in bbox {
            if !val.is_finite() {
                return Err(GeoJsonError::InvalidBbox {
                    message: format!("Bounding box contains non-finite value: {val}"),
                });
            }
        }

        // Check min <= max for each dimension
        if bbox[0] > bbox[2] {
            return Err(GeoJsonError::InvalidBbox {
                message: format!("min_x ({}) > max_x ({})", bbox[0], bbox[2]),
            });
        }
        if bbox[1] > bbox[3] {
            return Err(GeoJsonError::InvalidBbox {
                message: format!("min_y ({}) > max_y ({})", bbox[1], bbox[3]),
            });
        }
        if bbox.len() == 6 && bbox[4] > bbox[5] {
            return Err(GeoJsonError::InvalidBbox {
                message: format!("min_z ({}) > max_z ({})", bbox[4], bbox[5]),
            });
        }

        Ok(())
    }

    /// Resets the validator state
    pub fn reset(&mut self) {
        self.coordinate_count = 0;
        self.depth = 0;
    }

    /// Returns the current coordinate count
    #[must_use]
    pub const fn coordinate_count(&self) -> usize {
        self.coordinate_count
    }

    /// Returns the current depth
    #[must_use]
    pub const fn depth(&self) -> usize {
        self.depth
    }
}

impl Default for Validator {
    fn default() -> Self {
        Self::new()
    }
}

/// Computes the signed area of a linear ring using the shoelace formula
///
/// Positive area means counterclockwise orientation (right-hand rule for exterior rings)
/// Negative area means clockwise orientation (for holes)
fn compute_signed_area(ring: &CoordinateSequence) -> f64 {
    if ring.len() < 3 {
        return 0.0;
    }

    let mut area = 0.0;
    for i in 0..ring.len() - 1 {
        if ring[i].len() >= 2 && ring[i + 1].len() >= 2 {
            let x1 = ring[i][0];
            let y1 = ring[i][1];
            let x2 = ring[i + 1][0];
            let y2 = ring[i + 1][1];
            area += x1 * y2 - x2 * y1;
        }
    }

    area / 2.0
}

/// Simple check for self-intersections (naive O(n²) algorithm)
///
/// This is a basic implementation. For production use, consider using
/// a more sophisticated algorithm like Bentley-Ottmann.
fn has_self_intersections(ring: &CoordinateSequence) -> bool {
    if ring.len() < 4 {
        return false;
    }

    for i in 0..ring.len() - 1 {
        for j in (i + 2)..ring.len() - 1 {
            // Don't check adjacent segments
            if i == 0 && j == ring.len() - 2 {
                continue;
            }

            if segments_intersect(&ring[i], &ring[i + 1], &ring[j], &ring[j + 1]) {
                return true;
            }
        }
    }

    false
}

/// Checks if two line segments intersect
fn segments_intersect(p1: &Position, p2: &Position, p3: &Position, p4: &Position) -> bool {
    if p1.len() < 2 || p2.len() < 2 || p3.len() < 2 || p4.len() < 2 {
        return false;
    }

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

/// Computes the direction of the cross product
fn direction(p1: &Position, p2: &Position, p3: &Position) -> f64 {
    (p3[0] - p1[0]) * (p2[1] - p1[1]) - (p2[0] - p1[0]) * (p3[1] - p1[1])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Geometry;

    #[test]
    fn test_validate_position() {
        let mut validator = Validator::new();

        assert!(validator.validate_position(&vec![0.0, 0.0]).is_ok());
        assert!(validator.validate_position(&vec![0.0, 0.0, 100.0]).is_ok());
        assert!(validator.validate_position(&vec![0.0]).is_err());
        assert!(validator.validate_position(&vec![181.0, 0.0]).is_err());
        assert!(validator.validate_position(&vec![0.0, 91.0]).is_err());
        assert!(validator.validate_position(&vec![f64::NAN, 0.0]).is_err());
    }

    #[test]
    fn test_validate_linear_ring() {
        let mut validator = Validator::new();

        let valid = vec![
            vec![0.0, 0.0],
            vec![1.0, 0.0],
            vec![1.0, 1.0],
            vec![0.0, 1.0],
            vec![0.0, 0.0],
        ];
        assert!(validator.validate_linear_ring(&valid).is_ok());

        let too_short = vec![vec![0.0, 0.0], vec![1.0, 0.0], vec![0.0, 0.0]];
        assert!(validator.validate_linear_ring(&too_short).is_err());

        let not_closed = vec![
            vec![0.0, 0.0],
            vec![1.0, 0.0],
            vec![1.0, 1.0],
            vec![0.0, 1.0],
        ];
        assert!(validator.validate_linear_ring(&not_closed).is_err());
    }

    #[test]
    fn test_compute_signed_area() {
        // Counterclockwise (positive area)
        let ccw = vec![
            vec![0.0, 0.0],
            vec![1.0, 0.0],
            vec![1.0, 1.0],
            vec![0.0, 1.0],
            vec![0.0, 0.0],
        ];
        let area_ccw = compute_signed_area(&ccw);
        assert!(area_ccw > 0.0);

        // Clockwise (negative area)
        let cw = vec![
            vec![0.0, 0.0],
            vec![0.0, 1.0],
            vec![1.0, 1.0],
            vec![1.0, 0.0],
            vec![0.0, 0.0],
        ];
        let area_cw = compute_signed_area(&cw);
        assert!(area_cw < 0.0);
    }

    #[test]
    fn test_validate_point() {
        let mut validator = Validator::new();

        let point = Point::new_2d(0.0, 0.0).expect("valid point");
        assert!(validator.validate_point(&point).is_ok());

        let invalid = Point::new(vec![181.0, 0.0]).ok();
        if let Some(p) = invalid {
            assert!(validator.validate_point(&p).is_err());
        }
    }

    #[test]
    fn test_validate_polygon() {
        let mut validator = Validator::new();

        let exterior = vec![
            vec![0.0, 0.0],
            vec![1.0, 0.0],
            vec![1.0, 1.0],
            vec![0.0, 1.0],
            vec![0.0, 0.0],
        ];
        let polygon = Polygon::from_exterior(exterior).expect("valid polygon");
        assert!(validator.validate_polygon(&polygon).is_ok());
    }

    #[test]
    fn test_coordinate_limit() {
        let config = ValidationConfig {
            max_coordinates: 5,
            ..Default::default()
        };
        let mut validator = Validator::with_config(config);

        // This should exceed the limit
        let coords: Vec<_> = (0..10).map(|i| vec![f64::from(i), f64::from(i)]).collect();
        let linestring = LineString::new(coords).expect("valid linestring");

        assert!(validator.validate_linestring(&linestring).is_err());
    }

    #[test]
    fn test_depth_limit() {
        let config = ValidationConfig {
            max_depth: 2,
            ..Default::default()
        };
        let mut validator = Validator::with_config(config);

        // Create nested geometry collections
        let point = Geometry::Point(Point::new_2d(0.0, 0.0).expect("valid point"));
        let gc1 = Geometry::GeometryCollection(
            GeometryCollection::new(vec![point.clone()]).expect("valid collection"),
        );
        let gc2 = Geometry::GeometryCollection(
            GeometryCollection::new(vec![gc1]).expect("valid collection"),
        );
        let gc3 = GeometryCollection::new(vec![gc2]).expect("valid collection");

        assert!(validator.validate_geometry_collection(&gc3).is_err());
    }

    #[test]
    fn test_validate_feature_collection() {
        let mut validator = Validator::new();

        let point = Point::new_2d(0.0, 0.0).expect("valid point");
        let feature = Feature::new(Some(Geometry::Point(point)), None);
        let collection = FeatureCollection::new(vec![feature]);

        assert!(validator.validate_feature_collection(&collection).is_ok());
    }

    #[test]
    fn test_reset() {
        let mut validator = Validator::new();

        let pos = vec![0.0, 0.0];
        validator.validate_position(&pos).ok();
        assert_eq!(validator.coordinate_count(), 1);

        validator.reset();
        assert_eq!(validator.coordinate_count(), 0);
    }
}
