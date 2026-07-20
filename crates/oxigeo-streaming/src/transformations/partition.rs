//! Partitioning strategies for stream processing.

use crate::core::stream::StreamElement;
use crate::error::{Result, StreamingError};
use ahash::AHasher;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Strategy for partitioning stream elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartitionStrategy {
    /// Hash-based partitioning
    Hash,

    /// Range-based partitioning
    Range,

    /// Round-robin partitioning
    RoundRobin,

    /// Random partitioning
    Random,

    /// Broadcast to all partitions
    Broadcast,
}

/// Key selector function for partitioning.
pub trait KeySelector: Send + Sync {
    /// Extract key from an element.
    fn select_key(&self, element: &StreamElement) -> Vec<u8>;
}

/// Simple key selector that uses the element's key field.
pub struct ElementKeySelector;

impl KeySelector for ElementKeySelector {
    fn select_key(&self, element: &StreamElement) -> Vec<u8> {
        element.key.clone().unwrap_or_default()
    }
}

/// Partitioner trait.
pub trait Partitioner: Send + Sync {
    /// Determine the partition for an element.
    fn partition(&self, element: &StreamElement, num_partitions: usize) -> Result<usize>;

    /// Get the partitioning strategy.
    fn strategy(&self) -> PartitionStrategy;
}

/// Hash-based partitioner.
pub struct HashPartitioner<K>
where
    K: KeySelector,
{
    key_selector: Arc<K>,
}

impl<K> HashPartitioner<K>
where
    K: KeySelector,
{
    /// Create a new hash partitioner.
    pub fn new(key_selector: K) -> Self {
        Self {
            key_selector: Arc::new(key_selector),
        }
    }
}

impl<K> Partitioner for HashPartitioner<K>
where
    K: KeySelector,
{
    fn partition(&self, element: &StreamElement, num_partitions: usize) -> Result<usize> {
        if num_partitions == 0 {
            return Err(StreamingError::PartitionError(
                "Number of partitions must be greater than 0".to_string(),
            ));
        }

        let key = self.key_selector.select_key(element);
        let mut hasher = AHasher::default();
        key.hash(&mut hasher);
        let hash = hasher.finish();

        Ok((hash as usize) % num_partitions)
    }

    fn strategy(&self) -> PartitionStrategy {
        PartitionStrategy::Hash
    }
}

/// Range-based partitioner.
pub struct RangePartitioner<K>
where
    K: KeySelector,
{
    key_selector: Arc<K>,
    boundaries: Vec<Vec<u8>>,
}

impl<K> RangePartitioner<K>
where
    K: KeySelector,
{
    /// Create a new range partitioner.
    pub fn new(key_selector: K, boundaries: Vec<Vec<u8>>) -> Self {
        Self {
            key_selector: Arc::new(key_selector),
            boundaries,
        }
    }
}

impl<K> Partitioner for RangePartitioner<K>
where
    K: KeySelector,
{
    fn partition(&self, element: &StreamElement, num_partitions: usize) -> Result<usize> {
        if num_partitions == 0 {
            return Err(StreamingError::PartitionError(
                "Number of partitions must be greater than 0".to_string(),
            ));
        }

        let key = self.key_selector.select_key(element);

        for (i, boundary) in self.boundaries.iter().enumerate() {
            if &key < boundary {
                return Ok(i.min(num_partitions - 1));
            }
        }

        Ok(num_partitions - 1)
    }

    fn strategy(&self) -> PartitionStrategy {
        PartitionStrategy::Range
    }
}

/// Round-robin partitioner.
pub struct RoundRobinPartitioner {
    counter: Arc<AtomicUsize>,
}

