//! Spatial representation information for ISO 19115.

use serde::{Deserialize, Serialize};

/// Spatial representation information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SpatialRepresentationInfo {
    /// Grid spatial representation
    Grid(GridSpatialRepresentation),
    /// Vector spatial representation
    Vector(VectorSpatialRepresentation),
}

/// Grid spatial representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridSpatialRepresentation {
    /// Number of dimensions
    pub number_of_dimensions: usize,
    /// Axis dimension properties
    pub axis_dimension_properties: Vec<Dimension>,
    /// Cell geometry
    pub cell_geometry: CellGeometry,
    /// Transformation parameter availability
    pub transformation_parameter_availability: bool,
}

/// Dimension information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dimension {
    /// Dimension name
    pub dimension_name: DimensionName,
    /// Dimension size
    pub dimension_size: usize,
    /// Resolution
    pub resolution: Option<f64>,
}

/// Dimension name type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum DimensionName {
    /// Row
    Row,
    /// Column
    Column,
    /// Vertical
    Vertical,
    /// Track
    Track,
    /// Cross track
    CrossTrack,
    /// Line
    Line,
    /// Sample
    Sample,
    /// Time
    Time,
}

/// Cell geometry code.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum CellGeometry {
    /// Point
    Point,
    /// Area
    Area,
    /// Voxel
    Voxel,
    /// Stratum
    Stratum,
}

/// Vector spatial representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorSpatialRepresentation {
    /// Topology level
    pub topology_level: TopologyLevel,
    /// Geometric objects
    pub geometric_objects: Vec<GeometricObjects>,
}

/// Topology level code.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TopologyLevel {
    /// Geometry only
    GeometryOnly,
    /// Topology 1D
    Topology1D,
    /// Planar graph
    PlanarGraph,
    /// Full planar graph
    FullPlanarGraph,
    /// Surface graph
    SurfaceGraph,
    /// Full surface graph
    FullSurfaceGraph,
    /// Topology 3D
    Topology3D,
    /// Full topology 3D
    FullTopology3D,
    /// Abstract
    Abstract,
}

/// Geometric objects information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeometricObjects {
    /// Geometric object type
    pub geometric_object_type: GeometricObjectType,
    /// Geometric object count
    pub geometric_object_count: Option<usize>,
}

/// Geometric object type code.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum GeometricObjectType {
    /// Complex
    Complex,
    /// Composite
    Composite,
    /// Curve
    Curve,
    /// Point
    Point,
    /// Solid
    Solid,
    /// Surface
    Surface,
}
