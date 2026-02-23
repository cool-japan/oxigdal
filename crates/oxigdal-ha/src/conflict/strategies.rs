//! Conflict resolution strategies.

use super::{Conflict, ConflictResolver, ResolutionResult};
use crate::error::{HaError, HaResult};
use async_trait::async_trait;
use chrono::Utc;
use tracing::{debug, info};

/// Last-write-wins (LWW) conflict resolution.
pub struct LastWriteWins;

impl LastWriteWins {
    /// Create a new LWW resolver.
    pub fn new() -> Self {
        Self
    }
}

impl Default for LastWriteWins {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ConflictResolver for LastWriteWins {
    async fn resolve(&self, conflict: &Conflict) -> HaResult<ResolutionResult> {
        debug!("Resolving conflict {} using last-write-wins", conflict.id);

        let winner = conflict
            .values
            .iter()
            .max_by_key(|v| v.timestamp)
            .ok_or_else(|| HaError::ConflictResolution("No values to resolve".to_string()))?;

        info!(
            "LWW winner: node {} with timestamp {}",
            winner.node_id, winner.timestamp
        );

        Ok(ResolutionResult {
            conflict_id: conflict.id,
            resolved_value: Some(winner.data.clone()),
            strategy: self.strategy_name().to_string(),
            timestamp: Utc::now(),
        })
    }

    fn strategy_name(&self) -> &str {
        "last-write-wins"
    }
}

/// Highest priority wins conflict resolution.
pub struct HighestPriorityWins {
    /// Node priorities.
    priorities: std::collections::HashMap<uuid::Uuid, u32>,
}

impl HighestPriorityWins {
    /// Create a new highest priority wins resolver.
    pub fn new(priorities: std::collections::HashMap<uuid::Uuid, u32>) -> Self {
        Self { priorities }
    }
}

#[async_trait]
impl ConflictResolver for HighestPriorityWins {
    async fn resolve(&self, conflict: &Conflict) -> HaResult<ResolutionResult> {
        debug!(
            "Resolving conflict {} using highest-priority-wins",
            conflict.id
        );

        let winner = conflict
            .values
            .iter()
            .max_by_key(|v| self.priorities.get(&v.node_id).copied().unwrap_or(0))
            .ok_or_else(|| HaError::ConflictResolution("No values to resolve".to_string()))?;

        let priority = self.priorities.get(&winner.node_id).copied().unwrap_or(0);

        info!(
            "Highest priority winner: node {} with priority {}",
            winner.node_id, priority
        );

        Ok(ResolutionResult {
            conflict_id: conflict.id,
            resolved_value: Some(winner.data.clone()),
            strategy: self.strategy_name().to_string(),
            timestamp: Utc::now(),
        })
    }

    fn strategy_name(&self) -> &str {
        "highest-priority-wins"
    }
}

/// Vector clock based resolution.
pub struct VectorClockResolver;

impl VectorClockResolver {
    /// Create a new vector clock resolver.
    pub fn new() -> Self {
        Self
    }
}

impl Default for VectorClockResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ConflictResolver for VectorClockResolver {
    async fn resolve(&self, conflict: &Conflict) -> HaResult<ResolutionResult> {
        debug!("Resolving conflict {} using vector clock", conflict.id);

        if conflict.values.len() != 2 {
            return Err(HaError::ConflictResolution(
                "Vector clock resolution requires exactly 2 values".to_string(),
            ));
        }

        let value1 = &conflict.values[0];
        let value2 = &conflict.values[1];

        let resolved_value = if value1.vector_clock.happens_before(&value2.vector_clock) {
            info!(
                "Value from node {} happens before, selecting value2",
                value1.node_id
            );
            Some(value2.data.clone())
        } else if value2.vector_clock.happens_before(&value1.vector_clock) {
            info!(
                "Value from node {} happens before, selecting value1",
                value2.node_id
            );
            Some(value1.data.clone())
        } else {
            info!("Concurrent updates detected, falling back to LWW");
            let winner = if value1.timestamp > value2.timestamp {
                value1
            } else {
                value2
            };
            Some(winner.data.clone())
        };

        Ok(ResolutionResult {
            conflict_id: conflict.id,
            resolved_value,
            strategy: self.strategy_name().to_string(),
            timestamp: Utc::now(),
        })
    }

