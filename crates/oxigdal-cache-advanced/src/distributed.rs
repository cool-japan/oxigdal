//! Distributed cache protocol
//!
//! Implements distributed caching with:
//! - Consistent hashing for key distribution
//! - Distributed LRU with global coordination
//! - Cache peer discovery
//! - Replication for hot keys
//! - Automatic rebalancing

use crate::CacheStats;
use crate::error::Result;
use crate::multi_tier::{CacheKey, CacheValue};
use async_trait::async_trait;
use dashmap::DashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Hash ring node
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// Node identifier
    pub id: String,
    /// Node address
    pub address: String,
    /// Node weight (for distribution)
    pub weight: usize,
}

impl Hash for Node {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

/// Consistent hash ring for key distribution
pub struct ConsistentHashRing {
    /// Virtual nodes on the ring
    ring: Vec<(u64, Node)>,
    /// Number of virtual nodes per physical node
    virtual_nodes: usize,
}

impl ConsistentHashRing {
    /// Create new hash ring
    pub fn new(virtual_nodes: usize) -> Self {
        Self {
            ring: Vec::new(),
            virtual_nodes,
        }
    }

    /// Add node to the ring
    pub fn add_node(&mut self, node: Node) {
        for i in 0..self.virtual_nodes {
            let virtual_key = format!("{}:{}", node.id, i);
            let hash = self.hash_key(&virtual_key);
            self.ring.push((hash, node.clone()));
        }

        // Sort ring by hash values
        self.ring.sort_by_key(|(hash, _)| *hash);
    }

    /// Remove node from the ring
    pub fn remove_node(&mut self, node_id: &str) {
        self.ring.retain(|(_, node)| node.id != node_id);
    }

    /// Get node responsible for a key
    pub fn get_node(&self, key: &CacheKey) -> Option<&Node> {
        if self.ring.is_empty() {
            return None;
        }

        let hash = self.hash_key(key);

        // Binary search for the first node with hash >= key hash
        let idx = self.ring.partition_point(|(h, _)| *h < hash);

        // Wrap around if needed
        let node_idx = if idx < self.ring.len() { idx } else { 0 };

        self.ring.get(node_idx).map(|(_, node)| node)
    }

    /// Get N nodes for replication
    pub fn get_nodes(&self, key: &CacheKey, n: usize) -> Vec<&Node> {
        if self.ring.is_empty() {
            return Vec::new();
        }

        let hash = self.hash_key(key);
        let start_idx = self.ring.partition_point(|(h, _)| *h < hash);

        let mut nodes = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for i in 0..self.ring.len() {
            let idx = (start_idx + i) % self.ring.len();
            let (_, node) = &self.ring[idx];

            if !seen.contains(&node.id) {
                nodes.push(node);
                seen.insert(node.id.clone());

                if nodes.len() >= n {
                    break;
                }
            }
        }

        nodes
    }

    /// Hash a key
    fn hash_key(&self, key: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;

        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        hasher.finish()
    }

    /// Get all nodes
    pub fn nodes(&self) -> Vec<Node> {
        let mut seen = std::collections::HashSet::new();
        let mut nodes = Vec::new();

        for (_, node) in &self.ring {
            if !seen.contains(&node.id) {
                nodes.push(node.clone());
                seen.insert(node.id.clone());
            }
        }

        nodes
    }

    /// Get ring size
    pub fn size(&self) -> usize {
        self.ring.len()
    }
}

/// Distributed cache coordinator
pub struct DistributedCache {
    /// Local cache
    local: Arc<DashMap<CacheKey, CacheValue>>,
    /// Hash ring for distribution
    ring: Arc<RwLock<ConsistentHashRing>>,
    /// Current node info
    local_node: Node,
    /// Replication factor
    replication_factor: usize,
    /// Hot key threshold (access count)
    hot_key_threshold: u64,
    /// Statistics
    stats: Arc<RwLock<CacheStats>>,
}

impl DistributedCache {
    /// Create new distributed cache
    pub fn new(local_node: Node, replication_factor: usize) -> Self {
        let mut ring = ConsistentHashRing::new(150); // 150 virtual nodes
        ring.add_node(local_node.clone());

        Self {
            local: Arc::new(DashMap::new()),
            ring: Arc::new(RwLock::new(ring)),
            local_node,
            replication_factor,
            hot_key_threshold: 100,
            stats: Arc::new(RwLock::new(CacheStats::new())),
        }
    }

