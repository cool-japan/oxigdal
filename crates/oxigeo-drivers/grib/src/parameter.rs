//! Parameter tables for GRIB1 and GRIB2 formats.
//!
//! This module provides parameter definitions and lookups for WMO standard tables,
//! including meteorological variables like temperature, pressure, wind, precipitation, etc.

use crate::error::{GribError, Result};
use serde::{Deserialize, Serialize};

/// GRIB parameter definition
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Parameter {
    /// Parameter short name (e.g., "TMP", "UGRD", "VGRD")
    pub short_name: String,
    /// Parameter long name (e.g., "Temperature", "U component of wind")
    pub long_name: String,
    /// Units (e.g., "K", "m/s", "kg/m^2")
    pub units: String,
    /// Discipline (GRIB2 only, 0 for meteorological products)
    pub discipline: Option<u8>,
    /// Category within discipline
    pub category: u8,
    /// Parameter number within category
    pub number: u8,
}

impl Parameter {
    /// Create a new parameter definition
    pub fn new(
        short_name: impl Into<String>,
        long_name: impl Into<String>,
        units: impl Into<String>,
        discipline: Option<u8>,
        category: u8,
        number: u8,
    ) -> Self {
        Self {
            short_name: short_name.into(),
            long_name: long_name.into(),
            units: units.into(),
            discipline,
            category,
            number,
        }
    }
}

