//! Alert grouping and aggregation

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use super::{AlertInstance, AlertLevel};
/// Alert group for aggregating related alerts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertGroup {
    /// Group identifier.
    pub id: String,
    /// Grouping labels (key-value pairs that define the group).
    pub labels: HashMap<String, String>,
    /// Alerts in this group.
    pub alerts: Vec<String>,
    /// When the group was created.
    pub created_at: DateTime<Utc>,
    /// When the group was last updated.
    pub updated_at: DateTime<Utc>,
    /// Highest severity in the group.
    pub max_severity: AlertLevel,
}

impl AlertGroup {
    /// Create a new alert group.
    pub fn new(labels: HashMap<String, String>) -> Self {
        let label_str: String = {
            let mut sorted: Vec<_> = labels.iter().collect();
            sorted.sort_by_key(|(k, _)| *k);
            sorted
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join(",")
        };
        let id = format!("group:{}", label_str);

        Self {
            id,
            labels,
            alerts: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            max_severity: AlertLevel::Info,
        }
    }

    /// Add an alert to the group.
    pub fn add_alert(&mut self, alert_id: String, severity: AlertLevel) {
        if !self.alerts.contains(&alert_id) {
            self.alerts.push(alert_id);
        }
        if severity > self.max_severity {
            self.max_severity = severity;
        }
        self.updated_at = Utc::now();
    }

    /// Remove an alert from the group.
    pub fn remove_alert(&mut self, alert_id: &str) {
        self.alerts.retain(|id| id != alert_id);
        self.updated_at = Utc::now();
    }

    /// Check if the group is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.alerts.is_empty()
    }

    /// Get alert count.
    #[must_use]
    pub fn count(&self) -> usize {
        self.alerts.len()
    }
}

/// Alert grouper for managing alert groups.
pub struct AlertGrouper {
    groups: Arc<RwLock<HashMap<String, AlertGroup>>>,
    group_by_keys: Vec<String>,
}

impl AlertGrouper {
    /// Create a new alert grouper with default grouping keys.
    pub fn new() -> Self {
        Self {
            groups: Arc::new(RwLock::new(HashMap::new())),
            group_by_keys: vec!["alertname".to_string(), "severity".to_string()],
        }
    }

    /// Create with custom grouping keys.
    pub fn with_keys(keys: Vec<String>) -> Self {
        Self {
            groups: Arc::new(RwLock::new(HashMap::new())),
            group_by_keys: keys,
        }
    }

    /// Get grouping labels for an alert.
    fn get_group_labels(&self, alert: &AlertInstance) -> HashMap<String, String> {
        let mut labels = HashMap::new();
        for key in &self.group_by_keys {
            if let Some(value) = alert.labels.get(key) {
                labels.insert(key.clone(), value.clone());
            }
        }
        labels
    }

    /// Add an alert to its group.
    pub fn add_alert(&self, alert: &AlertInstance) -> String {
        let group_labels = self.get_group_labels(alert);
        let mut groups = self.groups.write();

        let group_id = {
            let mut sorted: Vec<_> = group_labels.iter().collect();
            sorted.sort_by_key(|(k, _)| *k);
            let label_str: String = sorted
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join(",");
            format!("group:{}", label_str)
        };

        let group = groups
            .entry(group_id.clone())
            .or_insert_with(|| AlertGroup::new(group_labels));

        group.add_alert(alert.id.clone(), alert.level);
        group_id
    }

    /// Remove an alert from its group.
    pub fn remove_alert(&self, alert: &AlertInstance) {
        let mut groups = self.groups.write();
        let group_labels = self.get_group_labels(alert);

        let group_id = {
            let mut sorted: Vec<_> = group_labels.iter().collect();
            sorted.sort_by_key(|(k, _)| *k);
            let label_str: String = sorted
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join(",");
            format!("group:{}", label_str)
        };

        if let Some(group) = groups.get_mut(&group_id) {
            group.remove_alert(&alert.id);
            if group.is_empty() {
                groups.remove(&group_id);
            }
        }
    }

    /// Get all groups.
    pub fn get_groups(&self) -> Vec<AlertGroup> {
        self.groups.read().values().cloned().collect()
    }

    /// Get a specific group by ID.
    pub fn get_group(&self, group_id: &str) -> Option<AlertGroup> {
        self.groups.read().get(group_id).cloned()
    }
}

impl Default for AlertGrouper {
    fn default() -> Self {
        Self::new()
    }
}
