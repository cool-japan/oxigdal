//! Version history tracking for common-ancestor lookup in three-way merges.
//!
//! `ConflictDetector::find_common_ancestor` needs a real common ancestor to
//! perform an honest three-way merge. Producing one requires persisted version history:
//! something that remembers prior [`Record`] snapshots for a given [`RecordId`] so that, once a
//! local and a remote copy of the same record diverge, the merge engine can look back and find
//! the version they both descended from.
//!
//! This module provides the [`AncestorStore`] trait for pluggable history persistence, plus
//! [`InMemoryAncestorStore`], a ready-to-use in-process implementation. Callers that want a
//! genuine three-way merge must record every confirmed record version via
//! [`AncestorStore::record_version`] as it becomes known (for example, right after a successful
//! local write or a remote pull), then attach the store via
//! [`crate::conflict::ConflictDetector::with_ancestor_store`].
//!
//! Without an attached store, `ConflictDetector::find_common_ancestor`
//! honestly reports "no ancestor available" (`Ok(None)`) rather than fabricating one, and
//! [`crate::merge::MergeEngine`] surfaces that absence explicitly instead of silently
//! degrading `ThreeWayMerge` into `LastWriteWins`.

use crate::types::{Record, RecordId, Version};
use dashmap::DashMap;
use std::collections::BTreeMap;

/// Default maximum number of historical versions retained per record id by
/// [`InMemoryAncestorStore`] before the oldest entries are evicted.
pub const DEFAULT_MAX_VERSIONS_PER_RECORD: usize = 64;

/// A store of historical [`Record`] versions, used to find a common ancestor between two
/// diverging copies of the same record.
///
/// Implementations must be safe to share across threads/tasks: [`crate::conflict::ConflictDetector`]
/// holds a store behind an `Arc<dyn AncestorStore>`.
pub trait AncestorStore: Send + Sync {
    /// Record a known-good version of a record (for example, right after it was written
    /// locally or received from a remote peer). Implementations should retain enough history
    /// to answer [`ancestor_at_or_before`](Self::ancestor_at_or_before) queries; they are free
    /// to prune old versions (e.g. to bound memory).
    fn record_version(&self, record: &Record);

    /// Find the most recent recorded version of `id` at or before `max_version` — the
    /// candidate common ancestor for a local/remote pair currently sitting at higher
    /// versions. Returns `None` if no version at or below `max_version` was ever recorded.
    fn ancestor_at_or_before(&self, id: &RecordId, max_version: Version) -> Option<Record>;
}

/// A simple thread-safe, in-memory [`AncestorStore`].
///
/// Keeps every recorded version for every record id in a `BTreeMap<u64, Record>` keyed by
/// version number, so ancestor lookups are `O(log n)`. Once a record id accumulates more than
/// `max_versions_per_record` entries, the oldest versions are evicted to bound memory use.
///
/// Suitable for tests, single-process deployments, or as a reference implementation;
/// long-running processes that need durable history across restarts should provide their own
/// [`AncestorStore`] backed by persistent storage instead.
pub struct InMemoryAncestorStore {
    history: DashMap<RecordId, BTreeMap<u64, Record>>,
    max_versions_per_record: usize,
}

impl InMemoryAncestorStore {
    /// Create a new, empty in-memory ancestor store with the default retention cap
    /// ([`DEFAULT_MAX_VERSIONS_PER_RECORD`] versions per record id).
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_MAX_VERSIONS_PER_RECORD)
    }

    /// Create a new, empty in-memory ancestor store that retains at most
    /// `max_versions_per_record` historical versions per record id.
    ///
    /// A value of `0` is treated as `1` (always keep at least the most recent version).
    pub fn with_capacity(max_versions_per_record: usize) -> Self {
        Self {
            history: DashMap::new(),
            max_versions_per_record: max_versions_per_record.max(1),
        }
    }

    /// Number of distinct record ids with at least one recorded version.
    pub fn tracked_ids(&self) -> usize {
        self.history.len()
    }

    /// Number of recorded versions currently retained for `id`.
    pub fn version_count(&self, id: &RecordId) -> usize {
        self.history
            .get(id)
            .map(|versions| versions.len())
            .unwrap_or(0)
    }
}

