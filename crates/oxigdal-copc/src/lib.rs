//! Pure Rust COPC (Cloud Optimized Point Cloud) reader.
//!
//! Implements end-to-end reading of COPC (Cloud Optimized Point Cloud) files
//! built on top of an ASPRS LAS 1.4 public header parser.
//!
//! # Module layout
//! - [`las_header`] parses the LAS public header block.
//! - [`copc_vlr`] defines the VLR / COPC info binary layout.
//! - [`vlr_chain`] walks the VLR sequence and locates COPC records.
//! - [`point_format`] deserializes LAS point records (formats 0-3, 6-8).
//! - [`hierarchy`] parses COPC octree hierarchy pages and traverses them.
//! - [`copc_reader`] ties everything together behind the [`CopcReader`] API.
//!
//! In-memory point storage and spatial indexing are provided by the [`point`],
//! [`octree`] and [`profile`] modules.

pub mod copc_reader;
pub mod copc_vlr;
pub mod crs_vlr;
pub mod error;
pub mod extended_vlr;
pub mod hierarchy;
pub mod las_header;
pub mod laz;
pub mod octree;
pub mod point;
pub mod point_format;
pub mod profile;
pub mod vlr_chain;

pub use copc_reader::CopcReader;
pub use copc_vlr::{CopcInfo, Vlr, VlrKey};
pub use crs_vlr::{
    CrsInfo, GeoKeyDirectory, GeoKeyEntry, extract_crs_info, parse_geo_key_directory,
};
pub use error::CopcError;
pub use extended_vlr::{ExtendedVlr, parse_evlr_chain, version_supports_evlr};
pub use hierarchy::{HierarchyEntry, VoxelKey};
pub use las_header::{LasHeader, LasVersion};
pub use laz::{LazItem, LazVlrInfo, decompress_chunk, detect_laszip_vlr, parse_laszip_vlr_data};
pub use octree::{Octree, OctreeNode, PointCloudStats};
pub use point::{BoundingBox3D, Point3D};
pub use point_format::{deserialize_point, deserialize_points};
pub use profile::{GroundFilter, HeightProfile, ProfileSegment};
pub use vlr_chain::{find_copc_hierarchy_vlr, find_copc_info, parse_vlrs};
