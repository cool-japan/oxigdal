//! Merge strategies for conflict resolution

use crate::error::{Error, Result};
use crate::types::{Conflict, ConflictType, Record};
use bytes::Bytes;

/// Merge strategy for resolving conflicts
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MergeStrategy {
    /// Always take the local version
    LocalWins,
    /// Always take the remote version
    RemoteWins,
    /// Take the version with the latest timestamp
    #[default]
    LastWriteWins,
    /// Three-way merge using a common ancestor for `UpdateUpdate` conflicts.
    ///
    /// `InsertInsert`, `DeleteDelete`, `DeleteUpdate`, and `UpdateDelete` conflicts have no
    /// meaningful common ancestor by construction and are always resolved automatically.
    /// `UpdateUpdate` conflicts, however, need a real common ancestor (see
    /// [`crate::conflict::ConflictDetector::with_ancestor_store`]) to perform an honest
    /// three-way merge. If none is available, [`MergeEngine::resolve`] returns a
    /// [`crate::error::Error::Merge`] error rather than silently degrading to
    /// `LastWriteWins` — unless an explicit fallback strategy was configured via
    /// [`MergeEngine::with_ancestor_fallback`].
    ThreeWayMerge,
    /// Take the larger version (by size)
    LargerWins,
    /// Manual resolution required
    Manual,
    /// Custom strategy (requires callback)
    Custom,
}

/// Outcome of resolving a conflict, including whether an explicit fallback strategy was
/// used in place of the configured one.
#[derive(Debug, Clone)]
pub struct MergeOutcome {
    /// The resolved record.
    pub record: Record,
    /// `true` if resolution fell back to a different strategy than the one configured on
    /// the [`MergeEngine`] — currently only possible for `ThreeWayMerge` on `UpdateUpdate`
    /// conflicts with no common ancestor, via [`MergeEngine::with_ancestor_fallback`].
    pub used_fallback: bool,
    /// The strategy actually applied to produce `record`.
    pub applied_strategy: MergeStrategy,
}

impl MergeOutcome {
    fn direct(record: Record, strategy: MergeStrategy) -> Self {
        Self {
            record,
            used_fallback: false,
            applied_strategy: strategy,
        }
    }
}

/// Merge engine for resolving conflicts
pub struct MergeEngine {
    strategy: MergeStrategy,
    custom_merger: Option<Box<dyn CustomMerger>>,
    /// Explicit strategy to fall back to when `ThreeWayMerge` hits an `UpdateUpdate`
    /// conflict with no common ancestor. `None` (the default) means: no ancestor means an
    /// explicit error, never a silent fallback.
    ancestor_fallback: Option<MergeStrategy>,
}

impl MergeEngine {
    /// Create a new merge engine with the given strategy
    pub fn new(strategy: MergeStrategy) -> Self {
        Self {
            strategy,
            custom_merger: None,
            ancestor_fallback: None,
        }
    }

    /// Set a custom merger
    pub fn with_custom_merger(mut self, merger: Box<dyn CustomMerger>) -> Self {
        self.custom_merger = Some(merger);
        self
    }

    /// Opt into an explicit fallback strategy for `ThreeWayMerge` `UpdateUpdate` conflicts
    /// that have no common ancestor available. Without this, [`Self::resolve`] returns an
    /// error for that case instead of silently guessing.
    ///
    /// `fallback` must not itself be [`MergeStrategy::ThreeWayMerge`]; passing it will cause
    /// [`Self::resolve`] to return an error at resolution time rather than panicking here.
    pub fn with_ancestor_fallback(mut self, fallback: MergeStrategy) -> Self {
        self.ancestor_fallback = Some(fallback);
        self
    }

    /// Resolve a conflict using the configured strategy
    pub fn resolve(&self, conflict: &Conflict) -> Result<Record> {
        self.resolve_detailed(conflict)
            .map(|outcome| outcome.record)
    }

    /// Resolve a conflict using the configured strategy, returning the full
    /// [`MergeOutcome`] (including whether a fallback strategy was used).
    pub fn resolve_detailed(&self, conflict: &Conflict) -> Result<MergeOutcome> {
        match self.strategy {
            MergeStrategy::ThreeWayMerge => self.three_way_merge(conflict),
            other => {
                let record = self.apply_simple_strategy(other, conflict)?;
                Ok(MergeOutcome::direct(record, other))
            }
        }
    }