impl RoundRobinPartitioner {
    /// Create a new round-robin partitioner.
    pub fn new() -> Self {
        Self {
            counter: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl Default for RoundRobinPartitioner {
    fn default() -> Self {
        Self::new()
    }
}

impl Partitioner for RoundRobinPartitioner {
    fn partition(&self, _element: &StreamElement, num_partitions: usize) -> Result<usize> {
        if num_partitions == 0 {
            return Err(StreamingError::PartitionError(
                "Number of partitions must be greater than 0".to_string(),
            ));
        }

        let partition = self.counter.fetch_add(1, Ordering::Relaxed) % num_partitions;
        Ok(partition)
    }

    fn strategy(&self) -> PartitionStrategy {
        PartitionStrategy::RoundRobin
    }
}

/// Broadcast partitioner (returns all partitions).
pub struct BroadcastPartitioner;

impl BroadcastPartitioner {
    /// Create a new broadcast partitioner.
    pub fn new() -> Self {
        Self
    }
}

impl Default for BroadcastPartitioner {
    fn default() -> Self {
        Self::new()
    }
}

impl Partitioner for BroadcastPartitioner {
    fn partition(&self, _element: &StreamElement, num_partitions: usize) -> Result<usize> {
        if num_partitions == 0 {
            return Err(StreamingError::PartitionError(
                "Number of partitions must be greater than 0".to_string(),
            ));
        }

        Ok(0)
    }

    fn strategy(&self) -> PartitionStrategy {
        PartitionStrategy::Broadcast
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_hash_partitioner() {
        let partitioner = HashPartitioner::new(ElementKeySelector);

        let elem = StreamElement::new(vec![1, 2, 3], Utc::now()).with_key(vec![1]);
        let partition = partitioner
            .partition(&elem, 4)
            .expect("Failed to partition element with hash partitioner");

        assert!(partition < 4);
    }

    #[test]
    fn test_hash_partitioner_consistency() {
        let partitioner = HashPartitioner::new(ElementKeySelector);

        let elem = StreamElement::new(vec![1, 2, 3], Utc::now()).with_key(vec![1]);
        let p1 = partitioner
            .partition(&elem, 4)
            .expect("Failed to partition element for consistency test (first call)");
        let p2 = partitioner
            .partition(&elem, 4)
            .expect("Failed to partition element for consistency test (second call)");

        assert_eq!(p1, p2);
    }

    #[test]
    fn test_range_partitioner() {
        let boundaries = vec![vec![5], vec![10], vec![15]];
        let partitioner = RangePartitioner::new(ElementKeySelector, boundaries);

        let elem1 = StreamElement::new(vec![1, 2, 3], Utc::now()).with_key(vec![3]);
        let elem2 = StreamElement::new(vec![1, 2, 3], Utc::now()).with_key(vec![7]);
        let elem3 = StreamElement::new(vec![1, 2, 3], Utc::now()).with_key(vec![12]);

        assert_eq!(
            partitioner
                .partition(&elem1, 4)
                .expect("Failed to partition element 1 with range partitioner"),
            0
        );
        assert_eq!(
            partitioner
                .partition(&elem2, 4)
                .expect("Failed to partition element 2 with range partitioner"),
            1
        );
        assert_eq!(
            partitioner
                .partition(&elem3, 4)
                .expect("Failed to partition element 3 with range partitioner"),
            2
        );
    }

    #[test]
    fn test_round_robin_partitioner() {
        let partitioner = RoundRobinPartitioner::new();

        let elem = StreamElement::new(vec![1, 2, 3], Utc::now());

        let mut partitions = Vec::new();
        for _ in 0..8 {
            partitions.push(
                partitioner
                    .partition(&elem, 4)
                    .expect("Failed to partition element with round-robin partitioner"),
            );
        }

        assert_eq!(partitions, vec![0, 1, 2, 3, 0, 1, 2, 3]);
    }

    #[test]
    fn test_broadcast_partitioner() {
        let partitioner = BroadcastPartitioner::new();

        let elem = StreamElement::new(vec![1, 2, 3], Utc::now());
        let partition = partitioner
            .partition(&elem, 4)
            .expect("Failed to partition element with broadcast partitioner");

        assert_eq!(partition, 0);
        assert_eq!(partitioner.strategy(), PartitionStrategy::Broadcast);
    }
}
