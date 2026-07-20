//! Conflict detection for concurrent modifications

use crate::error::{Error, Result};
use crate::history::AncestorStore;
use crate::types::{Conflict, ConflictType, Record, Version};
use chrono::{DateTime, Utc};
use std::sync::Arc;

/// Conflict detector for identifying concurrent modifications
pub struct ConflictDetector {
    /// Enable strict version checking
    strict_version_check: bool,
    /// Enable timestamp-based detection
    use_timestamps: bool,
    /// Optional version-history store used to look up a real common ancestor for
    /// three-way merges. When absent, `find_common_ancestor` honestly reports
    /// that no ancestor is available rather than fabricating one.
    ancestor_store: Option<Arc<dyn AncestorStore>>,
}

impl Default for ConflictDetector {
    fn default() -> Self {
        Self {
            strict_version_check: true,
            use_timestamps: true,
            ancestor_store: None,
        }
    }
}

impl ConflictDetector {
    /// Create a new conflict detector
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable or disable strict version checking
    pub fn with_strict_version_check(mut self, enabled: bool) -> Self {
        self.strict_version_check = enabled;
        self
    }

    /// Enable or disable timestamp-based detection
    pub fn with_timestamps(mut self, enabled: bool) -> Self {
        self.use_timestamps = enabled;
        self
    }

    /// Attach a version-history store so `find_common_ancestor` can return a real
    /// common ancestor instead of `None`. Callers are responsible for feeding the store via
    /// [`AncestorStore::record_version`] as record versions become known (e.g. after every
    /// successful local write or remote pull); without that, lookups will still legitimately
    /// come back empty even with a store attached.
    pub fn with_ancestor_store(mut self, store: Arc<dyn AncestorStore>) -> Self {
        self.ancestor_store = Some(store);
        self
    }

    /// Whether an ancestor store is currently attached.
    pub fn has_ancestor_store(&self) -> bool {
        self.ancestor_store.is_some()
    }

    /// Detect conflicts between local and remote records
    pub fn detect(&self, local: &Record, remote: &Record) -> Result<Option<Conflict>> {
        // Same record ID check
        if local.id != remote.id {
            return Err(Error::invalid_operation(
                "Cannot compare records with different IDs",
            ));
        }

        // Check if there's a conflict
        if self.has_conflict(local, remote) {
            // Try to find common ancestor (for now, we don't have it)
            let base = self.find_common_ancestor(local, remote)?;
            Ok(Some(Conflict::new(local.clone(), remote.clone(), base)))
        } else {
            Ok(None)
        }
    }

    /// Check if two records have a conflict
    fn has_conflict(&self, local: &Record, remote: &Record) -> bool {
        // If both are deleted, no conflict (unless timestamps differ significantly)
        if local.deleted && remote.deleted {
            return self.use_timestamps && self.significant_timestamp_difference(local, remote);
        }

        // If one is deleted and the other is not, there's a conflict
        if local.deleted != remote.deleted {
            return true;
        }

        // Version-based conflict detection
        if self.strict_version_check {
            // If versions don't match, there's a conflict
            if local.version != remote.version {
                return true;
            }
        }

        // Timestamp-based conflict detection
        if self.use_timestamps {
            // If both were modified around the same time with different data
            if self.concurrent_modifications(local, remote) && local.data != remote.data {
                return true;
            }
        }

        // Content-based detection
        // If data is different but timestamps are recent, it's a conflict
        if local.data != remote.data {
            if self.use_timestamps {
                return self.concurrent_modifications(local, remote);
            }
            return true;
        }

        false
    }

    /// Check if modifications were concurrent (within a time window)
    fn concurrent_modifications(&self, local: &Record, remote: &Record) -> bool {
        let time_diff = if local.updated_at > remote.updated_at {
            local.updated_at - remote.updated_at
        } else {
            remote.updated_at - local.updated_at
        };

        // Consider concurrent if within 1 minute
        time_diff.num_seconds() < 60
    }

    /// Check if there's a significant timestamp difference
    fn significant_timestamp_difference(&self, local: &Record, remote: &Record) -> bool {
        let time_diff = if local.updated_at > remote.updated_at {
            local.updated_at - remote.updated_at
        } else {
            remote.updated_at - local.updated_at
        };

        // Significant if more than 5 minutes apart
        time_diff.num_seconds() > 300
    }

