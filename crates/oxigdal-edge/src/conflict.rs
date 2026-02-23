//! CRDT-based conflict resolution for distributed edge nodes
//!
//! Provides Conflict-free Replicated Data Types (CRDTs) for automatic
//! conflict resolution in distributed edge computing environments.

use ahash::AHashMap;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fmt;

/// Vector clock for tracking causality
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VectorClock {
    clock: AHashMap<String, u64>,
}

impl VectorClock {
    /// Create a new vector clock
    pub fn new() -> Self {
        Self {
            clock: AHashMap::new(),
        }
    }

    /// Increment clock for node
    pub fn increment(&mut self, node_id: &str) {
        let counter = self.clock.entry(node_id.to_string()).or_insert(0);
        *counter = counter.saturating_add(1);
    }

    /// Get clock value for node
    pub fn get(&self, node_id: &str) -> u64 {
        self.clock.get(node_id).copied().unwrap_or(0)
    }

    /// Merge with another vector clock
    pub fn merge(&mut self, other: &VectorClock) {
        for (node_id, &other_count) in &other.clock {
            let count = self.clock.entry(node_id.clone()).or_insert(0);
            *count = (*count).max(other_count);
        }
    }

    /// Compare vector clocks for causality
    pub fn compare(&self, other: &VectorClock) -> ClockOrdering {
        let mut less = false;
        let mut greater = false;

        // Check all nodes in self
        for (node_id, &self_count) in &self.clock {
            let other_count = other.get(node_id);
            match self_count.cmp(&other_count) {
                Ordering::Less => less = true,
                Ordering::Greater => greater = true,
                Ordering::Equal => {}
            }
        }

        // Check nodes only in other
        for node_id in other.clock.keys() {
            if !self.clock.contains_key(node_id) {
                less = true;
            }
        }

        match (less, greater) {
            (true, false) => ClockOrdering::Before,
            (false, true) => ClockOrdering::After,
            (false, false) => ClockOrdering::Equal,
            (true, true) => ClockOrdering::Concurrent,
        }
    }

    /// Check if this clock is concurrent with another
    pub fn is_concurrent(&self, other: &VectorClock) -> bool {
        matches!(self.compare(other), ClockOrdering::Concurrent)
    }

    /// Check if this clock happens before another
    pub fn happens_before(&self, other: &VectorClock) -> bool {
        matches!(self.compare(other), ClockOrdering::Before)
    }
}

impl Default for VectorClock {
    fn default() -> Self {
        Self::new()
    }
}

/// Clock ordering relationship
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockOrdering {
    /// This clock is before the other
    Before,
    /// This clock is after the other
    After,
    /// Clocks are equal
    Equal,
    /// Clocks are concurrent (conflict)
    Concurrent,
}

/// Last-Write-Wins Register CRDT
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LwwRegister<T> {
    value: T,
    timestamp: VectorClock,
    logical_time: u64,
    node_id: String,
}

impl<T: Clone> LwwRegister<T> {
    /// Create new LWW register
    pub fn new(value: T, node_id: String) -> Self {
        let mut timestamp = VectorClock::new();
        timestamp.increment(&node_id);
        Self {
            value,
            timestamp,
            logical_time: 1,
            node_id,
        }
    }

    /// Get current value
    pub fn value(&self) -> &T {
        &self.value
    }

    /// Update value
    pub fn update(&mut self, value: T) {
        self.value = value;
        self.timestamp.increment(&self.node_id);
        self.logical_time += 1;
    }

    /// Merge with another register (conflict resolution)
    pub fn merge(&mut self, other: &LwwRegister<T>) {
        match self.timestamp.compare(&other.timestamp) {
            ClockOrdering::Before => {
                self.value = other.value.clone();
                self.timestamp = other.timestamp.clone();
                self.logical_time = other.logical_time;
            }
            ClockOrdering::Concurrent => {
                // For concurrent updates, use logical time as tie-breaker
                // If logical times are equal, use node_id
                let should_adopt_other = other.logical_time > self.logical_time
                    || (other.logical_time == self.logical_time && self.node_id < other.node_id);

                if should_adopt_other {
                    self.value = other.value.clone();
                    self.timestamp = other.timestamp.clone();
                    self.logical_time = other.logical_time;
                }
            }
            ClockOrdering::After | ClockOrdering::Equal => {
                // Keep current value
            }
        }
    }
}

/// Grow-only Set CRDT
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GSet<T: Eq + std::hash::Hash> {
    elements: HashSet<T>,
}

impl<T: Eq + std::hash::Hash> GSet<T> {
    /// Create new G-Set
    pub fn new() -> Self {
        Self {
            elements: HashSet::new(),
        }
    }

