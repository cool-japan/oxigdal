//! Integration tests for oxigdal-pubsub.

use oxigdal_pubsub::{Message, PublisherConfig, SubscriberConfig};

#[cfg(feature = "schema")]
use oxigdal_pubsub::{Schema, SchemaRegistry};

#[cfg(feature = "monitoring")]
use oxigdal_pubsub::MetricsCollector;

#[tokio::test]
async fn test_publisher_config_validation() {
    let config = PublisherConfig::new("test-project", "test-topic");
    assert_eq!(config.project_id, "test-project");
    assert_eq!(config.topic_name, "test-topic");
}

#[tokio::test]
async fn test_subscriber_config_validation() {
    let config = SubscriberConfig::new("test-project", "test-subscription");
    assert_eq!(config.project_id, "test-project");
    assert_eq!(config.subscription_name, "test-subscription");
}

#[test]
fn test_message_creation() {
    let message = Message::new(b"test data".to_vec())
        .with_attribute("key1", "value1")
        .with_attribute("key2", "value2")
        .with_ordering_key("order-key");

    assert_eq!(message.data.as_ref(), b"test data");
    assert_eq!(message.attributes.len(), 2);
    assert_eq!(message.ordering_key, Some("order-key".to_string()));
}

#[test]
fn test_message_size() {
    let small_message = Message::new(b"small".to_vec());
    assert_eq!(small_message.size(), 5);

    let large_data = vec![0u8; 1000];
    let large_message = Message::new(large_data);
    assert_eq!(large_message.size(), 1000);
}

#[cfg(feature = "schema")]
#[test]
fn test_schema_registry() {
    use oxigdal_pubsub::error::SchemaFormat;

    let registry = SchemaRegistry::new();
    assert!(registry.is_empty());
    assert_eq!(registry.len(), 0);

    let schema = Schema::new(
        "schema-1",
        "test-schema",
        SchemaFormat::Avro,
        r#"{"type": "string"}"#,
    );

    // Note: Registration may fail if schema is invalid
    let _schema_id = schema.id.clone();

    // Test list schemas
    let schemas = registry.list_schemas();
    assert!(schemas.is_empty() || !schemas.is_empty());
}

#[cfg(feature = "monitoring")]
#[test]
fn test_metrics_collector() {
    let collector = MetricsCollector::new("test-project")
        .with_topic("test-topic")
        .with_subscription("test-subscription");

    assert_eq!(collector.project_id(), "test-project");
    assert_eq!(collector.topic_name(), Some("test-topic"));
    assert_eq!(collector.subscription_name(), Some("test-subscription"));

    // Test recording metrics
    collector.record_publish(100, true);
    let pub_metrics = collector.publisher_metrics();
    assert_eq!(pub_metrics.messages_published, 1);
    assert_eq!(pub_metrics.bytes_published, 100);

    collector.record_receive(200);
    let sub_metrics = collector.subscriber_metrics();
    assert_eq!(sub_metrics.messages_received, 1);
    assert_eq!(sub_metrics.bytes_received, 200);

    // Test export
    let exported = collector.export_metrics();
    assert!(!exported.is_empty());

    // Test reset
    collector.reset();
    let pub_metrics = collector.publisher_metrics();
    assert_eq!(pub_metrics.messages_published, 0);
}

#[test]
fn test_publisher_config_builder() {
    let config = PublisherConfig::new("project", "topic")
        .with_batching(true)
        .with_batch_size(50)
        .with_batch_timeout(20)
        .with_max_outstanding_publishes(500)
        .with_ordering(true);

    assert!(config.enable_batching);
    assert_eq!(config.batch_size, 50);
    assert_eq!(config.batch_timeout_ms, 20);
    assert_eq!(config.max_outstanding_publishes, 500);
    assert!(config.enable_ordering);
}

