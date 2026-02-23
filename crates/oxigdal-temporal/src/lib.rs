//! OxiGDAL Temporal Analysis
//!
//! Comprehensive multi-temporal raster analysis library for geospatial time series.
//!
//! This crate provides advanced temporal analysis capabilities for raster data including:
//! - Time-indexed raster collections with lazy loading
//! - Temporal compositing (median, mean, max NDVI, quality-weighted)
//! - Temporal interpolation and gap filling
//! - Temporal aggregation (daily, weekly, monthly, yearly, rolling)
//! - Change detection (BFAST, LandTrendr, differencing)
//! - Trend analysis (Mann-Kendall, Sen's slope, linear regression)
//! - Seasonality detection and decomposition
//! - Anomaly detection
//! - Time series forecasting
//! - Breakpoint detection
//! - Multi-dimensional data cube operations
//!
//! # Features
//!
//! - `timeseries` - Time series raster collections
//! - `compositing` - Temporal compositing methods
//! - `interpolation` - Temporal interpolation
//! - `aggregation` - Temporal aggregation
//! - `change_detection` - Change detection algorithms
//! - `trend_analysis` - Trend analysis methods
//! - `phenology` - Vegetation phenology
//! - `datacube` - Data cube operations
//! - `zarr` - Zarr storage integration
//! - `parallel` - Parallel processing support
//!
//! # Examples
//!
//! ## Creating a Time Series
//!
//! ```rust,no_run
//! use oxigdal_temporal::timeseries::{TimeSeriesRaster, TemporalMetadata};
//! use chrono::{DateTime, NaiveDate, Utc};
//! use scirs2_core::ndarray::Array3;
//!
//! let mut ts = TimeSeriesRaster::new();
//!
//! let dt = DateTime::from_timestamp(1640995200, 0).expect("valid timestamp");
//! let date = NaiveDate::from_ymd_opt(2022, 1, 1).expect("valid date");
//! let metadata = TemporalMetadata::new(dt, date);
//! let data = Array3::zeros((100, 100, 3));
//!
//! ts.add_raster(metadata, data).expect("should add");
//! ```
//!
//! ## Temporal Compositing
//!
//! ```rust,no_run
//! use oxigdal_temporal::compositing::{TemporalCompositor, CompositingConfig, CompositingMethod};
//! # use oxigdal_temporal::timeseries::TimeSeriesRaster;
//! # let ts = TimeSeriesRaster::new();
//!
//! let config = CompositingConfig {
//!     method: CompositingMethod::Median,
//!     max_cloud_cover: Some(20.0),
//!     ..Default::default()
//! };
//!
//! let composite = TemporalCompositor::composite(&ts, &config)
//!     .expect("should create composite");
//! ```
//!
//! ## Trend Analysis
//!
//! ```rust,no_run
//! use oxigdal_temporal::analysis::trend::{TrendAnalyzer, TrendMethod};
//! # use oxigdal_temporal::timeseries::TimeSeriesRaster;
//! # let ts = TimeSeriesRaster::new();
//!
//! let result = TrendAnalyzer::analyze(&ts, TrendMethod::MannKendall)
//!     .expect("should analyze trend");
//! ```
//!
//! ## Change Detection
//!
//! ```rust,no_run
//! use oxigdal_temporal::change::{ChangeDetector, ChangeDetectionConfig, ChangeDetectionMethod};
//! # use oxigdal_temporal::timeseries::TimeSeriesRaster;
//! # let ts = TimeSeriesRaster::new();
//!
//! let config = ChangeDetectionConfig {
//!     method: ChangeDetectionMethod::BFAST,
//!     ..Default::default()
//! };
//!
//! let changes = ChangeDetector::detect(&ts, &config)
//!     .expect("should detect changes");
//! ```

#![cfg_attr(not(feature = "std"), no_std)]
#![warn(missing_docs)]
#![warn(clippy::unwrap_used)]
#![warn(clippy::expect_used)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod error;

// Core time series module
#[cfg(feature = "timeseries")]
pub mod timeseries;

// Stack module (always available)
pub mod stack;

// Analysis modules (requires timeseries)
#[cfg(feature = "timeseries")]
pub mod analysis;

// Change detection modules
#[cfg(feature = "change_detection")]
pub mod change;

// Compositing modules
#[cfg(feature = "compositing")]
pub mod compositing;

// Gap filling modules (requires timeseries)
#[cfg(feature = "timeseries")]
pub mod gap_filling;

// Aggregation module
#[cfg(feature = "aggregation")]
pub mod aggregation;

// Phenology module
#[cfg(feature = "phenology")]
pub mod phenology;

// Interpolation module (legacy, use gap_filling instead)
#[cfg(feature = "interpolation")]
pub mod interpolation {
    //! Legacy interpolation module - use gap_filling instead
    pub use crate::gap_filling::*;
}

// Trend module (legacy, use analysis::trend instead)
#[cfg(feature = "trend_analysis")]
pub mod trend {
    //! Legacy trend module - use analysis::trend instead
    pub use crate::analysis::trend::*;
}

// Re-exports for convenience
pub use error::{Result, TemporalError};

#[cfg(feature = "timeseries")]
pub use timeseries::{
    CubeDimensions, CubeMetadata, DataCube, PixelStatistics, TemporalMetadata, TemporalRasterEntry,
    TemporalResolution, TimeSeriesRaster, TimeSeriesStats,
};

pub use stack::{InterpolationMethod, RasterStack, StackConfig, StackMetadata};

// Analysis re-exports
#[cfg(feature = "timeseries")]
pub use analysis::anomaly::{AnomalyDetector, AnomalyMethod, AnomalyResult};
#[cfg(feature = "timeseries")]
pub use analysis::forecast::{ForecastMethod, ForecastParams, ForecastResult, Forecaster};
#[cfg(feature = "timeseries")]
pub use analysis::seasonality::{SeasonalityAnalyzer, SeasonalityMethod, SeasonalityResult};
#[cfg(feature = "timeseries")]
pub use analysis::trend::{TrendAnalyzer, TrendMethod, TrendResult};

#[cfg(feature = "compositing")]
pub use compositing::{CompositeResult, CompositingConfig, CompositingMethod, TemporalCompositor};

#[cfg(feature = "change_detection")]
pub use change::{
    BreakpointDetector, BreakpointMethod, BreakpointParams, BreakpointResult,
    ChangeDetectionConfig, ChangeDetectionMethod, ChangeDetectionResult, ChangeDetector,
};

#[cfg(feature = "timeseries")]
pub use gap_filling::{GapFillMethod, GapFillParams, GapFillResult, GapFiller};

#[cfg(feature = "aggregation")]
pub use aggregation::{
    AggregationConfig, AggregationResult, AggregationStatistic, TemporalAggregator, TemporalWindow,
};

#[cfg(feature = "phenology")]
pub use phenology::{PhenologyConfig, PhenologyExtractor, PhenologyMethod, PhenologyMetrics};

/// Library version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Get library version information
#[must_use]
pub fn version() -> &'static str {
    VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        let v = version();
        assert!(!v.is_empty());
        assert!(v.contains('.'));
    }

    #[test]
    fn test_error_types() {
        let err = TemporalError::invalid_input("test");
        assert!(format!("{}", err).contains("test"));
    }
}