/// GRIB2 parameter lookup using discipline-category-number
pub fn lookup_grib2_parameter(discipline: u8, category: u8, number: u8) -> Result<Parameter> {
    // WMO GRIB2 Code Table 4.2 - Parameter number by product discipline and parameter category
    // Discipline 0 = Meteorological products
    if discipline == 0 {
        match (category, number) {
            // Category 0: Temperature
            (0, 0) => Ok(Parameter::new("TMP", "Temperature", "K", Some(0), 0, 0)),
            (0, 1) => Ok(Parameter::new(
                "VTMP",
                "Virtual temperature",
                "K",
                Some(0),
                0,
                1,
            )),
            (0, 2) => Ok(Parameter::new(
                "POT",
                "Potential temperature",
                "K",
                Some(0),
                0,
                2,
            )),
            (0, 3) => Ok(Parameter::new(
                "EPOT",
                "Pseudo-adiabatic potential temperature",
                "K",
                Some(0),
                0,
                3,
            )),
            (0, 4) => Ok(Parameter::new(
                "TMAX",
                "Maximum temperature",
                "K",
                Some(0),
                0,
                4,
            )),
            (0, 5) => Ok(Parameter::new(
                "TMIN",
                "Minimum temperature",
                "K",
                Some(0),
                0,
                5,
            )),
            (0, 6) => Ok(Parameter::new(
                "DPT",
                "Dew point temperature",
                "K",
                Some(0),
                0,
                6,
            )),
            (0, 7) => Ok(Parameter::new(
                "DEPR",
                "Dew point depression",
                "K",
                Some(0),
                0,
                7,
            )),
            (0, 8) => Ok(Parameter::new("LAPR", "Lapse rate", "K/m", Some(0), 0, 8)),

            // Category 1: Moisture
            (1, 0) => Ok(Parameter::new(
                "SPFH",
                "Specific humidity",
                "kg/kg",
                Some(0),
                1,
                0,
            )),
            (1, 1) => Ok(Parameter::new(
                "RH",
                "Relative humidity",
                "%",
                Some(0),
                1,
                1,
            )),
            (1, 2) => Ok(Parameter::new(
                "MIXR",
                "Humidity mixing ratio",
                "kg/kg",
                Some(0),
                1,
                2,
            )),
            (1, 3) => Ok(Parameter::new(
                "PWAT",
                "Precipitable water",
                "kg/m^2",
                Some(0),
                1,
                3,
            )),
            (1, 4) => Ok(Parameter::new(
                "VAPP",
                "Vapor pressure",
                "Pa",
                Some(0),
                1,
                4,
            )),
            (1, 5) => Ok(Parameter::new(
                "SATD",
                "Saturation deficit",
                "Pa",
                Some(0),
                1,
                5,
            )),
            (1, 6) => Ok(Parameter::new(
                "EVP",
                "Evaporation",
                "kg/m^2",
                Some(0),
                1,
                6,
            )),
            (1, 7) => Ok(Parameter::new(
                "PRATE",
                "Precipitation rate",
                "kg/m^2/s",
                Some(0),
                1,
                7,
            )),
            (1, 8) => Ok(Parameter::new(
                "APCP",
                "Total precipitation",
                "kg/m^2",
                Some(0),
                1,
                8,
            )),
            (1, 9) => Ok(Parameter::new(
                "NCPCP",
                "Large scale precipitation",
                "kg/m^2",
                Some(0),
                1,
                9,
            )),
            (1, 10) => Ok(Parameter::new(
                "ACPCP",
                "Convective precipitation",
                "kg/m^2",
                Some(0),
                1,
                10,
            )),

            // Category 2: Momentum
            (2, 0) => Ok(Parameter::new(
                "WDIR",
                "Wind direction",
                "degree",
                Some(0),
                2,
                0,
            )),
            (2, 1) => Ok(Parameter::new("WIND", "Wind speed", "m/s", Some(0), 2, 1)),
            (2, 2) => Ok(Parameter::new(
                "UGRD",
                "U component of wind",
                "m/s",
                Some(0),
                2,
                2,
            )),
            (2, 3) => Ok(Parameter::new(
                "VGRD",
                "V component of wind",
                "m/s",
                Some(0),
                2,
                3,
            )),
            (2, 4) => Ok(Parameter::new(
                "STRM",
                "Stream function",
                "m^2/s",
                Some(0),
                2,
                4,
            )),
            (2, 5) => Ok(Parameter::new(
                "VPOT",
                "Velocity potential",
                "m^2/s",
                Some(0),
                2,
                5,
            )),
            (2, 6) => Ok(Parameter::new(
                "MNTSF",
                "Montgomery stream function",
                "m^2/s^2",
                Some(0),
                2,
                6,
            )),
            (2, 7) => Ok(Parameter::new(
                "SGCVV",
                "Sigma coordinate vertical velocity",
                "1/s",
                Some(0),
                2,
                7,
            )),
            (2, 8) => Ok(Parameter::new(
                "VVEL",
                "Vertical velocity (pressure)",
                "Pa/s",
                Some(0),
                2,
                8,
            )),
            (2, 9) => Ok(Parameter::new(
                "DZDT",
                "Vertical velocity (geometric)",
                "m/s",
                Some(0),
                2,
                9,
            )),
            (2, 10) => Ok(Parameter::new(
                "ABSV",
                "Absolute vorticity",
                "1/s",
                Some(0),
                2,
                10,
            )),

            // Category 3: Mass
            (3, 0) => Ok(Parameter::new("PRES", "Pressure", "Pa", Some(0), 3, 0)),
            (3, 1) => Ok(Parameter::new(
                "PRMSL",
                "Pressure reduced to MSL",
                "Pa",
                Some(0),
                3,
                1,
            )),
            (3, 2) => Ok(Parameter::new(
                "PTEND",
                "Pressure tendency",
                "Pa/s",
                Some(0),
                3,
                2,
            )),
            (3, 3) => Ok(Parameter::new(
                "ICAHT",
                "ICAO Standard Atmosphere reference height",
                "m",
                Some(0),
                3,
                3,
            )),
            (3, 4) => Ok(Parameter::new(
                "GP",
                "Geopotential",
                "m^2/s^2",
                Some(0),
                3,
                4,
            )),
            (3, 5) => Ok(Parameter::new(
                "HGT",
                "Geopotential height",
                "gpm",
                Some(0),
                3,
                5,
            )),
            (3, 6) => Ok(Parameter::new(
                "DIST",
                "Geometric height",
                "m",
                Some(0),
                3,
                6,
            )),
            (3, 7) => Ok(Parameter::new(
                "HSTDV",
                "Standard deviation of height",
                "m",
                Some(0),
                3,
                7,
            )),
            (3, 8) => Ok(Parameter::new(
                "PRESA",
                "Pressure anomaly",
                "Pa",
                Some(0),
                3,
                8,
            )),

            // Category 6: Cloud
            (6, 0) => Ok(Parameter::new("CICE", "Cloud ice", "kg/m^2", Some(0), 6, 0)),
            (6, 1) => Ok(Parameter::new(
                "TCDC",
                "Total cloud cover",
                "%",
                Some(0),
                6,
                1,
            )),
            (6, 2) => Ok(Parameter::new(
                "CDCON",
                "Convective cloud cover",
                "%",
                Some(0),
                6,
                2,
            )),
            (6, 3) => Ok(Parameter::new(
                "LCDC",
                "Low cloud cover",
                "%",
                Some(0),
                6,
                3,
            )),
            (6, 4) => Ok(Parameter::new(
                "MCDC",
                "Medium cloud cover",
                "%",
                Some(0),
                6,
                4,
            )),
            (6, 5) => Ok(Parameter::new(
                "HCDC",
                "High cloud cover",
                "%",
                Some(0),
                6,
                5,
            )),
            (6, 6) => Ok(Parameter::new(
                "CWAT",
                "Cloud water",
                "kg/m^2",
                Some(0),
                6,
                6,
            )),

            _ => Err(GribError::InvalidParameter {
                discipline,
                category,
                number,
            }),
        }
    } else {
        // Other disciplines not implemented yet
        Err(GribError::InvalidParameter {
            discipline,
            category,
            number,
        })
    }
}

