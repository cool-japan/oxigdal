//! Analysis operations for Python bindings
//!
//! Provides distance calculation, geometry validation, repair, coordinate transformation,
//! bounding box clipping, polygon merging, and dissolve operations.

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use oxigdal_algorithms::vector::{self as algo_vec, DistanceMethod, LineString, Point, Polygon};

use super::helpers::{
    coords_to_pylist, extract_all_coordinates, extract_geom_type, parse_epsg_code,
    parse_geojson_linestring, parse_geojson_point, parse_geojson_polygon, polygon_to_geojson,
    repair_linestring_coords, repair_ring, transform_geojson_coords,
};
use super::operations::envelope;

/// Computes the distance between two geometries.
///
/// Args:
///     geom1 (dict): First GeoJSON geometry
///     geom2 (dict): Second GeoJSON geometry
///     geodesic (bool): Use geodesic distance for geographic coordinates (default: False)
///
/// Returns:
///     float: Distance in units of the coordinate system (or meters if geodesic=True)
///
/// Example:
///     >>> point1 = {"type": "Point", "coordinates": [0, 0]}
///     >>> point2 = {"type": "Point", "coordinates": [3, 4]}
///     >>> dist = oxigdal.distance(point1, point2)
///     >>> print(dist)  # 5.0
///     >>>
///     >>> # Calculate geodesic distance
///     >>> dist_m = oxigdal.distance(point1, point2, geodesic=True)
#[pyfunction]
#[pyo3(signature = (geom1, geom2, geodesic=false))]
pub fn distance(
    geom1: &Bound<'_, PyDict>,
    geom2: &Bound<'_, PyDict>,
    geodesic: bool,
) -> PyResult<f64> {
    let type1 = extract_geom_type(geom1)?;
    let type2 = extract_geom_type(geom2)?;

    let method = if geodesic {
        DistanceMethod::Haversine
    } else {
        DistanceMethod::Euclidean
    };

    // Point to Point
    if type1 == "Point" && type2 == "Point" {
        let p1 = parse_geojson_point(geom1)?;
        let p2 = parse_geojson_point(geom2)?;
        return algo_vec::distance_point_to_point(&p1, &p2, method).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Distance failed: {}", e))
        });
    }

    // Point to LineString
    if type1 == "Point" && type2 == "LineString" {
        let p = parse_geojson_point(geom1)?;
        let ls = parse_geojson_linestring(geom2)?;
        return algo_vec::distance_point_to_linestring(&p, &ls, method).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Distance failed: {}", e))
        });
    }
    if type1 == "LineString" && type2 == "Point" {
        let p = parse_geojson_point(geom2)?;
        let ls = parse_geojson_linestring(geom1)?;
        return algo_vec::distance_point_to_linestring(&p, &ls, method).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Distance failed: {}", e))
        });
    }

    // Point to Polygon
    if type1 == "Point" && type2 == "Polygon" {
        let p = parse_geojson_point(geom1)?;
        let poly = parse_geojson_polygon(geom2)?;
        return algo_vec::distance_point_to_polygon(&p, &poly, method).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Distance failed: {}", e))
        });
    }
    if type1 == "Polygon" && type2 == "Point" {
        let p = parse_geojson_point(geom2)?;
        let poly = parse_geojson_polygon(geom1)?;
        return algo_vec::distance_point_to_polygon(&p, &poly, method).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Distance failed: {}", e))
        });
    }

    // Generic: compute minimum distance between all pairs of coordinates
    let coords1 = extract_all_coordinates(geom1)?;
    let coords2 = extract_all_coordinates(geom2)?;

    let mut min_dist = f64::INFINITY;
    for c1 in &coords1 {
        for c2 in &coords2 {
            let p1 = Point::from_coord(*c1);
            let p2 = Point::from_coord(*c2);
            let d = algo_vec::distance_point_to_point(&p1, &p2, method).map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("Distance failed: {}", e))
            })?;
            if d < min_dist {
                min_dist = d;
            }
        }
    }

    Ok(if min_dist.is_infinite() {
        0.0
    } else {
        min_dist
    })
}

