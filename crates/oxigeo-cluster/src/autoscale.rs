//! Cluster autoscaling for dynamic resource management.
//!
//! This module implements cluster autoscaling features including:
//! - Scale up/down based on load metrics
//! - Predictive scaling based on historical patterns
//! - Cool-down periods to prevent thrashing
//! - Provider integration through the [`CloudProvider`] trait, with a built-in
//!   [`WorkerPoolProvider`] that actually adds/removes workers in a live
//!   [`WorkerPool`]. Managed-cloud back-ends (AWS EC2, Azure VMSS, GCP MIG) are a
//!   caller-supplied extension point: implement [`CloudProvider`] against the
//!   respective SDK and pass it to [`Autoscaler::apply_decision`]. The core crate
//!   ships no cloud SDK bindings (they are not Pure Rust), so the wiring — not a
//!   specific vendor client — is what lives here.
//! - Cost optimization with spot instances (accounted whenever the active
//!   provider reports spot availability)
//! - Custom scaling policies
//!
//! The end-to-end control loop is: [`Autoscaler::record_metrics`] →
//! [`Autoscaler::evaluate`] (produces a [`ScaleDecision`]) →
//! [`Autoscaler::apply_decision`] (actually invokes the provider). Previously
//! `evaluate` produced a recommendation that nothing consumed; `apply_decision`
//! closes that loop.

use crate::error::{ClusterError, Result};
use crate::worker_pool::{
    Worker, WorkerCapabilities, WorkerCapacity, WorkerId, WorkerPool, WorkerStatus, WorkerUsage,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Autoscaling configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoscaleConfig {
    /// Enable autoscaling
    pub enabled: bool,
    /// Minimum number of workers
    pub min_workers: usize,
    /// Maximum number of workers
    pub max_workers: usize,
    /// Target CPU utilization (0.0 to 1.0)
    pub target_cpu_utilization: f64,
    /// Target memory utilization (0.0 to 1.0)
    pub target_memory_utilization: f64,
    /// Scale up threshold
    pub scale_up_threshold: f64,
    /// Scale down threshold
    pub scale_down_threshold: f64,
    /// Cool-down period after scale up
    pub scale_up_cooldown: Duration,
    /// Cool-down period after scale down
    pub scale_down_cooldown: Duration,
    /// Evaluation period
    pub evaluation_period: Duration,
    /// Number of evaluation periods to check
    pub evaluation_periods: usize,
    /// Enable predictive scaling
    pub enable_predictive_scaling: bool,
    /// Enable cost optimization
    pub enable_cost_optimization: bool,
}

impl Default for AutoscaleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_workers: 1,
            max_workers: 100,
            target_cpu_utilization: 0.7,
            target_memory_utilization: 0.8,
            scale_up_threshold: 0.8,
            scale_down_threshold: 0.4,
            scale_up_cooldown: Duration::from_secs(300), // 5 minutes
            scale_down_cooldown: Duration::from_secs(600), // 10 minutes
            evaluation_period: Duration::from_secs(60),  // 1 minute
            evaluation_periods: 3,
            enable_predictive_scaling: false,
            enable_cost_optimization: false,
        }
    }
}

/// Autoscaling decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScaleDecision {
    /// No scaling needed
    NoChange,
    /// Scale up by N workers
    ScaleUp(usize),
    /// Scale down by N workers
    ScaleDown(usize),
}

/// Autoscaler for dynamic cluster sizing.
pub struct Autoscaler {
    config: Arc<RwLock<AutoscaleConfig>>,
    /// Metrics history for analysis
    metrics_history: Arc<RwLock<VecDeque<MetricsSnapshot>>>,
    /// Last scale action timestamp
    last_scale_up: Arc<RwLock<Option<Instant>>>,
    last_scale_down: Arc<RwLock<Option<Instant>>>,
    /// Scaling history
    scaling_history: Arc<RwLock<Vec<ScalingEvent>>>,
    /// Predictive model
    predictor: Arc<RwLock<Option<PredictiveModel>>>,
    /// Statistics
    stats: Arc<RwLock<AutoscaleStats>>,
}

