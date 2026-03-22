//! Internal helper functions for vector operations
//!
//! Provides GeoJSON parsing, coordinate conversion, JSON/Python interop,
//! and geometry repair utilities shared across all vector submodules.

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use serde_json::Value as JsonValue;

use oxigdal_algorithms::vector::{
    self as algo_vec, BufferCapStyle, BufferJoinStyle, Coordinate, DistanceMethod, LineString,
    Point, Polygon,
};

// ============================================================================
// GeoJSON <-> oxigdal-core conversion helpers
// ============================================================================

/// Extracts a flat list of f64 from a Python list (for a single coordinate like [x, y])
pub(super) fn extract_coord_from_pylist(list: &Bound<'_, PyList>) -> PyResult<Coordinate> {
    let len = list.len();
    if len < 2 {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Coordinate must have at least 2 values [x, y]",
        ));
    }
    let x: f64 = list.get_item(0)?.extract()?;
    let y: f64 = list.get_item(1)?.extract()?;
    if len >= 3 {
        let z: f64 = list.get_item(2)?.extract()?;
        Ok(Coordinate::new_3d(x, y, z))
    } else {
        Ok(Coordinate::new_2d(x, y))
    }
}

/// Extracts a ring (list of coordinates) from a Python list
pub(super) fn extract_ring_from_pylist(list: &Bound<'_, PyList>) -> PyResult<Vec<Coordinate>> {
    let mut coords = Vec::with_capacity(list.len());
    for item in list.iter() {
        let coord_list = item
            .cast::<PyList>()
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("Expected list of coordinates"))?;
        coords.push(extract_coord_from_pylist(coord_list)?);
    }
    Ok(coords)
}

/// Parses a GeoJSON Point geometry dict into an oxigdal Point
pub(super) fn parse_geojson_point(geometry: &Bound<'_, PyDict>) -> PyResult<Point> {
    let coords_obj = geometry
        .get_item("coordinates")
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Missing 'coordinates' field"))?
        .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("Missing 'coordinates' field"))?;
    let coords_list = coords_obj
        .cast::<PyList>()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("'coordinates' must be a list"))?;
    let coord = extract_coord_from_pylist(coords_list)?;
    Ok(Point::from_coord(coord))
}

/// Parses a GeoJSON LineString geometry dict into an oxigdal LineString
pub(super) fn parse_geojson_linestring(geometry: &Bound<'_, PyDict>) -> PyResult<LineString> {
    let coords_obj = geometry
        .get_item("coordinates")
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Missing 'coordinates' field"))?
        .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("Missing 'coordinates' field"))?;
    let coords_list = coords_obj
        .cast::<PyList>()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("'coordinates' must be a list"))?;
    let coords = extract_ring_from_pylist(coords_list)?;
    LineString::new(coords)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("Invalid LineString: {}", e)))
}

/// Parses a GeoJSON Polygon geometry dict into an oxigdal Polygon
pub(super) fn parse_geojson_polygon(geometry: &Bound<'_, PyDict>) -> PyResult<Polygon> {
    let coords_obj = geometry
        .get_item("coordinates")
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Missing 'coordinates' field"))?
        .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("Missing 'coordinates' field"))?;
    let rings_list = coords_obj.cast::<PyList>().map_err(|_| {
        pyo3::exceptions::PyValueError::new_err("'coordinates' must be a list of rings")
    })?;

    if rings_list.is_empty() {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Polygon must have at least one ring (exterior)",
        ));
    }

    // First ring is exterior
    let ext_list = rings_list
        .get_item(0)?
        .cast::<PyList>()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Ring must be a list of coordinates"))?
        .clone();
    let ext_coords = extract_ring_from_pylist(&ext_list)?;
    let exterior = LineString::new(ext_coords).map_err(|e| {
        pyo3::exceptions::PyValueError::new_err(format!("Invalid exterior ring: {}", e))
    })?;

    // Remaining rings are interior (holes)
    let mut interiors = Vec::new();
    for i in 1..rings_list.len() {
        let hole_list = rings_list
            .get_item(i)?
            .cast::<PyList>()
            .map_err(|_| {
                pyo3::exceptions::PyValueError::new_err("Ring must be a list of coordinates")
            })?
            .clone();
        let hole_coords = extract_ring_from_pylist(&hole_list)?;
        let hole = LineString::new(hole_coords).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Invalid interior ring: {}", e))
        })?;
        interiors.push(hole);
    }

    Polygon::new(exterior, interiors)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("Invalid Polygon: {}", e)))
}

