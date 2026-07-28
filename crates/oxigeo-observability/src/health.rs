//! Health check and status monitoring.

use crate::error::Result;
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Health check manager.
pub struct HealthCheckManager {
    checks: Arc<RwLock<HashMap<String, Box<dyn HealthCheck + Send + Sync>>>>,
    status: Arc<RwLock<HealthStatus>>,
}

/// Health check trait.
pub trait HealthCheck: Send + Sync {
    /// Perform health check.
    fn check(&self) -> Result<CheckResult>;

    /// Get check name.
    fn name(&self) -> &str;
}

/// Health check result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    /// Name of the health check.
    pub name: String,
    /// Status of the checked component.
    pub status: ComponentStatus,
    /// Optional message with additional details.
    pub message: Option<String>,
    /// Timestamp when the check was performed.
    pub checked_at: DateTime<Utc>,
    /// Duration of the health check in milliseconds.
    pub duration_ms: f64,
}

/// Component health status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComponentStatus {
    /// Component is operating normally.
    Healthy,
    /// Component is operational but with reduced performance.
    Degraded,
    /// Component is not operational.
    Unhealthy,
}

/// Overall health status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    /// Overall status of the system.
    pub status: ComponentStatus,
    /// Results from individual health checks.
    pub checks: Vec<CheckResult>,
    /// Timestamp when the status was evaluated.
    pub checked_at: DateTime<Utc>,
}

impl HealthCheckManager {
    /// Create a new health check manager.
    pub fn new() -> Self {
        Self {
            checks: Arc::new(RwLock::new(HashMap::new())),
            status: Arc::new(RwLock::new(HealthStatus {
                status: ComponentStatus::Healthy,
                checks: Vec::new(),
                checked_at: Utc::now(),
            })),
        }
    }

    /// Register a health check.
    pub fn register(&self, check: Box<dyn HealthCheck + Send + Sync>) {
        let name = check.name().to_string();
        self.checks.write().insert(name, check);
    }

    /// Run all health checks.
    pub fn check_all(&self) -> Result<HealthStatus> {
        let checks = self.checks.read();
        let mut results = Vec::new();
        let mut overall_status = ComponentStatus::Healthy;

        for check in checks.values() {
            let result = check.check()?;

            match result.status {
                ComponentStatus::Unhealthy => overall_status = ComponentStatus::Unhealthy,
                ComponentStatus::Degraded if overall_status == ComponentStatus::Healthy => {
                    overall_status = ComponentStatus::Degraded
                }
                _ => {}
            }

            results.push(result);
        }

        let status = HealthStatus {
            status: overall_status,
            checks: results,
            checked_at: Utc::now(),
        };

        *self.status.write() = status.clone();
        Ok(status)
    }

    /// Get cached health status.
    pub fn get_status(&self) -> HealthStatus {
        self.status.read().clone()
    }
}

impl Default for HealthCheckManager {
    fn default() -> Self {
        Self::new()
    }
}

/// A real connectivity probe that can be injected into [`DatabaseHealthCheck`]
/// or [`CacheHealthCheck`].
///
/// This crate has no built-in database/cache client (it is a generic
/// observability library used across drivers with very different backing
/// stores), so it cannot open a real connection itself. Instead, callers
/// inject a small probe closure/type that performs the actual round-trip
/// (e.g. `SELECT 1` for a SQL database, or `PING` for a cache) against their
/// own already-configured connection/pool. This keeps the health check
/// honest: if no probe is supplied, [`DatabaseHealthCheck`]/[`CacheHealthCheck`]
/// report [`ComponentStatus::Unhealthy`] rather than fabricating success.
pub trait ConnectivityChecker: Send + Sync {
    /// Perform a real connectivity probe. Return `Ok(())` if the round-trip
    /// succeeded, or `Err` with a human-readable reason otherwise.
    fn ping(&self) -> std::result::Result<(), String>;
}

impl<F> ConnectivityChecker for F
where
    F: Fn() -> std::result::Result<(), String> + Send + Sync,
{
    fn ping(&self) -> std::result::Result<(), String> {
        (self)()
    }
}

/// Database health check.
///
/// Performs a real connectivity probe by delegating to an injected
/// [`ConnectivityChecker`] (see that trait's docs for why this crate cannot
/// open a database connection on its own). If no checker is configured, the
/// check honestly reports [`ComponentStatus::Unhealthy`] rather than
/// fabricating a healthy result.
pub struct DatabaseHealthCheck {
    name: String,
    checker: Option<Arc<dyn ConnectivityChecker>>,
}

