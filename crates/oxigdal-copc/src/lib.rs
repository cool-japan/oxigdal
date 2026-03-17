//! Pure Rust COPC (Cloud Optimized Point Cloud) reader.
//!
//! Implements an ASPRS LAS 1.4 public header parser ([`las_header`]) and
//! COPC-specific VLR types ([`copc_vlr`]).  In-memory point storage and
//! spatial indexing are provided by the [`point`], [`octree`] and [`profile`]
//! modules.

pub mod copc_vlr;
pub mod error;
pub mod las_header;
pub mod octree;
pub mod point;
pub mod profile;

pub use copc_vlr::{CopcInfo, Vlr, VlrKey};
pub use error::CopcError;
pub use las_header::{LasHeader, LasVersion};
pub use octree::{Octree, OctreeNode, PointCloudStats};
pub use point::{BoundingBox3D, Point3D};
pub use profile::{GroundFilter, HeightProfile, ProfileSegment};
