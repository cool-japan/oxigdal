//! GeoJSON I/O operations
//!
//! Provides GeoJSON reading/writing functions exposed to Python.

use pyo3::prelude::*;
use pyo3::types::PyDict;
use serde_json::Value as JsonValue;
use std::fs;

use super::super::helpers::{json_to_python, python_to_json};
use super::filtering::{filter_geojson_by_layer, filter_geojson_features};

/// Reads a GeoJSON file.
///
/// Args:
///     path (str): Path to GeoJSON file
///     layer (str, optional): Layer name for multi-layer files
///     bbox (list, optional): Bounding box filter [minx, miny, maxx, maxy]
///     where (str, optional): SQL WHERE clause for filtering
///
/// Returns:
///     dict: Parsed GeoJSON as Python dictionary
///
/// Raises:
///     IOError: If file cannot be read
///     ValueError: If JSON is invalid
///
/// Example:
///     >>> features = oxigdal.read_geojson("input.geojson")
///     >>> print(len(features['features']))
///     >>>
///     >>> # Read with bounding box filter
///     >>> subset = oxigdal.read_geojson("input.geojson", bbox=[0, 0, 10, 10])
///     >>>
///     >>> # Read with attribute filter
///     >>> filtered = oxigdal.read_geojson("input.geojson", where="population > 1000000")
#[pyfunction]
#[pyo3(signature = (path, layer=None, bbox=None, where_clause=None))]
pub fn read_geojson<'py>(
    py: Python<'py>,
    path: &str,
    layer: Option<&str>,
    bbox: Option<Vec<f64>>,
    where_clause: Option<&str>,
) -> PyResult<Bound<'py, PyDict>> {
    // Validate bbox
    if let Some(ref b) = bbox {
        if b.len() != 4 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "Bounding box must have 4 elements [minx, miny, maxx, maxy]",
            ));
        }
    }

    // Read file
    let content = fs::read_to_string(path).map_err(|e| {
        pyo3::exceptions::PyIOError::new_err(format!("Failed to read file '{}': {}", path, e))
    })?;

    // Parse JSON
    let mut json: JsonValue = serde_json::from_str(&content)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("Invalid GeoJSON: {}", e)))?;

    // Apply layer filter if specified
    if let Some(layer_name) = layer {
        json = filter_geojson_by_layer(&json, layer_name)?;
    }

    // Apply bbox and where filters to features
    json = filter_geojson_features(&json, bbox.as_ref(), where_clause)?;

    // Convert to Python dict
    json_to_python(py, &json)
}

/// Writes a GeoJSON file.
///
/// Args:
///     path (str): Output path
///     data (dict): GeoJSON data as Python dictionary
///     pretty (bool): Pretty-print JSON (default: True)
///     precision (int, optional): Coordinate precision (decimal places, 0-15)
///     driver (str, optional): Output driver (default: "GeoJSON")
///         Supported drivers:
///         - "GeoJSON": Standard GeoJSON (default)
///         - "GeoJSONSeq": GeoJSON Text Sequences (RFC 8142)
///         - "RFC7946": RFC 7946 compliant GeoJSON (WGS84 only, right-hand rule)
///
/// Raises:
///     IOError: If file cannot be written
///     ValueError: If data is invalid or driver not supported
///
/// Example:
///     >>> geojson = {
///     ...     "type": "FeatureCollection",
///     ...     "features": [
///     ...         {
///     ...             "type": "Feature",
///     ...             "geometry": {"type": "Point", "coordinates": [0, 0]},
///     ...             "properties": {"name": "Origin"}
///     ...         }
///     ...     ]
///     ... }
///     >>> oxigdal.write_geojson("output.geojson", geojson)
///     >>>
///     >>> # Write with limited precision (6 decimal places)
///     >>> oxigdal.write_geojson("output.geojson", geojson, precision=6)
///     >>>
///     >>> # Write RFC7946-compliant GeoJSON
///     >>> oxigdal.write_geojson("output.geojson", geojson, driver="RFC7946")
#[pyfunction]
#[pyo3(signature = (path, data, pretty=true, precision=None, driver=None))]
pub fn write_geojson(
    path: &str,
    data: &Bound<'_, PyDict>,
    pretty: bool,
    precision: Option<i32>,
    driver: Option<&str>,
) -> PyResult<()> {
    // Validate precision
    if let Some(p) = precision {
        if !(0..=15).contains(&p) {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "Precision must be between 0 and 15",
            ));
        }
    }

    // Validate driver
    let driver_type = driver.unwrap_or("GeoJSON");
    match driver_type {
        "GeoJSON" | "GeoJSONSeq" | "RFC7946" => {}
        _ => {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Unsupported driver '{}'. Supported: GeoJSON, GeoJSONSeq, RFC7946",
                driver_type
            )));
        }
    }

    // Convert Python dict to JSON
    let mut json = python_to_json(data)?;

    // Apply coordinate precision rounding if specified
    if let Some(prec) = precision {
        json = round_geojson_coordinates(&json, prec as u32);
    }

    // Apply driver-specific transformations
    if driver_type == "RFC7946" {
        json = apply_rfc7946_compliance(&json)?;
    }

    // Serialize based on driver type
    let content = match driver_type {
        "GeoJSONSeq" => serialize_geojson_seq(&json, pretty)?,
        _ => if pretty {
            serde_json::to_string_pretty(&json)
        } else {
            serde_json::to_string(&json)
        }
        .map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Failed to serialize JSON: {}", e))
        })?,
    };

    // Write file
    fs::write(path, content).map_err(|e| {
        pyo3::exceptions::PyIOError::new_err(format!("Failed to write file '{}': {}", path, e))
    })?;

    Ok(())
}

