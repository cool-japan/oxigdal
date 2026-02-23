//! Cluster coordinator with leader election and membership management.
//!
//! This module implements cluster coordination including Raft-based consensus,
//! leader election, membership management, configuration distribution, and
//! health check aggregation.

use crate::error::{ClusterError, Result};
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
    pub fn new(config: CoordinatorConfig) -> Self {
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
                            if self.should_start_election() {
                                if let Err(e) = self.start_election().await {
                                    error!("Failed to start election: {}", e);
                                }
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
        info!("Starting leader election");

        let term = {
            let mut state = self.inner.state.write();
            state.term += 1;
            state.role = NodeRole::Candidate;
            state.election_in_progress = true;
            state.term
        }; // Lock is dropped here

        self.inner.stats.elections.fetch_add(1, Ordering::Relaxed);

        self.inner
            .stats
            .term_changes
            .fetch_add(1, Ordering::Relaxed);

        // Request votes from other members
        let votes = self.request_votes(term).await?;

        // Check if we won the election
        let total_members = self.inner.members.len() + 1; // +1 for self
        let quorum = (total_members / 2) + 1;

        if votes >= quorum {
            self.become_leader(term)?;
        } else {
            // Lost election, become follower
            let mut state = self.inner.state.write();
            state.role = NodeRole::Follower;
            state.election_in_progress = false;
        }

        Ok(())
    }

    /// Request votes from other members.
    async fn request_votes(&self, _term: u64) -> Result<usize> {
        // In a real implementation, this would send vote requests to other nodes
        // For now, simulate by checking cluster size

        let active_members = self
            .inner
            .members
            .iter()
            .filter(|entry| entry.value().status == MemberStatus::Active)
            .count();

        // Assume we get votes from active members (simplified)
        Ok(active_members + 1) // +1 for self-vote
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
