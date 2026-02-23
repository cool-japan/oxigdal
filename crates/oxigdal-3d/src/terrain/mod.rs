//! Terrain processing modules
//!
//! This module provides terrain-related functionality including:
//! - TIN (Triangulated Irregular Network) creation and operations
//! - DEM to 3D mesh conversion
//! - Contour generation
//! - Surface analysis

pub mod dem_to_mesh;
pub mod tin;

// Re-exports
pub use dem_to_mesh::{DemMeshOptions, dem_to_mesh};
pub use tin::{Tin, TinPoint, TinTriangle, create_tin, tin_to_mesh};
