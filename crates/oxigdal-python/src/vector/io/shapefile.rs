//! Shapefile I/O operations
//!
//! Provides Shapefile reading/writing functions exposed to Python.

use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList};
use std::collections::HashMap;

use super::super::helpers::coords_to_pylist;
use super::filtering::apply_where_filter;

#[cfg(feature = "shapefile")]
use oxigdal_core::vector::{Geometry, PropertyValue};
#[cfg(feature = "shapefile")]
use oxigdal_shapefile::{
    FieldType, ShapeType, ShapefileReader, ShapefileSchemaBuilder, ShapefileWriter,
};

/// Reads a Shapefile.
///
/// Args:
///     path (str): Path to Shapefile (.shp)
///     encoding (str, optional): Character encoding for DBF (default: "utf-8")
///     bbox (list, optional): Bounding box filter [minx, miny, maxx, maxy]
///     where (str, optional): SQL WHERE clause for filtering
///
/// Returns:
///     dict: Features as GeoJSON-like dictionary
///
/// Raises:
///     IOError: If file cannot be read
///     ValueError: If Shapefile is invalid
///
/// Example:
///     >>> features = oxigdal.read_shapefile("input.shp")
///     >>> print(f"Read {len(features['features'])} features")
///     >>>
///     >>> # Read with encoding
///     >>> features = oxigdal.read_shapefile("input.shp", encoding="latin1")
#[pyfunction]
#[pyo3(signature = (path, encoding="utf-8", bbox=None, where_clause=None))]
pub fn read_shapefile<'py>(
    py: Python<'py>,
    path: &str,
    encoding: &str,
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

    #[cfg(not(feature = "shapefile"))]
    {
        let _ = (py, path, encoding, where_clause);
        Err(pyo3::exceptions::PyNotImplementedError::new_err(
            "Shapefile support not enabled. Recompile with --features shapefile",
        ))
    }

    #[cfg(feature = "shapefile")]
    {
        // Open shapefile
        let reader = ShapefileReader::open(path).map_err(|e| {
            pyo3::exceptions::PyIOError::new_err(format!(
                "Failed to open Shapefile '{}': {}",
                path, e
            ))
        })?;

        // Read all features
        let mut features = reader.read_features().map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Failed to read Shapefile: {}", e))
        })?;

        // Apply bbox filter if provided
        if let Some(b) = bbox {
            features.retain(|feature| {
                if let Some(ref geom) = feature.geometry {
                    if let Some((x_min, y_min, x_max, y_max)) = geom.bounds() {
                        // Check if geometry bounds intersect with filter bbox
                        return !(x_max < b[0] || x_min > b[2] || y_max < b[1] || y_min > b[3]);
                    }
                }
                false
            });
        }

        // Apply where clause filter if provided
        if let Some(where_str) = where_clause {
            features.retain(|feature| apply_where_filter(feature, where_str));
        }

        // Convert to GeoJSON-like dict
        let result = PyDict::new(py);
        result.set_item("type", "FeatureCollection")?;

        let features_list = PyList::empty(py);
        for feature in &features {
            let feature_dict = shapefile_feature_to_geojson(py, feature)?;
            features_list.append(feature_dict)?;
        }

        result.set_item("features", features_list)?;

        // Note: encoding parameter is for future use with DBF character encoding
        let _ = encoding;

        Ok(result)
    }
}

