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
/// When the `crs-transform` feature is enabled, this uses the `oxigdal-proj`
/// backend (pure-Rust proj4rs) for arbitrary CRS pairs.  The two hardcoded
/// paths (WGS84↔Web Mercator) are kept as a fallback so that the API behaves
/// identically when the feature is disabled.
pub struct CrsTransformer {
    source_crs: String,
    target_crs: String,
    #[cfg(feature = "crs-transform")]
    proj_transformer: Option<oxigdal_proj::Transformer>,
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

        #[cfg(feature = "crs-transform")]
        let proj_transformer = Self::build_proj_transformer(&source, &target);

        Ok(Self {
            source_crs: source,
            target_crs: target,
            #[cfg(feature = "crs-transform")]
            proj_transformer,
        })
    }

    /// Attempts to construct an `oxigdal_proj::Transformer` from CRS strings.
    ///
    /// The constructor is deliberately infallible from the caller's perspective:
    /// any initialisation failure is logged at debug level and returns `None`,
    /// which causes `transform_coordinate` to fall back to the hardcoded paths.
    #[cfg(feature = "crs-transform")]
    fn build_proj_transformer(source: &str, target: &str) -> Option<oxigdal_proj::Transformer> {
        let src_crs = Self::parse_crs_string(source).ok()?;
        let tgt_crs = Self::parse_crs_string(target).ok()?;
        match oxigdal_proj::Transformer::new(src_crs, tgt_crs) {
            Ok(t) => Some(t.with_strict(false)),
            Err(e) => {
                tracing::debug!(
                    "oxigdal-proj: could not initialise transformer {} → {}: {}",
                    source,
                    target,
                    e
                );
                None
            }
        }
    }

    /// Parses a CRS identifier string into an `oxigdal_proj::Crs`.
    ///
    /// Recognised formats:
    /// - `EPSG:<code>` (case-insensitive)
    /// - Any string starting with `+proj=` (PROJ string)
    /// - Everything else is passed to `Crs::from_wkt`
    #[cfg(feature = "crs-transform")]
    fn parse_crs_string(s: &str) -> core::result::Result<oxigdal_proj::Crs, oxigdal_proj::Error> {
        let upper = s.trim().to_uppercase();
        if let Some(code_str) = upper.strip_prefix("EPSG:") {
            let code = code_str
                .trim()
                .parse::<u32>()
                .map_err(|_| oxigdal_proj::Error::invalid_epsg_code(0))?;
            oxigdal_proj::Crs::from_epsg(code)
        } else if upper.starts_with("+PROJ=") || upper.starts_with("+proj=") {
            oxigdal_proj::Crs::from_proj(s)
        } else {
            oxigdal_proj::Crs::from_wkt(s)
        }
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
        // Special case: Identity transformation — always fast-path, no proj needed.
        if self.source_crs == self.target_crs {
            return Ok(*coord);
        }

        // When the `crs-transform` feature is active and the proj backend was
        // initialised successfully, delegate all non-identity transformations to
        // proj4rs (via oxigdal-proj).  The hardcoded paths below are only reached
        // when the feature is disabled or proj initialisation failed for this pair.
        #[cfg(feature = "crs-transform")]
        if let Some(ref t) = self.proj_transformer {
            return Self::transform_via_proj(t, coord);
        }

        // Hardcoded fast paths (feature off, or proj backend unavailable for pair).

        // WGS84 to Web Mercator (common transformation)
        if self.source_crs == "EPSG:4326" && self.target_crs == "EPSG:3857" {
            return self.wgs84_to_web_mercator(coord);
        }

        // Web Mercator to WGS84
        if self.source_crs == "EPSG:3857" && self.target_crs == "EPSG:4326" {
            return self.web_mercator_to_wgs84(coord);
        }

        // For other transformations, proj integration is required
        Err(AlgorithmError::UnsupportedOperation {
            operation: format!(
                "Coordinate transformation from {} to {} (requires crs-transform feature)",
                self.source_crs, self.target_crs
            ),
        })
    }

    /// Delegates a single coordinate transformation to `oxigdal_proj::Transformer`.
    ///
    /// Error mapping:
    /// - `OutOfAreaOfUse` → coordinate returned unchanged (logged at debug level, not an error).
    ///   This preserves backward-compatibility: callers that worked without the proj backend
    ///   would have received no area-of-use validation at all.
    /// - All other errors → `AlgorithmError::UnsupportedOperation`.
    #[cfg(feature = "crs-transform")]
    fn transform_via_proj(t: &oxigdal_proj::Transformer, coord: &Coordinate) -> Result<Coordinate> {
        let proj_coord = oxigdal_proj::Coordinate::new(coord.x, coord.y);
        match t.transform(&proj_coord) {
            Ok(out) => Ok(Coordinate::new_2d(out.x, out.y)),
            Err(oxigdal_proj::Error::OutOfAreaOfUse { lon, lat, .. }) => {
                tracing::debug!(
                    "oxigdal-proj: point ({}, {}) is outside area of use — returning unchanged",
                    lon,
                    lat
                );
                Ok(*coord)
            }
            Err(e) => Err(AlgorithmError::UnsupportedOperation {
                operation: format!("Coordinate transformation failed: {}", e),
            }),
        }
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
            // Latitude truly out of range (>90°): the Mercator formula produces
            // non-finite values, so this must error regardless of backend.
            let invalid = Coordinate::new_2d(0.0, 95.0);
            let result = t.transform_coordinate(&invalid);
            assert!(result.is_err(), "lat=95 must be rejected");

            // lat=89° is geometrically valid for WGS84 and within the domain of
            // Web Mercator when using a proper proj backend (which can compute it
            // accurately).  The hardcoded fallback imposes a tighter ±85.0511°
            // limit for safety, so we only assert the stricter check when the
            // proj backend is not active.
            #[cfg(not(feature = "crs-transform"))]
            {
                let near_pole = Coordinate::new_2d(0.0, 89.0);
                let result = t.transform_coordinate(&near_pole);
                assert!(
                    result.is_err(),
                    "lat=89 must be rejected by hardcoded fallback"
                );
            }
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

    // -----------------------------------------------------------------------
    // New tests for the crs-transform feature (proj backend integration)
    // -----------------------------------------------------------------------

    /// Identity transformation: source == target == EPSG:4326.
    /// Output coordinate must be numerically identical to input.
    #[test]
    fn test_crs_transformer_wgs84_identity_passthrough() {
        let transformer = CrsTransformer::new("EPSG:4326", "EPSG:4326")
            .expect("identity transformer must construct");

        let input = Coordinate::new_2d(13.4050, 52.5200); // Berlin
        let output = transformer
            .transform_coordinate(&input)
            .expect("identity transform must succeed");

        assert!(
            (output.x - input.x).abs() < f64::EPSILON,
            "x must be unchanged: {} != {}",
            output.x,
            input.x
        );
        assert!(
            (output.y - input.y).abs() < f64::EPSILON,
            "y must be unchanged: {} != {}",
            output.y,
            input.y
        );
    }

    /// A polygon with 5 vertices (closed ring) must produce exactly 5 output vertices
    /// regardless of which CRS path is used.
    #[test]
    fn test_crs_transformer_polygon_preserves_vertex_count() {
        let coords = vec![
            Coordinate::new_2d(-10.0, -10.0),
            Coordinate::new_2d(10.0, -10.0),
            Coordinate::new_2d(10.0, 10.0),
            Coordinate::new_2d(-10.0, 10.0),
            Coordinate::new_2d(-10.0, -10.0), // closing vertex
        ];
        let exterior = LineString::new(coords).expect("linestring must construct");
        let polygon = Polygon::new(exterior, vec![]).expect("polygon must construct");

        // Use WGS84 → Web Mercator (always supported, with or without crs-transform feature)
        let transformer =
            CrsTransformer::new("EPSG:4326", "EPSG:3857").expect("transformer must construct");
        let result = transformer
            .transform_polygon(&polygon)
            .expect("polygon transform must succeed");

        assert_eq!(
            result.exterior.coords.len(),
            5,
            "transformed polygon must retain 5 vertices"
        );
    }

    /// When `crs-transform` feature is OFF (or proj fails for an exotic pair),
    /// `CrsTransformer::new` must still succeed and `transform_coordinate` must
    /// either return `UnsupportedOperation` or a valid result — never panic.
    #[test]
    fn test_crs_transformer_unknown_epsg_falls_back_gracefully() {
        // EPSG:32637 is WGS 84 / UTM zone 37N.  Without the feature, the hardcoded
        // paths don't cover it, so we expect either UnsupportedOperation or a
        // successful transform (when proj backend is available).
        let result = CrsTransformer::new("EPSG:4326", "EPSG:32637");
        // Construction must always succeed
        assert!(
            result.is_ok(),
            "CrsTransformer::new must not fail for any non-empty CRS string"
        );

        let transformer = result.expect("already asserted Ok above");
        let coord = Coordinate::new_2d(37.0, 55.0);
        let transform_result = transformer.transform_coordinate(&coord);

        // Either UnsupportedOperation (no-feature) or a valid coordinate (with-feature).
        // The invariant: it must NOT panic, and if Err it must be UnsupportedOperation.
        if let Err(ref e) = transform_result {
            assert!(
                matches!(e, AlgorithmError::UnsupportedOperation { .. }),
                "unexpected error variant: {:?}",
                e
            );
        }
        // If Ok, the coordinate must be finite
        if let Ok(ref c) = transform_result {
            assert!(c.x.is_finite() && c.y.is_finite(), "output must be finite");
        }
    }

    /// WGS84 origin (0°, 0°) must map to Web Mercator origin (0, 0) — this is
    /// a well-known property of the Mercator projection.
    #[test]
    fn test_crs_transformer_wgs84_to_webmercator_known_point() {
        let transformer =
            CrsTransformer::new("EPSG:4326", "EPSG:3857").expect("transformer must construct");

        let origin = Coordinate::new_2d(0.0, 0.0);
        let result = transformer
            .transform_coordinate(&origin)
            .expect("transform of origin must succeed");

        assert!(
            result.x.abs() < 1.0,
            "Web Mercator X at lon=0 must be ~0, got {}",
            result.x
        );
        assert!(
            result.y.abs() < 1.0,
            "Web Mercator Y at lat=0 must be ~0, got {}",
            result.y
        );
    }
}
