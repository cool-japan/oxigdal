//! Spectral indices for remote sensing
//!
//! This module provides implementations of 20+ spectral indices for vegetation,
//! water, urban, and burn analysis.

pub mod burn;
pub mod urban;
pub mod vegetation;
pub mod water;

pub use burn::{d_nbr, nbr, nbr2};
pub use urban::{ibi, ndbi, ui};
pub use vegetation::{ci, evi, evi2, gndvi, grvi, msavi, ndmi, ndvi, ndwi, osavi, savi};
pub use water::{awei, mndwi, wri};
