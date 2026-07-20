//! Cluster transport abstractions for failover.
//!
//! The failover machinery (leader election, replica promotion, graceful
//! handover) needs to talk to remote nodes over some wire protocol. To keep
//! the crate transport-agnostic (and Pure-Rust per COOLJAPAN policy — no
//! tonic/gRPC C toolchain dependencies), the concrete transport is injected via
//! the [`ElectionTransport`] and [`NodeTransport`] traits.
//!
//! An embedding application supplies a real implementation (e.g. HTTP over an
//! oxi\* stack, QUIC, or a message bus). For single-process deployments, tests,
//! and local clusters, [`InProcessCluster`] provides a fully functional
//! in-memory transport that actually delivers vote requests and role-change
//! commands between node instances running in the same process.

use super::NodeRole;
use super::election::{VoteRequest, VoteResponse};
use crate::error::{HaError, HaResult};
use async_trait::async_trait;
use dashmap::DashMap;
use parking_lot::RwLock;
use std::sync::{Arc, Weak};
use uuid::Uuid;

/// Transport used by leader election to request votes from peer nodes.
#[async_trait]
pub trait ElectionTransport: Send + Sync {
    /// Send a [`VoteRequest`] to `peer` and await its [`VoteResponse`].
    ///
    /// Implementations must return an error if the peer is unreachable so the
    /// candidate correctly counts it as a missing (non-granted) vote rather
    /// than fabricating a grant.
    async fn request_vote(&self, peer: Uuid, request: VoteRequest) -> HaResult<VoteResponse>;
}

/// Transport used by promotion / graceful handover to change remote node roles.
#[async_trait]
pub trait NodeTransport: Send + Sync {
    /// Instruct `node_id` to stop accepting writes (fence the old leader).
    async fn stop_accepting_writes(&self, node_id: Uuid) -> HaResult<()>;

    /// Query the current replication lag (ms) of `node_id`.
    async fn query_replication_lag(&self, node_id: Uuid) -> HaResult<u64>;

    /// Instruct `node_id` to assume `role` (e.g. become Leader/Follower).
    async fn send_role_change(&self, node_id: Uuid, role: NodeRole) -> HaResult<()>;
}

/// Handler that can respond to an incoming vote request.
///
/// Implemented by [`super::election::LeaderElection`] so the in-process
/// transport can route a candidate's [`VoteRequest`] to a peer's real election
/// state machine.
#[async_trait]
pub trait VoteHandler: Send + Sync {
    /// Handle an incoming vote request and produce a response.
    async fn handle_vote_request(&self, request: VoteRequest) -> HaResult<VoteResponse>;
}

/// Observable per-node state maintained by [`InProcessCluster`].
#[derive(Debug, Clone)]
pub struct NodeState {
    /// Current role of the node.
    pub role: NodeRole,
    /// Whether the node currently accepts writes.
    pub accepting_writes: bool,
    /// Current replication lag in milliseconds.
    pub replication_lag_ms: u64,
}

impl Default for NodeState {
    fn default() -> Self {
        Self {
            role: NodeRole::Follower,
            accepting_writes: false,
            replication_lag_ms: 0,
        }
    }
}

/// In-process cluster transport.
///
/// Routes vote requests to registered [`VoteHandler`]s and applies role-change /
/// write-fencing / lag-query commands to registered [`NodeState`]s. This is a
/// real, working transport for same-process clusters and for tests — it does
/// not simulate success; a command to an unregistered node returns a
/// [`HaError::Network`] error just like an unreachable remote node would.
#[derive(Default)]
pub struct InProcessCluster {
    /// Vote handlers keyed by node id (weak to avoid Arc cycles with nodes).
    vote_handlers: DashMap<Uuid, Weak<dyn VoteHandler>>,
    /// Observable node states keyed by node id.
    node_states: DashMap<Uuid, Arc<RwLock<NodeState>>>,
}

impl InProcessCluster {
    /// Create a new empty in-process cluster.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a node's vote handler so peers can request votes from it.
    pub fn register_vote_handler(&self, node_id: Uuid, handler: &Arc<dyn VoteHandler>) {
        self.vote_handlers.insert(node_id, Arc::downgrade(handler));
    }

    /// Register (or replace) a node's observable state.
    pub fn register_node(&self, node_id: Uuid, state: NodeState) {
        self.node_states
            .insert(node_id, Arc::new(RwLock::new(state)));
    }

    /// Get a snapshot of a node's current state, if registered.
    pub fn node_state(&self, node_id: Uuid) -> Option<NodeState> {
        self.node_states.get(&node_id).map(|s| s.read().clone())
    }

    fn state_handle(&self, node_id: Uuid) -> HaResult<Arc<RwLock<NodeState>>> {
        self.node_states
            .get(&node_id)
            .map(|s| Arc::clone(s.value()))
            .ok_or_else(|| HaError::Network(format!("node {node_id} not reachable in cluster")))
    }
}

#[async_trait]
impl ElectionTransport for InProcessCluster {
    async fn request_vote(&self, peer: Uuid, request: VoteRequest) -> HaResult<VoteResponse> {
        let handler = self
            .vote_handlers
            .get(&peer)
            .and_then(|w| w.upgrade())
            .ok_or_else(|| HaError::Network(format!("peer {peer} not reachable for vote")))?;
        handler.handle_vote_request(request).await
    }
}

#[async_trait]
impl NodeTransport for InProcessCluster {
    async fn stop_accepting_writes(&self, node_id: Uuid) -> HaResult<()> {
        let state = self.state_handle(node_id)?;
        state.write().accepting_writes = false;
        Ok(())
    }

    async fn query_replication_lag(&self, node_id: Uuid) -> HaResult<u64> {
        let state = self.state_handle(node_id)?;
        let lag = state.read().replication_lag_ms;
        Ok(lag)
    }

    async fn send_role_change(&self, node_id: Uuid, role: NodeRole) -> HaResult<()> {
        let state = self.state_handle(node_id)?;
        let mut guard = state.write();
        guard.role = role;
        // A newly promoted leader begins accepting writes; a demoted node stops.
        guard.accepting_writes = role == NodeRole::Leader;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_role_change_and_fencing() {
        let cluster = InProcessCluster::new();
        let node = Uuid::new_v4();
        cluster.register_node(
            node,
            NodeState {
                role: NodeRole::Follower,
                accepting_writes: false,
                replication_lag_ms: 3,
            },
        );

        assert_eq!(cluster.query_replication_lag(node).await.unwrap(), 3);

        cluster
            .send_role_change(node, NodeRole::Leader)
            .await
            .unwrap();
        let state = cluster.node_state(node).unwrap();
        assert_eq!(state.role, NodeRole::Leader);
        assert!(state.accepting_writes);

        cluster.stop_accepting_writes(node).await.unwrap();
        assert!(!cluster.node_state(node).unwrap().accepting_writes);
    }

    #[tokio::test]
    async fn test_unreachable_node_errors() {
        let cluster = InProcessCluster::new();
        let missing = Uuid::new_v4();
        assert!(cluster.query_replication_lag(missing).await.is_err());
        assert!(
            cluster
                .send_role_change(missing, NodeRole::Leader)
                .await
                .is_err()
        );
    }
}
