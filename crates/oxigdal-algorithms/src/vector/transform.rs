//! Coordinate transformation operations for vector geometries
//!
//! This module provides coordinate transformation capabilities for converting
//! geometries between different coordinate reference systems (CRS).
//!
//! # Features
//!
//! - **Point Transformation**: Transform individual points between CRS
//! - **Geometry Transformation**: Transform entire geometries (LineString, Polygon, etc.)
//! - **Batch Transformation**: Efficiently transform multiple coordinates at once
//! - **Projection Support**: Support for common projections (WGS84, Web Mercator, UTM, etc.)
//!
//! # Examples
//!
//! ```
//! use oxigdal_algorithms::vector::{Point, Coordinate, transform_point};
//!
//! // Transform from WGS84 (EPSG:4326) to Web Mercator (EPSG:3857)
//! let wgs84_point = Point::new(-122.4194, 37.7749); // San Francisco
//! # // In real usage, you would transform like this:
//! # // let web_mercator = transform_point(&wgs84_point, "EPSG:4326", "EPSG:3857").unwrap();
//! ```

use crate::error::{AlgorithmError, Result};
use oxigdal_core::vector::{
    Coordinate, Geometry, GeometryCollection, LineString, MultiLineString, MultiPoint,
    MultiPolygon, Point, Polygon,
};

#[cfg(feature = "std")]
use std::vec::Vec;

/// Common coordinate reference systems
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommonCrs {
    /// WGS84 geographic coordinates (latitude/longitude)
    Wgs84,
    /// Web Mercator (used by Google Maps, OpenStreetMap)
    WebMercator,
    /// UTM Zone (specify zone number and hemisphere)
    Utm { zone: u8, north: bool },
}

impl CommonCrs {
    /// Returns the EPSG code for this CRS
    pub fn epsg_code(&self) -> String {
        match self {
            Self::Wgs84 => "EPSG:4326".to_string(),
            Self::WebMercator => "EPSG:3857".to_string(),
            Self::Utm { zone, north } => {
                if *north {
                    format!("EPSG:326{:02}", zone)
                } else {
                    format!("EPSG:327{:02}", zone)
                }
            }
        }
    }
}

/// Transformer for coordinate reference system conversions
///
/// This is a placeholder structure. In a full implementation, this would
/// integrate with oxigdal-proj or proj4rs for actual transformations.
pub struct CrsTransformer {
    source_crs: String,
    target_crs: String,
}

impl CrsTransformer {
    /// Creates a new CRS transformer
    ///
    /// # Arguments
    ///
    /// * `source_crs` - Source CRS (e.g., "EPSG:4326")
    /// * `target_crs` - Target CRS (e.g., "EPSG:3857")
    ///
    /// # Returns
    ///
    /// A new transformer
    ///
    /// # Errors
    ///
    /// Returns error if CRS definitions are invalid
    pub fn new(source_crs: impl Into<String>, target_crs: impl Into<String>) -> Result<Self> {
        let source = source_crs.into();
        let target = target_crs.into();

        // Validate CRS strings
        if source.is_empty() || target.is_empty() {
            return Err(AlgorithmError::InvalidParameter {
                parameter: "crs",
                message: "CRS definition cannot be empty".to_string(),
            });
        }

        Ok(Self {
            source_crs: source,
            target_crs: target,
        })
    }

    /// Creates a transformer from common CRS types
    pub fn from_common(source: CommonCrs, target: CommonCrs) -> Result<Self> {
        Self::new(source.epsg_code(), target.epsg_code())
    }

    /// Transforms a single coordinate
    ///
    /// # Arguments
    ///
    /// * `coord` - Input coordinate in source CRS
    ///
    /// # Returns
    ///
    /// Transformed coordinate in target CRS
    ///
    /// # Errors
    ///
    /// Returns error if transformation fails
    pub fn transform_coordinate(&self, coord: &Coordinate) -> Result<Coordinate> {
        // Special case: Identity transformation
        if self.source_crs == self.target_crs {
            return Ok(*coord);
        }

        // Special case: WGS84 to Web Mercator (common transformation)
        if self.source_crs == "EPSG:4326" && self.target_crs == "EPSG:3857" {
            return self.wgs84_to_web_mercator(coord);
        }

        // Special case: Web Mercator to WGS84
        if self.source_crs == "EPSG:3857" && self.target_crs == "EPSG:4326" {
            return self.web_mercator_to_wgs84(coord);
        }

        // For other transformations, would integrate with oxigdal-proj
        // For now, return an error indicating unsupported transformation
        Err(AlgorithmError::UnsupportedOperation {
            operation: format!(
                "Coordinate transformation from {} to {} (requires proj integration)",
                self.source_crs, self.target_crs
            ),
        })
    }

