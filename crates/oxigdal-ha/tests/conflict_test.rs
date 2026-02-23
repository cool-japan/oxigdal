//! Conflict resolution tests.

use oxigdal_ha::conflict::{
    Conflict, ConflictResolver, ConflictType, ConflictingValue,
    strategies::{LastWriteWins, VectorClockResolver},
};
use oxigdal_ha::replication::VectorClock;
use uuid::Uuid;

#[tokio::test]
async fn test_last_write_wins() {
    let resolver = LastWriteWins::new();

    let now = chrono::Utc::now();
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
        detected_at: chrono::Utc::now(),
    };

    let result = resolver.resolve(&conflict).await.ok();
    assert!(result.is_some());

    let result = result.expect("last-write-wins conflict resolution should succeed");
    assert_eq!(result.resolved_value, Some(vec![4, 5, 6]));
    assert_eq!(result.strategy, "last-write-wins");
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
                timestamp: chrono::Utc::now(),
                vector_clock: clock1,
            },
            ConflictingValue {
                node_id: node2,
                data: vec![4, 5, 6],
                timestamp: chrono::Utc::now(),
                vector_clock: clock2,
            },
        ],
        detected_at: chrono::Utc::now(),
    };

    let result = resolver.resolve(&conflict).await.ok();
    assert!(result.is_some());

    let result = result.expect("vector clock conflict resolution should succeed");
    assert_eq!(result.resolved_value, Some(vec![4, 5, 6]));
}
