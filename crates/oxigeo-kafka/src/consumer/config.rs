//! Consumer configuration

use crate::config::{IsolationLevel, KafkaConfig, KafkaConfigBuilder, OffsetReset};
use crate::consumer::offset::CommitStrategy;
use crate::error::{Error, Result};

/// Consumer configuration
#[derive(Debug, Clone)]
pub struct ConsumerConfig {
    /// Base Kafka configuration
    pub base_config: KafkaConfig,
    /// Consumer group ID
    pub group_id: String,
    /// Enable auto commit
    pub enable_auto_commit: bool,
    /// Auto commit interval in milliseconds
    pub auto_commit_interval_ms: u32,
    /// Auto offset reset strategy
    pub auto_offset_reset: OffsetReset,
    /// Isolation level
    pub isolation_level: IsolationLevel,
    /// Maximum poll interval in milliseconds
    pub max_poll_interval_ms: u32,
    /// Session timeout in milliseconds
    pub session_timeout_ms: u32,
    /// Heartbeat interval in milliseconds
    pub heartbeat_interval_ms: u32,
    /// Minimum bytes to fetch
    pub fetch_min_bytes: usize,
    /// Maximum bytes to fetch
    pub fetch_max_bytes: usize,
    /// Maximum fetch wait time in milliseconds
    pub fetch_wait_max_ms: u32,
    /// Maximum bytes per partition
    pub max_partition_fetch_bytes: Option<usize>,
    /// Commit strategy
    pub commit_strategy: CommitStrategy,
    /// Maximum concurrent message processing
    pub max_concurrent_messages: usize,
}

impl ConsumerConfig {
    /// Create a new consumer configuration builder
    pub fn builder() -> ConsumerConfigBuilder {
        ConsumerConfigBuilder::default()
    }
}

/// Builder for ConsumerConfig
#[derive(Debug, Default)]
pub struct ConsumerConfigBuilder {
    kafka_config_builder: KafkaConfigBuilder,
    group_id: Option<String>,
    enable_auto_commit: Option<bool>,
    auto_commit_interval_ms: Option<u32>,
    auto_offset_reset: Option<OffsetReset>,
    isolation_level: Option<IsolationLevel>,
    max_poll_interval_ms: Option<u32>,
    session_timeout_ms: Option<u32>,
    heartbeat_interval_ms: Option<u32>,
    fetch_min_bytes: Option<usize>,
    fetch_max_bytes: Option<usize>,
    fetch_wait_max_ms: Option<u32>,
    max_partition_fetch_bytes: Option<usize>,
    commit_strategy: Option<CommitStrategy>,
    max_concurrent_messages: Option<usize>,
}

impl ConsumerConfigBuilder {
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

    /// Set consumer group ID
    pub fn group_id(mut self, group: impl Into<String>) -> Self {
        self.group_id = Some(group.into());
        self
    }

    /// Enable auto commit
    pub fn enable_auto_commit(mut self, enable: bool) -> Self {
        self.enable_auto_commit = Some(enable);
        self
    }

    /// Set auto commit interval in milliseconds
    pub fn auto_commit_interval_ms(mut self, ms: u32) -> Self {
        self.auto_commit_interval_ms = Some(ms);
        self
    }

    /// Set auto offset reset strategy
    pub fn auto_offset_reset(mut self, reset: OffsetReset) -> Self {
        self.auto_offset_reset = Some(reset);
        self
    }

    /// Set isolation level
    pub fn isolation_level(mut self, level: IsolationLevel) -> Self {
        self.isolation_level = Some(level);
        self
    }

    /// Set maximum poll interval in milliseconds
    pub fn max_poll_interval_ms(mut self, ms: u32) -> Self {
        self.max_poll_interval_ms = Some(ms);
        self
    }

    /// Set session timeout in milliseconds
    pub fn session_timeout_ms(mut self, ms: u32) -> Self {
        self.session_timeout_ms = Some(ms);
        self
    }

    /// Set heartbeat interval in milliseconds
    pub fn heartbeat_interval_ms(mut self, ms: u32) -> Self {
        self.heartbeat_interval_ms = Some(ms);
        self
    }

    /// Set minimum bytes to fetch
    pub fn fetch_min_bytes(mut self, bytes: usize) -> Self {
        self.fetch_min_bytes = Some(bytes);
        self
    }

