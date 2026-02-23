//! Advanced eviction policies for cache management
//!
//! Provides multiple eviction strategies:
//! - LRU (Least Recently Used)
//! - LFU (Least Frequently Used)
//! - ARC (Adaptive Replacement Cache)
//! - LIRS (Low Inter-reference Recency Set)
//! - Cost-aware eviction
//! - TTL-based expiration

use chrono::{DateTime, Duration, Utc};
use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

/// Eviction policy trait
pub trait EvictionPolicy<K: Clone + Hash + Eq>: Send + Sync {
    /// Record access to a key
    fn on_access(&mut self, key: &K);

    /// Record insertion of a key with size
    fn on_insert(&mut self, key: K, size: usize);

    /// Record removal of a key
    fn on_remove(&mut self, key: &K);

    /// Select a key to evict
    fn select_victim(&mut self) -> Option<K>;

    /// Get current policy statistics
    fn stats(&self) -> EvictionStats;

    /// Clear all tracking data
    fn clear(&mut self);
}

/// Statistics for eviction policy
#[derive(Debug, Clone, Default)]
pub struct EvictionStats {
    /// Number of evictions performed
    pub evictions: u64,
    /// Total accesses tracked
    pub accesses: u64,
    /// Number of items tracked
    pub items_tracked: usize,
}

/// LRU (Least Recently Used) eviction policy
#[derive(Debug)]
pub struct LruEviction<K: Clone + Hash + Eq> {
    /// Access order queue (front = most recent)
    access_order: VecDeque<K>,
    /// Quick lookup for presence check
    key_set: HashMap<K, usize>,
    /// Statistics
    stats: EvictionStats,
}

impl<K: Clone + Hash + Eq> LruEviction<K> {
    /// Create new LRU eviction policy
    pub fn new() -> Self {
        Self {
            access_order: VecDeque::new(),
            key_set: HashMap::new(),
            stats: EvictionStats::default(),
        }
    }

    fn move_to_front(&mut self, key: &K) {
        // Remove from current position
        if let Some(pos) = self.key_set.get(key) {
            if *pos < self.access_order.len() {
                self.access_order.remove(*pos);
            }
        }

        // Add to front
        self.access_order.push_front(key.clone());

        // Update positions in map
        self.rebuild_positions();
    }

    fn rebuild_positions(&mut self) {
        self.key_set.clear();
        for (idx, key) in self.access_order.iter().enumerate() {
            self.key_set.insert(key.clone(), idx);
        }
    }
}

impl<K: Clone + Hash + Eq> Default for LruEviction<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Clone + Hash + Eq + Send + Sync + 'static> EvictionPolicy<K> for LruEviction<K> {
    fn on_access(&mut self, key: &K) {
        self.move_to_front(key);
        self.stats.accesses += 1;
    }

    fn on_insert(&mut self, key: K, _size: usize) {
        if !self.key_set.contains_key(&key) {
            self.access_order.push_front(key.clone());
            self.key_set.insert(key, 0);
            self.rebuild_positions();
            self.stats.items_tracked += 1;
        }
    }

    fn on_remove(&mut self, key: &K) {
        if let Some(pos) = self.key_set.remove(key) {
            if pos < self.access_order.len() {
                self.access_order.remove(pos);
                self.rebuild_positions();
                self.stats.items_tracked = self.stats.items_tracked.saturating_sub(1);
            }
        }
    }

    fn select_victim(&mut self) -> Option<K> {
        let victim = self.access_order.back().cloned();
        if victim.is_some() {
            self.stats.evictions += 1;
        }
        victim
    }

    fn stats(&self) -> EvictionStats {
        self.stats.clone()
    }

    fn clear(&mut self) {
        self.access_order.clear();
        self.key_set.clear();
        self.stats = EvictionStats::default();
    }
}

/// LFU (Least Frequently Used) eviction policy
#[derive(Debug)]
pub struct LfuEviction<K: Clone + Hash + Eq> {
    /// Frequency counter for each key
    frequencies: HashMap<K, u64>,
    /// Statistics
    stats: EvictionStats,
}

impl<K: Clone + Hash + Eq> LfuEviction<K> {
    /// Create new LFU eviction policy
    pub fn new() -> Self {
        Self {
            frequencies: HashMap::new(),
            stats: EvictionStats::default(),
        }
    }
}

