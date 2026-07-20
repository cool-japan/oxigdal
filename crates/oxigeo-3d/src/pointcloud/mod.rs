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

#[cfg(feature = "copc")]
pub mod copc_vlr;

#[cfg(feature = "ept")]
pub mod ept;

// Re-exports
pub use las::{
    Bounds3d, Classification, ColorRgb, ColorRgbNir, LasHeader, LasReader, LasWriter, Point,
    PointCloud, PointFormat, PointRecord, SpatialIndex,
};

#[cfg(feature = "copc")]
pub use copc::{CopcHierarchy, CopcInfo, CopcReader};

#[cfg(feature = "copc")]
pub use copc_vlr::{
    COPC_HIERARCHY_RECORD_ID, COPC_INFO_RECORD_ID, COPC_USER_ID, CopcInfoVlrPayload,
    HierarchyEntry, VoxelKey as CopcVoxelKey, find_copc_info_vlr, parse_copc_info,
    parse_hierarchy_page,
};

#[cfg(feature = "ept")]
pub use ept::{EptMetadata, EptOctree, EptReader};
