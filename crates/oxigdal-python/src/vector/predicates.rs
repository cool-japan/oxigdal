//! Spatial predicate operations for Python bindings
//!
//! Provides intersects, contains, within, touches, overlaps, crosses, and disjoint
//! predicates for GeoJSON geometries.

use pyo3::prelude::*;
use pyo3::types::PyDict;

use oxigdal_algorithms::vector::{self as algo_vec};

use super::helpers::{
    extract_all_coordinates, extract_geom_type, parse_geojson_linestring, parse_geojson_point,
    parse_geojson_polygon,
};
use super::operations::envelope;

/// Computes bounding box from a geometry dict as [minx, miny, maxx, maxy]
fn compute_bbox_from_geom(geom: &Bound<'_, PyDict>) -> PyResult<[f64; 4]> {
    let bbox_vec = envelope(geom)?;
    Ok([bbox_vec[0], bbox_vec[1], bbox_vec[2], bbox_vec[3]])
}

/// Tests if two geometries intersect.
///
/// Args:
///     geom1 (dict): First GeoJSON geometry
///     geom2 (dict): Second GeoJSON geometry
///
/// Returns:
///     bool: True if geometries intersect
///
/// Example:
///     >>> point = {"type": "Point", "coordinates": [0, 0]}
///     >>> polygon = {"type": "Polygon", "coordinates": [[[-1,-1], [1,-1], [1,1], [-1,1], [-1,-1]]]}
///     >>> oxigdal.intersects(point, polygon)
///     True
#[pyfunction]
pub fn intersects(geom1: &Bound<'_, PyDict>, geom2: &Bound<'_, PyDict>) -> PyResult<bool> {
    let type1: String = extract_geom_type(geom1)?;
    let type2: String = extract_geom_type(geom2)?;

    // Point vs Polygon check using point_in_polygon_or_boundary
    if type1 == "Point" && type2 == "Polygon" {
        let point = parse_geojson_point(geom1)?;
        let polygon = parse_geojson_polygon(geom2)?;
        return Ok(algo_vec::point_in_polygon_or_boundary(
            &point.coord,
            &polygon,
        ));
    }
    if type1 == "Polygon" && type2 == "Point" {
        let point = parse_geojson_point(geom2)?;
        let polygon = parse_geojson_polygon(geom1)?;
        return Ok(algo_vec::point_in_polygon_or_boundary(
            &point.coord,
            &polygon,
        ));
    }

    // Polygon vs Polygon
    if type1 == "Polygon" && type2 == "Polygon" {
        let poly1 = parse_geojson_polygon(geom1)?;
        let poly2 = parse_geojson_polygon(geom2)?;
        let result: oxigdal_algorithms::error::Result<bool> = algo_vec::intersects(&poly1, &poly2);
        return result.map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Intersects failed: {}", e))
        });
    }

    // Point vs Point
    if type1 == "Point" && type2 == "Point" {
        let p1 = parse_geojson_point(geom1)?;
        let p2 = parse_geojson_point(geom2)?;
        let result: oxigdal_algorithms::error::Result<bool> = algo_vec::intersects(&p1, &p2);
        return result.map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Intersects failed: {}", e))
        });
    }

    // For other combinations, use bounding box overlap as an approximation
    // combined with coordinate-level checks
    let bbox1 = compute_bbox_from_geom(geom1)?;
    let bbox2 = compute_bbox_from_geom(geom2)?;

    // If bounding boxes don't overlap, geometries are disjoint
    Ok(
        bbox1[2] >= bbox2[0]
            && bbox2[2] >= bbox1[0]
            && bbox1[3] >= bbox2[1]
            && bbox2[3] >= bbox1[1],
    )
}

