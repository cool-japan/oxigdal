//! Geometry operations for Python bindings
//!
//! Provides buffer, boolean operations (union, intersection, difference),
//! measurement (area, length), simplification, centroid, convex hull, and envelope.

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use oxigdal_algorithms::vector::{
    self as algo_vec, AreaMethod, BufferOptions, Coordinate, Point, Polygon, SimplifyMethod,
};

use super::helpers::{
    compute_geodesic_length, compute_planar_length, coords_to_pylist, extract_all_coordinates,
    extract_ring_from_pylist, parse_cap_style, parse_geojson_linestring, parse_geojson_point,
    parse_geojson_polygon, parse_join_style, polygon_to_geojson,
};

/// Buffers a geometry by a specified distance.
///
/// Args:
///     geometry (dict): GeoJSON geometry
///     distance (float): Buffer distance in the units of the geometry
///     segments (int): Number of segments per quadrant (default: 8)
///     cap_style (str): Cap style - "round", "flat", "square" (default: "round")
///     join_style (str): Join style - "round", "mitre", "bevel" (default: "round")
///     mitre_limit (float): Mitre limit for mitre joins (default: 5.0)
///
/// Returns:
///     dict: Buffered geometry as GeoJSON
///
/// Raises:
///     ValueError: If geometry is invalid
///
/// Example:
///     >>> point = {"type": "Point", "coordinates": [0.0, 0.0]}
///     >>> buffered = oxigdal.buffer_geometry(point, distance=100.0)
///     >>> print(buffered['type'])  # "Polygon"
///     >>>
///     >>> # Buffer with flat caps
///     >>> line_buffered = oxigdal.buffer_geometry(line, distance=10.0, cap_style="flat")
#[pyfunction]
#[pyo3(signature = (geometry, distance, segments=8, cap_style="round", join_style="round", mitre_limit=5.0))]
pub fn buffer_geometry<'py>(
    py: Python<'py>,
    geometry: &Bound<'py, PyDict>,
    distance: f64,
    segments: i32,
    cap_style: &str,
    join_style: &str,
    mitre_limit: f64,
) -> PyResult<Bound<'py, PyDict>> {
    if distance < 0.0 {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Distance must be non-negative",
        ));
    }

    if segments < 1 {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Segments must be positive",
        ));
    }

    let valid_cap_styles = ["round", "flat", "square"];
    if !valid_cap_styles.contains(&cap_style) {
        return Err(pyo3::exceptions::PyValueError::new_err(format!(
            "Invalid cap_style '{}'. Valid options: {:?}",
            cap_style, valid_cap_styles
        )));
    }

    let valid_join_styles = ["round", "mitre", "bevel"];
    if !valid_join_styles.contains(&join_style) {
        return Err(pyo3::exceptions::PyValueError::new_err(format!(
            "Invalid join_style '{}'. Valid options: {:?}",
            join_style, valid_join_styles
        )));
    }

    if mitre_limit < 1.0 {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Mitre limit must be >= 1.0",
        ));
    }

    let geom_type: String = geometry
        .get_item("type")
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Missing 'type' field"))?
        .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("Missing 'type' field"))?
        .extract()?;

    let options = BufferOptions {
        quadrant_segments: segments as usize,
        cap_style: parse_cap_style(cap_style),
        join_style: parse_join_style(join_style),
        miter_limit: mitre_limit,
        simplify_tolerance: 0.0,
    };

    let result_polygon = match geom_type.as_str() {
        "Point" => {
            let point = parse_geojson_point(geometry)?;
            algo_vec::buffer_point(&point, distance, &options).map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("Buffer failed: {}", e))
            })?
        }
        "LineString" => {
            let linestring = parse_geojson_linestring(geometry)?;
            algo_vec::buffer_linestring(&linestring, distance, &options).map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("Buffer failed: {}", e))
            })?
        }
        "Polygon" => {
            let polygon = parse_geojson_polygon(geometry)?;
            algo_vec::buffer_polygon(&polygon, distance, &options).map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("Buffer failed: {}", e))
            })?
        }
        _ => {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Buffer not supported for geometry type '{}'",
                geom_type
            )));
        }
    };

    polygon_to_geojson(py, &result_polygon)
}

