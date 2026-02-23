//! Image segmentation for geospatial data
//!
//! This module provides semantic, instance, and panoptic segmentation capabilities.

use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{GeoTransform, RasterDataType};
// use rayon::prelude::*;
use geo_types::{Coord, LineString, Polygon};
use geojson::{Feature, FeatureCollection, Geometry, Value as GeoJsonValue};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::debug;

use crate::error::{PostprocessingError, Result};

/// Segmentation mask
#[derive(Debug, Clone)]
pub struct SegmentationMask {
    /// The mask buffer (class IDs as integers)
    pub mask: RasterBuffer,
    /// Number of classes
    pub num_classes: usize,
    /// Class labels
    pub class_labels: Option<Vec<String>>,
}

/// Instance segmentation result
#[derive(Debug, Clone)]
pub struct InstanceSegmentation {
    /// Instance mask (instance IDs as integers)
    pub instances: RasterBuffer,
    /// Class for each instance
    pub instance_classes: HashMap<u32, usize>,
    /// Confidence for each instance
    pub instance_scores: HashMap<u32, f32>,
}

/// Panoptic segmentation result
#[derive(Debug, Clone)]
pub struct PanopticSegmentation {
    /// Panoptic mask (panoptic IDs as integers)
    pub mask: RasterBuffer,
    /// Segments information
    pub segments: Vec<PanopticSegment>,
}

/// A panoptic segment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanopticSegment {
    /// Segment ID
    pub id: u32,
    /// Class ID
    pub class_id: usize,
    /// Whether this is a "thing" (object instance) or "stuff" (background class)
    pub is_thing: bool,
    /// Pixel count
    pub pixel_count: u64,
    /// Confidence score
    pub score: f32,
}

/// COCO panoptic format annotation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CocoAnnotation {
    /// Segment ID
    pub id: u32,
    /// Category ID
    pub category_id: usize,
    /// RLE (Run-Length Encoding) of the mask
    pub segmentation: CocoRLE,
    /// Bounding box [x, y, width, height]
    pub bbox: [f64; 4],
    /// Area in pixels
    pub area: u64,
    /// Confidence score
    pub score: f32,
    /// Whether this is crowd
    pub iscrowd: u8,
}

/// Run-Length Encoding for COCO format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CocoRLE {
    /// Counts array (alternating between 0 and 1)
    pub counts: Vec<u32>,
    /// Size [height, width]
    pub size: [u64; 2],
}

/// Converts a probability map to a segmentation mask
///
/// # Errors
/// Returns an error if conversion fails
pub fn probability_to_mask(
    probabilities: &RasterBuffer,
    num_classes: usize,
    threshold: f32,
) -> Result<SegmentationMask> {
    if !(0.0..=1.0).contains(&threshold) {
        return Err(PostprocessingError::InvalidThreshold { value: threshold }.into());
    }

    debug!(
        "Converting {}x{} probability map to mask with {} classes",
        probabilities.width(),
        probabilities.height(),
        num_classes
    );

    let width = probabilities.width();
    let height = probabilities.height();

    // Create mask buffer (using UInt16 to support up to 65535 classes)
    let mut mask = RasterBuffer::zeros(width, height, RasterDataType::UInt16);

    // For each pixel, assign the class with highest probability
    for y in 0..height {
        for x in 0..width {
            let prob = probabilities.get_pixel(x, y).map_err(|e| {
                PostprocessingError::PolygonConversionFailed {
                    reason: format!("Failed to get probability: {}", e),
                }
            })?;

            // Threshold probability
            let class_id = if prob >= threshold as f64 {
                // In a real implementation, this would handle multi-channel probability maps
                1 // Foreground class
            } else {
                0 // Background class
            };

            mask.set_pixel(x, y, class_id as f64).map_err(|e| {
                PostprocessingError::PolygonConversionFailed {
                    reason: format!("Failed to set class: {}", e),
                }
            })?;
        }
    }

    Ok(SegmentationMask {
        mask,
        num_classes,
        class_labels: None,
    })
}

/// Applies morphological operations to clean up segmentation masks
///
/// # Errors
/// Returns an error if morphological operations fail
pub fn morphological_closing(mask: &RasterBuffer, kernel_size: u64) -> Result<RasterBuffer> {
    // Dilation followed by erosion
    let dilated = morphological_dilate(mask, kernel_size)?;
    morphological_erode(&dilated, kernel_size)
}

