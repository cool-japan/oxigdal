//! Checkpointing for Kinesis consumers with DynamoDB backend

use crate::error::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[cfg(feature = "checkpointing")]
use crate::error::KinesisError;
#[cfg(feature = "checkpointing")]
use tracing::{debug, info};

#[cfg(feature = "checkpointing")]
use aws_sdk_dynamodb::Client as DynamoDbClient;
#[cfg(feature = "checkpointing")]
use aws_sdk_dynamodb::types::AttributeValue;

/// Checkpoint information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    /// Shard ID
    pub shard_id: String,
    /// Sequence number
    pub sequence_number: String,
    /// Checkpoint timestamp
    pub checkpoint_time: DateTime<Utc>,
    /// Worker ID that owns this checkpoint
    pub worker_id: Option<String>,
    /// Lease expiration time
    pub lease_expiration: Option<DateTime<Utc>>,
}

impl Checkpoint {
    /// Creates a new checkpoint
    pub fn new(shard_id: impl Into<String>, sequence_number: impl Into<String>) -> Self {
        Self {
            shard_id: shard_id.into(),
            sequence_number: sequence_number.into(),
            checkpoint_time: Utc::now(),
            worker_id: None,
            lease_expiration: None,
        }
    }

    /// Sets the worker ID
    pub fn with_worker_id(mut self, worker_id: impl Into<String>) -> Self {
        self.worker_id = Some(worker_id.into());
        self
    }

    /// Sets the lease expiration
    pub fn with_lease_expiration(mut self, expiration: DateTime<Utc>) -> Self {
        self.lease_expiration = Some(expiration);
        self
    }

    /// Checks if the lease has expired
    pub fn is_lease_expired(&self) -> bool {
        self.lease_expiration
            .map(|exp| Utc::now() > exp)
            .unwrap_or(true)
    }
}

/// Checkpoint store trait
#[async_trait]
pub trait CheckpointStore: Send + Sync {
    /// Saves a checkpoint
    async fn save_checkpoint(&self, checkpoint: &Checkpoint) -> Result<()>;

    /// Gets a checkpoint for a shard
    async fn get_checkpoint(&self, shard_id: &str) -> Result<Option<Checkpoint>>;

    /// Deletes a checkpoint for a shard
    async fn delete_checkpoint(&self, shard_id: &str) -> Result<()>;

    /// Lists all checkpoints
    async fn list_checkpoints(&self) -> Result<Vec<Checkpoint>>;

    /// Acquires a lease for a shard
    async fn acquire_lease(
        &self,
        shard_id: &str,
        worker_id: &str,
        lease_duration_secs: i64,
    ) -> Result<bool>;

    /// Renews a lease for a shard
    async fn renew_lease(
        &self,
        shard_id: &str,
        worker_id: &str,
        lease_duration_secs: i64,
    ) -> Result<bool>;

    /// Releases a lease for a shard
    async fn release_lease(&self, shard_id: &str, worker_id: &str) -> Result<()>;
}

/// In-memory checkpoint store (for testing)
pub struct InMemoryCheckpointStore {
    checkpoints: Arc<parking_lot::RwLock<HashMap<String, Checkpoint>>>,
}

impl InMemoryCheckpointStore {
    /// Creates a new in-memory checkpoint store
    pub fn new() -> Self {
        Self {
            checkpoints: Arc::new(parking_lot::RwLock::new(HashMap::new())),
        }
    }
}

impl Default for InMemoryCheckpointStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CheckpointStore for InMemoryCheckpointStore {
    async fn save_checkpoint(&self, checkpoint: &Checkpoint) -> Result<()> {
        self.checkpoints
            .write()
            .insert(checkpoint.shard_id.clone(), checkpoint.clone());
        Ok(())
    }

    async fn get_checkpoint(&self, shard_id: &str) -> Result<Option<Checkpoint>> {
        Ok(self.checkpoints.read().get(shard_id).cloned())
    }

    async fn delete_checkpoint(&self, shard_id: &str) -> Result<()> {
        self.checkpoints.write().remove(shard_id);
        Ok(())
    }

    async fn list_checkpoints(&self) -> Result<Vec<Checkpoint>> {
        Ok(self.checkpoints.read().values().cloned().collect())
    }