/// Helper to convert a list of result polygons to a GeoJSON dict (Polygon, MultiPolygon, or
/// empty GeometryCollection)
fn polys_to_geojson<'py>(
    py: Python<'py>,
    result_polys: &[Polygon],
) -> PyResult<Bound<'py, PyDict>> {
    if result_polys.is_empty() {
        let result = PyDict::new(py);
        result.set_item("type", "GeometryCollection")?;
        result.set_item("geometries", PyList::empty(py))?;
        Ok(result)
    } else if result_polys.len() == 1 {
        polygon_to_geojson(py, &result_polys[0])
    } else {
        let result = PyDict::new(py);
        result.set_item("type", "MultiPolygon")?;
        let polys_list = PyList::empty(py);
        for poly in result_polys {
            let rings = PyList::empty(py);
            rings.append(coords_to_pylist(py, &poly.exterior.coords)?)?;
            for interior in &poly.interiors {
                rings.append(coords_to_pylist(py, &interior.coords)?)?;
            }
            polys_list.append(rings)?;
        }
        result.set_item("coordinates", polys_list)?;
        Ok(result)
    }
}

/// Computes the union of two geometries.
///
/// Args:
///     geom1 (dict): First GeoJSON geometry
///     geom2 (dict): Second GeoJSON geometry
///
/// Returns:
///     dict: Union geometry as GeoJSON
///
/// Raises:
///     ValueError: If geometries are invalid
///
/// Example:
///     >>> polygon1 = {"type": "Polygon", "coordinates": [[...]]}
///     >>> polygon2 = {"type": "Polygon", "coordinates": [[...]]}
///     >>> result = oxigdal.union(polygon1, polygon2)
#[pyfunction]
pub fn union<'py>(
    py: Python<'py>,
    geom1: &Bound<'py, PyDict>,
    geom2: &Bound<'py, PyDict>,
) -> PyResult<Bound<'py, PyDict>> {
    let poly1 = parse_geojson_polygon(geom1)?;
    let poly2 = parse_geojson_polygon(geom2)?;

    let result_polys = algo_vec::union_polygon(&poly1, &poly2)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("Union failed: {}", e)))?;

    if result_polys.len() == 1 {
        polygon_to_geojson(py, &result_polys[0])
    } else {
        // Return as MultiPolygon
        let result = PyDict::new(py);
        result.set_item("type", "MultiPolygon")?;
        let polys_list = PyList::empty(py);
        for poly in &result_polys {
            let rings = PyList::empty(py);
            rings.append(coords_to_pylist(py, &poly.exterior.coords)?)?;
            for interior in &poly.interiors {
                rings.append(coords_to_pylist(py, &interior.coords)?)?;
            }
            polys_list.append(rings)?;
        }
        result.set_item("coordinates", polys_list)?;
        Ok(result)
    }
}

/// Computes the intersection of two geometries.
///
/// Args:
///     geom1 (dict): First GeoJSON geometry
///     geom2 (dict): Second GeoJSON geometry
///
/// Returns:
///     dict: Intersection geometry as GeoJSON (may be empty)
///
/// Raises:
///     ValueError: If geometries are invalid
///
/// Example:
///     >>> result = oxigdal.intersection(polygon1, polygon2)
#[pyfunction]
pub fn intersection<'py>(
    py: Python<'py>,
    geom1: &Bound<'py, PyDict>,
    geom2: &Bound<'py, PyDict>,
) -> PyResult<Bound<'py, PyDict>> {
    let poly1 = parse_geojson_polygon(geom1)?;
    let poly2 = parse_geojson_polygon(geom2)?;

    let result_polys = algo_vec::intersect_polygons(&poly1, &poly2).map_err(|e| {
        pyo3::exceptions::PyValueError::new_err(format!("Intersection failed: {}", e))
    })?;

    polys_to_geojson(py, &result_polys)
}

