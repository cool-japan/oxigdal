//! Python bindings for OxiGDAL
//!
//! This crate provides comprehensive Python bindings for the OxiGDAL ecosystem,
//! enabling pure Rust geospatial processing from Python with NumPy integration.
//!
//! # Features
//!
//! - Raster I/O with NumPy array conversion
//! - Vector operations with GeoJSON support
//! - Coordinate transformations
//! - Raster algorithms (calculator, statistics, filters)
//! - Type stubs for IDE support
//! - Zero-copy data transfer where possible
//!
//! # Example Usage
//!
//! ```python
//! import oxigdal
//! import numpy as np
//!
//! # Open a raster file
//! ds = oxigdal.open("input.tif")
//! data = ds.read_band(1)  # Returns NumPy array
//!
//! # Get metadata
//! metadata = ds.get_metadata()
//! print(f"Size: {metadata['width']}x{metadata['height']}")
//!
//! # Process data
//! result = oxigdal.calc("A * 2", A=data)
//!
//! # Write output
//! oxigdal.write("output.tif", result, metadata=metadata)
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]
#![deny(clippy::unwrap_used)]
#![allow(clippy::module_name_repetitions)]

mod algorithms;
mod array;
mod dataset;
mod error;
mod expression;
mod raster;
mod vector;

use pyo3::prelude::*;

use crate::algorithms::*;
use crate::dataset::Dataset;
use crate::error::OxiGdalPyError;
use crate::raster::{
    RasterMetadataPy, WindowPy, build_overviews, calc, clip, create_raster, get_metadata, merge,
    open_raster, read, read_bands, resample, translate, warp, write,
};
use crate::vector::{
    area, buffer_geometry, centroid, clip_by_bbox, contains, convex_hull, crosses, difference,
    disjoint, dissolve, distance, envelope, intersection, intersects, is_valid, length, make_valid,
    merge_polygons, overlaps, read_geojson, simplify, symmetric_difference, touches, transform,
    union, within, write_geojson,
};
#[cfg(feature = "shapefile")]
use crate::vector::{read_shapefile, write_shapefile};

/// Opens a geospatial dataset.
///
/// Args:
///     path (str): Path to the file to open (local or remote URL)
///     mode (str, optional): Open mode - "r" for read (default), "w" for write
///
/// Returns:
///     Dataset: An opened dataset object
///
/// Raises:
///     IOError: If the file cannot be opened
///     ValueError: If the format is not supported
///
/// Example:
///     >>> ds = oxigdal.open("input.tif")
///     >>> print(ds.width, ds.height)
#[pyfunction]
#[pyo3(signature = (path, mode="r"))]
fn open(path: &str, mode: &str) -> PyResult<Dataset> {
    Dataset::open(path, mode)
}

/// Returns the version of OxiGDAL.
///
/// Returns:
///     str: Version string
///
/// Example:
///     >>> oxigdal.version()
///     '0.1.0'
#[pyfunction]
fn version() -> &'static str {
    oxigdal_core::VERSION
}