/// GRIB1 parameter lookup using parameter table version and parameter number
pub fn lookup_grib1_parameter(table_version: u8, parameter_number: u8) -> Result<Parameter> {
    // WMO GRIB1 Table 2 - Parameters (for table version 3)
    // Most common parameters from NCEP/NCAR tables
    if table_version == 3 {
        match parameter_number {
            1 => Ok(Parameter::new("PRES", "Pressure", "Pa", None, 3, 0)),
            2 => Ok(Parameter::new(
                "PRMSL",
                "Pressure reduced to MSL",
                "Pa",
                None,
                3,
                1,
            )),
            7 => Ok(Parameter::new(
                "HGT",
                "Geopotential height",
                "gpm",
                None,
                3,
                5,
            )),
            11 => Ok(Parameter::new("TMP", "Temperature", "K", None, 0, 0)),
            15 => Ok(Parameter::new(
                "TMAX",
                "Maximum temperature",
                "K",
                None,
                0,
                4,
            )),
            16 => Ok(Parameter::new(
                "TMIN",
                "Minimum temperature",
                "K",
                None,
                0,
                5,
            )),
            17 => Ok(Parameter::new(
                "DPT",
                "Dew point temperature",
                "K",
                None,
                0,
                6,
            )),
            33 => Ok(Parameter::new(
                "UGRD",
                "U component of wind",
                "m/s",
                None,
                2,
                2,
            )),
            34 => Ok(Parameter::new(
                "VGRD",
                "V component of wind",
                "m/s",
                None,
                2,
                3,
            )),
            39 => Ok(Parameter::new(
                "DZDT",
                "Vertical velocity",
                "m/s",
                None,
                2,
                9,
            )),
            51 => Ok(Parameter::new(
                "SPFH",
                "Specific humidity",
                "kg/kg",
                None,
                1,
                0,
            )),
            52 => Ok(Parameter::new("RH", "Relative humidity", "%", None, 1, 1)),
            59 => Ok(Parameter::new(
                "PRATE",
                "Precipitation rate",
                "kg/m^2/s",
                None,
                1,
                7,
            )),
            61 => Ok(Parameter::new(
                "APCP",
                "Total precipitation",
                "kg/m^2",
                None,
                1,
                8,
            )),
            63 => Ok(Parameter::new(
                "ACPCP",
                "Convective precipitation",
                "kg/m^2",
                None,
                1,
                10,
            )),
            65 => Ok(Parameter::new(
                "WEASD",
                "Water equiv. of accum. snow depth",
                "kg/m^2",
                None,
                1,
                13,
            )),
            71 => Ok(Parameter::new("TCDC", "Total cloud cover", "%", None, 6, 1)),
            _ => Err(GribError::InvalidParameter {
                discipline: 0,
                category: 255,
                number: parameter_number,
            }),
        }
    } else {
        // Other table versions not fully implemented
        Err(GribError::parse(format!(
            "Unsupported parameter table version: {}",
            table_version
        )))
    }
}

