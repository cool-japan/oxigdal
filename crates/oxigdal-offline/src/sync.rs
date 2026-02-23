//! Synchronization engine for offline-first data

use crate::conflict::ConflictDetector;
use crate::error::{Error, Result};
use crate::merge::MergeEngine;
use crate::queue::SyncQueue;
use crate::retry::RetryManager;
use crate::types::{Operation, Record};
use async_trait::async_trait;
use chrono::Utc;

/// Trait for remote sync backends
#[async_trait(?Send)]
pub trait RemoteBackend: Send + Sync {
    /// Push an operation to the remote
    async fn push_operation(&self, operation: &Operation) -> Result<()>;

    /// Fetch remote records that need to be synced
    async fn fetch_updates(&self, since: chrono::DateTime<Utc>) -> Result<Vec<Record>>;

    /// Check if the remote is reachable
    async fn ping(&self) -> Result<bool>;
}

/// Synchronization engine
pub struct SyncEngine {
    queue: SyncQueue,
    #[allow(dead_code)]
    conflict_detector: ConflictDetector,
    #[allow(dead_code)]
    merge_engine: MergeEngine,
    retry_manager: RetryManager,
    remote: Option<Box<dyn RemoteBackend>>,
}

impl SyncEngine {
    /// Create a new sync engine
    pub fn new(
        queue: SyncQueue,
        conflict_detector: ConflictDetector,
        merge_engine: MergeEngine,
        retry_manager: RetryManager,
    ) -> Self {
        Self {
            queue,
            conflict_detector,
            merge_engine,
            retry_manager,
            remote: None,
        }
    }

    /// Set the remote backend
    pub fn with_remote(mut self, remote: Box<dyn RemoteBackend>) -> Self {
        self.remote = Some(remote);
        self
    }

    /// Sync pending operations
    pub async fn sync(&self, batch_size: usize) -> Result<SyncResult> {
        // Check if remote is available
        let remote = self
            .remote
            .as_ref()
            .ok_or_else(|| Error::network("No remote backend configured"))?;

        if !remote.ping().await? {
            return Err(Error::network("Remote backend not reachable"));
        }

        let mut result = SyncResult::default();

        // Get pending operations
        let operations = self.queue.get_pending(batch_size).await?;
        result.total_operations = operations.len();

        tracing::info!(operation_count = operations.len(), "Starting sync");

        // Process each operation
        for operation in operations {
            let op_id = operation.id;
            let retry_count = operation.retry_count;

            match self.sync_operation(&operation, remote.as_ref()).await {
                Ok(()) => {
                    // Remove from queue on success
                    self.queue.dequeue(&op_id).await?;
                    result.successful += 1;

                    tracing::debug!(
                        operation_id = %op_id,
                        "Operation synced successfully"
                    );
                }
                Err(err) => {
                    // Handle failure
                    result.failed += 1;

                    // Check if should retry
                    if self.retry_manager.policy().should_retry(retry_count) {
                        self.queue.requeue_with_retry(operation).await?;
                        result.requeued += 1;

                        tracing::warn!(
                            operation_id = %op_id,
                            error = %err,
                            "Operation failed, requeued for retry"
                        );
                    } else {
                        // Exhausted retries
                        self.queue.dequeue(&op_id).await?;
                        result.exhausted += 1;

                        tracing::error!(
                            operation_id = %op_id,
                            error = %err,
                            "Operation failed after max retries"
                        );
                    }
                }
            }
        }

        tracing::info!(
            successful = result.successful,
            failed = result.failed,
            requeued = result.requeued,
            "Sync completed"
        );

        Ok(result)
    }

    /// Sync a single operation
    async fn sync_operation(
        &self,
        operation: &Operation,
        remote: &dyn RemoteBackend,
    ) -> Result<()> {
        // Use retry manager to execute with retries
        #[cfg(feature = "native")]
        {
            self.retry_manager
                .execute(|| async {
                    remote
                        .push_operation(operation)
                        .await
                        .map_err(|e| e.to_string())
                })
                .await
        }

        #[cfg(not(feature = "native"))]
        {
            remote.push_operation(operation).await
        }
    }

    /// Pull updates from remote
    pub async fn pull_updates(&self, since: chrono::DateTime<Utc>) -> Result<Vec<Record>> {
        let remote = self
            .remote
            .as_ref()
            .ok_or_else(|| Error::network("No remote backend configured"))?;

        remote.fetch_updates(since).await
    }

