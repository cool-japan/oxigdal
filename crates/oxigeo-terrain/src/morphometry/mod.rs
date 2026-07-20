//! Morphometric and hydrologic indices derived from a DEM.
//!
//! Builds on the `derivatives` and `hydrology` submodules to expose
//! second-order (curvature) and combined (TWI) terrain descriptors in
//! their canonical scientific form.
//!
//! - [`curvature`] — profile and plan curvature via Zevenbergen & Thorne (1987)
//! - [`twi`] — Topographic Wetness Index `ln(a / tan β)` from D-infinity flow
//! - [`valley_ridge`] — valley depth and ridge height via Laplace relaxation
//! - [`profile`] — terrain profile extraction along a polyline

pub mod curvature;
pub mod profile;
pub mod twi;
pub mod valley_ridge;

pub use curvature::{CurvatureResult, compute_curvature};
pub use profile::{ProfilePoint, TerrainProfile, extract_profile};
pub use twi::compute_twi;
pub use valley_ridge::{ridge_height, valley_depth};
