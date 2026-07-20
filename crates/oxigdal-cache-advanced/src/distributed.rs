//! Distributed cache protocol
//!
//! Implements distributed caching with:
//! - Consistent hashing for key distribution
//! - Distributed LRU with global coordination
//! - Cache peer discovery (see [`PeerDiscovery`] / [`StaticPeerDiscovery`])
//! - Replication for keys owned by remote nodes
//! - Automatic rebalancing
//!
//! Remote reads and replicated writes are routed through an injected
//! [`PeerTransport`]. When a key is owned by (or must be replicated to) a
//! remote node and **no** transport is configured, [`DistributedCache`] returns
//! an explicit [`crate::error::CacheError::Network`] error instead of silently
//! discarding the write or reporting a false cache miss. An in-process
//! [`InMemoryTransport`] is provided for single-process multi-node clusters and
//! tests; production deployments implement [`PeerTransport`] over a real
//! Pure-Rust RPC layer.

use crate::CacheStats;
use crate::error::{CacheError, Result};
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
    /// Transport for remote peer operations (reads and replicated writes).
    ///
    /// When `None`, any operation that targets a remote node fails loudly with
    /// [`CacheError::Network`] rather than silently no-op'ing.
    transport: Option<Arc<dyn PeerTransport>>,
}

impl DistributedCache {
    /// Create new distributed cache without a peer transport.
    ///
    /// Suitable for single-node deployments. Once remote peers are added to the
    /// ring (via [`DistributedCache::add_peer`]), operations that resolve to a
    /// remote node return [`CacheError::Network`] because there is no transport
    /// to carry them. Use [`DistributedCache::with_transport`] for multi-node
    /// clusters.
    pub fn new(local_node: Node, replication_factor: usize) -> Self {
        Self::build(local_node, replication_factor, None)
    }

    /// Create a new distributed cache wired to a peer transport.
    ///
    /// Remote reads are served via [`PeerTransport::remote_get`] and writes for
    /// keys owned by remote nodes are replicated via
    /// [`PeerTransport::remote_put`].
    pub fn with_transport(
        local_node: Node,
        replication_factor: usize,
        transport: Arc<dyn PeerTransport>,
    ) -> Self {
        Self::build(local_node, replication_factor, Some(transport))
    }

    fn build(
        local_node: Node,
        replication_factor: usize,
        transport: Option<Arc<dyn PeerTransport>>,
    ) -> Self {
        let mut ring = ConsistentHashRing::new(150); // 150 virtual nodes
        ring.add_node(local_node.clone());

        Self {
            local: Arc::new(DashMap::new()),
            ring: Arc::new(RwLock::new(ring)),
            local_node,
            replication_factor,
            hot_key_threshold: 100,
            stats: Arc::new(RwLock::new(CacheStats::new())),
            transport,
        }
    }

