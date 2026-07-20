//! Partitioning strategies for Kafka producers

use crate::utils::hash_partition;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// Partitioner type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartitionerType {
    /// Random partitioning
    Random,
    /// Round-robin partitioning
    RoundRobin,
    /// Hash-based partitioning (default)
    Hash,
    /// Consistent hash partitioning
    ConsistentHash,
}

/// Trait for custom partitioners
pub trait Partitioner: Send + Sync {
    /// Determine the partition for a given message
    fn partition(&self, topic: &str, key: &[u8], num_partitions: i32) -> i32;
}

/// Hash-based partitioner (Kafka default)
pub struct HashPartitioner;

impl Partitioner for HashPartitioner {
    fn partition(&self, _topic: &str, key: &[u8], num_partitions: i32) -> i32 {
        hash_partition(key, num_partitions)
    }
}

/// Round-robin partitioner
pub struct RoundRobinPartitioner {
    counter: AtomicU32,
}

impl RoundRobinPartitioner {
    /// Create a new round-robin partitioner
    pub fn new() -> Self {
        Self {
            counter: AtomicU32::new(0),
        }
    }
}

impl Default for RoundRobinPartitioner {
    fn default() -> Self {
        Self::new()
    }
}

impl Partitioner for RoundRobinPartitioner {
    fn partition(&self, _topic: &str, _key: &[u8], num_partitions: i32) -> i32 {
        let count = self.counter.fetch_add(1, Ordering::Relaxed);
        (count % num_partitions as u32) as i32
    }
}

/// Random partitioner
pub struct RandomPartitioner;

impl Partitioner for RandomPartitioner {
    fn partition(&self, _topic: &str, _key: &[u8], num_partitions: i32) -> i32 {
        use std::collections::hash_map::RandomState;
        use std::hash::BuildHasher;

        let random_state = RandomState::new();
        let hash = random_state.hash_one(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
        );
        (hash % num_partitions as u64) as i32
    }
}

/// Consistent hash partitioner using jump hash
pub struct ConsistentHashPartitioner;

impl ConsistentHashPartitioner {
    /// Jump consistent hash algorithm
    fn jump_hash(key: u64, num_buckets: i32) -> i32 {
        let mut k = key;
        let mut b: i64 = -1;
        let mut j: i64 = 0;

        while j < num_buckets as i64 {
            b = j;
            k = k.wrapping_mul(2862933555777941757).wrapping_add(1);
            j = ((b.wrapping_add(1) as f64)
                * ((1i64 << 31) as f64 / ((k >> 33).wrapping_add(1) as f64)))
                as i64;
        }

        b as i32
    }
}

impl Partitioner for ConsistentHashPartitioner {
    fn partition(&self, _topic: &str, key: &[u8], num_partitions: i32) -> i32 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let hash = hasher.finish();
        Self::jump_hash(hash, num_partitions)
    }
}

/// Custom partitioner wrapper
pub struct CustomPartitioner {
    partitioner: Arc<dyn Partitioner>,
}

impl CustomPartitioner {
    /// Create a new custom partitioner
    pub fn new(partitioner: Arc<dyn Partitioner>) -> Self {
        Self { partitioner }
    }
}

impl Partitioner for CustomPartitioner {
    fn partition(&self, topic: &str, key: &[u8], num_partitions: i32) -> i32 {
        self.partitioner.partition(topic, key, num_partitions)
    }
}

/// Create a partitioner based on type
pub fn create_partitioner(partitioner_type: &PartitionerType) -> Arc<dyn Partitioner> {
    match partitioner_type {
        PartitionerType::Hash => Arc::new(HashPartitioner),
        PartitionerType::RoundRobin => Arc::new(RoundRobinPartitioner::new()),
        PartitionerType::Random => Arc::new(RandomPartitioner),
        PartitionerType::ConsistentHash => Arc::new(ConsistentHashPartitioner),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_partitioner() {
        let partitioner = HashPartitioner;
        let key = b"test-key";
        let num_partitions = 10;

        let partition = partitioner.partition("test-topic", key, num_partitions);
        assert!(partition >= 0 && partition < num_partitions);

        // Same key should always go to same partition
        let partition2 = partitioner.partition("test-topic", key, num_partitions);
        assert_eq!(partition, partition2);
    }

    #[test]
    fn test_round_robin_partitioner() {
        let partitioner = RoundRobinPartitioner::new();
        let num_partitions = 3;

        let p1 = partitioner.partition("test-topic", b"key1", num_partitions);
        let p2 = partitioner.partition("test-topic", b"key2", num_partitions);
        let p3 = partitioner.partition("test-topic", b"key3", num_partitions);
        let p4 = partitioner.partition("test-topic", b"key4", num_partitions);

        assert!(p1 >= 0 && p1 < num_partitions);
        assert!(p2 >= 0 && p2 < num_partitions);
        assert!(p3 >= 0 && p3 < num_partitions);
        assert!(p4 >= 0 && p4 < num_partitions);

        // Should rotate through partitions
        assert_eq!((p1 + 1) % num_partitions, p2);
        assert_eq!((p2 + 1) % num_partitions, p3);
        assert_eq!((p3 + 1) % num_partitions, p4);
    }

    #[test]
    fn test_random_partitioner() {
        let partitioner = RandomPartitioner;
        let num_partitions = 10;

        for _ in 0..100 {
            let partition = partitioner.partition("test-topic", b"key", num_partitions);
            assert!(partition >= 0 && partition < num_partitions);
        }
    }

    #[test]
    fn test_consistent_hash_partitioner() {
        let partitioner = ConsistentHashPartitioner;
        let key = b"test-key";
        let num_partitions = 10;

        let partition = partitioner.partition("test-topic", key, num_partitions);
        assert!(partition >= 0 && partition < num_partitions);

        // Same key should always go to same partition
        let partition2 = partitioner.partition("test-topic", key, num_partitions);
        assert_eq!(partition, partition2);

        // Different keys should distribute well
        let mut partitions = vec![0; num_partitions as usize];
        for i in 0..1000 {
            let key = format!("key-{}", i);
            let partition = partitioner.partition("test-topic", key.as_bytes(), num_partitions);
            partitions[partition as usize] += 1;
        }

        // Check distribution is reasonably uniform (within 20% of average)
        let avg = 1000.0 / num_partitions as f64;
        for count in partitions {
            let ratio = count as f64 / avg;
            assert!(
                ratio > 0.6 && ratio < 1.4,
                "Distribution not uniform: {}",
                ratio
            );
        }
    }

    #[test]
    fn test_jump_hash() {
        // Test jump hash consistency
        let hash1 = ConsistentHashPartitioner::jump_hash(12345, 10);
        let hash2 = ConsistentHashPartitioner::jump_hash(12345, 10);
        assert_eq!(hash1, hash2);

        // Test that result is within bounds
        for i in 0..100 {
            let result = ConsistentHashPartitioner::jump_hash(i, 10);
            assert!((0..10).contains(&result));
        }
    }
}
