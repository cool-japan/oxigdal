//! Destination configurations for Firehose delivery streams

use serde::{Deserialize, Serialize};

/// S3 destination configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3DestinationConfig {
    /// S3 bucket ARN
    pub bucket_arn: String,
    /// IAM role ARN
    pub role_arn: String,
    /// Key prefix
    pub prefix: String,
    /// Error output prefix
    pub error_output_prefix: Option<String>,
    /// Buffer size in MB
    pub buffer_size_mb: i32,
    /// Buffer interval in seconds
    pub buffer_interval_seconds: i32,
    /// Compression format
    pub compression: S3CompressionFormat,
    /// Encryption configuration
    pub encryption: Option<S3EncryptionConfig>,
}

impl S3DestinationConfig {
    /// Creates a new S3 destination configuration
    pub fn new(
        bucket_arn: impl Into<String>,
        role_arn: impl Into<String>,
        prefix: impl Into<String>,
    ) -> Self {
        Self {
            bucket_arn: bucket_arn.into(),
            role_arn: role_arn.into(),
            prefix: prefix.into(),
            error_output_prefix: None,
            buffer_size_mb: 5,
            buffer_interval_seconds: 300,
            compression: S3CompressionFormat::None,
            encryption: None,
        }
    }

    /// Sets the error output prefix
    pub fn with_error_output_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.error_output_prefix = Some(prefix.into());
        self
    }

    /// Sets the buffer size
    pub fn with_buffer_size_mb(mut self, size_mb: i32) -> Self {
        self.buffer_size_mb = size_mb;
        self
    }

    /// Sets the buffer interval
    pub fn with_buffer_interval_seconds(mut self, seconds: i32) -> Self {
        self.buffer_interval_seconds = seconds;
        self
    }

    /// Sets the compression format
    pub fn with_compression(mut self, compression: S3CompressionFormat) -> Self {
        self.compression = compression;
        self
    }

    /// Sets the encryption configuration
    pub fn with_encryption(mut self, encryption: S3EncryptionConfig) -> Self {
        self.encryption = Some(encryption);
        self
    }
}

/// S3 compression format
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum S3CompressionFormat {
    /// No compression
    None,
    /// Gzip compression
    Gzip,
    /// Snappy compression
    Snappy,
    /// Zip compression
    Zip,
}

/// S3 encryption configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3EncryptionConfig {
    /// Encryption type
    pub encryption_type: S3EncryptionType,
    /// KMS key ARN (for KMS encryption)
    pub kms_key_arn: Option<String>,
}

impl S3EncryptionConfig {
    /// Creates a new S3 encryption configuration with SSE-S3
    pub fn sse_s3() -> Self {
        Self {
            encryption_type: S3EncryptionType::SseS3,
            kms_key_arn: None,
        }
    }

    /// Creates a new S3 encryption configuration with SSE-KMS
    pub fn sse_kms(kms_key_arn: impl Into<String>) -> Self {
        Self {
            encryption_type: S3EncryptionType::SseKms,
            kms_key_arn: Some(kms_key_arn.into()),
        }
    }
}

/// S3 encryption type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum S3EncryptionType {
    /// No encryption
    None,
    /// SSE-S3 (AES-256)
    SseS3,
    /// SSE-KMS
    SseKms,
}

/// S3 destination
pub struct S3Destination {
    config: S3DestinationConfig,
}

impl S3Destination {
    /// Creates a new S3 destination
    pub fn new(config: S3DestinationConfig) -> Self {
        Self { config }
    }

    /// Gets the destination configuration
    pub fn config(&self) -> &S3DestinationConfig {
        &self.config
    }

    /// Validates the configuration
    pub fn validate(&self) -> crate::error::Result<()> {
        if self.config.bucket_arn.is_empty() {
            return Err(crate::error::KinesisError::InvalidConfig {
                message: "Bucket ARN is required".to_string(),
            });
        }

        if self.config.role_arn.is_empty() {
            return Err(crate::error::KinesisError::InvalidConfig {
                message: "Role ARN is required".to_string(),
            });
        }

        if self.config.buffer_size_mb < 1 || self.config.buffer_size_mb > 128 {
            return Err(crate::error::KinesisError::InvalidConfig {
                message: "Buffer size must be between 1 and 128 MB".to_string(),
            });
        }

        if self.config.buffer_interval_seconds < 60 || self.config.buffer_interval_seconds > 900 {
            return Err(crate::error::KinesisError::InvalidConfig {
                message: "Buffer interval must be between 60 and 900 seconds".to_string(),
            });
        }

        Ok(())
    }
}

/// Redshift destination configuration (for future implementation)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedshiftDestinationConfig {
    /// Cluster JDBC URL
    pub cluster_jdbc_url: String,
    /// Database name
    pub database_name: String,
    /// Table name
    pub table_name: String,
    /// Username
    pub username: String,
    /// IAM role ARN
    pub role_arn: String,
    /// S3 configuration for intermediate data
    pub s3_config: S3DestinationConfig,
    /// COPY options
    pub copy_options: Option<String>,
}

impl RedshiftDestinationConfig {
    /// Creates a new Redshift destination configuration
    pub fn new(
        cluster_jdbc_url: impl Into<String>,
        database_name: impl Into<String>,
        table_name: impl Into<String>,
        username: impl Into<String>,
        role_arn: impl Into<String>,
        s3_config: S3DestinationConfig,
    ) -> Self {
        Self {
            cluster_jdbc_url: cluster_jdbc_url.into(),
            database_name: database_name.into(),
            table_name: table_name.into(),
            username: username.into(),
            role_arn: role_arn.into(),
            s3_config,
            copy_options: None,
        }
    }

