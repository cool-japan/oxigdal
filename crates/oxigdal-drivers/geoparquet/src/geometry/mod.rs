//! Geometry encoding and decoding
//!
//! This module provides WKB (Well-Known Binary) encoding and decoding
//! for all geometry types supported by GeoParquet.

mod types;
mod wkb;
pub mod wkb_extended;

pub use types::{
    Coordinate, Geometry, GeometryCollection, GeometryType, LineString, MultiLineString,
    MultiPoint, MultiPolygon, Point, Polygon,
};
pub use wkb::{WkbReader, WkbWriter};
pub use wkb_extended::{
    GeometryStats, Polygon2d, Polygon3d, WkbType, compute_geometry_stats, decode_point_2d,
    decode_point_z, encode_geometry_collection, encode_multi_polygon, encode_multi_polygon_z,
    encode_point_2d, encode_point_m, encode_point_z, encode_point_zm, encode_polygon,
    encode_polygon_z, wkb_bbox,
};

use crate::error::Result;

/// Trait for types that can be encoded to WKB
pub trait ToWkb {
    /// Encodes this geometry to WKB format
    fn to_wkb(&self) -> Result<Vec<u8>>;

    /// Encodes this geometry to WKB with specific byte order
    fn to_wkb_with_endian(&self, little_endian: bool) -> Result<Vec<u8>>;
}

/// Trait for types that can be decoded from WKB
pub trait FromWkb: Sized {
    /// Decodes a geometry from WKB format
    fn from_wkb(bytes: &[u8]) -> Result<Self>;
}
