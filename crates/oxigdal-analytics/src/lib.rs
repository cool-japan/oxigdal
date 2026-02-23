//! OxiGDAL Analytics - Advanced Geospatial Analytics
//!
//! This crate provides advanced analytics capabilities for geospatial data processing,
//! including time series analysis, spatial clustering, hotspot detection, change detection,
//! interpolation, and advanced zonal statistics.
//!
//! # Features
//!
//! ## Time Series Analysis
//!
//! Analyze temporal patterns in geospatial data:
//! - Trend detection (Mann-Kendall test, linear regression)
//! - Anomaly detection (Z-score, IQR, Modified Z-score)
//! - Seasonal decomposition
//! - Gap filling and smoothing
//!
//! ```
//! use oxigdal_analytics::timeseries::{TrendDetector, TrendMethod};
//! use scirs2_core::ndarray::array;
//!
//! let values = array![1.0, 2.0, 3.0, 4.0, 5.0];
//! let detector = TrendDetector::new(TrendMethod::MannKendall, 0.05);
//! let result = detector.detect(&values.view()).expect("Failed to detect trend");
//!
//! assert_eq!(result.direction, 1); // Positive trend
//! assert!(result.significant);
//! ```
//!
//! ## Spatial Clustering
//!
//! Identify clusters and outliers in spatial data:
//! - K-means clustering for image classification
//! - DBSCAN for spatial outlier detection
//! - Cluster validation metrics
//!
//! ```
//! use oxigdal_analytics::clustering::{KMeansClusterer, DbscanClusterer};
//! use scirs2_core::ndarray::array;
//!
//! let data = array![
//!     [0.0, 0.0],
//!     [0.1, 0.1],
//!     [10.0, 10.0],
//!     [10.1, 10.1],
//! ];
//!
//! // K-means clustering
//! let kmeans = KMeansClusterer::new(2, 100, 1e-4);
//! let result = kmeans.fit(&data.view()).expect("Failed to fit K-means clustering");
//! assert_eq!(result.centers.nrows(), 2);
//! ```
//!
//! ## Hotspot Analysis
//!
//! Detect spatial clusters of high or low values:
//! - Getis-Ord Gi* statistic (hot spot analysis)
//! - Moran's I (global and local spatial autocorrelation)
//! - LISA (Local Indicators of Spatial Association)
//!
//! ```
//! use oxigdal_analytics::hotspot::{GetisOrdGiStar, SpatialWeights};
//! use scirs2_core::ndarray::array;
//!
//! let values = array![1.0, 1.0, 10.0, 10.0];
//! let adj = array![
//!     [1.0, 1.0, 0.0, 0.0],
//!     [1.0, 1.0, 1.0, 0.0],
//!     [0.0, 1.0, 1.0, 1.0],
//!     [0.0, 0.0, 1.0, 1.0],
//! ];
//!
//! let weights = SpatialWeights::from_adjacency(adj).expect("Failed to create spatial weights from adjacency matrix");
//! let gi_star = GetisOrdGiStar::new(0.05);
//! let result = gi_star.calculate(&values.view(), &weights).expect("Failed to calculate Getis-Ord Gi* statistic");
//! ```
//!
//! ## Change Detection
//!
//! Detect changes between multi-temporal images:
//! - Image differencing
//! - Change Vector Analysis (CVA)
//! - Principal Component Analysis (PCA)
//! - Automatic threshold optimization (Otsu's method)
//!
//! ```
//! use oxigdal_analytics::change::{ChangeDetector, ChangeMethod};
//! use scirs2_core::ndarray::Array;
//!
//! let before = Array::from_shape_vec((2, 2, 1), vec![1.0, 2.0, 3.0, 4.0]).expect("Failed to create before array");
//! let after = Array::from_shape_vec((2, 2, 1), vec![2.0, 3.0, 4.0, 5.0]).expect("Failed to create after array");
//!
//! let detector = ChangeDetector::new(ChangeMethod::CVA);
//! let result = detector.detect(&before.view(), &after.view()).expect("Failed to detect changes");
//!
//! assert_eq!(result.magnitude.dim(), (2, 2));
//! ```
//!
//! ## Interpolation
//!
//! Create continuous surfaces from point data:
//! - Inverse Distance Weighting (IDW)
//! - Kriging (Ordinary and Universal)
//! - Variogram modeling
//! - Cross-validation
//!
//! ```
//! use oxigdal_analytics::interpolation::{IdwInterpolator, KrigingInterpolator, KrigingType, Variogram, VariogramModel};
//! use scirs2_core::ndarray::array;
//!
//! let points = array![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
//! let values = array![1.0, 2.0, 3.0];
//! let targets = array![[0.5, 0.5]];
//!
//! // IDW interpolation
//! let idw = IdwInterpolator::new(2.0);
//! let result = idw.interpolate(&points, &values.view(), &targets).expect("Failed to perform IDW interpolation");
//! ```
//!
//! ## Advanced Zonal Statistics
//!
//! Calculate statistics for regions:
//! - Multiple statistics (mean, median, min, max, std, etc.)
//! - Weighted statistics
//! - Multi-band support
//! - Custom aggregation functions
//!
//! ```
//! use oxigdal_analytics::zonal::{ZonalCalculator, ZonalStatistic};
//! use scirs2_core::ndarray::array;
//!
//! let values = array![[1.0, 2.0], [3.0, 4.0]];
//! let zones = array![[1, 1], [1, 1]];
//!
//! let calculator = ZonalCalculator::new();
//! let result = calculator.calculate(&values.view(), &zones.view()).expect("Failed to calculate zonal statistics");
//!
//! let zone1_stats = &result.zones[&1];
//! assert!(zone1_stats.contains_key(&ZonalStatistic::Mean));
//! ```
//!
//! # COOLJAPAN Policy Compliance
//!
//! This crate adheres to COOLJAPAN ecosystem policies:
//!
//! - **Pure Rust**: 100% Pure Rust implementation, no C/Fortran dependencies
//! - **No unwrap()**: All error cases are properly handled with `Result<T, E>`
//! - **SciRS2 Integration**: Uses `scirs2-core` for scientific computing instead of ndarray directly
//! - **Comprehensive Error Handling**: Custom error types for all failure modes
//! - **File Size Policy**: All source files are kept under 2000 lines
//! - **Latest Crates**: Uses latest versions from crates.io
//! - **Workspace Policy**: Follows workspace dependency management
//!
//! # Performance
//!
//! All algorithms are optimized for production use with:
//! - Efficient memory usage
//! - Vectorized operations where possible
//! - Optional parallel processing (enable `parallel` feature)
//! - Numerical stability checks
//!
//! # Safety
//!
//! This crate is designed with safety in mind:
//! - No unsafe code (except what's in dependencies)
//! - Comprehensive input validation
//! - Proper handling of edge cases (empty data, singular matrices, etc.)
//! - Clear error messages for debugging

#![warn(clippy::all)]
// Pedantic disabled to reduce noise - default clippy::all is sufficient
// #![warn(clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::panic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::similar_names)]
#![allow(clippy::too_many_lines)] // Analytics functions can be complex
// Allow partial documentation during development
#![allow(missing_docs)]
// Allow dead code for internal structures
#![allow(dead_code)]
// Allow needless range loop for explicit indexing
#![allow(clippy::needless_range_loop)]
// Allow unused imports in analytics modules
#![allow(unused_imports)]

pub mod change;
pub mod clustering;
pub mod error;
pub mod hotspot;
pub mod interpolation;
pub mod timeseries;
pub mod zonal;

// Re-export commonly used items
pub use error::{AnalyticsError, Result};

/// Crate version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Crate name
pub const NAME: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
        assert_eq!(NAME, "oxigdal-analytics");
    }
}