/// Applies morphological dilation
///
/// # Errors
/// Returns an error if dilation fails
pub fn morphological_dilate(mask: &RasterBuffer, kernel_size: u64) -> Result<RasterBuffer> {
    if kernel_size == 0 {
        return Ok(mask.clone());
    }

    let width = mask.width();
    let height = mask.height();
    let mut result = mask.clone();

    let radius = kernel_size / 2;

    for y in 0..height {
        for x in 0..width {
            let mut max_value: f32 = 0.0;

            // Check neighborhood
            for dy in 0..kernel_size {
                for dx in 0..kernel_size {
                    let ny = y.saturating_add(dy).saturating_sub(radius);
                    let nx = x.saturating_add(dx).saturating_sub(radius);

                    if nx < width && ny < height {
                        let value = mask.get_pixel(nx, ny).map_err(|e| {
                            PostprocessingError::MergingFailed {
                                reason: format!("Failed to get pixel: {}", e),
                            }
                        })?;
                        max_value = max_value.max(value as f32);
                    }
                }
            }

            result.set_pixel(x, y, max_value as f64).map_err(|e| {
                PostprocessingError::MergingFailed {
                    reason: format!("Failed to set pixel: {}", e),
                }
            })?;
        }
    }

    Ok(result)
}

/// Applies morphological erosion
///
/// # Errors
/// Returns an error if erosion fails
pub fn morphological_erode(mask: &RasterBuffer, kernel_size: u64) -> Result<RasterBuffer> {
    if kernel_size == 0 {
        return Ok(mask.clone());
    }

    let width = mask.width();
    let height = mask.height();
    let mut result = mask.clone();

    let radius = kernel_size / 2;

    for y in 0..height {
        for x in 0..width {
            let mut min_value = f64::MAX;

            // Check neighborhood
            for dy in 0..kernel_size {
                for dx in 0..kernel_size {
                    let ny = y.saturating_add(dy).saturating_sub(radius);
                    let nx = x.saturating_add(dx).saturating_sub(radius);

                    if nx < width && ny < height {
                        let value = mask.get_pixel(nx, ny).map_err(|e| {
                            PostprocessingError::MergingFailed {
                                reason: format!("Failed to get pixel: {}", e),
                            }
                        })?;
                        min_value = min_value.min(value);
                    }
                }
            }

            result
                .set_pixel(x, y, min_value)
                .map_err(|e| PostprocessingError::MergingFailed {
                    reason: format!("Failed to set pixel: {}", e),
                })?;
        }
    }

    Ok(result)
}

/// Finds connected components in a binary mask
///
/// # Errors
/// Returns an error if component labeling fails
pub fn find_connected_components(
    mask: &RasterBuffer,
    min_size: u64,
) -> Result<InstanceSegmentation> {
    debug!(
        "Finding connected components in {}x{} mask",
        mask.width(),
        mask.height()
    );

    let width = mask.width();
    let height = mask.height();

    let mut labels = RasterBuffer::zeros(width, height, RasterDataType::UInt32);
    let mut next_label = 1u32;
    let mut instance_classes = HashMap::new();
    let mut instance_scores = HashMap::new();
    let mut instance_sizes: HashMap<u32, u64> = HashMap::new();

    // Simple flood-fill based connected components
    for y in 0..height {
        for x in 0..width {
            let value = mask
                .get_pixel(x, y)
                .map_err(|e| PostprocessingError::MergingFailed {
                    reason: format!("Failed to get pixel: {}", e),
                })?;

            let current_label =
                labels
                    .get_pixel(x, y)
                    .map_err(|e| PostprocessingError::MergingFailed {
                        reason: format!("Failed to get label: {}", e),
                    })?;

            // If pixel is foreground and not yet labeled
            if value > 0.0 && current_label == 0.0 {
                let label = next_label;
                let size = flood_fill(&mut labels, x, y, label as f64, mask)?;

                if size >= min_size {
                    instance_classes.insert(label, value as usize);
                    instance_scores.insert(label, 1.0);
                    instance_sizes.insert(label, size);
                    next_label += 1;
                } else {
                    // Remove small components
                    remove_component(&mut labels, label as f64)?;
                }
            }
        }
    }

    debug!("Found {} instances", instance_classes.len());

    Ok(InstanceSegmentation {
        instances: labels,
        instance_classes,
        instance_scores,
    })
}

