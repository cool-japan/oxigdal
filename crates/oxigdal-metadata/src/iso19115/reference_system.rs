//! Reference system information for ISO 19115.

use serde::{Deserialize, Serialize};

/// Reference system information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceSystem {
    /// Reference system identifier
    pub reference_system_identifier: Option<Identifier>,
    /// Reference system type
    pub reference_system_type: Option<ReferenceSystemType>,
}

/// Identifier for reference system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identifier {
    /// Code
    pub code: String,
    /// Code space
    pub code_space: Option<String>,
    /// Version
    pub version: Option<String>,
    /// Authority
    pub authority: Option<String>,
}

impl Identifier {
    /// Create a new identifier.
    pub fn new(code: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            code_space: None,
            version: None,
            authority: None,
        }
    }

    /// Create an EPSG identifier.
    pub fn epsg(code: u32) -> Self {
        Self {
            code: code.to_string(),
            code_space: Some("EPSG".to_string()),
            version: None,
            authority: Some("EPSG".to_string()),
        }
    }

    /// Create a WKT identifier.
    pub fn wkt(wkt: impl Into<String>) -> Self {
        Self {
            code: wkt.into(),
            code_space: Some("WKT".to_string()),
            version: None,
            authority: None,
        }
    }
}

/// Reference system type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ReferenceSystemType {
    /// Compound CRS
    Compound,
    /// Engineering CRS
    Engineering,
    /// Geographic 2D CRS
    Geographic2D,
    /// Geographic 3D CRS
    Geographic3D,
    /// Geocentric CRS
    Geocentric,
    /// Projected CRS
    Projected,
    /// Temporal CRS
    Temporal,
    /// Vertical CRS
    Vertical,
}

/// Coordinate system axis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinateSystemAxis {
    /// Axis abbreviation
    pub axis_abbreviation: String,
    /// Axis direction
    pub axis_direction: AxisDirection,
    /// Axis unit
    pub axis_unit: String,
}

/// Axis direction.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum AxisDirection {
    /// East
    East,
    /// West
    West,
    /// North
    North,
    /// South
    South,
    /// Up
    Up,
    /// Down
    Down,
    /// Future
    Future,
    /// Past
    Past,
}

/// Datum information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Datum {
    /// Datum name
    pub name: String,
    /// Datum type
    pub datum_type: DatumType,
    /// Anchor point
    pub anchor_point: Option<String>,
    /// Realization epoch
    pub realization_epoch: Option<chrono::DateTime<chrono::Utc>>,
}

/// Datum type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum DatumType {
    /// Geodetic
    Geodetic,
    /// Vertical
    Vertical,
    /// Engineering
    Engineering,
    /// Temporal
    Temporal,
}

/// Ellipsoid information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ellipsoid {
    /// Ellipsoid name
    pub name: String,
    /// Semi-major axis (meters)
    pub semi_major_axis: f64,
    /// Second defining parameter
    pub second_defining_parameter: SecondDefiningParameter,
}

/// Second defining parameter for ellipsoid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SecondDefiningParameter {
    /// Semi-minor axis (meters)
    SemiMinorAxis(f64),
    /// Inverse flattening
    InverseFlattening(f64),
}

/// Prime meridian information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrimeMeridian {
    /// Prime meridian name
    pub name: String,
    /// Greenwich longitude (degrees)
    pub greenwich_longitude: f64,
}

impl Default for PrimeMeridian {
    fn default() -> Self {
        Self {
            name: "Greenwich".to_string(),
            greenwich_longitude: 0.0,
        }
    }
}
