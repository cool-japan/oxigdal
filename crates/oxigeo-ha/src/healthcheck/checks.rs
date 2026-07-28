//! Built-in health checks.
//!
//! These checks inspect real signals rather than always reporting `Healthy`:
//!
//! - [`LivenessCheck`] reports the service dead when its heartbeat
//!   ([`LivenessSignal`]) has gone stale beyond a configured threshold.
//! - [`ReadinessCheck`] reflects a [`ReadinessGate`] the service flips once it
//!   has finished initialization.
//! - [`DependencyCheck`] actually attempts to reach a dependency through an
//!   injected [`DependencyProbe`] and reports `Unhealthy` when the probe fails.

use super::{HealthCheck, HealthCheckResult, HealthCheckType, HealthStatus};
use crate::error::HaResult;
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use parking_lot::RwLock;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A shared liveness heartbeat.
///
/// The service periodically calls [`beat`](LivenessSignal::beat); a
/// [`LivenessCheck`] holding a clone observes the last beat and reports the
/// service dead once it goes stale.
#[derive(Clone)]
pub struct LivenessSignal {
    last_beat: Arc<RwLock<DateTime<Utc>>>,
}

impl Default for LivenessSignal {
    fn default() -> Self {
        Self::new()
    }
}

impl LivenessSignal {
    /// Create a new signal whose initial beat is "now".
    pub fn new() -> Self {
        Self {
            last_beat: Arc::new(RwLock::new(Utc::now())),
        }
    }

    /// Record a heartbeat at the current instant.
    pub fn beat(&self) {
        *self.last_beat.write() = Utc::now();
    }

    /// The timestamp of the most recent heartbeat.
    pub fn last_beat(&self) -> DateTime<Utc> {
        *self.last_beat.read()
    }
}

/// A readiness gate flipped by the service once it has finished initializing.
#[derive(Clone, Default)]
pub struct ReadinessGate {
    ready: Arc<AtomicBool>,
}

impl ReadinessGate {
    /// Create a new gate that starts in the "not ready" state.
    pub fn new() -> Self {
        Self {
            ready: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Mark the service ready for traffic.
    pub fn mark_ready(&self) {
        self.ready.store(true, Ordering::SeqCst);
    }

    /// Mark the service not ready (e.g. during shutdown or reload).
    pub fn mark_not_ready(&self) {
        self.ready.store(false, Ordering::SeqCst);
    }

    /// Whether the service is currently ready.
    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::SeqCst)
    }
}

/// Probes whether a named dependency is currently reachable/healthy.
#[async_trait]
pub trait DependencyProbe: Send + Sync {
    /// Attempt to reach the dependency. Return `Ok(())` when healthy, or an
    /// error describing why it is unavailable.
    async fn probe(&self) -> HaResult<()>;
}

/// Liveness health check backed by a real heartbeat signal.
pub struct LivenessCheck {
    /// Check name.
    name: String,
    /// Shared heartbeat signal.
    signal: LivenessSignal,
    /// Maximum tolerated staleness before the service is considered dead.
    max_staleness: Duration,
}

impl LivenessCheck {
    /// Create a new liveness check observing `signal`, tolerating up to
    /// `max_staleness` since the last heartbeat.
    pub fn new(name: String, signal: LivenessSignal, max_staleness: Duration) -> Self {
        Self {
            name,
            signal,
            max_staleness,
        }
    }
}

#[async_trait]
impl HealthCheck for LivenessCheck {
    fn name(&self) -> &str {
        &self.name
    }

    fn check_type(&self) -> HealthCheckType {
        HealthCheckType::Liveness
    }

    async fn check(&self) -> HaResult<HealthCheckResult> {
        let start = Utc::now();
        let staleness = start - self.signal.last_beat();

        let (status, message) = if staleness <= self.max_staleness {
            (
                HealthStatus::Healthy,
                format!("Heartbeat fresh ({}ms ago)", staleness.num_milliseconds()),
            )
        } else {
            (
                HealthStatus::Unhealthy,
                format!(
                    "Heartbeat stale: {}ms since last beat exceeds {}ms",
                    staleness.num_milliseconds(),
                    self.max_staleness.num_milliseconds()
                ),
            )
        };

        let duration_ms = (Utc::now() - start).num_milliseconds().max(0) as u64;

        Ok(HealthCheckResult {
            name: self.name.clone(),
            check_type: HealthCheckType::Liveness,
            status,
            message: Some(message),
            timestamp: Utc::now(),
            duration_ms,
        })
    }
}

/// Readiness health check backed by a real readiness gate.
pub struct ReadinessCheck {
    /// Check name.
    name: String,
    /// Shared readiness gate.
    gate: ReadinessGate,
}

impl ReadinessCheck {
    /// Create a new readiness check observing `gate`.
    pub fn new(name: String, gate: ReadinessGate) -> Self {
        Self { name, gate }
    }
}