/// Extracts all coordinates from any GeoJSON geometry as a flat list of Coordinates
pub(super) fn extract_all_coordinates(geometry: &Bound<'_, PyDict>) -> PyResult<Vec<Coordinate>> {
    let geom_type: String = geometry
        .get_item("type")
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Missing 'type' field"))?
        .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("Missing 'type' field"))?
        .extract()?;

    let coords_obj = geometry
        .get_item("coordinates")
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Missing 'coordinates' field"))?
        .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("Missing 'coordinates' field"))?;

    match geom_type.as_str() {
        "Point" => {
            let list = coords_obj.cast::<PyList>().map_err(|_| {
                pyo3::exceptions::PyValueError::new_err("'coordinates' must be a list")
            })?;
            Ok(vec![extract_coord_from_pylist(list)?])
        }
        "MultiPoint" | "LineString" => {
            let list = coords_obj.cast::<PyList>().map_err(|_| {
                pyo3::exceptions::PyValueError::new_err("'coordinates' must be a list")
            })?;
            extract_ring_from_pylist(list)
        }
        "MultiLineString" | "Polygon" => {
            let rings = coords_obj.cast::<PyList>().map_err(|_| {
                pyo3::exceptions::PyValueError::new_err("'coordinates' must be a list")
            })?;
            let mut all = Vec::new();
            for ring_item in rings.iter() {
                let ring = ring_item
                    .cast::<PyList>()
                    .map_err(|_| pyo3::exceptions::PyValueError::new_err("Ring must be a list"))?;
                all.extend(extract_ring_from_pylist(ring)?);
            }
            Ok(all)
        }
        "MultiPolygon" => {
            let polys = coords_obj.cast::<PyList>().map_err(|_| {
                pyo3::exceptions::PyValueError::new_err("'coordinates' must be a list")
            })?;
            let mut all = Vec::new();
            for poly_item in polys.iter() {
                let rings = poly_item.cast::<PyList>().map_err(|_| {
                    pyo3::exceptions::PyValueError::new_err("Polygon rings must be a list")
                })?;
                for ring_item in rings.iter() {
                    let ring = ring_item.cast::<PyList>().map_err(|_| {
                        pyo3::exceptions::PyValueError::new_err("Ring must be a list")
                    })?;
                    all.extend(extract_ring_from_pylist(ring)?);
                }
            }
            Ok(all)
        }
        _ => Err(pyo3::exceptions::PyValueError::new_err(format!(
            "Unsupported geometry type: {}",
            geom_type
        ))),
    }
}

/// Converts a Polygon to a GeoJSON Python dict
pub(super) fn polygon_to_geojson<'py>(
    py: Python<'py>,
    polygon: &Polygon,
) -> PyResult<Bound<'py, PyDict>> {
    let result = PyDict::new(py);
    result.set_item("type", "Polygon")?;

    let rings = PyList::empty(py);
    // Exterior ring
    let ext = coords_to_pylist(py, &polygon.exterior.coords)?;
    rings.append(ext)?;
    // Interior rings
    for interior in &polygon.interiors {
        let hole = coords_to_pylist(py, &interior.coords)?;
        rings.append(hole)?;
    }
    result.set_item("coordinates", rings)?;
    Ok(result)
}

/// Converts a list of Coordinates to a Python list of coordinate lists
pub(super) fn coords_to_pylist<'py>(
    py: Python<'py>,
    coords: &[Coordinate],
) -> PyResult<Bound<'py, PyList>> {
    let list = PyList::empty(py);
    for coord in coords {
        let c = if coord.has_z() {
            PyList::new(py, [coord.x, coord.y, coord.z.unwrap_or(0.0)])
                .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("{}", e)))?
        } else {
            PyList::new(py, [coord.x, coord.y])
                .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("{}", e)))?
        };
        list.append(c)?;
    }
    Ok(list)
}

/// Converts a BufferCapStyle from string
pub(super) fn parse_cap_style(s: &str) -> BufferCapStyle {
    match s {
        "flat" => BufferCapStyle::Flat,
        "square" => BufferCapStyle::Square,
        _ => BufferCapStyle::Round,
    }
}