/// Validates a geometry.
///
/// Args:
///     geometry (dict): GeoJSON geometry
///
/// Returns:
///     tuple: (is_valid, error_message)
///
/// Example:
///     >>> polygon = {"type": "Polygon", "coordinates": [[...]]}
///     >>> is_valid, error = oxigdal.is_valid(polygon)
///     >>> if not is_valid:
///     ...     print(f"Invalid geometry: {error}")
#[pyfunction]
pub fn is_valid(geometry: &Bound<'_, PyDict>) -> PyResult<(bool, Option<String>)> {
    let geom_type = extract_geom_type(geometry)?;

    match geom_type.as_str() {
        "Polygon" => {
            let polygon = parse_geojson_polygon(geometry)?;
            let issues = algo_vec::validate_polygon(&polygon).map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("Validation failed: {}", e))
            })?;

            let errors: Vec<_> = issues
                .iter()
                .filter(|i| i.severity == algo_vec::Severity::Error)
                .collect();

            if errors.is_empty() {
                Ok((true, None))
            } else {
                let descriptions: Vec<_> = errors.iter().map(|i| i.description.clone()).collect();
                Ok((false, Some(descriptions.join("; "))))
            }
        }
        "LineString" => {
            let ls = parse_geojson_linestring(geometry)?;
            let issues = algo_vec::validate_linestring(&ls).map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("Validation failed: {}", e))
            })?;

            let errors: Vec<_> = issues
                .iter()
                .filter(|i| i.severity == algo_vec::Severity::Error)
                .collect();

            if errors.is_empty() {
                Ok((true, None))
            } else {
                let descriptions: Vec<_> = errors.iter().map(|i| i.description.clone()).collect();
                Ok((false, Some(descriptions.join("; "))))
            }
        }
        "Point" => {
            // Points are always valid
            let point = parse_geojson_point(geometry)?;
            if point.coord.x.is_finite() && point.coord.y.is_finite() {
                Ok((true, None))
            } else {
                Ok((false, Some("Point has non-finite coordinates".to_string())))
            }
        }
        _ => {
            // For other types, do basic coordinate validation
            let coords = extract_all_coordinates(geometry);
            match coords {
                Ok(cs) => {
                    for c in &cs {
                        if !c.x.is_finite() || !c.y.is_finite() {
                            return Ok((
                                false,
                                Some("Geometry contains non-finite coordinates".to_string()),
                            ));
                        }
                    }
                    Ok((true, None))
                }
                Err(e) => Ok((false, Some(format!("Invalid geometry: {}", e)))),
            }
        }
    }
}

/// Makes a geometry valid.
///
/// Args:
///     geometry (dict): GeoJSON geometry
///
/// Returns:
///     dict: Valid geometry
///
/// Example:
///     >>> invalid_polygon = {"type": "Polygon", "coordinates": [[...]]}
///     >>> valid_polygon = oxigdal.make_valid(invalid_polygon)
#[pyfunction]
pub fn make_valid<'py>(
    py: Python<'py>,
    geometry: &Bound<'py, PyDict>,
) -> PyResult<Bound<'py, PyDict>> {
    let geom_type = extract_geom_type(geometry)?;

    match geom_type.as_str() {
        "Polygon" => {
            let polygon = parse_geojson_polygon(geometry)?;

            // Repair: remove duplicate consecutive vertices, ensure ring closure
            let repaired_ext = repair_ring(&polygon.exterior.coords)?;
            let exterior = LineString::new(repaired_ext).map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("Ring repair failed: {}", e))
            })?;

            let mut interiors = Vec::new();
            for hole in &polygon.interiors {
                let repaired_hole = repair_ring(&hole.coords)?;
                if repaired_hole.len() >= 4 {
                    let hole_ls = LineString::new(repaired_hole).map_err(|e| {
                        pyo3::exceptions::PyValueError::new_err(format!(
                            "Hole repair failed: {}",
                            e
                        ))
                    })?;
                    interiors.push(hole_ls);
                }
            }

            let repaired = Polygon::new(exterior, interiors).map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("Polygon repair failed: {}", e))
            })?;
            polygon_to_geojson(py, &repaired)
        }
        "LineString" => {
            let ls = parse_geojson_linestring(geometry)?;
            let repaired = repair_linestring_coords(&ls.coords)?;
            let repaired_ls = LineString::new(repaired).map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("LineString repair failed: {}", e))
            })?;

            let result = PyDict::new(py);
            result.set_item("type", "LineString")?;
            result.set_item("coordinates", coords_to_pylist(py, &repaired_ls.coords)?)?;
            Ok(result)
        }
        _ => {
            // For other types, return a copy
            let result_dict = PyDict::new(py);
            for (key, value) in geometry {
                result_dict.set_item(key, value)?;
            }
            Ok(result_dict)
        }
    }
}

