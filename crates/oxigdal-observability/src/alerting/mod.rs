//! Alert management and routing.

pub mod dedup;
pub mod escalation;
pub mod routing;
pub mod rules;

use crate::error::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Alert definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    /// Alert ID.
    pub id: String,

    /// Alert name.
    pub name: String,

    /// Alert severity.
    pub severity: AlertSeverity,

    /// Alert status.
    pub status: AlertStatus,

    /// Alert message.
    pub message: String,

    /// Additional labels.
    pub labels: std::collections::HashMap<String, String>,

    /// Alert timestamp.
    pub timestamp: DateTime<Utc>,

    /// When the alert started firing.
    pub starts_at: DateTime<Utc>,

    /// When the alert stopped firing (if resolved).
    pub ends_at: Option<DateTime<Utc>>,
}

/// Alert severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertSeverity {
    /// Critical severity - requires immediate attention.
    Critical,
    /// High severity - urgent issue.
    High,
    /// Medium severity - important but not urgent.
    Medium,
    /// Low severity - minor issue.
    Low,
    /// Informational - not an issue, just a notice.
    Info,
}

/// Alert status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertStatus {
    /// Alert is currently active.
    Firing,
    /// Alert has been resolved.
    Resolved,
    /// Alert has been acknowledged by an operator.
    Acknowledged,
    /// Alert has been silenced (notifications suppressed).
    Silenced,
}

impl Alert {
    /// Create a new alert.
    pub fn new(name: String, severity: AlertSeverity, message: String) -> Self {
        let now = Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            severity,
            status: AlertStatus::Firing,
            message,
            labels: std::collections::HashMap::new(),
            timestamp: now,
            starts_at: now,
            ends_at: None,
        }
    }

    /// Add a label to the alert.
    pub fn with_label(mut self, key: String, value: String) -> Self {
        self.labels.insert(key, value);
        self
    }

    /// Resolve the alert.
    pub fn resolve(&mut self) {
        self.status = AlertStatus::Resolved;
        self.ends_at = Some(Utc::now());
    }

    /// Acknowledge the alert.
    pub fn acknowledge(&mut self) {
        self.status = AlertStatus::Acknowledged;
    }
}

/// Alert manager for managing alerts lifecycle.
pub struct AlertManager {
    rules: rules::AlertRuleEngine,
    router: routing::AlertRouter,
    deduplicator: dedup::AlertDeduplicator,
}

impl AlertManager {
    /// Create a new alert manager.
    pub fn new() -> Self {
        Self {
            rules: rules::AlertRuleEngine::new(),
            router: routing::AlertRouter::new(),
            deduplicator: dedup::AlertDeduplicator::new(),
        }
    }

    /// Add an alert rule.
    pub fn add_rule(&mut self, rule: rules::AlertRule) {
        self.rules.add_rule(rule);
    }

    /// Add a routing rule.
    pub fn add_route(&mut self, route: routing::Route) {
        self.router.add_route(route);
    }

    /// Evaluate alert rules and fire alerts if needed.
    pub async fn evaluate_rules(&mut self) -> Result<Vec<Alert>> {
        let alerts = self.rules.evaluate().await?;

        // Deduplicate alerts
        let deduplicated = self.deduplicator.deduplicate(&alerts)?;

        // Route alerts
        for alert in &deduplicated {
            self.router.route(alert).await?;
        }

        Ok(deduplicated)
    }
}

impl Default for AlertManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alert_creation() {
        let alert = Alert::new(
            "test_alert".to_string(),
            AlertSeverity::High,
            "Test alert message".to_string(),
        )
        .with_label("service".to_string(), "oxigdal".to_string());

        assert_eq!(alert.name, "test_alert");
        assert_eq!(alert.severity, AlertSeverity::High);
        assert_eq!(alert.status, AlertStatus::Firing);
        assert_eq!(alert.labels.len(), 1);
    }

    #[test]
    fn test_alert_resolution() {
        let mut alert = Alert::new(
            "test_alert".to_string(),
            AlertSeverity::High,
            "Test alert message".to_string(),
        );

        alert.resolve();
        assert_eq!(alert.status, AlertStatus::Resolved);
        assert!(alert.ends_at.is_some());
    }
}