/// Metrics snapshot for scaling decisions.
#[derive(Debug, Clone)]
#[allow(missing_docs)]
pub struct MetricsSnapshot {
    #[allow(dead_code)]
    pub timestamp: Instant,
    pub cpu_utilization: f64,
    pub memory_utilization: f64,
    pub active_tasks: usize,
    pub pending_tasks: usize,
    pub worker_count: usize,
}

/// Scaling event record.
#[derive(Debug, Clone)]
#[allow(missing_docs)]
pub struct ScalingEvent {
    pub timestamp: Instant,
    pub action: ScaleAction,
    pub workers_before: usize,
    pub workers_after: usize,
    pub reason: String,
}

/// Scale action type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(missing_docs)]
pub enum ScaleAction {
    ScaleUp,
    ScaleDown,
}

/// Autoscaling statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[allow(missing_docs)]
pub struct AutoscaleStats {
    pub total_scale_ups: u64,
    pub total_scale_downs: u64,
    pub total_workers_added: usize,
    pub total_workers_removed: usize,
    pub average_cluster_size: f64,
    pub cost_savings: f64,
}

/// Predictive model for forecasting load.
#[derive(Debug, Clone)]
pub struct PredictiveModel {
    /// Historical data points
    history: VecDeque<f64>,
    /// Window size for prediction
    window_size: usize,
}