    /// Transforms multiple coordinates efficiently
    pub fn transform_coordinates(&self, coords: &[Coordinate]) -> Result<Vec<Coordinate>> {
        coords
            .iter()
            .map(|c| self.transform_coordinate(c))
            .collect()
    }

    /// Transforms a point
    pub fn transform_point(&self, point: &Point) -> Result<Point> {
        let transformed = self.transform_coordinate(&point.coord)?;
        Ok(Point::from_coord(transformed))
    }

    /// Transforms a linestring
    pub fn transform_linestring(&self, linestring: &LineString) -> Result<LineString> {
        let coords = self.transform_coordinates(&linestring.coords)?;
        LineString::new(coords).map_err(|e| AlgorithmError::GeometryError {
            message: format!("Failed to create transformed linestring: {}", e),
        })
    }

    /// Transforms a polygon
    pub fn transform_polygon(&self, polygon: &Polygon) -> Result<Polygon> {
        let exterior_coords = self.transform_coordinates(&polygon.exterior.coords)?;
        let exterior =
            LineString::new(exterior_coords).map_err(|e| AlgorithmError::GeometryError {
                message: format!("Failed to create transformed exterior ring: {}", e),
            })?;

        let mut interiors = Vec::new();
        for hole in &polygon.interiors {
            let hole_coords = self.transform_coordinates(&hole.coords)?;
            let hole_ring =
                LineString::new(hole_coords).map_err(|e| AlgorithmError::GeometryError {
                    message: format!("Failed to create transformed interior ring: {}", e),
                })?;
            interiors.push(hole_ring);
        }

        Polygon::new(exterior, interiors).map_err(|e| AlgorithmError::GeometryError {
            message: format!("Failed to create transformed polygon: {}", e),
        })
    }

    /// Transforms a geometry
    pub fn transform_geometry(&self, geometry: &Geometry) -> Result<Geometry> {
        match geometry {
            Geometry::Point(p) => Ok(Geometry::Point(self.transform_point(p)?)),
            Geometry::LineString(ls) => Ok(Geometry::LineString(self.transform_linestring(ls)?)),
            Geometry::Polygon(poly) => Ok(Geometry::Polygon(self.transform_polygon(poly)?)),
            Geometry::MultiPoint(mp) => {
                let mut points = Vec::new();
                for point in &mp.points {
                    points.push(self.transform_point(point)?);
                }
                Ok(Geometry::MultiPoint(MultiPoint { points }))
            }
            Geometry::MultiLineString(mls) => {
                let mut line_strings = Vec::new();
                for ls in &mls.line_strings {
                    line_strings.push(self.transform_linestring(ls)?);
                }
                Ok(Geometry::MultiLineString(MultiLineString { line_strings }))
            }
            Geometry::MultiPolygon(mp) => {
                let mut polygons = Vec::new();
                for poly in &mp.polygons {
                    polygons.push(self.transform_polygon(poly)?);
                }
                Ok(Geometry::MultiPolygon(MultiPolygon { polygons }))
            }
            Geometry::GeometryCollection(gc) => {
                let mut geometries = Vec::new();
                for geom in &gc.geometries {
                    geometries.push(self.transform_geometry(geom)?);
                }
                Ok(Geometry::GeometryCollection(GeometryCollection {
                    geometries,
                }))
            }
        }
    }

    /// WGS84 (EPSG:4326) to Web Mercator (EPSG:3857) transformation
    fn wgs84_to_web_mercator(&self, coord: &Coordinate) -> Result<Coordinate> {
        const EARTH_RADIUS: f64 = 6_378_137.0;

        // Validate latitude range
        if !(-90.0..=90.0).contains(&coord.y) {
            return Err(AlgorithmError::InvalidParameter {
                parameter: "latitude",
                message: format!("Latitude {} is out of range [-90, 90]", coord.y),
            });
        }

        // Web Mercator doesn't work well near poles
        if coord.y.abs() > 85.0511 {
            return Err(AlgorithmError::InvalidParameter {
                parameter: "latitude",
                message: format!(
                    "Latitude {} is too close to poles for Web Mercator (max ±85.0511°)",
                    coord.y
                ),
            });
        }

        let lon_rad = coord.x.to_radians();
        let lat_rad = coord.y.to_radians();

        let x = EARTH_RADIUS * lon_rad;
        let y = EARTH_RADIUS * ((std::f64::consts::PI / 4.0 + lat_rad / 2.0).tan().ln());

        Ok(Coordinate::new_2d(x, y))
    }

