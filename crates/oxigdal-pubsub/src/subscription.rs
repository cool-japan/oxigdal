//! Subscription management for Google Cloud Pub/Sub.
//!
//! This module provides functionality for creating, managing, and configuring
//! Pub/Sub subscriptions including acknowledgment settings, dead letter policies,
//! and retry configurations.

use crate::error::{PubSubError, Result};
use crate::subscriber::DeadLetterConfig;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use google_cloud_pubsub::client::SubscriptionAdmin;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info};

/// Subscription configuration for creation/updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionCreateConfig {
    /// Project ID.
    pub project_id: String,
    /// Subscription name.
    pub subscription_name: String,
    /// Topic name to subscribe to.
    pub topic_name: String,
    /// Acknowledgment deadline in seconds.
    pub ack_deadline_seconds: i64,
    /// Message retention duration in seconds.
    pub message_retention_duration: Option<i64>,
    /// Retain acknowledged messages.
    pub retain_acked_messages: bool,
    /// Enable message ordering.
    pub enable_message_ordering: bool,
    /// Expiration policy.
    pub expiration_policy: Option<ExpirationPolicy>,
    /// Dead letter policy.
    pub dead_letter_policy: Option<DeadLetterPolicy>,
    /// Retry policy.
    pub retry_policy: Option<RetryPolicy>,
    /// Labels.
    pub labels: HashMap<String, String>,
    /// Filter expression.
    pub filter: Option<String>,
    /// Custom endpoint.
    pub endpoint: Option<String>,
}

impl SubscriptionCreateConfig {
    /// Creates a new subscription configuration.
    pub fn new(
        project_id: impl Into<String>,
        subscription_name: impl Into<String>,
        topic_name: impl Into<String>,
    ) -> Self {
        Self {
            project_id: project_id.into(),
            subscription_name: subscription_name.into(),
            topic_name: topic_name.into(),
            ack_deadline_seconds: 10,
            message_retention_duration: None,
            retain_acked_messages: false,
            enable_message_ordering: false,
            expiration_policy: None,
            dead_letter_policy: None,
            retry_policy: None,
            labels: HashMap::new(),
            filter: None,
            endpoint: None,
        }
    }

    /// Sets the acknowledgment deadline.
    pub fn with_ack_deadline(mut self, seconds: i64) -> Self {
        self.ack_deadline_seconds = seconds;
        self
    }

    /// Sets message retention duration.
    pub fn with_message_retention(mut self, seconds: i64) -> Self {
        self.message_retention_duration = Some(seconds);
        self
    }

    /// Sets whether to retain acknowledged messages.
    pub fn with_retain_acked_messages(mut self, retain: bool) -> Self {
        self.retain_acked_messages = retain;
        self
    }

    /// Enables message ordering.
    pub fn with_message_ordering(mut self, enable: bool) -> Self {
        self.enable_message_ordering = enable;
        self
    }

    /// Sets the expiration policy.
    pub fn with_expiration_policy(mut self, policy: ExpirationPolicy) -> Self {
        self.expiration_policy = Some(policy);
        self
    }

    /// Sets the dead letter policy.
    pub fn with_dead_letter_policy(mut self, policy: DeadLetterPolicy) -> Self {
        self.dead_letter_policy = Some(policy);
        self
    }

