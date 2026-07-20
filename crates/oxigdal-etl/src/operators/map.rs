//! Map operator for element-wise transformations
//!
//! This module provides map operators for applying functions to each item in the stream.

use crate::error::Result;
use crate::transform::{MapTransform, Transform};

/// Map operator builder
pub struct MapOperator;

impl MapOperator {
    /// Create a simple byte transformation
    pub fn bytes<F>(name: String, f: F) -> Box<dyn Transform>
    where
        F: Fn(Vec<u8>) -> Vec<u8> + Send + Sync + Clone + 'static,
    {
        Box::new(MapTransform::new(name, move |item| {
            let f_clone = f.clone();
            Box::pin(async move { Ok(f_clone(item)) })
        }))
    }

    /// Create an async byte transformation
    pub fn bytes_async<F, Fut>(name: String, f: F) -> Box<dyn Transform>
    where
        F: Fn(Vec<u8>) -> Fut + Send + Sync + 'static,
        Fut: futures::Future<Output = Result<Vec<u8>>> + Send + 'static,
    {
        Box::new(MapTransform::new(name, move |item| {
            let fut = f(item);
            Box::pin(fut)
        }))
    }

    /// Create a JSON field extraction operator
    pub fn extract_json_field(field: String) -> Box<dyn Transform> {
        Box::new(MapTransform::new(
            format!("extract_field_{}", field),
            move |item| {
                let field = field.clone();
                Box::pin(async move {
                    let value: serde_json::Value = serde_json::from_slice(&item)?;
                    let extracted = value.get(&field).ok_or_else(|| {
                        crate::error::TransformError::MissingField {
                            field: field.clone(),
                        }
                    })?;
                    Ok(serde_json::to_vec(extracted)?)
                })
            },
        ))
    }

    /// Create a JSON transformation operator
    pub fn transform_json<F>(name: String, f: F) -> Box<dyn Transform>
    where
        F: Fn(serde_json::Value) -> Result<serde_json::Value> + Send + Sync + Clone + 'static,
    {
        Box::new(MapTransform::new(name, move |item| {
            let f_clone = f.clone();
            Box::pin(async move {
                let value: serde_json::Value = serde_json::from_slice(&item)?;
                let transformed = f_clone(value)?;
                Ok(serde_json::to_vec(&transformed)?)
            })
        }))
    }

    /// Create a string transformation operator
    pub fn string<F>(name: String, f: F) -> Box<dyn Transform>
    where
        F: Fn(String) -> Result<String> + Send + Sync + Clone + 'static,
    {
        Box::new(MapTransform::new(name, move |item| {
            let f_clone = f.clone();
            Box::pin(async move {
                let s = String::from_utf8(item).map_err(|e| {
                    crate::error::TransformError::InvalidInput {
                        message: format!("Invalid UTF-8: {}", e),
                    }
                })?;
                let transformed = f_clone(s)?;
                Ok(transformed.into_bytes())
            })
        }))
    }

    /// Create a compression operator
    #[cfg(feature = "std")]
    pub fn compress(compression: CompressionType) -> Box<dyn Transform> {
        Box::new(MapTransform::new(
            format!("compress_{:?}", compression),
            move |item| {
                Box::pin(async move {
                    let result: Result<Vec<u8>> = match compression {
                        CompressionType::Gzip => {
                            oxiarc_archive::gzip::compress(&item, 6).map_err(|e| {
                                crate::error::EtlError::Transform(
                                    crate::error::TransformError::Failed {
                                        message: e.to_string(),
                                    },
                                )
                            })
                        }
                        CompressionType::None => Ok(item),
                    };
                    result
                })
            },
        ))
    }

