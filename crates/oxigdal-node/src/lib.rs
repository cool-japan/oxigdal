//! Node.js bindings for OxiGDAL
//!
//! This crate provides comprehensive Node.js bindings for the OxiGDAL ecosystem,
//! enabling pure Rust geospatial processing from JavaScript/TypeScript with
//! zero-copy Buffer integration and full async/await support.
//!
//! # Features
//!
//! - **Raster I/O**: Read and write GeoTIFF, COG, and other raster formats
//! - **Vector I/O**: GeoJSON support with full geometry operations
//! - **Algorithms**: Resampling, terrain analysis, calculator, statistics
//! - **Async/Await**: Promise-based async operations for I/O and processing
//! - **Buffer Integration**: Zero-copy data transfer with Node.js Buffers
//! - **TypeScript**: Comprehensive TypeScript definitions included
//!
//! # Example Usage
//!
//! ```javascript
//! const oxigdal = require('@cooljapan/oxigdal-node');
//!
//! // Open a raster file
//! const dataset = oxigdal.openRaster('input.tif');
//! console.log(`Size: ${dataset.width}x${dataset.height}`);
//!
//! // Read a band
//! const band = dataset.readBand(0);
//! const stats = band.statistics();
//! console.log(`Mean: ${stats.mean}, StdDev: ${stats.stddev}`);
//!
//! // Compute hillshade
//! const hillshade = oxigdal.hillshade(band, 315, 45, 1.0);
//!
//! // Save result
//! const output = oxigdal.createRaster(dataset.width, dataset.height, 1, 'uint8');
//! output.writeBand(0, hillshade);
//! output.save('hillshade.tif');
//! ```
//!
//! # Async Example
//!
//! ```javascript
//! const oxigdal = require('@cooljapan/oxigdal-node');
//!
//! async function processRaster() {
//!   const dataset = await oxigdal.openRasterAsync('input.tif');
//!   const band = dataset.readBand(0);
//!   const slope = await oxigdal.slopeAsync(band, 1.0, false);
//!
//!   const output = oxigdal.createRaster(dataset.width, dataset.height, 1, 'float32');
//!   output.writeBand(0, slope);
//!   await oxigdal.saveRasterAsync(output, 'slope.tif');
//! }
//!
//! processRaster().catch(console.error);
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]
#![deny(clippy::unwrap_used)]
#![allow(clippy::module_name_repetitions)]

mod algorithms;
mod async_ops;
mod buffer;
mod error;
mod raster;
mod vector;

use napi_derive::napi;

/// Returns the version of OxiGDAL
#[napi]
pub fn version() -> String {
    oxigdal_core::VERSION.to_string()
}

/// Returns the OxiGDAL name
#[napi]
pub fn name() -> String {
    "OxiGDAL Node.js Bindings".to_string()
}

/// Module information
#[napi(object)]
pub struct ModuleInfo {
    /// Version string
    pub version: String,
    /// Module name
    pub name: String,
    /// Build information
    pub build_info: String,
    /// Supported formats
    pub formats: Vec<String>,
}

/// Returns module information
#[napi]
pub fn get_info() -> ModuleInfo {
    ModuleInfo {
        version: oxigdal_core::VERSION.to_string(),
        name: "OxiGDAL Node.js Bindings".to_string(),
        build_info: format!(
            "Built with Rust {} on {}",
            env!("CARGO_PKG_RUST_VERSION"),
            std::env::consts::OS
        ),
        formats: vec![
            "GeoTIFF".to_string(),
            "COG".to_string(),
            "GeoJSON".to_string(),
        ],
    }
}

/// Data type constants
#[napi(object)]
pub struct DataTypes {
    /// Unsigned 8-bit integer
    pub uint8: String,
    /// Signed 16-bit integer
    pub int16: String,
    /// Unsigned 16-bit integer
    pub uint16: String,
    /// Signed 32-bit integer
    pub int32: String,
    /// Unsigned 32-bit integer
    pub uint32: String,
    /// 32-bit floating point
    pub float32: String,
    /// 64-bit floating point
    pub float64: String,
}

/// Returns available data types
#[napi]
pub fn get_data_types() -> DataTypes {
    DataTypes {
        uint8: "uint8".to_string(),
        int16: "int16".to_string(),
        uint16: "uint16".to_string(),
        int32: "int32".to_string(),
        uint32: "uint32".to_string(),
        float32: "float32".to_string(),
        float64: "float64".to_string(),
    }
}

/// Resampling method constants
#[napi(object)]
pub struct ResamplingMethods {
    /// Nearest neighbor (fast, preserves exact values)
    pub nearest_neighbor: String,
    /// Bilinear interpolation (smooth, good for continuous data)
    pub bilinear: String,
    /// Bicubic interpolation (high quality, slower)
    pub bicubic: String,
    /// Lanczos resampling (highest quality, expensive)
    pub lanczos: String,
}

/// Returns available resampling methods
#[napi]
pub fn get_resampling_methods() -> ResamplingMethods {
    ResamplingMethods {
        nearest_neighbor: "NearestNeighbor".to_string(),
        bilinear: "Bilinear".to_string(),
        bicubic: "Bicubic".to_string(),
        lanczos: "Lanczos".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        let ver = version();
        assert!(!ver.is_empty());
    }

    #[test]
    fn test_info() {
        let info = get_info();
        assert!(!info.version.is_empty());
        assert!(!info.formats.is_empty());
    }

    #[test]
    fn test_data_types() {
        let types = get_data_types();
        assert_eq!(types.uint8, "uint8");
        assert_eq!(types.float32, "float32");
    }
}
