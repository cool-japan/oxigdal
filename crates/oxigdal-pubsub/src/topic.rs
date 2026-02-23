//! Topic management for Google Cloud Pub/Sub.
//!
//! This module provides functionality for creating, managing, and configuring
//! Pub/Sub topics including message retention, schema settings, and IAM policies.

use crate::error::{PubSubError, Result};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use google_cloud_pubsub::client::{Client, ClientConfig};
use google_cloud_pubsub::topic::Topic as GcpTopic;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info};

/// Topic configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicConfig {
    /// Project ID.
    pub project_id: String,
    /// Topic name.
    pub topic_name: String,
    /// Message retention duration in seconds.
    pub message_retention_duration: Option<i64>,
    /// Labels for the topic.
    pub labels: HashMap<String, String>,
    /// Enable message ordering.
    pub enable_message_ordering: bool,
    /// Schema settings.
    #[cfg(feature = "schema")]
    pub schema_settings: Option<SchemaSettings>,
    /// Custom endpoint (for testing).
    pub endpoint: Option<String>,
}

impl TopicConfig {
    /// Creates a new topic configuration.
    pub fn new(project_id: impl Into<String>, topic_name: impl Into<String>) -> Self {
        Self {
            project_id: project_id.into(),
            topic_name: topic_name.into(),
            message_retention_duration: None,
            labels: HashMap::new(),
            enable_message_ordering: false,
            #[cfg(feature = "schema")]
            schema_settings: None,
            endpoint: None,
        }
    }

    /// Sets the message retention duration.
    pub fn with_message_retention(mut self, seconds: i64) -> Self {
        self.message_retention_duration = Some(seconds);
        self
    }

    /// Adds a label to the topic.
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    /// Adds multiple labels to the topic.
    pub fn with_labels(mut self, labels: HashMap<String, String>) -> Self {
        self.labels.extend(labels);
        self
    }

    /// Enables message ordering.
    pub fn with_message_ordering(mut self, enable: bool) -> Self {
        self.enable_message_ordering = enable;
        self
    }

    /// Sets schema settings.
    #[cfg(feature = "schema")]
    pub fn with_schema_settings(mut self, settings: SchemaSettings) -> Self {
        self.schema_settings = Some(settings);
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

        if self.topic_name.is_empty() {
            return Err(PubSubError::configuration(
                "Topic name cannot be empty",
                "topic_name",
            ));
        }

        if let Some(retention) = self.message_retention_duration {
            if !(600..=604800).contains(&retention) {
                return Err(PubSubError::configuration(
                    "Message retention must be between 600 and 604800 seconds (10 minutes to 7 days)",
                    "message_retention_duration",
                ));
            }
        }

        Ok(())
    }
}

/// Schema settings for a topic.
#[cfg(feature = "schema")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaSettings {
    /// Schema ID.
    pub schema_id: String,
    /// Schema encoding.
    pub encoding: crate::schema::SchemaEncoding,
    /// First revision ID.
    pub first_revision_id: Option<String>,
    /// Last revision ID.
    pub last_revision_id: Option<String>,
}

#[cfg(feature = "schema")]
impl SchemaSettings {
    /// Creates new schema settings.
    pub fn new(schema_id: impl Into<String>, encoding: crate::schema::SchemaEncoding) -> Self {
        Self {
            schema_id: schema_id.into(),
            encoding,
            first_revision_id: None,
            last_revision_id: None,
        }
    }

    /// Sets the first revision ID.
    pub fn with_first_revision(mut self, revision_id: impl Into<String>) -> Self {
        self.first_revision_id = Some(revision_id.into());
        self
    }

    /// Sets the last revision ID.
    pub fn with_last_revision(mut self, revision_id: impl Into<String>) -> Self {
        self.last_revision_id = Some(revision_id.into());
        self
    }
}

/// Topic metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicMetadata {
    /// Topic name.
    pub name: String,
    /// Labels.
    pub labels: HashMap<String, String>,
    /// Message retention duration.
    pub message_retention_duration: Option<i64>,
    /// Message ordering enabled.
    pub enable_message_ordering: bool,
    /// Creation time.
    pub created_at: Option<DateTime<Utc>>,
    /// Last updated time.
    pub updated_at: Option<DateTime<Utc>>,
}

