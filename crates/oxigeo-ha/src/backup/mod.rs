//! Backup system with incremental, full, and differential backups.

pub mod differential;
pub mod full;
pub mod incremental;

use crate::error::HaResult;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Backup type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackupType {
    /// Full backup.
    Full,
    /// Incremental backup (changes since last backup).
    Incremental,
    /// Differential backup (changes since last full backup).
    Differential,
}

/// Backup compression type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackupCompression {
    /// No compression.
    None,
    /// LZ4 compression.
    Lz4,
    /// Zstandard compression.
    Zstd,
    /// Gzip compression.
    Gzip,
}

/// Backup metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupMetadata {
    /// Backup ID.
    pub id: Uuid,
    /// Backup type.
    pub backup_type: BackupType,
    /// Timestamp.
    pub timestamp: DateTime<Utc>,
    /// Size in bytes.
    pub size_bytes: u64,
    /// Compressed size in bytes.
    pub compressed_size_bytes: Option<u64>,
    /// Compression type.
    pub compression: BackupCompression,
    /// Checksum.
    pub checksum: u32,
    /// Parent backup ID (for incremental/differential).
    pub parent_id: Option<Uuid>,
}

/// Backup result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupResult {
    /// Backup metadata.
    pub metadata: BackupMetadata,
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// Success flag.
    pub success: bool,
}

/// Trait for backup manager.
#[async_trait]
pub trait BackupManager: Send + Sync {
    /// Create a backup.
    async fn create_backup(&self, backup_type: BackupType) -> HaResult<BackupResult>;

    /// Restore from backup.
    async fn restore_backup(&self, backup_id: Uuid) -> HaResult<()>;

    /// List backups.
    async fn list_backups(&self) -> HaResult<Vec<BackupMetadata>>;

    /// Verify backup integrity.
    async fn verify_backup(&self, backup_id: Uuid) -> HaResult<bool>;

    /// Delete backup.
    async fn delete_backup(&self, backup_id: Uuid) -> HaResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backup_metadata() {
        let metadata = BackupMetadata {
            id: Uuid::new_v4(),
            backup_type: BackupType::Full,
            timestamp: Utc::now(),
            size_bytes: 1024,
            compressed_size_bytes: Some(512),
            compression: BackupCompression::Zstd,
            checksum: 12345,
            parent_id: None,
        };

        assert_eq!(metadata.backup_type, BackupType::Full);
        assert_eq!(metadata.compression, BackupCompression::Zstd);
    }
}