    /// Set maximum bytes to fetch
    pub fn fetch_max_bytes(mut self, bytes: usize) -> Self {
        self.fetch_max_bytes = Some(bytes);
        self
    }

    /// Set maximum fetch wait time in milliseconds
    pub fn fetch_wait_max_ms(mut self, ms: u32) -> Self {
        self.fetch_wait_max_ms = Some(ms);
        self
    }

    /// Set maximum bytes per partition
    pub fn max_partition_fetch_bytes(mut self, bytes: usize) -> Self {
        self.max_partition_fetch_bytes = Some(bytes);
        self
    }

    /// Set commit strategy
    pub fn commit_strategy(mut self, strategy: CommitStrategy) -> Self {
        self.commit_strategy = Some(strategy);
        self
    }

    /// Set maximum concurrent message processing
    pub fn max_concurrent_messages(mut self, max: usize) -> Self {
        self.max_concurrent_messages = Some(max);
        self
    }

    /// Add additional Kafka configuration
    pub fn additional_config(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.kafka_config_builder = self.kafka_config_builder.additional_config(key, value);
        self
    }

    /// Build the consumer configuration
    pub fn build(self) -> Result<ConsumerConfig> {
        let base_config = self.kafka_config_builder.build()?;

        let group_id = self
            .group_id
            .ok_or_else(|| Error::Configuration("group_id is required".to_string()))?;

        let heartbeat_interval_ms = self.heartbeat_interval_ms.unwrap_or(3000);
        let session_timeout_ms = self.session_timeout_ms.unwrap_or(10000);

        // Validate heartbeat interval vs session timeout
        if heartbeat_interval_ms >= session_timeout_ms {
            return Err(Error::Configuration(
                "heartbeat_interval_ms must be less than session_timeout_ms".to_string(),
            ));
        }

        Ok(ConsumerConfig {
            base_config,
            group_id,
            enable_auto_commit: self.enable_auto_commit.unwrap_or(false),
            auto_commit_interval_ms: self.auto_commit_interval_ms.unwrap_or(5000),
            auto_offset_reset: self.auto_offset_reset.unwrap_or(OffsetReset::Latest),
            isolation_level: self
                .isolation_level
                .unwrap_or(IsolationLevel::ReadCommitted),
            max_poll_interval_ms: self.max_poll_interval_ms.unwrap_or(300000),
            session_timeout_ms,
            heartbeat_interval_ms,
            fetch_min_bytes: self.fetch_min_bytes.unwrap_or(1),
            fetch_max_bytes: self.fetch_max_bytes.unwrap_or(52428800), // 50 MB
            fetch_wait_max_ms: self.fetch_wait_max_ms.unwrap_or(500),
            max_partition_fetch_bytes: self.max_partition_fetch_bytes,
            commit_strategy: self.commit_strategy.unwrap_or(CommitStrategy::Manual),
            max_concurrent_messages: self.max_concurrent_messages.unwrap_or(100),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consumer_config_builder() {
        let config = ConsumerConfig::builder()
            .bootstrap_servers("localhost:9092")
            .group_id("test-group")
            .auto_offset_reset(OffsetReset::Earliest)
            .build();

        assert!(config.is_ok(), "Expected config to build successfully");
        if let Ok(config) = config {
            assert_eq!(config.group_id, "test-group");
            assert_eq!(config.auto_offset_reset, OffsetReset::Earliest);
        }
    }

    #[test]
    fn test_heartbeat_validation() {
        // Heartbeat interval must be less than session timeout
        let config = ConsumerConfig::builder()
            .bootstrap_servers("localhost:9092")
            .group_id("test-group")
            .heartbeat_interval_ms(10000)
            .session_timeout_ms(5000)
            .build();

        assert!(config.is_err());

        // Valid configuration
        let config = ConsumerConfig::builder()
            .bootstrap_servers("localhost:9092")
            .group_id("test-group")
            .heartbeat_interval_ms(3000)
            .session_timeout_ms(10000)
            .build();

        assert!(config.is_ok());
    }

    #[test]
    fn test_missing_group_id() {
        let config = ConsumerConfig::builder()
            .bootstrap_servers("localhost:9092")
            .build();

        assert!(config.is_err());
    }
}
