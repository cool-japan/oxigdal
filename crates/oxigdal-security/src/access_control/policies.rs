//! Policy engine for combining RBAC and ABAC.

use crate::access_control::{
    AccessControlEvaluator, AccessDecision, AccessRequest, abac::AbacEngine, rbac::RbacEngine,
};
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Policy enforcement mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnforcementMode {
    /// Only use RBAC.
    RbacOnly,
    /// Only use ABAC.
    AbacOnly,
    /// Both must allow (AND).
    Both,
    /// Either can allow (OR).
    Either,
    /// ABAC first, fallback to RBAC.
    AbacThenRbac,
    /// RBAC first, fallback to ABAC.
    RbacThenAbac,
}

/// Combined policy engine with RBAC and ABAC.
pub struct PolicyEngine {
    rbac: Arc<RbacEngine>,
    abac: Arc<AbacEngine>,
    enforcement_mode: parking_lot::RwLock<EnforcementMode>,
}

impl PolicyEngine {
    /// Create a new policy engine.
    pub fn new(rbac: Arc<RbacEngine>, abac: Arc<AbacEngine>) -> Self {
        Self {
            rbac,
            abac,
            enforcement_mode: parking_lot::RwLock::new(EnforcementMode::Either),
        }
    }

    /// Set enforcement mode.
    pub fn set_enforcement_mode(&self, mode: EnforcementMode) {
        *self.enforcement_mode.write() = mode;
    }

    /// Get enforcement mode.
    pub fn get_enforcement_mode(&self) -> EnforcementMode {
        *self.enforcement_mode.read()
    }

    /// Get RBAC engine.
    pub fn rbac(&self) -> &Arc<RbacEngine> {
        &self.rbac
    }

    /// Get ABAC engine.
    pub fn abac(&self) -> &Arc<AbacEngine> {
        &self.abac
    }
}

impl AccessControlEvaluator for PolicyEngine {
    fn evaluate(&self, request: &AccessRequest) -> Result<AccessDecision> {
        let mode = self.get_enforcement_mode();

        match mode {
            EnforcementMode::RbacOnly => self.rbac.evaluate(request),
            EnforcementMode::AbacOnly => self.abac.evaluate(request),
            EnforcementMode::Both => {
                let rbac_decision = self.rbac.evaluate(request)?;
                let abac_decision = self.abac.evaluate(request)?;

                if rbac_decision == AccessDecision::Allow && abac_decision == AccessDecision::Allow
                {
                    Ok(AccessDecision::Allow)
                } else {
                    Ok(AccessDecision::Deny)
                }
            }
            EnforcementMode::Either => {
                let rbac_decision = self.rbac.evaluate(request)?;
                let abac_decision = self.abac.evaluate(request)?;

                if rbac_decision == AccessDecision::Allow || abac_decision == AccessDecision::Allow
                {
                    Ok(AccessDecision::Allow)
                } else {
                    Ok(AccessDecision::Deny)
                }
            }
            EnforcementMode::AbacThenRbac => {
                let abac_decision = self.abac.evaluate(request)?;
                if abac_decision == AccessDecision::Allow {
                    Ok(AccessDecision::Allow)
                } else {
                    self.rbac.evaluate(request)
                }
            }
            EnforcementMode::RbacThenAbac => {
                let rbac_decision = self.rbac.evaluate(request)?;
                if rbac_decision == AccessDecision::Allow {
                    Ok(AccessDecision::Allow)
                } else {
                    self.abac.evaluate(request)
                }
            }
        }
    }
}

/// Spatial access control for region-based restrictions.
pub struct SpatialAccessControl {
    /// Region boundaries (region_id -> (min_lon, min_lat, max_lon, max_lat)).
    regions: dashmap::DashMap<String, (f64, f64, f64, f64)>,
    /// Subject to allowed regions.
    subject_regions: dashmap::DashMap<String, Vec<String>>,
}

impl SpatialAccessControl {
    /// Create a new spatial access control.
    pub fn new() -> Self {
        Self {
            regions: dashmap::DashMap::new(),
            subject_regions: dashmap::DashMap::new(),
        }
    }

    /// Define a region boundary.
    pub fn define_region(
        &self,
        region_id: String,
        min_lon: f64,
        min_lat: f64,
        max_lon: f64,
        max_lat: f64,
    ) -> Result<()> {
        self.regions
            .insert(region_id, (min_lon, min_lat, max_lon, max_lat));
        Ok(())
    }

    /// Grant subject access to a region.
    pub fn grant_region_access(&self, subject_id: &str, region_id: String) -> Result<()> {
        self.subject_regions
            .entry(subject_id.to_string())
            .or_default()
            .push(region_id);
        Ok(())
    }

