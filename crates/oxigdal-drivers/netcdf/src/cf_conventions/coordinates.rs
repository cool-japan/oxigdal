//! CF Conventions Coordinate System Support
//!
//! This module handles coordinate variable detection, axis identification,
//! and grid mapping support.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::dimension::Dimensions;
use crate::error::{NetCdfError, Result};
use crate::variable::Variable;

// ============================================================================
// Axis Type
// ============================================================================

/// Axis type for coordinate variables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AxisType {
    /// X (longitude-like)
    X,
    /// Y (latitude-like)
    Y,
    /// Z (vertical)
    Z,
    /// T (time)
    T,
}

impl AxisType {
    /// Get the CF axis attribute value.
    #[must_use]
    pub const fn cf_value(&self) -> &'static str {
        match self {
            Self::X => "X",
            Self::Y => "Y",
            Self::Z => "Z",
            Self::T => "T",
        }
    }

    /// Parse from axis attribute value.
    #[must_use]
    pub fn from_cf_value(value: &str) -> Option<Self> {
        match value.to_uppercase().as_str() {
            "X" => Some(Self::X),
            "Y" => Some(Self::Y),
            "Z" => Some(Self::Z),
            "T" => Some(Self::T),
            _ => None,
        }
    }
}

// ============================================================================
// Coordinate Variable Detection
// ============================================================================

/// Coordinate variable detection and classification.
#[derive(Debug, Clone)]
pub struct CoordinateDetector {
    /// Known coordinate standard names
    coordinate_names: HashSet<String>,
    /// Units indicating latitude
    latitude_units: HashSet<String>,
    /// Units indicating longitude
    longitude_units: HashSet<String>,
    /// Units indicating time
    time_units_prefixes: Vec<String>,
    /// Units indicating vertical
    vertical_units: HashSet<String>,
}

impl Default for CoordinateDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl CoordinateDetector {
    /// Create a new coordinate detector.
    #[must_use]
    pub fn new() -> Self {
        let mut detector = Self {
            coordinate_names: HashSet::new(),
            latitude_units: HashSet::new(),
            longitude_units: HashSet::new(),
            time_units_prefixes: Vec::new(),
            vertical_units: HashSet::new(),
        };
        detector.initialize();
        detector
    }

    fn initialize(&mut self) {
        // Coordinate standard names
        let coord_names = [
            "latitude",
            "longitude",
            "time",
            "depth",
            "height",
            "altitude",
            "air_pressure",
            "atmosphere_sigma_coordinate",
        ];
        for name in coord_names {
            self.coordinate_names.insert(name.to_string());
        }

        // Latitude units
        let lat_units = ["degrees_north", "degree_north", "degrees_N", "degree_N"];
        for unit in lat_units {
            self.latitude_units.insert(unit.to_string());
        }

        // Longitude units
        let lon_units = ["degrees_east", "degree_east", "degrees_E", "degree_E"];
        for unit in lon_units {
            self.longitude_units.insert(unit.to_string());
        }

        // Time unit prefixes
        self.time_units_prefixes = vec![
            "seconds since".to_string(),
            "minutes since".to_string(),
            "hours since".to_string(),
            "days since".to_string(),
        ];

        // Vertical units
        let vert_units = [
            "m",
            "km",
            "Pa",
            "hPa",
            "mbar",
            "bar",
            "dbar",
            "meter",
            "meters",
            "level",
            "sigma_level",
        ];
        for unit in vert_units {
            self.vertical_units.insert(unit.to_string());
        }
    }

