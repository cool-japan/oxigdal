//! Replication framework for high availability.
//!
//! This module provides comprehensive replication capabilities including:
//! - Active-active replication
//! - Asynchronous replication with batching
//! - Bi-directional sync
//! - Conflict-free replicated data types (CRDTs)
//! - Multiple replication topologies (star, mesh, tree)

pub mod active_active;
pub mod lag_monitor;
pub mod protocol;

use crate::error::{HaError, HaResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Replication topology types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplicationTopology {
    /// Star topology - one primary, multiple replicas.
    Star,
    /// Mesh topology - all nodes replicate to all other nodes.
    Mesh,
    /// Tree topology - hierarchical replication.
    Tree,
}

/// Replication mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplicationMode {
    /// Synchronous replication - wait for acknowledgment.
    Synchronous,
    /// Asynchronous replication - don't wait for acknowledgment.
    Asynchronous,
    /// Semi-synchronous - wait for at least one replica.
    SemiSynchronous,
}

/// Replication state for a node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplicationState {
    /// Node is actively replicating.
    Active,
    /// Node is catching up (lagging).
    CatchingUp,
    /// Node is paused.
    Paused,
    /// Node has failed.
    Failed,
    /// Node is being initialized.
    Initializing,
}

/// Replication configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationConfig {
    /// Replication mode.
    pub mode: ReplicationMode,
    /// Replication topology.
    pub topology: ReplicationTopology,
    /// Batch size for replication.
    pub batch_size: usize,
    /// Batch timeout in milliseconds.
    pub batch_timeout_ms: u64,
    /// Maximum replication lag in milliseconds.
    pub max_lag_ms: u64,
    /// Enable compression for replication data.
    pub enable_compression: bool,
    /// Compression level (1-9 for most algorithms).
    pub compression_level: u32,
    /// Maximum retry attempts.
    pub max_retries: u32,
    /// Retry backoff in milliseconds.
    pub retry_backoff_ms: u64,
}

impl Default for ReplicationConfig {
    fn default() -> Self {
        Self {
            mode: ReplicationMode::Asynchronous,
            topology: ReplicationTopology::Star,
            batch_size: 1000,
            batch_timeout_ms: 100,
            max_lag_ms: 5000,
            enable_compression: true,
            compression_level: 6,
            max_retries: 3,
            retry_backoff_ms: 1000,
        }
    }
}

/// Replica node information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaNode {
    /// Unique node ID.
    pub id: Uuid,
    /// Node name.
    pub name: String,
    /// Node address (e.g., "host:port").
    pub address: String,
    /// Node priority for leader election (higher is better).
    pub priority: u32,
    /// Current replication state.
    pub state: ReplicationState,
    /// Last successful replication timestamp.
    pub last_replicated_at: Option<DateTime<Utc>>,
    /// Current replication lag in milliseconds.
    pub lag_ms: Option<u64>,
}

/// Replication event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationEvent {
    /// Event ID.
    pub id: Uuid,
    /// Source node ID.
    pub source_node_id: Uuid,
    /// Target node ID.
    pub target_node_id: Uuid,
    /// Event timestamp.
    pub timestamp: DateTime<Utc>,
    /// Event data.
    pub data: Vec<u8>,
    /// Event sequence number.
    pub sequence: u64,
    /// Checksum for data integrity.
    pub checksum: u32,
}

impl ReplicationEvent {
    /// Create a new replication event.
    pub fn new(source_node_id: Uuid, target_node_id: Uuid, data: Vec<u8>, sequence: u64) -> Self {
        let checksum = crc32fast::hash(&data);
        Self {
            id: Uuid::new_v4(),
            source_node_id,
            target_node_id,
            timestamp: Utc::now(),
            data,
            sequence,
            checksum,
        }
    }

