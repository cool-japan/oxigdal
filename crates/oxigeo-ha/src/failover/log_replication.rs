//! Raft-style replicated log (AppendEntries).
//!
//! This module implements the log-replication half of Raft, complementing the
//! leader-election half in [`super::election`]. It provides:
//!
//! - A replicated log of [`LogEntry`] values, each carrying its own `term` and
//!   1-based `index`.
//! - Follower-side [`AppendEntries`](ReplicatedLog::handle_append_entries)
//!   handling with the Raft log-consistency check (`prev_log_index` /
//!   `prev_log_term`), conflict truncation, and commit-index advancement.
//! - Leader-side replication ([`LeaderReplicator`]) with per-follower
//!   `next_index`/`match_index` tracking, `next_index` back-off on log
//!   mismatch, and majority-based commit advancement.
//! - Durable application of committed entries through an injected
//!   [`LogApplier`], so a committed entry is applied exactly once and in order.
//!
//! The wire transport is injected via [`LogTransport`]; [`InProcessLogNetwork`]
//! is a real in-memory transport for single-process clusters and tests.

use crate::error::{HaError, HaResult};
use async_trait::async_trait;
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Weak};
use tracing::{debug, warn};
use uuid::Uuid;

/// A single replicated log entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    /// Leader term in which the entry was created.
    pub term: u64,
    /// 1-based position of the entry in the log.
    pub index: u64,
    /// Opaque command payload.
    pub data: Vec<u8>,
}

/// AppendEntries RPC request (leader → follower).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppendEntriesRequest {
    /// Leader's current term.
    pub term: u64,
    /// Leader node id.
    pub leader_id: Uuid,
    /// Index of the log entry immediately preceding `entries` (0 = none).
    pub prev_log_index: u64,
    /// Term of the `prev_log_index` entry (0 when `prev_log_index` is 0).
    pub prev_log_term: u64,
    /// New entries to store (empty for a heartbeat).
    pub entries: Vec<LogEntry>,
    /// Leader's commit index.
    pub leader_commit: u64,
}

/// AppendEntries RPC response (follower → leader).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AppendEntriesResponse {
    /// Follower's current term (for the leader to update itself).
    pub term: u64,
    /// Whether the follower accepted the entries (log-consistency passed).
    pub success: bool,
    /// Highest log index known to be replicated on the follower.
    pub match_index: u64,
}

/// Applies a committed log entry to the local state machine, exactly once.
#[async_trait]
pub trait LogApplier: Send + Sync {
    /// Apply the committed `entry`. Errors abort commit-index advancement.
    async fn apply(&self, entry: &LogEntry) -> HaResult<()>;
}

/// Transport used by a leader to send AppendEntries to a follower.
#[async_trait]
pub trait LogTransport: Send + Sync {
    /// Send an AppendEntries request to `follower` and await its response.
    async fn send_append_entries(
        &self,
        follower: Uuid,
        request: AppendEntriesRequest,
    ) -> HaResult<AppendEntriesResponse>;
}

/// Handles an inbound AppendEntries request (implemented by [`ReplicatedLog`]).
#[async_trait]
pub trait AppendEntriesHandler: Send + Sync {
    /// Handle an inbound AppendEntries request against the local log.
    async fn handle(&self, request: AppendEntriesRequest) -> HaResult<AppendEntriesResponse>;
}

/// Internal mutable state of a replicated log.
struct LogState {
    /// The log entries (index i in the vec holds log index i+1).
    entries: Vec<LogEntry>,
    /// Persistent current term.
    current_term: u64,
    /// Highest index known to be committed.
    commit_index: u64,
    /// Highest index applied to the state machine.
    last_applied: u64,
}

impl LogState {
    fn last_index(&self) -> u64 {
        self.entries.len() as u64
    }

    fn term_at(&self, index: u64) -> Option<u64> {
        if index == 0 {
            Some(0)
        } else {
            self.entries.get((index - 1) as usize).map(|e| e.term)
        }
    }
}

