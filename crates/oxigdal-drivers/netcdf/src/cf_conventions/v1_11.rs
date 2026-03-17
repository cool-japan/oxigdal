//! CF Conventions v1.11 additions and compliance helpers
//!
//! This module extends the existing CF conventions support with types and
//! functions specific to CF-1.11 and generally useful version-aware parsing.
//!
//! Reference: <https://cfconventions.org/cf-conventions/cf-conventions.html>

use std::fmt;

// ---------------------------------------------------------------------------
// CfVersion
// ---------------------------------------------------------------------------

/// A parsed CF Conventions version number (e.g. `CF-1.11` → major=1, minor=11).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CfVersion {
    /// Major version number.
    pub major: u8,
    /// Minor version number.
    pub minor: u8,
}

impl CfVersion {
    /// The CF Conventions v1.11 constant.
    #[must_use]
    pub const fn v1_11() -> Self {
        Self {
            major: 1,
            minor: 11,
        }
    }

    /// The CF Conventions v1.8 constant.
    #[must_use]
    pub const fn v1_8() -> Self {
        Self { major: 1, minor: 8 }
    }

    /// Parse from a numeric string such as `"1.11"` or `"1.8"`.
    #[must_use]
    pub fn parse_version(s: &str) -> Option<Self> {
        let mut parts = s.splitn(2, '.');
        let major: u8 = parts.next()?.parse().ok()?;
        let minor: u8 = parts.next()?.parse().ok()?;
        Some(Self { major, minor })
    }

    /// Returns true if this version is at least as new as `other`.
    #[must_use]
    pub fn is_at_least(&self, other: &Self) -> bool {
        self >= other
    }
}

impl fmt::Display for CfVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

// ---------------------------------------------------------------------------
// CfStandardName (static table entry)
// ---------------------------------------------------------------------------

/// A static entry in the CF standard name table.
#[derive(Debug, Clone, Copy)]
pub struct CfStandardName {
    /// CF standard name (e.g. `"air_temperature"`).
    pub name: &'static str,
    /// Canonical SI units string (e.g. `"K"`).
    pub canonical_units: &'static str,
    /// Short description.
    pub description: &'static str,
}

/// A representative subset of the CF standard name table covering common
/// climate, weather, and ocean variables.
pub const CF_STANDARD_NAMES: &[CfStandardName] = &[
    CfStandardName {
        name: "air_temperature",
        canonical_units: "K",
        description: "Air temperature",
    },
    CfStandardName {
        name: "air_pressure",
        canonical_units: "Pa",
        description: "Air pressure",
    },
    CfStandardName {
        name: "eastward_wind",
        canonical_units: "m s-1",
        description: "Eastward wind component",
    },
    CfStandardName {
        name: "northward_wind",
        canonical_units: "m s-1",
        description: "Northward wind component",
    },
    CfStandardName {
        name: "relative_humidity",
        canonical_units: "1",
        description: "Relative humidity",
    },
    CfStandardName {
        name: "specific_humidity",
        canonical_units: "1",
        description: "Specific humidity",
    },
    CfStandardName {
        name: "precipitation_flux",
        canonical_units: "kg m-2 s-1",
        description: "Precipitation flux",
    },
    CfStandardName {
        name: "surface_temperature",
        canonical_units: "K",
        description: "Surface temperature",
    },
    CfStandardName {
        name: "sea_surface_temperature",
        canonical_units: "K",
        description: "Sea surface temperature",
    },
    CfStandardName {
        name: "sea_surface_salinity",
        canonical_units: "1e-3",
        description: "Sea surface salinity (PSU)",
    },
    CfStandardName {
        name: "ocean_heat_content",
        canonical_units: "J m-2",
        description: "Ocean heat content",
    },
    CfStandardName {
        name: "land_area_fraction",
        canonical_units: "1",
        description: "Land area fraction",
    },
    CfStandardName {
        name: "soil_temperature",
        canonical_units: "K",
        description: "Soil temperature",
    },
    CfStandardName {
        name: "downwelling_shortwave_flux_in_air",
        canonical_units: "W m-2",
        description: "Downwelling shortwave radiation",
    },
    CfStandardName {
        name: "upwelling_longwave_flux_in_air",
        canonical_units: "W m-2",
        description: "Upwelling longwave radiation",
    },
    CfStandardName {
        name: "geopotential_height",
        canonical_units: "m",
        description: "Geopotential height",
    },
    CfStandardName {
        name: "cloud_area_fraction",
        canonical_units: "1",
        description: "Cloud area fraction",
    },
    CfStandardName {
        name: "toa_outgoing_longwave_flux",
        canonical_units: "W m-2",
        description: "TOA outgoing longwave flux",
    },
    CfStandardName {
        name: "net_primary_productivity_of_biomass_expressed_as_carbon",
        canonical_units: "kg m-2 s-1",
        description: "Net primary productivity",
    },
    CfStandardName {
        name: "mole_fraction_of_carbon_dioxide_in_air",
        canonical_units: "mol mol-1",
        description: "CO₂ mole fraction",
    },
];