/// Computes the difference of two geometries (geom1 - geom2).
///
/// Args:
///     geom1 (dict): First GeoJSON geometry
///     geom2 (dict): Second GeoJSON geometry
///
/// Returns:
///     dict: Difference geometry as GeoJSON
///
/// Raises:
///     ValueError: If geometries are invalid
///
/// Example:
///     >>> result = oxigdal.difference(large_polygon, hole_polygon)
#[pyfunction]
pub fn difference<'py>(
    py: Python<'py>,
    geom1: &Bound<'py, PyDict>,
    geom2: &Bound<'py, PyDict>,
) -> PyResult<Bound<'py, PyDict>> {
    let poly1 = parse_geojson_polygon(geom1)?;
    let poly2 = parse_geojson_polygon(geom2)?;

    let result_polys = algo_vec::difference_polygon(&poly1, &poly2).map_err(|e| {
        pyo3::exceptions::PyValueError::new_err(format!("Difference failed: {}", e))
    })?;

    polys_to_geojson(py, &result_polys)
}

/// Computes the symmetric difference of two geometries.
///
/// Args:
///     geom1 (dict): First GeoJSON geometry
///     geom2 (dict): Second GeoJSON geometry
///
/// Returns:
///     dict: Symmetric difference as GeoJSON
///
/// Example:
///     >>> result = oxigdal.symmetric_difference(polygon1, polygon2)
#[pyfunction]
pub fn symmetric_difference<'py>(
    py: Python<'py>,
    geom1: &Bound<'py, PyDict>,
    geom2: &Bound<'py, PyDict>,
) -> PyResult<Bound<'py, PyDict>> {
    let poly1 = parse_geojson_polygon(geom1)?;
    let poly2 = parse_geojson_polygon(geom2)?;

    let result_polys = algo_vec::symmetric_difference(&poly1, &poly2).map_err(|e| {
        pyo3::exceptions::PyValueError::new_err(format!("Symmetric difference failed: {}", e))
    })?;

    polys_to_geojson(py, &result_polys)
}

/// Calculates the area of a geometry.
///
/// Args:
///     geometry (dict): GeoJSON geometry
///     geodesic (bool): Use geodesic calculation for geographic coordinates (default: False)
///
/// Returns:
///     float: Area in square units (or square meters if geodesic=True)
///
/// Raises:
///     ValueError: If geometry type doesn't support area calculation
///
/// Example:
///     >>> polygon = {
///     ...     "type": "Polygon",
///     ...     "coordinates": [[...]]
///     ... }
///     >>> area = oxigdal.area(polygon)
///     >>>
///     >>> # Calculate geodesic area
///     >>> area_m2 = oxigdal.area(polygon, geodesic=True)
#[pyfunction]
#[pyo3(signature = (geometry, geodesic=false))]
pub fn area(geometry: &Bound<'_, PyDict>, geodesic: bool) -> PyResult<f64> {
    let geom_type: String = geometry
        .get_item("type")
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Missing 'type' field"))?
        .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("Missing 'type' field"))?
        .extract()?;

    let method = if geodesic {
        AreaMethod::Geodetic
    } else {
        AreaMethod::Planar
    };

    match geom_type.as_str() {
        "Polygon" => {
            let polygon = parse_geojson_polygon(geometry)?;
            algo_vec::area_polygon(&polygon, method).map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("Area calculation failed: {}", e))
            })
        }
        "MultiPolygon" => {
            let coords_obj = geometry
                .get_item("coordinates")
                .map_err(|_| {
                    pyo3::exceptions::PyValueError::new_err("Missing 'coordinates' field")
                })?
                .ok_or_else(|| {
                    pyo3::exceptions::PyValueError::new_err("Missing 'coordinates' field")
                })?;
            let polys_list = coords_obj.downcast::<PyList>().map_err(|_| {
                pyo3::exceptions::PyValueError::new_err("'coordinates' must be a list")
            })?;

            let mut total_area = 0.0;
            for poly_item in polys_list.iter() {
                // Build a temporary dict for each polygon
                let poly_dict = PyDict::new(geometry.py());
                poly_dict.set_item("type", "Polygon")?;
                poly_dict.set_item("coordinates", poly_item)?;
                let polygon = parse_geojson_polygon(&poly_dict)?;
                let poly_area = algo_vec::area_polygon(&polygon, method).map_err(|e| {
                    pyo3::exceptions::PyValueError::new_err(format!(
                        "Area calculation failed: {}",
                        e
                    ))
                })?;
                total_area += poly_area;
            }
            Ok(total_area)
        }
        _ => Err(pyo3::exceptions::PyValueError::new_err(format!(
            "Geometry type '{}' does not support area calculation",
            geom_type
        ))),
    }
}

