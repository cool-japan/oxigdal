//! Data locality optimization for minimizing data transfer.
//!
//! This module implements data locality tracking and optimization to minimize
//! network transfer by placing tasks near their required data.

use crate::error::{ClusterError, Result};
use crate::worker_pool::WorkerId;
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Data locality optimizer.
#[derive(Clone)]
pub struct DataLocalityOptimizer {
    inner: Arc<DataLocalityInner>,
}

struct DataLocalityInner {
    /// Data location tracking (data_key -> workers)
    data_locations: DashMap<String, HashSet<WorkerId>>,

    /// Worker data inventory (worker -> data_keys)
    worker_data: DashMap<WorkerId, HashSet<String>>,

    /// Data access patterns (data_key -> access count)
    access_patterns: DashMap<String, AtomicU64>,

    /// Data affinity (data_key -> preferred workers)
    data_affinity: DashMap<String, Vec<WorkerId>>,

    /// Prefetch schedule (data_key -> workers to prefetch)
    prefetch_schedule: RwLock<HashMap<String, Vec<WorkerId>>>,

    /// Configuration
    config: LocalityConfig,

    /// Statistics
    stats: Arc<LocalityStatistics>,
}

/// Locality optimizer configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalityConfig {
    /// Minimum replication factor
    pub min_replication: usize,

    /// Maximum replication factor
    pub max_replication: usize,

    /// Access count threshold for hot data
    pub hot_data_threshold: u64,

    /// Enable prefetching
    pub enable_prefetch: bool,

    /// Prefetch lookahead distance
    pub prefetch_lookahead: usize,

    /// Enable affinity tracking
    pub enable_affinity: bool,

    /// Affinity update interval (number of accesses)
    pub affinity_update_interval: u64,
}

impl Default for LocalityConfig {
    fn default() -> Self {
        Self {
            min_replication: 2,
            max_replication: 5,
            hot_data_threshold: 100,
            enable_prefetch: true,
            prefetch_lookahead: 10,
            enable_affinity: true,
            affinity_update_interval: 10,
        }
    }
}

/// Locality statistics.
#[derive(Debug, Default)]
struct LocalityStatistics {
    /// Locality hits (task placed on worker with data)
    locality_hits: AtomicU64,

    /// Locality misses (task placed on worker without data)
    locality_misses: AtomicU64,

    /// Data transfers initiated
    data_transfers: AtomicU64,

    /// Prefetches performed
    prefetches: AtomicU64,

    /// Bytes transferred
    bytes_transferred: AtomicU64,
}

/// Data location information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataLocation {
    /// Data key
    pub key: String,

    /// Workers that have this data
    pub workers: Vec<WorkerId>,

    /// Data size (bytes)
    pub size_bytes: u64,

    /// Access count
    pub access_count: u64,

    /// Last accessed time
    #[serde(skip)]
    pub last_accessed: Option<Instant>,
}

/// Task placement recommendation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementRecommendation {
    /// Recommended worker
    pub worker_id: WorkerId,

    /// Locality score (0.0-1.0, higher is better)
    pub locality_score: f64,

    /// Data already on worker
    pub local_data: Vec<String>,

    /// Data to transfer
    pub transfer_data: Vec<String>,

    /// Estimated transfer size (bytes)
    pub estimated_transfer_bytes: u64,
}

impl DataLocalityOptimizer {
    /// Create a new data locality optimizer.
    pub fn new(config: LocalityConfig) -> Self {
        Self {
            inner: Arc::new(DataLocalityInner {
                data_locations: DashMap::new(),
                worker_data: DashMap::new(),
                access_patterns: DashMap::new(),
                data_affinity: DashMap::new(),
                prefetch_schedule: RwLock::new(HashMap::new()),
                config,
                stats: Arc::new(LocalityStatistics::default()),
            }),
        }
    }

