//! GeoJSON type definitions
//!
//! This module provides strongly-typed representations of all GeoJSON types
//! according to RFC 7946, including:
//!
//! - Geometry types (Point, LineString, Polygon, MultiPoint, MultiLineString, MultiPolygon, GeometryCollection)
//! - Feature
//! - FeatureCollection
//! - CRS (Coordinate Reference System)
//! - Bounding boxes
//! - Properties

mod crs;
mod feature;
mod geometry;

pub use crs::{Crs, CrsType, NamedCrs};
pub use feature::{Feature, FeatureCollection, FeatureId, Properties};
pub use geometry::{
    Coordinate, CoordinateSequence, Geometry, GeometryCollection, GeometryType, LineString,
    MultiLineString, MultiPoint, MultiPolygon, Point, Polygon, Position,
};

/// Bounding box type (2D or 3D)
pub type BBox = Vec<f64>;

/// Foreign members (additional properties not defined in RFC 7946)
pub type ForeignMembers = serde_json::Map<String, serde_json::Value>;