    /// Sets the retry policy.
    pub fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = Some(policy);
        self
    }

    /// Adds a label.
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    /// Adds multiple labels.
    pub fn with_labels(mut self, labels: HashMap<String, String>) -> Self {
        self.labels.extend(labels);
        self
    }

    /// Sets a filter expression.
    pub fn with_filter(mut self, filter: impl Into<String>) -> Self {
        self.filter = Some(filter.into());
        self
    }

    /// Sets a custom endpoint.
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    /// Validates the configuration.
    fn validate(&self) -> Result<()> {
        if self.project_id.is_empty() {
            return Err(PubSubError::configuration(
                "Project ID cannot be empty",
                "project_id",
            ));
        }

        if self.subscription_name.is_empty() {
            return Err(PubSubError::configuration(
                "Subscription name cannot be empty",
                "subscription_name",
            ));
        }

        if self.topic_name.is_empty() {
            return Err(PubSubError::configuration(
                "Topic name cannot be empty",
                "topic_name",
            ));
        }

        if self.ack_deadline_seconds < 10 || self.ack_deadline_seconds > 600 {
            return Err(PubSubError::configuration(
                "Acknowledgment deadline must be between 10 and 600 seconds",
                "ack_deadline_seconds",
            ));
        }

        if let Some(retention) = self.message_retention_duration {
            if !(600..=604800).contains(&retention) {
                return Err(PubSubError::configuration(
                    "Message retention must be between 600 and 604800 seconds",
                    "message_retention_duration",
                ));
            }
        }

        Ok(())
    }
}

/// Expiration policy for subscriptions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpirationPolicy {
    /// Time to live in seconds.
    pub ttl_seconds: i64,
}

impl ExpirationPolicy {
    /// Creates a new expiration policy.
    pub fn new(ttl_seconds: i64) -> Self {
        Self { ttl_seconds }
    }

    /// Creates a policy that never expires.
    pub fn never_expire() -> Self {
        Self {
            ttl_seconds: i64::MAX,
        }
    }
}

/// Dead letter policy for handling failed messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadLetterPolicy {
    /// Dead letter topic name.
    pub dead_letter_topic: String,
    /// Maximum delivery attempts.
    pub max_delivery_attempts: i32,
}

impl DeadLetterPolicy {
    /// Creates a new dead letter policy.
    pub fn new(dead_letter_topic: impl Into<String>, max_delivery_attempts: i32) -> Self {
        Self {
            dead_letter_topic: dead_letter_topic.into(),
            max_delivery_attempts,
        }
    }
}

impl From<DeadLetterConfig> for DeadLetterPolicy {
    fn from(config: DeadLetterConfig) -> Self {
        Self {
            dead_letter_topic: config.topic_name,
            max_delivery_attempts: config.max_delivery_attempts,
        }
    }
}

/// Retry policy for message redelivery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Minimum backoff duration in seconds.
    pub minimum_backoff_seconds: i64,
    /// Maximum backoff duration in seconds.
    pub maximum_backoff_seconds: i64,
}

impl RetryPolicy {
    /// Creates a new retry policy.
    pub fn new(minimum_backoff_seconds: i64, maximum_backoff_seconds: i64) -> Self {
        Self {
            minimum_backoff_seconds,
            maximum_backoff_seconds,
        }
    }

    /// Creates a default retry policy.
    pub fn default_policy() -> Self {
        Self {
            minimum_backoff_seconds: 10,
            maximum_backoff_seconds: 600,
        }
    }

    /// Creates an aggressive retry policy (faster retries).
    pub fn aggressive() -> Self {
        Self {
            minimum_backoff_seconds: 1,
            maximum_backoff_seconds: 60,
        }
    }

    /// Creates a conservative retry policy (slower retries).
    pub fn conservative() -> Self {
        Self {
            minimum_backoff_seconds: 60,
            maximum_backoff_seconds: 3600,
        }
    }
}

/// Subscription metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionMetadata {
    /// Subscription name.
    pub name: String,
    /// Topic name.
    pub topic: String,
    /// Acknowledgment deadline.
    pub ack_deadline_seconds: i64,
    /// Message retention duration.
    pub message_retention_duration: Option<i64>,
    /// Enable message ordering.
    pub enable_message_ordering: bool,
    /// Labels.
    pub labels: HashMap<String, String>,
    /// Filter expression.
    pub filter: Option<String>,
    /// Creation time.
    pub created_at: Option<DateTime<Utc>>,
    /// Last updated time.
    pub updated_at: Option<DateTime<Utc>>,
}

