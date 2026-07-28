//! Point-in-time recovery implementation.
//!
//! Point-in-time recovery (PITR) reconstructs the store's state as of a target
//! timestamp by replaying the committed write-ahead log up to (and including)
//! that point in time. Unlike a fabricated "success" report, this manager reads
//! real WAL segments from disk via [`WalManager`], verifies each entry's
//! checksum, filters by timestamp, and hands every surviving entry to an
//! injected [`WalApplier`] that mutates the live store. The number returned is
//! the real count of entries applied — zero when the WAL is empty, and an error
//! when no applier is configured.

use super::wal::{WalApplier, WalManager};
use super::{RecoveryConfig, RecoveryResult, RecoveryTarget};
use crate::error::{HaError, HaResult};
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn};

/// Point-in-time recovery manager.
pub struct PitrManager {
    /// Configuration.
    config: Arc<RwLock<RecoveryConfig>>,
    /// Data directory (holds the write-ahead log segments).
    data_dir: PathBuf,
    /// Write-ahead log manager used to read/replay committed entries.
    wal: WalManager,
    /// Applier used to replay each WAL entry into the live store.
    applier: RwLock<Option<Arc<dyn WalApplier>>>,
    /// Current recovery position.
    recovery_position: Arc<RwLock<Option<DateTime<Utc>>>>,
}

impl PitrManager {
    /// Get the configuration.
    pub fn config(&self) -> &Arc<RwLock<RecoveryConfig>> {
        &self.config
    }

    /// Get the data directory.
    pub fn data_dir(&self) -> &PathBuf {
        &self.data_dir
    }

    /// Access the underlying WAL manager (e.g. to append entries).
    pub fn wal(&self) -> &WalManager {
        &self.wal
    }
}

impl PitrManager {
    /// Create a new PITR manager.
    ///
    /// The `data_dir` doubles as the WAL directory; the manager reads the
    /// `*.wal` segments stored there during recovery.
    pub fn new(config: RecoveryConfig, data_dir: PathBuf) -> Self {
        let wal = WalManager::new(config.clone(), data_dir.clone());
        Self {
            config: Arc::new(RwLock::new(config)),
            data_dir,
            wal,
            applier: RwLock::new(None),
            recovery_position: Arc::new(RwLock::new(None)),
        }
    }

    /// Inject the applier that replays WAL entries into the live store.
    ///
    /// This MUST be set before [`recover`](Self::recover) is called; recovery
    /// without an applier returns a typed error instead of a fabricated result.
    pub fn set_applier(&self, applier: Arc<dyn WalApplier>) {
        *self.applier.write() = Some(applier);
    }

    fn applier(&self) -> Option<Arc<dyn WalApplier>> {
        self.applier.read().clone()
    }

    /// Perform point-in-time recovery.
    pub async fn recover(&self, target: RecoveryTarget) -> HaResult<RecoveryResult> {
        let start_time = Utc::now();

        info!("Starting PITR to target: {:?}", target);

        self.validate_target(&target)?;

        let target_time = match target {
            RecoveryTarget::Latest => Utc::now(),
            RecoveryTarget::Timestamp(ts) => ts,
            RecoveryTarget::TransactionId(_) => {
                return Err(HaError::NotImplemented(
                    "Transaction ID recovery not yet implemented".to_string(),
                ));
            }
            RecoveryTarget::Snapshot(_) => {
                return Err(HaError::NotImplemented(
                    "Snapshot recovery should use SnapshotManager".to_string(),
                ));
            }
        };

        let transactions_replayed = self.replay_wal_to_time(target_time).await?;

        *self.recovery_position.write() = Some(target_time);

        let duration_ms = (Utc::now() - start_time).num_milliseconds().max(0) as u64;

        info!(
            "PITR complete: replayed {} transactions in {}ms",
            transactions_replayed, duration_ms
        );

        Ok(RecoveryResult {
            target: target.clone(),
            recovered_to: target_time,
            transactions_replayed,
            duration_ms,
            success: true,
        })
    }

    /// Replay WAL entries with a timestamp at or before `target_time`.
    ///
    /// Reads the real WAL segments (each entry's checksum is verified while
    /// reading), applies every entry whose timestamp is `<= target_time` in
    /// transaction order via the injected [`WalApplier`], and returns the count
    /// actually applied.
    async fn replay_wal_to_time(&self, target_time: DateTime<Utc>) -> HaResult<u64> {
        info!("Replaying WAL to time: {}", target_time);

        let applier = self.applier().ok_or_else(|| {
            HaError::PitrFailed(
                "no WAL applier configured; refusing to report a fabricated replay count"
                    .to_string(),
            )
        })?;

        // Ensure the WAL directory exists so a recovery over a never-written log
        // yields an honest zero rather than an I/O error.
        tokio::fs::create_dir_all(&self.data_dir)
            .await
            .map_err(|e| HaError::PitrFailed(format!("failed to access WAL directory: {}", e)))?;

        // read_entries() verifies each entry's checksum and returns them ordered
        // by transaction id (i.e. commit order).
        let entries = self.wal.read_entries().await?;

        let mut replayed = 0u64;
        for entry in &entries {
            if entry.timestamp <= target_time {
                // Defence in depth: re-verify integrity before applying.
                entry.verify_checksum()?;
                applier.apply(entry).await?;
                replayed += 1;
            }
        }

        if replayed == 0 {
            warn!(
                "PITR found no WAL entries at or before {} (log held {} entries)",
                target_time,
                entries.len()
            );
        }

        Ok(replayed)
    }

