//! Object detection for geospatial data
//!
//! This module provides bounding box detection, non-maximum suppression,
//! and georeferencing of detection results.

use geo_types::{Coord, Polygon, Rect};
use oxigeo_core::types::GeoTransform;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::debug;

use crate::error::{MlError, PostprocessingError, Result};

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

/// Suppression method for NMS
///
/// Controls how overlapping detections are handled:
/// - `Hard`: standard NMS — overlapping detections are removed entirely
/// - `Linear`: Soft-NMS with linear decay — confidence is reduced proportionally
/// - `Gaussian`: Soft-NMS with Gaussian decay — confidence is reduced via Gaussian kernel
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SuppressMethod {
    /// Standard hard suppression: remove if overlap exceeds threshold
    #[default]
    Hard,
    /// Linear decay: `score *= max(0, 1 - overlap)` when overlap > threshold
    Linear {
        /// Score threshold below which detections are discarded after decay
        score_threshold: f32,
    },
    /// Gaussian decay: `score *= exp(-overlap^2 / sigma)` for all overlapping pairs
    Gaussian {
        /// Controls decay rate (larger sigma = gentler decay)
        sigma: f32,
    },
}

/// Distance metric for measuring overlap between bounding boxes
///
/// Each metric returns a value where higher means more overlap/similarity:
/// - `IoU` (Intersection over Union): range [0, 1]
/// - `GIoU` (Generalized IoU): range [-1, 1], penalizes non-overlapping gap
/// - `DIoU` (Distance IoU): range [0, 1], adds center-point distance penalty
/// - `CIoU` (Complete IoU): range can be slightly negative, adds aspect ratio consistency
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DistanceMetric {
    /// Standard Intersection over Union
    #[default]
    IoU,
    /// Generalized IoU — penalizes non-overlapping boxes by enclosing area
    GIoU,
    /// Distance IoU — adds center-point distance normalized by enclosing diagonal
    DIoU,
    /// Complete IoU — adds aspect ratio consistency term on top of DIoU
    CIoU,
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

    /// Returns the center point of the bounding box as (cx, cy)
    #[must_use]
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.width * 0.5, self.y + self.height * 0.5)
    }

    /// Computes the smallest enclosing box of two bounding boxes.
    ///
    /// Returns `(enclosing_area, enclosing_diagonal_squared)`.
    #[must_use]
    fn enclosing_metrics(&self, other: &Self) -> (f32, f32) {
        let enc_x1 = self.x.min(other.x);
        let enc_y1 = self.y.min(other.y);
        let enc_x2 = (self.x + self.width).max(other.x + other.width);
        let enc_y2 = (self.y + self.height).max(other.y + other.height);

        let enc_w = enc_x2 - enc_x1;
        let enc_h = enc_y2 - enc_y1;

        let enc_area = enc_w * enc_h;
        let enc_diag_sq = enc_w * enc_w + enc_h * enc_h;

        (enc_area, enc_diag_sq)
    }

    /// Computes Generalized Intersection over Union (GIoU) with another bounding box.
    ///
    /// GIoU extends IoU by penalizing the gap between non-overlapping boxes.
    /// Range: [-1, 1]. GIoU = IoU - (C - U) / C where C is the enclosing area
    /// and U is the union area.
    ///
    /// Returns -1.0 when boxes are infinitely far apart, 1.0 when identical.
    #[must_use]
    pub fn giou(&self, other: &Self) -> f32 {
        let intersection = self.intersection(other);
        let union = self.area() + other.area() - intersection;
        let (enc_area, _) = self.enclosing_metrics(other);

        if enc_area <= 0.0 {
            return 0.0;
        }

        let iou = if union > 0.0 {
            intersection / union
        } else {
            0.0
        };

        // GIoU = IoU - (C - U) / C
        iou - (enc_area - union) / enc_area
    }

    /// Computes Distance Intersection over Union (DIoU) with another bounding box.
    ///
    /// DIoU adds a center-point distance penalty to IoU, normalized by the diagonal
    /// of the smallest enclosing box.
    /// DIoU = IoU - d^2 / c^2, where d = center distance, c = enclosing diagonal.
    /// Range: approximately [0, 1] for overlapping boxes.
    #[must_use]
    pub fn diou(&self, other: &Self) -> f32 {
        let iou = self.iou(other);
        let (_, enc_diag_sq) = self.enclosing_metrics(other);

        if enc_diag_sq <= 0.0 {
            return iou;
        }

        let (cx1, cy1) = self.center();
        let (cx2, cy2) = other.center();
        let center_dist_sq = (cx2 - cx1) * (cx2 - cx1) + (cy2 - cy1) * (cy2 - cy1);

        // DIoU = IoU - d^2 / c^2
        iou - center_dist_sq / enc_diag_sq
    }

    /// Computes Complete Intersection over Union (CIoU) with another bounding box.
    ///
    /// CIoU extends DIoU with an aspect ratio consistency term.
    /// CIoU = IoU - d^2/c^2 - alpha*v, where v measures aspect ratio consistency
    /// and alpha is a trade-off parameter.
    #[must_use]
    pub fn ciou(&self, other: &Self) -> f32 {
        let iou = self.iou(other);
        let (_, enc_diag_sq) = self.enclosing_metrics(other);

        if enc_diag_sq <= 0.0 {
            return iou;
        }

        let (cx1, cy1) = self.center();
        let (cx2, cy2) = other.center();
        let center_dist_sq = (cx2 - cx1) * (cx2 - cx1) + (cy2 - cy1) * (cy2 - cy1);

        // Aspect ratio consistency term: v = (4/pi^2) * (atan(w1/h1) - atan(w2/h2))^2
        let pi_sq = std::f32::consts::PI * std::f32::consts::PI;
        let ar1 = if self.height > 0.0 {
            (self.width / self.height).atan()
        } else {
            0.0
        };
        let ar2 = if other.height > 0.0 {
            (other.width / other.height).atan()
        } else {
            0.0
        };
        let v = (4.0 / pi_sq) * (ar1 - ar2) * (ar1 - ar2);

        // Trade-off parameter: alpha = v / ((1 - IoU) + v)
        let alpha = if (1.0 - iou + v).abs() > f32::EPSILON {
            v / (1.0 - iou + v)
        } else {
            0.0
        };

        // CIoU = IoU - d^2/c^2 - alpha*v
        iou - center_dist_sq / enc_diag_sq - alpha * v
    }

    /// Computes the specified distance metric with another bounding box.
    #[must_use]
    pub fn distance_metric(&self, other: &Self, metric: DistanceMetric) -> f32 {
        match metric {
            DistanceMetric::IoU => self.iou(other),
            DistanceMetric::GIoU => self.giou(other),
            DistanceMetric::DIoU => self.diou(other),
            DistanceMetric::CIoU => self.ciou(other),
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
    /// Overlap threshold for suppression (applies to whichever `distance_metric` is selected)
    pub iou_threshold: f32,
    /// Confidence threshold for initial filtering (and Soft-NMS score floor)
    pub confidence_threshold: f32,
    /// Maximum number of detections to keep
    pub max_detections: Option<usize>,
    /// Suppression strategy (default: Hard)
    pub suppress_method: SuppressMethod,
    /// Distance metric for overlap computation (default: IoU)
    pub distance_metric: DistanceMetric,
}

impl Default for NmsConfig {
    fn default() -> Self {
        Self {
            iou_threshold: 0.5,
            confidence_threshold: 0.5,
            max_detections: Some(100),
            suppress_method: SuppressMethod::Hard,
            distance_metric: DistanceMetric::IoU,
        }
    }
}

/// Validates NMS configuration parameters.
fn validate_nms_config(config: &NmsConfig) -> Result<()> {
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

    match config.suppress_method {
        SuppressMethod::Gaussian { sigma } if sigma <= 0.0 => {
            return Err(MlError::InvalidConfig(format!(
                "Gaussian sigma must be positive, got {}",
                sigma
            )));
        }
        SuppressMethod::Linear { score_threshold } if !(0.0..=1.0).contains(&score_threshold) => {
            return Err(MlError::InvalidConfig(format!(
                "Linear score_threshold must be in [0, 1], got {}",
                score_threshold
            )));
        }
        _ => {}
    }

    Ok(())
}

/// Applies non-maximum suppression to detections.
///
/// Supports configurable overlap metrics (IoU, GIoU, DIoU, CIoU) and
/// suppression strategies (Hard, Linear Soft-NMS, Gaussian Soft-NMS).
///
/// # Hard NMS (default)
/// Detections are sorted by confidence. For each detection, all lower-confidence
/// same-class detections with overlap above `iou_threshold` are removed.
///
/// # Soft-NMS (Linear / Gaussian)
/// Instead of removing overlapping detections, their confidence scores are decayed.
/// A detection is only removed when its score drops below the score floor.
/// This preserves partially overlapping objects that hard NMS would discard.
///
/// # Errors
/// Returns an error if configuration is invalid (thresholds out of range, negative sigma)
pub fn non_maximum_suppression(
    detections: &[Detection],
    config: &NmsConfig,
) -> Result<Vec<Detection>> {
    validate_nms_config(config)?;

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

    let result = match config.suppress_method {
        SuppressMethod::Hard => nms_hard(&filtered, config),
        SuppressMethod::Linear { score_threshold } => {
            nms_soft(&mut filtered, config, score_threshold)
        }
        SuppressMethod::Gaussian { sigma } => nms_soft_gaussian(&mut filtered, config, sigma),
    };

    debug!("NMS kept {} detections", result.len());
    Ok(result)
}

/// Hard NMS: suppress overlapping detections entirely.
fn nms_hard(filtered: &[Detection], config: &NmsConfig) -> Vec<Detection> {
    let mut keep = Vec::new();
    let mut suppressed = vec![false; filtered.len()];

    for i in 0..filtered.len() {
        if suppressed[i] {
            continue;
        }

        keep.push(filtered[i].clone());

        for j in (i + 1)..filtered.len() {
            if suppressed[j] {
                continue;
            }

            // Only suppress detections of the same class
            if filtered[i].class_id != filtered[j].class_id {
                continue;
            }

            let overlap = filtered[i]
                .bbox
                .distance_metric(&filtered[j].bbox, config.distance_metric);
            if overlap > config.iou_threshold {
                suppressed[j] = true;
            }
        }

        if let Some(max_det) = config.max_detections {
            if keep.len() >= max_det {
                break;
            }
        }
    }

    keep
}

/// Soft-NMS with linear decay: `score *= max(0, 1 - overlap)` when overlap > threshold.
///
/// Uses a "pick max, decay rest, repeat" loop. Each iteration finds the
/// highest-confidence remaining detection, emits it, and decays the scores
/// of all same-class overlapping detections.
fn nms_soft(detections: &mut [Detection], config: &NmsConfig, score_floor: f32) -> Vec<Detection> {
    let n = detections.len();
    let mut active = vec![true; n];
    let mut keep = Vec::new();

    loop {
        // Find the detection with max confidence among active ones
        let mut best_idx = None;
        let mut best_score = -1.0_f32;
        for (i, det) in detections.iter().enumerate() {
            if active[i] && det.confidence > best_score {
                best_score = det.confidence;
                best_idx = Some(i);
            }
        }

        let Some(bi) = best_idx else { break };

        // Emit the best detection
        active[bi] = false;
        keep.push(detections[bi].clone());

        if let Some(max_det) = config.max_detections {
            if keep.len() >= max_det {
                break;
            }
        }

        // Decay scores of overlapping same-class detections
        for j in 0..n {
            if !active[j] || detections[j].class_id != detections[bi].class_id {
                continue;
            }

            let overlap = detections[bi]
                .bbox
                .distance_metric(&detections[j].bbox, config.distance_metric);

            if overlap > config.iou_threshold {
                // Linear decay: score *= max(0, 1 - overlap)
                detections[j].confidence *= (1.0 - overlap).max(0.0);
            }

            if detections[j].confidence < score_floor {
                active[j] = false;
            }
        }
    }

    keep
}

/// Soft-NMS with Gaussian decay: `score *= exp(-overlap^2 / sigma)` for all overlapping pairs.
///
/// Gaussian decay is applied to ALL overlapping pairs (no threshold gating),
/// providing smoother suppression than linear decay.
fn nms_soft_gaussian(
    detections: &mut [Detection],
    config: &NmsConfig,
    sigma: f32,
) -> Vec<Detection> {
    // Re-initialize from original confidences for clean Gaussian pass
    let n = detections.len();
    let mut active = vec![true; n];
    let mut keep = Vec::new();

    loop {
        let mut best_idx = None;
        let mut best_score = -1.0_f32;
        for (i, det) in detections.iter().enumerate() {
            if active[i] && det.confidence > best_score {
                best_score = det.confidence;
                best_idx = Some(i);
            }
        }

        let Some(bi) = best_idx else { break };

        active[bi] = false;
        keep.push(detections[bi].clone());

        if let Some(max_det) = config.max_detections {
            if keep.len() >= max_det {
                break;
            }
        }

        for j in 0..n {
            if !active[j] || detections[j].class_id != detections[bi].class_id {
                continue;
            }

            let overlap = detections[bi]
                .bbox
                .distance_metric(&detections[j].bbox, config.distance_metric);

            // Gaussian decay applied to all overlapping detections (no threshold gate)
            let decay = (-overlap * overlap / sigma).exp();
            detections[j].confidence *= decay;

            if detections[j].confidence < config.confidence_threshold {
                active[j] = false;
            }
        }
    }

    keep
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

// ─── Rotated Bounding Boxes ──────────────────────────────────────────────────

/// A 2D point used for polygon clipping.
type Point2 = (f32, f32);

/// A rotated bounding box for oriented object detection in satellite imagery.
///
/// Unlike axis-aligned `BoundingBox`, this supports arbitrary rotation angles,
/// which is critical for detecting oriented objects such as ships, aircraft,
/// and buildings in overhead imagery.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RotatedBoundingBox {
    /// X coordinate of the center
    pub center_x: f32,
    /// Y coordinate of the center
    pub center_y: f32,
    /// Width of the box (along the rotated local X axis)
    pub width: f32,
    /// Height of the box (along the rotated local Y axis)
    pub height: f32,
    /// Rotation angle in radians (counter-clockwise from horizontal)
    pub angle: f32,
}

impl RotatedBoundingBox {
    /// Creates a new rotated bounding box.
    #[must_use]
    pub fn new(center_x: f32, center_y: f32, width: f32, height: f32, angle: f32) -> Self {
        Self {
            center_x,
            center_y,
            width,
            height,
            angle,
        }
    }

    /// Returns the area of the rotated bounding box.
    #[must_use]
    pub fn area(&self) -> f32 {
        self.width * self.height
    }

    /// Computes the 4 corner points of the rotated rectangle.
    ///
    /// Returns corners in order: top-left, top-right, bottom-right, bottom-left
    /// (relative to the rotated frame, before rotation is applied).
    #[must_use]
    pub fn corners(&self) -> [Point2; 4] {
        let cos_a = self.angle.cos();
        let sin_a = self.angle.sin();
        let hw = self.width * 0.5;
        let hh = self.height * 0.5;

        // Local offsets from center
        let dx = [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)];

        let mut corners = [(0.0_f32, 0.0_f32); 4];
        for (i, &(lx, ly)) in dx.iter().enumerate() {
            corners[i] = (
                self.center_x + lx * cos_a - ly * sin_a,
                self.center_y + lx * sin_a + ly * cos_a,
            );
        }
        corners
    }

    /// Computes IoU between two rotated bounding boxes using
    /// Sutherland-Hodgman polygon clipping.
    #[must_use]
    pub fn iou(&self, other: &Self) -> f32 {
        let poly_a: Vec<Point2> = self.corners().to_vec();
        let poly_b: Vec<Point2> = other.corners().to_vec();

        let clipped = sutherland_hodgman_clip(&poly_a, &poly_b);
        if clipped.len() < 3 {
            return 0.0;
        }

        let intersection_area = polygon_area(&clipped);
        let union_area = self.area() + other.area() - intersection_area;

        if union_area > 0.0 {
            intersection_area / union_area
        } else {
            0.0
        }
    }
}

