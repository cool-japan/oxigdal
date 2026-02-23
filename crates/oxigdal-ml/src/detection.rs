//! Object detection for geospatial data
//!
//! This module provides bounding box detection, non-maximum suppression,
//! and georeferencing of detection results.

use geo_types::{Coord, Polygon, Rect};
use oxigdal_core::types::GeoTransform;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::debug;

use crate::error::{PostprocessingError, Result};

/// A detected object with bounding box
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Detection {
    /// Bounding box in pixel coordinates
    pub bbox: BoundingBox,
    /// Class ID
    pub class_id: usize,
    /// Class label
    pub class_label: Option<String>,
    /// Confidence score (0.0 to 1.0)
    pub confidence: f32,
    /// Additional attributes
    pub attributes: HashMap<String, String>,
}

/// Bounding box in pixel coordinates
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BoundingBox {
    /// X coordinate of top-left corner
    pub x: f32,
    /// Y coordinate of top-left corner
    pub y: f32,
    /// Width
    pub width: f32,
    /// Height
    pub height: f32,
}

impl BoundingBox {
    /// Creates a new bounding box
    #[must_use]
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Returns the area of the bounding box
    #[must_use]
    pub fn area(&self) -> f32 {
        self.width * self.height
    }

    /// Computes intersection with another bounding box
    #[must_use]
    pub fn intersection(&self, other: &Self) -> f32 {
        let x1 = self.x.max(other.x);
        let y1 = self.y.max(other.y);
        let x2 = (self.x + self.width).min(other.x + other.width);
        let y2 = (self.y + self.height).min(other.y + other.height);

        let width = (x2 - x1).max(0.0);
        let height = (y2 - y1).max(0.0);

        width * height
    }

    /// Computes Intersection over Union (IoU) with another bounding box
    #[must_use]
    pub fn iou(&self, other: &Self) -> f32 {
        let intersection = self.intersection(other);
        let union = self.area() + other.area() - intersection;

        if union > 0.0 {
            intersection / union
        } else {
            0.0
        }
    }

    /// Converts to a geo-types Rect
    #[must_use]
    pub fn to_rect(&self) -> Rect {
        Rect::new(
            Coord {
                x: self.x as f64,
                y: self.y as f64,
            },
            Coord {
                x: (self.x + self.width) as f64,
                y: (self.y + self.height) as f64,
            },
        )
    }
}

/// Georeferenced detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoDetection {
    /// Original detection
    pub detection: Detection,
    /// Georeferenced bounding box (in geographic coordinates)
    pub geo_bbox: GeoBoundingBox,
}

/// Bounding box in geographic coordinates
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct GeoBoundingBox {
    /// Minimum X (longitude/easting)
    pub min_x: f64,
    /// Minimum Y (latitude/northing)
    pub min_y: f64,
    /// Maximum X (longitude/easting)
    pub max_x: f64,
    /// Maximum Y (latitude/northing)
    pub max_y: f64,
}

impl GeoBoundingBox {
    /// Converts to a geo-types Polygon
    #[must_use]
    pub fn to_polygon(&self) -> Polygon {
        Polygon::new(
            vec![
                Coord {
                    x: self.min_x,
                    y: self.min_y,
                },
                Coord {
                    x: self.max_x,
                    y: self.min_y,
                },
                Coord {
                    x: self.max_x,
                    y: self.max_y,
                },
                Coord {
                    x: self.min_x,
                    y: self.max_y,
                },
                Coord {
                    x: self.min_x,
                    y: self.min_y,
                },
            ]
            .into(),
            vec![],
        )
    }
}

/// Non-maximum suppression (NMS) parameters
#[derive(Debug, Clone)]
pub struct NmsConfig {
    /// IoU threshold for suppression
    pub iou_threshold: f32,
    /// Confidence threshold for filtering
    pub confidence_threshold: f32,
    /// Maximum number of detections to keep
    pub max_detections: Option<usize>,
}

impl Default for NmsConfig {
    fn default() -> Self {
        Self {
            iou_threshold: 0.5,
            confidence_threshold: 0.5,
            max_detections: Some(100),
        }
    }
}

