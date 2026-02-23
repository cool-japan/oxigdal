//! Filtering operations for vector data
//!
//! Provides filtering functions for GeoJSON and Shapefile features.

use pyo3::prelude::*;
use serde_json::Value as JsonValue;

#[cfg(feature = "shapefile")]
use oxigdal_core::vector::PropertyValue;

/// Filters GeoJSON by layer name (for multi-layer GeoJSON files)
pub(super) fn filter_geojson_by_layer(json: &JsonValue, layer_name: &str) -> PyResult<JsonValue> {
    // Check if this is a multi-layer GeoJSON (object with named layer properties)
    if let JsonValue::Object(map) = json {
        // Check if the specified layer exists
        if let Some(layer_value) = map.get(layer_name) {
            return Ok(layer_value.clone());
        } else if map.contains_key("type")
            && map.get("type").and_then(|v| v.as_str()) == Some("FeatureCollection")
        {
            // This is a standard single-layer FeatureCollection, ignore layer parameter
            return Ok(json.clone());
        } else {
            // Multi-layer format but layer not found
            let available_layers: Vec<&str> = map
                .keys()
                .filter(|k| k.as_str() != "type")
                .map(|k| k.as_str())
                .collect();
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Layer '{}' not found in GeoJSON. Available layers: {}",
                layer_name,
                available_layers.join(", ")
            )));
        }
    }
    // Not a valid GeoJSON structure
    Ok(json.clone())
}

/// Filters GeoJSON features by bbox and/or where clause
pub(super) fn filter_geojson_features(
    json: &JsonValue,
    bbox: Option<&Vec<f64>>,
    where_clause: Option<&str>,
) -> PyResult<JsonValue> {
    // Only filter if this is a FeatureCollection
    if let JsonValue::Object(map) = json {
        if map.get("type").and_then(|v| v.as_str()) == Some("FeatureCollection") {
            if let Some(JsonValue::Array(features)) = map.get("features") {
                // Apply filters
                let filtered_features: Vec<JsonValue> = features
                    .iter()
                    .filter(|feature| {
                        // Apply bbox filter
                        if let Some(b) = bbox {
                            if !feature_intersects_bbox(feature, b) {
                                return false;
                            }
                        }
                        // Apply where clause filter
                        if let Some(where_str) = where_clause {
                            if !apply_geojson_where_filter(feature, where_str) {
                                return false;
                            }
                        }
                        true
                    })
                    .cloned()
                    .collect();

                // Rebuild FeatureCollection with filtered features
                let mut result_map = serde_json::Map::new();
                result_map.insert(
                    "type".to_string(),
                    JsonValue::String("FeatureCollection".to_string()),
                );
                result_map.insert("features".to_string(), JsonValue::Array(filtered_features));

                // Copy other top-level properties (like crs, bbox, etc.)
                for (key, value) in map {
                    if key != "type" && key != "features" {
                        result_map.insert(key.clone(), value.clone());
                    }
                }

                return Ok(JsonValue::Object(result_map));
            }
        }
    }

    // Not a FeatureCollection or no features, return as-is
    Ok(json.clone())
}

/// Checks if a GeoJSON feature's geometry intersects with a bounding box
fn feature_intersects_bbox(feature: &JsonValue, bbox: &[f64]) -> bool {
    if bbox.len() != 4 {
        return true; // Invalid bbox, don't filter
    }

    let (minx, miny, maxx, maxy) = (bbox[0], bbox[1], bbox[2], bbox[3]);

    // Extract geometry
    let geometry = match feature.get("geometry") {
        Some(geom) if !geom.is_null() => geom,
        _ => return false, // No geometry, exclude
    };

    // Get geometry bounds
    if let Some((gminx, gminy, gmaxx, gmaxy)) = get_geometry_bounds(geometry) {
        // Check intersection: geometries intersect if they don't NOT intersect
        !(gmaxx < minx || gminx > maxx || gmaxy < miny || gminy > maxy)
    } else {
        false // Couldn't determine bounds, exclude
    }
}