    /// Apply a non-recursive strategy (i.e. anything except `ThreeWayMerge`). Used both by
    /// `resolve_detailed` for the top-level strategy and by `three_way_merge` when applying
    /// an explicit ancestor fallback.
    fn apply_simple_strategy(
        &self,
        strategy: MergeStrategy,
        conflict: &Conflict,
    ) -> Result<Record> {
        match strategy {
            MergeStrategy::LocalWins => self.local_wins(conflict),
            MergeStrategy::RemoteWins => self.remote_wins(conflict),
            MergeStrategy::LastWriteWins => self.last_write_wins(conflict),
            MergeStrategy::LargerWins => self.larger_wins(conflict),
            MergeStrategy::Manual => Err(Error::merge("Manual resolution required")),
            MergeStrategy::Custom => self.custom_merge(conflict),
            MergeStrategy::ThreeWayMerge => Err(Error::merge(
                "ThreeWayMerge cannot be used as its own ancestor-fallback strategy",
            )),
        }
    }

    /// Local wins strategy
    fn local_wins(&self, conflict: &Conflict) -> Result<Record> {
        Ok(conflict.local.clone())
    }

    /// Remote wins strategy
    fn remote_wins(&self, conflict: &Conflict) -> Result<Record> {
        Ok(conflict.remote.clone())
    }

    /// Last write wins strategy
    fn last_write_wins(&self, conflict: &Conflict) -> Result<Record> {
        if conflict.local.updated_at >= conflict.remote.updated_at {
            Ok(conflict.local.clone())
        } else {
            Ok(conflict.remote.clone())
        }
    }

    /// Three-way merge strategy.
    ///
    /// Only `UpdateUpdate` conflicts actually require a common ancestor (that's the classic
    /// three-way / diff3 case: both sides changed the same base, so we need the base to tell
    /// what each side actually changed). The other conflict types are resolved without one:
    /// `DeleteDelete`/`DeleteUpdate`/`UpdateDelete` just need to know which side deleted, and
    /// `InsertInsert` never has a common ancestor by construction (two independent inserts of
    /// the same key), so falling back to last-write-wins there is the intended behavior, not
    /// a degraded one.
    fn three_way_merge(&self, conflict: &Conflict) -> Result<MergeOutcome> {
        match conflict.conflict_type {
            ConflictType::DeleteDelete => {
                // Both deleted - take local
                Ok(MergeOutcome::direct(
                    conflict.local.clone(),
                    MergeStrategy::ThreeWayMerge,
                ))
            }
            ConflictType::DeleteUpdate => {
                // Local deleted, remote updated - keep deletion
                Ok(MergeOutcome::direct(
                    conflict.local.clone(),
                    MergeStrategy::ThreeWayMerge,
                ))
            }
            ConflictType::UpdateDelete => {
                // Local updated, remote deleted - keep deletion
                Ok(MergeOutcome::direct(
                    conflict.remote.clone(),
                    MergeStrategy::ThreeWayMerge,
                ))
            }
            ConflictType::InsertInsert => {
                // Both inserted with no shared ancestor possible - use last write wins.
                let record = self.last_write_wins(conflict)?;
                Ok(MergeOutcome::direct(record, MergeStrategy::ThreeWayMerge))
            }
            ConflictType::UpdateUpdate => match &conflict.base {
                Some(base) => {
                    let merged_data =
                        self.merge_data(&base.data, &conflict.local.data, &conflict.remote.data)?;

                    let mut result = conflict.local.clone();
                    result.data = merged_data;
                    result.version = conflict.local.version.next();
                    result.updated_at = chrono::Utc::now();

                    Ok(MergeOutcome::direct(result, MergeStrategy::ThreeWayMerge))
                }
                None => match self.ancestor_fallback {
                    Some(fallback) => {
                        let record = self.apply_simple_strategy(fallback, conflict)?;
                        Ok(MergeOutcome {
                            record,
                            used_fallback: true,
                            applied_strategy: fallback,
                        })
                    }
                    None => Err(Error::merge(
                        "ThreeWayMerge requires a common ancestor for UpdateUpdate conflicts, \
                         but none is available. Attach a real AncestorStore via \
                         ConflictDetector::with_ancestor_store so a base record can be found, \
                         or opt into an explicit fallback via \
                         MergeEngine::with_ancestor_fallback(strategy)",
                    )),
                },
            },
        }
    }

