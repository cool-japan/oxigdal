//! Morphometric and hydrologic indices derived from a DEM.
//!
//! Builds on the `derivatives` and `hydrology` submodules to expose
//! second-order (curvature) and combined (TWI) terrain descriptors in
//! their canonical scientific form.
//!
//! - [`curvature`] — profile and plan curvature via Zevenbergen & Thorne (1987)
//! - [`twi`] — Topographic Wetness Index `ln(a / tan β)` from D-infinity flow

pub mod curvature;
pub mod twi;

pub use curvature::{CurvatureResult, compute_curvature};
pub use twi::compute_twi;
