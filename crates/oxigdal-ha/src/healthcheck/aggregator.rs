//! Health check aggregator.

use super::{HealthCheck, HealthCheckResult, HealthStatus};
use crate::error::HaResult;
use std::sync::Arc;
use tracing::{debug, warn};

/// Health check aggregator.
pub struct HealthCheckAggregator {
    /// Registered health checks.
    checks: Vec<Arc<dyn HealthCheck>>,
}

impl HealthCheckAggregator {
    /// Create a new health check aggregator.
    pub fn new() -> Self {
        Self { checks: Vec::new() }
    }

    /// Register a health check.
    pub fn register(&mut self, check: Arc<dyn HealthCheck>) {
        debug!("Registering health check: {}", check.name());
        self.checks.push(check);
    }

    /// Execute all health checks.
    pub async fn check_all(&self) -> HaResult<Vec<HealthCheckResult>> {
        let mut results = Vec::new();

        for check in &self.checks {
            match check.check().await {
                Ok(result) => {
                    if result.status != HealthStatus::Healthy {
                        warn!(
                            "Health check {} returned status {:?}: {:?}",
                            result.name, result.status, result.message
                        );
                    }
                    results.push(result);
                }
                Err(e) => {
                    warn!("Health check {} failed: {}", check.name(), e);
                }
            }
        }

        Ok(results)
    }

    /// Get overall health status.
    pub async fn get_overall_status(&self) -> HaResult<HealthStatus> {
        let results = self.check_all().await?;

        if results.is_empty() {
            return Ok(HealthStatus::Healthy);
        }

        let has_unhealthy = results.iter().any(|r| r.status == HealthStatus::Unhealthy);
        let has_degraded = results.iter().any(|r| r.status == HealthStatus::Degraded);

        if has_unhealthy {
            Ok(HealthStatus::Unhealthy)
        } else if has_degraded {
            Ok(HealthStatus::Degraded)
        } else {
            Ok(HealthStatus::Healthy)
        }
    }
}

impl Default for HealthCheckAggregator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::healthcheck::checks::LivenessCheck;

    #[tokio::test]
    async fn test_aggregator() {
        let mut aggregator = HealthCheckAggregator::new();

        let check1 = Arc::new(LivenessCheck::new("check1".to_string()));
        let check2 = Arc::new(LivenessCheck::new("check2".to_string()));

        aggregator.register(check1);
        aggregator.register(check2);

        let results = aggregator.check_all().await.ok();
        assert!(results.is_some());

        let results = results.expect("should get results from aggregator check_all");
        assert_eq!(results.len(), 2);

        let status = aggregator.get_overall_status().await.ok();
        assert_eq!(status, Some(HealthStatus::Healthy));
    }
}
