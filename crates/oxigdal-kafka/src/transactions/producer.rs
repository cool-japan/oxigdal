//! Transactional producer implementation

use crate::error::{Error, Result};
use crate::transactions::{
    TransactionConfig, TransactionCoordinator, TransactionMetadata, TransactionState,
};
use crate::utils;

use rdkafka::ClientConfig;
use rdkafka::producer::{FutureProducer, FutureRecord, Producer};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Transactional Kafka producer
pub struct TransactionalProducer {
    /// Internal rdkafka producer
    producer: Arc<FutureProducer>,
    /// Transaction configuration
    config: TransactionConfig,
    /// Transaction coordinator
    coordinator: Arc<RwLock<TransactionCoordinator>>,
    /// Current transaction metadata
    current_transaction: Arc<RwLock<Option<TransactionMetadata>>>,
}

impl TransactionalProducer {
    /// Create a new transactional producer
    pub async fn new(config: TransactionConfig) -> Result<Self> {
        info!(
            "Creating transactional Kafka producer with ID: {}",
            config.transactional_id
        );

        let mut client_config = ClientConfig::new();
        config
            .base_config
            .apply_to_client_config(&mut client_config);

        // Transaction-specific configuration
        client_config.set("transactional.id", &config.transactional_id);
        client_config.set("enable.idempotence", "true");
        client_config.set("acks", "all");
        client_config.set("max.in.flight.requests.per.connection", "5");
        client_config.set(
            "transaction.timeout.ms",
            config.transaction_timeout.as_millis().to_string(),
        );

        let producer: FutureProducer = client_config
            .create()
            .map_err(|e| Error::Configuration(format!("Failed to create producer: {}", e)))?;

        // Initialize transactions
        producer
            .init_transactions(config.transaction_timeout)
            .map_err(|e| Error::Transaction(format!("Failed to initialize transactions: {}", e)))?;

        let coordinator = Arc::new(RwLock::new(TransactionCoordinator::new(
            config.transactional_id.clone(),
        )));

        info!("Transactional producer created and initialized");

        Ok(Self {
            producer: Arc::new(producer),
            config,
            coordinator,
            current_transaction: Arc::new(RwLock::new(None)),
        })
    }

    /// Begin a new transaction
    pub async fn begin_transaction(&self) -> Result<()> {
        let mut current = self.current_transaction.write().await;
        if current.is_some() {
            return Err(Error::Transaction(
                "Transaction already in progress".to_string(),
            ));
        }

        self.producer
            .begin_transaction()
            .map_err(|e| Error::Transaction(format!("Failed to begin transaction: {}", e)))?;

        let metadata = TransactionMetadata::new(
            self.config.transactional_id.clone(),
            self.config.transaction_timeout,
        );

        *current = Some(metadata.clone());

        let mut coordinator = self.coordinator.write().await;
        coordinator.begin_transaction(metadata);

        info!("Transaction begun: {}", self.config.transactional_id);

        Ok(())
    }

    /// Send a message within a transaction
    pub async fn send(&self, topic: &str, key: Option<&[u8]>, value: &[u8]) -> Result<(i32, i64)> {
        // Check if transaction is in progress
        let current = self.current_transaction.read().await;
        if current.is_none() {
            return Err(Error::Transaction(
                "No transaction in progress. Call begin_transaction() first.".to_string(),
            ));
        }

        // Check if transaction is expired
        if let Some(ref metadata) = *current {
            if metadata.is_expired() {
                drop(current);
                self.abort_transaction().await?;
                return Err(Error::Timeout("Transaction timeout".to_string()));
            }
        }
        drop(current);

        utils::validate_topic_name(topic)?;

        let mut record = FutureRecord::to(topic).payload(value);

        if let Some(key) = key {
            record = record.key(key);
        }

        let delivery_status = self
            .producer
            .send(record, self.config.request_timeout)
            .await
            .map_err(|(err, _)| {
                Error::Producer(format!("Failed to send message in transaction: {}", err))
            })?;

        debug!(
            "Message sent in transaction to topic {} partition {} offset {}",
            topic, delivery_status.partition, delivery_status.offset
        );

        Ok((delivery_status.partition, delivery_status.offset))
    }

    /// Commit the current transaction
    pub async fn commit_transaction(&self) -> Result<()> {
        let mut current = self.current_transaction.write().await;
        if current.is_none() {
            return Err(Error::Transaction("No transaction in progress".to_string()));
        }

        self.producer
            .commit_transaction(self.config.request_timeout)
            .map_err(|e| Error::Transaction(format!("Failed to commit transaction: {}", e)))?;

        *current = None;

        let mut coordinator = self.coordinator.write().await;
        coordinator.commit_transaction()?;

        info!("Transaction committed: {}", self.config.transactional_id);

        Ok(())
    }

    /// Abort the current transaction
    pub async fn abort_transaction(&self) -> Result<()> {
        let mut current = self.current_transaction.write().await;
        if current.is_none() {
            return Err(Error::Transaction("No transaction in progress".to_string()));
        }

        self.producer
            .abort_transaction(self.config.request_timeout)
            .map_err(|e| {
                Error::TransactionAborted(format!("Failed to abort transaction: {}", e))
            })?;

        *current = None;

        let mut coordinator = self.coordinator.write().await;
        coordinator.abort_transaction();

        warn!("Transaction aborted: {}", self.config.transactional_id);

        Ok(())
    }

    /// Get current transaction state
    pub async fn transaction_state(&self) -> TransactionState {
        let coordinator = self.coordinator.read().await;
        coordinator.state()
    }

    /// Check if transaction is in progress
    pub async fn is_in_transaction(&self) -> bool {
        self.current_transaction.read().await.is_some()
    }

    /// Get transaction configuration
    pub fn config(&self) -> &TransactionConfig {
        &self.config
    }
}

/// Transaction scope for automatic commit/abort
pub struct TransactionScope<'a> {
    producer: &'a TransactionalProducer,
    committed: bool,
}

impl<'a> TransactionScope<'a> {
    /// Create a new transaction scope
    pub async fn new(producer: &'a TransactionalProducer) -> Result<Self> {
        producer.begin_transaction().await?;
        Ok(Self {
            producer,
            committed: false,
        })
    }

    /// Commit the transaction
    pub async fn commit(mut self) -> Result<()> {
        self.producer.commit_transaction().await?;
        self.committed = true;
        Ok(())
    }

    /// Send a message within the transaction
    pub async fn send(&self, topic: &str, key: Option<&[u8]>, value: &[u8]) -> Result<(i32, i64)> {
        self.producer.send(topic, key, value).await
    }
}

impl<'a> Drop for TransactionScope<'a> {
    fn drop(&mut self) {
        if !self.committed {
            // Note: Cannot abort asynchronously in drop.
            // The transaction will be auto-aborted by Kafka when the transaction timeout expires.
            // For explicit abort, use commit() or call abort_transaction() on the producer directly.
            warn!(
                "TransactionScope dropped without commit - transaction will timeout and be aborted"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_transaction_config() {
        let config = TransactionConfig::builder()
            .bootstrap_servers("localhost:9092")
            .transactional_id("test-txn")
            .build();

        assert!(config.is_ok());
    }
}
