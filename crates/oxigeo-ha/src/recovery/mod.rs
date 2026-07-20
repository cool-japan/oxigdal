//! Point-in-time recovery and snapshot management.

pub mod pitr;
pub mod snapshot;
pub mod wal;

use crate::error::HaResult;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Recovery configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryConfig {
    /// Snapshot interval in seconds.
    pub snapshot_interval_secs: u64,
    /// WAL segment size in bytes.
    pub wal_segment_size: usize,
    /// WAL retention in seconds.
    pub wal_retention_secs: u64,
    /// Enable compression for snapshots.
    pub enable_snapshot_compression: bool,
    /// Enable compression for WAL.
    pub enable_wal_compression: bool,
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        Self {
            snapshot_interval_secs: 3600,
            wal_segment_size: 16 * 1024 * 1024,
            wal_retention_secs: 86400 * 7,
            enable_snapshot_compression: true,
            enable_wal_compression: true,
        }
    }
}

/// Recovery target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecoveryTarget {
    /// Recover to latest state.
    Latest,
    /// Recover to specific timestamp.
    Timestamp(DateTime<Utc>),
    /// Recover to specific transaction ID.
    TransactionId(u64),
    /// Recover to specific snapshot.
    Snapshot(Uuid),
}

/// Recovery result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryResult {
    /// Recovery target.
    pub target: RecoveryTarget,
    /// Recovered to timestamp.
    pub recovered_to: DateTime<Utc>,
    /// Number of transactions replayed.
    pub transactions_replayed: u64,
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// Success flag.
    pub success: bool,
}

/// Recovery statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecoveryStats {
    /// Total recoveries performed.
    pub total_recoveries: u64,
    /// Successful recoveries.
    pub successful_recoveries: u64,
    /// Failed recoveries.
    pub failed_recoveries: u64,
    /// Average recovery time in milliseconds.
    pub average_recovery_time_ms: u64,
    /// Last recovery time.
    pub last_recovery_at: Option<DateTime<Utc>>,
}

/// Trait for recovery manager.
#[async_trait]
pub trait RecoveryManager: Send + Sync {
    /// Start recovery system.
    async fn start(&self) -> HaResult<()>;

    /// Stop recovery system.
    async fn stop(&self) -> HaResult<()>;

    /// Perform recovery.
    async fn recover(&self, target: RecoveryTarget) -> HaResult<RecoveryResult>;

    /// Get recovery statistics.
    async fn get_stats(&self) -> HaResult<RecoveryStats>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recovery_config() {
        let config = RecoveryConfig::default();
        assert_eq!(config.snapshot_interval_secs, 3600);
        assert!(config.enable_snapshot_compression);
    }

    #[test]
    fn test_recovery_target() {
        let target = RecoveryTarget::Latest;
        matches!(target, RecoveryTarget::Latest);
    }
}
