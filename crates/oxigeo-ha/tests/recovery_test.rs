//! Recovery tests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use async_trait::async_trait;
use oxigeo_ha::error::HaResult;
use oxigeo_ha::recovery::snapshot::StateStore;
use oxigeo_ha::recovery::wal::{WalApplier, WalEntry};
use oxigeo_ha::recovery::{
    RecoveryConfig, RecoveryTarget, pitr::PitrManager, snapshot::SnapshotManager, wal::WalManager,
};
use parking_lot::RwLock;
use std::sync::Arc;
use uuid::Uuid;

/// Applier that records every replayed WAL entry payload.
#[derive(Default)]
struct RecordingApplier {
    applied: RwLock<Vec<Vec<u8>>>,
}

#[async_trait]
impl WalApplier for RecordingApplier {
    async fn apply(&self, entry: &WalEntry) -> HaResult<()> {
        self.applied.write().push(entry.data.clone());
        Ok(())
    }
}

/// In-memory state store for snapshots.
#[derive(Default)]
struct MemStore {
    state: RwLock<Vec<u8>>,
}

#[async_trait]
impl StateStore for MemStore {
    async fn capture(&self) -> HaResult<Vec<u8>> {
        Ok(self.state.read().clone())
    }
    async fn apply(&self, data: &[u8]) -> HaResult<()> {
        *self.state.write() = data.to_vec();
        Ok(())
    }
}

#[tokio::test]
async fn test_pitr_recovery_replays_real_wal() {
    let config = RecoveryConfig::default();
    let data_dir = std::env::temp_dir().join(format!("oxigeo-ha-it-pitr-{}", Uuid::new_v4()));
    let manager = PitrManager::new(config, data_dir);

    // Write real WAL entries.
    manager.wal().initialize().await.unwrap();
    manager.wal().write_entry(vec![1, 2, 3]).await.unwrap();
    manager.wal().write_entry(vec![4, 5, 6]).await.unwrap();

    let applier = Arc::new(RecordingApplier::default());
    manager.set_applier(Arc::clone(&applier) as Arc<dyn WalApplier>);

    let result = manager
        .recover(RecoveryTarget::Latest)
        .await
        .expect("PITR recovery should complete successfully");
    assert!(result.success);
    assert_eq!(result.transactions_replayed, 2);
    assert_eq!(
        applier.applied.read().clone(),
        vec![vec![1, 2, 3], vec![4, 5, 6]]
    );
}

#[tokio::test]
async fn test_pitr_without_applier_errors() {
    let config = RecoveryConfig::default();
    let data_dir = std::env::temp_dir().join(format!("oxigeo-ha-it-pitr-na-{}", Uuid::new_v4()));
    let manager = PitrManager::new(config, data_dir);
    // No applier → must not fabricate a success.
    assert!(manager.recover(RecoveryTarget::Latest).await.is_err());
}

#[tokio::test]
async fn test_snapshot_management() {
    let config = RecoveryConfig::default();
    let snapshot_dir =
        std::env::temp_dir().join(format!("oxigeo-ha-it-snapshots-{}", Uuid::new_v4()));
    tokio::fs::create_dir_all(&snapshot_dir).await.ok();

    let manager = SnapshotManager::new(config, snapshot_dir);
    let store = Arc::new(MemStore::default());
    let payload = b"real dataset state bytes".to_vec();
    *store.state.write() = payload.clone();
    manager.set_store(Arc::clone(&store) as Arc<dyn StateStore>);

    let metadata = manager
        .create_snapshot(1000)
        .await
        .expect("snapshot creation should return metadata");
    assert_eq!(metadata.transaction_id, 1000);
    assert_eq!(metadata.size_bytes, payload.len() as u64);

    let snapshots = manager
        .list_snapshots()
        .await
        .expect("should list snapshots");
    assert!(snapshots.iter().any(|s| s.id == metadata.id));

    // Restore actually writes the captured state back.
    *store.state.write() = Vec::new();
    manager.restore_snapshot(metadata.id).await.unwrap();
    assert_eq!(*store.state.read(), payload);
}

#[tokio::test]
async fn test_wal_operations() {
    let config = RecoveryConfig::default();
    let wal_dir = std::env::temp_dir().join(format!("oxigeo-ha-it-wal-{}", Uuid::new_v4()));
    tokio::fs::create_dir_all(&wal_dir).await.ok();

    let manager = WalManager::new(config, wal_dir);
    assert!(manager.initialize().await.is_ok());

    let entry = manager.write_entry(vec![1, 2, 3, 4, 5]).await.ok();
    assert!(entry.is_some());

    let entry = entry.expect("WAL entry write should succeed");
    assert_eq!(entry.transaction_id, 1);
    assert!(entry.verify_checksum().is_ok());
}