/// Transforms geometry coordinates to a different CRS.
///
/// Args:
///     geometry (dict): GeoJSON geometry
///     src_crs (str): Source CRS (EPSG code or WKT)
///     dst_crs (str): Destination CRS (EPSG code or WKT)
///
/// Returns:
///     dict: Transformed geometry
///
/// Example:
///     >>> # Transform from WGS84 to Web Mercator
///     >>> point_wgs84 = {"type": "Point", "coordinates": [-122.4194, 37.7749]}
///     >>> point_mercator = oxigdal.transform(point_wgs84, "EPSG:4326", "EPSG:3857")
///     >>> print(point_mercator['coordinates'])
#[pyfunction]
pub fn transform<'py>(
    py: Python<'py>,
    geometry: &Bound<'py, PyDict>,
    src_crs: &str,
    dst_crs: &str,
) -> PyResult<Bound<'py, PyDict>> {
    let src_epsg = parse_epsg_code(src_crs)?;
    let dst_epsg = parse_epsg_code(dst_crs)?;

    let transformer = oxigdal_proj::Transformer::from_epsg(src_epsg, dst_epsg).map_err(|e| {
        pyo3::exceptions::PyValueError::new_err(format!(
            "Failed to create transformer from {} to {}: {}",
            src_crs, dst_crs, e
        ))
    })?;

    transform_geojson_coords(py, geometry, &transformer)
}

/// Clips geometries by a bounding box.
///
/// Args:
///     geometries (list): List of GeoJSON geometries or FeatureCollection
///     bbox (list): Bounding box [minx, miny, maxx, maxy]
///
/// Returns:
///     list: Clipped geometries
///
/// Example:
///     >>> features = {"type": "FeatureCollection", "features": [...]}
///     >>> clipped = oxigdal.clip_by_bbox(features, [-180, -90, 0, 90])
#[pyfunction]
pub fn clip_by_bbox<'py>(
    py: Python<'py>,
    geometries: &Bound<'py, PyAny>,
    bbox: Vec<f64>,
) -> PyResult<Bound<'py, PyList>> {
    if bbox.len() != 4 {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Bounding box must have 4 elements [minx, miny, maxx, maxy]",
        ));
    }

    let min_x = bbox[0];
    let min_y = bbox[1];
    let max_x = bbox[2];
    let max_y = bbox[3];

    // Try to interpret as a FeatureCollection dict
    if let Ok(dict) = geometries.downcast::<PyDict>() {
        if let Ok(Some(features_obj)) = dict.get_item("features") {
            if let Ok(features_list) = features_obj.downcast::<PyList>() {
                let result = PyList::empty(py);
                for feature_item in features_list.iter() {
                    if let Ok(feature_dict) = feature_item.downcast::<PyDict>() {
                        if let Ok(Some(geom_obj)) = feature_dict.get_item("geometry") {
                            if let Ok(geom_dict) = geom_obj.downcast::<PyDict>() {
                                let geom_bbox = envelope(geom_dict)?;
                                // Check if geometry bbox intersects with clip bbox
                                if geom_bbox[2] >= min_x
                                    && geom_bbox[0] <= max_x
                                    && geom_bbox[3] >= min_y
                                    && geom_bbox[1] <= max_y
                                {
                                    result.append(feature_item)?;
                                }
                            }
                        }
                    }
                }
                return Ok(result);
            }
        }
    }

    // Try as list of geometries
    if let Ok(list) = geometries.downcast::<PyList>() {
        let result = PyList::empty(py);
        for item in list.iter() {
            if let Ok(geom_dict) = item.downcast::<PyDict>() {
                let geom_bbox = envelope(geom_dict)?;
                if geom_bbox[2] >= min_x
                    && geom_bbox[0] <= max_x
                    && geom_bbox[3] >= min_y
                    && geom_bbox[1] <= max_y
                {
                    result.append(item)?;
                }
            }
        }
        return Ok(result);
    }

    Ok(PyList::empty(py))
}

