//! Apache Kafka integration for OxiGeo
//!
//! This crate provides comprehensive Apache Kafka support for OxiGeo, including:
//!
//! - **Producer**: Async producer with batching, partitioning, and compression
//! - **Consumer**: Async consumer with consumer groups and offset management
//! - **Schema Registry**: Avro schema management with evolution support
//! - **Transactions**: Transactional producer/consumer with exactly-once semantics
//!
//! # Features
//!
//! - `producer` - Enable producer functionality (default; pulls in `backend-rdkafka`)
//! - `consumer` - Enable consumer functionality (default; pulls in `backend-rdkafka`)
//! - `schema-registry` - Enable Avro schema registry support
//! - `transactions` - Enable transactional support (pulls in `backend-rdkafka`)
//! - `compression-*` - Enable various compression algorithms
//! - `backend-rdkafka` - Enable the C `librdkafka` FFI backend. Building
//!   with `--no-default-features` (or otherwise without `producer`,
//!   `consumer`, or `transactions`) yields a Pure Rust build exposing only
//!   the message/schema/config data types, with no rdkafka symbol in the
//!   dependency graph.
//!
//! # Example
//!
//! ```no_run
//! use oxigeo_kafka::producer::{ProducerConfig, KafkaProducer};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = ProducerConfig::builder()
//!     .bootstrap_servers("localhost:9092")
//!     .build()?;
//!
//! let producer = KafkaProducer::new(config).await?;
//! producer.send("my-topic", Some(b"key".as_slice()), b"value").await?;
//! # Ok(())
//! # }
//! ```

#![cfg_attr(not(feature = "std"), no_std)]
#![deny(missing_docs, unsafe_code, clippy::unwrap_used, clippy::panic)]
#![warn(clippy::expect_used)]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

pub mod config;
pub mod error;

#[cfg(feature = "producer")]
pub mod producer;

#[cfg(feature = "consumer")]
pub mod consumer;

#[cfg(feature = "schema-registry")]
pub mod schema_registry;

#[cfg(feature = "transactions")]
pub mod transactions;

mod utils;

// Re-exports
pub use error::{Error, Result};

#[cfg(feature = "producer")]
pub use producer::{KafkaProducer, ProducerConfig};

#[cfg(feature = "consumer")]
pub use consumer::{ConsumerConfig, KafkaConsumer};

#[cfg(feature = "schema-registry")]
pub use schema_registry::{SchemaRegistry, SchemaRegistryConfig};

#[cfg(feature = "transactions")]
pub use transactions::{TransactionConfig, TransactionalProducer};