/// Converts a BufferJoinStyle from string
pub(super) fn parse_join_style(s: &str) -> BufferJoinStyle {
    match s {
        "mitre" => BufferJoinStyle::Miter,
        "bevel" => BufferJoinStyle::Bevel,
        _ => BufferJoinStyle::Round,
    }
}

/// Parses an EPSG code from a CRS string like "EPSG:4326"
pub(super) fn parse_epsg_code(crs_str: &str) -> PyResult<u32> {
    let trimmed = crs_str.trim();
    if let Some(code_str) = trimmed
        .strip_prefix("EPSG:")
        .or_else(|| trimmed.strip_prefix("epsg:"))
    {
        code_str.parse::<u32>().map_err(|_| {
            pyo3::exceptions::PyValueError::new_err(format!("Invalid EPSG code: '{}'", code_str))
        })
    } else {
        Err(pyo3::exceptions::PyValueError::new_err(format!(
            "CRS must be in format 'EPSG:XXXX', got '{}'",
            trimmed
        )))
    }
}

/// Transforms a list of coordinates using an oxigdal-proj Transformer
pub(super) fn transform_coordinates(
    coords: &[Coordinate],
    transformer: &oxigdal_proj::Transformer,
) -> PyResult<Vec<Coordinate>> {
    let mut result = Vec::with_capacity(coords.len());
    for coord in coords {
        let proj_coord = oxigdal_proj::Coordinate::new(coord.x, coord.y);
        let transformed = transformer.transform(&proj_coord).map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!(
                "Coordinate transformation failed: {}",
                e
            ))
        })?;
        if let Some(z) = coord.z {
            result.push(Coordinate::new_3d(transformed.x, transformed.y, z));
        } else {
            result.push(Coordinate::new_2d(transformed.x, transformed.y));
        }
    }
    Ok(result)
}

/// Recursively transforms all coordinates in a GeoJSON geometry Python dict
pub(super) fn transform_geojson_coords<'py>(
    py: Python<'py>,
    geometry: &Bound<'py, PyDict>,
    transformer: &oxigdal_proj::Transformer,
) -> PyResult<Bound<'py, PyDict>> {
    let geom_type: String = geometry
        .get_item("type")
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Missing 'type' field"))?
        .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("Missing 'type' field"))?
        .extract()?;

    let all_coords = extract_all_coordinates(geometry)?;
    let transformed_coords = transform_coordinates(&all_coords, transformer)?;

    // Rebuild the geometry with transformed coordinates
    let result = PyDict::new(py);
    result.set_item("type", &geom_type)?;

    match geom_type.as_str() {
        "Point" => {
            if let Some(c) = transformed_coords.first() {
                let coord_list = if c.has_z() {
                    PyList::new(py, [c.x, c.y, c.z.unwrap_or(0.0)])
                        .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("{}", e)))?
                } else {
                    PyList::new(py, [c.x, c.y])
                        .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("{}", e)))?
                };
                result.set_item("coordinates", coord_list)?;
            }
        }
        "MultiPoint" | "LineString" => {
            result.set_item("coordinates", coords_to_pylist(py, &transformed_coords)?)?;
        }
        "MultiLineString" | "Polygon" => {
            // Need to rebuild the ring structure from the original geometry
            let coords_obj = geometry
                .get_item("coordinates")
                .map_err(|_| pyo3::exceptions::PyValueError::new_err("Missing 'coordinates'"))?
                .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("Missing 'coordinates'"))?;
            let rings = coords_obj.cast::<PyList>().map_err(|_| {
                pyo3::exceptions::PyValueError::new_err("'coordinates' must be a list")
            })?;

            let rings_result = PyList::empty(py);
            let mut offset = 0;
            for ring_item in rings.iter() {
                let ring = ring_item
                    .cast::<PyList>()
                    .map_err(|_| pyo3::exceptions::PyValueError::new_err("Ring must be a list"))?;
                let ring_len = ring.len();
                let end = (offset + ring_len).min(transformed_coords.len());
                let ring_coords = &transformed_coords[offset..end];
                rings_result.append(coords_to_pylist(py, ring_coords)?)?;
                offset = end;
            }
            result.set_item("coordinates", rings_result)?;
        }
        "MultiPolygon" => {
            let coords_obj = geometry
                .get_item("coordinates")
                .map_err(|_| pyo3::exceptions::PyValueError::new_err("Missing 'coordinates'"))?
                .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("Missing 'coordinates'"))?;
            let polys = coords_obj.cast::<PyList>().map_err(|_| {
                pyo3::exceptions::PyValueError::new_err("'coordinates' must be a list")
            })?;

            let polys_result = PyList::empty(py);
            let mut offset = 0;
            for poly_item in polys.iter() {
                let rings = poly_item.cast::<PyList>().map_err(|_| {
                    pyo3::exceptions::PyValueError::new_err("Polygon rings must be a list")
                })?;
                let rings_result = PyList::empty(py);
                for ring_item in rings.iter() {
                    let ring = ring_item.cast::<PyList>().map_err(|_| {
                        pyo3::exceptions::PyValueError::new_err("Ring must be a list")
                    })?;
                    let ring_len = ring.len();
                    let end = (offset + ring_len).min(transformed_coords.len());
                    let ring_coords = &transformed_coords[offset..end];
                    rings_result.append(coords_to_pylist(py, ring_coords)?)?;
                    offset = end;
                }
                polys_result.append(rings_result)?;
            }
            result.set_item("coordinates", polys_result)?;
        }
        _ => {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Unsupported geometry type for transformation: {}",
                geom_type
            )));
        }
    }

    Ok(result)
}

