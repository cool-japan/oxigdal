//! Data replication for reliability and availability.
//!
//! This module implements data replication with configurable replication factor,
//! quorum-based reads/writes, replica placement strategy, and automatic re-replication.

use crate::error::{ClusterError, Result};
use crate::worker_pool::WorkerId;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tracing::{debug, info, warn};

/// Replication manager.
#[derive(Clone)]
pub struct ReplicationManager {
    inner: Arc<ReplicationInner>,
}

struct ReplicationInner {
    /// Replica locations (data_id -> replicas)
    replicas: DashMap<String, ReplicaSet>,

    /// Worker health (for placement decisions)
    worker_health: DashMap<WorkerId, WorkerHealth>,

    /// Configuration
    config: ReplicationConfig,

    /// Statistics
    stats: Arc<ReplicationStats>,
}

/// Replication configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationConfig {
    /// Default replication factor
    pub replication_factor: usize,

    /// Minimum replication factor
    pub min_replicas: usize,

    /// Maximum replication factor
    pub max_replicas: usize,

    /// Read quorum size
    pub read_quorum: usize,

    /// Write quorum size
    pub write_quorum: usize,

    /// Replica placement strategy
    pub placement_strategy: PlacementStrategy,

    /// Enable automatic re-replication
    pub auto_rereplication: bool,

    /// Re-replication check interval
    pub rereplication_interval: std::time::Duration,

    /// Rack awareness (spread across racks)
    pub rack_aware: bool,
}

impl Default for ReplicationConfig {
    fn default() -> Self {
        Self {
            replication_factor: 3,
            min_replicas: 2,
            max_replicas: 5,
            read_quorum: 2,
            write_quorum: 2,
            placement_strategy: PlacementStrategy::Random,
            auto_rereplication: true,
            rereplication_interval: std::time::Duration::from_secs(60),
            rack_aware: false,
        }
    }
}

/// Replica placement strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlacementStrategy {
    /// Random placement
    Random,

    /// Least loaded workers first
    LeastLoaded,

    /// Rack-aware (spread across racks)
    RackAware,

    /// Zone-aware (spread across zones)
    ZoneAware,
}

/// Replica set for a data item.
#[derive(Debug, Clone)]
pub struct ReplicaSet {
    /// Data ID
    pub data_id: String,

    /// Replica locations
    pub replicas: Vec<Replica>,

    /// Primary replica (for write coordination)
    pub primary: Option<WorkerId>,

    /// Data version
    pub version: u64,

    /// Created at
    pub created_at: Instant,

    /// Last updated
    pub last_updated: Instant,

    /// Data size (bytes)
    pub size_bytes: u64,
}

/// Individual replica.
#[derive(Debug, Clone)]
pub struct Replica {
    /// Worker ID
    pub worker_id: WorkerId,

    /// Replica status
    pub status: ReplicaStatus,

    /// Version
    pub version: u64,

    /// Created at
    pub created_at: Instant,

    /// Last verified
    pub last_verified: Option<Instant>,

    /// Rack ID (for rack-aware placement)
    pub rack_id: Option<String>,

    /// Zone ID (for zone-aware placement)
    pub zone_id: Option<String>,
}

/// Replica status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplicaStatus {
    /// Replica is healthy
    Healthy,

    /// Replica is being created
    Replicating,

    /// Replica is stale (version mismatch)
    Stale,

    /// Replica is unavailable
    Unavailable,
}

/// Worker health information.
#[derive(Debug, Clone)]
pub struct WorkerHealth {
    /// Worker ID
    pub worker_id: WorkerId,

    /// Health status
    pub healthy: bool,

    /// Load (0.0-1.0)
    pub load: f64,

    /// Available storage (bytes)
    pub available_storage: u64,

    /// Rack ID
    pub rack_id: Option<String>,

    /// Zone ID
    pub zone_id: Option<String>,

    /// Last updated
    pub last_updated: Instant,
}

/// Replication statistics.
#[derive(Debug, Default)]
struct ReplicationStats {
    /// Total replicas created
    replicas_created: AtomicU64,

    /// Replicas removed
    replicas_removed: AtomicU64,

    /// Re-replications performed
    rereplications: AtomicU64,

    /// Quorum reads
    quorum_reads: AtomicU64,

    /// Quorum writes
    quorum_writes: AtomicU64,

    /// Quorum failures
    quorum_failures: AtomicU64,
}

