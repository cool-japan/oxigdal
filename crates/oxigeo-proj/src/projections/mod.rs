//! Pure Rust implementations of map projections.
//!
//! This module provides forward and inverse implementations for a wide variety
//! of map projections, organized by projection family:
//!
//! - **Cylindrical**: Sinusoidal, Cassini-Soldner, Gauss-Kruger (TM variant), Equirectangular
//! - **Pseudocylindrical**: Mollweide, Robinson, Eckert IV, Eckert VI
//! - **Conic**: Equidistant Conic, Albers Equal-Area Conic
//! - **Azimuthal**: Azimuthal Equidistant, Gnomonic
//! - **Polyconic**: American Polyconic
//!
//! All projections use radians internally. Degree conversion must be handled
//! at the calling layer.

pub mod albers;
pub mod azimuthal;
pub mod conic;
pub mod cylindrical;
pub mod equirectangular;
pub mod oblique_mercator;
pub mod polyconic;
pub mod pseudocylindrical;

pub use albers::{albers_forward, albers_inverse};
pub use azimuthal::{
    azimuthal_equidistant_forward, azimuthal_equidistant_inverse, gnomonic_forward,
    gnomonic_inverse,
};
pub use conic::equidistant_conic_forward;
pub use cylindrical::{
    cassini_forward, cassini_inverse, gauss_kruger_forward, gauss_kruger_inverse,
    sinusoidal_forward, sinusoidal_inverse,
};
pub use equirectangular::{equirectangular_forward, equirectangular_inverse};
pub use oblique_mercator::{oblique_mercator_forward, oblique_mercator_inverse};
pub use polyconic::{polyconic_forward, polyconic_inverse};
pub use pseudocylindrical::{
    eckert4_forward, eckert4_inverse, eckert6_forward, eckert6_inverse, mollweide_forward,
    mollweide_inverse, robinson_forward, robinson_inverse,
};