    /// Larger wins strategy
    fn larger_wins(&self, conflict: &Conflict) -> Result<Record> {
        if conflict.local.data.len() >= conflict.remote.data.len() {
            Ok(conflict.local.clone())
        } else {
            Ok(conflict.remote.clone())
        }
    }

    /// Custom merge strategy
    fn custom_merge(&self, conflict: &Conflict) -> Result<Record> {
        match &self.custom_merger {
            Some(merger) => merger.merge(conflict),
            None => Err(Error::merge("No custom merger configured")),
        }
    }

    /// Merge data using three-way merge algorithm
    fn merge_data(&self, base: &Bytes, local: &Bytes, remote: &Bytes) -> Result<Bytes> {
        // Simple three-way merge implementation
        // In production, this would be more sophisticated

        // If one side is unchanged, use the other
        if base == local {
            return Ok(remote.clone());
        }
        if base == remote {
            return Ok(local.clone());
        }

        // If both changed to the same thing, no conflict
        if local == remote {
            return Ok(local.clone());
        }

        // Try line-based merge for text data
        match self.try_line_merge(base, local, remote) {
            Ok(merged) => Ok(merged),
            Err(_) => {
                // Fall back to choosing the larger version
                if local.len() >= remote.len() {
                    Ok(local.clone())
                } else {
                    Ok(remote.clone())
                }
            }
        }
    }

    /// Try to perform line-based merge
    fn try_line_merge(&self, base: &Bytes, local: &Bytes, remote: &Bytes) -> Result<Bytes> {
        // Convert to strings (if possible)
        let base_str = std::str::from_utf8(base).map_err(|_| Error::merge("Not text data"))?;
        let local_str = std::str::from_utf8(local).map_err(|_| Error::merge("Not text data"))?;
        let remote_str = std::str::from_utf8(remote).map_err(|_| Error::merge("Not text data"))?;

        // Split into lines
        let base_lines: Vec<_> = base_str.lines().collect();
        let local_lines: Vec<_> = local_str.lines().collect();
        let remote_lines: Vec<_> = remote_str.lines().collect();

        // Simple merge: if changes don't overlap, combine them
        let mut result = Vec::new();

        // This is a very simplified merge
        // A real implementation would use diff3 or similar algorithm
        if base_lines == local_lines {
            // Local unchanged, use remote
            result.extend(remote_lines);
        } else if base_lines == remote_lines {
            // Remote unchanged, use local
            result.extend(local_lines);
        } else {
            // Both changed - potential conflict
            // For now, just concatenate with a marker
            result.push("<<<<<<< LOCAL");
            result.extend(local_lines);
            result.push("=======");
            result.extend(remote_lines);
            result.push(">>>>>>> REMOTE");
        }

        Ok(Bytes::from(result.join("\n")))
    }

    /// Check if a conflict can be automatically resolved
    pub fn can_auto_resolve(&self, conflict: &Conflict) -> bool {
        match self.strategy {
            MergeStrategy::Manual => false,
            MergeStrategy::Custom => self
                .custom_merger
                .as_ref()
                .map(|m| m.can_resolve(conflict))
                .unwrap_or(false),
            MergeStrategy::ThreeWayMerge => match conflict.conflict_type {
                // Only UpdateUpdate genuinely needs a base (or a configured fallback);
                // every other conflict type is resolved without one.
                ConflictType::UpdateUpdate => {
                    conflict.base.is_some() || self.ancestor_fallback.is_some()
                }
                ConflictType::DeleteDelete
                | ConflictType::DeleteUpdate
                | ConflictType::UpdateDelete
                | ConflictType::InsertInsert => true,
            },
            _ => true, // Other strategies always auto-resolve
        }
    }
}

/// Trait for custom merge implementations
pub trait CustomMerger: Send + Sync {
    /// Merge two conflicting records
    fn merge(&self, conflict: &Conflict) -> Result<Record>;

    /// Check if this merger can resolve the given conflict
    fn can_resolve(&self, conflict: &Conflict) -> bool;
}

/// Example custom merger that uses a callback
pub struct CallbackMerger<F>
where
    F: Fn(&Conflict) -> Result<Record> + Send + Sync,
{
    callback: F,
}

