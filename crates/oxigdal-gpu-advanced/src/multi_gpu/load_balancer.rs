//! Load balancing and workload migration across multiple GPUs.
//!
//! This module provides advanced load balancing capabilities including:
//! - GPU utilization monitoring
//! - Workload migration between devices
//! - Data transfer cost estimation
//! - Multiple load balancing strategies

use super::{GpuDevice, SelectionStrategy};
use crate::error::{GpuAdvancedError, Result};
use parking_lot::RwLock;
use std::cmp::Ordering;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering as AtomicOrdering};
use std::time::{Duration, Instant};

/// Load balancer for distributing work across GPUs
pub struct LoadBalancer {
    /// Available devices
    devices: Vec<Arc<GpuDevice>>,
    /// Selection strategy
    strategy: SelectionStrategy,
    /// Round-robin counter
    rr_counter: AtomicUsize,
    /// Load statistics
    stats: Arc<RwLock<LoadStats>>,
    /// Migration configuration
    migration_config: Arc<RwLock<MigrationConfig>>,
    /// Migration history for adaptive decisions
    migration_history: Arc<RwLock<MigrationHistory>>,
    /// Workload tracker per device
    workload_tracker: Arc<RwLock<WorkloadTracker>>,
}

/// Load balancing statistics
#[derive(Debug, Clone, Default)]
pub struct LoadStats {
    /// Total tasks assigned per device
    pub tasks_per_device: Vec<usize>,
    /// Total execution time per device (microseconds)
    pub time_per_device: Vec<u64>,
    /// Current active tasks per device
    pub active_tasks: Vec<usize>,
    /// Memory usage per device (bytes)
    pub memory_per_device: Vec<u64>,
    /// Migration count per device (as source)
    pub migrations_from: Vec<usize>,
    /// Migration count per device (as destination)
    pub migrations_to: Vec<usize>,
}

/// Configuration for workload migration decisions
#[derive(Debug, Clone)]
pub struct MigrationConfig {
    /// Utilization threshold above which a GPU is considered overloaded (0.0 to 1.0)
    pub overload_threshold: f32,
    /// Utilization threshold below which a GPU is considered underutilized (0.0 to 1.0)
    pub underutilization_threshold: f32,
    /// Minimum utilization difference to trigger migration
    pub min_imbalance_threshold: f32,
    /// Base cost for data transfer (in arbitrary units representing time)
    pub transfer_cost_base: f64,
    /// Cost per byte transferred (in arbitrary units)
    pub transfer_cost_per_byte: f64,
    /// Minimum workload size to consider for migration (bytes)
    pub min_migration_size: u64,
    /// Maximum pending migrations per device
    pub max_pending_migrations: usize,
    /// Cooldown period between migrations for same device (seconds)
    pub migration_cooldown_secs: u64,
    /// Whether to enable predictive migration based on trends
    pub enable_predictive_migration: bool,
    /// History window size for trend analysis
    pub history_window_size: usize,
    /// Weight for memory pressure in migration decisions (0.0 to 1.0)
    pub memory_weight: f32,
    /// Weight for compute utilization in migration decisions (0.0 to 1.0)
    pub compute_weight: f32,
}

impl Default for MigrationConfig {
    fn default() -> Self {
        Self {
            overload_threshold: 0.8,
            underutilization_threshold: 0.3,
            min_imbalance_threshold: 0.2,
            transfer_cost_base: 1.0,
            transfer_cost_per_byte: 0.000001, // 1 microsecond per megabyte
            min_migration_size: 1024,         // 1 KB minimum
            max_pending_migrations: 4,
            migration_cooldown_secs: 5,
            enable_predictive_migration: true,
            history_window_size: 100,
            memory_weight: 0.4,
            compute_weight: 0.6,
        }
    }
}

/// Represents a migratable workload
#[derive(Debug, Clone)]
pub struct MigratableWorkload {
    /// Unique identifier for the workload
    pub id: u64,
    /// Source device index
    pub source_device: usize,
    /// Estimated memory footprint in bytes
    pub memory_size: u64,
    /// Estimated compute intensity (0.0 to 1.0)
    pub compute_intensity: f32,
    /// Priority level (higher = more important)
    pub priority: u32,
    /// Creation timestamp
    pub created_at: Instant,
    /// Whether this workload is currently being migrated
    pub migrating: bool,
    /// Data dependencies (other workload IDs this depends on)
    pub dependencies: Vec<u64>,
}

impl MigratableWorkload {
    /// Create a new migratable workload
    pub fn new(
        id: u64,
        source_device: usize,
        memory_size: u64,
        compute_intensity: f32,
        priority: u32,
    ) -> Self {
        Self {
            id,
            source_device,
            memory_size,
            compute_intensity,
            priority,
            created_at: Instant::now(),
            migrating: false,
            dependencies: Vec::new(),
        }
    }

    /// Add a dependency to this workload
    pub fn with_dependency(mut self, dep_id: u64) -> Self {
        self.dependencies.push(dep_id);
        self
    }

    /// Calculate migration cost based on configuration
    pub fn calculate_migration_cost(&self, config: &MigrationConfig) -> f64 {
        config.transfer_cost_base
            + (self.memory_size as f64 * config.transfer_cost_per_byte)
            + (self.compute_intensity as f64 * 0.1) // Compute intensity penalty
    }
}

/// A planned migration operation
#[derive(Debug, Clone)]
pub struct MigrationPlan {
    /// The workload to migrate
    pub workload: MigratableWorkload,
    /// Source device index
    pub source_device: usize,
    /// Destination device index
    pub target_device: usize,
    /// Estimated transfer cost
    pub estimated_cost: f64,
    /// Expected benefit (load reduction on source)
    pub expected_benefit: f64,
    /// Net benefit (benefit - cost)
    pub net_benefit: f64,
    /// Plan creation timestamp
    pub created_at: Instant,
    /// Whether the plan is approved for execution
    pub approved: bool,
}