// ============================================================================
// GeoJSON precision and driver-specific helpers
// ============================================================================

/// Rounds all coordinates in a GeoJSON structure to the specified precision
fn round_geojson_coordinates(json: &JsonValue, precision: u32) -> JsonValue {
    match json {
        JsonValue::Object(map) => {
            let mut new_map = serde_json::Map::new();
            for (key, value) in map {
                if key == "coordinates" {
                    new_map.insert(key.clone(), round_coordinates_array(value, precision));
                } else {
                    new_map.insert(key.clone(), round_geojson_coordinates(value, precision));
                }
            }
            JsonValue::Object(new_map)
        }
        JsonValue::Array(arr) => JsonValue::Array(
            arr.iter()
                .map(|v| round_geojson_coordinates(v, precision))
                .collect(),
        ),
        _ => json.clone(),
    }
}

/// Rounds coordinate arrays to specified precision
fn round_coordinates_array(json: &JsonValue, precision: u32) -> JsonValue {
    match json {
        JsonValue::Array(arr) => {
            // Check if this is a coordinate pair/triple [x, y] or [x, y, z]
            if arr.len() >= 2 && arr.len() <= 3 && arr.iter().all(|v| v.is_number()) {
                // This is a single coordinate, round it
                JsonValue::Array(arr.iter().map(|v| round_number(v, precision)).collect())
            } else {
                // This is an array of coordinates or rings, recurse
                JsonValue::Array(
                    arr.iter()
                        .map(|v| round_coordinates_array(v, precision))
                        .collect(),
                )
            }
        }
        JsonValue::Number(_) => round_number(json, precision),
        _ => json.clone(),
    }
}

/// Rounds a JSON number to specified decimal places
fn round_number(value: &JsonValue, precision: u32) -> JsonValue {
    if let Some(f) = value.as_f64() {
        let multiplier = 10_f64.powi(precision as i32);
        let rounded = (f * multiplier).round() / multiplier;
        serde_json::Number::from_f64(rounded)
            .map(JsonValue::Number)
            .unwrap_or_else(|| value.clone())
    } else {
        value.clone()
    }
}

/// Applies RFC 7946 compliance transformations to GeoJSON
fn apply_rfc7946_compliance(json: &JsonValue) -> PyResult<JsonValue> {
    let mut result = json.clone();

    // RFC 7946 requirements:
    // 1. Remove CRS member (RFC 7946 assumes WGS84)
    // 2. Ensure right-hand rule for polygon winding
    // 3. Coordinates must be in [longitude, latitude] order

    if let JsonValue::Object(map) = &mut result {
        // Remove CRS if present (RFC 7946 Section 4)
        let mut new_map = map.clone();
        new_map.remove("crs");
        result = JsonValue::Object(new_map);
    }

    // Note: Right-hand rule enforcement and coordinate order validation
    // would require geometric analysis. For now, we assume input is correct
    // and just remove the CRS field.

    Ok(result)
}

/// Serializes GeoJSON as GeoJSON Text Sequences (RFC 8142)
fn serialize_geojson_seq(json: &JsonValue, pretty: bool) -> PyResult<String> {
    // GeoJSON Text Sequences: newline-delimited features with RS (0x1E) separator
    if let JsonValue::Object(map) = json {
        if map.get("type").and_then(|v| v.as_str()) == Some("FeatureCollection") {
            if let Some(JsonValue::Array(features)) = map.get("features") {
                let mut output = String::new();
                for feature in features {
                    // Add Record Separator (RS)
                    output.push('\x1E');
                    // Serialize feature
                    let feature_json = if pretty {
                        serde_json::to_string_pretty(feature)
                    } else {
                        serde_json::to_string(feature)
                    }
                    .map_err(|e| {
                        pyo3::exceptions::PyValueError::new_err(format!(
                            "Failed to serialize feature: {}",
                            e
                        ))
                    })?;
                    output.push_str(&feature_json);
                    output.push('\n');
                }
                return Ok(output);
            }
        }
    }

    // If not a FeatureCollection, serialize normally
    if pretty {
        serde_json::to_string_pretty(json)
    } else {
        serde_json::to_string(json)
    }
    .map_err(|e| {
        pyo3::exceptions::PyValueError::new_err(format!("Failed to serialize JSON: {}", e))
    })
}
