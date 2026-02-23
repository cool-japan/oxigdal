//! Main offline manager that coordinates all components

use crate::config::Config;
use crate::conflict::ConflictDetector;
use crate::error::{Error, Result};
use crate::merge::MergeEngine;
use crate::optimistic::OptimisticTracker;
use crate::queue::SyncQueue;
use crate::retry::RetryManager;
use crate::storage::StorageBackend;
use crate::sync::{RemoteBackend, SyncEngine, SyncResult};
use crate::types::{Operation, Record};
use bytes::Bytes;
use std::sync::Arc;
use tokio::sync::RwLock;

#[cfg(feature = "native")]
use crate::storage::sqlite::SqliteBackend;

#[cfg(all(feature = "wasm", not(feature = "native")))]
use crate::storage::indexeddb::IndexedDbBackend;

/// Offline manager - main entry point for offline-first data management
pub struct OfflineManager {
    config: Config,
    storage: Arc<RwLock<Box<dyn StorageBackend>>>,
    queue: SyncQueue,
    sync_engine: Option<SyncEngine>,
    optimistic_tracker: OptimisticTracker,
}

impl OfflineManager {
    /// Create a new offline manager with the given configuration
    pub async fn new(config: Config) -> Result<Self> {
        config.validate()?;

        // Create storage backend based on platform
        #[cfg(feature = "native")]
        let storage = Self::create_native_storage(&config).await?;

        #[cfg(all(feature = "wasm", not(feature = "native")))]
        let storage = Self::create_wasm_storage(&config).await?;

        #[cfg(all(not(feature = "native"), not(feature = "wasm")))]
        {
            return Err(Error::config(
                "No storage backend available (enable 'native' or 'wasm' feature)",
            ));
        }

        let storage = Arc::new(RwLock::new(storage));

        // Create sync queue
        let queue = SyncQueue::new(storage.clone(), config.max_queue_size);

        // Create optimistic tracker
        let optimistic_tracker = OptimisticTracker::new(config.max_operation_age_secs);

        Ok(Self {
            config,
            storage,
            queue,
            sync_engine: None,
            optimistic_tracker,
        })
    }

    /// Create native storage backend
    #[cfg(feature = "native")]
    async fn create_native_storage(config: &Config) -> Result<Box<dyn StorageBackend>> {
        let path = config.storage_path.as_deref().unwrap_or(":memory:");

        let mut backend = if path == ":memory:" {
            SqliteBackend::in_memory()?
        } else {
            SqliteBackend::new(path)?
        };

        backend.initialize().await?;

        Ok(Box::new(backend))
    }

    /// Create WASM storage backend
    #[cfg(all(feature = "wasm", not(feature = "native")))]
    async fn create_wasm_storage(config: &Config) -> Result<Box<dyn StorageBackend>> {
        let mut backend = IndexedDbBackend::new(config.database_name.clone());
        backend.initialize().await?;
        Ok(Box::new(backend))
    }

    /// Set up remote sync backend
    ///
    /// # Errors
    /// Returns an error if the retry policy configuration is invalid.
    pub fn with_remote(mut self, remote: Box<dyn RemoteBackend>) -> Result<Self> {
        let conflict_detector = ConflictDetector::new();
        let merge_engine = MergeEngine::new(self.config.merge_strategy);
        let retry_policy = crate::retry::RetryPolicy::new(
            self.config.retry_max_attempts,
            self.config.initial_retry_delay(),
            self.config.max_retry_delay(),
            self.config.retry_backoff_multiplier,
            self.config.retry_jitter_factor,
        )?;
        let retry_manager = RetryManager::new(retry_policy);

        let sync_engine = SyncEngine::new(
            self.queue.clone(),
            conflict_detector,
            merge_engine,
            retry_manager,
        )
        .with_remote(remote);

        self.sync_engine = Some(sync_engine);
        Ok(self)
    }