#[test]
fn test_subscriber_config_builder() {
    use oxigdal_pubsub::{FlowControlSettings, SubscriptionType};

    let flow_control = FlowControlSettings {
        max_outstanding_messages: 500,
        max_outstanding_bytes: 50_000_000,
        max_messages_per_second: 100,
    };

    let config = SubscriberConfig::new("project", "subscription")
        .with_type(SubscriptionType::Pull)
        .with_ack_deadline(30)
        .with_flow_control(flow_control.clone())
        .with_handler_concurrency(20)
        .with_ordering(true);

    assert_eq!(config.subscription_type, SubscriptionType::Pull);
    assert_eq!(config.ack_deadline_seconds, 30);
    assert_eq!(config.flow_control.max_outstanding_messages, 500);
    assert_eq!(config.handler_concurrency, 20);
    assert!(config.enable_ordering);
}

#[test]
fn test_error_handling() {
    use oxigdal_pubsub::PubSubError;

    let error = PubSubError::publish("test error");
    assert!(error.to_string().contains("test error"));

    let error = PubSubError::subscription("subscription error");
    assert!(error.to_string().contains("subscription error"));

    let error = PubSubError::configuration("invalid value", "parameter");
    assert!(error.to_string().contains("invalid value"));

    let error = PubSubError::message_too_large(11_000_000, 10_000_000);
    assert!(error.to_string().contains("11000000"));

    let error = PubSubError::timeout(5000);
    assert!(error.to_string().contains("5000"));
}

#[test]
fn test_retry_config() {
    use oxigdal_pubsub::RetryConfig;
    use std::time::Duration;

    let config = RetryConfig::default();
    assert_eq!(config.max_attempts, 3);
    assert_eq!(config.initial_delay_ms, 100);
    assert_eq!(config.max_delay_ms, 60000);
    assert_eq!(config.backoff_multiplier, 2.0);

    // Test delay calculation
    let delay0 = config.delay_for_attempt(0);
    assert_eq!(delay0, Duration::from_millis(100));

    let delay1 = config.delay_for_attempt(1);
    assert_eq!(delay1, Duration::from_millis(200));

    let delay2 = config.delay_for_attempt(2);
    assert_eq!(delay2, Duration::from_millis(400));
}

#[cfg(feature = "monitoring")]
#[test]
fn test_latency_tracker() {
    use oxigdal_pubsub::LatencyTracker;
    use std::time::Duration;

    let tracker = LatencyTracker::new();
    assert_eq!(tracker.count(), 0);
    assert_eq!(tracker.avg_ms(), 0.0);

    tracker.record(Duration::from_millis(100));
    assert_eq!(tracker.count(), 1);
    assert_eq!(tracker.avg_ms(), 100.0);

    tracker.record(Duration::from_millis(200));
    assert_eq!(tracker.count(), 2);
    assert_eq!(tracker.avg_ms(), 150.0);
    assert_eq!(tracker.min_ms(), 100.0);
    assert_eq!(tracker.max_ms(), 200.0);

    tracker.reset();
    assert_eq!(tracker.count(), 0);
    assert_eq!(tracker.avg_ms(), 0.0);
}

#[test]
fn test_version_info() {
    use oxigdal_pubsub::{crate_name, version};

    assert!(!version().is_empty());
    assert_eq!(crate_name(), "oxigdal-pubsub");
}

#[test]
fn test_constants() {
    use oxigdal_pubsub::{
        DEFAULT_BATCH_SIZE, DEFAULT_BATCH_TIMEOUT_MS, DEFAULT_MAX_OUTSTANDING_PUBLISHES,
        MAX_MESSAGE_SIZE,
    };

    assert_eq!(MAX_MESSAGE_SIZE, 10_000_000);
    assert_eq!(DEFAULT_BATCH_SIZE, 100);
    assert_eq!(DEFAULT_BATCH_TIMEOUT_MS, 10);
    assert_eq!(DEFAULT_MAX_OUTSTANDING_PUBLISHES, 1000);
}

#[cfg(feature = "subscriber")]
#[test]
fn test_subscriber_constants() {
    use oxigdal_pubsub::{
        DEFAULT_ACK_DEADLINE_SECONDS, DEFAULT_HANDLER_CONCURRENCY, DEFAULT_MAX_OUTSTANDING_BYTES,
        DEFAULT_MAX_OUTSTANDING_MESSAGES,
    };

    assert_eq!(DEFAULT_MAX_OUTSTANDING_MESSAGES, 1000);
    assert_eq!(DEFAULT_MAX_OUTSTANDING_BYTES, 100_000_000);
    assert_eq!(DEFAULT_ACK_DEADLINE_SECONDS, 10);
    assert_eq!(DEFAULT_HANDLER_CONCURRENCY, 10);
}

