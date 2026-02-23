//! Alert deduplication.

use super::Alert;
use crate::error::Result;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

/// Alert deduplicator.
pub struct AlertDeduplicator {
    seen_alerts: Arc<RwLock<HashMap<String, Alert>>>,
}

impl AlertDeduplicator {
    /// Create a new alert deduplicator.
    pub fn new() -> Self {
        Self {
            seen_alerts: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Deduplicate alerts based on name and labels.
    pub fn deduplicate(&self, alerts: &[Alert]) -> Result<Vec<Alert>> {
        let mut seen = self.seen_alerts.write();
        let mut deduplicated = Vec::new();

        for alert in alerts {
            let key = format!("{}:{:?}", alert.name, alert.labels);

            if let std::collections::hash_map::Entry::Vacant(e) = seen.entry(key) {
                e.insert(alert.clone());
                deduplicated.push(alert.clone());
            }
        }

        Ok(deduplicated)
    }

    /// Clear deduplication cache.
    pub fn clear(&self) {
        self.seen_alerts.write().clear();
    }
}

impl Default for AlertDeduplicator {
    fn default() -> Self {
        Self::new()
    }
}