/// Writes a Shapefile.
///
/// Args:
///     path (str): Output path (.shp)
///     data (dict): GeoJSON-like feature collection
///     encoding (str, optional): Character encoding for DBF (default: "utf-8")
///     driver (str, optional): Output driver (default: "ESRI Shapefile")
///
/// Raises:
///     IOError: If file cannot be written
///     ValueError: If data is invalid
///
/// Example:
///     >>> features = {
///     ...     "type": "FeatureCollection",
///     ...     "features": [...]
///     ... }
///     >>> oxigdal.write_shapefile("output.shp", features)
#[pyfunction]
#[pyo3(signature = (path, data, encoding="utf-8", driver=None))]
pub fn write_shapefile(
    path: &str,
    data: &Bound<'_, PyDict>,
    encoding: &str,
    driver: Option<&str>,
) -> PyResult<()> {
    #[cfg(not(feature = "shapefile"))]
    {
        let _ = (path, data, encoding, driver);
        Err(pyo3::exceptions::PyNotImplementedError::new_err(
            "Shapefile support not enabled. Recompile with --features shapefile",
        ))
    }

    #[cfg(feature = "shapefile")]
    {
        // Validate that data is a FeatureCollection
        let data_type: String = data
            .get_item("type")
            .ok()
            .flatten()
            .and_then(|v| v.extract().ok())
            .unwrap_or_default();

        if data_type != "FeatureCollection" {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "Data must be a FeatureCollection",
            ));
        }

        // Extract features
        let features_obj = data
            .get_item("features")
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("Missing 'features' field"))?
            .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("Missing 'features' field"))?;

        let features_list = features_obj
            .downcast::<PyList>()
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("'features' must be a list"))?;

        if features_list.is_empty() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "Cannot write empty feature collection",
            ));
        }

        // Convert Python features to ShapefileFeatures
        let mut shapefile_features = Vec::new();
        let mut shape_type: Option<ShapeType> = None;

        for (idx, feature_item) in features_list.iter().enumerate() {
            let feature_dict = feature_item
                .downcast::<PyDict>()
                .map_err(|_| pyo3::exceptions::PyValueError::new_err("Feature must be a dict"))?;

            let shapefile_feature = geojson_to_shapefile_feature(feature_dict, idx as i32 + 1)?;

            // Infer shape type from first geometry
            if shape_type.is_none() {
                if let Some(ref geom) = shapefile_feature.geometry {
                    shape_type = Some(geometry_to_shape_type(geom)?);
                }
            }

            shapefile_features.push(shapefile_feature);
        }

        let shape_type = shape_type
            .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("No valid geometries found"))?;

        // Build schema from first feature's attributes
        let schema = build_schema_from_features(&shapefile_features)?;

        // Create writer and write features
        let mut writer = ShapefileWriter::new(path, shape_type, schema).map_err(|e| {
            pyo3::exceptions::PyIOError::new_err(format!(
                "Failed to create Shapefile writer: {}",
                e
            ))
        })?;

        writer.write_features(&shapefile_features).map_err(|e| {
            pyo3::exceptions::PyIOError::new_err(format!("Failed to write Shapefile: {}", e))
        })?;

        // Note: encoding and driver parameters are for future use
        let _ = (encoding, driver);

        Ok(())
    }
}

// ============================================================================
// Helper functions for Shapefile conversion
// ============================================================================

#[cfg(feature = "shapefile")]
/// Converts a ShapefileFeature to a GeoJSON-like Python dict
fn shapefile_feature_to_geojson<'py>(
    py: Python<'py>,
    feature: &oxigdal_shapefile::ShapefileFeature,
) -> PyResult<Bound<'py, PyDict>> {
    let feature_dict = PyDict::new(py);
    feature_dict.set_item("type", "Feature")?;

    // Convert geometry
    if let Some(ref geom) = feature.geometry {
        let geom_dict = geometry_to_geojson(py, geom)?;
        feature_dict.set_item("geometry", geom_dict)?;
    } else {
        feature_dict.set_item("geometry", py.None())?;
    }

    // Convert properties
    let props_dict = PyDict::new(py);
    for (key, value) in &feature.attributes {
        let py_value = property_value_to_python(py, value)?;
        props_dict.set_item(key, py_value)?;
    }
    feature_dict.set_item("properties", props_dict)?;

    Ok(feature_dict)
}

