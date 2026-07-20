//! CF Conventions Metadata Validation
//!
//! This module handles metadata validation including standard names, units,
//! cell methods, and cell measures.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::error::{NetCdfError, Result};

// ============================================================================
// Standard Name Table
// ============================================================================

/// CF Standard Name entry with canonical units.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StandardNameEntry {
    /// The standard name
    pub name: String,
    /// Canonical units for this standard name
    pub canonical_units: String,
    /// Description of the quantity
    pub description: String,
}

/// CF Standard Name Table.
#[derive(Debug, Clone, Default)]
pub struct StandardNameTable {
    entries: HashMap<String, StandardNameEntry>,
}

impl StandardNameTable {
    /// Create a new empty standard name table.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Create the CF 1.8 standard name table with common entries.
    #[must_use]
    pub fn cf_1_8() -> Self {
        let mut table = Self::new();
        table.add_cf_standard_names();
        table
    }

    /// Add an entry to the table.
    pub fn add(&mut self, entry: StandardNameEntry) {
        self.entries.insert(entry.name.clone(), entry);
    }

    /// Get an entry by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&StandardNameEntry> {
        self.entries.get(name)
    }

    /// Check if a standard name exists.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    /// Get the canonical units for a standard name.
    #[must_use]
    pub fn canonical_units(&self, name: &str) -> Option<&str> {
        self.entries.get(name).map(|e| e.canonical_units.as_str())
    }

    /// Add common CF standard names (subset of the full table).
    fn add_cf_standard_names(&mut self) {
        // Atmospheric variables
        self.add_entry("air_temperature", "K", "Air temperature");
        self.add_entry("air_pressure", "Pa", "Air pressure");
        self.add_entry(
            "air_pressure_at_sea_level",
            "Pa",
            "Air pressure at sea level",
        );
        self.add_entry("surface_air_pressure", "Pa", "Surface air pressure");
        self.add_entry("relative_humidity", "1", "Relative humidity");
        self.add_entry("specific_humidity", "1", "Specific humidity");
        self.add_entry("dew_point_temperature", "K", "Dew point temperature");
        self.add_entry("air_density", "kg m-3", "Air density");
        self.add_entry("geopotential_height", "m", "Geopotential height");
        self.add_entry("altitude", "m", "Altitude above mean sea level");

        // Wind variables
        self.add_entry("wind_speed", "m s-1", "Wind speed");
        self.add_entry("wind_from_direction", "degree", "Wind from direction");
        self.add_entry("eastward_wind", "m s-1", "Eastward wind component");
        self.add_entry("northward_wind", "m s-1", "Northward wind component");
        self.add_entry("upward_air_velocity", "m s-1", "Upward air velocity");

        // Precipitation and clouds
        self.add_entry("precipitation_amount", "kg m-2", "Precipitation amount");
        self.add_entry("precipitation_flux", "kg m-2 s-1", "Precipitation flux");
        self.add_entry("rainfall_rate", "kg m-2 s-1", "Rainfall rate");
        self.add_entry("snowfall_amount", "kg m-2", "Snowfall amount");
        self.add_entry("cloud_area_fraction", "1", "Cloud area fraction");
        self.add_entry("cloud_base_altitude", "m", "Cloud base altitude");
        self.add_entry("cloud_top_altitude", "m", "Cloud top altitude");

        // Radiation
        self.add_entry(
            "surface_downwelling_shortwave_flux",
            "W m-2",
            "Surface downwelling shortwave flux",
        );
        self.add_entry(
            "surface_upwelling_shortwave_flux",
            "W m-2",
            "Surface upwelling shortwave flux",
        );
        self.add_entry(
            "surface_downwelling_longwave_flux",
            "W m-2",
            "Surface downwelling longwave flux",
        );
        self.add_entry(
            "surface_upwelling_longwave_flux",
            "W m-2",
            "Surface upwelling longwave flux",
        );
        self.add_entry(
            "toa_incoming_shortwave_flux",
            "W m-2",
            "TOA incoming shortwave flux",
        );
        self.add_entry(
            "toa_outgoing_shortwave_flux",
            "W m-2",
            "TOA outgoing shortwave flux",
        );
        self.add_entry(
            "toa_outgoing_longwave_flux",
            "W m-2",
            "TOA outgoing longwave flux",
        );

        // Ocean variables
        self.add_entry("sea_surface_temperature", "K", "Sea surface temperature");
        self.add_entry("sea_water_temperature", "K", "Sea water temperature");
        self.add_entry("sea_water_salinity", "1e-3", "Sea water salinity (PSU)");
        self.add_entry(
            "sea_surface_height_above_geoid",
            "m",
            "Sea surface height above geoid",
        );
        self.add_entry("sea_water_pressure", "dbar", "Sea water pressure");
        self.add_entry("sea_water_density", "kg m-3", "Sea water density");
        self.add_entry(
            "eastward_sea_water_velocity",
            "m s-1",
            "Eastward sea water velocity",
        );
        self.add_entry(
            "northward_sea_water_velocity",
            "m s-1",
            "Northward sea water velocity",
        );
        self.add_entry(
            "upward_sea_water_velocity",
            "m s-1",
            "Upward sea water velocity",
        );
        self.add_entry(
            "ocean_mixed_layer_thickness",
            "m",
            "Ocean mixed layer thickness",
        );

        // Land surface
        self.add_entry("surface_temperature", "K", "Surface temperature");
        self.add_entry("soil_temperature", "K", "Soil temperature");
        self.add_entry("soil_moisture_content", "kg m-2", "Soil moisture content");
        self.add_entry("surface_snow_amount", "kg m-2", "Surface snow amount");
        self.add_entry("surface_snow_thickness", "m", "Surface snow thickness");
        self.add_entry("vegetation_area_fraction", "1", "Vegetation area fraction");
        self.add_entry("leaf_area_index", "1", "Leaf area index");

        // Coordinate variables
        self.add_entry("time", "s", "Time");
        self.add_entry("latitude", "degrees_north", "Latitude");
        self.add_entry("longitude", "degrees_east", "Longitude");
        self.add_entry("depth", "m", "Depth below surface");
        self.add_entry("height", "m", "Height above surface");
        self.add_entry("air_pressure", "Pa", "Air pressure coordinate");
        self.add_entry(
            "atmosphere_sigma_coordinate",
            "1",
            "Atmosphere sigma coordinate",
        );
        self.add_entry(
            "atmosphere_hybrid_sigma_pressure_coordinate",
            "1",
            "Atmosphere hybrid coordinate",
        );

        // Trace gases
        self.add_entry(
            "mole_fraction_of_carbon_dioxide_in_air",
            "1e-6",
            "CO2 mole fraction",
        );
        self.add_entry(
            "mole_fraction_of_methane_in_air",
            "1e-9",
            "Methane mole fraction",
        );
        self.add_entry(
            "mole_fraction_of_ozone_in_air",
            "1e-9",
            "Ozone mole fraction",
        );
        self.add_entry(
            "mass_concentration_of_pm2p5_ambient_aerosol_in_air",
            "kg m-3",
            "PM2.5 concentration",
        );
        self.add_entry(
            "mass_concentration_of_pm10_ambient_aerosol_in_air",
            "kg m-3",
            "PM10 concentration",
        );
    }

    /// Helper to add a standard name entry.
    fn add_entry(&mut self, name: &str, units: &str, description: &str) {
        self.add(StandardNameEntry {
            name: name.to_string(),
            canonical_units: units.to_string(),
            description: description.to_string(),
        });
    }
}

