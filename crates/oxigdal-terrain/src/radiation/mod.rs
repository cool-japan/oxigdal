//! Topographic solar radiation modeling.
//!
//! This module models solar irradiance over a Digital Elevation Model (DEM),
//! following the rigorous approach used by GRASS GIS `r.sun` and the ArcGIS
//! Area Solar Radiation toolset.
//!
//! It provides:
//! - [`solar_position`]: rigorous solar geometry (declination, hour angle,
//!   zenith, altitude, azimuth) for a given latitude, day-of-year and solar time.
//! - [`hillshade_at`]: instantaneous shaded relief for an explicit sun position
//!   (the "hillshade with sun position" deliverable), returning the cosine of the
//!   angle of incidence in `[0, 1]`.
//! - [`solar_radiation`]: time-integrated insolation (Wh/m²) across a day with the
//!   sun moving over time, accounting for slope, aspect, cast shadows, the direct
//!   beam (Beer-Lambert atmospheric attenuation) and an optional isotropic diffuse
//!   sky component.
//!
//! All formulas document their literature source inline. NoData cells (NaN, or an
//! explicit NoData value matched in the DEM) propagate as `NaN` through every
//! output array.

pub mod solar;

pub use solar::{
    SolarOptions, SolarPosition, SolarRadiationResult, hillshade_at, solar_position,
    solar_radiation,
};