    /// Add element to set
    pub fn insert(&mut self, element: T) {
        self.elements.insert(element);
    }

    /// Check if set contains element
    pub fn contains(&self, element: &T) -> bool {
        self.elements.contains(element)
    }

    /// Get set size
    pub fn len(&self) -> usize {
        self.elements.len()
    }

    /// Check if set is empty
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// Merge with another G-Set
    pub fn merge(&mut self, other: &GSet<T>)
    where
        T: Clone,
    {
        for element in &other.elements {
            self.elements.insert(element.clone());
        }
    }
}

impl<T: Eq + std::hash::Hash> Default for GSet<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Two-Phase Set CRDT (supports both add and remove)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwoPhaseSet<T: Eq + std::hash::Hash + Clone> {
    added: HashSet<T>,
    removed: HashSet<T>,
}

impl<T: Eq + std::hash::Hash + Clone> TwoPhaseSet<T> {
    /// Create new Two-Phase Set
    pub fn new() -> Self {
        Self {
            added: HashSet::new(),
            removed: HashSet::new(),
        }
    }

    /// Add element to set
    pub fn insert(&mut self, element: T) {
        if !self.removed.contains(&element) {
            self.added.insert(element);
        }
    }

    /// Remove element from set
    pub fn remove(&mut self, element: &T) -> bool {
        if self.added.contains(element) {
            self.removed.insert(element.clone());
            true
        } else {
            false
        }
    }

    /// Check if set contains element
    pub fn contains(&self, element: &T) -> bool {
        self.added.contains(element) && !self.removed.contains(element)
    }

    /// Get visible elements
    pub fn elements(&self) -> impl Iterator<Item = &T> {
        self.added.iter().filter(|e| !self.removed.contains(e))
    }

    /// Get set size
    pub fn len(&self) -> usize {
        self.elements().count()
    }

    /// Check if set is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Merge with another Two-Phase Set
    pub fn merge(&mut self, other: &TwoPhaseSet<T>) {
        for element in &other.added {
            self.added.insert(element.clone());
        }
        for element in &other.removed {
            self.removed.insert(element.clone());
        }
    }
}

impl<T: Eq + std::hash::Hash + Clone> Default for TwoPhaseSet<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// CRDT Map combining multiple CRDTs
pub type CrdtSet<T> = TwoPhaseSet<T>;

/// CRDT Map for key-value storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrdtMap<K, V>
where
    K: Eq + std::hash::Hash + Clone,
    V: Clone,
{
    entries: HashMap<K, LwwRegister<V>>,
    node_id: String,
}

impl<K, V> CrdtMap<K, V>
where
    K: Eq + std::hash::Hash + Clone,
    V: Clone,
{
    /// Create new CRDT map
    pub fn new(node_id: String) -> Self {
        Self {
            entries: HashMap::new(),
            node_id,
        }
    }

    /// Insert or update key-value pair
    pub fn insert(&mut self, key: K, value: V) {
        if let Some(register) = self.entries.get_mut(&key) {
            register.update(value);
        } else {
            let register = LwwRegister::new(value, self.node_id.clone());
            self.entries.insert(key, register);
        }
    }

    /// Get value for key
    pub fn get(&self, key: &K) -> Option<&V> {
        self.entries.get(key).map(|r| r.value())
    }

    /// Check if map contains key
    pub fn contains_key(&self, key: &K) -> bool {
        self.entries.contains_key(key)
    }

    /// Get map size
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if map is empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate over key-value pairs
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.entries.iter().map(|(k, v)| (k, v.value()))
    }

    /// Merge with another CRDT map
    pub fn merge(&mut self, other: &CrdtMap<K, V>) {
        for (key, other_register) in &other.entries {
            if let Some(register) = self.entries.get_mut(key) {
                register.merge(other_register);
            } else {
                self.entries.insert(key.clone(), other_register.clone());
            }
        }
    }
}

/// Conflict resolver for edge nodes
pub struct ConflictResolver {
    node_id: String,
}

impl ConflictResolver {
    /// Create new conflict resolver
    pub fn new(node_id: String) -> Self {
        Self { node_id }
    }

    /// Create CRDT map
    pub fn create_map<K, V>(&self) -> CrdtMap<K, V>
    where
        K: Eq + std::hash::Hash + Clone,
        V: Clone,
    {
        CrdtMap::new(self.node_id.clone())
    }

    /// Create CRDT set
    pub fn create_set<T: Eq + std::hash::Hash + Clone>(&self) -> CrdtSet<T> {
        CrdtSet::new()
    }