impl DatabaseHealthCheck {
    /// Create a new database health check with no real probe configured.
    ///
    /// Until [`Self::with_checker`] (or [`Self::new_with_checker`]) is used to
    /// inject a real connectivity probe, [`HealthCheck::check`] reports
    /// [`ComponentStatus::Unhealthy`] with an explanatory message, since there
    /// is no honest way to claim the database is reachable without probing it.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            checker: None,
        }
    }

    /// Create a new database health check backed by a real connectivity
    /// probe (e.g. a closure that runs `SELECT 1` against a connection pool).
    pub fn new_with_checker(
        name: impl Into<String>,
        checker: Arc<dyn ConnectivityChecker>,
    ) -> Self {
        Self {
            name: name.into(),
            checker: Some(checker),
        }
    }

    /// Attach (or replace) the connectivity probe used by this check.
    #[must_use]
    pub fn with_checker(mut self, checker: Arc<dyn ConnectivityChecker>) -> Self {
        self.checker = Some(checker);
        self
    }
}

impl HealthCheck for DatabaseHealthCheck {
    fn check(&self) -> Result<CheckResult> {
        let start = std::time::Instant::now();

        let (status, message) = match &self.checker {
            Some(checker) => match checker.ping() {
                Ok(()) => (
                    ComponentStatus::Healthy,
                    "Database connectivity probe succeeded".to_string(),
                ),
                Err(reason) => (
                    ComponentStatus::Unhealthy,
                    format!("Database connectivity probe failed: {reason}"),
                ),
            },
            None => (
                ComponentStatus::Unhealthy,
                "No ConnectivityChecker configured: cannot verify database connectivity \
                 (use DatabaseHealthCheck::new_with_checker/with_checker to wire in a real probe)"
                    .to_string(),
            ),
        };

        Ok(CheckResult {
            name: self.name.clone(),
            status,
            message: Some(message),
            checked_at: Utc::now(),
            duration_ms: start.elapsed().as_secs_f64() * 1000.0,
        })
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// Cache health check.
///
/// Performs a real connectivity probe by delegating to an injected
/// [`ConnectivityChecker`] (e.g. a `PING`/round-trip against the cache
/// client). If no checker is configured, the check honestly reports
/// [`ComponentStatus::Unhealthy`] rather than fabricating a healthy result.
pub struct CacheHealthCheck {
    name: String,
    checker: Option<Arc<dyn ConnectivityChecker>>,
}

impl CacheHealthCheck {
    /// Create a new cache health check with no real probe configured.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            checker: None,
        }
    }

    /// Create a new cache health check backed by a real connectivity probe.
    pub fn new_with_checker(
        name: impl Into<String>,
        checker: Arc<dyn ConnectivityChecker>,
    ) -> Self {
        Self {
            name: name.into(),
            checker: Some(checker),
        }
    }

    /// Attach (or replace) the connectivity probe used by this check.
    #[must_use]
    pub fn with_checker(mut self, checker: Arc<dyn ConnectivityChecker>) -> Self {
        self.checker = Some(checker);
        self
    }
}