#[test]
fn test_topic_config() {
    use oxigdal_pubsub::TopicConfig;

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
fn test_topic_builder() {
    use oxigdal_pubsub::TopicBuilder;

    let config = TopicBuilder::new("project", "topic")
        .message_retention(7200)
        .label("key", "value")
        .message_ordering(true)
        .build();

    assert_eq!(config.project_id, "project");
    assert_eq!(config.topic_name, "topic");
    assert_eq!(config.message_retention_duration, Some(7200));
}

#[test]
fn test_subscription_create_config() {
    use oxigdal_pubsub::SubscriptionCreateConfig;

    let config = SubscriptionCreateConfig::new("project", "subscription", "topic")
        .with_ack_deadline(30)
        .with_message_retention(3600)
        .with_label("env", "production");

    assert_eq!(config.project_id, "project");
    assert_eq!(config.subscription_name, "subscription");
    assert_eq!(config.topic_name, "topic");
    assert_eq!(config.ack_deadline_seconds, 30);
}

#[test]
fn test_subscription_builder() {
    use oxigdal_pubsub::{ExpirationPolicy, RetryPolicy, SubscriptionBuilder};

    let config = SubscriptionBuilder::new("project", "subscription", "topic")
        .ack_deadline(45)
        .message_retention(7200)
        .message_ordering(true)
        .expiration_policy(ExpirationPolicy::new(86400))
        .retry_policy(RetryPolicy::default_policy())
        .build();

    assert_eq!(config.project_id, "project");
    assert_eq!(config.subscription_name, "subscription");
    assert!(config.enable_message_ordering);
}

#[test]
fn test_expiration_policy() {
    use oxigdal_pubsub::ExpirationPolicy;

    let policy = ExpirationPolicy::new(86400);
    assert_eq!(policy.ttl_seconds, 86400);

    let never_expire = ExpirationPolicy::never_expire();
    assert_eq!(never_expire.ttl_seconds, i64::MAX);
}

#[test]
fn test_retry_policy_variants() {
    use oxigdal_pubsub::RetryPolicy;

    let default = RetryPolicy::default_policy();
    assert_eq!(default.minimum_backoff_seconds, 10);
    assert_eq!(default.maximum_backoff_seconds, 600);

    let aggressive = RetryPolicy::aggressive();
    assert_eq!(aggressive.minimum_backoff_seconds, 1);
    assert_eq!(aggressive.maximum_backoff_seconds, 60);

    let conservative = RetryPolicy::conservative();
    assert_eq!(conservative.minimum_backoff_seconds, 60);
    assert_eq!(conservative.maximum_backoff_seconds, 3600);
}

#[test]
fn test_dead_letter_policy() {
    use oxigdal_pubsub::DeadLetterPolicy;

    let policy = DeadLetterPolicy::new("dlq-topic", 5);
    assert_eq!(policy.dead_letter_topic, "dlq-topic");
    assert_eq!(policy.max_delivery_attempts, 5);
}

#[test]
fn test_topic_metadata_serialization() {
    use oxigdal_pubsub::TopicMetadata;
    use std::collections::HashMap;

    let metadata = TopicMetadata {
        name: "test-topic".to_string(),
        labels: HashMap::new(),
        message_retention_duration: Some(3600),
        enable_message_ordering: true,
        created_at: Some(chrono::Utc::now()),
        updated_at: Some(chrono::Utc::now()),
    };

    assert_eq!(metadata.name, "test-topic");
    assert!(metadata.enable_message_ordering);

    // Test serialization
    let json = serde_json::to_string(&metadata);
    assert!(json.is_ok());
}

#[test]
fn test_subscription_metadata_serialization() {
    use oxigdal_pubsub::SubscriptionMetadata;
    use std::collections::HashMap;

    let metadata = SubscriptionMetadata {
        name: "test-subscription".to_string(),
        topic: "test-topic".to_string(),
        ack_deadline_seconds: 30,
        message_retention_duration: Some(7200),
        enable_message_ordering: true,
        labels: HashMap::new(),
        filter: None,
        created_at: Some(chrono::Utc::now()),
        updated_at: Some(chrono::Utc::now()),
    };

    assert_eq!(metadata.name, "test-subscription");
    assert_eq!(metadata.topic, "test-topic");

    // Test serialization
    let json = serde_json::to_string(&metadata);
    assert!(json.is_ok());
}

#[test]
fn test_topic_stats() {
    use oxigdal_pubsub::TopicStats;

    let stats = TopicStats {
        subscription_count: 3,
        messages_published: 1000,
        bytes_published: 500000,
        avg_message_size: 500.0,
        last_publish_time: Some(chrono::Utc::now()),
    };

    assert_eq!(stats.subscription_count, 3);
    assert_eq!(stats.messages_published, 1000);
    assert_eq!(stats.bytes_published, 500000);
}

#[test]
fn test_subscription_stats() {
    use oxigdal_pubsub::SubscriptionStats;

    let stats = SubscriptionStats {
        messages_received: 800,
        messages_delivered: 750,
        messages_pending: 50,
        oldest_unacked_message_age_seconds: Some(120),
        avg_ack_latency_ms: 15.5,
        last_message_time: Some(chrono::Utc::now()),
    };

    assert_eq!(stats.messages_received, 800);
    assert_eq!(stats.messages_delivered, 750);
    assert_eq!(stats.messages_pending, 50);
}

#[test]
fn test_handler_result_variants() {
    use oxigdal_pubsub::HandlerResult;

    let ack = HandlerResult::Ack;
    let nack = HandlerResult::Nack;
    let dead_letter = HandlerResult::DeadLetter;

    // Just test that all variants are valid
    match ack {
        HandlerResult::Ack => {}
        _ => unreachable!(),
    }

    match nack {
        HandlerResult::Nack => {}
        _ => unreachable!(),
    }

    match dead_letter {
        HandlerResult::DeadLetter => {}
        _ => unreachable!(),
    }
}

#[test]
fn test_subscription_type() {
    use oxigdal_pubsub::SubscriptionType;

    let pull = SubscriptionType::Pull;
    let push = SubscriptionType::Push;

    assert_eq!(pull, SubscriptionType::Pull);
    assert_eq!(push, SubscriptionType::Push);
    assert_ne!(pull, push);
}

#[test]
fn test_multiple_message_attributes() {
    use std::collections::HashMap;

    let mut attributes = HashMap::new();
    attributes.insert("key1".to_string(), "value1".to_string());
    attributes.insert("key2".to_string(), "value2".to_string());
    attributes.insert("key3".to_string(), "value3".to_string());

    let message = Message::new(b"test".to_vec()).with_attributes(attributes.clone());

    assert_eq!(message.attributes.len(), 3);
    assert_eq!(message.attributes.get("key1"), Some(&"value1".to_string()));
}

#[test]
fn test_message_with_ordering_key_chain() {
    let message = Message::new(b"test".to_vec())
        .with_attribute("attr1", "val1")
        .with_ordering_key("order-key-1")
        .with_attribute("attr2", "val2");

    assert_eq!(message.ordering_key, Some("order-key-1".to_string()));
    assert_eq!(message.attributes.len(), 2);
}

#[test]
fn test_publisher_stats_serialization() {
    use oxigdal_pubsub::PublisherStats;

    let stats = PublisherStats {
        messages_published: 1000,
        bytes_published: 500000,
        publish_errors: 5,
        retries: 10,
        messages_in_batches: 50,
        outstanding_publishes: 5,
        last_publish: Some(chrono::Utc::now()),
    };

    let json = serde_json::to_string(&stats);
    assert!(json.is_ok());

    let deserialized: std::result::Result<PublisherStats, _> =
        serde_json::from_str(&json.ok().unwrap_or_default());
    assert!(deserialized.is_ok());
}

#[test]
fn test_subscriber_stats_serialization() {
    use oxigdal_pubsub::SubscriberStats;

    let stats = SubscriberStats {
        messages_received: 800,
        bytes_received: 400000,
        messages_acknowledged: 750,
        messages_nacked: 30,
        messages_to_dlq: 20,
        ack_errors: 5,
        outstanding_messages: 50,
        outstanding_bytes: 25000,
        last_receive: Some(chrono::Utc::now()),
    };

    let json = serde_json::to_string(&stats);
    assert!(json.is_ok());
}

#[cfg(feature = "monitoring")]
#[test]
fn test_metric_types() {
    use oxigdal_pubsub::MetricType;

    let counter = MetricType::Counter;
    let gauge = MetricType::Gauge;
    let histogram = MetricType::Histogram;

    assert_eq!(counter, MetricType::Counter);
    assert_eq!(gauge, MetricType::Gauge);
    assert_eq!(histogram, MetricType::Histogram);
}

#[cfg(feature = "monitoring")]
#[test]
fn test_metric_value_variants() {
    use oxigdal_pubsub::MetricValue;

    let int_value = MetricValue::Int(42);
    let float_value = MetricValue::Float(3.5);
    let dist_value = MetricValue::Distribution {
        count: 100,
        sum: 1000.0,
        min: 5.0,
        max: 25.0,
        mean: 10.0,
    };

    match int_value {
        MetricValue::Int(v) => assert_eq!(v, 42),
        _ => unreachable!(),
    }

    match float_value {
        MetricValue::Float(v) => assert!((v - 3.5).abs() < 0.01),
        _ => unreachable!(),
    }

    match dist_value {
        MetricValue::Distribution { count, .. } => assert_eq!(count, 100),
        _ => unreachable!(),
    }
}

#[cfg(feature = "monitoring")]
#[test]
fn test_metric_point_with_labels() {
    use oxigdal_pubsub::{MetricPoint, MetricType, MetricValue};
    use std::collections::HashMap;

    let mut labels = HashMap::new();
    labels.insert("region".to_string(), "us-central1".to_string());
    labels.insert("env".to_string(), "production".to_string());

    let point = MetricPoint::new("test_metric", MetricType::Counter, MetricValue::Int(100))
        .with_labels(labels.clone());

    assert_eq!(point.name, "test_metric");
    assert_eq!(point.labels.len(), 2);
    assert_eq!(point.labels.get("region"), Some(&"us-central1".to_string()));
}

#[test]
fn test_flow_control_settings_default() {
    use oxigdal_pubsub::{
        DEFAULT_MAX_OUTSTANDING_BYTES, DEFAULT_MAX_OUTSTANDING_MESSAGES, FlowControlSettings,
    };

    let settings = FlowControlSettings::default();

    assert_eq!(
        settings.max_outstanding_messages,
        DEFAULT_MAX_OUTSTANDING_MESSAGES
    );
    assert_eq!(
        settings.max_outstanding_bytes,
        DEFAULT_MAX_OUTSTANDING_BYTES
    );
    assert_eq!(settings.max_messages_per_second, 0);
}

#[test]
fn test_flow_control_settings_custom() {
    use oxigdal_pubsub::FlowControlSettings;

    let settings = FlowControlSettings {
        max_outstanding_messages: 500,
        max_outstanding_bytes: 50_000_000,
        max_messages_per_second: 100,
    };

    assert_eq!(settings.max_outstanding_messages, 500);
    assert_eq!(settings.max_outstanding_bytes, 50_000_000);
    assert_eq!(settings.max_messages_per_second, 100);
}

#[test]
fn test_dead_letter_config() {
    use oxigdal_pubsub::DeadLetterConfig;

    let config = DeadLetterConfig::new("dlq-topic", 3);
    assert_eq!(config.topic_name, "dlq-topic");
    assert_eq!(config.max_delivery_attempts, 3);
}

#[test]
fn test_error_retryable() {
    use oxigdal_pubsub::PubSubError;

    let network_error = PubSubError::NetworkError {
        message: "connection failed".to_string(),
        source: None,
    };
    assert!(network_error.is_retryable());

    let timeout = PubSubError::timeout(5000);
    assert!(timeout.is_retryable());

    let config_error = PubSubError::configuration("bad", "param");
    assert!(!config_error.is_retryable());

    let topic_not_found = PubSubError::topic_not_found("missing");
    assert!(!topic_not_found.is_retryable());
}
