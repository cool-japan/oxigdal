//! Transactional producer and consumer support

mod config;
mod coordinator;
mod producer;

pub use config::{TransactionConfig, TransactionConfigBuilder};
pub use coordinator::{TransactionCoordinator, TransactionState};
pub use producer::{TransactionScope, TransactionalProducer};

use std::time::Duration;

/// Transaction ID generator
pub struct TransactionIdGenerator {
    prefix: String,
    counter: std::sync::atomic::AtomicU64,
}

impl TransactionIdGenerator {
    /// Create a new transaction ID generator
    pub fn new(prefix: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
            counter: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Generate a new transaction ID
    pub fn next(&self) -> String {
        let id = self
            .counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        format!("{}-{}", self.prefix, id)
    }
}

/// Transaction metadata
#[derive(Debug, Clone)]
pub struct TransactionMetadata {
    /// Transaction ID
    pub transaction_id: String,
    /// Producer ID
    pub producer_id: i64,
    /// Producer epoch
    pub producer_epoch: i16,
    /// Transaction timeout
    pub timeout: Duration,
    /// Start time
    pub start_time: std::time::Instant,
}

impl TransactionMetadata {
    /// Create new transaction metadata
    pub fn new(transaction_id: String, timeout: Duration) -> Self {
        Self {
            transaction_id,
            producer_id: -1,
            producer_epoch: -1,
            timeout,
            start_time: std::time::Instant::now(),
        }
    }

    /// Check if transaction is expired
    pub fn is_expired(&self) -> bool {
        self.start_time.elapsed() >= self.timeout
    }

    /// Get elapsed time
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_id_generator() {
        let generator = TransactionIdGenerator::new("test");

        let id1 = generator.next();
        let id2 = generator.next();
        let id3 = generator.next();

        assert_eq!(id1, "test-0");
        assert_eq!(id2, "test-1");
        assert_eq!(id3, "test-2");
    }

    #[test]
    fn test_transaction_metadata() {
        let metadata = TransactionMetadata::new("test-txn".to_string(), Duration::from_secs(60));

        assert_eq!(metadata.transaction_id, "test-txn");
        assert!(!metadata.is_expired());
        assert!(metadata.elapsed() < Duration::from_secs(1));
    }
}