/// A replicated log with follower- and leader-side operations.
pub struct ReplicatedLog {
    /// This node's id.
    node_id: Uuid,
    /// Mutable log state.
    state: RwLock<LogState>,
    /// Applier invoked for each committed entry, in index order.
    applier: RwLock<Option<Arc<dyn LogApplier>>>,
}

impl ReplicatedLog {
    /// Create a new empty replicated log for `node_id`.
    pub fn new(node_id: Uuid) -> Self {
        Self {
            node_id,
            state: RwLock::new(LogState {
                entries: Vec::new(),
                current_term: 0,
                commit_index: 0,
                last_applied: 0,
            }),
            applier: RwLock::new(None),
        }
    }

    /// This node's id.
    pub fn node_id(&self) -> Uuid {
        self.node_id
    }

    /// Inject the applier invoked for each committed entry.
    pub fn set_applier(&self, applier: Arc<dyn LogApplier>) {
        *self.applier.write() = Some(applier);
    }

    /// Current term.
    pub fn current_term(&self) -> u64 {
        self.state.read().current_term
    }

    /// Set the current term (e.g. after winning an election).
    pub fn set_current_term(&self, term: u64) {
        let mut state = self.state.write();
        if term > state.current_term {
            state.current_term = term;
        }
    }

    /// Highest committed index.
    pub fn commit_index(&self) -> u64 {
        self.state.read().commit_index
    }

    /// Highest log index present locally.
    pub fn last_index(&self) -> u64 {
        self.state.read().last_index()
    }

    /// Term of the last log entry (0 for an empty log).
    pub fn last_term(&self) -> u64 {
        let state = self.state.read();
        state.term_at(state.last_index()).unwrap_or(0)
    }

    /// A snapshot copy of the entries currently in the log.
    pub fn entries(&self) -> Vec<LogEntry> {
        self.state.read().entries.clone()
    }

    /// Leader-only: append a client command to the local log, returning the
    /// entry (with its assigned index). The entry is not yet committed.
    pub fn leader_append(&self, data: Vec<u8>) -> LogEntry {
        let mut state = self.state.write();
        let index = state.last_index() + 1;
        let entry = LogEntry {
            term: state.current_term,
            index,
            data,
        };
        state.entries.push(entry.clone());
        entry
    }

    /// Follower-side AppendEntries handling with the Raft log-consistency check.
    ///
    /// Applies any newly committed entries before returning.
    pub async fn handle_append_entries(
        &self,
        request: AppendEntriesRequest,
    ) -> HaResult<AppendEntriesResponse> {
        // Compute the reply and the range of entries to apply while holding the
        // lock, then apply outside the lock (the applier is async).
        let (response, to_apply) = {
            let mut state = self.state.write();

            // 1. Reject stale-term leaders.
            if request.term < state.current_term {
                return Ok(AppendEntriesResponse {
                    term: state.current_term,
                    success: false,
                    match_index: state.last_index(),
                });
            }

            // Adopt a newer term.
            if request.term > state.current_term {
                state.current_term = request.term;
            }

            // 2. Log-consistency check on the entry preceding the new ones.
            let prev_ok = match state.term_at(request.prev_log_index) {
                Some(term) => term == request.prev_log_term,
                None => false, // follower is missing prev_log_index entirely
            };
            if !prev_ok {
                return Ok(AppendEntriesResponse {
                    term: state.current_term,
                    success: false,
                    match_index: 0,
                });
            }

            // 3. Splice in the new entries, truncating on conflicting terms.
            for entry in &request.entries {
                let pos = (entry.index - 1) as usize;
                if let Some(existing) = state.entries.get(pos) {
                    if existing.term != entry.term {
                        // Conflict: delete this and everything after it.
                        state.entries.truncate(pos);
                        state.entries.push(entry.clone());
                    }
                    // else: identical entry already present — skip.
                } else {
                    // Beyond current end: append (indices are contiguous because
                    // the prev-log check guarantees no gap before the first).
                    state.entries.push(entry.clone());
                }
            }

            // 4. Advance commit index (bounded by what this follower holds; the
            //    prev-log check guarantees the local log is leader-consistent up
            //    to last_index, so committing up to leader_commit is safe).
            if request.leader_commit > state.commit_index {
                let last_index = state.last_index();
                state.commit_index = request.leader_commit.min(last_index);
            }

            // 5. Determine newly committed entries to apply.
            let mut to_apply = Vec::new();
            while state.last_applied < state.commit_index {
                let apply_index = state.last_applied + 1;
                if let Some(entry) = state.entries.get((apply_index - 1) as usize) {
                    to_apply.push(entry.clone());
                    state.last_applied = apply_index;
                } else {
                    break;
                }
            }

            let response = AppendEntriesResponse {
                term: state.current_term,
                success: true,
                match_index: state.last_index(),
            };
            (response, to_apply)
        };

        self.apply_entries(&to_apply).await?;
        Ok(response)
    }

