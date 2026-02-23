//! Write-ahead logging for point-in-time recovery.

use super::RecoveryConfig;
use crate::error::{HaError, HaResult};
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tracing::{debug, info};
use uuid::Uuid;

/// WAL entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalEntry {
    /// Entry ID.
    pub id: Uuid,
    /// Transaction ID.
    pub transaction_id: u64,
    /// Timestamp.
    pub timestamp: DateTime<Utc>,
    /// Entry data.
    pub data: Vec<u8>,
    /// Checksum.
    pub checksum: u32,
}

impl WalEntry {
    /// Create a new WAL entry.
    pub fn new(transaction_id: u64, data: Vec<u8>) -> Self {
        let checksum = crc32fast::hash(&data);
        Self {
            id: Uuid::new_v4(),
            transaction_id,
            timestamp: Utc::now(),
            data,
            checksum,
        }
    }

    /// Verify checksum.
    pub fn verify_checksum(&self) -> HaResult<()> {
        let actual = crc32fast::hash(&self.data);
        if actual == self.checksum {
            Ok(())
        } else {
            Err(HaError::ChecksumMismatch {
                expected: self.checksum,
                actual,
            })
        }
    }
}

/// WAL segment.
#[derive(Debug)]
struct WalSegment {
    /// Segment ID.
    #[allow(dead_code)]
    id: Uuid,
    /// File path.
    path: PathBuf,
    /// Current size.
    size: usize,
    /// Entry count.
    entry_count: usize,
}

/// Write-ahead log manager.
pub struct WalManager {
    /// Configuration.
    config: Arc<RwLock<RecoveryConfig>>,
    /// WAL directory.
    wal_dir: PathBuf,
    /// Current segment.
    current_segment: Arc<RwLock<Option<WalSegment>>>,
    /// Next transaction ID.
    next_transaction_id: Arc<RwLock<u64>>,
}

