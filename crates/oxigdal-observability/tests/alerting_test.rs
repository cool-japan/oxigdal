//! Tests for alert management.

use oxigdal_observability::alerting::{
    Alert, AlertManager, AlertSeverity, AlertStatus, dedup::AlertDeduplicator,
};

#[test]
fn test_alert_creation() {
    let alert = Alert::new(
        "test_alert".to_string(),
        AlertSeverity::High,
        "Test message".to_string(),
    );

    assert_eq!(alert.status, AlertStatus::Firing);
    assert_eq!(alert.severity, AlertSeverity::High);
}

#[test]
fn test_alert_resolution() {
    let mut alert = Alert::new(
        "test_alert".to_string(),
        AlertSeverity::High,
        "Test message".to_string(),
    );

    alert.resolve();
    assert_eq!(alert.status, AlertStatus::Resolved);
    assert!(alert.ends_at.is_some());
}

#[test]
fn test_alert_deduplication() {
    let dedup = AlertDeduplicator::new();

    let alerts = vec![
        Alert::new(
            "alert1".to_string(),
            AlertSeverity::High,
            "msg1".to_string(),
        ),
        Alert::new(
            "alert1".to_string(),
            AlertSeverity::High,
            "msg1".to_string(),
        ),
        Alert::new("alert2".to_string(), AlertSeverity::Low, "msg2".to_string()),
    ];

    let deduplicated = dedup.deduplicate(&alerts).expect("Failed to deduplicate");
    assert_eq!(deduplicated.len(), 2);
}

#[test]
fn test_alert_manager() {
    let manager = AlertManager::new();
    // Basic creation test
    let _ = manager;
}
