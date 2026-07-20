//! Error types for projection and coordinate transformation operations.
//!
//! This module provides comprehensive error handling for all projection-related operations,
//! following the no-unwrap policy.

#[cfg(not(feature = "std"))]
use alloc::string::String;

/// Result type for projection operations.
pub type Result<T> = core::result::Result<T, Error>;

/// Comprehensive error type for projection operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Invalid EPSG code
    #[error("Invalid EPSG code: {code}")]
    InvalidEpsgCode {
        /// The invalid EPSG code
        code: u32,
    },

    /// EPSG code not found in database
    #[error("EPSG code {code} not found in database")]
    EpsgCodeNotFound {
        /// The EPSG code that was not found
        code: u32,
    },

    /// Invalid PROJ string
    #[error("Invalid PROJ string: {reason}")]
    InvalidProjString {
        /// Reason for the invalid PROJ string
        reason: String,
    },

    /// Invalid WKT (Well-Known Text) string
    #[error("Invalid WKT string: {reason}")]
    InvalidWkt {
        /// Reason for the invalid WKT
        reason: String,
    },

    /// WKT parsing error
    #[error("WKT parsing error at position {position}: {message}")]
    WktParseError {
        /// Position in the WKT string where error occurred
        position: usize,
        /// Error message
        message: String,
    },

    /// Coordinate transformation error
    #[error("Coordinate transformation failed: {reason}")]
    TransformationError {
        /// Reason for transformation failure
        reason: String,
    },

    /// Unsupported CRS (Coordinate Reference System)
    #[error("Unsupported CRS: {crs_type}")]
    UnsupportedCrs {
        /// Type of CRS that is not supported
        crs_type: String,
    },

    /// Incompatible source and target CRS
    #[error("Incompatible CRS for transformation: source={src}, target={tgt}")]
    IncompatibleCrs {
        /// Source CRS description
        src: String,
        /// Target CRS description
        tgt: String,
    },

    /// Invalid coordinate
    #[error("Invalid coordinate: {reason}")]
    InvalidCoordinate {
        /// Reason for invalid coordinate
        reason: String,
    },

    /// Out of bounds coordinate
    #[error("Coordinate out of valid bounds: ({x}, {y})")]
    CoordinateOutOfBounds {
        /// X coordinate
        x: f64,
        /// Y coordinate
        y: f64,
    },

    /// Invalid bounding box
    #[error("Invalid bounding box: {reason}")]
    InvalidBoundingBox {
        /// Reason for invalid bounding box
        reason: String,
    },

    /// Missing required parameter
    #[error("Missing required parameter: {parameter}")]
    MissingParameter {
        /// Name of missing parameter
        parameter: String,
    },

    /// Invalid parameter value
    #[error("Invalid parameter value for {parameter}: {reason}")]
    InvalidParameter {
        /// Parameter name
        parameter: String,
        /// Reason for invalid value
        reason: String,
    },

    /// Datum transformation error
    #[error("Datum transformation failed: {reason}")]
    DatumTransformError {
        /// Reason for datum transformation failure
        reason: String,
    },

    /// Projection initialization error
    #[error("Failed to initialize projection: {reason}")]
    ProjectionInitError {
        /// Reason for initialization failure
        reason: String,
    },

    /// Unsupported projection
    #[error("Unsupported projection: {projection}")]
    UnsupportedProjection {
        /// Name of unsupported projection
        projection: String,
    },

    /// Numerical error (e.g., division by zero, sqrt of negative)
    #[error("Numerical error in projection calculation: {operation}")]
    NumericalError {
        /// Operation that caused the error
        operation: String,
    },

    /// Convergence failure in iterative algorithms
    #[error("Failed to converge after {iterations} iterations")]
    ConvergenceError {
        /// Number of iterations attempted
        iterations: usize,
    },

    /// JSON serialization/deserialization error
    #[cfg(feature = "std")]
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    /// I/O error
    #[cfg(feature = "std")]
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// UTF-8 conversion error
    #[cfg(feature = "std")]
    #[error("UTF-8 conversion error: {0}")]
    Utf8Error(#[from] std::str::Utf8Error),

    /// Error from proj4rs library
    #[cfg(feature = "std")]
    #[error("Proj4rs error: {0}")]
    Proj4rsError(String),

    /// Error from PROJ C library (when using proj-sys feature)
    #[cfg(feature = "proj-sys")]
    #[error("PROJ library error: {0}")]
    ProjSysError(String),

    /// Coordinate is outside the source CRS area of use
    #[error("Coordinate ({lon}, {lat}) is outside area of use for CRS '{crs}'")]
    OutOfAreaOfUse {
        /// Longitude
        lon: f64,
        /// Latitude
        lat: f64,
        /// CRS description
        crs: String,
    },

    /// Coordinate lies outside the registered area-of-use bounding box for
    /// the source EPSG (raised by `Transformer` when the per-instance check
    /// mode is `AreaOfUseCheck::Strict`).
    #[error(
        "coordinate ({lon}, {lat}) outside area-of-use for EPSG:{epsg} \
         (west={west}, south={south}, east={east}, north={north})"
    )]
    OutsideAreaOfUse {
        /// Longitude of the offending point (degrees, WGS84).
        lon: f64,
        /// Latitude of the offending point (degrees, WGS84).
        lat: f64,
        /// Source EPSG code whose area-of-use was violated.
        epsg: u32,
        /// Western bound of the area-of-use (degrees).
        west: f64,
        /// Southern bound of the area-of-use (degrees).
        south: f64,
        /// Eastern bound of the area-of-use (degrees).
        east: f64,
        /// Northern bound of the area-of-use (degrees).
        north: f64,
    },

    /// NTv2 binary grid-shift parse error
    #[error("NTv2 parse error: {0}")]
    Ntv2ParseError(String),

    /// Coordinate is outside all loaded NTv2 sub-grid extents
    #[error("Coordinate ({lon}, {lat}) is outside NTv2 grid extent")]
    Ntv2OutOfGrid {
        /// Longitude in degrees
        lon: f64,
        /// Latitude in degrees
        lat: f64,
    },

    /// Invalid arguments for compound CRS construction.
    #[error("Invalid compound CRS: {reason}")]
    InvalidCompoundCrs {
        /// Reason the compound CRS is invalid
        reason: String,
    },

    /// Geoid model required for vertical datum transformation is not available.
    #[error("Geoid model not available for vertical CRS: {vertical_crs}")]
    GeoidNotAvailable {
        /// Name/description of the vertical CRS that requires a geoid model
        vertical_crs: String,
    },

    /// Geoid grid file has an unexpected size, layout, or could not be read.
    #[error("geoid file format: {0}")]
    GeoidFileFormat(String),

    /// No geoid model has been attached to a [`crate::transform::Transformer`]
    /// that is being asked to perform a compound-CRS transform requiring a
    /// vertical-datum shift.
    #[error("no geoid model available for compound CRS transform")]
    NoGeoidAvailable,

    /// Error from PROJ.db SQLite file (always present so non-proj-db builds also compile)
    #[error("PROJ.db error: {0}")]
    ProjDbError(String),

    /// Generic error for cases not covered by specific error types
    #[error("{0}")]
    Other(String),

    /// Pipeline PROJ string could not be parsed.
    #[error("Pipeline parse error: {0}")]
    PipelineParseError(String),

    /// A step inside a coordinate pipeline failed.
    #[error("Pipeline step {step} failed: {inner}")]
    PipelineStepError {
        /// Zero-based index of the failing step
        step: usize,
        /// Error message from the step
        inner: String,
    },
}