/// Applies non-maximum suppression to detections
///
/// # Errors
/// Returns an error if NMS fails
pub fn non_maximum_suppression(
    detections: &[Detection],
    config: &NmsConfig,
) -> Result<Vec<Detection>> {
    if !(0.0..=1.0).contains(&config.iou_threshold) {
        return Err(PostprocessingError::InvalidThreshold {
            value: config.iou_threshold,
        }
        .into());
    }

    if !(0.0..=1.0).contains(&config.confidence_threshold) {
        return Err(PostprocessingError::InvalidThreshold {
            value: config.confidence_threshold,
        }
        .into());
    }

    debug!("Applying NMS to {} detections", detections.len());

    // Filter by confidence threshold
    let mut filtered: Vec<_> = detections
        .iter()
        .filter(|d| d.confidence >= config.confidence_threshold)
        .cloned()
        .collect();

    // Sort by confidence (descending)
    filtered.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut keep = Vec::new();
    let mut suppressed = vec![false; filtered.len()];

    for i in 0..filtered.len() {
        if suppressed[i] {
            continue;
        }

        keep.push(filtered[i].clone());

        // Suppress overlapping detections
        for j in (i + 1)..filtered.len() {
            if suppressed[j] {
                continue;
            }

            // Only suppress detections of the same class
            if filtered[i].class_id != filtered[j].class_id {
                continue;
            }

            let iou = filtered[i].bbox.iou(&filtered[j].bbox);
            if iou > config.iou_threshold {
                suppressed[j] = true;
            }
        }

        // Check max detections limit
        if let Some(max_det) = config.max_detections {
            if keep.len() >= max_det {
                break;
            }
        }
    }

    debug!("NMS kept {} detections", keep.len());

    Ok(keep)
}

/// Georeferences detections using a geotransform
///
/// # Errors
/// Returns an error if georeferencing fails
pub fn georeference_detections(
    detections: &[Detection],
    geotransform: &GeoTransform,
) -> Result<Vec<GeoDetection>> {
    detections
        .iter()
        .map(|det| {
            let geo_bbox = pixel_bbox_to_geo(&det.bbox, geotransform)?;
            Ok(GeoDetection {
                detection: det.clone(),
                geo_bbox,
            })
        })
        .collect()
}

/// Converts a pixel bounding box to geographic coordinates
fn pixel_bbox_to_geo(bbox: &BoundingBox, gt: &GeoTransform) -> Result<GeoBoundingBox> {
    // Top-left corner
    let (min_x, max_y) = gt.pixel_to_world(bbox.x as f64, bbox.y as f64);

    // Bottom-right corner
    let (max_x, min_y) =
        gt.pixel_to_world((bbox.x + bbox.width) as f64, (bbox.y + bbox.height) as f64);

    Ok(GeoBoundingBox {
        min_x,
        min_y,
        max_x,
        max_y,
    })
}

/// Filters detections by class
#[must_use]
pub fn filter_by_class(detections: &[Detection], class_id: usize) -> Vec<Detection> {
    detections
        .iter()
        .filter(|d| d.class_id == class_id)
        .cloned()
        .collect()
}

/// Filters detections by confidence threshold
#[must_use]
pub fn filter_by_confidence(detections: &[Detection], threshold: f32) -> Vec<Detection> {
    detections
        .iter()
        .filter(|d| d.confidence >= threshold)
        .cloned()
        .collect()
}

/// Filters detections by area threshold
#[must_use]
pub fn filter_by_area(
    detections: &[Detection],
    min_area: f32,
    max_area: Option<f32>,
) -> Vec<Detection> {
    detections
        .iter()
        .filter(|d| {
            let area = d.bbox.area();
            area >= min_area && max_area.is_none_or(|max| area <= max)
        })
        .cloned()
        .collect()
}

/// Groups detections by class
#[must_use]
pub fn group_by_class(detections: &[Detection]) -> HashMap<usize, Vec<Detection>> {
    let mut groups: HashMap<usize, Vec<Detection>> = HashMap::new();

    for det in detections {
        groups.entry(det.class_id).or_default().push(det.clone());
    }

    groups
}

/// Computes detection statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionStatistics {
    /// Total number of detections
    pub total_detections: usize,
    /// Detections per class
    pub detections_per_class: HashMap<usize, usize>,
    /// Average confidence
    pub average_confidence: f32,
    /// Average bounding box area
    pub average_area: f32,
}