    /// Create a decompression operator
    #[cfg(feature = "std")]
    pub fn decompress(compression: CompressionType) -> Box<dyn Transform> {
        Box::new(MapTransform::new(
            format!("decompress_{:?}", compression),
            move |item| {
                Box::pin(async move {
                    let result: Result<Vec<u8>> = match compression {
                        CompressionType::Gzip => {
                            let mut reader = std::io::Cursor::new(item.as_slice());
                            oxiarc_archive::gzip::decompress(&mut reader).map_err(|e| {
                                crate::error::EtlError::Transform(
                                    crate::error::TransformError::Failed {
                                        message: e.to_string(),
                                    },
                                )
                            })
                        }
                        CompressionType::None => Ok(item),
                    };
                    result
                })
            },
        ))
    }
}

/// Compression type
#[derive(Debug, Clone, Copy)]
pub enum CompressionType {
    /// No compression
    None,
    /// Gzip compression
    Gzip,
}

/// Specialized geospatial map operators
pub struct GeoMapOperator;

impl GeoMapOperator {
    /// Extract bounding box from GeoJSON
    pub fn extract_bbox() -> Box<dyn Transform> {
        Box::new(MapTransform::new("extract_bbox".to_string(), |item| {
            Box::pin(async move {
                let value: serde_json::Value = serde_json::from_slice(&item)?;

                // Extract bbox from GeoJSON
                let bbox = if let Some(bbox) = value.get("bbox") {
                    bbox.clone()
                } else if let Some(geometry) = value.get("geometry") {
                    // Calculate bbox from geometry coordinates
                    if let Some(coords) = geometry.get("coordinates") {
                        calculate_bbox(coords)?
                    } else {
                        return Err(crate::error::TransformError::MissingField {
                            field: "coordinates".to_string(),
                        }
                        .into());
                    }
                } else {
                    return Err(crate::error::TransformError::MissingField {
                        field: "bbox or geometry".to_string(),
                    }
                    .into());
                };

                Ok(serde_json::to_vec(&bbox)?)
            })
        }))
    }

    /// Transform coordinates to a different CRS.
    ///
    /// The input item is parsed as GeoJSON. Every `"coordinates"` array found anywhere in the
    /// document (geometries, `Feature.geometry`, `FeatureCollection.features[*].geometry`,
    /// `GeometryCollection.geometries[*]`, ...) is transformed in place from `source_epsg` to
    /// `target_epsg`. Only the `x`/`y` components of each coordinate tuple are reprojected; an
    /// optional third (`z`) component, if present, is left untouched. Documents that contain no
    /// `"coordinates"` key are passed through unchanged.
    ///
    /// A single [`oxigdal_proj::Transformer`] is built once per item and reused across every
    /// coordinate in the document, rather than rebuilding the transformation pipeline (CRS
    /// resolution + `oxiproj::Transformer` construction) for each coordinate as a per-coordinate
    /// [`oxigdal_proj::transform_epsg`] call would. This amortises the CRS-setup cost across the
    /// whole geometry, which dominates runtime for dense geometries.
    pub fn transform_crs(source_epsg: u32, target_epsg: u32) -> Box<dyn Transform> {
        Box::new(MapTransform::new(
            format!("transform_crs_{}_{}", source_epsg, target_epsg),
            move |item| {
                Box::pin(async move {
                    let mut value: serde_json::Value = serde_json::from_slice(&item)?;
                    // Building the `oxigdal_proj::Transformer` opens the bundled PROJ database,
                    // which internally builds its own current-thread Tokio runtime and calls
                    // `block_on`. Invoking that directly on an async worker thread panics with
                    // "Cannot start a runtime from within a runtime", so the reprojection is
                    // offloaded onto a blocking thread that carries no runtime context.
                    let value = tokio::task::spawn_blocking(move || {
                        // Build the transformer ONCE and reuse it for every coordinate in the
                        // document instead of rebuilding it per-coordinate.
                        let transformer =
                            oxigdal_proj::Transformer::from_epsg(source_epsg, target_epsg)
                                .map_err(|e| crate::error::TransformError::Failed {
                                    message: format!(
                                        "Failed to build CRS transformer EPSG:{} -> EPSG:{}: {}",
                                        source_epsg, target_epsg, e
                                    ),
                                })?;
                        walk_coordinates(&mut value, &transformer)?;
                        Ok::<serde_json::Value, crate::error::EtlError>(value)
                    })
                    .await
                    .map_err(|e| crate::error::TransformError::Failed {
                        message: format!("CRS transform task failed to join: {}", e),
                    })??;
                    Ok(serde_json::to_vec(&value)?)
                })
            },
        ))
    }

