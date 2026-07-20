//! Transaction coordinator for managing transaction state

use crate::error::{Error, Result};
use crate::transactions::TransactionMetadata;

/// Transaction state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionState {
    /// No transaction in progress
    None,
    /// Transaction in progress
    InProgress,
    /// Transaction being committed
    Committing,
    /// Transaction committed
    Committed,
    /// Transaction being aborted
    Aborting,
    /// Transaction aborted
    Aborted,
    /// Transaction failed
    Failed,
}

/// Transaction coordinator for managing transaction lifecycle
pub struct TransactionCoordinator {
    /// Transaction ID (stored for debugging and identification)
    #[allow(dead_code)]
    transaction_id: String,
    /// Current state
    state: TransactionState,
    /// Current transaction metadata
    current_transaction: Option<TransactionMetadata>,
    /// Transaction history
    history: Vec<TransactionRecord>,
}

/// Record of a completed transaction
#[derive(Debug, Clone)]
pub struct TransactionRecord {
    /// Transaction metadata
    pub metadata: TransactionMetadata,
    /// Final state
    pub state: TransactionState,
    /// Completion time
    pub completed_at: std::time::Instant,
}

impl TransactionCoordinator {
    /// Create a new transaction coordinator
    pub fn new(transaction_id: String) -> Self {
        Self {
            transaction_id,
            state: TransactionState::None,
            current_transaction: None,
            history: Vec::new(),
        }
    }

    /// Begin a new transaction
    pub fn begin_transaction(&mut self, metadata: TransactionMetadata) {
        if self.state != TransactionState::None {
            // This shouldn't happen, but handle gracefully
            self.abort_transaction();
        }

        self.current_transaction = Some(metadata);
        self.state = TransactionState::InProgress;
    }

    /// Commit the current transaction
    pub fn commit_transaction(&mut self) -> Result<()> {
        if self.state != TransactionState::InProgress {
            return Err(Error::InvalidState(format!(
                "Cannot commit transaction in state: {:?}",
                self.state
            )));
        }

        self.state = TransactionState::Committing;

        if let Some(metadata) = self.current_transaction.take() {
            self.history.push(TransactionRecord {
                metadata,
                state: TransactionState::Committed,
                completed_at: std::time::Instant::now(),
            });
        }

        self.state = TransactionState::Committed;

        // Reset to None after commit
        self.state = TransactionState::None;

        Ok(())
    }

    /// Abort the current transaction
    pub fn abort_transaction(&mut self) {
        if self.state == TransactionState::None {
            return;
        }

        self.state = TransactionState::Aborting;

        if let Some(metadata) = self.current_transaction.take() {
            self.history.push(TransactionRecord {
                metadata,
                state: TransactionState::Aborted,
                completed_at: std::time::Instant::now(),
            });
        }

        self.state = TransactionState::Aborted;

        // Reset to None after abort
        self.state = TransactionState::None;
    }

    /// Mark transaction as failed
    pub fn fail_transaction(&mut self, _error: String) {
        self.state = TransactionState::Failed;

        if let Some(metadata) = self.current_transaction.take() {
            self.history.push(TransactionRecord {
                metadata,
                state: TransactionState::Failed,
                completed_at: std::time::Instant::now(),
            });
        }

        self.state = TransactionState::None;
    }

    /// Get current state
    pub fn state(&self) -> TransactionState {
        self.state
    }

    /// Get current transaction metadata
    pub fn current_transaction(&self) -> Option<&TransactionMetadata> {
        self.current_transaction.as_ref()
    }

    /// Get transaction history
    pub fn history(&self) -> &[TransactionRecord] {
        &self.history
    }

