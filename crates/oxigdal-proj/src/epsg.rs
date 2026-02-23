//! EPSG code database with common coordinate reference systems.
//!
//! This module provides a built-in database of common EPSG codes used in geospatial applications.
//! The database includes approximately 100 of the most commonly used coordinate reference systems,
//! including WGS84, Web Mercator, UTM zones, and common national grids.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// EPSG code definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EpsgDefinition {
    /// EPSG code
    pub code: u32,
    /// Human-readable name
    pub name: String,
    /// PROJ string representation
    pub proj_string: String,
    /// WKT (Well-Known Text) representation
    pub wkt: Option<String>,
    /// CRS type (geographic, projected, etc.)
    pub crs_type: CrsType,
    /// Area of use
    pub area_of_use: String,
    /// Unit of measurement
    pub unit: String,
    /// Datum name
    pub datum: String,
}

/// Type of coordinate reference system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CrsType {
    /// Geographic CRS (latitude/longitude)
    Geographic,
    /// Projected CRS (planar coordinates)
    Projected,
    /// Geocentric CRS (3D Cartesian)
    Geocentric,
    /// Vertical CRS (heights/depths)
    Vertical,
    /// Compound CRS (combination of horizontal + vertical)
    Compound,
    /// Engineering CRS (local coordinate systems)
    Engineering,
}

/// EPSG database containing common coordinate reference systems.
pub struct EpsgDatabase {
    definitions: HashMap<u32, EpsgDefinition>,
}

impl EpsgDatabase {
    /// Creates a new EPSG database with built-in definitions.
    pub fn new() -> Self {
        let mut db = Self {
            definitions: HashMap::new(),
        };
        db.initialize_builtin_codes();
        db
    }

    /// Looks up an EPSG code in the database.
    pub fn lookup(&self, code: u32) -> Result<&EpsgDefinition> {
        self.definitions
            .get(&code)
            .ok_or_else(|| Error::epsg_not_found(code))
    }

    /// Checks if an EPSG code exists in the database.
    pub fn contains(&self, code: u32) -> bool {
        self.definitions.contains_key(&code)
    }

    /// Returns all available EPSG codes.
    pub fn codes(&self) -> Vec<u32> {
        let mut codes: Vec<u32> = self.definitions.keys().copied().collect();
        codes.sort_unstable();
        codes
    }

    /// Returns the number of EPSG codes in the database.
    pub fn len(&self) -> usize {
        self.definitions.len()
    }

    /// Checks if the database is empty.
    pub fn is_empty(&self) -> bool {
        self.definitions.is_empty()
    }

    /// Adds a custom EPSG definition to the database.
    pub fn add_definition(&mut self, definition: EpsgDefinition) {
        self.definitions.insert(definition.code, definition);
    }

