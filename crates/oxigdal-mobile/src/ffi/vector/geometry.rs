//! Geometry types and conversion functions for FFI.
//!
//! Provides geometry representations compatible with C FFI and conversion utilities.

use super::super::types::OxiGdalBbox;

/// Geometry types supported by the FFI layer.
#[derive(Debug, Clone)]
pub enum FfiGeometry {
    /// Point geometry with x, y coordinates and optional z.
    Point {
        /// X coordinate (longitude)
        x: f64,
        /// Y coordinate (latitude)
        y: f64,
        /// Optional Z coordinate (elevation)
        z: Option<f64>,
    },
    /// LineString geometry as a sequence of coordinates.
    LineString {
        /// Coordinates as (x, y, optional z) tuples
        coords: Vec<(f64, f64, Option<f64>)>,
    },
    /// Polygon geometry with exterior ring and optional interior rings (holes).
    Polygon {
        /// Exterior ring coordinates
        exterior: Vec<(f64, f64, Option<f64>)>,
        /// Interior rings (holes)
        interiors: Vec<Vec<(f64, f64, Option<f64>)>>,
    },
    /// MultiPoint geometry as a collection of points.
    MultiPoint {
        /// Points as (x, y, optional z) tuples
        points: Vec<(f64, f64, Option<f64>)>,
    },
    /// MultiLineString geometry as a collection of line strings.
    MultiLineString {
        /// Line strings, each as a sequence of coordinates
        line_strings: Vec<Vec<(f64, f64, Option<f64>)>>,
    },
    /// MultiPolygon geometry as a collection of polygons.
    MultiPolygon {
        /// Polygons as (exterior ring, interior rings) pairs
        polygons: Vec<(
            Vec<(f64, f64, Option<f64>)>,
            Vec<Vec<(f64, f64, Option<f64>)>>,
        )>,
    },
    /// GeometryCollection as a heterogeneous collection of geometries.
    GeometryCollection {
        /// Contained geometries
        geometries: Vec<FfiGeometry>,
    },
}

impl FfiGeometry {
    /// Returns the geometry type as a string.
    #[must_use]
    pub fn geometry_type(&self) -> &'static str {
        match self {
            Self::Point { .. } => "Point",
            Self::LineString { .. } => "LineString",
            Self::Polygon { .. } => "Polygon",
            Self::MultiPoint { .. } => "MultiPoint",
            Self::MultiLineString { .. } => "MultiLineString",
            Self::MultiPolygon { .. } => "MultiPolygon",
            Self::GeometryCollection { .. } => "GeometryCollection",
        }
    }

    /// Calculates the bounding box of the geometry.
    #[must_use]
    pub fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        match self {
            Self::Point { x, y, .. } => {
                if x.is_nan() || y.is_nan() {
                    None
                } else {
                    Some((*x, *y, *x, *y))
                }
            }
            Self::LineString { coords } => bounds_from_coords(coords),
            Self::Polygon { exterior, .. } => bounds_from_coords(exterior),
            Self::MultiPoint { points } => bounds_from_coords(points),
            Self::MultiLineString { line_strings } => {
                merge_bounds(line_strings.iter().filter_map(|ls| bounds_from_coords(ls)))
            }
            Self::MultiPolygon { polygons } => merge_bounds(
                polygons
                    .iter()
                    .filter_map(|(ext, _)| bounds_from_coords(ext)),
            ),
            Self::GeometryCollection { geometries } => {
                merge_bounds(geometries.iter().filter_map(Self::bounds))
            }
        }
    }

    /// Converts geometry to WKT format.
    #[must_use]
    pub fn to_wkt(&self) -> String {
        match self {
            Self::Point { x, y, z } => {
                if let Some(z_val) = z {
                    format!("POINT Z ({} {} {})", x, y, z_val)
                } else {
                    format!("POINT ({} {})", x, y)
                }
            }
            Self::LineString { coords } => {
                let coord_str = coords_to_wkt_string(coords);
                format!("LINESTRING ({})", coord_str)
            }
            Self::Polygon {
                exterior,
                interiors,
            } => {
                let mut rings = vec![format!("({})", coords_to_wkt_string(exterior))];
                for interior in interiors {
                    rings.push(format!("({})", coords_to_wkt_string(interior)));
                }
                format!("POLYGON ({})", rings.join(", "))
            }
            Self::MultiPoint { points } => {
                let point_strs: Vec<String> = points
                    .iter()
                    .map(|(x, y, z)| {
                        if let Some(z_val) = z {
                            format!("({} {} {})", x, y, z_val)
                        } else {
                            format!("({} {})", x, y)
                        }
                    })
                    .collect();
                format!("MULTIPOINT ({})", point_strs.join(", "))
            }
            Self::MultiLineString { line_strings } => {
                let ls_strs: Vec<String> = line_strings
                    .iter()
                    .map(|ls| format!("({})", coords_to_wkt_string(ls)))
                    .collect();
                format!("MULTILINESTRING ({})", ls_strs.join(", "))
            }
            Self::MultiPolygon { polygons } => {
                let poly_strs: Vec<String> = polygons
                    .iter()
                    .map(|(ext, ints)| {
                        let mut rings = vec![format!("({})", coords_to_wkt_string(ext))];
                        for interior in ints {
                            rings.push(format!("({})", coords_to_wkt_string(interior)));
                        }
                        format!("({})", rings.join(", "))
                    })
                    .collect();
                format!("MULTIPOLYGON ({})", poly_strs.join(", "))
            }
            Self::GeometryCollection { geometries } => {
                let geom_strs: Vec<String> = geometries.iter().map(Self::to_wkt).collect();
                format!("GEOMETRYCOLLECTION ({})", geom_strs.join(", "))
            }
        }
    }
}

/// Helper function to calculate bounds from a list of coordinates.
pub(super) fn bounds_from_coords(
    coords: &[(f64, f64, Option<f64>)],
) -> Option<(f64, f64, f64, f64)> {
    if coords.is_empty() {
        return None;
    }

    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for (x, y, _) in coords {
        if !x.is_nan() && !y.is_nan() {
            min_x = min_x.min(*x);
            min_y = min_y.min(*y);
            max_x = max_x.max(*x);
            max_y = max_y.max(*y);
        }
    }

    if min_x.is_infinite() {
        None
    } else {
        Some((min_x, min_y, max_x, max_y))
    }
}

/// Helper function to merge multiple bounding boxes.
pub(super) fn merge_bounds(
    bounds_iter: impl Iterator<Item = (f64, f64, f64, f64)>,
) -> Option<(f64, f64, f64, f64)> {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut has_bounds = false;

    for (x_min, y_min, x_max, y_max) in bounds_iter {
        has_bounds = true;
        min_x = min_x.min(x_min);
        min_y = min_y.min(y_min);
        max_x = max_x.max(x_max);
        max_y = max_y.max(y_max);
    }

    if has_bounds {
        Some((min_x, min_y, max_x, max_y))
    } else {
        None
    }
}

/// Helper function to convert coordinates to WKT coordinate string.
pub(super) fn coords_to_wkt_string(coords: &[(f64, f64, Option<f64>)]) -> String {
    coords
        .iter()
        .map(|(x, y, z)| {
            if let Some(z_val) = z {
                format!("{} {} {}", x, y, z_val)
            } else {
                format!("{} {}", x, y)
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}