/// Flood fill algorithm for connected components
fn flood_fill(
    labels: &mut RasterBuffer,
    start_x: u64,
    start_y: u64,
    label: f64,
    mask: &RasterBuffer,
) -> Result<u64> {
    let width = labels.width();
    let height = labels.height();
    let mut stack = vec![(start_x, start_y)];
    let mut size = 0u64;

    while let Some((x, y)) = stack.pop() {
        if x >= width || y >= height {
            continue;
        }

        let current_label =
            labels
                .get_pixel(x, y)
                .map_err(|e| PostprocessingError::MergingFailed {
                    reason: format!("Failed to get label: {}", e),
                })?;

        let mask_value = mask
            .get_pixel(x, y)
            .map_err(|e| PostprocessingError::MergingFailed {
                reason: format!("Failed to get mask value: {}", e),
            })?;

        if current_label == 0.0 && mask_value > 0.0 {
            labels
                .set_pixel(x, y, label)
                .map_err(|e| PostprocessingError::MergingFailed {
                    reason: format!("Failed to set label: {}", e),
                })?;
            size += 1;

            // Add neighbors to stack
            if x > 0 {
                stack.push((x - 1, y));
            }
            if x + 1 < width {
                stack.push((x + 1, y));
            }
            if y > 0 {
                stack.push((x, y - 1));
            }
            if y + 1 < height {
                stack.push((x, y + 1));
            }
        }
    }

    Ok(size)
}

/// Removes a component from the label map
fn remove_component(labels: &mut RasterBuffer, label: f64) -> Result<()> {
    for y in 0..labels.height() {
        for x in 0..labels.width() {
            let current =
                labels
                    .get_pixel(x, y)
                    .map_err(|e| PostprocessingError::MergingFailed {
                        reason: format!("Failed to get label: {}", e),
                    })?;

            if (current - label).abs() < f64::EPSILON {
                labels
                    .set_pixel(x, y, 0.0)
                    .map_err(|e| PostprocessingError::MergingFailed {
                        reason: format!("Failed to set label: {}", e),
                    })?;
            }
        }
    }

    Ok(())
}

/// Computes class statistics from a segmentation mask
///
/// # Errors
/// Returns an error if computation fails
pub fn compute_class_statistics(
    mask: &SegmentationMask,
) -> Result<HashMap<usize, ClassStatistics>> {
    let mut stats: HashMap<usize, ClassStatistics> = HashMap::new();

    for y in 0..mask.mask.height() {
        for x in 0..mask.mask.width() {
            let class_id =
                mask.mask
                    .get_pixel(x, y)
                    .map_err(|e| PostprocessingError::MergingFailed {
                        reason: format!("Failed to get class: {}", e),
                    })? as usize;

            let stat = stats.entry(class_id).or_insert_with(|| ClassStatistics {
                class_id,
                pixel_count: 0,
                percentage: 0.0,
            });

            stat.pixel_count += 1;
        }
    }

    // Compute percentages
    let total_pixels = (mask.mask.width() * mask.mask.height()) as f64;
    for stat in stats.values_mut() {
        stat.percentage = (stat.pixel_count as f64 / total_pixels) * 100.0;
    }

    Ok(stats)
}

/// Class statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassStatistics {
    /// Class ID
    pub class_id: usize,
    /// Number of pixels
    pub pixel_count: u64,
    /// Percentage of total pixels
    pub percentage: f64,
}

