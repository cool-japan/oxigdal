//! Local, in-memory DR control plane.
//!
//! [`InMemoryDrControlPlane`] is a real (not simulated) implementation of both
//! [`DrExecutor`] and [`DrProbe`]. It maintains observable per-region state —
//! reachability, primary flag, traffic acceptance, and a data watermark — and
//! actually mutates that state when the orchestrator issues cutover commands.
//! Callers (and tests) can inspect the resulting state to confirm that traffic
//! was truly redirected and the DR region truly promoted, and readiness probes
//! reflect the genuine state rather than always returning `true`.
//!
//! An embedding application that manages real regions (DNS, load balancers,
//! managed databases) supplies its own [`DrExecutor`]/[`DrProbe`]; this
//! implementation serves single-process deployments, staging, and tests.

use super::{DrExecutor, DrProbe};
use crate::error::{HaError, HaResult};
use async_trait::async_trait;
use dashmap::DashMap;
use parking_lot::RwLock;

/// Observable state of a single region within the control plane.
#[derive(Debug, Clone)]
pub struct RegionState {
    /// Whether the region is reachable from the control plane.
    pub reachable: bool,
    /// Whether the region currently serves as the primary.
    pub is_primary: bool,
    /// Whether the region currently accepts client traffic.
    pub accepts_traffic: bool,
    /// Monotonic data watermark (e.g. last applied transaction id).
    pub data_watermark: u64,
    /// Whether the region has passed failover readiness preparation.
    pub failover_ready: bool,
}

impl Default for RegionState {
    fn default() -> Self {
        Self {
            reachable: true,
            is_primary: false,
            accepts_traffic: false,
            data_watermark: 0,
            failover_ready: true,
        }
    }
}

/// In-memory DR control plane implementing [`DrExecutor`] and [`DrProbe`].
#[derive(Default)]
pub struct InMemoryDrControlPlane {
    /// Per-region observable state.
    regions: DashMap<String, RwLock<RegionState>>,
    /// The region currently receiving client traffic.
    traffic_target: RwLock<Option<String>>,
}

impl InMemoryDrControlPlane {
    /// Create an empty control plane.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register (or replace) a region's state.
    pub fn register_region(&self, region: impl Into<String>, state: RegionState) {
        self.regions.insert(region.into(), RwLock::new(state));
    }

    /// Get a snapshot of a region's current state, if registered.
    pub fn region_state(&self, region: &str) -> Option<RegionState> {
        self.regions.get(region).map(|s| s.read().clone())
    }

    /// The region currently receiving client traffic, if any.
    pub fn traffic_target(&self) -> Option<String> {
        self.traffic_target.read().clone()
    }

    /// Set a region's data watermark (e.g. after applying replication).
    pub fn set_watermark(&self, region: &str, watermark: u64) -> HaResult<()> {
        self.with_region(region, |state| {
            state.data_watermark = watermark;
            Ok(())
        })
    }

    /// Set a region's reachability (e.g. to simulate a partition in tests).
    pub fn set_reachable(&self, region: &str, reachable: bool) -> HaResult<()> {
        self.with_region(region, |state| {
            state.reachable = reachable;
            Ok(())
        })
    }

    fn with_region<T>(
        &self,
        region: &str,
        f: impl FnOnce(&mut RegionState) -> HaResult<T>,
    ) -> HaResult<T> {
        let handle = self.regions.get(region).ok_or_else(|| {
            HaError::DisasterRecovery(format!("region '{region}' is not registered"))
        })?;
        let mut guard = handle.write();
        f(&mut guard)
    }

    fn read_region<T>(&self, region: &str, f: impl FnOnce(&RegionState) -> T) -> HaResult<T> {
        let handle = self.regions.get(region).ok_or_else(|| {
            HaError::DisasterRecovery(format!("region '{region}' is not registered"))
        })?;
        let guard = handle.read();
        Ok(f(&guard))
    }

    fn ensure_reachable(&self, region: &str) -> HaResult<()> {
        let reachable = self.read_region(region, |s| s.reachable)?;
        if !reachable {
            return Err(HaError::DisasterRecovery(format!(
                "region '{region}' is not reachable"
            )));
        }
        Ok(())
    }
}