    /// Resolve conflict between two values using Last-Write-Wins
    pub fn resolve_lww<T: Clone>(
        &self,
        local: &T,
        local_clock: &VectorClock,
        remote: &T,
        remote_clock: &VectorClock,
    ) -> T {
        match local_clock.compare(remote_clock) {
            ClockOrdering::Before => remote.clone(),
            ClockOrdering::After | ClockOrdering::Equal => local.clone(),
            ClockOrdering::Concurrent => {
                // Tie-break deterministically
                if self.node_id.as_str() < "remote" {
                    remote.clone()
                } else {
                    local.clone()
                }
            }
        }
    }

    /// Get node ID
    pub fn node_id(&self) -> &str {
        &self.node_id
    }
}

impl fmt::Display for VectorClock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{{")?;
        for (i, (node, count)) in self.clock.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}: {}", node, count)?;
        }
        write!(f, "}}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vector_clock_increment() {
        let mut clock = VectorClock::new();
        clock.increment("node1");
        clock.increment("node1");
        clock.increment("node2");

        assert_eq!(clock.get("node1"), 2);
        assert_eq!(clock.get("node2"), 1);
        assert_eq!(clock.get("node3"), 0);
    }

    #[test]
    fn test_vector_clock_merge() {
        let mut clock1 = VectorClock::new();
        clock1.increment("node1");
        clock1.increment("node1");

        let mut clock2 = VectorClock::new();
        clock2.increment("node2");

        clock1.merge(&clock2);
        assert_eq!(clock1.get("node1"), 2);
        assert_eq!(clock1.get("node2"), 1);
    }

    #[test]
    fn test_vector_clock_compare() {
        let mut clock1 = VectorClock::new();
        clock1.increment("node1");

        let mut clock2 = VectorClock::new();
        clock2.increment("node1");
        clock2.increment("node1");

        assert_eq!(clock1.compare(&clock2), ClockOrdering::Before);
        assert_eq!(clock2.compare(&clock1), ClockOrdering::After);

        let mut clock3 = VectorClock::new();
        clock3.increment("node2");

        assert_eq!(clock1.compare(&clock3), ClockOrdering::Concurrent);
    }

    #[test]
    fn test_lww_register() {
        let mut reg1 = LwwRegister::new(42, "node1".to_string());
        let mut reg2 = LwwRegister::new(100, "node2".to_string());

        reg1.update(50);
        reg2.merge(&reg1);

        assert_eq!(*reg2.value(), 50);
    }

    #[test]
    fn test_gset() {
        let mut set1 = GSet::new();
        set1.insert(1);
        set1.insert(2);

        let mut set2 = GSet::new();
        set2.insert(2);
        set2.insert(3);

        set1.merge(&set2);

        assert_eq!(set1.len(), 3);
        assert!(set1.contains(&1));
        assert!(set1.contains(&2));
        assert!(set1.contains(&3));
    }

    #[test]
    fn test_two_phase_set() {
        let mut set = TwoPhaseSet::new();
        set.insert(1);
        set.insert(2);
        set.insert(3);

        assert_eq!(set.len(), 3);
        assert!(set.contains(&2));

        set.remove(&2);
        assert_eq!(set.len(), 2);
        assert!(!set.contains(&2));

        // Once removed, cannot be added again
        set.insert(2);
        assert!(!set.contains(&2));
    }

    #[test]
    fn test_two_phase_set_merge() {
        let mut set1 = TwoPhaseSet::new();
        set1.insert(1);
        set1.insert(2);

        let mut set2 = TwoPhaseSet::new();
        set2.insert(2);
        set2.insert(3);
        set2.remove(&2);

        set1.merge(&set2);

        assert!(set1.contains(&1));
        assert!(!set1.contains(&2)); // Removed in set2
        assert!(set1.contains(&3));
    }

    #[test]
    fn test_crdt_map() {
        let mut map1 = CrdtMap::new("node1".to_string());
        map1.insert("key1", 100);
        map1.insert("key2", 200);

        let mut map2 = CrdtMap::new("node2".to_string());
        map2.insert("key2", 250);
        map2.insert("key3", 300);

        map1.merge(&map2);

        assert_eq!(map1.get(&"key1"), Some(&100));
        assert_eq!(map1.get(&"key3"), Some(&300));
        // key2 will be resolved based on vector clocks
    }

    #[test]
    fn test_conflict_resolver() {
        let resolver = ConflictResolver::new("node1".to_string());
        assert_eq!(resolver.node_id(), "node1");

        let map: CrdtMap<String, i32> = resolver.create_map();
        assert!(map.is_empty());

        let set: CrdtSet<i32> = resolver.create_set();
        assert!(set.is_empty());
    }
}