    /// Perform bidirectional sync
    pub async fn bidirectional_sync(
        &self,
        since: chrono::DateTime<Utc>,
        batch_size: usize,
    ) -> Result<SyncResult> {
        // First, push local changes
        let mut result = self.sync(batch_size).await?;

        // Then, pull remote changes
        let remote_records = self.pull_updates(since).await?;
        result.remote_fetched = remote_records.len();

        tracing::info!(
            remote_fetched = result.remote_fetched,
            "Pulled remote updates"
        );

        Ok(result)
    }

    /// Check if remote is available
    pub async fn is_remote_available(&self) -> bool {
        if let Some(remote) = &self.remote {
            remote.ping().await.unwrap_or(false)
        } else {
            false
        }
    }
}

/// Result of a sync operation
#[derive(Debug, Clone, Default)]
pub struct SyncResult {
    /// Total number of operations processed
    pub total_operations: usize,
    /// Number of successful operations
    pub successful: usize,
    /// Number of failed operations
    pub failed: usize,
    /// Number of operations requeued for retry
    pub requeued: usize,
    /// Number of operations that exhausted retries
    pub exhausted: usize,
    /// Number of remote records fetched
    pub remote_fetched: usize,
}

impl SyncResult {
    /// Check if sync was fully successful
    pub fn is_success(&self) -> bool {
        self.failed == 0 && self.exhausted == 0
    }

    /// Get success rate
    pub fn success_rate(&self) -> f64 {
        if self.total_operations == 0 {
            return 1.0;
        }
        self.successful as f64 / self.total_operations as f64
    }

    /// Get a summary string
    pub fn summary(&self) -> String {
        format!(
            "Sync: {}/{} successful, {} failed, {} requeued, {} fetched",
            self.successful, self.total_operations, self.failed, self.requeued, self.remote_fetched
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::merge::MergeStrategy;
    use crate::retry::RetryPolicy;
    use crate::storage::StorageBackend;
    use crate::storage::sqlite::SqliteBackend;
    use bytes::Bytes;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::RwLock;

    struct MockRemoteBackend {
        fail_count: AtomicUsize,
        max_failures: usize,
    }

    impl MockRemoteBackend {
        fn new(max_failures: usize) -> Self {
            Self {
                fail_count: AtomicUsize::new(0),
                max_failures,
            }
        }
    }

    #[async_trait(?Send)]
    impl RemoteBackend for MockRemoteBackend {
        async fn push_operation(&self, _operation: &Operation) -> Result<()> {
            let count = self.fail_count.fetch_add(1, Ordering::SeqCst);
            if count < self.max_failures {
                Err(Error::network("Simulated network error"))
            } else {
                Ok(())
            }
        }

        async fn fetch_updates(&self, _since: chrono::DateTime<Utc>) -> Result<Vec<Record>> {
            Ok(Vec::new())
        }

        async fn ping(&self) -> Result<bool> {
            Ok(true)
        }
    }

    async fn create_test_engine(max_failures: usize) -> SyncEngine {
        let mut backend = SqliteBackend::in_memory().expect("failed to create backend");
        backend.initialize().await.expect("failed to initialize");

        let storage: Arc<RwLock<Box<dyn StorageBackend>>> =
            Arc::new(RwLock::new(Box::new(backend)));

        let queue = SyncQueue::new(storage, 100);
        let detector = ConflictDetector::new();
        let merger = MergeEngine::new(MergeStrategy::LastWriteWins);
        let retry_policy = RetryPolicy::new(
            3,
            core::time::Duration::from_millis(10),
            core::time::Duration::from_millis(100),
            2.0,
            0.0,
        )
        .expect("failed to create policy");
        let retry_manager = RetryManager::new(retry_policy);

        let remote = MockRemoteBackend::new(max_failures);

        SyncEngine::new(queue, detector, merger, retry_manager).with_remote(Box::new(remote))
    }

    #[tokio::test]
    #[cfg(feature = "native")]
    async fn test_sync_success() {
        let engine = create_test_engine(0).await; // No failures

        // Add an operation to queue
        let record = Record::new("test".to_string(), Bytes::from("data"));
        let operation = Operation::insert(&record);
        engine
            .queue
            .enqueue(operation)
            .await
            .expect("failed to enqueue");

        // Sync
        let result = engine.sync(10).await.expect("failed to sync");
        assert_eq!(result.successful, 1);
        assert_eq!(result.failed, 0);
    }

    #[tokio::test]
    async fn test_sync_result() {
        let result = SyncResult {
            total_operations: 10,
            successful: 8,
            failed: 1,
            requeued: 1,
            exhausted: 0,
            remote_fetched: 5,
        };

        assert!(!result.is_success());
        assert_eq!(result.success_rate(), 0.8);
        assert!(result.summary().contains("8/10"));
    }
}