    /// Detect the axis type from variable attributes.
    #[must_use]
    pub fn detect_axis(&self, var: &Variable) -> Option<AxisType> {
        // Check explicit axis attribute
        if let Some(attr) = var.attributes().get("axis") {
            if let Ok(axis_val) = attr.value().as_text() {
                if let Some(axis) = AxisType::from_cf_value(axis_val) {
                    return Some(axis);
                }
            }
        }

        // Check standard_name
        if let Some(attr) = var.attributes().get("standard_name") {
            if let Ok(name) = attr.value().as_text() {
                match name {
                    "latitude" => return Some(AxisType::Y),
                    "longitude" => return Some(AxisType::X),
                    "time" => return Some(AxisType::T),
                    "depth" | "height" | "altitude" | "air_pressure" => return Some(AxisType::Z),
                    _ => {}
                }
            }
        }

        // Check units
        if let Some(attr) = var.attributes().get("units") {
            if let Ok(units) = attr.value().as_text() {
                if self.latitude_units.contains(units) {
                    return Some(AxisType::Y);
                }
                if self.longitude_units.contains(units) {
                    return Some(AxisType::X);
                }
                for prefix in &self.time_units_prefixes {
                    if units.starts_with(prefix) {
                        return Some(AxisType::T);
                    }
                }
                if self.vertical_units.contains(units) {
                    return Some(AxisType::Z);
                }
            }
        }

        // Check positive attribute (vertical)
        if var.attributes().get("positive").is_some() {
            return Some(AxisType::Z);
        }

        None
    }

    /// Check if a variable is a coordinate variable.
    #[must_use]
    pub fn is_coordinate_variable(&self, var: &Variable, dimensions: &Dimensions) -> bool {
        // Classic definition: 1D variable with same name as its dimension
        if var.dimension_names().len() == 1 {
            if let Some(dim_name) = var.dimension_names().first() {
                if dim_name == var.name() && dimensions.contains(dim_name) {
                    return true;
                }
            }
        }

        // Also check for coordinate standard names
        if let Some(attr) = var.attributes().get("standard_name") {
            if let Ok(name) = attr.value().as_text() {
                if self.coordinate_names.contains(name) {
                    return true;
                }
            }
        }

        false
    }
}

// ============================================================================
// Grid Mapping Support
// ============================================================================

/// Supported grid mapping types in CF conventions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GridMappingType {
    /// Latitude-longitude (equirectangular)
    LatitudeLongitude,
    /// Rotated latitude-longitude
    RotatedLatitudeLongitude,
    /// Stereographic (polar)
    Stereographic,
    /// Polar stereographic
    PolarStereographic,
    /// Lambert conformal conic
    LambertConformalConic,
    /// Lambert azimuthal equal area
    LambertAzimuthalEqualArea,
    /// Albers equal area conic
    AlbersConicEqualArea,
    /// Transverse Mercator
    TransverseMercator,
    /// Universal Transverse Mercator
    Utm,
    /// Mercator
    Mercator,
    /// Sinusoidal
    Sinusoidal,
    /// Geostationary satellite view
    GeostationarySatellite,
    /// Vertical perspective
    VerticalPerspective,
    /// Unknown/unsupported
    Unknown,
}

