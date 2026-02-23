//! Shard management for Kinesis Data Streams

use crate::error::{KinesisError, Result};
use aws_sdk_kinesis::Client as KinesisClient;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;

/// Shard iterator type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ShardIteratorType {
    /// Start from the oldest record
    TrimHorizon,
    /// Start from the latest record
    Latest,
    /// Start at a specific sequence number
    AtSequenceNumber(String),
    /// Start after a specific sequence number
    AfterSequenceNumber(String),
    /// Start at a specific timestamp
    AtTimestamp(i64),
}

/// Shard information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardInfo {
    /// Shard ID
    pub shard_id: String,
    /// Parent shard ID
    pub parent_shard_id: Option<String>,
    /// Adjacent parent shard ID
    pub adjacent_parent_shard_id: Option<String>,
    /// Hash key range start
    pub hash_key_range_start: String,
    /// Hash key range end
    pub hash_key_range_end: String,
    /// Sequence number range start
    pub sequence_number_range_start: String,
    /// Sequence number range end (None if shard is still open)
    pub sequence_number_range_end: Option<String>,
}

impl ShardInfo {
    /// Checks if the shard is open (still accepting writes)
    pub fn is_open(&self) -> bool {
        self.sequence_number_range_end.is_none()
    }

    /// Checks if the shard is closed
    pub fn is_closed(&self) -> bool {
        !self.is_open()
    }

    /// Checks if this is a root shard (no parent)
    pub fn is_root(&self) -> bool {
        self.parent_shard_id.is_none() && self.adjacent_parent_shard_id.is_none()
    }

    /// Checks if this is a child shard (has parent)
    pub fn is_child(&self) -> bool {
        !self.is_root()
    }
}

/// Shard manager for managing stream shards
pub struct ShardManager {
    client: Arc<KinesisClient>,
    stream_name: String,
    shards: parking_lot::RwLock<HashMap<String, ShardInfo>>,
}

impl ShardManager {
    /// Creates a new shard manager
    pub fn new(client: KinesisClient, stream_name: impl Into<String>) -> Self {
        Self {
            client: Arc::new(client),
            stream_name: stream_name.into(),
            shards: parking_lot::RwLock::new(HashMap::new()),
        }
    }

    /// Refreshes shard information from Kinesis
    pub async fn refresh(&self) -> Result<()> {
        let shards = self.list_shards().await?;

        let mut shard_map = HashMap::new();
        for shard in shards {
            shard_map.insert(shard.shard_id.clone(), shard);
        }

        *self.shards.write() = shard_map;

        info!("Refreshed {} shards", self.shards.read().len());
        Ok(())
    }

    /// Lists all shards in the stream
    pub async fn list_shards(&self) -> Result<Vec<ShardInfo>> {
        let mut shards = Vec::new();
        let mut exclusive_start_shard_id = None;

        loop {
            let mut request = self.client.list_shards().stream_name(&self.stream_name);

            if let Some(start_shard_id) = exclusive_start_shard_id {
                request = request.exclusive_start_shard_id(start_shard_id);
            }

            let response = request.send().await.map_err(|e| KinesisError::Service {
                message: e.to_string(),
            })?;

            let response_shards = response.shards();
            for shard in response_shards {
                let shard_info = ShardInfo {
                    shard_id: shard.shard_id().to_string(),
                    parent_shard_id: shard.parent_shard_id().map(|s| s.to_string()),
                    adjacent_parent_shard_id: shard
                        .adjacent_parent_shard_id()
                        .map(|s| s.to_string()),
                    hash_key_range_start: shard
                        .hash_key_range()
                        .map(|h| h.starting_hash_key().to_string())
                        .unwrap_or_default(),
                    hash_key_range_end: shard
                        .hash_key_range()
                        .map(|h| h.ending_hash_key().to_string())
                        .unwrap_or_default(),
                    sequence_number_range_start: shard
                        .sequence_number_range()
                        .map(|s| s.starting_sequence_number().to_string())
                        .unwrap_or_default(),
                    sequence_number_range_end: shard
                        .sequence_number_range()
                        .and_then(|s| s.ending_sequence_number())
                        .map(|s| s.to_string()),
                };

                shards.push(shard_info);
            }

            // Check if there are more shards
            if response.next_token().is_none() {
                break;
            }

            exclusive_start_shard_id = response_shards.last().map(|s| s.shard_id().to_string());
        }

        Ok(shards)
    }

    /// Gets all shards from cache
    pub fn get_all_shards(&self) -> Vec<ShardInfo> {
        self.shards.read().values().cloned().collect()
    }

    /// Gets a specific shard by ID
    pub fn get_shard(&self, shard_id: &str) -> Option<ShardInfo> {
        self.shards.read().get(shard_id).cloned()
    }

    /// Gets all open shards
    pub fn get_open_shards(&self) -> Vec<ShardInfo> {
        self.shards
            .read()
            .values()
            .filter(|s| s.is_open())
            .cloned()
            .collect()
    }

    /// Gets all closed shards
    pub fn get_closed_shards(&self) -> Vec<ShardInfo> {
        self.shards
            .read()
            .values()
            .filter(|s| s.is_closed())
            .cloned()
            .collect()
    }

    /// Gets all root shards (no parent)
    pub fn get_root_shards(&self) -> Vec<ShardInfo> {
        self.shards
            .read()
            .values()
            .filter(|s| s.is_root())
            .cloned()
            .collect()
    }

    /// Gets child shards of a specific parent shard
    pub fn get_child_shards(&self, parent_shard_id: &str) -> Vec<ShardInfo> {
        self.shards
            .read()
            .values()
            .filter(|s| {
                s.parent_shard_id.as_deref() == Some(parent_shard_id)
                    || s.adjacent_parent_shard_id.as_deref() == Some(parent_shard_id)
            })
            .cloned()
            .collect()
    }