/// Calculates the length of a geometry.
///
/// Args:
///     geometry (dict): GeoJSON geometry
///     geodesic (bool): Use geodesic calculation for geographic coordinates (default: False)
///
/// Returns:
///     float: Length in linear units (or meters if geodesic=True)
///
/// Raises:
///     ValueError: If geometry type doesn't support length calculation
///
/// Example:
///     >>> linestring = {
///     ...     "type": "LineString",
///     ...     "coordinates": [[0, 0], [1, 1]]
///     ... }
///     >>> length = oxigdal.length(linestring)
///     >>>
///     >>> # Calculate geodesic length
///     >>> length_m = oxigdal.length(linestring, geodesic=True)
#[pyfunction]
#[pyo3(signature = (geometry, geodesic=false))]
pub fn length(geometry: &Bound<'_, PyDict>, geodesic: bool) -> PyResult<f64> {
    let geom_type: String = geometry
        .get_item("type")
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Missing 'type' field"))?
        .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("Missing 'type' field"))?
        .extract()?;

    match geom_type.as_str() {
        "LineString" => {
            let ls = parse_geojson_linestring(geometry)?;
            if geodesic {
                compute_geodesic_length(&ls.coords)
            } else {
                Ok(compute_planar_length(&ls.coords))
            }
        }
        "MultiLineString" => {
            let coords_obj = geometry
                .get_item("coordinates")
                .map_err(|_| {
                    pyo3::exceptions::PyValueError::new_err("Missing 'coordinates' field")
                })?
                .ok_or_else(|| {
                    pyo3::exceptions::PyValueError::new_err("Missing 'coordinates' field")
                })?;
            let lines_list = coords_obj.downcast::<PyList>().map_err(|_| {
                pyo3::exceptions::PyValueError::new_err("'coordinates' must be a list")
            })?;

            let mut total_length = 0.0;
            for line_item in lines_list.iter() {
                let ring_list = line_item
                    .downcast::<PyList>()
                    .map_err(|_| pyo3::exceptions::PyValueError::new_err("Ring must be a list"))?;
                let coords = extract_ring_from_pylist(ring_list)?;
                if geodesic {
                    total_length += compute_geodesic_length(&coords)?;
                } else {
                    total_length += compute_planar_length(&coords);
                }
            }
            Ok(total_length)
        }
        "Polygon" => {
            let polygon = parse_geojson_polygon(geometry)?;
            // Length of polygon = perimeter of all rings
            let mut total = if geodesic {
                compute_geodesic_length(&polygon.exterior.coords)?
            } else {
                compute_planar_length(&polygon.exterior.coords)
            };
            for interior in &polygon.interiors {
                if geodesic {
                    total += compute_geodesic_length(&interior.coords)?;
                } else {
                    total += compute_planar_length(&interior.coords);
                }
            }
            Ok(total)
        }
        "MultiPolygon" => {
            let coords_obj = geometry
                .get_item("coordinates")
                .map_err(|_| {
                    pyo3::exceptions::PyValueError::new_err("Missing 'coordinates' field")
                })?
                .ok_or_else(|| {
                    pyo3::exceptions::PyValueError::new_err("Missing 'coordinates' field")
                })?;
            let polys_list = coords_obj.downcast::<PyList>().map_err(|_| {
                pyo3::exceptions::PyValueError::new_err("'coordinates' must be a list")
            })?;

            let mut total = 0.0;
            for poly_item in polys_list.iter() {
                let rings = poly_item.downcast::<PyList>().map_err(|_| {
                    pyo3::exceptions::PyValueError::new_err("Polygon rings must be a list")
                })?;
                for ring_item in rings.iter() {
                    let ring = ring_item.downcast::<PyList>().map_err(|_| {
                        pyo3::exceptions::PyValueError::new_err("Ring must be a list")
                    })?;
                    let coords = extract_ring_from_pylist(ring)?;
                    if geodesic {
                        total += compute_geodesic_length(&coords)?;
                    } else {
                        total += compute_planar_length(&coords);
                    }
                }
            }
            Ok(total)
        }
        _ => Err(pyo3::exceptions::PyValueError::new_err(format!(
            "Geometry type '{}' does not support length calculation",
            geom_type
        ))),
    }
}

