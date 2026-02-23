//! Terrain derivatives module.
//!
//! Provides various terrain derivatives and surface characteristics:
//! - Slope: rate of change of elevation
//! - Aspect: direction of slope
//! - Curvature: surface curvature (profile, plan, total)
//! - Hillshade: shaded relief for visualization
//! - TPI: Topographic Position Index
//! - TRI: Terrain Ruggedness Index
//! - Roughness: surface roughness measures

pub mod aspect;
pub mod curvature;
pub mod hillshade;
pub mod roughness;
pub mod slope;
pub mod tpi;
pub mod tri;

// Re-exports
pub use aspect::{AspectAlgorithm, FlatHandling, aspect, aspect_horn, aspect_zevenbergen_thorne};
pub use curvature::{
    CurvatureType, curvature, plan_curvature, profile_curvature, tangential_curvature,
    total_curvature,
};
pub use hillshade::{
    HillshadeAlgorithm, hillshade, hillshade_combined, hillshade_multidirectional,
    hillshade_traditional,
};
pub use roughness::{
    RoughnessMethod, roughness, roughness_range, roughness_stddev, vector_ruggedness_measure,
};
pub use slope::{
    EdgeStrategy, SlopeAlgorithm, SlopeUnits, slope, slope_horn, slope_zevenbergen_thorne,
};
pub use tpi::tpi;
pub use tri::{tri, tri_riley};

#[cfg(feature = "parallel")]
pub use tpi::tpi_parallel;

#[cfg(feature = "parallel")]
pub use tri::tri_parallel;
