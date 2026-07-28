//! Incremental backup implementation.

use super::{BackupCompression, BackupMetadata, BackupSource, BackupType};
use crate::error::{HaError, HaResult};
use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

/// Incremental backup manager.
pub struct IncrementalBackup {
    /// Backup directory.
    backup_dir: PathBuf,
    /// Compression type.
    compression: BackupCompression,
    /// Source that supplies the real changed bytes.
    source: RwLock<Option<Arc<dyn BackupSource>>>,
}

impl IncrementalBackup {
    /// Get the backup directory.
    pub fn backup_dir(&self) -> &PathBuf {
        &self.backup_dir
    }
}

impl IncrementalBackup {
    /// Create a new incremental backup manager.
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

    /// Create an incremental backup (changes since `parent_id`), persisted to disk.
    pub async fn create(&self, parent_id: Option<Uuid>) -> HaResult<BackupMetadata> {
        info!("Creating incremental backup (parent: {:?})", parent_id);

        let data = self.source()?.read_changes_since(parent_id).await?;
        let metadata = super::persist_backup(
            &self.backup_dir,
            BackupType::Incremental,
            self.compression,
            parent_id,
            &data,
        )
        .await?;

        info!(
            "Incremental backup {} persisted ({} bytes) to {}",
            metadata.id,
            metadata.size_bytes,
            self.backup_dir.display()
        );

        Ok(metadata)
    }

    /// Restore an incremental backup payload by reading it back and applying it.
    pub async fn restore(&self, backup_id: Uuid) -> HaResult<()> {
        info!("Restoring incremental backup {}", backup_id);
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
        last_since: RwLock<Option<Uuid>>,
    }

    #[async_trait]
    impl BackupSource for MemSource {
        async fn read_full(&self) -> HaResult<Vec<u8>> {
            Ok(self.state.read().clone())
        }
        async fn read_changes_since(&self, since: Option<Uuid>) -> HaResult<Vec<u8>> {
            *self.last_since.write() = since;
            Ok(self.state.read().clone())
        }
        async fn apply(&self, data: &[u8]) -> HaResult<()> {
            *self.state.write() = data.to_vec();
            Ok(())
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("oxigeo-ha-incr-{}-{}", tag, Uuid::new_v4()))
    }

    #[tokio::test]
    async fn test_incremental_without_source_errors() {
        let backup = IncrementalBackup::new(temp_dir("no-source"), BackupCompression::Lz4);
        assert!(backup.create(None).await.is_err());
    }

    #[tokio::test]
    async fn test_incremental_forwards_parent_and_persists() {
        let dir = temp_dir("parent");
        let backup = IncrementalBackup::new(dir.clone(), BackupCompression::Lz4);
        let source = Arc::new(MemSource::default());
        *source.state.write() = b"delta-bytes".to_vec();
        backup.set_source(Arc::clone(&source) as Arc<dyn BackupSource>);

        let parent = Uuid::new_v4();
        let metadata = backup.create(Some(parent)).await.unwrap();
        assert_eq!(metadata.backup_type, BackupType::Incremental);
        assert_eq!(metadata.parent_id, Some(parent));
        // The source was actually queried with the parent watermark.
        assert_eq!(*source.last_since.read(), Some(parent));

        let data_path = dir.join(format!("{}.backup", metadata.id));
        assert!(tokio::fs::metadata(&data_path).await.is_ok());

        *source.state.write() = Vec::new();
        backup.restore(metadata.id).await.unwrap();
        assert_eq!(*source.state.read(), b"delta-bytes");
    }
}
