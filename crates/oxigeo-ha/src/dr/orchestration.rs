//! DR orchestration logic.
//!
//! [`DrOrchestrator::execute_failover`] drives a real cutover: it fences the
//! failing primary, promotes the DR region, repoints client traffic, and
//! verifies the new primary — each step delegating to an injected
//! [`DrExecutor`]. Any step failing (or the final verification returning false)
//! aborts with a typed error instead of reporting a fabricated success.

use super::{DrConfig, DrExecutor, DrFailoverResult};
use crate::error::{HaError, HaResult};
use chrono::Utc;
use parking_lot::RwLock;
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

/// DR orchestrator.
pub struct DrOrchestrator {
    /// Configuration.
    config: DrConfig,
    /// Executor that performs the concrete cutover actions.
    executor: RwLock<Option<Arc<dyn DrExecutor>>>,
}

impl DrOrchestrator {
    /// Create a new DR orchestrator.
    pub fn new(config: DrConfig) -> Self {
        Self {
            config,
            executor: RwLock::new(None),
        }
    }

    /// Inject the executor used to perform the cutover.
    ///
    /// Required before [`execute_failover`](Self::execute_failover); a failover
    /// without an executor returns a typed error rather than a fake result.
    pub fn set_executor(&self, executor: Arc<dyn DrExecutor>) {
        *self.executor.write() = Some(executor);
    }

    fn executor(&self) -> HaResult<Arc<dyn DrExecutor>> {
        self.executor.read().clone().ok_or_else(|| {
            HaError::DrFailoverFailed(
                "no DR executor configured; refusing to report a fake failover".to_string(),
            )
        })
    }

    /// Execute DR failover from the primary region to the DR region.
    pub async fn execute_failover(&self) -> HaResult<DrFailoverResult> {
        let started_at = Utc::now();
        let executor = self.executor()?;
        let primary = &self.config.primary_region;
        let dr = &self.config.dr_region;

        info!("Starting DR failover from {} to {}", primary, dr);

        info!("Step 1: Stopping traffic to primary region {}", primary);
        executor.stop_traffic(primary).await?;

        info!("Step 2: Promoting DR region {} to primary", dr);
        executor.promote_region(dr).await?;

        info!("Step 3: Redirecting traffic from {} to {}", primary, dr);
        executor.redirect_traffic(primary, dr).await?;

        info!("Step 4: Verifying new primary {}", dr);
        if !executor.verify_primary(dr).await? {
            return Err(HaError::DrFailoverFailed(format!(
                "DR region '{dr}' did not come up as a healthy primary after cutover"
            )));
        }

        let completed_at = Utc::now();
        let rto_achieved_seconds = (completed_at - started_at).num_seconds().max(0) as u64;

        info!(
            "DR failover complete in {} seconds (RTO target: {} seconds)",
            rto_achieved_seconds, self.config.rto_seconds
        );

        Ok(DrFailoverResult {
            id: Uuid::new_v4(),
            started_at,
            completed_at,
            old_primary: primary.clone(),
            new_primary: dr.clone(),
            rto_achieved_seconds,
            success: true,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::dr::control_plane::{InMemoryDrControlPlane, RegionState};

    fn wired_plane() -> Arc<InMemoryDrControlPlane> {
        let plane = Arc::new(InMemoryDrControlPlane::new());
        plane.register_region(
            "us-east-1",
            RegionState {
                is_primary: true,
                accepts_traffic: true,
                data_watermark: 42,
                ..Default::default()
            },
        );
        plane.register_region(
            "us-west-2",
            RegionState {
                data_watermark: 42,
                ..Default::default()
            },
        );
        plane
    }

    #[tokio::test]
    async fn test_failover_without_executor_errors() {
        let orchestrator = DrOrchestrator::new(DrConfig::default());
        assert!(orchestrator.execute_failover().await.is_err());
    }

    #[tokio::test]
    async fn test_failover_performs_real_cutover() {
        let plane = wired_plane();
        let orchestrator = DrOrchestrator::new(DrConfig::default());
        orchestrator.set_executor(Arc::clone(&plane) as Arc<dyn DrExecutor>);

        let result = orchestrator.execute_failover().await.unwrap();
        assert!(result.success);
        assert_eq!(result.old_primary, "us-east-1");
        assert_eq!(result.new_primary, "us-west-2");

        // The cutover really happened on the control plane.
        assert_eq!(plane.traffic_target().as_deref(), Some("us-west-2"));
        assert!(plane.region_state("us-west-2").unwrap().is_primary);
        assert!(!plane.region_state("us-east-1").unwrap().accepts_traffic);
    }

    #[tokio::test]
    async fn test_failover_aborts_if_dr_unreachable() {
        let plane = wired_plane();
        plane.set_reachable("us-west-2", false).unwrap();
        let orchestrator = DrOrchestrator::new(DrConfig::default());
        orchestrator.set_executor(Arc::clone(&plane) as Arc<dyn DrExecutor>);

        // Promotion of an unreachable DR region must fail, not fake success.
        assert!(orchestrator.execute_failover().await.is_err());
    }
}
