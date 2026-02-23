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
    /// Three-way merge using common ancestor
    ThreeWayMerge,
    /// Take the larger version (by size)
    LargerWins,
    /// Manual resolution required
    Manual,
    /// Custom strategy (requires callback)
    Custom,
}

/// Merge engine for resolving conflicts
pub struct MergeEngine {
    strategy: MergeStrategy,
    custom_merger: Option<Box<dyn CustomMerger>>,
}

impl MergeEngine {
    /// Create a new merge engine with the given strategy
    pub fn new(strategy: MergeStrategy) -> Self {
        Self {
            strategy,
            custom_merger: None,
        }
    }

    /// Set a custom merger
    pub fn with_custom_merger(mut self, merger: Box<dyn CustomMerger>) -> Self {
        self.custom_merger = Some(merger);
        self
    }

    /// Resolve a conflict using the configured strategy
    pub fn resolve(&self, conflict: &Conflict) -> Result<Record> {
        match self.strategy {
            MergeStrategy::LocalWins => self.local_wins(conflict),
            MergeStrategy::RemoteWins => self.remote_wins(conflict),
            MergeStrategy::LastWriteWins => self.last_write_wins(conflict),
            MergeStrategy::ThreeWayMerge => self.three_way_merge(conflict),
            MergeStrategy::LargerWins => self.larger_wins(conflict),
            MergeStrategy::Manual => Err(Error::merge("Manual resolution required")),
            MergeStrategy::Custom => self.custom_merge(conflict),
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

    /// Three-way merge strategy
    fn three_way_merge(&self, conflict: &Conflict) -> Result<Record> {
        // If no common ancestor, fall back to last-write-wins
        let base = match &conflict.base {
            Some(b) => b,
            None => return self.last_write_wins(conflict),
        };

        // Perform three-way merge
        match conflict.conflict_type {
            ConflictType::UpdateUpdate => {
                // Try to merge data
                let merged_data =
                    self.merge_data(&base.data, &conflict.local.data, &conflict.remote.data)?;

                let mut result = conflict.local.clone();
                result.data = merged_data;
                result.version = conflict.local.version.next();
                result.updated_at = chrono::Utc::now();

                Ok(result)
            }
            ConflictType::DeleteDelete => {
                // Both deleted - take local
                Ok(conflict.local.clone())
            }
            ConflictType::DeleteUpdate => {
                // Local deleted, remote updated - keep deletion
                Ok(conflict.local.clone())
            }
            ConflictType::UpdateDelete => {
                // Local updated, remote deleted - keep deletion
                Ok(conflict.remote.clone())
            }
            ConflictType::InsertInsert => {
                // Both inserted - use last write wins
                self.last_write_wins(conflict)
            }
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
            MergeStrategy::ThreeWayMerge => {
                // Can auto-resolve if we have a base or if it's a simple conflict
                conflict.base.is_some()
                    || matches!(conflict.conflict_type, ConflictType::DeleteDelete)
            }
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
}
