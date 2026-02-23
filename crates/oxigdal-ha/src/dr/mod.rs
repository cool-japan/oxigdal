//! Disaster recovery orchestration.

pub mod orchestration;
pub mod runbook;
pub mod testing;

use crate::error::HaResult;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// DR configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrConfig {
    /// Primary region.
    pub primary_region: String,
    /// DR region.
    pub dr_region: String,
    /// RTO (Recovery Time Objective) in seconds.
    pub rto_seconds: u64,
    /// RPO (Recovery Point Objective) in seconds.
    pub rpo_seconds: u64,
    /// Enable automatic DR failover.
    pub enable_auto_failover: bool,
}

impl Default for DrConfig {
    fn default() -> Self {
        Self {
            primary_region: "us-east-1".to_string(),
            dr_region: "us-west-2".to_string(),
            rto_seconds: 300,
            rpo_seconds: 60,
            enable_auto_failover: false,
        }
    }
}

/// DR failover result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrFailoverResult {
    /// Failover ID.
    pub id: Uuid,
    /// Start timestamp.
    pub started_at: DateTime<Utc>,
    /// End timestamp.
    pub completed_at: DateTime<Utc>,
    /// Old primary region.
    pub old_primary: String,
    /// New primary region.
    pub new_primary: String,
    /// RTO achieved in seconds.
    pub rto_achieved_seconds: u64,
    /// Success flag.
    pub success: bool,
}

/// DR test result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrTestResult {
    /// Test ID.
    pub id: Uuid,
    /// Test timestamp.
    pub timestamp: DateTime<Utc>,
    /// Test duration in milliseconds.
    pub duration_ms: u64,
    /// Success flag.
    pub success: bool,
    /// Issues found.
    pub issues: Vec<String>,
}

/// Trait for DR manager.
#[async_trait]
pub trait DrManager: Send + Sync {
    /// Perform DR failover.
    async fn failover(&self) -> HaResult<DrFailoverResult>;

    /// Test DR procedures.
    async fn test_dr(&self) -> HaResult<DrTestResult>;

    /// Get current DR status.
    async fn get_status(&self) -> HaResult<DrStatus>;
}

/// DR status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrStatus {
    /// Current primary region.
    pub current_primary: String,
    /// DR region ready.
    pub dr_ready: bool,
    /// Last test timestamp.
    pub last_test_at: Option<DateTime<Utc>>,
    /// Last failover timestamp.
    pub last_failover_at: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dr_config() {
        let config = DrConfig::default();
        assert_eq!(config.primary_region, "us-east-1");
        assert_eq!(config.rto_seconds, 300);
        assert_eq!(config.rpo_seconds, 60);
    }
}
