//! Spatial Regression Module
//!
//! This module provides spatial regression methods that allow model
//! relationships to vary across geographic space:
//! - Geographically Weighted Regression (GWR)
//!
//! Unlike a global regression that fits a single set of coefficients for the
//! entire study area, these models estimate local relationships, producing
//! per-location coefficient surfaces that can reveal spatial non-stationarity.

pub mod gwr;

pub use gwr::{GwrBandwidth, GwrKernel, GwrOptions, GwrResult, gwr_fit};