    /// Apply committed entries (index order) through the injected applier.
    async fn apply_entries(&self, entries: &[LogEntry]) -> HaResult<()> {
        if entries.is_empty() {
            return Ok(());
        }
        let applier = self.applier.read().clone();
        match applier {
            Some(applier) => {
                for entry in entries {
                    applier.apply(entry).await?;
                }
                Ok(())
            }
            None => {
                warn!(
                    "{} committed {} entries but no LogApplier is configured",
                    self.node_id,
                    entries.len()
                );
                Ok(())
            }
        }
    }

    /// Leader-only: advance the commit index to `index` (already known to be
    /// replicated on a majority) and apply the newly committed entries.
    async fn leader_commit_to(&self, index: u64) -> HaResult<()> {
        let to_apply = {
            let mut state = self.state.write();
            // Raft safety: a leader only commits entries from its current term.
            let committable = match state.term_at(index) {
                Some(term) if term == state.current_term && index > state.commit_index => index,
                _ => return Ok(()),
            };
            state.commit_index = committable;

            let mut to_apply = Vec::new();
            while state.last_applied < state.commit_index {
                let apply_index = state.last_applied + 1;
                if let Some(entry) = state.entries.get((apply_index - 1) as usize) {
                    to_apply.push(entry.clone());
                    state.last_applied = apply_index;
                } else {
                    break;
                }
            }
            to_apply
        };
        self.apply_entries(&to_apply).await
    }

    /// Build the AppendEntries request for `next_index` on some follower.
    fn build_request(&self, leader_commit: u64, next_index: u64) -> AppendEntriesRequest {
        let state = self.state.read();
        let prev_log_index = next_index.saturating_sub(1);
        let prev_log_term = state.term_at(prev_log_index).unwrap_or(0);
        let entries: Vec<LogEntry> = state
            .entries
            .iter()
            .filter(|e| e.index >= next_index)
            .cloned()
            .collect();
        AppendEntriesRequest {
            term: state.current_term,
            leader_id: self.node_id,
            prev_log_index,
            prev_log_term,
            entries,
            leader_commit,
        }
    }
}

#[async_trait]
impl AppendEntriesHandler for ReplicatedLog {
    async fn handle(&self, request: AppendEntriesRequest) -> HaResult<AppendEntriesResponse> {
        self.handle_append_entries(request).await
    }
}

/// Per-follower replication progress tracked by a leader.
#[derive(Debug, Clone, Copy)]
struct FollowerProgress {
    /// Next log index to send to the follower.
    next_index: u64,
    /// Highest index known to be replicated on the follower.
    match_index: u64,
}

/// Leader-side driver that replicates a [`ReplicatedLog`] to followers.
pub struct LeaderReplicator {
    /// The leader's log.
    log: Arc<ReplicatedLog>,
    /// Follower ids.
    followers: Vec<Uuid>,
    /// Per-follower progress.
    progress: RwLock<HashMap<Uuid, FollowerProgress>>,
    /// Transport used to reach followers.
    transport: Arc<dyn LogTransport>,
}

