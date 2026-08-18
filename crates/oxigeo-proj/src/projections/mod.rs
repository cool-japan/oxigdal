//! Pure Rust implementations of map projections.
//!
//! This module provides forward and inverse implementations for a wide variety
//! of map projections, organized by projection family:
//!
//! - **Cylindrical**: Sinusoidal, Cassini-Soldner, ellipsoidal Transverse Mercator
//!   (`tmerc_forward` / `tmerc_inverse`, with Gauss-Kruger as a parameter preset),
//!   Equirectangular
//! - **Pseudocylindrical**: Mollweide, Robinson, Eckert IV, Eckert VI
//! - **Conic**: Equidistant Conic, Albers Equal-Area Conic
//! - **Azimuthal**: Azimuthal Equidistant, Gnomonic
//! - **Polyconic**: American Polyconic
//!
//! All projections use radians internally. Degree conversion must be handled
//! at the calling layer.

pub mod additional;
pub mod albers;
pub mod azimuthal;
pub mod conic;
pub mod cylindrical;
pub mod equirectangular;
pub mod oblique_mercator;
pub mod polyconic;
pub mod pseudocylindrical;

pub use additional::{
    bonne_forward, bonne_inverse, craster_forward, craster_inverse, goode_forward, goode_inverse,
    hammer_forward, hammer_inverse, miller_forward, miller_inverse, werner_forward, werner_inverse,
};
pub use albers::{albers_forward, albers_inverse};
pub use azimuthal::{
    azimuthal_equidistant_forward, azimuthal_equidistant_inverse, gnomonic_forward,
    gnomonic_inverse,
};
pub use conic::equidistant_conic_forward;
// `tmerc_forward` / `tmerc_inverse` are the ellipsoidal Transverse Mercator kernel that
// `gauss_kruger_*` delegates to. They are re-exported here so that callers projecting
// UTM or national grids can reach them without naming the `cylindrical` submodule — and
// so that the ellipsoidal kernel is no harder to find than the spherical
// `transform::cylindrical::TransverseMercator`, which must not be used for those grids.
pub use cylindrical::{
    cassini_forward, cassini_inverse, gauss_kruger_forward, gauss_kruger_inverse,
    sinusoidal_forward, sinusoidal_inverse, tmerc_forward, tmerc_inverse,
};
pub use equirectangular::{equirectangular_forward, equirectangular_inverse};
pub use oblique_mercator::{oblique_mercator_forward, oblique_mercator_inverse};
pub use polyconic::{polyconic_forward, polyconic_inverse};
pub use pseudocylindrical::{
    eckert4_forward, eckert4_inverse, eckert6_forward, eckert6_inverse, mollweide_forward,
    mollweide_inverse, robinson_forward, robinson_inverse,
};