impl PanopticSegmentation {
    /// Creates panoptic segmentation from semantic and instance predictions
    ///
    /// # Arguments
    /// * `semantic_logits` - Semantic segmentation logits (argmax for classes)
    /// * `instance_heatmap` - Instance heatmap for detecting object instances
    /// * `num_classes` - Number of semantic classes
    /// * `thing_classes` - List of class IDs that are "things" (countable objects)
    /// * `min_instance_size` - Minimum instance size in pixels
    /// * `nms_threshold` - IoU threshold for non-maximum suppression
    ///
    /// # Errors
    /// Returns an error if segmentation creation fails
    pub fn from_predictions(
        semantic_logits: &RasterBuffer,
        instance_heatmap: &RasterBuffer,
        num_classes: usize,
        thing_classes: &[usize],
        min_instance_size: u64,
        nms_threshold: f32,
    ) -> Result<Self> {
        debug!(
            "Creating panoptic segmentation from semantic ({}x{}) and instance predictions",
            semantic_logits.width(),
            semantic_logits.height()
        );

        let width = semantic_logits.width();
        let height = semantic_logits.height();

        // Get semantic classes via argmax (simplified: using values directly)
        let mut semantic_mask = RasterBuffer::zeros(width, height, RasterDataType::UInt16);
        for y in 0..height {
            for x in 0..width {
                let logit = semantic_logits.get_pixel(x, y).map_err(|e| {
                    PostprocessingError::MergingFailed {
                        reason: format!("Failed to get semantic logit: {}", e),
                    }
                })?;
                // In practice, this would be argmax over channels
                let class_id = (logit.max(0.0) * (num_classes as f64 - 1.0)) as u16;
                semantic_mask
                    .set_pixel(x, y, class_id as f64)
                    .map_err(|e| PostprocessingError::MergingFailed {
                        reason: format!("Failed to set semantic class: {}", e),
                    })?;
            }
        }

        // Extract instances from heatmap
        let instances = extract_instances(instance_heatmap, min_instance_size)?;

        // Apply NMS to instances
        let filtered_instances = nms_instances(&instances, nms_threshold)?;

        // Merge semantic and instance segmentations
        let mut panoptic_mask = RasterBuffer::zeros(width, height, RasterDataType::UInt32);
        let mut segments = Vec::new();
        let mut next_id = 1u32;

        // First, add "stuff" classes (non-thing classes)
        let mut stuff_pixel_counts: HashMap<usize, u64> = HashMap::new();
        for y in 0..height {
            for x in 0..width {
                let class_id = semantic_mask.get_pixel(x, y).map_err(|e| {
                    PostprocessingError::MergingFailed {
                        reason: format!("Failed to get semantic class: {}", e),
                    }
                })? as usize;

                if !thing_classes.contains(&class_id) {
                    // This is a stuff class
                    panoptic_mask
                        .set_pixel(x, y, class_id as f64)
                        .map_err(|e| PostprocessingError::MergingFailed {
                            reason: format!("Failed to set panoptic ID: {}", e),
                        })?;
                    *stuff_pixel_counts.entry(class_id).or_insert(0) += 1;
                }
            }
        }

        // Add stuff segments
        for (class_id, count) in stuff_pixel_counts {
            segments.push(PanopticSegment {
                id: class_id as u32,
                class_id,
                is_thing: false,
                pixel_count: count,
                score: 1.0,
            });
        }

        // Add "thing" instances
        for (instance_id, class_id) in &filtered_instances.instance_classes {
            let score = filtered_instances
                .instance_scores
                .get(instance_id)
                .copied()
                .unwrap_or(1.0);

            let mut pixel_count = 0u64;

            // Assign panoptic ID to instance pixels
            for y in 0..height {
                for x in 0..width {
                    let inst_id = filtered_instances.instances.get_pixel(x, y).map_err(|e| {
                        PostprocessingError::MergingFailed {
                            reason: format!("Failed to get instance ID: {}", e),
                        }
                    })? as u32;

                    if inst_id == *instance_id {
                        panoptic_mask.set_pixel(x, y, next_id as f64).map_err(|e| {
                            PostprocessingError::MergingFailed {
                                reason: format!("Failed to set panoptic ID: {}", e),
                            }
                        })?;
                        pixel_count += 1;
                    }
                }
            }

            segments.push(PanopticSegment {
                id: next_id,
                class_id: *class_id,
                is_thing: true,
                pixel_count,
                score,
            });

            next_id += 1;
        }

        debug!(
            "Created panoptic segmentation with {} segments",
            segments.len()
        );

        Ok(Self {
            mask: panoptic_mask,
            segments,
        })
    }

    /// Exports panoptic segmentation to COCO format
    ///
    /// # Errors
    /// Returns an error if export fails
    pub fn to_coco_format(&self) -> Result<Vec<CocoAnnotation>> {
        debug!("Converting {} segments to COCO format", self.segments.len());

        let mut annotations = Vec::new();

        for segment in &self.segments {
            // Extract binary mask for this segment
            let binary_mask = extract_segment_mask(&self.mask, segment.id)?;

            // Compute RLE
            let rle = compute_rle(&binary_mask)?;

            // Compute bounding box
            let bbox = compute_bbox(&binary_mask)?;

            annotations.push(CocoAnnotation {
                id: segment.id,
                category_id: segment.class_id,
                segmentation: rle,
                bbox,
                area: segment.pixel_count,
                score: segment.score,
                iscrowd: 0,
            });
        }

        Ok(annotations)
    }

