//! Image classification for geospatial data
//!
//! This module provides scene classification, land cover classification,
//! and multi-label classification capabilities.

use oxigdal_core::buffer::RasterBuffer;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::debug;

use crate::error::{PostprocessingError, Result};

/// Classification result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassificationResult {
    /// Predicted class ID
    pub class_id: usize,
    /// Class label
    pub class_label: Option<String>,
    /// Confidence score (0.0 to 1.0)
    pub confidence: f32,
    /// All class probabilities
    pub probabilities: HashMap<usize, f32>,
}

/// Multi-label classification result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiLabelResult {
    /// Predicted labels with confidence scores
    pub labels: Vec<LabelPrediction>,
    /// Threshold used for predictions
    pub threshold: f32,
}

/// A single label prediction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelPrediction {
    /// Class ID
    pub class_id: usize,
    /// Class label
    pub class_label: Option<String>,
    /// Confidence score
    pub confidence: f32,
}

/// Land cover classification result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LandCoverResult {
    /// Classification result
    pub classification: ClassificationResult,
    /// Land cover type
    pub land_cover_type: LandCoverType,
}

/// Land cover types (simplified taxonomy)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LandCoverType {
    /// Water bodies
    Water,
    /// Developed/urban areas
    Developed,
    /// Barren land
    Barren,
    /// Forest
    Forest,
    /// Shrubland
    Shrubland,
    /// Herbaceous/grassland
    Herbaceous,
    /// Planted/cultivated
    Cultivated,
    /// Wetlands
    Wetlands,
    /// Ice/snow
    IceSnow,
    /// Unknown/other
    Unknown,
}

/// Performs single-label classification from probability buffer
///
/// # Errors
/// Returns an error if classification fails
pub fn classify_single_label(
    probabilities: &RasterBuffer,
    class_labels: Option<&[String]>,
    confidence_threshold: f32,
) -> Result<ClassificationResult> {
    if !(0.0..=1.0).contains(&confidence_threshold) {
        return Err(PostprocessingError::InvalidThreshold {
            value: confidence_threshold,
        }
        .into());
    }

    debug!(
        "Classifying image with {} classes",
        probabilities.pixel_count()
    );

    // For single-label classification, we expect a 1D probability vector
    // This is a simplified implementation
    let num_pixels = probabilities.pixel_count();

    let mut max_prob = 0.0f32;
    let mut max_class = 0usize;
    let mut prob_map = HashMap::new();

    // Collect probabilities
    // In a real implementation, this would properly handle the probability tensor
    for i in 0..num_pixels.min(1000) {
        let x = i % probabilities.width();
        let y = i / probabilities.width();

        let prob = probabilities
            .get_pixel(x, y)
            .map_err(|e| PostprocessingError::ExportFailed {
                reason: format!("Failed to get probability: {}", e),
            })? as f32;

        prob_map.insert(i as usize, prob);

        if prob > max_prob {
            max_prob = prob;
            max_class = i as usize;
        }
    }

    let class_label = class_labels.and_then(|labels| labels.get(max_class).cloned());

    Ok(ClassificationResult {
        class_id: max_class,
        class_label,
        confidence: max_prob,
        probabilities: prob_map,
    })
}

/// Performs multi-label classification
///
/// # Errors
/// Returns an error if classification fails
pub fn classify_multi_label(
    probabilities: &RasterBuffer,
    class_labels: Option<&[String]>,
    threshold: f32,
) -> Result<MultiLabelResult> {
    if !(0.0..=1.0).contains(&threshold) {
        return Err(PostprocessingError::InvalidThreshold { value: threshold }.into());
    }

    debug!("Multi-label classification with threshold {}", threshold);

    let mut predictions = Vec::new();
    let num_pixels = probabilities.pixel_count();

    // Check each class probability
    for i in 0..num_pixels.min(1000) {
        let x = i % probabilities.width();
        let y = i / probabilities.width();

        let prob = probabilities
            .get_pixel(x, y)
            .map_err(|e| PostprocessingError::ExportFailed {
                reason: format!("Failed to get probability: {}", e),
            })? as f32;

        if prob >= threshold {
            let class_label = class_labels.and_then(|labels| labels.get(i as usize).cloned());

            predictions.push(LabelPrediction {
                class_id: i as usize,
                class_label,
                confidence: prob,
            });
        }
    }

    // Sort by confidence (descending)
    predictions.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(MultiLabelResult {
        labels: predictions,
        threshold,
    })
}

/// Classifies land cover from model output
///
/// # Errors
/// Returns an error if classification fails
pub fn classify_land_cover(
    probabilities: &RasterBuffer,
    confidence_threshold: f32,
) -> Result<LandCoverResult> {
    let classification = classify_single_label(probabilities, None, confidence_threshold)?;

    let land_cover_type = map_class_to_land_cover(classification.class_id);

    Ok(LandCoverResult {
        classification,
        land_cover_type,
    })
}

/// Maps a class ID to a land cover type
fn map_class_to_land_cover(class_id: usize) -> LandCoverType {
    // This is a simplified mapping
    // In practice, this would be configured per model
    match class_id {
        0 => LandCoverType::Water,
        1 => LandCoverType::Developed,
        2 => LandCoverType::Barren,
        3 => LandCoverType::Forest,
        4 => LandCoverType::Shrubland,
        5 => LandCoverType::Herbaceous,
        6 => LandCoverType::Cultivated,
        7 => LandCoverType::Wetlands,
        8 => LandCoverType::IceSnow,
        _ => LandCoverType::Unknown,
    }
}