#[async_trait]
impl DrExecutor for InMemoryDrControlPlane {
    async fn stop_traffic(&self, region: &str) -> HaResult<()> {
        self.ensure_reachable(region)?;
        self.with_region(region, |state| {
            state.accepts_traffic = false;
            Ok(())
        })?;
        let mut target = self.traffic_target.write();
        if target.as_deref() == Some(region) {
            *target = None;
        }
        Ok(())
    }

    async fn promote_region(&self, region: &str) -> HaResult<()> {
        self.ensure_reachable(region)?;
        self.with_region(region, |state| {
            if !state.failover_ready {
                return Err(HaError::DisasterRecovery(format!(
                    "region '{region}' is not failover-ready; cannot promote"
                )));
            }
            state.is_primary = true;
            state.accepts_traffic = true;
            Ok(())
        })
    }

    async fn redirect_traffic(&self, from: &str, to: &str) -> HaResult<()> {
        self.ensure_reachable(to)?;
        // Demote the old primary (best-effort if it is still reachable).
        if self.regions.contains_key(from) {
            let _ = self.with_region(from, |state| {
                state.is_primary = false;
                state.accepts_traffic = false;
                Ok(())
            });
        }
        self.with_region(to, |state| {
            state.accepts_traffic = true;
            Ok(())
        })?;
        *self.traffic_target.write() = Some(to.to_string());
        Ok(())
    }

    async fn verify_primary(&self, region: &str) -> HaResult<bool> {
        self.read_region(region, |state| {
            state.reachable && state.is_primary && state.accepts_traffic
        })
    }
}

#[async_trait]
impl DrProbe for InMemoryDrControlPlane {
    async fn check_connectivity(&self, region: &str) -> HaResult<bool> {
        self.read_region(region, |state| state.reachable)
    }

    async fn check_data_consistency(&self, primary: &str, dr: &str) -> HaResult<bool> {
        let primary_wm = self.read_region(primary, |s| (s.reachable, s.data_watermark))?;
        let dr_wm = self.read_region(dr, |s| (s.reachable, s.data_watermark))?;
        if !primary_wm.0 || !dr_wm.0 {
            return Ok(false);
        }
        // The DR region is consistent when it has caught up to the primary.
        Ok(dr_wm.1 >= primary_wm.1)
    }

    async fn check_failover_readiness(&self, region: &str) -> HaResult<bool> {
        self.read_region(region, |state| state.reachable && state.failover_ready)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn plane() -> InMemoryDrControlPlane {
        let plane = InMemoryDrControlPlane::new();
        plane.register_region(
            "primary",
            RegionState {
                is_primary: true,
                accepts_traffic: true,
                data_watermark: 100,
                ..Default::default()
            },
        );
        plane.register_region(
            "dr",
            RegionState {
                data_watermark: 100,
                ..Default::default()
            },
        );
        plane
    }

    #[tokio::test]
    async fn test_cutover_actually_moves_traffic_and_primary() {
        let plane = plane();
        plane.stop_traffic("primary").await.unwrap();
        plane.promote_region("dr").await.unwrap();
        plane.redirect_traffic("primary", "dr").await.unwrap();

        assert!(plane.verify_primary("dr").await.unwrap());
        assert_eq!(plane.traffic_target().as_deref(), Some("dr"));
        let old = plane.region_state("primary").unwrap();
        assert!(!old.is_primary);
        assert!(!old.accepts_traffic);
    }

    #[tokio::test]
    async fn test_promote_unreachable_region_errors() {
        let plane = plane();
        plane.set_reachable("dr", false).unwrap();
        assert!(plane.promote_region("dr").await.is_err());
    }

    #[tokio::test]
    async fn test_data_consistency_reflects_watermarks() {
        let plane = plane();
        assert!(plane.check_data_consistency("primary", "dr").await.unwrap());
        // DR falls behind → inconsistent.
        plane.set_watermark("dr", 50).unwrap();
        assert!(!plane.check_data_consistency("primary", "dr").await.unwrap());
    }

    #[tokio::test]
    async fn test_connectivity_and_readiness_probes() {
        let plane = plane();
        assert!(plane.check_connectivity("dr").await.unwrap());
        plane.set_reachable("dr", false).unwrap();
        assert!(!plane.check_connectivity("dr").await.unwrap());
        assert!(!plane.check_failover_readiness("dr").await.unwrap());
    }
}