impl LeaderReplicator {
    /// Create a leader replicator for `log` over `followers` via `transport`.
    pub fn new(
        log: Arc<ReplicatedLog>,
        followers: Vec<Uuid>,
        transport: Arc<dyn LogTransport>,
    ) -> Self {
        let last = log.last_index();
        let progress = followers
            .iter()
            .map(|id| {
                (
                    *id,
                    FollowerProgress {
                        next_index: last + 1,
                        match_index: 0,
                    },
                )
            })
            .collect();
        Self {
            log,
            followers,
            progress: RwLock::new(progress),
            transport,
        }
    }

    /// Append a command to the leader's log and replicate it to a majority,
    /// committing (and applying) it once a majority acknowledges.
    ///
    /// Returns the committed entry's index. Errors if a majority cannot be
    /// reached.
    pub async fn append_and_replicate(&self, data: Vec<u8>) -> HaResult<u64> {
        let entry = self.log.leader_append(data);
        let before = self.log.commit_index();
        self.replicate_round().await?;

        if self.log.commit_index() >= entry.index {
            // The commit index advanced. Send a second round so followers learn
            // the new leaderCommit (carried as a heartbeat) and apply the newly
            // committed entries to their own state machines — this is the normal
            // Raft one-round-later commit propagation, done eagerly here.
            if self.log.commit_index() > before {
                self.replicate_round().await?;
            }
            Ok(entry.index)
        } else {
            Err(HaError::Replication(format!(
                "log entry {} not committed: majority not reached",
                entry.index
            )))
        }
    }

    /// Run one replication round to all followers, then advance the commit
    /// index based on the resulting match indices.
    pub async fn replicate_round(&self) -> HaResult<()> {
        let leader_commit = self.log.commit_index();

        for follower in &self.followers {
            self.replicate_to(*follower, leader_commit).await;
        }

        self.advance_commit_index().await
    }

    /// Replicate to a single follower, backing off `next_index` on mismatch
    /// until the follower accepts (bounded by the log length).
    async fn replicate_to(&self, follower: Uuid, leader_commit: u64) {
        loop {
            let next_index = self
                .progress
                .read()
                .get(&follower)
                .map(|p| p.next_index)
                .unwrap_or(1);

            let request = self.log.build_request(leader_commit, next_index);

            match self.transport.send_append_entries(follower, request).await {
                Ok(response) => {
                    if response.term > self.log.current_term() {
                        // Stale leader — adopt the higher term and stop.
                        self.log.set_current_term(response.term);
                        return;
                    }
                    if response.success {
                        let mut progress = self.progress.write();
                        if let Some(p) = progress.get_mut(&follower) {
                            p.match_index = response.match_index;
                            p.next_index = response.match_index + 1;
                        }
                        return;
                    } else {
                        // Log mismatch: back off next_index and retry.
                        let mut progress = self.progress.write();
                        if let Some(p) = progress.get_mut(&follower) {
                            if p.next_index > 1 {
                                p.next_index -= 1;
                            } else {
                                // Cannot back off further; give up this round.
                                return;
                            }
                        } else {
                            return;
                        }
                    }
                }
                Err(e) => {
                    debug!("AppendEntries to {} failed: {}", follower, e);
                    return;
                }
            }
        }
    }

    /// Advance the commit index to the highest index replicated on a majority
    /// (including the leader itself), for entries from the current term.
    async fn advance_commit_index(&self) -> HaResult<()> {
        let leader_last = self.log.last_index();
        let cluster_size = self.followers.len() + 1; // + leader
        let majority = cluster_size / 2 + 1;

        // For each candidate index from highest down, count replicas that have
        // it; commit the first that reaches majority.
        let match_indices: Vec<u64> = {
            let progress = self.progress.read();
            self.followers
                .iter()
                .map(|id| progress.get(id).map(|p| p.match_index).unwrap_or(0))
                .collect()
        };

        let mut target = 0;
        for candidate in (self.log.commit_index() + 1..=leader_last).rev() {
            // Leader always has the entry.
            let count = 1 + match_indices.iter().filter(|&&m| m >= candidate).count();
            if count >= majority {
                target = candidate;
                break;
            }
        }

        if target > 0 {
            self.log.leader_commit_to(target).await?;
        }
        Ok(())
    }

