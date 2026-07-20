//! Synchronization protocols for edge-to-cloud data sync

pub mod manager;
pub mod protocol;

// Error types imported per-module as needed
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub use manager::SyncManager;
pub use protocol::SyncProtocol;

/// Synchronization strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncStrategy {
    /// Manual sync only
    Manual,
    /// Periodic sync at fixed intervals
    Periodic,
    /// Incremental sync of changes only
    Incremental,
    /// Batch sync with compression
    Batch,
    /// Real-time sync (when connected)
    Realtime,
}

/// Synchronization status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncStatus {
    /// Not synced yet
    NotSynced,
    /// Sync in progress
    Syncing,
    /// Successfully synced
    Synced,
    /// Sync failed
    Failed(String),
    /// Sync pending
    Pending,
}

impl SyncStatus {
    /// Check if sync is complete
    pub fn is_complete(&self) -> bool {
        matches!(self, Self::Synced)
    }

    /// Check if sync is in progress
    pub fn is_syncing(&self) -> bool {
        matches!(self, Self::Syncing)
    }

    /// Check if sync failed
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed(_))
    }
}

/// Sync metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncMetadata {
    /// Unique sync ID
    pub sync_id: String,
    /// Sync strategy used
    pub strategy: SyncStrategy,
    /// Sync status
    pub status: SyncStatus,
    /// Start timestamp
    pub started_at: DateTime<Utc>,
    /// Completion timestamp
    pub completed_at: Option<DateTime<Utc>>,
    /// Number of items synced
    pub items_synced: usize,
    /// Total bytes transferred
    pub bytes_transferred: usize,
    /// Error message if failed
    pub error: Option<String>,
}

impl SyncMetadata {
    /// Create new sync metadata
    pub fn new(sync_id: String, strategy: SyncStrategy) -> Self {
        Self {
            sync_id,
            strategy,
            status: SyncStatus::Pending,
            started_at: Utc::now(),
            completed_at: None,
            items_synced: 0,
            bytes_transferred: 0,
            error: None,
        }
    }

    /// Mark sync as started
    pub fn start(&mut self) {
        self.status = SyncStatus::Syncing;
        self.started_at = Utc::now();
    }

    /// Mark sync as completed
    pub fn complete(&mut self, items: usize, bytes: usize) {
        self.status = SyncStatus::Synced;
        self.completed_at = Some(Utc::now());
        self.items_synced = items;
        self.bytes_transferred = bytes;
    }

    /// Mark sync as failed
    pub fn fail(&mut self, error: String) {
        self.status = SyncStatus::Failed(error.clone());
        self.completed_at = Some(Utc::now());
        self.error = Some(error);
    }

    /// Get sync duration
    pub fn duration(&self) -> Option<chrono::Duration> {
        self.completed_at.map(|end| end - self.started_at)
    }

    /// Get throughput in bytes per second
    pub fn throughput(&self) -> Option<f64> {
        self.duration().map(|d| {
            let secs = d.num_milliseconds() as f64 / 1000.0;
            if secs > 0.0 {
                self.bytes_transferred as f64 / secs
            } else {
                0.0
            }
        })
    }
}

/// Sync item representing a piece of data to sync
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncItem {
    /// Item ID
    pub id: String,
    /// Item key
    pub key: String,
    /// Item data
    pub data: Vec<u8>,
    /// Item version
    pub version: u64,
    /// Last modified timestamp
    pub modified_at: DateTime<Utc>,
    /// Checksum for validation
    pub checksum: String,
}

impl SyncItem {
    /// Create new sync item
    pub fn new(id: String, key: String, data: Vec<u8>, version: u64) -> Self {
        let checksum = Self::calculate_checksum(&data);
        Self {
            id,
            key,
            data,
            version,
            modified_at: Utc::now(),
            checksum,
        }
    }

    /// Calculate checksum using blake3
    fn calculate_checksum(data: &[u8]) -> String {
        let hash = blake3::hash(data);
        hash.to_hex().to_string()
    }

    /// Verify checksum
    pub fn verify_checksum(&self) -> bool {
        Self::calculate_checksum(&self.data) == self.checksum
    }

    /// Get data size
    pub fn size(&self) -> usize {
        self.data.len()
    }
}

/// Sync batch for batch synchronization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncBatch {
    /// Batch ID
    pub batch_id: String,
    /// Items in batch
    pub items: Vec<SyncItem>,
    /// Batch creation time
    pub created_at: DateTime<Utc>,
    /// Compression applied
    pub compressed: bool,
}

impl SyncBatch {
    /// Create new sync batch
    pub fn new(batch_id: String) -> Self {
        Self {
            batch_id,
            items: Vec::new(),
            created_at: Utc::now(),
            compressed: false,
        }
    }

    /// Add item to batch
    pub fn add_item(&mut self, item: SyncItem) {
        self.items.push(item);
    }

    /// Get batch size in bytes
    pub fn size(&self) -> usize {
        self.items.iter().map(|item| item.size()).sum()
    }