/// Look up a CF standard name entry by name.
#[must_use]
pub fn lookup_standard_name(name: &str) -> Option<&'static CfStandardName> {
    CF_STANDARD_NAMES.iter().find(|n| n.name == name)
}

// ---------------------------------------------------------------------------
// CfCoordinateType
// ---------------------------------------------------------------------------

/// CF coordinate variable type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CfCoordinateType {
    /// Longitude coordinate
    Longitude,
    /// Latitude coordinate
    Latitude,
    /// Vertical coordinate
    Vertical,
    /// Time coordinate
    Time,
    /// Auxiliary coordinate (not a dimension coordinate)
    Auxiliary,
    /// Scalar (zero-dimensional) coordinate
    Scalar,
}

impl CfCoordinateType {
    /// Returns a short label string.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Longitude => "longitude",
            Self::Latitude => "latitude",
            Self::Vertical => "vertical",
            Self::Time => "time",
            Self::Auxiliary => "auxiliary",
            Self::Scalar => "scalar",
        }
    }

    /// Returns `true` for spatial coordinates (lon/lat/vertical).
    #[must_use]
    pub fn is_spatial(&self) -> bool {
        matches!(self, Self::Longitude | Self::Latitude | Self::Vertical)
    }
}

// ---------------------------------------------------------------------------
// CfGeometryType — CF v1.8+, expanded in v1.11
// ---------------------------------------------------------------------------

/// CF geometry type (CF §7.5, expanded in v1.11).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CfGeometryType {
    /// Single point
    Point,
    /// Open line string
    Line,
    /// Closed polygon
    Polygon,
    /// Collection of points
    MultiPoint,
    /// Collection of lines
    MultiLine,
    /// Collection of polygons
    MultiPolygon,
}

impl CfGeometryType {
    /// Returns the CF attribute string (lowercase).
    #[must_use]
    pub const fn cf_name(&self) -> &'static str {
        match self {
            Self::Point => "point",
            Self::Line => "line",
            Self::Polygon => "polygon",
            Self::MultiPoint => "multipoint",
            Self::MultiLine => "multiline",
            Self::MultiPolygon => "multipolygon",
        }
    }

    /// Parse from the CF attribute string (case-insensitive).
    #[must_use]
    pub fn from_cf_name(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "point" => Some(Self::Point),
            "line" => Some(Self::Line),
            "polygon" => Some(Self::Polygon),
            "multipoint" => Some(Self::MultiPoint),
            "multiline" => Some(Self::MultiLine),
            "multipolygon" => Some(Self::MultiPolygon),
            _ => None,
        }
    }

    /// Returns `true` for multi-geometry types.
    #[must_use]
    pub fn is_multi(&self) -> bool {
        matches!(
            self,
            Self::MultiPoint | Self::MultiLine | Self::MultiPolygon
        )
    }
}

