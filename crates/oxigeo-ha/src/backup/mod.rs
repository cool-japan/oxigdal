//! Backup system with incremental, full, and differential backups.

pub mod differential;
pub mod full;
pub mod incremental;

use crate::error::{HaError, HaResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Supplies the real bytes to back up (and applies them on restore).
///
/// Backup managers own persistence — checksum, compression and durable file
/// I/O — but the actual dataset content is injected through this trait so the
/// HA crate stays storage-agnostic. Without a source, a backup fails loudly
/// with a typed error instead of persisting canned placeholder bytes.
#[async_trait]
pub trait BackupSource: Send + Sync {
    /// Read a full dump of all current dataset state.
    async fn read_full(&self) -> HaResult<Vec<u8>>;

    /// Read the changes accumulated since a prior backup.
    ///
    /// `since` is the id of the reference backup (the parent for an incremental
    /// backup, or the base full backup for a differential backup). `None` means
    /// "since the beginning of time".
    async fn read_changes_since(&self, since: Option<Uuid>) -> HaResult<Vec<u8>>;

    /// Apply a restored backup payload back into the dataset.
    async fn apply(&self, data: &[u8]) -> HaResult<()>;
}

/// Compress `data` according to `compression` using the Pure-Rust oxiarc stack.
pub(crate) fn compress(compression: BackupCompression, data: &[u8]) -> HaResult<Vec<u8>> {
    match compression {
        BackupCompression::None => Ok(data.to_vec()),
        BackupCompression::Lz4 => {
            oxiarc_lz4::compress_bytes(data).map_err(|e| HaError::Compression(e.to_string()))
        }
        BackupCompression::Zstd => {
            oxiarc_zstd::encode_all(data, 3).map_err(|e| HaError::Compression(e.to_string()))
        }
        BackupCompression::Gzip => {
            oxiarc_deflate::gzip_compress(data, 6).map_err(|e| HaError::Compression(e.to_string()))
        }
    }
}

/// Decompress `data` produced by [`compress`], given the original byte length.
pub(crate) fn decompress(
    compression: BackupCompression,
    data: &[u8],
    original_len: usize,
) -> HaResult<Vec<u8>> {
    match compression {
        BackupCompression::None => Ok(data.to_vec()),
        BackupCompression::Lz4 => oxiarc_lz4::decompress_bytes(data, original_len)
            .map_err(|e| HaError::Decompression(e.to_string())),
        BackupCompression::Zstd => {
            oxiarc_zstd::decode_all(data).map_err(|e| HaError::Decompression(e.to_string()))
        }
        BackupCompression::Gzip => {
            oxiarc_deflate::gzip_decompress(data).map_err(|e| HaError::Decompression(e.to_string()))
        }
    }
}

/// Persist a collected backup payload to `backup_dir` and return its metadata.
///
/// Writes two real files: `<id>.backup` (the checksummed, optionally compressed
/// payload) and `<id>.meta` (JSON metadata). The checksum is computed over the
/// *uncompressed* payload so restore can verify integrity after decompression.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn persist_backup(
    backup_dir: &Path,
    backup_type: BackupType,
    compression: BackupCompression,
    parent_id: Option<Uuid>,
    data: &[u8],
) -> HaResult<BackupMetadata> {
    tokio::fs::create_dir_all(backup_dir)
        .await
        .map_err(|e| HaError::Backup(format!("failed to create backup directory: {}", e)))?;

    let backup_id = Uuid::new_v4();
    let size_bytes = data.len() as u64;
    let checksum = crc32fast::hash(data);

    let stored = compress(compression, data)?;
    let compressed_size_bytes = if compression == BackupCompression::None {
        None
    } else {
        Some(stored.len() as u64)
    };

    let data_path = backup_dir.join(format!("{}.backup", backup_id));
    tokio::fs::write(&data_path, &stored)
        .await
        .map_err(|e| HaError::Backup(format!("failed to write backup payload: {}", e)))?;

    let metadata = BackupMetadata {
        id: backup_id,
        backup_type,
        timestamp: Utc::now(),
        size_bytes,
        compressed_size_bytes,
        compression,
        checksum,
        parent_id,
    };

    let meta_path = backup_dir.join(format!("{}.meta", backup_id));
    let json = serde_json::to_vec_pretty(&metadata)?;
    tokio::fs::write(&meta_path, json)
        .await
        .map_err(|e| HaError::Backup(format!("failed to write backup metadata: {}", e)))?;

    Ok(metadata)
}

/// Read a persisted backup payload back from disk, verifying its checksum.
///
/// Returns the decompressed, integrity-verified bytes ready to hand to
/// [`BackupSource::apply`].
pub(crate) async fn read_backup_payload(
    backup_dir: &Path,
    metadata: &BackupMetadata,
) -> HaResult<Vec<u8>> {
    let data_path = backup_dir.join(format!("{}.backup", metadata.id));
    let stored = tokio::fs::read(&data_path)
        .await
        .map_err(|e| HaError::Backup(format!("failed to read backup {}: {}", metadata.id, e)))?;

    let data = decompress(metadata.compression, &stored, metadata.size_bytes as usize)?;

    let checksum = crc32fast::hash(&data);
    if checksum != metadata.checksum {
        return Err(HaError::ChecksumMismatch {
            expected: metadata.checksum,
            actual: checksum,
        });
    }

    Ok(data)
}

/// Load backup metadata for `backup_id` from `backup_dir`.
pub(crate) async fn load_metadata(backup_dir: &Path, backup_id: Uuid) -> HaResult<BackupMetadata> {
    let meta_path: PathBuf = backup_dir.join(format!("{}.meta", backup_id));
    let json = tokio::fs::read(&meta_path)
        .await
        .map_err(|_| HaError::BackupNotFound {
            id: backup_id.to_string(),
        })?;
    let metadata = serde_json::from_slice(&json)?;
    Ok(metadata)
}

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