impl ReplicationManager {
    /// Create a new replication manager.
    pub fn new(config: ReplicationConfig) -> Self {
        Self {
            inner: Arc::new(ReplicationInner {
                replicas: DashMap::new(),
                worker_health: DashMap::new(),
                config,
                stats: Arc::new(ReplicationStats::default()),
            }),
        }
    }

    /// Create with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(ReplicationConfig::default())
    }

    /// Create replica set for data.
    pub fn create_replicas(
        &self,
        data_id: String,
        size_bytes: u64,
        available_workers: &[WorkerId],
    ) -> Result<ReplicaSet> {
        if available_workers.len() < self.inner.config.min_replicas {
            return Err(ClusterError::ReplicaPlacementError(format!(
                "Not enough workers: need {}, have {}",
                self.inner.config.min_replicas,
                available_workers.len()
            )));
        }

        // Select workers for replicas
        let selected_workers =
            self.select_replica_workers(available_workers, self.inner.config.replication_factor)?;

        let now = Instant::now();
        let mut replicas = Vec::new();

        for worker_id in selected_workers {
            let health = self.inner.worker_health.get(&worker_id);

            let replica = Replica {
                worker_id,
                status: ReplicaStatus::Healthy,
                version: 1,
                created_at: now,
                last_verified: Some(now),
                rack_id: health.as_ref().and_then(|h| h.rack_id.clone()),
                zone_id: health.as_ref().and_then(|h| h.zone_id.clone()),
            };

            replicas.push(replica);

            self.inner
                .stats
                .replicas_created
                .fetch_add(1, Ordering::Relaxed);
        }

        let replica_set = ReplicaSet {
            data_id: data_id.clone(),
            replicas,
            primary: None, // Will be elected if needed
            version: 1,
            created_at: now,
            last_updated: now,
            size_bytes,
        };

        self.inner.replicas.insert(data_id, replica_set.clone());

        Ok(replica_set)
    }

    /// Select workers for replica placement.
    fn select_replica_workers(
        &self,
        available: &[WorkerId],
        count: usize,
    ) -> Result<Vec<WorkerId>> {
        let count = count.min(available.len());

        match self.inner.config.placement_strategy {
            PlacementStrategy::Random => {
                // Simple random selection
                Ok(available.iter().take(count).copied().collect())
            }
            PlacementStrategy::LeastLoaded => {
                // Sort by load and select least loaded
                let mut workers_with_load: Vec<_> = available
                    .iter()
                    .map(|&id| {
                        let load = self
                            .inner
                            .worker_health
                            .get(&id)
                            .map(|h| h.load)
                            .unwrap_or(1.0);
                        (id, load)
                    })
                    .collect();

                workers_with_load
                    .sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

                Ok(workers_with_load
                    .into_iter()
                    .take(count)
                    .map(|(id, _)| id)
                    .collect())
            }
            PlacementStrategy::RackAware => {
                // Spread across racks
                self.select_rack_aware(available, count)
            }
            PlacementStrategy::ZoneAware => {
                // Spread across zones
                self.select_zone_aware(available, count)
            }
        }
    }

    /// Select workers with rack awareness.
    fn select_rack_aware(&self, available: &[WorkerId], count: usize) -> Result<Vec<WorkerId>> {
        let mut selected = Vec::new();
        let mut used_racks = HashSet::new();

        // First pass: select one from each rack
        for &worker_id in available {
            if selected.len() >= count {
                break;
            }

            if let Some(health) = self.inner.worker_health.get(&worker_id) {
                if let Some(rack_id) = &health.rack_id {
                    if !used_racks.contains(rack_id) {
                        selected.push(worker_id);
                        used_racks.insert(rack_id.clone());
                    }
                }
            }
        }

        // Second pass: fill remaining slots
        for &worker_id in available {
            if selected.len() >= count {
                break;
            }
            if !selected.contains(&worker_id) {
                selected.push(worker_id);
            }
        }

        Ok(selected)
    }

    /// Select workers with zone awareness.
    fn select_zone_aware(&self, available: &[WorkerId], count: usize) -> Result<Vec<WorkerId>> {
        let mut selected = Vec::new();
        let mut used_zones = HashSet::new();

        // First pass: select one from each zone
        for &worker_id in available {
            if selected.len() >= count {
                break;
            }

            if let Some(health) = self.inner.worker_health.get(&worker_id) {
                if let Some(zone_id) = &health.zone_id {
                    if !used_zones.contains(zone_id) {
                        selected.push(worker_id);
                        used_zones.insert(zone_id.clone());
                    }
                }
            }
        }

        // Second pass: fill remaining slots
        for &worker_id in available {
            if selected.len() >= count {
                break;
            }
            if !selected.contains(&worker_id) {
                selected.push(worker_id);
            }
        }

        Ok(selected)
    }

    /// Get replica set for data.
    pub fn get_replicas(&self, data_id: &str) -> Option<ReplicaSet> {
        self.inner.replicas.get(data_id).map(|r| r.clone())
    }

    /// Perform quorum read.
    pub async fn quorum_read(&self, data_id: &str) -> Result<Vec<WorkerId>> {
        let replica_set = self
            .get_replicas(data_id)
            .ok_or_else(|| ClusterError::DataNotAvailable(data_id.to_string()))?;

        let healthy_replicas: Vec<_> = replica_set
            .replicas
            .iter()
            .filter(|r| r.status == ReplicaStatus::Healthy)
            .map(|r| r.worker_id)
            .collect();

        if healthy_replicas.len() < self.inner.config.read_quorum {
            self.inner
                .stats
                .quorum_failures
                .fetch_add(1, Ordering::Relaxed);

            return Err(ClusterError::QuorumNotReached {
                required: self.inner.config.read_quorum,
                actual: healthy_replicas.len(),
            });
        }

        self.inner
            .stats
            .quorum_reads
            .fetch_add(1, Ordering::Relaxed);

        Ok(healthy_replicas
            .into_iter()
            .take(self.inner.config.read_quorum)
            .collect())
    }

    /// Perform quorum write.
    pub async fn quorum_write(&self, data_id: &str) -> Result<Vec<WorkerId>> {
        let replica_set = self
            .get_replicas(data_id)
            .ok_or_else(|| ClusterError::DataNotAvailable(data_id.to_string()))?;

        let healthy_replicas: Vec<_> = replica_set
            .replicas
            .iter()
            .filter(|r| r.status == ReplicaStatus::Healthy)
            .map(|r| r.worker_id)
            .collect();

        if healthy_replicas.len() < self.inner.config.write_quorum {
            self.inner
                .stats
                .quorum_failures
                .fetch_add(1, Ordering::Relaxed);

            return Err(ClusterError::QuorumNotReached {
                required: self.inner.config.write_quorum,
                actual: healthy_replicas.len(),
            });
        }

        self.inner
            .stats
            .quorum_writes
            .fetch_add(1, Ordering::Relaxed);

        Ok(healthy_replicas
            .into_iter()
            .take(self.inner.config.write_quorum)
            .collect())
    }

    /// Update worker health.
    pub fn update_worker_health(&self, health: WorkerHealth) {
        self.inner.worker_health.insert(health.worker_id, health);
    }

    /// Mark replica as failed.
    pub fn mark_replica_failed(&self, data_id: &str, worker_id: WorkerId) -> Result<()> {
        if let Some(mut replica_set) = self.inner.replicas.get_mut(data_id) {
            for replica in &mut replica_set.replicas {
                if replica.worker_id == worker_id {
                    replica.status = ReplicaStatus::Unavailable;
                    warn!(
                        "Marked replica as unavailable: {} on {}",
                        data_id, worker_id
                    );
                    break;
                }
            }
        }

        Ok(())
    }

    /// Check and perform re-replication if needed.
    pub fn check_rereplication(&self, available_workers: &[WorkerId]) -> Vec<(String, WorkerId)> {
        if !self.inner.config.auto_rereplication {
            return Vec::new();
        }

        let mut rereplications = Vec::new();

        for entry in self.inner.replicas.iter() {
            let data_id = entry.key().clone();
            let replica_set = entry.value();

            let healthy_count = replica_set
                .replicas
                .iter()
                .filter(|r| r.status == ReplicaStatus::Healthy)
                .count();

            if healthy_count < self.inner.config.replication_factor {
                // Need to create more replicas to maintain replication factor
                let needed = self.inner.config.replication_factor - healthy_count;

                // Find workers not already having this data
                let existing: HashSet<_> =
                    replica_set.replicas.iter().map(|r| r.worker_id).collect();

                let candidates: Vec<_> = available_workers
                    .iter()
                    .filter(|w| !existing.contains(w))
                    .copied()
                    .collect();

                match self.select_replica_workers(&candidates, needed) {
                    Ok(new_workers) => {
                        for worker_id in new_workers {
                            rereplications.push((data_id.clone(), worker_id));

                            self.inner
                                .stats
                                .rereplications
                                .fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    Err(e) => {
                        warn!(
                            "Failed to select workers for re-replication of {}: {}",
                            data_id, e
                        );
                    }
                }
            }
        }

        if !rereplications.is_empty() {
            info!("Scheduled {} re-replications", rereplications.len());
        }

        rereplications
    }

    /// Add replica to existing set.
    pub fn add_replica(&self, data_id: &str, worker_id: WorkerId) -> Result<()> {
        if let Some(mut replica_set) = self.inner.replicas.get_mut(data_id) {
            let health = self.inner.worker_health.get(&worker_id);

            let replica = Replica {
                worker_id,
                status: ReplicaStatus::Healthy,
                version: replica_set.version,
                created_at: Instant::now(),
                last_verified: Some(Instant::now()),
                rack_id: health.as_ref().and_then(|h| h.rack_id.clone()),
                zone_id: health.as_ref().and_then(|h| h.zone_id.clone()),
            };

            replica_set.replicas.push(replica);
            replica_set.last_updated = Instant::now();

            self.inner
                .stats
                .replicas_created
                .fetch_add(1, Ordering::Relaxed);

            debug!("Added replica for {} on worker {}", data_id, worker_id);
        }

        Ok(())
    }

    /// Remove replica.
    pub fn remove_replica(&self, data_id: &str, worker_id: WorkerId) -> Result<()> {
        if let Some(mut replica_set) = self.inner.replicas.get_mut(data_id) {
            replica_set.replicas.retain(|r| r.worker_id != worker_id);
            replica_set.last_updated = Instant::now();

            self.inner
                .stats
                .replicas_removed
                .fetch_add(1, Ordering::Relaxed);

            debug!("Removed replica for {} from worker {}", data_id, worker_id);
        }

        Ok(())
    }

    /// Get statistics.
    pub fn get_statistics(&self) -> ReplicationStatistics {
        ReplicationStatistics {
            replicas_created: self.inner.stats.replicas_created.load(Ordering::Relaxed),
            replicas_removed: self.inner.stats.replicas_removed.load(Ordering::Relaxed),
            rereplications: self.inner.stats.rereplications.load(Ordering::Relaxed),
            quorum_reads: self.inner.stats.quorum_reads.load(Ordering::Relaxed),
            quorum_writes: self.inner.stats.quorum_writes.load(Ordering::Relaxed),
            quorum_failures: self.inner.stats.quorum_failures.load(Ordering::Relaxed),
            total_replica_sets: self.inner.replicas.len(),
            total_workers: self.inner.worker_health.len(),
        }
    }
}

/// Replication statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationStatistics {
    /// Replicas created
    pub replicas_created: u64,

    /// Replicas removed
    pub replicas_removed: u64,

    /// Re-replications
    pub rereplications: u64,

    /// Quorum reads
    pub quorum_reads: u64,

    /// Quorum writes
    pub quorum_writes: u64,

    /// Quorum failures
    pub quorum_failures: u64,

    /// Total replica sets
    pub total_replica_sets: usize,

    /// Total workers
    pub total_workers: usize,
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_replication_manager_creation() {
        let mgr = ReplicationManager::with_defaults();
        let stats = mgr.get_statistics();
        assert_eq!(stats.replicas_created, 0);
    }

    #[test]
    fn test_create_replicas() {
        let mgr = ReplicationManager::with_defaults();

        let workers: Vec<_> = (0..5).map(|_| WorkerId::new()).collect();

        let result = mgr.create_replicas("data1".to_string(), 1000, &workers);
        assert!(result.is_ok());

        if let Ok(replica_set) = result {
            assert_eq!(replica_set.replicas.len(), 3); // default replication factor
        }
    }

    #[tokio::test]
    async fn test_quorum_operations() {
        let mgr = ReplicationManager::with_defaults();

        let workers: Vec<_> = (0..5).map(|_| WorkerId::new()).collect();

        mgr.create_replicas("data1".to_string(), 1000, &workers)
            .ok();

        let read_result = mgr.quorum_read("data1").await;
        assert!(read_result.is_ok());

        let write_result = mgr.quorum_write("data1").await;
        assert!(write_result.is_ok());
    }

    #[test]
    fn test_rereplication() {
        let mgr = ReplicationManager::with_defaults();

        let workers: Vec<_> = (0..5).map(|_| WorkerId::new()).collect();

        mgr.create_replicas("data1".to_string(), 1000, &workers)
            .ok();

        // Mark a replica as failed
        mgr.mark_replica_failed("data1", workers[0]).ok();

        // Check for re-replication
        let rereplications = mgr.check_rereplication(&workers);
        assert!(!rereplications.is_empty());
    }
}