    /// Exports panoptic segmentation to GeoJSON with geographic coordinates
    ///
    /// # Arguments
    /// * `geo_transform` - Geographic transformation for pixel to world coordinates
    ///
    /// # Errors
    /// Returns an error if export fails
    pub fn to_geojson(&self, geo_transform: &GeoTransform) -> Result<FeatureCollection> {
        debug!(
            "Converting {} segments to GeoJSON with geo transform",
            self.segments.len()
        );

        let mut features = Vec::new();

        for segment in &self.segments {
            // Extract binary mask for this segment
            let binary_mask = extract_segment_mask(&self.mask, segment.id)?;

            // Convert mask to polygon
            let polygon = mask_to_polygon(&binary_mask, geo_transform)?;

            // Calculate area in square meters
            let area_m2 = calculate_area(&polygon, geo_transform)?;

            // Create GeoJSON geometry
            let coords = polygon_to_geojson_coords(&polygon);
            let geometry = Geometry::new(GeoJsonValue::Polygon(coords));

            // Create properties
            let mut properties = serde_json::Map::new();
            properties.insert(
                "instance_id".to_string(),
                serde_json::Value::Number(segment.id.into()),
            );
            properties.insert(
                "class_id".to_string(),
                serde_json::Value::Number(segment.class_id.into()),
            );
            properties.insert(
                "is_thing".to_string(),
                serde_json::Value::Bool(segment.is_thing),
            );
            properties.insert(
                "pixel_count".to_string(),
                serde_json::Value::Number(segment.pixel_count.into()),
            );
            properties.insert("confidence".to_string(), serde_json::json!(segment.score));
            properties.insert("area_m2".to_string(), serde_json::json!(area_m2));

            let feature = Feature {
                bbox: None,
                geometry: Some(geometry),
                id: Some(geojson::feature::Id::Number(segment.id.into())),
                properties: Some(properties),
                foreign_members: None,
            };

            features.push(feature);
        }

        Ok(FeatureCollection {
            bbox: None,
            features,
            foreign_members: None,
        })
    }
}

/// Extracts instances from heatmap using connected components
///
/// # Errors
/// Returns an error if extraction fails
fn extract_instances(heatmap: &RasterBuffer, min_size: u64) -> Result<InstanceSegmentation> {
    // Threshold heatmap to get binary mask
    let threshold = 0.5;
    let mut binary_mask =
        RasterBuffer::zeros(heatmap.width(), heatmap.height(), RasterDataType::UInt8);

    for y in 0..heatmap.height() {
        for x in 0..heatmap.width() {
            let value =
                heatmap
                    .get_pixel(x, y)
                    .map_err(|e| PostprocessingError::MergingFailed {
                        reason: format!("Failed to get heatmap value: {}", e),
                    })?;

            if value > threshold {
                binary_mask.set_pixel(x, y, 1.0).map_err(|e| {
                    PostprocessingError::MergingFailed {
                        reason: format!("Failed to set binary mask: {}", e),
                    }
                })?;
            }
        }
    }

    // Find connected components
    find_connected_components(&binary_mask, min_size)
}

