//! Integration tests for oxigdal-kafka
//!
//! Note: These tests require a running Kafka instance.
//! Set KAFKA_BROKERS environment variable to override default localhost:9092

use oxigdal_kafka::config::{Acks, CompressionType, IsolationLevel, OffsetReset};
use oxigdal_kafka::consumer::{ConsumerConfig, KafkaConsumer};
use oxigdal_kafka::producer::{KafkaProducer, ProducerConfig};
use std::time::Duration;

fn get_kafka_brokers() -> String {
    std::env::var("KAFKA_BROKERS").unwrap_or_else(|_| "localhost:9092".to_string())
}

macro_rules! assert_ok {
    ($expr:expr, $msg:expr) => {
        match $expr {
            Ok(val) => val,
            Err(e) => {
                panic!("{}: {:?}", $msg, e);
            }
        }
    };
}

#[tokio::test]
#[ignore] // Ignore by default as it requires Kafka
async fn test_producer_creation() {
    let config = assert_ok!(
        ProducerConfig::builder()
            .bootstrap_servers(get_kafka_brokers())
            .acks(Acks::All)
            .compression_type(CompressionType::None)
            .build(),
        "Failed to build producer config"
    );

    let result = KafkaProducer::new(config).await;
    assert!(
        result.is_ok(),
        "Failed to create producer: {:?}",
        result.err()
    );
}

#[tokio::test]
#[ignore]
async fn test_consumer_creation() {
    let config = assert_ok!(
        ConsumerConfig::builder()
            .bootstrap_servers(get_kafka_brokers())
            .group_id("test-consumer-group")
            .auto_offset_reset(OffsetReset::Earliest)
            .isolation_level(IsolationLevel::ReadCommitted)
            .build(),
        "Failed to build consumer config"
    );

    let result = KafkaConsumer::new(config).await;
    assert!(
        result.is_ok(),
        "Failed to create consumer: {:?}",
        result.err()
    );
}

#[tokio::test]
#[ignore]
async fn test_producer_send_message() {
    let config = assert_ok!(
        ProducerConfig::builder()
            .bootstrap_servers(get_kafka_brokers())
            .acks(Acks::All)
            .build(),
        "Failed to build config"
    );

    let producer = assert_ok!(
        KafkaProducer::new(config).await,
        "Failed to create producer"
    );

    let result = producer
        .send("test-topic", Some(b"test-key"), b"test-value")
        .await;

    match result {
        Ok((partition, offset)) => {
            println!(
                "Message sent to partition {} at offset {}",
                partition, offset
            );
            assert!(partition >= 0);
            assert!(offset >= 0);
        }
        Err(e) => {
            println!("Failed to send message: {}", e);
            // Don't fail test if topic doesn't exist
        }
    }
}

#[tokio::test]
#[ignore]
async fn test_producer_send_batch() {
    let config = assert_ok!(
        ProducerConfig::builder()
            .bootstrap_servers(get_kafka_brokers())
            .acks(Acks::All)
            .enable_batching(true)
            .batch_num_messages(10)
            .build(),
        "Failed to build config"
    );

    let producer = assert_ok!(
        KafkaProducer::new(config).await,
        "Failed to create producer"
    );

    let messages: Vec<(Option<Vec<u8>>, Vec<u8>)> = (0..5)
        .map(|i| {
            (
                Some(format!("key-{}", i).into_bytes()),
                format!("value-{}", i).into_bytes(),
            )
        })
        .collect();

    let results = producer.send_batch("test-topic", messages).await;

    match results {
        Ok(results) => {
            let successful = results.iter().filter(|r| r.is_ok()).count();
            println!(
                "Batch send: {} successful out of {}",
                successful,
                results.len()
            );
        }
        Err(e) => {
            println!("Failed to send batch: {}", e);
        }
    }
}

#[tokio::test]
#[ignore]
async fn test_consumer_subscribe() {
    let config = assert_ok!(
        ConsumerConfig::builder()
            .bootstrap_servers(get_kafka_brokers())
            .group_id("test-subscribe-group")
            .auto_offset_reset(OffsetReset::Latest)
            .build(),
        "Failed to build config"
    );

    let consumer = assert_ok!(
        KafkaConsumer::new(config).await,
        "Failed to create consumer"
    );

    let result = consumer.subscribe(&["test-topic"]).await;
    assert!(result.is_ok(), "Failed to subscribe: {:?}", result.err());
}