#[async_trait]
impl HealthCheck for ReadinessCheck {
    fn name(&self) -> &str {
        &self.name
    }

    fn check_type(&self) -> HealthCheckType {
        HealthCheckType::Readiness
    }

    async fn check(&self) -> HaResult<HealthCheckResult> {
        let start = Utc::now();

        let (status, message) = if self.gate.is_ready() {
            (HealthStatus::Healthy, "Service is ready".to_string())
        } else {
            (
                HealthStatus::Unhealthy,
                "Service has not finished initialization".to_string(),
            )
        };

        let duration_ms = (Utc::now() - start).num_milliseconds().max(0) as u64;

        Ok(HealthCheckResult {
            name: self.name.clone(),
            check_type: HealthCheckType::Readiness,
            status,
            message: Some(message),
            timestamp: Utc::now(),
            duration_ms,
        })
    }
}

/// Dependency health check that actually probes the dependency.
pub struct DependencyCheck {
    /// Check name.
    name: String,
    /// Dependency name.
    dependency: String,
    /// Probe used to reach the dependency.
    probe: Arc<dyn DependencyProbe>,
}

impl DependencyCheck {
    /// Create a new dependency check that reaches `dependency` via `probe`.
    pub fn new(name: String, dependency: String, probe: Arc<dyn DependencyProbe>) -> Self {
        Self {
            name,
            dependency,
            probe,
        }
    }
}

#[async_trait]
impl HealthCheck for DependencyCheck {
    fn name(&self) -> &str {
        &self.name
    }

    fn check_type(&self) -> HealthCheckType {
        HealthCheckType::Dependency
    }

    async fn check(&self) -> HaResult<HealthCheckResult> {
        let start = Utc::now();

        let (status, message) = match self.probe.probe().await {
            Ok(()) => (
                HealthStatus::Healthy,
                format!("Dependency {} is available", self.dependency),
            ),
            Err(e) => (
                HealthStatus::Unhealthy,
                format!("Dependency {} is unavailable: {}", self.dependency, e),
            ),
        };

        let duration_ms = (Utc::now() - start).num_milliseconds().max(0) as u64;

        Ok(HealthCheckResult {
            name: self.name.clone(),
            check_type: HealthCheckType::Dependency,
            status,
            message: Some(message),
            timestamp: Utc::now(),
            duration_ms,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::error::HaError;

    #[tokio::test]
    async fn test_liveness_healthy_when_fresh() {
        let signal = LivenessSignal::new();
        signal.beat();
        let check = LivenessCheck::new("live".to_string(), signal, Duration::seconds(5));
        let result = check.check().await.unwrap();
        assert_eq!(result.status, HealthStatus::Healthy);
    }

    #[tokio::test]
    async fn test_liveness_unhealthy_when_stale() {
        let signal = LivenessSignal::new();
        // Zero tolerance and a beat in the past → stale.
        {
            *signal.last_beat.write() = Utc::now() - Duration::seconds(30);
        }
        let check = LivenessCheck::new("live".to_string(), signal, Duration::seconds(5));
        let result = check.check().await.unwrap();
        assert_eq!(result.status, HealthStatus::Unhealthy);
    }

    #[tokio::test]
    async fn test_readiness_reflects_gate() {
        let gate = ReadinessGate::new();
        let check = ReadinessCheck::new("ready".to_string(), gate.clone());

        // Not ready initially.
        assert_eq!(check.check().await.unwrap().status, HealthStatus::Unhealthy);

        gate.mark_ready();
        assert_eq!(check.check().await.unwrap().status, HealthStatus::Healthy);

        gate.mark_not_ready();
        assert_eq!(check.check().await.unwrap().status, HealthStatus::Unhealthy);
    }

    struct AlwaysUp;
    #[async_trait]
    impl DependencyProbe for AlwaysUp {
        async fn probe(&self) -> HaResult<()> {
            Ok(())
        }
    }

    struct AlwaysDown;
    #[async_trait]
    impl DependencyProbe for AlwaysDown {
        async fn probe(&self) -> HaResult<()> {
            Err(HaError::Network("connection refused".to_string()))
        }
    }

    #[tokio::test]
    async fn test_dependency_healthy_when_probe_succeeds() {
        let check = DependencyCheck::new(
            "dep".to_string(),
            "postgres".to_string(),
            Arc::new(AlwaysUp),
        );
        let result = check.check().await.unwrap();
        assert_eq!(result.status, HealthStatus::Healthy);
    }

    #[tokio::test]
    async fn test_dependency_unhealthy_when_probe_fails() {
        let check = DependencyCheck::new(
            "dep".to_string(),
            "postgres".to_string(),
            Arc::new(AlwaysDown),
        );
        let result = check.check().await.unwrap();
        assert_eq!(result.status, HealthStatus::Unhealthy);
        assert!(
            result
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("unavailable")
        );
    }
}