// ============================================================================
// Units Validation
// ============================================================================

/// Units validator for CF conventions.
#[derive(Debug, Clone, Default)]
pub struct UnitsValidator {
    /// Known valid unit strings
    valid_units: HashSet<String>,
    /// Unit prefixes (SI)
    prefixes: HashMap<String, f64>,
    /// Base units
    base_units: HashMap<String, String>,
}

impl UnitsValidator {
    /// Create a new units validator with CF standard units.
    #[must_use]
    pub fn new() -> Self {
        let mut validator = Self {
            valid_units: HashSet::new(),
            prefixes: HashMap::new(),
            base_units: HashMap::new(),
        };
        validator.initialize_units();
        validator
    }

    /// Initialize standard units.
    fn initialize_units(&mut self) {
        // SI prefixes
        self.prefixes.insert("Y".to_string(), 1e24);
        self.prefixes.insert("Z".to_string(), 1e21);
        self.prefixes.insert("E".to_string(), 1e18);
        self.prefixes.insert("P".to_string(), 1e15);
        self.prefixes.insert("T".to_string(), 1e12);
        self.prefixes.insert("G".to_string(), 1e9);
        self.prefixes.insert("M".to_string(), 1e6);
        self.prefixes.insert("k".to_string(), 1e3);
        self.prefixes.insert("h".to_string(), 1e2);
        self.prefixes.insert("da".to_string(), 1e1);
        self.prefixes.insert("d".to_string(), 1e-1);
        self.prefixes.insert("c".to_string(), 1e-2);
        self.prefixes.insert("m".to_string(), 1e-3);
        self.prefixes.insert("u".to_string(), 1e-6);
        self.prefixes.insert("n".to_string(), 1e-9);
        self.prefixes.insert("p".to_string(), 1e-12);
        self.prefixes.insert("f".to_string(), 1e-15);
        self.prefixes.insert("a".to_string(), 1e-18);

        // Base units
        let base = [
            ("m", "meter"),
            ("s", "second"),
            ("kg", "kilogram"),
            ("K", "kelvin"),
            ("A", "ampere"),
            ("mol", "mole"),
            ("cd", "candela"),
            ("rad", "radian"),
            ("sr", "steradian"),
        ];
        for (abbr, full) in base {
            self.base_units.insert(abbr.to_string(), full.to_string());
            self.valid_units.insert(abbr.to_string());
            self.valid_units.insert(full.to_string());
        }

        // Derived units
        let derived = [
            "Hz", "N", "Pa", "J", "W", "C", "V", "F", "ohm", "S", "Wb", "T", "H", "lm", "lx", "Bq",
            "Gy", "Sv",
        ];
        for unit in derived {
            self.valid_units.insert(unit.to_string());
        }

        // Common non-SI units accepted in CF
        let accepted = [
            "1",
            "percent",
            "%",
            "ppm",
            "ppb",
            "ppt",
            "degree",
            "degrees",
            "degree_north",
            "degrees_north",
            "degree_east",
            "degrees_east",
            "degree_true",
            "degrees_true",
            "bar",
            "mbar",
            "millibar",
            "atm",
            "atmosphere",
            "minute",
            "min",
            "hour",
            "h",
            "day",
            "d",
            "year",
            "yr",
            "a",
            "dbar",
            "decibar",
            "celsius",
            "degC",
            "degree_Celsius",
            "degrees_Celsius",
            "PSU",
            "psu",
            "kg m-2",
            "kg m-2 s-1",
            "W m-2",
            "m s-1",
            "m2 s-1",
            "kg m-3",
            "mm",
            "cm",
            "km",
            "g",
            "mg",
            "ug",
        ];
        for unit in accepted {
            self.valid_units.insert(unit.to_string());
        }
    }

