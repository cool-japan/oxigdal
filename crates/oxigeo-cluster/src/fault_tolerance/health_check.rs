//! Health check mechanisms for fault tolerance.
//!
//! Provides comprehensive health checking including:
//! - Liveness checks (is the service alive?)
//! - Readiness checks (is the service ready to serve traffic?)
//! - Dependency health checks
//! - Composite health aggregation

use crate::error::{ClusterError, Result};
use async_trait::async_trait;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Notify;
use tracing::{debug, info, warn};

/// Health status of a component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    /// Component is healthy and functioning normally
    Healthy,
    /// Component is degraded but still functional
    Degraded,
    /// Component is unhealthy and not functioning
    Unhealthy,
    /// Component health is unknown (not yet checked)
    Unknown,
}

impl std::fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Healthy => write!(f, "Healthy"),
            Self::Degraded => write!(f, "Degraded"),
            Self::Unhealthy => write!(f, "Unhealthy"),
            Self::Unknown => write!(f, "Unknown"),
        }
    }
}

impl HealthStatus {
    /// Check if the status is considered healthy (healthy or degraded).
    pub fn is_healthy(&self) -> bool {
        matches!(self, Self::Healthy | Self::Degraded)
    }

    /// Check if the status indicates a problem.
    pub fn is_problematic(&self) -> bool {
        matches!(self, Self::Unhealthy | Self::Unknown)
    }

    /// Combine two health statuses (worst wins).
    pub fn combine(&self, other: &Self) -> Self {
        match (self, other) {
            (Self::Unhealthy, _) | (_, Self::Unhealthy) => Self::Unhealthy,
            (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
            (Self::Degraded, _) | (_, Self::Degraded) => Self::Degraded,
            _ => Self::Healthy,
        }
    }
}

/// Health check result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResult {
    /// Component name
    pub name: String,
    /// Health status
    pub status: HealthStatus,
    /// Optional message
    pub message: Option<String>,
    /// Check duration
    pub duration_ms: u64,
    /// Timestamp of check
    pub checked_at: chrono::DateTime<chrono::Utc>,
    /// Additional details
    pub details: HashMap<String, String>,
}

impl HealthCheckResult {
    /// Create a healthy result.
    pub fn healthy(name: &str, duration: Duration) -> Self {
        Self {
            name: name.to_string(),
            status: HealthStatus::Healthy,
            message: None,
            duration_ms: duration.as_millis() as u64,
            checked_at: chrono::Utc::now(),
            details: HashMap::new(),
        }
    }

    /// Create a degraded result.
    pub fn degraded(name: &str, message: &str, duration: Duration) -> Self {
        Self {
            name: name.to_string(),
            status: HealthStatus::Degraded,
            message: Some(message.to_string()),
            duration_ms: duration.as_millis() as u64,
            checked_at: chrono::Utc::now(),
            details: HashMap::new(),
        }
    }

    /// Create an unhealthy result.
    pub fn unhealthy(name: &str, message: &str, duration: Duration) -> Self {
        Self {
            name: name.to_string(),
            status: HealthStatus::Unhealthy,
            message: Some(message.to_string()),
            duration_ms: duration.as_millis() as u64,
            checked_at: chrono::Utc::now(),
            details: HashMap::new(),
        }
    }

    /// Add a detail to the result.
    pub fn with_detail(mut self, key: &str, value: &str) -> Self {
        self.details.insert(key.to_string(), value.to_string());
        self
    }
}

/// Trait for implementing health checks.
#[async_trait]
pub trait HealthCheck: Send + Sync {
    /// Get the name of this health check.
    fn name(&self) -> &str;

    /// Perform the health check.
    async fn check(&self) -> HealthCheckResult;

    /// Check if this is a critical dependency.
    fn is_critical(&self) -> bool {
        false
    }
}

/// Health check configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckConfig {
    /// Check interval
    pub interval: Duration,
    /// Timeout for individual checks
    pub timeout: Duration,
    /// Number of consecutive failures before marking unhealthy
    pub failure_threshold: u32,
    /// Number of consecutive successes before marking healthy
    pub success_threshold: u32,
    /// Enable automatic recovery attempts
    pub auto_recovery: bool,
    /// Enable periodic background checks
    pub background_checks: bool,
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(30),
            timeout: Duration::from_secs(10),
            failure_threshold: 3,
            success_threshold: 2,
            auto_recovery: true,
            background_checks: true,
        }
    }
}

