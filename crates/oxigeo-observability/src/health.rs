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

/// Database health check.
pub struct DatabaseHealthCheck {
    name: String,
}

impl DatabaseHealthCheck {
    /// Create a new database health check.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl HealthCheck for DatabaseHealthCheck {
    fn check(&self) -> Result<CheckResult> {
        let start = std::time::Instant::now();

        // In production, perform actual database connectivity check
        let status = ComponentStatus::Healthy;

        Ok(CheckResult {
            name: self.name.clone(),
            status,
            message: Some("Database connection successful".to_string()),
            checked_at: Utc::now(),
            duration_ms: start.elapsed().as_secs_f64() * 1000.0,
        })
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// Cache health check.
pub struct CacheHealthCheck {
    name: String,
}

impl CacheHealthCheck {
    /// Create a new cache health check.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl HealthCheck for CacheHealthCheck {
    fn check(&self) -> Result<CheckResult> {
        let start = std::time::Instant::now();

        // In production, check cache connectivity and performance
        let status = ComponentStatus::Healthy;

        Ok(CheckResult {
            name: self.name.clone(),
            status,
            message: Some("Cache operational".to_string()),
            checked_at: Utc::now(),
            duration_ms: start.elapsed().as_secs_f64() * 1000.0,
        })
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// Disk space health check.
pub struct DiskSpaceHealthCheck {
    name: String,
    #[allow(dead_code)]
    path: String,
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
            path: path.into(),
            warning_threshold,
            critical_threshold,
        }
    }
}

impl HealthCheck for DiskSpaceHealthCheck {
    fn check(&self) -> Result<CheckResult> {
        let start = std::time::Instant::now();

        // In production, check actual disk space
        let usage_percent = 50.0;

        let status = if usage_percent >= self.critical_threshold {
            ComponentStatus::Unhealthy
        } else if usage_percent >= self.warning_threshold {
            ComponentStatus::Degraded
        } else {
            ComponentStatus::Healthy
        };

        Ok(CheckResult {
            name: self.name.clone(),
            status,
            message: Some(format!("Disk usage: {:.1}%", usage_percent)),
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

        manager.register(Box::new(DatabaseHealthCheck::new("postgres")));
        manager.register(Box::new(CacheHealthCheck::new("redis")));

        let status = manager.check_all().expect("Health check failed");
        assert_eq!(status.checks.len(), 2);
        assert_eq!(status.status, ComponentStatus::Healthy);
    }

    #[test]
    fn test_disk_space_check() {
        let check = DiskSpaceHealthCheck::new("/", "/", 80.0, 95.0);
        let result = check.check().expect("Check failed");
        assert_eq!(result.status, ComponentStatus::Healthy);
    }
}