impl MigrationPlan {
    /// Create a new migration plan
    pub fn new(
        workload: MigratableWorkload,
        target_device: usize,
        config: &MigrationConfig,
    ) -> Self {
        let source_device = workload.source_device;
        let estimated_cost = workload.calculate_migration_cost(config);
        let expected_benefit = workload.compute_intensity as f64 * 10.0; // Arbitrary benefit scale
        let net_benefit = expected_benefit - estimated_cost;

        Self {
            workload,
            source_device,
            target_device,
            estimated_cost,
            expected_benefit,
            net_benefit,
            created_at: Instant::now(),
            approved: net_benefit > 0.0,
        }
    }

    /// Check if migration should proceed
    pub fn should_migrate(&self) -> bool {
        self.approved && self.net_benefit > 0.0
    }
}

/// Result of a migration operation
#[derive(Debug, Clone)]
pub struct MigrationResult {
    /// Whether the migration succeeded
    pub success: bool,
    /// Source device index
    pub source_device: usize,
    /// Target device index
    pub target_device: usize,
    /// Workload ID that was migrated
    pub workload_id: u64,
    /// Actual transfer time
    pub transfer_time: Duration,
    /// Bytes transferred
    pub bytes_transferred: u64,
    /// Error message if failed
    pub error_message: Option<String>,
}

/// History of migrations for adaptive decisions
#[derive(Debug, Default)]
pub struct MigrationHistory {
    /// Recent migration results
    entries: VecDeque<MigrationHistoryEntry>,
    /// Maximum history size
    max_size: usize,
    /// Total successful migrations
    total_successful: usize,
    /// Total failed migrations
    total_failed: usize,
}

/// Single entry in migration history
#[derive(Debug, Clone)]
pub struct MigrationHistoryEntry {
    /// Timestamp of the migration
    pub timestamp: Instant,
    /// Source device
    pub source_device: usize,
    /// Target device
    pub target_device: usize,
    /// Whether it succeeded
    pub success: bool,
    /// Transfer time
    pub transfer_time: Duration,
    /// Bytes transferred
    pub bytes_transferred: u64,
}