/// Computes the planar length of a list of coordinates (perimeter/linestring length)
pub(super) fn compute_planar_length(coords: &[Coordinate]) -> f64 {
    let mut length = 0.0;
    for i in 0..coords.len().saturating_sub(1) {
        let dx = coords[i + 1].x - coords[i].x;
        let dy = coords[i + 1].y - coords[i].y;
        length += (dx * dx + dy * dy).sqrt();
    }
    length
}

/// Computes geodesic length using the Haversine formula
pub(super) fn compute_geodesic_length(coords: &[Coordinate]) -> PyResult<f64> {
    let mut length = 0.0;
    for i in 0..coords.len().saturating_sub(1) {
        let p1 = Point::from_coord(coords[i]);
        let p2 = Point::from_coord(coords[i + 1]);
        let dist = algo_vec::distance_point_to_point(&p1, &p2, DistanceMethod::Haversine).map_err(
            |e| {
                pyo3::exceptions::PyValueError::new_err(format!(
                    "Geodesic distance calculation failed: {}",
                    e
                ))
            },
        )?;
        length += dist;
    }
    Ok(length)
}

// ============================================================================
// Internal utility helpers
// ============================================================================

/// Extracts the "type" field from a GeoJSON geometry dict
pub(super) fn extract_geom_type(geom: &Bound<'_, PyDict>) -> PyResult<String> {
    geom.get_item("type")
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Missing 'type' field"))?
        .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("Missing 'type' field"))?
        .extract()
}

/// Repairs a ring by removing consecutive duplicates and ensuring closure
pub(super) fn repair_ring(coords: &[Coordinate]) -> PyResult<Vec<Coordinate>> {
    let mut repaired = repair_linestring_coords(coords)?;

    // Ensure ring closure
    if repaired.len() >= 3 {
        let first = repaired[0];
        let last = repaired[repaired.len() - 1];
        if (first.x - last.x).abs() > f64::EPSILON || (first.y - last.y).abs() > f64::EPSILON {
            repaired.push(first);
        }
    }

    Ok(repaired)
}

/// Repairs linestring coordinates by removing consecutive duplicates
pub(super) fn repair_linestring_coords(coords: &[Coordinate]) -> PyResult<Vec<Coordinate>> {
    if coords.is_empty() {
        return Ok(Vec::new());
    }

    let mut repaired = Vec::with_capacity(coords.len());
    repaired.push(coords[0]);

    for coord in &coords[1..] {
        if let Some(last) = repaired.last() {
            if (coord.x - last.x).abs() > f64::EPSILON || (coord.y - last.y).abs() > f64::EPSILON {
                repaired.push(*coord);
            }
        }
    }

    Ok(repaired)
}

// ============================================================================
// JSON <-> Python conversion helpers
// ============================================================================