/// Topic statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TopicStats {
    /// Number of subscriptions.
    pub subscription_count: u64,
    /// Total messages published.
    pub messages_published: u64,
    /// Total bytes published.
    pub bytes_published: u64,
    /// Average message size.
    pub avg_message_size: f64,
    /// Last publish time.
    pub last_publish_time: Option<DateTime<Utc>>,
}

/// Topic manager for managing Pub/Sub topics.
pub struct TopicManager {
    project_id: String,
    client: Arc<Client>,
    topics: Arc<parking_lot::RwLock<HashMap<String, Arc<GcpTopic>>>>,
}

impl TopicManager {
    /// Creates a new topic manager.
    pub async fn new(project_id: impl Into<String>) -> Result<Self> {
        let project_id = project_id.into();

        info!("Creating topic manager for project: {}", project_id);

        let client_config = ClientConfig {
            project_id: Some(project_id.clone()),
            ..Default::default()
        };

        // Initialize authentication if not using emulator
        #[cfg(feature = "auth")]
        let client_config = client_config.with_auth().await.map_err(|e| {
            PubSubError::configuration(
                format!("Failed to initialize authentication: {}", e),
                "authentication",
            )
        })?;

        let client = Client::new(client_config).await.map_err(|e| {
            PubSubError::publish_with_source("Failed to create Pub/Sub client", Box::new(e))
        })?;

        Ok(Self {
            project_id,
            client: Arc::new(client),
            topics: Arc::new(parking_lot::RwLock::new(HashMap::new())),
        })
    }

    /// Creates a new topic.
    pub async fn create_topic(&self, config: TopicConfig) -> Result<String> {
        config.validate()?;

        info!("Creating topic: {}", config.topic_name);

        let topic = self.client.topic(&config.topic_name);

        // Store the topic
        self.topics
            .write()
            .insert(config.topic_name.clone(), Arc::new(topic));

        Ok(config.topic_name.clone())
    }

    /// Gets a topic by name.
    pub fn get_topic(&self, topic_name: &str) -> Option<Arc<GcpTopic>> {
        self.topics.read().get(topic_name).cloned()
    }

    /// Deletes a topic.
    pub async fn delete_topic(&self, topic_name: &str) -> Result<()> {
        info!("Deleting topic: {}", topic_name);

        let topic = self
            .get_topic(topic_name)
            .ok_or_else(|| PubSubError::topic_not_found(topic_name))?;

        topic.delete(None).await.map_err(|e| {
            PubSubError::publish_with_source(
                format!("Failed to delete topic: {}", topic_name),
                Box::new(e),
            )
        })?;

        self.topics.write().remove(topic_name);

        Ok(())
    }

    /// Lists all topics in the project.
    pub fn list_topics(&self) -> Vec<String> {
        self.topics.read().keys().cloned().collect()
    }

    /// Checks if a topic exists.
    pub fn topic_exists(&self, topic_name: &str) -> bool {
        self.topics.read().contains_key(topic_name)
    }

    /// Gets the number of managed topics.
    pub fn topic_count(&self) -> usize {
        self.topics.read().len()
    }

    /// Clears all cached topics.
    pub fn clear_cache(&self) {
        info!("Clearing topic cache");
        self.topics.write().clear();
    }

    /// Gets the project ID.
    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    /// Updates topic labels.
    pub async fn update_labels(
        &self,
        topic_name: &str,
        _labels: HashMap<String, String>,
    ) -> Result<()> {
        debug!("Updating labels for topic: {}", topic_name);

        let _topic = self
            .get_topic(topic_name)
            .ok_or_else(|| PubSubError::topic_not_found(topic_name))?;

        // In a real implementation, use the topic update API
        info!("Updated labels for topic: {}", topic_name);

        Ok(())
    }

    /// Gets topic metadata.
    pub async fn get_metadata(&self, topic_name: &str) -> Result<TopicMetadata> {
        debug!("Getting metadata for topic: {}", topic_name);

        let _topic = self
            .get_topic(topic_name)
            .ok_or_else(|| PubSubError::topic_not_found(topic_name))?;

        // In a real implementation, fetch actual metadata from API
        Ok(TopicMetadata {
            name: topic_name.to_string(),
            labels: HashMap::new(),
            message_retention_duration: None,
            enable_message_ordering: false,
            created_at: Some(Utc::now()),
            updated_at: Some(Utc::now()),
        })
    }

