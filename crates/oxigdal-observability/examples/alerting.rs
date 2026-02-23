//! Example of alert management.

use oxigdal_observability::alerting::{
    Alert, AlertManager, AlertSeverity,
    routing::{Destination, Route},
    rules::AlertRule,
};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut manager = AlertManager::new();

    // Add alert rule
    let rule = AlertRule {
        name: "high_error_rate".to_string(),
        condition: Arc::new(|| {
            // In production, check actual metrics
            false
        }),
        severity: AlertSeverity::High,
        message: "Error rate exceeded threshold".to_string(),
    };

    manager.add_rule(rule);

    // Add routing rule
    let route = Route {
        matcher: Box::new(|alert| alert.severity == AlertSeverity::High),
        destinations: vec![Destination::Webhook {
            url: "https://example.com/webhook".to_string(),
        }],
    };

    manager.add_route(route);

    // Evaluate rules
    let alerts = manager.evaluate_rules().await?;
    println!("Evaluated rules, found {} alerts", alerts.len());

    // Create and route a test alert
    let alert = Alert::new(
        "test_alert".to_string(),
        AlertSeverity::High,
        "Test alert message".to_string(),
    );

    println!("Alert created: {}", alert.id);

    Ok(())
}
