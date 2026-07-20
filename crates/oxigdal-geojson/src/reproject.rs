//! CRS reprojection for GeoJSON feature collections.
//!
//! Provides [`Reprojector`], which transforms every coordinate in a
//! [`FeatureCollection`], a [`GeoJsonFeature`], or a single
//! [`GeoJsonGeometry`] from a source CRS to a target CRS.
//!
//! Currently supports:
//! - Identity transforms (source == target).
//! - EPSG:4326 ↔ EPSG:3857 (WGS 84 geographic ↔ Web Mercator).
//! - Any pair that `oxigdal-proj`'s `Transformer::from_epsg` can handle.
//!
//! # Feature flag
//!
//! This module is compiled only when the `reproject` Cargo feature is enabled.

use serde_json::Value;

use crate::error::GeoJsonError;
use crate::parser::FeatureCollection;
use crate::types::{GeoJsonFeature, GeoJsonGeometry};

#[cfg(feature = "reproject")]
use oxigdal_proj::{Coordinate, Transformer};

// ─────────────────────────────────────────────────────────────────────────────
// ReprojectOptions
// ─────────────────────────────────────────────────────────────────────────────

/// Configuration for a reprojection operation.
#[derive(Debug, Clone)]
pub struct ReprojectOptions {
    /// EPSG code string for the source CRS, e.g. `"EPSG:4326"`.
    pub source_crs: String,
    /// EPSG code string for the target CRS, e.g. `"EPSG:3857"`.
    pub target_crs: String,
    /// When `Some(n)`, divide each segment into `n` sub-segments before
    /// reprojecting (densification).  `None` means no densification.
    pub densify_segments: Option<usize>,
    /// Return `Err` when a reprojected coordinate is NaN.
    pub error_on_nan: bool,
}

