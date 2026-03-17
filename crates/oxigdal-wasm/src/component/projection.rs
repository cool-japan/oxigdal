//! Projection component interface (wasm32-wasip2 compatible).
//!
//! Provides lightweight, serialisable CRS descriptors and coordinate
//! transformation helpers that do not depend on any C-backed projection
//! library, making them safe to use in a WASM Component Model context.

use crate::component::types::{ComponentError, ComponentResult};

/// A projected (or geographic) coordinate pair.
#[derive(Debug, Clone, PartialEq)]
pub struct ComponentCoord {
    /// Easting, longitude, or X in the CRS units.
    pub x: f64,
    /// Northing, latitude, or Y in the CRS units.
    pub y: f64,
}

impl ComponentCoord {
    /// Create a new coordinate pair.
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Euclidean distance to `other` in CRS units.
    pub fn distance_to(&self, other: &Self) -> f64 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }

    /// Midpoint between `self` and `other`.
    pub fn midpoint(&self, other: &Self) -> Self {
        Self::new((self.x + other.x) / 2.0, (self.y + other.y) / 2.0)
    }
}

/// WASM-safe descriptor for a Coordinate Reference System.
///
/// This does *not* perform projection transformations — it is a value object
/// that carries enough metadata for host-side rendering decisions.
#[derive(Debug, Clone)]
pub struct ComponentProjection {
    /// EPSG authority code, if known.
    pub epsg_code: Option<u32>,
    /// Well-Known Text (WKT 2) representation.
    pub wkt: String,
    /// PROJ string, if available.
    pub proj_string: Option<String>,
    /// True for geographic CRS (angles in degrees).
    pub is_geographic: bool,
    /// True for projected CRS (linear units, usually metres or feet).
    pub is_projected: bool,
    /// Name of the CRS (e.g. "WGS 84").
    pub name: String,
    /// Units of measure (e.g. "degree", "metre").
    pub units: String,
}

impl ComponentProjection {
    /// WGS 84 geographic CRS (EPSG:4326).
    pub fn wgs84() -> Self {
        Self {
            epsg_code: Some(4326),
            wkt: concat!(
                r#"GEOGCRS["WGS 84",DATUM["World Geodetic System 1984","#,
                r#"ELLIPSOID["WGS 84",6378137,298.257223563]],CS[ellipsoidal,2],"#,
                r#"AXIS["latitude",north],AXIS["longitude",east],UNIT["degree",0.0174532925199433]]"#
            ).into(),
            proj_string: Some("+proj=longlat +datum=WGS84 +no_defs".into()),
            is_geographic: true,
            is_projected: false,
            name: "WGS 84".into(),
            units: "degree".into(),
        }
    }

    /// Web Mercator / Pseudo-Mercator (EPSG:3857).
    pub fn web_mercator() -> Self {
        Self {
            epsg_code: Some(3857),
            wkt: concat!(
                r#"PROJCRS["WGS 84 / Pseudo-Mercator",BASEGEOGCRS["WGS 84"],"#,
                r#"CONVERSION["Popular Visualisation Pseudo-Mercator"]]"#
            ).into(),
            proj_string: Some(
                "+proj=merc +a=6378137 +b=6378137 +lat_ts=0 +lon_0=0 +x_0=0 +y_0=0 +k=1 +units=m +nadgrids=@null +no_defs"
                    .into(),
            ),
            is_geographic: false,
            is_projected: true,
            name: "WGS 84 / Pseudo-Mercator".into(),
            units: "metre".into(),
        }
    }