    /// Initializes the database with built-in EPSG codes.
    fn initialize_builtin_codes(&mut self) {
        // WGS84 - Most common geographic CRS
        self.add_definition(EpsgDefinition {
            code: 4326,
            name: "WGS 84".to_string(),
            proj_string: "+proj=longlat +datum=WGS84 +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Geographic,
            area_of_use: "World".to_string(),
            unit: "degree".to_string(),
            datum: "WGS84".to_string(),
        });

        // Web Mercator - Most common projected CRS for web maps
        self.add_definition(EpsgDefinition {
            code: 3857,
            name: "WGS 84 / Pseudo-Mercator".to_string(),
            proj_string: "+proj=merc +a=6378137 +b=6378137 +lat_ts=0 +lon_0=0 +x_0=0 +y_0=0 +k=1 +units=m +nadgrids=@null +wktext +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "World between 85.06°S and 85.06°N".to_string(),
            unit: "metre".to_string(),
            datum: "WGS84".to_string(),
        });

        // WGS 84 / World Mercator
        self.add_definition(EpsgDefinition {
            code: 3395,
            name: "WGS 84 / World Mercator".to_string(),
            proj_string: "+proj=merc +lon_0=0 +k=1 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs"
                .to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "World between 80°S and 84°N".to_string(),
            unit: "metre".to_string(),
            datum: "WGS84".to_string(),
        });

        // NAD83 - North American Datum 1983
        self.add_definition(EpsgDefinition {
            code: 4269,
            name: "NAD83".to_string(),
            proj_string: "+proj=longlat +datum=NAD83 +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Geographic,
            area_of_use: "North America".to_string(),
            unit: "degree".to_string(),
            datum: "NAD83".to_string(),
        });

        // ETRS89 - European Terrestrial Reference System 1989
        self.add_definition(EpsgDefinition {
            code: 4258,
            name: "ETRS89".to_string(),
            proj_string: "+proj=longlat +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Geographic,
            area_of_use: "Europe".to_string(),
            unit: "degree".to_string(),
            datum: "ETRS89".to_string(),
        });

        // UTM Zones (Zone 1N to 60N for WGS84)
        for zone in 1..=60 {
            let code = 32600 + zone;
            let central_meridian = -183 + (zone as i32 * 6);
            self.add_definition(EpsgDefinition {
                code,
                name: format!("WGS 84 / UTM zone {}N", zone),
                proj_string: format!("+proj=utm +zone={} +datum=WGS84 +units=m +no_defs", zone),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: format!(
                    "Between {}°E and {}°E, northern hemisphere",
                    central_meridian - 3,
                    central_meridian + 3
                ),
                unit: "metre".to_string(),
                datum: "WGS84".to_string(),
            });
        }

        // UTM Zones (Zone 1S to 60S for WGS84)
        for zone in 1..=60 {
            let code = 32700 + zone;
            let central_meridian = -183 + (zone as i32 * 6);
            self.add_definition(EpsgDefinition {
                code,
                name: format!("WGS 84 / UTM zone {}S", zone),
                proj_string: format!(
                    "+proj=utm +zone={} +south +datum=WGS84 +units=m +no_defs",
                    zone
                ),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: format!(
                    "Between {}°E and {}°E, southern hemisphere",
                    central_meridian - 3,
                    central_meridian + 3
                ),
                unit: "metre".to_string(),
                datum: "WGS84".to_string(),
            });
        }

        // Common national/regional systems

        // NAD83 / UTM zone 10N (US West Coast)
        self.add_definition(EpsgDefinition {
            code: 26910,
            name: "NAD83 / UTM zone 10N".to_string(),
            proj_string: "+proj=utm +zone=10 +datum=NAD83 +units=m +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "North America - 126°W to 120°W".to_string(),
            unit: "metre".to_string(),
            datum: "NAD83".to_string(),
        });

        // OSGB 1936 / British National Grid
        self.add_definition(EpsgDefinition {
            code: 27700,
            name: "OSGB 1936 / British National Grid".to_string(),
            proj_string: "+proj=tmerc +lat_0=49 +lon_0=-2 +k=0.9996012717 +x_0=400000 +y_0=-100000 +ellps=airy +datum=OSGB36 +units=m +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "United Kingdom".to_string(),
            unit: "metre".to_string(),
            datum: "OSGB36".to_string(),
        });

        // ETRS89 / UTM zone 32N (Central Europe)
        self.add_definition(EpsgDefinition {
            code: 25832,
            name: "ETRS89 / UTM zone 32N".to_string(),
            proj_string: "+proj=utm +zone=32 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs"
                .to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "Europe - 6°E to 12°E".to_string(),
            unit: "metre".to_string(),
            datum: "ETRS89".to_string(),
        });

        // GDA94 - Geocentric Datum of Australia 1994
        self.add_definition(EpsgDefinition {
            code: 4283,
            name: "GDA94".to_string(),
            proj_string: "+proj=longlat +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Geographic,
            area_of_use: "Australia".to_string(),
            unit: "degree".to_string(),
            datum: "GDA94".to_string(),
        });

        // JGD2000 - Japanese Geodetic Datum 2000
        self.add_definition(EpsgDefinition {
            code: 4612,
            name: "JGD2000".to_string(),
            proj_string: "+proj=longlat +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Geographic,
            area_of_use: "Japan".to_string(),
            unit: "degree".to_string(),
            datum: "JGD2000".to_string(),
        });

        // NZGD2000 - New Zealand Geodetic Datum 2000
        self.add_definition(EpsgDefinition {
            code: 4167,
            name: "NZGD2000".to_string(),
            proj_string: "+proj=longlat +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Geographic,
            area_of_use: "New Zealand".to_string(),
            unit: "degree".to_string(),
            datum: "NZGD2000".to_string(),
        });

        // SIRGAS 2000 - Sistema de Referencia Geocentrico para las Americas
        self.add_definition(EpsgDefinition {
            code: 4674,
            name: "SIRGAS 2000".to_string(),
            proj_string: "+proj=longlat +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Geographic,
            area_of_use: "Latin America".to_string(),
            unit: "degree".to_string(),
            datum: "SIRGAS2000".to_string(),
        });

        // China Geodetic Coordinate System 2000
        self.add_definition(EpsgDefinition {
            code: 4490,
            name: "China Geodetic Coordinate System 2000".to_string(),
            proj_string: "+proj=longlat +ellps=GRS80 +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Geographic,
            area_of_use: "China".to_string(),
            unit: "degree".to_string(),
            datum: "CGCS2000".to_string(),
        });

        // WGS 72 (older version, still used in some applications)
        self.add_definition(EpsgDefinition {
            code: 4322,
            name: "WGS 72".to_string(),
            proj_string: "+proj=longlat +ellps=WGS72 +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Geographic,
            area_of_use: "World".to_string(),
            unit: "degree".to_string(),
            datum: "WGS72".to_string(),
        });

        // EPSG:3034 - ETRS89 / LCC Europe (Lambert Conformal Conic)
        self.add_definition(EpsgDefinition {
            code: 3034,
            name: "ETRS89 / LCC Europe".to_string(),
            proj_string: "+proj=lcc +lat_1=35 +lat_2=65 +lat_0=52 +lon_0=10 +x_0=4000000 +y_0=2800000 +ellps=GRS80 +units=m +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "Europe".to_string(),
            unit: "metre".to_string(),
            datum: "ETRS89".to_string(),
        });

        // EPSG:2163 - US National Atlas Equal Area (for thematic mapping)
        self.add_definition(EpsgDefinition {
            code: 2163,
            name: "US National Atlas Equal Area".to_string(),
            proj_string: "+proj=laea +lat_0=45 +lon_0=-100 +x_0=0 +y_0=0 +a=6370997 +b=6370997 +units=m +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "United States".to_string(),
            unit: "metre".to_string(),
            datum: "Sphere".to_string(),
        });

        // EPSG:3413 - WGS 84 / NSIDC Sea Ice Polar Stereographic North
        self.add_definition(EpsgDefinition {
            code: 3413,
            name: "WGS 84 / NSIDC Sea Ice Polar Stereographic North".to_string(),
            proj_string: "+proj=stere +lat_0=90 +lat_ts=70 +lon_0=-45 +k=1 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "Northern hemisphere - north of 60°N".to_string(),
            unit: "metre".to_string(),
            datum: "WGS84".to_string(),
        });

        // EPSG:3031 - WGS 84 / Antarctic Polar Stereographic
        self.add_definition(EpsgDefinition {
            code: 3031,
            name: "WGS 84 / Antarctic Polar Stereographic".to_string(),
            proj_string: "+proj=stere +lat_0=-90 +lat_ts=-71 +lon_0=0 +k=1 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "Antarctica".to_string(),
            unit: "metre".to_string(),
            datum: "WGS84".to_string(),
        });
    }
}