    /// Shared handle to this node's local store.
    ///
    /// Exposed so an [`InMemoryTransport`] (or another in-process transport) can
    /// be wired to serve this node's data to its peers.
    pub fn local_store(&self) -> Arc<DashMap<CacheKey, CacheValue>> {
        Arc::clone(&self.local)
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
    ///
    /// If the key is owned by the local node, it is served from the local store.
    /// If it is owned by a remote node, the lookup is routed through the
    /// configured [`PeerTransport`]. When the owner is remote and no transport
    /// is configured this returns [`CacheError::Network`] rather than reporting
    /// a false cache miss.
    pub async fn get(&self, key: &CacheKey) -> Result<Option<CacheValue>> {
        // Resolve the responsible node without holding the ring lock across awaits.
        let owner = {
            let ring = self.ring.read().await;
            ring.get_node(key).cloned()
        };

        let owner = match owner {
            Some(node) => node,
            None => return Ok(None), // empty ring: nothing can be stored
        };

        if owner.id == self.local_node.id {
            // Local lookup
            if let Some(mut value) = self.local.get_mut(key) {
                value.record_access();

                let mut stats = self.stats.write().await;
                stats.hits += 1;

                return Ok(Some(value.clone()));
            }

            let mut stats = self.stats.write().await;
            stats.misses += 1;
            return Ok(None);
        }

        // Remote lookup: never silently miss. Require a transport.
        let transport = self.transport.as_ref().ok_or_else(|| {
            CacheError::Network(format!(
                "key is owned by remote node '{}' ({}) but no peer transport is configured",
                owner.id, owner.address
            ))
        })?;

        let result = transport.remote_get(&owner, key).await?;

        let mut stats = self.stats.write().await;
        if result.is_some() {
            stats.hits += 1;
        } else {
            stats.misses += 1;
        }

        Ok(result)
    }

    /// Put value into distributed cache
    ///
    /// The value is written to every replica returned by the consistent-hash
    /// ring (up to the replication factor). The local replica is written
    /// directly; remote replicas are written through the configured
    /// [`PeerTransport`]. If the key must be replicated to a remote node and no
    /// transport is configured, this returns [`CacheError::Network`] instead of
    /// silently discarding the write.
    pub async fn put(&self, key: CacheKey, value: CacheValue) -> Result<()> {
        // Snapshot the replica set without holding the ring lock across awaits.
        let replicas: Vec<Node> = {
            let ring = self.ring.read().await;
            ring.get_nodes(&key, self.replication_factor)
                .into_iter()
                .cloned()
                .collect()
        };

        if replicas.is_empty() {
            return Err(CacheError::Network(
                "cannot store key: hash ring has no nodes".to_string(),
            ));
        }

        let store_locally = replicas.iter().any(|n| n.id == self.local_node.id);
        let remote_replicas: Vec<&Node> = replicas
            .iter()
            .filter(|n| n.id != self.local_node.id)
            .collect();

        // Fail loudly rather than silently dropping remote replicas.
        if !remote_replicas.is_empty() && self.transport.is_none() {
            let ids: Vec<&str> = remote_replicas.iter().map(|n| n.id.as_str()).collect();
            return Err(CacheError::Network(format!(
                "key must be replicated to remote node(s) [{}] but no peer transport is configured",
                ids.join(", ")
            )));
        }

        if store_locally {
            self.local.insert(key.clone(), value.clone());

            let mut stats = self.stats.write().await;
            stats.bytes_stored += value.size as u64;
            stats.item_count += 1;
        }

        // Replicate to remote nodes through the transport.
        if let Some(transport) = self.transport.as_ref() {
            for node in remote_replicas {
                transport.remote_put(node, &key, &value).await?;
            }
        }

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

/// Transport for cache operations that target remote peer nodes.
///
/// [`DistributedCache`] uses this to perform remote reads and to replicate
/// writes for keys owned by other nodes. Implementations must be Pure Rust
/// (no C/C++/Fortran deps). A network implementation carries these calls over
/// an RPC layer; [`InMemoryTransport`] carries them within a single process.
#[async_trait]
pub trait PeerTransport: Send + Sync {
    /// Fetch a value from a remote node.
    ///
    /// Returns `Ok(None)` for a genuine remote miss and `Err` for a transport
    /// failure (unreachable node, protocol error, ...).
    async fn remote_get(&self, node: &Node, key: &CacheKey) -> Result<Option<CacheValue>>;

    /// Store (replicate) a value on a remote node.
    async fn remote_put(&self, node: &Node, key: &CacheKey, value: &CacheValue) -> Result<()>;

    /// Remove a value from a remote node. Returns whether it existed.
    async fn remote_remove(&self, node: &Node, key: &CacheKey) -> Result<bool>;
}

/// In-process [`PeerTransport`] backed by shared per-node stores.
///
/// Each participating [`DistributedCache`] registers its local store (obtained
/// via [`DistributedCache::local_store`]) under its node id. Remote operations
/// then read from / write to the target node's actual store, so writes
/// round-trip correctly. This is a fully functional transport for single-process
/// multi-node clusters and the basis for deterministic tests.
#[derive(Default)]
pub struct InMemoryTransport {
    /// Registered node stores keyed by node id.
    stores: DashMap<String, Arc<DashMap<CacheKey, CacheValue>>>,
}

impl InMemoryTransport {
    /// Create an empty transport with no registered nodes.
    pub fn new() -> Self {
        Self {
            stores: DashMap::new(),
        }
    }

    /// Register a node's store so peers can reach it through this transport.
    pub fn register_node(
        &self,
        node_id: impl Into<String>,
        store: Arc<DashMap<CacheKey, CacheValue>>,
    ) {
        self.stores.insert(node_id.into(), store);
    }

    /// Deregister a node's store.
    pub fn unregister_node(&self, node_id: &str) {
        self.stores.remove(node_id);
    }

    fn store_for(&self, node: &Node) -> Result<Arc<DashMap<CacheKey, CacheValue>>> {
        self.stores
            .get(&node.id)
            .map(|entry| Arc::clone(entry.value()))
            .ok_or_else(|| {
                CacheError::Network(format!(
                    "peer node '{}' ({}) is not registered in the transport",
                    node.id, node.address
                ))
            })
    }
}

#[async_trait]
impl PeerTransport for InMemoryTransport {
    async fn remote_get(&self, node: &Node, key: &CacheKey) -> Result<Option<CacheValue>> {
        let store = self.store_for(node)?;
        Ok(store.get(key).map(|entry| entry.value().clone()))
    }

    async fn remote_put(&self, node: &Node, key: &CacheKey, value: &CacheValue) -> Result<()> {
        let store = self.store_for(node)?;
        store.insert(key.clone(), value.clone());
        Ok(())
    }

    async fn remote_remove(&self, node: &Node, key: &CacheKey) -> Result<bool> {
        let store = self.store_for(node)?;
        Ok(store.remove(key).is_some())
    }
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

    fn node(id: &str, port: u16) -> Node {
        Node {
            id: id.to_string(),
            address: format!("127.0.0.1:{}", port),
            weight: 1,
        }
    }

    /// Find a key that the ring (containing `a` then `b`) assigns to node `b`.
    fn key_owned_by(a: &Node, b: &Node, target: &str) -> CacheKey {
        let mut ring = ConsistentHashRing::new(150);
        ring.add_node(a.clone());
        ring.add_node(b.clone());
        for i in 0..100_000u32 {
            let key = format!("probe-key-{}", i);
            if let Some(owner) = ring.get_node(&key)
                && owner.id == target
            {
                return key;
            }
        }
        panic!("no key mapped to node {target}");
    }

    #[tokio::test]
    async fn test_remote_ops_fail_loud_without_transport() {
        let node_a = node("node_a", 9001);
        let node_b = node("node_b", 9002);

        // Single-node cache with NO transport, then a remote peer is added.
        let cache = DistributedCache::new(node_a.clone(), 1);
        cache.add_peer(node_b.clone()).await;

        let remote_key = key_owned_by(&node_a, &node_b, "node_b");
        let value = CacheValue::new(Bytes::from("payload"), crate::compression::DataType::Binary);

        // put() must NOT silently drop the write — it must error.
        let put_err = cache.put(remote_key.clone(), value).await;
        assert!(
            put_err.is_err(),
            "put for a remote-owned key with no transport must fail loudly"
        );

        // get() must NOT report a false miss — it must error.
        let get_err = cache.get(&remote_key).await;
        assert!(
            get_err.is_err(),
            "get for a remote-owned key with no transport must fail loudly"
        );
    }

    #[tokio::test]
    async fn test_remote_put_get_roundtrip_with_transport() {
        let node_a = node("node_a", 9101);
        let node_b = node("node_b", 9102);

        let transport = Arc::new(InMemoryTransport::new());

        let cache_a = DistributedCache::with_transport(node_a.clone(), 1, transport.clone());
        let cache_b = DistributedCache::with_transport(node_b.clone(), 1, transport.clone());

        // Wire both nodes' stores into the shared transport.
        transport.register_node(node_a.id.clone(), cache_a.local_store());
        transport.register_node(node_b.id.clone(), cache_b.local_store());

        // node_a's ring knows about both peers.
        cache_a.add_peer(node_b.clone()).await;

        let remote_key = key_owned_by(&node_a, &node_b, "node_b");
        let value = CacheValue::new(
            Bytes::from("replicated payload"),
            crate::compression::DataType::Binary,
        );

        // Writing a remote-owned key replicates it to node_b via the transport.
        cache_a
            .put(remote_key.clone(), value.clone())
            .await
            .expect("put should replicate to the remote node");

        // The value must actually exist on node_b's store.
        assert!(
            cache_b.local_store().contains_key(&remote_key),
            "value must be replicated onto the remote node's store"
        );

        // Reading it back through node_a must round-trip via the transport.
        let retrieved = cache_a
            .get(&remote_key)
            .await
            .expect("remote get should succeed via transport");
        assert!(
            retrieved.is_some(),
            "remote-owned key must round-trip, not report a false miss"
        );
        assert_eq!(
            retrieved.map(|v| v.data),
            Some(Bytes::from("replicated payload"))
        );
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