/// Merges overlapping polygons.
///
/// Args:
///     polygons (list): List of GeoJSON polygon geometries
///
/// Returns:
///     list: Merged polygons
///
/// Example:
///     >>> overlapping = [polygon1, polygon2, polygon3]
///     >>> merged = oxigdal.merge_polygons(overlapping)
#[pyfunction]
pub fn merge_polygons<'py>(
    py: Python<'py>,
    polygons: &Bound<'py, PyList>,
) -> PyResult<Bound<'py, PyList>> {
    let mut polys = Vec::new();
    for item in polygons.iter() {
        let dict = item
            .downcast::<PyDict>()
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("Each polygon must be a dict"))?;
        polys.push(parse_geojson_polygon(dict)?);
    }

    let merged = algo_vec::merge_polygons(&polys, 0.0)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("Merge failed: {}", e)))?;

    let result = PyList::empty(py);
    for poly in &merged {
        result.append(polygon_to_geojson(py, poly)?)?;
    }
    Ok(result)
}

/// Dissolves polygons based on an attribute.
///
/// Args:
///     features (dict): GeoJSON FeatureCollection
///     attribute (str): Attribute name to dissolve by
///
/// Returns:
///     dict: Dissolved FeatureCollection
///
/// Example:
///     >>> features = {"type": "FeatureCollection", "features": [...]}
///     >>> dissolved = oxigdal.dissolve(features, "state")
#[pyfunction]
pub fn dissolve<'py>(
    py: Python<'py>,
    features: &Bound<'py, PyDict>,
    attribute: &str,
) -> PyResult<Bound<'py, PyDict>> {
    // Get the features list
    let features_obj = features
        .get_item("features")
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Missing 'features' field"))?
        .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("Missing 'features' field"))?;
    let features_list = features_obj
        .downcast::<PyList>()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("'features' must be a list"))?;

    // Group features by attribute value
    let mut groups: std::collections::HashMap<String, Vec<Polygon>> =
        std::collections::HashMap::new();

    for feature_item in features_list.iter() {
        let feature_dict = feature_item
            .downcast::<PyDict>()
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("Feature must be a dict"))?;

        // Get attribute value from properties
        let attr_val = if let Ok(Some(props_obj)) = feature_dict.get_item("properties") {
            if let Ok(props_dict) = props_obj.downcast::<PyDict>() {
                if let Ok(Some(val_obj)) = props_dict.get_item(attribute) {
                    val_obj.str().map(|s| s.to_string()).unwrap_or_default()
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        // Parse geometry
        if let Ok(Some(geom_obj)) = feature_dict.get_item("geometry") {
            if let Ok(geom_dict) = geom_obj.downcast::<PyDict>() {
                if let Ok(polygon) = parse_geojson_polygon(geom_dict) {
                    groups.entry(attr_val).or_default().push(polygon);
                }
            }
        }
    }

    // Union each group
    let result_features = PyList::empty(py);
    for (attr_val, polys) in &groups {
        let merged = algo_vec::cascaded_union(polys).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Dissolve union failed: {}", e))
        })?;

        for poly in &merged {
            let feature = PyDict::new(py);
            feature.set_item("type", "Feature")?;
            feature.set_item("geometry", polygon_to_geojson(py, poly)?)?;
            let props = PyDict::new(py);
            props.set_item(attribute, attr_val.as_str())?;
            feature.set_item("properties", props)?;
            result_features.append(feature)?;
        }
    }

    let result = PyDict::new(py);
    result.set_item("type", "FeatureCollection")?;
    result.set_item("features", result_features)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_distance_points() {
        pyo3::prepare_freethreaded_python();
        Python::with_gil(|py| {
            let g1 = PyDict::new(py);
            g1.set_item("type", "Point").ok();
            if let Ok(cl) = PyList::new(py, [0.0, 0.0]) {
                g1.set_item("coordinates", cl).ok();
            }

            let g2 = PyDict::new(py);
            g2.set_item("type", "Point").ok();
            if let Ok(cl) = PyList::new(py, [3.0, 4.0]) {
                g2.set_item("coordinates", cl).ok();
            }

            let result = distance(&g1, &g2, false);
            assert!(result.is_ok());
            if let Ok(d) = result {
                assert!((d - 5.0).abs() < 1e-10);
            }
        });
    }
}
