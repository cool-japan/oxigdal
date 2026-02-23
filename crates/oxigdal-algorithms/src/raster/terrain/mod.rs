//! Advanced terrain analysis algorithms
//!
//! This module provides comprehensive terrain analysis metrics beyond basic
//! slope and aspect calculations. These metrics are essential for geomorphology,
//! hydrology, and landscape ecology studies.
//!
//! # Terrain Metrics
//!
//! ## Topographic Position Index (TPI)
//!
//! TPI compares the elevation of a point to the mean elevation of its neighborhood.
//! Positive values indicate ridges/peaks, negative values indicate valleys, and
//! values near zero indicate flat areas or mid-slope positions.
//! Supports both rectangular and annular (ring) neighborhoods.
//!
//! ## Terrain Ruggedness Index (TRI)
//!
//! TRI quantifies topographic heterogeneity by calculating the difference between
//! a center cell and its neighbors. Supports both Riley et al. (1999) method
//! (square root of sum of squared differences) and the simple mean absolute
//! difference method.
//!
//! ## Surface Roughness
//!
//! Roughness measures the variability in elevation within a neighborhood, using
//! either standard deviation or range (max - min) methods.
//!
//! ## Curvature
//!
//! Curvature describes the shape of the terrain surface:
//! - **Profile curvature**: Rate of change of slope (affects flow acceleration)
//! - **Planform curvature**: Curvature perpendicular to slope (affects flow convergence)
//! - **Total curvature**: Combined measure of surface curvature (Laplacian)
//! - **Mean curvature**: Average of principal curvatures
//! - **Gaussian curvature**: Product of principal curvatures
//! - **Tangential curvature**: Curvature in direction perpendicular to slope
//! - **Convergence index**: Measure of flow convergence/divergence
//!
//! ## Vector Ruggedness Measure (VRM)
//!
//! VRM calculates terrain ruggedness as the dispersion of normal vectors
//! to the surface, providing a scale-independent measure of roughness.
//!
//! ## Terrain Classification
//!
//! Landform classification using TPI at multiple scales following Weiss (2001)
//! and Jenness (2006) methodologies, classifying terrain into categories such
//! as valley bottoms, ridges, plains, upper/lower slopes, etc.
//!
//! ## Topographic Wetness Index (TWI)
//!
//! TWI = ln(a / tan(beta)) where a is specific catchment area and beta is slope.
//! Used in hydrological modeling to predict soil moisture distribution.
//!
//! ## Stream Power Index (SPI)
//!
//! SPI = a * tan(beta) where a is specific catchment area and beta is slope.
//! Measures erosive power of flowing water.
//!
//! # References
//!
//! - Burrough, P.A. & McDonnell, R.A. (1998). Principles of GIS.
//! - Riley, S.J. et al. (1999). A terrain ruggedness index.
//! - Weiss, A. (2001). Topographic Position and Landforms Analysis.
//! - Jenness, J. (2006). Topographic Position Index extension for ArcView.
//! - Zevenbergen, L.W. & Thorne, C.R. (1987). Quantitative analysis of land surface topography.
//! - Sappington, J.M. et al. (2007). Quantifying landscape ruggedness (VRM).

use crate::error::{AlgorithmError, Result};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

#[cfg(feature = "parallel")]
use rayon::prelude::*;

// ---------------------------------------------------------------------------
// TRI method variants
// ---------------------------------------------------------------------------

/// Method for computing Terrain Ruggedness Index
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriMethod {
    /// Riley et al. (1999): sqrt(sum of squared differences)
    Riley,
    /// Simple mean absolute difference
    MeanAbsoluteDifference,
    /// Wilson et al. (2007): root mean square difference
    RootMeanSquare,
}

// ---------------------------------------------------------------------------
// Roughness method variants
// ---------------------------------------------------------------------------

/// Method for computing surface roughness
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoughnessMethod {
    /// Standard deviation of elevations
    StandardDeviation,
    /// Range (max - min) of elevations
    Range,
    /// Coefficient of variation (stddev / mean)
    CoefficientOfVariation,
}

// ---------------------------------------------------------------------------
// Curvature types
// ---------------------------------------------------------------------------

/// Curvature types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurvatureType {
    /// Profile curvature (in direction of slope)
    Profile,
    /// Planform curvature (perpendicular to slope)
    Planform,
    /// Total curvature (Laplacian)
    Total,
    /// Mean curvature
    Mean,
    /// Gaussian curvature
    Gaussian,
    /// Tangential curvature (perpendicular to slope direction on surface)
    Tangential,
}

// ---------------------------------------------------------------------------
// Landform classification
// ---------------------------------------------------------------------------

/// Landform classes following Weiss (2001) TPI-based classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum LandformClass {
    /// Deep valley / canyon
    Valley = 1,
    /// Lower slope / footslope
    LowerSlope = 2,
    /// Flat area / plain
    Flat = 3,
    /// Middle slope
    MiddleSlope = 4,
    /// Upper slope / shoulder
    UpperSlope = 5,
    /// Ridge / hilltop
    Ridge = 6,
}

impl LandformClass {
    /// Returns a human-readable name for the landform class
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Valley => "Valley",
            Self::LowerSlope => "Lower Slope",
            Self::Flat => "Flat",
            Self::MiddleSlope => "Middle Slope",
            Self::UpperSlope => "Upper Slope",
            Self::Ridge => "Ridge",
        }
    }
}

// ---------------------------------------------------------------------------
// TPI Neighborhood configuration
// ---------------------------------------------------------------------------

/// Neighborhood shape for TPI computation
#[derive(Debug, Clone, Copy)]
pub enum TpiNeighborhood {
    /// Rectangular neighborhood with given odd size
    Rectangular(usize),
    /// Annular (ring) neighborhood with inner and outer radii (in cells)
    Annular {
        /// Inner radius (cells excluded closer than this)
        inner_radius: f64,
        /// Outer radius (cells beyond this are excluded)
        outer_radius: f64,
    },
}

// ===========================================================================
// TPI
// ===========================================================================

mod curvature;
mod landform;
mod roughness;
/// Computes Topographic Position Index (TPI) with configurable neighborhood
///
/// TPI = elevation - mean(neighborhood elevation)
///
/// Supports rectangular and annular (ring) neighborhoods.
/// Annular neighborhoods are particularly useful for multi-scale analysis
/// (Weiss, 2001; Jenness, 2006).
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `neighborhood_size` - Size of neighborhood (must be odd)
/// * `cell_size` - Size of each cell (for scaling)
///
/// # Errors
///
/// Returns an error if the operation fails
// Submodules
mod slope_aspect;

// Re-exports
pub use curvature::compute_curvature;
pub use landform::{
    classify_landforms, classify_landforms_multiscale, compute_spi, compute_terrain_shape_index,
    compute_twi,
};
pub use roughness::{
    compute_convergence_index, compute_roughness, compute_roughness_advanced, compute_tpi,
    compute_tpi_advanced, compute_tri, compute_tri_advanced, compute_vrm,
};
pub use slope_aspect::{compute_aspect_degrees, compute_slope_degrees};

#[cfg(test)]
mod tests;