    /// Construct a minimal descriptor from an EPSG code.
    ///
    /// Only a small set of well-known codes are pre-populated; others receive
    /// a placeholder WKT.  For full WKT, use an external PROJ/WKT database.
    pub fn from_epsg(epsg: u32) -> Self {
        match epsg {
            4326 => Self::wgs84(),
            3857 => Self::web_mercator(),
            4269 => Self {
                epsg_code: Some(4269),
                wkt: "GEOGCRS[\"NAD83\",DATUM[\"North American Datum 1983\"]]".into(),
                proj_string: Some("+proj=longlat +ellps=GRS80 +datum=NAD83 +no_defs".into()),
                is_geographic: true,
                is_projected: false,
                name: "NAD83".into(),
                units: "degree".into(),
            },
            32601..=32660 => {
                let zone = epsg - 32600;
                Self {
                    epsg_code: Some(epsg),
                    wkt: format!("PROJCRS[\"WGS 84 / UTM zone {zone}N\"]"),
                    proj_string: Some(format!(
                        "+proj=utm +zone={zone} +datum=WGS84 +units=m +no_defs"
                    )),
                    is_geographic: false,
                    is_projected: true,
                    name: format!("WGS 84 / UTM zone {zone}N"),
                    units: "metre".into(),
                }
            }
            32701..=32760 => {
                let zone = epsg - 32700;
                Self {
                    epsg_code: Some(epsg),
                    wkt: format!("PROJCRS[\"WGS 84 / UTM zone {zone}S\"]"),
                    proj_string: Some(format!(
                        "+proj=utm +zone={zone} +south +datum=WGS84 +units=m +no_defs"
                    )),
                    is_geographic: false,
                    is_projected: true,
                    name: format!("WGS 84 / UTM zone {zone}S"),
                    units: "metre".into(),
                }
            }
            _ => Self {
                epsg_code: Some(epsg),
                wkt: format!("EPSG:{epsg}"),
                proj_string: None,
                is_geographic: false,
                is_projected: false,
                name: format!("EPSG:{epsg}"),
                units: "unknown".into(),
            },
        }
    }

    /// Returns `true` if both descriptors refer to the same EPSG authority code.
    pub fn same_crs(&self, other: &Self) -> bool {
        match (self.epsg_code, other.epsg_code) {
            (Some(a), Some(b)) => a == b,
            _ => self.wkt == other.wkt,
        }
    }
}

/// A geographic-to-projected transformation descriptor.
///
/// This is intentionally a pure-Rust, lookup-table-based approximation
/// suitable for WASM environments where PROJ is unavailable.  For precise
/// transformations call the `oxigdal-proj` crate on the native side and pass
/// the pre-projected data to WASM.
#[derive(Debug, Clone)]
pub struct ComponentTransform {
    /// Source CRS.
    pub source: ComponentProjection,
    /// Target CRS.
    pub target: ComponentProjection,
}

impl ComponentTransform {
    /// Create a transform descriptor (does not initialise any native library).
    pub fn new(source: ComponentProjection, target: ComponentProjection) -> Self {
        Self { source, target }
    }

    /// Apply a trivial identity transform (source == target).
    ///
    /// Returns an error if the CRS pair is not the same.
    pub fn transform_identity(
        &self,
        coords: &[ComponentCoord],
    ) -> ComponentResult<Vec<ComponentCoord>> {
        if !self.source.same_crs(&self.target) {
            return Err(ComponentError::unsupported(format!(
                "Non-identity transforms are not supported in the WASM component \
                 interface (source={}, target={}). Pre-project coordinates on the \
                 native side.",
                self.source.name, self.target.name
            )));
        }
        Ok(coords.to_vec())
    }

