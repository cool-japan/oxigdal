//! Radiometric corrections
//!
//! This module provides radiometric calibration and correction algorithms for remote sensing data.

pub mod atmospheric;
pub mod brdf;
pub mod calibration;

pub use atmospheric::{AtmosphericCorrection, DarkObjectSubtraction};
pub use brdf::{BrdfNormalization, RossThickLiSparse};
pub use calibration::{RadiometricCalibration, ThermalCalibration};