/// Simplifies a geometry using the Douglas-Peucker algorithm.
///
/// Args:
///     geometry (dict): GeoJSON geometry
///     tolerance (float): Simplification tolerance
///     preserve_topology (bool): Preserve topology (default: True)
///
/// Returns:
///     dict: Simplified geometry
///
/// Raises:
///     ValueError: If geometry is invalid or tolerance is negative
///
/// Example:
///     >>> simplified = oxigdal.simplify(complex_linestring, tolerance=10.0)
///     >>>
///     >>> # Simplify without topology preservation
///     >>> simplified = oxigdal.simplify(polygon, tolerance=5.0, preserve_topology=False)
#[pyfunction]
#[pyo3(signature = (geometry, tolerance, preserve_topology=true))]
pub fn simplify<'py>(
    py: Python<'py>,
    geometry: &Bound<'py, PyDict>,
    tolerance: f64,
    preserve_topology: bool,
) -> PyResult<Bound<'py, PyDict>> {
    if tolerance < 0.0 {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Tolerance must be non-negative",
        ));
    }

    let geom_type: String = geometry
        .get_item("type")
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Missing 'type' field"))?
        .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("Missing 'type' field"))?
        .extract()?;

    let method = if preserve_topology {
        SimplifyMethod::TopologyPreserving
    } else {
        SimplifyMethod::DouglasPeucker
    };

    match geom_type.as_str() {
        "LineString" => {
            let ls = parse_geojson_linestring(geometry)?;
            let simplified =
                algo_vec::simplify_linestring(&ls, tolerance, method).map_err(|e| {
                    pyo3::exceptions::PyValueError::new_err(format!("Simplification failed: {}", e))
                })?;

            let result = PyDict::new(py);
            result.set_item("type", "LineString")?;
            result.set_item("coordinates", coords_to_pylist(py, &simplified.coords)?)?;
            Ok(result)
        }
        "Polygon" => {
            let polygon = parse_geojson_polygon(geometry)?;
            let simplified =
                algo_vec::simplify_polygon(&polygon, tolerance, method).map_err(|e| {
                    pyo3::exceptions::PyValueError::new_err(format!("Simplification failed: {}", e))
                })?;
            polygon_to_geojson(py, &simplified)
        }
        _ => {
            // For unsupported types, return a copy
            let result_dict = PyDict::new(py);
            for (key, value) in geometry {
                result_dict.set_item(key, value)?;
            }
            Ok(result_dict)
        }
    }
}

