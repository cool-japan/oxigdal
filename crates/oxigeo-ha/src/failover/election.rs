//! Leader election implementation (Raft-style term/vote state machine).
//!
//! This module implements the *leader election* half of Raft: a quorum-based
//! term/vote state machine with `VoteRequest`/`VoteResponse`, majority computed
//! from the real cluster membership, and transport-backed vote broadcast. The
//! *log replication* half of Raft (AppendEntries with log-consistency checks,
//! commit-index advancement, and conflict truncation) lives in
//! [`super::log_replication`]. Deployments that only need active-active,
//! CRDT-style replication use [`crate::replication`] instead; deployments that
//! need linearizable, log-replicated consistency compose this election module
//! with [`super::log_replication::ReplicatedLog`].

use super::transport::{ElectionTransport, VoteHandler};
use super::{FailoverConfig, NodeRole};
use crate::error::{HaError, HaResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::Notify;
use tokio::time::{Duration, sleep, timeout};
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Election state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ElectionState {
    /// Not participating in election.
    Idle,
    /// Voting in progress.
    Voting,
    /// Election complete.
    Complete,
}

/// Vote request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoteRequest {
    /// Candidate ID.
    pub candidate_id: Uuid,
    /// Election term.
    pub term: u64,
    /// Candidate priority.
    pub priority: u32,
    /// Timestamp.
    pub timestamp: DateTime<Utc>,
}

/// Vote response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoteResponse {
    /// Voter node ID.
    pub voter_id: Uuid,
    /// Election term.
    pub term: u64,
    /// Vote granted.
    pub granted: bool,
    /// Timestamp.
    pub timestamp: DateTime<Utc>,
}

/// Election result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElectionResult {
    /// Winner node ID.
    pub winner_id: Uuid,
    /// Election term.
    pub term: u64,
    /// Total votes.
    pub total_votes: usize,
    /// Votes received.
    pub votes_received: usize,
    /// Election duration in milliseconds.
    pub duration_ms: u64,
    /// Timestamp.
    pub timestamp: DateTime<Utc>,
}

/// Leader election manager.
pub struct LeaderElection {
    /// Node ID.
    node_id: Uuid,
    /// Node priority.
    priority: u32,
    /// Configuration.
    config: Arc<RwLock<FailoverConfig>>,
    /// Current term.
    current_term: Arc<AtomicU64>,
    /// Current role.
    current_role: Arc<RwLock<NodeRole>>,
    /// Voted for in current term.
    voted_for: Arc<RwLock<Option<Uuid>>>,
    /// Current leader.
    current_leader: Arc<RwLock<Option<Uuid>>>,
    /// Election state.
    election_state: Arc<RwLock<ElectionState>>,
    /// Votes received.
    votes_received: Arc<DashMap<Uuid, VoteResponse>>,
    /// Known peer node IDs (the rest of the cluster, excluding self).
    peers: Arc<RwLock<Vec<Uuid>>>,
    /// Transport used to broadcast vote requests to peers.
    transport: Arc<RwLock<Option<Arc<dyn ElectionTransport>>>>,
    /// Shutdown notifier.
    shutdown: Arc<Notify>,
}

impl LeaderElection {
    /// Get the node priority.
    pub fn priority(&self) -> u32 {
        self.priority
    }

    /// Get the shutdown notifier (for external shutdown signaling).
    pub fn shutdown_notifier(&self) -> Arc<Notify> {
        Arc::clone(&self.shutdown)
    }
}

impl LeaderElection {
    /// Create a new leader election manager.
    pub fn new(node_id: Uuid, priority: u32, config: FailoverConfig) -> Self {
        Self {
            node_id,
            priority,
            config: Arc::new(RwLock::new(config)),
            current_term: Arc::new(AtomicU64::new(0)),
            current_role: Arc::new(RwLock::new(NodeRole::Follower)),
            voted_for: Arc::new(RwLock::new(None)),
            current_leader: Arc::new(RwLock::new(None)),
            election_state: Arc::new(RwLock::new(ElectionState::Idle)),
            votes_received: Arc::new(DashMap::new()),
            peers: Arc::new(RwLock::new(Vec::new())),
            transport: Arc::new(RwLock::new(None)),
            shutdown: Arc::new(Notify::new()),
        }
    }

