//! W-TinyLFU (Window-TinyLFU) eviction policy.
//!
//! Implements the W-TinyLFU algorithm from Einziger et al. 2017 (ACM TOS §4):
//! a small LRU window (1% of capacity) feeds into a Segmented-LRU main cache
//! (80% protected + 20% probation) gated by a TinyLFU admission filter backed
//! by a Count-Min Sketch frequency estimator.
//!
//! The TinyLFU admission filter only promotes a window candidate into the main
//! cache when its estimated frequency exceeds that of the current probation
//! victim, preventing rare items from displacing frequently-used ones.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};

use super::{CountMinSketch, EvictionPolicy, EvictionPolicyType, EvictionStats};

/// W-TinyLFU (Einziger et al. 2017 ACM TOS §4).
///
/// Architecture:
/// - **Window** (1% capacity, LRU): absorbs new insertions; overflows are
///   admitted to main cache only if TinyLFU approves them.
/// - **Probation** (≈20% of main): candidates for eviction; items promoted
///   from window land here.
/// - **Protected** (≈80% of main): frequently-accessed segment; items accessed
///   while in probation are promoted here.
///
/// Frequency estimation uses a [`CountMinSketch`] with 4-bit packed counters
/// and periodic aging to maintain recency sensitivity.
pub struct WTinyLfuEviction<K: Hash + Eq + Clone> {
    /// Total nominal capacity tracked by this policy
    capacity: usize,
    /// Window segment (LRU order, front = oldest)
    window: VecDeque<K>,
    /// Probation segment (LRU order, front = oldest / next eviction candidate)
    probation: VecDeque<K>,
    /// Protected segment (LRU order, front = oldest)
    protected: VecDeque<K>,
    /// Maximum items held in the window segment
    window_capacity: usize,
    /// Maximum items held in the protected segment
    protected_capacity: usize,
    /// Fast membership test for window segment
    in_window: HashMap<K, ()>,
    /// Fast membership test for probation segment
    in_probation: HashMap<K, ()>,
    /// Fast membership test for protected segment
    in_protected: HashMap<K, ()>,
    /// Count-Min Sketch for TinyLFU admission gating
    cms: CountMinSketch,
    /// Eviction statistics
    stats: EvictionStats,
}

impl<K: Hash + Eq + Clone + Send + Sync + 'static> WTinyLfuEviction<K> {
    /// Create a new W-TinyLFU eviction policy with the given nominal capacity.
    ///
    /// Segment sizing follows the TinyLFU paper recommendation:
    /// - Window: max(1, capacity / 100) items
    /// - Protected: max(1, main_capacity * 80 / 100) items
    /// - Probation: remainder of main_capacity (unbounded in this implementation)
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(4);
        let window_capacity = (capacity / 100).max(1);
        let main_capacity = capacity - window_capacity;
        let protected_capacity = (main_capacity * 80 / 100).max(1);

        Self {
            capacity,
            window: VecDeque::new(),
            probation: VecDeque::new(),
            protected: VecDeque::new(),
            window_capacity,
            protected_capacity,
            in_window: HashMap::new(),
            in_probation: HashMap::new(),
            in_protected: HashMap::new(),
            cms: CountMinSketch::new(capacity),
            stats: EvictionStats::default(),
        }
    }

    /// Hash a key to a 64-bit value using the standard library DefaultHasher.
    fn hash_key(k: &K) -> u64 {
        let mut hasher = DefaultHasher::new();
        k.hash(&mut hasher);
        hasher.finish()
    }

    /// Drain any window overflow into probation, gated by TinyLFU admission.
    ///
    /// Repeatedly pops the oldest window entry while `window.len() > window_capacity`.
    /// Each candidate is compared against the current front of probation:
    /// - If the candidate's estimated frequency exceeds the probation front's,
    ///   it is admitted to probation (the probation item is not evicted here —
    ///   the main-cache overflow is handled by `select_victim`).
    /// - Otherwise the candidate is silently dropped (TinyLFU filtered).
    fn drain_window_overflow(&mut self) {
        while self.window.len() > self.window_capacity {
            let candidate = match self.window.pop_front() {
                Some(k) => k,
                None => break,
            };
            self.in_window.remove(&candidate);

            let candidate_freq = self.cms.estimate(Self::hash_key(&candidate));
            let admit = if let Some(victim) = self.probation.front() {
                candidate_freq > self.cms.estimate(Self::hash_key(victim))
            } else {
                // Probation is empty → always admit
                true
            };

            if admit {
                self.in_probation.insert(candidate.clone(), ());
                self.probation.push_back(candidate);
            }
            // else: candidate is frequency-filtered and dropped
        }
    }
}

