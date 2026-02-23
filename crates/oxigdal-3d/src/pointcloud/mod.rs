//! Point cloud processing modules
//!
//! This module provides comprehensive point cloud support including:
//! - LAS/LAZ format reading and writing
//! - COPC (Cloud Optimized Point Cloud)
//! - EPT (Entwine Point Tiles)
//! - Spatial indexing and querying

pub mod las;

#[cfg(feature = "copc")]
pub mod copc;

#[cfg(feature = "ept")]
pub mod ept;

// Re-exports
pub use las::{
    Bounds3d, Classification, ColorRgb, ColorRgbNir, LasHeader, LasReader, LasWriter, Point,
    PointCloud, PointFormat, PointRecord, SpatialIndex,
};

#[cfg(feature = "copc")]
pub use copc::{CopcHierarchy, CopcInfo, CopcReader};

#[cfg(feature = "ept")]
pub use ept::{EptMetadata, EptOctree, EptReader};