    /// Approximate WGS-84 → Web-Mercator transform (for small extents).
    ///
    /// Uses the standard spherical Mercator formula with WGS-84 semi-major axis.
    pub fn wgs84_to_web_mercator(
        coords: &[ComponentCoord],
    ) -> ComponentResult<Vec<ComponentCoord>> {
        const A: f64 = 6_378_137.0; // WGS-84 semi-major axis in metres
        const MAX_LAT: f64 = 85.051_129; // clip to valid Web Mercator range

        let mut out = Vec::with_capacity(coords.len());
        for c in coords {
            if c.x < -180.0 || c.x > 180.0 {
                return Err(ComponentError::invalid_input(format!(
                    "Longitude {} out of range [-180, 180]",
                    c.x
                )));
            }
            if c.y.abs() > MAX_LAT {
                return Err(ComponentError::invalid_input(format!(
                    "Latitude {} outside valid Web Mercator range ±{MAX_LAT}",
                    c.y
                )));
            }
            let x = A * c.x.to_radians();
            let y = A * ((std::f64::consts::PI / 4.0 + c.y.to_radians() / 2.0).tan()).ln();
            out.push(ComponentCoord::new(x, y));
        }
        Ok(out)
    }

    /// Approximate Web-Mercator → WGS-84 inverse.
    pub fn web_mercator_to_wgs84(
        coords: &[ComponentCoord],
    ) -> ComponentResult<Vec<ComponentCoord>> {
        const A: f64 = 6_378_137.0;
        let mut out = Vec::with_capacity(coords.len());
        for c in coords {
            let lon = c.x.to_degrees() / A;
            let lat = (2.0 * (c.y / A).exp().atan() - std::f64::consts::FRAC_PI_2).to_degrees();
            out.push(ComponentCoord::new(lon, lat));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wgs84_descriptor() {
        let p = ComponentProjection::wgs84();
        assert_eq!(p.epsg_code, Some(4326));
        assert!(p.is_geographic);
        assert!(!p.is_projected);
    }

    #[test]
    fn web_mercator_descriptor() {
        let p = ComponentProjection::web_mercator();
        assert_eq!(p.epsg_code, Some(3857));
        assert!(p.is_projected);
        assert!(!p.is_geographic);
    }

    #[test]
    fn from_epsg_utm() {
        let p = ComponentProjection::from_epsg(32632);
        assert_eq!(p.epsg_code, Some(32632));
        assert!(p.is_projected);
        assert!(p.name.contains("32N"));
    }

    #[test]
    fn same_crs_true() {
        let a = ComponentProjection::wgs84();
        let b = ComponentProjection::wgs84();
        assert!(a.same_crs(&b));
    }

    #[test]
    fn same_crs_false() {
        let a = ComponentProjection::wgs84();
        let b = ComponentProjection::web_mercator();
        assert!(!a.same_crs(&b));
    }

    #[test]
    fn coord_distance() {
        let a = ComponentCoord::new(0.0, 0.0);
        let b = ComponentCoord::new(3.0, 4.0);
        assert!((a.distance_to(&b) - 5.0).abs() < 1e-10);
    }

    #[test]
    fn coord_midpoint() {
        let a = ComponentCoord::new(0.0, 0.0);
        let b = ComponentCoord::new(10.0, 10.0);
        let m = a.midpoint(&b);
        assert_eq!(m, ComponentCoord::new(5.0, 5.0));
    }

    #[test]
    fn wgs84_to_web_mercator_origin() {
        let coords = vec![ComponentCoord::new(0.0, 0.0)];
        let out = ComponentTransform::wgs84_to_web_mercator(&coords).expect("transform");
        assert!((out[0].x).abs() < 1e-6);
        assert!((out[0].y).abs() < 1e-6);
    }

    #[test]
    fn web_mercator_roundtrip() {
        let input = ComponentCoord::new(13.405, 52.52); // Berlin approx
        let fwd = ComponentTransform::wgs84_to_web_mercator(std::slice::from_ref(&input))
            .expect("forward");
        let bwd = ComponentTransform::web_mercator_to_wgs84(&fwd).expect("backward");
        assert!((bwd[0].x - input.x).abs() < 1e-8);
        assert!((bwd[0].y - input.y).abs() < 1e-8);
    }

    #[test]
    fn longitude_out_of_range_error() {
        let coords = vec![ComponentCoord::new(200.0, 0.0)];
        assert!(ComponentTransform::wgs84_to_web_mercator(&coords).is_err());
    }
}
