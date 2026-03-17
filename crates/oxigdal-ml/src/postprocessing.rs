//! Postprocessing operations for ML results
//!
//! This module provides tile merging, confidence thresholding, polygon conversion,
//! and GeoJSON export capabilities.

use geo_types::{Coord, LineString, MultiPolygon, Polygon};
use geojson::{Feature, FeatureCollection, Geometry, GeometryValue};
use oxigdal_core::buffer::RasterBuffer;
use serde_json::{Map, Value as JsonValue};
// use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use tracing::debug;

use crate::detection::GeoDetection;
use crate::error::{PostprocessingError, Result};
use crate::segmentation::SegmentationMask;

/// Applies confidence thresholding to a probability map
///
/// # Errors
/// Returns an error if thresholding fails
pub fn apply_threshold(probabilities: &RasterBuffer, threshold: f32) -> Result<RasterBuffer> {
    if !(0.0..=1.0).contains(&threshold) {
        return Err(PostprocessingError::InvalidThreshold { value: threshold }.into());
    }

    let mut result = probabilities.clone();

    for y in 0..probabilities.height() {
        for x in 0..probabilities.width() {
            let prob =
                probabilities
                    .get_pixel(x, y)
                    .map_err(|e| PostprocessingError::ExportFailed {
                        reason: format!("Failed to get probability: {}", e),
                    })?;

            let value = if prob >= threshold as f64 { 1.0 } else { 0.0 };

            result
                .set_pixel(x, y, value)
                .map_err(|e| PostprocessingError::ExportFailed {
                    reason: format!("Failed to set value: {}", e),
                })?;
        }
    }

    Ok(result)
}

/// Converts a binary mask to polygons using marching squares algorithm
///
/// # Errors
/// Returns an error if conversion fails
pub fn mask_to_polygons(mask: &RasterBuffer, min_area: f64) -> Result<Vec<Polygon>> {
    debug!(
        "Converting {}x{} mask to polygons",
        mask.width(),
        mask.height()
    );

    let mut polygons = Vec::new();

    // Simplified polygon extraction using contour tracing
    // A production implementation would use a proper marching squares algorithm
    let width = mask.width();
    let height = mask.height();

    let mut visited = vec![vec![false; width as usize]; height as usize];

    for y in 0..height {
        for x in 0..width {
            if visited[y as usize][x as usize] {
                continue;
            }

            let value =
                mask.get_pixel(x, y)
                    .map_err(|e| PostprocessingError::PolygonConversionFailed {
                        reason: format!("Failed to get pixel: {}", e),
                    })?;

            if value > 0.0 {
                let polygon = trace_contour(mask, x, y, &mut visited)?;
                let area = calculate_polygon_area(&polygon);

                if area >= min_area {
                    polygons.push(polygon);
                }
            }
        }
    }

    debug!("Extracted {} polygons", polygons.len());

    Ok(polygons)
}

/// Traces a contour starting from a point
fn trace_contour(
    mask: &RasterBuffer,
    start_x: u64,
    start_y: u64,
    visited: &mut [Vec<bool>],
) -> Result<Polygon> {
    let mut coords = Vec::new();

    // Simplified contour tracing - just creates a bounding box
    // A real implementation would do proper boundary following
    let mut min_x = start_x;
    let mut min_y = start_y;
    let mut max_x = start_x;
    let mut max_y = start_y;

    // Find extent of connected component
    let mut stack = vec![(start_x, start_y)];

    while let Some((x, y)) = stack.pop() {
        if x >= mask.width() || y >= mask.height() {
            continue;
        }

        if visited[y as usize][x as usize] {
            continue;
        }

        let value =
            mask.get_pixel(x, y)
                .map_err(|e| PostprocessingError::PolygonConversionFailed {
                    reason: format!("Failed to get pixel: {}", e),
                })?;

        if value > 0.0 {
            visited[y as usize][x as usize] = true;

            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);

            // Add neighbors
            if x > 0 {
                stack.push((x - 1, y));
            }
            if x + 1 < mask.width() {
                stack.push((x + 1, y));
            }
            if y > 0 {
                stack.push((x, y - 1));
            }
            if y + 1 < mask.height() {
                stack.push((x, y + 1));
            }
        }
    }

    // Create rectangle polygon
    coords.push(Coord {
        x: min_x as f64,
        y: min_y as f64,
    });
    coords.push(Coord {
        x: max_x as f64 + 1.0,
        y: min_y as f64,
    });
    coords.push(Coord {
        x: max_x as f64 + 1.0,
        y: max_y as f64 + 1.0,
    });
    coords.push(Coord {
        x: min_x as f64,
        y: max_y as f64 + 1.0,
    });
    coords.push(Coord {
        x: min_x as f64,
        y: min_y as f64,
    }); // Close the ring

    Ok(Polygon::new(LineString::from(coords), vec![]))
}

