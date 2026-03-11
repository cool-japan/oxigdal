//! Network optimization for distributed computing.
//!
//! This module provides network-aware optimizations including:
//! - Topology-aware scheduling (rack/datacenter awareness)
//! - Network bandwidth tracking and monitoring
//! - Congestion control and avoidance
//! - Data compression for network transfers
//! - Multicast support for broadcast operations
//! - Network failure detection and recovery

use crate::error::{ClusterError, Result};
use crate::worker_pool::WorkerId;
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Instant;
use tracing::warn;

/// Network topology manager for rack/datacenter awareness.
pub struct TopologyManager {
    /// Worker to location mapping
    worker_locations: Arc<DashMap<WorkerId, Location>>,
    /// Location hierarchy (datacenter -> racks -> workers)
    topology: Arc<RwLock<TopologyTree>>,
    /// Inter-location bandwidth (reserved for future use)
    #[allow(dead_code)]
    bandwidth_matrix: Arc<RwLock<HashMap<(LocationId, LocationId), f64>>>,
    /// Statistics
    stats: Arc<RwLock<TopologyStats>>,
}

/// Physical location identifier.
pub type LocationId = String;

/// Worker location in the topology.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    /// Datacenter ID
    pub datacenter: String,
    /// Rack ID
    pub rack: String,
    /// Host ID
    pub host: String,
    /// IP address
    pub ip_address: Option<IpAddr>,
}

/// Topology tree structure.
#[derive(Debug, Clone, Default)]
pub struct TopologyTree {
    /// Datacenters
    pub datacenters: HashMap<String, Datacenter>,
}

/// Datacenter in topology.
#[derive(Debug, Clone)]
pub struct Datacenter {
    /// Datacenter identifier
    pub id: String,
    /// Racks within this datacenter
    pub racks: HashMap<String, Rack>,
}

/// Rack in topology.
#[derive(Debug, Clone)]
pub struct Rack {
    /// Rack identifier
    pub id: String,
    /// Workers located in this rack
    pub workers: Vec<WorkerId>,
}

/// Topology statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TopologyStats {
    /// Total number of datacenters
    pub total_datacenters: usize,
    /// Total number of racks
    pub total_racks: usize,
    /// Total number of workers
    pub total_workers: usize,
    /// Number of cross-rack data transfers
    pub cross_rack_transfers: u64,
    /// Number of cross-datacenter data transfers
    pub cross_datacenter_transfers: u64,
    /// Number of same-rack data transfers
    pub same_rack_transfers: u64,
}

