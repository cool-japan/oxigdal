//! CRS Registry — comprehensive coordinate reference system lookup and metadata.
//!
//! Provides a pre-loaded registry of well-known CRS definitions (EPSG codes),
//! along with rich metadata: type, datum, unit, area of use, and PROJ strings.

#[cfg(feature = "std")]
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Type of coordinate reference system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CrsType {
    /// Geographic 2D (latitude/longitude).
    Geographic2D,
    /// Geographic 3D (latitude/longitude/ellipsoidal height).
    Geographic3D,
    /// Projected (easting/northing, metres or feet).
    Projected,
    /// Vertical (height or depth).
    Vertical,
    /// Compound (horizontal + vertical).
    Compound,
    /// Engineering (local coordinate system).
    Engineering,
    /// Geocentric (ECEF XYZ).
    Geocentric,
}

/// Unit of measurement for CRS axes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum CrsUnit {
    /// Decimal degrees.
    Degree,
    /// Radians.
    Radian,
    /// Metres (SI).
    Metre,
    /// US Survey Foot (1200/3937 m).
    Foot,
    /// International Foot (0.3048 m exactly).
    FootIntl,
    /// Kilometres.
    Kilometre,
    /// US Navy Foot (same as international foot).
    UsNavyFoot,
}

impl CrsUnit {
    /// Returns the conversion factor to metres.
    pub fn to_metres(&self) -> f64 {
        match self {
            CrsUnit::Degree => 111_319.490_793_274, // meters per degree at equator (approx)
            CrsUnit::Radian => 6_371_000.0,         // Earth radius in metres
            CrsUnit::Metre => 1.0,
            CrsUnit::Foot => 0.304_800_609_601_219, // US Survey Foot = 1200/3937
            CrsUnit::FootIntl => 0.3048,            // International Foot (exact)
            CrsUnit::Kilometre => 1_000.0,
            CrsUnit::UsNavyFoot => 0.3048,
        }
    }

    /// Returns the human-readable unit name.
    pub fn name(&self) -> &'static str {
        match self {
            CrsUnit::Degree => "degree",
            CrsUnit::Radian => "radian",
            CrsUnit::Metre => "metre",
            CrsUnit::Foot => "US survey foot",
            CrsUnit::FootIntl => "foot",
            CrsUnit::Kilometre => "kilometre",
            CrsUnit::UsNavyFoot => "US Navy foot",
        }
    }
}

/// Axis order convention for a CRS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AxisOrder {
    /// X = East, Y = North (most projected CRS).
    EastNorth,
    /// Y = North, X = East (geographic CRS per ISO 6709 / EPSG convention).
    NorthEast,
    /// Longitude first, latitude second (RFC 7946 GeoJSON convention).
    LonLat,
}

/// Geographic area of use for a CRS.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AreaOfUse {
    /// Human-readable description of the area.
    pub description: String,
    /// Bounding box: `[west, south, east, north]` in decimal degrees.
    pub bbox: [f64; 4],
}

impl AreaOfUse {
    /// Construct a new `AreaOfUse`.
    pub fn new(
        description: impl Into<String>,
        west: f64,
        south: f64,
        east: f64,
        north: f64,
    ) -> Self {
        Self {
            description: description.into(),
            bbox: [west, south, east, north],
        }
    }

    /// Return whether the given (lat, lon) falls within this area.
    pub fn contains(&self, lat: f64, lon: f64) -> bool {
        let [west, south, east, north] = self.bbox;
        lon >= west && lon <= east && lat >= south && lat <= north
    }
}

/// A complete definition of a coordinate reference system.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CrsDefinition {
    /// EPSG code, if applicable.
    pub epsg_code: Option<i32>,
    /// Human-readable name.
    pub name: String,
    /// CRS type.
    pub crs_type: CrsType,
    /// Datum name.
    pub datum: String,
    /// Axis unit.
    pub unit: CrsUnit,
    /// PROJ.4 / PROJ string representation.
    pub proj_string: Option<String>,
    /// WKT PROJCS or GEOGCS name (as it appears inside the WKT).
    pub wkt_name: Option<String>,
    /// Geographic area of use.
    pub area_of_use: Option<AreaOfUse>,
    /// Whether this definition is deprecated.
    pub deprecated: bool,
}

impl CrsDefinition {
    /// Returns `true` if the CRS is geographic (2D or 3D).
    pub fn is_geographic(&self) -> bool {
        matches!(self.crs_type, CrsType::Geographic2D | CrsType::Geographic3D)
    }

