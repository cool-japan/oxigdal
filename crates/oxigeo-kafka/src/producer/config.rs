//! Producer configuration

use crate::config::{Acks, CompressionType, KafkaConfig, KafkaConfigBuilder};
use crate::error::{Error, Result};
use crate::producer::partitioner::PartitionerType;

/// Producer configuration
#[derive(Debug, Clone)]
pub struct ProducerConfig {
    /// Base Kafka configuration
    pub base_config: KafkaConfig,
    /// Acknowledgment level
    pub acks: Acks,
    /// Compression type
    pub compression_type: CompressionType,
    /// Maximum in-flight requests per connection
    pub max_in_flight: u32,
    /// Linger time in milliseconds (batching delay)
    pub linger_ms: u32,
    /// Maximum batch size in bytes
    pub batch_size: usize,
    /// Maximum number of messages in a batch
    pub batch_num_messages: usize,
    /// Request timeout in milliseconds
    pub request_timeout_ms: u32,
    /// Message timeout in milliseconds
    pub message_timeout_ms: u32,
    /// Enable idempotent producer
    pub enable_idempotence: bool,
    /// Maximum number of retries
    pub max_retries: Option<u32>,
    /// Retry backoff in milliseconds
    pub retry_backoff_ms: Option<u32>,
    /// Partitioner type
    pub partitioner_type: PartitionerType,
    /// Enable batching
    pub enable_batching: bool,
    /// Maximum concurrent sends
    pub max_concurrent_sends: usize,
}

impl ProducerConfig {
    /// Create a new producer configuration builder
    pub fn builder() -> ProducerConfigBuilder {
        ProducerConfigBuilder::default()
    }
}

/// Builder for ProducerConfig
#[derive(Debug, Default)]
pub struct ProducerConfigBuilder {
    kafka_config_builder: KafkaConfigBuilder,
    acks: Option<Acks>,
    compression_type: Option<CompressionType>,
    max_in_flight: Option<u32>,
    linger_ms: Option<u32>,
    batch_size: Option<usize>,
    batch_num_messages: Option<usize>,
    request_timeout_ms: Option<u32>,
    message_timeout_ms: Option<u32>,
    enable_idempotence: Option<bool>,
    max_retries: Option<u32>,
    retry_backoff_ms: Option<u32>,
    partitioner_type: Option<PartitionerType>,
    enable_batching: Option<bool>,
    max_concurrent_sends: Option<usize>,
}

impl ProducerConfigBuilder {
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

    /// Set acknowledgment level
    pub fn acks(mut self, acks: Acks) -> Self {
        self.acks = Some(acks);
        self
    }

    /// Set compression type
    pub fn compression_type(mut self, compression: CompressionType) -> Self {
        self.compression_type = Some(compression);
        self
    }

    /// Set maximum in-flight requests per connection
    pub fn max_in_flight(mut self, max: u32) -> Self {
        self.max_in_flight = Some(max);
        self
    }

    /// Set linger time in milliseconds
    pub fn linger_ms(mut self, ms: u32) -> Self {
        self.linger_ms = Some(ms);
        self
    }

    /// Set maximum batch size in bytes
    pub fn batch_size(mut self, size: usize) -> Self {
        self.batch_size = Some(size);
        self
    }

    /// Set maximum number of messages in a batch
    pub fn batch_num_messages(mut self, num: usize) -> Self {
        self.batch_num_messages = Some(num);
        self
    }

    /// Set request timeout in milliseconds
    pub fn request_timeout_ms(mut self, ms: u32) -> Self {
        self.request_timeout_ms = Some(ms);
        self
    }

    /// Set message timeout in milliseconds
    pub fn message_timeout_ms(mut self, ms: u32) -> Self {
        self.message_timeout_ms = Some(ms);
        self
    }

    /// Enable idempotent producer
    pub fn enable_idempotence(mut self, enable: bool) -> Self {
        self.enable_idempotence = Some(enable);
        self
    }