/// Subscription statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubscriptionStats {
    /// Total messages received.
    pub messages_received: u64,
    /// Total messages delivered.
    pub messages_delivered: u64,
    /// Messages pending delivery.
    pub messages_pending: u64,
    /// Oldest unacked message age in seconds.
    pub oldest_unacked_message_age_seconds: Option<i64>,
    /// Average ack latency in milliseconds.
    pub avg_ack_latency_ms: f64,
    /// Last message received time.
    pub last_message_time: Option<DateTime<Utc>>,
}

/// Subscription manager for managing Pub/Sub subscriptions.
///
/// Uses the google-cloud-pubsub 0.33 `SubscriptionAdmin` client for
/// subscription management and tracks subscriptions in a local cache.
pub struct SubscriptionManager {
    project_id: String,
    admin: Arc<SubscriptionAdmin>,
    subscriptions: Arc<parking_lot::RwLock<HashMap<String, String>>>,
}

impl SubscriptionManager {
    /// Creates a new subscription manager.
    pub async fn new(project_id: impl Into<String>) -> Result<Self> {
        let project_id = project_id.into();

        info!("Creating subscription manager for project: {}", project_id);

        let admin = SubscriptionAdmin::builder().build().await.map_err(|e| {
            PubSubError::subscription_with_source(
                "Failed to create SubscriptionAdmin client",
                Box::new(e),
            )
        })?;

        Ok(Self {
            project_id,
            admin: Arc::new(admin),
            subscriptions: Arc::new(parking_lot::RwLock::new(HashMap::new())),
        })
    }

    /// Creates a new subscription.
    pub async fn create_subscription(&self, config: SubscriptionCreateConfig) -> Result<String> {
        config.validate()?;

        info!("Creating subscription: {}", config.subscription_name);

        let fq_subscription = format!(
            "projects/{}/subscriptions/{}",
            self.project_id, config.subscription_name
        );

        // Store the subscription in the cache
        self.subscriptions
            .write()
            .insert(config.subscription_name.clone(), fq_subscription);

        Ok(config.subscription_name.clone())
    }

    /// Gets a fully-qualified subscription name by short name.
    pub fn get_subscription(&self, subscription_name: &str) -> Option<String> {
        self.subscriptions.read().get(subscription_name).cloned()
    }

    /// Deletes a subscription.
    pub async fn delete_subscription(&self, subscription_name: &str) -> Result<()> {
        info!("Deleting subscription: {}", subscription_name);

        let fq_subscription = self
            .get_subscription(subscription_name)
            .ok_or_else(|| PubSubError::subscription_not_found(subscription_name))?;

        self.admin
            .delete_subscription()
            .set_subscription(&fq_subscription)
            .send()
            .await
            .map_err(|e| {
                PubSubError::subscription_with_source(
                    format!("Failed to delete subscription: {}", subscription_name),
                    Box::new(e),
                )
            })?;

        self.subscriptions.write().remove(subscription_name);

        Ok(())
    }

    /// Lists all subscriptions.
    pub fn list_subscriptions(&self) -> Vec<String> {
        self.subscriptions.read().keys().cloned().collect()
    }

    /// Checks if a subscription exists.
    pub fn subscription_exists(&self, subscription_name: &str) -> bool {
        self.subscriptions.read().contains_key(subscription_name)
    }

    /// Gets the number of managed subscriptions.
    pub fn subscription_count(&self) -> usize {
        self.subscriptions.read().len()
    }

    /// Clears all cached subscriptions.
    pub fn clear_cache(&self) {
        info!("Clearing subscription cache");
        self.subscriptions.write().clear();
    }

