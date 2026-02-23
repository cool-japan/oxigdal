//! Temporal modeling for geospatial time series.
//!
//! This module provides LSTM-based temporal modeling capabilities for
//! geospatial time series analysis, including:
//!
//! - NDVI trend forecasting
//! - Crop yield prediction
//! - Multi-variate time series analysis
//! - Temporal gap filling
//! - Drought monitoring and prediction
//!
//! # Features
//!
//! This module requires the `temporal` feature to be enabled.
//!
//! # Examples
//!
//! ```rust,no_run
//! use oxigdal_ml::temporal::forecasting::{TemporalForecaster, ForecastConfig};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a forecaster for NDVI prediction
//! let config = ForecastConfig {
//!     input_features: 10,
//!     hidden_dim: 128,
//!     num_layers: 2,
//!     forecast_horizon: 12,
//!     ..Default::default()
//! };
//!
//! let forecaster = TemporalForecaster::new(config)?;
//! # Ok(())
//! # }
//! ```

pub mod forecasting;

pub use forecasting::{ForecastConfig, ForecastResult, TemporalForecaster};