/// Computes top-k predictions
///
/// # Errors
/// Returns an error if computation fails
pub fn top_k_predictions(
    probabilities: &RasterBuffer,
    class_labels: Option<&[String]>,
    k: usize,
) -> Result<Vec<LabelPrediction>> {
    if k == 0 {
        return Ok(Vec::new());
    }

    let mut predictions = Vec::new();
    let num_pixels = probabilities.pixel_count();

    // Collect all probabilities
    for i in 0..num_pixels.min(1000) {
        let x = i % probabilities.width();
        let y = i / probabilities.width();

        let prob = probabilities
            .get_pixel(x, y)
            .map_err(|e| PostprocessingError::ExportFailed {
                reason: format!("Failed to get probability: {}", e),
            })? as f32;

        let class_label = class_labels.and_then(|labels| labels.get(i as usize).cloned());

        predictions.push(LabelPrediction {
            class_id: i as usize,
            class_label,
            confidence: prob,
        });
    }

    // Sort by confidence (descending)
    predictions.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Return top k
    predictions.truncate(k);

    Ok(predictions)
}

/// Computes confusion metrics for evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfusionMetrics {
    /// True positives
    pub true_positives: u64,
    /// False positives
    pub false_positives: u64,
    /// True negatives
    pub true_negatives: u64,
    /// False negatives
    pub false_negatives: u64,
    /// Precision
    pub precision: f64,
    /// Recall
    pub recall: f64,
    /// F1 score
    pub f1_score: f64,
    /// Accuracy
    pub accuracy: f64,
}

/// Computes confusion metrics from predictions and ground truth
///
/// # Errors
/// Returns an error if computation fails
pub fn compute_confusion_metrics(
    predictions: &RasterBuffer,
    ground_truth: &RasterBuffer,
    positive_class: usize,
) -> Result<ConfusionMetrics> {
    if predictions.width() != ground_truth.width() || predictions.height() != ground_truth.height()
    {
        return Err(PostprocessingError::MergingFailed {
            reason: "Prediction and ground truth dimensions don't match".to_string(),
        }
        .into());
    }

    let mut tp = 0u64;
    let mut fp = 0u64;
    let mut tn = 0u64;
    let mut fn_count = 0u64;

    for y in 0..predictions.height() {
        for x in 0..predictions.width() {
            let pred =
                predictions
                    .get_pixel(x, y)
                    .map_err(|e| PostprocessingError::ExportFailed {
                        reason: format!("Failed to get prediction: {}", e),
                    })? as usize;

            let truth =
                ground_truth
                    .get_pixel(x, y)
                    .map_err(|e| PostprocessingError::ExportFailed {
                        reason: format!("Failed to get ground truth: {}", e),
                    })? as usize;

            let pred_positive = pred == positive_class;
            let truth_positive = truth == positive_class;

            match (pred_positive, truth_positive) {
                (true, true) => tp += 1,
                (true, false) => fp += 1,
                (false, false) => tn += 1,
                (false, true) => fn_count += 1,
            }
        }
    }

    let precision = if tp + fp > 0 {
        tp as f64 / (tp + fp) as f64
    } else {
        0.0
    };

    let recall = if tp + fn_count > 0 {
        tp as f64 / (tp + fn_count) as f64
    } else {
        0.0
    };

    let f1_score = if precision + recall > 0.0 {
        2.0 * (precision * recall) / (precision + recall)
    } else {
        0.0
    };

    let total = tp + fp + tn + fn_count;
    let accuracy = if total > 0 {
        (tp + tn) as f64 / total as f64
    } else {
        0.0
    };

    Ok(ConfusionMetrics {
        true_positives: tp,
        false_positives: fp,
        true_negatives: tn,
        false_negatives: fn_count,
        precision,
        recall,
        f1_score,
        accuracy,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigdal_core::types::RasterDataType;

    #[test]
    fn test_classify_single_label() {
        let probs = RasterBuffer::zeros(10, 1, RasterDataType::Float32);
        let result = classify_single_label(&probs, None, 0.5);
        assert!(result.is_ok());
    }

    #[test]
    fn test_classify_multi_label() {
        let probs = RasterBuffer::zeros(10, 1, RasterDataType::Float32);
        let result = classify_multi_label(&probs, None, 0.5);
        assert!(result.is_ok());
    }

    #[test]
    fn test_land_cover_mapping() {
        assert_eq!(map_class_to_land_cover(0), LandCoverType::Water);
        assert_eq!(map_class_to_land_cover(3), LandCoverType::Forest);
        assert_eq!(map_class_to_land_cover(999), LandCoverType::Unknown);
    }

    #[test]
    fn test_top_k_predictions() {
        let probs = RasterBuffer::zeros(10, 1, RasterDataType::Float32);
        let result = top_k_predictions(&probs, None, 3);
        assert!(result.is_ok());
    }

    #[test]
    fn test_confusion_metrics() {
        let preds = RasterBuffer::zeros(10, 10, RasterDataType::UInt8);
        let truth = RasterBuffer::zeros(10, 10, RasterDataType::UInt8);
        let result = compute_confusion_metrics(&preds, &truth, 1);
        assert!(result.is_ok());
    }
}
