//! Leader election implementation (Raft-based).

use super::{FailoverConfig, NodeRole};
use crate::error::{HaError, HaResult};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::Notify;
use tokio::time::{Duration, sleep};
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
            shutdown: Arc::new(Notify::new()),
        }
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

        sleep(election_timeout).await;

        let votes_count = self.votes_received.len();
        let total_nodes = votes_count;

        let majority = (total_nodes / 2) + 1;
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

#[cfg(test)]
mod tests {
    use super::*;

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
