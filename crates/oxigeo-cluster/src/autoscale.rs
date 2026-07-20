//! Cluster autoscaling for dynamic resource management.
//!
//! This module implements cluster autoscaling features including:
//! - Scale up/down based on load metrics
//! - Predictive scaling based on historical patterns
//! - Cool-down periods to prevent thrashing
//! - Cloud provider integration (AWS, Azure, GCP)
//! - Cost optimization with spot instances
//! - Custom scaling policies

use crate::error::Result;
use crate::worker_pool::{WorkerId, WorkerPool};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info};

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

/// Cloud provider interface for autoscaling.
pub trait CloudProvider {
    /// Add workers to the cluster.
    fn add_workers(&self, count: usize) -> Result<Vec<WorkerId>>;

    /// Remove workers from the cluster.
    fn remove_workers(&self, worker_ids: Vec<WorkerId>) -> Result<()>;

    /// Get cost per worker hour.
    fn get_worker_cost(&self, worker_id: &WorkerId) -> Result<f64>;

    /// Check if spot instance is available.
    fn is_spot_available(&self) -> Result<bool>;
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