    /// Set the known peer node IDs (the rest of the cluster, excluding self).
    ///
    /// The cluster size used to compute the election majority is
    /// `peers.len() + 1`. This MUST be set to the real cluster membership for
    /// quorum to be correct in a multi-node deployment.
    pub fn set_peers(&self, peers: Vec<Uuid>) {
        *self.peers.write() = peers;
    }

    /// Get the currently configured peer node IDs.
    pub fn peers(&self) -> Vec<Uuid> {
        self.peers.read().clone()
    }

    /// Inject the transport used to broadcast vote requests to peers.
    pub fn set_transport(&self, transport: Arc<dyn ElectionTransport>) {
        *self.transport.write() = Some(transport);
    }

    /// Get current role.
    pub fn get_role(&self) -> NodeRole {
        *self.current_role.read()
    }

    /// Get current leader.
    pub fn get_leader(&self) -> Option<Uuid> {
        *self.current_leader.read()
    }

    /// Get current term.
    pub fn get_term(&self) -> u64 {
        self.current_term.load(Ordering::SeqCst)
    }

    /// Start election.
    pub async fn start_election(&self) -> HaResult<ElectionResult> {
        let start_time = Utc::now();

        info!("Starting leader election (term {})", self.get_term() + 1);

        *self.election_state.write() = ElectionState::Voting;

        self.current_term.fetch_add(1, Ordering::SeqCst);
        *self.current_role.write() = NodeRole::Candidate;
        *self.voted_for.write() = Some(self.node_id);

        self.votes_received.clear();

        let self_vote = VoteResponse {
            voter_id: self.node_id,
            term: self.get_term(),
            granted: true,
            timestamp: Utc::now(),
        };
        self.votes_received.insert(self.node_id, self_vote);

        let election_timeout = {
            let config = self.config.read();
            Duration::from_millis(config.election_timeout_ms)
        };

        // The cluster size (and therefore the quorum) is derived from the real
        // known membership, NOT from however many votes happen to have arrived.
        let peers = self.peers.read().clone();
        let total_nodes = peers.len() + 1;
        let majority = (total_nodes / 2) + 1;

        let term = self.get_term();
        let transport = self.transport.read().clone();

        match transport {
            Some(transport) => {
                // Actively broadcast a VoteRequest to every known peer and
                // collect the granted responses (each bounded by the election
                // timeout so an unresponsive peer cannot stall the election).
                let request = VoteRequest {
                    candidate_id: self.node_id,
                    term,
                    priority: self.priority,
                    timestamp: Utc::now(),
                };

                let vote_futures = peers.into_iter().map(|peer| {
                    let transport = Arc::clone(&transport);
                    let request = request.clone();
                    async move {
                        match timeout(election_timeout, transport.request_vote(peer, request)).await
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

                let responses = futures::future::join_all(vote_futures).await;
                for response in responses.into_iter().flatten() {
                    self.handle_vote_response(response).await?;
                }
            }
            None => {
                // No transport wired: wait out the election timeout so that any
                // externally-driven VoteResponses (delivered via
                // `handle_vote_response`) have a chance to arrive before tally.
                sleep(election_timeout).await;
            }
        }

        let votes_count = self.votes_received.len();
        let won = votes_count >= majority;

        *self.election_state.write() = ElectionState::Complete;

        let duration_ms = (Utc::now() - start_time).num_milliseconds() as u64;

        if won {
            info!("Won election with {} votes", votes_count);
            *self.current_role.write() = NodeRole::Leader;
            *self.current_leader.write() = Some(self.node_id);

            Ok(ElectionResult {
                winner_id: self.node_id,
                term: self.get_term(),
                total_votes: total_nodes,
                votes_received: votes_count,
                duration_ms,
                timestamp: Utc::now(),
            })
        } else {
            warn!(
                "Lost election with {} votes (need {})",
                votes_count, majority
            );
            *self.current_role.write() = NodeRole::Follower;

            Err(HaError::LeaderElectionFailed(format!(
                "Not enough votes: {} < {}",
                votes_count, majority
            )))
        }
    }

    /// Handle vote request.
    pub async fn handle_vote_request(&self, request: VoteRequest) -> HaResult<VoteResponse> {
        debug!(
            "Received vote request from {} for term {}",
            request.candidate_id, request.term
        );

        let current_term = self.get_term();

        if request.term < current_term {
            return Ok(VoteResponse {
                voter_id: self.node_id,
                term: current_term,
                granted: false,
                timestamp: Utc::now(),
            });
        }

        if request.term > current_term {
            self.current_term.store(request.term, Ordering::SeqCst);
            *self.current_role.write() = NodeRole::Follower;
            *self.voted_for.write() = None;
        }

        let voted_for = *self.voted_for.read();

        let granted = match voted_for {
            None => {
                *self.voted_for.write() = Some(request.candidate_id);
                true
            }
            Some(id) if id == request.candidate_id => true,
            Some(_) => false,
        };

        Ok(VoteResponse {
            voter_id: self.node_id,
            term: self.get_term(),
            granted,
            timestamp: Utc::now(),
        })
    }

    /// Handle vote response.
    pub async fn handle_vote_response(&self, response: VoteResponse) -> HaResult<()> {
        if response.term > self.get_term() {
            self.current_term.store(response.term, Ordering::SeqCst);
            *self.current_role.write() = NodeRole::Follower;
            *self.voted_for.write() = None;
            return Ok(());
        }

        if response.term == self.get_term() && response.granted {
            debug!("Received vote from {}", response.voter_id);
            self.votes_received.insert(response.voter_id, response);
        }

        Ok(())
    }

    /// Step down from leadership.
    pub async fn step_down(&self) -> HaResult<()> {
        info!("Stepping down from leadership");

        *self.current_role.write() = NodeRole::Follower;
        *self.current_leader.write() = None;
        *self.voted_for.write() = None;

        Ok(())
    }

    /// Become leader (for testing/manual promotion).
    pub async fn become_leader(&self) -> HaResult<()> {
        info!("Becoming leader");

        *self.current_role.write() = NodeRole::Leader;
        *self.current_leader.write() = Some(self.node_id);

        Ok(())
    }
}

#[async_trait]
impl VoteHandler for LeaderElection {
    async fn handle_vote_request(&self, request: VoteRequest) -> HaResult<VoteResponse> {
        // Delegate to the inherent method (which drives the real term/vote
        // state machine); the trait exists so transports can route peer RPCs.
        LeaderElection::handle_vote_request(self, request).await
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::failover::transport::InProcessCluster;

    /// Build a candidate wired to `cluster` with the given peers, and register
    /// its own vote handler so peers can (reciprocally) reach it.
    fn wire_node(
        cluster: &Arc<InProcessCluster>,
        priority: u32,
        peers: Vec<Uuid>,
    ) -> Arc<LeaderElection> {
        let node = Arc::new(LeaderElection::new(
            Uuid::new_v4(),
            priority,
            FailoverConfig {
                election_timeout_ms: 200,
                ..Default::default()
            },
        ));
        node.set_peers(peers);
        node.set_transport(Arc::clone(cluster) as Arc<dyn ElectionTransport>);
        let handler: Arc<dyn VoteHandler> = Arc::clone(&node) as Arc<dyn VoteHandler>;
        cluster.register_vote_handler(node.node_id, &handler);
        node
    }

    #[tokio::test]
    async fn test_election_requires_real_peer_votes() {
        // Three-node cluster; only node A runs an election. B and C are
        // reachable and vote-idle, so they grant → A wins with a real quorum.
        let cluster = Arc::new(InProcessCluster::new());
        let ids: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();

        let a = Arc::new(LeaderElection::new(
            ids[0],
            100,
            FailoverConfig {
                election_timeout_ms: 200,
                ..Default::default()
            },
        ));
        a.set_peers(vec![ids[1], ids[2]]);
        a.set_transport(Arc::clone(&cluster) as Arc<dyn ElectionTransport>);

        let b = Arc::new(LeaderElection::new(ids[1], 90, FailoverConfig::default()));
        let c = Arc::new(LeaderElection::new(ids[2], 90, FailoverConfig::default()));
        let bh: Arc<dyn VoteHandler> = Arc::clone(&b) as Arc<dyn VoteHandler>;
        let ch: Arc<dyn VoteHandler> = Arc::clone(&c) as Arc<dyn VoteHandler>;
        cluster.register_vote_handler(ids[1], &bh);
        cluster.register_vote_handler(ids[2], &ch);

        let result = a.start_election().await.expect("A should win with 3 votes");
        assert_eq!(result.winner_id, ids[0]);
        assert_eq!(result.total_votes, 3);
        assert_eq!(result.votes_received, 3);
        assert_eq!(a.get_role(), NodeRole::Leader);
    }

    #[tokio::test]
    async fn test_candidate_with_no_reachable_peers_loses() {
        // A >1-node cluster where the two peers are NOT registered (partition):
        // the candidate must lose because it only has its own self-vote.
        let cluster = Arc::new(InProcessCluster::new());
        let node = wire_node(&cluster, 100, vec![Uuid::new_v4(), Uuid::new_v4()]);

        let result = node.start_election().await;
        assert!(
            result.is_err(),
            "candidate with only a self-vote in a 3-node cluster must lose"
        );
        assert_eq!(node.get_role(), NodeRole::Follower);
    }

    #[tokio::test]
    async fn test_single_node_cluster_self_elects() {
        // No peers configured → cluster size 1 → self-vote is a majority.
        let cluster = Arc::new(InProcessCluster::new());
        let node = wire_node(&cluster, 100, vec![]);

        let result = node
            .start_election()
            .await
            .expect("single node self-elects");
        assert_eq!(result.total_votes, 1);
        assert_eq!(node.get_role(), NodeRole::Leader);
    }

    #[tokio::test]
    async fn test_split_vote_denied_by_peer() {
        // Peer B has already voted for someone else this term, so it denies A's
        // request; with one grant short of quorum in a 3-node cluster, A loses.
        let cluster = Arc::new(InProcessCluster::new());
        let a_peers = vec![Uuid::new_v4(), Uuid::new_v4()];
        let a = wire_node(&cluster, 100, a_peers.clone());

        // Only one peer (a_peers[0]) is reachable and it has already granted its
        // vote for a different candidate at the same term.
        let b = Arc::new(LeaderElection::new(
            a_peers[0],
            90,
            FailoverConfig::default(),
        ));
        // Pre-commit B's vote to an unrelated candidate for the term A will use.
        let other_candidate = Uuid::new_v4();
        let _ = b
            .handle_vote_request(VoteRequest {
                candidate_id: other_candidate,
                term: 1,
                priority: 50,
                timestamp: Utc::now(),
            })
            .await
            .unwrap();
        let bh: Arc<dyn VoteHandler> = Arc::clone(&b) as Arc<dyn VoteHandler>;
        cluster.register_vote_handler(a_peers[0], &bh);
        // a_peers[1] is left unregistered (unreachable).

        let result = a.start_election().await;
        assert!(result.is_err(), "A should not reach quorum on a split vote");
    }

    #[tokio::test]
    async fn test_leader_election() {
        let config = FailoverConfig::default();
        let election = LeaderElection::new(Uuid::new_v4(), 100, config);

        assert_eq!(election.get_role(), NodeRole::Follower);
        assert_eq!(election.get_term(), 0);

        let request = VoteRequest {
            candidate_id: Uuid::new_v4(),
            term: 1,
            priority: 50,
            timestamp: Utc::now(),
        };

        let response = election.handle_vote_request(request).await.ok();
        assert!(response.is_some());

        if let Some(resp) = response {
            assert!(resp.granted);
        }
    }

    #[tokio::test]
    async fn test_become_leader() {
        let config = FailoverConfig::default();
        let election = LeaderElection::new(Uuid::new_v4(), 100, config);

        assert!(election.become_leader().await.is_ok());
        assert_eq!(election.get_role(), NodeRole::Leader);
        assert!(election.get_leader().is_some());
    }
}
