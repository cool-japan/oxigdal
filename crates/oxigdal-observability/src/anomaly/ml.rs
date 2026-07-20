//! ML-based anomaly detection.

use super::{Anomaly, AnomalyDetector, AnomalySeverity, AnomalyType, DataPoint};
use crate::error::{ObservabilityError, Result};
use parking_lot::RwLock;

// ---------------------------------------------------------------------------
// IsolationForest
// ---------------------------------------------------------------------------

#[allow(dead_code)]
enum INode {
    Split {
        feature_idx: usize,
        split_value: f64,
        left: usize,
        right: usize,
    },
    Leaf {
        depth: u8,
        size: usize,
    },
}

type ITree = Vec<INode>;

struct ForestModel {
    trees: Vec<ITree>,
    sample_size: usize,
    training_mean: f64,
}

/// Isolation-Forest anomaly detector.
pub struct IsolationForestDetector {
    forest: RwLock<Option<ForestModel>>,
    threshold: f64,
}

impl IsolationForestDetector {
    /// Create a new detector with the given anomaly score threshold (0–1).
    pub fn new(threshold: f64) -> Self {
        Self {
            forest: RwLock::new(None),
            threshold,
        }
    }
}

fn build_node(arena: &mut Vec<INode>, values: &[f64], depth: u8, max_depth: u8) -> usize {
    if values.len() <= 1 || depth >= max_depth {
        let idx = arena.len();
        arena.push(INode::Leaf {
            depth,
            size: values.len(),
        });
        return idx;
    }

    let mut min = values[0];
    let mut max = values[0];
    for &v in &values[1..] {
        if v < min {
            min = v;
        }
        if v > max {
            max = v;
        }
    }

    if min >= max {
        let idx = arena.len();
        arena.push(INode::Leaf {
            depth,
            size: values.len(),
        });
        return idx;
    }

    let split_value = min + fastrand::f64() * (max - min);

    // Reserve slot for split node (placeholder).
    let split_idx = arena.len();
    arena.push(INode::Leaf { depth: 0, size: 0 });

    let left_vals: Vec<f64> = values
        .iter()
        .copied()
        .filter(|&v| v < split_value)
        .collect();
    let right_vals: Vec<f64> = values
        .iter()
        .copied()
        .filter(|&v| v >= split_value)
        .collect();

    let left_idx = build_node(arena, &left_vals, depth + 1, max_depth);
    let right_idx = build_node(arena, &right_vals, depth + 1, max_depth);

    arena[split_idx] = INode::Split {
        feature_idx: 0,
        split_value,
        left: left_idx,
        right: right_idx,
    };

    split_idx
}

fn build_itree(values: &[f64], depth: u8, max_depth: u8) -> ITree {
    let mut arena: Vec<INode> = Vec::new();
    build_node(&mut arena, values, depth, max_depth);
    arena
}

fn c(n: usize) -> f64 {
    if n <= 1 {
        0.0
    } else {
        2.0 * ((n - 1) as f64).ln() + 0.5772_1566_49 - 2.0 * (n - 1) as f64 / n as f64
    }
}

impl AnomalyDetector for IsolationForestDetector {
    fn update_baseline(&mut self, data: &[DataPoint]) -> Result<()> {
        if data.is_empty() {
            return Err(ObservabilityError::AnomalyDetectionError(
                "No data provided for baseline update".to_string(),
            ));
        }

        let subsample_size = 256.min(data.len());
        let mut trees: Vec<ITree> = Vec::with_capacity(100);

        for _ in 0..100 {
            let values: Vec<f64> = (0..subsample_size)
                .map(|_| data[fastrand::usize(..data.len())].value)
                .collect();
            trees.push(build_itree(&values, 0, 12));
        }

        let training_mean = data.iter().map(|d| d.value).sum::<f64>() / data.len() as f64;

        *self.forest.write() = Some(ForestModel {
            trees,
            sample_size: subsample_size,
            training_mean,
        });

        Ok(())
    }

    fn detect(&self, data: &[DataPoint]) -> Result<Vec<Anomaly>> {
        let guard = self.forest.read();
        let model = guard.as_ref().ok_or_else(|| {
            ObservabilityError::AnomalyDetectionError(
                "Baseline not established for Isolation Forest".to_string(),
            )
        })?;

        let num_trees = model.trees.len();
        let cn = c(model.sample_size);

        let mut anomalies = Vec::new();

        for dp in data {
            let mut total_path = 0.0_f64;

            for tree in &model.trees {
                let mut node_idx = 0usize;
                let path_len = loop {
                    match &tree[node_idx] {
                        INode::Leaf { depth, size } => {
                            break *depth as f64 + c(*size);
                        }
                        INode::Split {
                            split_value,
                            left,
                            right,
                            ..
                        } => {
                            if dp.value < *split_value {
                                node_idx = *left;
                            } else {
                                node_idx = *right;
                            }
                        }
                    }
                };
                total_path += path_len;
            }

            let avg_path_len = total_path / num_trees as f64;
            let raw_score = if cn > 0.0 {
                2.0_f64.powf(-avg_path_len / cn)
            } else {
                0.5
            };
            let score = raw_score.clamp(0.0, 1.0);

            if score > self.threshold {
                let severity = if score > 0.9 {
                    AnomalySeverity::Critical
                } else if score > 0.8 {
                    AnomalySeverity::High
                } else if score > 0.65 {
                    AnomalySeverity::Medium
                } else {
                    AnomalySeverity::Low
                };

                let anomaly_type = if dp.value > model.training_mean {
                    AnomalyType::Spike
                } else {
                    AnomalyType::Drop
                };

                anomalies.push(Anomaly {
                    timestamp: dp.timestamp,
                    metric_name: "isolation_forest".to_string(),
                    observed_value: dp.value,
                    expected_value: model.training_mean,
                    score,
                    severity,
                    anomaly_type,
                    description: format!("Isolation Forest score: {:.4}", score),
                });
            }
        }

        Ok(anomalies)
    }
}