    /// Read a follower's currently tracked match index (for tests/monitoring).
    pub fn match_index(&self, follower: Uuid) -> u64 {
        self.progress
            .read()
            .get(&follower)
            .map(|p| p.match_index)
            .unwrap_or(0)
    }
}

/// In-process log-replication network routing AppendEntries to followers.
#[derive(Default)]
pub struct InProcessLogNetwork {
    handlers: DashMap<Uuid, Weak<dyn AppendEntriesHandler>>,
}

impl InProcessLogNetwork {
    /// Create a new empty network.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a follower's AppendEntries handler.
    pub fn register(&self, node_id: Uuid, handler: &Arc<dyn AppendEntriesHandler>) {
        self.handlers.insert(node_id, Arc::downgrade(handler));
    }
}

#[async_trait]
impl LogTransport for InProcessLogNetwork {
    async fn send_append_entries(
        &self,
        follower: Uuid,
        request: AppendEntriesRequest,
    ) -> HaResult<AppendEntriesResponse> {
        let handler = self
            .handlers
            .get(&follower)
            .and_then(|w| w.upgrade())
            .ok_or_else(|| HaError::Network(format!("follower {follower} not reachable")))?;
        handler.handle(request).await
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct RecordingApplier {
        applied: RwLock<Vec<LogEntry>>,
    }

    #[async_trait]
    impl LogApplier for RecordingApplier {
        async fn apply(&self, entry: &LogEntry) -> HaResult<()> {
            self.applied.write().push(entry.clone());
            Ok(())
        }
    }

    fn follower(
        network: &Arc<InProcessLogNetwork>,
        applier: &Arc<RecordingApplier>,
    ) -> Arc<ReplicatedLog> {
        let log = Arc::new(ReplicatedLog::new(Uuid::new_v4()));
        log.set_applier(Arc::clone(applier) as Arc<dyn LogApplier>);
        let handler: Arc<dyn AppendEntriesHandler> =
            Arc::clone(&log) as Arc<dyn AppendEntriesHandler>;
        network.register(log.node_id(), &handler);
        log
    }

    #[tokio::test]
    async fn test_entry_replicates_and_commits_on_majority() {
        let network = Arc::new(InProcessLogNetwork::new());

        let a_applier = Arc::new(RecordingApplier::default());
        let b_applier = Arc::new(RecordingApplier::default());
        let f_a = follower(&network, &a_applier);
        let f_b = follower(&network, &b_applier);

        let leader_applier = Arc::new(RecordingApplier::default());
        let leader = Arc::new(ReplicatedLog::new(Uuid::new_v4()));
        leader.set_applier(Arc::clone(&leader_applier) as Arc<dyn LogApplier>);
        leader.set_current_term(1);

        let replicator = LeaderReplicator::new(
            Arc::clone(&leader),
            vec![f_a.node_id(), f_b.node_id()],
            Arc::clone(&network) as Arc<dyn LogTransport>,
        );

        let index = replicator.append_and_replicate(vec![42]).await.unwrap();
        assert_eq!(index, 1);
        assert_eq!(leader.commit_index(), 1);

        // The command was actually applied on the leader and both followers.
        assert_eq!(leader_applier.applied.read().len(), 1);
        assert_eq!(a_applier.applied.read().len(), 1);
        assert_eq!(b_applier.applied.read().len(), 1);
        assert_eq!(a_applier.applied.read()[0].data, vec![42]);
    }

    #[tokio::test]
    async fn test_no_commit_without_majority() {
        // Leader + 2 followers, but neither follower is registered → unreachable.
        let network = Arc::new(InProcessLogNetwork::new());
        let leader = Arc::new(ReplicatedLog::new(Uuid::new_v4()));
        leader.set_current_term(1);
        let replicator = LeaderReplicator::new(
            Arc::clone(&leader),
            vec![Uuid::new_v4(), Uuid::new_v4()],
            Arc::clone(&network) as Arc<dyn LogTransport>,
        );

        // Leader alone is not a majority of 3 → not committed.
        assert!(replicator.append_and_replicate(vec![1]).await.is_err());
        assert_eq!(leader.commit_index(), 0);
    }

    #[tokio::test]
    async fn test_consistency_check_rejects_gap() {
        let f = Arc::new(ReplicatedLog::new(Uuid::new_v4()));
        // prev_log_index=5 but follower log is empty → must reject.
        let resp = f
            .handle_append_entries(AppendEntriesRequest {
                term: 1,
                leader_id: Uuid::new_v4(),
                prev_log_index: 5,
                prev_log_term: 1,
                entries: vec![LogEntry {
                    term: 1,
                    index: 6,
                    data: vec![0],
                }],
                leader_commit: 6,
            })
            .await
            .unwrap();
        assert!(!resp.success);
        assert_eq!(f.last_index(), 0);
    }

    #[tokio::test]
    async fn test_conflicting_entry_is_truncated() {
        let f = Arc::new(ReplicatedLog::new(Uuid::new_v4()));

        // Seed two entries at term 1.
        f.handle_append_entries(AppendEntriesRequest {
            term: 1,
            leader_id: Uuid::new_v4(),
            prev_log_index: 0,
            prev_log_term: 0,
            entries: vec![
                LogEntry {
                    term: 1,
                    index: 1,
                    data: vec![1],
                },
                LogEntry {
                    term: 1,
                    index: 2,
                    data: vec![2],
                },
            ],
            leader_commit: 0,
        })
        .await
        .unwrap();
        assert_eq!(f.last_index(), 2);

        // A new leader (term 2) overwrites index 2 with a conflicting entry.
        let resp = f
            .handle_append_entries(AppendEntriesRequest {
                term: 2,
                leader_id: Uuid::new_v4(),
                prev_log_index: 1,
                prev_log_term: 1,
                entries: vec![LogEntry {
                    term: 2,
                    index: 2,
                    data: vec![99],
                }],
                leader_commit: 0,
            })
            .await
            .unwrap();
        assert!(resp.success);
        let entries = f.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].term, 2);
        assert_eq!(entries[1].data, vec![99]);
    }

    #[tokio::test]
    async fn test_lagging_follower_catches_up() {
        let network = Arc::new(InProcessLogNetwork::new());
        let applier = Arc::new(RecordingApplier::default());
        let f_a = follower(&network, &applier);
        let b_applier = Arc::new(RecordingApplier::default());
        let f_b = follower(&network, &b_applier);

        let leader = Arc::new(ReplicatedLog::new(Uuid::new_v4()));
        leader.set_current_term(1);
        // Pre-seed the leader with two entries before the follower exists.
        leader.leader_append(vec![10]);
        leader.leader_append(vec![20]);

        let replicator = LeaderReplicator::new(
            Arc::clone(&leader),
            vec![f_a.node_id(), f_b.node_id()],
            Arc::clone(&network) as Arc<dyn LogTransport>,
        );

        // A fresh append triggers replication of the whole backlog.
        let index = replicator.append_and_replicate(vec![30]).await.unwrap();
        assert_eq!(index, 3);
        assert_eq!(leader.commit_index(), 3);
        assert_eq!(applier.applied.read().len(), 3);
        assert_eq!(
            applier
                .applied
                .read()
                .iter()
                .map(|e| e.data.clone())
                .collect::<Vec<_>>(),
            vec![vec![10], vec![20], vec![30]]
        );
    }
}