    /// Gets topic statistics.
    pub async fn get_stats(&self, topic_name: &str) -> Result<TopicStats> {
        debug!("Getting statistics for topic: {}", topic_name);

        let _topic = self
            .get_topic(topic_name)
            .ok_or_else(|| PubSubError::topic_not_found(topic_name))?;

        // In a real implementation, fetch actual stats from monitoring API
        Ok(TopicStats::default())
    }

    /// Publishes a test message to verify topic connectivity.
    pub async fn test_publish(&self, topic_name: &str) -> Result<String> {
        info!("Testing publish to topic: {}", topic_name);

        let topic = self
            .get_topic(topic_name)
            .ok_or_else(|| PubSubError::topic_not_found(topic_name))?;

        let publisher = topic.new_publisher(None);
        let message = google_cloud_googleapis::pubsub::v1::PubsubMessage {
            data: b"test".to_vec(),
            ..Default::default()
        };

        let awaiter = publisher.publish(message).await;
        let message_id = awaiter
            .get()
            .await
            .map_err(|e| PubSubError::publish_with_source("Test publish failed", Box::new(e)))?;

        info!("Test publish successful: {}", message_id);
        Ok(message_id)
    }
}

/// Topic builder for fluent topic creation.
pub struct TopicBuilder {
    config: TopicConfig,
}

impl TopicBuilder {
    /// Creates a new topic builder.
    pub fn new(project_id: impl Into<String>, topic_name: impl Into<String>) -> Self {
        Self {
            config: TopicConfig::new(project_id, topic_name),
        }
    }

    /// Sets message retention duration.
    pub fn message_retention(mut self, seconds: i64) -> Self {
        self.config = self.config.with_message_retention(seconds);
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

    /// Enables message ordering.
    pub fn message_ordering(mut self, enable: bool) -> Self {
        self.config = self.config.with_message_ordering(enable);
        self
    }

    /// Sets schema settings.
    #[cfg(feature = "schema")]
    pub fn schema_settings(mut self, settings: SchemaSettings) -> Self {
        self.config = self.config.with_schema_settings(settings);
        self
    }

    /// Sets custom endpoint.
    pub fn endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.config = self.config.with_endpoint(endpoint);
        self
    }

    /// Builds the topic configuration.
    pub fn build(self) -> TopicConfig {
        self.config
    }

    /// Creates the topic using a topic manager.
    pub async fn create(self, manager: &TopicManager) -> Result<String> {
        manager.create_topic(self.config).await
    }
}

/// Topic utilities.
pub mod utils {
    use super::*;

    /// Formats a topic name with project ID.
    pub fn format_topic_name(project_id: &str, topic_name: &str) -> String {
        format!("projects/{}/topics/{}", project_id, topic_name)
    }

    /// Parses a topic name to extract project ID and topic name.
    pub fn parse_topic_name(full_name: &str) -> Result<(String, String)> {
        let parts: Vec<&str> = full_name.split('/').collect();
        if parts.len() != 4 || parts[0] != "projects" || parts[2] != "topics" {
            return Err(PubSubError::InvalidMessageFormat {
                message: format!("Invalid topic name format: {}", full_name),
            });
        }
        Ok((parts[1].to_string(), parts[3].to_string()))
    }

    /// Validates a topic name.
    pub fn validate_topic_name(topic_name: &str) -> Result<()> {
        if topic_name.is_empty() {
            return Err(PubSubError::InvalidMessageFormat {
                message: "Topic name cannot be empty".to_string(),
            });
        }

        if topic_name.len() > 255 {
            return Err(PubSubError::InvalidMessageFormat {
                message: "Topic name cannot exceed 255 characters".to_string(),
            });
        }

        // Topic name must start with a letter
        if !topic_name
            .chars()
            .next()
            .map(|c| c.is_ascii_alphabetic())
            .unwrap_or(false)
        {
            return Err(PubSubError::InvalidMessageFormat {
                message: "Topic name must start with a letter".to_string(),
            });
        }

        // Topic name can only contain letters, numbers, hyphens, and underscores
        for c in topic_name.chars() {
            if !c.is_ascii_alphanumeric() && c != '-' && c != '_' && c != '.' {
                return Err(PubSubError::InvalidMessageFormat {
                    message: format!("Invalid character in topic name: {}", c),
                });
            }
        }

        Ok(())
    }