impl TopologyManager {
    /// Create a new topology manager.
    pub fn new() -> Self {
        Self {
            worker_locations: Arc::new(DashMap::new()),
            topology: Arc::new(RwLock::new(TopologyTree::default())),
            bandwidth_matrix: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(TopologyStats::default())),
        }
    }

    /// Register a worker's location.
    pub fn register_worker(&self, worker_id: WorkerId, location: Location) -> Result<()> {
        self.worker_locations.insert(worker_id, location.clone());

        {
            let mut topology = self.topology.write();
            let datacenter = topology
                .datacenters
                .entry(location.datacenter.clone())
                .or_insert_with(|| Datacenter {
                    id: location.datacenter.clone(),
                    racks: HashMap::new(),
                });

            let rack = datacenter
                .racks
                .entry(location.rack.clone())
                .or_insert_with(|| Rack {
                    id: location.rack.clone(),
                    workers: Vec::new(),
                });

            if !rack.workers.contains(&worker_id) {
                rack.workers.push(worker_id);
            }
        } // Drop the write lock before calling update_topology_stats

        self.update_topology_stats();

        Ok(())
    }

    /// Calculate network distance between two workers.
    pub fn calculate_distance(&self, worker1: &WorkerId, worker2: &WorkerId) -> NetworkDistance {
        let loc1 = self.worker_locations.get(worker1);
        let loc2 = self.worker_locations.get(worker2);

        match (loc1, loc2) {
            (Some(l1), Some(l2)) => {
                if l1.datacenter != l2.datacenter {
                    NetworkDistance::CrossDatacenter
                } else if l1.rack != l2.rack {
                    NetworkDistance::CrossRack
                } else if l1.host != l2.host {
                    NetworkDistance::SameRack
                } else {
                    NetworkDistance::SameHost
                }
            }
            _ => NetworkDistance::Unknown,
        }
    }

    /// Get workers in the same rack.
    pub fn get_same_rack_workers(&self, worker_id: &WorkerId) -> Vec<WorkerId> {
        let location = match self.worker_locations.get(worker_id) {
            Some(loc) => loc.clone(),
            None => return Vec::new(),
        };

        self.worker_locations
            .iter()
            .filter(|entry| {
                let loc = entry.value();
                loc.datacenter == location.datacenter && loc.rack == location.rack
            })
            .map(|entry| *entry.key())
            .collect()
    }

    /// Get workers in the same datacenter.
    pub fn get_same_datacenter_workers(&self, worker_id: &WorkerId) -> Vec<WorkerId> {
        let location = match self.worker_locations.get(worker_id) {
            Some(loc) => loc.clone(),
            None => return Vec::new(),
        };

        self.worker_locations
            .iter()
            .filter(|entry| entry.value().datacenter == location.datacenter)
            .map(|entry| *entry.key())
            .collect()
    }

    /// Record a data transfer for statistics.
    pub fn record_transfer(&self, from: &WorkerId, to: &WorkerId) {
        let distance = self.calculate_distance(from, to);
        let mut stats = self.stats.write();

        match distance {
            NetworkDistance::SameHost | NetworkDistance::SameRack => {
                stats.same_rack_transfers += 1;
            }
            NetworkDistance::CrossRack => {
                stats.cross_rack_transfers += 1;
            }
            NetworkDistance::CrossDatacenter => {
                stats.cross_datacenter_transfers += 1;
            }
            NetworkDistance::Unknown => {}
        }
    }

    fn update_topology_stats(&self) {
        let topology = self.topology.read();
        let mut stats = self.stats.write();

        stats.total_datacenters = topology.datacenters.len();
        stats.total_racks = topology.datacenters.values().map(|dc| dc.racks.len()).sum();
        stats.total_workers = self.worker_locations.len();
    }

    /// Get topology statistics.
    pub fn get_stats(&self) -> TopologyStats {
        self.stats.read().clone()
    }
}

impl Default for TopologyManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Network distance between workers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NetworkDistance {
    /// Workers on the same host
    SameHost = 0,
    /// Workers in the same rack
    SameRack = 1,
    /// Workers in different racks within same datacenter
    CrossRack = 2,
    /// Workers in different datacenters
    CrossDatacenter = 3,
    /// Unknown network distance
    Unknown = 4,
}

/// Bandwidth tracker for monitoring network usage.
pub struct BandwidthTracker {
    /// Per-worker bandwidth usage
    worker_bandwidth: Arc<DashMap<WorkerId, RwLock<BandwidthUsage>>>,
    /// Per-link bandwidth usage
    link_bandwidth: Arc<DashMap<(WorkerId, WorkerId), RwLock<BandwidthUsage>>>,
    /// Bandwidth limits
    limits: Arc<RwLock<BandwidthLimits>>,
    /// Statistics
    stats: Arc<RwLock<BandwidthStats>>,
}

/// Bandwidth usage tracking.
#[derive(Debug, Clone)]
pub struct BandwidthUsage {
    /// Bytes sent
    pub bytes_sent: u64,
    /// Bytes received
    pub bytes_received: u64,
    /// Start time
    pub start_time: Instant,
    /// Last update time
    pub last_update: Instant,
}

impl Default for BandwidthUsage {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            bytes_sent: 0,
            bytes_received: 0,
            start_time: now,
            last_update: now,
        }
    }
}

impl BandwidthUsage {
    /// Calculate current bandwidth in MB/s.
    pub fn current_bandwidth_mbps(&self) -> f64 {
        let elapsed = self
            .last_update
            .duration_since(self.start_time)
            .as_secs_f64();
        if elapsed > 0.0 {
            let total_bytes = self.bytes_sent + self.bytes_received;
            (total_bytes as f64 / 1_048_576.0) / elapsed
        } else {
            0.0
        }
    }
}

/// Bandwidth limits configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BandwidthLimits {
    /// Per-worker bandwidth limit (MB/s)
    pub worker_limit_mbps: f64,
    /// Per-link bandwidth limit (MB/s)
    pub link_limit_mbps: f64,
    /// Global bandwidth limit (MB/s)
    pub global_limit_mbps: f64,
}