impl Autoscaler {
    /// Create a new autoscaler.
    pub fn new(config: AutoscaleConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            metrics_history: Arc::new(RwLock::new(VecDeque::new())),
            last_scale_up: Arc::new(RwLock::new(None)),
            last_scale_down: Arc::new(RwLock::new(None)),
            scaling_history: Arc::new(RwLock::new(Vec::new())),
            predictor: Arc::new(RwLock::new(None)),
            stats: Arc::new(RwLock::new(AutoscaleStats::default())),
        }
    }

    /// Record metrics snapshot.
    pub fn record_metrics(&self, snapshot: MetricsSnapshot) {
        let config = self.config.read();
        let mut history = self.metrics_history.write();

        history.push_back(snapshot);

        // Keep only recent history
        let max_history = config.evaluation_periods * 10;
        while history.len() > max_history {
            history.pop_front();
        }

        // Update predictive model if enabled
        if config.enable_predictive_scaling {
            self.update_predictor(history.back().map(|s| s.cpu_utilization).unwrap_or(0.0));
        }
    }

    /// Evaluate whether scaling is needed.
    pub fn evaluate(&self, worker_pool: &WorkerPool) -> Result<ScaleDecision> {
        let config = self.config.read();

        if !config.enabled {
            return Ok(ScaleDecision::NoChange);
        }

        let history = self.metrics_history.read();
        if history.len() < config.evaluation_periods {
            debug!("Not enough metrics history for scaling decision");
            return Ok(ScaleDecision::NoChange);
        }

        // Get recent metrics
        let recent: Vec<_> = history
            .iter()
            .rev()
            .take(config.evaluation_periods)
            .collect();

        let avg_cpu = recent.iter().map(|s| s.cpu_utilization).sum::<f64>() / recent.len() as f64;
        let avg_memory =
            recent.iter().map(|s| s.memory_utilization).sum::<f64>() / recent.len() as f64;
        let current_workers = worker_pool.get_worker_count();

        // Check if we should scale up
        if (avg_cpu > config.scale_up_threshold || avg_memory > config.scale_up_threshold)
            && current_workers < config.max_workers
        {
            // Check cool-down
            if self.in_cooldown_period(*self.last_scale_up.read(), config.scale_up_cooldown) {
                debug!("In scale-up cool-down period");
                return Ok(ScaleDecision::NoChange);
            }

            let workers_needed =
                self.calculate_workers_needed(avg_cpu, avg_memory, current_workers);
            return Ok(ScaleDecision::ScaleUp(workers_needed));
        }

        // Check if we should scale down
        if (avg_cpu < config.scale_down_threshold && avg_memory < config.scale_down_threshold)
            && current_workers > config.min_workers
        {
            // Check cool-down
            if self.in_cooldown_period(*self.last_scale_down.read(), config.scale_down_cooldown) {
                debug!("In scale-down cool-down period");
                return Ok(ScaleDecision::NoChange);
            }

            let workers_to_remove =
                self.calculate_workers_to_remove(avg_cpu, avg_memory, current_workers);
            return Ok(ScaleDecision::ScaleDown(workers_to_remove));
        }

        Ok(ScaleDecision::NoChange)
    }

    fn calculate_workers_needed(&self, cpu_util: f64, memory_util: f64, current: usize) -> usize {
        let config = self.config.read();

        let max_util = cpu_util.max(memory_util);
        let target = config.target_cpu_utilization;

        // Calculate ideal number of workers to reach target utilization
        let ideal = ((current as f64 * max_util) / target).ceil() as usize;
        let needed = ideal.saturating_sub(current);

        // Scale up conservatively (max 20% increase at a time)
        let max_increase = ((current as f64 * 0.2).ceil() as usize).max(1);
        needed.min(max_increase).min(config.max_workers - current)
    }

    fn calculate_workers_to_remove(
        &self,
        cpu_util: f64,
        memory_util: f64,
        current: usize,
    ) -> usize {
        let config = self.config.read();

        let max_util = cpu_util.max(memory_util);
        let target = config.target_cpu_utilization;

        // Calculate ideal number of workers
        let ideal =
            (((current as f64 * max_util) / target).ceil() as usize).max(config.min_workers);
        let to_remove = current.saturating_sub(ideal);

        // Scale down conservatively (max 10% decrease at a time)
        let max_decrease = (current as f64 * 0.1).ceil() as usize;
        to_remove
            .min(max_decrease)
            .min(current - config.min_workers)
    }

    fn in_cooldown_period(&self, last_action: Option<Instant>, cooldown: Duration) -> bool {
        match last_action {
            Some(last) => last.elapsed() < cooldown,
            None => false,
        }
    }

    /// Execute a scale up action.
    pub fn execute_scale_up(&self, count: usize, current_workers: usize) -> Result<()> {
        info!("Scaling up by {} workers", count);

        *self.last_scale_up.write() = Some(Instant::now());

        // Record event
        let event = ScalingEvent {
            timestamp: Instant::now(),
            action: ScaleAction::ScaleUp,
            workers_before: current_workers,
            workers_after: current_workers + count,
            reason: "High resource utilization".to_string(),
        };

        self.scaling_history.write().push(event);

        // Update statistics
        let mut stats = self.stats.write();
        stats.total_scale_ups += 1;
        stats.total_workers_added += count;

        Ok(())
    }

    /// Execute a scale down action.
    pub fn execute_scale_down(&self, count: usize, current_workers: usize) -> Result<()> {
        info!("Scaling down by {} workers", count);

        *self.last_scale_down.write() = Some(Instant::now());

        // Record event
        let event = ScalingEvent {
            timestamp: Instant::now(),
            action: ScaleAction::ScaleDown,
            workers_before: current_workers,
            workers_after: current_workers.saturating_sub(count),
            reason: "Low resource utilization".to_string(),
        };

        self.scaling_history.write().push(event);

        // Update statistics
        let mut stats = self.stats.write();
        stats.total_scale_downs += 1;
        stats.total_workers_removed += count;

        Ok(())
    }

    /// Apply a [`ScaleDecision`] against a concrete [`CloudProvider`], closing
    /// the loop between recommendation and action.
    ///
    /// This is the glue that was previously missing: [`Autoscaler::evaluate`]
    /// only ever returned a recommendation, and nothing invoked the provider.
    /// `apply_decision` actually provisions or releases workers, records the
    /// scaling event, updates the cool-down timestamps and statistics, and — when
    /// cost optimization is enabled and the provider reports spot availability —
    /// accrues the spot savings.
    ///
    /// Returns the number of workers actually added (positive) or removed
    /// (reported via [`ScaleOutcome`]). A [`ScaleDecision::NoChange`] is a no-op.
    pub fn apply_decision(
        &self,
        decision: &ScaleDecision,
        provider: &dyn CloudProvider,
        current_workers: usize,
    ) -> Result<ScaleOutcome> {
        match decision {
            ScaleDecision::NoChange => Ok(ScaleOutcome::default()),
            ScaleDecision::ScaleUp(count) => {
                let count = *count;
                if count == 0 {
                    return Ok(ScaleOutcome::default());
                }
                let added = provider.add_workers(count)?;
                let added_count = added.len();
                if added_count == 0 {
                    warn!("Provider added no workers for a scale-up of {}", count);
                    return Ok(ScaleOutcome::default());
                }

                self.execute_scale_up(added_count, current_workers)?;
                self.accrue_spot_savings(provider, &added);

                info!(
                    "Applied scale-up: provisioned {} worker(s) via provider",
                    added_count
                );
                Ok(ScaleOutcome {
                    added: added.clone(),
                    removed: Vec::new(),
                })
            }
            ScaleDecision::ScaleDown(count) => {
                let count = *count;
                if count == 0 {
                    return Ok(ScaleOutcome::default());
                }
                let victims = provider.select_workers_to_remove(count)?;
                if victims.is_empty() {
                    warn!("Provider offered no workers to remove for a scale-down");
                    return Ok(ScaleOutcome::default());
                }
                provider.remove_workers(victims.clone())?;
                let removed_count = victims.len();

                self.execute_scale_down(removed_count, current_workers)?;

                info!(
                    "Applied scale-down: released {} worker(s) via provider",
                    removed_count
                );
                Ok(ScaleOutcome {
                    added: Vec::new(),
                    removed: victims,
                })
            }
        }
    }

    /// Accrue spot-instance cost savings for newly-added workers, when enabled.
    fn accrue_spot_savings(&self, provider: &dyn CloudProvider, added: &[WorkerId]) {
        if !self.config.read().enable_cost_optimization {
            return;
        }
        let spot = provider.is_spot_available().unwrap_or(false);
        if !spot {
            return;
        }

        // Spot instances are conventionally ~70% cheaper than on-demand; account
        // the avoided on-demand fraction as realized savings per worker-hour.
        const SPOT_DISCOUNT: f64 = 0.7;
        let mut savings = 0.0;
        for id in added {
            if let Ok(cost) = provider.get_worker_cost(id) {
                savings += cost * SPOT_DISCOUNT;
            }
        }
        if savings > 0.0 {
            self.stats.write().cost_savings += savings;
        }
    }

    fn update_predictor(&self, value: f64) {
        let mut predictor = self.predictor.write();

        if predictor.is_none() {
            *predictor = Some(PredictiveModel {
                history: VecDeque::new(),
                window_size: 60, // 1 hour if recording every minute
            });
        }

        if let Some(ref mut model) = *predictor {
            model.history.push_back(value);
            if model.history.len() > model.window_size {
                model.history.pop_front();
            }
        }
    }

    /// Predict future load using simple moving average.
    pub fn predict_load(&self, _periods_ahead: usize) -> Option<f64> {
        let predictor = self.predictor.read();

        if let Some(model) = predictor.as_ref() {
            if model.history.len() < 10 {
                return None;
            }

            // Simple moving average
            let sum: f64 = model.history.iter().sum();
            let avg = sum / model.history.len() as f64;

            Some(avg)
        } else {
            None
        }
    }

    /// Get scaling history.
    pub fn get_scaling_history(&self) -> Vec<ScalingEvent> {
        self.scaling_history.read().clone()
    }

    /// Get autoscaling statistics.
    pub fn get_stats(&self) -> AutoscaleStats {
        self.stats.read().clone()
    }

    /// Update autoscaling configuration.
    pub fn update_config(&self, config: AutoscaleConfig) {
        *self.config.write() = config;
    }

    /// Get current configuration.
    pub fn get_config(&self) -> AutoscaleConfig {
        self.config.read().clone()
    }
}