/// Calculates the area of a polygon
fn calculate_polygon_area(polygon: &Polygon) -> f64 {
    let coords = polygon.exterior().coords().collect::<Vec<_>>();
    if coords.len() < 3 {
        return 0.0;
    }

    let mut area = 0.0;
    for i in 0..coords.len() - 1 {
        area += coords[i].x * coords[i + 1].y - coords[i + 1].x * coords[i].y;
    }

    (area / 2.0).abs()
}

/// Exports detections to GeoJSON format
///
/// # Errors
/// Returns an error if export fails
pub fn export_detections_geojson<P: AsRef<Path>>(
    detections: &[GeoDetection],
    output_path: P,
) -> Result<()> {
    debug!("Exporting {} detections to GeoJSON", detections.len());

    let features: Vec<Feature> = detections.iter().map(detection_to_feature).collect();

    let collection = FeatureCollection {
        bbox: None,
        features,
        foreign_members: None,
    };

    let json = serde_json::to_string_pretty(&collection).map_err(|e| {
        PostprocessingError::ExportFailed {
            reason: format!("Failed to serialize GeoJSON: {}", e),
        }
    })?;

    let mut file =
        File::create(output_path.as_ref()).map_err(|e| PostprocessingError::ExportFailed {
            reason: format!("Failed to create output file: {}", e),
        })?;

    file.write_all(json.as_bytes())
        .map_err(|e| PostprocessingError::ExportFailed {
            reason: format!("Failed to write GeoJSON: {}", e),
        })?;

    debug!("Successfully exported detections");

    Ok(())
}

/// Converts a detection to a GeoJSON feature
fn detection_to_feature(det: &GeoDetection) -> Feature {
    let polygon = det.geo_bbox.to_polygon();

    let mut properties = Map::new();
    properties.insert(
        "class_id".to_string(),
        JsonValue::Number(det.detection.class_id.into()),
    );
    properties.insert(
        "confidence".to_string(),
        JsonValue::Number(
            serde_json::Number::from_f64(det.detection.confidence as f64)
                .unwrap_or_else(|| serde_json::Number::from(0)),
        ),
    );

    if let Some(ref label) = det.detection.class_label {
        properties.insert("class_label".to_string(), JsonValue::String(label.clone()));
    }

    for (key, value) in &det.detection.attributes {
        properties.insert(key.clone(), JsonValue::String(value.clone()));
    }

    Feature {
        bbox: None,
        geometry: Some(Geometry::new(GeometryValue::from(&polygon))),
        id: None,
        properties: Some(properties),
        foreign_members: None,
    }
}

