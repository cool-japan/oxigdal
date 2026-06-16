//! Visibility analysis module.

pub mod fresnel;
pub mod los;
pub mod viewshed;

pub use fresnel::{ClearanceSample, FresnelResult, fresnel_clearance, fresnel_zone_radius};
pub use los::line_of_sight;
pub use viewshed::{viewshed_binary, viewshed_cumulative};

#[cfg(feature = "parallel")]
pub use viewshed::viewshed_cumulative_parallel;