    /// Web Mercator (EPSG:3857) to WGS84 (EPSG:4326) transformation
    fn web_mercator_to_wgs84(&self, coord: &Coordinate) -> Result<Coordinate> {
        const EARTH_RADIUS: f64 = 6_378_137.0;

        let lon = (coord.x / EARTH_RADIUS).to_degrees();
        let lat =
            (2.0 * (coord.y / EARTH_RADIUS).exp().atan() - std::f64::consts::PI / 2.0).to_degrees();

        // Clamp to valid ranges
        let lon = lon.clamp(-180.0, 180.0);
        let lat = lat.clamp(-90.0, 90.0);

        Ok(Coordinate::new_2d(lon, lat))
    }
}

/// Transforms a point between coordinate reference systems
///
/// # Arguments
///
/// * `point` - Input point
/// * `source_crs` - Source CRS (e.g., "EPSG:4326")
/// * `target_crs` - Target CRS (e.g., "EPSG:3857")
///
/// # Returns
///
/// Transformed point
///
/// # Errors
///
/// Returns error if transformation fails
pub fn transform_point(point: &Point, source_crs: &str, target_crs: &str) -> Result<Point> {
    let transformer = CrsTransformer::new(source_crs, target_crs)?;
    transformer.transform_point(point)
}

/// Transforms a linestring between coordinate reference systems
pub fn transform_linestring(
    linestring: &LineString,
    source_crs: &str,
    target_crs: &str,
) -> Result<LineString> {
    let transformer = CrsTransformer::new(source_crs, target_crs)?;
    transformer.transform_linestring(linestring)
}

/// Transforms a polygon between coordinate reference systems
pub fn transform_polygon(polygon: &Polygon, source_crs: &str, target_crs: &str) -> Result<Polygon> {
    let transformer = CrsTransformer::new(source_crs, target_crs)?;
    transformer.transform_polygon(polygon)
}

