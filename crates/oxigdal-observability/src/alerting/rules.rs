//! Alert rule engine.

use super::{Alert, AlertSeverity};
use crate::error::Result;
use parking_lot::RwLock;
use std::sync::Arc;

/// Alert rule.
#[derive(Clone)]
pub struct AlertRule {
    /// Name of the alert rule.
    pub name: String,
    /// Condition function that returns true when alert should trigger.
    pub condition: Arc<dyn Fn() -> bool + Send + Sync>,
    /// Severity level for this alert.
    pub severity: AlertSeverity,
    /// Message to include when alert is triggered.
    pub message: String,
}

/// Alert rule engine.
pub struct AlertRuleEngine {
    rules: Arc<RwLock<Vec<AlertRule>>>,
}

impl AlertRuleEngine {
    /// Create a new alert rule engine.
    pub fn new() -> Self {
        Self {
            rules: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Add an alert rule.
    pub fn add_rule(&mut self, rule: AlertRule) {
        self.rules.write().push(rule);
    }

    /// Evaluate all rules and generate alerts.
    pub async fn evaluate(&self) -> Result<Vec<Alert>> {
        let rules = self.rules.read();
        let mut alerts = Vec::new();

        for rule in rules.iter() {
            if (rule.condition)() {
                let alert = Alert::new(rule.name.clone(), rule.severity, rule.message.clone());
                alerts.push(alert);
            }
        }

        Ok(alerts)
    }
}

impl Default for AlertRuleEngine {
    fn default() -> Self {
        Self::new()
    }
}