impl<F> CallbackMerger<F>
where
    F: Fn(&Conflict) -> Result<Record> + Send + Sync,
{
    /// Create a new callback merger
    pub fn new(callback: F) -> Self {
        Self { callback }
    }
}

impl<F> CustomMerger for CallbackMerger<F>
where
    F: Fn(&Conflict) -> Result<Record> + Send + Sync,
{
    fn merge(&self, conflict: &Conflict) -> Result<Record> {
        (self.callback)(conflict)
    }

    fn can_resolve(&self, _conflict: &Conflict) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Version;

    fn create_record(key: &str, data: &str, version: u64) -> Record {
        let mut record = Record::new(key.to_string(), Bytes::from(data.to_string()));
        record.version = Version::from_u64(version);
        record
    }

    #[test]
    fn test_local_wins() {
        let local = create_record("test", "local data", 1);
        let mut remote = create_record("test", "remote data", 2);
        remote.id = local.id;

        let conflict = Conflict::new(local.clone(), remote, None);
        let engine = MergeEngine::new(MergeStrategy::LocalWins);
        let result = engine.resolve(&conflict).expect("failed to resolve");

        assert_eq!(result.data, Bytes::from("local data"));
    }

    #[test]
    fn test_remote_wins() {
        let local = create_record("test", "local data", 1);
        let mut remote = create_record("test", "remote data", 2);
        remote.id = local.id;

        let conflict = Conflict::new(local, remote.clone(), None);
        let engine = MergeEngine::new(MergeStrategy::RemoteWins);
        let result = engine.resolve(&conflict).expect("failed to resolve");

        assert_eq!(result.data, Bytes::from("remote data"));
    }

    #[test]
    fn test_last_write_wins() {
        let mut local = create_record("test", "local data", 1);
        let mut remote = create_record("test", "remote data", 2);
        remote.id = local.id;

        // Set remote timestamp to be later
        remote.updated_at = chrono::Utc::now();
        local.updated_at = chrono::Utc::now() - chrono::Duration::minutes(5);

        let conflict = Conflict::new(local, remote.clone(), None);
        let engine = MergeEngine::new(MergeStrategy::LastWriteWins);
        let result = engine.resolve(&conflict).expect("failed to resolve");

        assert_eq!(result.data, Bytes::from("remote data"));
    }

    #[test]
    fn test_larger_wins() {
        let local = create_record("test", "short", 1);
        let mut remote = create_record("test", "much longer data", 2);
        remote.id = local.id;

        let conflict = Conflict::new(local, remote.clone(), None);
        let engine = MergeEngine::new(MergeStrategy::LargerWins);
        let result = engine.resolve(&conflict).expect("failed to resolve");

        assert_eq!(result.data, Bytes::from("much longer data"));
    }

    #[test]
    fn test_custom_merger() {
        let local = create_record("test", "local", 1);
        let mut remote = create_record("test", "remote", 2);
        remote.id = local.id;

        let conflict = Conflict::new(local.clone(), remote, None);

        let callback = |_conflict: &Conflict| -> Result<Record> {
            Ok(create_record("test", "custom merged", 3))
        };

        let merger = CallbackMerger::new(callback);
        let engine = MergeEngine::new(MergeStrategy::Custom).with_custom_merger(Box::new(merger));

        let result = engine.resolve(&conflict).expect("failed to resolve");
        assert_eq!(result.data, Bytes::from("custom merged"));
    }

    #[test]
    fn test_manual_strategy() {
        let local = create_record("test", "local", 1);
        let mut remote = create_record("test", "remote", 2);
        remote.id = local.id;

        let conflict = Conflict::new(local, remote, None);
        let engine = MergeEngine::new(MergeStrategy::Manual);

        let result = engine.resolve(&conflict);
        assert!(result.is_err());
    }

    /// UpdateUpdate + ThreeWayMerge with NO common ancestor and no configured fallback
    /// must return an explicit error, never silently degrade to LastWriteWins.
    #[test]
    fn test_three_way_merge_update_update_without_base_errors_by_default() {
        let mut local = create_record("test", "local data", 2);
        let mut remote = create_record("test", "remote data", 2);
        let id = local.id;
        remote.id = id;
        local.updated_at = chrono::Utc::now();
        remote.updated_at = chrono::Utc::now();

        // Force an UpdateUpdate conflict type with no base.
        let conflict = Conflict::new(local, remote, None);
        assert_eq!(conflict.conflict_type, ConflictType::UpdateUpdate);

        let engine = MergeEngine::new(MergeStrategy::ThreeWayMerge);
        let result = engine.resolve(&conflict);

        let err = result.expect_err("must error, not silently fall back to LastWriteWins");
        let message = err.to_string();
        assert!(
            message.contains("common ancestor"),
            "error should explain the missing-ancestor condition: {message}"
        );
    }

    /// Once an explicit fallback is configured via `with_ancestor_fallback`, an UpdateUpdate
    /// conflict without a base resolves using that strategy, and the outcome reports that a
    /// fallback was used.
    #[test]
    fn test_three_way_merge_explicit_ancestor_fallback() {
        let mut local = create_record("test", "local data", 1);
        let mut remote = create_record("test", "remote data", 2);
        remote.id = local.id;

        // remote is strictly newer, so LastWriteWins-as-fallback should pick remote.
        remote.updated_at = chrono::Utc::now();
        local.updated_at = chrono::Utc::now() - chrono::Duration::minutes(5);

        let conflict = Conflict::new(local, remote.clone(), None);
        assert_eq!(conflict.conflict_type, ConflictType::UpdateUpdate);

        let engine = MergeEngine::new(MergeStrategy::ThreeWayMerge)
            .with_ancestor_fallback(MergeStrategy::LastWriteWins);

        let outcome = engine
            .resolve_detailed(&conflict)
            .expect("fallback should resolve the conflict");

        assert!(outcome.used_fallback);
        assert_eq!(outcome.applied_strategy, MergeStrategy::LastWriteWins);
        assert_eq!(outcome.record.data, Bytes::from("remote data"));
    }

    /// With a real common ancestor supplied, ThreeWayMerge performs an actual merge instead
    /// of using any fallback.
    #[test]
    fn test_three_way_merge_with_real_ancestor_merges() {
        let base = create_record("test", "line1\nline2", 1);
        let id = base.id;

        let mut local = base.clone();
        local.id = id;
        local.version = Version::from_u64(2);
        // Local unchanged from base.

        let mut remote = base.clone();
        remote.id = id;
        remote.version = Version::from_u64(2);
        remote.data = Bytes::from("line1\nline2\nline3");

        let conflict = Conflict::new(local, remote, Some(base));
        assert_eq!(conflict.conflict_type, ConflictType::UpdateUpdate);

        let engine = MergeEngine::new(MergeStrategy::ThreeWayMerge);
        let outcome = engine
            .resolve_detailed(&conflict)
            .expect("three-way merge with a real base should succeed");

        assert!(!outcome.used_fallback);
        assert_eq!(outcome.applied_strategy, MergeStrategy::ThreeWayMerge);
        // Local unchanged from base -> remote's changes win.
        assert_eq!(outcome.record.data, Bytes::from("line1\nline2\nline3"));
    }

    /// Conflict types that never need a base (DeleteDelete, DeleteUpdate, UpdateDelete,
    /// InsertInsert) must resolve automatically under ThreeWayMerge even with no ancestor
    /// store and no configured fallback.
    #[test]
    fn test_three_way_merge_delete_conflicts_need_no_ancestor() {
        let mut local = create_record("test", "local", 1);
        let mut remote = create_record("test", "remote", 1);
        remote.id = local.id;
        local.deleted = true;

        let conflict = Conflict::new(local, remote, None);
        assert_eq!(conflict.conflict_type, ConflictType::DeleteUpdate);

        let engine = MergeEngine::new(MergeStrategy::ThreeWayMerge);
        let result = engine.resolve(&conflict);
        assert!(result.is_ok());
        assert!(engine.can_auto_resolve(&conflict));
    }

    #[test]
    fn test_invalid_ancestor_fallback_of_three_way_merge_itself_errors() {
        let mut local = create_record("test", "local data", 2);
        let mut remote = create_record("test", "remote data", 2);
        remote.id = local.id;
        local.updated_at = chrono::Utc::now();
        remote.updated_at = chrono::Utc::now();

        let conflict = Conflict::new(local, remote, None);
        let engine = MergeEngine::new(MergeStrategy::ThreeWayMerge)
            .with_ancestor_fallback(MergeStrategy::ThreeWayMerge);

        let result = engine.resolve(&conflict);
        assert!(result.is_err());
    }
}
