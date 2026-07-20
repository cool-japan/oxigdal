//! Cluster coordinator with leader election and membership management.
//!
//! This module implements cluster coordination including Raft-based consensus,
//! leader election, membership management, configuration distribution, and
//! health check aggregation.

use crate::error::{ClusterError, Result};
use crate::transport::{NodeTransport, UnconfiguredTransport, VoteRequest, VoteResponse};
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::Notify;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Cluster coordinator.
#[derive(Clone)]
pub struct ClusterCoordinator {
    inner: Arc<CoordinatorInner>,
}

struct CoordinatorInner {
    /// Node ID (this coordinator's ID)
    node_id: NodeId,

    /// Cluster state
    state: Arc<RwLock<ClusterState>>,

    /// Member registry
    members: DashMap<NodeId, ClusterMember>,

    /// Configuration store
    config_store: Arc<RwLock<HashMap<String, Vec<u8>>>>,

    /// Leader state
    leader_state: Arc<RwLock<LeaderState>>,

    /// Configuration
    config: CoordinatorConfig,

    /// Running flag
    running: AtomicBool,

    /// Health check notification
    health_notify: Arc<Notify>,

    /// Statistics
    stats: Arc<CoordinatorStats>,

    /// Transport used to send consensus RPCs (vote requests) to peers.
    transport: Arc<dyn NodeTransport>,

    /// Persisted vote record: term -> candidate this node voted for in that term.
    ///
    /// Enforces the Raft invariant of at most one vote granted per term, so a
    /// peer cannot be double-counted across concurrent candidates.
    voted_for: Arc<RwLock<HashMap<u64, NodeId>>>,
}

/// Coordinator configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinatorConfig {
    /// Election timeout
    pub election_timeout: Duration,

    /// Heartbeat interval
    pub heartbeat_interval: Duration,

    /// Health check interval
    pub health_check_interval: Duration,

    /// Member timeout
    pub member_timeout: Duration,

    /// Configuration sync interval
    pub config_sync_interval: Duration,

    /// Minimum cluster size
    pub min_cluster_size: usize,
}

impl Default for CoordinatorConfig {
    fn default() -> Self {
        Self {
            election_timeout: Duration::from_secs(5),
            heartbeat_interval: Duration::from_secs(1),
            health_check_interval: Duration::from_secs(10),
            member_timeout: Duration::from_secs(30),
            config_sync_interval: Duration::from_secs(60),
            min_cluster_size: 3,
        }
    }
}

/// Node identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub Uuid);

impl NodeId {
    /// Create a new random node ID.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for NodeId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Cluster state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterState {
    /// Current term
    pub term: u64,

    /// Current leader
    pub leader: Option<NodeId>,

    /// Node role
    pub role: NodeRole,

    /// Last heartbeat from leader
    #[serde(skip)]
    pub last_leader_heartbeat: Option<Instant>,

    /// Election in progress
    pub election_in_progress: bool,
}

/// Node role in cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeRole {
    /// Follower (default state)
    Follower,

    /// Candidate (during election)
    Candidate,

    /// Leader (elected)
    Leader,
}

/// Leader state (only for leader node).
#[derive(Debug, Clone, Default)]
pub struct LeaderState {
    /// Elected at
    pub elected_at: Option<Instant>,

    /// Last heartbeat sent
    pub last_heartbeat_sent: Option<Instant>,

    /// Follower state
    pub followers: HashMap<NodeId, FollowerState>,
}

/// Follower state (tracked by leader).
#[derive(Debug, Clone)]
pub struct FollowerState {
    /// Last heartbeat received
    pub last_heartbeat: Instant,

    /// Acknowledged term
    pub acked_term: u64,

    /// Health status
    pub healthy: bool,
}

/// Cluster member information.
#[derive(Debug, Clone)]
pub struct ClusterMember {
    /// Node ID
    pub node_id: NodeId,

    /// Address
    pub address: String,

    /// Role
    pub role: NodeRole,

    /// Status
    pub status: MemberStatus,

    /// Joined at
    pub joined_at: Instant,

