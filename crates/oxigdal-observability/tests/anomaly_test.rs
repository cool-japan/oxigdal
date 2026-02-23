//! Tests for anomaly detection.

use chrono::Utc;
use oxigdal_observability::anomaly::{
    AnomalyDetector, DataPoint,
    statistical::{IqrDetector, ZScoreDetector},
};

#[test]
fn test_zscore_detector() {
    let mut detector = ZScoreDetector::new(3.0);

    let baseline_data = vec![
        DataPoint::new(Utc::now(), 10.0),
        DataPoint::new(Utc::now(), 12.0),
        DataPoint::new(Utc::now(), 11.0),
        DataPoint::new(Utc::now(), 10.5),
    ];

    detector
        .update_baseline(&baseline_data)
        .expect("Failed to update baseline");

    let test_data = vec![
        DataPoint::new(Utc::now(), 50.0), // Clear anomaly
    ];

    let anomalies = detector.detect(&test_data).expect("Failed to detect");
    assert!(!anomalies.is_empty());
}

#[test]
fn test_iqr_detector() {
    let mut detector = IqrDetector::new(1.5);

    let baseline_data = vec![
        DataPoint::new(Utc::now(), 10.0),
        DataPoint::new(Utc::now(), 12.0),
        DataPoint::new(Utc::now(), 11.0),
        DataPoint::new(Utc::now(), 10.5),
        DataPoint::new(Utc::now(), 11.5),
    ];

    detector
        .update_baseline(&baseline_data)
        .expect("Failed to update baseline");

    let test_data = vec![
        DataPoint::new(Utc::now(), 100.0), // Anomaly
    ];

    let anomalies = detector.detect(&test_data).expect("Failed to detect");
    assert!(!anomalies.is_empty());
}