/// Applies non-maximum suppression to instances
///
/// # Errors
/// Returns an error if NMS fails
fn nms_instances(
    instances: &InstanceSegmentation,
    iou_threshold: f32,
) -> Result<InstanceSegmentation> {
    if !(0.0..=1.0).contains(&iou_threshold) {
        return Err(PostprocessingError::NmsFailed {
            reason: format!("Invalid IoU threshold: {}", iou_threshold),
        }
        .into());
    }

    let mut keep_instances: HashMap<u32, usize> = HashMap::new();
    let mut keep_scores: HashMap<u32, f32> = HashMap::new();

    // Sort instances by score (descending)
    let mut sorted_instances: Vec<_> = instances.instance_scores.iter().collect();
    sorted_instances.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));

    for (instance_id, score) in sorted_instances {
        let mut suppressed = false;

        // Check IoU with already kept instances
        for kept_id in keep_instances.keys() {
            let iou = compute_instance_iou(&instances.instances, *instance_id, *kept_id)?;

            if iou > iou_threshold {
                suppressed = true;
                break;
            }
        }

        if !suppressed {
            if let Some(&class_id) = instances.instance_classes.get(instance_id) {
                keep_instances.insert(*instance_id, class_id);
                keep_scores.insert(*instance_id, *score);
            }
        }
    }

    // Create new instance segmentation with only kept instances
    let mut filtered_instances = RasterBuffer::zeros(
        instances.instances.width(),
        instances.instances.height(),
        RasterDataType::UInt32,
    );

    for y in 0..instances.instances.height() {
        for x in 0..instances.instances.width() {
            let inst_id =
                instances
                    .instances
                    .get_pixel(x, y)
                    .map_err(|e| PostprocessingError::NmsFailed {
                        reason: format!("Failed to get instance ID: {}", e),
                    })? as u32;

            if keep_instances.contains_key(&inst_id) {
                filtered_instances
                    .set_pixel(x, y, inst_id as f64)
                    .map_err(|e| PostprocessingError::NmsFailed {
                        reason: format!("Failed to set filtered instance: {}", e),
                    })?;
            }
        }
    }

    Ok(InstanceSegmentation {
        instances: filtered_instances,
        instance_classes: keep_instances,
        instance_scores: keep_scores,
    })
}

/// Computes IoU between two instances
fn compute_instance_iou(instances: &RasterBuffer, id1: u32, id2: u32) -> Result<f32> {
    let mut area1 = 0u64;
    let mut area2 = 0u64;
    let mut intersection = 0u64;

    for y in 0..instances.height() {
        for x in 0..instances.width() {
            let inst_id = instances
                .get_pixel(x, y)
                .map_err(|e| PostprocessingError::NmsFailed {
                    reason: format!("Failed to get instance ID: {}", e),
                })? as u32;

            if inst_id == id1 {
                area1 += 1;
                if id1 == id2 {
                    intersection += 1;
                }
            }
            if inst_id == id2 {
                area2 += 1;
                if id1 != id2 && inst_id == id1 {
                    intersection += 1;
                }
            }
        }
    }

    let union = area1 + area2 - intersection;
    if union == 0 {
        return Ok(0.0);
    }

    Ok(intersection as f32 / union as f32)
}

/// Extracts a binary mask for a specific segment
fn extract_segment_mask(mask: &RasterBuffer, segment_id: u32) -> Result<RasterBuffer> {
    let mut binary = RasterBuffer::zeros(mask.width(), mask.height(), RasterDataType::UInt8);

    for y in 0..mask.height() {
        for x in 0..mask.width() {
            let id =
                mask.get_pixel(x, y)
                    .map_err(|e| PostprocessingError::PolygonConversionFailed {
                        reason: format!("Failed to get segment ID: {}", e),
                    })? as u32;

            if id == segment_id {
                binary.set_pixel(x, y, 1.0).map_err(|e| {
                    PostprocessingError::PolygonConversionFailed {
                        reason: format!("Failed to set binary mask: {}", e),
                    }
                })?;
            }
        }
    }

    Ok(binary)
}

/// Computes Run-Length Encoding of a binary mask
fn compute_rle(mask: &RasterBuffer) -> Result<CocoRLE> {
    let width = mask.width();
    let height = mask.height();
    let mut counts = Vec::new();
    let mut current_value = 0u8;
    let mut current_count = 0u32;

    // Scan in row-major order
    for y in 0..height {
        for x in 0..width {
            let value = mask
                .get_pixel(x, y)
                .map_err(|e| PostprocessingError::ExportFailed {
                    reason: format!("Failed to get mask value: {}", e),
                })? as u8;

            if value == current_value {
                current_count += 1;
            } else {
                if current_count > 0 {
                    counts.push(current_count);
                }
                current_value = value;
                current_count = 1;
            }
        }
    }

    // Push final count
    if current_count > 0 {
        counts.push(current_count);
    }

    Ok(CocoRLE {
        counts,
        size: [height, width],
    })
}

