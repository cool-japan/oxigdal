//! Error types for terrain analysis operations.

use thiserror::Error;

/// Result type for terrain operations.
pub type Result<T> = core::result::Result<T, TerrainError>;

/// Errors that can occur during terrain analysis.
#[derive(Debug, Error)]
pub enum TerrainError {
    /// Invalid DEM dimensions
    #[error("Invalid DEM dimensions: width={width}, height={height}")]
    InvalidDimensions {
        /// Width of the DEM
        width: usize,
        /// Height of the DEM
        height: usize,
    },

    /// Invalid cell size
    #[error("Invalid cell size: {size}. Cell size must be positive.")]
    InvalidCellSize {
        /// Cell size value
        size: f64,
    },

    /// Invalid azimuth angle for hillshade
    #[error("Invalid azimuth: {azimuth}. Azimuth must be between 0 and 360 degrees.")]
    InvalidAzimuth {
        /// Azimuth value
        azimuth: f64,
    },

    /// Invalid altitude angle for hillshade
    #[error("Invalid altitude: {altitude}. Altitude must be between 0 and 90 degrees.")]
    InvalidAltitude {
        /// Altitude value
        altitude: f64,
    },

    /// Invalid observer position for viewshed
    #[error("Invalid observer position: ({x}, {y}). Position must be within DEM bounds.")]
    InvalidObserverPosition {
        /// X coordinate
        x: usize,
        /// Y coordinate
        y: usize,
    },

    /// Invalid observer height
    #[error("Invalid observer height: {height}. Height must be non-negative.")]
    InvalidObserverHeight {
        /// Height value
        height: f64,
    },

    /// Invalid target height
    #[error("Invalid target height: {height}. Height must be non-negative.")]
    InvalidTargetHeight {
        /// Height value
        height: f64,
    },

    /// Invalid radius for analysis
    #[error("Invalid radius: {radius}. Radius must be positive.")]
    InvalidRadius {
        /// Radius value
        radius: f64,
    },

    /// Invalid threshold value
    #[error("Invalid threshold: {threshold}. {message}")]
    InvalidThreshold {
        /// Threshold value
        threshold: f64,
        /// Error message
        message: String,
    },

    /// Missing or invalid NoData value
    #[error("Missing or invalid NoData handling: {message}")]
    InvalidNoData {
        /// Error message
        message: String,
    },

    /// Flow direction algorithm error
    #[error("Flow direction error: {message}")]
    FlowDirectionError {
        /// Error message
        message: String,
    },

    /// Watershed delineation error
    #[error("Watershed delineation error: {message}")]
    WatershedError {
        /// Error message
        message: String,
    },

    /// Viewshed computation error
    #[error("Viewshed computation error: {message}")]
    ViewshedError {
        /// Error message
        message: String,
    },

    /// Computation error with generic message
    #[error("Computation error: {message}")]
    ComputationError {
        /// Error message
        message: String,
    },

    /// Insufficient memory for operation
    #[error("Insufficient memory for operation: {message}")]
    InsufficientMemory {
        /// Error message
        message: String,
    },

    /// Core library error
    #[error("Core error: {0}")]
    CoreError(#[from] oxigdal_core::error::OxiGdalError),
}