    /// Validate a units string.
    #[must_use]
    pub fn is_valid(&self, units: &str) -> bool {
        if units.is_empty() {
            return false;
        }

        // Dimensionless
        if units == "1" || units.is_empty() {
            return true;
        }

        // Check if it's a known unit
        if self.valid_units.contains(units) {
            return true;
        }

        // Try to parse compound units
        self.parse_compound_units(units).is_some()
    }

    /// Parse compound units (e.g., "kg m-2 s-1").
    fn parse_compound_units(&self, units: &str) -> Option<Vec<(String, i32)>> {
        let parts: Vec<&str> = units.split_whitespace().collect();
        let mut parsed = Vec::new();

        for part in parts {
            // Handle negative exponents (e.g., "m-2")
            if let Some(idx) = part.find('-') {
                let (base, exp_str) = part.split_at(idx);
                let exp: i32 = exp_str.parse().ok()?;
                if self.is_base_unit(base) {
                    parsed.push((base.to_string(), exp));
                } else {
                    return None;
                }
            } else if let Some(idx) = part.find(|c: char| c.is_ascii_digit()) {
                let (base, exp_str) = part.split_at(idx);
                let exp: i32 = exp_str.parse().ok()?;
                if self.is_base_unit(base) {
                    parsed.push((base.to_string(), exp));
                } else {
                    return None;
                }
            } else if self.is_base_unit(part) {
                parsed.push((part.to_string(), 1));
            } else {
                return None;
            }
        }

        if parsed.is_empty() {
            None
        } else {
            Some(parsed)
        }
    }

