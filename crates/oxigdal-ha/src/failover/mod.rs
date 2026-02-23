//! Automatic failover orchestration.
//!
//! This module provides:
//! - Failure detection (heartbeat, health checks)
//! - Leader election (Raft-based)
//! - Replica promotion
//! - Client traffic redirection
//! - Graceful degradation
//! - Automatic recovery
//! - Failback support

pub mod client_redirect;
pub mod detection;
pub mod election;
pub mod promotion;

use crate::error::HaResult;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Failover configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailoverConfig {
    /// Heartbeat interval in milliseconds.
    pub heartbeat_interval_ms: u64,
    /// Heartbeat timeout in milliseconds.
    pub heartbeat_timeout_ms: u64,
    /// Health check interval in milliseconds.
    pub health_check_interval_ms: u64,
    /// Health check timeout in milliseconds.
    pub health_check_timeout_ms: u64,
    /// Maximum failover time in milliseconds.
    pub max_failover_time_ms: u64,
    /// Enable automatic failback.
    pub enable_failback: bool,
    /// Failback delay in milliseconds.
    pub failback_delay_ms: u64,
    /// Election timeout in milliseconds.
    pub election_timeout_ms: u64,
}

impl Default for FailoverConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval_ms: 1000,
            heartbeat_timeout_ms: 5000,
            health_check_interval_ms: 2000,
            health_check_timeout_ms: 5000,
            max_failover_time_ms: 1000,
            enable_failback: true,
            failback_delay_ms: 60000,
            election_timeout_ms: 3000,
        }
    }
}

/// Node role in the cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeRole {
    /// Leader node.
    Leader,
    /// Follower node.
    Follower,
    /// Candidate node (during election).
    Candidate,
    /// Observer node (read-only).
    Observer,
}

/// Failover state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailoverState {
    /// Normal operation.
    Normal,
    /// Detecting failure.
    Detecting,
    /// Election in progress.
    Electing,
    /// Promoting new leader.
    Promoting,
    /// Redirecting clients.
    Redirecting,
    /// Failover complete.
    Complete,
    /// Failback in progress.
    Failback,
}

/// Failover event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailoverEvent {
    /// Event ID.
    pub id: Uuid,
    /// Event timestamp.
    pub timestamp: DateTime<Utc>,
    /// Old leader node ID.
    pub old_leader_id: Option<Uuid>,
    /// New leader node ID.
    pub new_leader_id: Uuid,
    /// Failover duration in milliseconds.
    pub duration_ms: u64,
    /// Reason for failover.
    pub reason: String,
}

/// Failover statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FailoverStats {
    /// Total number of failovers.
    pub total_failovers: u64,
    /// Successful failovers.
    pub successful_failovers: u64,
    /// Failed failovers.
    pub failed_failovers: u64,
    /// Average failover time in milliseconds.
    pub average_failover_time_ms: u64,
    /// Minimum failover time in milliseconds.
    pub min_failover_time_ms: u64,
    /// Maximum failover time in milliseconds.
    pub max_failover_time_ms: u64,
    /// Last failover time.
    pub last_failover_at: Option<DateTime<Utc>>,
}

/// Trait for failover orchestrator.
#[async_trait]
pub trait FailoverOrchestrator: Send + Sync {
    /// Start failover monitoring.
    async fn start(&self) -> HaResult<()>;

    /// Stop failover monitoring.
    async fn stop(&self) -> HaResult<()>;

    /// Trigger manual failover.
    async fn trigger_failover(&self, reason: String) -> HaResult<FailoverEvent>;

    /// Get current node role.
    async fn get_role(&self) -> HaResult<NodeRole>;

    /// Get current leader.
    async fn get_leader(&self) -> HaResult<Option<Uuid>>;

    /// Get failover statistics.
    async fn get_stats(&self) -> HaResult<FailoverStats>;

    /// Check if failover is in progress.
    async fn is_failover_in_progress(&self) -> HaResult<bool>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_failover_config() {
        let config = FailoverConfig::default();
        assert_eq!(config.heartbeat_interval_ms, 1000);
        assert_eq!(config.max_failover_time_ms, 1000);
        assert!(config.enable_failback);
    }

    #[test]
    fn test_node_role() {
        assert_eq!(NodeRole::Leader, NodeRole::Leader);
        assert_ne!(NodeRole::Leader, NodeRole::Follower);
    }
}
