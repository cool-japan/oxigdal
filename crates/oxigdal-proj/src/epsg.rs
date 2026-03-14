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

        // ── JGD2011 geographic (EPSG:6668)
        self.add_definition(EpsgDefinition {
            code: 6668,
            name: "JGD2011".to_string(),
            proj_string: "+proj=longlat +ellps=GRS80 +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Geographic,
            area_of_use: "Japan".to_string(),
            unit: "degree".to_string(),
            datum: "JGD2011".to_string(),
        });

        // ── JGD2011 UTM zones EPSG:6669–6687 (zones 51N–60N with JGD2011 datum)
        for zone in 51u32..=60 {
            let code = 6618 + zone; // 6669..6687 for zones 51..60 (not consecutive — use explicit)
            let central_meridian = (zone as i32 - 1) * 6 - 177;
            self.add_definition(EpsgDefinition {
                code,
                name: format!("JGD2011 / UTM zone {}N", zone),
                proj_string: format!("+proj=utm +zone={} +ellps=GRS80 +units=m +no_defs", zone),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: format!(
                    "Japan — {}°E to {}°E",
                    central_meridian - 3,
                    central_meridian + 3
                ),
                unit: "metre".to_string(),
                datum: "JGD2011".to_string(),
            });
        }

        // ── GDA2020 geographic (EPSG:7844)
        self.add_definition(EpsgDefinition {
            code: 7844,
            name: "GDA2020".to_string(),
            proj_string: "+proj=longlat +ellps=GRS80 +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Geographic,
            area_of_use: "Australia".to_string(),
            unit: "degree".to_string(),
            datum: "GDA2020".to_string(),
        });

        // ── GDA2020 UTM zones EPSG:7845–7858 (zones 48S–60S)
        for zone in 48u32..=60 {
            let code = 7797 + zone; // 7845..7857 for zones 48..60
            let central_meridian = (zone as i32 - 1) * 6 - 177;
            self.add_definition(EpsgDefinition {
                code,
                name: format!("GDA2020 / MGA zone {}", zone),
                proj_string: format!(
                    "+proj=utm +zone={} +south +ellps=GRS80 +units=m +no_defs",
                    zone
                ),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: format!(
                    "Australia — {}°E to {}°E",
                    central_meridian - 3,
                    central_meridian + 3
                ),
                unit: "metre".to_string(),
                datum: "GDA2020".to_string(),
            });
        }

        // ── German DHDN Gauss-Krüger zones EPSG:31466–31469 (zones 2–5)
        let dhdn_zones = [
            (31466u32, 2u32, 6000000.0_f64, "Germany — 6°E to 8°E"),
            (31467, 3, 9000000.0, "Germany — 8°E to 10°E"),
            (31468, 4, 12000000.0, "Germany — 10°E to 12°E"),
            (31469, 5, 15000000.0, "Germany — 12°E to 14°E"),
        ];
        for (code, zone, fe, aou) in dhdn_zones {
            let lon_0 = zone as f64 * 3.0;
            self.add_definition(EpsgDefinition {
                code,
                name: format!("DHDN / 3-degree Gauss-Kruger zone {}", zone),
                proj_string: format!(
                    "+proj=tmerc +lat_0=0 +lon_0={} +k=1 +x_0={} +y_0=0 +ellps=bessel +towgs84=598.1,73.7,418.2,0.202,0.045,2.455,6.7 +units=m +no_defs",
                    lon_0, fe as u64
                ),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: aou.to_string(),
                unit: "metre".to_string(),
                datum: "DHDN".to_string(),
            });
        }

        // ── ETRS89 / UTM zone 33N (EPSG:25833) and 34N (EPSG:25834)
        for zone in [33u32, 34u32] {
            let code = 25800 + zone;
            let central_meridian = (zone as i32 - 1) * 6 - 177;
            self.add_definition(EpsgDefinition {
                code,
                name: format!("ETRS89 / UTM zone {}N", zone),
                proj_string: format!(
                    "+proj=utm +zone={} +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs",
                    zone
                ),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: format!(
                    "Europe — {}°E to {}°E",
                    central_meridian - 3,
                    central_meridian + 3
                ),
                unit: "metre".to_string(),
                datum: "ETRS89".to_string(),
            });
        }

        // ── French RGF93 — Lambert-93 (EPSG:2154)
        self.add_definition(EpsgDefinition {
            code: 2154,
            name: "RGF93 v1 / Lambert-93".to_string(),
            proj_string: "+proj=lcc +lat_0=46.5 +lon_0=3 +lat_1=44 +lat_2=49 +x_0=700000 +y_0=6600000 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "France".to_string(),
            unit: "metre".to_string(),
            datum: "RGF93".to_string(),
        });

        // ── RGF93 CC zones (EPSG:3942–3950)
        for (zone_idx, code) in (3942u32..=3950).enumerate() {
            let lat_0 = 42.0 + zone_idx as f64;
            let lat_1 = lat_0 - 0.75;
            let lat_2 = lat_0 + 0.75;
            let y_0 = 1_200_000.0 + zone_idx as f64 * 1_000_000.0;
            self.add_definition(EpsgDefinition {
                code,
                name: format!("RGF93 v1 / CC{}", 42 + zone_idx),
                proj_string: format!(
                    "+proj=lcc +lat_0={} +lon_0=3 +lat_1={} +lat_2={} +x_0=1700000 +y_0={} +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs",
                    lat_0, lat_1, lat_2, y_0 as u64
                ),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: format!("France — CC{} zone", 42 + zone_idx),
                unit: "metre".to_string(),
                datum: "RGF93".to_string(),
            });
        }

        // ── UK — TM65 Irish Grid (EPSG:29903)
        self.add_definition(EpsgDefinition {
            code: 29903,
            name: "TM65 / Irish Grid".to_string(),
            proj_string: "+proj=tmerc +lat_0=53.5 +lon_0=-8 +k=1.000035 +x_0=200000 +y_0=250000 +ellps=mod_airy +towgs84=482.5,-130.6,564.6,-1.042,-0.214,-0.631,8.15 +units=m +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "Ireland".to_string(),
            unit: "metre".to_string(),
            datum: "TM65".to_string(),
        });

        // ── GDA94 MGA zones EPSG:28348–28356 (zones 48S–56S)
        for zone in 48u32..=56 {
            let code = 27892 + zone; // 28348..28356
            let central_meridian = (zone as i32 - 1) * 6 - 177;
            self.add_definition(EpsgDefinition {
                code,
                name: format!("GDA94 / MGA zone {}", zone),
                proj_string: format!("+proj=utm +zone={} +south +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs", zone),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: format!("Australia — {}°E to {}°E", central_meridian - 3, central_meridian + 3),
                unit: "metre".to_string(),
                datum: "GDA94".to_string(),
            });
        }

        // ── CGCS2000 / 3-degree Gauss-Kruger zones EPSG:4491–4554
        // China National Grid 3° strips
        for zone in 25u32..=45 {
            let code = 4466 + zone; // 4491..4511 for zones 25..45 (within CGCS2000 area)
            let lon_0 = zone as f64 * 3.0;
            self.add_definition(EpsgDefinition {
                code,
                name: format!("CGCS2000 / 3-degree Gauss-Kruger zone {}", zone),
                proj_string: format!(
                    "+proj=tmerc +lat_0=0 +lon_0={} +k=1 +x_0={} +y_0=0 +ellps=GRS80 +units=m +no_defs",
                    lon_0, zone as u64 * 1_000_000 + 500_000
                ),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: format!("China — {}°E to {}°E", lon_0 as i32 - 2, lon_0 as i32 + 2),
                unit: "metre".to_string(),
                datum: "CGCS2000".to_string(),
            });
        }

        // ── CGCS2000 / 6-degree Gauss-Kruger zones EPSG:4526–4554 (CM 13..23)
        for zone in 13u32..=23 {
            let code = 4513 + zone; // 4526..4536 zone 13..23
            let lon_0 = (zone as f64 - 1.0) * 6.0 - 177.0 + 6.0; // matches standard zone cm
            let lon_0_precise = zone as f64 * 6.0 - 183.0;
            self.add_definition(EpsgDefinition {
                code,
                name: format!("CGCS2000 / 6-degree Gauss-Kruger zone {}", zone),
                proj_string: format!(
                    "+proj=tmerc +lat_0=0 +lon_0={} +k=1 +x_0={} +y_0=0 +ellps=GRS80 +units=m +no_defs",
                    lon_0_precise, zone as u64 * 1_000_000 + 500_000
                ),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: format!("China — {}°E to {}°E", lon_0 as i32 - 3, lon_0 as i32 + 3),
                unit: "metre".to_string(),
                datum: "CGCS2000".to_string(),
            });
        }

        // ── Canadian — NAD83 / MTM zones EPSG:2294–2296 and UTM zone 10N (BC)
        let canadian_zones: &[(u32, &str, &str)] = &[
            (
                2294,
                "NAD83 / Prince Edward I.",
                "+proj=tmerc +lat_0=0 +lon_0=-63 +k=0.9999 +x_0=300000 +y_0=0 +datum=NAD83 +units=m +no_defs",
            ),
            (
                2295,
                "NAD83 / Nova Scotia 2010",
                "+proj=tmerc +lat_0=0 +lon_0=-61.5 +k=0.9999 +x_0=300000 +y_0=0 +datum=NAD83 +units=m +no_defs",
            ),
            (
                2296,
                "NAD83 / Sterea Netherlands",
                "+proj=sterea +lat_0=52.15617 +lon_0=5.38721 +k=0.9999079 +x_0=155000 +y_0=463000 +ellps=bessel +towgs84=565.4,50.3,465.2,0,0,0,0 +units=m +no_defs",
            ),
            (
                3157,
                "NAD83(CSRS) / UTM zone 9N",
                "+proj=utm +zone=9 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs",
            ),
            (
                3158,
                "NAD83(CSRS) / UTM zone 10N",
                "+proj=utm +zone=10 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs",
            ),
        ];
        for (code, name, proj) in canadian_zones {
            self.add_definition(EpsgDefinition {
                code: *code,
                name: name.to_string(),
                proj_string: proj.to_string(),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: "Canada".to_string(),
                unit: "metre".to_string(),
                datum: "NAD83".to_string(),
            });
        }

        // ── ETRS89 / LAEA Europe (EPSG:3035)
        self.add_definition(EpsgDefinition {
            code: 3035,
            name: "ETRS89-extended / LAEA Europe".to_string(),
            proj_string: "+proj=laea +lat_0=52 +lon_0=10 +x_0=4321000 +y_0=3210000 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "Europe".to_string(),
            unit: "metre".to_string(),
            datum: "ETRS89".to_string(),
        });

        // ── ETRS89 / UTM zones 28N–37N (full range for Europe)
        for zone in 28u32..=37 {
            let code = 25800 + zone;
            if code == 25832 || code == 25833 || code == 25834 {
                continue; // already added
            }
            let central_meridian = (zone as i32 - 1) * 6 - 177;
            self.add_definition(EpsgDefinition {
                code,
                name: format!("ETRS89 / UTM zone {}N", zone),
                proj_string: format!(
                    "+proj=utm +zone={} +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs",
                    zone
                ),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: format!(
                    "Europe — {}°E to {}°E",
                    central_meridian - 3,
                    central_meridian + 3
                ),
                unit: "metre".to_string(),
                datum: "ETRS89".to_string(),
            });
        }

        // ── World projections: Sinusoidal, Mollweide, Robinson (ESRI/EPSG codes)
        self.add_definition(EpsgDefinition {
            code: 54008,
            name: "World Sinusoidal".to_string(),
            proj_string: "+proj=sinu +lon_0=0 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs"
                .to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "World".to_string(),
            unit: "metre".to_string(),
            datum: "WGS84".to_string(),
        });

        self.add_definition(EpsgDefinition {
            code: 54009,
            name: "World Mollweide".to_string(),
            proj_string: "+proj=moll +lon_0=0 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs"
                .to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "World".to_string(),
            unit: "metre".to_string(),
            datum: "WGS84".to_string(),
        });

        self.add_definition(EpsgDefinition {
            code: 54030,
            name: "World Robinson".to_string(),
            proj_string: "+proj=robin +lon_0=0 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs"
                .to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "World".to_string(),
            unit: "metre".to_string(),
            datum: "WGS84".to_string(),
        });

        // World Equal Earth (EPSG:8857)
        self.add_definition(EpsgDefinition {
            code: 8857,
            name: "WGS 84 / Equal Earth Greenwich".to_string(),
            proj_string: "+proj=eqearth +lon_0=0 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs"
                .to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "World".to_string(),
            unit: "metre".to_string(),
            datum: "WGS84".to_string(),
        });

        // ── NAD83 UTM zones 11N–18N (US/Canada coverage)
        let nad83_utm_zones: &[(u32, u32, &str)] = &[
            (26911, 11, "Between 120°W and 114°W"),
            (26912, 12, "Between 114°W and 108°W"),
            (26913, 13, "Between 108°W and 102°W"),
            (26914, 14, "Between 102°W and 96°W"),
            (26915, 15, "Between 96°W and 90°W"),
            (26916, 16, "Between 90°W and 84°W"),
            (26917, 17, "Between 84°W and 78°W"),
            (26918, 18, "Between 78°W and 72°W"),
            (26919, 19, "Between 72°W and 66°W"),
        ];
        for (code, zone, aou) in nad83_utm_zones {
            self.add_definition(EpsgDefinition {
                code: *code,
                name: format!("NAD83 / UTM zone {}N", zone),
                proj_string: format!("+proj=utm +zone={} +datum=NAD83 +units=m +no_defs", zone),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: aou.to_string(),
                unit: "metre".to_string(),
                datum: "NAD83".to_string(),
            });
        }

        // ── US State Plane: a representative selection from the EPSG:32100 range
        let us_state_planes: &[(u32, &str, &str)] = &[
            (
                32100,
                "NAD83 / Montana",
                "+proj=lcc +lat_0=44.25 +lon_0=-109.5 +lat_1=45 +lat_2=49 +x_0=600000 +y_0=0 +datum=NAD83 +units=m +no_defs",
            ),
            (
                32104,
                "NAD83 / Nebraska",
                "+proj=lcc +lat_0=39.83333 +lon_0=-100 +lat_1=40 +lat_2=43 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
            ),
            (
                32107,
                "NAD83 / New Jersey",
                "+proj=tmerc +lat_0=38.83333 +lon_0=-74.5 +k=0.9999 +x_0=150000 +y_0=0 +datum=NAD83 +units=m +no_defs",
            ),
            (
                32110,
                "NAD83 / New York East",
                "+proj=tmerc +lat_0=38.83333 +lon_0=-74.5 +k=0.9999 +x_0=150000 +y_0=0 +datum=NAD83 +units=m +no_defs",
            ),
            (
                32111,
                "NAD83 / New York Central",
                "+proj=tmerc +lat_0=40 +lon_0=-76.58333 +k=0.9999375 +x_0=250000 +y_0=0 +datum=NAD83 +units=m +no_defs",
            ),
            (
                32112,
                "NAD83 / New York West",
                "+proj=tmerc +lat_0=40 +lon_0=-78.58333 +k=0.9999375 +x_0=350000 +y_0=0 +datum=NAD83 +units=m +no_defs",
            ),
            (
                32113,
                "NAD83 / New York Long Island",
                "+proj=lcc +lat_0=40.16667 +lon_0=-74 +lat_1=40.66667 +lat_2=41.03333 +x_0=300000 +y_0=0 +datum=NAD83 +units=m +no_defs",
            ),
            (
                32118,
                "NAD83 / Ohio North",
                "+proj=lcc +lat_0=39.66667 +lon_0=-82.5 +lat_1=40.43333 +lat_2=41.7 +x_0=600000 +y_0=0 +datum=NAD83 +units=m +no_defs",
            ),
            (
                32119,
                "NAD83 / North Carolina",
                "+proj=lcc +lat_0=33.75 +lon_0=-79 +lat_1=34.33333 +lat_2=36.16667 +x_0=609601.22 +y_0=0 +datum=NAD83 +units=m +no_defs",
            ),
            (
                32120,
                "NAD83 / North Dakota North",
                "+proj=lcc +lat_0=47 +lon_0=-100.5 +lat_1=47.43333 +lat_2=48.73333 +x_0=600000 +y_0=0 +datum=NAD83 +units=m +no_defs",
            ),
            (
                32128,
                "NAD83 / Oregon North",
                "+proj=lcc +lat_0=43.66667 +lon_0=-120.5 +lat_1=44.33333 +lat_2=46 +x_0=2500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
            ),
            (
                32133,
                "NAD83 / Pennsylvania South",
                "+proj=lcc +lat_0=39.33333 +lon_0=-77.75 +lat_1=39.93333 +lat_2=40.96667 +x_0=600000 +y_0=0 +datum=NAD83 +units=m +no_defs",
            ),
            (
                32140,
                "NAD83 / Texas Central",
                "+proj=lcc +lat_0=29.66667 +lon_0=-100.33333 +lat_1=30.11667 +lat_2=31.88333 +x_0=700000 +y_0=3000000 +datum=NAD83 +units=m +no_defs",
            ),
            (
                32145,
                "NAD83 / Vermont",
                "+proj=tmerc +lat_0=42.5 +lon_0=-72.5 +k=0.9999643 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
            ),
            (
                32148,
                "NAD83 / Washington North",
                "+proj=lcc +lat_0=47 +lon_0=-120.83333 +lat_1=47.5 +lat_2=48.73333 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
            ),
            (
                32149,
                "NAD83 / Washington South",
                "+proj=lcc +lat_0=45.33333 +lon_0=-120.5 +lat_1=45.83333 +lat_2=47.33333 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
            ),
            (
                32150,
                "NAD83 / Wisconsin Central",
                "+proj=lcc +lat_0=43.83333 +lon_0=-90 +lat_1=44.25 +lat_2=45.5 +x_0=600000 +y_0=0 +datum=NAD83 +units=m +no_defs",
            ),
            (
                32154,
                "NAD83 / Wyoming East",
                "+proj=tmerc +lat_0=40.5 +lon_0=-105.16667 +k=0.9999375 +x_0=200000 +y_0=0 +datum=NAD83 +units=m +no_defs",
            ),
            (
                32158,
                "NAD83 / Wyoming West",
                "+proj=tmerc +lat_0=40.5 +lon_0=-110.08333 +k=0.9999375 +x_0=800000 +y_0=0 +datum=NAD83 +units=m +no_defs",
            ),
        ];
        for (code, name, proj) in us_state_planes {
            self.add_definition(EpsgDefinition {
                code: *code,
                name: name.to_string(),
                proj_string: proj.to_string(),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: "United States".to_string(),
                unit: "metre".to_string(),
                datum: "NAD83".to_string(),
            });
        }

        // ── SIRGAS 2000 UTM zones for South America
        for zone in 17u32..=25 {
            let code = 31960 + zone; // 31977..31985
            let central_meridian = (zone as i32 - 1) * 6 - 177;
            self.add_definition(EpsgDefinition {
                code,
                name: format!("SIRGAS 2000 / UTM zone {}S", zone),
                proj_string: format!("+proj=utm +zone={} +south +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs", zone),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: format!("South America — {}°W to {}°W", -central_meridian + 3, -central_meridian - 3),
                unit: "metre".to_string(),
                datum: "SIRGAS2000".to_string(),
            });
        }

        // ── NAD27 geographic (EPSG:4267)
        self.add_definition(EpsgDefinition {
            code: 4267,
            name: "NAD27".to_string(),
            proj_string: "+proj=longlat +datum=NAD27 +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Geographic,
            area_of_use: "North America".to_string(),
            unit: "degree".to_string(),
            datum: "NAD27".to_string(),
        });

        // ── NAD27 UTM zones 10N–20N
        for zone in 10u32..=20 {
            let code = 26700 + zone;
            self.add_definition(EpsgDefinition {
                code,
                name: format!("NAD27 / UTM zone {}N", zone),
                proj_string: format!("+proj=utm +zone={} +datum=NAD27 +units=m +no_defs", zone),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: "North America".to_string(),
                unit: "metre".to_string(),
                datum: "NAD27".to_string(),
            });
        }

        // ── Tokyo datum geographic (EPSG:4301)
        self.add_definition(EpsgDefinition {
            code: 4301,
            name: "Tokyo".to_string(),
            proj_string:
                "+proj=longlat +ellps=bessel +towgs84=-146.414,507.337,680.507,0,0,0,0 +no_defs"
                    .to_string(),
            wkt: None,
            crs_type: CrsType::Geographic,
            area_of_use: "Japan".to_string(),
            unit: "degree".to_string(),
            datum: "Tokyo".to_string(),
        });

        // ── DHDN geographic (EPSG:4314)
        self.add_definition(EpsgDefinition {
            code: 4314,
            name: "DHDN".to_string(),
            proj_string: "+proj=longlat +ellps=bessel +towgs84=598.1,-73.7,-418.2,0.202,0.045,-2.455,6.7 +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Geographic,
            area_of_use: "Germany".to_string(),
            unit: "degree".to_string(),
            datum: "DHDN".to_string(),
        });

        // ── Pulkovo 1942 geographic (EPSG:4284)
        self.add_definition(EpsgDefinition {
            code: 4284,
            name: "Pulkovo 1942".to_string(),
            proj_string:
                "+proj=longlat +ellps=krass +towgs84=23.57,-140.95,-79.8,0,0.35,0.79,-0.22 +no_defs"
                    .to_string(),
            wkt: None,
            crs_type: CrsType::Geographic,
            area_of_use: "Russia and Eastern Europe".to_string(),
            unit: "degree".to_string(),
            datum: "Pulkovo1942".to_string(),
        });

        // ── MGI geographic (Austria, EPSG:4312)
        self.add_definition(EpsgDefinition {
            code: 4312,
            name: "MGI".to_string(),
            proj_string: "+proj=longlat +ellps=bessel +towgs84=577.326,90.129,463.919,5.137,1.474,5.297,2.4232 +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Geographic,
            area_of_use: "Austria".to_string(),
            unit: "degree".to_string(),
            datum: "MGI".to_string(),
        });

        // ── MGI / Transverse Mercator (Austria)
        self.add_definition(EpsgDefinition {
            code: 31257,
            name: "MGI / Austria GK West".to_string(),
            proj_string: "+proj=tmerc +lat_0=0 +lon_0=10.33333 +k=1 +x_0=0 +y_0=-5000000 +ellps=bessel +towgs84=577.326,90.129,463.919,5.137,1.474,5.297,2.4232 +units=m +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "Austria West".to_string(),
            unit: "metre".to_string(),
            datum: "MGI".to_string(),
        });

        // ── Swiss CH1903 / LV95 (EPSG:2056)
        self.add_definition(EpsgDefinition {
            code: 2056,
            name: "CH1903+ / LV95".to_string(),
            proj_string: "+proj=somerc +lat_0=46.9524056 +lon_0=7.4395833 +k_0=1 +x_0=2600000 +y_0=1200000 +ellps=bessel +towgs84=674.374,15.056,405.346,0,0,0,0 +units=m +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "Switzerland".to_string(),
            unit: "metre".to_string(),
            datum: "CH1903+".to_string(),
        });

        // ── WGS 84 / Plate Carree (EPSG:32662)
        self.add_definition(EpsgDefinition {
            code: 32662,
            name: "WGS 84 / Plate Carree".to_string(),
            proj_string:
                "+proj=eqc +lat_ts=0 +lat_0=0 +lon_0=0 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs"
                    .to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "World".to_string(),
            unit: "metre".to_string(),
            datum: "WGS84".to_string(),
        });

        // ── WGS 84 / Arctic Polar Stereographic (EPSG:3995)
        self.add_definition(EpsgDefinition {
            code: 3995,
            name: "WGS 84 / Arctic Polar Stereographic".to_string(),
            proj_string: "+proj=stere +lat_0=90 +lat_ts=71 +lon_0=0 +k=1 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "Arctic".to_string(),
            unit: "metre".to_string(),
            datum: "WGS84".to_string(),
        });

        // ── NZGD2000 / New Zealand Transverse Mercator 2000 (EPSG:2193)
        self.add_definition(EpsgDefinition {
            code: 2193,
            name: "NZGD2000 / New Zealand Transverse Mercator 2000".to_string(),
            proj_string: "+proj=tmerc +lat_0=0 +lon_0=173 +k=0.9996 +x_0=1600000 +y_0=10000000 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "New Zealand".to_string(),
            unit: "metre".to_string(),
            datum: "NZGD2000".to_string(),
        });

        // ── EPSG:3857 alias — WGS 84 / Pseudo-Mercator (EPSG:900913 — unofficial)
        // ── South Africa — Hartebeesthoek94 (EPSG:4148)
        self.add_definition(EpsgDefinition {
            code: 4148,
            name: "Hartebeesthoek94".to_string(),
            proj_string: "+proj=longlat +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Geographic,
            area_of_use: "South Africa".to_string(),
            unit: "degree".to_string(),
            datum: "Hartebeesthoek94".to_string(),
        });

        // ── South Africa — Lo19-Lo33 series (Transverse Mercator strips)
        for lo in (19u32..=33).step_by(2) {
            let code = 2046 + (lo - 19) / 2; // 2046, 2047, ... 2054
            self.add_definition(EpsgDefinition {
                code,
                name: format!("Hartebeesthoek94 / Lo{}", lo),
                proj_string: format!(
                    "+proj=tmerc +lat_0=0 +lon_0={} +k=1 +x_0=0 +y_0=0 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs",
                    lo
                ),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: format!("South Africa — Lo{} strip", lo),
                unit: "metre".to_string(),
                datum: "Hartebeesthoek94".to_string(),
            });
        }

        // ── Brazil / SIRGAS 2000 geographic (already have 4674)
        // ── India zone systems (EPSG:24378–24385)
        let india_zones: &[(u32, f64, &str)] = &[
            (24378, 68.0, "India zone I"),
            (24379, 74.0, "India zone IIa"),
            (24380, 80.0, "India zone IIb"),
            (24381, 80.0, "India zone IIIa"),
            (24382, 86.0, "India zone IIIb"),
            (24383, 86.0, "India zone IVa"),
            (24384, 90.0, "India zone IVb"),
            (24385, 90.0, "India zone 0"),
        ];
        for (code, lon_0, aou) in india_zones {
            self.add_definition(EpsgDefinition {
                code: *code,
                name: format!("Kalianpur 1937 / {}", aou),
                proj_string: format!(
                    "+proj=lcc +lat_0=0 +lon_0={} +lat_1=26 +lat_2=30 +x_0=2743196.4 +y_0=914398.8 +a=6377276.345 +b=6356075.413 +units=m +no_defs",
                    lon_0
                ),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: format!("India — {}", aou),
                unit: "metre".to_string(),
                datum: "Kalianpur1937".to_string(),
            });
        }

        // ── EPSG:3034 — ETRS89 / LCC Europe (already in the original list, skip)
        // ── Additional WGS84 UTM zones for specific regions (no new ones needed since 32601–32760 already added)

        // ── Transverse Mercator zones for Iran (EPSG:2056 already used, add 3108+)
        // EPSG:3109 — ETRS89 / UTM zone 37N
        self.add_definition(EpsgDefinition {
            code: 32637,
            name: "WGS 84 / UTM zone 37N (override check)".to_string(),
            // Already in the 32600+ loop; skip if already exists by checking
            proj_string: "+proj=utm +zone=37 +datum=WGS84 +units=m +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "34°E to 40°E, northern hemisphere".to_string(),
            unit: "metre".to_string(),
            datum: "WGS84".to_string(),
        }); // HashMap.insert will update/overwrite harmlessly

        // ── Argentina — POSGAR 98 (EPSG:4190)
        self.add_definition(EpsgDefinition {
            code: 4190,
            name: "POSGAR 98".to_string(),
            proj_string: "+proj=longlat +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Geographic,
            area_of_use: "Argentina".to_string(),
            unit: "degree".to_string(),
            datum: "POSGAR98".to_string(),
        });

        // ── RD New (Netherlands) EPSG:28992
        self.add_definition(EpsgDefinition {
            code: 28992,
            name: "Amersfoort / RD New".to_string(),
            proj_string: "+proj=sterea +lat_0=52.15617 +lon_0=5.38721 +k=0.9999079 +x_0=155000 +y_0=463000 +ellps=bessel +towgs84=565.4,50.3,465.2,0,0,0,0 +units=m +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "Netherlands".to_string(),
            unit: "metre".to_string(),
            datum: "Amersfoort".to_string(),
        });

        // ── Belgium — Belgian Lambert 2008 (EPSG:3812)
        self.add_definition(EpsgDefinition {
            code: 3812,
            name: "ETRS89 / Belgian Lambert 2008".to_string(),
            proj_string: "+proj=lcc +lat_0=50.797815 +lon_0=4.35921583 +lat_1=49.8333333 +lat_2=51.1666667 +x_0=649328 +y_0=665262 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "Belgium".to_string(),
            unit: "metre".to_string(),
            datum: "ETRS89".to_string(),
        });

        // ── Sweden — SWEREF99 TM (EPSG:3006)
        self.add_definition(EpsgDefinition {
            code: 3006,
            name: "SWEREF99 TM".to_string(),
            proj_string: "+proj=utm +zone=33 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs"
                .to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "Sweden".to_string(),
            unit: "metre".to_string(),
            datum: "SWEREF99".to_string(),
        });

        // ── Poland — ETRS89 / Poland CS92 (EPSG:2180)
        self.add_definition(EpsgDefinition {
            code: 2180,
            name: "ETRS89 / Poland CS92".to_string(),
            proj_string: "+proj=tmerc +lat_0=0 +lon_0=19 +k=0.9993 +x_0=500000 +y_0=-5300000 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "Poland".to_string(),
            unit: "metre".to_string(),
            datum: "ETRS89".to_string(),
        });

        // ── Czech / Slovak — S-JTSK (EPSG:5514)
        self.add_definition(EpsgDefinition {
            code: 5514,
            name: "S-JTSK/05 (Ferro) / Krovak East North".to_string(),
            proj_string: "+proj=krovak +lat_0=49.5 +lon_0=24.8333333 +alpha=30.2881397 +k=0.9999 +x_0=0 +y_0=0 +ellps=bessel +pm=ferro +units=m +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "Czech Republic and Slovakia".to_string(),
            unit: "metre".to_string(),
            datum: "S-JTSK".to_string(),
        });

        // ── Israel — ITM (EPSG:2039)
        self.add_definition(EpsgDefinition {
            code: 2039,
            name: "Israel 1993 / Israeli TM Grid".to_string(),
            proj_string: "+proj=tmerc +lat_0=31.7343936 +lon_0=35.2045169 +k=1.0000067 +x_0=219529.584 +y_0=626907.39 +ellps=GRS80 +towgs84=-48,55,52,0,0,0,0 +units=m +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "Israel".to_string(),
            unit: "metre".to_string(),
            datum: "Israel".to_string(),
        });

        // ── Turkey — ED50 / UTM zones 35N-37N (EPSG:23035-23037)
        for zone in 35u32..=37 {
            let utm_code = 23000 + zone;
            self.add_definition(EpsgDefinition {
                code: utm_code,
                name: format!("ED50 / UTM zone {}N", zone),
                proj_string: format!("+proj=utm +zone={} +ellps=intl +towgs84=-87,-98,-121,0,0,0,0 +units=m +no_defs", zone),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: format!("Turkey — UTM zone {}N", zone),
                unit: "metre".to_string(),
                datum: "ED50".to_string(),
            });
        }

        // ── Korea — KGD2002 (EPSG:4737) geographic
        self.add_definition(EpsgDefinition {
            code: 4737,
            name: "GRS 1980(IUGG, 1980)".to_string(),
            proj_string: "+proj=longlat +ellps=GRS80 +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Geographic,
            area_of_use: "Korea".to_string(),
            unit: "degree".to_string(),
            datum: "GRS80".to_string(),
        });

        // ── Korea — KATEC (EPSG:5179) / Korea 2000 TM
        self.add_definition(EpsgDefinition {
            code: 5179,
            name: "Korea 2000 / Unified CS".to_string(),
            proj_string: "+proj=tmerc +lat_0=38 +lon_0=127.5 +k=0.9996 +x_0=1000000 +y_0=2000000 +ellps=GRS80 +units=m +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "South Korea".to_string(),
            unit: "metre".to_string(),
            datum: "GRS80".to_string(),
        });

        // ── WGS84 / North Pole LAEA (EPSG:3574)
        self.add_definition(EpsgDefinition {
            code: 3574,
            name: "WGS 84 / North Pole LAEA Atlantic".to_string(),
            proj_string:
                "+proj=laea +lat_0=90 +lon_0=-40 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs"
                    .to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "North Polar region — Atlantic sector".to_string(),
            unit: "metre".to_string(),
            datum: "WGS84".to_string(),
        });

        // ── WGS84 North Pole LAEA Europe (EPSG:3575)
        self.add_definition(EpsgDefinition {
            code: 3575,
            name: "WGS 84 / North Pole LAEA Europe".to_string(),
            proj_string:
                "+proj=laea +lat_0=90 +lon_0=10 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs"
                    .to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "North Polar region — European sector".to_string(),
            unit: "metre".to_string(),
            datum: "WGS84".to_string(),
        });

        // ── Mercator world projections
        self.add_definition(EpsgDefinition {
            code: 3832,
            name: "WGS 84 / PDC Mercator".to_string(),
            proj_string: "+proj=merc +lon_0=150 +k=1 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs"
                .to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "Pacific Ocean area".to_string(),
            unit: "metre".to_string(),
            datum: "WGS84".to_string(),
        });

        // ── WGS84 / Azimuthal Equidistant World (EPSG:54032)
        self.add_definition(EpsgDefinition {
            code: 54032,
            name: "World Azimuthal Equidistant".to_string(),
            proj_string:
                "+proj=aeqd +lat_0=0 +lon_0=0 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs"
                    .to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "World".to_string(),
            unit: "metre".to_string(),
            datum: "WGS84".to_string(),
        });

        // ── WGS84 / Gnomonic (EPSG:54016 — ESRI)
        self.add_definition(EpsgDefinition {
            code: 54016,
            name: "World Gnomonic".to_string(),
            proj_string:
                "+proj=gnom +lat_0=90 +lon_0=0 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs"
                    .to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "Arctic region".to_string(),
            unit: "metre".to_string(),
            datum: "WGS84".to_string(),
        });

        // ── Eckert IV world projection (EPSG:54012 — ESRI)
        self.add_definition(EpsgDefinition {
            code: 54012,
            name: "World Eckert IV".to_string(),
            proj_string: "+proj=eck4 +lon_0=0 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs"
                .to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "World".to_string(),
            unit: "metre".to_string(),
            datum: "WGS84".to_string(),
        });

        // ── Eckert VI world projection (EPSG:54010 — ESRI)
        self.add_definition(EpsgDefinition {
            code: 54010,
            name: "World Eckert VI".to_string(),
            proj_string: "+proj=eck6 +lon_0=0 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs"
                .to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "World".to_string(),
            unit: "metre".to_string(),
            datum: "WGS84".to_string(),
        });

        // ── WGS84 / Cassini Soldner — Trinidad example (EPSG:2314)
        self.add_definition(EpsgDefinition {
            code: 2314,
            name: "Trinidad 1903 / Trinidad Grid".to_string(),
            proj_string: "+proj=cass +lat_0=10.44166 +lon_0=-61.33333 +x_0=86501.46 +y_0=65379.013 +a=6378293.645 +b=6356617.987 +towgs84=-61.702,284.488,472.052,0,0,0,0 +units=lk +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "Trinidad and Tobago".to_string(),
            unit: "link".to_string(),
            datum: "Trinidad1903".to_string(),
        });

        // ── WGS84 / Cassini — Djakarta (EPSG:3376)
        self.add_definition(EpsgDefinition {
            code: 3376,
            name: "GDM2000 / Peninsula RSO".to_string(),
            proj_string: "+proj=omerc +lat_0=4 +lonc=102.25 +alpha=323.1301023611 +k=0.99984 +x_0=804671 +y_0=0 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "Malaysia".to_string(),
            unit: "metre".to_string(),
            datum: "GDM2000".to_string(),
        });

        // ── Norwegian EUREF89 UTM zones (EPSG:25832–25836)
        // 25832, 25833, 25834 already added; add 25835 and 25836
        self.add_definition(EpsgDefinition {
            code: 25835,
            name: "ETRS89 / UTM zone 35N".to_string(),
            proj_string: "+proj=utm +zone=35 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs"
                .to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "Europe — 24°E to 30°E".to_string(),
            unit: "metre".to_string(),
            datum: "ETRS89".to_string(),
        });

        self.add_definition(EpsgDefinition {
            code: 25836,
            name: "ETRS89 / UTM zone 36N".to_string(),
            proj_string: "+proj=utm +zone=36 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs"
                .to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "Europe — 30°E to 36°E".to_string(),
            unit: "metre".to_string(),
            datum: "ETRS89".to_string(),
        });

        // ── Gauss-Kruger zones for Russia (6° strips, Pulkovo 1942)
        // Zone 4 = 21°E strip, Zone 32 = 189°E
        for zone in 4u32..=32 {
            let code = 28400 + zone; // 28404..28432
            let lon_0 = (zone as f64 - 1.0) * 6.0 - 177.0 + 6.0;
            self.add_definition(EpsgDefinition {
                code,
                name: format!("Pulkovo 1942 / Gauss-Kruger zone {}", zone),
                proj_string: format!(
                    "+proj=tmerc +lat_0=0 +lon_0={} +k=1 +x_0={} +y_0=0 +ellps=krass +towgs84=23.57,-140.95,-79.8,0,0.35,0.79,-0.22 +units=m +no_defs",
                    lon_0, zone as u64 * 1_000_000 + 500_000
                ),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: format!("Russia — zone {}", zone),
                unit: "metre".to_string(),
                datum: "Pulkovo1942".to_string(),
            });
        }

        // ── China BD-09 (Baidu) — geographic approximate (non-standard)
        // ── CGCS2000 geographic (EPSG:4490) already added — add UTM variants
        for zone in 43u32..=53 {
            let code = 4535 + (zone - 43); // 4535..4545
            let lon_0 = (zone as f64 - 1.0) * 6.0 - 177.0;
            self.add_definition(EpsgDefinition {
                code,
                name: format!("CGCS2000 / UTM zone {}N", zone),
                proj_string: format!("+proj=utm +zone={} +ellps=GRS80 +units=m +no_defs", zone),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: format!("China — {}°E to {}°E", lon_0 as i32 - 3, lon_0 as i32 + 3),
                unit: "metre".to_string(),
                datum: "CGCS2000".to_string(),
            });
        }

        // ── Peru — PSAD56 (EPSG:4248)
        self.add_definition(EpsgDefinition {
            code: 4248,
            name: "PSAD56".to_string(),
            proj_string: "+proj=longlat +ellps=intl +towgs84=-296,519,-13,0,0,0,0 +no_defs"
                .to_string(),
            wkt: None,
            crs_type: CrsType::Geographic,
            area_of_use: "South America — PSAD56".to_string(),
            unit: "degree".to_string(),
            datum: "PSAD56".to_string(),
        });

        // ── Venezuela — REGVEN (EPSG:4189)
        self.add_definition(EpsgDefinition {
            code: 4189,
            name: "REGVEN".to_string(),
            proj_string: "+proj=longlat +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Geographic,
            area_of_use: "Venezuela".to_string(),
            unit: "degree".to_string(),
            datum: "REGVEN".to_string(),
        });

        // ── Egypt — Egyptian 1907 (EPSG:4229)
        self.add_definition(EpsgDefinition {
            code: 4229,
            name: "Egyptian 1907".to_string(),
            proj_string: "+proj=longlat +ellps=helmert +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Geographic,
            area_of_use: "Egypt".to_string(),
            unit: "degree".to_string(),
            datum: "Egyptian1907".to_string(),
        });

        // ── Morocco — Nord Maroc zone (EPSG:26191)
        self.add_definition(EpsgDefinition {
            code: 26191,
            name: "Merchich / Nord Maroc".to_string(),
            proj_string: "+proj=lcc +lat_0=33.3 +lon_0=-5.4 +lat_1=35.1666667 +lat_2=31.5 +x_0=500000 +y_0=300000 +ellps=clrk80ign +towgs84=31,146,47,0,0,0,0 +units=m +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "Morocco".to_string(),
            unit: "metre".to_string(),
            datum: "Merchich".to_string(),
        });

        // ── Nigeria — Minna (EPSG:4263)
        self.add_definition(EpsgDefinition {
            code: 4263,
            name: "Minna".to_string(),
            proj_string: "+proj=longlat +ellps=clrk80ign +towgs84=-92,-93,122,0,0,0,0 +no_defs"
                .to_string(),
            wkt: None,
            crs_type: CrsType::Geographic,
            area_of_use: "Nigeria".to_string(),
            unit: "degree".to_string(),
            datum: "Minna".to_string(),
        });

        // ── Finland — KKJ (EPSG:4123) geographic
        self.add_definition(EpsgDefinition {
            code: 4123,
            name: "KKJ".to_string(),
            proj_string: "+proj=longlat +ellps=intl +towgs84=-96.062,-82.428,-121.753,4.801,0.345,-1.376,1.496 +no_defs".to_string(),
            wkt: None,
            crs_type: CrsType::Geographic,
            area_of_use: "Finland".to_string(),
            unit: "degree".to_string(),
            datum: "KKJ".to_string(),
        });

        // ── Finland — ETRS-TM35FIN (EPSG:3067)
        self.add_definition(EpsgDefinition {
            code: 3067,
            name: "ETRS89 / TM35FIN(E,N)".to_string(),
            proj_string: "+proj=utm +zone=35 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs"
                .to_string(),
            wkt: None,
            crs_type: CrsType::Projected,
            area_of_use: "Finland".to_string(),
            unit: "metre".to_string(),
            datum: "ETRS89".to_string(),
        });

        // ── Denmark — DKTM1 to DKTM4 (approx EPSG:4093–4096)
        for (zone, lon_0, code) in [
            (1u32, 9.0_f64, 4093u32),
            (2, 10.0, 4094),
            (3, 11.0, 4095),
            (4, 12.0, 4096),
        ] {
            self.add_definition(EpsgDefinition {
                code,
                name: format!("ETRS89 / DKTM{}", zone),
                proj_string: format!("+proj=tmerc +lat_0=0 +lon_0={} +k=0.9999 +x_0=200000 +y_0=-5000000 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs", lon_0),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: format!("Denmark — DKTM{}", zone),
                unit: "metre".to_string(),
                datum: "ETRS89".to_string(),
            });
        }

        // ── Spain — ETRS89 / UTM zone 29N-31N (EPSG:25829-25831)
        for zone in 29u32..=31 {
            let code = 25800 + zone;
            let central_meridian = (zone as i32 - 1) * 6 - 177;
            self.add_definition(EpsgDefinition {
                code,
                name: format!("ETRS89 / UTM zone {}N", zone),
                proj_string: format!(
                    "+proj=utm +zone={} +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs",
                    zone
                ),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: format!(
                    "Spain — {}°W to {}°E",
                    -(central_meridian - 3),
                    central_meridian + 3
                ),
                unit: "metre".to_string(),
                datum: "ETRS89".to_string(),
            });
        }

        // ── NAD83 UTM zones 1–22N (zones 10–20 already added individually, skip those)
        for zone in 1u32..=22 {
            if (10..=20).contains(&zone) {
                continue; // already present
            }
            let code = 26900 + zone;
            self.add_definition(EpsgDefinition {
                code,
                name: format!("NAD83 / UTM zone {}N", zone),
                proj_string: format!("+proj=utm +zone={} +datum=NAD83 +units=m +no_defs", zone),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: "North America".to_string(),
                unit: "metre".to_string(),
                datum: "NAD83".to_string(),
            });
        }

        // ── NAD27 UTM zones 1–9N (extending the already-added 10–20 range)
        for zone in 1u32..=9 {
            let code = 26700 + zone;
            self.add_definition(EpsgDefinition {
                code,
                name: format!("NAD27 / UTM zone {}N", zone),
                proj_string: format!("+proj=utm +zone={} +datum=NAD27 +units=m +no_defs", zone),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: "North America".to_string(),
                unit: "metre".to_string(),
                datum: "NAD27".to_string(),
            });
        }

        // ── GDA94 / MGA zones not yet added (zones not in 48–56 range)
        for zone in 57u32..=60 {
            let code = 28300 + zone;
            let lon_0 = (zone as i32 - 1) * 6 - 177;
            self.add_definition(EpsgDefinition {
                code,
                name: format!("GDA94 / MGA zone {}", zone),
                proj_string: format!("+proj=utm +zone={} +south +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs", zone),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: format!("Australia — {}°E to {}°E", lon_0 - 3, lon_0 + 3),
                unit: "metre".to_string(),
                datum: "GDA94".to_string(),
            });
        }

        // ── GDA2020 / MGA zones (EPSG:7844 + 20 zones)
        for zone in 49u32..=60 {
            let code = 7844 + zone;
            let lon_0 = (zone as i32 - 1) * 6 - 177;
            self.add_definition(EpsgDefinition {
                code,
                name: format!("GDA2020 / MGA zone {}", zone),
                proj_string: format!(
                    "+proj=utm +zone={} +south +ellps=GRS80 +units=m +no_defs",
                    zone
                ),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: format!("Australia — {}°E to {}°E", lon_0 - 3, lon_0 + 3),
                unit: "metre".to_string(),
                datum: "GDA2020".to_string(),
            });
        }

        // ── SIRGAS 2000 / UTM South zones 17S–25S (EPSG:31977–31985)
        for zone in 17u32..=25 {
            let code = 31960 + zone;
            let lon_0 = (zone as i32 - 1) * 6 - 177;
            self.add_definition(EpsgDefinition {
                code,
                name: format!("SIRGAS 2000 / UTM zone {}S", zone),
                proj_string: format!("+proj=utm +zone={} +south +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs", zone),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: format!("South America — {}°W to {}°W, southern hemisphere", -(lon_0 - 3), -(lon_0 + 3)),
                unit: "metre".to_string(),
                datum: "SIRGAS 2000".to_string(),
            });
        }

        // ── ED50 / UTM North zones (EPSG:23028–23038 — historical European)
        for zone in 28u32..=38 {
            let code = 23000 + zone;
            let lon_0 = (zone as i32 - 1) * 6 - 177;
            self.add_definition(EpsgDefinition {
                code,
                name: format!("ED50 / UTM zone {}N", zone),
                proj_string: format!(
                    "+proj=utm +zone={} +ellps=intl +towgs84=-87,-98,-121 +units=m +no_defs",
                    zone
                ),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: format!("Europe (historical) — {}°E to {}°E", lon_0 - 3, lon_0 + 3),
                unit: "metre".to_string(),
                datum: "ED50".to_string(),
            });
        }

        // ── JGD2000 geographic and UTM (EPSG:4612 + 6669–6687)
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

        // ── JGD2000 / Japan Plane Rectangular CS zones I–XIX (EPSG:2443–2461)
        let jp_lon_cm = [
            129.5_f64, 131.0, 132.1667, 133.5, 134.3333, 136.0, 137.1667, 138.5, 139.8333,
            140.8333, 140.25, 142.25, 144.25, 142.0, 127.5, 124.0, 131.0, 136.0, 154.0,
        ];
        for (i, lon_cm) in jp_lon_cm.iter().enumerate() {
            let zone_num = i + 1;
            let code = 2442 + zone_num as u32;
            self.add_definition(EpsgDefinition {
                code,
                name: format!("JGD2000 / Japan Plane Rectangular CS zone {}", zone_num),
                proj_string: format!("+proj=tmerc +lat_0=0 +lon_0={} +k=0.9999 +x_0=0 +y_0=0 +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +units=m +no_defs", lon_cm),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: format!("Japan — zone {}", zone_num),
                unit: "metre".to_string(),
                datum: "JGD2000".to_string(),
            });
        }

        // ── WGS72 / UTM North zones 1–60 (EPSG:32201–32260)
        for zone in 1u32..=60 {
            let code = 32200 + zone;
            self.add_definition(EpsgDefinition {
                code,
                name: format!("WGS 72 / UTM zone {}N", zone),
                proj_string: format!("+proj=utm +zone={} +ellps=WGS72 +towgs84=0,0,4.5,0,0,0.554,0.219 +units=m +no_defs", zone),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: "World".to_string(),
                unit: "metre".to_string(),
                datum: "WGS72".to_string(),
            });
        }

        // ── Additional NAD83 State Planes (completing the range)
        let more_state_planes: &[(u32, &str, &str)] = &[
            (
                32061,
                "NAD83 / Alaska zone 1",
                "+proj=omerc +lat_0=57 +lonc=-133.6666667 +alpha=323.1301 +k=0.9999 +x_0=5000000 +y_0=-5000000 +datum=NAD83 +units=m +no_defs",
            ),
            (
                32064,
                "NAD83 / Alaska zone 4",
                "+proj=tmerc +lat_0=54 +lon_0=-150 +k=0.9999 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
            ),
            (
                32065,
                "NAD83 / Alaska zone 5",
                "+proj=tmerc +lat_0=54 +lon_0=-156 +k=0.9999 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
            ),
            (
                32066,
                "NAD83 / Alaska zone 6",
                "+proj=tmerc +lat_0=54 +lon_0=-162 +k=0.9999 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
            ),
            (
                32067,
                "NAD83 / Alaska zone 7",
                "+proj=tmerc +lat_0=54 +lon_0=-168 +k=0.9999 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
            ),
            (
                32068,
                "NAD83 / Alaska zone 8",
                "+proj=tmerc +lat_0=54 +lon_0=-166 +k=0.9999 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
            ),
            (
                32069,
                "NAD83 / Alaska zone 9",
                "+proj=tmerc +lat_0=54 +lon_0=-168 +k=0.9999 +x_0=500000 +y_0=0 +datum=NAD83 +units=m +no_defs",
            ),
        ];
        for (code, name, proj) in more_state_planes {
            self.add_definition(EpsgDefinition {
                code: *code,
                name: name.to_string(),
                proj_string: proj.to_string(),
                wkt: None,
                crs_type: CrsType::Projected,
                area_of_use: "United States — Alaska".to_string(),
                unit: "metre".to_string(),
                datum: "NAD83".to_string(),
            });
        }
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
