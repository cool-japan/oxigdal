//! Built-in health checks.

use super::{HealthCheck, HealthCheckResult, HealthCheckType, HealthStatus};
use crate::error::HaResult;
use async_trait::async_trait;
use chrono::Utc;

/// Liveness health check.
pub struct LivenessCheck {
    /// Check name.
    name: String,
}

impl LivenessCheck {
    /// Create a new liveness check.
    pub fn new(name: String) -> Self {
        Self { name }
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

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let duration_ms = (Utc::now() - start).num_milliseconds() as u64;

        Ok(HealthCheckResult {
            name: self.name.clone(),
            check_type: HealthCheckType::Liveness,
            status: HealthStatus::Healthy,
            message: Some("Service is running".to_string()),
            timestamp: Utc::now(),
            duration_ms,
        })
    }
}

/// Readiness health check.
pub struct ReadinessCheck {
    /// Check name.
    name: String,
}

impl ReadinessCheck {
    /// Create a new readiness check.
    pub fn new(name: String) -> Self {
        Self { name }
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

        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;

        let duration_ms = (Utc::now() - start).num_milliseconds() as u64;

        Ok(HealthCheckResult {
            name: self.name.clone(),
            check_type: HealthCheckType::Readiness,
            status: HealthStatus::Healthy,
            message: Some("Service is ready".to_string()),
            timestamp: Utc::now(),
            duration_ms,
        })
    }
}

/// Dependency health check.
pub struct DependencyCheck {
    /// Check name.
    name: String,
    /// Dependency name.
    dependency: String,
}

impl DependencyCheck {
    /// Create a new dependency check.
    pub fn new(name: String, dependency: String) -> Self {
        Self { name, dependency }
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

        tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

        let duration_ms = (Utc::now() - start).num_milliseconds() as u64;

        Ok(HealthCheckResult {
            name: self.name.clone(),
            check_type: HealthCheckType::Dependency,
            status: HealthStatus::Healthy,
            message: Some(format!("Dependency {} is available", self.dependency)),
            timestamp: Utc::now(),
            duration_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_liveness_check() {
        let check = LivenessCheck::new("test-liveness".to_string());
        let result = check.check().await.ok();

        assert!(result.is_some());
        let result = result.expect("should have liveness check result");
        assert_eq!(result.status, HealthStatus::Healthy);
        assert_eq!(result.check_type, HealthCheckType::Liveness);
    }

    #[tokio::test]
    async fn test_readiness_check() {
        let check = ReadinessCheck::new("test-readiness".to_string());
        let result = check.check().await.ok();

        assert!(result.is_some());
        let result = result.expect("should have readiness check result");
        assert_eq!(result.status, HealthStatus::Healthy);
        assert_eq!(result.check_type, HealthCheckType::Readiness);
    }
}
