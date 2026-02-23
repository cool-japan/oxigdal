//! Vector data types and utilities
//!
//! This module provides types for representing and working with vector geospatial data,
//! including geometries, features, and feature collections.
//!
//! # Geometry Types
//!
//! Following the OGC Simple Features specification:
//! - [`geometry::Point`] - 0-dimensional geometry
//! - [`geometry::LineString`] - 1-dimensional geometry
//! - [`geometry::Polygon`] - 2-dimensional geometry
//! - [`geometry::MultiPoint`] - Collection of points
//! - [`geometry::MultiLineString`] - Collection of line strings
//! - [`geometry::MultiPolygon`] - Collection of polygons
//! - [`geometry::GeometryCollection`] - Heterogeneous collection
//!
//! # Features
//!
//! A [`feature::Feature`] combines a geometry with properties (attributes).
//! Features can be organized into [`feature::FeatureCollection`]s.
//!
//! # Example
//!
//! ```
//! use oxigdal_core::vector::{
//!     geometry::{Point, Coordinate, Geometry},
//!     feature::{Feature, PropertyValue},
//! };
//!
//! // Create a point geometry
//! let point = Point::new(10.0, 20.0);
//!
//! // Create a feature with the geometry
//! let mut feature = Feature::new(Geometry::Point(point));
//!
//! // Add properties
//! feature.set_property("name", PropertyValue::String("My Point".to_string()));
//! feature.set_property("value", PropertyValue::Integer(42));
//! ```

pub mod feature;
pub mod geometry;

// Re-export commonly used types
pub use feature::{Feature, FeatureCollection, FeatureId, PropertyValue};
pub use geometry::{
    Coordinate, Geometry, GeometryCollection, LineString, MultiLineString, MultiPoint,
    MultiPolygon, Point, Polygon,
};