/// Level type definitions for GRIB
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelType {
    /// Surface (ground or water surface)
    Surface,
    /// Isobaric surface (pressure level in Pa)
    Isobaric,
    /// Mean sea level
    MeanSeaLevel,
    /// Specified height above ground (m)
    HeightAboveGround,
    /// Sigma level
    Sigma,
    /// Hybrid level
    Hybrid,
    /// Depth below land surface (m)
    DepthBelowLand,
    /// Isentropic level (K)
    Isentropic,
    /// Entire atmosphere (single layer)
    EntireAtmosphere,
    /// Unknown or unsupported level type
    Unknown(u8),
}

impl LevelType {
    /// Create level type from GRIB2 fixed surface type code
    pub fn from_grib2_code(code: u8) -> Self {
        match code {
            1 => Self::Surface,
            100 => Self::Isobaric,
            101 => Self::MeanSeaLevel,
            103 => Self::HeightAboveGround,
            104 => Self::Sigma,
            105 => Self::Hybrid,
            106 => Self::DepthBelowLand,
            107 => Self::Isentropic,
            200 => Self::EntireAtmosphere,
            _ => Self::Unknown(code),
        }
    }

    /// Create level type from GRIB1 level type code
    pub fn from_grib1_code(code: u8) -> Self {
        match code {
            1 => Self::Surface,
            100 => Self::Isobaric,
            102 => Self::MeanSeaLevel,
            105 => Self::HeightAboveGround,
            107 => Self::Sigma,
            109 => Self::Hybrid,
            111 => Self::DepthBelowLand,
            113 => Self::Isentropic,
            200 => Self::EntireAtmosphere,
            _ => Self::Unknown(code),
        }
    }

    /// Get human-readable description
    pub fn description(&self) -> &'static str {
        match self {
            Self::Surface => "Surface",
            Self::Isobaric => "Isobaric (pressure level)",
            Self::MeanSeaLevel => "Mean sea level",
            Self::HeightAboveGround => "Height above ground",
            Self::Sigma => "Sigma level",
            Self::Hybrid => "Hybrid level",
            Self::DepthBelowLand => "Depth below land surface",
            Self::Isentropic => "Isentropic (potential temperature)",
            Self::EntireAtmosphere => "Entire atmosphere",
            Self::Unknown(_) => "Unknown level type",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grib2_temperature_lookup() {
        let param = lookup_grib2_parameter(0, 0, 0).expect("Temperature lookup failed");
        assert_eq!(param.short_name, "TMP");
        assert_eq!(param.units, "K");
        assert_eq!(param.discipline, Some(0));
    }

    #[test]
    fn test_grib2_wind_lookup() {
        let u_wind = lookup_grib2_parameter(0, 2, 2).expect("U-wind lookup failed");
        assert_eq!(u_wind.short_name, "UGRD");
        assert_eq!(u_wind.units, "m/s");

        let v_wind = lookup_grib2_parameter(0, 2, 3).expect("V-wind lookup failed");
        assert_eq!(v_wind.short_name, "VGRD");
    }

    #[test]
    fn test_grib2_invalid_parameter() {
        let result = lookup_grib2_parameter(0, 99, 99);
        assert!(result.is_err());
    }

    #[test]
    fn test_grib1_parameter_lookup() {
        let temp = lookup_grib1_parameter(3, 11).expect("Temperature lookup failed");
        assert_eq!(temp.short_name, "TMP");
        assert_eq!(temp.units, "K");
    }

    #[test]
    fn test_level_type_grib2() {
        assert_eq!(LevelType::from_grib2_code(1), LevelType::Surface);
        assert_eq!(LevelType::from_grib2_code(100), LevelType::Isobaric);
        assert_eq!(
            LevelType::from_grib2_code(103),
            LevelType::HeightAboveGround
        );
    }

    #[test]
    fn test_level_type_description() {
        assert_eq!(LevelType::Surface.description(), "Surface");
        assert_eq!(
            LevelType::Isobaric.description(),
            "Isobaric (pressure level)"
        );
    }
}
