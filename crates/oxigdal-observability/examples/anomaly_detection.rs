//! Example of anomaly detection.

use chrono::Utc;
use oxigdal_observability::anomaly::{
    AnomalyDetector, DataPoint,
    statistical::{IqrDetector, ZScoreDetector},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create baseline data
    let baseline: Vec<DataPoint> = (0..100)
        .map(|i| DataPoint::new(Utc::now(), 10.0 + (i as f64 % 5.0)))
        .collect();

    // Z-score detector
    let mut zscore = ZScoreDetector::new(3.0);
    zscore.update_baseline(&baseline)?;

    // Test data with anomaly
    let test_data = vec![
        DataPoint::new(Utc::now(), 11.0), // Normal
        DataPoint::new(Utc::now(), 50.0), // Anomaly
        DataPoint::new(Utc::now(), 12.0), // Normal
    ];

    let anomalies = zscore.detect(&test_data)?;
    println!("Z-score detected {} anomalies", anomalies.len());

    for anomaly in &anomalies {
        println!(
            "Anomaly: {} - Score: {:.2}, Severity: {:?}",
            anomaly.description, anomaly.score, anomaly.severity
        );
    }

    // IQR detector
    let mut iqr = IqrDetector::new(1.5);
    iqr.update_baseline(&baseline)?;

    let anomalies = iqr.detect(&test_data)?;
    println!("\nIQR detected {} anomalies", anomalies.len());

    Ok(())
}