    fn strategy_name(&self) -> &str {
        "vector-clock"
    }
}

/// Custom merge function resolver.
pub struct CustomMergeResolver<F>
where
    F: Fn(&Conflict) -> HaResult<Vec<u8>> + Send + Sync,
{
    /// Merge function.
    merge_fn: F,
}

impl<F> CustomMergeResolver<F>
where
    F: Fn(&Conflict) -> HaResult<Vec<u8>> + Send + Sync,
{
    /// Create a new custom merge resolver.
    pub fn new(merge_fn: F) -> Self {
        Self { merge_fn }
    }
}

#[async_trait]
impl<F> ConflictResolver for CustomMergeResolver<F>
where
    F: Fn(&Conflict) -> HaResult<Vec<u8>> + Send + Sync,
{
    async fn resolve(&self, conflict: &Conflict) -> HaResult<ResolutionResult> {
        debug!("Resolving conflict {} using custom merge", conflict.id);

        let merged = (self.merge_fn)(conflict)?;

        info!("Custom merge complete, result size: {} bytes", merged.len());

        Ok(ResolutionResult {
            conflict_id: conflict.id,
            resolved_value: Some(merged),
            strategy: self.strategy_name().to_string(),
            timestamp: Utc::now(),
        })
    }

    fn strategy_name(&self) -> &str {
        "custom-merge"
    }
}

/// Manual resolution (requires human intervention).
pub struct ManualResolver;

impl ManualResolver {
    /// Create a new manual resolver.
    pub fn new() -> Self {
        Self
    }
}

impl Default for ManualResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ConflictResolver for ManualResolver {
    async fn resolve(&self, conflict: &Conflict) -> HaResult<ResolutionResult> {
        info!("Conflict {} requires manual resolution", conflict.id);

        Ok(ResolutionResult {
            conflict_id: conflict.id,
            resolved_value: None,
            strategy: self.strategy_name().to_string(),
            timestamp: Utc::now(),
        })
    }

    fn strategy_name(&self) -> &str {
        "manual"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conflict::{ConflictType, ConflictingValue};
    use crate::replication::VectorClock;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_last_write_wins() {
        let resolver = LastWriteWins::new();

        let now = Utc::now();
        let earlier = now - chrono::Duration::seconds(10);

        let conflict = Conflict {
            id: Uuid::new_v4(),
            conflict_type: ConflictType::WriteWrite,
            key: "test".to_string(),
            values: vec![
                ConflictingValue {
                    node_id: Uuid::new_v4(),
                    data: vec![1, 2, 3],
                    timestamp: earlier,
                    vector_clock: VectorClock::new(),
                },
                ConflictingValue {
                    node_id: Uuid::new_v4(),
                    data: vec![4, 5, 6],
                    timestamp: now,
                    vector_clock: VectorClock::new(),
                },
            ],
            detected_at: Utc::now(),
        };

        let result = resolver.resolve(&conflict).await.ok();
        assert!(result.is_some());

        if let Some(r) = result {
            assert_eq!(r.resolved_value, Some(vec![4, 5, 6]));
        }
    }

    #[tokio::test]
    async fn test_vector_clock_resolver() {
        let resolver = VectorClockResolver::new();

        let node1 = Uuid::new_v4();
        let node2 = Uuid::new_v4();

        let mut clock1 = VectorClock::new();
        clock1.increment(node1);

        let mut clock2 = VectorClock::new();
        clock2.increment(node2);
        clock2.merge(&clock1);

        let conflict = Conflict {
            id: Uuid::new_v4(),
            conflict_type: ConflictType::ConcurrentUpdate,
            key: "test".to_string(),
            values: vec![
                ConflictingValue {
                    node_id: node1,
                    data: vec![1, 2, 3],
                    timestamp: Utc::now(),
                    vector_clock: clock1,
                },
                ConflictingValue {
                    node_id: node2,
                    data: vec![4, 5, 6],
                    timestamp: Utc::now(),
                    vector_clock: clock2,
                },
            ],
            detected_at: Utc::now(),
        };

        let result = resolver.resolve(&conflict).await.ok();
        assert!(result.is_some());

        if let Some(r) = result {
            assert_eq!(r.resolved_value, Some(vec![4, 5, 6]));
        }
    }
}