    /// Check if a string is a base unit (with optional prefix).
    fn is_base_unit(&self, unit: &str) -> bool {
        if self.valid_units.contains(unit) {
            return true;
        }

        // Check with prefix
        for prefix in self.prefixes.keys() {
            if unit.starts_with(prefix) {
                let base = &unit[prefix.len()..];
                if self.base_units.contains_key(base) || self.valid_units.contains(base) {
                    return true;
                }
            }
        }

        false
    }

    /// Check if units are compatible with canonical units.
    #[must_use]
    pub fn are_compatible(&self, units: &str, canonical: &str) -> bool {
        if units == canonical {
            return true;
        }

        // Temperature special cases
        if (units == "K" || units == "kelvin") && (canonical == "K" || canonical == "kelvin") {
            return true;
        }
        if (units == "celsius" || units == "degC" || units == "degree_Celsius")
            && (canonical == "K" || canonical == "kelvin")
        {
            return true;
        }

        // Latitude/longitude
        if units == "degrees_north" && canonical == "degrees_north" {
            return true;
        }
        if units == "degrees_east" && canonical == "degrees_east" {
            return true;
        }

        // Dimensionless
        if (units == "1" || units.is_empty()) && (canonical == "1" || canonical.is_empty()) {
            return true;
        }

        // Same base, different prefix (simplified check)
        self.have_same_dimension(units, canonical)
    }

    /// Check if two units have the same dimension.
    fn have_same_dimension(&self, units1: &str, units2: &str) -> bool {
        // Simplified check - just compare normalized forms
        let norm1 = self.normalize_units(units1);
        let norm2 = self.normalize_units(units2);
        norm1 == norm2
    }

    /// Normalize units string.
    fn normalize_units(&self, units: &str) -> String {
        units
            .replace("meter", "m")
            .replace("second", "s")
            .replace("kilogram", "kg")
            .replace("kelvin", "K")
            .to_lowercase()
    }
}

// ============================================================================
// Cell Methods
// ============================================================================

/// Cell method operation types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CellMethodOperation {
    /// Point value (no processing)
    Point,
    /// Sum over cell
    Sum,
    /// Mean/average over cell
    Mean,
    /// Maximum over cell
    Maximum,
    /// Minimum over cell
    Minimum,
    /// Median over cell
    Median,
    /// Mode over cell
    Mode,
    /// Standard deviation
    StandardDeviation,
    /// Variance
    Variance,
    /// Range (max - min)
    Range,
    /// Mid-range ((max + min) / 2)
    MidRange,
}

