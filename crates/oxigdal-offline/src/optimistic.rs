//! Optimistic update tracking for offline-first operations

use crate::error::{Error, Result};
use crate::types::{Operation, OperationId, Record, RecordId, Version};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use std::sync::Arc;

/// Tracker for optimistic updates
pub struct OptimisticTracker {
    /// Pending optimistic updates
    pending: Arc<DashMap<RecordId, OptimisticUpdate>>,
    /// Maximum age for optimistic updates (in seconds)
    max_age_secs: u64,
}

impl OptimisticTracker {
    /// Create a new optimistic tracker
    pub fn new(max_age_secs: u64) -> Self {
        Self {
            pending: Arc::new(DashMap::new()),
            max_age_secs,
        }
    }

    /// Create with default settings
    pub fn default_settings() -> Self {
        Self::new(300) // 5 minutes
    }

    /// Track an optimistic update
    pub fn track(&self, record: &Record, operation: &Operation) -> Result<()> {
        let update = OptimisticUpdate {
            record_id: record.id,
            operation_id: operation.id,
            original_version: operation.base_version,
            optimistic_version: operation.target_version,
            original_data: None, // Don't store original data to save memory
            timestamp: Utc::now(),
            confirmed: false,
        };

        self.pending.insert(record.id, update);

        tracing::debug!(
            record_id = %record.id,
            operation_id = %operation.id,
            "Tracked optimistic update"
        );

        Ok(())
    }

    /// Confirm an optimistic update
    pub fn confirm(&self, record_id: &RecordId, operation_id: &OperationId) -> Result<()> {
        if let Some(mut update) = self.pending.get_mut(record_id) {
            if update.operation_id == *operation_id {
                update.confirmed = true;

                tracing::debug!(
                    record_id = %record_id,
                    operation_id = %operation_id,
                    "Confirmed optimistic update"
                );

                return Ok(());
            }
        }

        Err(Error::not_found(format!(
            "No pending optimistic update for record {record_id}"
        )))
    }

    /// Rollback an optimistic update
    pub fn rollback(&self, record_id: &RecordId) -> Result<OptimisticUpdate> {
        let update = self
            .pending
            .remove(record_id)
            .ok_or_else(|| {
                Error::not_found(format!(
                    "No pending optimistic update for record {record_id}"
                ))
            })?
            .1;

        tracing::warn!(
            record_id = %record_id,
            operation_id = %update.operation_id,
            "Rolled back optimistic update"
        );

        Ok(update)
    }

    /// Get pending update for a record
    pub fn get_pending(&self, record_id: &RecordId) -> Option<OptimisticUpdate> {
        self.pending.get(record_id).map(|u| u.clone())
    }

    /// Check if a record has a pending optimistic update
    pub fn has_pending(&self, record_id: &RecordId) -> bool {
        self.pending.contains_key(record_id)
    }

    /// Get all pending updates
    pub fn all_pending(&self) -> Vec<OptimisticUpdate> {
        self.pending.iter().map(|u| u.value().clone()).collect()
    }

    /// Clean up old confirmed updates
    pub fn cleanup(&self) -> usize {
        let now = Utc::now();
        let max_age = chrono::Duration::seconds(self.max_age_secs as i64);

        let to_remove: Vec<_> = self
            .pending
            .iter()
            .filter(|entry| {
                let update = entry.value();
                update.confirmed && (now - update.timestamp) > max_age
            })
            .map(|entry| *entry.key())
            .collect();

        let count = to_remove.len();
        for record_id in to_remove {
            self.pending.remove(&record_id);
        }

        if count > 0 {
            tracing::debug!(cleaned_count = count, "Cleaned up old optimistic updates");
        }

        count
    }

    /// Prune all unconfirmed updates older than max age
    pub fn prune_old(&self) -> usize {
        let now = Utc::now();
        let max_age = chrono::Duration::seconds(self.max_age_secs as i64);

        let to_remove: Vec<_> = self
            .pending
            .iter()
            .filter(|entry| {
                let update = entry.value();
                !update.confirmed && (now - update.timestamp) > max_age
            })
            .map(|entry| *entry.key())
            .collect();

        let count = to_remove.len();
        for record_id in to_remove {
            self.pending.remove(&record_id);
        }

        if count > 0 {
            tracing::warn!(
                pruned_count = count,
                "Pruned old unconfirmed optimistic updates"
            );
        }

        count
    }

    /// Get statistics
    pub fn statistics(&self) -> OptimisticStatistics {
        let all_updates = self.all_pending();

        let total = all_updates.len();
        let confirmed = all_updates.iter().filter(|u| u.confirmed).count();
        let unconfirmed = total - confirmed;

        OptimisticStatistics {
            total_pending: total,
            confirmed,
            unconfirmed,
            oldest_pending: all_updates.iter().map(|u| u.timestamp).min(),
        }
    }
}