impl Error {
    /// Creates an invalid EPSG code error.
    pub fn invalid_epsg_code(code: u32) -> Self {
        Self::InvalidEpsgCode { code }
    }

    /// Creates an EPSG code not found error.
    pub fn epsg_not_found(code: u32) -> Self {
        Self::EpsgCodeNotFound { code }
    }

    /// Creates an invalid PROJ string error.
    pub fn invalid_proj_string<S: Into<String>>(reason: S) -> Self {
        Self::InvalidProjString {
            reason: reason.into(),
        }
    }

    /// Creates an invalid WKT error.
    pub fn invalid_wkt<S: Into<String>>(reason: S) -> Self {
        Self::InvalidWkt {
            reason: reason.into(),
        }
    }

    /// Creates a WKT parsing error.
    pub fn wkt_parse_error<S: Into<String>>(position: usize, message: S) -> Self {
        Self::WktParseError {
            position,
            message: message.into(),
        }
    }

    /// Creates a transformation error.
    pub fn transformation_error<S: Into<String>>(reason: S) -> Self {
        Self::TransformationError {
            reason: reason.into(),
        }
    }

    /// Creates an unsupported CRS error.
    pub fn unsupported_crs<S: Into<String>>(crs_type: S) -> Self {
        Self::UnsupportedCrs {
            crs_type: crs_type.into(),
        }
    }

    /// Creates an incompatible CRS error.
    pub fn incompatible_crs<S: Into<String>>(src: S, tgt: S) -> Self {
        Self::IncompatibleCrs {
            src: src.into(),
            tgt: tgt.into(),
        }
    }

    /// Creates an invalid coordinate error.
    pub fn invalid_coordinate<S: Into<String>>(reason: S) -> Self {
        Self::InvalidCoordinate {
            reason: reason.into(),
        }
    }

    /// Creates a coordinate out of bounds error.
    pub fn coordinate_out_of_bounds(x: f64, y: f64) -> Self {
        Self::CoordinateOutOfBounds { x, y }
    }

    /// Creates an invalid bounding box error.
    pub fn invalid_bounding_box<S: Into<String>>(reason: S) -> Self {
        Self::InvalidBoundingBox {
            reason: reason.into(),
        }
    }

    /// Creates a missing parameter error.
    pub fn missing_parameter<S: Into<String>>(parameter: S) -> Self {
        Self::MissingParameter {
            parameter: parameter.into(),
        }
    }

    /// Creates an invalid parameter error.
    pub fn invalid_parameter<S: Into<String>>(parameter: S, reason: S) -> Self {
        Self::InvalidParameter {
            parameter: parameter.into(),
            reason: reason.into(),
        }
    }