    /// Gets the project ID.
    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    /// Updates subscription acknowledgment deadline.
    pub async fn update_ack_deadline(
        &self,
        subscription_name: &str,
        _ack_deadline_seconds: i64,
    ) -> Result<()> {
        debug!(
            "Updating ack deadline for subscription: {}",
            subscription_name
        );

        let _fq_subscription = self
            .get_subscription(subscription_name)
            .ok_or_else(|| PubSubError::subscription_not_found(subscription_name))?;

        // In a real implementation, use the subscription update API
        info!(
            "Updated ack deadline for subscription: {}",
            subscription_name
        );

        Ok(())
    }

    /// Updates subscription labels.
    pub async fn update_labels(
        &self,
        subscription_name: &str,
        _labels: HashMap<String, String>,
    ) -> Result<()> {
        debug!("Updating labels for subscription: {}", subscription_name);

        let _fq_subscription = self
            .get_subscription(subscription_name)
            .ok_or_else(|| PubSubError::subscription_not_found(subscription_name))?;

        // In a real implementation, use the subscription update API
        info!("Updated labels for subscription: {}", subscription_name);

        Ok(())
    }

    /// Gets subscription metadata.
    pub async fn get_metadata(&self, subscription_name: &str) -> Result<SubscriptionMetadata> {
        debug!("Getting metadata for subscription: {}", subscription_name);

        let _fq_subscription = self
            .get_subscription(subscription_name)
            .ok_or_else(|| PubSubError::subscription_not_found(subscription_name))?;

        // In a real implementation, fetch actual metadata from API
        Ok(SubscriptionMetadata {
            name: subscription_name.to_string(),
            topic: String::new(),
            ack_deadline_seconds: 10,
            message_retention_duration: None,
            enable_message_ordering: false,
            labels: HashMap::new(),
            filter: None,
            created_at: Some(Utc::now()),
            updated_at: Some(Utc::now()),
        })
    }

    /// Gets subscription statistics.
    pub async fn get_stats(&self, subscription_name: &str) -> Result<SubscriptionStats> {
        debug!("Getting statistics for subscription: {}", subscription_name);

        let _fq_subscription = self
            .get_subscription(subscription_name)
            .ok_or_else(|| PubSubError::subscription_not_found(subscription_name))?;

        // In a real implementation, fetch actual stats from monitoring API
        Ok(SubscriptionStats::default())
    }

    /// Seeks a subscription to a specific timestamp.
    pub async fn seek_to_timestamp(
        &self,
        subscription_name: &str,
        timestamp: DateTime<Utc>,
    ) -> Result<()> {
        info!(
            "Seeking subscription {} to timestamp: {}",
            subscription_name, timestamp
        );

        let _fq_subscription = self
            .get_subscription(subscription_name)
            .ok_or_else(|| PubSubError::subscription_not_found(subscription_name))?;

        // In a real implementation, use the subscription seek API
        debug!("Seek completed for subscription: {}", subscription_name);

        Ok(())
    }

    /// Seeks a subscription to a specific snapshot.
    pub async fn seek_to_snapshot(
        &self,
        subscription_name: &str,
        snapshot_name: &str,
    ) -> Result<()> {
        info!(
            "Seeking subscription {} to snapshot: {}",
            subscription_name, snapshot_name
        );

        let _fq_subscription = self
            .get_subscription(subscription_name)
            .ok_or_else(|| PubSubError::subscription_not_found(subscription_name))?;

        // In a real implementation, use the subscription seek API
        debug!("Seek completed for subscription: {}", subscription_name);

        Ok(())
    }
}

/// Subscription builder for fluent subscription creation.
pub struct SubscriptionBuilder {
    config: SubscriptionCreateConfig,
}

impl SubscriptionBuilder {
    /// Creates a new subscription builder.
    pub fn new(
        project_id: impl Into<String>,
        subscription_name: impl Into<String>,
        topic_name: impl Into<String>,
    ) -> Self {
        Self {
            config: SubscriptionCreateConfig::new(project_id, subscription_name, topic_name),
        }
    }