// ---------------------------------------------------------------------------
// CellMethodName
// ---------------------------------------------------------------------------

/// CF cell method operation name (CF §7.3).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CellMethodName {
    /// Instantaneous value
    Point,
    /// Summation
    Sum,
    /// Maximum value
    Maximum,
    /// Median value
    Median,
    /// Midrange value
    Midrange,
    /// Minimum value
    Minimum,
    /// Mean value
    Mean,
    /// Modal (most common) value
    Mode,
    /// Standard deviation
    StandardDeviation,
    /// Variance
    Variance,
    /// Unknown method
    Unknown(String),
}

impl CellMethodName {
    /// Parse from a CF cell method string token.
    #[must_use]
    pub fn parse_method(s: &str) -> Self {
        match s {
            "point" => Self::Point,
            "sum" => Self::Sum,
            "maximum" => Self::Maximum,
            "median" => Self::Median,
            "midrange" => Self::Midrange,
            "minimum" => Self::Minimum,
            "mean" => Self::Mean,
            "mode" => Self::Mode,
            "standard_deviation" => Self::StandardDeviation,
            "variance" => Self::Variance,
            other => Self::Unknown(other.to_string()),
        }
    }

    /// Returns the CF string token for this method.
    #[must_use]
    pub fn to_cf_str(&self) -> &str {
        match self {
            Self::Point => "point",
            Self::Sum => "sum",
            Self::Maximum => "maximum",
            Self::Median => "median",
            Self::Midrange => "midrange",
            Self::Minimum => "minimum",
            Self::Mean => "mean",
            Self::Mode => "mode",
            Self::StandardDeviation => "standard_deviation",
            Self::Variance => "variance",
            Self::Unknown(s) => s.as_str(),
        }
    }

    /// Returns `true` for known (non-`Unknown`) methods.
    #[must_use]
    pub fn is_known(&self) -> bool {
        !matches!(self, Self::Unknown(_))
    }
}

// ---------------------------------------------------------------------------
// CfCellMethod
// ---------------------------------------------------------------------------

/// A single cell method entry (CF §7.3).
///
/// Represents one token of the `cell_methods` attribute, e.g.
/// `"time: mean"` or `"area: mean where land"`.
#[derive(Debug, Clone)]
pub struct CfCellMethod {
    /// Coordinate names (can be multiple, e.g. `["lat", "lon"]`).
    pub coordinates: Vec<String>,
    /// The method applied.
    pub method: CellMethodName,
    /// Optional `where <type>` qualifier.
    pub where_type: Option<String>,
    /// Optional `over <type>` qualifier.
    pub over_type: Option<String>,
    /// Optional `interval: <value> <units>` specification.
    pub interval: Option<String>,
    /// Free-form comment in parentheses.
    pub comment: Option<String>,
}

impl CfCellMethod {
    /// Construct a simple cell method with one coordinate.
    #[must_use]
    pub fn simple(coordinate: impl Into<String>, method: CellMethodName) -> Self {
        Self {
            coordinates: vec![coordinate.into()],
            method,
            where_type: None,
            over_type: None,
            interval: None,
            comment: None,
        }
    }
}

// ---------------------------------------------------------------------------
// CfGeometryContainer — new in CF v1.8, refined in v1.11
// ---------------------------------------------------------------------------

/// CF geometry container variable attributes (CF §7.5).
#[derive(Debug, Clone)]
pub struct CfGeometryContainer {
    /// Geometry type.
    pub geometry_type: CfGeometryType,
    /// Space-separated list of node coordinate variables.
    pub node_coordinates: String,
    /// Variable name holding the node count per geometry (required for multi-geometry).
    pub node_count: Option<String>,
    /// Variable name holding the part node count (for multi-part geometries).
    pub part_node_count: Option<String>,
    /// Variable name holding interior ring flags (for polygons with holes).
    pub interior_ring: Option<String>,
}