    /// Creates a datum transform error.
    pub fn datum_transform_error<S: Into<String>>(reason: S) -> Self {
        Self::DatumTransformError {
            reason: reason.into(),
        }
    }

    /// Creates a projection initialization error.
    pub fn projection_init_error<S: Into<String>>(reason: S) -> Self {
        Self::ProjectionInitError {
            reason: reason.into(),
        }
    }

    /// Creates an unsupported projection error.
    pub fn unsupported_projection<S: Into<String>>(projection: S) -> Self {
        Self::UnsupportedProjection {
            projection: projection.into(),
        }
    }

    /// Creates a numerical error.
    pub fn numerical_error<S: Into<String>>(operation: S) -> Self {
        Self::NumericalError {
            operation: operation.into(),
        }
    }

    /// Creates a convergence error.
    pub fn convergence_error(iterations: usize) -> Self {
        Self::ConvergenceError { iterations }
    }

    /// Creates an error from proj4rs library.
    #[cfg(feature = "std")]
    pub fn from_proj4rs<S: Into<String>>(message: S) -> Self {
        Self::Proj4rsError(message.into())
    }

    /// Creates an out-of-area-of-use error.
    pub fn out_of_area_of_use<S: Into<String>>(lon: f64, lat: f64, crs: S) -> Self {
        Self::OutOfAreaOfUse {
            lon,
            lat,
            crs: crs.into(),
        }
    }

    /// Creates an NTv2 parse error.
    pub fn ntv2_parse_error<S: Into<String>>(message: S) -> Self {
        Self::Ntv2ParseError(message.into())
    }

    /// Creates an NTv2 out-of-grid error.
    pub fn ntv2_out_of_grid(lon: f64, lat: f64) -> Self {
        Self::Ntv2OutOfGrid { lon, lat }
    }

    /// Creates an invalid compound CRS error.
    pub fn invalid_compound_crs(reason: impl Into<String>) -> Self {
        Self::InvalidCompoundCrs {
            reason: reason.into(),
        }
    }

    /// Creates a geoid-not-available error.
    pub fn geoid_not_available(vertical_crs: impl Into<String>) -> Self {
        Self::GeoidNotAvailable {
            vertical_crs: vertical_crs.into(),
        }
    }

    /// Creates a generic other error.
    pub fn other<S: Into<String>>(message: S) -> Self {
        Self::Other(message.into())
    }
}

// Implement conversion from proj4rs errors (only when proj4rs-compat feature is enabled)
#[cfg(feature = "proj4rs-compat")]
impl From<proj4rs::errors::Error> for Error {
    fn from(err: proj4rs::errors::Error) -> Self {
        Self::from_proj4rs(format!("{:?}", err))
    }
}

#[cfg(feature = "proj-sys")]
impl From<proj::ProjError> for Error {
    fn from(err: proj::ProjError) -> Self {
        Self::ProjSysError(format!("{}", err))
    }
}

// Implement conversion from oxiproj errors
impl From<oxiproj::TransformError> for Error {
    fn from(e: oxiproj::TransformError) -> Self {
        Self::TransformationError {
            reason: e.to_string(),
        }
    }
}

impl From<oxiproj::ProjError> for Error {
    fn from(e: oxiproj::ProjError) -> Self {
        match e {
            oxiproj::ProjError::InvalidCoord => Self::InvalidCoordinate {
                reason: "invalid coordinate (OxiProj)".to_string(),
            },
            oxiproj::ProjError::OutsideProjectionDomain => Self::CoordinateOutOfBounds {
                x: f64::NAN,
                y: f64::NAN,
            },
            oxiproj::ProjError::NoConvergence => Self::ConvergenceError { iterations: 0 },
            other => Self::TransformationError {
                reason: other.to_string(),
            },
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = Error::invalid_epsg_code(12345);
        assert!(matches!(err, Error::InvalidEpsgCode { code: 12345 }));

        let err = Error::epsg_not_found(4326);
        assert!(matches!(err, Error::EpsgCodeNotFound { code: 4326 }));

        let err = Error::invalid_proj_string("missing parameter");
        assert!(matches!(err, Error::InvalidProjString { .. }));

        let err = Error::transformation_error("invalid coordinates");
        assert!(matches!(err, Error::TransformationError { .. }));
    }

    #[test]
    fn test_error_display() {
        let err = Error::invalid_epsg_code(12345);
        assert_eq!(format!("{}", err), "Invalid EPSG code: 12345");

        let err = Error::coordinate_out_of_bounds(180.5, 90.5);
        assert_eq!(
            format!("{}", err),
            "Coordinate out of valid bounds: (180.5, 90.5)"
        );
    }

    #[test]
    fn test_result_type() {
        fn returns_ok() -> Result<i32> {
            Ok(42)
        }

        fn returns_error() -> Result<i32> {
            Err(Error::invalid_epsg_code(0))
        }

        assert!(returns_ok().is_ok());
        assert!(returns_error().is_err());
    }
}
