//! # OxiGDAL 3D
//!
//! 3D visualization, point cloud processing, and terrain mesh support for OxiGDAL.
//!
//! This crate provides comprehensive 3D geospatial data handling capabilities:
//!
//! ## Features
//!
//! - **Point Clouds**: LAS/LAZ, COPC (Cloud Optimized Point Cloud), EPT (Entwine Point Tiles)
//! - **Mesh Formats**: OBJ, glTF 2.0/GLB export with texture support
//! - **Terrain**: TIN (Triangulated Irregular Network), DEM to mesh conversion
//! - **Visualization**: 3D Tiles (Cesium) for web-based 3D mapping
//! - **Classification**: Ground, vegetation, building extraction
//!
//! ## Example Usage
//!
//! ```no_run
//! use oxigdal_3d::*;
//!
//! # fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! // Read LAS point cloud
//! let mut las = pointcloud::LasReader::open("points.las")?;
//! let point_cloud = las.read_all()?;
//!
//! // Classify ground points
//! let ground = classification::classify_ground(&point_cloud.points)?;
//!
//! // Create TIN from ground points
//! let tin = terrain::create_tin(&ground)?;
//!
//! // Export as glTF mesh
//! let mesh = terrain::tin_to_mesh(&tin)?;
//! mesh::export_gltf(&mesh, "terrain.glb")?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Architecture
//!
//! - Pure Rust implementation where possible
//! - Streaming support for large datasets
//! - Memory-efficient processing with spatial indexing
//! - Web-ready formats (glTF, 3D Tiles)
//! - Feature-gated C dependencies (PDAL, PCL) per COOLJAPAN Pure Rust Policy

#![deny(clippy::unwrap_used, clippy::panic)]
#![warn(missing_docs, unsafe_code)]

// Core modules
pub mod error;

// Point cloud modules
#[cfg(feature = "las-laz")]
pub mod pointcloud;

// Mesh modules
#[cfg(feature = "mesh")]
pub mod mesh;

// Terrain modules
#[cfg(feature = "terrain")]
pub mod terrain;

// Visualization modules
#[cfg(feature = "tiles3d")]
pub mod visualization;

// Classification module
pub mod classification;

// Re-exports
pub use error::{Error, Result};

#[cfg(feature = "las-laz")]
pub use pointcloud::{LasReader, LasWriter, Point, PointFormat};

#[cfg(feature = "mesh")]
pub use mesh::{Mesh, export_gltf, export_obj};

#[cfg(feature = "terrain")]
pub use terrain::{Tin, create_tin, tin_to_mesh};

#[cfg(feature = "tiles3d")]
pub use visualization::{Tileset, create_3d_tileset, write_3d_tiles};

/// Library version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }
}