impl CfGeometryContainer {
    /// Create a minimal geometry container for points.
    #[must_use]
    pub fn point(node_coordinates: impl Into<String>) -> Self {
        Self {
            geometry_type: CfGeometryType::Point,
            node_coordinates: node_coordinates.into(),
            node_count: None,
            part_node_count: None,
            interior_ring: None,
        }
    }

    /// Create a polygon container (requires part_node_count for holes).
    #[must_use]
    pub fn polygon(node_coordinates: impl Into<String>, node_count: impl Into<String>) -> Self {
        Self {
            geometry_type: CfGeometryType::Polygon,
            node_coordinates: node_coordinates.into(),
            node_count: Some(node_count.into()),
            part_node_count: None,
            interior_ring: None,
        }
    }

    /// Returns `true` if this geometry type can have interior rings (polygons).
    #[must_use]
    pub fn can_have_holes(&self) -> bool {
        matches!(
            self.geometry_type,
            CfGeometryType::Polygon | CfGeometryType::MultiPolygon
        )
    }
}

// ---------------------------------------------------------------------------
// validate_cf_version — parse "CF-1.11" or "CF-1.8, ACDD-1.3"
// ---------------------------------------------------------------------------

/// Parse the highest CF version from a `Conventions` attribute string.
///
/// Handles comma-separated lists such as `"CF-1.11, ACDD-1.3"`.
/// Returns `None` if no `CF-` prefix is found.
#[must_use]
pub fn validate_cf_version(conventions: &str) -> Option<CfVersion> {
    let mut best: Option<CfVersion> = None;
    for part in conventions.split(',') {
        let part = part.trim();
        if let Some(stripped) = part.strip_prefix("CF-") {
            if let Some(v) = CfVersion::parse_version(stripped) {
                best = Some(match best {
                    None => v,
                    Some(existing) => {
                        if v > existing {
                            v
                        } else {
                            existing
                        }
                    }
                });
            }
        }
    }
    best
}