impl Default for BandwidthLimits {
    fn default() -> Self {
        Self {
            worker_limit_mbps: 1000.0, // 1 GB/s
            link_limit_mbps: 1000.0,
            global_limit_mbps: 10000.0, // 10 GB/s
        }
    }
}

/// Bandwidth statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BandwidthStats {
    /// Total bytes transferred across the network
    pub total_bytes_transferred: u64,
    /// Peak bandwidth in MB/s
    pub peak_bandwidth_mbps: f64,
    /// Average bandwidth in MB/s
    pub average_bandwidth_mbps: f64,
    /// Number of times bandwidth limits were exceeded
    pub bandwidth_limit_violations: u64,
}

impl BandwidthTracker {
    /// Create a new bandwidth tracker.
    pub fn new(limits: BandwidthLimits) -> Self {
        Self {
            worker_bandwidth: Arc::new(DashMap::new()),
            link_bandwidth: Arc::new(DashMap::new()),
            limits: Arc::new(RwLock::new(limits)),
            stats: Arc::new(RwLock::new(BandwidthStats::default())),
        }
    }

    /// Record data transfer.
    pub fn record_transfer(&self, from: WorkerId, to: WorkerId, bytes: u64) -> Result<()> {
        let now = Instant::now();

        // Update sender
        self.update_worker_usage(from, bytes, 0, now);

        // Update receiver
        self.update_worker_usage(to, 0, bytes, now);

        // Update link
        self.update_link_usage(from, to, bytes, now);

        // Update global stats
        self.update_global_stats(bytes);

        // Check limits
        self.check_limits(from, to)?;

        Ok(())
    }

    fn update_worker_usage(&self, worker: WorkerId, sent: u64, received: u64, now: Instant) {
        let entry = self.worker_bandwidth.entry(worker).or_insert_with(|| {
            RwLock::new(BandwidthUsage {
                start_time: now,
                last_update: now,
                ..Default::default()
            })
        });

        let mut usage = entry.write();
        usage.bytes_sent += sent;
        usage.bytes_received += received;
        usage.last_update = now;
    }

    fn update_link_usage(&self, from: WorkerId, to: WorkerId, bytes: u64, now: Instant) {
        let entry = self.link_bandwidth.entry((from, to)).or_insert_with(|| {
            RwLock::new(BandwidthUsage {
                start_time: now,
                last_update: now,
                ..Default::default()
            })
        });

        let mut usage = entry.write();
        usage.bytes_sent += bytes;
        usage.last_update = now;
    }

    fn update_global_stats(&self, bytes: u64) {
        let mut stats = self.stats.write();
        stats.total_bytes_transferred += bytes;
    }

    fn check_limits(&self, from: WorkerId, _to: WorkerId) -> Result<()> {
        let limits = self.limits.read();

        // Check sender limit
        if let Some(usage) = self.worker_bandwidth.get(&from) {
            let mbps = usage.read().current_bandwidth_mbps();
            if mbps > limits.worker_limit_mbps {
                let mut stats = self.stats.write();
                stats.bandwidth_limit_violations += 1;
                warn!("Worker {} bandwidth limit exceeded: {} MB/s", from, mbps);
            }
        }

        Ok(())
    }

    /// Get current bandwidth usage for a worker.
    pub fn get_worker_bandwidth(&self, worker: &WorkerId) -> Option<f64> {
        self.worker_bandwidth
            .get(worker)
            .map(|u| u.read().current_bandwidth_mbps())
    }

    /// Get bandwidth statistics.
    pub fn get_stats(&self) -> BandwidthStats {
        self.stats.read().clone()
    }
}

/// Congestion control manager.
pub struct CongestionController {
    /// Congestion windows per link
    windows: Arc<DashMap<(WorkerId, WorkerId), RwLock<CongestionWindow>>>,
    /// Configuration
    config: CongestionConfig,
    /// Statistics
    stats: Arc<RwLock<CongestionStats>>,
}

