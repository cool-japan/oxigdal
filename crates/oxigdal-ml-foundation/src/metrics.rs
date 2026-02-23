//! Evaluation metrics for machine learning models.
//!
//! Provides common metrics for classification, segmentation, and detection tasks.

use crate::{Error, Result};
use ndarray::{Array2, ArrayView1, ArrayView2};

/// Evaluation metrics container.
#[derive(Debug, Clone, Default)]
pub struct Metrics {
    /// Accuracy (TP + TN) / (TP + TN + FP + FN)
    pub accuracy: f64,
    /// Precision TP / (TP + FP)
    pub precision: f64,
    /// Recall TP / (TP + FN)
    pub recall: f64,
    /// F1 score: 2 * (precision * recall) / (precision + recall)
    pub f1_score: f64,
    /// Intersection over Union (IoU)
    pub iou: f64,
    /// Mean IoU across classes
    pub mean_iou: f64,
    /// Per-class IoU scores
    pub per_class_iou: Vec<f64>,
    /// Confusion matrix
    pub confusion_matrix: Option<Array2<usize>>,
}

impl Metrics {
    /// Creates a new empty metrics container.
    pub fn new() -> Self {
        Self::default()
    }

    /// Computes accuracy from predictions and ground truth.
    ///
    /// # Arguments
    /// * `predictions` - Predicted class labels
    /// * `ground_truth` - True class labels
    pub fn compute_accuracy(
        predictions: ArrayView1<usize>,
        ground_truth: ArrayView1<usize>,
    ) -> Result<f64> {
        if predictions.len() != ground_truth.len() {
            return Err(Error::invalid_dimensions(
                format!("{}", ground_truth.len()),
                format!("{}", predictions.len()),
            ));
        }

        if predictions.is_empty() {
            return Err(Error::Metric("Empty predictions".to_string()));
        }

        let correct = predictions
            .iter()
            .zip(ground_truth.iter())
            .filter(|(pred, gt)| pred == gt)
            .count();

        Ok(correct as f64 / predictions.len() as f64)
    }

    /// Computes confusion matrix from predictions and ground truth.
    ///
    /// # Arguments
    /// * `predictions` - Predicted class labels
    /// * `ground_truth` - True class labels
    /// * `num_classes` - Number of classes
    pub fn compute_confusion_matrix(
        predictions: ArrayView1<usize>,
        ground_truth: ArrayView1<usize>,
        num_classes: usize,
    ) -> Result<Array2<usize>> {
        if predictions.len() != ground_truth.len() {
            return Err(Error::invalid_dimensions(
                format!("{}", ground_truth.len()),
                format!("{}", predictions.len()),
            ));
        }

        let mut matrix = Array2::<usize>::zeros((num_classes, num_classes));

        for (pred, gt) in predictions.iter().zip(ground_truth.iter()) {
            if *pred >= num_classes || *gt >= num_classes {
                return Err(Error::Metric(format!(
                    "Class index out of bounds: pred={}, gt={}, num_classes={}",
                    pred, gt, num_classes
                )));
            }
            matrix[[*gt, *pred]] += 1;
        }

        Ok(matrix)
    }

    /// Computes precision, recall, and F1 score from confusion matrix.
    ///
    /// # Arguments
    /// * `confusion_matrix` - Confusion matrix (rows: true labels, cols: predicted labels)
    /// * `class_idx` - Class index to compute metrics for
    pub fn compute_precision_recall_f1(
        confusion_matrix: ArrayView2<usize>,
        class_idx: usize,
    ) -> Result<(f64, f64, f64)> {
        let num_classes = confusion_matrix.nrows();
        if confusion_matrix.ncols() != num_classes {
            return Err(Error::Metric("Non-square confusion matrix".to_string()));
        }

        if class_idx >= num_classes {
            return Err(Error::Metric(format!(
                "Class index {} out of bounds (num_classes={})",
                class_idx, num_classes
            )));
        }

        // True positives: diagonal element
        let tp = confusion_matrix[[class_idx, class_idx]] as f64;

        // False positives: sum of column minus TP
        let fp = confusion_matrix
            .column(class_idx)
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != class_idx)
            .map(|(_, &v)| v as f64)
            .sum::<f64>();

