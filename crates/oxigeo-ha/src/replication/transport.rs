//! Replication transport abstractions.
//!
//! Replication needs to actually move event bytes to remote replicas and apply
//! incoming events to the local store. To keep the crate transport-agnostic and
//! Pure-Rust (COOLJAPAN policy — no C/C++ RPC toolchains), the wire protocol is
//! injected through the [`ReplicaTransport`] trait and local application is
//! injected through the [`EventApplier`] trait.
//!
//! [`InProcessReplicaNetwork`] is a real, working transport for same-process
//! clusters and tests: it delivers events to a peer node's
//! [`EventReceiver`] and returns a genuine acknowledgment. A command to an
//! unregistered replica errors just like an unreachable remote would.

use super::ReplicationEvent;
use crate::error::{HaError, HaResult};
use async_trait::async_trait;
use dashmap::DashMap;
use std::sync::{Arc, Weak};
use uuid::Uuid;

/// Acknowledgment returned by a replica after it durably receives events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransportAck {
    /// The replica that acknowledged.
    pub replica_id: Uuid,
    /// Number of events the replica accepted.
    pub events_acked: usize,
    /// Number of payload bytes the replica accepted.
    pub bytes_acked: usize,
}

/// Transport used to send replication events to a specific replica.
#[async_trait]
pub trait ReplicaTransport: Send + Sync {
    /// Send `events` to `replica_id` and await a real acknowledgment.
    ///
    /// Implementations MUST return an error if the events did not reach the
    /// replica; the caller only updates lag/success statistics on `Ok`.
    async fn send_events(
        &self,
        replica_id: Uuid,
        events: Vec<ReplicationEvent>,
    ) -> HaResult<TransportAck>;
}

/// Applies an inbound replication event to the local store / WAL.
#[async_trait]
pub trait EventApplier: Send + Sync {
    /// Durably apply `event` to local state.
    async fn apply(&self, event: &ReplicationEvent) -> HaResult<()>;
}

/// Receives inbound replication events (implemented by the replication engine).
#[async_trait]
pub trait EventReceiver: Send + Sync {
    /// Receive and apply an event originating from another node.
    async fn receive_event(&self, event: ReplicationEvent) -> HaResult<()>;
}

/// In-process replica network.
///
/// Routes events to the registered [`EventReceiver`] of the target node. Real
/// delivery (not a simulated sleep): if the target is not registered, delivery
/// fails with a [`HaError::Network`] error.
#[derive(Default)]
pub struct InProcessReplicaNetwork {
    /// Receivers keyed by node id (weak to avoid Arc cycles with the engines).
    receivers: DashMap<Uuid, Weak<dyn EventReceiver>>,
}

impl InProcessReplicaNetwork {
    /// Create a new empty in-process network.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a node's event receiver so peers can deliver events to it.
    pub fn register(&self, node_id: Uuid, receiver: &Arc<dyn EventReceiver>) {
        self.receivers.insert(node_id, Arc::downgrade(receiver));
    }
}

#[async_trait]
impl ReplicaTransport for InProcessReplicaNetwork {
    async fn send_events(
        &self,
        replica_id: Uuid,
        events: Vec<ReplicationEvent>,
    ) -> HaResult<TransportAck> {
        let receiver = self
            .receivers
            .get(&replica_id)
            .and_then(|w| w.upgrade())
            .ok_or_else(|| HaError::Network(format!("replica {replica_id} not reachable")))?;

        let bytes_acked: usize = events.iter().map(|e| e.data.len()).sum();
        let events_acked = events.len();

        for event in events {
            // Any application failure on the remote side surfaces as a
            // replication error so the sender does not record a false success.
            receiver.receive_event(event).await?;
        }

        Ok(TransportAck {
            replica_id,
            events_acked,
            bytes_acked,
        })
    }
}
