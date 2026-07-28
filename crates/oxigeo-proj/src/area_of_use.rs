//! Area-of-use validation for CRS definitions.
//!
//! This module provides geographic bounds that define the valid area of use
//! for coordinate reference systems identified by EPSG codes. It enables
//! validation of coordinates against the defined bounds of their CRS.

use alloc::format;
use alloc::string::String;

/// Geographic bounds defining the valid area of use for a CRS.
/// All values in decimal degrees (WGS84).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AreaOfUse {
    /// Western longitude bound (degrees)
    pub west: f64,
    /// Southern latitude bound (degrees)
    pub south: f64,
    /// Eastern longitude bound (degrees)
    pub east: f64,
    /// Northern latitude bound (degrees)
    pub north: f64,
    /// Human-readable area description
    pub name: &'static str,
}

impl AreaOfUse {
    /// Creates a new `AreaOfUse` with the given bounds and description.
    pub const fn new(west: f64, south: f64, east: f64, north: f64, name: &'static str) -> Self {
        Self {
            west,
            south,
            east,
            north,
            name,
        }
    }

    /// Check if a point (lon_deg, lat_deg) falls within this area of use.
    ///
    /// Handles antimeridian crossing where `west > east` (the region wraps the
    /// antimeridian, i.e. it occupies `[west, 180] ∪ [-180, east]`).
    pub fn contains(&self, lon_deg: f64, lat_deg: f64) -> bool {
        let lat_ok = lat_deg >= self.south && lat_deg <= self.north;
        if !lat_ok {
            return false;
        }
        if self.west <= self.east {
            lon_deg >= self.west && lon_deg <= self.east
        } else {
            // Antimeridian crossing: lon ∈ [west, 180] ∪ [-180, east]
            lon_deg >= self.west || lon_deg <= self.east
        }
    }

    /// Check if a point is within this area with a tolerance margin (degrees).
    pub fn contains_with_tolerance(&self, lon_deg: f64, lat_deg: f64, tolerance_deg: f64) -> bool {
        lon_deg >= self.west - tolerance_deg
            && lon_deg <= self.east + tolerance_deg
            && lat_deg >= self.south - tolerance_deg
            && lat_deg <= self.north + tolerance_deg
    }

    /// Validate that a coordinate pair falls within the area of use.
    /// Returns `Ok(())` if valid, `Err` with a descriptive message if outside bounds.
    pub fn validate(&self, lon_deg: f64, lat_deg: f64) -> core::result::Result<(), String> {
        if self.contains(lon_deg, lat_deg) {
            Ok(())
        } else {
            Err(format!(
                "Coordinate ({}, {}) is outside the area of use '{}' \
                 (bounds: west={}, south={}, east={}, north={})",
                lon_deg, lat_deg, self.name, self.west, self.south, self.east, self.north
            ))
        }
    }
}

/// Compute the area of use for a UTM North zone (EPSG:32601–32660).
const fn utm_north_area(zone: u32) -> AreaOfUse {
    let central_meridian = (zone as i32 - 1) * 6 - 177;
    let west = central_meridian - 3;
    let east = central_meridian + 3;
    AreaOfUse::new(west as f64, 0.0, east as f64, 84.0, "UTM North zone")
}

/// Compute the area of use for a UTM South zone (EPSG:32701–32760).
const fn utm_south_area(zone: u32) -> AreaOfUse {
    let central_meridian = (zone as i32 - 1) * 6 - 177;
    let west = central_meridian - 3;
    let east = central_meridian + 3;
    AreaOfUse::new(west as f64, -80.0, east as f64, 0.0, "UTM South zone")
}