// ---------------------------------------------------------------------------
// AutoencoderDetector (PCA-based reconstruction error proxy)
// ---------------------------------------------------------------------------

struct PcaModel {
    mean: f64,
    variance: f64,
    /// 95th-percentile reconstruction error from training data.
    scale: f64,
}

/// Autoencoder-style anomaly detector (PCA reconstruction error proxy).
pub struct AutoencoderDetector {
    model: RwLock<Option<PcaModel>>,
    threshold: f64,
}

impl AutoencoderDetector {
    /// Create a new detector with the given threshold multiplier.
    pub fn new(threshold: f64) -> Self {
        Self {
            model: RwLock::new(None),
            threshold,
        }
    }
}

impl AnomalyDetector for AutoencoderDetector {
    fn update_baseline(&mut self, data: &[DataPoint]) -> Result<()> {
        if data.is_empty() {
            return Err(ObservabilityError::AnomalyDetectionError(
                "No data provided for baseline update".to_string(),
            ));
        }

        let n = data.len() as f64;
        let mean = data.iter().map(|d| d.value).sum::<f64>() / n;
        let variance = data.iter().map(|d| (d.value - mean).powi(2)).sum::<f64>() / n;

        let mut errors: Vec<f64> = data
            .iter()
            .map(|d| (d.value - mean).powi(2) / variance.max(1e-10))
            .collect();

        errors.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let p95_idx = (errors.len() * 95 / 100).min(errors.len() - 1);
        let scale = errors[p95_idx];

        *self.model.write() = Some(PcaModel {
            mean,
            variance,
            scale,
        });

        Ok(())
    }

    fn detect(&self, data: &[DataPoint]) -> Result<Vec<Anomaly>> {
        let guard = self.model.read();
        let model = guard.as_ref().ok_or_else(|| {
            ObservabilityError::AnomalyDetectionError(
                "Baseline not established for Autoencoder".to_string(),
            )
        })?;

        let mut anomalies = Vec::new();

        for dp in data {
            let reconstruction_error = (dp.value - model.mean).powi(2) / model.variance.max(1e-10);
            let threshold_value = self.threshold * model.scale;

            if reconstruction_error > threshold_value {
                let score = (reconstruction_error / (threshold_value * 3.0).max(1e-10)).min(1.0);

                let severity = if score > 0.9 {
                    AnomalySeverity::Critical
                } else if score > 0.8 {
                    AnomalySeverity::High
                } else if score > 0.65 {
                    AnomalySeverity::Medium
                } else {
                    AnomalySeverity::Low
                };

                let anomaly_type = if dp.value > model.mean {
                    AnomalyType::Spike
                } else {
                    AnomalyType::Drop
                };

                anomalies.push(Anomaly {
                    timestamp: dp.timestamp,
                    metric_name: "autoencoder".to_string(),
                    observed_value: dp.value,
                    expected_value: model.mean,
                    score,
                    severity,
                    anomaly_type,
                    description: format!("Reconstruction error: {:.4}", reconstruction_error),
                });
            }
        }

        Ok(anomalies)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_point(v: f64) -> DataPoint {
        DataPoint::new(Utc::now(), v)
    }

    #[test]
    fn test_isolation_forest_no_baseline() {
        let detector = IsolationForestDetector::new(0.6);
        let data = vec![make_point(10.0)];
        let result = detector.detect(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_isolation_forest_detects_anomaly() {
        let mut detector = IsolationForestDetector::new(0.5);
        // Train on normal data clustered around 10.0
        let normal: Vec<DataPoint> = (0..30)
            .map(|i| make_point(10.0 + (i as f64 % 3.0) * 0.1))
            .collect();
        detector.update_baseline(&normal).expect("baseline failed");
        // Detect with one clear outlier
        let data = vec![make_point(1000.0)];
        let anomalies = detector.detect(&data).expect("detect failed");
        assert!(!anomalies.is_empty(), "Should detect 1000.0 as anomaly");
        assert_eq!(anomalies[0].anomaly_type, AnomalyType::Spike);
    }

    #[test]
    fn test_autoencoder_no_baseline() {
        let detector = AutoencoderDetector::new(1.0);
        let data = vec![make_point(10.0)];
        let result = detector.detect(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_autoencoder_detects_anomaly() {
        let mut detector = AutoencoderDetector::new(1.0);
        let normal: Vec<DataPoint> = (0..30)
            .map(|i| make_point(10.0 + (i as f64 % 3.0) * 0.1))
            .collect();
        detector.update_baseline(&normal).expect("baseline failed");
        let data = vec![make_point(1000.0)];
        let anomalies = detector.detect(&data).expect("detect failed");
        assert!(!anomalies.is_empty(), "Should detect 1000.0 as anomaly");
        assert_eq!(anomalies[0].anomaly_type, AnomalyType::Spike);
    }
}