#[cfg(feature = "shapefile")]
/// Converts an OxiGDAL Geometry to a GeoJSON-like Python dict
fn geometry_to_geojson<'py>(py: Python<'py>, geometry: &Geometry) -> PyResult<Bound<'py, PyDict>> {
    let geom_dict = PyDict::new(py);

    match geometry {
        Geometry::Point(point) => {
            geom_dict.set_item("type", "Point")?;
            let coords = PyList::new(py, [point.coord.x, point.coord.y])
                .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("{}", e)))?;
            geom_dict.set_item("coordinates", coords)?;
        }
        Geometry::LineString(linestring) => {
            geom_dict.set_item("type", "LineString")?;
            let coords = coords_to_pylist(py, &linestring.coords)?;
            geom_dict.set_item("coordinates", coords)?;
        }
        Geometry::Polygon(polygon) => {
            geom_dict.set_item("type", "Polygon")?;
            let rings = PyList::empty(py);

            // Exterior ring
            let ext = coords_to_pylist(py, &polygon.exterior.coords)?;
            rings.append(ext)?;

            // Interior rings
            for interior in &polygon.interiors {
                let hole = coords_to_pylist(py, &interior.coords)?;
                rings.append(hole)?;
            }

            geom_dict.set_item("coordinates", rings)?;
        }
        Geometry::MultiPoint(multipoint) => {
            geom_dict.set_item("type", "MultiPoint")?;
            let points = PyList::empty(py);
            for point in &multipoint.points {
                let coord = PyList::new(py, [point.coord.x, point.coord.y])
                    .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("{}", e)))?;
                points.append(coord)?;
            }
            geom_dict.set_item("coordinates", points)?;
        }
        Geometry::MultiLineString(multilinestring) => {
            geom_dict.set_item("type", "MultiLineString")?;
            let lines = PyList::empty(py);
            for linestring in &multilinestring.line_strings {
                let coords = coords_to_pylist(py, &linestring.coords)?;
                lines.append(coords)?;
            }
            geom_dict.set_item("coordinates", lines)?;
        }
        Geometry::MultiPolygon(multipolygon) => {
            geom_dict.set_item("type", "MultiPolygon")?;
            let polygons = PyList::empty(py);
            for polygon in &multipolygon.polygons {
                let rings = PyList::empty(py);

                // Exterior ring
                let ext = coords_to_pylist(py, &polygon.exterior.coords)?;
                rings.append(ext)?;

                // Interior rings
                for interior in &polygon.interiors {
                    let hole = coords_to_pylist(py, &interior.coords)?;
                    rings.append(hole)?;
                }

                polygons.append(rings)?;
            }
            geom_dict.set_item("coordinates", polygons)?;
        }
        Geometry::GeometryCollection(_) => {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "GeometryCollection not supported in Shapefile format",
            ));
        }
    }

    Ok(geom_dict)
}

#[cfg(feature = "shapefile")]
/// Converts a PropertyValue to a Python object
fn property_value_to_python<'py>(
    py: Python<'py>,
    value: &PropertyValue,
) -> PyResult<Bound<'py, PyAny>> {
    match value {
        PropertyValue::Null => Ok(py.None().into_bound(py)),
        PropertyValue::Bool(b) => Ok(b
            .into_pyobject(py)
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("Bool conversion: {}", e))
            })?
            .to_owned()
            .into_any()),
        PropertyValue::Integer(i) => Ok(i
            .into_pyobject(py)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("Int conversion: {}", e)))?
            .into_any()),
        PropertyValue::UInteger(u) => Ok(u
            .into_pyobject(py)
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("UInt conversion: {}", e))
            })?
            .into_any()),
        PropertyValue::Float(f) => Ok(f
            .into_pyobject(py)
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("Float conversion: {}", e))
            })?
            .into_any()),
        PropertyValue::String(s) => Ok(s
            .into_pyobject(py)
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("String conversion: {}", e))
            })?
            .into_any()),
        PropertyValue::Array(_) | PropertyValue::Object(_) => {
            // These are not directly supported in Shapefile, return null
            Ok(py.None().into_bound(py))
        }
    }
}

