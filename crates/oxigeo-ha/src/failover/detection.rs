//! Failure detection using heartbeat and health checks.

use super::FailoverConfig;
use crate::error::{HaError, HaResult};
use chrono::{DateTime, Duration, Utc};
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{Notify, mpsc};
use tokio::time::{Duration as TokioDuration, sleep};
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Health status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    /// Node is healthy.
    Healthy,
    /// Node is degraded but functional.
    Degraded,
    /// Node is unhealthy.
    Unhealthy,
    /// Node status is unknown.
    Unknown,
}

/// Node heartbeat information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Heartbeat {
    /// Node ID.
    pub node_id: Uuid,
    /// Timestamp of the heartbeat.
    pub timestamp: DateTime<Utc>,
    /// Health status.
    pub health_status: HealthStatus,
    /// Sequence number.
    pub sequence: u64,
}

/// Failure detection result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureDetection {
    /// Node ID.
    pub node_id: Uuid,
    /// Detected as failed.
    pub is_failed: bool,
    /// Last heartbeat timestamp.
    pub last_heartbeat: Option<DateTime<Utc>>,
    /// Time since last heartbeat in milliseconds.
    pub time_since_heartbeat_ms: Option<u64>,
    /// Health status.
    pub health_status: HealthStatus,
}

/// Failure detector.
pub struct FailureDetector {
    /// Configuration.
    config: Arc<RwLock<FailoverConfig>>,
    /// Heartbeat tracking.
    heartbeats: Arc<DashMap<Uuid, Heartbeat>>,
    /// Running flag.
    running: AtomicBool,
    /// Failure notification channel.
    failure_tx: mpsc::UnboundedSender<Uuid>,
    /// Failure notification receiver.
    failure_rx: Arc<RwLock<Option<mpsc::UnboundedReceiver<Uuid>>>>,
    /// Shutdown notifier.
    shutdown: Arc<Notify>,
}

impl FailureDetector {
    /// Create a new failure detector.
    pub fn new(config: FailoverConfig) -> Self {
        let (failure_tx, failure_rx) = mpsc::unbounded_channel();

        Self {
            config: Arc::new(RwLock::new(config)),
            heartbeats: Arc::new(DashMap::new()),
            running: AtomicBool::new(false),
            failure_tx,
            failure_rx: Arc::new(RwLock::new(Some(failure_rx))),
            shutdown: Arc::new(Notify::new()),
        }
    }

    /// Start failure detection.
    pub async fn start(&self) -> HaResult<()> {
        if self.running.swap(true, Ordering::SeqCst) {
            return Err(HaError::InvalidState(
                "Failure detector already running".to_string(),
            ));
        }

        info!("Starting failure detector");

        let config = Arc::clone(&self.config);
        let heartbeats = Arc::clone(&self.heartbeats);
        let failure_tx = self.failure_tx.clone();
        let shutdown = Arc::clone(&self.shutdown);

        tokio::spawn(async move {
            Self::detection_loop(config, heartbeats, failure_tx, shutdown).await;
        });

        Ok(())
    }

    /// Stop failure detection.
    pub async fn stop(&self) -> HaResult<()> {
        if !self.running.swap(false, Ordering::SeqCst) {
            return Err(HaError::InvalidState(
                "Failure detector not running".to_string(),
            ));
        }

        info!("Stopping failure detector");
        self.shutdown.notify_waiters();

        Ok(())
    }

    /// Detection loop.
    async fn detection_loop(
        config: Arc<RwLock<FailoverConfig>>,
        heartbeats: Arc<DashMap<Uuid, Heartbeat>>,
        failure_tx: mpsc::UnboundedSender<Uuid>,
        shutdown: Arc<Notify>,
    ) {
        loop {
            let interval = {
                let cfg = config.read();
                TokioDuration::from_millis(cfg.heartbeat_interval_ms)
            };

            tokio::select! {
                _ = shutdown.notified() => {
                    info!("Failure detector shutting down");
                    break;
                }
                _ = sleep(interval) => {
                    Self::check_failures(&config, &heartbeats, &failure_tx);
                }
            }
        }
    }

    /// Check for failures.
    fn check_failures(
        config: &Arc<RwLock<FailoverConfig>>,
        heartbeats: &Arc<DashMap<Uuid, Heartbeat>>,
        failure_tx: &mpsc::UnboundedSender<Uuid>,
    ) {
        let cfg = config.read();
        let timeout = Duration::milliseconds(cfg.heartbeat_timeout_ms as i64);
        let now = Utc::now();

        for entry in heartbeats.iter() {
            let node_id = *entry.key();
            let heartbeat = entry.value();

            let elapsed = now - heartbeat.timestamp;
            if elapsed > timeout {
                warn!(
                    "Node {} failed: no heartbeat for {}ms",
                    node_id,
                    elapsed.num_milliseconds()
                );

                if let Err(e) = failure_tx.send(node_id) {
                    warn!("Failed to send failure notification: {}", e);
                }

                heartbeats.remove(&node_id);
            }
        }
    }

    /// Record a heartbeat.
    pub fn record_heartbeat(&self, heartbeat: Heartbeat) -> HaResult<()> {
        debug!("Received heartbeat from node {}", heartbeat.node_id);
        self.heartbeats.insert(heartbeat.node_id, heartbeat);
        Ok(())
    }

    /// Get failure detection result for a node.
    pub fn get_detection(&self, node_id: Uuid) -> HaResult<FailureDetection> {
        let heartbeat = self.heartbeats.get(&node_id);

        match heartbeat {
            Some(hb) => {
                let now = Utc::now();
                let elapsed = now - hb.timestamp;
                let time_since_heartbeat_ms = elapsed.num_milliseconds() as u64;

                let cfg = self.config.read();
                let is_failed = time_since_heartbeat_ms > cfg.heartbeat_timeout_ms;

                Ok(FailureDetection {
                    node_id,
                    is_failed,
                    last_heartbeat: Some(hb.timestamp),
                    time_since_heartbeat_ms: Some(time_since_heartbeat_ms),
                    health_status: hb.health_status,
                })
            }
            None => Ok(FailureDetection {
                node_id,
                is_failed: true,
                last_heartbeat: None,
                time_since_heartbeat_ms: None,
                health_status: HealthStatus::Unknown,
            }),
        }
    }

    /// Get all monitored nodes.
    pub fn get_monitored_nodes(&self) -> Vec<Uuid> {
        self.heartbeats.iter().map(|e| *e.key()).collect()
    }

    /// Get failure notification receiver.
    pub fn get_failure_receiver(&self) -> HaResult<mpsc::UnboundedReceiver<Uuid>> {
        let mut rx_guard = self.failure_rx.write();
        rx_guard
            .take()
            .ok_or_else(|| HaError::InvalidState("Failure receiver already taken".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_failure_detector() {
        let config = FailoverConfig::default();
        let detector = FailureDetector::new(config);

        assert!(detector.start().await.is_ok());

        let node_id = Uuid::new_v4();
        let heartbeat = Heartbeat {
            node_id,
            timestamp: Utc::now(),
            health_status: HealthStatus::Healthy,
            sequence: 1,
        };

        assert!(detector.record_heartbeat(heartbeat).is_ok());

        let detection = detector.get_detection(node_id).ok();
        assert!(detection.is_some());

        let detection = detection.expect("should have detection result for healthy node");
        assert!(!detection.is_failed);
        assert_eq!(detection.health_status, HealthStatus::Healthy);

        assert!(detector.stop().await.is_ok());
    }
}
