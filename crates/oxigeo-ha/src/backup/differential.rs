//! Differential backup implementation.

use super::{BackupCompression, BackupMetadata, BackupSource, BackupType};
use crate::error::{HaError, HaResult};
use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

/// Differential backup manager.
pub struct DifferentialBackup {
    /// Backup directory.
    backup_dir: PathBuf,
    /// Compression type.
    compression: BackupCompression,
    /// Source that supplies the real changed-since-full bytes.
    source: RwLock<Option<Arc<dyn BackupSource>>>,
}

impl DifferentialBackup {
    /// Get the backup directory.
    pub fn backup_dir(&self) -> &PathBuf {
        &self.backup_dir
    }
}

impl DifferentialBackup {
    /// Create a new differential backup manager.
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

    /// Create a differential backup (changes since the base full backup).
    pub async fn create(&self, full_backup_id: Uuid) -> HaResult<BackupMetadata> {
        info!("Creating differential backup (full: {})", full_backup_id);

        let data = self
            .source()?
            .read_changes_since(Some(full_backup_id))
            .await?;
        let metadata = super::persist_backup(
            &self.backup_dir,
            BackupType::Differential,
            self.compression,
            Some(full_backup_id),
            &data,
        )
        .await?;

        info!(
            "Differential backup {} persisted ({} bytes) to {}",
            metadata.id,
            metadata.size_bytes,
            self.backup_dir.display()
        );

        Ok(metadata)
    }

    /// Restore a differential backup payload by reading it back and applying it.
    pub async fn restore(&self, backup_id: Uuid) -> HaResult<()> {
        info!("Restoring differential backup {}", backup_id);
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
        std::env::temp_dir().join(format!("oxigeo-ha-diff-{}-{}", tag, Uuid::new_v4()))
    }

    #[tokio::test]
    async fn test_differential_without_source_errors() {
        let backup = DifferentialBackup::new(temp_dir("no-source"), BackupCompression::Gzip);
        assert!(backup.create(Uuid::new_v4()).await.is_err());
    }

    #[tokio::test]
    async fn test_differential_gzip_roundtrip() {
        let dir = temp_dir("gzip");
        let backup = DifferentialBackup::new(dir.clone(), BackupCompression::Gzip);
        let source = Arc::new(MemSource::default());
        let payload: Vec<u8> = b"differential change set - repeated ".repeat(64);
        *source.state.write() = payload.clone();
        backup.set_source(Arc::clone(&source) as Arc<dyn BackupSource>);

        let full = Uuid::new_v4();
        let metadata = backup.create(full).await.unwrap();
        assert_eq!(metadata.backup_type, BackupType::Differential);
        assert_eq!(metadata.parent_id, Some(full));
        assert_eq!(*source.last_since.read(), Some(full));
        assert!(metadata.compressed_size_bytes.is_some());

        let data_path = dir.join(format!("{}.backup", metadata.id));
        assert!(tokio::fs::metadata(&data_path).await.is_ok());

        *source.state.write() = Vec::new();
        backup.restore(metadata.id).await.unwrap();
        assert_eq!(*source.state.read(), payload);
    }
}
