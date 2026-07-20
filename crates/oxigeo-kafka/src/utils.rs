//! Utility functions for Kafka operations

use crate::error::{Error, Result};
use std::time::Duration;

/// Validate topic name
pub fn validate_topic_name(topic: &str) -> Result<()> {
    if topic.is_empty() {
        return Err(Error::InvalidTopic(
            "Topic name cannot be empty".to_string(),
        ));
    }

    if topic.len() > 249 {
        return Err(Error::InvalidTopic(
            "Topic name cannot exceed 249 characters".to_string(),
        ));
    }

    // Topic name can only contain ASCII alphanumerics, '.', '_', and '-'
    if !topic
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
    {
        return Err(Error::InvalidTopic(
            "Topic name can only contain ASCII alphanumerics, '.', '_', and '-'".to_string(),
        ));
    }

    // Topic name cannot be "." or ".."
    if topic == "." || topic == ".." {
        return Err(Error::InvalidTopic(
            "Topic name cannot be '.' or '..'".to_string(),
        ));
    }

    Ok(())
}

/// Calculate optimal batch size based on message size
#[allow(dead_code)]
pub fn calculate_batch_size(avg_message_size: usize, max_batch_bytes: usize) -> usize {
    if avg_message_size == 0 {
        return 1000; // Default
    }
    let batch_size = max_batch_bytes / avg_message_size;
    batch_size.clamp(1, 10000)
}

/// Convert duration to milliseconds for Kafka configuration
#[allow(dead_code)]
pub fn duration_to_millis(duration: Duration) -> String {
    duration.as_millis().to_string()
}

/// Hash a key to a partition
pub fn hash_partition(key: &[u8], num_partitions: i32) -> i32 {
    if num_partitions <= 0 {
        return 0;
    }
    let hash = murmur2_hash(key);
    (hash as i32).abs() % num_partitions
}

/// Murmur2 hash implementation (compatible with Kafka's default partitioner)
fn murmur2_hash(data: &[u8]) -> u32 {
    const M: u32 = 0x5bd1_e995;
    const R: u32 = 24;
    const SEED: u32 = 0x9747_b28c;

    let mut h = SEED ^ (data.len() as u32);
    let mut i = 0;

    while i + 4 <= data.len() {
        let mut k = u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]);
        k = k.wrapping_mul(M);
        k ^= k >> R;
        k = k.wrapping_mul(M);

        h = h.wrapping_mul(M);
        h ^= k;

        i += 4;
    }

    match data.len() - i {
        3 => {
            h ^= u32::from(data[i + 2]) << 16;
            h ^= u32::from(data[i + 1]) << 8;
            h ^= u32::from(data[i]);
            h = h.wrapping_mul(M);
        }
        2 => {
            h ^= u32::from(data[i + 1]) << 8;
            h ^= u32::from(data[i]);
            h = h.wrapping_mul(M);
        }
        1 => {
            h ^= u32::from(data[i]);
            h = h.wrapping_mul(M);
        }
        _ => {}
    }

    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^= h >> 15;

    h
}

/// Generate a unique transaction ID
#[allow(dead_code)]
pub fn generate_transaction_id(prefix: &str) -> String {
    format!("{}-{}", prefix, uuid::Uuid::new_v4())
}

/// Parse Kafka version string
#[allow(dead_code)]
pub fn parse_kafka_version(version: &str) -> Result<(u8, u8, u8)> {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() != 3 {
        return Err(Error::Configuration(format!(
            "Invalid Kafka version format: {}",
            version
        )));
    }

    let major = parts[0]
        .parse::<u8>()
        .map_err(|e| Error::Configuration(format!("Invalid major version: {}", e)))?;
    let minor = parts[1]
        .parse::<u8>()
        .map_err(|e| Error::Configuration(format!("Invalid minor version: {}", e)))?;
    let patch = parts[2]
        .parse::<u8>()
        .map_err(|e| Error::Configuration(format!("Invalid patch version: {}", e)))?;

    Ok((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_topic_name() {
        assert!(validate_topic_name("valid-topic").is_ok());
        assert!(validate_topic_name("valid_topic").is_ok());
        assert!(validate_topic_name("valid.topic").is_ok());
        assert!(validate_topic_name("valid123").is_ok());

        assert!(validate_topic_name("").is_err());
        assert!(validate_topic_name(".").is_err());
        assert!(validate_topic_name("..").is_err());
        assert!(validate_topic_name("invalid topic").is_err());
        assert!(validate_topic_name("invalid/topic").is_err());
    }

    #[test]
    fn test_calculate_batch_size() {
        assert_eq!(calculate_batch_size(100, 100000), 1000);
        assert_eq!(calculate_batch_size(1000, 100000), 100);
        assert_eq!(calculate_batch_size(0, 100000), 1000);
        assert!(calculate_batch_size(10, 1000000) <= 10000);
    }

    #[test]
    fn test_hash_partition() {
        let key = b"test-key";
        let partition = hash_partition(key, 10);
        assert!((0..10).contains(&partition));

        // Same key should always hash to same partition
        assert_eq!(hash_partition(key, 10), hash_partition(key, 10));
    }

    #[test]
    fn test_murmur2_hash() {
        // Test consistency
        let data = b"test data";
        assert_eq!(murmur2_hash(data), murmur2_hash(data));

        // Different data should produce different hashes (usually)
        let data2 = b"different data";
        assert_ne!(murmur2_hash(data), murmur2_hash(data2));
    }

    #[test]
    fn test_parse_kafka_version() {
        assert_eq!(parse_kafka_version("2.8.0").ok(), Some((2, 8, 0)));
        assert_eq!(parse_kafka_version("3.0.1").ok(), Some((3, 0, 1)));
        assert!(parse_kafka_version("invalid").is_err());
        assert!(parse_kafka_version("2.8").is_err());
    }
}