    /// Sets the COPY options
    pub fn with_copy_options(mut self, options: impl Into<String>) -> Self {
        self.copy_options = Some(options.into());
        self
    }
}

/// Elasticsearch destination configuration (for future implementation)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElasticsearchDestinationConfig {
    /// Elasticsearch domain endpoint
    pub domain_endpoint: String,
    /// Index name
    pub index_name: String,
    /// Type name
    pub type_name: String,
    /// IAM role ARN
    pub role_arn: String,
    /// Index rotation period
    pub index_rotation_period: Option<IndexRotationPeriod>,
    /// Buffering hints
    pub buffer_size_mb: i32,
    /// Buffer interval in seconds
    pub buffer_interval_seconds: i32,
}

/// Index rotation period
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexRotationPeriod {
    /// No rotation
    NoRotation,
    /// Rotate every hour
    OneHour,
    /// Rotate every day
    OneDay,
    /// Rotate every week
    OneWeek,
    /// Rotate every month
    OneMonth,
}

impl ElasticsearchDestinationConfig {
    /// Creates a new Elasticsearch destination configuration
    pub fn new(
        domain_endpoint: impl Into<String>,
        index_name: impl Into<String>,
        type_name: impl Into<String>,
        role_arn: impl Into<String>,
    ) -> Self {
        Self {
            domain_endpoint: domain_endpoint.into(),
            index_name: index_name.into(),
            type_name: type_name.into(),
            role_arn: role_arn.into(),
            index_rotation_period: None,
            buffer_size_mb: 5,
            buffer_interval_seconds: 300,
        }
    }

    /// Sets the index rotation period
    pub fn with_index_rotation(mut self, period: IndexRotationPeriod) -> Self {
        self.index_rotation_period = Some(period);
        self
    }

    /// Sets the buffer size
    pub fn with_buffer_size_mb(mut self, size_mb: i32) -> Self {
        self.buffer_size_mb = size_mb;
        self
    }

    /// Sets the buffer interval
    pub fn with_buffer_interval_seconds(mut self, seconds: i32) -> Self {
        self.buffer_interval_seconds = seconds;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s3_destination_config() {
        let config = S3DestinationConfig::new(
            "arn:aws:s3:::my-bucket",
            "arn:aws:iam::123456789012:role/firehose-role",
            "data/",
        )
        .with_buffer_size_mb(10)
        .with_compression(S3CompressionFormat::Gzip);

        assert_eq!(config.bucket_arn, "arn:aws:s3:::my-bucket");
        assert_eq!(config.buffer_size_mb, 10);
        assert_eq!(config.compression, S3CompressionFormat::Gzip);
    }

    #[test]
    fn test_s3_encryption_sse_s3() {
        let encryption = S3EncryptionConfig::sse_s3();
        assert_eq!(encryption.encryption_type, S3EncryptionType::SseS3);
        assert!(encryption.kms_key_arn.is_none());
    }

    #[test]
    fn test_s3_encryption_sse_kms() {
        let encryption = S3EncryptionConfig::sse_kms(
            "arn:aws:kms:us-east-1:123456789012:key/12345678-1234-1234-1234-123456789012",
        );
        assert_eq!(encryption.encryption_type, S3EncryptionType::SseKms);
        assert!(encryption.kms_key_arn.is_some());
    }

    #[test]
    fn test_s3_destination_validation() {
        let config = S3DestinationConfig::new(
            "arn:aws:s3:::my-bucket",
            "arn:aws:iam::123456789012:role/firehose-role",
            "data/",
        );
        let destination = S3Destination::new(config);
        assert!(destination.validate().is_ok());

        // Test invalid buffer size
        let mut config = S3DestinationConfig::new(
            "arn:aws:s3:::my-bucket",
            "arn:aws:iam::123456789012:role/firehose-role",
            "data/",
        );
        config.buffer_size_mb = 200;
        let destination = S3Destination::new(config);
        assert!(destination.validate().is_err());
    }

    #[test]
    fn test_redshift_destination_config() {
        let s3_config = S3DestinationConfig::new(
            "arn:aws:s3:::my-bucket",
            "arn:aws:iam::123456789012:role/firehose-role",
            "data/",
        );

        let config = RedshiftDestinationConfig::new(
            "jdbc:redshift://cluster.region.redshift.amazonaws.com:5439/dev",
            "mydatabase",
            "mytable",
            "myuser",
            "arn:aws:iam::123456789012:role/redshift-role",
            s3_config,
        )
        .with_copy_options("JSON 'auto'");

        assert_eq!(config.database_name, "mydatabase");
        assert_eq!(config.table_name, "mytable");
        assert_eq!(config.copy_options, Some("JSON 'auto'".to_string()));
    }

    #[test]
    fn test_elasticsearch_destination_config() {
        let config = ElasticsearchDestinationConfig::new(
            "https://search-domain.us-east-1.es.amazonaws.com",
            "my-index",
            "my-type",
            "arn:aws:iam::123456789012:role/es-role",
        )
        .with_index_rotation(IndexRotationPeriod::OneDay)
        .with_buffer_size_mb(10);

        assert_eq!(
            config.domain_endpoint,
            "https://search-domain.us-east-1.es.amazonaws.com"
        );
        assert_eq!(config.index_name, "my-index");
        assert_eq!(
            config.index_rotation_period,
            Some(IndexRotationPeriod::OneDay)
        );
        assert_eq!(config.buffer_size_mb, 10);
    }
}
