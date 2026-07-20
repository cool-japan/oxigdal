//! Node-to-node transport for cluster consensus RPCs.
//!
//! Leader election in [`crate::coordinator`] must contact real peers rather than
//! fabricate votes from a node's local view of the cluster. This module defines
//! the minimal RPC surface for that — a [`VoteRequest`]/[`VoteResponse`] pair and
//! the [`NodeTransport`] trait a deployment plugs a real network implementation
//! (for example a `tonic` gRPC client) into.
//!
//! A default [`UnconfiguredTransport`] is provided so a coordinator built without
//! an explicit transport is *safe by construction*: it can never reach a peer, so
//! it can never gather a fabricated quorum. A single-node cluster still elects
//! itself through its own self-vote, but a multi-node cluster with no wired
//! transport correctly fails to elect a leader instead of silently splitting.

use crate::coordinator::NodeId;
use crate::error::{ClusterError, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// A request asking a peer to grant its vote for a candidate in a given term.
///
/// Mirrors the Raft `RequestVote` RPC. The log fields are carried so a future
/// log-replication layer can enforce the up-to-date-log check; the current
/// coordinator has no replicated log and sends zeroes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoteRequest {
    /// Candidate's election term.
    pub term: u64,
    /// Candidate requesting the vote.
    pub candidate_id: NodeId,
    /// Index of the candidate's last log entry.
    pub last_log_index: u64,
    /// Term of the candidate's last log entry.
    pub last_log_term: u64,
}

/// A peer's response to a [`VoteRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoteResponse {
    /// The responder's current term, so a candidate can detect a higher term and
    /// step down.
    pub term: u64,
    /// Whether the responder granted its vote.
    pub vote_granted: bool,
}

/// Transport used by the coordinator to send consensus RPCs to peers.
///
/// Implementations are responsible for actually reaching `peer` over the network
/// (using `peer_address`) and returning the peer's real response. Returning an
/// error models an unreachable peer — the caller treats that as "no vote", never
/// as a granted vote.
#[async_trait]
pub trait NodeTransport: Send + Sync {
    /// Send a vote request to a single peer and await its response.
    async fn request_vote(
        &self,
        peer: NodeId,
        peer_address: &str,
        request: VoteRequest,
    ) -> Result<VoteResponse>;
}

/// The default transport installed when no real transport is configured.
///
/// It never reaches a peer, so it never yields a vote. This is deliberately the
/// safe default: without a real network layer a node must not be able to assemble
/// a quorum out of thin air.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnconfiguredTransport;

#[async_trait]
impl NodeTransport for UnconfiguredTransport {
    async fn request_vote(
        &self,
        peer: NodeId,
        _peer_address: &str,
        _request: VoteRequest,
    ) -> Result<VoteResponse> {
        Err(ClusterError::NetworkError(format!(
            "no node transport configured; cannot reach peer {peer}"
        )))
    }
}
