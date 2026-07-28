//! DR testing and validation.
//!
//! [`DrTester::execute_test`] runs real readiness checks against an injected
//! [`DrProbe`]: connectivity to the DR region, data consistency between primary
//! and DR, and failover readiness. Each check reflects the genuine state of the
//! DR region — a failing probe (or a probe error) surfaces as a recorded issue
//! and a failed test, so readiness sign-off cannot be silently rubber-stamped.

use super::{DrConfig, DrProbe, DrTestResult};
use crate::error::{HaError, HaResult};
use chrono::Utc;
use parking_lot::RwLock;
use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;

/// DR test executor.
pub struct DrTester {
    /// Configuration.
    config: DrConfig,
    /// Probe used to inspect real DR region state.
    probe: RwLock<Option<Arc<dyn DrProbe>>>,
}

impl DrTester {
    /// Get the configuration.
    pub fn config(&self) -> &DrConfig {
        &self.config
    }
}

impl DrTester {
    /// Create a new DR tester.
    pub fn new(config: DrConfig) -> Self {
        Self {
            config,
            probe: RwLock::new(None),
        }
    }

    /// Inject the probe used to inspect DR region state.
    ///
    /// Required before [`execute_test`](Self::execute_test); a test without a
    /// probe returns a typed error rather than an unconditional pass.
    pub fn set_probe(&self, probe: Arc<dyn DrProbe>) {
        *self.probe.write() = Some(probe);
    }

    fn probe(&self) -> HaResult<Arc<dyn DrProbe>> {
        self.probe.read().clone().ok_or_else(|| {
            HaError::DisasterRecovery(
                "no DR probe configured; refusing to report an unconditional pass".to_string(),
            )
        })
    }

    /// Execute DR test against the real DR region state.
    pub async fn execute_test(&self) -> HaResult<DrTestResult> {
        let start_time = Utc::now();
        let mut issues = Vec::new();
        let probe = self.probe()?;
        let primary = &self.config.primary_region;
        let dr = &self.config.dr_region;

        info!("Starting DR test");

        info!("Testing DR region connectivity");
        match probe.check_connectivity(dr).await {
            Ok(true) => {}
            Ok(false) => issues.push(format!("DR region '{dr}' is not reachable")),
            Err(e) => issues.push(format!("DR connectivity probe failed: {e}")),
        }

        info!("Testing DR region data consistency");
        match probe.check_data_consistency(primary, dr).await {
            Ok(true) => {}
            Ok(false) => issues.push(format!(
                "DR region '{dr}' is not data-consistent with primary '{primary}'"
            )),
            Err(e) => issues.push(format!("DR data-consistency probe failed: {e}")),
        }

        info!("Testing failover procedures");
        match probe.check_failover_readiness(dr).await {
            Ok(true) => {}
            Ok(false) => issues.push(format!("DR region '{dr}' is not failover-ready")),
            Err(e) => issues.push(format!("DR failover-readiness probe failed: {e}")),
        }

        let duration_ms = (Utc::now() - start_time).num_milliseconds().max(0) as u64;
        let success = issues.is_empty();

        if success {
            info!("DR test completed successfully in {}ms", duration_ms);
        } else {
            warn!("DR test completed with {} issues", issues.len());
        }

        Ok(DrTestResult {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            duration_ms,
            success,
            issues,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::dr::control_plane::{InMemoryDrControlPlane, RegionState};

    fn plane() -> Arc<InMemoryDrControlPlane> {
        let plane = Arc::new(InMemoryDrControlPlane::new());
        plane.register_region(
            "us-east-1",
            RegionState {
                is_primary: true,
                accepts_traffic: true,
                data_watermark: 10,
                ..Default::default()
            },
        );
        plane.register_region(
            "us-west-2",
            RegionState {
                data_watermark: 10,
                ..Default::default()
            },
        );
        plane
    }

    #[tokio::test]
    async fn test_without_probe_errors() {
        let tester = DrTester::new(DrConfig::default());
        assert!(tester.execute_test().await.is_err());
    }

    #[tokio::test]
    async fn test_healthy_dr_passes() {
        let tester = DrTester::new(DrConfig::default());
        tester.set_probe(Arc::clone(&plane()) as Arc<dyn DrProbe>);
        let result = tester.execute_test().await.unwrap();
        assert!(result.success, "issues: {:?}", result.issues);
        assert!(result.issues.is_empty());
    }

    #[tokio::test]
    async fn test_unreachable_dr_fails_with_issues() {
        let p = plane();
        p.set_reachable("us-west-2", false).unwrap();
        let tester = DrTester::new(DrConfig::default());
        tester.set_probe(Arc::clone(&p) as Arc<dyn DrProbe>);

        let result = tester.execute_test().await.unwrap();
        assert!(!result.success);
        // Both connectivity and readiness should flag the unreachable region.
        assert!(!result.issues.is_empty());
    }

    #[tokio::test]
    async fn test_lagging_dr_flags_inconsistency() {
        let p = plane();
        p.set_watermark("us-west-2", 3).unwrap(); // behind primary's 10
        let tester = DrTester::new(DrConfig::default());
        tester.set_probe(Arc::clone(&p) as Arc<dyn DrProbe>);

        let result = tester.execute_test().await.unwrap();
        assert!(!result.success);
        assert!(result.issues.iter().any(|i| i.contains("data-consistent")));
    }
}