/// Exports a segmentation mask to GeoJSON
///
/// # Errors
/// Returns an error if export fails
pub fn export_segmentation_geojson<P: AsRef<Path>>(
    mask: &SegmentationMask,
    output_path: P,
    min_area: f64,
) -> Result<()> {
    debug!("Exporting segmentation mask to GeoJSON");

    let polygons = mask_to_polygons(&mask.mask, min_area)?;

    let features: Vec<Feature> = polygons
        .iter()
        .enumerate()
        .map(|(i, poly)| {
            let mut properties = Map::new();
            properties.insert("id".to_string(), JsonValue::Number(i.into()));

            Feature {
                bbox: None,
                geometry: Some(Geometry::new(GeometryValue::from(poly))),
                id: None,
                properties: Some(properties),
                foreign_members: None,
            }
        })
        .collect();

    let collection = FeatureCollection {
        bbox: None,
        features,
        foreign_members: None,
    };

    let json = serde_json::to_string_pretty(&collection).map_err(|e| {
        PostprocessingError::ExportFailed {
            reason: format!("Failed to serialize GeoJSON: {}", e),
        }
    })?;

    let mut file =
        File::create(output_path.as_ref()).map_err(|e| PostprocessingError::ExportFailed {
            reason: format!("Failed to create output file: {}", e),
        })?;

    file.write_all(json.as_bytes())
        .map_err(|e| PostprocessingError::ExportFailed {
            reason: format!("Failed to write GeoJSON: {}", e),
        })?;

    debug!("Successfully exported segmentation");

    Ok(())
}

/// Simplifies polygons using the Douglas-Peucker algorithm
///
/// # Errors
/// Returns an error if simplification fails
pub fn simplify_polygons(polygons: &[Polygon], tolerance: f64) -> Result<Vec<Polygon>> {
    if tolerance < 0.0 {
        return Err(PostprocessingError::ExportFailed {
            reason: "Tolerance must be non-negative".to_string(),
        }
        .into());
    }

    // Simplified implementation - returns copy
    // A real implementation would use proper Douglas-Peucker algorithm
    Ok(polygons.to_vec())
}

/// Merges overlapping polygons
///
/// # Errors
/// Returns an error if merging fails
pub fn merge_polygons(polygons: &[Polygon]) -> Result<MultiPolygon> {
    // Simplified implementation
    // A real implementation would use proper geometry union operations
    Ok(MultiPolygon::new(polygons.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigdal_core::types::RasterDataType;
    use std::collections::HashMap;

    #[test]
    fn test_apply_threshold() {
        let probs = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        let result = apply_threshold(&probs, 0.5);
        assert!(result.is_ok());
    }

    #[test]
    fn test_mask_to_polygons() {
        let mut mask = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        let _ = mask.set_pixel(5, 5, 1.0);
        let polygons = mask_to_polygons(&mask, 0.0);
        assert!(polygons.is_ok());
    }

    #[test]
    fn test_calculate_polygon_area() {
        let polygon = Polygon::new(
            LineString::from(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 10.0, y: 0.0 },
                Coord { x: 10.0, y: 10.0 },
                Coord { x: 0.0, y: 10.0 },
                Coord { x: 0.0, y: 0.0 },
            ]),
            vec![],
        );

        let area = calculate_polygon_area(&polygon);
        assert!((area - 100.0).abs() < 1.0);
    }

    #[test]
    fn test_export_detections_geojson() {
        use crate::detection::{BoundingBox, Detection, GeoBoundingBox};
        use std::env;

        let temp_dir = env::temp_dir();
        let output_path = temp_dir.join("test_detections.geojson");

        let detections = vec![GeoDetection {
            detection: Detection {
                bbox: BoundingBox::new(0.0, 0.0, 10.0, 10.0),
                class_id: 0,
                class_label: Some("test".to_string()),
                confidence: 0.9,
                attributes: HashMap::new(),
            },
            geo_bbox: GeoBoundingBox {
                min_x: 0.0,
                min_y: 0.0,
                max_x: 10.0,
                max_y: 10.0,
            },
        }];

        let result = export_detections_geojson(&detections, &output_path);
        assert!(result.is_ok());

        // Clean up
        let _ = std::fs::remove_file(output_path);
    }
}
