//! Cache coherency protocols for distributed caching
//!
//! Implements various coherency protocols:
//! - MSI protocol (Modified, Shared, Invalid)
//! - MESI protocol (Modified, Exclusive, Shared, Invalid)
//! - Directory-based coherency for large clusters
//! - Invalidation batching for performance

use crate::error::Result;
use crate::multi_tier::CacheKey;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Cache line state in MSI protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MSIState {
    /// Modified - cache has exclusive ownership and has been modified
    Modified,
    /// Shared - cache has a valid copy, may be shared with others
    Shared,
    /// Invalid - cache line is not valid
    Invalid,
}

/// Cache line state in MESI protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MESIState {
    /// Modified - exclusive ownership, modified
    Modified,
    /// Exclusive - exclusive ownership, not modified
    Exclusive,
    /// Shared - valid copy, may be shared
    Shared,
    /// Invalid - not valid
    Invalid,
}

/// Coherency message types
#[derive(Debug, Clone)]
pub enum CoherencyMessage {
    /// Read request
    Read(CacheKey),
    /// Write request
    Write(CacheKey),
    /// Invalidate request
    Invalidate(CacheKey),
    /// Invalidate acknowledgment
    InvalidateAck(CacheKey),
    /// Write-back notification
    WriteBack(CacheKey),
    /// Shared response
    Shared(CacheKey),
}

/// MSI coherency protocol implementation
pub struct MSIProtocol {
    /// Cache line states
    states: Arc<RwLock<HashMap<CacheKey, MSIState>>>,
    /// Node ID
    #[allow(dead_code)]
    node_id: String,
    /// Other nodes in the system
    peer_nodes: Arc<RwLock<HashSet<String>>>,
    /// Pending invalidations
    pending_invalidations: Arc<RwLock<HashMap<CacheKey, HashSet<String>>>>,
}