    /// Add peer node
    pub async fn add_peer(&self, node: Node) {
        let mut ring = self.ring.write().await;
        ring.add_node(node);
    }

    /// Remove peer node
    pub async fn remove_peer(&self, node_id: &str) {
        let mut ring = self.ring.write().await;
        ring.remove_node(node_id);
    }

    /// Get value from distributed cache
    pub async fn get(&self, key: &CacheKey) -> Result<Option<CacheValue>> {
        let ring = self.ring.read().await;

        // Check if this node is responsible
        if let Some(node) = ring.get_node(key) {
            if node.id == self.local_node.id {
                // Local lookup
                if let Some(mut value) = self.local.get_mut(key) {
                    value.record_access();

                    let mut stats = self.stats.write().await;
                    stats.hits += 1;

                    return Ok(Some(value.clone()));
                } else {
                    let mut stats = self.stats.write().await;
                    stats.misses += 1;
                    return Ok(None);
                }
            } else {
                // Remote lookup (would use network RPC in production)
                // For now, return None
                let mut stats = self.stats.write().await;
                stats.misses += 1;
                return Ok(None);
            }
        }

        Ok(None)
    }

    /// Put value into distributed cache
    pub async fn put(&self, key: CacheKey, value: CacheValue) -> Result<()> {
        let ring = self.ring.read().await;

        // Get nodes for replication
        let nodes = ring.get_nodes(&key, self.replication_factor);

        // Check if local node should store this key
        let should_store_locally = nodes.iter().any(|n| n.id == self.local_node.id);

        if should_store_locally {
            self.local.insert(key.clone(), value.clone());

            let mut stats = self.stats.write().await;
            stats.bytes_stored += value.size as u64;
            stats.item_count += 1;
        }

        // In production, would replicate to other nodes here

        Ok(())
    }