/// Look up the area of use for a given EPSG code.
/// Returns `None` if the EPSG code is not recognized or has no defined bounds.
pub fn area_of_use_for_epsg(code: u32) -> Option<AreaOfUse> {
    match code {
        // WGS84 geographic
        4326 => Some(AreaOfUse::new(-180.0, -90.0, 180.0, 90.0, "World")),
        // Web Mercator
        3857 => Some(AreaOfUse::new(
            -180.0,
            -85.06,
            180.0,
            85.06,
            "World between 85.06°S and 85.06°N",
        )),
        // NAD83 geographic
        4269 => Some(AreaOfUse::new(
            -172.54,
            14.92,
            -47.74,
            86.46,
            "North America",
        )),
        // ETRS89 geographic
        4258 => Some(AreaOfUse::new(-16.1, 32.88, 40.18, 84.73, "Europe")),
        // GDA94 geographic
        4283 => Some(AreaOfUse::new(93.41, -60.55, 173.34, -8.47, "Australia")),
        // GDA2020 geographic
        7844 => Some(AreaOfUse::new(93.41, -60.55, 173.34, -8.47, "Australia")),
        // JGD2000 geographic
        4612 => Some(AreaOfUse::new(122.38, 17.09, 157.65, 46.05, "Japan")),
        // JGD2011 geographic
        6668 => Some(AreaOfUse::new(122.38, 17.09, 157.65, 46.05, "Japan")),
        // NZGD2000 geographic
        4167 => Some(AreaOfUse::new(160.6, -55.95, -171.2, -25.88, "New Zealand")),
        // RGF93 / Lambert-93 (France)
        2154 => Some(AreaOfUse::new(-9.86, 41.15, 10.38, 51.56, "France")),
        // OSGB 1936 / British National Grid
        27700 => Some(AreaOfUse::new(-7.56, 49.96, 1.78, 60.84, "United Kingdom")),
        // ETRS89-extended / LAEA Europe
        3035 => Some(AreaOfUse::new(-35.58, 24.6, 44.83, 84.73, "Europe")),
        // NZGD2000 / New Zealand Transverse Mercator
        2193 => Some(AreaOfUse::new(166.37, -47.33, 178.63, -34.1, "New Zealand")),
        // NAD83 / UTM zone 10N (US West Coast)
        26910 => Some(AreaOfUse::new(
            -126.0,
            0.0,
            -120.0,
            84.0,
            "UTM zone 10N (NAD83)",
        )),
        // NAD83 / UTM zone 17N (US East)
        26917 => Some(AreaOfUse::new(
            -84.0,
            0.0,
            -78.0,
            84.0,
            "UTM zone 17N (NAD83)",
        )),
        // NAD83 / California zone 5 (ftUS)
        2229 => Some(AreaOfUse::new(
            -121.42,
            32.76,
            -114.12,
            35.81,
            "USA - California zone 5",
        )),
        // NAD83 / Texas Central
        2277 => Some(AreaOfUse::new(
            -105.73,
            29.78,
            -93.51,
            32.27,
            "USA - Texas Central",
        )),
        // China Geodetic Coordinate System 2000
        4490 => Some(AreaOfUse::new(73.62, 16.7, 134.77, 53.56, "China")),
        // Korean 1985 / Modified Central
        5174 => Some(AreaOfUse::new(124.53, 33.14, 131.01, 38.45, "South Korea")),
        // Indian 1975 / UTM zone 47N
        24047 => Some(AreaOfUse::new(96.0, 0.0, 102.0, 84.0, "UTM zone 47N")),
        // Pulkovo 1942 / Gauss-Kruger zone 6
        28406 => Some(AreaOfUse::new(30.0, 40.0, 36.0, 80.0, "Russia zone 6")),
        // SWEREF99 TM
        3006 => Some(AreaOfUse::new(10.03, 54.96, 24.17, 69.07, "Sweden")),
        // IRENET95 / Irish Transverse Mercator
        2157 => Some(AreaOfUse::new(-10.56, 51.39, -5.34, 55.43, "Ireland")),
        // HD72 / EOV (Hungary)
        23700 => Some(AreaOfUse::new(16.11, 45.74, 22.9, 48.58, "Hungary")),
        // DHDN / 3-degree Gauss-Kruger zone 3
        31467 => Some(AreaOfUse::new(7.5, 47.27, 10.51, 55.06, "Germany zone 3")),
        // WGS84 / UTM North zones 1-60
        32601..=32660 => {
            let zone = code - 32600;
            Some(utm_north_area(zone))
        }
        // WGS84 / UTM South zones 1-60
        32701..=32760 => {
            let zone = code - 32700;
            Some(utm_south_area(zone))
        }
        _ => None,
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_wgs84_contains_world() {
        let area = area_of_use_for_epsg(4326).expect("WGS84 should be defined");
        assert!(area.contains(0.0, 0.0));
        assert!(area.contains(180.0, 90.0));
        assert!(area.contains(-180.0, -90.0));
    }

    #[test]
    fn test_web_mercator_excludes_poles() {
        let area = area_of_use_for_epsg(3857).expect("Web Mercator should be defined");
        assert!(!area.contains(0.0, 86.0));
        assert!(!area.contains(0.0, -86.0));
        assert!(area.contains(0.0, 50.0));
    }

    #[test]
    fn test_utm_zone_33n() {
        let area = area_of_use_for_epsg(32633).expect("UTM 33N should be defined");
        // Zone 33N: central meridian = (33-1)*6 - 177 = 15, bounds [12, 0, 18, 84]
        assert!(area.contains(15.0, 45.0));
        assert!(!area.contains(0.0, 45.0));
    }

    #[test]
    fn test_utm_zone_1s() {
        let area = area_of_use_for_epsg(32701).expect("UTM 1S should be defined");
        // Zone 1S: central meridian = (1-1)*6 - 177 = -177, bounds [-180, -80, -174, 0]
        assert!(area.contains(-177.0, -45.0));
        assert!(!area.contains(-177.0, 45.0));
    }

    #[test]
    fn test_british_national_grid() {
        let area = area_of_use_for_epsg(27700).expect("BNG should be defined");
        // London area
        assert!(area.contains(-1.0, 51.5));
        // Outside UK
        assert!(!area.contains(10.0, 51.5));
    }

    #[test]
    fn test_france_lambert93() {
        let area = area_of_use_for_epsg(2154).expect("Lambert-93 should be defined");
        // Paris
        assert!(area.contains(2.35, 48.86));
        // Eastern Europe
        assert!(!area.contains(20.0, 48.0));
    }

    #[test]
    fn test_unknown_epsg_returns_none() {
        assert!(area_of_use_for_epsg(99999).is_none());
    }

    #[test]
    fn test_contains_with_tolerance() {
        let area = area_of_use_for_epsg(27700).expect("BNG should be defined");
        // Point slightly outside the western bound (-7.56)
        assert!(!area.contains(-8.0, 55.0));
        assert!(area.contains_with_tolerance(-8.0, 55.0, 1.0));
    }

    #[test]
    fn test_validate_ok() {
        let area = area_of_use_for_epsg(4326).expect("WGS84 should be defined");
        assert!(area.validate(0.0, 0.0).is_ok());
    }

    #[test]
    fn test_validate_error() {
        let area = area_of_use_for_epsg(3857).expect("Web Mercator should be defined");
        let result = area.validate(0.0, 90.0);
        assert!(result.is_err());
        let msg = result.expect_err("should be error");
        assert!(msg.contains("outside the area of use"));
    }

    #[test]
    fn test_nz_bounds() {
        let area = area_of_use_for_epsg(2193).expect("NZTM should be defined");
        // Wellington
        assert!(area.contains(174.77, -41.29));
    }

    #[test]
    fn test_japan_bounds() {
        let area = area_of_use_for_epsg(6668).expect("JGD2011 should be defined");
        // Tokyo
        assert!(area.contains(139.69, 35.69));
    }

    #[test]
    fn test_utm_north_all_zones_valid() {
        for zone in 1..=60 {
            let code = 32600 + zone;
            let area = area_of_use_for_epsg(code).expect("UTM North zone should be defined");
            let cm = (zone as i32 - 1) * 6 - 177;
            assert!(area.contains(cm as f64, 42.0));
            assert_eq!(area.south, 0.0);
            assert_eq!(area.north, 84.0);
        }
    }

    #[test]
    fn test_utm_south_all_zones_valid() {
        for zone in 1..=60 {
            let code = 32700 + zone;
            let area = area_of_use_for_epsg(code).expect("UTM South zone should be defined");
            let cm = (zone as i32 - 1) * 6 - 177;
            assert!(area.contains(cm as f64, -42.0));
            assert_eq!(area.south, -80.0);
            assert_eq!(area.north, 0.0);
        }
    }

    #[test]
    fn test_area_of_use_new() {
        let area = AreaOfUse::new(-10.0, -20.0, 30.0, 40.0, "Test area");
        assert_eq!(area.west, -10.0);
        assert_eq!(area.south, -20.0);
        assert_eq!(area.east, 30.0);
        assert_eq!(area.north, 40.0);
        assert_eq!(area.name, "Test area");
    }
}
