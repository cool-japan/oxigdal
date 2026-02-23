//! Configuration for offline data management

use crate::error::{Error, Result};
use crate::merge::MergeStrategy;
use core::time::Duration;

/// Configuration for offline data management
#[derive(Debug, Clone)]
pub struct Config {
    /// Maximum size of sync queue
    pub max_queue_size: usize,

    /// Merge strategy for conflict resolution
    pub merge_strategy: MergeStrategy,

    /// Maximum number of retry attempts
    pub retry_max_attempts: usize,

    /// Initial retry delay (in milliseconds)
    pub retry_initial_delay_ms: u64,

    /// Maximum retry delay (in milliseconds)
    pub retry_max_delay_ms: u64,

    /// Retry backoff multiplier
    pub retry_backoff_multiplier: f64,

    /// Jitter factor for retry delay (0.0 to 1.0)
    pub retry_jitter_factor: f64,

    /// Sync batch size (number of operations per sync batch)
    pub sync_batch_size: usize,

    /// Automatic sync interval (in seconds, 0 = disabled)
    pub auto_sync_interval_secs: u64,

    /// Enable optimistic updates
    pub enable_optimistic_updates: bool,

    /// Maximum age of operations in queue (in seconds, 0 = unlimited)
    pub max_operation_age_secs: u64,

    /// Storage path for native platforms
    pub storage_path: Option<String>,

    /// Database name for WASM platforms
    pub database_name: String,

    /// Enable compression for stored data
    pub enable_compression: bool,

    /// Compression threshold (bytes, data smaller than this won't be compressed)
    pub compression_threshold: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_queue_size: 10_000,
            merge_strategy: MergeStrategy::LastWriteWins,
            retry_max_attempts: 5,
            retry_initial_delay_ms: 1000,
            retry_max_delay_ms: 60_000,
            retry_backoff_multiplier: 2.0,
            retry_jitter_factor: 0.1,
            sync_batch_size: 100,
            auto_sync_interval_secs: 60,
            enable_optimistic_updates: true,
            max_operation_age_secs: 86400, // 24 hours
            storage_path: None,
            database_name: "oxigdal-offline".to_string(),
            enable_compression: true,
            compression_threshold: 1024,
        }
    }
}

impl Config {
    /// Create a new configuration builder
    pub fn builder() -> ConfigBuilder {
        ConfigBuilder::default()
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        if self.max_queue_size == 0 {
            return Err(Error::config("max_queue_size must be > 0"));
        }

        if self.retry_max_attempts == 0 {
            return Err(Error::config("retry_max_attempts must be > 0"));
        }

        if self.retry_initial_delay_ms == 0 {
            return Err(Error::config("retry_initial_delay_ms must be > 0"));
        }

        if self.retry_max_delay_ms < self.retry_initial_delay_ms {
            return Err(Error::config(
                "retry_max_delay_ms must be >= retry_initial_delay_ms",
            ));
        }

        if self.retry_backoff_multiplier <= 1.0 {
            return Err(Error::config("retry_backoff_multiplier must be > 1.0"));
        }

        if !(0.0..=1.0).contains(&self.retry_jitter_factor) {
            return Err(Error::config(
                "retry_jitter_factor must be between 0.0 and 1.0",
            ));
        }

        if self.sync_batch_size == 0 {
            return Err(Error::config("sync_batch_size must be > 0"));
        }

        if self.database_name.is_empty() {
            return Err(Error::config("database_name cannot be empty"));
        }

        Ok(())
    }

    /// Get initial retry delay as Duration
    pub fn initial_retry_delay(&self) -> Duration {
        Duration::from_millis(self.retry_initial_delay_ms)
    }

    /// Get max retry delay as Duration
    pub fn max_retry_delay(&self) -> Duration {
        Duration::from_millis(self.retry_max_delay_ms)
    }

    /// Get auto sync interval as Duration
    pub fn auto_sync_interval(&self) -> Option<Duration> {
        if self.auto_sync_interval_secs == 0 {
            None
        } else {
            Some(Duration::from_secs(self.auto_sync_interval_secs))
        }
    }

    /// Get max operation age as Duration
    pub fn max_operation_age(&self) -> Option<Duration> {
        if self.max_operation_age_secs == 0 {
            None
        } else {
            Some(Duration::from_secs(self.max_operation_age_secs))
        }
    }
}

/// Builder for Config
#[derive(Debug, Clone, Default)]
pub struct ConfigBuilder {
    max_queue_size: Option<usize>,
    merge_strategy: Option<MergeStrategy>,
    retry_max_attempts: Option<usize>,
    retry_initial_delay_ms: Option<u64>,
    retry_max_delay_ms: Option<u64>,
    retry_backoff_multiplier: Option<f64>,
    retry_jitter_factor: Option<f64>,
    sync_batch_size: Option<usize>,
    auto_sync_interval_secs: Option<u64>,
    enable_optimistic_updates: Option<bool>,
    max_operation_age_secs: Option<u64>,
    storage_path: Option<String>,
    database_name: Option<String>,
    enable_compression: Option<bool>,
    compression_threshold: Option<usize>,
}