    /// Write data to local storage
    pub async fn write(&self, key: &str, data: &[u8]) -> Result<Record> {
        let record = Record::new(key.to_string(), Bytes::copy_from_slice(data));

        // Store in local storage
        {
            let mut storage = self.storage.write().await;
            storage.put_record(&record).await?;
        }

        // Create operation for sync queue
        let operation = Operation::insert(&record);

        // Enqueue for sync
        self.queue.enqueue(operation.clone()).await?;

        // Track optimistic update if enabled
        if self.config.enable_optimistic_updates {
            self.optimistic_tracker.track(&record, &operation)?;
        }

        tracing::debug!(
            key = %key,
            size = data.len(),
            "Wrote record"
        );

        Ok(record)
    }

    /// Read data from local storage
    pub async fn read(&self, key: &str) -> Result<Option<Record>> {
        let storage = self.storage.read().await;
        storage.get_record(key).await
    }

    /// Update existing record
    pub async fn update(&self, key: &str, data: &[u8]) -> Result<Record> {
        // Get existing record
        let mut record = self
            .read(key)
            .await?
            .ok_or_else(|| Error::not_found(format!("Record not found: {key}")))?;

        let old_version = record.version;

        // Update record
        record.update(Bytes::copy_from_slice(data));

        // Store in local storage
        {
            let mut storage = self.storage.write().await;
            storage.put_record(&record).await?;
        }

        // Create operation for sync queue
        let operation = Operation::update(&record, old_version);

        // Enqueue for sync
        self.queue.enqueue(operation.clone()).await?;

        // Track optimistic update if enabled
        if self.config.enable_optimistic_updates {
            self.optimistic_tracker.track(&record, &operation)?;
        }

        tracing::debug!(
            key = %key,
            size = data.len(),
            version = %record.version,
            "Updated record"
        );

        Ok(record)
    }

    /// Delete a record
    pub async fn delete(&self, key: &str) -> Result<()> {
        let mut storage = self.storage.write().await;
        storage.delete_record(key).await?;

        tracing::debug!(
            key = %key,
            "Deleted record"
        );

        Ok(())
    }

    /// List all records
    pub async fn list(&self) -> Result<Vec<Record>> {
        let storage = self.storage.read().await;
        storage.list_records().await
    }

    /// Synchronize with remote
    pub async fn sync(&self) -> Result<SyncResult> {
        let engine = self
            .sync_engine
            .as_ref()
            .ok_or_else(|| Error::internal("No sync engine configured"))?;

        engine.sync(self.config.sync_batch_size).await
    }

    /// Check if remote is available
    pub async fn is_online(&self) -> bool {
        if let Some(engine) = &self.sync_engine {
            engine.is_remote_available().await
        } else {
            false
        }
    }

    /// Get queue size
    pub async fn queue_size(&self) -> Result<usize> {
        self.queue.size().await
    }

    /// Get storage statistics
    pub async fn statistics(&self) -> Result<ManagerStatistics> {
        let storage = self.storage.read().await;
        let storage_stats = storage.get_statistics().await?;

        let queue_size = self.queue.size().await?;
        let optimistic_stats = self.optimistic_tracker.statistics();

        Ok(ManagerStatistics {
            storage_stats,
            queue_size,
            optimistic_pending: optimistic_stats.total_pending,
            optimistic_confirmed: optimistic_stats.confirmed,
            optimistic_unconfirmed: optimistic_stats.unconfirmed,
        })
    }

    /// Clear all local data (use with caution!)
    pub async fn clear_all(&self) -> Result<()> {
        let mut storage = self.storage.write().await;
        storage.clear_records().await?;
        storage.clear_operations().await?;

        tracing::warn!("Cleared all local data");

        Ok(())
    }

    /// Compact storage
    pub async fn compact(&self) -> Result<()> {
        let mut storage = self.storage.write().await;
        storage.compact().await?;

        tracing::info!("Compacted storage");

        Ok(())
    }

    /// Perform maintenance tasks
    pub async fn maintenance(&self) -> Result<MaintenanceReport> {
        let mut report = MaintenanceReport::default();

        // Cleanup optimistic tracker
        let cleaned = self.optimistic_tracker.cleanup();
        report.optimistic_cleaned = cleaned;

        // Prune old operations
        if let Some(max_age) = self.config.max_operation_age() {
            let pruned = self.queue.prune_old(max_age.as_secs()).await?;
            report.operations_pruned = pruned;
        }

        // Prune old optimistic updates
        let pruned = self.optimistic_tracker.prune_old();
        report.optimistic_pruned = pruned;

        tracing::info!(
            optimistic_cleaned = report.optimistic_cleaned,
            operations_pruned = report.operations_pruned,
            optimistic_pruned = report.optimistic_pruned,
            "Maintenance completed"
        );

        Ok(report)
    }

