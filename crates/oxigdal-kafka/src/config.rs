//! Common configuration types for Kafka operations

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Compression type for messages
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionType {
    /// No compression
    None,
    /// Gzip compression
    #[cfg(feature = "compression-gzip")]
    Gzip,
    /// Snappy compression
    #[cfg(feature = "compression-snappy")]
    Snappy,
    /// LZ4 compression
    #[cfg(feature = "compression-lz4")]
    Lz4,
    /// Zstd compression
    #[cfg(feature = "compression-zstd")]
    Zstd,
}

impl CompressionType {
    /// Convert to rdkafka compression string
    pub fn to_rdkafka_str(&self) -> &'static str {
        match self {
            CompressionType::None => "none",
            #[cfg(feature = "compression-gzip")]
            CompressionType::Gzip => "gzip",
            #[cfg(feature = "compression-snappy")]
            CompressionType::Snappy => "snappy",
            #[cfg(feature = "compression-lz4")]
            CompressionType::Lz4 => "lz4",
            #[cfg(feature = "compression-zstd")]
            CompressionType::Zstd => "zstd",
        }
    }
}

/// Acknowledgment level for producer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Acks {
    /// No acknowledgment
    None,
    /// Leader acknowledgment only
    Leader,
    /// All in-sync replicas acknowledgment
    All,
}

impl Acks {
    /// Convert to rdkafka acks string
    pub fn to_rdkafka_str(&self) -> &'static str {
        match self {
            Acks::None => "0",
            Acks::Leader => "1",
            Acks::All => "all",
        }
    }
}

/// Partitioning strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartitionStrategy {
    /// Random partitioning
    Random,
    /// Round-robin partitioning
    RoundRobin,
    /// Hash-based partitioning (using key)
    Hash,
    /// Consistent hash partitioning
    ConsistentHash,
    /// Manual partition assignment
    Manual,
}

/// Security protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecurityProtocol {
    /// Plaintext (no security)
    Plaintext,
    /// SSL/TLS
    Ssl,
    /// SASL plaintext
    SaslPlaintext,
    /// SASL with SSL/TLS
    SaslSsl,
}

impl SecurityProtocol {
    /// Convert to rdkafka security protocol string
    pub fn to_rdkafka_str(&self) -> &'static str {
        match self {
            SecurityProtocol::Plaintext => "plaintext",
            SecurityProtocol::Ssl => "ssl",
            SecurityProtocol::SaslPlaintext => "sasl_plaintext",
            SecurityProtocol::SaslSsl => "sasl_ssl",
        }
    }
}

/// SASL mechanism
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SaslMechanism {
    /// Plain SASL mechanism
    Plain,
    /// SCRAM-SHA-256
    ScramSha256,
    /// SCRAM-SHA-512
    ScramSha512,
    /// GSSAPI (Kerberos)
    Gssapi,
}

impl SaslMechanism {
    /// Convert to rdkafka SASL mechanism string
    pub fn to_rdkafka_str(&self) -> &'static str {
        match self {
            SaslMechanism::Plain => "PLAIN",
            SaslMechanism::ScramSha256 => "SCRAM-SHA-256",
            SaslMechanism::ScramSha512 => "SCRAM-SHA-512",
            SaslMechanism::Gssapi => "GSSAPI",
        }
    }
}

/// Offset reset strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OffsetReset {
    /// Start from earliest offset
    Earliest,
    /// Start from latest offset
    Latest,
    /// Throw error if no offset found
    Error,
}

impl OffsetReset {
    /// Convert to rdkafka offset reset string
    pub fn to_rdkafka_str(&self) -> &'static str {
        match self {
            OffsetReset::Earliest => "earliest",
            OffsetReset::Latest => "latest",
            OffsetReset::Error => "error",
        }
    }
}

/// Isolation level for consumer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IsolationLevel {
    /// Read uncommitted messages
    ReadUncommitted,
    /// Read only committed messages
    ReadCommitted,
}

impl IsolationLevel {
    /// Convert to rdkafka isolation level string
    pub fn to_rdkafka_str(&self) -> &'static str {
        match self {
            IsolationLevel::ReadUncommitted => "read_uncommitted",
            IsolationLevel::ReadCommitted => "read_committed",
        }
    }
}

/// Common Kafka configuration
#[derive(Debug, Clone)]
pub struct KafkaConfig {
    /// Bootstrap servers (comma-separated list)
    pub bootstrap_servers: String,
    /// Client ID
    pub client_id: Option<String>,
    /// Security protocol
    pub security_protocol: SecurityProtocol,
    /// SASL mechanism
    pub sasl_mechanism: Option<SaslMechanism>,
    /// SASL username
    pub sasl_username: Option<String>,
    /// SASL password
    pub sasl_password: Option<String>,
    /// SSL CA certificate path
    pub ssl_ca_location: Option<String>,
    /// SSL certificate path
    pub ssl_certificate_location: Option<String>,
    /// SSL key path
    pub ssl_key_location: Option<String>,
    /// SSL key password
    pub ssl_key_password: Option<String>,
    /// Additional configuration properties
    pub additional_config: HashMap<String, String>,
}

impl KafkaConfig {
    /// Create a new Kafka configuration builder
    pub fn builder() -> KafkaConfigBuilder {
        KafkaConfigBuilder::default()
    }