    /// Set maximum number of retries
    pub fn max_retries(mut self, retries: u32) -> Self {
        self.max_retries = Some(retries);
        self
    }

    /// Set retry backoff in milliseconds
    pub fn retry_backoff_ms(mut self, ms: u32) -> Self {
        self.retry_backoff_ms = Some(ms);
        self
    }

    /// Set partitioner type
    pub fn partitioner_type(mut self, partitioner: PartitionerType) -> Self {
        self.partitioner_type = Some(partitioner);
        self
    }

    /// Enable batching
    pub fn enable_batching(mut self, enable: bool) -> Self {
        self.enable_batching = Some(enable);
        self
    }

    /// Set maximum concurrent sends
    pub fn max_concurrent_sends(mut self, max: usize) -> Self {
        self.max_concurrent_sends = Some(max);
        self
    }

    /// Add additional Kafka configuration
    pub fn additional_config(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.kafka_config_builder = self.kafka_config_builder.additional_config(key, value);
        self
    }

    /// Build the producer configuration
    pub fn build(self) -> Result<ProducerConfig> {
        let base_config = self.kafka_config_builder.build()?;

        let enable_idempotence = self.enable_idempotence.unwrap_or(true);

        // Validate idempotence requirements
        if enable_idempotence {
            let max_in_flight = self.max_in_flight.unwrap_or(5);
            if max_in_flight > 5 {
                return Err(Error::Configuration(
                    "max_in_flight must be <= 5 when idempotence is enabled".to_string(),
                ));
            }

            let acks = self.acks.unwrap_or(Acks::All);
            if acks != Acks::All {
                return Err(Error::Configuration(
                    "acks must be 'all' when idempotence is enabled".to_string(),
                ));
            }
        }

        Ok(ProducerConfig {
            base_config,
            acks: self.acks.unwrap_or(Acks::All),
            compression_type: self.compression_type.unwrap_or(CompressionType::None),
            max_in_flight: self.max_in_flight.unwrap_or(5),
            linger_ms: self.linger_ms.unwrap_or(0),
            batch_size: self.batch_size.unwrap_or(16384),
            batch_num_messages: self.batch_num_messages.unwrap_or(1000),
            request_timeout_ms: self.request_timeout_ms.unwrap_or(30000),
            message_timeout_ms: self.message_timeout_ms.unwrap_or(300000),
            enable_idempotence,
            max_retries: self.max_retries,
            retry_backoff_ms: self.retry_backoff_ms,
            partitioner_type: self.partitioner_type.unwrap_or(PartitionerType::Hash),
            enable_batching: self.enable_batching.unwrap_or(true),
            max_concurrent_sends: self.max_concurrent_sends.unwrap_or(100),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_producer_config_builder() {
        let config = ProducerConfig::builder()
            .bootstrap_servers("localhost:9092")
            .acks(Acks::All)
            .compression_type(CompressionType::None)
            .build();

        assert!(config.is_ok(), "Expected config to build successfully");
        if let Ok(config) = config {
            assert_eq!(config.acks, Acks::All);
            assert_eq!(config.compression_type, CompressionType::None);
        }
    }

    #[test]
    fn test_idempotence_validation() {
        // Idempotence requires acks=all
        let config = ProducerConfig::builder()
            .bootstrap_servers("localhost:9092")
            .enable_idempotence(true)
            .acks(Acks::Leader)
            .build();

        assert!(config.is_err());

        // Idempotence requires max_in_flight <= 5
        let config = ProducerConfig::builder()
            .bootstrap_servers("localhost:9092")
            .enable_idempotence(true)
            .max_in_flight(10)
            .build();

        assert!(config.is_err());

        // Valid idempotent configuration
        let config = ProducerConfig::builder()
            .bootstrap_servers("localhost:9092")
            .enable_idempotence(true)
            .acks(Acks::All)
            .max_in_flight(5)
            .build();

        assert!(config.is_ok());
    }
}
