//! OxiGDAL Sensors - Remote Sensing and Satellite Sensor Data Processing
//!
//! This crate provides comprehensive support for remote sensing and satellite sensor data processing,
//! including sensor definitions, radiometric corrections, spectral indices, and image classification.
//!
//! # Features
//!
//! ## Sensor Support
//!
//! Comprehensive sensor definitions for major satellite platforms:
//! - **Landsat**: 5 TM, 7 ETM+, 8/9 OLI/TIRS
//! - **Sentinel**: 2 MSI (optical), 1 SAR
//! - **MODIS**: Terra/Aqua (36 bands)
//! - **ASTER**: VNIR, SWIR, and TIR subsystems
//!
//! ```
//! use oxigdal_sensors::sensors::landsat;
//!
//! let sensor = landsat::landsat8_oli_tirs();
//! assert_eq!(sensor.bands.len(), 11);
//!
//! let red_band = sensor.get_band_by_common_name("Red");
//! assert!(red_band.is_some());
//! ```
//!
//! ## Radiometric Calibration
//!
//! Convert Digital Numbers (DN) to physical units:
//!
//! ```
//! use oxigdal_sensors::radiometry::calibration::{RadiometricCalibration, earth_sun_distance};
//! use scirs2_core::ndarray::array;
//!
//! let cal = RadiometricCalibration::new(0.00002, 0.0)
//!     .with_solar_irradiance(1554.0);
//!
//! let dn = array![[1000.0, 2000.0], [3000.0, 4000.0]];
//! let radiance = cal.dn_to_radiance(&dn.view());
//!
//! let doy = earth_sun_distance(180);
//! assert!(doy.is_ok());
//! ```
//!
//! ## Atmospheric Correction
//!
//! Multiple atmospheric correction methods:
//!
//! ```
//! use oxigdal_sensors::radiometry::atmospheric::{DarkObjectSubtraction, AtmosphericCorrection};
//! use scirs2_core::ndarray::array;
//!
//! let dos = DarkObjectSubtraction::default_params();
//! let toa = array![[0.05, 0.10], [0.15, 0.20]];
//!
//! let corrected = dos.correct(&toa.view());
//! assert!(corrected.is_ok());
//! ```
//!
//! ## Spectral Indices (20+)
//!
//! Comprehensive collection of spectral indices:
//!
//! ### Vegetation Indices
//! - NDVI, EVI, EVI2, SAVI, MSAVI, OSAVI
//! - GNDVI, GRVI, CI (Chlorophyll Index)
//! - NDWI, NDMI (water content)
//!
//! ```
//! use oxigdal_sensors::indices::vegetation::{ndvi, evi, savi};
//! use scirs2_core::ndarray::array;
//!
//! let nir = array![[0.5, 0.6], [0.7, 0.8]];
//! let red = array![[0.1, 0.1], [0.1, 0.1]];
//!
//! let ndvi_result = ndvi(&nir.view(), &red.view());
//! assert!(ndvi_result.is_ok());
//!
//! let blue = array![[0.05, 0.05], [0.05, 0.05]];
//! let evi_result = evi(&nir.view(), &red.view(), &blue.view());
//! assert!(evi_result.is_ok());
//! ```
//!
//! ### Burn Indices
//! - nbr, d_nbr, nbr2
//!
//! ```
//! use oxigdal_sensors::indices::burn::{nbr, d_nbr};
//! use scirs2_core::ndarray::array;
//!
//! let nir = array![[0.5, 0.6]];
//! let swir2 = array![[0.2, 0.2]];
//!
//! let nbr_result = nbr(&nir.view(), &swir2.view());
//! assert!(nbr_result.is_ok());
//! ```
//!
//! ### Urban Indices
//! - NDBI, UI, IBI
//!
//! ### Water Indices
//! - MNDWI, AWEI, WRI
//!
//! ## Pan-Sharpening
//!
//! Multiple pan-sharpening algorithms:
//! - Brovey Transform
//! - IHS (Intensity-Hue-Saturation)
//! - PCA (Principal Component Analysis)
//!
//! ## Image Classification
//!
//! - Unsupervised: K-Means, ISODATA
//! - Supervised: Maximum Likelihood
//!
//! # COOLJAPAN Policy Compliance
//!
//! This crate adheres to COOLJAPAN ecosystem policies:
//!
//! - **Pure Rust**: 100% Pure Rust implementation, no C/Fortran dependencies
//! - **No unwrap()**: All error cases are properly handled with `Result<T, E>`
//! - **SciRS2 Integration**: Uses `scirs2-core` for scientific computing
//! - **Comprehensive Error Handling**: Custom error types for all failure modes
//! - **File Size Policy**: All source files are kept under 2000 lines
//! - **Latest Crates**: Uses latest versions from crates.io
//! - **Workspace Policy**: Follows workspace dependency management
//!
//! # Performance
//!
//! All algorithms are optimized for production use with:
//! - Efficient memory usage
//! - Vectorized operations using ndarray
//! - Optional parallel processing (enable `parallel` feature)
//! - Numerical stability checks
//!
//! # Safety
//!
//! This crate is designed with safety in mind:
//! - No unsafe code (except what's in dependencies)
//! - Comprehensive input validation
//! - Proper handling of edge cases
//! - Clear error messages for debugging

#![warn(missing_docs)]
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
#![allow(clippy::too_many_lines)]
#![allow(clippy::doc_markdown)]

pub mod classification;
pub mod error;
pub mod indices;
pub mod pan_sharpening;
pub mod radiometry;
pub mod sensors;

// Re-export commonly used items
pub use error::{Result, SensorError};

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
        assert_eq!(NAME, "oxigdal-sensors");
    }
}
