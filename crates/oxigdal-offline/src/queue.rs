//! Sync queue management

use crate::error::{Error, Result};
use crate::storage::StorageBackend;
use crate::types::{Operation, OperationId};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Sync queue manager
#[derive(Clone)]
pub struct SyncQueue {
    storage: Arc<RwLock<Box<dyn StorageBackend>>>,
    max_size: usize,
}

impl SyncQueue {
    /// Create a new sync queue
    pub fn new(storage: Arc<RwLock<Box<dyn StorageBackend>>>, max_size: usize) -> Self {
        Self { storage, max_size }
    }

    /// Enqueue an operation
    pub async fn enqueue(&self, operation: Operation) -> Result<()> {
        // Check queue size
        let current_size = self.size().await?;
        if current_size >= self.max_size {
            return Err(Error::capacity_exceeded(format!(
                "Queue is full (size: {current_size}, max: {})",
                self.max_size
            )));
        }

        // Add to storage
        let mut storage = self.storage.write().await;
        storage.enqueue_operation(&operation).await?;

        tracing::debug!(
            operation_id = %operation.id,
            operation_type = %operation.operation_type,
            "Operation enqueued"
        );

        Ok(())
    }

    /// Dequeue an operation
    pub async fn dequeue(&self, operation_id: &OperationId) -> Result<()> {
        let mut storage = self.storage.write().await;
        storage.dequeue_operation(operation_id).await?;

        tracing::debug!(
            operation_id = %operation_id,
            "Operation dequeued"
        );

        Ok(())
    }

    /// Get pending operations
    pub async fn get_pending(&self, limit: usize) -> Result<Vec<Operation>> {
        let storage = self.storage.read().await;
        storage.get_pending_operations(limit).await
    }

    /// Update an operation (e.g., increment retry count)
    pub async fn update(&self, operation: &Operation) -> Result<()> {
        let mut storage = self.storage.write().await;
        storage.update_operation(operation).await
    }

    /// Get queue size
    pub async fn size(&self) -> Result<usize> {
        let storage = self.storage.read().await;
        storage.count_pending_operations().await
    }

    /// Clear the queue
    pub async fn clear(&self) -> Result<()> {
        let mut storage = self.storage.write().await;
        storage.clear_operations().await
    }

    /// Get operations ready for retry
    pub async fn get_ready_for_retry(&self, limit: usize) -> Result<Vec<Operation>> {
        let operations = self.get_pending(limit).await?;

        // Filter operations that are ready for retry
        // (no recent retry or enough time has passed)
        let now = chrono::Utc::now();
        let ready: Vec<_> = operations
            .into_iter()
            .filter(|op| {
                op.last_retry
                    .map(|last| (now - last).num_seconds() > 60)
                    .unwrap_or(true)
            })
            .collect();

        Ok(ready)
    }

    /// Prune old operations
    pub async fn prune_old(&self, max_age_secs: u64) -> Result<usize> {
        let operations = self.get_pending(usize::MAX).await?;
        let now = chrono::Utc::now();

        let mut pruned = 0;
        for op in operations {
            let age = (now - op.created_at).num_seconds();
            if age > max_age_secs as i64 {
                self.dequeue(&op.id).await?;
                pruned += 1;
            }
        }

        if pruned > 0 {
            tracing::info!(pruned_count = pruned, max_age_secs, "Pruned old operations");
        }

        Ok(pruned)
    }

    /// Get operations by priority
    pub async fn get_by_priority(&self, min_priority: u8, limit: usize) -> Result<Vec<Operation>> {
        let operations = self.get_pending(limit * 2).await?;

        let mut filtered: Vec<_> = operations
            .into_iter()
            .filter(|op| op.priority >= min_priority)
            .take(limit)
            .collect();

        // Sort by priority (descending) and age (ascending)
        filtered.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then_with(|| a.created_at.cmp(&b.created_at))
        });

        Ok(filtered)
    }

    /// Requeue failed operation with incremented retry count
    pub async fn requeue_with_retry(&self, mut operation: Operation) -> Result<()> {
        operation.increment_retry();
        self.update(&operation).await?;

        tracing::debug!(
            operation_id = %operation.id,
            retry_count = operation.retry_count,
            "Operation requeued with retry"
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::sqlite::SqliteBackend;
    use crate::types::Record;
    use bytes::Bytes;

    async fn create_test_queue() -> SyncQueue {
        let mut backend = SqliteBackend::in_memory().expect("failed to create backend");
        backend.initialize().await.expect("failed to initialize");

        let storage: Arc<RwLock<Box<dyn StorageBackend>>> =
            Arc::new(RwLock::new(Box::new(backend)));

        SyncQueue::new(storage, 100)
    }

    #[tokio::test]
    async fn test_enqueue_dequeue() {
        let queue = create_test_queue().await;

        let record = Record::new("test".to_string(), Bytes::from("data"));
        let operation = Operation::insert(&record);
        let op_id = operation.id;

        queue.enqueue(operation).await.expect("failed to enqueue");

        let size = queue.size().await.expect("failed to get size");
        assert_eq!(size, 1);

        queue.dequeue(&op_id).await.expect("failed to dequeue");

        let size = queue.size().await.expect("failed to get size");
        assert_eq!(size, 0);
    }

    #[tokio::test]
    async fn test_queue_capacity() {
        let queue = SyncQueue::new(
            Arc::new(RwLock::new(Box::new(
                SqliteBackend::in_memory().expect("failed"),
            ))),
            2,
        );

        let record1 = Record::new("test1".to_string(), Bytes::from("data1"));
        let record2 = Record::new("test2".to_string(), Bytes::from("data2"));
        let record3 = Record::new("test3".to_string(), Bytes::from("data3"));

        // Initialize storage
        {
            let mut storage = queue.storage.write().await;
            storage.initialize().await.expect("failed");
        }

        queue
            .enqueue(Operation::insert(&record1))
            .await
            .expect("failed");
        queue
            .enqueue(Operation::insert(&record2))
            .await
            .expect("failed");

        let result = queue.enqueue(Operation::insert(&record3)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_requeue_with_retry() {
        let queue = create_test_queue().await;

        let record = Record::new("test".to_string(), Bytes::from("data"));
        let operation = Operation::insert(&record);
        assert_eq!(operation.retry_count, 0);

        queue
            .enqueue(operation.clone())
            .await
            .expect("failed to enqueue");

        queue
            .requeue_with_retry(operation.clone())
            .await
            .expect("failed to requeue");

        let pending = queue.get_pending(10).await.expect("failed to get pending");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].retry_count, 1);
    }
}