impl Default for ReprojectOptions {
    fn default() -> Self {
        Self {
            source_crs: "EPSG:4326".to_string(),
            target_crs: "EPSG:4326".to_string(),
            densify_segments: None,
            error_on_nan: true,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Reprojector
// ─────────────────────────────────────────────────────────────────────────────

/// Transforms GeoJSON geometries from one EPSG-based CRS to another.
///
/// The reprojector is constructed once (parsing and resolving the CRS pair)
/// and can then be applied to any number of geometries.
pub struct Reprojector {
    source_epsg: u32,
    target_epsg: u32,
    options: ReprojectOptions,
    /// The underlying proj transformer (None when source == target).
    #[allow(dead_code)]
    transformer: Option<Transformer>,
}

impl Reprojector {
    /// Construct a reprojector from the given options.
    ///
    /// # Errors
    ///
    /// Returns [`GeoJsonError::ReprojectError`] when either CRS string cannot
    /// be parsed as an EPSG code, or when the CRS pair is unsupported.
    pub fn new(options: ReprojectOptions) -> Result<Self, GeoJsonError> {
        let source_epsg = parse_epsg_code(&options.source_crs)?;
        let target_epsg = parse_epsg_code(&options.target_crs)?;

        // Build the underlying transformer for supported pairs.
        let transformer = if source_epsg == target_epsg {
            None
        } else {
            match build_transformer(source_epsg, target_epsg) {
                Ok(t) => Some(t),
                Err(e) => {
                    // Only hard-fail if it is not one of the pairs we handle
                    // ourselves analytically below.
                    if !is_analytically_supported(source_epsg, target_epsg) {
                        return Err(e);
                    }
                    // For the analytically-handled pairs we don't need the
                    // proj transformer at all.
                    None
                }
            }
        };

        Ok(Self {
            source_epsg,
            target_epsg,
            options,
            transformer,
        })
    }

    /// Reproject a single 2-D position `[x, y]`.
    ///
    /// Returns a new `[f64; 2]` with transformed coordinates.
    ///
    /// # Errors
    ///
    /// Returns [`GeoJsonError::ReprojectError`] on unsupported CRS pairs or
    /// NaN output (when `options.error_on_nan` is `true`).
    pub fn reproject_2d(&self, pos: [f64; 2]) -> Result<[f64; 2], GeoJsonError> {
        let [x, y] = pos;
        let (nx, ny) = self.transform_xy(x, y)?;
        if self.options.error_on_nan && (!nx.is_finite() || !ny.is_finite()) {
            return Err(GeoJsonError::ReprojectError(format!(
                "Reprojected coordinate is non-finite: ({nx}, {ny})"
            )));
        }
        Ok([nx, ny])
    }

    /// Reproject a single 3-D position `[x, y, z]`.  Z passes through.
    ///
    /// # Errors
    ///
    /// Same as [`reproject_2d`](Self::reproject_2d).
    pub fn reproject_3d(&self, pos: [f64; 3]) -> Result<[f64; 3], GeoJsonError> {
        let [x, y, z] = pos;
        let (nx, ny) = self.transform_xy(x, y)?;
        if self.options.error_on_nan && (!nx.is_finite() || !ny.is_finite()) {
            return Err(GeoJsonError::ReprojectError(format!(
                "Reprojected coordinate is non-finite: ({nx}, {ny})"
            )));
        }
        Ok([nx, ny, z])
    }

    /// Reproject a [`GeoJsonGeometry`] in-place.
    ///
    /// Every coordinate position is transformed; Z ordinates are preserved.
    /// `GeometryCollection` members are reprojected recursively.
    ///
    /// # Errors
    ///
    /// Returns on the first coordinate that fails.
    pub fn reproject_geometry(&self, geom: &mut GeoJsonGeometry) -> Result<(), GeoJsonError> {
        match geom {
            GeoJsonGeometry::Null => {}

            GeoJsonGeometry::Point(pos) => {
                *pos = self.reproject_2d(*pos)?;
            }
            GeoJsonGeometry::PointZ(pos) => {
                *pos = self.reproject_3d(*pos)?;
            }

            GeoJsonGeometry::LineString(pts) => {
                for p in pts.iter_mut() {
                    *p = self.reproject_2d(*p)?;
                }
            }
            GeoJsonGeometry::LineStringZ(pts) => {
                for p in pts.iter_mut() {
                    *p = self.reproject_3d(*p)?;
                }
            }

            GeoJsonGeometry::Polygon(rings) => {
                for ring in rings.iter_mut() {
                    for p in ring.iter_mut() {
                        *p = self.reproject_2d(*p)?;
                    }
                }
            }
            GeoJsonGeometry::PolygonZ(rings) => {
                for ring in rings.iter_mut() {
                    for p in ring.iter_mut() {
                        *p = self.reproject_3d(*p)?;
                    }
                }
            }

            GeoJsonGeometry::MultiPoint(pts) => {
                for p in pts.iter_mut() {
                    *p = self.reproject_2d(*p)?;
                }
            }
            GeoJsonGeometry::MultiPointZ(pts) => {
                for p in pts.iter_mut() {
                    *p = self.reproject_3d(*p)?;
                }
            }

            GeoJsonGeometry::MultiLineString(lines) => {
                for line in lines.iter_mut() {
                    for p in line.iter_mut() {
                        *p = self.reproject_2d(*p)?;
                    }
                }
            }
            GeoJsonGeometry::MultiLineStringZ(lines) => {
                for line in lines.iter_mut() {
                    for p in line.iter_mut() {
                        *p = self.reproject_3d(*p)?;
                    }
                }
            }

            GeoJsonGeometry::MultiPolygon(polys) => {
                for poly in polys.iter_mut() {
                    for ring in poly.iter_mut() {
                        for p in ring.iter_mut() {
                            *p = self.reproject_2d(*p)?;
                        }
                    }
                }
            }
            GeoJsonGeometry::MultiPolygonZ(polys) => {
                for poly in polys.iter_mut() {
                    for ring in poly.iter_mut() {
                        for p in ring.iter_mut() {
                            *p = self.reproject_3d(*p)?;
                        }
                    }
                }
            }

            GeoJsonGeometry::GeometryCollection(geoms) => {
                for g in geoms.iter_mut() {
                    self.reproject_geometry(g)?;
                }
            }
        }
        Ok(())
    }

    /// Reproject the geometry of a single [`GeoJsonFeature`] in-place.
    ///
    /// Properties and ID are untouched.
    ///
    /// # Errors
    ///
    /// Propagates from [`reproject_geometry`](Self::reproject_geometry).
    pub fn reproject_feature(&self, feature: &mut GeoJsonFeature) -> Result<(), GeoJsonError> {
        if let Some(geom) = &mut feature.geometry {
            self.reproject_geometry(geom)?;
        }
        Ok(())
    }

    /// Reproject every feature in a [`FeatureCollection`] in-place.
    ///
    /// When the target CRS is EPSG:4326, the deprecated `crs` member is
    /// removed from the collection (RFC 7946 §3.3).
    ///
    /// # Errors
    ///
    /// Returns on the first per-feature failure.
    pub fn reproject_feature_collection(
        &self,
        fc: &mut FeatureCollection,
    ) -> Result<(), GeoJsonError> {
        for feature in fc.features.iter_mut() {
            self.reproject_feature(feature)?;
        }
        // Remove the deprecated crs member when the target is WGS 84
        if self.target_epsg == 4326 {
            fc.crs = None;
        }
        Ok(())
    }

    // ── Internal helpers ────────────────────────────────────────────────────

    /// Dispatch a single (x, y) pair through the appropriate transform path.
    fn transform_xy(&self, x: f64, y: f64) -> Result<(f64, f64), GeoJsonError> {
        // Identity: no work needed.
        if self.source_epsg == self.target_epsg {
            return Ok((x, y));
        }

        // If we have a full proj transformer, use it first.
        if let Some(ref t) = self.transformer {
            let coord = Coordinate::new(x, y);
            let out = t.transform(&coord).map_err(|e| {
                GeoJsonError::ReprojectError(format!(
                    "PROJ transform EPSG:{} -> EPSG:{} failed: {e}",
                    self.source_epsg, self.target_epsg
                ))
            })?;
            return Ok((out.x, out.y));
        }

        // Fall back to our analytical implementations.
        transform_coords_analytic(x, y, self.source_epsg, self.target_epsg)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Public helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Extract the CRS name string from the legacy GeoJSON `crs` member.
///
/// Recognises the `{"type":"name","properties":{"name":"..."}}` form defined
/// by the pre-RFC 7946 GeoJSON specification.
///
/// Returns `None` when no `crs` key is present or when its structure does not
/// match the expected form.
pub fn extract_crs_from_geojson_value(value: &Value) -> Option<String> {
    let crs = value.get("crs")?;
    let props = crs.get("properties")?;
    let name = props.get("name")?.as_str()?;
    Some(name.to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
// Private helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Parse an EPSG code from a CRS identifier string.
///
/// Recognised patterns:
/// - `"EPSG:4326"` → `4326`
/// - `"epsg:4326"` (case-insensitive prefix) → `4326`
/// - `"urn:ogc:def:crs:EPSG::4326"` → `4326`
/// - `"urn:ogc:def:crs:OGC:1.3:CRS84"` → error (not numeric)
///
/// # Errors
///
/// Returns [`GeoJsonError::ReprojectError`] when the string cannot be parsed.
fn parse_epsg_code(crs_str: &str) -> Result<u32, GeoJsonError> {
    let s = crs_str.trim();

    // "EPSG:NNNN" (case-insensitive)
    let upper = s.to_uppercase();
    if let Some(rest) = upper.strip_prefix("EPSG:") {
        return rest
            .trim()
            .parse::<u32>()
            .map_err(|_| GeoJsonError::ReprojectError(format!("Invalid EPSG code: {s}")));
    }

    // URN form: "urn:ogc:def:crs:EPSG::NNNN"
    if upper.contains("EPSG") {
        // Walk from the end; find the last colon-separated token that is numeric.
        if let Some(code_str) = s.rsplit(':').find(|tok| !tok.is_empty())
            && let Ok(code) = code_str.trim().parse::<u32>()
        {
            return Ok(code);
        }
    }

    Err(GeoJsonError::ReprojectError(format!(
        "Cannot parse EPSG code from CRS string: {s}"
    )))
}

/// Return `true` when we handle `(from, to)` analytically without proj.
fn is_analytically_supported(from: u32, to: u32) -> bool {
    matches!((from, to), (4326, 3857) | (3857, 4326))
}

/// Build a `Transformer` for the given EPSG pair.
fn build_transformer(src: u32, dst: u32) -> Result<Transformer, GeoJsonError> {
    Transformer::from_epsg(src, dst)
        .map_err(|e| GeoJsonError::ReprojectError(format!("Cannot init transformer: {e}")))
}

/// Analytic transform for the supported hard-coded pairs.
///
/// Only EPSG:4326 ↔ EPSG:3857 are handled here; all other pairs return `Err`.
fn transform_coords_analytic(
    x: f64,
    y: f64,
    from_epsg: u32,
    to_epsg: u32,
) -> Result<(f64, f64), GeoJsonError> {
    const EARTH_CIRCUMFERENCE_HALF: f64 = 20_037_508.342_789_244;
    const PI: f64 = std::f64::consts::PI;

    match (from_epsg, to_epsg) {
        // WGS 84 lon/lat (degrees) → Web Mercator (metres)
        (4326, 3857) => {
            let x_out = x * EARTH_CIRCUMFERENCE_HALF / 180.0;
            let lat_rad = y.to_radians();
            let y_out = (PI / 4.0 + lat_rad / 2.0).tan().ln() * EARTH_CIRCUMFERENCE_HALF / PI;
            Ok((x_out, y_out))
        }
        // Web Mercator (metres) → WGS 84 lon/lat (degrees)
        (3857, 4326) => {
            let lon = x * 180.0 / EARTH_CIRCUMFERENCE_HALF;
            let lat =
                (2.0 * (x * PI / EARTH_CIRCUMFERENCE_HALF).exp().atan() - PI / 2.0).to_degrees();
            Ok((lon, lat))
        }
        _ => Err(GeoJsonError::ReprojectError(format!(
            "Unsupported CRS transform: EPSG:{from_epsg} -> EPSG:{to_epsg}"
        ))),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_epsg_direct() {
        assert_eq!(parse_epsg_code("EPSG:4326").expect("ok"), 4326);
        assert_eq!(parse_epsg_code("EPSG:3857").expect("ok"), 3857);
    }

    #[test]
    fn test_parse_epsg_urn() {
        assert_eq!(
            parse_epsg_code("urn:ogc:def:crs:EPSG::3857").expect("ok"),
            3857
        );
    }

    #[test]
    fn test_parse_epsg_invalid() {
        assert!(parse_epsg_code("+proj=longlat +datum=WGS84").is_err());
    }

    #[test]
    fn test_identity_reprojection() {
        let opts = ReprojectOptions::default();
        let r = Reprojector::new(opts).expect("ok");
        let out = r.reproject_2d([10.0, 20.0]).expect("ok");
        assert_eq!(out, [10.0, 20.0]);
    }

    #[test]
    fn test_wgs84_to_web_mercator_origin() {
        let opts = ReprojectOptions {
            source_crs: "EPSG:4326".into(),
            target_crs: "EPSG:3857".into(),
            ..Default::default()
        };
        let r = Reprojector::new(opts).expect("ok");
        let [x, y] = r.reproject_2d([0.0, 0.0]).expect("ok");
        assert!(x.abs() < 1e-6, "x={x}");
        assert!(y.abs() < 1e-6, "y={y}");
    }
}
