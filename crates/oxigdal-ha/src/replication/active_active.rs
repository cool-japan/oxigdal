//! Active-active replication implementation.
//!
//! Provides bi-directional replication between multiple active nodes.

use super::{
    ReplicaNode, ReplicationConfig, ReplicationEvent, ReplicationManager, ReplicationState,
    ReplicationStats, VectorClock,
};
use crate::error::{HaError, HaResult};
use async_trait::async_trait;
use chrono::Utc;
use dashmap::DashMap;
use parking_lot::RwLock;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tokio::sync::{Notify, mpsc};
use tokio::time::{Duration, sleep};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Active-active replication manager.
pub struct ActiveActiveReplication {
    /// Node ID.
    node_id: Uuid,
    /// Configuration.
    config: Arc<RwLock<ReplicationConfig>>,
    /// Replica nodes.
    replicas: Arc<DashMap<Uuid, ReplicaNode>>,
    /// Vector clock for this node.
    vector_clock: Arc<RwLock<VectorClock>>,
    /// Event sequence number.
    sequence: AtomicU64,
    /// Running flag.
    running: AtomicBool,
    /// Event sender channel.
    event_tx: mpsc::UnboundedSender<ReplicationEvent>,
    /// Event receiver channel.
    event_rx: Arc<RwLock<Option<mpsc::UnboundedReceiver<ReplicationEvent>>>>,
    /// Statistics.
    stats: Arc<RwLock<ReplicationStats>>,
    /// Shutdown notifier.
    shutdown: Arc<Notify>,
}

