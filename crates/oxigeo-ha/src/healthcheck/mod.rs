//! Health check system for HA monitoring.

pub mod aggregator;
pub mod checks;

use crate::error::HaResult;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Health check status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    /// Service is healthy.
    Healthy,
    /// Service is degraded but functional.
    Degraded,
    /// Service is unhealthy.
    Unhealthy,
}

/// Health check type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthCheckType {
    /// Liveness check (is service running).
    Liveness,
    /// Readiness check (is service ready for traffic).
    Readiness,
    /// Dependency check (are dependencies available).
    Dependency,
    /// Custom check.
    Custom,
}

/// Health check result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResult {
    /// Check name.
    pub name: String,
    /// Check type.
    pub check_type: HealthCheckType,
    /// Status.
    pub status: HealthStatus,
    /// Message.
    pub message: Option<String>,
    /// Timestamp.
    pub timestamp: DateTime<Utc>,
    /// Check duration in milliseconds.
    pub duration_ms: u64,
}

/// Trait for health check.
#[async_trait]
pub trait HealthCheck: Send + Sync {
    /// Get check name.
    fn name(&self) -> &str;

    /// Get check type.
    fn check_type(&self) -> HealthCheckType;

    /// Execute the health check.
    async fn check(&self) -> HaResult<HealthCheckResult>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_status() {
        assert_eq!(HealthStatus::Healthy, HealthStatus::Healthy);
        assert_ne!(HealthStatus::Healthy, HealthStatus::Unhealthy);
    }
}