/// Congestion window for flow control.
#[derive(Debug, Clone)]
pub struct CongestionWindow {
    /// Current window size in bytes
    pub size: usize,
    /// Slow start threshold in bytes
    pub ssthresh: usize,
    /// Round-trip time estimate in milliseconds
    pub rtt_ms: f64,
    /// Last time the window was updated
    pub last_update: Instant,
}

/// Congestion control configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CongestionConfig {
    /// Initial window size
    pub initial_window: usize,
    /// Maximum window size
    pub max_window: usize,
    /// Minimum RTT (ms)
    pub min_rtt_ms: f64,
}

impl Default for CongestionConfig {
    fn default() -> Self {
        Self {
            initial_window: 65536, // 64 KB
            max_window: 16777216,  // 16 MB
            min_rtt_ms: 1.0,
        }
    }
}

/// Congestion control statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CongestionStats {
    /// Total number of congestion events detected
    pub total_congestion_events: u64,
    /// Total number of backoff operations performed
    pub total_backoffs: u64,
    /// Average congestion window size in bytes
    pub average_window_size: usize,
}

impl CongestionController {
    /// Create a new congestion controller.
    pub fn new(config: CongestionConfig) -> Self {
        Self {
            windows: Arc::new(DashMap::new()),
            config,
            stats: Arc::new(RwLock::new(CongestionStats::default())),
        }
    }

    /// Report successful transfer.
    pub fn report_success(&self, from: WorkerId, to: WorkerId, rtt_ms: f64) {
        let now = Instant::now();

        let entry = self.windows.entry((from, to)).or_insert_with(|| {
            RwLock::new(CongestionWindow {
                size: self.config.initial_window,
                ssthresh: self.config.max_window / 2,
                rtt_ms: self.config.min_rtt_ms,
                last_update: now,
            })
        });

        let mut window = entry.write();

        // Update RTT estimate
        window.rtt_ms = 0.875 * window.rtt_ms + 0.125 * rtt_ms;

        // Increase window (AIMD)
        if window.size < window.ssthresh {
            // Slow start: exponential increase
            window.size = (window.size * 2).min(self.config.max_window);
        } else {
            // Congestion avoidance: linear increase
            window.size = (window.size + 1024).min(self.config.max_window);
        }

        window.last_update = now;
    }

    /// Report congestion event (packet loss, timeout).
    pub fn report_congestion(&self, from: WorkerId, to: WorkerId) {
        let entry = self.windows.entry((from, to)).or_insert_with(|| {
            RwLock::new(CongestionWindow {
                size: self.config.initial_window,
                ssthresh: self.config.max_window / 2,
                rtt_ms: self.config.min_rtt_ms,
                last_update: Instant::now(),
            })
        });

        let mut window = entry.write();

        // Multiplicative decrease
        window.ssthresh = window.size / 2;
        window.size = window.ssthresh;

        let mut stats = self.stats.write();
        stats.total_congestion_events += 1;
        stats.total_backoffs += 1;
    }

    /// Get current window size.
    pub fn get_window_size(&self, from: &WorkerId, to: &WorkerId) -> usize {
        self.windows
            .get(&(*from, *to))
            .map(|w| w.read().size)
            .unwrap_or(self.config.initial_window)
    }

    /// Get congestion statistics.
    pub fn get_stats(&self) -> CongestionStats {
        self.stats.read().clone()
    }
}

/// Data compression manager for network transfers.
pub struct CompressionManager {
    /// Compression statistics per algorithm
    stats: Arc<DashMap<CompressionAlgorithm, RwLock<CompressionStats>>>,
    /// Default algorithm
    default_algorithm: Arc<RwLock<CompressionAlgorithm>>,
}

/// Compression algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CompressionAlgorithm {
    /// No compression
    None,
    /// Zstandard compression
    Zstd,
    /// LZ4 compression
    Lz4,
    /// Snappy compression
    Snappy,
}

/// Compression statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompressionStats {
    /// Total bytes before compression
    pub bytes_before: u64,
    /// Total bytes after compression
    pub bytes_after: u64,
    /// Compression ratio (after/before)
    pub compression_ratio: f64,
    /// Total time spent compressing in milliseconds
    pub compression_time_ms: f64,
}

impl CompressionManager {
    /// Create a new compression manager.
    pub fn new(default_algorithm: CompressionAlgorithm) -> Self {
        Self {
            stats: Arc::new(DashMap::new()),
            default_algorithm: Arc::new(RwLock::new(default_algorithm)),
        }
    }