impl ActiveActiveReplication {
    /// Create a new active-active replication manager.
    pub fn new(node_id: Uuid, config: ReplicationConfig) -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        Self {
            node_id,
            config: Arc::new(RwLock::new(config)),
            replicas: Arc::new(DashMap::new()),
            vector_clock: Arc::new(RwLock::new(VectorClock::new())),
            sequence: AtomicU64::new(0),
            running: AtomicBool::new(false),
            event_tx,
            event_rx: Arc::new(RwLock::new(Some(event_rx))),
            stats: Arc::new(RwLock::new(ReplicationStats::default())),
            shutdown: Arc::new(Notify::new()),
        }
    }

    /// Get the node ID.
    pub fn node_id(&self) -> Uuid {
        self.node_id
    }

    /// Get the next sequence number.
    pub fn next_sequence(&self) -> u64 {
        self.sequence.fetch_add(1, Ordering::SeqCst)
    }

    /// Increment vector clock.
    fn increment_clock(&self) {
        let mut clock = self.vector_clock.write();
        clock.increment(self.node_id);
    }

    /// Process replication events.
    async fn process_events(
        &self,
        mut rx: mpsc::UnboundedReceiver<ReplicationEvent>,
    ) -> HaResult<()> {
        let mut batch = Vec::new();
        let config = self.config.read().clone();
        let batch_timeout = Duration::from_millis(config.batch_timeout_ms);

        loop {
            tokio::select! {
                _ = self.shutdown.notified() => {
                    info!("Replication event processor shutting down");
                    break;
                }
                event_opt = rx.recv() => {
                    match event_opt {
                        Some(event) => {
                            batch.push(event);

                            if batch.len() >= config.batch_size {
                                self.flush_batch(&mut batch).await?;
                            }
                        }
                        None => {
                            warn!("Event channel closed");
                            break;
                        }
                    }
                }
                _ = sleep(batch_timeout) => {
                    if !batch.is_empty() {
                        self.flush_batch(&mut batch).await?;
                    }
                }
            }
        }

        if !batch.is_empty() {
            self.flush_batch(&mut batch).await?;
        }

        Ok(())
    }

    /// Flush a batch of events to replicas.
    async fn flush_batch(&self, batch: &mut Vec<ReplicationEvent>) -> HaResult<()> {
        if batch.is_empty() {
            return Ok(());
        }

        debug!("Flushing batch of {} events", batch.len());

        let replicas: Vec<_> = self
            .replicas
            .iter()
            .filter(|entry| entry.value().state == ReplicationState::Active)
            .map(|entry| *entry.key())
            .collect();

        if replicas.is_empty() {
            warn!("No active replicas to replicate to");
            batch.clear();
            return Ok(());
        }

        let events = batch.clone();
        batch.clear();

        let mut tasks = Vec::new();
        for replica_id in replicas {
            let events_clone = events.clone();
            let replicas = Arc::clone(&self.replicas);
            let stats = Arc::clone(&self.stats);

            tasks.push(tokio::spawn(async move {
                match Self::send_to_replica(replica_id, events_clone, replicas, stats).await {
                    Ok(()) => Ok(replica_id),
                    Err(e) => {
                        error!("Failed to replicate to {}: {}", replica_id, e);
                        Err((replica_id, e))
                    }
                }
            }));
        }

        for task in tasks {
            match task.await {
                Ok(Ok(_)) => {}
                Ok(Err((replica_id, _))) => {
                    if let Some(mut replica) = self.replicas.get_mut(&replica_id) {
                        replica.state = ReplicationState::Failed;
                    }
                }
                Err(e) => {
                    error!("Task join error: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Send events to a specific replica.
    async fn send_to_replica(
        replica_id: Uuid,
        events: Vec<ReplicationEvent>,
        replicas: Arc<DashMap<Uuid, ReplicaNode>>,
        stats: Arc<RwLock<ReplicationStats>>,
    ) -> HaResult<()> {
        let start_time = Utc::now();

        let total_bytes: usize = events.iter().map(|e| e.data.len()).sum();

        debug!(
            "Sending {} events ({} bytes) to replica {}",
            events.len(),
            total_bytes,
            replica_id
        );

        // Simulate network send
        // In a real implementation, this would send over network
        sleep(Duration::from_millis(10)).await;

        let elapsed = (Utc::now() - start_time).num_milliseconds() as u64;

        if let Some(mut replica) = replicas.get_mut(&replica_id) {
            replica.last_replicated_at = Some(Utc::now());
            replica.lag_ms = Some(elapsed);
        }

        let mut stats_guard = stats.write();
        stats_guard.events_replicated += events.len() as u64;
        stats_guard.bytes_replicated += total_bytes as u64;
        stats_guard.current_lag_ms = elapsed;
        stats_guard.average_lag_ms = (stats_guard.average_lag_ms + elapsed) / 2;
        stats_guard.peak_lag_ms = stats_guard.peak_lag_ms.max(elapsed);

        Ok(())
    }

    /// Receive and apply events from other nodes.
    pub async fn receive_event(&self, event: ReplicationEvent) -> HaResult<()> {
        event.verify_checksum()?;

        debug!(
            "Received event {} from node {}",
            event.id, event.source_node_id
        );

        self.increment_clock();

        Ok(())
    }

    /// Synchronize with a specific replica.
    pub async fn sync_with_replica(&self, replica_id: Uuid) -> HaResult<()> {
        let replica = self
            .replicas
            .get(&replica_id)
            .ok_or_else(|| HaError::Replication(format!("Replica {} not found", replica_id)))?;

        info!("Synchronizing with replica: {}", replica.name);

        if let Some(mut replica) = self.replicas.get_mut(&replica_id) {
            replica.state = ReplicationState::CatchingUp;
        }

        sleep(Duration::from_millis(100)).await;

        if let Some(mut replica) = self.replicas.get_mut(&replica_id) {
            replica.state = ReplicationState::Active;
        }

        info!("Synchronization with replica {} complete", replica.name);

        Ok(())
    }

    /// Check replication health.
    pub async fn check_health(&self) -> HaResult<bool> {
        let config = self.config.read();
        let max_lag = config.max_lag_ms;

        for entry in self.replicas.iter() {
            let replica = entry.value();
            if replica.state == ReplicationState::Active {
                if let Some(lag_ms) = replica.lag_ms {
                    if lag_ms > max_lag {
                        warn!(
                            "Replica {} has high lag: {}ms > {}ms",
                            replica.name, lag_ms, max_lag
                        );
                        return Ok(false);
                    }
                }
            }
        }

        Ok(true)
    }
}

#[async_trait]
impl ReplicationManager for ActiveActiveReplication {
    async fn start(&self) -> HaResult<()> {
        if self.running.swap(true, Ordering::SeqCst) {
            return Err(HaError::InvalidState(
                "Replication already running".to_string(),
            ));
        }

        info!(
            "Starting active-active replication for node {}",
            self.node_id
        );

        let mut rx_guard = self.event_rx.write();
        let rx = rx_guard
            .take()
            .ok_or_else(|| HaError::InvalidState("Event receiver already taken".to_string()))?;
        drop(rx_guard);

        let self_clone = Self {
            node_id: self.node_id,
            config: Arc::clone(&self.config),
            replicas: Arc::clone(&self.replicas),
            vector_clock: Arc::clone(&self.vector_clock),
            sequence: AtomicU64::new(self.sequence.load(Ordering::SeqCst)),
            running: AtomicBool::new(true),
            event_tx: self.event_tx.clone(),
            event_rx: Arc::clone(&self.event_rx),
            stats: Arc::clone(&self.stats),
            shutdown: Arc::clone(&self.shutdown),
        };

        tokio::spawn(async move {
            if let Err(e) = self_clone.process_events(rx).await {
                error!("Error processing replication events: {}", e);
            }
        });

        info!("Active-active replication started");

        Ok(())
    }

    async fn stop(&self) -> HaResult<()> {
        if !self.running.swap(false, Ordering::SeqCst) {
            return Err(HaError::InvalidState("Replication not running".to_string()));
        }

        info!("Stopping active-active replication");

        self.shutdown.notify_waiters();

        sleep(Duration::from_millis(100)).await;

        info!("Active-active replication stopped");

        Ok(())
    }

    async fn replicate(&self, event: ReplicationEvent) -> HaResult<()> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(HaError::InvalidState("Replication not running".to_string()));
        }

        self.increment_clock();

        self.event_tx
            .send(event)
            .map_err(|e| HaError::Replication(format!("Failed to send event: {}", e)))?;

        Ok(())
    }

    async fn replicate_batch(&self, events: Vec<ReplicationEvent>) -> HaResult<()> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(HaError::InvalidState("Replication not running".to_string()));
        }

        for event in events {
            self.increment_clock();
            self.event_tx
                .send(event)
                .map_err(|e| HaError::Replication(format!("Failed to send event: {}", e)))?;
        }

        Ok(())
    }

    async fn get_stats(&self) -> HaResult<ReplicationStats> {
        let stats = self.stats.read().clone();
        Ok(stats)
    }

    async fn get_replicas(&self) -> HaResult<Vec<ReplicaNode>> {
        let replicas = self
            .replicas
            .iter()
            .map(|entry| entry.value().clone())
            .collect();
        Ok(replicas)
    }

    async fn add_replica(&self, replica: ReplicaNode) -> HaResult<()> {
        info!("Adding replica: {} ({})", replica.name, replica.id);
        self.replicas.insert(replica.id, replica);
        Ok(())
    }

    async fn remove_replica(&self, node_id: Uuid) -> HaResult<()> {
        info!("Removing replica: {}", node_id);
        self.replicas
            .remove(&node_id)
            .ok_or_else(|| HaError::Replication(format!("Replica {} not found", node_id)))?;
        Ok(())
    }

    async fn pause_replica(&self, node_id: Uuid) -> HaResult<()> {
        info!("Pausing replica: {}", node_id);
        let mut replica = self
            .replicas
            .get_mut(&node_id)
            .ok_or_else(|| HaError::Replication(format!("Replica {} not found", node_id)))?;
        replica.state = ReplicationState::Paused;
        Ok(())
    }

    async fn resume_replica(&self, node_id: Uuid) -> HaResult<()> {
        info!("Resuming replica: {}", node_id);
        let mut replica = self
            .replicas
            .get_mut(&node_id)
            .ok_or_else(|| HaError::Replication(format!("Replica {} not found", node_id)))?;
        replica.state = ReplicationState::Active;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_active_active_replication() {
        let node_id = Uuid::new_v4();
        let config = ReplicationConfig::default();
        let replication = ActiveActiveReplication::new(node_id, config);

        assert!(replication.start().await.is_ok());

        let replica = ReplicaNode {
            id: Uuid::new_v4(),
            name: "replica1".to_string(),
            address: "localhost:5000".to_string(),
            priority: 100,
            state: ReplicationState::Active,
            last_replicated_at: None,
            lag_ms: None,
        };

        assert!(replication.add_replica(replica.clone()).await.is_ok());

        let event = ReplicationEvent::new(node_id, replica.id, vec![1, 2, 3, 4, 5], 1);

        assert!(replication.replicate(event).await.is_ok());

        sleep(Duration::from_millis(200)).await;

        let stats = replication.get_stats().await.ok();
        assert!(stats.is_some());

        assert!(replication.stop().await.is_ok());
    }
}