    /// Check if subject can access a point.
    pub fn can_access_point(&self, subject_id: &str, lon: f64, lat: f64) -> bool {
        if let Some(regions) = self.subject_regions.get(subject_id) {
            for region_id in regions.iter() {
                if let Some(bounds) = self.regions.get(region_id) {
                    let (min_lon, min_lat, max_lon, max_lat) = *bounds;
                    if lon >= min_lon && lon <= max_lon && lat >= min_lat && lat <= max_lat {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Check if subject can access a bounding box.
    pub fn can_access_bbox(
        &self,
        subject_id: &str,
        min_lon: f64,
        min_lat: f64,
        max_lon: f64,
        max_lat: f64,
    ) -> bool {
        if let Some(regions) = self.subject_regions.get(subject_id) {
            for region_id in regions.iter() {
                if let Some(bounds) = self.regions.get(region_id) {
                    let (r_min_lon, r_min_lat, r_max_lon, r_max_lat) = *bounds;
                    // Check if bbox is entirely within region
                    if min_lon >= r_min_lon
                        && max_lon <= r_max_lon
                        && min_lat >= r_min_lat
                        && max_lat <= r_max_lat
                    {
                        return true;
                    }
                }
            }
        }
        false
    }
}

impl Default for SpatialAccessControl {
    fn default() -> Self {
        Self::new()
    }
}

/// Temporal access control for time-based restrictions.
pub struct TemporalAccessControl {
    /// Subject to time windows (start, end).
    time_windows: dashmap::DashMap<String, Vec<(chrono::NaiveTime, chrono::NaiveTime)>>,
    /// Subject to date ranges.
    date_ranges: dashmap::DashMap<String, Vec<(chrono::NaiveDate, Option<chrono::NaiveDate>)>>,
}

impl TemporalAccessControl {
    /// Create a new temporal access control.
    pub fn new() -> Self {
        Self {
            time_windows: dashmap::DashMap::new(),
            date_ranges: dashmap::DashMap::new(),
        }
    }

    /// Set allowed time window for subject (e.g., 9:00-17:00).
    pub fn set_time_window(
        &self,
        subject_id: String,
        start: chrono::NaiveTime,
        end: chrono::NaiveTime,
    ) {
        self.time_windows
            .entry(subject_id)
            .or_default()
            .push((start, end));
    }

    /// Set allowed date range for subject.
    pub fn set_date_range(
        &self,
        subject_id: String,
        start: chrono::NaiveDate,
        end: Option<chrono::NaiveDate>,
    ) {
        self.date_ranges
            .entry(subject_id)
            .or_default()
            .push((start, end));
    }

    /// Check if subject can access at current time.
    pub fn can_access_now(&self, subject_id: &str) -> bool {
        let now = chrono::Utc::now();
        let current_time = now.time();
        let current_date = now.date_naive();

        // Check time windows
        if let Some(windows) = self.time_windows.get(subject_id) {
            let mut time_allowed = false;
            for (start, end) in windows.iter() {
                if current_time >= *start && current_time <= *end {
                    time_allowed = true;
                    break;
                }
            }
            if !time_allowed && !windows.is_empty() {
                return false;
            }
        }

        // Check date ranges
        if let Some(ranges) = self.date_ranges.get(subject_id) {
            let mut date_allowed = false;
            for (start, end) in ranges.iter() {
                if current_date >= *start {
                    if let Some(end_date) = end {
                        if current_date <= *end_date {
                            date_allowed = true;
                            break;
                        }
                    } else {
                        date_allowed = true;
                        break;
                    }
                }
            }
            if !date_allowed && !ranges.is_empty() {
                return false;
            }
        }

        true
    }
}

impl Default for TemporalAccessControl {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policy_engine_either_mode() {
        let rbac = Arc::new(RbacEngine::new());
        let abac = Arc::new(AbacEngine::new());
        let engine = PolicyEngine::new(rbac, abac);

        engine.set_enforcement_mode(EnforcementMode::Either);
        assert_eq!(engine.get_enforcement_mode(), EnforcementMode::Either);
    }

    #[test]
    fn test_spatial_access_control() {
        let sac = SpatialAccessControl::new();

        // Define US region
        sac.define_region(
            "us".to_string(),
            -125.0, // min_lon
            24.0,   // min_lat
            -66.0,  // max_lon
            49.0,   // max_lat
        )
        .expect("Failed to define region");

        sac.grant_region_access("user-123", "us".to_string())
            .expect("Failed to grant access");

        // Point in US
        assert!(sac.can_access_point("user-123", -100.0, 40.0));

        // Point outside US
        assert!(!sac.can_access_point("user-123", 0.0, 51.0));
    }

    #[test]
    fn test_spatial_bbox_access() {
        let sac = SpatialAccessControl::new();

        sac.define_region("region-1".to_string(), 0.0, 0.0, 10.0, 10.0)
            .expect("Failed to define region");

        sac.grant_region_access("user-123", "region-1".to_string())
            .expect("Failed to grant access");

        // Bbox within region
        assert!(sac.can_access_bbox("user-123", 1.0, 1.0, 9.0, 9.0));

        // Bbox partially outside region
        assert!(!sac.can_access_bbox("user-123", 5.0, 5.0, 15.0, 15.0));
    }

    #[test]
    fn test_temporal_access_control() {
        let tac = TemporalAccessControl::new();

        // Set time window: 9:00 - 17:00
        let start = chrono::NaiveTime::from_hms_opt(9, 0, 0).expect("Invalid time");
        let end = chrono::NaiveTime::from_hms_opt(17, 0, 0).expect("Invalid time");
        tac.set_time_window("user-123".to_string(), start, end);

        // This test depends on current time, so we just check it doesn't panic
        let _ = tac.can_access_now("user-123");
    }
}
