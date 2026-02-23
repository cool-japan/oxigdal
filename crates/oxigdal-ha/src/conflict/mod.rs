//! Conflict resolution for active-active replication.

pub mod strategies;

use crate::error::HaResult;
use crate::replication::VectorClock;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Conflict type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictType {
    /// Write-write conflict.
    WriteWrite,
    /// Delete-write conflict.
    DeleteWrite,
    /// Concurrent updates.
    ConcurrentUpdate,
}

/// Conflicting value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictingValue {
    /// Node ID that created this value.
    pub node_id: Uuid,
    /// Value data.
    pub data: Vec<u8>,
    /// Timestamp.
    pub timestamp: DateTime<Utc>,
    /// Vector clock.
    pub vector_clock: VectorClock,
}

/// Conflict.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    /// Conflict ID.
    pub id: Uuid,
    /// Conflict type.
    pub conflict_type: ConflictType,
    /// Key that has conflict.
    pub key: String,
    /// Conflicting values.
    pub values: Vec<ConflictingValue>,
    /// Detected at.
    pub detected_at: DateTime<Utc>,
}

/// Conflict resolution result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolutionResult {
    /// Conflict ID.
    pub conflict_id: Uuid,
    /// Resolved value.
    pub resolved_value: Option<Vec<u8>>,
    /// Resolution strategy used.
    pub strategy: String,
    /// Timestamp.
    pub timestamp: DateTime<Utc>,
}

/// Trait for conflict resolution strategy.
#[async_trait]
pub trait ConflictResolver: Send + Sync {
    /// Resolve a conflict.
    async fn resolve(&self, conflict: &Conflict) -> HaResult<ResolutionResult>;

    /// Get strategy name.
    fn strategy_name(&self) -> &str;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conflict_creation() {
        let conflict = Conflict {
            id: Uuid::new_v4(),
            conflict_type: ConflictType::WriteWrite,
            key: "test_key".to_string(),
            values: vec![],
            detected_at: Utc::now(),
        };

        assert_eq!(conflict.conflict_type, ConflictType::WriteWrite);
        assert_eq!(conflict.key, "test_key");
    }
}