/// Helper function to convert serde_json::Value to Python object
pub(super) fn json_to_python<'py>(
    py: Python<'py>,
    value: &JsonValue,
) -> PyResult<Bound<'py, PyDict>> {
    match value {
        JsonValue::Object(map) => {
            let dict = PyDict::new(py);
            for (key, val) in map {
                let py_val = json_value_to_python(py, val)?;
                dict.set_item(key, py_val)?;
            }
            Ok(dict)
        }
        _ => Err(pyo3::exceptions::PyValueError::new_err(
            "Expected JSON object at root",
        )),
    }
}

/// Recursively converts a serde_json::Value to a Python object
pub(super) fn json_value_to_python<'py>(
    py: Python<'py>,
    value: &JsonValue,
) -> PyResult<Bound<'py, PyAny>> {
    match value {
        JsonValue::Null => Ok(py.None().into_bound(py)),
        JsonValue::Bool(b) => {
            let py_bool = b.into_pyobject(py).map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("Bool conversion: {}", e))
            })?;
            Ok(py_bool.to_owned().into_any())
        }
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i.into_pyobject(py)
                    .map_err(|e| {
                        pyo3::exceptions::PyValueError::new_err(format!("Int conversion: {}", e))
                    })?
                    .into_any())
            } else if let Some(f) = n.as_f64() {
                Ok(f.into_pyobject(py)
                    .map_err(|e| {
                        pyo3::exceptions::PyValueError::new_err(format!("Float conversion: {}", e))
                    })?
                    .into_any())
            } else {
                Ok(py.None().into_bound(py))
            }
        }
        JsonValue::String(s) => Ok(s
            .into_pyobject(py)
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("String conversion: {}", e))
            })?
            .into_any()),
        JsonValue::Array(arr) => {
            let list = PyList::empty(py);
            for item in arr {
                list.append(json_value_to_python(py, item)?)?;
            }
            Ok(list.into_any())
        }
        JsonValue::Object(map) => {
            let dict = PyDict::new(py);
            for (key, val) in map {
                dict.set_item(key, json_value_to_python(py, val)?)?;
            }
            Ok(dict.into_any())
        }
    }
}

/// Helper function to convert Python dict to serde_json::Value
pub(super) fn python_to_json(dict: &Bound<'_, PyDict>) -> PyResult<JsonValue> {
    let mut map = serde_json::Map::new();

    for (key, value) in dict {
        let key_str: String = key.extract()?;
        let json_val = python_value_to_json(&value)?;
        map.insert(key_str, json_val);
    }

    Ok(JsonValue::Object(map))
}

/// Recursively converts a Python object to serde_json::Value
pub(super) fn python_value_to_json(value: &Bound<'_, PyAny>) -> PyResult<JsonValue> {
    if value.is_none() {
        Ok(JsonValue::Null)
    } else if let Ok(b) = value.extract::<bool>() {
        Ok(JsonValue::Bool(b))
    } else if let Ok(i) = value.extract::<i64>() {
        Ok(JsonValue::Number(i.into()))
    } else if let Ok(f) = value.extract::<f64>() {
        serde_json::Number::from_f64(f)
            .map(JsonValue::Number)
            .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("Invalid float value"))
    } else if let Ok(s) = value.extract::<String>() {
        Ok(JsonValue::String(s))
    } else if let Ok(list) = value.cast::<PyList>() {
        let mut arr = Vec::new();
        for item in list.iter() {
            arr.push(python_value_to_json(&item)?);
        }
        Ok(JsonValue::Array(arr))
    } else if let Ok(nested_dict) = value.cast::<PyDict>() {
        python_to_json(nested_dict)
    } else {
        Err(pyo3::exceptions::PyValueError::new_err(
            "Unsupported Python type for JSON conversion",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_epsg_code() {
        assert_eq!(parse_epsg_code("EPSG:4326").ok(), Some(4326));
        assert_eq!(parse_epsg_code("EPSG:3857").ok(), Some(3857));
        assert!(parse_epsg_code("WKT:something").is_err());
    }

    #[test]
    fn test_repair_ring() {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(0.0, 0.0), // duplicate
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let repaired = repair_ring(&coords);
        assert!(repaired.is_ok());
        let r = repaired.ok();
        assert!(r.is_some());
        if let Some(ring) = r {
            // Should remove the duplicate and keep closure
            assert!(ring.len() >= 4);
        }
    }
}