/// State tracking for a health check.
struct HealthCheckState {
    /// Current status
    status: HealthStatus,
    /// Last check result
    last_result: Option<HealthCheckResult>,
    /// Consecutive failures
    consecutive_failures: u32,
    /// Consecutive successes
    consecutive_successes: u32,
    /// Last check time
    last_check: Option<Instant>,
}

impl Default for HealthCheckState {
    fn default() -> Self {
        Self {
            status: HealthStatus::Unknown,
            last_result: None,
            consecutive_failures: 0,
            consecutive_successes: 0,
            last_check: None,
        }
    }
}

/// Health check statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HealthCheckStats {
    /// Total checks performed
    pub total_checks: u64,
    /// Total healthy checks
    pub total_healthy: u64,
    /// Total degraded checks
    pub total_degraded: u64,
    /// Total unhealthy checks
    pub total_unhealthy: u64,
    /// Average check duration in milliseconds
    pub avg_check_duration_ms: u64,
    /// Number of registered health checks
    pub registered_checks: usize,
}

/// Health check manager for managing multiple health checks.
pub struct HealthCheckManager {
    inner: Arc<HealthCheckManagerInner>,
}

struct HealthCheckManagerInner {
    /// Configuration
    config: HealthCheckConfig,
    /// Registered health checks
    checks: RwLock<HashMap<String, Arc<dyn HealthCheck>>>,
    /// Health check state
    state: RwLock<HashMap<String, HealthCheckState>>,
    /// Statistics
    stats: RwLock<HealthCheckStats>,
    /// Shutdown notification
    shutdown: Notify,
    /// Running flag
    running: RwLock<bool>,
}

impl HealthCheckManager {
    /// Create a new health check manager.
    pub fn new(config: HealthCheckConfig) -> Self {
        Self {
            inner: Arc::new(HealthCheckManagerInner {
                config,
                checks: RwLock::new(HashMap::new()),
                state: RwLock::new(HashMap::new()),
                stats: RwLock::new(HealthCheckStats::default()),
                shutdown: Notify::new(),
                running: RwLock::new(false),
            }),
        }
    }