impl CellMethodOperation {
    /// Get the CF string representation.
    #[must_use]
    pub const fn cf_string(&self) -> &'static str {
        match self {
            Self::Point => "point",
            Self::Sum => "sum",
            Self::Mean => "mean",
            Self::Maximum => "maximum",
            Self::Minimum => "minimum",
            Self::Median => "median",
            Self::Mode => "mode",
            Self::StandardDeviation => "standard_deviation",
            Self::Variance => "variance",
            Self::Range => "range",
            Self::MidRange => "mid_range",
        }
    }

    /// Parse from CF string.
    pub fn from_cf_string(s: &str) -> Option<Self> {
        match s {
            "point" => Some(Self::Point),
            "sum" => Some(Self::Sum),
            "mean" => Some(Self::Mean),
            "maximum" | "max" => Some(Self::Maximum),
            "minimum" | "min" => Some(Self::Minimum),
            "median" => Some(Self::Median),
            "mode" => Some(Self::Mode),
            "standard_deviation" => Some(Self::StandardDeviation),
            "variance" => Some(Self::Variance),
            "range" => Some(Self::Range),
            "mid_range" => Some(Self::MidRange),
            _ => None,
        }
    }
}

/// A parsed cell method entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellMethod {
    /// Dimension(s) this method applies to
    pub dimensions: Vec<String>,
    /// The operation performed
    pub operation: CellMethodOperation,
    /// Where clause (e.g., "where land")
    pub where_clause: Option<String>,
    /// Over clause (e.g., "over area")
    pub over_clause: Option<String>,
    /// Within clause (e.g., "within days")
    pub within_clause: Option<String>,
    /// Comment
    pub comment: Option<String>,
    /// Interval specification
    pub interval: Option<String>,
}

impl CellMethod {
    /// Create a new cell method.
    #[must_use]
    pub fn new(dimensions: Vec<String>, operation: CellMethodOperation) -> Self {
        Self {
            dimensions,
            operation,
            where_clause: None,
            over_clause: None,
            within_clause: None,
            comment: None,
            interval: None,
        }
    }

    /// Parse cell methods string (CF conventions format).
    ///
    /// Format: "dim1: method1 dim2: method2 ..."
    pub fn parse_cell_methods(cell_methods: &str) -> Result<Vec<Self>> {
        let mut methods: Vec<Self> = Vec::new();

        // Simple parser for cell_methods attribute
        // Format: "time: mean area: mean" or "time: mean (interval: 1 hour)"
        let mut current_dims: Vec<String> = Vec::new();
        let mut in_parenthesis = false;
        let mut paren_content = String::new();

        let parts: Vec<&str> = cell_methods.split_whitespace().collect();
        let mut i = 0;

        while i < parts.len() {
            let part = parts[i];

            // Handle parenthetical content
            if let Some(stripped) = part.strip_prefix('(') {
                in_parenthesis = true;
                paren_content = stripped.to_string();
                if part.ends_with(')') {
                    in_parenthesis = false;
                    paren_content =
                        paren_content[..paren_content.len().saturating_sub(1)].to_string();
                    // Add to previous method's comment/interval
                    if let Some(last) = methods.last_mut() {
                        if let Some(stripped) = paren_content.strip_prefix("interval:") {
                            last.interval = Some(stripped.trim().to_string());
                        } else if let Some(stripped) = paren_content.strip_prefix("comment:") {
                            last.comment = Some(stripped.trim().to_string());
                        }
                    }
                }
                i += 1;
                continue;
            }

            if in_parenthesis {
                paren_content.push(' ');
                paren_content.push_str(part);
                if part.ends_with(')') {
                    in_parenthesis = false;
                    paren_content =
                        paren_content[..paren_content.len().saturating_sub(1)].to_string();
                    if let Some(last) = methods.last_mut() {
                        if let Some(stripped) = paren_content.strip_prefix("interval:") {
                            last.interval = Some(stripped.trim().to_string());
                        } else if let Some(stripped) = paren_content.strip_prefix("comment:") {
                            last.comment = Some(stripped.trim().to_string());
                        }
                    }
                }
                i += 1;
                continue;
            }

            // Check if this is a dimension (ends with :)
            if let Some(dim) = part.strip_suffix(':') {
                current_dims.push(dim.to_string());
                i += 1;
            } else if !current_dims.is_empty() {
                // This should be a method
                if let Some(op) = CellMethodOperation::from_cf_string(part) {
                    let method = CellMethod::new(current_dims.clone(), op);
                    methods.push(method);
                    current_dims.clear();
                } else if part == "where" || part == "over" || part == "within" {
                    // Handle where/over/within clauses
                    if let Some(last) = methods.last_mut() {
                        i += 1;
                        if i < parts.len() {
                            match part {
                                "where" => last.where_clause = Some(parts[i].to_string()),
                                "over" => last.over_clause = Some(parts[i].to_string()),
                                "within" => last.within_clause = Some(parts[i].to_string()),
                                _ => {}
                            }
                        }
                    }
                    i += 1;
                    continue;
                }
                i += 1;
            } else {
                // Unknown token, skip
                i += 1;
            }
        }

        Ok(methods)
    }

