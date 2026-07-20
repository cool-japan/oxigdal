//! Topology operations for advanced vector processing
//!
//! This module provides comprehensive topology operations:
//!
//! - **Overlay operations**: Advanced geometric overlay with multiple types
//! - **Erase operations**: Multi-geometry erase and clip operations
//! - **Split operations**: Geometry splitting with lines and polygons
//!
//! All operations maintain topological consistency and handle edge cases.

mod erase;
mod overlay;
mod split;

pub use erase::{
    EraseOptions, erase_geometries, erase_multipolygon, erase_polygon, erase_polygon_batch,
    erase_with_buffer,
};
pub use overlay::{
    OverlayOptions, OverlayType, overlay_geometries, overlay_multipolygon, overlay_polygon,
    overlay_polygon_batch,
};
pub use split::{
    SplitOptions, SplitResult, split_linestring_by_points, split_polygon_by_line,
    split_polygon_by_polygon, split_polygons_batch,
};