    /// Create with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(HealthCheckConfig::default())
    }

    /// Register a health check.
    pub fn register(&self, check: Arc<dyn HealthCheck>) {
        let name = check.name().to_string();
        self.inner.checks.write().insert(name.clone(), check);
        self.inner
            .state
            .write()
            .insert(name.clone(), HealthCheckState::default());
        self.inner.stats.write().registered_checks = self.inner.checks.read().len();

        debug!("Registered health check: {}", name);
    }

    /// Unregister a health check.
    pub fn unregister(&self, name: &str) {
        self.inner.checks.write().remove(name);
        self.inner.state.write().remove(name);
        self.inner.stats.write().registered_checks = self.inner.checks.read().len();

        debug!("Unregistered health check: {}", name);
    }

    /// Run a single health check by name.
    pub async fn check(&self, name: &str) -> Result<HealthCheckResult> {
        let check = self.inner.checks.read().get(name).cloned().ok_or_else(|| {
            ClusterError::InvalidOperation(format!("Health check not found: {}", name))
        })?;

        let start = Instant::now();
        let result = tokio::time::timeout(self.inner.config.timeout, check.check()).await;

        let result = match result {
            Ok(result) => result,
            Err(_) => HealthCheckResult::unhealthy(name, "Health check timed out", start.elapsed()),
        };

        self.update_state(name, &result);
        self.update_stats(&result);

        Ok(result)
    }

    /// Run all health checks.
    pub async fn check_all(&self) -> Vec<HealthCheckResult> {
        let checks: Vec<_> = self.inner.checks.read().clone().into_iter().collect();
        let mut results = Vec::with_capacity(checks.len());

        for (name, check) in checks {
            let start = Instant::now();
            let result = tokio::time::timeout(self.inner.config.timeout, check.check()).await;

            let result = match result {
                Ok(result) => result,
                Err(_) => {
                    HealthCheckResult::unhealthy(&name, "Health check timed out", start.elapsed())
                }
            };

            self.update_state(&name, &result);
            self.update_stats(&result);
            results.push(result);
        }

        results
    }

    /// Get aggregate health status.
    pub fn aggregate_status(&self) -> HealthStatus {
        let state = self.inner.state.read();
        let checks = self.inner.checks.read();

        let mut aggregate = HealthStatus::Healthy;

        for (name, check_state) in state.iter() {
            let is_critical = checks.get(name).map(|c| c.is_critical()).unwrap_or(false);

            if is_critical && check_state.status == HealthStatus::Unhealthy {
                return HealthStatus::Unhealthy;
            }

            aggregate = aggregate.combine(&check_state.status);
        }

        aggregate
    }

    /// Get liveness status (is the service alive?).
    pub fn is_alive(&self) -> bool {
        // Liveness is a simple check - if we can respond, we're alive
        true
    }

    /// Get readiness status (is the service ready to serve traffic?).
    pub fn is_ready(&self) -> bool {
        let status = self.aggregate_status();
        status.is_healthy()
    }

    /// Get health status for a specific check.
    pub fn get_status(&self, name: &str) -> Option<HealthStatus> {
        self.inner.state.read().get(name).map(|s| s.status)
    }

    /// Get last result for a specific check.
    pub fn get_last_result(&self, name: &str) -> Option<HealthCheckResult> {
        self.inner
            .state
            .read()
            .get(name)
            .and_then(|s| s.last_result.clone())
    }

    /// Get all health check results.
    pub fn get_all_results(&self) -> HashMap<String, HealthCheckResult> {
        self.inner
            .state
            .read()
            .iter()
            .filter_map(|(name, state)| state.last_result.clone().map(|r| (name.clone(), r)))
            .collect()
    }

    /// Update state after a health check.
    fn update_state(&self, name: &str, result: &HealthCheckResult) {
        let mut state = self.inner.state.write();
        if let Some(check_state) = state.get_mut(name) {
            check_state.last_result = Some(result.clone());
            check_state.last_check = Some(Instant::now());

            match result.status {
                HealthStatus::Healthy => {
                    check_state.consecutive_failures = 0;
                    check_state.consecutive_successes += 1;

                    if check_state.consecutive_successes >= self.inner.config.success_threshold {
                        check_state.status = HealthStatus::Healthy;
                    }
                }
                HealthStatus::Degraded => {
                    check_state.consecutive_failures = 0;
                    check_state.consecutive_successes = 0;
                    check_state.status = HealthStatus::Degraded;
                }
                HealthStatus::Unhealthy => {
                    check_state.consecutive_successes = 0;
                    check_state.consecutive_failures += 1;

                    if check_state.consecutive_failures >= self.inner.config.failure_threshold {
                        check_state.status = HealthStatus::Unhealthy;
                    }
                }
                HealthStatus::Unknown => {
                    // Keep previous state
                }
            }
        }
    }

    /// Update statistics after a health check.
    fn update_stats(&self, result: &HealthCheckResult) {
        let mut stats = self.inner.stats.write();
        stats.total_checks += 1;

        match result.status {
            HealthStatus::Healthy => stats.total_healthy += 1,
            HealthStatus::Degraded => stats.total_degraded += 1,
            HealthStatus::Unhealthy | HealthStatus::Unknown => stats.total_unhealthy += 1,
        }

        // Update average duration
        let total_duration =
            stats.avg_check_duration_ms * (stats.total_checks - 1) + result.duration_ms;
        stats.avg_check_duration_ms = total_duration / stats.total_checks;
    }

    /// Start background health checks.
    pub async fn start(&self) {
        if !self.inner.config.background_checks {
            return;
        }

        {
            let mut running = self.inner.running.write();
            if *running {
                return;
            }
            *running = true;
        }

        info!("Starting background health checks");

        let inner = Arc::clone(&self.inner);
        let interval = inner.config.interval;

        tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);

            loop {
                tokio::select! {
                    _ = inner.shutdown.notified() => {
                        info!("Stopping background health checks");
                        break;
                    }
                    _ = interval_timer.tick() => {
                        let checks: Vec<_> = inner.checks.read().values().cloned().collect();

                        for check in checks {
                            let name = check.name().to_string();
                            let result = tokio::time::timeout(
                                inner.config.timeout,
                                check.check()
                            ).await;

                            let result = match result {
                                Ok(r) => r,
                                Err(_) => HealthCheckResult::unhealthy(
                                    &name,
                                    "Health check timed out",
                                    inner.config.timeout,
                                ),
                            };

                            // Update state
                            {
                                let mut state = inner.state.write();
                                if let Some(check_state) = state.get_mut(&name) {
                                    check_state.last_result = Some(result.clone());
                                    check_state.last_check = Some(Instant::now());

                                    match result.status {
                                        HealthStatus::Healthy => {
                                            check_state.consecutive_failures = 0;
                                            check_state.consecutive_successes += 1;
                                            if check_state.consecutive_successes >= inner.config.success_threshold {
                                                check_state.status = HealthStatus::Healthy;
                                            }
                                        }
                                        HealthStatus::Degraded => {
                                            check_state.status = HealthStatus::Degraded;
                                        }
                                        HealthStatus::Unhealthy => {
                                            check_state.consecutive_successes = 0;
                                            check_state.consecutive_failures += 1;
                                            if check_state.consecutive_failures >= inner.config.failure_threshold {
                                                check_state.status = HealthStatus::Unhealthy;
                                                warn!("Health check {} is unhealthy", name);
                                            }
                                        }
                                        HealthStatus::Unknown => {}
                                    }
                                }
                            }

                            // Update stats
                            {
                                let mut stats = inner.stats.write();
                                stats.total_checks += 1;
                                match result.status {
                                    HealthStatus::Healthy => stats.total_healthy += 1,
                                    HealthStatus::Degraded => stats.total_degraded += 1,
                                    _ => stats.total_unhealthy += 1,
                                }
                            }
                        }
                    }
                }
            }
        });
    }

    /// Stop background health checks.
    pub fn stop(&self) {
        *self.inner.running.write() = false;
        self.inner.shutdown.notify_one();
        info!("Stopped background health checks");
    }

    /// Get statistics.
    pub fn get_stats(&self) -> HealthCheckStats {
        self.inner.stats.read().clone()
    }
}