    /// Get statistics
    pub fn statistics(&self) -> TransactionStatistics {
        let mut stats = TransactionStatistics::default();

        for record in &self.history {
            stats.total_transactions += 1;
            match record.state {
                TransactionState::Committed => stats.committed += 1,
                TransactionState::Aborted => stats.aborted += 1,
                TransactionState::Failed => stats.failed += 1,
                _ => {}
            }

            let duration = record.metadata.elapsed();
            if duration > stats.max_duration {
                stats.max_duration = duration;
            }
            if stats.min_duration.is_zero() || duration < stats.min_duration {
                stats.min_duration = duration;
            }
            stats.total_duration += duration;
        }

        if stats.total_transactions > 0 {
            stats.avg_duration = stats.total_duration / stats.total_transactions as u32;
        }

        stats
    }

    /// Clear history
    pub fn clear_history(&mut self) {
        self.history.clear();
    }
}

/// Transaction statistics
#[derive(Debug, Default, Clone)]
pub struct TransactionStatistics {
    /// Total number of transactions
    pub total_transactions: u64,
    /// Number of committed transactions
    pub committed: u64,
    /// Number of aborted transactions
    pub aborted: u64,
    /// Number of failed transactions
    pub failed: u64,
    /// Minimum transaction duration
    pub min_duration: std::time::Duration,
    /// Maximum transaction duration
    pub max_duration: std::time::Duration,
    /// Average transaction duration
    pub avg_duration: std::time::Duration,
    /// Total transaction duration
    pub total_duration: std::time::Duration,
}

impl TransactionStatistics {
    /// Get success rate
    pub fn success_rate(&self) -> f64 {
        if self.total_transactions == 0 {
            1.0
        } else {
            self.committed as f64 / self.total_transactions as f64
        }
    }

    /// Get abort rate
    pub fn abort_rate(&self) -> f64 {
        if self.total_transactions == 0 {
            0.0
        } else {
            self.aborted as f64 / self.total_transactions as f64
        }
    }

    /// Get failure rate
    pub fn failure_rate(&self) -> f64 {
        if self.total_transactions == 0 {
            0.0
        } else {
            self.failed as f64 / self.total_transactions as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_transaction_coordinator() {
        let mut coordinator = TransactionCoordinator::new("test-txn".to_string());

        assert_eq!(coordinator.state(), TransactionState::None);

        // Begin transaction
        let metadata = TransactionMetadata::new("test-txn".to_string(), Duration::from_secs(60));
        coordinator.begin_transaction(metadata);
        assert_eq!(coordinator.state(), TransactionState::InProgress);

        // Commit transaction
        coordinator.commit_transaction().ok();
        assert_eq!(coordinator.state(), TransactionState::None);
        assert_eq!(coordinator.history().len(), 1);
    }

    #[test]
    fn test_transaction_abort() {
        let mut coordinator = TransactionCoordinator::new("test-txn".to_string());

        let metadata = TransactionMetadata::new("test-txn".to_string(), Duration::from_secs(60));
        coordinator.begin_transaction(metadata);

        coordinator.abort_transaction();
        assert_eq!(coordinator.state(), TransactionState::None);
        assert_eq!(coordinator.history().len(), 1);
        assert_eq!(coordinator.history()[0].state, TransactionState::Aborted);
    }

    #[test]
    fn test_transaction_statistics() {
        let mut coordinator = TransactionCoordinator::new("test-txn".to_string());

        // Commit one transaction
        let metadata = TransactionMetadata::new("test-txn".to_string(), Duration::from_secs(60));
        coordinator.begin_transaction(metadata);
        coordinator.commit_transaction().ok();

        // Abort one transaction
        let metadata = TransactionMetadata::new("test-txn".to_string(), Duration::from_secs(60));
        coordinator.begin_transaction(metadata);
        coordinator.abort_transaction();

        let stats = coordinator.statistics();
        assert_eq!(stats.total_transactions, 2);
        assert_eq!(stats.committed, 1);
        assert_eq!(stats.aborted, 1);
        assert_eq!(stats.success_rate(), 0.5);
        assert_eq!(stats.abort_rate(), 0.5);
    }
}