impl HealthCheck for CacheHealthCheck {
    fn check(&self) -> Result<CheckResult> {
        let start = std::time::Instant::now();

        let (status, message) = match &self.checker {
            Some(checker) => match checker.ping() {
                Ok(()) => (
                    ComponentStatus::Healthy,
                    "Cache connectivity probe succeeded".to_string(),
                ),
                Err(reason) => (
                    ComponentStatus::Unhealthy,
                    format!("Cache connectivity probe failed: {reason}"),
                ),
            },
            None => (
                ComponentStatus::Unhealthy,
                "No ConnectivityChecker configured: cannot verify cache connectivity \
                 (use CacheHealthCheck::new_with_checker/with_checker to wire in a real probe)"
                    .to_string(),
            ),
        };

        Ok(CheckResult {
            name: self.name.clone(),
            status,
            message: Some(message),
            checked_at: Utc::now(),
            duration_ms: start.elapsed().as_secs_f64() * 1000.0,
        })
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// Disk space health check.
///
/// Uses [`sysinfo::Disks`] to find the real filesystem mounted at (or
/// containing) `path` and computes actual usage from its reported
/// total/available space.
pub struct DiskSpaceHealthCheck {
    name: String,
    path: std::path::PathBuf,
    warning_threshold: f64,
    critical_threshold: f64,
}

impl DiskSpaceHealthCheck {
    /// Create a new disk space health check.
    pub fn new(
        name: impl Into<String>,
        path: impl Into<String>,
        warning_threshold: f64,
        critical_threshold: f64,
    ) -> Self {
        Self {
            name: name.into(),
            path: std::path::PathBuf::from(path.into()),
            warning_threshold,
            critical_threshold,
        }
    }

    /// Find the disk whose mount point is the longest matching prefix of
    /// `path`, returning `(total_space, available_space)` in bytes.
    ///
    /// Returns `None` if no disk information could be gathered for the path
    /// (e.g. an unsupported platform or a path that doesn't exist on any
    /// known mount point).
    fn disk_usage_bytes(&self) -> Option<(u64, u64)> {
        let path = self
            .path
            .canonicalize()
            .unwrap_or_else(|_| self.path.clone());

        let disks = sysinfo::Disks::new_with_refreshed_list();
        let mut best_match: Option<(&std::path::Path, u64, u64)> = None;

        for disk in disks.list() {
            let mount_point = disk.mount_point();
            if path.starts_with(mount_point) {
                let is_better = match best_match {
                    Some((current_best, _, _)) => {
                        mount_point.as_os_str().len() > current_best.as_os_str().len()
                    }
                    None => true,
                };
                if is_better {
                    best_match = Some((mount_point, disk.total_space(), disk.available_space()));
                }
            }
        }

        best_match.map(|(_, total, available)| (total, available))
    }
}

impl HealthCheck for DiskSpaceHealthCheck {
    fn check(&self) -> Result<CheckResult> {
        let start = std::time::Instant::now();

        let (status, message) = match self.disk_usage_bytes() {
            Some((total, available)) if total > 0 => {
                let used = total.saturating_sub(available);
                let usage_percent = (used as f64 / total as f64) * 100.0;

                let status = if usage_percent >= self.critical_threshold {
                    ComponentStatus::Unhealthy
                } else if usage_percent >= self.warning_threshold {
                    ComponentStatus::Degraded
                } else {
                    ComponentStatus::Healthy
                };

                (status, format!("Disk usage: {usage_percent:.1}%"))
            }
            _ => (
                ComponentStatus::Unhealthy,
                format!(
                    "Could not determine disk usage for path '{}': no matching mount point found \
                     via sysinfo::Disks",
                    self.path.display()
                ),
            ),
        };

        Ok(CheckResult {
            name: self.name.clone(),
            status,
            message: Some(message),
            checked_at: Utc::now(),
            duration_ms: start.elapsed().as_secs_f64() * 1000.0,
        })
    }

    fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_check_manager() {
        let manager = HealthCheckManager::new();

        manager.register(Box::new(DatabaseHealthCheck::new_with_checker(
            "postgres",
            Arc::new(|| Ok(())),
        )));
        manager.register(Box::new(CacheHealthCheck::new_with_checker(
            "redis",
            Arc::new(|| Ok(())),
        )));

        let status = manager.check_all().expect("Health check failed");
        assert_eq!(status.checks.len(), 2);
        assert_eq!(status.status, ComponentStatus::Healthy);
    }

    #[test]
    fn test_database_health_check_without_checker_is_unhealthy() {
        // No injected checker: must NOT fabricate a Healthy result.
        let check = DatabaseHealthCheck::new("postgres");
        let result = check.check().expect("check should not error");
        assert_eq!(result.status, ComponentStatus::Unhealthy);
        assert!(
            result
                .message
                .expect("message")
                .contains("No ConnectivityChecker")
        );
    }

    #[test]
    fn test_database_health_check_reports_real_failure() {
        let checker: Arc<dyn ConnectivityChecker> =
            Arc::new(|| Err("connection refused".to_string()));
        let check = DatabaseHealthCheck::new("postgres").with_checker(checker);
        let result = check.check().expect("check should not error");
        assert_eq!(result.status, ComponentStatus::Unhealthy);
        assert!(
            result
                .message
                .expect("message")
                .contains("connection refused")
        );
    }

    #[test]
    fn test_cache_health_check_without_checker_is_unhealthy() {
        let check = CacheHealthCheck::new("redis");
        let result = check.check().expect("check should not error");
        assert_eq!(result.status, ComponentStatus::Unhealthy);
    }

    #[test]
    fn test_cache_health_check_success() {
        let checker: Arc<dyn ConnectivityChecker> = Arc::new(|| Ok(()));
        let check = CacheHealthCheck::new("redis").with_checker(checker);
        let result = check.check().expect("check should not error");
        assert_eq!(result.status, ComponentStatus::Healthy);
    }

    #[test]
    fn test_disk_space_check_reports_real_usage() {
        // "/" always exists and is always resolvable to a mount point on
        // every platform sysinfo supports (unix root, or a drive letter on
        // Windows after path normalization). Thresholds are set to 100/100
        // so the check can never spuriously report Unhealthy/Degraded due to
        // the *actual* disk fill level of the CI machine -- what we are
        // verifying here is that a real (non-hardcoded) percentage was
        // computed at all.
        let check = DiskSpaceHealthCheck::new("root", "/", 100.0, 100.1);
        let result = check.check().expect("Check failed");
        let message = result.message.expect("message");
        assert!(
            message.starts_with("Disk usage:"),
            "expected a real usage message, got: {message}"
        );
        // The hardcoded-50.0 bug this test used to validate is gone: assert
        // we don't just always print exactly 50.0% by construction (a real
        // disk essentially never sits at exactly that value to float
        // precision, so this also guards against silent regressions).
        assert_ne!(message, "Disk usage: 50.0%");
    }

    #[test]
    fn test_disk_space_check_nonexistent_path_reports_status_without_panicking() {
        // A path that doesn't exist anywhere still resolves to *some* mount
        // point on Unix (the check walks up to the root "/"), so this should
        // not error -- it must simply not fabricate data.
        let check = DiskSpaceHealthCheck::new(
            "bogus",
            "/this/path/almost-certainly-does-not-exist-oxigeo-test",
            80.0,
            95.0,
        );
        let result = check.check();
        assert!(result.is_ok());
    }
}