    /// Get current recovery position.
    pub fn get_recovery_position(&self) -> Option<DateTime<Utc>> {
        *self.recovery_position.read()
    }

    /// Validate recovery target.
    pub fn validate_target(&self, target: &RecoveryTarget) -> HaResult<()> {
        if let RecoveryTarget::Timestamp(ts) = target
            && *ts > Utc::now()
        {
            return Err(HaError::PitrFailed(
                "Cannot recover to future timestamp".to_string(),
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::recovery::wal::WalEntry;
    use async_trait::async_trait;

    /// Records the payloads of every WAL entry actually replayed, so tests can
    /// assert on *real* replay rather than a fabricated count.
    #[derive(Default)]
    struct RecordingApplier {
        applied: RwLock<Vec<Vec<u8>>>,
    }

    #[async_trait]
    impl WalApplier for RecordingApplier {
        async fn apply(&self, entry: &WalEntry) -> HaResult<()> {
            self.applied.write().push(entry.data.clone());
            Ok(())
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("oxigeo-ha-pitr-{}-{}", tag, uuid::Uuid::new_v4()))
    }

    #[tokio::test]
    async fn test_recover_without_applier_errors() {
        let dir = temp_dir("no-applier");
        let manager = PitrManager::new(RecoveryConfig::default(), dir);
        // No applier set → must fail, not fabricate a count.
        assert!(manager.recover(RecoveryTarget::Latest).await.is_err());
    }

    #[tokio::test]
    async fn test_recover_replays_real_wal_entries() {
        let dir = temp_dir("replay");
        let manager = PitrManager::new(RecoveryConfig::default(), dir);

        manager.wal().initialize().await.unwrap();
        manager.wal().write_entry(vec![1, 1, 1]).await.unwrap();
        manager.wal().write_entry(vec![2, 2, 2]).await.unwrap();
        manager.wal().write_entry(vec![3, 3, 3]).await.unwrap();

        let applier = Arc::new(RecordingApplier::default());
        manager.set_applier(Arc::clone(&applier) as Arc<dyn WalApplier>);

        let result = manager.recover(RecoveryTarget::Latest).await.unwrap();
        assert!(result.success);
        assert_eq!(result.transactions_replayed, 3);

        // The applier actually received every payload, in commit order.
        let applied = applier.applied.read().clone();
        assert_eq!(applied, vec![vec![1, 1, 1], vec![2, 2, 2], vec![3, 3, 3]]);
    }

    #[tokio::test]
    async fn test_recover_to_timestamp_filters_entries() {
        let dir = temp_dir("filter");
        let manager = PitrManager::new(RecoveryConfig::default(), dir);

        manager.wal().initialize().await.unwrap();
        manager.wal().write_entry(vec![10]).await.unwrap();
        manager.wal().write_entry(vec![20]).await.unwrap();

        // Cutoff strictly before any entry → nothing should be replayed.
        let cutoff = Utc::now() - chrono::Duration::hours(1);

        let applier = Arc::new(RecordingApplier::default());
        manager.set_applier(Arc::clone(&applier) as Arc<dyn WalApplier>);

        let result = manager
            .recover(RecoveryTarget::Timestamp(cutoff))
            .await
            .unwrap();
        assert_eq!(result.transactions_replayed, 0);
        assert!(applier.applied.read().is_empty());
    }

    #[tokio::test]
    async fn test_recover_empty_wal_reports_zero() {
        let dir = temp_dir("empty");
        let manager = PitrManager::new(RecoveryConfig::default(), dir);

        let applier = Arc::new(RecordingApplier::default());
        manager.set_applier(Arc::clone(&applier) as Arc<dyn WalApplier>);

        let result = manager.recover(RecoveryTarget::Latest).await.unwrap();
        assert_eq!(result.transactions_replayed, 0);
    }

    #[tokio::test]
    async fn test_future_timestamp_rejected() {
        let dir = temp_dir("future");
        let manager = PitrManager::new(RecoveryConfig::default(), dir);
        let applier = Arc::new(RecordingApplier::default());
        manager.set_applier(Arc::clone(&applier) as Arc<dyn WalApplier>);

        let future = Utc::now() + chrono::Duration::hours(1);
        assert!(
            manager
                .recover(RecoveryTarget::Timestamp(future))
                .await
                .is_err()
        );
    }
}
