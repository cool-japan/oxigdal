//! Alert history tracking

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
/// Event type for alert history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertHistoryEventType {
    /// Alert was created.
    Created,
    /// Alert transitioned to pending.
    Pending,
    /// Alert started firing.
    Firing,
    /// Alert was resolved.
    Resolved,
    /// Alert was silenced.
    Silenced,
    /// Alert was acknowledged.
    Acknowledged,
    /// Notification was sent.
    NotificationSent { channel: String },
    /// Notification failed.
    NotificationFailed { channel: String, error: String },
    /// Labels were updated.
    LabelsUpdated,
}

/// Alert history event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertHistoryEvent {
    /// Event ID.
    pub id: String,
    /// Alert ID.
    pub alert_id: String,
    /// Event type.
    pub event_type: AlertHistoryEventType,
    /// When the event occurred.
    pub timestamp: DateTime<Utc>,
    /// Additional details.
    pub details: Option<String>,
}

impl AlertHistoryEvent {
    /// Create a new history event.
    pub fn new(alert_id: impl Into<String>, event_type: AlertHistoryEventType) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            alert_id: alert_id.into(),
            event_type,
            timestamp: Utc::now(),
            details: None,
        }
    }

    /// Add details to the event.
    #[must_use]
    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }
}

/// Alert history tracker.
pub struct AlertHistory {
    /// Events stored by alert ID.
    events: Arc<RwLock<HashMap<String, VecDeque<AlertHistoryEvent>>>>,
    /// Maximum events per alert.
    max_events_per_alert: usize,
    /// Global event log (recent events across all alerts).
    global_log: Arc<RwLock<VecDeque<AlertHistoryEvent>>>,
    /// Maximum global log size.
    max_global_log_size: usize,
}

impl AlertHistory {
    /// Create a new alert history tracker.
    pub fn new() -> Self {
        Self {
            events: Arc::new(RwLock::new(HashMap::new())),
            max_events_per_alert: 100,
            global_log: Arc::new(RwLock::new(VecDeque::new())),
            max_global_log_size: 10000,
        }
    }

    /// Create with custom limits.
    pub fn with_limits(max_per_alert: usize, max_global: usize) -> Self {
        Self {
            events: Arc::new(RwLock::new(HashMap::new())),
            max_events_per_alert: max_per_alert,
            global_log: Arc::new(RwLock::new(VecDeque::new())),
            max_global_log_size: max_global,
        }
    }

    /// Record an event.
    pub fn record(&self, event: AlertHistoryEvent) {
        // Add to per-alert history
        {
            let mut events = self.events.write();
            let alert_events = events.entry(event.alert_id.clone()).or_default();
            alert_events.push_back(event.clone());
            while alert_events.len() > self.max_events_per_alert {
                alert_events.pop_front();
            }
        }

        // Add to global log
        {
            let mut global = self.global_log.write();
            global.push_back(event);
            while global.len() > self.max_global_log_size {
                global.pop_front();
            }
        }
    }

    /// Get history for a specific alert.
    pub fn get_alert_history(&self, alert_id: &str) -> Vec<AlertHistoryEvent> {
        self.events
            .read()
            .get(alert_id)
            .map(|events| events.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Get recent global events.
    pub fn get_recent_events(&self, limit: usize) -> Vec<AlertHistoryEvent> {
        self.global_log
            .read()
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect()
    }

    /// Get events in a time range.
    pub fn get_events_in_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Vec<AlertHistoryEvent> {
        self.global_log
            .read()
            .iter()
            .filter(|e| e.timestamp >= start && e.timestamp <= end)
            .cloned()
            .collect()
    }

    /// Get event count for an alert.
    pub fn event_count(&self, alert_id: &str) -> usize {
        self.events
            .read()
            .get(alert_id)
            .map(|e| e.len())
            .unwrap_or(0)
    }

    /// Clear history for a specific alert.
    pub fn clear_alert_history(&self, alert_id: &str) {
        self.events.write().remove(alert_id);
    }

    /// Clear all history.
    pub fn clear_all(&self) {
        self.events.write().clear();
        self.global_log.write().clear();
    }
}

impl Default for AlertHistory {
    fn default() -> Self {
        Self::new()
    }
}