impl MSIProtocol {
    /// Create new MSI protocol instance
    pub fn new(node_id: String) -> Self {
        Self {
            states: Arc::new(RwLock::new(HashMap::new())),
            node_id,
            peer_nodes: Arc::new(RwLock::new(HashSet::new())),
            pending_invalidations: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add peer node
    pub async fn add_peer(&self, peer_id: String) {
        self.peer_nodes.write().await.insert(peer_id);
    }

    /// Remove peer node
    pub async fn remove_peer(&self, peer_id: &str) {
        self.peer_nodes.write().await.remove(peer_id);
    }

    /// Get current state of a cache line
    pub async fn get_state(&self, key: &CacheKey) -> MSIState {
        self.states
            .read()
            .await
            .get(key)
            .copied()
            .unwrap_or(MSIState::Invalid)
    }

    /// Handle read request
    pub async fn handle_read(&self, key: &CacheKey) -> Result<Vec<CoherencyMessage>> {
        let state = self.get_state(key).await;
        let mut messages = Vec::new();

        match state {
            MSIState::Modified | MSIState::Shared => {
                // Already have valid copy, no action needed
                Ok(messages)
            }
            MSIState::Invalid => {
                // Request from other nodes
                messages.push(CoherencyMessage::Read(key.clone()));

                // Transition to Shared state
                self.states
                    .write()
                    .await
                    .insert(key.clone(), MSIState::Shared);

                Ok(messages)
            }
        }
    }

    /// Handle write request
    pub async fn handle_write(&self, key: &CacheKey) -> Result<Vec<CoherencyMessage>> {
        let state = self.get_state(key).await;
        let mut messages = Vec::new();

        match state {
            MSIState::Modified => {
                // Already have exclusive access
                Ok(messages)
            }
            MSIState::Shared => {
                // Need to invalidate all other copies
                let peers = self.peer_nodes.read().await;
                for _peer in peers.iter() {
                    messages.push(CoherencyMessage::Invalidate(key.clone()));
                }

                // Track pending invalidations
                self.pending_invalidations
                    .write()
                    .await
                    .insert(key.clone(), peers.clone());

                // Transition to Modified state
                self.states
                    .write()
                    .await
                    .insert(key.clone(), MSIState::Modified);

                Ok(messages)
            }
            MSIState::Invalid => {
                // Request exclusive access
                let peers = self.peer_nodes.read().await;
                for _peer in peers.iter() {
                    messages.push(CoherencyMessage::Invalidate(key.clone()));
                }

                self.pending_invalidations
                    .write()
                    .await
                    .insert(key.clone(), peers.clone());

                self.states
                    .write()
                    .await
                    .insert(key.clone(), MSIState::Modified);

                Ok(messages)
            }
        }
    }

    /// Handle invalidation request from remote node
    pub async fn handle_remote_invalidate(&self, key: &CacheKey) -> Result<CoherencyMessage> {
        let state = self.get_state(key).await;

        match state {
            MSIState::Modified => {
                // Need to write back modified data
                self.states
                    .write()
                    .await
                    .insert(key.clone(), MSIState::Invalid);
                Ok(CoherencyMessage::WriteBack(key.clone()))
            }
            MSIState::Shared => {
                // Just invalidate
                self.states
                    .write()
                    .await
                    .insert(key.clone(), MSIState::Invalid);
                Ok(CoherencyMessage::InvalidateAck(key.clone()))
            }
            MSIState::Invalid => {
                // Already invalid
                Ok(CoherencyMessage::InvalidateAck(key.clone()))
            }
        }
    }

    /// Handle invalidation acknowledgment
    pub async fn handle_invalidate_ack(&self, key: &CacheKey, from_node: &str) {
        let mut pending = self.pending_invalidations.write().await;
        if let Some(waiting) = pending.get_mut(key) {
            waiting.remove(from_node);
            if waiting.is_empty() {
                pending.remove(key);
            }
        }
    }

    /// Check if invalidations are complete
    pub async fn invalidations_complete(&self, key: &CacheKey) -> bool {
        let pending = self.pending_invalidations.read().await;
        !pending.contains_key(key)
    }

    /// Evict cache line
    pub async fn evict(&self, key: &CacheKey) -> Result<Option<CoherencyMessage>> {
        let state = self.get_state(key).await;

        match state {
            MSIState::Modified => {
                // Write back modified data
                self.states.write().await.remove(key);
                Ok(Some(CoherencyMessage::WriteBack(key.clone())))
            }
            MSIState::Shared | MSIState::Invalid => {
                // No write-back needed
                self.states.write().await.remove(key);
                Ok(None)
            }
        }
    }
}

/// MESI coherency protocol implementation
pub struct MESIProtocol {
    /// Cache line states
    states: Arc<RwLock<HashMap<CacheKey, MESIState>>>,
    /// Node ID
    #[allow(dead_code)]
    node_id: String,
    /// Peer nodes
    peer_nodes: Arc<RwLock<HashSet<String>>>,
    /// Pending invalidations
    pending_invalidations: Arc<RwLock<HashMap<CacheKey, HashSet<String>>>>,
}

impl MESIProtocol {
    /// Create new MESI protocol instance
    pub fn new(node_id: String) -> Self {
        Self {
            states: Arc::new(RwLock::new(HashMap::new())),
            node_id,
            peer_nodes: Arc::new(RwLock::new(HashSet::new())),
            pending_invalidations: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add peer node
    pub async fn add_peer(&self, peer_id: String) {
        self.peer_nodes.write().await.insert(peer_id);
    }

    /// Get current state
    pub async fn get_state(&self, key: &CacheKey) -> MESIState {
        self.states
            .read()
            .await
            .get(key)
            .copied()
            .unwrap_or(MESIState::Invalid)
    }

    /// Handle read request
    pub async fn handle_read(
        &self,
        key: &CacheKey,
        has_other_copy: bool,
    ) -> Result<Vec<CoherencyMessage>> {
        let state = self.get_state(key).await;
        let mut messages = Vec::new();

        match state {
            MESIState::Modified | MESIState::Exclusive | MESIState::Shared => {
                // Already have valid copy
                Ok(messages)
            }
            MESIState::Invalid => {
                messages.push(CoherencyMessage::Read(key.clone()));

                // Transition based on whether other copies exist
                let new_state = if has_other_copy {
                    MESIState::Shared
                } else {
                    MESIState::Exclusive
                };

                self.states.write().await.insert(key.clone(), new_state);
                Ok(messages)
            }
        }
    }

    /// Handle write request
    pub async fn handle_write(&self, key: &CacheKey) -> Result<Vec<CoherencyMessage>> {
        let state = self.get_state(key).await;
        let mut messages = Vec::new();

        match state {
            MESIState::Modified => {
                // Already have exclusive modified access
                Ok(messages)
            }
            MESIState::Exclusive => {
                // Upgrade to Modified
                self.states
                    .write()
                    .await
                    .insert(key.clone(), MESIState::Modified);
                Ok(messages)
            }
            MESIState::Shared | MESIState::Invalid => {
                // Invalidate all other copies
                let peers = self.peer_nodes.read().await;
                for _peer in peers.iter() {
                    messages.push(CoherencyMessage::Invalidate(key.clone()));
                }

                self.pending_invalidations
                    .write()
                    .await
                    .insert(key.clone(), peers.clone());

                self.states
                    .write()
                    .await
                    .insert(key.clone(), MESIState::Modified);

                Ok(messages)
            }
        }
    }

    /// Handle remote read request
    pub async fn handle_remote_read(&self, key: &CacheKey) -> Result<CoherencyMessage> {
        let state = self.get_state(key).await;

        match state {
            MESIState::Modified => {
                // Downgrade to Shared and provide data
                self.states
                    .write()
                    .await
                    .insert(key.clone(), MESIState::Shared);
                Ok(CoherencyMessage::Shared(key.clone()))
            }
            MESIState::Exclusive => {
                // Downgrade to Shared
                self.states
                    .write()
                    .await
                    .insert(key.clone(), MESIState::Shared);
                Ok(CoherencyMessage::Shared(key.clone()))
            }
            MESIState::Shared => {
                // Already shared
                Ok(CoherencyMessage::Shared(key.clone()))
            }
            MESIState::Invalid => {
                // No valid copy
                Ok(CoherencyMessage::InvalidateAck(key.clone()))
            }
        }
    }

    /// Evict cache line
    pub async fn evict(&self, key: &CacheKey) -> Result<Option<CoherencyMessage>> {
        let state = self.get_state(key).await;

        match state {
            MESIState::Modified => {
                self.states.write().await.remove(key);
                Ok(Some(CoherencyMessage::WriteBack(key.clone())))
            }
            _ => {
                self.states.write().await.remove(key);
                Ok(None)
            }
        }
    }
}

/// Directory-based coherency for large-scale systems
pub struct DirectoryCoherency {
    /// Directory entries (key -> set of nodes with copies)
    directory: Arc<RwLock<HashMap<CacheKey, HashSet<String>>>>,
    /// Modified state tracking (key -> node with modified copy)
    modified_by: Arc<RwLock<HashMap<CacheKey, String>>>,
    /// Local node ID
    node_id: String,
}

impl DirectoryCoherency {
    /// Create new directory coherency
    pub fn new(node_id: String) -> Self {
        Self {
            directory: Arc::new(RwLock::new(HashMap::new())),
            modified_by: Arc::new(RwLock::new(HashMap::new())),
            node_id,
        }
    }

    /// Handle read request
    pub async fn handle_read(&self, key: &CacheKey) -> Result<Vec<CoherencyMessage>> {
        let mut dir = self.directory.write().await;
        let modified = self.modified_by.read().await;

        let mut messages = Vec::new();

        if let Some(_modifier) = modified.get(key) {
            // Request data from modifier
            messages.push(CoherencyMessage::Read(key.clone()));
        }

        // Add this node to sharers
        dir.entry(key.clone())
            .or_insert_with(HashSet::new)
            .insert(self.node_id.clone());

        Ok(messages)
    }

    /// Handle write request
    pub async fn handle_write(&self, key: &CacheKey) -> Result<Vec<CoherencyMessage>> {
        let mut dir = self.directory.write().await;
        let mut modified = self.modified_by.write().await;

        let mut messages = Vec::new();

        // Invalidate all sharers
        if let Some(sharers) = dir.get(key) {
            for sharer in sharers.iter() {
                if sharer != &self.node_id {
                    messages.push(CoherencyMessage::Invalidate(key.clone()));
                }
            }
        }

        // Mark as modified by this node
        modified.insert(key.clone(), self.node_id.clone());

        // Clear sharers
        dir.insert(key.clone(), {
            let mut set = HashSet::new();
            set.insert(self.node_id.clone());
            set
        });

        Ok(messages)
    }

    /// Handle invalidation acknowledgment
    pub async fn handle_invalidate_ack(&self, key: &CacheKey, from_node: &str) {
        let mut dir = self.directory.write().await;
        if let Some(sharers) = dir.get_mut(key) {
            sharers.remove(from_node);
        }
    }

    /// Get nodes with copies
    pub async fn get_sharers(&self, key: &CacheKey) -> HashSet<String> {
        self.directory
            .read()
            .await
            .get(key)
            .cloned()
            .unwrap_or_default()
    }
}

/// Batched invalidation for performance
pub struct InvalidationBatcher {
    /// Pending invalidations
    pending: Arc<RwLock<HashMap<String, HashSet<CacheKey>>>>,
    /// Batch size threshold
    batch_size: usize,
}

impl InvalidationBatcher {
    /// Create new invalidation batcher
    pub fn new(batch_size: usize) -> Self {
        Self {
            pending: Arc::new(RwLock::new(HashMap::new())),
            batch_size,
        }
    }

    /// Add invalidation to batch
    pub async fn add_invalidation(&self, node: String, key: CacheKey) -> Option<Vec<CacheKey>> {
        let mut pending = self.pending.write().await;
        let keys = pending.entry(node.clone()).or_insert_with(HashSet::new);

        keys.insert(key);

        // Flush if batch size reached
        if keys.len() >= self.batch_size {
            let batch: Vec<CacheKey> = keys.iter().cloned().collect();
            keys.clear();
            Some(batch)
        } else {
            None
        }
    }

    /// Flush all pending invalidations
    pub async fn flush(&self) -> HashMap<String, Vec<CacheKey>> {
        let mut pending = self.pending.write().await;
        let result: HashMap<String, Vec<CacheKey>> = pending
            .iter()
            .map(|(node, keys)| (node.clone(), keys.iter().cloned().collect()))
            .collect();

        pending.clear();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_msi_protocol() {
        let protocol = MSIProtocol::new("node1".to_string());
        protocol.add_peer("node2".to_string()).await;

        let key = "test_key".to_string();

        // Read should transition to Shared
        let messages = protocol.handle_read(&key).await.unwrap_or_default();
        assert_eq!(messages.len(), 1);
        assert_eq!(protocol.get_state(&key).await, MSIState::Shared);

        // Write should send invalidations
        let messages = protocol.handle_write(&key).await.unwrap_or_default();
        assert!(!messages.is_empty());
        assert_eq!(protocol.get_state(&key).await, MSIState::Modified);
    }

    #[tokio::test]
    async fn test_mesi_protocol() {
        let protocol = MESIProtocol::new("node1".to_string());
        protocol.add_peer("node2".to_string()).await;

        let key = "test_key".to_string();

        // Read without other copies should be Exclusive
        let _messages = protocol.handle_read(&key, false).await.unwrap_or_default();
        assert_eq!(protocol.get_state(&key).await, MESIState::Exclusive);

        // Write should upgrade to Modified
        let _messages = protocol.handle_write(&key).await.unwrap_or_default();
        assert_eq!(protocol.get_state(&key).await, MESIState::Modified);
    }

    #[tokio::test]
    async fn test_directory_coherency() {
        let dir = DirectoryCoherency::new("node1".to_string());
        let key = "test_key".to_string();

        let _messages = dir.handle_read(&key).await.unwrap_or_default();
        let sharers = dir.get_sharers(&key).await;
        assert!(sharers.contains("node1"));

        let messages = dir.handle_write(&key).await.unwrap_or_default();
        assert!(messages.is_empty()); // No other sharers yet
    }

    #[tokio::test]
    async fn test_invalidation_batcher() {
        let batcher = InvalidationBatcher::new(3);

        // Add invalidations
        let result = batcher
            .add_invalidation("node1".to_string(), "key1".to_string())
            .await;
        assert!(result.is_none());

        let result = batcher
            .add_invalidation("node1".to_string(), "key2".to_string())
            .await;
        assert!(result.is_none());

        // This should trigger flush
        let result = batcher
            .add_invalidation("node1".to_string(), "key3".to_string())
            .await;
        assert!(result.is_some());
        let batch = result.unwrap_or_default();
        assert_eq!(batch.len(), 3);
    }
}