    /// Returns `true` if the CRS is projected.
    pub fn is_projected(&self) -> bool {
        matches!(self.crs_type, CrsType::Projected)
    }

    /// Returns the conventional axis order for this CRS.
    pub fn axis_order(&self) -> AxisOrder {
        match self.crs_type {
            CrsType::Geographic2D | CrsType::Geographic3D => AxisOrder::NorthEast,
            CrsType::Projected => AxisOrder::EastNorth,
            CrsType::Geocentric => AxisOrder::EastNorth,
            _ => AxisOrder::EastNorth,
        }
    }
}

/// A registry of coordinate reference system definitions, indexed by EPSG code.
#[cfg(feature = "std")]
pub struct CrsRegistry {
    definitions: HashMap<i32, CrsDefinition>,
}

#[cfg(feature = "std")]
impl CrsRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            definitions: HashMap::new(),
        }
    }

    /// Create a registry pre-loaded with ~140+ common CRS definitions.
    pub fn default_registry() -> Self {
        let mut reg = Self::new();
        reg.load_defaults();
        reg
    }

    /// Look up a CRS by EPSG code.
    pub fn get(&self, epsg_code: i32) -> Option<&CrsDefinition> {
        self.definitions.get(&epsg_code)
    }

    /// Register (insert or replace) a CRS definition.
    pub fn register(&mut self, def: CrsDefinition) {
        if let Some(code) = def.epsg_code {
            self.definitions.insert(code, def);
        }
    }

    /// Return the number of registered definitions.
    pub fn count(&self) -> usize {
        self.definitions.len()
    }

    /// Find all CRS whose name contains `name` (case-insensitive partial match).
    pub fn find_by_name(&self, name: &str) -> Vec<&CrsDefinition> {
        let query = name.to_lowercase();
        self.definitions
            .values()
            .filter(|def| def.name.to_lowercase().contains(&query))
            .collect()
    }

    /// Return all CRS of a given type.
    pub fn by_type(&self, crs_type: &CrsType) -> Vec<&CrsDefinition> {
        self.definitions
            .values()
            .filter(|def| &def.crs_type == crs_type)
            .collect()
    }

    /// Return all CRS whose area of use covers the given (lat, lon) point.
    pub fn covering_point(&self, lat: f64, lon: f64) -> Vec<&CrsDefinition> {
        self.definitions
            .values()
            .filter(|def| {
                def.area_of_use
                    .as_ref()
                    .map(|a| a.contains(lat, lon))
                    .unwrap_or(false)
            })
            .collect()
    }

    // -------------------------------------------------------------------------
    // Private loader
    // -------------------------------------------------------------------------

    fn insert(&mut self, def: CrsDefinition) {
        if let Some(code) = def.epsg_code {
            self.definitions.insert(code, def);
        }
    }

    fn geo2d(code: i32, name: &str, datum: &str, proj: &str, area: AreaOfUse) -> CrsDefinition {
        CrsDefinition {
            epsg_code: Some(code),
            name: name.to_string(),
            crs_type: CrsType::Geographic2D,
            datum: datum.to_string(),
            unit: CrsUnit::Degree,
            proj_string: Some(proj.to_string()),
            wkt_name: Some(name.to_string()),
            area_of_use: Some(area),
            deprecated: false,
        }
    }

    fn proj_m(code: i32, name: &str, datum: &str, proj: &str, area: AreaOfUse) -> CrsDefinition {
        CrsDefinition {
            epsg_code: Some(code),
            name: name.to_string(),
            crs_type: CrsType::Projected,
            datum: datum.to_string(),
            unit: CrsUnit::Metre,
            proj_string: Some(proj.to_string()),
            wkt_name: Some(name.to_string()),
            area_of_use: Some(area),
            deprecated: false,
        }
    }

    fn load_defaults(&mut self) {
        // ------------------------------------------------------------------
        // Geographic CRS
        // ------------------------------------------------------------------
        self.insert(Self::geo2d(
            4326,
            "WGS 84",
            "WGS_1984",
            "+proj=longlat +datum=WGS84 +no_defs",
            AreaOfUse::new("World", -180.0, -90.0, 180.0, 90.0),
        ));
        self.insert(Self::geo2d(
            4258,
            "ETRS89",
            "European_Terrestrial_Reference_System_1989",
            "+proj=longlat +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +no_defs",
            AreaOfUse::new("Europe", -16.1, 32.88, 40.18, 84.17),
        ));
        self.insert(Self::geo2d(
            4269,
            "NAD83",
            "North_American_Datum_1983",
            "+proj=longlat +datum=NAD83 +no_defs",
            AreaOfUse::new("North America", -172.54, 23.81, -47.74, 86.46),
        ));
        self.insert(Self::geo2d(
            4267,
            "NAD27",
            "North_American_Datum_1927",
            "+proj=longlat +datum=NAD27 +no_defs",
            AreaOfUse::new("North America", -172.54, 23.81, -47.74, 86.46),
        ));
        self.insert(Self::geo2d(
            4230,
            "ED50",
            "European_Datum_1950",
            "+proj=longlat +ellps=intl +towgs84=-87,-98,-121,0,0,0,0 +no_defs",
            AreaOfUse::new("Europe", -16.1, 34.88, 48.0, 84.17),
        ));
        self.insert(Self::geo2d(
            4283,
            "GDA94",
            "Geocentric_Datum_of_Australia_1994",
            "+proj=longlat +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +no_defs",
            AreaOfUse::new("Australia", 112.85, -43.7, 153.69, -9.86),
        ));
        self.insert(Self::geo2d(
            7844,
            "GDA2020",
            "GDA2020",
            "+proj=longlat +ellps=GRS80 +no_defs",
            AreaOfUse::new("Australia", 112.85, -43.7, 153.69, -9.86),
        ));
        self.insert(Self::geo2d(
            6326,
            "WGS 84 (G1762)",
            "WGS_1984",
            "+proj=longlat +datum=WGS84 +no_defs",
            AreaOfUse::new("World", -180.0, -90.0, 180.0, 90.0),
        ));

        // ------------------------------------------------------------------
        // Projected CRS (non-UTM)
        // ------------------------------------------------------------------
        self.insert(Self::proj_m(
            3857,
            "WGS 84 / Pseudo-Mercator",
            "WGS_1984",
            "+proj=merc +a=6378137 +b=6378137 +lat_ts=0 +lon_0=0 +x_0=0 +y_0=0 +k=1 +units=m +nadgrids=@null +no_defs",
            AreaOfUse::new("World", -180.0, -85.06, 180.0, 85.06),
        ));
        self.insert(Self::proj_m(
            27700,
            "OSGB36 / British National Grid",
            "OSGB_1936",
            "+proj=tmerc +lat_0=49 +lon_0=-2 +k=0.9996012717 +x_0=400000 +y_0=-100000 +ellps=airy +towgs84=446.448,-125.157,542.06,0.15,0.247,0.842,-20.489 +units=m +no_defs",
            AreaOfUse::new("United Kingdom", -8.82, 49.79, 1.92, 60.94),
        ));
        self.insert(Self::proj_m(
            2154,
            "RGF93 / Lambert-93",
            "Reseau_Geodesique_Francais_1993",
            "+proj=lcc +lat_0=46.5 +lon_0=3 +lat_1=49 +lat_2=44 +x_0=700000 +y_0=6600000 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs",
            AreaOfUse::new("France", -9.86, 41.15, 10.38, 51.56),
        ));
        self.insert(Self::proj_m(
            25832,
            "ETRS89 / UTM zone 32N",
            "European_Terrestrial_Reference_System_1989",
            "+proj=utm +zone=32 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs",
            AreaOfUse::new("Germany (UTM 32N)", 6.0, 47.27, 12.01, 57.9),
        ));
        self.insert(Self::proj_m(
            25833,
            "ETRS89 / UTM zone 33N",
            "European_Terrestrial_Reference_System_1989",
            "+proj=utm +zone=33 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs",
            AreaOfUse::new("Central Europe (UTM 33N)", 12.0, 47.27, 18.01, 57.9),
        ));
        self.insert(Self::proj_m(
            31467,
            "DHDN / Gauss-Kruger zone 3",
            "Deutsches_Hauptdreiecksnetz",
            "+proj=tmerc +lat_0=0 +lon_0=9 +k=1 +x_0=3500000 +y_0=0 +ellps=bessel +towgs84=598.1,73.7,418.2,0.202,0.045,-2.455,6.7 +units=m +no_defs",
            AreaOfUse::new("Germany", 7.5, 47.27, 10.5, 55.06),
        ));
        self.insert(Self::proj_m(
            5514,
            "S-JTSK / Krovak East North",
            "System_Jednotne_Trigonometricke_Site_Katastralni",
            "+proj=krovak +lat_0=49.5 +lon_0=24.8333333333333 +alpha=30.2881397527778 +k=0.9999 +x_0=0 +y_0=0 +ellps=bessel +towgs84=589,76,480,0,0,0,0 +units=m +no_defs",
            AreaOfUse::new("Czech Republic and Slovakia", 12.09, 47.73, 22.56, 51.06),
        ));
        self.insert(Self::proj_m(
            29902,
            "TM65 / Irish National Grid",
            "TM65",
            "+proj=tmerc +lat_0=53.5 +lon_0=-8 +k=1.000035 +x_0=200000 +y_0=250000 +a=6377340.189 +b=6356034.447938534 +towgs84=482.5,-130.6,564.6,-1.042,-0.214,-0.631,8.15 +units=m +no_defs",
            AreaOfUse::new("Ireland", -10.56, 51.39, -5.34, 55.43),
        ));
        self.insert(Self::proj_m(
            3006,
            "SWEREF99 TM",
            "SWEREF99",
            "+proj=utm +zone=33 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs",
            AreaOfUse::new("Sweden", 10.03, 54.96, 24.17, 69.07),
        ));
        self.insert(Self::proj_m(
            3035,
            "ETRS89-extended / LAEA Europe",
            "European_Terrestrial_Reference_System_1989",
            "+proj=laea +lat_0=52 +lon_0=10 +x_0=4321000 +y_0=3210000 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs",
            AreaOfUse::new("Europe", -35.58, 24.6, 44.83, 84.17),
        ));

        // ------------------------------------------------------------------
        // WGS84 UTM North zones 1–60 (EPSG:32601–32660)
        // ------------------------------------------------------------------
        for zone in 1u8..=60u8 {
            let code = 32600 + i32::from(zone);
            let name = format!("WGS 84 / UTM zone {}N", zone);
            let west = f64::from(zone - 1) * 6.0 - 180.0;
            let east = west + 6.0;
            let proj = format!("+proj=utm +zone={} +datum=WGS84 +units=m +no_defs", zone);
            self.insert(CrsDefinition {
                epsg_code: Some(code),
                name: name.clone(),
                crs_type: CrsType::Projected,
                datum: "WGS_1984".to_string(),
                unit: CrsUnit::Metre,
                proj_string: Some(proj),
                wkt_name: Some(name),
                area_of_use: Some(AreaOfUse::new(
                    format!("WGS84 UTM zone {}N", zone),
                    west,
                    0.0,
                    east,
                    84.0,
                )),
                deprecated: false,
            });
        }

        // ------------------------------------------------------------------
        // WGS84 UTM South zones 1–60 (EPSG:32701–32760)
        // ------------------------------------------------------------------
        for zone in 1u8..=60u8 {
            let code = 32700 + i32::from(zone);
            let name = format!("WGS 84 / UTM zone {}S", zone);
            let west = f64::from(zone - 1) * 6.0 - 180.0;
            let east = west + 6.0;
            let proj = format!(
                "+proj=utm +zone={} +south +datum=WGS84 +units=m +no_defs",
                zone
            );
            self.insert(CrsDefinition {
                epsg_code: Some(code),
                name: name.clone(),
                crs_type: CrsType::Projected,
                datum: "WGS_1984".to_string(),
                unit: CrsUnit::Metre,
                proj_string: Some(proj),
                wkt_name: Some(name),
                area_of_use: Some(AreaOfUse::new(
                    format!("WGS84 UTM zone {}S", zone),
                    west,
                    -80.0,
                    east,
                    0.0,
                )),
                deprecated: false,
            });
        }
    }
}

#[cfg(feature = "std")]
impl Default for CrsRegistry {
    fn default() -> Self {
        Self::default_registry()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_has_wgs84() {
        let reg = CrsRegistry::default_registry();
        let def = reg.get(4326).expect("EPSG:4326 must exist");
        assert!(def.name.contains("WGS 84"));
    }

    #[test]
    fn test_crs_unit_metre() {
        assert!((CrsUnit::Metre.to_metres() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_crs_unit_foot() {
        let f = CrsUnit::Foot.to_metres();
        assert!((f - 0.3048).abs() < 0.001);
    }

    #[test]
    fn test_registry_count() {
        let reg = CrsRegistry::default_registry();
        assert!(reg.count() >= 50);
    }
}