impl<K: Hash + Eq + Clone + Send + Sync + 'static> EvictionPolicy<K> for WTinyLfuEviction<K> {
    /// Record a cache hit for `key`, updating CMS and segment positions.
    ///
    /// - Window: refresh to back (most-recently-used end).
    /// - Probation: promote to protected (with potential demotion of oldest protected item).
    /// - Protected: refresh to back.
    fn on_access(&mut self, key: &K) {
        self.cms.increment(Self::hash_key(key));
        self.stats.accesses += 1;

        if self.in_window.contains_key(key) {
            // Refresh position within window (LRU: move to back = MRU end)
            self.window.retain(|k| k != key);
            self.window.push_back(key.clone());
        } else if self.in_probation.contains_key(key) {
            // Promote from probation → protected
            self.probation.retain(|k| k != key);
            self.in_probation.remove(key);

            // If protected is full, demote its oldest entry back to probation
            if self.protected.len() >= self.protected_capacity
                && let Some(demoted) = self.protected.pop_front()
            {
                self.in_protected.remove(&demoted);
                self.in_probation.insert(demoted.clone(), ());
                self.probation.push_back(demoted);
            }

            self.in_protected.insert(key.clone(), ());
            self.protected.push_back(key.clone());
        } else if self.in_protected.contains_key(key) {
            // Refresh position within protected (LRU: move to back = MRU end)
            self.protected.retain(|k| k != key);
            self.protected.push_back(key.clone());
        }
        // Key not in any segment: ghost access — CMS already updated, nothing else to do
    }

    /// Record a cache insertion for `key` (with `_size` bytes, unused for ordering).
    ///
    /// The key is placed into the window segment. If the window overflows,
    /// the TinyLFU admission filter decides whether the oldest window entry
    /// enters probation or is discarded.
    fn on_insert(&mut self, key: K, _size: usize) {
        self.cms.increment(Self::hash_key(&key));
        self.stats.items_tracked += 1;

        // New entries always go into the window
        self.in_window.insert(key.clone(), ());
        self.window.push_back(key);

        // Drain any window overflow through the TinyLFU filter
        self.drain_window_overflow();
    }

    /// Remove tracking for `key` from whichever segment holds it.
    fn on_remove(&mut self, key: &K) {
        if self.in_window.remove(key).is_some() {
            self.window.retain(|k| k != key);
            self.stats.items_tracked = self.stats.items_tracked.saturating_sub(1);
        } else if self.in_probation.remove(key).is_some() {
            self.probation.retain(|k| k != key);
            self.stats.items_tracked = self.stats.items_tracked.saturating_sub(1);
        } else if self.in_protected.remove(key).is_some() {
            self.protected.retain(|k| k != key);
            self.stats.items_tracked = self.stats.items_tracked.saturating_sub(1);
        }
    }

    /// Select a victim key for eviction.
    ///
    /// Priority order mirrors the W-TinyLFU paper:
    /// 1. Oldest probation entry (lowest recency + frequency among main candidates).
    /// 2. Oldest window entry (if probation is empty).
    /// 3. Oldest protected entry (last resort).
    fn select_victim(&mut self) -> Option<K> {
        if let Some(victim) = self.probation.pop_front() {
            self.in_probation.remove(&victim);
            self.stats.evictions += 1;
            return Some(victim);
        }

        if let Some(victim) = self.window.pop_front() {
            self.in_window.remove(&victim);
            self.stats.evictions += 1;
            return Some(victim);
        }

        if let Some(victim) = self.protected.pop_front() {
            self.in_protected.remove(&victim);
            self.stats.evictions += 1;
            return Some(victim);
        }

        None
    }

    /// Return a snapshot of current eviction statistics.
    fn stats(&self) -> EvictionStats {
        self.stats.clone()
    }

    /// Clear all segment state, reset the CMS, and zero statistics.
    fn clear(&mut self) {
        self.window.clear();
        self.probation.clear();
        self.protected.clear();
        self.in_window.clear();
        self.in_probation.clear();
        self.in_protected.clear();
        self.cms = CountMinSketch::new(self.capacity);
        self.stats = EvictionStats::default();
    }

    fn policy_type(&self) -> EvictionPolicyType {
        EvictionPolicyType::WTinyLfu
    }
}

impl<K: Hash + Eq + Clone + Send + Sync + 'static> WTinyLfuEviction<K> {
    /// Return the policy type identifier for this eviction strategy.
    pub fn policy_type(&self) -> EvictionPolicyType {
        EvictionPolicyType::WTinyLfu
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_empty() {
        let cache = WTinyLfuEviction::<u32>::new(100);
        let s = cache.stats();
        assert_eq!(s.evictions, 0);
        assert_eq!(s.items_tracked, 0);
    }

    #[test]
    fn test_insert_then_victim() {
        let mut cache = WTinyLfuEviction::<u32>::new(10);
        for i in 0..15u32 {
            cache.on_insert(i, 1);
        }
        // After inserting 15 items into a capacity-10 policy, at least some
        // should have been pushed through to probation; a victim must exist.
        let v = cache.select_victim();
        assert!(v.is_some());
    }

    #[test]
    fn test_clear_empties_all() {
        let mut cache = WTinyLfuEviction::<u64>::new(10);
        for i in 0..20u64 {
            cache.on_insert(i, 1);
        }
        cache.clear();
        assert!(cache.select_victim().is_none());
    }

    #[test]
    fn test_policy_type() {
        let cache = WTinyLfuEviction::<u64>::new(10);
        assert_eq!(cache.policy_type(), EvictionPolicyType::WTinyLfu);
    }
}