/// Validate a units string (permissive check — full UDUNITS2 not implemented).
///
/// Returns `false` for empty strings. Accepts `"1"`, `"dimensionless"`, and
/// strings containing any recognised SI base unit symbol.
#[must_use]
pub fn validate_units(units: &str) -> bool {
    if units.is_empty() {
        return false;
    }
    if units == "1" || units == "dimensionless" {
        return true;
    }
    // Percentage is dimensionless
    if units == "%" {
        return true;
    }
    // Accept if it contains at least one known SI base unit symbol as a whole token.
    // Split on whitespace and common unit separators, then check for exact matches.
    const KNOWN: &[&str] = &["K", "Pa", "m", "s", "kg", "mol", "W", "J", "N", "Hz", "A"];
    // Also accept compound forms like "m s-1", "kg m-2 s-1" where the base unit
    // appears as a token possibly followed by an exponent (e.g., "s-1", "m-2").
    units.split_whitespace().any(|token| {
        // Strip optional trailing exponent (e.g., "s-1" -> "s", "m-2" -> "m")
        let base = token
            .find(|c: char| c == '-' || c.is_ascii_digit())
            .map_or(token, |i| &token[..i]);
        KNOWN.contains(&base)
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- CfVersion --

    #[test]
    fn test_cf_version_display() {
        assert_eq!(CfVersion::v1_11().to_string(), "1.11");
        assert_eq!(CfVersion::v1_8().to_string(), "1.8");
    }

    #[test]
    fn test_cf_version_from_str_valid() {
        let v = CfVersion::parse_version("1.11").expect("parse 1.11");
        assert_eq!(v, CfVersion::v1_11());
    }

    #[test]
    fn test_cf_version_from_str_invalid() {
        assert!(CfVersion::parse_version("abc").is_none());
        assert!(CfVersion::parse_version("1").is_none());
        assert!(CfVersion::parse_version("").is_none());
    }

    #[test]
    fn test_cf_version_ordering() {
        assert!(CfVersion::v1_11() > CfVersion::v1_8());
        assert!(CfVersion::v1_8() < CfVersion::v1_11());
        assert_eq!(
            CfVersion::v1_8(),
            CfVersion::parse_version("1.8").expect("parse 1.8")
        );
    }

    #[test]
    fn test_cf_version_is_at_least() {
        assert!(CfVersion::v1_11().is_at_least(&CfVersion::v1_8()));
        assert!(!CfVersion::v1_8().is_at_least(&CfVersion::v1_11()));
        assert!(CfVersion::v1_11().is_at_least(&CfVersion::v1_11()));
    }

    // -- CF_STANDARD_NAMES table --

    #[test]
    fn test_standard_names_table_non_empty() {
        assert!(!CF_STANDARD_NAMES.is_empty());
        assert!(CF_STANDARD_NAMES.len() >= 20);
    }

    #[test]
    fn test_lookup_standard_name_found() {
        let entry = lookup_standard_name("air_temperature").expect("air_temperature lookup");
        assert_eq!(entry.canonical_units, "K");
        assert!(!entry.description.is_empty());
    }

    #[test]
    fn test_lookup_standard_name_not_found() {
        assert!(lookup_standard_name("nonexistent_variable_xyz").is_none());
    }

    #[test]
    fn test_lookup_wind_components() {
        assert!(lookup_standard_name("eastward_wind").is_some());
        assert!(lookup_standard_name("northward_wind").is_some());
        assert_eq!(
            lookup_standard_name("eastward_wind")
                .expect("eastward_wind lookup")
                .canonical_units,
            "m s-1"
        );
    }

    #[test]
    fn test_lookup_ocean_variables() {
        assert!(lookup_standard_name("sea_surface_temperature").is_some());
        assert!(lookup_standard_name("sea_surface_salinity").is_some());
    }

    #[test]
    fn test_all_standard_names_have_non_empty_fields() {
        for entry in CF_STANDARD_NAMES {
            assert!(!entry.name.is_empty(), "empty name");
            assert!(
                !entry.canonical_units.is_empty(),
                "empty units for {}",
                entry.name
            );
            assert!(
                !entry.description.is_empty(),
                "empty desc for {}",
                entry.name
            );
        }
    }

    // -- CfCoordinateType --

    #[test]
    fn test_coordinate_type_labels() {
        assert_eq!(CfCoordinateType::Longitude.label(), "longitude");
        assert_eq!(CfCoordinateType::Time.label(), "time");
    }

    #[test]
    fn test_coordinate_type_is_spatial() {
        assert!(CfCoordinateType::Longitude.is_spatial());
        assert!(CfCoordinateType::Latitude.is_spatial());
        assert!(CfCoordinateType::Vertical.is_spatial());
        assert!(!CfCoordinateType::Time.is_spatial());
        assert!(!CfCoordinateType::Scalar.is_spatial());
    }

    // -- CfGeometryType --

    #[test]
    fn test_geometry_type_cf_names_roundtrip() {
        let types = [
            CfGeometryType::Point,
            CfGeometryType::Line,
            CfGeometryType::Polygon,
            CfGeometryType::MultiPoint,
            CfGeometryType::MultiLine,
            CfGeometryType::MultiPolygon,
        ];
        for t in &types {
            let name = t.cf_name();
            let back = CfGeometryType::from_cf_name(name).expect("roundtrip cf_name");
            assert_eq!(&back, t);
        }
    }

    #[test]
    fn test_geometry_type_case_insensitive() {
        assert_eq!(
            CfGeometryType::from_cf_name("POLYGON"),
            Some(CfGeometryType::Polygon)
        );
        assert_eq!(
            CfGeometryType::from_cf_name("MultiPoint"),
            Some(CfGeometryType::MultiPoint)
        );
    }

    #[test]
    fn test_geometry_type_unknown() {
        assert!(CfGeometryType::from_cf_name("hexagon").is_none());
    }

    #[test]
    fn test_geometry_type_is_multi() {
        assert!(!CfGeometryType::Point.is_multi());
        assert!(!CfGeometryType::Polygon.is_multi());
        assert!(CfGeometryType::MultiPolygon.is_multi());
        assert!(CfGeometryType::MultiLine.is_multi());
    }

    // -- CellMethodName --

    #[test]
    fn test_cell_method_from_str_known() {
        assert_eq!(CellMethodName::parse_method("mean"), CellMethodName::Mean);
        assert_eq!(
            CellMethodName::parse_method("maximum"),
            CellMethodName::Maximum
        );
        assert_eq!(
            CellMethodName::parse_method("standard_deviation"),
            CellMethodName::StandardDeviation
        );
    }

    #[test]
    fn test_cell_method_from_str_unknown() {
        let u = CellMethodName::parse_method("trimmed_mean");
        assert!(!u.is_known());
        assert!(matches!(u, CellMethodName::Unknown(_)));
    }

    #[test]
    fn test_cell_method_roundtrip() {
        let methods = [
            "point",
            "sum",
            "maximum",
            "median",
            "midrange",
            "minimum",
            "mean",
            "mode",
            "standard_deviation",
            "variance",
        ];
        for &m in &methods {
            let parsed = CellMethodName::parse_method(m);
            assert!(parsed.is_known(), "{m} should be known");
            assert_eq!(parsed.to_cf_str(), m);
        }
    }

    // -- validate_cf_version --

    #[test]
    fn test_validate_cf_version_simple() {
        let v = validate_cf_version("CF-1.11").expect("validate CF-1.11");
        assert_eq!(v, CfVersion::v1_11());
    }

    #[test]
    fn test_validate_cf_version_with_acdd() {
        let v = validate_cf_version("CF-1.8, ACDD-1.3").expect("validate CF-1.8");
        assert_eq!(v, CfVersion::v1_8());
    }

    #[test]
    fn test_validate_cf_version_multiple_cf() {
        // Should return the highest version
        let v = validate_cf_version("CF-1.6, CF-1.11").expect("validate multiple CF versions");
        assert_eq!(v, CfVersion::v1_11());
    }

    #[test]
    fn test_validate_cf_version_none() {
        assert!(validate_cf_version("ACDD-1.3").is_none());
        assert!(validate_cf_version("").is_none());
    }

    // -- validate_units --

    #[test]
    fn test_validate_units_known_si() {
        assert!(validate_units("K"));
        assert!(validate_units("Pa"));
        assert!(validate_units("m s-1"));
        assert!(validate_units("kg m-2 s-1"));
        assert!(validate_units("W m-2"));
        assert!(validate_units("J m-2"));
        assert!(validate_units("mol mol-1"));
    }

    #[test]
    fn test_validate_units_dimensionless() {
        assert!(validate_units("1"));
        assert!(validate_units("dimensionless"));
        assert!(validate_units("%"));
    }

    #[test]
    fn test_validate_units_empty() {
        assert!(!validate_units(""));
    }

    #[test]
    fn test_validate_units_unknown_string() {
        // "furlong" contains no recognised SI symbol
        assert!(!validate_units("furlongs per fortnight"));
    }

    // -- CfGeometryContainer --

    #[test]
    fn test_geometry_container_point() {
        let c = CfGeometryContainer::point("x y");
        assert_eq!(c.geometry_type, CfGeometryType::Point);
        assert_eq!(c.node_coordinates, "x y");
        assert!(!c.can_have_holes());
    }

    #[test]
    fn test_geometry_container_polygon_can_have_holes() {
        let c = CfGeometryContainer::polygon("x y", "node_count");
        assert!(c.can_have_holes());
        assert_eq!(c.node_count.as_deref(), Some("node_count"));
    }

    // -- CfCellMethod --

    #[test]
    fn test_cell_method_simple() {
        let cm = CfCellMethod::simple("time", CellMethodName::Mean);
        assert_eq!(cm.coordinates, vec!["time"]);
        assert_eq!(cm.method, CellMethodName::Mean);
        assert!(cm.where_type.is_none());
    }
}