    async fn acquire_lease(
        &self,
        shard_id: &str,
        worker_id: &str,
        lease_duration_secs: i64,
    ) -> Result<bool> {
        let mut checkpoints = self.checkpoints.write();

        if let Some(checkpoint) = checkpoints.get(shard_id) {
            // Check if lease is expired or owned by same worker
            if checkpoint.is_lease_expired() || checkpoint.worker_id.as_deref() == Some(worker_id) {
                let mut updated = checkpoint.clone();
                updated.worker_id = Some(worker_id.to_string());
                updated.lease_expiration = Some(
                    Utc::now()
                        + chrono::Duration::try_seconds(lease_duration_secs).unwrap_or_default(),
                );
                checkpoints.insert(shard_id.to_string(), updated);
                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            // No checkpoint exists, create one
            let checkpoint = Checkpoint::new(shard_id, "")
                .with_worker_id(worker_id)
                .with_lease_expiration(
                    Utc::now()
                        + chrono::Duration::try_seconds(lease_duration_secs).unwrap_or_default(),
                );
            checkpoints.insert(shard_id.to_string(), checkpoint);
            Ok(true)
        }
    }

    async fn renew_lease(
        &self,
        shard_id: &str,
        worker_id: &str,
        lease_duration_secs: i64,
    ) -> Result<bool> {
        let mut checkpoints = self.checkpoints.write();

        if let Some(checkpoint) = checkpoints.get(shard_id) {
            if checkpoint.worker_id.as_deref() == Some(worker_id) {
                let mut updated = checkpoint.clone();
                updated.lease_expiration = Some(
                    Utc::now()
                        + chrono::Duration::try_seconds(lease_duration_secs).unwrap_or_default(),
                );
                checkpoints.insert(shard_id.to_string(), updated);
                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            Ok(false)
        }
    }

    async fn release_lease(&self, shard_id: &str, worker_id: &str) -> Result<()> {
        let mut checkpoints = self.checkpoints.write();

        if let Some(checkpoint) = checkpoints.get(shard_id) {
            if checkpoint.worker_id.as_deref() == Some(worker_id) {
                let mut updated = checkpoint.clone();
                updated.worker_id = None;
                updated.lease_expiration = None;
                checkpoints.insert(shard_id.to_string(), updated);
            }
        }

        Ok(())
    }
}

/// DynamoDB checkpoint store
#[cfg(feature = "checkpointing")]
pub struct DynamoDbCheckpointStore {
    client: Arc<DynamoDbClient>,
    table_name: String,
}

#[cfg(feature = "checkpointing")]
impl DynamoDbCheckpointStore {
    /// Creates a new DynamoDB checkpoint store
    pub fn new(client: DynamoDbClient, table_name: impl Into<String>) -> Self {
        Self {
            client: Arc::new(client),
            table_name: table_name.into(),
        }
    }

    /// Creates a new DynamoDB checkpoint store from environment
    pub async fn from_env(table_name: impl Into<String>) -> Self {
        let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        let client = DynamoDbClient::new(&config);
        Self::new(client, table_name)
    }

    /// Creates the checkpoint table if it doesn't exist
    pub async fn create_table_if_not_exists(&self) -> Result<()> {
        // Check if table exists
        match self
            .client
            .describe_table()
            .table_name(&self.table_name)
            .send()
            .await
        {
            Ok(_) => {
                info!("Checkpoint table already exists: {}", self.table_name);
                return Ok(());
            }
            Err(_) => {
                // Table doesn't exist, create it
            }
        }

        info!("Creating checkpoint table: {}", self.table_name);

        self.client
            .create_table()
            .table_name(&self.table_name)
            .key_schema(
                aws_sdk_dynamodb::types::KeySchemaElement::builder()
                    .attribute_name("shard_id")
                    .key_type(aws_sdk_dynamodb::types::KeyType::Hash)
                    .build()
                    .map_err(|e| KinesisError::Checkpoint {
                        message: e.to_string(),
                    })?,
            )
            .attribute_definitions(
                aws_sdk_dynamodb::types::AttributeDefinition::builder()
                    .attribute_name("shard_id")
                    .attribute_type(aws_sdk_dynamodb::types::ScalarAttributeType::S)
                    .build()
                    .map_err(|e| KinesisError::Checkpoint {
                        message: e.to_string(),
                    })?,
            )
            .billing_mode(aws_sdk_dynamodb::types::BillingMode::PayPerRequest)
            .send()
            .await
            .map_err(|e| KinesisError::Checkpoint {
                message: e.to_string(),
            })?;

        // Wait for table to become active
        self.wait_for_table_active().await?;

        Ok(())
    }

    /// Waits for table to become active
    async fn wait_for_table_active(&self) -> Result<()> {
        use tokio::time::{Duration, sleep};

        loop {
            let response = self
                .client
                .describe_table()
                .table_name(&self.table_name)
                .send()
                .await
                .map_err(|e| KinesisError::Checkpoint {
                    message: e.to_string(),
                })?;

            if let Some(table) = response.table() {
                match table.table_status() {
                    Some(status) if status.as_str() == "ACTIVE" => {
                        info!("Checkpoint table is now active");
                        return Ok(());
                    }
                    Some(status) if status.as_str() == "CREATING" => {
                        debug!("Waiting for checkpoint table to become active...");
                        sleep(Duration::from_secs(1)).await;
                    }
                    Some(status) => {
                        return Err(KinesisError::Checkpoint {
                            message: format!("Table in unexpected state: {}", status.as_str()),
                        });
                    }
                    None => {
                        return Err(KinesisError::Checkpoint {
                            message: "Table status not available".to_string(),
                        });
                    }
                }
            }
        }
    }
}

#[cfg(feature = "checkpointing")]
#[async_trait]
impl CheckpointStore for DynamoDbCheckpointStore {
    async fn save_checkpoint(&self, checkpoint: &Checkpoint) -> Result<()> {
        let mut item = HashMap::new();
        item.insert(
            "shard_id".to_string(),
            AttributeValue::S(checkpoint.shard_id.clone()),
        );
        item.insert(
            "sequence_number".to_string(),
            AttributeValue::S(checkpoint.sequence_number.clone()),
        );
        item.insert(
            "checkpoint_time".to_string(),
            AttributeValue::S(checkpoint.checkpoint_time.to_rfc3339()),
        );

        if let Some(worker_id) = &checkpoint.worker_id {
            item.insert(
                "worker_id".to_string(),
                AttributeValue::S(worker_id.clone()),
            );
        }

        if let Some(lease_expiration) = &checkpoint.lease_expiration {
            item.insert(
                "lease_expiration".to_string(),
                AttributeValue::S(lease_expiration.to_rfc3339()),
            );
        }

        self.client
            .put_item()
            .table_name(&self.table_name)
            .set_item(Some(item))
            .send()
            .await
            .map_err(|e| KinesisError::Checkpoint {
                message: e.to_string(),
            })?;

        Ok(())
    }

    async fn get_checkpoint(&self, shard_id: &str) -> Result<Option<Checkpoint>> {
        let mut key = HashMap::new();
        key.insert(
            "shard_id".to_string(),
            AttributeValue::S(shard_id.to_string()),
        );

        let response = self
            .client
            .get_item()
            .table_name(&self.table_name)
            .set_key(Some(key))
            .send()
            .await
            .map_err(|e| KinesisError::Checkpoint {
                message: e.to_string(),
            })?;

        if let Some(item) = response.item() {
            let checkpoint = Checkpoint {
                shard_id: item
                    .get("shard_id")
                    .and_then(|v| v.as_s().ok())
                    .unwrap_or(&String::new())
                    .clone(),
                sequence_number: item
                    .get("sequence_number")
                    .and_then(|v| v.as_s().ok())
                    .unwrap_or(&String::new())
                    .clone(),
                checkpoint_time: item
                    .get("checkpoint_time")
                    .and_then(|v| v.as_s().ok())
                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(Utc::now),
                worker_id: item.get("worker_id").and_then(|v| v.as_s().ok()).cloned(),
                lease_expiration: item
                    .get("lease_expiration")
                    .and_then(|v| v.as_s().ok())
                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.with_timezone(&Utc)),
            };

            Ok(Some(checkpoint))
        } else {
            Ok(None)
        }
    }

    async fn delete_checkpoint(&self, shard_id: &str) -> Result<()> {
        let mut key = HashMap::new();
        key.insert(
            "shard_id".to_string(),
            AttributeValue::S(shard_id.to_string()),
        );

        self.client
            .delete_item()
            .table_name(&self.table_name)
            .set_key(Some(key))
            .send()
            .await
            .map_err(|e| KinesisError::Checkpoint {
                message: e.to_string(),
            })?;

        Ok(())
    }

    async fn list_checkpoints(&self) -> Result<Vec<Checkpoint>> {
        let response = self
            .client
            .scan()
            .table_name(&self.table_name)
            .send()
            .await
            .map_err(|e| KinesisError::Checkpoint {
                message: e.to_string(),
            })?;

        let mut checkpoints = Vec::new();

        for item in response.items() {
            let checkpoint = Checkpoint {
                shard_id: item
                    .get("shard_id")
                    .and_then(|v: &AttributeValue| v.as_s().ok())
                    .unwrap_or(&String::new())
                    .clone(),
                sequence_number: item
                    .get("sequence_number")
                    .and_then(|v: &AttributeValue| v.as_s().ok())
                    .unwrap_or(&String::new())
                    .clone(),
                checkpoint_time: item
                    .get("checkpoint_time")
                    .and_then(|v: &AttributeValue| v.as_s().ok())
                    .and_then(|s: &String| DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt: chrono::DateTime<chrono::FixedOffset>| dt.with_timezone(&Utc))
                    .unwrap_or_else(Utc::now),
                worker_id: item
                    .get("worker_id")
                    .and_then(|v: &AttributeValue| v.as_s().ok())
                    .cloned(),
                lease_expiration: item
                    .get("lease_expiration")
                    .and_then(|v: &AttributeValue| v.as_s().ok())
                    .and_then(|s: &String| DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt: chrono::DateTime<chrono::FixedOffset>| dt.with_timezone(&Utc)),
            };

            checkpoints.push(checkpoint);
        }

        Ok(checkpoints)
    }

    async fn acquire_lease(
        &self,
        shard_id: &str,
        worker_id: &str,
        lease_duration_secs: i64,
    ) -> Result<bool> {
        // Implementation would use conditional updates in DynamoDB
        // For simplicity, this is a basic implementation
        let checkpoint = self.get_checkpoint(shard_id).await?;

        if let Some(mut checkpoint) = checkpoint {
            if checkpoint.is_lease_expired() || checkpoint.worker_id.as_deref() == Some(worker_id) {
                checkpoint.worker_id = Some(worker_id.to_string());
                checkpoint.lease_expiration = Some(
                    Utc::now()
                        + chrono::Duration::try_seconds(lease_duration_secs).unwrap_or_default(),
                );
                self.save_checkpoint(&checkpoint).await?;
                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            let checkpoint = Checkpoint::new(shard_id, "")
                .with_worker_id(worker_id)
                .with_lease_expiration(
                    Utc::now()
                        + chrono::Duration::try_seconds(lease_duration_secs).unwrap_or_default(),
                );
            self.save_checkpoint(&checkpoint).await?;
            Ok(true)
        }
    }

    async fn renew_lease(
        &self,
        shard_id: &str,
        worker_id: &str,
        lease_duration_secs: i64,
    ) -> Result<bool> {
        let checkpoint = self.get_checkpoint(shard_id).await?;

        if let Some(mut checkpoint) = checkpoint {
            if checkpoint.worker_id.as_deref() == Some(worker_id) {
                checkpoint.lease_expiration = Some(
                    Utc::now()
                        + chrono::Duration::try_seconds(lease_duration_secs).unwrap_or_default(),
                );
                self.save_checkpoint(&checkpoint).await?;
                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            Ok(false)
        }
    }

    async fn release_lease(&self, shard_id: &str, worker_id: &str) -> Result<()> {
        let checkpoint = self.get_checkpoint(shard_id).await?;

        if let Some(mut checkpoint) = checkpoint {
            if checkpoint.worker_id.as_deref() == Some(worker_id) {
                checkpoint.worker_id = None;
                checkpoint.lease_expiration = None;
                self.save_checkpoint(&checkpoint).await?;
            }
        }

        Ok(())
    }
}

/// Checkpointer for managing checkpoints
pub struct Checkpointer<S: CheckpointStore> {
    store: Arc<S>,
    worker_id: String,
    lease_duration_secs: i64,
}

impl<S: CheckpointStore> Checkpointer<S> {
    /// Creates a new checkpointer
    pub fn new(store: S, worker_id: impl Into<String>) -> Self {
        Self {
            store: Arc::new(store),
            worker_id: worker_id.into(),
            lease_duration_secs: 60, // Default 60 seconds
        }
    }

    /// Sets the lease duration
    pub fn with_lease_duration_secs(mut self, duration: i64) -> Self {
        self.lease_duration_secs = duration;
        self
    }

    /// Saves a checkpoint
    pub async fn checkpoint(&self, shard_id: &str, sequence_number: &str) -> Result<()> {
        let checkpoint = Checkpoint::new(shard_id, sequence_number).with_worker_id(&self.worker_id);

        self.store.save_checkpoint(&checkpoint).await
    }

    /// Gets the last checkpoint for a shard
    pub async fn get_checkpoint(&self, shard_id: &str) -> Result<Option<Checkpoint>> {
        self.store.get_checkpoint(shard_id).await
    }

    /// Acquires a lease for a shard
    pub async fn acquire_lease(&self, shard_id: &str) -> Result<bool> {
        self.store
            .acquire_lease(shard_id, &self.worker_id, self.lease_duration_secs)
            .await
    }

    /// Renews a lease for a shard
    pub async fn renew_lease(&self, shard_id: &str) -> Result<bool> {
        self.store
            .renew_lease(shard_id, &self.worker_id, self.lease_duration_secs)
            .await
    }

    /// Releases a lease for a shard
    pub async fn release_lease(&self, shard_id: &str) -> Result<()> {
        self.store.release_lease(shard_id, &self.worker_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checkpoint_creation() {
        let checkpoint = Checkpoint::new("shard-0001", "12345");
        assert_eq!(checkpoint.shard_id, "shard-0001");
        assert_eq!(checkpoint.sequence_number, "12345");
        assert!(checkpoint.worker_id.is_none());
    }

    #[test]
    fn test_checkpoint_with_worker() {
        let checkpoint = Checkpoint::new("shard-0001", "12345").with_worker_id("worker-1");
        assert_eq!(checkpoint.worker_id, Some("worker-1".to_string()));
    }

    #[test]
    fn test_checkpoint_lease_expiration() {
        let future_time = Utc::now() + chrono::Duration::try_hours(1).unwrap_or_default();
        let checkpoint = Checkpoint::new("shard-0001", "12345").with_lease_expiration(future_time);
        assert!(!checkpoint.is_lease_expired());

        let past_time = Utc::now() - chrono::Duration::try_hours(1).unwrap_or_default();
        let checkpoint = Checkpoint::new("shard-0001", "12345").with_lease_expiration(past_time);
        assert!(checkpoint.is_lease_expired());
    }

    #[tokio::test]
    async fn test_in_memory_checkpoint_store() {
        let store = InMemoryCheckpointStore::new();
        let checkpoint = Checkpoint::new("shard-0001", "12345");

        store.save_checkpoint(&checkpoint).await.ok();

        let retrieved = store.get_checkpoint("shard-0001").await.ok().flatten();
        assert!(retrieved.is_some());
        assert_eq!(
            retrieved.as_ref().map(|c| c.sequence_number.as_str()),
            Some("12345")
        );

        store.delete_checkpoint("shard-0001").await.ok();
        let retrieved = store.get_checkpoint("shard-0001").await.ok().flatten();
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_lease_acquisition() {
        let store = InMemoryCheckpointStore::new();

        // Acquire lease
        let acquired = store.acquire_lease("shard-0001", "worker-1", 60).await.ok();
        assert_eq!(acquired, Some(true));

        // Try to acquire same lease with different worker
        let acquired = store.acquire_lease("shard-0001", "worker-2", 60).await.ok();
        assert_eq!(acquired, Some(false));

        // Renew lease with same worker
        let renewed = store.renew_lease("shard-0001", "worker-1", 60).await.ok();
        assert_eq!(renewed, Some(true));

        // Release lease
        store.release_lease("shard-0001", "worker-1").await.ok();

        // Now worker-2 should be able to acquire
        let acquired = store.acquire_lease("shard-0001", "worker-2", 60).await.ok();
        assert_eq!(acquired, Some(true));
    }
}
