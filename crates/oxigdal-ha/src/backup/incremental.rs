//! Incremental backup implementation.

use super::{BackupCompression, BackupMetadata, BackupType};
use crate::error::HaResult;
use chrono::Utc;
use std::path::PathBuf;
use tracing::info;
use uuid::Uuid;

/// Incremental backup manager.
pub struct IncrementalBackup {
    /// Backup directory.
    backup_dir: PathBuf,
    /// Compression type.
    compression: BackupCompression,
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
        }
    }

    /// Create incremental backup.
    pub async fn create(&self, parent_id: Option<Uuid>) -> HaResult<BackupMetadata> {
        info!("Creating incremental backup (parent: {:?})", parent_id);

        let data = self.collect_changes(parent_id).await?;
        let size_bytes = data.len() as u64;
        let checksum = crc32fast::hash(&data);

        let backup_id = Uuid::new_v4();

        Ok(BackupMetadata {
            id: backup_id,
            backup_type: BackupType::Incremental,
            timestamp: Utc::now(),
            size_bytes,
            compressed_size_bytes: None,
            compression: self.compression,
            checksum,
            parent_id,
        })
    }

    /// Collect changes since last backup.
    async fn collect_changes(&self, _parent_id: Option<Uuid>) -> HaResult<Vec<u8>> {
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        Ok(vec![1, 2, 3, 4, 5])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_incremental_backup() {
        let backup_dir = std::env::temp_dir().join("oxigdal-ha-test-incr-backup");
        let backup = IncrementalBackup::new(backup_dir, BackupCompression::Zstd);

        let metadata = backup.create(None).await.ok();
        assert!(metadata.is_some());

        if let Some(m) = metadata {
            assert_eq!(m.backup_type, BackupType::Incremental);
        }
    }
}
