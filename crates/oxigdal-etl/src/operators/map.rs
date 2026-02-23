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
                            use flate2::Compression;
                            use flate2::write::GzEncoder;
                            use std::io::Write;

                            let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
                            encoder.write_all(&item).map_err(|e| {
                                crate::error::TransformError::Failed {
                                    message: e.to_string(),
                                }
                            })?;
                            encoder.finish().map_err(|e| {
                                crate::error::TransformError::Failed {
                                    message: e.to_string(),
                                }
                                .into()
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
                            use flate2::read::GzDecoder;
                            use std::io::Read;

                            let mut decoder = GzDecoder::new(&item[..]);
                            let mut decompressed = Vec::new();
                            decoder.read_to_end(&mut decompressed).map_err(|e| {
                                crate::error::TransformError::Failed {
                                    message: e.to_string(),
                                }
                            })?;
                            Ok(decompressed)
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

    /// Transform coordinates to different CRS
    pub fn transform_crs(source_epsg: u32, target_epsg: u32) -> Box<dyn Transform> {
        Box::new(MapTransform::new(
            format!("transform_crs_{}_{}", source_epsg, target_epsg),
            move |item| {
                Box::pin(async move {
                    // In real implementation, use oxigdal-proj for transformation
                    // For now, just pass through
                    tracing::warn!(
                        "CRS transformation not yet implemented: {} -> {}",
                        source_epsg,
                        target_epsg
                    );
                    Ok(item)
                })
            },
        ))
    }

    /// Calculate NDVI from raster bands
    pub fn calculate_ndvi() -> Box<dyn Transform> {
        Box::new(MapTransform::new("calculate_ndvi".to_string(), |item| {
            Box::pin(async move {
                // In real implementation, process raster data
                // For now, just pass through
                tracing::warn!("NDVI calculation not yet implemented");
                Ok(item)
            })
        }))
    }
}

/// Helper function to calculate bounding box from coordinates
fn calculate_bbox(_coords: &serde_json::Value) -> Result<serde_json::Value> {
    // Simplified bbox calculation
    // In real implementation, handle different geometry types
    Ok(serde_json::json!([0.0, 0.0, 0.0, 0.0]))
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
}