impl MigrationHistory {
    /// Create a new migration history
    pub fn new(max_size: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(max_size),
            max_size,
            total_successful: 0,
            total_failed: 0,
        }
    }

    /// Add an entry to the history
    pub fn add_entry(&mut self, entry: MigrationHistoryEntry) {
        if entry.success {
            self.total_successful += 1;
        } else {
            self.total_failed += 1;
        }

        if self.entries.len() >= self.max_size {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    /// Get success rate for migrations between specific devices
    pub fn success_rate(&self, source: usize, target: usize) -> f64 {
        let filtered: Vec<_> = self
            .entries
            .iter()
            .filter(|e| e.source_device == source && e.target_device == target)
            .collect();

        if filtered.is_empty() {
            return 1.0; // Assume success if no history
        }

        let successful = filtered.iter().filter(|e| e.success).count();
        successful as f64 / filtered.len() as f64
    }

    /// Get average transfer time for migrations between devices
    pub fn average_transfer_time(&self, source: usize, target: usize) -> Option<Duration> {
        let filtered: Vec<_> = self
            .entries
            .iter()
            .filter(|e| e.source_device == source && e.target_device == target && e.success)
            .collect();

        if filtered.is_empty() {
            return None;
        }

        let total: Duration = filtered.iter().map(|e| e.transfer_time).sum();
        Some(total / filtered.len() as u32)
    }

    /// Get total bytes transferred
    pub fn total_bytes_transferred(&self) -> u64 {
        self.entries.iter().map(|e| e.bytes_transferred).sum()
    }

    /// Get overall success rate
    pub fn overall_success_rate(&self) -> f64 {
        let total = self.total_successful + self.total_failed;
        if total == 0 {
            return 1.0;
        }
        self.total_successful as f64 / total as f64
    }
}

/// Tracks workload distribution over time
#[derive(Debug)]
pub struct WorkloadTracker {
    /// Per-device utilization samples
    utilization_samples: Vec<VecDeque<UtilizationSample>>,
    /// Per-device pending workloads
    pending_workloads: Vec<Vec<MigratableWorkload>>,
    /// Global workload counter
    next_workload_id: AtomicU64,
    /// Last rebalance timestamp per device (None if never rebalanced)
    last_rebalance: Vec<Option<Instant>>,
}

/// A single utilization sample
#[derive(Debug, Clone)]
pub struct UtilizationSample {
    /// Sample timestamp
    pub timestamp: Instant,
    /// Compute utilization (0.0 to 1.0)
    pub compute: f32,
    /// Memory utilization (0.0 to 1.0)
    pub memory: f32,
    /// Active task count
    pub active_tasks: usize,
}

impl WorkloadTracker {
    /// Create a new workload tracker for N devices
    pub fn new(device_count: usize, history_size: usize) -> Self {
        let mut utilization_samples = Vec::with_capacity(device_count);
        let mut pending_workloads = Vec::with_capacity(device_count);
        let mut last_rebalance = Vec::with_capacity(device_count);

        for _ in 0..device_count {
            utilization_samples.push(VecDeque::with_capacity(history_size));
            pending_workloads.push(Vec::new());
            // Initialize to None - devices start without cooldown since no rebalancing has happened
            last_rebalance.push(None);
        }

        Self {
            utilization_samples,
            pending_workloads,
            next_workload_id: AtomicU64::new(0),
            last_rebalance,
        }
    }

    /// Generate a new workload ID
    pub fn next_workload_id(&self) -> u64 {
        self.next_workload_id.fetch_add(1, AtomicOrdering::Relaxed)
    }

    /// Record a utilization sample for a device
    pub fn record_sample(&mut self, device_index: usize, sample: UtilizationSample) {
        if let Some(samples) = self.utilization_samples.get_mut(device_index) {
            if samples.len() >= samples.capacity() {
                samples.pop_front();
            }
            samples.push_back(sample);
        }
    }

    /// Get average utilization for a device over recent samples
    pub fn average_utilization(&self, device_index: usize, window: usize) -> Option<(f32, f32)> {
        let samples = self.utilization_samples.get(device_index)?;
        if samples.is_empty() {
            return None;
        }

        let take_count = window.min(samples.len());
        let recent: Vec<_> = samples.iter().rev().take(take_count).collect();

        let avg_compute = recent.iter().map(|s| s.compute).sum::<f32>() / take_count as f32;
        let avg_memory = recent.iter().map(|s| s.memory).sum::<f32>() / take_count as f32;

        Some((avg_compute, avg_memory))
    }

    /// Get utilization trend (positive = increasing, negative = decreasing)
    pub fn utilization_trend(&self, device_index: usize, window: usize) -> Option<f32> {
        let samples = self.utilization_samples.get(device_index)?;
        if samples.len() < 2 {
            return None;
        }

        let take_count = window.min(samples.len());
        // Get recent samples in chronological order (oldest first, newest last)
        // Skip older samples and take the most recent ones
        let skip_count = samples.len().saturating_sub(take_count);
        let recent: Vec<_> = samples.iter().skip(skip_count).collect();

        if recent.len() < 2 {
            return None;
        }

        // Simple linear regression slope
        // x = 0 is oldest, x = n-1 is newest
        // Positive slope means utilization is increasing over time
        let n = recent.len() as f32;
        let mut sum_x = 0.0f32;
        let mut sum_y = 0.0f32;
        let mut sum_xy = 0.0f32;
        let mut sum_xx = 0.0f32;

        for (i, sample) in recent.iter().enumerate() {
            let x = i as f32;
            let y = sample.compute;
            sum_x += x;
            sum_y += y;
            sum_xy += x * y;
            sum_xx += x * x;
        }

        let denominator = n * sum_xx - sum_x * sum_x;
        if denominator.abs() < f32::EPSILON {
            return Some(0.0);
        }

        Some((n * sum_xy - sum_x * sum_y) / denominator)
    }

    /// Add a pending workload to a device
    pub fn add_workload(&mut self, device_index: usize, workload: MigratableWorkload) {
        if let Some(workloads) = self.pending_workloads.get_mut(device_index) {
            workloads.push(workload);
        }
    }

    /// Remove a workload by ID
    pub fn remove_workload(
        &mut self,
        device_index: usize,
        workload_id: u64,
    ) -> Option<MigratableWorkload> {
        if let Some(workloads) = self.pending_workloads.get_mut(device_index) {
            if let Some(pos) = workloads.iter().position(|w| w.id == workload_id) {
                return Some(workloads.remove(pos));
            }
        }
        None
    }

    /// Get migratable workloads from a device (not already migrating, no pending dependencies)
    pub fn get_migratable_workloads(&self, device_index: usize) -> Vec<&MigratableWorkload> {
        self.pending_workloads
            .get(device_index)
            .map(|workloads| {
                workloads
                    .iter()
                    .filter(|w| !w.migrating && w.dependencies.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Update last rebalance time for a device
    pub fn update_rebalance_time(&mut self, device_index: usize) {
        if let Some(time) = self.last_rebalance.get_mut(device_index) {
            *time = Some(Instant::now());
        }
    }

    /// Check if device is in cooldown period
    ///
    /// Returns false if no rebalancing has ever happened on this device.
    pub fn is_in_cooldown(&self, device_index: usize, cooldown_secs: u64) -> bool {
        self.last_rebalance
            .get(device_index)
            .and_then(|opt| opt.as_ref())
            .map(|t| t.elapsed().as_secs() < cooldown_secs)
            .unwrap_or(false)
    }

    /// Get pending workload count for a device
    pub fn pending_count(&self, device_index: usize) -> usize {
        self.pending_workloads
            .get(device_index)
            .map(|w| w.len())
            .unwrap_or(0)
    }
}

/// Device load information for balancing decisions
#[derive(Debug, Clone)]
pub struct DeviceLoad {
    /// Device index
    pub device_index: usize,
    /// Current compute utilization (0.0 to 1.0)
    pub compute_utilization: f32,
    /// Current memory utilization (0.0 to 1.0)
    pub memory_utilization: f32,
    /// Combined load score
    pub combined_load: f32,
    /// Active task count
    pub active_tasks: usize,
    /// Pending workload count
    pub pending_workloads: usize,
    /// Device score (higher = better for new work)
    pub score: f32,
    /// Utilization trend (positive = increasing)
    pub trend: f32,
}

impl DeviceLoad {
    /// Check if device is overloaded
    pub fn is_overloaded(&self, config: &MigrationConfig) -> bool {
        self.combined_load > config.overload_threshold
    }

    /// Check if device is underutilized
    pub fn is_underutilized(&self, config: &MigrationConfig) -> bool {
        self.combined_load < config.underutilization_threshold
    }
}

impl LoadBalancer {
    /// Create a new load balancer
    pub fn new(devices: Vec<Arc<GpuDevice>>, strategy: SelectionStrategy) -> Self {
        let device_count = devices.len();
        let stats = LoadStats {
            tasks_per_device: vec![0; device_count],
            time_per_device: vec![0; device_count],
            active_tasks: vec![0; device_count],
            memory_per_device: vec![0; device_count],
            migrations_from: vec![0; device_count],
            migrations_to: vec![0; device_count],
        };

        let config = MigrationConfig::default();
        let tracker = WorkloadTracker::new(device_count, config.history_window_size);
        let history = MigrationHistory::new(config.history_window_size);

        Self {
            devices,
            strategy,
            rr_counter: AtomicUsize::new(0),
            stats: Arc::new(RwLock::new(stats)),
            migration_config: Arc::new(RwLock::new(config)),
            migration_history: Arc::new(RwLock::new(history)),
            workload_tracker: Arc::new(RwLock::new(tracker)),
        }
    }

    /// Get the migration configuration
    pub fn migration_config(&self) -> MigrationConfig {
        self.migration_config.read().clone()
    }

    /// Update migration configuration
    pub fn set_migration_config(&self, config: MigrationConfig) {
        *self.migration_config.write() = config;
    }

    /// Select a device using the configured strategy
    pub fn select_device(&self) -> Result<Arc<GpuDevice>> {
        if self.devices.is_empty() {
            return Err(GpuAdvancedError::GpuNotFound(
                "No devices available".to_string(),
            ));
        }

        match self.strategy {
            SelectionStrategy::RoundRobin => self.select_round_robin(),
            SelectionStrategy::LeastLoaded => self.select_least_loaded(),
            SelectionStrategy::BestScore => self.select_best_score(),
            SelectionStrategy::Affinity => self.select_affinity(),
        }
    }

    /// Round-robin selection
    fn select_round_robin(&self) -> Result<Arc<GpuDevice>> {
        let index = self.rr_counter.fetch_add(1, AtomicOrdering::Relaxed) % self.devices.len();
        self.devices
            .get(index)
            .cloned()
            .ok_or(GpuAdvancedError::InvalidGpuIndex {
                index,
                total: self.devices.len(),
            })
    }

    /// Select least loaded device
    fn select_least_loaded(&self) -> Result<Arc<GpuDevice>> {
        let stats = self.stats.read();

        let (index, _) = self
            .devices
            .iter()
            .enumerate()
            .map(|(i, device)| {
                let active_tasks = stats.active_tasks.get(i).copied().unwrap_or(0);
                let workload = device.get_workload();
                let load = (active_tasks as f32) + workload;
                (i, load)
            })
            .min_by(|(_, load_a), (_, load_b)| {
                load_a.partial_cmp(load_b).unwrap_or(Ordering::Equal)
            })
            .ok_or_else(|| {
                GpuAdvancedError::LoadBalancingError("No device available".to_string())
            })?;

        self.devices
            .get(index)
            .cloned()
            .ok_or(GpuAdvancedError::InvalidGpuIndex {
                index,
                total: self.devices.len(),
            })
    }

    /// Select device with best score
    fn select_best_score(&self) -> Result<Arc<GpuDevice>> {
        let (index, _) = self
            .devices
            .iter()
            .enumerate()
            .map(|(i, device)| (i, device.get_score()))
            .max_by(|(_, score_a), (_, score_b)| {
                score_a.partial_cmp(score_b).unwrap_or(Ordering::Equal)
            })
            .ok_or_else(|| {
                GpuAdvancedError::LoadBalancingError("No device available".to_string())
            })?;

        self.devices
            .get(index)
            .cloned()
            .ok_or(GpuAdvancedError::InvalidGpuIndex {
                index,
                total: self.devices.len(),
            })
    }

    /// Select device using affinity (prefers previously used device)
    fn select_affinity(&self) -> Result<Arc<GpuDevice>> {
        // For now, use thread-local affinity based on thread ID
        let thread_id = std::thread::current().id();
        let hash = {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            thread_id.hash(&mut hasher);
            hasher.finish()
        };

        let index = (hash as usize) % self.devices.len();
        self.devices
            .get(index)
            .cloned()
            .ok_or(GpuAdvancedError::InvalidGpuIndex {
                index,
                total: self.devices.len(),
            })
    }

    /// Select device using weighted strategy based on device performance
    pub fn select_weighted(&self) -> Result<Arc<GpuDevice>> {
        if self.devices.is_empty() {
            return Err(GpuAdvancedError::GpuNotFound(
                "No devices available".to_string(),
            ));
        }

        let config = self.migration_config.read();

        // Calculate weighted scores for each device
        let mut best_index = 0;
        let mut best_score = f32::MIN;

        for (i, device) in self.devices.iter().enumerate() {
            let compute_util = device.get_workload();
            let memory_usage = device.get_memory_usage();
            let max_memory = device.info.max_buffer_size;
            let memory_util = if max_memory > 0 {
                memory_usage as f32 / max_memory as f32
            } else {
                0.0
            };

            // Weighted combination of factors
            let availability =
                1.0 - (compute_util * config.compute_weight + memory_util * config.memory_weight);
            let type_bonus = device.get_score();
            let score = availability * type_bonus;

            if score > best_score {
                best_score = score;
                best_index = i;
            }
        }

        self.devices
            .get(best_index)
            .cloned()
            .ok_or(GpuAdvancedError::InvalidGpuIndex {
                index: best_index,
                total: self.devices.len(),
            })
    }

    /// Get current load information for all devices
    pub fn get_device_loads(&self) -> Vec<DeviceLoad> {
        let config = self.migration_config.read();
        let tracker = self.workload_tracker.read();
        let stats = self.stats.read();

        self.devices
            .iter()
            .enumerate()
            .map(|(i, device)| {
                let compute_utilization = device.get_workload();
                let memory_usage = device.get_memory_usage();
                let max_memory = device.info.max_buffer_size;
                let memory_utilization = if max_memory > 0 {
                    memory_usage as f32 / max_memory as f32
                } else {
                    0.0
                };

                let combined_load = compute_utilization * config.compute_weight
                    + memory_utilization * config.memory_weight;

                let trend = tracker.utilization_trend(i, 10).unwrap_or(0.0);

                DeviceLoad {
                    device_index: i,
                    compute_utilization,
                    memory_utilization,
                    combined_load,
                    active_tasks: stats.active_tasks.get(i).copied().unwrap_or(0),
                    pending_workloads: tracker.pending_count(i),
                    score: device.get_score(),
                    trend,
                }
            })
            .collect()
    }

    /// Identify overloaded devices
    pub fn identify_overloaded_devices(&self) -> Vec<DeviceLoad> {
        let config = self.migration_config.read();
        self.get_device_loads()
            .into_iter()
            .filter(|load| load.is_overloaded(&config))
            .collect()
    }

    /// Identify underutilized devices
    pub fn identify_underutilized_devices(&self) -> Vec<DeviceLoad> {
        let config = self.migration_config.read();
        self.get_device_loads()
            .into_iter()
            .filter(|load| load.is_underutilized(&config))
            .collect()
    }

    /// Check if load is imbalanced (requires rebalancing)
    pub fn is_imbalanced(&self) -> bool {
        let loads = self.get_device_loads();
        if loads.len() < 2 {
            return false;
        }

        let config = self.migration_config.read();

        // Find max and min load
        let max_load = loads
            .iter()
            .map(|l| l.combined_load)
            .fold(f32::MIN, f32::max);
        let min_load = loads
            .iter()
            .map(|l| l.combined_load)
            .fold(f32::MAX, f32::min);

        (max_load - min_load) > config.min_imbalance_threshold
    }

    /// Calculate data transfer cost between two devices
    pub fn calculate_transfer_cost(
        &self,
        source_device: usize,
        target_device: usize,
        data_size: u64,
    ) -> Result<f64> {
        if source_device >= self.devices.len() || target_device >= self.devices.len() {
            return Err(GpuAdvancedError::InvalidGpuIndex {
                index: source_device.max(target_device),
                total: self.devices.len(),
            });
        }

        let config = self.migration_config.read();
        let history = self.migration_history.read();

        // Base cost from configuration
        let mut cost =
            config.transfer_cost_base + (data_size as f64 * config.transfer_cost_per_byte);

        // Adjust based on historical transfer times
        if let Some(avg_time) = history.average_transfer_time(source_device, target_device) {
            // Scale cost based on historical performance
            let time_factor = avg_time.as_secs_f64();
            cost *= 1.0 + time_factor;
        }

        // Adjust for historical success rate
        let success_rate = history.success_rate(source_device, target_device);
        if success_rate < 1.0 {
            // Increase cost for unreliable transfers
            cost *= 1.0 + (1.0 - success_rate) * 0.5;
        }

        Ok(cost)
    }

    /// Create a migration plan for a workload
    pub fn create_migration_plan(
        &self,
        workload: MigratableWorkload,
        target_device: usize,
    ) -> Result<MigrationPlan> {
        if target_device >= self.devices.len() {
            return Err(GpuAdvancedError::InvalidGpuIndex {
                index: target_device,
                total: self.devices.len(),
            });
        }

        let config = self.migration_config.read();
        let plan = MigrationPlan::new(workload, target_device, &config);

        Ok(plan)
    }

    /// Find best migration target for an overloaded device
    pub fn find_migration_target(&self, source_device: usize) -> Result<Option<usize>> {
        let loads = self.get_device_loads();
        let config = self.migration_config.read();
        let tracker = self.workload_tracker.read();

        // Find the source load
        let source_load = loads
            .iter()
            .find(|l| l.device_index == source_device)
            .ok_or(GpuAdvancedError::InvalidGpuIndex {
                index: source_device,
                total: self.devices.len(),
            })?;

        // Find candidate targets (underutilized devices not in cooldown)
        let mut candidates: Vec<_> = loads
            .iter()
            .filter(|l| {
                l.device_index != source_device
                    && l.is_underutilized(&config)
                    && !tracker.is_in_cooldown(l.device_index, config.migration_cooldown_secs)
            })
            .collect();

        if candidates.is_empty() {
            return Ok(None);
        }

        // Sort by combined load (ascending) and score (descending)
        candidates.sort_by(|a, b| match a.combined_load.partial_cmp(&b.combined_load) {
            Some(Ordering::Equal) | None => {
                b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal)
            }
            Some(ordering) => ordering,
        });

        // Return the best candidate if migration would improve balance
        if let Some(best) = candidates.first() {
            let load_diff = source_load.combined_load - best.combined_load;
            if load_diff > config.min_imbalance_threshold {
                return Ok(Some(best.device_index));
            }
        }

        Ok(None)
    }

    /// Select workload to migrate from an overloaded device
    pub fn select_workload_for_migration(
        &self,
        source_device: usize,
    ) -> Option<MigratableWorkload> {
        let config = self.migration_config.read();
        let tracker = self.workload_tracker.read();

        let migratable = tracker.get_migratable_workloads(source_device);

        // Filter by minimum size and sort by priority and compute intensity
        let mut candidates: Vec<_> = migratable
            .into_iter()
            .filter(|w| w.memory_size >= config.min_migration_size)
            .collect();

        candidates.sort_by(|a, b| {
            // Prefer higher priority and higher compute intensity
            match b.priority.cmp(&a.priority) {
                Ordering::Equal => b
                    .compute_intensity
                    .partial_cmp(&a.compute_intensity)
                    .unwrap_or(Ordering::Equal),
                other => other,
            }
        });

        candidates.first().map(|w| (*w).clone())
    }

    /// Execute a migration (simulated - actual data transfer would use sync module)
    pub fn execute_migration(&self, plan: &MigrationPlan) -> Result<MigrationResult> {
        if !plan.should_migrate() {
            return Ok(MigrationResult {
                success: false,
                source_device: plan.source_device,
                target_device: plan.target_device,
                workload_id: plan.workload.id,
                transfer_time: Duration::ZERO,
                bytes_transferred: 0,
                error_message: Some("Migration not approved".to_string()),
            });
        }

        let start = Instant::now();

        // Update workload tracker
        {
            let mut tracker = self.workload_tracker.write();

            // Remove from source
            if tracker
                .remove_workload(plan.source_device, plan.workload.id)
                .is_none()
            {
                return Ok(MigrationResult {
                    success: false,
                    source_device: plan.source_device,
                    target_device: plan.target_device,
                    workload_id: plan.workload.id,
                    transfer_time: Duration::ZERO,
                    bytes_transferred: 0,
                    error_message: Some("Workload not found on source device".to_string()),
                });
            }

            // Add to target with updated source
            let mut migrated = plan.workload.clone();
            migrated.source_device = plan.target_device;
            tracker.add_workload(plan.target_device, migrated);

            // Update rebalance times
            tracker.update_rebalance_time(plan.source_device);
            tracker.update_rebalance_time(plan.target_device);
        }

        // Update statistics
        {
            let mut stats = self.stats.write();
            if let Some(from) = stats.migrations_from.get_mut(plan.source_device) {
                *from = from.saturating_add(1);
            }
            if let Some(to) = stats.migrations_to.get_mut(plan.target_device) {
                *to = to.saturating_add(1);
            }
        }

        let transfer_time = start.elapsed();

        // Record in history
        {
            let mut history = self.migration_history.write();
            history.add_entry(MigrationHistoryEntry {
                timestamp: Instant::now(),
                source_device: plan.source_device,
                target_device: plan.target_device,
                success: true,
                transfer_time,
                bytes_transferred: plan.workload.memory_size,
            });
        }

        Ok(MigrationResult {
            success: true,
            source_device: plan.source_device,
            target_device: plan.target_device,
            workload_id: plan.workload.id,
            transfer_time,
            bytes_transferred: plan.workload.memory_size,
            error_message: None,
        })
    }

    /// Rebalance workloads across devices
    ///
    /// This method implements the core workload migration logic:
    /// 1. Monitor GPU utilization across all devices
    /// 2. Identify overloaded and underutilized GPUs
    /// 3. Calculate transfer costs for potential migrations
    /// 4. Execute migrations that improve overall balance
    pub fn rebalance(&self) -> Result<Vec<MigrationResult>> {
        // Check if rebalancing is needed
        if !self.is_imbalanced() {
            return Ok(Vec::new());
        }

        let mut results = Vec::new();

        // Sample current utilization for all devices
        self.sample_utilization();

        // Identify overloaded devices
        let overloaded = self.identify_overloaded_devices();
        if overloaded.is_empty() {
            return Ok(results);
        }

        // Process each overloaded device
        for source_load in overloaded {
            // Check migration limit
            if results.len() >= self.migration_config.read().max_pending_migrations {
                break;
            }

            // Find a suitable target
            let target = match self.find_migration_target(source_load.device_index)? {
                Some(t) => t,
                None => continue,
            };

            // Select workload to migrate
            let workload = match self.select_workload_for_migration(source_load.device_index) {
                Some(w) => w,
                None => continue,
            };

            // Create and execute migration plan
            let plan = self.create_migration_plan(workload, target)?;
            if plan.should_migrate() {
                let result = self.execute_migration(&plan)?;
                results.push(result);
            }
        }

        // Handle predictive migration if enabled
        let config = self.migration_config.read();
        if config.enable_predictive_migration {
            drop(config);
            self.handle_predictive_migrations(&mut results)?;
        }

        Ok(results)
    }

    /// Sample current utilization for all devices
    fn sample_utilization(&self) {
        let stats = self.stats.read();
        let mut tracker = self.workload_tracker.write();

        for (i, device) in self.devices.iter().enumerate() {
            let compute = device.get_workload();
            let memory_usage = device.get_memory_usage();
            let max_memory = device.info.max_buffer_size;
            let memory = if max_memory > 0 {
                memory_usage as f32 / max_memory as f32
            } else {
                0.0
            };

            tracker.record_sample(
                i,
                UtilizationSample {
                    timestamp: Instant::now(),
                    compute,
                    memory,
                    active_tasks: stats.active_tasks.get(i).copied().unwrap_or(0),
                },
            );
        }
    }

    /// Handle predictive migrations based on utilization trends
    fn handle_predictive_migrations(&self, results: &mut Vec<MigrationResult>) -> Result<()> {
        // Collect device indices that need predictive migration
        let candidates: Vec<(usize, f32, f32)> = {
            let config = self.migration_config.read();
            let tracker = self.workload_tracker.read();
            let device_loads = self.get_device_loads();

            self.devices
                .iter()
                .enumerate()
                .filter_map(|(i, _device)| {
                    // Check utilization trend
                    let trend = tracker.utilization_trend(i, 20)?;

                    // Find load for this device
                    let load = device_loads.iter().find(|l| l.device_index == i)?;

                    // If trend is strongly increasing and device is moderately loaded
                    if trend > 0.05
                        && load.combined_load > 0.5
                        && load.combined_load < config.overload_threshold
                    {
                        Some((i, trend, load.combined_load))
                    } else {
                        None
                    }
                })
                .collect()
        }; // Locks are released here

        // Process candidates outside the lock
        let max_migrations = self.migration_config.read().max_pending_migrations;
        for (device_index, _trend, _combined_load) in candidates {
            // Check if we've hit the migration limit
            if results.len() >= max_migrations {
                break;
            }

            // Preemptively migrate to prevent overload
            if let Some(target) = self.find_migration_target(device_index)? {
                if let Some(workload) = self.select_workload_for_migration(device_index) {
                    let plan = self.create_migration_plan(workload, target)?;
                    if plan.should_migrate() {
                        let result = self.execute_migration(&plan)?;
                        results.push(result);
                    }
                }
            }
        }

        Ok(())
    }

    /// Register a new workload on a device
    pub fn register_workload(
        &self,
        device_index: usize,
        memory_size: u64,
        compute_intensity: f32,
        priority: u32,
    ) -> Result<u64> {
        if device_index >= self.devices.len() {
            return Err(GpuAdvancedError::InvalidGpuIndex {
                index: device_index,
                total: self.devices.len(),
            });
        }

        let mut tracker = self.workload_tracker.write();
        let workload_id = tracker.next_workload_id();

        let workload = MigratableWorkload::new(
            workload_id,
            device_index,
            memory_size,
            compute_intensity,
            priority,
        );

        tracker.add_workload(device_index, workload);

        Ok(workload_id)
    }

    /// Unregister a workload (completed or cancelled)
    pub fn unregister_workload(&self, device_index: usize, workload_id: u64) -> Result<()> {
        if device_index >= self.devices.len() {
            return Err(GpuAdvancedError::InvalidGpuIndex {
                index: device_index,
                total: self.devices.len(),
            });
        }

        let mut tracker = self.workload_tracker.write();
        tracker.remove_workload(device_index, workload_id);

        Ok(())
    }

    /// Mark task started on device
    pub fn task_started(&self, device_index: usize) {
        let mut stats = self.stats.write();
        if let Some(count) = stats.tasks_per_device.get_mut(device_index) {
            *count = count.saturating_add(1);
        }
        if let Some(active) = stats.active_tasks.get_mut(device_index) {
            *active = active.saturating_add(1);
        }
    }

    /// Mark task completed on device
    pub fn task_completed(&self, device_index: usize, duration_us: u64) {
        let mut stats = self.stats.write();
        if let Some(active) = stats.active_tasks.get_mut(device_index) {
            *active = active.saturating_sub(1);
        }
        if let Some(time) = stats.time_per_device.get_mut(device_index) {
            *time = time.saturating_add(duration_us);
        }
    }

    /// Get load statistics
    pub fn get_stats(&self) -> LoadStats {
        self.stats.read().clone()
    }

    /// Print load statistics
    pub fn print_stats(&self) {
        let stats = self.stats.read();
        println!("\nLoad Balancer Statistics:");
        println!("  Strategy: {:?}", self.strategy);

        for (i, device) in self.devices.iter().enumerate() {
            let tasks = stats.tasks_per_device.get(i).copied().unwrap_or(0);
            let time_us = stats.time_per_device.get(i).copied().unwrap_or(0);
            let active = stats.active_tasks.get(i).copied().unwrap_or(0);
            let avg_time_us = if tasks > 0 {
                time_us / (tasks as u64)
            } else {
                0
            };

            let migrations_from = stats.migrations_from.get(i).copied().unwrap_or(0);
            let migrations_to = stats.migrations_to.get(i).copied().unwrap_or(0);

            println!("\n  GPU {}: {}", i, device.info.name);
            println!("    Total tasks: {}", tasks);
            println!("    Active tasks: {}", active);
            println!("    Total time: {} ms", time_us / 1000);
            println!("    Avg task time: {} us", avg_time_us);
            println!(
                "    Current workload: {:.1}%",
                device.get_workload() * 100.0
            );
            println!("    Migrations from: {}", migrations_from);
            println!("    Migrations to: {}", migrations_to);
        }
    }

    /// Reset statistics
    pub fn reset_stats(&self) {
        let mut stats = self.stats.write();
        let device_count = self.devices.len();
        stats.tasks_per_device = vec![0; device_count];
        stats.time_per_device = vec![0; device_count];
        stats.active_tasks = vec![0; device_count];
        stats.memory_per_device = vec![0; device_count];
        stats.migrations_from = vec![0; device_count];
        stats.migrations_to = vec![0; device_count];
    }

    /// Get device utilization (0.0 to 1.0)
    pub fn get_device_utilization(&self, device_index: usize) -> f32 {
        self.devices
            .get(device_index)
            .map(|device| device.get_workload())
            .unwrap_or(0.0)
    }

    /// Get overall cluster utilization (0.0 to 1.0)
    pub fn get_cluster_utilization(&self) -> f32 {
        if self.devices.is_empty() {
            return 0.0;
        }

        let total_utilization: f32 = self
            .devices
            .iter()
            .map(|device| device.get_workload())
            .sum();

        total_utilization / (self.devices.len() as f32)
    }

    /// Suggest optimal device for next task
    pub fn suggest_device(&self, estimated_memory: u64) -> Result<Arc<GpuDevice>> {
        // Filter devices with enough memory
        let candidates: Vec<_> = self
            .devices
            .iter()
            .filter(|device| {
                let memory_usage = device.get_memory_usage();
                let max_memory = device.info.max_buffer_size;
                (max_memory - memory_usage) >= estimated_memory
            })
            .collect();

        if candidates.is_empty() {
            return Err(GpuAdvancedError::GpuNotFound(
                "No device with enough memory".to_string(),
            ));
        }

        // Select based on strategy
        self.select_device()
    }

    /// Get migration history statistics
    pub fn get_migration_stats(&self) -> (usize, usize, f64) {
        let history = self.migration_history.read();
        (
            history.total_successful,
            history.total_failed,
            history.overall_success_rate(),
        )
    }

    /// Get device count
    pub fn device_count(&self) -> usize {
        self.devices.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_stats() {
        let stats = LoadStats::default();
        assert_eq!(stats.tasks_per_device.len(), 0);
    }

    #[test]
    fn test_selection_strategy() {
        // Test that strategies are copy
        let strategy = SelectionStrategy::RoundRobin;
        let _strategy2 = strategy;
        // This compiles, proving Copy trait works
    }

    #[test]
    fn test_migration_config_default() {
        let config = MigrationConfig::default();
        assert!(config.overload_threshold > 0.0);
        assert!(config.overload_threshold <= 1.0);
        assert!(config.underutilization_threshold >= 0.0);
        assert!(config.underutilization_threshold < config.overload_threshold);
    }

    #[test]
    fn test_migratable_workload() {
        let workload = MigratableWorkload::new(1, 0, 1024 * 1024, 0.5, 10);
        assert_eq!(workload.id, 1);
        assert_eq!(workload.source_device, 0);
        assert_eq!(workload.memory_size, 1024 * 1024);
        assert!(!workload.migrating);

        let workload_with_dep = workload.with_dependency(0);
        assert_eq!(workload_with_dep.dependencies.len(), 1);
    }

    #[test]
    fn test_migration_cost_calculation() {
        let config = MigrationConfig::default();
        let workload = MigratableWorkload::new(1, 0, 1024 * 1024, 0.5, 10);

        let cost = workload.calculate_migration_cost(&config);
        assert!(cost > config.transfer_cost_base);
    }

    #[test]
    fn test_migration_plan() {
        let config = MigrationConfig::default();
        let workload = MigratableWorkload::new(1, 0, 1024 * 1024, 0.8, 10);
        let plan = MigrationPlan::new(workload, 1, &config);

        assert_eq!(plan.source_device, 0);
        assert_eq!(plan.target_device, 1);
        assert!(plan.estimated_cost > 0.0);
    }

    #[test]
    fn test_migration_history() {
        let mut history = MigrationHistory::new(10);

        history.add_entry(MigrationHistoryEntry {
            timestamp: Instant::now(),
            source_device: 0,
            target_device: 1,
            success: true,
            transfer_time: Duration::from_millis(10),
            bytes_transferred: 1024,
        });

        assert_eq!(history.total_successful, 1);
        assert_eq!(history.total_failed, 0);
        assert!((history.overall_success_rate() - 1.0).abs() < f64::EPSILON);

        history.add_entry(MigrationHistoryEntry {
            timestamp: Instant::now(),
            source_device: 0,
            target_device: 1,
            success: false,
            transfer_time: Duration::from_millis(5),
            bytes_transferred: 0,
        });

        assert_eq!(history.total_failed, 1);
        assert!((history.overall_success_rate() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_workload_tracker() {
        let mut tracker = WorkloadTracker::new(2, 100);

        let id1 = tracker.next_workload_id();
        let id2 = tracker.next_workload_id();
        assert_ne!(id1, id2);

        let workload = MigratableWorkload::new(id1, 0, 1024, 0.5, 10);
        tracker.add_workload(0, workload);
        assert_eq!(tracker.pending_count(0), 1);

        let removed = tracker.remove_workload(0, id1);
        assert!(removed.is_some());
        assert_eq!(tracker.pending_count(0), 0);
    }

    #[test]
    fn test_utilization_sample() {
        let mut tracker = WorkloadTracker::new(2, 100);

        for i in 0..10 {
            tracker.record_sample(
                0,
                UtilizationSample {
                    timestamp: Instant::now(),
                    compute: 0.1 * (i as f32),
                    memory: 0.05 * (i as f32),
                    active_tasks: i,
                },
            );
        }

        let (avg_compute, avg_memory) = tracker
            .average_utilization(0, 5)
            .expect("Should have samples");
        assert!(avg_compute > 0.0);
        assert!(avg_memory > 0.0);

        let trend = tracker.utilization_trend(0, 10).expect("Should have trend");
        assert!(trend > 0.0); // Increasing trend
    }

    #[test]
    fn test_device_load() {
        let config = MigrationConfig::default();

        let load = DeviceLoad {
            device_index: 0,
            compute_utilization: 0.9,
            memory_utilization: 0.5,
            combined_load: 0.85,
            active_tasks: 5,
            pending_workloads: 3,
            score: 0.7,
            trend: 0.1,
        };

        assert!(load.is_overloaded(&config));
        assert!(!load.is_underutilized(&config));

        let underutilized_load = DeviceLoad {
            device_index: 1,
            compute_utilization: 0.1,
            memory_utilization: 0.1,
            combined_load: 0.1,
            active_tasks: 0,
            pending_workloads: 0,
            score: 0.9,
            trend: -0.05,
        };

        assert!(!underutilized_load.is_overloaded(&config));
        assert!(underutilized_load.is_underutilized(&config));
    }

    #[test]
    fn test_migration_history_average_time() {
        let mut history = MigrationHistory::new(10);

        history.add_entry(MigrationHistoryEntry {
            timestamp: Instant::now(),
            source_device: 0,
            target_device: 1,
            success: true,
            transfer_time: Duration::from_millis(10),
            bytes_transferred: 1024,
        });

        history.add_entry(MigrationHistoryEntry {
            timestamp: Instant::now(),
            source_device: 0,
            target_device: 1,
            success: true,
            transfer_time: Duration::from_millis(20),
            bytes_transferred: 2048,
        });

        let avg = history
            .average_transfer_time(0, 1)
            .expect("Should have average");
        assert_eq!(avg, Duration::from_millis(15));

        assert!(history.average_transfer_time(1, 0).is_none());
    }

    #[test]
    fn test_migration_history_success_rate() {
        let mut history = MigrationHistory::new(10);

        // No entries - assume success
        assert!((history.success_rate(0, 1) - 1.0).abs() < f64::EPSILON);

        // Add entries
        for _ in 0..3 {
            history.add_entry(MigrationHistoryEntry {
                timestamp: Instant::now(),
                source_device: 0,
                target_device: 1,
                success: true,
                transfer_time: Duration::from_millis(10),
                bytes_transferred: 1024,
            });
        }

        history.add_entry(MigrationHistoryEntry {
            timestamp: Instant::now(),
            source_device: 0,
            target_device: 1,
            success: false,
            transfer_time: Duration::from_millis(5),
            bytes_transferred: 0,
        });

        let rate = history.success_rate(0, 1);
        assert!((rate - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn test_workload_tracker_cooldown() {
        let mut tracker = WorkloadTracker::new(2, 100);

        // Initially not in cooldown
        assert!(!tracker.is_in_cooldown(0, 1));

        // Update rebalance time
        tracker.update_rebalance_time(0);

        // Now in cooldown
        assert!(tracker.is_in_cooldown(0, 1));

        // Wait and check (using 0 seconds should always pass)
        assert!(!tracker.is_in_cooldown(0, 0));
    }

    #[test]
    fn test_workload_tracker_migratable() {
        let mut tracker = WorkloadTracker::new(2, 100);

        let workload1 = MigratableWorkload::new(0, 0, 1024, 0.5, 10);
        let mut workload2 = MigratableWorkload::new(1, 0, 2048, 0.7, 5);
        workload2.migrating = true;
        let workload3 = MigratableWorkload::new(2, 0, 4096, 0.3, 15).with_dependency(0);

        tracker.add_workload(0, workload1);
        tracker.add_workload(0, workload2);
        tracker.add_workload(0, workload3);

        let migratable = tracker.get_migratable_workloads(0);

        // Only workload1 should be migratable (workload2 is migrating, workload3 has dependency)
        assert_eq!(migratable.len(), 1);
        assert_eq!(migratable[0].id, 0);
    }

    #[test]
    fn test_utilization_trend_calculation() {
        let mut tracker = WorkloadTracker::new(1, 100);

        // Add increasing samples
        for i in 0..20 {
            tracker.record_sample(
                0,
                UtilizationSample {
                    timestamp: Instant::now(),
                    compute: 0.05 * (i as f32),
                    memory: 0.02 * (i as f32),
                    active_tasks: i,
                },
            );
        }

        let trend = tracker
            .utilization_trend(0, 20)
            .expect("Should compute trend");
        assert!(
            trend > 0.0,
            "Trend should be positive for increasing samples"
        );

        // Add decreasing samples
        let mut tracker2 = WorkloadTracker::new(1, 100);
        for i in 0..20 {
            tracker2.record_sample(
                0,
                UtilizationSample {
                    timestamp: Instant::now(),
                    compute: 1.0 - 0.05 * (i as f32),
                    memory: 0.5 - 0.02 * (i as f32),
                    active_tasks: 20 - i,
                },
            );
        }

        let trend2 = tracker2
            .utilization_trend(0, 20)
            .expect("Should compute trend");
        assert!(
            trend2 < 0.0,
            "Trend should be negative for decreasing samples"
        );
    }
}