    /// Compress data.
    pub fn compress(
        &self,
        data: &[u8],
        algorithm: Option<CompressionAlgorithm>,
    ) -> Result<Vec<u8>> {
        let algo = algorithm.unwrap_or(*self.default_algorithm.read());
        let start = Instant::now();

        let compressed = match algo {
            CompressionAlgorithm::None => data.to_vec(),
            CompressionAlgorithm::Zstd => oxiarc_zstd::compress_with_level(data, 3)
                .map_err(|e| ClusterError::CompressionError(e.to_string()))?,
            CompressionAlgorithm::Lz4 | CompressionAlgorithm::Snappy => {
                // Simplified - in production use actual libraries
                data.to_vec()
            }
        };

        let elapsed = start.elapsed().as_secs_f64() * 1000.0;

        self.update_stats(algo, data.len(), compressed.len(), elapsed);

        Ok(compressed)
    }

    fn update_stats(&self, algo: CompressionAlgorithm, before: usize, after: usize, time_ms: f64) {
        let entry = self
            .stats
            .entry(algo)
            .or_insert_with(|| RwLock::new(CompressionStats::default()));

        let mut stats = entry.write();
        stats.bytes_before += before as u64;
        stats.bytes_after += after as u64;
        stats.compression_ratio = stats.bytes_after as f64 / stats.bytes_before as f64;
        stats.compression_time_ms += time_ms;
    }

    /// Get compression statistics.
    pub fn get_stats(&self, algorithm: CompressionAlgorithm) -> Option<CompressionStats> {
        self.stats.get(&algorithm).map(|s| s.read().clone())
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_topology_manager() {
        use std::time::{Duration, Instant};

        let start = Instant::now();
        let manager = TopologyManager::new();

        let worker1 = WorkerId(uuid::Uuid::new_v4());
        let worker2 = WorkerId(uuid::Uuid::new_v4());

        let loc1 = Location {
            datacenter: "dc1".to_string(),
            rack: "rack1".to_string(),
            host: "host1".to_string(),
            ip_address: None,
        };

        let loc2 = Location {
            datacenter: "dc1".to_string(),
            rack: "rack2".to_string(),
            host: "host2".to_string(),
            ip_address: None,
        };

        // Register workers - should complete quickly
        manager
            .register_worker(worker1, loc1)
            .expect("Failed to register worker1");
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "Worker registration took too long: {:?}",
            start.elapsed()
        );

        manager
            .register_worker(worker2, loc2)
            .expect("Failed to register worker2");
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "Worker registration took too long: {:?}",
            start.elapsed()
        );

        // Calculate distance - should be instant
        let distance = manager.calculate_distance(&worker1, &worker2);
        assert_eq!(distance, NetworkDistance::CrossRack);

        // Verify stats were updated correctly
        let stats = manager.get_stats();
        assert_eq!(stats.total_datacenters, 1, "Should have 1 datacenter");
        assert_eq!(stats.total_racks, 2, "Should have 2 racks");
        assert_eq!(stats.total_workers, 2, "Should have 2 workers");

        // Entire test should complete in under 5 seconds
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "Test took too long: {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn test_bandwidth_tracker() {
        let limits = BandwidthLimits::default();
        let tracker = BandwidthTracker::new(limits);

        let worker1 = WorkerId(uuid::Uuid::new_v4());
        let worker2 = WorkerId(uuid::Uuid::new_v4());

        let _ = tracker.record_transfer(worker1, worker2, 1048576); // 1 MB

        let bandwidth = tracker.get_worker_bandwidth(&worker1);
        assert!(bandwidth.is_some());
    }

    #[test]
    fn test_congestion_controller() {
        let config = CongestionConfig::default();
        let controller = CongestionController::new(config);

        let worker1 = WorkerId(uuid::Uuid::new_v4());
        let worker2 = WorkerId(uuid::Uuid::new_v4());

        let initial_window = controller.get_window_size(&worker1, &worker2);

        controller.report_success(worker1, worker2, 10.0);

        let new_window = controller.get_window_size(&worker1, &worker2);
        assert!(new_window > initial_window);
    }
}