/// Extracts bounding box from a GeoJSON geometry
fn get_geometry_bounds(geometry: &JsonValue) -> Option<(f64, f64, f64, f64)> {
    let geom_type = geometry.get("type")?.as_str()?;
    let coords = geometry.get("coordinates")?;

    match geom_type {
        "Point" => {
            let arr = coords.as_array()?;
            if arr.len() >= 2 {
                let x = arr[0].as_f64()?;
                let y = arr[1].as_f64()?;
                Some((x, y, x, y))
            } else {
                None
            }
        }
        "MultiPoint" | "LineString" => {
            let arr = coords.as_array()?;
            compute_coords_bounds(arr)
        }
        "MultiLineString" | "Polygon" => {
            let rings = coords.as_array()?;
            let mut bounds: Option<(f64, f64, f64, f64)> = None;
            for ring in rings {
                if let Some(ring_arr) = ring.as_array() {
                    if let Some(ring_bounds) = compute_coords_bounds(ring_arr) {
                        bounds = Some(merge_bounds(bounds, ring_bounds));
                    }
                }
            }
            bounds
        }
        "MultiPolygon" => {
            let polygons = coords.as_array()?;
            let mut bounds: Option<(f64, f64, f64, f64)> = None;
            for polygon in polygons {
                if let Some(rings) = polygon.as_array() {
                    for ring in rings {
                        if let Some(ring_arr) = ring.as_array() {
                            if let Some(ring_bounds) = compute_coords_bounds(ring_arr) {
                                bounds = Some(merge_bounds(bounds, ring_bounds));
                            }
                        }
                    }
                }
            }
            bounds
        }
        "GeometryCollection" => {
            let geometries = geometry.get("geometries")?.as_array()?;
            let mut bounds: Option<(f64, f64, f64, f64)> = None;
            for geom in geometries {
                if let Some(geom_bounds) = get_geometry_bounds(geom) {
                    bounds = Some(merge_bounds(bounds, geom_bounds));
                }
            }
            bounds
        }
        _ => None,
    }
}

/// Computes bounds from an array of coordinates
fn compute_coords_bounds(coords: &[JsonValue]) -> Option<(f64, f64, f64, f64)> {
    let mut minx = f64::INFINITY;
    let mut miny = f64::INFINITY;
    let mut maxx = f64::NEG_INFINITY;
    let mut maxy = f64::NEG_INFINITY;

    for coord in coords {
        if let Some(arr) = coord.as_array() {
            if arr.len() >= 2 {
                if let (Some(x), Some(y)) = (arr[0].as_f64(), arr[1].as_f64()) {
                    minx = minx.min(x);
                    miny = miny.min(y);
                    maxx = maxx.max(x);
                    maxy = maxy.max(y);
                }
            }
        }
    }

    if minx.is_finite() && miny.is_finite() && maxx.is_finite() && maxy.is_finite() {
        Some((minx, miny, maxx, maxy))
    } else {
        None
    }
}

/// Merges two bounding boxes
fn merge_bounds(
    bounds1: Option<(f64, f64, f64, f64)>,
    bounds2: (f64, f64, f64, f64),
) -> (f64, f64, f64, f64) {
    match bounds1 {
        Some((minx1, miny1, maxx1, maxy1)) => (
            minx1.min(bounds2.0),
            miny1.min(bounds2.1),
            maxx1.max(bounds2.2),
            maxy1.max(bounds2.3),
        ),
        None => bounds2,
    }
}