    /// Apply configuration to rdkafka ClientConfig
    pub fn apply_to_client_config(&self, config: &mut rdkafka::ClientConfig) {
        config.set("bootstrap.servers", &self.bootstrap_servers);

        if let Some(ref client_id) = self.client_id {
            config.set("client.id", client_id);
        }

        config.set("security.protocol", self.security_protocol.to_rdkafka_str());

        if let Some(mechanism) = self.sasl_mechanism {
            config.set("sasl.mechanism", mechanism.to_rdkafka_str());
        }

        if let Some(ref username) = self.sasl_username {
            config.set("sasl.username", username);
        }

        if let Some(ref password) = self.sasl_password {
            config.set("sasl.password", password);
        }

        if let Some(ref ca_location) = self.ssl_ca_location {
            config.set("ssl.ca.location", ca_location);
        }

        if let Some(ref cert_location) = self.ssl_certificate_location {
            config.set("ssl.certificate.location", cert_location);
        }

        if let Some(ref key_location) = self.ssl_key_location {
            config.set("ssl.key.location", key_location);
        }

        if let Some(ref key_password) = self.ssl_key_password {
            config.set("ssl.key.password", key_password);
        }

        for (key, value) in &self.additional_config {
            config.set(key, value);
        }
    }
}

/// Builder for KafkaConfig
#[derive(Debug, Default)]
pub struct KafkaConfigBuilder {
    bootstrap_servers: Option<String>,
    client_id: Option<String>,
    security_protocol: Option<SecurityProtocol>,
    sasl_mechanism: Option<SaslMechanism>,
    sasl_username: Option<String>,
    sasl_password: Option<String>,
    ssl_ca_location: Option<String>,
    ssl_certificate_location: Option<String>,
    ssl_key_location: Option<String>,
    ssl_key_password: Option<String>,
    additional_config: HashMap<String, String>,
}

impl KafkaConfigBuilder {
    /// Set bootstrap servers
    pub fn bootstrap_servers(mut self, servers: impl Into<String>) -> Self {
        self.bootstrap_servers = Some(servers.into());
        self
    }

    /// Set client ID
    pub fn client_id(mut self, id: impl Into<String>) -> Self {
        self.client_id = Some(id.into());
        self
    }

    /// Set security protocol
    pub fn security_protocol(mut self, protocol: SecurityProtocol) -> Self {
        self.security_protocol = Some(protocol);
        self
    }

    /// Set SASL mechanism
    pub fn sasl_mechanism(mut self, mechanism: SaslMechanism) -> Self {
        self.sasl_mechanism = Some(mechanism);
        self
    }

    /// Set SASL username
    pub fn sasl_username(mut self, username: impl Into<String>) -> Self {
        self.sasl_username = Some(username.into());
        self
    }

    /// Set SASL password
    pub fn sasl_password(mut self, password: impl Into<String>) -> Self {
        self.sasl_password = Some(password.into());
        self
    }

    /// Set SSL CA certificate location
    pub fn ssl_ca_location(mut self, location: impl Into<String>) -> Self {
        self.ssl_ca_location = Some(location.into());
        self
    }

    /// Set SSL certificate location
    pub fn ssl_certificate_location(mut self, location: impl Into<String>) -> Self {
        self.ssl_certificate_location = Some(location.into());
        self
    }

    /// Set SSL key location
    pub fn ssl_key_location(mut self, location: impl Into<String>) -> Self {
        self.ssl_key_location = Some(location.into());
        self
    }

    /// Set SSL key password
    pub fn ssl_key_password(mut self, password: impl Into<String>) -> Self {
        self.ssl_key_password = Some(password.into());
        self
    }

    /// Add additional configuration property
    pub fn additional_config(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.additional_config.insert(key.into(), value.into());
        self
    }

    /// Build the configuration
    pub fn build(self) -> Result<KafkaConfig> {
        let bootstrap_servers = self
            .bootstrap_servers
            .ok_or_else(|| Error::Configuration("bootstrap_servers is required".to_string()))?;

        Ok(KafkaConfig {
            bootstrap_servers,
            client_id: self.client_id,
            security_protocol: self
                .security_protocol
                .unwrap_or(SecurityProtocol::Plaintext),
            sasl_mechanism: self.sasl_mechanism,
            sasl_username: self.sasl_username,
            sasl_password: self.sasl_password,
            ssl_ca_location: self.ssl_ca_location,
            ssl_certificate_location: self.ssl_certificate_location,
            ssl_key_location: self.ssl_key_location,
            ssl_key_password: self.ssl_key_password,
            additional_config: self.additional_config,
        })
    }
}

/// Retry configuration
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retries
    pub max_retries: usize,
    /// Initial retry backoff
    pub initial_backoff: Duration,
    /// Maximum retry backoff
    pub max_backoff: Duration,
    /// Backoff multiplier
    pub backoff_multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(30),
            backoff_multiplier: 2.0,
        }
    }
}

impl RetryConfig {
    /// Calculate backoff duration for a given attempt
    pub fn backoff_duration(&self, attempt: usize) -> Duration {
        let backoff_ms = (self.initial_backoff.as_millis() as f64
            * self.backoff_multiplier.powi(attempt as i32)) as u64;
        let backoff = Duration::from_millis(backoff_ms);
        backoff.min(self.max_backoff)
    }
}
