//! Backup tests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use async_trait::async_trait;
use oxigeo_ha::backup::{
    BackupCompression, BackupSource, BackupType, differential::DifferentialBackup,
    full::FullBackup, incremental::IncrementalBackup,
};
use oxigeo_ha::error::HaResult;
use parking_lot::RwLock;
use std::sync::Arc;
use uuid::Uuid;

/// In-memory backup source that yields (and restores) real dataset bytes.
#[derive(Default)]
struct MemSource {
    state: RwLock<Vec<u8>>,
}

#[async_trait]
impl BackupSource for MemSource {
    async fn read_full(&self) -> HaResult<Vec<u8>> {
        Ok(self.state.read().clone())
    }
    async fn read_changes_since(&self, _since: Option<Uuid>) -> HaResult<Vec<u8>> {
        Ok(self.state.read().clone())
    }
    async fn apply(&self, data: &[u8]) -> HaResult<()> {
        *self.state.write() = data.to_vec();
        Ok(())
    }
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("oxigeo-ha-it-{}-{}", tag, Uuid::new_v4()))
}

#[tokio::test]
async fn test_full_backup_persists_to_disk() {
    let dir = temp_dir("full-backup");
    let backup = FullBackup::new(dir.clone(), BackupCompression::Zstd);
    let source = Arc::new(MemSource::default());
    let payload: Vec<u8> = (0u8..=255).cycle().take(4096).collect();
    *source.state.write() = payload.clone();
    backup.set_source(Arc::clone(&source) as Arc<dyn BackupSource>);

    let metadata = backup
        .create()
        .await
        .expect("full backup creation should return metadata");
    assert_eq!(metadata.backup_type, BackupType::Full);
    assert_eq!(metadata.compression, BackupCompression::Zstd);

    // A real backup file exists.
    let data_path = dir.join(format!("{}.backup", metadata.id));
    assert!(tokio::fs::metadata(&data_path).await.is_ok());

    // Restore rewrites the original bytes.
    *source.state.write() = Vec::new();
    backup.restore(metadata.id).await.unwrap();
    assert_eq!(*source.state.read(), payload);
}

#[tokio::test]
async fn test_incremental_backup() {
    let dir = temp_dir("incr-backup");
    let backup = IncrementalBackup::new(dir.clone(), BackupCompression::Lz4);
    let source = Arc::new(MemSource::default());
    *source.state.write() = b"incremental delta".to_vec();
    backup.set_source(Arc::clone(&source) as Arc<dyn BackupSource>);

    let parent_id = Uuid::new_v4();
    let metadata = backup
        .create(Some(parent_id))
        .await
        .expect("incremental backup creation should return metadata");
    assert_eq!(metadata.backup_type, BackupType::Incremental);
    assert_eq!(metadata.parent_id, Some(parent_id));

    let data_path = dir.join(format!("{}.backup", metadata.id));
    assert!(tokio::fs::metadata(&data_path).await.is_ok());
}

#[tokio::test]
async fn test_differential_backup() {
    let dir = temp_dir("diff-backup");
    let backup = DifferentialBackup::new(dir.clone(), BackupCompression::Gzip);
    let source = Arc::new(MemSource::default());
    *source.state.write() = b"differential change set".to_vec();
    backup.set_source(Arc::clone(&source) as Arc<dyn BackupSource>);

    let full_backup_id = Uuid::new_v4();
    let metadata = backup
        .create(full_backup_id)
        .await
        .expect("differential backup creation should return metadata");
    assert_eq!(metadata.backup_type, BackupType::Differential);
    assert_eq!(metadata.parent_id, Some(full_backup_id));

    let data_path = dir.join(format!("{}.backup", metadata.id));
    assert!(tokio::fs::metadata(&data_path).await.is_ok());
}

#[tokio::test]
async fn test_backup_without_source_errors() {
    let backup = FullBackup::new(temp_dir("no-source"), BackupCompression::None);
    assert!(backup.create().await.is_err());
}