impl Default for InMemoryAncestorStore {
    fn default() -> Self {
        Self::new()
    }
}

impl AncestorStore for InMemoryAncestorStore {
    fn record_version(&self, record: &Record) {
        let mut versions = self.history.entry(record.id).or_default();
        versions.insert(record.version.value(), record.clone());

        while versions.len() > self.max_versions_per_record {
            let oldest_key = match versions.keys().next() {
                Some(key) => *key,
                None => break,
            };
            versions.remove(&oldest_key);
        }
    }

    fn ancestor_at_or_before(&self, id: &RecordId, max_version: Version) -> Option<Record> {
        let versions = self.history.get(id)?;
        versions
            .range(..=max_version.value())
            .next_back()
            .map(|(_, record)| record.clone())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use bytes::Bytes;

    fn record_with_version(key: &str, data: &str, version: u64) -> Record {
        let mut record = Record::new(key.to_string(), Bytes::from(data.to_string()));
        record.version = Version::from_u64(version);
        record
    }

    #[test]
    fn empty_store_has_no_ancestor() {
        let store = InMemoryAncestorStore::new();
        let id = RecordId::new();
        assert!(
            store
                .ancestor_at_or_before(&id, Version::from_u64(5))
                .is_none()
        );
    }

    #[test]
    fn finds_exact_version() {
        let store = InMemoryAncestorStore::new();
        let record = record_with_version("k", "v0", 0);
        let id = record.id;
        store.record_version(&record);

        let found = store
            .ancestor_at_or_before(&id, Version::from_u64(0))
            .expect("ancestor should be found");
        assert_eq!(found.data, Bytes::from("v0"));
    }

    #[test]
    fn finds_nearest_version_at_or_before() {
        let store = InMemoryAncestorStore::new();
        let mut v0 = record_with_version("k", "v0", 0);
        let id = RecordId::new();
        v0.id = id;
        store.record_version(&v0);

        let mut v2 = record_with_version("k", "v2", 2);
        v2.id = id;
        store.record_version(&v2);

        // Querying at version 5 (neither recorded) should return the closest
        // version at or below it, i.e. v2.
        let found = store
            .ancestor_at_or_before(&id, Version::from_u64(5))
            .expect("ancestor should be found");
        assert_eq!(found.data, Bytes::from("v2"));

        // Querying at version 1 should return v0, since v2 is above it.
        let found = store
            .ancestor_at_or_before(&id, Version::from_u64(1))
            .expect("ancestor should be found");
        assert_eq!(found.data, Bytes::from("v0"));
    }

    #[test]
    fn no_ancestor_below_earliest_recorded_version() {
        let store = InMemoryAncestorStore::new();
        let mut v3 = record_with_version("k", "v3", 3);
        let id = RecordId::new();
        v3.id = id;
        store.record_version(&v3);

        assert!(
            store
                .ancestor_at_or_before(&id, Version::from_u64(1))
                .is_none()
        );
    }

    #[test]
    fn evicts_oldest_version_beyond_capacity() {
        let store = InMemoryAncestorStore::with_capacity(2);
        let id = RecordId::new();

        for v in 0..5u64 {
            let mut record = record_with_version("k", "data", v);
            record.id = id;
            store.record_version(&record);
        }

        assert_eq!(store.version_count(&id), 2);
        // Only the two most recent versions (3, 4) should remain.
        assert!(
            store
                .ancestor_at_or_before(&id, Version::from_u64(2))
                .is_none()
        );
        let found = store
            .ancestor_at_or_before(&id, Version::from_u64(4))
            .expect("most recent version should remain");
        assert_eq!(found.version.value(), 4);
    }

    #[test]
    fn different_ids_do_not_interfere() {
        let store = InMemoryAncestorStore::new();
        let a = record_with_version("a", "a-data", 0);
        let b = record_with_version("b", "b-data", 0);

        store.record_version(&a);
        store.record_version(&b);

        assert_eq!(store.tracked_ids(), 2);
        let found_a = store
            .ancestor_at_or_before(&a.id, Version::from_u64(0))
            .expect("a should be found");
        assert_eq!(found_a.data, Bytes::from("a-data"));
    }
}