impl WalManager {
    /// Create a new WAL manager.
    pub fn new(config: RecoveryConfig, wal_dir: PathBuf) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            wal_dir,
            current_segment: Arc::new(RwLock::new(None)),
            next_transaction_id: Arc::new(RwLock::new(1)),
        }
    }

    /// Initialize WAL.
    pub async fn initialize(&self) -> HaResult<()> {
        info!("Initializing WAL");

        tokio::fs::create_dir_all(&self.wal_dir)
            .await
            .map_err(|e| HaError::Wal(format!("Failed to create WAL directory: {}", e)))?;

        self.create_new_segment().await?;

        Ok(())
    }

    /// Write an entry to WAL.
    pub async fn write_entry(&self, data: Vec<u8>) -> HaResult<WalEntry> {
        let transaction_id = {
            let mut next_id = self.next_transaction_id.write();
            let id = *next_id;
            *next_id += 1;
            id
        };

        let entry = WalEntry::new(transaction_id, data);

        debug!("Writing WAL entry {} (txn {})", entry.id, transaction_id);

        let serialized = oxicode::serde::encode_to_vec(&entry, oxicode::config::standard())?;

        self.write_to_segment(&serialized).await?;

        Ok(entry)
    }

    /// Write data to current segment.
    async fn write_to_segment(&self, data: &[u8]) -> HaResult<()> {
        let should_rotate = {
            let segment_guard = self.current_segment.read();
            match segment_guard.as_ref() {
                Some(segment) => {
                    let config = self.config.read();
                    segment.size + data.len() > config.wal_segment_size
                }
                None => true,
            }
        };

        if should_rotate {
            self.create_new_segment().await?;
        }

        let segment_path = {
            let mut segment_guard = self.current_segment.write();
            let segment = segment_guard
                .as_mut()
                .ok_or_else(|| HaError::Wal("No current segment".to_string()))?;

            segment.size += data.len();
            segment.entry_count += 1;
            segment.path.clone()
        };

        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&segment_path)
            .await
            .map_err(|e| HaError::Wal(format!("Failed to open segment: {}", e)))?;

        file.write_all(data)
            .await
            .map_err(|e| HaError::Wal(format!("Failed to write to segment: {}", e)))?;

        file.flush()
            .await
            .map_err(|e| HaError::Wal(format!("Failed to flush segment: {}", e)))?;

        Ok(())
    }

    /// Create a new WAL segment.
    async fn create_new_segment(&self) -> HaResult<()> {
        let segment_id = Uuid::new_v4();
        let segment_path = self.wal_dir.join(format!("{}.wal", segment_id));

        info!("Creating new WAL segment: {}", segment_id);

        let segment = WalSegment {
            id: segment_id,
            path: segment_path,
            size: 0,
            entry_count: 0,
        };

        *self.current_segment.write() = Some(segment);

        Ok(())
    }

    /// Read WAL entries.
    pub async fn read_entries(&self) -> HaResult<Vec<WalEntry>> {
        let mut entries = Vec::new();

        let mut dir_entries = tokio::fs::read_dir(&self.wal_dir)
            .await
            .map_err(|e| HaError::Wal(format!("Failed to read WAL directory: {}", e)))?;

        while let Some(entry) = dir_entries
            .next_entry()
            .await
            .map_err(|e| HaError::Wal(format!("Failed to read directory entry: {}", e)))?
        {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("wal") {
                let segment_entries = self.read_segment(&path).await?;
                entries.extend(segment_entries);
            }
        }

        entries.sort_by_key(|e| e.transaction_id);

        Ok(entries)
    }

    /// Read entries from a segment.
    async fn read_segment(&self, path: &PathBuf) -> HaResult<Vec<WalEntry>> {
        let data = tokio::fs::read(path)
            .await
            .map_err(|e| HaError::Wal(format!("Failed to read segment: {}", e)))?;

        let mut entries = Vec::new();
        let mut offset = 0;

        while offset < data.len() {
            match oxicode::serde::decode_owned_from_slice::<WalEntry, _>(
                &data[offset..],
                oxicode::config::standard(),
            ) {
                Ok((entry, bytes_read)) => {
                    entry.verify_checksum()?;
                    entries.push(entry);
                    offset += bytes_read;
                }
                Err(_) => break,
            }
        }

        Ok(entries)
    }

    /// Cleanup old WAL segments.
    pub async fn cleanup_old_segments(&self) -> HaResult<usize> {
        let retention_secs = self.config.read().wal_retention_secs;
        let cutoff = Utc::now() - chrono::Duration::seconds(retention_secs as i64);

        let mut deleted_count = 0;

        let mut dir_entries = tokio::fs::read_dir(&self.wal_dir)
            .await
            .map_err(|e| HaError::Wal(format!("Failed to read WAL directory: {}", e)))?;

        while let Some(entry) = dir_entries
            .next_entry()
            .await
            .map_err(|e| HaError::Wal(format!("Failed to read directory entry: {}", e)))?
        {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("wal") {
                if let Ok(metadata) = tokio::fs::metadata(&path).await {
                    if let Ok(modified) = metadata.modified() {
                        let modified_time: DateTime<Utc> = modified.into();
                        if modified_time < cutoff {
                            tokio::fs::remove_file(&path).await.ok();
                            deleted_count += 1;
                        }
                    }
                }
            }
        }

        if deleted_count > 0 {
            info!("Cleaned up {} old WAL segments", deleted_count);
        }

        Ok(deleted_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_wal_operations() {
        let config = RecoveryConfig::default();
        let wal_dir = std::env::temp_dir().join("oxigdal-ha-test-wal");
        tokio::fs::create_dir_all(&wal_dir).await.ok();

        let manager = WalManager::new(config, wal_dir);
        assert!(manager.initialize().await.is_ok());

        let entry = manager.write_entry(vec![1, 2, 3, 4, 5]).await.ok();
        assert!(entry.is_some());

        if let Some(e) = entry {
            assert_eq!(e.transaction_id, 1);
            assert!(e.verify_checksum().is_ok());
        }
    }
}