    /// Verify event data integrity.
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

/// Replication statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReplicationStats {
    /// Total events replicated.
    pub events_replicated: u64,
    /// Total bytes replicated.
    pub bytes_replicated: u64,
    /// Total events failed.
    pub events_failed: u64,
    /// Current replication lag in milliseconds.
    pub current_lag_ms: u64,
    /// Average replication lag in milliseconds.
    pub average_lag_ms: u64,
    /// Peak replication lag in milliseconds.
    pub peak_lag_ms: u64,
    /// Replication throughput in events per second.
    pub throughput_eps: f64,
    /// Replication bandwidth in bytes per second.
    pub bandwidth_bps: f64,
}

/// Trait for replication manager.
#[async_trait]
pub trait ReplicationManager: Send + Sync {
    /// Start replication.
    async fn start(&self) -> HaResult<()>;

    /// Stop replication.
    async fn stop(&self) -> HaResult<()>;

    /// Replicate an event to replicas.
    async fn replicate(&self, event: ReplicationEvent) -> HaResult<()>;

    /// Replicate a batch of events.
    async fn replicate_batch(&self, events: Vec<ReplicationEvent>) -> HaResult<()>;

    /// Get replication statistics.
    async fn get_stats(&self) -> HaResult<ReplicationStats>;

    /// Get replica nodes.
    async fn get_replicas(&self) -> HaResult<Vec<ReplicaNode>>;

    /// Add a replica node.
    async fn add_replica(&self, replica: ReplicaNode) -> HaResult<()>;

    /// Remove a replica node.
    async fn remove_replica(&self, node_id: Uuid) -> HaResult<()>;

    /// Pause replication to a specific replica.
    async fn pause_replica(&self, node_id: Uuid) -> HaResult<()>;

    /// Resume replication to a specific replica.
    async fn resume_replica(&self, node_id: Uuid) -> HaResult<()>;
}

/// Vector clock for tracking causality.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VectorClock {
    /// Clock values per node.
    pub clocks: HashMap<Uuid, u64>,
}

impl VectorClock {
    /// Create a new vector clock.
    pub fn new() -> Self {
        Self {
            clocks: HashMap::new(),
        }
    }

    /// Increment the clock for a node.
    pub fn increment(&mut self, node_id: Uuid) {
        let counter = self.clocks.entry(node_id).or_insert(0);
        *counter += 1;
    }

    /// Merge with another vector clock.
    pub fn merge(&mut self, other: &VectorClock) {
        for (node_id, &clock) in &other.clocks {
            let counter = self.clocks.entry(*node_id).or_insert(0);
            *counter = (*counter).max(clock);
        }
    }

    /// Check if this clock happens before another.
    pub fn happens_before(&self, other: &VectorClock) -> bool {
        let mut strictly_less = false;
        for (node_id, &our_clock) in &self.clocks {
            let their_clock = other.clocks.get(node_id).copied().unwrap_or(0);
            if our_clock > their_clock {
                return false;
            }
            if our_clock < their_clock {
                strictly_less = true;
            }
        }
        for (node_id, &their_clock) in &other.clocks {
            if !self.clocks.contains_key(node_id) && their_clock > 0 {
                strictly_less = true;
            }
        }
        strictly_less
    }

    /// Check if this clock is concurrent with another.
    pub fn is_concurrent(&self, other: &VectorClock) -> bool {
        !self.happens_before(other) && !other.happens_before(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vector_clock_ordering() {
        let mut clock1 = VectorClock::new();
        let mut clock2 = VectorClock::new();

        let node1 = Uuid::new_v4();
        let node2 = Uuid::new_v4();

        clock1.increment(node1);
        assert!(!clock1.happens_before(&clock2));
        assert!(clock2.happens_before(&clock1));

        clock2.increment(node2);
        assert!(clock1.is_concurrent(&clock2));
        assert!(clock2.is_concurrent(&clock1));

        clock2.merge(&clock1);
        assert!(clock1.happens_before(&clock2));
        assert!(!clock2.happens_before(&clock1));
    }

    #[test]
    fn test_replication_event_checksum() {
        let source = Uuid::new_v4();
        let target = Uuid::new_v4();
        let data = vec![1, 2, 3, 4, 5];

        let event = ReplicationEvent::new(source, target, data, 1);
        assert!(event.verify_checksum().is_ok());
    }
}