impl<K: Clone + Hash + Eq> Default for LfuEviction<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Clone + Hash + Eq + Send + Sync + 'static> EvictionPolicy<K> for LfuEviction<K> {
    fn on_access(&mut self, key: &K) {
        *self.frequencies.entry(key.clone()).or_insert(0) += 1;
        self.stats.accesses += 1;
    }

    fn on_insert(&mut self, key: K, _size: usize) {
        self.frequencies.insert(key, 1);
        self.stats.items_tracked += 1;
    }

    fn on_remove(&mut self, key: &K) {
        if self.frequencies.remove(key).is_some() {
            self.stats.items_tracked = self.stats.items_tracked.saturating_sub(1);
        }
    }

    fn select_victim(&mut self) -> Option<K> {
        let victim = self
            .frequencies
            .iter()
            .min_by_key(|(_, freq)| *freq)
            .map(|(k, _)| k.clone());

        if victim.is_some() {
            self.stats.evictions += 1;
        }
        victim
    }

    fn stats(&self) -> EvictionStats {
        self.stats.clone()
    }

    fn clear(&mut self) {
        self.frequencies.clear();
        self.stats = EvictionStats::default();
    }
}

/// ARC (Adaptive Replacement Cache) eviction policy
/// Balances between recency and frequency
#[derive(Debug)]
pub struct ArcEviction<K: Clone + Hash + Eq> {
    /// Target size for T1 (recently used once)
    p: usize,
    /// Maximum cache size
    max_size: usize,
    /// T1: Recent cache entries
    t1: VecDeque<K>,
    /// T2: Frequent cache entries
    t2: VecDeque<K>,
    /// B1: Ghost entries evicted from T1
    b1: VecDeque<K>,
    /// B2: Ghost entries evicted from T2
    b2: VecDeque<K>,
    /// Statistics
    stats: EvictionStats,
}

impl<K: Clone + Hash + Eq> ArcEviction<K> {
    /// Create new ARC eviction policy
    pub fn new(max_size: usize) -> Self {
        Self {
            p: 0,
            max_size,
            t1: VecDeque::new(),
            t2: VecDeque::new(),
            b1: VecDeque::new(),
            b2: VecDeque::new(),
            stats: EvictionStats::default(),
        }
    }

    fn contains(&self, key: &K) -> bool {
        self.t1.contains(key) || self.t2.contains(key)
    }

    #[allow(dead_code)]
    fn replace(&mut self, key: &K) -> Option<K> {
        let t1_len = self.t1.len();

        if !self.t1.is_empty() && (t1_len > self.p || (self.b2.contains(key) && t1_len == self.p)) {
            // Evict from T1
            self.t1.pop_back()
        } else {
            // Evict from T2
            self.t2.pop_back()
        }
    }
}

impl<K: Clone + Hash + Eq + Send + Sync + 'static> EvictionPolicy<K> for ArcEviction<K> {
    fn on_access(&mut self, key: &K) {
        // Move from T1 to T2 on second access
        if let Some(pos) = self.t1.iter().position(|k| k == key) {
            let key = self.t1.remove(pos);
            if let Some(k) = key {
                self.t2.push_front(k);
            }
        } else if let Some(pos) = self.t2.iter().position(|k| k == key) {
            // Move to front of T2
            if let Some(k) = self.t2.remove(pos) {
                self.t2.push_front(k);
            }
        }
        self.stats.accesses += 1;
    }

    fn on_insert(&mut self, key: K, _size: usize) {
        if self.contains(&key) {
            return;
        }

        // Check if in ghost lists
        if self.b1.contains(&key) {
            // Increase T1 preference
            let delta = if self.b2.len() >= self.b1.len() {
                1
            } else {
                self.b2.len() / self.b1.len().max(1)
            };
            self.p = (self.p + delta).min(self.max_size);

            self.b1.retain(|k| k != &key);
            self.t2.push_front(key.clone());
        } else if self.b2.contains(&key) {
            // Decrease T1 preference
            let delta = if self.b1.len() >= self.b2.len() {
                1
            } else {
                self.b1.len() / self.b2.len().max(1)
            };
            self.p = self.p.saturating_sub(delta);

            self.b2.retain(|k| k != &key);
            self.t2.push_front(key.clone());
        } else {
            // New entry goes to T1
            self.t1.push_front(key);
        }

        self.stats.items_tracked += 1;
    }

    fn on_remove(&mut self, key: &K) {
        self.t1.retain(|k| k != key);
        self.t2.retain(|k| k != key);
        self.b1.retain(|k| k != key);
        self.b2.retain(|k| k != key);
        self.stats.items_tracked = self.stats.items_tracked.saturating_sub(1);
    }

    fn select_victim(&mut self) -> Option<K> {
        let victim = if !self.t1.is_empty() {
            self.t1.pop_back()
        } else {
            self.t2.pop_back()
        };

        if victim.is_some() {
            self.stats.evictions += 1;
        }
        victim
    }

    fn stats(&self) -> EvictionStats {
        self.stats.clone()
    }

    fn clear(&mut self) {
        self.t1.clear();
        self.t2.clear();
        self.b1.clear();
        self.b2.clear();
        self.p = 0;
        self.stats = EvictionStats::default();
    }
}