impl Clone for HealthCheckManager {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

/// Simple function-based health check.
pub struct FunctionHealthCheck<F>
where
    F: Fn() -> HealthCheckResult + Send + Sync,
{
    name: String,
    check_fn: F,
    critical: bool,
}

impl<F> FunctionHealthCheck<F>
where
    F: Fn() -> HealthCheckResult + Send + Sync,
{
    /// Create a new function-based health check.
    pub fn new(name: &str, check_fn: F) -> Self {
        Self {
            name: name.to_string(),
            check_fn,
            critical: false,
        }
    }

    /// Mark this check as critical.
    pub fn critical(mut self) -> Self {
        self.critical = true;
        self
    }
}

#[async_trait]
impl<F> HealthCheck for FunctionHealthCheck<F>
where
    F: Fn() -> HealthCheckResult + Send + Sync,
{
    fn name(&self) -> &str {
        &self.name
    }

    async fn check(&self) -> HealthCheckResult {
        (self.check_fn)()
    }

    fn is_critical(&self) -> bool {
        self.critical
    }
}

/// Composite health check that aggregates multiple checks.
pub struct CompositeHealthCheck {
    name: String,
    checks: Vec<Arc<dyn HealthCheck>>,
    critical: bool,
}

impl CompositeHealthCheck {
    /// Create a new composite health check.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            checks: Vec::new(),
            critical: false,
        }
    }

    /// Add a health check to the composite.
    pub fn with_check(mut self, check: Arc<dyn HealthCheck>) -> Self {
        self.checks.push(check);
        self
    }

    /// Add a health check to the composite (builder pattern).
    #[allow(clippy::should_implement_trait)]
    pub fn add(mut self, check: Arc<dyn HealthCheck>) -> Self {
        self.checks.push(check);
        self
    }

    /// Mark as critical.
    pub fn critical(mut self) -> Self {
        self.critical = true;
        self
    }
}