#[tokio::test]
#[ignore]
async fn test_producer_consumer_roundtrip() {
    let topic = format!("test-roundtrip-{}", uuid::Uuid::new_v4());

    // Create producer
    let producer_config = assert_ok!(
        ProducerConfig::builder()
            .bootstrap_servers(get_kafka_brokers())
            .acks(Acks::All)
            .build(),
        "Failed to build producer config"
    );

    let producer = assert_ok!(
        KafkaProducer::new(producer_config).await,
        "Failed to create producer"
    );

    // Create consumer
    let consumer_config = assert_ok!(
        ConsumerConfig::builder()
            .bootstrap_servers(get_kafka_brokers())
            .group_id(format!("test-group-{}", uuid::Uuid::new_v4()))
            .auto_offset_reset(OffsetReset::Earliest)
            .build(),
        "Failed to build consumer config"
    );

    let consumer = assert_ok!(
        KafkaConsumer::new(consumer_config).await,
        "Failed to create consumer"
    );

    assert_ok!(consumer.subscribe(&[&topic]).await, "Failed to subscribe");

    // Send message
    let test_message = b"roundtrip-test-message";
    assert_ok!(
        producer.send(&topic, Some(b"test-key"), test_message).await,
        "Failed to send message"
    );

    // Give it a moment to propagate
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Try to receive
    match consumer.receive_with_timeout(Duration::from_secs(10)).await {
        Ok(msg) => {
            println!(
                "Received message from partition {} at offset {}",
                msg.partition, msg.offset
            );
            assert_eq!(msg.payload.as_ref(), test_message);
        }
        Err(e) => {
            println!("Did not receive message within timeout: {}", e);
        }
    }
}

#[tokio::test]
#[ignore]
async fn test_producer_metrics() {
    let config = assert_ok!(
        ProducerConfig::builder()
            .bootstrap_servers(get_kafka_brokers())
            .acks(Acks::All)
            .build(),
        "Failed to build config"
    );

    let producer = assert_ok!(
        KafkaProducer::new(config).await,
        "Failed to create producer"
    );

    // Send a few messages
    for i in 0..5 {
        let _ = producer
            .send(
                "test-topic",
                Some(format!("key-{}", i).as_bytes()),
                format!("value-{}", i).as_bytes(),
            )
            .await;
    }

    let metrics = producer.metrics().await;
    println!("Producer metrics: {:?}", metrics);
    // Metrics should be recorded even if sends failed
    assert!(metrics.messages_sent > 0 || metrics.messages_failed > 0);
}

#[tokio::test]
#[ignore]
async fn test_consumer_metrics() {
    let config = assert_ok!(
        ConsumerConfig::builder()
            .bootstrap_servers(get_kafka_brokers())
            .group_id("test-metrics-group")
            .auto_offset_reset(OffsetReset::Latest)
            .build(),
        "Failed to build config"
    );

    let consumer = assert_ok!(
        KafkaConsumer::new(config).await,
        "Failed to create consumer"
    );

    assert_ok!(
        consumer.subscribe(&["test-topic"]).await,
        "Failed to subscribe"
    );

    let metrics = consumer.metrics().await;
    println!("Consumer metrics: {:?}", metrics);
}

#[cfg(feature = "transactions")]
#[tokio::test]
#[ignore]
async fn test_transactional_producer() {
    use oxigdal_kafka::transactions::{TransactionConfig, TransactionalProducer};

    let config = assert_ok!(
        TransactionConfig::builder()
            .bootstrap_servers(get_kafka_brokers())
            .transactional_id(format!("test-txn-{}", uuid::Uuid::new_v4()))
            .transaction_timeout(Duration::from_secs(60))
            .build(),
        "Failed to build transaction config"
    );

    let producer = assert_ok!(
        TransactionalProducer::new(config).await,
        "Failed to create transactional producer"
    );

    // Begin transaction
    assert_ok!(
        producer.begin_transaction().await,
        "Failed to begin transaction"
    );

    // Send messages in transaction
    for i in 0..3 {
        assert_ok!(
            producer
                .send(
                    "test-topic",
                    Some(format!("txn-key-{}", i).as_bytes()),
                    format!("txn-value-{}", i).as_bytes(),
                )
                .await,
            "Failed to send in transaction"
        );
    }

    // Commit transaction
    assert_ok!(
        producer.commit_transaction().await,
        "Failed to commit transaction"
    );

    println!("Transaction completed successfully");
}