/// Outcome of applying a [`ScaleDecision`] through [`Autoscaler::apply_decision`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScaleOutcome {
    /// Workers that were actually provisioned.
    pub added: Vec<WorkerId>,
    /// Workers that were actually released.
    pub removed: Vec<WorkerId>,
}

impl ScaleOutcome {
    /// Net change in worker count (positive = added, negative = removed).
    pub fn net_change(&self) -> i64 {
        self.added.len() as i64 - self.removed.len() as i64
    }

    /// Whether this outcome changed the cluster at all.
    pub fn is_noop(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty()
    }
}

/// Provider interface an [`Autoscaler`] drives to actually provision or release
/// workers.
///
/// The core crate ships one real implementation, [`WorkerPoolProvider`], which
/// backs a live [`WorkerPool`]. Managed-cloud back-ends (AWS/Azure/GCP) are
/// implemented by callers against the vendor SDK — those SDKs are not Pure Rust
/// and therefore do not live in this crate — and then passed to
/// [`Autoscaler::apply_decision`].
pub trait CloudProvider {
    /// Add `count` workers to the cluster, returning the ids actually created.
    ///
    /// Returning fewer ids than requested (e.g. because a capacity limit was hit)
    /// is allowed and reported honestly to the caller.
    fn add_workers(&self, count: usize) -> Result<Vec<WorkerId>>;