/// Computes the centroid of a geometry.
///
/// Args:
///     geometry (dict): GeoJSON geometry
///
/// Returns:
///     dict: Centroid as Point geometry
///
/// Example:
///     >>> polygon = {"type": "Polygon", "coordinates": [[...]]}
///     >>> centroid = oxigdal.centroid(polygon)
///     >>> print(centroid)  # {"type": "Point", "coordinates": [x, y]}
#[pyfunction]
pub fn centroid<'py>(
    py: Python<'py>,
    geometry: &Bound<'py, PyDict>,
) -> PyResult<Bound<'py, PyDict>> {
    let geom_type: String = geometry
        .get_item("type")
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Missing 'type' field"))?
        .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("Missing 'type' field"))?
        .extract()?;

    let center = match geom_type.as_str() {
        "Point" => {
            let point = parse_geojson_point(geometry)?;
            algo_vec::centroid_point(&point)
        }
        "LineString" => {
            let ls = parse_geojson_linestring(geometry)?;
            algo_vec::centroid_linestring(&ls).map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("Centroid failed: {}", e))
            })?
        }
        "Polygon" => {
            let polygon = parse_geojson_polygon(geometry)?;
            algo_vec::centroid_polygon(&polygon).map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("Centroid failed: {}", e))
            })?
        }
        "MultiPoint" => {
            // Compute average of all points
            let all_coords = extract_all_coordinates(geometry)?;
            if all_coords.is_empty() {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "Cannot compute centroid of empty geometry",
                ));
            }
            let mut sum_x = 0.0;
            let mut sum_y = 0.0;
            for c in &all_coords {
                sum_x += c.x;
                sum_y += c.y;
            }
            let n = all_coords.len() as f64;
            Point::from_coord(Coordinate::new_2d(sum_x / n, sum_y / n))
        }
        _ => {
            // For other types, compute average of all coordinates
            let all_coords = extract_all_coordinates(geometry)?;
            if all_coords.is_empty() {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "Cannot compute centroid of empty geometry",
                ));
            }
            let mut sum_x = 0.0;
            let mut sum_y = 0.0;
            for c in &all_coords {
                sum_x += c.x;
                sum_y += c.y;
            }
            let n = all_coords.len() as f64;
            Point::from_coord(Coordinate::new_2d(sum_x / n, sum_y / n))
        }
    };

    let result_dict = PyDict::new(py);
    result_dict.set_item("type", "Point")?;
    let coord_list = PyList::new(py, [center.coord.x, center.coord.y])
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("{}", e)))?;
    result_dict.set_item("coordinates", coord_list)?;
    Ok(result_dict)
}

/// Computes the convex hull of a geometry.
///
/// Args:
///     geometry (dict): GeoJSON geometry
///
/// Returns:
///     dict: Convex hull as Polygon geometry
///
/// Example:
///     >>> points = {"type": "MultiPoint", "coordinates": [[...]]}
///     >>> hull = oxigdal.convex_hull(points)
#[pyfunction]
pub fn convex_hull<'py>(
    py: Python<'py>,
    geometry: &Bound<'py, PyDict>,
) -> PyResult<Bound<'py, PyDict>> {
    let all_coords = extract_all_coordinates(geometry)?;

    if all_coords.len() < 3 {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Need at least 3 points for convex hull computation",
        ));
    }

    let hull_coords = algo_vec::convex_hull(&all_coords).map_err(|e| {
        pyo3::exceptions::PyValueError::new_err(format!("Convex hull failed: {}", e))
    })?;

    // Close the ring for GeoJSON
    let mut closed = hull_coords;
    if let Some(first) = closed.first().copied() {
        closed.push(first);
    }

    let result = PyDict::new(py);
    result.set_item("type", "Polygon")?;
    let rings = PyList::empty(py);
    rings.append(coords_to_pylist(py, &closed)?)?;
    result.set_item("coordinates", rings)?;
    Ok(result)
}