#[cfg(feature = "shapefile")]
/// Converts a GeoJSON-like Python feature dict to a ShapefileFeature
fn geojson_to_shapefile_feature(
    feature_dict: &Bound<'_, PyDict>,
    record_number: i32,
) -> PyResult<oxigdal_shapefile::ShapefileFeature> {
    // Extract geometry
    let geometry = if let Some(geom_obj) = feature_dict.get_item("geometry").ok().flatten() {
        if !geom_obj.is_none() {
            let geom_dict = geom_obj
                .downcast::<PyDict>()
                .map_err(|_| pyo3::exceptions::PyValueError::new_err("Geometry must be a dict"))?;
            Some(geojson_to_geometry(geom_dict)?)
        } else {
            None
        }
    } else {
        None
    };

    // Extract properties
    let mut attributes = HashMap::new();
    if let Some(props_obj) = feature_dict.get_item("properties").ok().flatten() {
        if !props_obj.is_none() {
            let props_dict = props_obj.downcast::<PyDict>().map_err(|_| {
                pyo3::exceptions::PyValueError::new_err("Properties must be a dict")
            })?;

            for (key, value) in props_dict {
                let key_str: String = key.extract()?;
                let prop_value = python_to_property_value(&value)?;
                attributes.insert(key_str, prop_value);
            }
        }
    }

    Ok(oxigdal_shapefile::ShapefileFeature::new(
        record_number,
        geometry,
        attributes,
    ))
}

