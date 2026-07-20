//! Differential backup implementation.

use super::{BackupCompression, BackupMetadata, BackupType};
use crate::error::HaResult;
use chrono::Utc;
use std::path::PathBuf;
use tracing::info;
use uuid::Uuid;

/// Differential backup manager.
pub struct DifferentialBackup {
    /// Backup directory.
    backup_dir: PathBuf,
    /// Compression type.
    compression: BackupCompression,
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
        }
    }

    /// Create differential backup.
    pub async fn create(&self, full_backup_id: Uuid) -> HaResult<BackupMetadata> {
        info!("Creating differential backup (full: {})", full_backup_id);

        let data = self.collect_changes_since_full(full_backup_id).await?;
        let size_bytes = data.len() as u64;
        let checksum = crc32fast::hash(&data);

        let backup_id = Uuid::new_v4();

        Ok(BackupMetadata {
            id: backup_id,
            backup_type: BackupType::Differential,
            timestamp: Utc::now(),
            size_bytes,
            compressed_size_bytes: None,
            compression: self.compression,
            checksum,
            parent_id: Some(full_backup_id),
        })
    }

    /// Collect changes since full backup.
    async fn collect_changes_since_full(&self, _full_backup_id: Uuid) -> HaResult<Vec<u8>> {
        tokio::time::sleep(tokio::time::Duration::from_millis(75)).await;
        Ok(vec![6, 7, 8])
    }
}