/// Computes the envelope (bounding box) of a geometry.
///
/// Args:
///     geometry (dict): GeoJSON geometry
///
/// Returns:
///     list: Bounding box as [minx, miny, maxx, maxy]
///
/// Example:
///     >>> polygon = {"type": "Polygon", "coordinates": [[...]]}
///     >>> bbox = oxigdal.envelope(polygon)
///     >>> print(bbox)  # [minx, miny, maxx, maxy]
#[pyfunction]
pub fn envelope(geometry: &Bound<'_, PyDict>) -> PyResult<Vec<f64>> {
    let all_coords = extract_all_coordinates(geometry)?;

    if all_coords.is_empty() {
        return Ok(vec![0.0, 0.0, 0.0, 0.0]);
    }

    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for coord in &all_coords {
        if coord.x < min_x {
            min_x = coord.x;
        }
        if coord.y < min_y {
            min_y = coord.y;
        }
        if coord.x > max_x {
            max_x = coord.x;
        }
        if coord.y > max_y {
            max_y = coord.y;
        }
    }

    Ok(vec![min_x, min_y, max_x, max_y])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_validation() {
        pyo3::prepare_freethreaded_python();
        Python::with_gil(|py| {
            let geom = PyDict::new(py);
            geom.set_item("type", "Point").ok();

            assert!(buffer_geometry(py, &geom, -1.0, 8, "round", "round", 5.0).is_err());
            assert!(buffer_geometry(py, &geom, 10.0, 0, "round", "round", 5.0).is_err());
            assert!(buffer_geometry(py, &geom, 10.0, 8, "invalid", "round", 5.0).is_err());
        });
    }

    #[test]
    fn test_simplify_validation() {
        pyo3::prepare_freethreaded_python();
        Python::with_gil(|py| {
            let geom = PyDict::new(py);
            geom.set_item("type", "LineString").ok();

            assert!(simplify(py, &geom, -1.0, true).is_err());
        });
    }

    #[test]
    fn test_area_validation() {
        pyo3::prepare_freethreaded_python();
        Python::with_gil(|py| {
            let geom = PyDict::new(py);
            geom.set_item("type", "Point").ok();

            // Points don't support area calculation
            assert!(area(&geom, false).is_err());
        });
    }

    #[test]
    fn test_area_polygon() {
        pyo3::prepare_freethreaded_python();
        Python::with_gil(|py| {
            let geom = PyDict::new(py);
            geom.set_item("type", "Polygon").ok();
            let outer = PyList::empty(py);
            let ring = PyList::empty(py);
            for coord in &[
                [0.0, 0.0],
                [10.0, 0.0],
                [10.0, 10.0],
                [0.0, 10.0],
                [0.0, 0.0],
            ] {
                let c = PyList::new(py, coord).ok();
                if let Some(cl) = c {
                    ring.append(cl).ok();
                }
            }
            outer.append(ring).ok();
            geom.set_item("coordinates", outer).ok();

            let result = area(&geom, false);
            assert!(result.is_ok());
            if let Ok(a) = result {
                assert!((a - 100.0).abs() < 1e-8);
            }
        });
    }

    #[test]
    fn test_envelope_point() {
        pyo3::prepare_freethreaded_python();
        Python::with_gil(|py| {
            let geom = PyDict::new(py);
            geom.set_item("type", "Point").ok();
            let coords = PyList::new(py, [5.0, 10.0]).ok();
            if let Some(cl) = coords {
                geom.set_item("coordinates", cl).ok();
            }

            let result = envelope(&geom);
            assert!(result.is_ok());
            if let Ok(bbox) = result {
                assert_eq!(bbox, vec![5.0, 10.0, 5.0, 10.0]);
            }
        });
    }
}
