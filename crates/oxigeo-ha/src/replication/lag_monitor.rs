//! Replication lag monitoring.

use crate::error::{HaError, HaResult};
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::Notify;
use tokio::time::{Duration as TokioDuration, sleep};
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Lag monitoring configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LagMonitorConfig {
    /// Monitoring interval in milliseconds.
    pub interval_ms: u64,
    /// Warning threshold in milliseconds.
    pub warning_threshold_ms: u64,
    /// Critical threshold in milliseconds.
    pub critical_threshold_ms: u64,
    /// History size (number of samples to keep).
    pub history_size: usize,
}

impl Default for LagMonitorConfig {
    fn default() -> Self {
        Self {
            interval_ms: 1000,
            warning_threshold_ms: 5000,
            critical_threshold_ms: 10000,
            history_size: 100,
        }
    }
}

/// Lag severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LagSeverity {
    /// Normal - lag is below warning threshold.
    Normal,
    /// Warning - lag is above warning threshold.
    Warning,
    /// Critical - lag is above critical threshold.
    Critical,
}

/// Lag measurement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LagMeasurement {
    /// Measurement timestamp.
    pub timestamp: DateTime<Utc>,
    /// Lag in milliseconds.
    pub lag_ms: u64,
    /// Severity level.
    pub severity: LagSeverity,
}

/// Lag statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LagStats {
    /// Current lag in milliseconds.
    pub current_lag_ms: u64,
    /// Average lag in milliseconds.
    pub average_lag_ms: u64,
    /// Minimum lag in milliseconds.
    pub min_lag_ms: u64,
    /// Maximum lag in milliseconds.
    pub max_lag_ms: u64,
    /// Standard deviation of lag.
    pub std_dev_ms: f64,
    /// 95th percentile lag.
    pub p95_lag_ms: u64,
    /// 99th percentile lag.
    pub p99_lag_ms: u64,
}

/// Replication lag monitor.
pub struct LagMonitor {
    /// Node ID.
    node_id: Uuid,
    /// Configuration.
    config: Arc<RwLock<LagMonitorConfig>>,
    /// Lag measurements per replica.
    measurements: Arc<RwLock<HashMap<Uuid, VecDeque<LagMeasurement>>>>,
    /// Shutdown notifier.
    shutdown: Arc<Notify>,
}

impl LagMonitor {
    /// Create a new lag monitor.
    pub fn new(node_id: Uuid, config: LagMonitorConfig) -> Self {
        Self {
            node_id,
            config: Arc::new(RwLock::new(config)),
            measurements: Arc::new(RwLock::new(HashMap::new())),
            shutdown: Arc::new(Notify::new()),
        }
    }

    /// Start monitoring.
    pub async fn start(&self) -> HaResult<()> {
        info!("Starting lag monitor for node {}", self.node_id);

        let config = Arc::clone(&self.config);
        let measurements = Arc::clone(&self.measurements);
        let shutdown = Arc::clone(&self.shutdown);

        tokio::spawn(async move {
            Self::monitor_loop(config, measurements, shutdown).await;
        });

        Ok(())
    }

    /// Stop monitoring.
    pub async fn stop(&self) -> HaResult<()> {
        info!("Stopping lag monitor");
        self.shutdown.notify_waiters();
        Ok(())
    }

    /// Monitor loop.
    async fn monitor_loop(
        config: Arc<RwLock<LagMonitorConfig>>,
        _measurements: Arc<RwLock<HashMap<Uuid, VecDeque<LagMeasurement>>>>,
        shutdown: Arc<Notify>,
    ) {
        loop {
            let interval = {
                let cfg = config.read();
                TokioDuration::from_millis(cfg.interval_ms)
            };

            tokio::select! {
                _ = shutdown.notified() => {
                    info!("Lag monitor shutting down");
                    break;
                }
                _ = sleep(interval) => {
                    debug!("Checking replication lag");
                    // Monitoring logic would go here
                }
            }
        }
    }

    /// Record a lag measurement.
    pub fn record_lag(&self, replica_id: Uuid, lag_ms: u64) -> HaResult<()> {
        let config = self.config.read();
        let severity = if lag_ms >= config.critical_threshold_ms {
            LagSeverity::Critical
        } else if lag_ms >= config.warning_threshold_ms {
            LagSeverity::Warning
        } else {
            LagSeverity::Normal
        };

        let measurement = LagMeasurement {
            timestamp: Utc::now(),
            lag_ms,
            severity,
        };

        let mut measurements_guard = self.measurements.write();
        let history = measurements_guard
            .entry(replica_id)
            .or_insert_with(|| VecDeque::with_capacity(config.history_size));

        history.push_back(measurement.clone());

        while history.len() > config.history_size {
            history.pop_front();
        }

        match severity {
            LagSeverity::Normal => {
                debug!("Replica {} lag: {}ms (normal)", replica_id, lag_ms);
            }
            LagSeverity::Warning => {
                warn!("Replica {} lag: {}ms (warning)", replica_id, lag_ms);
            }
            LagSeverity::Critical => {
                warn!("Replica {} lag: {}ms (critical)", replica_id, lag_ms);
            }
        }

        Ok(())
    }