    /// Calculate NDVI (Normalized Difference Vegetation Index) from raster bands.
    ///
    /// Expects a JSON object with `"red"` and `"nir"` fields, each a JSON array of the same
    /// length containing the per-pixel reflectance values for the red and near-infrared bands.
    /// Adds an `"ndvi"` field to the object containing `(nir - red) / (nir + red)` computed
    /// element-wise. Pixels where `nir + red == 0.0` (e.g. masked/no-data pixels) would produce
    /// an undefined ratio; those pixels are emitted as `0.0` rather than `NaN`/`null` so that
    /// downstream consumers expecting a plain numeric array are unaffected.
    pub fn calculate_ndvi() -> Box<dyn Transform> {
        Box::new(MapTransform::new("calculate_ndvi".to_string(), |item| {
            Box::pin(async move {
                let mut value: serde_json::Value = serde_json::from_slice(&item)?;

                let red = extract_band(&value, "red")?;
                let nir = extract_band(&value, "nir")?;

                if red.len() != nir.len() {
                    return Err(crate::error::TransformError::InvalidInput {
                        message: format!(
                            "'red' and 'nir' bands must have the same length (red={}, nir={})",
                            red.len(),
                            nir.len()
                        ),
                    }
                    .into());
                }

                let ndvi: Vec<f64> = red
                    .iter()
                    .zip(nir.iter())
                    .map(|(&r, &n)| {
                        let denom = n + r;
                        if denom == 0.0 { 0.0 } else { (n - r) / denom }
                    })
                    .collect();

                match value.as_object_mut() {
                    Some(obj) => {
                        obj.insert("ndvi".to_string(), serde_json::json!(ndvi));
                    }
                    None => {
                        return Err(crate::error::TransformError::InvalidInput {
                            message: "Expected a JSON object with 'red' and 'nir' band arrays"
                                .to_string(),
                        }
                        .into());
                    }
                }

                Ok(serde_json::to_vec(&value)?)
            })
        }))
    }
}

/// Extracts a required numeric band array (e.g. `"red"`, `"nir"`) from a JSON object.
fn extract_band(value: &serde_json::Value, field: &str) -> Result<Vec<f64>> {
    let array = value
        .get(field)
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| crate::error::TransformError::MissingField {
            field: field.to_string(),
        })?;

    array
        .iter()
        .map(|v| {
            v.as_f64().ok_or_else(|| {
                crate::error::TransformError::InvalidInput {
                    message: format!("'{}' band must contain only numbers", field),
                }
                .into()
            })
        })
        .collect()
}

/// Returns `true` if `items` looks like a leaf coordinate tuple (`[x, y]` or `[x, y, z]`),
/// i.e. a non-empty JSON array whose elements are all numbers.
fn is_coordinate_pair(items: &[serde_json::Value]) -> bool {
    !items.is_empty() && items.iter().all(serde_json::Value::is_number)
}