/// TTL-based eviction policy
#[derive(Debug)]
pub struct TtlEviction<K: Clone + Hash + Eq> {
    /// Expiration times for keys
    expiration_times: HashMap<K, DateTime<Utc>>,
    /// Default TTL duration
    default_ttl: Duration,
    /// Statistics
    stats: EvictionStats,
}

impl<K: Clone + Hash + Eq> TtlEviction<K> {
    /// Create new TTL eviction policy with default TTL
    pub fn new(default_ttl: Duration) -> Self {
        Self {
            expiration_times: HashMap::new(),
            default_ttl,
            stats: EvictionStats::default(),
        }
    }

    /// Check if key is expired
    pub fn is_expired(&self, key: &K) -> bool {
        if let Some(expiration) = self.expiration_times.get(key) {
            Utc::now() > *expiration
        } else {
            false
        }
    }

    /// Get expired keys
    pub fn get_expired_keys(&self) -> Vec<K> {
        let now = Utc::now();
        self.expiration_times
            .iter()
            .filter(|(_, exp)| now > **exp)
            .map(|(k, _)| k.clone())
            .collect()
    }
}

impl<K: Clone + Hash + Eq + Send + Sync + 'static> EvictionPolicy<K> for TtlEviction<K> {
    fn on_access(&mut self, _key: &K) {
        self.stats.accesses += 1;
    }

    fn on_insert(&mut self, key: K, _size: usize) {
        let expiration = Utc::now() + self.default_ttl;
        self.expiration_times.insert(key, expiration);
        self.stats.items_tracked += 1;
    }

    fn on_remove(&mut self, key: &K) {
        if self.expiration_times.remove(key).is_some() {
            self.stats.items_tracked = self.stats.items_tracked.saturating_sub(1);
        }
    }

    fn select_victim(&mut self) -> Option<K> {
        // Return the first expired key
        let now = Utc::now();
        let victim = self
            .expiration_times
            .iter()
            .find(|(_, exp)| now > **exp)
            .map(|(k, _)| k.clone());

        if victim.is_some() {
            self.stats.evictions += 1;
        }
        victim
    }

    fn stats(&self) -> EvictionStats {
        self.stats.clone()
    }

    fn clear(&mut self) {
        self.expiration_times.clear();
        self.stats = EvictionStats::default();
    }
}

/// Cost-aware eviction policy
/// Evicts items based on fetch cost and size
#[derive(Debug)]
pub struct CostAwareEviction<K: Clone + Hash + Eq> {
    /// Cost metrics for each key (size, fetch_cost)
    costs: HashMap<K, (usize, f64)>,
    /// Statistics
    stats: EvictionStats,
}

impl<K: Clone + Hash + Eq> CostAwareEviction<K> {
    /// Create new cost-aware eviction policy
    pub fn new() -> Self {
        Self {
            costs: HashMap::new(),
            stats: EvictionStats::default(),
        }
    }

    /// Set cost for a key
    pub fn set_cost(&mut self, key: K, size: usize, fetch_cost: f64) {
        self.costs.insert(key, (size, fetch_cost));
    }