/// Python module for OxiGDAL.
///
/// This module provides Python bindings for the OxiGDAL pure Rust geospatial
/// data abstraction library.
#[pymodule]
fn _oxigdal(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Register module metadata
    m.add("__version__", oxigdal_core::VERSION)?;
    m.add("__author__", "COOLJAPAN OU (Team Kitasan)")?;

    // Core functions
    m.add_function(wrap_pyfunction!(open, m)?)?;
    m.add_function(wrap_pyfunction!(version, m)?)?;

    // Raster I/O functions
    m.add_function(wrap_pyfunction!(open_raster, m)?)?;
    m.add_function(wrap_pyfunction!(create_raster, m)?)?;
    m.add_function(wrap_pyfunction!(read, m)?)?;
    m.add_function(wrap_pyfunction!(read_bands, m)?)?;
    m.add_function(wrap_pyfunction!(write, m)?)?;
    m.add_function(wrap_pyfunction!(get_metadata, m)?)?;

    // Raster processing functions
    m.add_function(wrap_pyfunction!(calc, m)?)?;
    m.add_function(wrap_pyfunction!(warp, m)?)?;
    m.add_function(wrap_pyfunction!(resample, m)?)?;
    m.add_function(wrap_pyfunction!(clip, m)?)?;
    m.add_function(wrap_pyfunction!(merge, m)?)?;
    m.add_function(wrap_pyfunction!(translate, m)?)?;
    m.add_function(wrap_pyfunction!(build_overviews, m)?)?;

    // Vector I/O functions
    m.add_function(wrap_pyfunction!(read_geojson, m)?)?;
    m.add_function(wrap_pyfunction!(write_geojson, m)?)?;
    #[cfg(feature = "shapefile")]
    m.add_function(wrap_pyfunction!(read_shapefile, m)?)?;
    #[cfg(feature = "shapefile")]
    m.add_function(wrap_pyfunction!(write_shapefile, m)?)?;

    // Vector geometry operations
    m.add_function(wrap_pyfunction!(buffer_geometry, m)?)?;
    m.add_function(wrap_pyfunction!(union, m)?)?;
    m.add_function(wrap_pyfunction!(intersection, m)?)?;
    m.add_function(wrap_pyfunction!(difference, m)?)?;
    m.add_function(wrap_pyfunction!(symmetric_difference, m)?)?;
    m.add_function(wrap_pyfunction!(simplify, m)?)?;
    m.add_function(wrap_pyfunction!(centroid, m)?)?;
    m.add_function(wrap_pyfunction!(convex_hull, m)?)?;
    m.add_function(wrap_pyfunction!(envelope, m)?)?;

    // Vector spatial predicates
    m.add_function(wrap_pyfunction!(intersects, m)?)?;
    m.add_function(wrap_pyfunction!(contains, m)?)?;
    m.add_function(wrap_pyfunction!(within, m)?)?;
    m.add_function(wrap_pyfunction!(touches, m)?)?;
    m.add_function(wrap_pyfunction!(overlaps, m)?)?;
    m.add_function(wrap_pyfunction!(crosses, m)?)?;
    m.add_function(wrap_pyfunction!(disjoint, m)?)?;

    // Vector measurements
    m.add_function(wrap_pyfunction!(area, m)?)?;
    m.add_function(wrap_pyfunction!(length, m)?)?;
    m.add_function(wrap_pyfunction!(distance, m)?)?;

    // Vector utilities
    m.add_function(wrap_pyfunction!(is_valid, m)?)?;
    m.add_function(wrap_pyfunction!(make_valid, m)?)?;
    m.add_function(wrap_pyfunction!(transform, m)?)?;
    m.add_function(wrap_pyfunction!(clip_by_bbox, m)?)?;
    m.add_function(wrap_pyfunction!(merge_polygons, m)?)?;
    m.add_function(wrap_pyfunction!(dissolve, m)?)?;

    // Algorithm functions - Statistics
    m.add_function(wrap_pyfunction!(statistics, m)?)?;
    m.add_function(wrap_pyfunction!(histogram, m)?)?;

    // Algorithm functions - Filters
    m.add_function(wrap_pyfunction!(convolve, m)?)?;
    m.add_function(wrap_pyfunction!(gaussian_blur, m)?)?;
    m.add_function(wrap_pyfunction!(median_filter, m)?)?;

    // Algorithm functions - Morphology
    m.add_function(wrap_pyfunction!(erosion, m)?)?;
    m.add_function(wrap_pyfunction!(dilation, m)?)?;
    m.add_function(wrap_pyfunction!(opening, m)?)?;
    m.add_function(wrap_pyfunction!(closing, m)?)?;

    // Algorithm functions - Spectral Indices
    m.add_function(wrap_pyfunction!(ndvi, m)?)?;
    m.add_function(wrap_pyfunction!(evi, m)?)?;
    m.add_function(wrap_pyfunction!(ndwi, m)?)?;

    // Algorithm functions - Classification
    m.add_function(wrap_pyfunction!(kmeans_classify, m)?)?;
    m.add_function(wrap_pyfunction!(supervised_classify, m)?)?;

    // Algorithm functions - Edge Detection
    m.add_function(wrap_pyfunction!(sobel_edges, m)?)?;
    m.add_function(wrap_pyfunction!(canny_edges, m)?)?;

    // Classes
    m.add_class::<Dataset>()?;
    m.add_class::<RasterMetadataPy>()?;
    m.add_class::<WindowPy>()?;
    m.add_class::<OxiGdalPyError>()?;

    Ok(())
}