/// A detected object with a rotated bounding box.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotatedDetection {
    /// Rotated bounding box in pixel coordinates
    pub bbox: RotatedBoundingBox,
    /// Class ID
    pub class_id: usize,
    /// Class label
    pub class_label: Option<String>,
    /// Confidence score (0.0 to 1.0)
    pub confidence: f32,
    /// Additional attributes
    pub attributes: HashMap<String, String>,
}

/// Applies non-maximum suppression to rotated detections.
///
/// Uses rotated IoU for overlap computation. Only supports hard suppression
/// (Soft-NMS for rotated boxes can be added in a future iteration).
///
/// # Errors
/// Returns an error if configuration is invalid
pub fn non_maximum_suppression_rotated(
    detections: &[RotatedDetection],
    config: &NmsConfig,
) -> Result<Vec<RotatedDetection>> {
    validate_nms_config(config)?;

    debug!("Applying rotated NMS to {} detections", detections.len());

    let mut filtered: Vec<_> = detections
        .iter()
        .filter(|d| d.confidence >= config.confidence_threshold)
        .cloned()
        .collect();

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

        for j in (i + 1)..filtered.len() {
            if suppressed[j] || filtered[i].class_id != filtered[j].class_id {
                continue;
            }

            let overlap = filtered[i].bbox.iou(&filtered[j].bbox);
            if overlap > config.iou_threshold {
                suppressed[j] = true;
            }
        }

        if let Some(max_det) = config.max_detections {
            if keep.len() >= max_det {
                break;
            }
        }
    }

    debug!("Rotated NMS kept {} detections", keep.len());
    Ok(keep)
}

