//! Terrain mesh generation.
//!
//! This module provides algorithms for generating triangulated irregular networks
//! (TINs) from digital elevation models (DEMs).
//!
//! # Available algorithms
//!
//! - [`tin_from_dem`]: Greedy VIP (Very Important Points) adaptive TIN refinement.

pub mod tin;

pub use tin::{TerrainTin, tin_from_dem};