impl ConfigBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Set max queue size
    pub fn max_queue_size(mut self, size: usize) -> Self {
        self.max_queue_size = Some(size);
        self
    }

    /// Set merge strategy
    pub fn merge_strategy(mut self, strategy: MergeStrategy) -> Self {
        self.merge_strategy = Some(strategy);
        self
    }

    /// Set retry max attempts
    pub fn retry_max_attempts(mut self, attempts: usize) -> Self {
        self.retry_max_attempts = Some(attempts);
        self
    }

    /// Set retry initial delay in milliseconds
    pub fn retry_initial_delay_ms(mut self, ms: u64) -> Self {
        self.retry_initial_delay_ms = Some(ms);
        self
    }

    /// Set retry max delay in milliseconds
    pub fn retry_max_delay_ms(mut self, ms: u64) -> Self {
        self.retry_max_delay_ms = Some(ms);
        self
    }

    /// Set retry backoff multiplier
    pub fn retry_backoff_multiplier(mut self, multiplier: f64) -> Self {
        self.retry_backoff_multiplier = Some(multiplier);
        self
    }

    /// Set retry jitter factor
    pub fn retry_jitter_factor(mut self, factor: f64) -> Self {
        self.retry_jitter_factor = Some(factor);
        self
    }

    /// Set sync batch size
    pub fn sync_batch_size(mut self, size: usize) -> Self {
        self.sync_batch_size = Some(size);
        self
    }

    /// Set auto sync interval in seconds
    pub fn auto_sync_interval_secs(mut self, secs: u64) -> Self {
        self.auto_sync_interval_secs = Some(secs);
        self
    }

    /// Enable or disable optimistic updates
    pub fn enable_optimistic_updates(mut self, enable: bool) -> Self {
        self.enable_optimistic_updates = Some(enable);
        self
    }

    /// Set max operation age in seconds
    pub fn max_operation_age_secs(mut self, secs: u64) -> Self {
        self.max_operation_age_secs = Some(secs);
        self
    }

    /// Set storage path
    pub fn storage_path(mut self, path: String) -> Self {
        self.storage_path = Some(path);
        self
    }

    /// Set database name
    pub fn database_name(mut self, name: String) -> Self {
        self.database_name = Some(name);
        self
    }

    /// Enable or disable compression
    pub fn enable_compression(mut self, enable: bool) -> Self {
        self.enable_compression = Some(enable);
        self
    }

    /// Set compression threshold
    pub fn compression_threshold(mut self, threshold: usize) -> Self {
        self.compression_threshold = Some(threshold);
        self
    }

    /// Build the configuration
    pub fn build(self) -> Result<Config> {
        let defaults = Config::default();

        let config = Config {
            max_queue_size: self.max_queue_size.unwrap_or(defaults.max_queue_size),
            merge_strategy: self.merge_strategy.unwrap_or(defaults.merge_strategy),
            retry_max_attempts: self
                .retry_max_attempts
                .unwrap_or(defaults.retry_max_attempts),
            retry_initial_delay_ms: self
                .retry_initial_delay_ms
                .unwrap_or(defaults.retry_initial_delay_ms),
            retry_max_delay_ms: self
                .retry_max_delay_ms
                .unwrap_or(defaults.retry_max_delay_ms),
            retry_backoff_multiplier: self
                .retry_backoff_multiplier
                .unwrap_or(defaults.retry_backoff_multiplier),
            retry_jitter_factor: self
                .retry_jitter_factor
                .unwrap_or(defaults.retry_jitter_factor),
            sync_batch_size: self.sync_batch_size.unwrap_or(defaults.sync_batch_size),
            auto_sync_interval_secs: self
                .auto_sync_interval_secs
                .unwrap_or(defaults.auto_sync_interval_secs),
            enable_optimistic_updates: self
                .enable_optimistic_updates
                .unwrap_or(defaults.enable_optimistic_updates),
            max_operation_age_secs: self
                .max_operation_age_secs
                .unwrap_or(defaults.max_operation_age_secs),
            storage_path: self.storage_path.or(defaults.storage_path),
            database_name: self.database_name.unwrap_or(defaults.database_name),
            enable_compression: self
                .enable_compression
                .unwrap_or(defaults.enable_compression),
            compression_threshold: self
                .compression_threshold
                .unwrap_or(defaults.compression_threshold),
        };

        config.validate()?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_builder() {
        let config = Config::builder()
            .max_queue_size(5000)
            .retry_max_attempts(3)
            .sync_batch_size(50)
            .build();

        assert!(config.is_ok());
        let config = config.expect("failed to build config");
        assert_eq!(config.max_queue_size, 5000);
        assert_eq!(config.retry_max_attempts, 3);
        assert_eq!(config.sync_batch_size, 50);
    }

    #[test]
    fn test_invalid_config() {
        let result = Config::builder().max_queue_size(0).build();
        assert!(result.is_err());
    }

    #[test]
    fn test_retry_delay_conversion() {
        let config = Config::default();
        let initial = config.initial_retry_delay();
        assert_eq!(initial.as_millis(), 1000);
    }
}