    /// Get lag statistics for a replica.
    pub fn get_stats(&self, replica_id: Uuid) -> HaResult<LagStats> {
        let measurements_guard = self.measurements.read();
        let history = measurements_guard.get(&replica_id).ok_or_else(|| {
            HaError::Replication(format!("No measurements for replica {}", replica_id))
        })?;

        if history.is_empty() {
            return Err(HaError::Replication(
                "No lag measurements available".to_string(),
            ));
        }

        let lags: Vec<u64> = history.iter().map(|m| m.lag_ms).collect();

        let current_lag_ms = lags.last().copied().unwrap_or(0);
        let sum: u64 = lags.iter().sum();
        let count = lags.len() as u64;
        let average_lag_ms = sum / count;

        let min_lag_ms = lags.iter().copied().min().unwrap_or(0);
        let max_lag_ms = lags.iter().copied().max().unwrap_or(0);

        let variance: f64 = lags
            .iter()
            .map(|&lag| {
                let diff = lag as f64 - average_lag_ms as f64;
                diff * diff
            })
            .sum::<f64>()
            / count as f64;

        let std_dev_ms = variance.sqrt();

        let mut sorted_lags = lags.clone();
        sorted_lags.sort_unstable();

        let p95_index = (sorted_lags.len() as f64 * 0.95) as usize;
        let p99_index = (sorted_lags.len() as f64 * 0.99) as usize;

        let p95_lag_ms = sorted_lags.get(p95_index).copied().unwrap_or(max_lag_ms);
        let p99_lag_ms = sorted_lags.get(p99_index).copied().unwrap_or(max_lag_ms);

        Ok(LagStats {
            current_lag_ms,
            average_lag_ms,
            min_lag_ms,
            max_lag_ms,
            std_dev_ms,
            p95_lag_ms,
            p99_lag_ms,
        })
    }

    /// Check if lag is within acceptable limits.
    pub fn is_healthy(&self, replica_id: Uuid) -> HaResult<bool> {
        let measurements_guard = self.measurements.read();
        let history = measurements_guard.get(&replica_id).ok_or_else(|| {
            HaError::Replication(format!("No measurements for replica {}", replica_id))
        })?;

        if let Some(latest) = history.back() {
            Ok(latest.severity != LagSeverity::Critical)
        } else {
            Ok(true)
        }
    }

    /// Get all replica IDs being monitored.
    pub fn get_monitored_replicas(&self) -> Vec<Uuid> {
        let measurements_guard = self.measurements.read();
        measurements_guard.keys().copied().collect()
    }

    /// Clear measurements for a replica.
    pub fn clear_measurements(&self, replica_id: Uuid) -> HaResult<()> {
        let mut measurements_guard = self.measurements.write();
        measurements_guard.remove(&replica_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_lag_monitor() {
        let node_id = Uuid::new_v4();
        let config = LagMonitorConfig::default();
        let monitor = LagMonitor::new(node_id, config);

        assert!(monitor.start().await.is_ok());

        let replica_id = Uuid::new_v4();

        assert!(monitor.record_lag(replica_id, 100).is_ok());
        assert!(monitor.record_lag(replica_id, 200).is_ok());
        assert!(monitor.record_lag(replica_id, 150).is_ok());

        let stats = monitor.get_stats(replica_id).ok();
        assert!(stats.is_some());

        if let Some(s) = stats {
            assert_eq!(s.current_lag_ms, 150);
            assert_eq!(s.min_lag_ms, 100);
            assert_eq!(s.max_lag_ms, 200);
        }

        if let Ok(is_healthy) = monitor.is_healthy(replica_id) {
            assert!(is_healthy);
        }

        assert!(monitor.stop().await.is_ok());
    }

    #[test]
    fn test_lag_severity() {
        let node_id = Uuid::new_v4();
        let config = LagMonitorConfig {
            warning_threshold_ms: 5000,
            critical_threshold_ms: 10000,
            ..Default::default()
        };
        let monitor = LagMonitor::new(node_id, config);

        let replica_id = Uuid::new_v4();

        monitor.record_lag(replica_id, 1000).ok();
        let measurements = monitor.measurements.read();
        let history = measurements
            .get(&replica_id)
            .expect("should have measurements for replica");
        assert_eq!(
            history
                .back()
                .expect("should have latest measurement")
                .severity,
            LagSeverity::Normal
        );
        drop(measurements);

        monitor.record_lag(replica_id, 6000).ok();
        let measurements = monitor.measurements.read();
        let history = measurements
            .get(&replica_id)
            .expect("should have measurements for replica");
        assert_eq!(
            history
                .back()
                .expect("should have latest measurement")
                .severity,
            LagSeverity::Warning
        );
        drop(measurements);

        monitor.record_lag(replica_id, 12000).ok();
        let measurements = monitor.measurements.read();
        let history = measurements
            .get(&replica_id)
            .expect("should have measurements for replica");
        assert_eq!(
            history
                .back()
                .expect("should have latest measurement")
                .severity,
            LagSeverity::Critical
        );
    }
}