/// Computes statistics from detections
#[must_use]
pub fn compute_statistics(detections: &[Detection]) -> DetectionStatistics {
    let total_detections = detections.len();

    let mut detections_per_class: HashMap<usize, usize> = HashMap::new();
    let mut total_confidence = 0.0f32;
    let mut total_area = 0.0f32;

    for det in detections {
        *detections_per_class.entry(det.class_id).or_insert(0) += 1;
        total_confidence += det.confidence;
        total_area += det.bbox.area();
    }

    let average_confidence = if total_detections > 0 {
        total_confidence / total_detections as f32
    } else {
        0.0
    };

    let average_area = if total_detections > 0 {
        total_area / total_detections as f32
    } else {
        0.0
    };

    DetectionStatistics {
        total_detections,
        detections_per_class,
        average_confidence,
        average_area,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bounding_box() {
        let bbox1 = BoundingBox::new(0.0, 0.0, 10.0, 10.0);
        let bbox2 = BoundingBox::new(5.0, 5.0, 10.0, 10.0);

        assert!((bbox1.area() - 100.0).abs() < f32::EPSILON);
        assert!(bbox1.intersection(&bbox2) > 0.0);
        assert!(bbox1.iou(&bbox2) > 0.0);
        assert!(bbox1.iou(&bbox2) < 1.0);
    }

    #[test]
    fn test_nms_config_default() {
        let config = NmsConfig::default();
        assert!((config.iou_threshold - 0.5).abs() < f32::EPSILON);
        assert!((config.confidence_threshold - 0.5).abs() < f32::EPSILON);
        assert_eq!(config.max_detections, Some(100));
    }

    #[test]
    fn test_nms() {
        let detections = vec![
            Detection {
                bbox: BoundingBox::new(0.0, 0.0, 10.0, 10.0),
                class_id: 0,
                class_label: None,
                confidence: 0.9,
                attributes: HashMap::new(),
            },
            Detection {
                bbox: BoundingBox::new(2.0, 2.0, 10.0, 10.0),
                class_id: 0,
                class_label: None,
                confidence: 0.8,
                attributes: HashMap::new(),
            },
        ];

        let config = NmsConfig::default();
        let result = non_maximum_suppression(&detections, &config);
        assert!(result.is_ok());
        let result = result.ok().unwrap_or_default();
        // IoU = 64/136 ≈ 0.47, just below threshold of 0.5, both kept
        assert_eq!(result.len(), 2); // Low overlap, both kept
    }

    #[test]
    fn test_nms_suppression() {
        // Test with high overlap to verify suppression works
        let detections = vec![
            Detection {
                bbox: BoundingBox::new(0.0, 0.0, 10.0, 10.0),
                class_id: 0,
                class_label: None,
                confidence: 0.9,
                attributes: HashMap::new(),
            },
            Detection {
                bbox: BoundingBox::new(1.0, 1.0, 10.0, 10.0),
                class_id: 0,
                class_label: None,
                confidence: 0.8,
                attributes: HashMap::new(),
            },
        ];

        let config = NmsConfig::default();
        let result = non_maximum_suppression(&detections, &config);
        assert!(result.is_ok());
        let result = result.ok().unwrap_or_default();
        // IoU = 81/119 ≈ 0.68, above threshold of 0.5, one suppressed
        assert_eq!(result.len(), 1);
        assert!((result[0].confidence - 0.9).abs() < f32::EPSILON);
    }

    #[test]
    fn test_filter_by_confidence() {
        let detections = vec![
            Detection {
                bbox: BoundingBox::new(0.0, 0.0, 10.0, 10.0),
                class_id: 0,
                class_label: None,
                confidence: 0.9,
                attributes: HashMap::new(),
            },
            Detection {
                bbox: BoundingBox::new(5.0, 5.0, 10.0, 10.0),
                class_id: 0,
                class_label: None,
                confidence: 0.3,
                attributes: HashMap::new(),
            },
        ];

        let filtered = filter_by_confidence(&detections, 0.5);
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn test_compute_statistics() {
        let detections = vec![
            Detection {
                bbox: BoundingBox::new(0.0, 0.0, 10.0, 10.0),
                class_id: 0,
                class_label: None,
                confidence: 0.9,
                attributes: HashMap::new(),
            },
            Detection {
                bbox: BoundingBox::new(5.0, 5.0, 10.0, 10.0),
                class_id: 1,
                class_label: None,
                confidence: 0.8,
                attributes: HashMap::new(),
            },
        ];

        let stats = compute_statistics(&detections);
        assert_eq!(stats.total_detections, 2);
        assert_eq!(stats.detections_per_class.len(), 2);
    }
}
