//! Recovery tests.

use oxigdal_ha::recovery::{
    RecoveryConfig, RecoveryTarget, pitr::PitrManager, snapshot::SnapshotManager, wal::WalManager,
};

#[tokio::test]
async fn test_pitr_recovery() {
    let config = RecoveryConfig::default();
    let data_dir = std::env::temp_dir().join("oxigdal-ha-test-pitr");
    let manager = PitrManager::new(config, data_dir);

    let result = manager.recover(RecoveryTarget::Latest).await.ok();
    assert!(result.is_some());

    let result = result.expect("PITR recovery should complete successfully");
    assert!(result.success);
    assert!(result.transactions_replayed > 0);
}

#[tokio::test]
async fn test_snapshot_management() {
    let config = RecoveryConfig::default();
    let snapshot_dir = std::env::temp_dir().join("oxigdal-ha-test-snapshots");
    tokio::fs::create_dir_all(&snapshot_dir).await.ok();

    let manager = SnapshotManager::new(config, snapshot_dir);

    let metadata = manager.create_snapshot(1000).await.ok();
    assert!(metadata.is_some());

    let metadata = metadata.expect("snapshot creation should return metadata");
    assert_eq!(metadata.transaction_id, 1000);

    let snapshots = manager.list_snapshots().await.ok();
    assert!(snapshots.is_some());
}

#[tokio::test]
async fn test_wal_operations() {
    let config = RecoveryConfig::default();
    let wal_dir = std::env::temp_dir().join("oxigdal-ha-test-wal");
    tokio::fs::create_dir_all(&wal_dir).await.ok();

    let manager = WalManager::new(config, wal_dir);
    assert!(manager.initialize().await.is_ok());

    let entry = manager.write_entry(vec![1, 2, 3, 4, 5]).await.ok();
    assert!(entry.is_some());

    let entry = entry.expect("WAL entry write should succeed");
    assert_eq!(entry.transaction_id, 1);
    assert!(entry.verify_checksum().is_ok());
}
