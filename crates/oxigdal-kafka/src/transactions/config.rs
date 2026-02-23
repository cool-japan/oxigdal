//! Transaction configuration

use crate::config::{Acks, CompressionType, KafkaConfig, KafkaConfigBuilder};
use crate::error::{Error, Result};
use std::time::Duration;

/// Transaction configuration
#[derive(Debug, Clone)]
pub struct TransactionConfig {
    /// Base Kafka configuration
    pub base_config: KafkaConfig,
    /// Transaction ID
    pub transactional_id: String,
    /// Transaction timeout
    pub transaction_timeout: Duration,
    /// Acknowledgment level (must be All for transactions)
    pub acks: Acks,
    /// Enable idempotence (must be true for transactions)
    pub enable_idempotence: bool,
    /// Maximum in-flight requests (must be <= 5 for transactions)
    pub max_in_flight: u32,
    /// Compression type
    pub compression_type: CompressionType,
    /// Request timeout
    pub request_timeout: Duration,
}

impl TransactionConfig {
    /// Create a new transaction configuration builder
    pub fn builder() -> TransactionConfigBuilder {
        TransactionConfigBuilder::default()
    }
}

/// Builder for TransactionConfig
#[derive(Debug, Default)]
pub struct TransactionConfigBuilder {
    kafka_config_builder: KafkaConfigBuilder,
    transactional_id: Option<String>,
    transaction_timeout: Option<Duration>,
    compression_type: Option<CompressionType>,
    request_timeout: Option<Duration>,
}

impl TransactionConfigBuilder {
    /// Set bootstrap servers
    pub fn bootstrap_servers(mut self, servers: impl Into<String>) -> Self {
        self.kafka_config_builder = self.kafka_config_builder.bootstrap_servers(servers);
        self
    }

    /// Set client ID
    pub fn client_id(mut self, id: impl Into<String>) -> Self {
        self.kafka_config_builder = self.kafka_config_builder.client_id(id);
        self
    }

    /// Set transactional ID
    pub fn transactional_id(mut self, id: impl Into<String>) -> Self {
        self.transactional_id = Some(id.into());
        self
    }

    /// Set transaction timeout
    pub fn transaction_timeout(mut self, timeout: Duration) -> Self {
        self.transaction_timeout = Some(timeout);
        self
    }

    /// Set compression type
    pub fn compression_type(mut self, compression: CompressionType) -> Self {
        self.compression_type = Some(compression);
        self
    }

    /// Set request timeout
    pub fn request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = Some(timeout);
        self
    }

    /// Add additional Kafka configuration
    pub fn additional_config(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.kafka_config_builder = self.kafka_config_builder.additional_config(key, value);
        self
    }

    /// Build the transaction configuration
    pub fn build(self) -> Result<TransactionConfig> {
        let base_config = self.kafka_config_builder.build()?;

        let transactional_id = self
            .transactional_id
            .ok_or_else(|| Error::Configuration("transactional_id is required".to_string()))?;

        if transactional_id.is_empty() {
            return Err(Error::Configuration(
                "transactional_id cannot be empty".to_string(),
            ));
        }

        Ok(TransactionConfig {
            base_config,
            transactional_id,
            transaction_timeout: self.transaction_timeout.unwrap_or(Duration::from_secs(60)),
            acks: Acks::All,          // Required for transactions
            enable_idempotence: true, // Required for transactions
            max_in_flight: 5,         // Maximum for transactions
            compression_type: self.compression_type.unwrap_or(CompressionType::None),
            request_timeout: self.request_timeout.unwrap_or(Duration::from_secs(30)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_config_builder() {
        let config = TransactionConfig::builder()
            .bootstrap_servers("localhost:9092")
            .transactional_id("test-txn-1")
            .transaction_timeout(Duration::from_secs(30))
            .build();

        assert!(config.is_ok());
        if let Ok(config) = config {
            assert_eq!(config.transactional_id, "test-txn-1");
            assert_eq!(config.transaction_timeout, Duration::from_secs(30));
            assert_eq!(config.acks, Acks::All);
            assert!(config.enable_idempotence);
        }
    }

    #[test]
    fn test_missing_transactional_id() {
        let config = TransactionConfig::builder()
            .bootstrap_servers("localhost:9092")
            .build();

        assert!(config.is_err());
    }

    #[test]
    fn test_empty_transactional_id() {
        let config = TransactionConfig::builder()
            .bootstrap_servers("localhost:9092")
            .transactional_id("")
            .build();

        assert!(config.is_err());
    }
}