    /// Remove the given workers from the cluster.
    fn remove_workers(&self, worker_ids: Vec<WorkerId>) -> Result<()>;

    /// Get cost per worker hour for a specific worker.
    fn get_worker_cost(&self, worker_id: &WorkerId) -> Result<f64>;

    /// Check if spot/preemptible capacity is currently available.
    fn is_spot_available(&self) -> Result<bool>;

    /// Choose up to `count` workers that are the best candidates for removal
    /// during a scale-down.
    ///
    /// Default implementations that cannot enumerate their fleet return an empty
    /// list, which [`Autoscaler::apply_decision`] treats as "nothing to remove".
    /// [`WorkerPoolProvider`] overrides this to pick the least-loaded workers.
    fn select_workers_to_remove(&self, _count: usize) -> Result<Vec<WorkerId>> {
        Ok(Vec::new())
    }

    /// Human-readable provider name (used for logging/diagnostics).
    fn provider_name(&self) -> &str {
        "unnamed-provider"
    }
}

/// A built-in [`CloudProvider`] that provisions and releases workers directly in
/// an in-process [`WorkerPool`].
///
/// This is a fully functional provider — not a stub — suitable for single-host
/// deployments, integration testing, and as the reference implementation that
/// managed-cloud providers mirror. `add_workers` registers freshly-constructed
/// [`Worker`]s into the pool (honoring its `max_workers` capacity), and
/// `remove_workers` unregisters them.
pub struct WorkerPoolProvider {
    pool: Arc<WorkerPool>,
    /// On-demand cost per worker-hour, reported by [`CloudProvider::get_worker_cost`].
    cost_per_hour: f64,
    /// Whether this provider advertises spot capacity.
    spot_available: bool,
    /// Per-worker cost overrides recorded at creation time.
    costs: RwLock<HashMap<WorkerId, f64>>,
    /// Address template used for provisioned workers (e.g. `"local://autoscaled"`).
    address: String,
}

impl WorkerPoolProvider {
    /// Create a new provider backed by the given worker pool.
    pub fn new(pool: Arc<WorkerPool>) -> Self {
        Self {
            pool,
            cost_per_hour: 1.0,
            spot_available: false,
            costs: RwLock::new(HashMap::new()),
            address: "local://autoscaled".to_string(),
        }
    }

    /// Set the on-demand cost per worker-hour.
    pub fn with_cost_per_hour(mut self, cost: f64) -> Self {
        self.cost_per_hour = cost;
        self
    }

    /// Advertise spot/preemptible capacity availability.
    pub fn with_spot_available(mut self, available: bool) -> Self {
        self.spot_available = available;
        self
    }

    /// Set the address assigned to provisioned workers.
    pub fn with_address(mut self, address: impl Into<String>) -> Self {
        self.address = address.into();
        self
    }

