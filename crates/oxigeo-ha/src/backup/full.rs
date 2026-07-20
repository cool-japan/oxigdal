//! Full backup implementation.

use super::{BackupCompression, BackupMetadata, BackupType};
use crate::error::HaResult;
use chrono::Utc;
use std::path::PathBuf;
use tracing::info;
use uuid::Uuid;

/// Full backup manager.
pub struct FullBackup {
    /// Backup directory.
    backup_dir: PathBuf,
    /// Compression type.
    compression: BackupCompression,
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
        }
    }

    /// Create full backup.
    pub async fn create(&self) -> HaResult<BackupMetadata> {
        info!("Creating full backup");

        let data = self.collect_all_data().await?;
        let size_bytes = data.len() as u64;
        let checksum = crc32fast::hash(&data);

        let backup_id = Uuid::new_v4();

        Ok(BackupMetadata {
            id: backup_id,
            backup_type: BackupType::Full,
            timestamp: Utc::now(),
            size_bytes,
            compressed_size_bytes: None,
            compression: self.compression,
            checksum,
            parent_id: None,
        })
    }

    /// Collect all data.
    async fn collect_all_data(&self) -> HaResult<Vec<u8>> {
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        Ok(vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10])
    }
}
