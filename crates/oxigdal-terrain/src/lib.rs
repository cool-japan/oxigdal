//! Advanced terrain analysis and DEM processing for OxiGDAL.
//!
//! This crate provides comprehensive terrain analysis capabilities including:
//! - **Derivatives**: slope, aspect, curvature, hillshade, TPI, TRI, roughness
//! - **Hydrology**: flow direction, flow accumulation, watershed delineation, stream networks
//! - **Visibility**: viewshed analysis, line of sight
//! - **Geomorphometry**: landform classification, convergence, openness
//!
//! # Features
//!
//! - `derivatives`: Terrain derivatives (slope, aspect, etc.)
//! - `hydrology`: Hydrological analysis
//! - `visibility`: Viewshed and line of sight
//! - `geomorphometry`: Landform classification
//! - `parallel`: Parallel processing with Rayon
//!
//! # Examples
//!
//! ```rust,ignore
//! use oxigdal_terrain::derivatives::{slope_horn, SlopeUnits};
//! use scirs2_core::prelude::*;
//!
//! let dem = Array2::from_elem((100, 100), 100.0_f32);
//! let slope = slope_horn(&dem, 10.0, SlopeUnits::Degrees, None)?;
//! ```
//!
//! # Performance
//!
//! Most algorithms support optional parallelization through the `parallel` feature.
//! For large DEMs, consider using parallel variants for improved performance.

#![cfg_attr(not(feature = "std"), no_std)]
#![warn(missing_docs)]

pub mod error;

#[cfg(feature = "derivatives")]
pub mod derivatives;

#[cfg(feature = "hydrology")]
pub mod hydrology;

#[cfg(feature = "visibility")]
pub mod visibility;

#[cfg(feature = "geomorphometry")]
pub mod geomorphometry;

// Re-exports
pub use error::{Result, TerrainError};

#[cfg(feature = "derivatives")]
pub use derivatives::{
    AspectAlgorithm, CurvatureType, HillshadeAlgorithm, RoughnessMethod, SlopeAlgorithm,
    SlopeUnits, aspect, curvature, hillshade, roughness, slope, tpi, tri,
};

#[cfg(feature = "hydrology")]
pub use hydrology::{
    FlowAlgorithm, extract_streams, fill_sinks, flow_accumulation, flow_direction,
    watershed_from_point,
};

#[cfg(feature = "visibility")]
pub use visibility::{line_of_sight, viewshed_binary, viewshed_cumulative};

#[cfg(feature = "geomorphometry")]
pub use geomorphometry::{
    LandformClass, classify_iwahashi_pike, classify_weiss, convergence_index, negative_openness,
    positive_openness,
};
