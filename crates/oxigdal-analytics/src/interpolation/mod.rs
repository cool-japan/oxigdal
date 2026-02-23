//! Spatial Interpolation Module
//!
//! This module provides spatial interpolation methods for creating continuous surfaces:
//! - Inverse Distance Weighting (IDW)
//! - Kriging (Ordinary and Universal)
//! - Variogram fitting
//! - Cross-validation

pub mod idw;
pub mod kriging;

pub use idw::{IdwInterpolator, IdwResult};
pub use kriging::{
    KrigingInterpolator, KrigingResult, KrigingType, SemivariogramCalculator, Variogram,
    VariogramModel,
};