impl GridMappingType {
    /// Get the CF grid mapping name.
    #[must_use]
    pub const fn cf_name(&self) -> &'static str {
        match self {
            Self::LatitudeLongitude => "latitude_longitude",
            Self::RotatedLatitudeLongitude => "rotated_latitude_longitude",
            Self::Stereographic => "stereographic",
            Self::PolarStereographic => "polar_stereographic",
            Self::LambertConformalConic => "lambert_conformal_conic",
            Self::LambertAzimuthalEqualArea => "lambert_azimuthal_equal_area",
            Self::AlbersConicEqualArea => "albers_conical_equal_area",
            Self::TransverseMercator => "transverse_mercator",
            Self::Utm => "universal_transverse_mercator",
            Self::Mercator => "mercator",
            Self::Sinusoidal => "sinusoidal",
            Self::GeostationarySatellite => "geostationary",
            Self::VerticalPerspective => "vertical_perspective",
            Self::Unknown => "unknown",
        }
    }

    /// Parse from CF grid mapping name.
    #[must_use]
    pub fn from_cf_name(name: &str) -> Self {
        match name {
            "latitude_longitude" => Self::LatitudeLongitude,
            "rotated_latitude_longitude" => Self::RotatedLatitudeLongitude,
            "stereographic" => Self::Stereographic,
            "polar_stereographic" => Self::PolarStereographic,
            "lambert_conformal_conic" => Self::LambertConformalConic,
            "lambert_azimuthal_equal_area" => Self::LambertAzimuthalEqualArea,
            "albers_conical_equal_area" => Self::AlbersConicEqualArea,
            "transverse_mercator" => Self::TransverseMercator,
            "universal_transverse_mercator" => Self::Utm,
            "mercator" => Self::Mercator,
            "sinusoidal" => Self::Sinusoidal,
            "geostationary" => Self::GeostationarySatellite,
            "vertical_perspective" => Self::VerticalPerspective,
            _ => Self::Unknown,
        }
    }

    /// Get required attributes for this grid mapping.
    #[must_use]
    pub fn required_attributes(&self) -> &'static [&'static str] {
        match self {
            Self::LatitudeLongitude => &[],
            Self::RotatedLatitudeLongitude => {
                &["grid_north_pole_latitude", "grid_north_pole_longitude"]
            }
            Self::Stereographic => &[
                "latitude_of_projection_origin",
                "longitude_of_projection_origin",
            ],
            Self::PolarStereographic => &[
                "straight_vertical_longitude_from_pole",
                "latitude_of_projection_origin",
            ],
            Self::LambertConformalConic => &[
                "standard_parallel",
                "longitude_of_central_meridian",
                "latitude_of_projection_origin",
            ],
            Self::LambertAzimuthalEqualArea => &[
                "longitude_of_projection_origin",
                "latitude_of_projection_origin",
            ],
            Self::AlbersConicEqualArea => &[
                "standard_parallel",
                "longitude_of_central_meridian",
                "latitude_of_projection_origin",
            ],
            Self::TransverseMercator => &[
                "scale_factor_at_central_meridian",
                "longitude_of_central_meridian",
                "latitude_of_projection_origin",
            ],
            Self::Utm => &["utm_zone_number"],
            Self::Mercator => &["longitude_of_projection_origin"],
            Self::Sinusoidal => &["longitude_of_central_meridian"],
            Self::GeostationarySatellite => {
                &["longitude_of_projection_origin", "perspective_point_height"]
            }
            Self::VerticalPerspective => &[
                "latitude_of_projection_origin",
                "longitude_of_projection_origin",
                "perspective_point_height",
            ],
            Self::Unknown => &[],
        }
    }
}

/// Grid mapping definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridMapping {
    /// Variable name holding the grid mapping
    pub name: String,
    /// Grid mapping type
    pub mapping_type: GridMappingType,
    /// Projection parameters
    pub parameters: HashMap<String, f64>,
    /// CRS (WKT or EPSG code if available)
    pub crs_wkt: Option<String>,
}

impl GridMapping {
    /// Create a new grid mapping.
    #[must_use]
    pub fn new(name: impl Into<String>, mapping_type: GridMappingType) -> Self {
        Self {
            name: name.into(),
            mapping_type,
            parameters: HashMap::new(),
            crs_wkt: None,
        }
    }

    /// Set a parameter.
    pub fn set_parameter(&mut self, name: impl Into<String>, value: f64) {
        self.parameters.insert(name.into(), value);
    }

    /// Get a parameter.
    #[must_use]
    pub fn get_parameter(&self, name: &str) -> Option<f64> {
        self.parameters.get(name).copied()
    }

    /// Validate the grid mapping.
    pub fn validate(&self) -> Result<()> {
        let required = self.mapping_type.required_attributes();
        for attr in required {
            if !self.parameters.contains_key(*attr) {
                return Err(NetCdfError::CfConventionsError(format!(
                    "Grid mapping '{}' missing required attribute '{}'",
                    self.name, attr
                )));
            }
        }
        Ok(())
    }
}