    /// Last seen
    pub last_seen: Instant,

    /// Version
    pub version: String,

    /// Metadata
    pub metadata: HashMap<String, String>,
}

/// Member status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemberStatus {
    /// Member is active
    Active,

    /// Member is suspected to be down
    Suspected,

    /// Member is confirmed down
    Down,

    /// Member left gracefully
    Left,
}

/// Coordinator statistics.
#[derive(Debug, Default)]
struct CoordinatorStats {
    /// Elections conducted
    elections: AtomicU64,

    /// Term changes
    term_changes: AtomicU64,

    /// Leadership changes
    leadership_changes: AtomicU64,

    /// Heartbeats sent
    heartbeats_sent: AtomicU64,

    /// Config syncs
    config_syncs: AtomicU64,

    /// Health checks
    health_checks: AtomicU64,
}

impl ClusterCoordinator {
    /// Create a new cluster coordinator.
    ///
    /// The coordinator uses [`UnconfiguredTransport`] and therefore cannot reach
    /// remote peers: it is safe by construction and will not fabricate a quorum.
    /// Wire a real network transport with [`ClusterCoordinator::with_transport`]
    /// to enable genuine multi-node leader election.
    pub fn new(config: CoordinatorConfig) -> Self {
        Self::with_transport(config, Arc::new(UnconfiguredTransport))
    }

    /// Create a coordinator backed by a specific [`NodeTransport`] implementation.
    pub fn with_transport(config: CoordinatorConfig, transport: Arc<dyn NodeTransport>) -> Self {
        let node_id = NodeId::new();

        Self {
            inner: Arc::new(CoordinatorInner {
                node_id,
                state: Arc::new(RwLock::new(ClusterState {
                    term: 0,
                    leader: None,
                    role: NodeRole::Follower,
                    last_leader_heartbeat: None,
                    election_in_progress: false,
                })),
                members: DashMap::new(),
                config_store: Arc::new(RwLock::new(HashMap::new())),
                leader_state: Arc::new(RwLock::new(LeaderState::default())),
                config,
                running: AtomicBool::new(false),
                health_notify: Arc::new(Notify::new()),
                stats: Arc::new(CoordinatorStats::default()),
                transport,
                voted_for: Arc::new(RwLock::new(HashMap::new())),
            }),
        }
    }