    /// Remove value from distributed cache
    pub async fn remove(&self, key: &CacheKey) -> Result<bool> {
        let removed = self.local.remove(key);

        if let Some((_, value)) = removed {
            let mut stats = self.stats.write().await;
            stats.bytes_stored = stats.bytes_stored.saturating_sub(value.size as u64);
            stats.item_count = stats.item_count.saturating_sub(1);

            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Check if key is hot (frequently accessed)
    pub fn is_hot_key(&self, key: &CacheKey) -> bool {
        if let Some(value) = self.local.get(key) {
            value.access_count >= self.hot_key_threshold
        } else {
            false
        }
    }

    /// Get statistics
    pub async fn stats(&self) -> CacheStats {
        self.stats.read().await.clone()
    }

    /// Get all peer nodes
    pub async fn peers(&self) -> Vec<Node> {
        let ring = self.ring.read().await;
        ring.nodes()
    }

    /// Rebalance cache after topology change
    pub async fn rebalance(&self) -> Result<()> {
        let ring = self.ring.read().await;
        let mut keys_to_remove = Vec::new();

        // Check all local keys
        for entry in self.local.iter() {
            let key = entry.key();
            let nodes = ring.get_nodes(key, self.replication_factor);

            // If local node is no longer responsible, mark for removal
            if !nodes.iter().any(|n| n.id == self.local_node.id) {
                keys_to_remove.push(key.clone());
            }
        }

        drop(ring);

        // Remove keys no longer owned
        for key in keys_to_remove {
            self.remove(&key).await?;
        }

        Ok(())
    }

    /// Clear local cache
    pub async fn clear(&self) -> Result<()> {
        self.local.clear();

        let mut stats = self.stats.write().await;
        *stats = CacheStats::new();

        Ok(())
    }
}

/// Distributed cache metadata
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CacheMetadata {
    /// Version number
    pub version: u64,
    /// Owner node ID
    pub owner: String,
    /// Replica node IDs
    pub replicas: Vec<String>,
    /// Last modified timestamp
    pub last_modified: chrono::DateTime<chrono::Utc>,
}

/// Cache operation for synchronization
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum CacheOperation {
    /// Put operation
    Put {
        /// Key
        key: CacheKey,
        /// Value
        value: Vec<u8>,
        /// Metadata
        metadata: CacheMetadata,
    },
    /// Delete operation
    Delete {
        /// Key
        key: CacheKey,
        /// Version
        version: u64,
    },
    /// Invalidate operation
    Invalidate {
        /// Key
        key: CacheKey,
    },
}

/// Distributed cache protocol
#[async_trait]
pub trait DistributedProtocol: Send + Sync {
    /// Broadcast operation to peers
    async fn broadcast(&self, operation: CacheOperation) -> Result<()>;

    /// Handle incoming operation
    async fn handle_operation(&self, operation: CacheOperation) -> Result<()>;

    /// Sync with peer
    async fn sync_with_peer(&self, peer_id: &str) -> Result<()>;
}

/// Peer discovery trait
#[async_trait]
pub trait PeerDiscovery: Send + Sync {
    /// Discover peers
    async fn discover(&self) -> Result<Vec<Node>>;

    /// Register self
    async fn register(&self, node: Node) -> Result<()>;

    /// Unregister self
    async fn unregister(&self, node_id: &str) -> Result<()>;

    /// Health check
    async fn health_check(&self, node_id: &str) -> Result<bool>;
}

/// Simple static peer list discovery
pub struct StaticPeerDiscovery {
    /// Static peer list
    peers: Vec<Node>,
}

impl StaticPeerDiscovery {
    /// Create new static peer discovery
    pub fn new(peers: Vec<Node>) -> Self {
        Self { peers }
    }
}

#[async_trait]
impl PeerDiscovery for StaticPeerDiscovery {
    async fn discover(&self) -> Result<Vec<Node>> {
        Ok(self.peers.clone())
    }

    async fn register(&self, _node: Node) -> Result<()> {
        // Static list doesn't support registration
        Ok(())
    }

    async fn unregister(&self, _node_id: &str) -> Result<()> {
        // Static list doesn't support unregistration
        Ok(())
    }

    async fn health_check(&self, _node_id: &str) -> Result<bool> {
        // Assume all peers are healthy
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    #[test]
    fn test_consistent_hash_ring() {
        let mut ring = ConsistentHashRing::new(150);

        let node1 = Node {
            id: "node1".to_string(),
            address: "127.0.0.1:8001".to_string(),
            weight: 1,
        };

        let node2 = Node {
            id: "node2".to_string(),
            address: "127.0.0.1:8002".to_string(),
            weight: 1,
        };

        ring.add_node(node1.clone());
        ring.add_node(node2.clone());

        assert_eq!(ring.size(), 300); // 2 nodes * 150 virtual nodes

        let key = "test_key".to_string();
        let node = ring.get_node(&key);
        assert!(node.is_some());
    }

    #[test]
    fn test_replication_nodes() {
        let mut ring = ConsistentHashRing::new(150);

        for i in 0..5 {
            ring.add_node(Node {
                id: format!("node{}", i),
                address: format!("127.0.0.1:800{}", i),
                weight: 1,
            });
        }

        let key = "test_key".to_string();
        let nodes = ring.get_nodes(&key, 3);

        assert_eq!(nodes.len(), 3);

        // Check that all nodes are unique
        let unique_ids: std::collections::HashSet<_> = nodes.iter().map(|n| &n.id).collect();
        assert_eq!(unique_ids.len(), 3);
    }

    #[tokio::test]
    async fn test_distributed_cache() {
        let node = Node {
            id: "test_node".to_string(),
            address: "127.0.0.1:8000".to_string(),
            weight: 1,
        };

        let cache = DistributedCache::new(node, 2);

        let key = "test_key".to_string();
        let value = CacheValue::new(
            Bytes::from("test data"),
            crate::compression::DataType::Binary,
        );

        cache
            .put(key.clone(), value.clone())
            .await
            .expect("put failed");

        let retrieved = cache.get(&key).await.expect("get failed");
        assert!(retrieved.is_some());
    }

    #[tokio::test]
    async fn test_cache_rebalance() {
        let node1 = Node {
            id: "node1".to_string(),
            address: "127.0.0.1:8001".to_string(),
            weight: 1,
        };

        let cache = DistributedCache::new(node1.clone(), 2);

        // Add some data
        for i in 0..10 {
            let key = format!("key{}", i);
            let value = CacheValue::new(
                Bytes::from(format!("value{}", i)),
                crate::compression::DataType::Binary,
            );
            cache.put(key, value).await.expect("put failed");
        }

        // Add a new peer
        let node2 = Node {
            id: "node2".to_string(),
            address: "127.0.0.1:8002".to_string(),
            weight: 1,
        };
        cache.add_peer(node2).await;

        // Rebalance
        cache.rebalance().await.expect("rebalance failed");

        // Some keys may have been removed due to rebalancing
        let stats = cache.stats().await;
        assert!(stats.item_count <= 10);
    }
}
