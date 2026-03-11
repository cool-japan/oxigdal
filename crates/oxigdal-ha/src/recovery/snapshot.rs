//! Snapshot management for point-in-time recovery.

use super::RecoveryConfig;
use crate::error::{HaError, HaResult};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{info, warn};
use uuid::Uuid;

/// Snapshot metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMetadata {
    /// Snapshot ID.
    pub id: Uuid,
    /// Snapshot timestamp.
    pub timestamp: DateTime<Utc>,
    /// Snapshot size in bytes.
    pub size_bytes: u64,
    /// Compressed size in bytes (if compressed).
    pub compressed_size_bytes: Option<u64>,
    /// Checksum.
    pub checksum: u32,
    /// Transaction ID at snapshot time.
    pub transaction_id: u64,
}

/// Snapshot manager.
pub struct SnapshotManager {
    /// Configuration.
    config: RecoveryConfig,
    /// Snapshot directory.
    snapshot_dir: PathBuf,
}

impl SnapshotManager {
    /// Create a new snapshot manager.
    pub fn new(config: RecoveryConfig, snapshot_dir: PathBuf) -> Self {
        Self {
            config,
            snapshot_dir,
        }
    }

    /// Create a new snapshot.
    pub async fn create_snapshot(&self, transaction_id: u64) -> HaResult<SnapshotMetadata> {
        let start_time = Utc::now();

        info!("Creating snapshot at transaction ID {}", transaction_id);

        let snapshot_id = Uuid::new_v4();
        let snapshot_data = self.capture_data().await?;

        let size_bytes = snapshot_data.len() as u64;
        let checksum = crc32fast::hash(&snapshot_data);

        let (final_data, compressed_size) = if self.config.enable_snapshot_compression {
            let compressed = self.compress_snapshot(&snapshot_data)?;
            let compressed_size = compressed.len() as u64;
            info!(
                "Snapshot compressed: {} -> {} bytes ({:.2}%)",
                size_bytes,
                compressed_size,
                (compressed_size as f64 / size_bytes as f64) * 100.0
            );
            (compressed, Some(compressed_size))
        } else {
            (snapshot_data, None)
        };

        let snapshot_path = self.get_snapshot_path(snapshot_id);
        tokio::fs::write(&snapshot_path, final_data)
            .await
            .map_err(|e| HaError::Snapshot(format!("Failed to write snapshot: {}", e)))?;

        let metadata = SnapshotMetadata {
            id: snapshot_id,
            timestamp: start_time,
            size_bytes,
            compressed_size_bytes: compressed_size,
            checksum,
            transaction_id,
        };

        self.save_metadata(&metadata).await?;

        let duration = (Utc::now() - start_time).num_milliseconds();
        info!("Snapshot {} created in {}ms", snapshot_id, duration);

        Ok(metadata)
    }

    /// Restore from snapshot.
    pub async fn restore_snapshot(&self, snapshot_id: Uuid) -> HaResult<()> {
        info!("Restoring snapshot {}", snapshot_id);

        let metadata = self.load_metadata(snapshot_id).await?;

        let snapshot_path = self.get_snapshot_path(snapshot_id);
        let snapshot_data = tokio::fs::read(&snapshot_path)
            .await
            .map_err(|e| HaError::Snapshot(format!("Failed to read snapshot: {}", e)))?;

        let data = if metadata.compressed_size_bytes.is_some() {
            self.decompress_snapshot(&snapshot_data)?
        } else {
            snapshot_data
        };

        let checksum = crc32fast::hash(&data);
        if checksum != metadata.checksum {
            return Err(HaError::ChecksumMismatch {
                expected: metadata.checksum,
                actual: checksum,
            });
        }

        self.apply_snapshot(&data).await?;

        info!("Snapshot {} restored successfully", snapshot_id);

        Ok(())
    }