    /// Get configuration
    pub fn config(&self) -> &Config {
        &self.config
    }
}

/// Manager statistics
#[derive(Debug, Clone)]
pub struct ManagerStatistics {
    /// Storage backend statistics
    pub storage_stats: crate::storage::StorageStatistics,
    /// Number of operations in sync queue
    pub queue_size: usize,
    /// Number of pending optimistic updates
    pub optimistic_pending: usize,
    /// Number of confirmed optimistic updates
    pub optimistic_confirmed: usize,
    /// Number of unconfirmed optimistic updates
    pub optimistic_unconfirmed: usize,
}

impl ManagerStatistics {
    /// Get a summary string
    pub fn summary(&self) -> String {
        format!(
            "Storage: {} records ({} bytes), Queue: {} operations, Optimistic: {} pending",
            self.storage_stats.record_count,
            self.storage_stats.record_size_bytes,
            self.queue_size,
            self.optimistic_pending
        )
    }
}

/// Maintenance report
#[derive(Debug, Clone, Default)]
pub struct MaintenanceReport {
    /// Number of optimistic updates cleaned up
    pub optimistic_cleaned: usize,
    /// Number of operations pruned
    pub operations_pruned: usize,
    /// Number of optimistic updates pruned
    pub optimistic_pruned: usize,
}

impl MaintenanceReport {
    /// Get a summary string
    pub fn summary(&self) -> String {
        format!(
            "Cleaned {} optimistic, pruned {} operations, pruned {} optimistic",
            self.optimistic_cleaned, self.operations_pruned, self.optimistic_pruned
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn create_test_manager() -> OfflineManager {
        let config = Config::builder()
            .storage_path(":memory:".to_string())
            .build()
            .expect("failed to create config");

        OfflineManager::new(config)
            .await
            .expect("failed to create manager")
    }

    #[tokio::test]
    async fn test_write_read() {
        let manager = create_test_manager().await;

        let _record = manager
            .write("test_key", b"test_data")
            .await
            .expect("failed to write");

        let read = manager.read("test_key").await.expect("failed to read");

        assert!(read.is_some());
        let read = read.expect("no record");
        assert_eq!(read.key, "test_key");
        assert_eq!(read.data, Bytes::from("test_data"));
    }

    #[tokio::test]
    async fn test_update() {
        let manager = create_test_manager().await;

        manager
            .write("test_key", b"original")
            .await
            .expect("failed to write");

        let updated = manager
            .update("test_key", b"updated")
            .await
            .expect("failed to update");

        assert_eq!(updated.data, Bytes::from("updated"));
        assert_eq!(updated.version.value(), 1);
    }

    #[tokio::test]
    async fn test_delete() {
        let manager = create_test_manager().await;

        manager
            .write("test_key", b"data")
            .await
            .expect("failed to write");

        manager.delete("test_key").await.expect("failed to delete");

        let read = manager.read("test_key").await.expect("failed to read");
        assert!(read.is_none());
    }

    #[tokio::test]
    async fn test_queue_size() {
        let manager = create_test_manager().await;

        manager
            .write("key1", b"data1")
            .await
            .expect("failed to write");
        manager
            .write("key2", b"data2")
            .await
            .expect("failed to write");

        let size = manager.queue_size().await.expect("failed to get size");
        assert_eq!(size, 2);
    }

    #[tokio::test]
    async fn test_statistics() {
        let manager = create_test_manager().await;

        manager
            .write("test", b"data")
            .await
            .expect("failed to write");

        let stats = manager.statistics().await.expect("failed to get stats");
        assert_eq!(stats.storage_stats.record_count, 1);
        assert_eq!(stats.queue_size, 1);
    }

    #[tokio::test]
    async fn test_maintenance() {
        let manager = create_test_manager().await;

        manager
            .write("test", b"data")
            .await
            .expect("failed to write");

        let report = manager
            .maintenance()
            .await
            .expect("failed to run maintenance");
        assert!(report.summary().contains("Cleaned"));
    }
}