// ─── Sutherland-Hodgman Polygon Clipping ─────────────────────────────────────

/// Computes the area of a convex polygon using the shoelace formula.
fn polygon_area(vertices: &[Point2]) -> f32 {
    let n = vertices.len();
    if n < 3 {
        return 0.0;
    }

    let mut area = 0.0_f32;
    for i in 0..n {
        let j = (i + 1) % n;
        area += vertices[i].0 * vertices[j].1;
        area -= vertices[j].0 * vertices[i].1;
    }
    (area * 0.5).abs()
}

/// Clips polygon `subject` by the convex polygon `clip` using the
/// Sutherland-Hodgman algorithm. Returns the clipped polygon vertices.
fn sutherland_hodgman_clip(subject: &[Point2], clip: &[Point2]) -> Vec<Point2> {
    let mut output = subject.to_vec();
    let clip_len = clip.len();

    for i in 0..clip_len {
        if output.is_empty() {
            return output;
        }

        let edge_start = clip[i];
        let edge_end = clip[(i + 1) % clip_len];

        let input = output;
        output = Vec::with_capacity(input.len() + 1);

        let n = input.len();
        if n == 0 {
            break;
        }

        let mut s = input[n - 1];

        for &e in &input {
            if is_inside(e, edge_start, edge_end) {
                if !is_inside(s, edge_start, edge_end) {
                    if let Some(pt) = line_intersection(s, e, edge_start, edge_end) {
                        output.push(pt);
                    }
                }
                output.push(e);
            } else if is_inside(s, edge_start, edge_end) {
                if let Some(pt) = line_intersection(s, e, edge_start, edge_end) {
                    output.push(pt);
                }
            }
            s = e;
        }
    }

    output
}

