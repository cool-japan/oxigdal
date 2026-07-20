//! Alert instance and state machine

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{AlertLevel, AlertRuleDefinition, AlertState};

/// A single alert instance generated from a rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertInstance {
    /// Unique identifier for this alert instance.
    pub id: String,
    /// ID of the rule that generated this alert.
    pub rule_id: String,
    /// Current state.
    pub state: AlertState,
    /// Alert level.
    pub level: AlertLevel,
    /// Alert summary.
    pub summary: String,
    /// Detailed description.
    pub description: String,
    /// Labels for identification and routing.
    pub labels: HashMap<String, String>,
    /// Annotations for additional context.
    pub annotations: HashMap<String, String>,
    /// When the alert started pending.
    pub pending_at: Option<DateTime<Utc>>,
    /// When the alert started firing.
    pub firing_at: Option<DateTime<Utc>>,
    /// When the alert was resolved.
    pub resolved_at: Option<DateTime<Utc>>,
    /// When the alert was last updated.
    pub updated_at: DateTime<Utc>,
    /// Number of times this alert has fired.
    pub fire_count: u64,
    /// Fingerprint for deduplication.
    pub fingerprint: String,
    /// Generator URL for investigation.
    pub generator_url: Option<String>,
}

impl AlertInstance {
    /// Create a new alert instance from a rule.
    pub fn from_rule(rule: &AlertRuleDefinition) -> Self {
        let now = Utc::now();
        let id = uuid::Uuid::new_v4().to_string();
        let fingerprint = Self::compute_fingerprint(&rule.id, &rule.labels);

        Self {
            id,
            rule_id: rule.id.clone(),
            state: AlertState::Inactive,
            level: rule.level,
            summary: rule.name.clone(),
            description: rule.description.clone(),
            labels: rule.labels.clone(),
            annotations: rule.annotations.clone(),
            pending_at: None,
            firing_at: None,
            resolved_at: None,
            updated_at: now,
            fire_count: 0,
            fingerprint,
            generator_url: rule.dashboard_url.clone(),
        }
    }

    /// Compute a fingerprint for deduplication.
    fn compute_fingerprint(rule_id: &str, labels: &HashMap<String, String>) -> String {
        let mut sorted_labels: Vec<_> = labels.iter().collect();
        sorted_labels.sort_by_key(|(k, _)| *k);

        let label_str: String = sorted_labels
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join(",");

        format!("{}:{}", rule_id, label_str)
    }

    /// Transition to pending state.
    pub fn transition_to_pending(&mut self) {
        if self.state == AlertState::Inactive {
            self.state = AlertState::Pending;
            self.pending_at = Some(Utc::now());
            self.updated_at = Utc::now();
        }
    }

    /// Transition to firing state.
    pub fn transition_to_firing(&mut self) {
        if matches!(self.state, AlertState::Pending | AlertState::Inactive) {
            self.state = AlertState::Firing;
            self.firing_at = Some(Utc::now());
            self.fire_count += 1;
            self.updated_at = Utc::now();
        }
    }

    /// Transition to resolved state.
    pub fn transition_to_resolved(&mut self) {
        if self.state.is_active() {
            self.state = AlertState::Resolved;
            self.resolved_at = Some(Utc::now());
            self.updated_at = Utc::now();
        }
    }

    /// Transition to silenced state.
    pub fn transition_to_silenced(&mut self) {
        self.state = AlertState::Silenced;
        self.updated_at = Utc::now();
    }

    /// Transition to acknowledged state.
    pub fn acknowledge(&mut self) {
        if self.state == AlertState::Firing {
            self.state = AlertState::Acknowledged;
            self.updated_at = Utc::now();
        }
    }

    /// Reset to inactive state.
    pub fn reset(&mut self) {
        self.state = AlertState::Inactive;
        self.pending_at = None;
        self.firing_at = None;
        self.resolved_at = None;
        self.updated_at = Utc::now();
    }
}