    /// Convert to CF string format.
    #[must_use]
    pub fn to_cf_string(&self) -> String {
        let dims = self.dimensions.join(": ");
        let mut result = format!("{}: {}", dims, self.operation.cf_string());

        if let Some(ref where_clause) = self.where_clause {
            result.push_str(&format!(" where {}", where_clause));
        }
        if let Some(ref over_clause) = self.over_clause {
            result.push_str(&format!(" over {}", over_clause));
        }
        if let Some(ref within_clause) = self.within_clause {
            result.push_str(&format!(" within {}", within_clause));
        }
        if let Some(ref interval) = self.interval {
            result.push_str(&format!(" (interval: {})", interval));
        }
        if let Some(ref comment) = self.comment {
            result.push_str(&format!(" (comment: {})", comment));
        }

        result
    }
}

// ============================================================================
// Cell Measures
// ============================================================================

/// Cell measure type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CellMeasureType {
    /// Area of the cell
    Area,
    /// Volume of the cell
    Volume,
}

impl CellMeasureType {
    /// Get the CF string.
    #[must_use]
    pub const fn cf_string(&self) -> &'static str {
        match self {
            Self::Area => "area",
            Self::Volume => "volume",
        }
    }

    /// Parse from CF string.
    #[must_use]
    pub fn from_cf_string(s: &str) -> Option<Self> {
        match s {
            "area" => Some(Self::Area),
            "volume" => Some(Self::Volume),
            _ => None,
        }
    }
}

/// Cell measure definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellMeasure {
    /// Type of measure
    pub measure_type: CellMeasureType,
    /// Variable name containing the measure
    pub variable_name: String,
}

impl CellMeasure {
    /// Create a new cell measure.
    #[must_use]
    pub fn new(measure_type: CellMeasureType, variable_name: impl Into<String>) -> Self {
        Self {
            measure_type,
            variable_name: variable_name.into(),
        }
    }

    /// Parse cell_measures attribute.
    ///
    /// Format: "area: cell_area volume: cell_volume"
    pub fn parse_cell_measures(cell_measures: &str) -> Result<Vec<Self>> {
        let mut measures = Vec::new();
        let parts: Vec<&str> = cell_measures.split_whitespace().collect();

        let mut i = 0;
        while i < parts.len() {
            if parts[i].ends_with(':') {
                let measure_type_str = &parts[i][..parts[i].len() - 1];
                if let Some(measure_type) = CellMeasureType::from_cf_string(measure_type_str) {
                    i += 1;
                    if i < parts.len() {
                        measures.push(CellMeasure::new(measure_type, parts[i]));
                    }
                }
            }
            i += 1;
        }

        Ok(measures)
    }
}