#[cfg(feature = "shapefile")]
/// Converts a GeoJSON-like Python geometry dict to an OxiGDAL Geometry
fn geojson_to_geometry(geom_dict: &Bound<'_, PyDict>) -> PyResult<Geometry> {
    use oxigdal_core::vector::{
        LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon,
    };

    let geom_type: String = geom_dict
        .get_item("type")
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Missing geometry 'type' field"))?
        .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("Missing geometry 'type' field"))?
        .extract()?;

    let coords_obj = geom_dict
        .get_item("coordinates")
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Missing 'coordinates' field"))?
        .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("Missing 'coordinates' field"))?;

    match geom_type.as_str() {
        "Point" => {
            let coords_list = coords_obj.downcast::<PyList>().map_err(|_| {
                pyo3::exceptions::PyValueError::new_err("Point coordinates must be a list")
            })?;
            let x: f64 = coords_list.get_item(0)?.extract()?;
            let y: f64 = coords_list.get_item(1)?.extract()?;
            Ok(Geometry::Point(Point::new(x, y)))
        }
        "LineString" => {
            let coords_list = coords_obj.downcast::<PyList>().map_err(|_| {
                pyo3::exceptions::PyValueError::new_err("LineString coordinates must be a list")
            })?;
            let coords = extract_coords_from_list(coords_list)?;
            let linestring = LineString::new(coords).map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("Invalid LineString: {}", e))
            })?;
            Ok(Geometry::LineString(linestring))
        }
        "Polygon" => {
            let rings_list = coords_obj.downcast::<PyList>().map_err(|_| {
                pyo3::exceptions::PyValueError::new_err(
                    "Polygon coordinates must be a list of rings",
                )
            })?;

            if rings_list.is_empty() {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "Polygon must have at least one ring",
                ));
            }

            // Exterior ring
            let ext_item = rings_list.get_item(0)?;
            let ext_list = ext_item
                .downcast::<PyList>()
                .map_err(|_| pyo3::exceptions::PyValueError::new_err("Ring must be a list"))?;
            let ext_coords = extract_coords_from_list(ext_list)?;
            let exterior = LineString::new(ext_coords).map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("Invalid exterior ring: {}", e))
            })?;

            // Interior rings
            let mut interiors = Vec::new();
            for i in 1..rings_list.len() {
                let int_item = rings_list.get_item(i)?;
                let int_list = int_item
                    .downcast::<PyList>()
                    .map_err(|_| pyo3::exceptions::PyValueError::new_err("Ring must be a list"))?;
                let int_coords = extract_coords_from_list(int_list)?;
                let interior = LineString::new(int_coords).map_err(|e| {
                    pyo3::exceptions::PyValueError::new_err(format!("Invalid interior ring: {}", e))
                })?;
                interiors.push(interior);
            }

            let polygon = Polygon::new(exterior, interiors).map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("Invalid Polygon: {}", e))
            })?;
            Ok(Geometry::Polygon(polygon))
        }
        "MultiPoint" => {
            let points_list = coords_obj.downcast::<PyList>().map_err(|_| {
                pyo3::exceptions::PyValueError::new_err("MultiPoint coordinates must be a list")
            })?;

            let mut points = Vec::new();
            for item in points_list.iter() {
                let coord_list = item.downcast::<PyList>().map_err(|_| {
                    pyo3::exceptions::PyValueError::new_err("Point coordinate must be a list")
                })?;
                let x: f64 = coord_list.get_item(0)?.extract()?;
                let y: f64 = coord_list.get_item(1)?.extract()?;
                points.push(Point::new(x, y));
            }

            Ok(Geometry::MultiPoint(MultiPoint::new(points)))
        }
        "MultiLineString" => {
            let lines_list = coords_obj.downcast::<PyList>().map_err(|_| {
                pyo3::exceptions::PyValueError::new_err(
                    "MultiLineString coordinates must be a list",
                )
            })?;

            let mut linestrings = Vec::new();
            for item in lines_list.iter() {
                let coords_list = item.downcast::<PyList>().map_err(|_| {
                    pyo3::exceptions::PyValueError::new_err("LineString coordinates must be a list")
                })?;
                let coords = extract_coords_from_list(coords_list)?;
                let linestring = LineString::new(coords).map_err(|e| {
                    pyo3::exceptions::PyValueError::new_err(format!("Invalid LineString: {}", e))
                })?;
                linestrings.push(linestring);
            }

            Ok(Geometry::MultiLineString(MultiLineString::new(linestrings)))
        }
        "MultiPolygon" => {
            let polys_list = coords_obj.downcast::<PyList>().map_err(|_| {
                pyo3::exceptions::PyValueError::new_err("MultiPolygon coordinates must be a list")
            })?;

            let mut polygons = Vec::new();
            for poly_item in polys_list.iter() {
                let rings_list = poly_item.downcast::<PyList>().map_err(|_| {
                    pyo3::exceptions::PyValueError::new_err("Polygon rings must be a list")
                })?;

                if rings_list.is_empty() {
                    continue;
                }

                // Exterior ring
                let ext_item = rings_list.get_item(0)?;
                let ext_list = ext_item
                    .downcast::<PyList>()
                    .map_err(|_| pyo3::exceptions::PyValueError::new_err("Ring must be a list"))?;
                let ext_coords = extract_coords_from_list(ext_list)?;
                let exterior = LineString::new(ext_coords).map_err(|e| {
                    pyo3::exceptions::PyValueError::new_err(format!("Invalid exterior ring: {}", e))
                })?;

                // Interior rings
                let mut interiors = Vec::new();
                for i in 1..rings_list.len() {
                    let int_item = rings_list.get_item(i)?;
                    let int_list = int_item.downcast::<PyList>().map_err(|_| {
                        pyo3::exceptions::PyValueError::new_err("Ring must be a list")
                    })?;
                    let int_coords = extract_coords_from_list(int_list)?;
                    let interior = LineString::new(int_coords).map_err(|e| {
                        pyo3::exceptions::PyValueError::new_err(format!(
                            "Invalid interior ring: {}",
                            e
                        ))
                    })?;
                    interiors.push(interior);
                }

                let polygon = Polygon::new(exterior, interiors).map_err(|e| {
                    pyo3::exceptions::PyValueError::new_err(format!("Invalid Polygon: {}", e))
                })?;
                polygons.push(polygon);
            }

            Ok(Geometry::MultiPolygon(MultiPolygon::new(polygons)))
        }
        _ => Err(pyo3::exceptions::PyValueError::new_err(format!(
            "Unsupported geometry type: {}",
            geom_type
        ))),
    }
}

#[cfg(feature = "shapefile")]
/// Extracts coordinates from a Python list
fn extract_coords_from_list(
    list: &Bound<'_, PyList>,
) -> PyResult<Vec<oxigdal_core::vector::Coordinate>> {
    use oxigdal_core::vector::Coordinate;

    let mut coords = Vec::new();
    for item in list.iter() {
        let coord_list = item
            .downcast::<PyList>()
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("Coordinate must be a list"))?;

        if coord_list.len() < 2 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "Coordinate must have at least 2 values [x, y]",
            ));
        }

        let x: f64 = coord_list.get_item(0)?.extract()?;
        let y: f64 = coord_list.get_item(1)?.extract()?;

        if coord_list.len() >= 3 {
            let z: f64 = coord_list.get_item(2)?.extract()?;
            coords.push(Coordinate::new_3d(x, y, z));
        } else {
            coords.push(Coordinate::new_2d(x, y));
        }
    }

    Ok(coords)
}