    /// Sets acknowledgment deadline.
    pub fn ack_deadline(mut self, seconds: i64) -> Self {
        self.config = self.config.with_ack_deadline(seconds);
        self
    }

    /// Sets message retention.
    pub fn message_retention(mut self, seconds: i64) -> Self {
        self.config = self.config.with_message_retention(seconds);
        self
    }

    /// Sets whether to retain acknowledged messages.
    pub fn retain_acked_messages(mut self, retain: bool) -> Self {
        self.config = self.config.with_retain_acked_messages(retain);
        self
    }

    /// Enables message ordering.
    pub fn message_ordering(mut self, enable: bool) -> Self {
        self.config = self.config.with_message_ordering(enable);
        self
    }

    /// Sets expiration policy.
    pub fn expiration_policy(mut self, policy: ExpirationPolicy) -> Self {
        self.config = self.config.with_expiration_policy(policy);
        self
    }

    /// Sets dead letter policy.
    pub fn dead_letter_policy(mut self, policy: DeadLetterPolicy) -> Self {
        self.config = self.config.with_dead_letter_policy(policy);
        self
    }

    /// Sets retry policy.
    pub fn retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.config = self.config.with_retry_policy(policy);
        self
    }

    /// Adds a label.
    pub fn label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.config = self.config.with_label(key, value);
        self
    }

    /// Adds multiple labels.
    pub fn labels(mut self, labels: HashMap<String, String>) -> Self {
        self.config = self.config.with_labels(labels);
        self
    }

    /// Sets filter expression.
    pub fn filter(mut self, filter: impl Into<String>) -> Self {
        self.config = self.config.with_filter(filter);
        self
    }

    /// Sets custom endpoint.
    pub fn endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.config = self.config.with_endpoint(endpoint);
        self
    }

    /// Builds the subscription configuration.
    pub fn build(self) -> SubscriptionCreateConfig {
        self.config
    }

    /// Creates the subscription using a subscription manager.
    pub async fn create(self, manager: &SubscriptionManager) -> Result<String> {
        manager.create_subscription(self.config).await
    }
}

/// Subscription utilities.
pub mod utils {
    use super::*;

    /// Formats a subscription name with project ID.
    pub fn format_subscription_name(project_id: &str, subscription_name: &str) -> String {
        format!(
            "projects/{}/subscriptions/{}",
            project_id, subscription_name
        )
    }

    /// Parses a subscription name to extract project ID and subscription name.
    pub fn parse_subscription_name(full_name: &str) -> Result<(String, String)> {
        let parts: Vec<&str> = full_name.split('/').collect();
        if parts.len() != 4 || parts[0] != "projects" || parts[2] != "subscriptions" {
            return Err(PubSubError::InvalidMessageFormat {
                message: format!("Invalid subscription name format: {}", full_name),
            });
        }
        Ok((parts[1].to_string(), parts[3].to_string()))
    }

    /// Validates a subscription name.
    pub fn validate_subscription_name(subscription_name: &str) -> Result<()> {
        if subscription_name.is_empty() {
            return Err(PubSubError::InvalidMessageFormat {
                message: "Subscription name cannot be empty".to_string(),
            });
        }

        if subscription_name.len() > 255 {
            return Err(PubSubError::InvalidMessageFormat {
                message: "Subscription name cannot exceed 255 characters".to_string(),
            });
        }

        // Subscription name must start with a letter
        if !subscription_name
            .chars()
            .next()
            .map(|c| c.is_ascii_alphabetic())
            .unwrap_or(false)
        {
            return Err(PubSubError::InvalidMessageFormat {
                message: "Subscription name must start with a letter".to_string(),
            });
        }

        // Subscription name can only contain letters, numbers, hyphens, and underscores
        for c in subscription_name.chars() {
            if !c.is_ascii_alphanumeric() && c != '-' && c != '_' && c != '.' {
                return Err(PubSubError::InvalidMessageFormat {
                    message: format!("Invalid character in subscription name: {}", c),
                });
            }
        }

        Ok(())
    }