    /// Find the common ancestor of `local` and `remote`, if one is known.
    ///
    /// This performs a real lookup against the attached [`AncestorStore`] (see
    /// [`Self::with_ancestor_store`]): it asks for the most recent recorded version of this
    /// record *strictly before* `min(local.version, remote.version)`. Version numbers here
    /// are a monotonic per-record counter (see [`Version::next`]/[`Version::increment`]), so
    /// the lower of the two current versions cannot itself be the shared ancestor — it's one
    /// of the two diverging sides. The last point they could possibly still agree on is
    /// therefore strictly before it.
    ///
    /// If either side is still at version 0 (never updated past its initial insert), there is
    /// no earlier version to look up at all. If no store is attached, or the store has no
    /// recorded version for this record below that point, this honestly returns `Ok(None)` —
    /// it never fabricates a base record. Callers (see [`crate::merge::MergeEngine`]) must
    /// treat `None` as "no ancestor available", not as permission to silently guess.
    fn find_common_ancestor(&self, local: &Record, remote: &Record) -> Result<Option<Record>> {
        let Some(store) = &self.ancestor_store else {
            return Ok(None);
        };

        let min_version = local.version.value().min(remote.version.value());
        let Some(ancestor_bound) = min_version.checked_sub(1) else {
            return Ok(None);
        };

        Ok(store.ancestor_at_or_before(&local.id, Version::from_u64(ancestor_bound)))
    }

    /// Detect conflict type
    pub fn detect_type(&self, conflict: &Conflict) -> ConflictType {
        if conflict.local.deleted && conflict.remote.deleted {
            ConflictType::DeleteDelete
        } else if conflict.local.deleted {
            ConflictType::DeleteUpdate
        } else if conflict.remote.deleted {
            ConflictType::UpdateDelete
        } else if conflict.base.is_some()
            || (conflict.local.version.value() > 0 && conflict.remote.version.value() > 0)
        {
            // If there's a base record or both versions > 0, it's an update conflict
            ConflictType::UpdateUpdate
        } else {
            ConflictType::InsertInsert
        }
    }

    /// Check if conflict can be auto-resolved
    pub fn can_auto_resolve(&self, conflict: &Conflict) -> bool {
        match self.detect_type(conflict) {
            // Can auto-resolve if both sides deleted
            ConflictType::DeleteDelete => true,
            // Can auto-resolve with Last-Write-Wins strategy
            ConflictType::UpdateUpdate => true,
            // Cannot auto-resolve insert conflicts (need user decision)
            ConflictType::InsertInsert => false,
            // Delete-Update conflicts need careful handling
            ConflictType::DeleteUpdate | ConflictType::UpdateDelete => false,
        }
    }

    /// Get conflict resolution difficulty (0.0 = easy, 1.0 = hard)
    pub fn resolution_difficulty(&self, conflict: &Conflict) -> f64 {
        let conflict_type = self.detect_type(conflict);

        match conflict_type {
            ConflictType::DeleteDelete => 0.0, // Trivial
            ConflictType::UpdateUpdate => {
                // Difficulty based on data similarity
                let similarity = self.compute_similarity(&conflict.local, &conflict.remote);
                1.0 - similarity
            }
            ConflictType::InsertInsert => 0.5, // Medium difficulty
            ConflictType::DeleteUpdate | ConflictType::UpdateDelete => 0.8, // Hard
        }
    }

    /// Compute similarity between two records (0.0 = different, 1.0 = identical)
    fn compute_similarity(&self, local: &Record, remote: &Record) -> f64 {
        // Simple byte-level comparison
        let local_bytes = local.data.as_ref();
        let remote_bytes = remote.data.as_ref();

        if local_bytes == remote_bytes {
            return 1.0;
        }

        // Compute Jaccard similarity at byte level
        let local_set: std::collections::HashSet<_> = local_bytes.iter().collect();
        let remote_set: std::collections::HashSet<_> = remote_bytes.iter().collect();

        let intersection = local_set.intersection(&remote_set).count();
        let union = local_set.union(&remote_set).count();

        if union == 0 {
            return 0.0;
        }

        intersection as f64 / union as f64
    }

    /// Create conflict report
    pub fn create_report(&self, conflict: &Conflict) -> ConflictReport {
        let conflict_type = self.detect_type(conflict);
        let can_auto_resolve = self.can_auto_resolve(conflict);
        let difficulty = self.resolution_difficulty(conflict);

        ConflictReport {
            conflict_type,
            can_auto_resolve,
            difficulty,
            local_version: conflict.local.version,
            remote_version: conflict.remote.version,
            local_timestamp: conflict.local.updated_at,
            remote_timestamp: conflict.remote.updated_at,
            data_size_local: conflict.local.data.len(),
            data_size_remote: conflict.remote.data.len(),
            has_common_ancestor: conflict.base.is_some(),
        }
    }
}