#[cfg(feature = "shapefile")]
/// Converts a Python value to a PropertyValue
fn python_to_property_value(value: &Bound<'_, PyAny>) -> PyResult<PropertyValue> {
    if value.is_none() {
        Ok(PropertyValue::Null)
    } else if let Ok(b) = value.extract::<bool>() {
        Ok(PropertyValue::Bool(b))
    } else if let Ok(i) = value.extract::<i64>() {
        Ok(PropertyValue::Integer(i))
    } else if let Ok(u) = value.extract::<u64>() {
        Ok(PropertyValue::UInteger(u))
    } else if let Ok(f) = value.extract::<f64>() {
        Ok(PropertyValue::Float(f))
    } else if let Ok(s) = value.extract::<String>() {
        Ok(PropertyValue::String(s))
    } else {
        // For unsupported types, store as null
        Ok(PropertyValue::Null)
    }
}

#[cfg(feature = "shapefile")]
/// Determines the ShapeType from an OxiGDAL Geometry
fn geometry_to_shape_type(geometry: &Geometry) -> PyResult<ShapeType> {
    match geometry {
        Geometry::Point(_) => Ok(ShapeType::Point),
        Geometry::LineString(_) | Geometry::MultiLineString(_) => Ok(ShapeType::PolyLine),
        Geometry::Polygon(_) | Geometry::MultiPolygon(_) => Ok(ShapeType::Polygon),
        Geometry::MultiPoint(_) => Ok(ShapeType::MultiPoint),
        Geometry::GeometryCollection(_) => Err(pyo3::exceptions::PyValueError::new_err(
            "GeometryCollection not supported in Shapefile format",
        )),
    }
}

#[cfg(feature = "shapefile")]
/// Builds a schema from features' attributes
fn build_schema_from_features(
    features: &[oxigdal_shapefile::ShapefileFeature],
) -> PyResult<Vec<oxigdal_shapefile::FieldDescriptor>> {
    if features.is_empty() {
        return Ok(Vec::new());
    }

    // Collect all unique field names
    let mut field_names = std::collections::HashSet::new();
    for feature in features {
        for key in feature.attributes.keys() {
            field_names.insert(key.clone());
        }
    }

    let mut builder = ShapefileSchemaBuilder::new();

    for field_name in field_names {
        // Infer field type from first non-null value
        let mut field_type = FieldType::Character;
        let mut max_length = 50;

        for feature in features {
            if let Some(value) = feature.attributes.get(&field_name) {
                match value {
                    PropertyValue::Integer(_) => {
                        field_type = FieldType::Number;
                        max_length = 18;
                        break;
                    }
                    PropertyValue::Float(_) => {
                        field_type = FieldType::Float;
                        max_length = 18;
                        break;
                    }
                    PropertyValue::Bool(_) => {
                        field_type = FieldType::Logical;
                        max_length = 1;
                        break;
                    }
                    PropertyValue::String(s) => {
                        max_length = max_length.max(s.len().min(254) as u8);
                    }
                    _ => {}
                }
            }
        }

        // Truncate field name to 10 characters (DBF limitation)
        let truncated_name = if field_name.len() > 10 {
            &field_name[..10]
        } else {
            &field_name
        };

        builder = match field_type {
            FieldType::Character => builder.add_character_field(truncated_name, max_length),
            FieldType::Number => builder.add_numeric_field(truncated_name, max_length, 0),
            FieldType::Float => builder.add_numeric_field(truncated_name, max_length, 6),
            FieldType::Logical => builder.add_logical_field(truncated_name),
            FieldType::Date => builder.add_date_field(truncated_name),
            _ => builder.add_character_field(truncated_name, max_length),
        }
        .map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!(
                "Failed to add field '{}': {}",
                field_name, e
            ))
        })?;
    }

    Ok(builder.build())
}