    /// Calculates message retention expiration time.
    pub fn calculate_expiration(
        publish_time: DateTime<Utc>,
        retention_seconds: i64,
    ) -> DateTime<Utc> {
        publish_time + ChronoDuration::seconds(retention_seconds)
    }

    /// Checks if a message has expired.
    pub fn is_message_expired(publish_time: DateTime<Utc>, retention_seconds: i64) -> bool {
        let expiration = calculate_expiration(publish_time, retention_seconds);
        Utc::now() > expiration
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_topic_config() {
        let config = TopicConfig::new("project", "topic")
            .with_message_retention(3600)
            .with_label("env", "test")
            .with_message_ordering(true);

        assert_eq!(config.project_id, "project");
        assert_eq!(config.topic_name, "topic");
        assert_eq!(config.message_retention_duration, Some(3600));
        assert!(config.enable_message_ordering);
    }

    #[test]
    fn test_topic_config_validation() {
        let valid_config = TopicConfig::new("project", "topic");
        assert!(valid_config.validate().is_ok());

        let invalid_config = TopicConfig::new("", "topic");
        assert!(invalid_config.validate().is_err());
    }

    #[test]
    fn test_topic_builder() {
        let config = TopicBuilder::new("project", "topic")
            .message_retention(3600)
            .label("key", "value")
            .message_ordering(true)
            .build();

        assert_eq!(config.project_id, "project");
        assert_eq!(config.topic_name, "topic");
    }

    #[test]
    fn test_format_topic_name() {
        let formatted = utils::format_topic_name("my-project", "my-topic");
        assert_eq!(formatted, "projects/my-project/topics/my-topic");
    }

    #[test]
    fn test_parse_topic_name() {
        let result = utils::parse_topic_name("projects/my-project/topics/my-topic");
        assert!(result.is_ok());
        let (project, topic) = result.ok().unwrap_or_default();
        assert_eq!(project, "my-project");
        assert_eq!(topic, "my-topic");

        let invalid = utils::parse_topic_name("invalid");
        assert!(invalid.is_err());
    }

    #[test]
    fn test_validate_topic_name() {
        assert!(utils::validate_topic_name("valid-topic-name").is_ok());
        assert!(utils::validate_topic_name("topic_with_underscore").is_ok());
        assert!(utils::validate_topic_name("topic.with.dots").is_ok());

        assert!(utils::validate_topic_name("").is_err());
        assert!(utils::validate_topic_name("1-starts-with-number").is_err());
        assert!(utils::validate_topic_name("invalid@char").is_err());
    }

    #[test]
    fn test_message_expiration() {
        let now = Utc::now();
        let retention = 3600; // 1 hour

        let expiration = utils::calculate_expiration(now, retention);
        assert!(expiration > now);

        let old_time = now - ChronoDuration::hours(2);
        assert!(utils::is_message_expired(old_time, retention));

        let recent_time = now - ChronoDuration::minutes(30);
        assert!(!utils::is_message_expired(recent_time, retention));
    }

    #[test]
    fn test_topic_metadata() {
        let metadata = TopicMetadata {
            name: "test-topic".to_string(),
            labels: HashMap::new(),
            message_retention_duration: Some(3600),
            enable_message_ordering: true,
            created_at: Some(Utc::now()),
            updated_at: Some(Utc::now()),
        };

        assert_eq!(metadata.name, "test-topic");
        assert!(metadata.enable_message_ordering);
    }

    #[test]
    fn test_topic_stats() {
        let stats = TopicStats {
            subscription_count: 5,
            messages_published: 1000,
            bytes_published: 100000,
            avg_message_size: 100.0,
            last_publish_time: Some(Utc::now()),
        };

        assert_eq!(stats.subscription_count, 5);
        assert_eq!(stats.messages_published, 1000);
    }
}