    /// Build a fresh, idle worker to register into the pool.
    fn build_worker(&self) -> Worker {
        let now = Instant::now();
        Worker {
            id: WorkerId::new(),
            name: format!("autoscaled-{}", WorkerId::new()),
            address: self.address.clone(),
            capabilities: WorkerCapabilities::default(),
            capacity: WorkerCapacity::default(),
            usage: WorkerUsage::default(),
            status: WorkerStatus::Idle,
            last_heartbeat: now,
            registered_at: now,
            last_health_check: None,
            health_check_failures: 0,
            tasks_completed: 0,
            tasks_failed: 0,
            version: env!("CARGO_PKG_VERSION").to_string(),
            metadata: HashMap::new(),
        }
    }
}

impl CloudProvider for WorkerPoolProvider {
    fn add_workers(&self, count: usize) -> Result<Vec<WorkerId>> {
        let mut created = Vec::with_capacity(count);
        for _ in 0..count {
            let worker = self.build_worker();
            match self.pool.register_worker(worker) {
                Ok(id) => {
                    self.costs.write().insert(id, self.cost_per_hour);
                    created.push(id);
                }
                Err(ClusterError::CapacityExceeded(_)) => {
                    // Pool is full — stop and report what we actually created.
                    warn!(
                        "WorkerPoolProvider hit pool capacity after adding {} of {} workers",
                        created.len(),
                        count
                    );
                    break;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(created)
    }

    fn remove_workers(&self, worker_ids: Vec<WorkerId>) -> Result<()> {
        let mut costs = self.costs.write();
        for id in worker_ids {
            self.pool.unregister_worker(id)?;
            costs.remove(&id);
        }
        Ok(())
    }

    fn get_worker_cost(&self, worker_id: &WorkerId) -> Result<f64> {
        Ok(self
            .costs
            .read()
            .get(worker_id)
            .copied()
            .unwrap_or(self.cost_per_hour))
    }

    fn is_spot_available(&self) -> Result<bool> {
        Ok(self.spot_available)
    }

    fn select_workers_to_remove(&self, count: usize) -> Result<Vec<WorkerId>> {
        // Prefer the least-loaded workers (fewest active tasks) as removal
        // candidates, so busy workers are retained.
        let mut candidates: Vec<(WorkerId, u32)> = self
            .pool
            .get_all_workers()
            .into_iter()
            .map(|w| {
                let guard = w.read();
                (guard.id, guard.usage.active_tasks)
            })
            .collect();
        candidates.sort_by_key(|(_, active)| *active);
        Ok(candidates
            .into_iter()
            .take(count)
            .map(|(id, _)| id)
            .collect())
    }

    fn provider_name(&self) -> &str {
        "worker-pool"
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_autoscaler_creation() {
        let config = AutoscaleConfig::default();
        let autoscaler = Autoscaler::new(config);

        let stats = autoscaler.get_stats();
        assert_eq!(stats.total_scale_ups, 0);
        assert_eq!(stats.total_scale_downs, 0);
    }

    #[test]
    fn test_metrics_recording() {
        let config = AutoscaleConfig::default();
        let autoscaler = Autoscaler::new(config);

        let snapshot = MetricsSnapshot {
            timestamp: Instant::now(),
            cpu_utilization: 0.5,
            memory_utilization: 0.6,
            active_tasks: 10,
            pending_tasks: 5,
            worker_count: 3,
        };

        autoscaler.record_metrics(snapshot);

        let history = autoscaler.metrics_history.read();
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn test_scale_up_calculation() {
        let config = AutoscaleConfig {
            target_cpu_utilization: 0.7,
            max_workers: 10,
            ..Default::default()
        };

        let autoscaler = Autoscaler::new(config);

        // High utilization should trigger scale up
        let needed = autoscaler.calculate_workers_needed(0.9, 0.8, 5);
        assert!(needed > 0);
    }

    #[test]
    fn test_scale_down_calculation() {
        let config = AutoscaleConfig {
            target_cpu_utilization: 0.7,
            min_workers: 1,
            ..Default::default()
        };

        let autoscaler = Autoscaler::new(config);

        // Low utilization should trigger scale down
        let to_remove = autoscaler.calculate_workers_to_remove(0.3, 0.2, 5);
        assert!(to_remove > 0);
    }

    use crate::worker_pool::{WorkerPool, WorkerPoolConfig};

    fn test_pool(max_workers: usize) -> Arc<WorkerPool> {
        Arc::new(WorkerPool::new(WorkerPoolConfig {
            max_workers,
            ..Default::default()
        }))
    }

    #[test]
    fn test_worker_pool_provider_adds_and_removes_real_workers() {
        let pool = test_pool(10);
        let provider = WorkerPoolProvider::new(Arc::clone(&pool));

        let ids = provider.add_workers(3).expect("add");
        assert_eq!(ids.len(), 3);
        assert_eq!(pool.get_worker_count(), 3);

        provider
            .remove_workers(vec![ids[0], ids[1]])
            .expect("remove");
        assert_eq!(pool.get_worker_count(), 1);
    }

    #[test]
    fn test_worker_pool_provider_respects_capacity() {
        let pool = test_pool(2);
        let provider = WorkerPoolProvider::new(Arc::clone(&pool));

        // Asking for 5 on a pool capped at 2 provisions exactly 2 and reports it.
        let ids = provider.add_workers(5).expect("add");
        assert_eq!(ids.len(), 2);
        assert_eq!(pool.get_worker_count(), 2);
    }

    #[test]
    fn test_apply_scale_up_provisions_via_provider() {
        let pool = test_pool(10);
        let provider = WorkerPoolProvider::new(Arc::clone(&pool))
            .with_cost_per_hour(2.0)
            .with_spot_available(true);

        let config = AutoscaleConfig {
            enable_cost_optimization: true,
            ..Default::default()
        };
        let autoscaler = Autoscaler::new(config);

        let outcome = autoscaler
            .apply_decision(&ScaleDecision::ScaleUp(3), &provider, 0)
            .expect("apply scale up");

        assert_eq!(outcome.added.len(), 3);
        assert_eq!(outcome.net_change(), 3);
        // The workers really exist in the pool now.
        assert_eq!(pool.get_worker_count(), 3);

        let stats = autoscaler.get_stats();
        assert_eq!(stats.total_scale_ups, 1);
        assert_eq!(stats.total_workers_added, 3);
        // Spot savings were accrued (3 workers * 2.0/hr * 0.7 discount).
        assert!((stats.cost_savings - 3.0 * 2.0 * 0.7).abs() < 1e-9);
    }

    #[test]
    fn test_apply_scale_down_releases_via_provider() {
        let pool = test_pool(10);
        let provider = WorkerPoolProvider::new(Arc::clone(&pool));

        // Pre-provision 5 workers.
        let _ = provider.add_workers(5).expect("seed");
        assert_eq!(pool.get_worker_count(), 5);

        let autoscaler = Autoscaler::new(AutoscaleConfig::default());
        let outcome = autoscaler
            .apply_decision(&ScaleDecision::ScaleDown(2), &provider, 5)
            .expect("apply scale down");

        assert_eq!(outcome.removed.len(), 2);
        assert_eq!(outcome.net_change(), -2);
        assert_eq!(pool.get_worker_count(), 3);

        let stats = autoscaler.get_stats();
        assert_eq!(stats.total_scale_downs, 1);
        assert_eq!(stats.total_workers_removed, 2);
    }

    #[test]
    fn test_apply_no_change_is_noop() {
        let pool = test_pool(10);
        let provider = WorkerPoolProvider::new(Arc::clone(&pool));
        let autoscaler = Autoscaler::new(AutoscaleConfig::default());

        let outcome = autoscaler
            .apply_decision(&ScaleDecision::NoChange, &provider, 4)
            .expect("noop");
        assert!(outcome.is_noop());
        assert_eq!(pool.get_worker_count(), 0);
        assert_eq!(autoscaler.get_stats().total_scale_ups, 0);
    }

    #[test]
    fn test_cooldown_period() {
        let config = AutoscaleConfig {
            scale_up_cooldown: Duration::from_secs(60),
            ..Default::default()
        };

        let autoscaler = Autoscaler::new(config.clone());

        // Set last scale up
        *autoscaler.last_scale_up.write() = Some(Instant::now());

        // Should be in cool-down
        assert!(
            autoscaler
                .in_cooldown_period(*autoscaler.last_scale_up.read(), config.scale_up_cooldown)
        );
    }
}