impl Default for EpsgDatabase {
    fn default() -> Self {
        Self::new()
    }
}

/// Global EPSG database instance.
static EPSG_DB: once_cell::sync::Lazy<EpsgDatabase> = once_cell::sync::Lazy::new(EpsgDatabase::new);

/// Looks up an EPSG code in the global database.
pub fn lookup_epsg(code: u32) -> Result<&'static EpsgDefinition> {
    EPSG_DB.lookup(code)
}

/// Checks if an EPSG code exists in the global database.
pub fn contains_epsg(code: u32) -> bool {
    EPSG_DB.contains(code)
}

/// Returns all available EPSG codes from the global database.
pub fn available_epsg_codes() -> Vec<u32> {
    EPSG_DB.codes()
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_epsg_database_creation() {
        let db = EpsgDatabase::new();
        assert!(!db.is_empty());
        assert!(db.len() > 100); // Should have at least 100 codes
    }

    #[test]
    fn test_wgs84_lookup() {
        let db = EpsgDatabase::new();
        let wgs84 = db.lookup(4326);
        assert!(wgs84.is_ok());
        let wgs84 = wgs84.expect("WGS84 should exist");
        assert_eq!(wgs84.code, 4326);
        assert_eq!(wgs84.name, "WGS 84");
        assert_eq!(wgs84.crs_type, CrsType::Geographic);
        assert_eq!(wgs84.datum, "WGS84");
    }

    #[test]
    fn test_web_mercator_lookup() {
        let db = EpsgDatabase::new();
        let web_merc = db.lookup(3857);
        assert!(web_merc.is_ok());
        let web_merc = web_merc.expect("Web Mercator should exist");
        assert_eq!(web_merc.code, 3857);
        assert_eq!(web_merc.crs_type, CrsType::Projected);
        assert_eq!(web_merc.unit, "metre");
    }

    #[test]
    fn test_utm_zones() {
        let db = EpsgDatabase::new();

        // Test UTM zone 1N
        let utm_1n = db.lookup(32601);
        assert!(utm_1n.is_ok());
        let utm_1n = utm_1n.expect("UTM 1N should exist");
        assert!(utm_1n.name.contains("UTM zone 1N"));

        // Test UTM zone 60N
        let utm_60n = db.lookup(32660);
        assert!(utm_60n.is_ok());

        // Test UTM zone 1S
        let utm_1s = db.lookup(32701);
        assert!(utm_1s.is_ok());

        // Test UTM zone 60S
        let utm_60s = db.lookup(32760);
        assert!(utm_60s.is_ok());
    }

    #[test]
    fn test_nonexistent_code() {
        let db = EpsgDatabase::new();
        let result = db.lookup(99999);
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(Error::EpsgCodeNotFound { code: 99999 })
        ));
    }

    #[test]
    fn test_contains() {
        let db = EpsgDatabase::new();
        assert!(db.contains(4326));
        assert!(db.contains(3857));
        assert!(!db.contains(99999));
    }

    #[test]
    fn test_codes_sorted() {
        let db = EpsgDatabase::new();
        let codes = db.codes();
        assert!(!codes.is_empty());

        // Check that codes are sorted
        for i in 1..codes.len() {
            assert!(codes[i - 1] < codes[i]);
        }
    }

    #[test]
    fn test_custom_definition() {
        let mut db = EpsgDatabase::new();
        let custom = EpsgDefinition {
            code: 99999,
            name: "Custom CRS".to_string(),
            proj_string: "+proj=longlat +datum=WGS84 +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Geographic,
            area_of_use: "Custom area".to_string(),
            unit: "degree".to_string(),
            datum: "WGS84".to_string(),
        };

        db.add_definition(custom.clone());
        assert!(db.contains(99999));

        let retrieved = db.lookup(99999);
        assert!(retrieved.is_ok());
        assert_eq!(retrieved.expect("should exist").name, "Custom CRS");
    }

    #[test]
    fn test_global_lookup() {
        let wgs84 = lookup_epsg(4326);
        assert!(wgs84.is_ok());

        assert!(contains_epsg(4326));
        assert!(!contains_epsg(99999));

        let codes = available_epsg_codes();
        assert!(!codes.is_empty());
    }
}
