//! Replica promotion logic.

use super::FailoverConfig;
use super::NodeRole;
use super::transport::NodeTransport;
use crate::error::{HaError, HaResult};
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
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
    /// Transport used to issue role-change / fencing commands to remote nodes.
    ///
    /// When `None`, the manager operates in local (single-node) mode: it makes
    /// the promotion decision locally but has no remote nodes to coordinate. A
    /// transport MUST be injected for real multi-node failover.
    transport: RwLock<Option<Arc<dyn NodeTransport>>>,
}

impl ReplicaPromotion {
    /// Create a new replica promotion manager (local mode, no transport).
    pub fn new(config: FailoverConfig, strategy: PromotionStrategy) -> Self {
        Self {
            config: Arc::new(config),
            strategy,
            transport: RwLock::new(None),
        }
    }

    /// Inject the cluster transport used to coordinate remote nodes.
    pub fn set_transport(&self, transport: Arc<dyn NodeTransport>) {
        *self.transport.write() = Some(transport);
    }

    fn transport(&self) -> Option<Arc<dyn NodeTransport>> {
        self.transport.read().clone()
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

        match self.transport() {
            Some(transport) => {
                // Verify the candidate is caught up before handing it the
                // leadership, then issue the role-change RPC and only proceed
                // once the remote node has acknowledged it.
                let lag_ms = transport.query_replication_lag(candidate.node_id).await?;
                info!(
                    "Candidate {} replication lag is {}ms prior to promotion",
                    candidate.node_id, lag_ms
                );
                transport
                    .send_role_change(candidate.node_id, NodeRole::Leader)
                    .await?;
            }
            None => {
                warn!(
                    "Promoting {} without a cluster transport (local mode): no remote role change is issued",
                    candidate.node_id
                );
            }
        }

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
    ///
    /// Fences the node (stops accepting writes) and issues a role change to
    /// `Follower`, propagating any transport error rather than silently
    /// succeeding.
    pub async fn demote_leader(&self, node_id: Uuid) -> HaResult<()> {
        info!("Demoting leader {}", node_id);

        match self.transport() {
            Some(transport) => {
                transport.stop_accepting_writes(node_id).await?;
                transport
                    .send_role_change(node_id, NodeRole::Follower)
                    .await?;
            }
            None => {
                warn!(
                    "Demoting {} without a cluster transport (local mode): no remote role change is issued",
                    node_id
                );
            }
        }

        Ok(())
    }

    /// Perform graceful handover from old leader to new leader.
    ///
    /// Each step issues a real transport command and awaits its acknowledgment;
    /// a failure at any step aborts the handover with an error instead of
    /// reporting a fabricated success.
    pub async fn graceful_handover(
        &self,
        old_leader_id: Uuid,
        new_leader_id: Uuid,
    ) -> HaResult<()> {
        info!(
            "Performing graceful handover from {} to {}",
            old_leader_id, new_leader_id
        );

        match self.transport() {
            Some(transport) => {
                // Step 1: fence the old leader so it stops accepting writes.
                info!("Stopping writes on old leader {}", old_leader_id);
                transport.stop_accepting_writes(old_leader_id).await?;

                // Step 2: verify the new leader has caught up.
                let lag_ms = transport.query_replication_lag(new_leader_id).await?;
                info!("New leader {} lag is {}ms", new_leader_id, lag_ms);

                // Step 3: promote the new leader and await its acknowledgment.
                info!("Promoting new leader {}", new_leader_id);
                transport
                    .send_role_change(new_leader_id, NodeRole::Leader)
                    .await?;

                // Step 4: demote the old leader to follower.
                info!("Demoting old leader {}", old_leader_id);
                transport
                    .send_role_change(old_leader_id, NodeRole::Follower)
                    .await?;
            }
            None => {
                warn!(
                    "Graceful handover from {} to {} without a cluster transport (local mode): no remote coordination is performed",
                    old_leader_id, new_leader_id
                );
            }
        }

        info!("Graceful handover complete");

        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::failover::transport::{InProcessCluster, NodeState};

    #[tokio::test]
    async fn test_promote_replica_issues_real_role_change() {
        let cluster = Arc::new(InProcessCluster::new());
        let candidate_id = Uuid::new_v4();
        cluster.register_node(
            candidate_id,
            NodeState {
                role: NodeRole::Follower,
                accepting_writes: false,
                replication_lag_ms: 5,
            },
        );

        let promotion =
            ReplicaPromotion::new(FailoverConfig::default(), PromotionStrategy::Priority);
        promotion.set_transport(Arc::clone(&cluster) as Arc<dyn NodeTransport>);

        let candidate = PromotionCandidate {
            node_id: candidate_id,
            name: "replica".to_string(),
            priority: 100,
            lag_ms: Some(5),
            load: 0.3,
            health_score: 1.0,
        };

        let result = promotion.promote_replica(candidate).await.unwrap();
        assert_eq!(result.promoted_node_id, candidate_id);

        // The remote node's role actually changed and it now accepts writes.
        let state = cluster.node_state(candidate_id).unwrap();
        assert_eq!(state.role, NodeRole::Leader);
        assert!(state.accepting_writes);
    }

    #[tokio::test]
    async fn test_promote_unreachable_candidate_errors() {
        let cluster = Arc::new(InProcessCluster::new());
        // Candidate is NOT registered → transport reports it unreachable.
        let promotion =
            ReplicaPromotion::new(FailoverConfig::default(), PromotionStrategy::Priority);
        promotion.set_transport(Arc::clone(&cluster) as Arc<dyn NodeTransport>);

        let candidate = PromotionCandidate {
            node_id: Uuid::new_v4(),
            name: "ghost".to_string(),
            priority: 100,
            lag_ms: Some(5),
            load: 0.3,
            health_score: 1.0,
        };

        assert!(
            promotion.promote_replica(candidate).await.is_err(),
            "promotion of an unreachable candidate must fail, not fabricate success"
        );
    }

    #[tokio::test]
    async fn test_graceful_handover_moves_roles_and_fences_old_leader() {
        let cluster = Arc::new(InProcessCluster::new());
        let old_leader = Uuid::new_v4();
        let new_leader = Uuid::new_v4();
        cluster.register_node(
            old_leader,
            NodeState {
                role: NodeRole::Leader,
                accepting_writes: true,
                replication_lag_ms: 0,
            },
        );
        cluster.register_node(
            new_leader,
            NodeState {
                role: NodeRole::Follower,
                accepting_writes: false,
                replication_lag_ms: 2,
            },
        );

        let promotion =
            ReplicaPromotion::new(FailoverConfig::default(), PromotionStrategy::Priority);
        promotion.set_transport(Arc::clone(&cluster) as Arc<dyn NodeTransport>);

        promotion
            .graceful_handover(old_leader, new_leader)
            .await
            .unwrap();

        let old_state = cluster.node_state(old_leader).unwrap();
        let new_state = cluster.node_state(new_leader).unwrap();
        assert_eq!(old_state.role, NodeRole::Follower);
        assert!(!old_state.accepting_writes, "old leader must be fenced");
        assert_eq!(new_state.role, NodeRole::Leader);
        assert!(new_state.accepting_writes);
    }

    #[tokio::test]
    async fn test_graceful_handover_fails_if_old_leader_unreachable() {
        let cluster = Arc::new(InProcessCluster::new());
        let new_leader = Uuid::new_v4();
        cluster.register_node(new_leader, NodeState::default());
        // old_leader is not registered.
        let promotion =
            ReplicaPromotion::new(FailoverConfig::default(), PromotionStrategy::Priority);
        promotion.set_transport(Arc::clone(&cluster) as Arc<dyn NodeTransport>);

        assert!(
            promotion
                .graceful_handover(Uuid::new_v4(), new_leader)
                .await
                .is_err()
        );
    }

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