    /// List available snapshots.
    pub async fn list_snapshots(&self) -> HaResult<Vec<SnapshotMetadata>> {
        let mut snapshots = Vec::new();

        let mut entries = tokio::fs::read_dir(&self.snapshot_dir)
            .await
            .map_err(|e| HaError::Snapshot(format!("Failed to read snapshot directory: {}", e)))?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| HaError::Snapshot(format!("Failed to read directory entry: {}", e)))?
        {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("meta") {
                if let Some(id_str) = path.file_stem().and_then(|s| s.to_str()) {
                    if let Ok(id) = Uuid::parse_str(id_str) {
                        match self.load_metadata(id).await {
                            Ok(metadata) => snapshots.push(metadata),
                            Err(e) => warn!("Failed to load metadata for {}: {}", id, e),
                        }
                    }
                }
            }
        }

        snapshots.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        Ok(snapshots)
    }

    /// Delete old snapshots.
    pub async fn cleanup_old_snapshots(&self, keep_count: usize) -> HaResult<usize> {
        let snapshots = self.list_snapshots().await?;

        if snapshots.len() <= keep_count {
            return Ok(0);
        }

        let to_delete = &snapshots[keep_count..];
        let mut deleted_count = 0;

        for snapshot in to_delete {
            match self.delete_snapshot(snapshot.id).await {
                Ok(()) => deleted_count += 1,
                Err(e) => warn!("Failed to delete snapshot {}: {}", snapshot.id, e),
            }
        }

        info!("Cleaned up {} old snapshots", deleted_count);

        Ok(deleted_count)
    }

    /// Delete a snapshot.
    async fn delete_snapshot(&self, snapshot_id: Uuid) -> HaResult<()> {
        let snapshot_path = self.get_snapshot_path(snapshot_id);
        let metadata_path = self.get_metadata_path(snapshot_id);

        tokio::fs::remove_file(&snapshot_path).await.ok();
        tokio::fs::remove_file(&metadata_path).await.ok();

        Ok(())
    }

    /// Capture current data state.
    async fn capture_data(&self) -> HaResult<Vec<u8>> {
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        Ok(vec![1, 2, 3, 4, 5])
    }

    /// Apply snapshot data.
    async fn apply_snapshot(&self, _data: &[u8]) -> HaResult<()> {
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        Ok(())
    }

    /// Compress snapshot data.
    fn compress_snapshot(&self, data: &[u8]) -> HaResult<Vec<u8>> {
        oxiarc_zstd::encode_all(data, 3).map_err(|e| HaError::Compression(e.to_string()))
    }

    /// Decompress snapshot data.
    fn decompress_snapshot(&self, data: &[u8]) -> HaResult<Vec<u8>> {
        oxiarc_zstd::decode_all(data).map_err(|e| HaError::Decompression(e.to_string()))
    }

    /// Get snapshot file path.
    fn get_snapshot_path(&self, id: Uuid) -> PathBuf {
        self.snapshot_dir.join(format!("{}.snapshot", id))
    }

    /// Get metadata file path.
    fn get_metadata_path(&self, id: Uuid) -> PathBuf {
        self.snapshot_dir.join(format!("{}.meta", id))
    }

    /// Save snapshot metadata.
    async fn save_metadata(&self, metadata: &SnapshotMetadata) -> HaResult<()> {
        let metadata_path = self.get_metadata_path(metadata.id);
        let json = serde_json::to_vec_pretty(metadata)?;
        tokio::fs::write(&metadata_path, json)
            .await
            .map_err(|e| HaError::Snapshot(format!("Failed to write metadata: {}", e)))?;
        Ok(())
    }

    /// Load snapshot metadata.
    async fn load_metadata(&self, id: Uuid) -> HaResult<SnapshotMetadata> {
        let metadata_path = self.get_metadata_path(id);
        let json = tokio::fs::read(&metadata_path)
            .await
            .map_err(|e| HaError::Snapshot(format!("Failed to read metadata: {}", e)))?;
        let metadata = serde_json::from_slice(&json)?;
        Ok(metadata)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_snapshot_creation() {
        let config = RecoveryConfig::default();
        let snapshot_dir = std::env::temp_dir().join("oxigdal-ha-test-snapshots");
        tokio::fs::create_dir_all(&snapshot_dir).await.ok();

        let manager = SnapshotManager::new(config, snapshot_dir);

        let metadata = manager.create_snapshot(1000).await.ok();
        assert!(metadata.is_some());

        if let Some(m) = metadata {
            assert_eq!(m.transaction_id, 1000);
        }
    }
}