/// An optimistic update
#[derive(Debug, Clone)]
pub struct OptimisticUpdate {
    /// Record ID
    pub record_id: RecordId,
    /// Operation ID
    pub operation_id: OperationId,
    /// Original version before optimistic update
    pub original_version: Version,
    /// Optimistic version after update
    pub optimistic_version: Version,
    /// Original data (for rollback, optional to save memory)
    pub original_data: Option<bytes::Bytes>,
    /// Timestamp when update was made
    pub timestamp: DateTime<Utc>,
    /// Whether the update has been confirmed
    pub confirmed: bool,
}

impl OptimisticUpdate {
    /// Get age of the update
    pub fn age(&self) -> chrono::Duration {
        Utc::now() - self.timestamp
    }

    /// Check if the update is stale
    pub fn is_stale(&self, max_age_secs: u64) -> bool {
        self.age().num_seconds() > max_age_secs as i64
    }
}

/// Statistics for optimistic updates
#[derive(Debug, Clone)]
pub struct OptimisticStatistics {
    /// Total pending updates
    pub total_pending: usize,
    /// Confirmed but not yet cleaned up
    pub confirmed: usize,
    /// Unconfirmed updates
    pub unconfirmed: usize,
    /// Timestamp of oldest pending update
    pub oldest_pending: Option<DateTime<Utc>>,
}

impl OptimisticStatistics {
    /// Get a summary string
    pub fn summary(&self) -> String {
        format!(
            "Pending: {}, Confirmed: {}, Unconfirmed: {}",
            self.total_pending, self.confirmed, self.unconfirmed
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Record;
    use bytes::Bytes;

    #[test]
    fn test_track_and_confirm() {
        let tracker = OptimisticTracker::default_settings();

        let record = Record::new("test".to_string(), Bytes::from("data"));
        let operation = Operation::insert(&record);

        tracker.track(&record, &operation).expect("failed to track");

        assert!(tracker.has_pending(&record.id));

        tracker
            .confirm(&record.id, &operation.id)
            .expect("failed to confirm");

        let pending = tracker.get_pending(&record.id);
        assert!(pending.is_some());
        assert!(pending.expect("no pending").confirmed);
    }

    #[test]
    fn test_rollback() {
        let tracker = OptimisticTracker::default_settings();

        let record = Record::new("test".to_string(), Bytes::from("data"));
        let operation = Operation::insert(&record);

        tracker.track(&record, &operation).expect("failed to track");

        let rolled_back = tracker.rollback(&record.id).expect("failed to rollback");
        assert_eq!(rolled_back.record_id, record.id);
        assert!(!tracker.has_pending(&record.id));
    }

    #[test]
    fn test_cleanup() {
        let tracker = OptimisticTracker::new(0); // 0 seconds max age for immediate cleanup

        let record = Record::new("test".to_string(), Bytes::from("data"));
        let operation = Operation::insert(&record);

        tracker.track(&record, &operation).expect("failed to track");
        tracker
            .confirm(&record.id, &operation.id)
            .expect("failed to confirm");

        // Wait a bit
        std::thread::sleep(std::time::Duration::from_millis(10));

        let cleaned = tracker.cleanup();
        assert_eq!(cleaned, 1);
        assert!(!tracker.has_pending(&record.id));
    }

    #[test]
    fn test_statistics() {
        let tracker = OptimisticTracker::default_settings();

        let record1 = Record::new("test1".to_string(), Bytes::from("data1"));
        let record2 = Record::new("test2".to_string(), Bytes::from("data2"));

        let op1 = Operation::insert(&record1);
        let op2 = Operation::insert(&record2);

        tracker.track(&record1, &op1).expect("failed to track");
        tracker.track(&record2, &op2).expect("failed to track");
        tracker
            .confirm(&record1.id, &op1.id)
            .expect("failed to confirm");

        let stats = tracker.statistics();
        assert_eq!(stats.total_pending, 2);
        assert_eq!(stats.confirmed, 1);
        assert_eq!(stats.unconfirmed, 1);
    }

    #[test]
    fn test_prune_old() {
        let tracker = OptimisticTracker::new(0); // 0 seconds for immediate pruning

        let record = Record::new("test".to_string(), Bytes::from("data"));
        let operation = Operation::insert(&record);

        tracker.track(&record, &operation).expect("failed to track");

        // Wait a bit
        std::thread::sleep(std::time::Duration::from_millis(10));

        let pruned = tracker.prune_old();
        assert_eq!(pruned, 1);
        assert!(!tracker.has_pending(&record.id));
    }
}