    /// Create with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(LocalityConfig::default())
    }

    /// Register data location.
    pub fn register_data(&self, data_key: String, worker_id: WorkerId) -> Result<()> {
        // Add to data locations
        self.inner
            .data_locations
            .entry(data_key.clone())
            .or_default()
            .insert(worker_id);

        // Add to worker data inventory
        self.inner
            .worker_data
            .entry(worker_id)
            .or_default()
            .insert(data_key.clone());

        // Initialize access pattern
        self.inner
            .access_patterns
            .entry(data_key)
            .or_insert_with(|| AtomicU64::new(0));

        Ok(())
    }

    /// Unregister data location.
    pub fn unregister_data(&self, data_key: &str, worker_id: WorkerId) -> Result<()> {
        // Remove from data locations
        if let Some(mut locations) = self.inner.data_locations.get_mut(data_key) {
            locations.remove(&worker_id);
        }

        // Remove from worker data inventory
        if let Some(mut worker_data) = self.inner.worker_data.get_mut(&worker_id) {
            worker_data.remove(data_key);
        }

        Ok(())
    }

    /// Record data access.
    pub fn record_access(&self, data_key: &str, worker_id: WorkerId) -> Result<()> {
        // Increment access count
        let access_count = self
            .inner
            .access_patterns
            .entry(data_key.to_string())
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed)
            + 1;

        // Update affinity if enabled
        if self.inner.config.enable_affinity
            && access_count % self.inner.config.affinity_update_interval == 0
        {
            self.update_affinity(data_key, worker_id)?;
        }

        Ok(())
    }

    /// Update data affinity.
    fn update_affinity(&self, data_key: &str, worker_id: WorkerId) -> Result<()> {
        let mut affinity = self
            .inner
            .data_affinity
            .entry(data_key.to_string())
            .or_default();

        // Add worker if not already in affinity list
        if !affinity.contains(&worker_id) {
            affinity.push(worker_id);

            // Keep only top N workers
            if affinity.len() > 5 {
                affinity.remove(0);
            }
        } else {
            // Move to end (most recent)
            if let Some(pos) = affinity.iter().position(|&id| id == worker_id) {
                affinity.remove(pos);
                affinity.push(worker_id);
            }
        }

        Ok(())
    }

    /// Get workers that have specific data.
    pub fn get_workers_with_data(&self, data_key: &str) -> Vec<WorkerId> {
        self.inner
            .data_locations
            .get(data_key)
            .map(|locations| locations.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Get data available on a worker.
    pub fn get_worker_data(&self, worker_id: WorkerId) -> Vec<String> {
        self.inner
            .worker_data
            .get(&worker_id)
            .map(|data| data.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Get data location information.
    pub fn get_data_location(&self, data_key: &str) -> Option<DataLocation> {
        let workers = self
            .inner
            .data_locations
            .get(data_key)?
            .iter()
            .copied()
            .collect();

        let access_count = self
            .inner
            .access_patterns
            .get(data_key)
            .map(|c| c.load(Ordering::Relaxed))
            .unwrap_or(0);

        Some(DataLocation {
            key: data_key.to_string(),
            workers,
            size_bytes: 0, // Would need to track separately
            access_count,
            last_accessed: None,
        })
    }

    /// Recommend task placement based on data locality.
    pub fn recommend_placement(
        &self,
        required_data: &[String],
        candidate_workers: &[WorkerId],
    ) -> Result<PlacementRecommendation> {
        if candidate_workers.is_empty() {
            return Err(ClusterError::DataLocalityError(
                "No candidate workers provided".to_string(),
            ));
        }

        let mut best_worker = candidate_workers[0];
        let mut best_score = 0.0;
        let mut best_local = Vec::new();
        let mut best_transfer = Vec::new();

        for &worker_id in candidate_workers {
            let worker_data = self.get_worker_data(worker_id);
            let worker_data_set: HashSet<_> = worker_data.iter().collect();

            let mut local_data = Vec::new();
            let mut transfer_data = Vec::new();

            for data_key in required_data {
                if worker_data_set.contains(&data_key) {
                    local_data.push(data_key.clone());
                } else {
                    transfer_data.push(data_key.clone());
                }
            }

            // Calculate locality score
            let locality_score = if required_data.is_empty() {
                1.0
            } else {
                local_data.len() as f64 / required_data.len() as f64
            };

            if locality_score > best_score {
                best_score = locality_score;
                best_worker = worker_id;
                best_local = local_data;
                best_transfer = transfer_data;
            }
        }

        // Record hit or miss
        if best_score == 1.0 {
            self.inner
                .stats
                .locality_hits
                .fetch_add(1, Ordering::Relaxed);
        } else {
            self.inner
                .stats
                .locality_misses
                .fetch_add(1, Ordering::Relaxed);
        }

        Ok(PlacementRecommendation {
            worker_id: best_worker,
            locality_score: best_score,
            local_data: best_local,
            transfer_data: best_transfer,
            estimated_transfer_bytes: 0, // Would need data size info
        })
    }

    /// Schedule data prefetch for upcoming tasks.
    pub fn schedule_prefetch(&self, data_key: String, target_workers: Vec<WorkerId>) -> Result<()> {
        if !self.inner.config.enable_prefetch {
            return Ok(());
        }

        let mut schedule = self.inner.prefetch_schedule.write();
        schedule.insert(data_key, target_workers);

        Ok(())
    }

    /// Get prefetch schedule.
    pub fn get_prefetch_schedule(&self) -> HashMap<String, Vec<WorkerId>> {
        self.inner.prefetch_schedule.read().clone()
    }

    /// Clear prefetch schedule.
    pub fn clear_prefetch_schedule(&self) {
        self.inner.prefetch_schedule.write().clear();
    }

    /// Record data transfer.
    pub fn record_transfer(
        &self,
        data_key: &str,
        _from_worker: WorkerId,
        to_worker: WorkerId,
        bytes: u64,
    ) -> Result<()> {
        self.inner
            .stats
            .data_transfers
            .fetch_add(1, Ordering::Relaxed);

        self.inner
            .stats
            .bytes_transferred
            .fetch_add(bytes, Ordering::Relaxed);

        // Register data at new location
        self.register_data(data_key.to_string(), to_worker)?;

        Ok(())
    }

    /// Record prefetch.
    pub fn record_prefetch(&self) {
        self.inner.stats.prefetches.fetch_add(1, Ordering::Relaxed);
    }

    /// Get hot data (frequently accessed).
    pub fn get_hot_data(&self) -> Vec<(String, u64)> {
        let mut hot_data: Vec<_> = self
            .inner
            .access_patterns
            .iter()
            .filter_map(|entry| {
                let key = entry.key().clone();
                let count = entry.value().load(Ordering::Relaxed);
                if count >= self.inner.config.hot_data_threshold {
                    Some((key, count))
                } else {
                    None
                }
            })
            .collect();

        hot_data.sort_by_key(|x| std::cmp::Reverse(x.1));
        hot_data
    }

    /// Suggest replication for hot data.
    pub fn suggest_replication(&self) -> Vec<(String, usize)> {
        let mut suggestions = Vec::new();

        for (data_key, access_count) in self.get_hot_data() {
            let current_replication = self
                .inner
                .data_locations
                .get(&data_key)
                .map(|locs| locs.len())
                .unwrap_or(0);

            // Suggest higher replication for hot data
            let suggested_replication = if access_count > self.inner.config.hot_data_threshold * 10
            {
                self.inner.config.max_replication
            } else if access_count > self.inner.config.hot_data_threshold * 5 {
                (self.inner.config.max_replication + self.inner.config.min_replication) / 2
            } else {
                self.inner.config.min_replication
            };

            if suggested_replication > current_replication {
                suggestions.push((data_key, suggested_replication - current_replication));
            }
        }

        suggestions
    }

    /// Get affinity for data.
    pub fn get_affinity(&self, data_key: &str) -> Vec<WorkerId> {
        self.inner
            .data_affinity
            .get(data_key)
            .map(|affinity| affinity.clone())
            .unwrap_or_default()
    }

    /// Remove worker from all data tracking.
    pub fn remove_worker(&self, worker_id: WorkerId) -> Result<()> {
        // Remove from all data locations
        if let Some((_, data_keys)) = self.inner.worker_data.remove(&worker_id) {
            for data_key in data_keys {
                if let Some(mut locations) = self.inner.data_locations.get_mut(&data_key) {
                    locations.remove(&worker_id);
                }
            }
        }

        // Remove from affinity lists
        for mut affinity in self.inner.data_affinity.iter_mut() {
            affinity.retain(|&id| id != worker_id);
        }

        Ok(())
    }

    /// Get locality statistics.
    pub fn get_statistics(&self) -> LocalityStats {
        let locality_hits = self.inner.stats.locality_hits.load(Ordering::Relaxed);
        let locality_misses = self.inner.stats.locality_misses.load(Ordering::Relaxed);

        let total_placements = locality_hits + locality_misses;
        let hit_rate = if total_placements > 0 {
            locality_hits as f64 / total_placements as f64
        } else {
            0.0
        };

        LocalityStats {
            locality_hits,
            locality_misses,
            hit_rate,
            data_transfers: self.inner.stats.data_transfers.load(Ordering::Relaxed),
            prefetches: self.inner.stats.prefetches.load(Ordering::Relaxed),
            bytes_transferred: self.inner.stats.bytes_transferred.load(Ordering::Relaxed),
            tracked_data_keys: self.inner.data_locations.len(),
            tracked_workers: self.inner.worker_data.len(),
        }
    }

    /// Reset statistics.
    pub fn reset_statistics(&self) {
        self.inner.stats.locality_hits.store(0, Ordering::Relaxed);
        self.inner.stats.locality_misses.store(0, Ordering::Relaxed);
        self.inner.stats.data_transfers.store(0, Ordering::Relaxed);
        self.inner.stats.prefetches.store(0, Ordering::Relaxed);
        self.inner
            .stats
            .bytes_transferred
            .store(0, Ordering::Relaxed);
    }
}

/// Locality statistics snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalityStats {
    /// Locality hits
    pub locality_hits: u64,

    /// Locality misses
    pub locality_misses: u64,

    /// Hit rate (0.0-1.0)
    pub hit_rate: f64,

    /// Data transfers
    pub data_transfers: u64,

    /// Prefetches
    pub prefetches: u64,

    /// Bytes transferred
    pub bytes_transferred: u64,

    /// Number of tracked data keys
    pub tracked_data_keys: usize,

    /// Number of tracked workers
    pub tracked_workers: usize,
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_locality_optimizer_creation() {
        let optimizer = DataLocalityOptimizer::with_defaults();
        let stats = optimizer.get_statistics();
        assert_eq!(stats.locality_hits, 0);
    }

    #[test]
    fn test_register_data() {
        let optimizer = DataLocalityOptimizer::with_defaults();
        let worker_id = WorkerId::new();

        let result = optimizer.register_data("data1".to_string(), worker_id);
        assert!(result.is_ok());

        let workers = optimizer.get_workers_with_data("data1");
        assert_eq!(workers.len(), 1);
        assert_eq!(workers[0], worker_id);
    }

    #[test]
    fn test_data_access_tracking() {
        let optimizer = DataLocalityOptimizer::with_defaults();
        let worker_id = WorkerId::new();

        optimizer.register_data("data1".to_string(), worker_id).ok();
        optimizer.record_access("data1", worker_id).ok();
        optimizer.record_access("data1", worker_id).ok();

        let location = optimizer.get_data_location("data1");
        assert!(location.is_some());
        if let Some(location) = location {
            assert_eq!(location.access_count, 2);
        }
    }

    #[test]
    fn test_placement_recommendation() {
        let optimizer = DataLocalityOptimizer::with_defaults();

        let worker1 = WorkerId::new();
        let worker2 = WorkerId::new();

        optimizer.register_data("data1".to_string(), worker1).ok();
        optimizer.register_data("data2".to_string(), worker1).ok();
        optimizer.register_data("data3".to_string(), worker2).ok();

        let required_data = vec!["data1".to_string(), "data2".to_string()];
        let candidates = vec![worker1, worker2];

        let recommendation = optimizer.recommend_placement(&required_data, &candidates);
        assert!(recommendation.is_ok());

        if let Ok(rec) = recommendation {
            assert_eq!(rec.worker_id, worker1);
            assert_eq!(rec.locality_score, 1.0);
        }
    }

    #[test]
    fn test_hot_data_detection() {
        let optimizer = DataLocalityOptimizer::with_defaults();
        let worker_id = WorkerId::new();

        optimizer
            .register_data("hot_data".to_string(), worker_id)
            .ok();

        // Access data many times
        for _ in 0..150 {
            optimizer.record_access("hot_data", worker_id).ok();
        }

        let hot_data = optimizer.get_hot_data();
        assert!(!hot_data.is_empty());
        assert_eq!(hot_data[0].0, "hot_data");
    }
}