/// Applies a WHERE clause filter to a GeoJSON feature
fn apply_geojson_where_filter(feature: &JsonValue, where_clause: &str) -> bool {
    // Extract properties
    let properties = match feature.get("properties") {
        Some(props) if !props.is_null() => props,
        _ => return true, // No properties, include by default
    };

    // Parse and apply where clause
    let trimmed = where_clause.trim();

    // Support common operators: =, !=, >, <, >=, <=
    let operators = [">=", "<=", "!=", "=", ">", "<"];

    for op in &operators {
        if let Some((field, value)) = trimmed.split_once(op) {
            let field = field.trim();
            let value = value.trim().trim_matches('\'').trim_matches('"');

            if let Some(prop_value) = properties.get(field) {
                return match *op {
                    "=" => compare_values_equal(prop_value, value),
                    "!=" => !compare_values_equal(prop_value, value),
                    ">" => compare_values_greater(prop_value, value),
                    "<" => compare_values_less(prop_value, value),
                    ">=" => {
                        compare_values_greater(prop_value, value)
                            || compare_values_equal(prop_value, value)
                    }
                    "<=" => {
                        compare_values_less(prop_value, value)
                            || compare_values_equal(prop_value, value)
                    }
                    _ => true,
                };
            } else {
                return false; // Field not found, exclude
            }
        }
    }

    // If we can't parse the where clause, include the feature
    true
}

/// Compares JSON value with string for equality
fn compare_values_equal(json_value: &JsonValue, str_value: &str) -> bool {
    match json_value {
        JsonValue::String(s) => s == str_value,
        JsonValue::Number(n) => {
            if let Ok(i) = str_value.parse::<i64>() {
                n.as_i64() == Some(i)
            } else if let Ok(f) = str_value.parse::<f64>() {
                if let Some(nf) = n.as_f64() {
                    (nf - f).abs() < f64::EPSILON
                } else {
                    false
                }
            } else {
                false
            }
        }
        JsonValue::Bool(b) => {
            if let Ok(val) = str_value.parse::<bool>() {
                *b == val
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Compares JSON value > string value
fn compare_values_greater(json_value: &JsonValue, str_value: &str) -> bool {
    match json_value {
        JsonValue::Number(n) => {
            if let Ok(i) = str_value.parse::<i64>() {
                n.as_i64().is_some_and(|v| v > i)
            } else if let Ok(f) = str_value.parse::<f64>() {
                n.as_f64().is_some_and(|v| v > f)
            } else {
                false
            }
        }
        JsonValue::String(s) => s.as_str() > str_value,
        _ => false,
    }
}

/// Compares JSON value < string value
fn compare_values_less(json_value: &JsonValue, str_value: &str) -> bool {
    match json_value {
        JsonValue::Number(n) => {
            if let Ok(i) = str_value.parse::<i64>() {
                n.as_i64().is_some_and(|v| v < i)
            } else if let Ok(f) = str_value.parse::<f64>() {
                n.as_f64().is_some_and(|v| v < f)
            } else {
                false
            }
        }
        JsonValue::String(s) => s.as_str() < str_value,
        _ => false,
    }
}

#[cfg(feature = "shapefile")]
/// Applies a simple WHERE clause filter to a feature
/// This is a basic implementation that supports simple equality checks
pub(super) fn apply_where_filter(
    feature: &oxigdal_shapefile::ShapefileFeature,
    where_clause: &str,
) -> bool {
    // Basic parsing: "field = value" or "field > value" etc.
    // For now, we'll implement a simple exact match: "field = 'value'"

    // Remove whitespace and parse
    let trimmed = where_clause.trim();

    // Try to split by common operators
    if let Some((field, value)) = trimmed.split_once('=') {
        let field = field.trim();
        let value = value.trim().trim_matches('\'').trim_matches('"');

        if let Some(prop_value) = feature.attributes.get(field) {
            match prop_value {
                PropertyValue::String(s) => return s == value,
                PropertyValue::Integer(i) => {
                    if let Ok(val) = value.parse::<i64>() {
                        return *i == val;
                    }
                }
                PropertyValue::Float(f) => {
                    if let Ok(val) = value.parse::<f64>() {
                        return (*f - val).abs() < f64::EPSILON;
                    }
                }
                PropertyValue::Bool(b) => {
                    if let Ok(val) = value.parse::<bool>() {
                        return *b == val;
                    }
                }
                _ => {}
            }
        }
        return false;
    }

    // If we can't parse the where clause, include the feature
    true
}