        // False negatives: sum of row minus TP
        let fn_count = confusion_matrix
            .row(class_idx)
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != class_idx)
            .map(|(_, &v)| v as f64)
            .sum::<f64>();

        let precision = if tp + fp > 0.0 { tp / (tp + fp) } else { 0.0 };

        let recall = if tp + fn_count > 0.0 {
            tp / (tp + fn_count)
        } else {
            0.0
        };

        let f1 = if precision + recall > 0.0 {
            2.0 * (precision * recall) / (precision + recall)
        } else {
            0.0
        };

        Ok((precision, recall, f1))
    }

    /// Computes Intersection over Union (IoU) for binary segmentation.
    ///
    /// # Arguments
    /// * `predictions` - Predicted binary mask (0 or 1)
    /// * `ground_truth` - True binary mask (0 or 1)
    pub fn compute_iou_binary(
        predictions: ArrayView1<usize>,
        ground_truth: ArrayView1<usize>,
    ) -> Result<f64> {
        if predictions.len() != ground_truth.len() {
            return Err(Error::invalid_dimensions(
                format!("{}", ground_truth.len()),
                format!("{}", predictions.len()),
            ));
        }

        let intersection = predictions
            .iter()
            .zip(ground_truth.iter())
            .filter(|(p, g)| **p == 1 && **g == 1)
            .count();

        let union = predictions
            .iter()
            .zip(ground_truth.iter())
            .filter(|(p, g)| **p == 1 || **g == 1)
            .count();

        if union == 0 {
            // Both masks are empty
            return Ok(1.0);
        }

        Ok(intersection as f64 / union as f64)
    }

    /// Computes mean IoU across multiple classes.
    ///
    /// # Arguments
    /// * `predictions` - Predicted class labels
    /// * `ground_truth` - True class labels
    /// * `num_classes` - Number of classes
    pub fn compute_mean_iou(
        predictions: ArrayView1<usize>,
        ground_truth: ArrayView1<usize>,
        num_classes: usize,
    ) -> Result<(f64, Vec<f64>)> {
        if predictions.len() != ground_truth.len() {
            return Err(Error::invalid_dimensions(
                format!("{}", ground_truth.len()),
                format!("{}", predictions.len()),
            ));
        }

        let mut per_class_iou = Vec::with_capacity(num_classes);

        for class_idx in 0..num_classes {
            let pred_mask = predictions.mapv(|v| if v == class_idx { 1 } else { 0 });
            let gt_mask = ground_truth.mapv(|v| if v == class_idx { 1 } else { 0 });

            let iou = Self::compute_iou_binary(pred_mask.view(), gt_mask.view())?;
            per_class_iou.push(iou);
        }

        let mean_iou = per_class_iou.iter().sum::<f64>() / num_classes as f64;

        Ok((mean_iou, per_class_iou))
    }

    /// Computes all metrics for multi-class classification/segmentation.
    ///
    /// # Arguments
    /// * `predictions` - Predicted class labels
    /// * `ground_truth` - True class labels
    /// * `num_classes` - Number of classes
    pub fn compute_all(
        predictions: ArrayView1<usize>,
        ground_truth: ArrayView1<usize>,
        num_classes: usize,
    ) -> Result<Self> {
        let accuracy = Self::compute_accuracy(predictions, ground_truth)?;
        let confusion_matrix =
            Self::compute_confusion_matrix(predictions, ground_truth, num_classes)?;

        // Compute macro-averaged precision, recall, F1
        let mut precisions = Vec::new();
        let mut recalls = Vec::new();
        let mut f1_scores = Vec::new();

        for class_idx in 0..num_classes {
            let (precision, recall, f1) =
                Self::compute_precision_recall_f1(confusion_matrix.view(), class_idx)?;
            precisions.push(precision);
            recalls.push(recall);
            f1_scores.push(f1);
        }

        let macro_precision = precisions.iter().sum::<f64>() / num_classes as f64;
        let macro_recall = recalls.iter().sum::<f64>() / num_classes as f64;
        let macro_f1 = f1_scores.iter().sum::<f64>() / num_classes as f64;

        let (mean_iou, per_class_iou) =
            Self::compute_mean_iou(predictions, ground_truth, num_classes)?;

        Ok(Self {
            accuracy,
            precision: macro_precision,
            recall: macro_recall,
            f1_score: macro_f1,
            iou: mean_iou, // For compatibility
            mean_iou,
            per_class_iou,
            confusion_matrix: Some(confusion_matrix),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use ndarray::arr1;

    #[test]
    fn test_accuracy() {
        let predictions = arr1(&[0, 1, 2, 1, 0]);
        let ground_truth = arr1(&[0, 1, 2, 2, 0]);

        let accuracy = Metrics::compute_accuracy(predictions.view(), ground_truth.view())
            .expect("Failed to compute accuracy");

        assert_relative_eq!(accuracy, 0.8, epsilon = 1e-6);
    }

    #[test]
    fn test_confusion_matrix() {
        let predictions = arr1(&[0, 1, 2, 1, 0]);
        let ground_truth = arr1(&[0, 1, 2, 2, 0]);

        let cm = Metrics::compute_confusion_matrix(predictions.view(), ground_truth.view(), 3)
            .expect("Failed to compute confusion matrix");

        assert_eq!(cm[[0, 0]], 2); // Class 0 correctly predicted twice
        assert_eq!(cm[[1, 1]], 1); // Class 1 correctly predicted once
        assert_eq!(cm[[2, 2]], 1); // Class 2 correctly predicted once
        assert_eq!(cm[[2, 1]], 1); // Class 2 misclassified as 1
    }

    #[test]
    fn test_iou_binary() {
        let predictions = arr1(&[1, 1, 0, 0]);
        let ground_truth = arr1(&[1, 0, 0, 1]);

        let iou = Metrics::compute_iou_binary(predictions.view(), ground_truth.view())
            .expect("Failed to compute IoU");

        // Intersection: 1, Union: 3
        assert_relative_eq!(iou, 1.0 / 3.0, epsilon = 1e-6);
    }

    #[test]
    fn test_compute_all() {
        let predictions = arr1(&[0, 1, 2, 1, 0, 2]);
        let ground_truth = arr1(&[0, 1, 2, 2, 0, 1]);

        let metrics = Metrics::compute_all(predictions.view(), ground_truth.view(), 3)
            .expect("Failed to compute metrics");

        assert!(metrics.accuracy > 0.0);
        assert!(metrics.accuracy <= 1.0);
        assert!(metrics.mean_iou >= 0.0);
        assert!(metrics.mean_iou <= 1.0);
        assert_eq!(metrics.per_class_iou.len(), 3);
        assert!(metrics.confusion_matrix.is_some());
    }

    #[test]
    fn test_empty_predictions_error() {
        let predictions = arr1(&[]);
        let ground_truth = arr1(&[]);

        let result = Metrics::compute_accuracy(predictions.view(), ground_truth.view());
        assert!(result.is_err());
    }

    #[test]
    fn test_mismatched_lengths_error() {
        let predictions = arr1(&[0, 1, 2]);
        let ground_truth = arr1(&[0, 1]);

        let result = Metrics::compute_accuracy(predictions.view(), ground_truth.view());
        assert!(result.is_err());
    }
}