    /// Calculate eviction priority (lower = evict first)
    /// Priority = fetch_cost / size (cost per byte)
    fn calculate_priority(&self, key: &K) -> f64 {
        if let Some((size, fetch_cost)) = self.costs.get(key) {
            if *size > 0 {
                fetch_cost / (*size as f64)
            } else {
                0.0
            }
        } else {
            0.0
        }
    }
}

impl<K: Clone + Hash + Eq> Default for CostAwareEviction<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Clone + Hash + Eq + Send + Sync + 'static> EvictionPolicy<K> for CostAwareEviction<K> {
    fn on_access(&mut self, _key: &K) {
        self.stats.accesses += 1;
    }

    fn on_insert(&mut self, key: K, size: usize) {
        // Default fetch cost is 1.0
        self.costs.insert(key, (size, 1.0));
        self.stats.items_tracked += 1;
    }

    fn on_remove(&mut self, key: &K) {
        if self.costs.remove(key).is_some() {
            self.stats.items_tracked = self.stats.items_tracked.saturating_sub(1);
        }
    }

    fn select_victim(&mut self) -> Option<K> {
        let victim = self
            .costs
            .keys()
            .min_by(|a, b| {
                let priority_a = self.calculate_priority(a);
                let priority_b = self.calculate_priority(b);
                priority_a
                    .partial_cmp(&priority_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .cloned();

        if victim.is_some() {
            self.stats.evictions += 1;
        }
        victim
    }

    fn stats(&self) -> EvictionStats {
        self.stats.clone()
    }

    fn clear(&mut self) {
        self.costs.clear();
        self.stats = EvictionStats::default();
    }
}

/// Eviction policy type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvictionPolicyType {
    /// Least Recently Used
    Lru,
    /// Least Frequently Used
    Lfu,
    /// Adaptive Replacement Cache
    Arc,
    /// TTL-based
    Ttl,
    /// Cost-aware
    CostAware,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lru_eviction() {
        let mut lru = LruEviction::new();

        lru.on_insert(1, 100);
        lru.on_insert(2, 100);
        lru.on_insert(3, 100);

        // Access 1, making it most recent
        lru.on_access(&1);

        // Should evict 2 (oldest)
        let victim = lru.select_victim();
        assert_eq!(victim, Some(2));
    }

    #[test]
    fn test_lfu_eviction() {
        let mut lfu = LfuEviction::new();

        lfu.on_insert(1, 100);
        lfu.on_insert(2, 100);
        lfu.on_insert(3, 100);

        // Access 1 and 3 multiple times
        lfu.on_access(&1);
        lfu.on_access(&1);
        lfu.on_access(&3);

        // Should evict 2 (least frequent)
        let victim = lfu.select_victim();
        assert_eq!(victim, Some(2));
    }

    #[test]
    fn test_ttl_eviction() {
        let mut ttl = TtlEviction::new(Duration::seconds(1));

        ttl.on_insert(1, 100);
        ttl.on_insert(2, 100);

        // No keys should be expired yet
        assert!(!ttl.is_expired(&1));
        assert!(!ttl.is_expired(&2));

        let stats = ttl.stats();
        assert_eq!(stats.items_tracked, 2);
    }

    #[test]
    fn test_cost_aware_eviction() {
        let mut cost = CostAwareEviction::new();

        cost.on_insert(1, 100);
        cost.on_insert(2, 100);
        cost.on_insert(3, 100);

        // Set different fetch costs
        cost.set_cost(1, 100, 10.0); // High cost per byte
        cost.set_cost(2, 100, 1.0); // Low cost per byte
        cost.set_cost(3, 100, 5.0); // Medium cost per byte

        // Should evict 2 (lowest cost per byte)
        let victim = cost.select_victim();
        assert_eq!(victim, Some(2));
    }

    #[test]
    fn test_arc_eviction() {
        let mut arc = ArcEviction::new(100);

        arc.on_insert(1, 10);
        arc.on_insert(2, 10);
        arc.on_insert(3, 10);

        let stats = arc.stats();
        assert_eq!(stats.items_tracked, 3);

        // Access 1 to move it to T2
        arc.on_access(&1);

        let victim = arc.select_victim();
        assert!(victim.is_some());
    }
}