    /// Get number of items
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Check if batch is empty
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Sync state tracker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncState {
    /// Last sync timestamp
    pub last_sync: Option<DateTime<Utc>>,
    /// Items pending sync
    pub pending_items: HashMap<String, SyncItem>,
    /// Current sync metadata
    pub current_sync: Option<SyncMetadata>,
    /// Sync history
    pub history: Vec<SyncMetadata>,
}

impl Default for SyncState {
    fn default() -> Self {
        Self::new()
    }
}

impl SyncState {
    /// Create new sync state
    pub fn new() -> Self {
        Self {
            last_sync: None,
            pending_items: HashMap::new(),
            current_sync: None,
            history: Vec::new(),
        }
    }

    /// Add pending item
    pub fn add_pending(&mut self, item: SyncItem) {
        self.pending_items.insert(item.id.clone(), item);
    }

    /// Remove pending item
    pub fn remove_pending(&mut self, item_id: &str) -> Option<SyncItem> {
        self.pending_items.remove(item_id)
    }

    /// Get pending items count
    pub fn pending_count(&self) -> usize {
        self.pending_items.len()
    }

    /// Start new sync
    pub fn start_sync(&mut self, metadata: SyncMetadata) {
        self.current_sync = Some(metadata);
    }

    /// Complete current sync
    pub fn complete_sync(&mut self) {
        if let Some(mut sync) = self.current_sync.take() {
            sync.complete(0, 0);
            self.last_sync = Some(Utc::now());
            self.history.push(sync);

            // Keep only last 100 syncs in history
            if self.history.len() > 100 {
                self.history.remove(0);
            }
        }
    }

    /// Fail current sync
    pub fn fail_sync(&mut self, error: String) {
        if let Some(mut sync) = self.current_sync.take() {
            sync.fail(error);
            self.history.push(sync);

            // Keep only last 100 syncs in history
            if self.history.len() > 100 {
                self.history.remove(0);
            }
        }
    }

    /// Get sync statistics
    pub fn statistics(&self) -> SyncStatistics {
        let total_syncs = self.history.len();
        let successful = self
            .history
            .iter()
            .filter(|s| s.status.is_complete())
            .count();
        let failed = self.history.iter().filter(|s| s.status.is_failed()).count();

        let avg_throughput = if successful > 0 {
            let sum: f64 = self.history.iter().filter_map(|s| s.throughput()).sum();
            sum / successful as f64
        } else {
            0.0
        };

        SyncStatistics {
            total_syncs,
            successful_syncs: successful,
            failed_syncs: failed,
            pending_items: self.pending_count(),
            last_sync: self.last_sync,
            avg_throughput_bps: avg_throughput,
        }
    }
}

/// Sync statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStatistics {
    /// Total number of syncs
    pub total_syncs: usize,
    /// Successful syncs
    pub successful_syncs: usize,
    /// Failed syncs
    pub failed_syncs: usize,
    /// Pending items
    pub pending_items: usize,
    /// Last sync timestamp
    pub last_sync: Option<DateTime<Utc>>,
    /// Average throughput in bytes per second
    pub avg_throughput_bps: f64,
}

impl SyncStatistics {
    /// Get success rate
    pub fn success_rate(&self) -> f64 {
        if self.total_syncs == 0 {
            return 100.0;
        }
        (self.successful_syncs as f64 / self.total_syncs as f64) * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_metadata() {
        let mut metadata = SyncMetadata::new("sync-1".to_string(), SyncStrategy::Incremental);
        assert_eq!(metadata.status, SyncStatus::Pending);

        metadata.start();
        assert_eq!(metadata.status, SyncStatus::Syncing);

        metadata.complete(10, 1024);
        assert_eq!(metadata.status, SyncStatus::Synced);
        assert_eq!(metadata.items_synced, 10);
        assert_eq!(metadata.bytes_transferred, 1024);
    }

    #[test]
    fn test_sync_item() {
        let item = SyncItem::new(
            "item-1".to_string(),
            "key-1".to_string(),
            vec![1, 2, 3, 4, 5],
            1,
        );

        assert_eq!(item.size(), 5);
        assert!(item.verify_checksum());
    }

    #[test]
    fn test_sync_batch() {
        let mut batch = SyncBatch::new("batch-1".to_string());
        assert!(batch.is_empty());

        let item = SyncItem::new("item-1".to_string(), "key-1".to_string(), vec![1, 2, 3], 1);
        batch.add_item(item);

        assert_eq!(batch.len(), 1);
        assert_eq!(batch.size(), 3);
    }

    #[test]
    fn test_sync_state() {
        let mut state = SyncState::new();
        assert_eq!(state.pending_count(), 0);

        let item = SyncItem::new("item-1".to_string(), "key-1".to_string(), vec![1, 2, 3], 1);
        state.add_pending(item);

        assert_eq!(state.pending_count(), 1);

        let removed = state.remove_pending("item-1");
        assert!(removed.is_some());
        assert_eq!(state.pending_count(), 0);
    }

    #[test]
    fn test_sync_statistics() {
        let mut state = SyncState::new();

        for i in 0..5 {
            let mut metadata = SyncMetadata::new(format!("sync-{}", i), SyncStrategy::Incremental);
            metadata.start();
            metadata.complete(10, 1024);
            state.history.push(metadata);
        }

        let stats = state.statistics();
        assert_eq!(stats.total_syncs, 5);
        assert_eq!(stats.successful_syncs, 5);
        assert_eq!(stats.success_rate(), 100.0);
    }
}
