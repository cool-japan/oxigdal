//! Synchronization protocol implementation

use super::{SyncItem, SyncMetadata, SyncStrategy};
use crate::error::{EdgeError, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Sync protocol trait
#[async_trait]
pub trait SyncProtocol: Send + Sync {
    /// Push data to remote
    async fn push(&self, items: Vec<SyncItem>) -> Result<SyncMetadata>;

    /// Pull data from remote
    async fn pull(&self, since: Option<chrono::DateTime<chrono::Utc>>) -> Result<Vec<SyncItem>>;

    /// Sync bidirectionally
    async fn sync(&self, local_items: Vec<SyncItem>) -> Result<SyncResult>;

    /// Check connectivity
    async fn is_connected(&self) -> bool;
}

/// Sync result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    /// Items pushed to remote
    pub pushed: Vec<String>,
    /// Items pulled from remote
    pub pulled: Vec<SyncItem>,
    /// Sync metadata
    pub metadata: SyncMetadata,
    /// Conflicts detected
    pub conflicts: Vec<Conflict>,
}

impl SyncResult {
    /// Create new sync result
    pub fn new(metadata: SyncMetadata) -> Self {
        Self {
            pushed: Vec::new(),
            pulled: Vec::new(),
            metadata,
            conflicts: Vec::new(),
        }
    }

    /// Check if sync was successful
    pub fn is_successful(&self) -> bool {
        self.metadata.status.is_complete()
    }

    /// Get total items synced
    pub fn total_items(&self) -> usize {
        self.pushed.len() + self.pulled.len()
    }
}

/// Conflict between local and remote data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    /// Item ID
    pub item_id: String,
    /// Local version
    pub local_version: u64,
    /// Remote version
    pub remote_version: u64,
    /// Conflict resolution strategy
    pub resolution: ConflictResolution,
}

/// Conflict resolution strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictResolution {
    /// Use local version
    UseLocal,
    /// Use remote version
    UseRemote,
    /// Merge versions
    Merge,
    /// Manual resolution required
    Manual,
}

/// Mock sync protocol for testing and offline mode
pub struct MockSyncProtocol {
    storage: parking_lot::RwLock<HashMap<String, SyncItem>>,
    connected: parking_lot::RwLock<bool>,
}

impl Default for MockSyncProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl MockSyncProtocol {
    /// Create new mock protocol
    pub fn new() -> Self {
        Self {
            storage: parking_lot::RwLock::new(HashMap::new()),
            connected: parking_lot::RwLock::new(true),
        }
    }

    /// Set connection status
    pub fn set_connected(&self, connected: bool) {
        *self.connected.write() = connected;
    }
}

#[async_trait]
impl SyncProtocol for MockSyncProtocol {
    async fn push(&self, items: Vec<SyncItem>) -> Result<SyncMetadata> {
        if !self.is_connected().await {
            return Err(EdgeError::network("Not connected"));
        }

        let mut storage = self.storage.write();
        let mut metadata = SyncMetadata::new(
            format!("sync-{}", chrono::Utc::now().timestamp()),
            SyncStrategy::Manual,
        );

        metadata.start();

        for item in items {
            storage.insert(item.id.clone(), item);
        }

        metadata.complete(storage.len(), 0);
        Ok(metadata)
    }

    async fn pull(&self, _since: Option<chrono::DateTime<chrono::Utc>>) -> Result<Vec<SyncItem>> {
        if !self.is_connected().await {
            return Err(EdgeError::network("Not connected"));
        }

        let storage = self.storage.read();
        Ok(storage.values().cloned().collect())
    }

    async fn sync(&self, local_items: Vec<SyncItem>) -> Result<SyncResult> {
        if !self.is_connected().await {
            return Err(EdgeError::network("Not connected"));
        }

        let mut metadata = SyncMetadata::new(
            format!("sync-{}", chrono::Utc::now().timestamp()),
            SyncStrategy::Manual,
        );

        metadata.start();

        // Push local items
        let mut storage = self.storage.write();
        let mut pushed = Vec::new();

        for item in &local_items {
            storage.insert(item.id.clone(), item.clone());
            pushed.push(item.id.clone());
        }

        // Pull remote items (simplified)
        let pulled: Vec<SyncItem> = storage
            .values()
            .filter(|item| !local_items.iter().any(|l| l.id == item.id))
            .cloned()
            .collect();

        metadata.complete(pushed.len() + pulled.len(), 0);

        Ok(SyncResult {
            pushed,
            pulled,
            metadata,
            conflicts: Vec::new(),
        })
    }

    async fn is_connected(&self) -> bool {
        *self.connected.read()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_protocol_push() -> Result<()> {
        let protocol = MockSyncProtocol::new();

        let item = SyncItem::new("item-1".to_string(), "key-1".to_string(), vec![1, 2, 3], 1);

        let metadata = protocol.push(vec![item]).await?;
        assert!(metadata.status.is_complete());

        Ok(())
    }

    #[tokio::test]
    async fn test_mock_protocol_pull() -> Result<()> {
        let protocol = MockSyncProtocol::new();

        let item = SyncItem::new("item-1".to_string(), "key-1".to_string(), vec![1, 2, 3], 1);

        protocol.push(vec![item.clone()]).await?;

        let items = protocol.pull(None).await?;
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, item.id);

        Ok(())
    }

    #[tokio::test]
    async fn test_mock_protocol_sync() -> Result<()> {
        let protocol = MockSyncProtocol::new();

        let item = SyncItem::new("item-1".to_string(), "key-1".to_string(), vec![1, 2, 3], 1);

        let result = protocol.sync(vec![item]).await?;
        assert!(result.is_successful());
        assert_eq!(result.pushed.len(), 1);

        Ok(())
    }

    #[tokio::test]
    async fn test_mock_protocol_connectivity() -> Result<()> {
        let protocol = MockSyncProtocol::new();

        assert!(protocol.is_connected().await);

        protocol.set_connected(false);
        assert!(!protocol.is_connected().await);

        Ok(())
    }

    #[tokio::test]
    async fn test_mock_protocol_offline() {
        let protocol = MockSyncProtocol::new();
        protocol.set_connected(false);

        let item = SyncItem::new("item-1".to_string(), "key-1".to_string(), vec![1, 2, 3], 1);

        let result = protocol.push(vec![item]).await;
        assert!(result.is_err());
    }
}