/// Transforms a geometry between coordinate reference systems
pub fn transform_geometry(
    geometry: &Geometry,
    source_crs: &str,
    target_crs: &str,
) -> Result<Geometry> {
    let transformer = CrsTransformer::new(source_crs, target_crs)?;
    transformer.transform_geometry(geometry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_common_crs_epsg_codes() {
        assert_eq!(CommonCrs::Wgs84.epsg_code(), "EPSG:4326");
        assert_eq!(CommonCrs::WebMercator.epsg_code(), "EPSG:3857");
        assert_eq!(
            CommonCrs::Utm {
                zone: 10,
                north: true
            }
            .epsg_code(),
            "EPSG:32610"
        );
        assert_eq!(
            CommonCrs::Utm {
                zone: 33,
                north: false
            }
            .epsg_code(),
            "EPSG:32733"
        );
    }

    #[test]
    fn test_transformer_creation() {
        let transformer = CrsTransformer::new("EPSG:4326", "EPSG:3857");
        assert!(transformer.is_ok());

        let empty = CrsTransformer::new("", "EPSG:3857");
        assert!(empty.is_err());
    }

    #[test]
    fn test_identity_transformation() {
        let transformer = CrsTransformer::new("EPSG:4326", "EPSG:4326");
        assert!(transformer.is_ok());

        if let Ok(t) = transformer {
            let coord = Coordinate::new_2d(10.0, 20.0);
            let result = t.transform_coordinate(&coord);
            assert!(result.is_ok());

            if let Ok(transformed) = result {
                assert!((transformed.x - 10.0).abs() < f64::EPSILON);
                assert!((transformed.y - 20.0).abs() < f64::EPSILON);
            }
        }
    }

    #[test]
    fn test_wgs84_to_web_mercator() {
        let transformer = CrsTransformer::new("EPSG:4326", "EPSG:3857");
        assert!(transformer.is_ok());

        if let Ok(t) = transformer {
            // Transform origin (0, 0)
            let origin = Coordinate::new_2d(0.0, 0.0);
            let result = t.transform_coordinate(&origin);
            assert!(result.is_ok());

            if let Ok(transformed) = result {
                assert!(transformed.x.abs() < 1.0);
                assert!(transformed.y.abs() < 1.0);
            }

            // Transform San Francisco
            let sf = Coordinate::new_2d(-122.4194, 37.7749);
            let result = t.transform_coordinate(&sf);
            assert!(result.is_ok());

            if let Ok(transformed) = result {
                // Web Mercator x should be negative (west of prime meridian)
                assert!(transformed.x < 0.0);
                // y should be positive (north of equator)
                assert!(transformed.y > 0.0);
            }
        }
    }

    #[test]
    fn test_web_mercator_to_wgs84() {
        let transformer = CrsTransformer::new("EPSG:3857", "EPSG:4326");
        assert!(transformer.is_ok());

        if let Ok(t) = transformer {
            // Transform origin
            let origin = Coordinate::new_2d(0.0, 0.0);
            let result = t.transform_coordinate(&origin);
            assert!(result.is_ok());

            if let Ok(transformed) = result {
                assert!(transformed.x.abs() < 1e-6);
                assert!(transformed.y.abs() < 1e-6);
            }
        }
    }

    #[test]
    fn test_round_trip_transformation() {
        let to_merc = CrsTransformer::new("EPSG:4326", "EPSG:3857");
        let to_wgs = CrsTransformer::new("EPSG:3857", "EPSG:4326");

        assert!(to_merc.is_ok());
        assert!(to_wgs.is_ok());

        if let (Ok(t1), Ok(t2)) = (to_merc, to_wgs) {
            let original = Coordinate::new_2d(-122.4194, 37.7749);

            let merc = t1.transform_coordinate(&original);
            assert!(merc.is_ok());

            if let Ok(m) = merc {
                let back = t2.transform_coordinate(&m);
                assert!(back.is_ok());

                if let Ok(b) = back {
                    // Should be close to original (within tolerance)
                    assert!((b.x - original.x).abs() < 1e-6);
                    assert!((b.y - original.y).abs() < 1e-6);
                }
            }
        }
    }

    #[test]
    fn test_transform_point() {
        let point = Point::new(-122.4194, 37.7749);
        let result = transform_point(&point, "EPSG:4326", "EPSG:3857");
        assert!(result.is_ok());

        if let Ok(transformed) = result {
            assert!(transformed.coord.x < 0.0);
            assert!(transformed.coord.y > 0.0);
        }
    }

    #[test]
    fn test_transform_linestring() {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(2.0, 2.0),
        ];
        let linestring = LineString::new(coords);
        assert!(linestring.is_ok());

        if let Ok(ls) = linestring {
            let result = transform_linestring(&ls, "EPSG:4326", "EPSG:3857");
            assert!(result.is_ok());

            if let Ok(transformed) = result {
                assert_eq!(transformed.coords.len(), 3);
            }
        }
    }

    #[test]
    fn test_transform_polygon() {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(4.0, 0.0),
            Coordinate::new_2d(4.0, 4.0),
            Coordinate::new_2d(0.0, 4.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let exterior = LineString::new(coords);
        assert!(exterior.is_ok());

        if let Ok(ext) = exterior {
            let polygon = Polygon::new(ext, vec![]);
            assert!(polygon.is_ok());

            if let Ok(poly) = polygon {
                let result = transform_polygon(&poly, "EPSG:4326", "EPSG:3857");
                assert!(result.is_ok());

                if let Ok(transformed) = result {
                    assert_eq!(transformed.exterior.coords.len(), 5);
                }
            }
        }
    }

    #[test]
    fn test_invalid_latitude() {
        let transformer = CrsTransformer::new("EPSG:4326", "EPSG:3857");
        assert!(transformer.is_ok());

        if let Ok(t) = transformer {
            // Latitude too high
            let invalid = Coordinate::new_2d(0.0, 95.0);
            let result = t.transform_coordinate(&invalid);
            assert!(result.is_err());

            // Latitude near pole (outside Web Mercator range)
            let near_pole = Coordinate::new_2d(0.0, 89.0);
            let result = t.transform_coordinate(&near_pole);
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_batch_transformation() {
        let transformer = CrsTransformer::new("EPSG:4326", "EPSG:3857");
        assert!(transformer.is_ok());

        if let Ok(t) = transformer {
            let coords = vec![
                Coordinate::new_2d(0.0, 0.0),
                Coordinate::new_2d(1.0, 1.0),
                Coordinate::new_2d(-1.0, -1.0),
            ];

            let result = t.transform_coordinates(&coords);
            assert!(result.is_ok());

            if let Ok(transformed) = result {
                assert_eq!(transformed.len(), 3);
            }
        }
    }

    #[test]
    fn test_from_common_crs() {
        let transformer = CrsTransformer::from_common(CommonCrs::Wgs84, CommonCrs::WebMercator);
        assert!(transformer.is_ok());

        if let Ok(t) = transformer {
            assert_eq!(t.source_crs, "EPSG:4326");
            assert_eq!(t.target_crs, "EPSG:3857");
        }
    }
}