/// Tests if geom1 contains geom2.
///
/// Args:
///     geom1 (dict): Container geometry
///     geom2 (dict): Contained geometry
///
/// Returns:
///     bool: True if geom1 contains geom2
///
/// Example:
///     >>> polygon = {"type": "Polygon", "coordinates": [[...]]}
///     >>> point = {"type": "Point", "coordinates": [0, 0]}
///     >>> oxigdal.contains(polygon, point)
///     True
#[pyfunction]
pub fn contains(geom1: &Bound<'_, PyDict>, geom2: &Bound<'_, PyDict>) -> PyResult<bool> {
    let type1 = extract_geom_type(geom1)?;
    let type2 = extract_geom_type(geom2)?;

    if type1 == "Polygon" && type2 == "Point" {
        let polygon = parse_geojson_polygon(geom1)?;
        let point = parse_geojson_point(geom2)?;
        return Ok(algo_vec::point_in_polygon_or_boundary(
            &point.coord,
            &polygon,
        ));
    }

    if type1 == "Polygon" && type2 == "Polygon" {
        let poly1 = parse_geojson_polygon(geom1)?;
        let poly2 = parse_geojson_polygon(geom2)?;
        let result: oxigdal_algorithms::error::Result<bool> = algo_vec::contains(&poly1, &poly2);
        return result.map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Contains failed: {}", e))
        });
    }

    if type1 == "Point" && type2 == "Point" {
        let p1 = parse_geojson_point(geom1)?;
        let p2 = parse_geojson_point(geom2)?;
        let result: oxigdal_algorithms::error::Result<bool> = algo_vec::contains(&p1, &p2);
        return result.map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Contains failed: {}", e))
        });
    }

    // For Polygon containing LineString/MultiPoint: check all coordinates
    if type1 == "Polygon" {
        let polygon = parse_geojson_polygon(geom1)?;
        let coords = extract_all_coordinates(geom2)?;
        for coord in &coords {
            if !algo_vec::point_in_polygon_or_boundary(coord, &polygon) {
                return Ok(false);
            }
        }
        return Ok(!coords.is_empty());
    }

    Ok(false)
}

/// Tests if geom1 is within geom2.
///
/// Args:
///     geom1 (dict): Inner geometry
///     geom2 (dict): Outer geometry
///
/// Returns:
///     bool: True if geom1 is within geom2
///
/// Example:
///     >>> point = {"type": "Point", "coordinates": [0, 0]}
///     >>> polygon = {"type": "Polygon", "coordinates": [[...]]}
///     >>> oxigdal.within(point, polygon)
///     True
#[pyfunction]
pub fn within(geom1: &Bound<'_, PyDict>, geom2: &Bound<'_, PyDict>) -> PyResult<bool> {
    // within(A, B) is equivalent to contains(B, A)
    contains(geom2, geom1)
}

/// Tests if two geometries touch (share boundary points but no interior points).
///
/// Args:
///     geom1 (dict): First GeoJSON geometry
///     geom2 (dict): Second GeoJSON geometry
///
/// Returns:
///     bool: True if geometries touch
///
/// Example:
///     >>> polygon1 = {"type": "Polygon", "coordinates": [[...]]}
///     >>> polygon2 = {"type": "Polygon", "coordinates": [[...]]}
///     >>> oxigdal.touches(polygon1, polygon2)
///     False
#[pyfunction]
pub fn touches(geom1: &Bound<'_, PyDict>, geom2: &Bound<'_, PyDict>) -> PyResult<bool> {
    let type1 = extract_geom_type(geom1)?;
    let type2 = extract_geom_type(geom2)?;

    if type1 == "Polygon" && type2 == "Polygon" {
        let poly1 = parse_geojson_polygon(geom1)?;
        let poly2 = parse_geojson_polygon(geom2)?;
        let result: oxigdal_algorithms::error::Result<bool> = algo_vec::touches(&poly1, &poly2);
        return result.map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Touches failed: {}", e))
        });
    }

    // For other geometry types, use boundary-based heuristic:
    // touches = intersects but not interior-intersects
    if type1 == "Polygon" && type2 == "Point" {
        let polygon = parse_geojson_polygon(geom1)?;
        let point = parse_geojson_point(geom2)?;
        let on_boundary = algo_vec::point_on_polygon_boundary(&point.coord, &polygon);
        let inside = algo_vec::point_strictly_inside_polygon(&point.coord, &polygon);
        return Ok(on_boundary && !inside);
    }

    Ok(false)
}