    /// Create with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(CoordinatorConfig::default())
    }

    /// Get this node's ID.
    pub fn node_id(&self) -> NodeId {
        self.inner.node_id
    }

    /// Start the coordinator.
    pub async fn start(&self) -> Result<()> {
        if self.inner.running.swap(true, Ordering::SeqCst) {
            return Err(ClusterError::InvalidState(
                "Coordinator already running".to_string(),
            ));
        }

        info!(
            "Starting cluster coordinator (node: {})",
            self.inner.node_id
        );

        // Spawn coordinator loops
        let coord = self.clone();
        tokio::spawn(async move {
            coord.run_coordinator_loop().await;
        });

        let coord = self.clone();
        tokio::spawn(async move {
            coord.run_health_check_loop().await;
        });

        Ok(())
    }

    /// Stop the coordinator.
    pub async fn stop(&self) -> Result<()> {
        info!("Stopping cluster coordinator");
        self.inner.running.store(false, Ordering::SeqCst);
        self.inner.health_notify.notify_waiters();
        Ok(())
    }

    /// Main coordinator loop.
    async fn run_coordinator_loop(&self) {
        let mut heartbeat_interval = tokio::time::interval(self.inner.config.heartbeat_interval);

        while self.inner.running.load(Ordering::SeqCst) {
            tokio::select! {
                _ = heartbeat_interval.tick() => {
                    let state = self.inner.state.read().clone();

                    match state.role {
                        NodeRole::Leader => {
                            // Send heartbeats as leader
                            if let Err(e) = self.send_leader_heartbeats().await {
                                error!("Failed to send leader heartbeats: {}", e);
                            }
                        }
                        NodeRole::Follower => {
                            // Check for election timeout
                            if self.should_start_election()
                                && let Err(e) = self.start_election().await {
                                    error!("Failed to start election: {}", e);
                                }
                        }
                        NodeRole::Candidate => {
                            // Election in progress, handled separately
                        }
                    }
                }
            }
        }

        info!("Coordinator loop stopped");
    }

    /// Health check loop.
    async fn run_health_check_loop(&self) {
        let mut interval = tokio::time::interval(self.inner.config.health_check_interval);

        while self.inner.running.load(Ordering::SeqCst) {
            interval.tick().await;

            if let Err(e) = self.check_member_health().await {
                error!("Health check failed: {}", e);
            }

            self.inner
                .stats
                .health_checks
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Check if election should be started.
    fn should_start_election(&self) -> bool {
        let state = self.inner.state.read();

        if state.election_in_progress {
            return false;
        }

        if let Some(last_heartbeat) = state.last_leader_heartbeat {
            if last_heartbeat.elapsed() > self.inner.config.election_timeout {
                return true;
            }
        } else {
            // No leader heartbeat received, start election
            return true;
        }

        false
    }

    /// Start leader election.
    async fn start_election(&self) -> Result<()> {
        // Known cluster size including this node.
        let total_members = self.inner.members.len() + 1; // +1 for self

        // Safety guard: refuse to run an election when the known cluster is smaller
        // than the configured minimum. This blocks a lone or nearly-isolated node
        // from crowning itself while the rest of the cluster is unreachable.
        if total_members < self.inner.config.min_cluster_size {
            debug!(
                "Known cluster size {} is below min_cluster_size {}; skipping election",
                total_members, self.inner.config.min_cluster_size
            );
            let mut state = self.inner.state.write();
            state.role = NodeRole::Follower;
            state.election_in_progress = false;
            return Ok(());
        }

        info!("Starting leader election");

        let term = {
            let mut state = self.inner.state.write();
            state.term += 1;
            state.role = NodeRole::Candidate;
            state.election_in_progress = true;
            state.leader = None;
            state.term
        }; // Lock is dropped here

        // This node votes for itself in this term.
        self.inner
            .voted_for
            .write()
            .insert(term, self.inner.node_id);

        self.inner.stats.elections.fetch_add(1, Ordering::Relaxed);

        self.inner
            .stats
            .term_changes
            .fetch_add(1, Ordering::Relaxed);

        // Request votes from other members via the transport.
        let votes = self.request_votes(term).await?;

        // A higher term seen during vote collection makes us step down; abort.
        {
            let state = self.inner.state.read();
            if state.role != NodeRole::Candidate || state.term != term {
                return Ok(());
            }
        }

        // Win condition: a strict majority of the known cluster AND never fewer
        // than min_cluster_size participating votes. Both guards must hold, which is
        // what prevents a minority partition from ever electing a leader.
        let quorum = (total_members / 2) + 1;
        let required = quorum
            .max(self.inner.config.min_cluster_size)
            .min(total_members);

        if votes >= required {
            self.become_leader(term)?;
        } else {
            // Lost election, become follower
            let mut state = self.inner.state.write();
            state.role = NodeRole::Follower;
            state.election_in_progress = false;
        }

        Ok(())
    }

    /// Request votes from every known peer through the transport.
    ///
    /// Only votes actually granted by a reachable peer are counted; unreachable
    /// peers (transport errors or timeouts) count as no vote. If any peer reports a
    /// higher term, this node steps down and the returned tally is zero so the
    /// caller abandons the election.
    async fn request_votes(&self, term: u64) -> Result<usize> {
        let candidate_id = self.inner.node_id;

        // Snapshot all known peers regardless of locally-tracked status: a peer this
        // node believes is down may in fact be the one holding a fresher term.
        let peers: Vec<(NodeId, String)> = self
            .inner
            .members
            .iter()
            .map(|entry| (*entry.key(), entry.value().address.clone()))
            .collect();

        let timeout = self.inner.config.election_timeout;

        let requests = peers.into_iter().map(|(peer, address)| {
            let transport = Arc::clone(&self.inner.transport);
            let request = VoteRequest {
                term,
                candidate_id,
                last_log_index: 0,
                last_log_term: 0,
            };
            async move {
                match tokio::time::timeout(timeout, transport.request_vote(peer, &address, request))
                    .await
                {
                    Ok(Ok(response)) => Some(response),
                    Ok(Err(e)) => {
                        warn!("Vote request to {} failed: {}", peer, e);
                        None
                    }
                    Err(_) => {
                        warn!("Vote request to {} timed out", peer);
                        None
                    }
                }
            }
        });

        let responses: Vec<Option<VoteResponse>> = futures::future::join_all(requests).await;

        // Start with this node's own self-vote.
        let mut granted = 1usize;
        let mut highest_term = term;

        for response in responses.into_iter().flatten() {
            if response.term > highest_term {
                highest_term = response.term;
            }
            // Only count a grant from a peer that is on our term (or older).
            if response.vote_granted && response.term <= term {
                granted += 1;
            }
        }

        if highest_term > term {
            warn!(
                "Observed higher term {} during election for term {}; stepping down",
                highest_term, term
            );
            self.step_down(highest_term);
            return Ok(0);
        }

        Ok(granted)
    }

    /// Step down to follower, adopting a newer term if one was observed.
    fn step_down(&self, new_term: u64) {
        let mut state = self.inner.state.write();
        if new_term > state.term {
            state.term = new_term;
            self.inner
                .stats
                .term_changes
                .fetch_add(1, Ordering::Relaxed);
        }
        state.role = NodeRole::Follower;
        state.election_in_progress = false;
        state.leader = None;
    }

    /// Handle an incoming vote request from a candidate peer (callee side).
    ///
    /// A real network transport dispatches received [`VoteRequest`] RPCs here. The
    /// vote is granted at most once per term (Raft's single-vote invariant),
    /// rejected outright when the candidate's term is stale, and this node adopts a
    /// newer term (stepping down) when the candidate is ahead.
    pub fn handle_vote_request(&self, request: VoteRequest) -> VoteResponse {
        let current_term = {
            let mut state = self.inner.state.write();

            // Reject candidates behind our current term.
            if request.term < state.term {
                return VoteResponse {
                    term: state.term,
                    vote_granted: false,
                };
            }

            // A newer term means we are stale: adopt it and revert to follower.
            if request.term > state.term {
                state.term = request.term;
                state.role = NodeRole::Follower;
                state.leader = None;
                state.election_in_progress = false;
                self.inner
                    .stats
                    .term_changes
                    .fetch_add(1, Ordering::Relaxed);
            }

            state.term
        };

        let mut voted_for = self.inner.voted_for.write();
        let vote_granted = match voted_for.get(&current_term) {
            // Idempotent: re-granting to the same candidate is safe; a different
            // candidate in the same term is refused.
            Some(existing) => *existing == request.candidate_id,
            None => {
                // No log replication layer yet, so the up-to-date-log check is a
                // no-op; last_log_index/last_log_term are carried for when it lands.
                voted_for.insert(current_term, request.candidate_id);
                true
            }
        };

        VoteResponse {
            term: current_term,
            vote_granted,
        }
    }

    /// Become the cluster leader.
    fn become_leader(&self, term: u64) -> Result<()> {
        info!("Became cluster leader for term {}", term);

        let mut state = self.inner.state.write();
        state.role = NodeRole::Leader;
        state.leader = Some(self.inner.node_id);
        state.election_in_progress = false;
        drop(state);

        let mut leader_state = self.inner.leader_state.write();
        leader_state.elected_at = Some(Instant::now());
        leader_state.followers.clear();

        // Initialize follower state for all members
        for entry in self.inner.members.iter() {
            leader_state.followers.insert(
                *entry.key(),
                FollowerState {
                    last_heartbeat: Instant::now(),
                    acked_term: term,
                    healthy: true,
                },
            );
        }

        drop(leader_state);

        self.inner
            .stats
            .leadership_changes
            .fetch_add(1, Ordering::Relaxed);

        Ok(())
    }

    /// Send heartbeats as leader.
    async fn send_leader_heartbeats(&self) -> Result<()> {
        let state = self.inner.state.read().clone();

        if state.role != NodeRole::Leader {
            return Ok(());
        }

        // In real implementation, send heartbeats to all followers
        // For now, just update timestamp

        let mut leader_state = self.inner.leader_state.write();
        leader_state.last_heartbeat_sent = Some(Instant::now());

        self.inner
            .stats
            .heartbeats_sent
            .fetch_add(1, Ordering::Relaxed);

        debug!("Sent leader heartbeats");

        Ok(())
    }

    /// Check member health.
    async fn check_member_health(&self) -> Result<()> {
        let now = Instant::now();
        let timeout = self.inner.config.member_timeout;

        for mut entry in self.inner.members.iter_mut() {
            let member = entry.value_mut();

            let age = now.duration_since(member.last_seen);

            if age > timeout {
                if member.status == MemberStatus::Active {
                    member.status = MemberStatus::Suspected;
                    warn!("Member {} suspected down", member.node_id);
                } else if member.status == MemberStatus::Suspected && age > timeout * 2 {
                    member.status = MemberStatus::Down;
                    warn!("Member {} confirmed down", member.node_id);
                }
            }
        }

        Ok(())
    }

    /// Register a new member.
    pub fn register_member(&self, member: ClusterMember) -> Result<()> {
        info!("Registering member: {}", member.node_id);

        self.inner.members.insert(member.node_id, member);

        Ok(())
    }

    /// Unregister a member.
    pub fn unregister_member(&self, node_id: NodeId) -> Result<()> {
        info!("Unregistering member: {}", node_id);

        if let Some((_, mut member)) = self.inner.members.remove(&node_id) {
            member.status = MemberStatus::Left;
        }

        // Remove from leader's follower list
        let mut leader_state = self.inner.leader_state.write();
        leader_state.followers.remove(&node_id);

        Ok(())
    }

    /// Get all members.
    pub fn get_members(&self) -> Vec<ClusterMember> {
        self.inner
            .members
            .iter()
            .map(|e| e.value().clone())
            .collect()
    }

    /// Get active members.
    pub fn get_active_members(&self) -> Vec<ClusterMember> {
        self.inner
            .members
            .iter()
            .filter(|e| e.value().status == MemberStatus::Active)
            .map(|e| e.value().clone())
            .collect()
    }

    /// Get current leader.
    pub fn get_leader(&self) -> Option<NodeId> {
        self.inner.state.read().leader
    }

    /// Check if this node is the leader.
    pub fn is_leader(&self) -> bool {
        let state = self.inner.state.read();
        state.role == NodeRole::Leader
    }

    /// Store configuration value.
    pub fn set_config(&self, key: String, value: Vec<u8>) -> Result<()> {
        let mut config = self.inner.config_store.write();
        config.insert(key.clone(), value);

        debug!("Stored config: {}", key);

        self.inner
            .stats
            .config_syncs
            .fetch_add(1, Ordering::Relaxed);

        Ok(())
    }

    /// Get configuration value.
    pub fn get_config(&self, key: &str) -> Option<Vec<u8>> {
        self.inner.config_store.read().get(key).cloned()
    }

    /// Get cluster statistics.
    pub fn get_statistics(&self) -> CoordinatorStatistics {
        let state = self.inner.state.read();

        CoordinatorStatistics {
            node_id: self.inner.node_id,
            role: state.role,
            current_term: state.term,
            current_leader: state.leader,
            total_members: self.inner.members.len(),
            active_members: self.get_active_members().len(),
            elections: self.inner.stats.elections.load(Ordering::Relaxed),
            term_changes: self.inner.stats.term_changes.load(Ordering::Relaxed),
            leadership_changes: self.inner.stats.leadership_changes.load(Ordering::Relaxed),
            heartbeats_sent: self.inner.stats.heartbeats_sent.load(Ordering::Relaxed),
            config_syncs: self.inner.stats.config_syncs.load(Ordering::Relaxed),
            health_checks: self.inner.stats.health_checks.load(Ordering::Relaxed),
        }
    }
}

/// Coordinator statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinatorStatistics {
    /// This node's ID
    pub node_id: NodeId,

    /// Current role
    pub role: NodeRole,

    /// Current term
    pub current_term: u64,

    /// Current leader
    pub current_leader: Option<NodeId>,

    /// Total members
    pub total_members: usize,

    /// Active members
    pub active_members: usize,

    /// Elections conducted
    pub elections: u64,

    /// Term changes
    pub term_changes: u64,

    /// Leadership changes
    pub leadership_changes: u64,

    /// Heartbeats sent
    pub heartbeats_sent: u64,

    /// Config syncs
    pub config_syncs: u64,

    /// Health checks
    pub health_checks: u64,
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    /// Shared in-memory registry of coordinators, keyed by node id, each tagged
    /// with a partition group. Peers can only reach peers in the same group.
    #[derive(Clone, Default)]
    struct MockNet {
        handlers: Arc<RwLock<HashMap<NodeId, (usize, ClusterCoordinator)>>>,
    }

    impl MockNet {
        fn new() -> Self {
            Self::default()
        }

        fn register(&self, id: NodeId, group: usize, coord: ClusterCoordinator) {
            self.handlers.write().insert(id, (group, coord));
        }

        fn lookup(&self, id: NodeId) -> Option<(usize, ClusterCoordinator)> {
            self.handlers.read().get(&id).cloned()
        }
    }

    /// Transport that routes a vote request to the target coordinator's real
    /// `handle_vote_request`, but only if the target shares this node's partition
    /// group; otherwise it reports a network error (simulating a partition).
    struct MockTransport {
        net: MockNet,
        group: usize,
    }

    #[async_trait]
    impl NodeTransport for MockTransport {
        async fn request_vote(
            &self,
            peer: NodeId,
            _peer_address: &str,
            request: VoteRequest,
        ) -> Result<VoteResponse> {
            let (peer_group, handler) = self
                .net
                .lookup(peer)
                .ok_or_else(|| ClusterError::NetworkError(format!("unknown peer {peer}")))?;
            if peer_group != self.group {
                return Err(ClusterError::NetworkError(format!(
                    "partitioned from {peer}"
                )));
            }
            Ok(handler.handle_vote_request(request))
        }
    }

    fn make_member(node_id: NodeId) -> ClusterMember {
        ClusterMember {
            node_id,
            address: format!("mock://{node_id}"),
            role: NodeRole::Follower,
            status: MemberStatus::Active,
            joined_at: Instant::now(),
            last_seen: Instant::now(),
            version: "test".to_string(),
            metadata: HashMap::new(),
        }
    }

    /// Build `groups.len()` coordinators wired through a shared MockNet, each in
    /// the given partition group, fully cross-registered as members.
    fn build_cluster(groups: &[usize], config: CoordinatorConfig) -> Vec<ClusterCoordinator> {
        let net = MockNet::new();
        let mut coords = Vec::new();
        for &g in groups {
            let transport = Arc::new(MockTransport {
                net: net.clone(),
                group: g,
            });
            let coord = ClusterCoordinator::with_transport(config.clone(), transport);
            net.register(coord.node_id(), g, coord.clone());
            coords.push(coord);
        }
        for i in 0..coords.len() {
            for j in 0..coords.len() {
                if i != j {
                    coords[i]
                        .register_member(make_member(coords[j].node_id()))
                        .ok();
                }
            }
        }
        coords
    }

    #[tokio::test]
    async fn test_partition_prevents_split_brain() {
        let config = CoordinatorConfig {
            min_cluster_size: 3,
            ..Default::default()
        };
        // Nodes 0,1,2 form the majority partition; nodes 3,4 the minority.
        let coords = build_cluster(&[0, 0, 0, 1, 1], config);

        // Both partitions independently attempt to elect a leader in the same term.
        coords[0]
            .start_election()
            .await
            .expect("majority election should run");
        coords[3]
            .start_election()
            .await
            .expect("minority election should run");

        assert!(
            coords[0].is_leader(),
            "majority partition should elect a leader"
        );
        assert!(
            !coords[3].is_leader(),
            "minority partition must not elect a leader"
        );

        let leaders = coords.iter().filter(|c| c.is_leader()).count();
        assert_eq!(leaders, 1, "split-brain: more than one leader elected");
    }

    #[tokio::test]
    async fn test_full_cluster_elects_single_leader() {
        let config = CoordinatorConfig {
            min_cluster_size: 3,
            ..Default::default()
        };
        let coords = build_cluster(&[0, 0, 0, 0, 0], config);

        coords[0]
            .start_election()
            .await
            .expect("election should run");

        assert!(coords[0].is_leader());
        assert_eq!(coords.iter().filter(|c| c.is_leader()).count(), 1);
    }

    #[tokio::test]
    async fn test_unconfigured_transport_never_fabricates_quorum() {
        // Default coordinator uses UnconfiguredTransport: it can reach no peer.
        let coord = ClusterCoordinator::with_defaults();
        for _ in 0..4 {
            coord.register_member(make_member(NodeId::new())).ok();
        }

        coord.start_election().await.expect("election should run");

        assert!(
            !coord.is_leader(),
            "must not become leader from locally-fabricated votes"
        );
    }

    #[tokio::test]
    async fn test_below_min_cluster_size_skips_election() {
        let config = CoordinatorConfig {
            min_cluster_size: 5,
            ..Default::default()
        };
        let coord = ClusterCoordinator::new(config);
        coord.register_member(make_member(NodeId::new())).ok(); // total known = 2 < 5

        coord.start_election().await.expect("should return Ok");

        assert!(!coord.is_leader());
        let stats = coord.get_statistics();
        assert_eq!(
            stats.elections, 0,
            "election must be skipped when below min_cluster_size"
        );
    }

    #[test]
    fn test_handle_vote_request_single_vote_per_term() {
        let coord = ClusterCoordinator::with_defaults();
        let candidate_a = NodeId::new();
        let candidate_b = NodeId::new();

        let req = |term: u64, id: NodeId| VoteRequest {
            term,
            candidate_id: id,
            last_log_index: 0,
            last_log_term: 0,
        };

        // First vote in term 1 is granted.
        assert!(coord.handle_vote_request(req(1, candidate_a)).vote_granted);
        // Re-asking with the same candidate is idempotent.
        assert!(coord.handle_vote_request(req(1, candidate_a)).vote_granted);
        // A different candidate in the same term is refused.
        assert!(!coord.handle_vote_request(req(1, candidate_b)).vote_granted);
        // A stale term is refused.
        assert!(!coord.handle_vote_request(req(0, candidate_b)).vote_granted);
        // A higher term is granted and adopted.
        let resp = coord.handle_vote_request(req(2, candidate_b));
        assert!(resp.vote_granted);
        assert_eq!(resp.term, 2);
    }

    #[test]
    fn test_coordinator_creation() {
        let coord = ClusterCoordinator::with_defaults();
        let node_id = coord.node_id();
        assert_ne!(node_id.0, Uuid::nil());
    }

    #[test]
    fn test_member_registration() {
        let coord = ClusterCoordinator::with_defaults();

        let member = ClusterMember {
            node_id: NodeId::new(),
            address: "localhost:8080".to_string(),
            role: NodeRole::Follower,
            status: MemberStatus::Active,
            joined_at: Instant::now(),
            last_seen: Instant::now(),
            version: "1.0.0".to_string(),
            metadata: HashMap::new(),
        };

        coord.register_member(member.clone()).ok();

        let members = coord.get_members();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].node_id, member.node_id);
    }

    #[test]
    fn test_config_storage() {
        let coord = ClusterCoordinator::with_defaults();

        coord.set_config("test_key".to_string(), vec![1, 2, 3]).ok();

        let value = coord.get_config("test_key");
        assert_eq!(value, Some(vec![1, 2, 3]));
    }

    #[tokio::test]
    async fn test_coordinator_start_stop() {
        let coord = ClusterCoordinator::with_defaults();

        let start_result = coord.start().await;
        assert!(start_result.is_ok());

        let stop_result = coord.stop().await;
        assert!(stop_result.is_ok());
    }
}
