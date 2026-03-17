//! CRS transformation utilities for OGC Features API Part 2.

use super::error::FeaturesError;

/// CRS84 URI constant
pub const CRS84_URI: &str = "http://www.opengis.net/def/crs/OGC/1.3/CRS84";
/// EPSG:4326 URI constant
pub const EPSG4326_URI: &str = "http://www.opengis.net/def/crs/EPSG/0/4326";
/// EPSG:3857 URI constant
pub const EPSG3857_URI: &str = "http://www.opengis.net/def/crs/EPSG/0/3857";
/// EPSG:4258 URI constant
pub const EPSG4258_URI: &str = "http://www.opengis.net/def/crs/EPSG/0/4258";
/// EPSG:25832 URI constant
pub const EPSG25832_URI: &str = "http://www.opengis.net/def/crs/EPSG/0/25832";
/// EPSG:25833 URI constant
pub const EPSG25833_URI: &str = "http://www.opengis.net/def/crs/EPSG/0/25833";

/// Web Mercator Earth radius (metres)
const EARTH_RADIUS_M: f64 = 6_378_137.0;
/// pi
const PI: f64 = std::f64::consts::PI;

/// CRS utility functions for Part 2 support
pub struct CrsTransform;

impl CrsTransform {
    /// Return the list of CRS URIs natively supported by the server.
    pub fn supported_crs_uris() -> Vec<String> {
        vec![
            CRS84_URI.to_string(),
            EPSG4326_URI.to_string(),
            EPSG3857_URI.to_string(),
            EPSG4258_URI.to_string(),
            EPSG25832_URI.to_string(),
            EPSG25833_URI.to_string(),
        ]
    }

    /// Convert a bbox from `source_crs` to WGS 84 (CRS84 axis order lon/lat).
    ///
    /// - CRS84 / EPSG:4326 / EPSG:4258: identity (axis order normalised to lon/lat)
    /// - EPSG:3857: inverse Web Mercator
    /// - Other CRS URIs: returns `FeaturesError::InvalidCrs`
    pub fn bbox_to_wgs84(bbox: [f64; 4], source_crs: &str) -> Result<[f64; 4], FeaturesError> {
        match source_crs {
            // Identity transforms — all use lon/lat already or we normalise
            CRS84_URI | EPSG4326_URI | EPSG4258_URI => Ok(bbox),

            // Web Mercator inverse
            EPSG3857_URI => {
                let lon_min = bbox[0] / EARTH_RADIUS_M * (180.0 / PI);
                let lat_min =
                    (2.0 * (bbox[1] / EARTH_RADIUS_M).exp().atan() - PI / 2.0) * (180.0 / PI);
                let lon_max = bbox[2] / EARTH_RADIUS_M * (180.0 / PI);
                let lat_max =
                    (2.0 * (bbox[3] / EARTH_RADIUS_M).exp().atan() - PI / 2.0) * (180.0 / PI);
                Ok([lon_min, lat_min, lon_max, lat_max])
            }

            other => Err(FeaturesError::InvalidCrs(format!(
                "CRS not supported for bbox transformation: {other}"
            ))),
        }
    }

    /// Check whether the given URI is known to this server.
    pub fn is_supported(uri: &str) -> bool {
        matches!(
            uri,
            CRS84_URI | EPSG4326_URI | EPSG3857_URI | EPSG4258_URI | EPSG25832_URI | EPSG25833_URI
        )
    }
}