/// Computes bounding box of a binary mask
fn compute_bbox(mask: &RasterBuffer) -> Result<[f64; 4]> {
    let width = mask.width();
    let height = mask.height();

    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = 0u64;
    let mut max_y = 0u64;

    for y in 0..height {
        for x in 0..width {
            let value = mask
                .get_pixel(x, y)
                .map_err(|e| PostprocessingError::ExportFailed {
                    reason: format!("Failed to get mask value: {}", e),
                })? as u8;

            if value > 0 {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }

    if min_x > max_x {
        // Empty mask
        return Ok([0.0, 0.0, 0.0, 0.0]);
    }

    Ok([
        min_x as f64,
        min_y as f64,
        (max_x - min_x + 1) as f64,
        (max_y - min_y + 1) as f64,
    ])
}

/// Converts a binary mask to a polygon with geographic coordinates
fn mask_to_polygon(mask: &RasterBuffer, geo_transform: &GeoTransform) -> Result<Polygon<f64>> {
    // Find contour of the mask using a simple boundary tracing algorithm
    let boundary_points = trace_boundary(mask)?;

    if boundary_points.is_empty() {
        return Err(PostprocessingError::PolygonConversionFailed {
            reason: "No boundary points found".to_string(),
        }
        .into());
    }

    // Convert pixel coordinates to geographic coordinates
    let mut geo_coords = Vec::new();
    for (x, y) in boundary_points {
        let (geo_x, geo_y) = geo_transform.pixel_to_world(x as f64, y as f64);
        geo_coords.push(Coord { x: geo_x, y: geo_y });
    }

    // Close the polygon
    if let Some(first) = geo_coords.first() {
        geo_coords.push(*first);
    }

    Ok(Polygon::new(LineString::from(geo_coords), vec![]))
}

/// Traces the boundary of a binary mask
fn trace_boundary(mask: &RasterBuffer) -> Result<Vec<(u64, u64)>> {
    let width = mask.width();
    let height = mask.height();
    let mut boundary = Vec::new();

    // Simple boundary detection: find pixels that have at least one background neighbor
    for y in 0..height {
        for x in 0..width {
            let value =
                mask.get_pixel(x, y)
                    .map_err(|e| PostprocessingError::PolygonConversionFailed {
                        reason: format!("Failed to get mask value: {}", e),
                    })? as u8;

            if value > 0 {
                // Check if this is a boundary pixel
                let mut is_boundary = false;

                // Check 4-connected neighbors
                for (dx, dy) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)] {
                    let nx = x as i64 + dx;
                    let ny = y as i64 + dy;

                    if nx < 0 || ny < 0 || nx >= width as i64 || ny >= height as i64 {
                        is_boundary = true;
                        break;
                    }

                    let neighbor_value = mask.get_pixel(nx as u64, ny as u64).map_err(|e| {
                        PostprocessingError::PolygonConversionFailed {
                            reason: format!("Failed to get neighbor value: {}", e),
                        }
                    })? as u8;

                    if neighbor_value == 0 {
                        is_boundary = true;
                        break;
                    }
                }

                if is_boundary {
                    boundary.push((x, y));
                }
            }
        }
    }

    Ok(boundary)
}

/// Calculates geographic area of a polygon in square meters
fn calculate_area(polygon: &Polygon<f64>, geo_transform: &GeoTransform) -> Result<f64> {
    // Use the Shoelace formula for polygon area
    let exterior = polygon.exterior();
    let coords: Vec<_> = exterior.coords().collect();

    if coords.len() < 3 {
        return Ok(0.0);
    }

    let mut area = 0.0;
    for i in 0..coords.len() - 1 {
        area += coords[i].x * coords[i + 1].y - coords[i + 1].x * coords[i].y;
    }
    area = (area / 2.0).abs();

    // Convert to square meters using pixel resolution
    let (pixel_width, pixel_height) = geo_transform.resolution();
    let area_m2 = area * pixel_width * pixel_height;

    Ok(area_m2)
}

