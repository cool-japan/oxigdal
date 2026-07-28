//! Full backup implementation.

use super::{BackupCompression, BackupMetadata, BackupSource, BackupType};
use crate::error::{HaError, HaResult};
use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

/// Full backup manager.
pub struct FullBackup {
    /// Backup directory.
    backup_dir: PathBuf,
    /// Compression type.
    compression: BackupCompression,
    /// Source that supplies the real dataset bytes.
    source: RwLock<Option<Arc<dyn BackupSource>>>,
}

impl FullBackup {
    /// Get the backup directory.
    pub fn backup_dir(&self) -> &PathBuf {
        &self.backup_dir
    }
}

impl FullBackup {
    /// Create a new full backup manager.
    pub fn new(backup_dir: PathBuf, compression: BackupCompression) -> Self {
        Self {
            backup_dir,
            compression,
            source: RwLock::new(None),
        }
    }

    /// Inject the backup source. Required before [`create`](Self::create).
    pub fn set_source(&self, source: Arc<dyn BackupSource>) {
        *self.source.write() = Some(source);
    }

    fn source(&self) -> HaResult<Arc<dyn BackupSource>> {
        self.source.read().clone().ok_or_else(|| {
            HaError::Backup("no backup source configured; refusing to persist canned bytes".into())
        })
    }

    /// Create a full backup, collecting real data and persisting it to disk.
    pub async fn create(&self) -> HaResult<BackupMetadata> {
        info!("Creating full backup");

        let data = self.source()?.read_full().await?;
        let metadata = super::persist_backup(
            &self.backup_dir,
            BackupType::Full,
            self.compression,
            None,
            &data,
        )
        .await?;

        info!(
            "Full backup {} persisted ({} bytes) to {}",
            metadata.id,
            metadata.size_bytes,
            self.backup_dir.display()
        );

        Ok(metadata)
    }

    /// Restore a full backup by reading it back from disk and applying it.
    pub async fn restore(&self, backup_id: Uuid) -> HaResult<()> {
        info!("Restoring full backup {}", backup_id);
        let metadata = super::load_metadata(&self.backup_dir, backup_id).await?;
        let data = super::read_backup_payload(&self.backup_dir, &metadata).await?;
        self.source()?.apply(&data).await
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use async_trait::async_trait;

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

    fn temp_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("oxigeo-ha-full-{}-{}", tag, Uuid::new_v4()))
    }

    #[tokio::test]
    async fn test_full_backup_without_source_errors() {
        let backup = FullBackup::new(temp_dir("no-source"), BackupCompression::Zstd);
        assert!(backup.create().await.is_err());
    }

    #[tokio::test]
    async fn test_full_backup_persists_and_restores() {
        let dir = temp_dir("roundtrip");
        let backup = FullBackup::new(dir.clone(), BackupCompression::Zstd);
        let source = Arc::new(MemSource::default());
        let payload: Vec<u8> = (0u8..=255).cycle().take(8192).collect();
        *source.state.write() = payload.clone();
        backup.set_source(Arc::clone(&source) as Arc<dyn BackupSource>);

        let metadata = backup.create().await.unwrap();
        assert_eq!(metadata.backup_type, BackupType::Full);
        assert_eq!(metadata.size_bytes, payload.len() as u64);

        // The backup file actually exists on disk.
        let data_path = dir.join(format!("{}.backup", metadata.id));
        assert!(tokio::fs::metadata(&data_path).await.is_ok());

        // Mutate live state then restore — the original bytes come back.
        *source.state.write() = vec![9; 3];
        backup.restore(metadata.id).await.unwrap();
        assert_eq!(*source.state.read(), payload);
    }

    #[tokio::test]
    async fn test_full_backup_uncompressed() {
        let dir = temp_dir("uncompressed");
        let backup = FullBackup::new(dir, BackupCompression::None);
        let source = Arc::new(MemSource::default());
        *source.state.write() = b"raw".to_vec();
        backup.set_source(Arc::clone(&source) as Arc<dyn BackupSource>);

        let metadata = backup.create().await.unwrap();
        assert!(metadata.compressed_size_bytes.is_none());
    }
}