/// Conflict report
#[derive(Debug, Clone)]
pub struct ConflictReport {
    /// Type of conflict
    pub conflict_type: ConflictType,
    /// Can be auto-resolved
    pub can_auto_resolve: bool,
    /// Resolution difficulty (0.0 to 1.0)
    pub difficulty: f64,
    /// Local version
    pub local_version: Version,
    /// Remote version
    pub remote_version: Version,
    /// Local timestamp
    pub local_timestamp: DateTime<Utc>,
    /// Remote timestamp
    pub remote_timestamp: DateTime<Utc>,
    /// Data size (local)
    pub data_size_local: usize,
    /// Data size (remote)
    pub data_size_remote: usize,
    /// Has common ancestor
    pub has_common_ancestor: bool,
}

impl ConflictReport {
    /// Get a human-readable description
    pub fn description(&self) -> String {
        format!(
            "Conflict type: {}, Auto-resolve: {}, Difficulty: {:.2}",
            self.conflict_type, self.can_auto_resolve, self.difficulty
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    fn create_record(key: &str, data: &str, version: u64) -> Record {
        let mut record = Record::new(key.to_string(), Bytes::from(data.to_string()));
        record.version = Version::from_u64(version);
        record
    }

    #[test]
    fn test_no_conflict_same_data() {
        let detector = ConflictDetector::new();
        let local = create_record("test", "data", 1);
        let mut remote = local.clone();
        remote.id = local.id; // Ensure same ID

        let conflict = detector.detect(&local, &remote);
        assert!(conflict.is_ok());
        assert!(conflict.expect("failed").is_none());
    }

    #[test]
    fn test_version_conflict() {
        let detector = ConflictDetector::new().with_strict_version_check(true);

        let local = create_record("test", "data1", 1);
        let mut remote = local.clone();
        remote.id = local.id;
        remote.version = Version::from_u64(2);
        remote.data = Bytes::from("data2");

        let conflict = detector.detect(&local, &remote);
        assert!(conflict.is_ok());
        assert!(conflict.expect("failed").is_some());
    }

    #[test]
    fn test_delete_conflict() {
        let detector = ConflictDetector::new();

        let mut local = create_record("test", "data", 1);
        local.mark_deleted();

        let mut remote = create_record("test", "data", 1);
        remote.id = local.id;
        remote.update(Bytes::from("new data"));

        let conflict = detector.detect(&local, &remote);
        assert!(conflict.is_ok());
        assert!(conflict.expect("failed").is_some());
    }

    #[test]
    fn test_conflict_report() {
        let local = create_record("test", "data1", 1);
        let mut remote = local.clone();
        remote.id = local.id;
        remote.version = Version::from_u64(2);
        remote.data = Bytes::from("data2");

        let conflict = Conflict::new(local, remote, None);
        let detector = ConflictDetector::new();
        let report = detector.create_report(&conflict);

        assert_eq!(report.conflict_type, ConflictType::UpdateUpdate);
        assert!(report.difficulty > 0.0);
    }

    #[test]
    fn test_no_ancestor_store_configured_returns_none() {
        // Without an attached AncestorStore, find_common_ancestor must honestly report
        // "no ancestor available" rather than fabricating one.
        let detector = ConflictDetector::new();
        assert!(!detector.has_ancestor_store());

        let local = create_record("test", "local", 2);
        let mut remote = create_record("test", "remote", 3);
        remote.id = local.id;

        let conflict = detector
            .detect(&local, &remote)
            .expect("detect should succeed")
            .expect("should be a conflict");
        assert!(conflict.base.is_none());
    }

    #[test]
    fn test_ancestor_store_supplies_real_base() {
        use crate::history::InMemoryAncestorStore;
        use std::sync::Arc;

        let store = Arc::new(InMemoryAncestorStore::new());

        // Record the shared ancestor at version 1.
        let base = create_record("test", "base data", 1);
        let id = base.id;
        store.record_version(&base);

        let detector = ConflictDetector::new().with_ancestor_store(store);
        assert!(detector.has_ancestor_store());

        let mut local = create_record("test", "local data", 2);
        local.id = id;
        let mut remote = create_record("test", "remote data", 3);
        remote.id = id;

        let conflict = detector
            .detect(&local, &remote)
            .expect("detect should succeed")
            .expect("should be a conflict");

        let found_base = conflict.base.expect("ancestor store should supply a base");
        assert_eq!(found_base.data, Bytes::from("base data"));
        assert_eq!(found_base.version.value(), 1);
    }

    #[test]
    fn test_similarity_computation() {
        let detector = ConflictDetector::new();

        let local = create_record("test", "hello world", 1);
        let mut remote = local.clone();
        remote.id = local.id;

        // Same data = 1.0 similarity
        let similarity = detector.compute_similarity(&local, &remote);
        assert_eq!(similarity, 1.0);

        // Different data
        remote.data = Bytes::from("goodbye world");
        let similarity = detector.compute_similarity(&local, &remote);
        assert!(similarity > 0.0 && similarity < 1.0);
    }
}