#[async_trait]
impl HealthCheck for CompositeHealthCheck {
    fn name(&self) -> &str {
        &self.name
    }

    async fn check(&self) -> HealthCheckResult {
        let start = Instant::now();
        let mut aggregate_status = HealthStatus::Healthy;
        let mut details = HashMap::new();

        for check in &self.checks {
            let result = check.check().await;
            details.insert(result.name.clone(), result.status.to_string());
            aggregate_status = aggregate_status.combine(&result.status);
        }

        let duration = start.elapsed();

        let mut result = match aggregate_status {
            HealthStatus::Healthy => HealthCheckResult::healthy(&self.name, duration),
            HealthStatus::Degraded => {
                HealthCheckResult::degraded(&self.name, "Some components degraded", duration)
            }
            HealthStatus::Unhealthy => {
                HealthCheckResult::unhealthy(&self.name, "Some components unhealthy", duration)
            }
            HealthStatus::Unknown => {
                HealthCheckResult::unhealthy(&self.name, "Unknown component status", duration)
            }
        };

        result.details = details;
        result
    }

    fn is_critical(&self) -> bool {
        self.critical
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    struct AlwaysHealthyCheck;

    #[async_trait]
    impl HealthCheck for AlwaysHealthyCheck {
        fn name(&self) -> &str {
            "always_healthy"
        }

        async fn check(&self) -> HealthCheckResult {
            HealthCheckResult::healthy("always_healthy", Duration::from_millis(1))
        }
    }

    struct AlwaysUnhealthyCheck;

    #[async_trait]
    impl HealthCheck for AlwaysUnhealthyCheck {
        fn name(&self) -> &str {
            "always_unhealthy"
        }

        async fn check(&self) -> HealthCheckResult {
            HealthCheckResult::unhealthy(
                "always_unhealthy",
                "Test failure",
                Duration::from_millis(1),
            )
        }

        fn is_critical(&self) -> bool {
            true
        }
    }

    #[tokio::test]
    async fn test_health_check_manager_creation() {
        let manager = HealthCheckManager::with_defaults();
        assert_eq!(manager.aggregate_status(), HealthStatus::Healthy);
    }

    #[tokio::test]
    async fn test_register_health_check() {
        let manager = HealthCheckManager::with_defaults();
        manager.register(Arc::new(AlwaysHealthyCheck));

        let result = manager.check("always_healthy").await;
        assert!(result.is_ok());
        assert_eq!(result.ok().map(|r| r.status), Some(HealthStatus::Healthy));
    }

    #[tokio::test]
    async fn test_unhealthy_check() {
        let config = HealthCheckConfig {
            failure_threshold: 1,
            ..Default::default()
        };
        let manager = HealthCheckManager::new(config);
        manager.register(Arc::new(AlwaysUnhealthyCheck));

        let _ = manager.check("always_unhealthy").await;

        assert_eq!(manager.aggregate_status(), HealthStatus::Unhealthy);
    }

    #[tokio::test]
    async fn test_check_all() {
        let manager = HealthCheckManager::with_defaults();
        manager.register(Arc::new(AlwaysHealthyCheck));

        let results = manager.check_all().await;
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_health_status_combine() {
        assert_eq!(
            HealthStatus::Healthy.combine(&HealthStatus::Healthy),
            HealthStatus::Healthy
        );
        assert_eq!(
            HealthStatus::Healthy.combine(&HealthStatus::Degraded),
            HealthStatus::Degraded
        );
        assert_eq!(
            HealthStatus::Healthy.combine(&HealthStatus::Unhealthy),
            HealthStatus::Unhealthy
        );
        assert_eq!(
            HealthStatus::Degraded.combine(&HealthStatus::Unhealthy),
            HealthStatus::Unhealthy
        );
    }

    #[test]
    fn test_liveness_readiness() {
        let manager = HealthCheckManager::with_defaults();
        assert!(manager.is_alive());
        assert!(manager.is_ready());
    }

    #[tokio::test]
    async fn test_composite_health_check() {
        let composite = CompositeHealthCheck::new("composite").add(Arc::new(AlwaysHealthyCheck));

        let result = composite.check().await;
        assert_eq!(result.status, HealthStatus::Healthy);
    }
}