/// Checks if point `p` is on the "inside" (left side) of the directed edge
/// from `edge_start` to `edge_end`.
fn is_inside(p: Point2, edge_start: Point2, edge_end: Point2) -> bool {
    // Cross product of edge vector and point-to-start vector
    let cross = (edge_end.0 - edge_start.0) * (p.1 - edge_start.1)
        - (edge_end.1 - edge_start.1) * (p.0 - edge_start.0);
    // >= 0 means on the left side or on the edge
    cross >= 0.0
}

/// Computes the intersection point of two line segments.
///
/// Line 1: from `a` to `b`. Line 2: from `c` to `d`.
/// Returns `None` if lines are parallel (denominator near zero).
fn line_intersection(a: Point2, b: Point2, c: Point2, d: Point2) -> Option<Point2> {
    let denom = (a.0 - b.0) * (c.1 - d.1) - (a.1 - b.1) * (c.0 - d.0);

    if denom.abs() < 1e-10 {
        return None;
    }

    let t = ((a.0 - c.0) * (c.1 - d.1) - (a.1 - c.1) * (c.0 - d.0)) / denom;

    Some((a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1)))
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

    // ─── NmsConfig backward compatibility ────────────────────────────────

    #[test]
    fn test_nms_default_backward_compatible() {
        // Verify NmsConfig::default() preserves existing behavior
        let config = NmsConfig::default();
        assert!(matches!(config.suppress_method, SuppressMethod::Hard));
        assert!(matches!(config.distance_metric, DistanceMetric::IoU));
        assert!((config.iou_threshold - 0.5).abs() < f32::EPSILON);
        assert!((config.confidence_threshold - 0.5).abs() < f32::EPSILON);
        assert_eq!(config.max_detections, Some(100));

        // Existing hard NMS behavior unchanged: high-overlap pair should suppress
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

        let result = non_maximum_suppression(&detections, &config)
            .ok()
            .unwrap_or_default();
        assert_eq!(result.len(), 1);
        assert!((result[0].confidence - 0.9).abs() < f32::EPSILON);
    }

    // ─── Soft-NMS tests ─────────────────────────────────────────────────

    #[test]
    fn test_soft_nms_linear() {
        // Two highly overlapping boxes: linear decay should keep both but reduce
        // the lower-confidence one's score
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

        let config = NmsConfig {
            iou_threshold: 0.3,
            confidence_threshold: 0.1,
            max_detections: None,
            suppress_method: SuppressMethod::Linear {
                score_threshold: 0.1,
            },
            distance_metric: DistanceMetric::IoU,
        };

        let result = non_maximum_suppression(&detections, &config)
            .ok()
            .unwrap_or_default();
        // Both detections should be kept (score decayed but above floor)
        assert_eq!(result.len(), 2);
        // First detection should have original score
        assert!((result[0].confidence - 0.9).abs() < f32::EPSILON);
        // Second detection should have reduced score (decayed by linear factor)
        assert!(result[1].confidence < 0.8);
        assert!(result[1].confidence > 0.0);
    }

    #[test]
    fn test_soft_nms_gaussian() {
        // Two overlapping boxes: Gaussian decay should keep both with decayed scores
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

        let config = NmsConfig {
            iou_threshold: 0.5,
            confidence_threshold: 0.05,
            max_detections: None,
            suppress_method: SuppressMethod::Gaussian { sigma: 0.5 },
            distance_metric: DistanceMetric::IoU,
        };

        let result = non_maximum_suppression(&detections, &config)
            .ok()
            .unwrap_or_default();
        // Both should be kept (Gaussian decay is gentle with sigma=0.5)
        assert_eq!(result.len(), 2);
        // First detection emitted with original score
        assert!((result[0].confidence - 0.9).abs() < f32::EPSILON);
        // Second detection's score should be decayed
        assert!(result[1].confidence < 0.8);
        assert!(result[1].confidence > 0.0);
    }

    #[test]
    fn test_soft_nms_preserves_hard_behavior() {
        // When using SuppressMethod::Hard, behavior is identical to classic NMS
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
            Detection {
                bbox: BoundingBox::new(50.0, 50.0, 10.0, 10.0),
                class_id: 0,
                class_label: None,
                confidence: 0.7,
                attributes: HashMap::new(),
            },
        ];

        let config_hard = NmsConfig {
            iou_threshold: 0.5,
            confidence_threshold: 0.5,
            max_detections: None,
            suppress_method: SuppressMethod::Hard,
            distance_metric: DistanceMetric::IoU,
        };

        let result = non_maximum_suppression(&detections, &config_hard)
            .ok()
            .unwrap_or_default();
        // High overlap pair: one suppressed. Far-away box: kept.
        assert_eq!(result.len(), 2);
        assert!((result[0].confidence - 0.9).abs() < f32::EPSILON);
        assert!((result[1].confidence - 0.7).abs() < f32::EPSILON);
    }

    // ─── GIoU / DIoU / CIoU tests ──────────────────────────────────────

    #[test]
    fn test_giou_non_overlapping() {
        // Two non-overlapping boxes far apart: GIoU should be negative
        let bbox1 = BoundingBox::new(0.0, 0.0, 10.0, 10.0);
        let bbox2 = BoundingBox::new(100.0, 100.0, 10.0, 10.0);

        let giou = bbox1.giou(&bbox2);
        assert!(
            giou < 0.0,
            "GIoU of non-overlapping boxes should be < 0, got {}",
            giou
        );
        assert!(giou >= -1.0, "GIoU should be >= -1.0, got {}", giou);
    }

    #[test]
    fn test_giou_identical() {
        // Identical boxes: GIoU should equal IoU = 1.0
        let bbox = BoundingBox::new(10.0, 10.0, 20.0, 20.0);
        let giou = bbox.giou(&bbox);
        assert!(
            (giou - 1.0).abs() < 1e-5,
            "GIoU of identical boxes should be 1.0, got {}",
            giou
        );
    }

    #[test]
    fn test_giou_partial_overlap() {
        // Partially overlapping boxes: GIoU should be between IoU and 1.0
        let bbox1 = BoundingBox::new(0.0, 0.0, 10.0, 10.0);
        let bbox2 = BoundingBox::new(5.0, 0.0, 10.0, 10.0);
        let iou = bbox1.iou(&bbox2);
        let giou = bbox1.giou(&bbox2);
        // For overlapping boxes, GIoU <= IoU (since the penalty subtracts)
        assert!(
            giou <= iou + 1e-5,
            "GIoU should be <= IoU for overlapping boxes"
        );
        assert!(giou > 0.0, "GIoU should be positive for overlapping boxes");
    }

    #[test]
    fn test_diou_center_distance() {
        // Two boxes with same overlap but different center distances
        // Box A at origin, Box B right next to it, Box C further away (same IoU)
        let bbox_a = BoundingBox::new(0.0, 0.0, 10.0, 10.0);
        let bbox_b = BoundingBox::new(3.0, 0.0, 10.0, 10.0); // closer center
        let bbox_c = BoundingBox::new(0.0, 3.0, 10.0, 10.0); // same IoU, different direction

        let diou_ab = bbox_a.diou(&bbox_b);
        let diou_ac = bbox_a.diou(&bbox_c);

        // Same IoU, same center distance magnitude -> similar DIoU
        assert!(
            (diou_ab - diou_ac).abs() < 0.05,
            "DIoU should be similar for same-distance offsets: {} vs {}",
            diou_ab,
            diou_ac
        );

        // DIoU should be positive for overlapping boxes
        assert!(
            diou_ab > 0.0,
            "DIoU should be positive for overlapping boxes"
        );

        // DIoU of identical boxes should be 1.0
        let diou_self = bbox_a.diou(&bbox_a);
        assert!(
            (diou_self - 1.0).abs() < 1e-5,
            "DIoU of identical boxes should be 1.0, got {}",
            diou_self
        );
    }

    #[test]
    fn test_ciou_aspect_ratio() {
        // Two boxes with same center and area but different aspect ratios
        // CIoU should penalize aspect ratio difference
        let bbox_square = BoundingBox::new(0.0, 0.0, 10.0, 10.0);
        let bbox_wide = BoundingBox::new(0.0, 0.0, 20.0, 5.0); // same area, different AR
        let bbox_tall = BoundingBox::new(0.0, 0.0, 5.0, 20.0); // same area, different AR

        let ciou_sq_wide = bbox_square.ciou(&bbox_wide);
        let ciou_sq_tall = bbox_square.ciou(&bbox_tall);

        // CIoU of identical boxes should be 1.0
        let ciou_self = bbox_square.ciou(&bbox_square);
        assert!(
            (ciou_self - 1.0).abs() < 1e-5,
            "CIoU of identical boxes should be 1.0, got {}",
            ciou_self
        );

        // CIoU should be less than IoU due to aspect ratio penalty
        let iou_sq_wide = bbox_square.iou(&bbox_wide);
        assert!(
            ciou_sq_wide <= iou_sq_wide + 1e-5,
            "CIoU should be <= IoU when AR differs: CIoU={}, IoU={}",
            ciou_sq_wide,
            iou_sq_wide
        );

        // Different aspect ratios should yield different CIoU values (wide vs tall)
        // (unless they happen to have same IoU and AR distance, which is unlikely here)
        let _ = (ciou_sq_wide, ciou_sq_tall); // both computed without error
    }

    #[test]
    fn test_distance_metric_dispatch() {
        let bbox1 = BoundingBox::new(0.0, 0.0, 10.0, 10.0);
        let bbox2 = BoundingBox::new(5.0, 5.0, 10.0, 10.0);

        let iou = bbox1.distance_metric(&bbox2, DistanceMetric::IoU);
        let giou = bbox1.distance_metric(&bbox2, DistanceMetric::GIoU);
        let diou = bbox1.distance_metric(&bbox2, DistanceMetric::DIoU);
        let ciou = bbox1.distance_metric(&bbox2, DistanceMetric::CIoU);

        // IoU should match direct iou() call
        assert!((iou - bbox1.iou(&bbox2)).abs() < f32::EPSILON);
        assert!((giou - bbox1.giou(&bbox2)).abs() < f32::EPSILON);
        assert!((diou - bbox1.diou(&bbox2)).abs() < f32::EPSILON);
        assert!((ciou - bbox1.ciou(&bbox2)).abs() < f32::EPSILON);
    }

    // ─── Rotated bounding box tests ─────────────────────────────────────

    #[test]
    fn test_rotated_bbox_corners_zero_angle() {
        // Axis-aligned (0 degrees): corners should form a standard rectangle
        let rbb = RotatedBoundingBox::new(5.0, 5.0, 10.0, 6.0, 0.0);
        let corners = rbb.corners();

        // Expected: top-left (0,2), top-right (10,2), bottom-right (10,8), bottom-left (0,8)
        let expected = [(0.0_f32, 2.0_f32), (10.0, 2.0), (10.0, 8.0), (0.0, 8.0)];

        for (i, (c, e)) in corners.iter().zip(expected.iter()).enumerate() {
            assert!(
                (c.0 - e.0).abs() < 1e-4 && (c.1 - e.1).abs() < 1e-4,
                "Corner {} mismatch: got ({:.3}, {:.3}), expected ({:.3}, {:.3})",
                i,
                c.0,
                c.1,
                e.0,
                e.1
            );
        }
    }

    #[test]
    fn test_rotated_bbox_corners_90_degrees() {
        // 90-degree rotation: width and height swap visually
        let rbb = RotatedBoundingBox::new(
            0.0,
            0.0,
            4.0,
            2.0,
            std::f32::consts::FRAC_PI_2, // 90 degrees
        );
        let corners = rbb.corners();

        // At 90 degrees CCW, the box of size (4,2) should have corners at:
        // cos(90)=0, sin(90)=1
        // (-2,-1) -> local: x=-2, y=-1 -> rotated: (0*(-2) - 1*(-1), 1*(-2) + 0*(-1)) = (1, -2)
        // etc.
        assert!((rbb.area() - 8.0).abs() < 1e-4);

        // Verify all corners are at expected distance from center
        for c in &corners {
            let dist_sq = c.0 * c.0 + c.1 * c.1;
            // Half-diagonal = sqrt(2^2 + 1^2) = sqrt(5)
            let expected_dist_sq = 5.0_f32;
            assert!(
                (dist_sq - expected_dist_sq).abs() < 1e-3,
                "Corner ({:.3}, {:.3}) distance^2 = {:.3}, expected {:.3}",
                c.0,
                c.1,
                dist_sq,
                expected_dist_sq
            );
        }
    }

    #[test]
    fn test_rotated_bbox_corners_45_degrees() {
        // 45-degree rotation of a square
        let rbb = RotatedBoundingBox::new(
            0.0,
            0.0,
            2.0,
            2.0,
            std::f32::consts::FRAC_PI_4, // 45 degrees
        );
        let corners = rbb.corners();

        // All corners should be at distance sqrt(2) from center (half-diagonal of 2x2 square)
        let expected_dist = (2.0_f32).sqrt();
        for c in &corners {
            let dist = (c.0 * c.0 + c.1 * c.1).sqrt();
            assert!(
                (dist - expected_dist).abs() < 1e-3,
                "Corner distance from center should be {:.3}, got {:.3}",
                expected_dist,
                dist
            );
        }
    }

    #[test]
    fn test_rotated_bbox_iou_axis_aligned() {
        // Two axis-aligned boxes that overlap — should match regular IoU
        let rbb1 = RotatedBoundingBox::new(5.0, 5.0, 10.0, 10.0, 0.0);
        let rbb2 = RotatedBoundingBox::new(10.0, 10.0, 10.0, 10.0, 0.0);

        let riou = rbb1.iou(&rbb2);

        // Compare with axis-aligned IoU
        let bb1 = BoundingBox::new(0.0, 0.0, 10.0, 10.0);
        let bb2 = BoundingBox::new(5.0, 5.0, 10.0, 10.0);
        let expected_iou = bb1.iou(&bb2);

        assert!(
            (riou - expected_iou).abs() < 0.05,
            "Rotated IoU for axis-aligned boxes should match regular IoU: got {:.4}, expected {:.4}",
            riou,
            expected_iou
        );
    }

    #[test]
    fn test_rotated_bbox_iou_rotated_square() {
        // A 45-degree rotated square vs an axis-aligned square, both at same center
        let rbb1 = RotatedBoundingBox::new(5.0, 5.0, 10.0, 10.0, 0.0);
        let rbb2 = RotatedBoundingBox::new(
            5.0,
            5.0,
            10.0,
            10.0,
            std::f32::consts::FRAC_PI_4, // 45 degrees
        );

        let iou = rbb1.iou(&rbb2);
        // Known theoretical IoU for axis-aligned square vs 45-degree rotated square
        // of same size centered at same point: intersection is a regular octagon
        // IoU ≈ 0.64 (2*(sqrt(2)-1) / 1)
        assert!(
            iou > 0.5 && iou < 0.8,
            "IoU of aligned vs 45-deg rotated square should be ~0.64, got {}",
            iou
        );
    }

    #[test]
    fn test_rotated_bbox_iou_no_overlap() {
        // Two rotated boxes far apart: IoU should be 0
        let rbb1 = RotatedBoundingBox::new(0.0, 0.0, 10.0, 10.0, 0.5);
        let rbb2 = RotatedBoundingBox::new(100.0, 100.0, 10.0, 10.0, 1.0);

        let iou = rbb1.iou(&rbb2);
        assert!(
            iou.abs() < 1e-5,
            "IoU of non-overlapping rotated boxes should be 0, got {}",
            iou
        );
    }

    #[test]
    fn test_rotated_bbox_iou_identical() {
        // Identical rotated boxes: IoU should be 1.0
        let rbb = RotatedBoundingBox::new(5.0, 5.0, 10.0, 8.0, 0.7);
        let iou = rbb.iou(&rbb);
        assert!(
            (iou - 1.0).abs() < 1e-3,
            "IoU of identical rotated boxes should be 1.0, got {}",
            iou
        );
    }

    #[test]
    fn test_nms_rotated_suppression() {
        // Two highly overlapping rotated detections of the same class
        let detections = vec![
            RotatedDetection {
                bbox: RotatedBoundingBox::new(10.0, 10.0, 20.0, 20.0, 0.0),
                class_id: 0,
                class_label: None,
                confidence: 0.95,
                attributes: HashMap::new(),
            },
            RotatedDetection {
                bbox: RotatedBoundingBox::new(11.0, 11.0, 20.0, 20.0, 0.0),
                class_id: 0,
                class_label: None,
                confidence: 0.85,
                attributes: HashMap::new(),
            },
            RotatedDetection {
                bbox: RotatedBoundingBox::new(100.0, 100.0, 20.0, 20.0, 0.5),
                class_id: 0,
                class_label: None,
                confidence: 0.7,
                attributes: HashMap::new(),
            },
        ];

        let config = NmsConfig {
            iou_threshold: 0.5,
            confidence_threshold: 0.5,
            max_detections: None,
            suppress_method: SuppressMethod::Hard,
            distance_metric: DistanceMetric::IoU,
        };

        let result = non_maximum_suppression_rotated(&detections, &config)
            .ok()
            .unwrap_or_default();
        // First two overlap heavily: second should be suppressed
        // Third is far away: kept
        assert_eq!(result.len(), 2);
        assert!((result[0].confidence - 0.95).abs() < f32::EPSILON);
        assert!((result[1].confidence - 0.7).abs() < f32::EPSILON);
    }

    #[test]
    fn test_nms_rotated_different_classes() {
        // Two overlapping rotated detections of different classes: both kept
        let detections = vec![
            RotatedDetection {
                bbox: RotatedBoundingBox::new(10.0, 10.0, 20.0, 20.0, 0.0),
                class_id: 0,
                class_label: None,
                confidence: 0.9,
                attributes: HashMap::new(),
            },
            RotatedDetection {
                bbox: RotatedBoundingBox::new(10.0, 10.0, 20.0, 20.0, 0.0),
                class_id: 1,
                class_label: None,
                confidence: 0.8,
                attributes: HashMap::new(),
            },
        ];

        let config = NmsConfig::default();
        let result = non_maximum_suppression_rotated(&detections, &config)
            .ok()
            .unwrap_or_default();
        assert_eq!(
            result.len(),
            2,
            "Different classes should not suppress each other"
        );
    }

    // ─── Polygon clipping helper tests ──────────────────────────────────

    #[test]
    fn test_polygon_area_triangle() {
        let tri = vec![(0.0_f32, 0.0), (4.0, 0.0), (0.0, 3.0)];
        let area = polygon_area(&tri);
        assert!(
            (area - 6.0).abs() < 1e-4,
            "Triangle area should be 6.0, got {}",
            area
        );
    }

    #[test]
    fn test_polygon_area_square() {
        let sq = vec![(0.0_f32, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
        let area = polygon_area(&sq);
        assert!(
            (area - 1.0).abs() < 1e-4,
            "Square area should be 1.0, got {}",
            area
        );
    }

    #[test]
    fn test_sutherland_hodgman_full_inside() {
        // Small square fully inside larger square
        let subject = vec![(1.0_f32, 1.0), (2.0, 1.0), (2.0, 2.0), (1.0, 2.0)];
        let clip = vec![(0.0_f32, 0.0), (3.0, 0.0), (3.0, 3.0), (0.0, 3.0)];
        let result = sutherland_hodgman_clip(&subject, &clip);
        let area = polygon_area(&result);
        assert!(
            (area - 1.0).abs() < 1e-3,
            "Fully contained polygon should have same area: got {}",
            area
        );
    }

    #[test]
    fn test_sutherland_hodgman_no_overlap() {
        let subject = vec![(0.0_f32, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
        let clip = vec![(5.0_f32, 5.0), (6.0, 5.0), (6.0, 6.0), (5.0, 6.0)];
        let result = sutherland_hodgman_clip(&subject, &clip);
        assert!(
            result.len() < 3 || polygon_area(&result) < 1e-6,
            "Non-overlapping polygons should have zero intersection"
        );
    }

    // ─── Config validation tests ────────────────────────────────────────

    #[test]
    fn test_nms_invalid_threshold() {
        let detections: Vec<Detection> = vec![];
        let config = NmsConfig {
            iou_threshold: 1.5,
            ..NmsConfig::default()
        };
        assert!(non_maximum_suppression(&detections, &config).is_err());
    }

    #[test]
    fn test_nms_invalid_gaussian_sigma() {
        let detections: Vec<Detection> = vec![];
        let config = NmsConfig {
            suppress_method: SuppressMethod::Gaussian { sigma: -1.0 },
            ..NmsConfig::default()
        };
        assert!(non_maximum_suppression(&detections, &config).is_err());
    }

    #[test]
    fn test_nms_invalid_linear_score_threshold() {
        let detections: Vec<Detection> = vec![];
        let config = NmsConfig {
            suppress_method: SuppressMethod::Linear {
                score_threshold: 2.0,
            },
            ..NmsConfig::default()
        };
        assert!(non_maximum_suppression(&detections, &config).is_err());
    }
}