/// Converts a Polygon to GeoJSON coordinate format
fn polygon_to_geojson_coords(polygon: &Polygon<f64>) -> Vec<Vec<Vec<f64>>> {
    let exterior: Vec<Vec<f64>> = polygon
        .exterior()
        .coords()
        .map(|coord| vec![coord.x, coord.y])
        .collect();

    vec![exterior]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_probability_to_mask() {
        let probs = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        let mask = probability_to_mask(&probs, 2, 0.5);
        assert!(mask.is_ok());
    }

    #[test]
    fn test_morphological_operations() {
        let mask = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        let dilated = morphological_dilate(&mask, 3);
        assert!(dilated.is_ok());

        let eroded = morphological_erode(&mask, 3);
        assert!(eroded.is_ok());

        let closed = morphological_closing(&mask, 3);
        assert!(closed.is_ok());
    }

    #[test]
    fn test_find_connected_components() {
        let mut mask = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        // Set some pixels to create a component
        let _ = mask.set_pixel(5, 5, 1.0);
        let _ = mask.set_pixel(5, 6, 1.0);

        let result = find_connected_components(&mask, 1);
        assert!(result.is_ok());
    }

    #[test]
    fn test_panoptic_from_predictions() {
        let semantic = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        let instance = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        let result =
            PanopticSegmentation::from_predictions(&semantic, &instance, 5, &[1, 2], 10, 0.5);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compute_rle() {
        let mut mask = RasterBuffer::zeros(4, 4, RasterDataType::UInt8);
        let _ = mask.set_pixel(0, 0, 1.0);
        let _ = mask.set_pixel(1, 0, 1.0);

        let rle = compute_rle(&mask);
        assert!(rle.is_ok());
        let rle = rle.expect("RLE computation failed");
        assert_eq!(rle.size, [4, 4]);
        assert!(!rle.counts.is_empty());
    }

    #[test]
    fn test_compute_bbox() {
        let mut mask = RasterBuffer::zeros(10, 10, RasterDataType::UInt8);
        let _ = mask.set_pixel(2, 3, 1.0);
        let _ = mask.set_pixel(5, 7, 1.0);

        let bbox = compute_bbox(&mask);
        assert!(bbox.is_ok());
        let bbox = bbox.expect("BBox computation failed");
        assert_eq!(bbox[0], 2.0); // min_x
        assert_eq!(bbox[1], 3.0); // min_y
        assert_eq!(bbox[2], 4.0); // width
        assert_eq!(bbox[3], 5.0); // height
    }

    #[test]
    fn test_extract_segment_mask() {
        let mut mask = RasterBuffer::zeros(10, 10, RasterDataType::UInt32);
        let _ = mask.set_pixel(5, 5, 42.0);
        let _ = mask.set_pixel(5, 6, 42.0);
        let _ = mask.set_pixel(3, 3, 1.0);

        let binary = extract_segment_mask(&mask, 42);
        assert!(binary.is_ok());
        let binary = binary.expect("Extract failed");

        // Check that only pixels with ID 42 are set
        assert_eq!(binary.get_pixel(5, 5).ok(), Some(1.0));
        assert_eq!(binary.get_pixel(5, 6).ok(), Some(1.0));
        assert_eq!(binary.get_pixel(3, 3).ok(), Some(0.0));
    }

    #[test]
    fn test_nms_instances_validation() {
        let instances = RasterBuffer::zeros(10, 10, RasterDataType::UInt32);
        let instance_seg = InstanceSegmentation {
            instances,
            instance_classes: HashMap::new(),
            instance_scores: HashMap::new(),
        };

        // Test invalid threshold
        let result = nms_instances(&instance_seg, -0.1);
        assert!(result.is_err());

        let result = nms_instances(&instance_seg, 1.5);
        assert!(result.is_err());

        // Test valid threshold
        let result = nms_instances(&instance_seg, 0.5);
        assert!(result.is_ok());
    }

    #[test]
    fn test_mask_to_polygon() {
        let mut mask = RasterBuffer::zeros(5, 5, RasterDataType::UInt8);
        // Create a small square
        let _ = mask.set_pixel(1, 1, 1.0);
        let _ = mask.set_pixel(2, 1, 1.0);
        let _ = mask.set_pixel(1, 2, 1.0);
        let _ = mask.set_pixel(2, 2, 1.0);

        let geo_transform = GeoTransform::north_up(0.0, 0.0, 1.0, -1.0);
        let result = mask_to_polygon(&mask, &geo_transform);
        assert!(result.is_ok());
    }

    #[test]
    fn test_calculate_area() {
        use geo_types::{Coord, LineString, Polygon};

        // Create a simple polygon (1x1 square in pixel space)
        let coords = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 0.0, y: 1.0 },
            Coord { x: 0.0, y: 0.0 },
        ];
        let polygon = Polygon::new(LineString::from(coords), vec![]);

        // 1 degree per pixel
        let geo_transform = GeoTransform::north_up(0.0, 0.0, 1.0, -1.0);
        let area = calculate_area(&polygon, &geo_transform);
        assert!(area.is_ok());
        let area_val = area.expect("Area calculation failed");
        assert!(area_val > 0.0);
    }
}