/// Recursively walks a JSON value, reprojecting every `"coordinates"` array it finds.
///
/// This descends into objects and arrays looking for the GeoJSON `"coordinates"` key so that
/// nested structures (`Feature`, `FeatureCollection`, `GeometryCollection`) are all handled
/// without needing bespoke per-geometry-type logic. The same `transformer` is reused for every
/// coordinate so the CRS-setup cost is paid only once per document.
fn walk_coordinates(
    value: &mut serde_json::Value,
    transformer: &oxigdal_proj::Transformer,
) -> Result<()> {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                if key == "coordinates" {
                    transform_coordinate_array(child, transformer)?;
                } else {
                    walk_coordinates(child, transformer)?;
                }
            }
            Ok(())
        }
        serde_json::Value::Array(items) => {
            for item in items.iter_mut() {
                walk_coordinates(item, transformer)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Reprojects a GeoJSON `"coordinates"` value in place.
///
/// GeoJSON nests coordinate arrays to different depths depending on geometry type (`Point` is
/// `[x, y]`, `LineString`/`MultiPoint` is `[[x, y], ...]`, `Polygon`/`MultiLineString` is
/// `[[[x, y], ...], ...]`, and so on). This recurses until it finds a leaf tuple of numbers and
/// reprojects that tuple's `x`/`y` in place.
fn transform_coordinate_array(
    value: &mut serde_json::Value,
    transformer: &oxigdal_proj::Transformer,
) -> Result<()> {
    let items = match value {
        serde_json::Value::Array(items) => items,
        other => {
            return Err(crate::error::TransformError::InvalidInput {
                message: format!(
                    "Expected 'coordinates' to be a JSON array, got {}",
                    describe_json_type(other)
                ),
            }
            .into());
        }
    };

    if is_coordinate_pair(items) {
        if items.len() < 2 {
            return Err(crate::error::TransformError::InvalidInput {
                message: format!(
                    "Coordinate tuple must have at least 2 numbers, got {}",
                    items.len()
                ),
            }
            .into());
        }

        let x = items[0]
            .as_f64()
            .ok_or_else(|| crate::error::TransformError::InvalidInput {
                message: "Coordinate 'x' is not a valid number".to_string(),
            })?;
        let y = items[1]
            .as_f64()
            .ok_or_else(|| crate::error::TransformError::InvalidInput {
                message: "Coordinate 'y' is not a valid number".to_string(),
            })?;

        let transformed = transformer
            .transform(&oxigdal_proj::Coordinate::new(x, y))
            .map_err(|e| crate::error::TransformError::Failed {
                message: format!(
                    "CRS transformation EPSG:{} -> EPSG:{} failed: {}",
                    transformer.source_crs(),
                    transformer.target_crs(),
                    e
                ),
            })?;

        items[0] = serde_json::json!(transformed.x);
        items[1] = serde_json::json!(transformed.y);
    } else {
        for item in items.iter_mut() {
            transform_coordinate_array(item, transformer)?;
        }
    }

    Ok(())
}

/// Short human-readable name of a JSON value's type, used in error messages.
fn describe_json_type(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "a boolean",
        serde_json::Value::Number(_) => "a number",
        serde_json::Value::String(_) => "a string",
        serde_json::Value::Array(_) => "an array",
        serde_json::Value::Object(_) => "an object",
    }
}

/// Helper function to calculate a 2D bounding box `[min_x, min_y, max_x, max_y]` from a GeoJSON
/// `"coordinates"` value, regardless of how deeply the coordinate tuples are nested.
fn calculate_bbox(coords: &serde_json::Value) -> Result<serde_json::Value> {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    accumulate_bbox(coords, &mut min_x, &mut min_y, &mut max_x, &mut max_y)?;

    if !min_x.is_finite() || !min_y.is_finite() || !max_x.is_finite() || !max_y.is_finite() {
        return Err(crate::error::TransformError::InvalidInput {
            message: "No numeric coordinate tuples found while computing bounding box".to_string(),
        }
        .into());
    }

    Ok(serde_json::json!([min_x, min_y, max_x, max_y]))
}

/// Recursively folds every coordinate tuple in `value` into the running `[min, max]` extents.
fn accumulate_bbox(
    value: &serde_json::Value,
    min_x: &mut f64,
    min_y: &mut f64,
    max_x: &mut f64,
    max_y: &mut f64,
) -> Result<()> {
    let items = match value {
        serde_json::Value::Array(items) => items,
        other => {
            return Err(crate::error::TransformError::InvalidInput {
                message: format!(
                    "Expected 'coordinates' to be a JSON array, got {}",
                    describe_json_type(other)
                ),
            }
            .into());
        }
    };

    if is_coordinate_pair(items) {
        let x = items[0]
            .as_f64()
            .ok_or_else(|| crate::error::TransformError::InvalidInput {
                message: "Coordinate 'x' is not a valid number".to_string(),
            })?;
        let y = items
            .get(1)
            .and_then(serde_json::Value::as_f64)
            .ok_or_else(|| crate::error::TransformError::InvalidInput {
                message: "Coordinate 'y' is not a valid number".to_string(),
            })?;

        *min_x = min_x.min(x);
        *min_y = min_y.min(y);
        *max_x = max_x.max(x);
        *max_y = max_y.max(y);
    } else {
        for item in items {
            accumulate_bbox(item, min_x, min_y, max_x, max_y)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_bytes_map() {
        let op = MapOperator::bytes("double".to_string(), |mut bytes| {
            let copy = bytes.clone();
            bytes.extend_from_slice(&copy);
            bytes
        });

        let result = op.transform(vec![1, 2, 3]).await.expect("Failed");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], vec![1, 2, 3, 1, 2, 3]);
    }

    #[tokio::test]
    async fn test_extract_json_field() {
        let op = MapOperator::extract_json_field("name".to_string());

        let json = serde_json::json!({"name": "test", "value": 42});
        let item = serde_json::to_vec(&json).expect("Failed to serialize");

        let result = op.transform(item).await.expect("Failed");
        assert_eq!(result.len(), 1);

        let extracted: serde_json::Value =
            serde_json::from_slice(&result[0]).expect("Failed to parse");
        assert_eq!(extracted, "test");
    }

    #[tokio::test]
    async fn test_transform_json() {
        let op = MapOperator::transform_json("add_field".to_string(), |mut value| {
            if let Some(obj) = value.as_object_mut() {
                obj.insert("added".to_string(), serde_json::json!(true));
            }
            Ok(value)
        });

        let json = serde_json::json!({"original": "value"});
        let item = serde_json::to_vec(&json).expect("Failed to serialize");

        let result = op.transform(item).await.expect("Failed");
        let transformed: serde_json::Value =
            serde_json::from_slice(&result[0]).expect("Failed to parse");

        assert_eq!(
            transformed.get("original").and_then(|v| v.as_str()),
            Some("value")
        );
        assert_eq!(
            transformed.get("added").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[tokio::test]
    async fn test_transform_crs_point_geometry() {
        // WGS84 (EPSG:4326) -> Web Mercator (EPSG:3857)
        let op = GeoMapOperator::transform_crs(4326, 3857);

        let json = serde_json::json!({
            "type": "Point",
            "coordinates": [-122.4194, 37.7749]
        });
        let item = serde_json::to_vec(&json).expect("Failed to serialize");

        let result = op.transform(item).await.expect("transform_crs failed");
        assert_eq!(result.len(), 1);

        let transformed: serde_json::Value =
            serde_json::from_slice(&result[0]).expect("Failed to parse");
        let coords = transformed
            .get("coordinates")
            .and_then(|v| v.as_array())
            .expect("coordinates missing");

        let x = coords[0].as_f64().expect("x not a number");
        let y = coords[1].as_f64().expect("y not a number");

        // Spherical Web Mercator (EPSG:3857) X for lon -122.4194 is ~-13,627,665 m.
        assert!((x - (-13_627_665.27)).abs() < 1.0);
        assert!(y > 4_000_000.0 && y < 4_600_000.0);
    }

    #[tokio::test]
    async fn test_transform_crs_feature_collection() {
        let op = GeoMapOperator::transform_crs(4326, 3857);

        let json = serde_json::json!({
            "type": "FeatureCollection",
            "features": [
                {
                    "type": "Feature",
                    "properties": {},
                    "geometry": {
                        "type": "LineString",
                        "coordinates": [[-122.4194, 37.7749], [-122.42, 37.78]]
                    }
                }
            ]
        });
        let item = serde_json::to_vec(&json).expect("Failed to serialize");

        let result = op.transform(item).await.expect("transform_crs failed");
        let transformed: serde_json::Value =
            serde_json::from_slice(&result[0]).expect("Failed to parse");

        let coords = transformed["features"][0]["geometry"]["coordinates"]
            .as_array()
            .expect("coordinates missing");
        assert_eq!(coords.len(), 2);
        // Each nested tuple should have been reprojected away from the original lon/lat values.
        let first = coords[0].as_array().expect("tuple missing");
        assert!((first[0].as_f64().expect("x") - (-122.4194)).abs() > 1.0);
    }

    #[tokio::test]
    async fn test_transform_crs_no_coordinates_passthrough() {
        let op = GeoMapOperator::transform_crs(4326, 3857);

        let json = serde_json::json!({"foo": "bar"});
        let item = serde_json::to_vec(&json).expect("Failed to serialize");

        let result = op.transform(item).await.expect("transform_crs failed");
        let transformed: serde_json::Value =
            serde_json::from_slice(&result[0]).expect("Failed to parse");
        assert_eq!(transformed, json);
    }

    #[tokio::test]
    async fn test_extract_bbox_calculates_real_bbox() {
        let bbox_op = GeoMapOperator::extract_bbox();
        let json = serde_json::json!({
            "type": "Feature",
            "geometry": {
                "type": "Polygon",
                "coordinates": [[
                    [10.0, 20.0],
                    [15.0, 25.0],
                    [5.0, 30.0],
                    [10.0, 20.0]
                ]]
            }
        });
        let item = serde_json::to_vec(&json).expect("Failed to serialize");

        let result = bbox_op.transform(item).await.expect("extract_bbox failed");
        let bbox: serde_json::Value = serde_json::from_slice(&result[0]).expect("Failed to parse");

        assert_eq!(bbox, serde_json::json!([5.0, 20.0, 15.0, 30.0]));
    }

    #[tokio::test]
    async fn test_calculate_ndvi() {
        let op = GeoMapOperator::calculate_ndvi();

        let json = serde_json::json!({
            "red": [0.1, 0.2, 0.0],
            "nir": [0.5, 0.4, 0.0]
        });
        let item = serde_json::to_vec(&json).expect("Failed to serialize");

        let result = op.transform(item).await.expect("calculate_ndvi failed");
        let transformed: serde_json::Value =
            serde_json::from_slice(&result[0]).expect("Failed to parse");

        let ndvi = transformed
            .get("ndvi")
            .and_then(|v| v.as_array())
            .expect("ndvi missing");

        assert_eq!(ndvi.len(), 3);
        assert!((ndvi[0].as_f64().expect("n0") - ((0.5 - 0.1) / (0.5 + 0.1))).abs() < 1e-12);
        assert!((ndvi[1].as_f64().expect("n1") - ((0.4 - 0.2) / (0.4 + 0.2))).abs() < 1e-12);
        // Zero-denominator pixel is emitted as 0.0, not NaN/null.
        assert_eq!(ndvi[2].as_f64().expect("n2"), 0.0);
    }

    #[tokio::test]
    async fn test_calculate_ndvi_mismatched_lengths() {
        let op = GeoMapOperator::calculate_ndvi();

        let json = serde_json::json!({
            "red": [0.1, 0.2],
            "nir": [0.5]
        });
        let item = serde_json::to_vec(&json).expect("Failed to serialize");

        let result = op.transform(item).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_calculate_ndvi_missing_field() {
        let op = GeoMapOperator::calculate_ndvi();

        let json = serde_json::json!({"red": [0.1, 0.2]});
        let item = serde_json::to_vec(&json).expect("Failed to serialize");

        let result = op.transform(item).await;
        assert!(result.is_err());
    }
}