    /// Splits a shard at the given hash key
    pub async fn split_shard(&self, shard_id: &str, new_starting_hash_key: &str) -> Result<()> {
        info!("Splitting shard: {}", shard_id);

        self.client
            .split_shard()
            .stream_name(&self.stream_name)
            .shard_to_split(shard_id)
            .new_starting_hash_key(new_starting_hash_key)
            .send()
            .await
            .map_err(|e| KinesisError::Service {
                message: e.to_string(),
            })?;

        // Refresh shard information
        self.refresh().await?;

        Ok(())
    }

    /// Merges two adjacent shards
    pub async fn merge_shards(
        &self,
        shard_to_merge: &str,
        adjacent_shard_to_merge: &str,
    ) -> Result<()> {
        info!(
            "Merging shards: {} and {}",
            shard_to_merge, adjacent_shard_to_merge
        );

        self.client
            .merge_shards()
            .stream_name(&self.stream_name)
            .shard_to_merge(shard_to_merge)
            .adjacent_shard_to_merge(adjacent_shard_to_merge)
            .send()
            .await
            .map_err(|e| KinesisError::Service {
                message: e.to_string(),
            })?;

        // Refresh shard information
        self.refresh().await?;

        Ok(())
    }

    /// Updates shard count (scales the stream)
    pub async fn update_shard_count(&self, target_shard_count: i32) -> Result<()> {
        info!("Updating shard count to: {}", target_shard_count);

        self.client
            .update_shard_count()
            .stream_name(&self.stream_name)
            .target_shard_count(target_shard_count)
            .scaling_type(aws_sdk_kinesis::types::ScalingType::UniformScaling)
            .send()
            .await
            .map_err(|e| KinesisError::Service {
                message: e.to_string(),
            })?;

        // Refresh shard information
        self.refresh().await?;

        Ok(())
    }

    /// Gets the total number of shards
    pub fn shard_count(&self) -> usize {
        self.shards.read().len()
    }

    /// Gets the number of open shards
    pub fn open_shard_count(&self) -> usize {
        self.shards.read().values().filter(|s| s.is_open()).count()
    }

    /// Gets the number of closed shards
    pub fn closed_shard_count(&self) -> usize {
        self.shards
            .read()
            .values()
            .filter(|s| s.is_closed())
            .count()
    }

    /// Calculates the hash key for a partition key
    pub fn calculate_hash_key(partition_key: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        partition_key.hash(&mut hasher);
        let hash = hasher.finish();

        // Scale to 128-bit range (Kinesis uses MD5)
        // This is a simplified version - production should use MD5
        format!("{:032x}", hash as u128)
    }

    /// Finds the shard for a given partition key
    pub fn find_shard_for_partition_key(&self, partition_key: &str) -> Option<ShardInfo> {
        let hash_key = Self::calculate_hash_key(partition_key);

        self.shards
            .read()
            .values()
            .filter(|s| s.is_open())
            .find(|s| hash_key >= s.hash_key_range_start && hash_key <= s.hash_key_range_end)
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shard_info_open() {
        let shard = ShardInfo {
            shard_id: "shardId-000000000000".to_string(),
            parent_shard_id: None,
            adjacent_parent_shard_id: None,
            hash_key_range_start: "0".to_string(),
            hash_key_range_end: "340282366920938463463374607431768211455".to_string(),
            sequence_number_range_start: "49590338271490256608559692538361571095921575989136588898"
                .to_string(),
            sequence_number_range_end: None,
        };

        assert!(shard.is_open());
        assert!(!shard.is_closed());
        assert!(shard.is_root());
        assert!(!shard.is_child());
    }

    #[test]
    fn test_shard_info_closed() {
        let shard = ShardInfo {
            shard_id: "shardId-000000000000".to_string(),
            parent_shard_id: None,
            adjacent_parent_shard_id: None,
            hash_key_range_start: "0".to_string(),
            hash_key_range_end: "170141183460469231731687303715884105727".to_string(),
            sequence_number_range_start: "49590338271490256608559692538361571095921575989136588898"
                .to_string(),
            sequence_number_range_end: Some(
                "49590343671614563306579692538412791935923146059171209234".to_string(),
            ),
        };

        assert!(!shard.is_open());
        assert!(shard.is_closed());
    }

    #[test]
    fn test_shard_info_child() {
        let shard = ShardInfo {
            shard_id: "shardId-000000000001".to_string(),
            parent_shard_id: Some("shardId-000000000000".to_string()),
            adjacent_parent_shard_id: None,
            hash_key_range_start: "0".to_string(),
            hash_key_range_end: "85070591730234615865843651857942052863".to_string(),
            sequence_number_range_start: "49590343671636863852599692538423902855924716129205829650"
                .to_string(),
            sequence_number_range_end: None,
        };

        assert!(!shard.is_root());
        assert!(shard.is_child());
    }

    #[test]
    fn test_calculate_hash_key() {
        let hash1 = ShardManager::calculate_hash_key("partition-1");
        let hash2 = ShardManager::calculate_hash_key("partition-2");
        let hash3 = ShardManager::calculate_hash_key("partition-1");

        // Same partition key should produce same hash
        assert_eq!(hash1, hash3);

        // Different partition keys should produce different hashes
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_shard_iterator_type_serialization() {
        let iter_type = ShardIteratorType::Latest;
        let json = serde_json::to_string(&iter_type).ok();
        assert!(json.is_some());

        let iter_type = ShardIteratorType::AtSequenceNumber("12345".to_string());
        let json = serde_json::to_string(&iter_type).ok();
        assert!(json.is_some());
    }
}