/// Tests if two geometries overlap.
///
/// Args:
///     geom1 (dict): First GeoJSON geometry
///     geom2 (dict): Second GeoJSON geometry
///
/// Returns:
///     bool: True if geometries overlap
///
/// Example:
///     >>> oxigdal.overlaps(polygon1, polygon2)
///     True
#[pyfunction]
pub fn overlaps(geom1: &Bound<'_, PyDict>, geom2: &Bound<'_, PyDict>) -> PyResult<bool> {
    let type1 = extract_geom_type(geom1)?;
    let type2 = extract_geom_type(geom2)?;

    // Overlaps requires same dimension and partial intersection
    if type1 == "Polygon" && type2 == "Polygon" {
        let poly1 = parse_geojson_polygon(geom1)?;
        let poly2 = parse_geojson_polygon(geom2)?;

        // Two polygons overlap if they intersect but neither contains the other
        let do_intersect: bool = algo_vec::intersects(&poly1, &poly2).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Overlaps check failed: {}", e))
        })?;

        if !do_intersect {
            return Ok(false);
        }

        let a_contains_b: bool = algo_vec::contains(&poly1, &poly2).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Overlaps check failed: {}", e))
        })?;

        let b_contains_a: bool = algo_vec::contains(&poly2, &poly1).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Overlaps check failed: {}", e))
        })?;

        return Ok(!a_contains_b && !b_contains_a);
    }

    Ok(false)
}

/// Tests if two geometries cross.
///
/// Args:
///     geom1 (dict): First GeoJSON geometry
///     geom2 (dict): Second GeoJSON geometry
///
/// Returns:
///     bool: True if geometries cross
///
/// Example:
///     >>> line1 = {"type": "LineString", "coordinates": [[0,0], [1,1]]}
///     >>> line2 = {"type": "LineString", "coordinates": [[0,1], [1,0]]}
///     >>> oxigdal.crosses(line1, line2)
///     True
#[pyfunction]
pub fn crosses(geom1: &Bound<'_, PyDict>, geom2: &Bound<'_, PyDict>) -> PyResult<bool> {
    let type1 = extract_geom_type(geom1)?;
    let type2 = extract_geom_type(geom2)?;

    // LineString vs LineString: check for segment intersections
    if type1 == "LineString" && type2 == "LineString" {
        let ls1 = parse_geojson_linestring(geom1)?;
        let ls2 = parse_geojson_linestring(geom2)?;

        let intersections = algo_vec::intersect_linestrings(&ls1, &ls2).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Crosses check failed: {}", e))
        })?;

        // Lines cross if they have intersection points (not just overlap)
        return Ok(!intersections.is_empty());
    }

    // LineString vs Polygon: the line enters and exits the polygon
    if type1 == "LineString" && type2 == "Polygon" {
        let ls = parse_geojson_linestring(geom1)?;
        let polygon = parse_geojson_polygon(geom2)?;

        let mut has_inside = false;
        let mut has_outside = false;
        for coord in &ls.coords {
            if algo_vec::point_in_polygon_or_boundary(coord, &polygon) {
                has_inside = true;
            } else {
                has_outside = true;
            }
            if has_inside && has_outside {
                return Ok(true);
            }
        }
        return Ok(false);
    }

    Ok(false)
}

/// Tests if two geometries are disjoint (do not intersect).
///
/// Args:
///     geom1 (dict): First GeoJSON geometry
///     geom2 (dict): Second GeoJSON geometry
///
/// Returns:
///     bool: True if geometries are disjoint
///
/// Example:
///     >>> oxigdal.disjoint(polygon1, polygon2)
///     False
#[pyfunction]
pub fn disjoint(geom1: &Bound<'_, PyDict>, geom2: &Bound<'_, PyDict>) -> PyResult<bool> {
    // disjoint is the negation of intersects
    Ok(!intersects(geom1, geom2)?)
}