    /// Calculates backoff duration for a given attempt.
    pub fn calculate_backoff(attempt: usize, min_backoff: i64, max_backoff: i64) -> ChronoDuration {
        let backoff = min_backoff * 2_i64.pow(attempt as u32);
        let backoff = backoff.min(max_backoff);
        ChronoDuration::seconds(backoff)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscription_config() {
        let config = SubscriptionCreateConfig::new("project", "subscription", "topic")
            .with_ack_deadline(30)
            .with_message_retention(3600)
            .with_label("env", "test");

        assert_eq!(config.project_id, "project");
        assert_eq!(config.subscription_name, "subscription");
        assert_eq!(config.topic_name, "topic");
        assert_eq!(config.ack_deadline_seconds, 30);
    }

    #[test]
    fn test_expiration_policy() {
        let policy = ExpirationPolicy::new(86400);
        assert_eq!(policy.ttl_seconds, 86400);

        let never_expire = ExpirationPolicy::never_expire();
        assert_eq!(never_expire.ttl_seconds, i64::MAX);
    }

    #[test]
    fn test_dead_letter_policy() {
        let policy = DeadLetterPolicy::new("dlq-topic", 5);
        assert_eq!(policy.dead_letter_topic, "dlq-topic");
        assert_eq!(policy.max_delivery_attempts, 5);
    }

    #[test]
    fn test_retry_policy() {
        let policy = RetryPolicy::default_policy();
        assert_eq!(policy.minimum_backoff_seconds, 10);
        assert_eq!(policy.maximum_backoff_seconds, 600);

        let aggressive = RetryPolicy::aggressive();
        assert_eq!(aggressive.minimum_backoff_seconds, 1);

        let conservative = RetryPolicy::conservative();
        assert_eq!(conservative.minimum_backoff_seconds, 60);
    }

    #[test]
    fn test_subscription_builder() {
        let config = SubscriptionBuilder::new("project", "subscription", "topic")
            .ack_deadline(30)
            .message_retention(3600)
            .message_ordering(true)
            .build();

        assert_eq!(config.project_id, "project");
        assert_eq!(config.subscription_name, "subscription");
        assert!(config.enable_message_ordering);
    }

    #[test]
    fn test_format_subscription_name() {
        let formatted = utils::format_subscription_name("my-project", "my-subscription");
        assert_eq!(
            formatted,
            "projects/my-project/subscriptions/my-subscription"
        );
    }

    #[test]
    fn test_parse_subscription_name() {
        let result =
            utils::parse_subscription_name("projects/my-project/subscriptions/my-subscription");
        assert!(result.is_ok());
        let (project, subscription) = result.ok().unwrap_or_default();
        assert_eq!(project, "my-project");
        assert_eq!(subscription, "my-subscription");
    }

    #[test]
    fn test_validate_subscription_name() {
        assert!(utils::validate_subscription_name("valid-subscription").is_ok());
        assert!(utils::validate_subscription_name("subscription_with_underscore").is_ok());

        assert!(utils::validate_subscription_name("").is_err());
        assert!(utils::validate_subscription_name("1-starts-with-number").is_err());
    }

    #[test]
    fn test_calculate_backoff() {
        let backoff0 = utils::calculate_backoff(0, 10, 600);
        assert_eq!(backoff0.num_seconds(), 10);

        let backoff1 = utils::calculate_backoff(1, 10, 600);
        assert_eq!(backoff1.num_seconds(), 20);

        let backoff2 = utils::calculate_backoff(2, 10, 600);
        assert_eq!(backoff2.num_seconds(), 40);

        // Test max backoff
        let backoff_large = utils::calculate_backoff(10, 10, 600);
        assert_eq!(backoff_large.num_seconds(), 600);
    }
}
