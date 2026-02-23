//! Replica promotion logic.

use super::FailoverConfig;
use crate::error::{HaError, HaResult};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;

/// Promotion strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PromotionStrategy {
    /// Promote based on priority.
    Priority,
    /// Promote based on least lag.
    LeastLag,
    /// Promote based on load.
    LeastLoad,
    /// Manual promotion.
    Manual,
}

/// Candidate for promotion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionCandidate {
    /// Node ID.
    pub node_id: Uuid,
    /// Node name.
    pub name: String,
    /// Node priority.
    pub priority: u32,
    /// Current lag in milliseconds.
    pub lag_ms: Option<u64>,
    /// Current load (0.0-1.0).
    pub load: f64,
    /// Health score (0.0-1.0).
    pub health_score: f64,
}

impl PromotionCandidate {
    /// Calculate promotion score based on strategy.
    pub fn calculate_score(&self, strategy: PromotionStrategy) -> f64 {
        match strategy {
            PromotionStrategy::Priority => self.priority as f64,
            PromotionStrategy::LeastLag => {
                let lag = self.lag_ms.unwrap_or(u64::MAX) as f64;
                1.0 / (lag + 1.0)
            }
            PromotionStrategy::LeastLoad => 1.0 - self.load,
            PromotionStrategy::Manual => 0.0,
        }
    }
}

/// Promotion result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionResult {
    /// Promoted node ID.
    pub promoted_node_id: Uuid,
    /// Strategy used.
    pub strategy: PromotionStrategy,
    /// Promotion score.
    pub score: f64,
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// Timestamp.
    pub timestamp: DateTime<Utc>,
}

/// Replica promotion manager.
pub struct ReplicaPromotion {
    /// Configuration.
    config: Arc<FailoverConfig>,
    /// Default promotion strategy.
    strategy: PromotionStrategy,
}

impl ReplicaPromotion {
    /// Create a new replica promotion manager.
    pub fn new(config: FailoverConfig, strategy: PromotionStrategy) -> Self {
        Self {
            config: Arc::new(config),
            strategy,
        }
    }

    /// Select best candidate for promotion.
    pub async fn select_candidate(
        &self,
        candidates: Vec<PromotionCandidate>,
    ) -> HaResult<PromotionCandidate> {
        if candidates.is_empty() {
            return Err(HaError::NoHealthyReplicas);
        }

        info!(
            "Selecting promotion candidate from {} options using {:?} strategy",
            candidates.len(),
            self.strategy
        );

        let mut best_candidate = None;
        let mut best_score = f64::MIN;

        for candidate in candidates {
            let score = candidate.calculate_score(self.strategy);

            if score > best_score {
                best_score = score;
                best_candidate = Some(candidate);
            }
        }

        best_candidate.ok_or_else(|| HaError::Failover("No suitable candidate found".to_string()))
    }

    /// Promote a replica to leader.
    pub async fn promote_replica(
        &self,
        candidate: PromotionCandidate,
    ) -> HaResult<PromotionResult> {
        let start_time = Utc::now();

        info!(
            "Promoting replica {} ({}) to leader",
            candidate.name, candidate.node_id
        );

        // Simulate promotion process
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let duration_ms = (Utc::now() - start_time).num_milliseconds() as u64;

        if duration_ms > self.config.max_failover_time_ms {
            warn!(
                "Promotion took {}ms (exceeds target of {}ms)",
                duration_ms, self.config.max_failover_time_ms
            );
        }

        let score = candidate.calculate_score(self.strategy);

        Ok(PromotionResult {
            promoted_node_id: candidate.node_id,
            strategy: self.strategy,
            score,
            duration_ms,
            timestamp: Utc::now(),
        })
    }

    /// Demote a leader back to follower.
    pub async fn demote_leader(&self, node_id: Uuid) -> HaResult<()> {
        info!("Demoting leader {}", node_id);

        // Simulate demotion process
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        Ok(())
    }

    /// Perform graceful handover from old leader to new leader.
    pub async fn graceful_handover(
        &self,
        old_leader_id: Uuid,
        new_leader_id: Uuid,
    ) -> HaResult<()> {
        info!(
            "Performing graceful handover from {} to {}",
            old_leader_id, new_leader_id
        );

        // Step 1: Stop writes on old leader
        info!("Stopping writes on old leader");
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Step 2: Ensure new leader is caught up
        info!("Waiting for new leader to catch up");
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Step 3: Promote new leader
        info!("Promoting new leader");
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Step 4: Demote old leader
        info!("Demoting old leader");
        self.demote_leader(old_leader_id).await?;

        info!("Graceful handover complete");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_select_candidate_by_priority() {
        let config = FailoverConfig::default();
        let promotion = ReplicaPromotion::new(config, PromotionStrategy::Priority);

        let candidates = vec![
            PromotionCandidate {
                node_id: Uuid::new_v4(),
                name: "node1".to_string(),
                priority: 100,
                lag_ms: Some(10),
                load: 0.5,
                health_score: 1.0,
            },
            PromotionCandidate {
                node_id: Uuid::new_v4(),
                name: "node2".to_string(),
                priority: 200,
                lag_ms: Some(20),
                load: 0.6,
                health_score: 1.0,
            },
            PromotionCandidate {
                node_id: Uuid::new_v4(),
                name: "node3".to_string(),
                priority: 150,
                lag_ms: Some(5),
                load: 0.4,
                health_score: 1.0,
            },
        ];

        let selected = promotion.select_candidate(candidates).await.ok();
        assert!(selected.is_some());

        if let Some(sel) = selected {
            assert_eq!(sel.name, "node2");
            assert_eq!(sel.priority, 200);
        }
    }

    #[tokio::test]
    async fn test_select_candidate_by_least_lag() {
        let config = FailoverConfig::default();
        let promotion = ReplicaPromotion::new(config, PromotionStrategy::LeastLag);

        let candidates = vec![
            PromotionCandidate {
                node_id: Uuid::new_v4(),
                name: "node1".to_string(),
                priority: 100,
                lag_ms: Some(10),
                load: 0.5,
                health_score: 1.0,
            },
            PromotionCandidate {
                node_id: Uuid::new_v4(),
                name: "node2".to_string(),
                priority: 200,
                lag_ms: Some(20),
                load: 0.6,
                health_score: 1.0,
            },
            PromotionCandidate {
                node_id: Uuid::new_v4(),
                name: "node3".to_string(),
                priority: 150,
                lag_ms: Some(5),
                load: 0.4,
                health_score: 1.0,
            },
        ];

        let selected = promotion.select_candidate(candidates).await.ok();
        assert!(selected.is_some());

        if let Some(sel) = selected {
            assert_eq!(sel.name, "node3");
            assert_eq!(sel.lag_ms, Some(5));
        }
    }

    #[tokio::test]
    async fn test_promote_replica() {
        let config = FailoverConfig::default();
        let promotion = ReplicaPromotion::new(config, PromotionStrategy::Priority);

        let candidate = PromotionCandidate {
            node_id: Uuid::new_v4(),
            name: "node1".to_string(),
            priority: 100,
            lag_ms: Some(10),
            load: 0.5,
            health_score: 1.0,
        };

        let result = promotion.promote_replica(candidate).await.ok();
        assert!(result.is_some());

        if let Some(r) = result {
            assert_eq!(r.strategy, PromotionStrategy::Priority);
        }
    }
}
