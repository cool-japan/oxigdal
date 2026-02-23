//! Background sync capabilities for PWA.

use crate::error::{PwaError, Result};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::ServiceWorkerRegistration;

/// Background sync registration options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncOptions {
    /// Tag name for the sync event
    pub tag: String,

    /// Minimum interval between sync attempts (not widely supported)
    pub min_interval: Option<u64>,
}

impl SyncOptions {
    /// Create new sync options with a tag.
    pub fn new(tag: impl Into<String>) -> Self {
        Self {
            tag: tag.into(),
            min_interval: None,
        }
    }

    /// Set minimum interval between syncs.
    pub fn with_min_interval(mut self, interval_ms: u64) -> Self {
        self.min_interval = Some(interval_ms);
        self
    }
}

/// Background sync manager.
pub struct BackgroundSync {
    registration: ServiceWorkerRegistration,
}

impl BackgroundSync {
    /// Create a new background sync manager.
    pub fn new(registration: ServiceWorkerRegistration) -> Self {
        Self { registration }
    }

    /// Check if background sync is supported.
    pub fn is_supported() -> bool {
        if let Some(window) = web_sys::window() {
            let navigator = window.navigator();
            let sw_container = navigator.service_worker();
            // Check if registration has sync property
            js_sys::Reflect::has(&sw_container, &JsValue::from_str("sync")).unwrap_or(false)
        } else {
            false
        }
    }

    /// Get the sync manager using reflection.
    fn get_sync_manager(&self) -> Result<JsValue> {
        let sync = js_sys::Reflect::get(&self.registration, &JsValue::from_str("sync"))
            .map_err(|_e| PwaError::BackgroundSyncNotSupported)?;

        if sync.is_undefined() || sync.is_null() {
            return Err(PwaError::BackgroundSyncNotSupported);
        }

        Ok(sync)
    }

    /// Register a background sync.
    pub async fn register(&self, options: &SyncOptions) -> Result<()> {
        if !Self::is_supported() {
            return Err(PwaError::BackgroundSyncNotSupported);
        }

        let sync_manager = self.get_sync_manager()?;

        // Call register method on sync manager
        let register_fn = js_sys::Reflect::get(&sync_manager, &JsValue::from_str("register"))
            .map_err(|_| {
                PwaError::BackgroundSyncRegistration("register method not found".to_string())
            })?;

        let register_fn = register_fn.dyn_into::<js_sys::Function>().map_err(|_| {
            PwaError::BackgroundSyncRegistration("register is not a function".to_string())
        })?;

        let promise = register_fn
            .call1(&sync_manager, &JsValue::from_str(&options.tag))
            .map_err(|e| PwaError::BackgroundSyncRegistration(format!("{:?}", e)))?;

        let promise = promise.dyn_into::<js_sys::Promise>().map_err(|_| {
            PwaError::BackgroundSyncRegistration("register did not return a promise".to_string())
        })?;

        JsFuture::from(promise).await.map_err(|e| {
            PwaError::BackgroundSyncRegistration(format!("Registration failed: {:?}", e))
        })?;

        Ok(())
    }

    /// Get all registered sync tags.
    pub async fn get_tags(&self) -> Result<Vec<String>> {
        if !Self::is_supported() {
            return Ok(Vec::new());
        }

        let sync_manager = self.get_sync_manager()?;

        // Call getTags method on sync manager
        let get_tags_fn = js_sys::Reflect::get(&sync_manager, &JsValue::from_str("getTags"))
            .map_err(|_| {
                PwaError::BackgroundSyncRegistration("getTags method not found".to_string())
            })?;

        let get_tags_fn = get_tags_fn.dyn_into::<js_sys::Function>().map_err(|_| {
            PwaError::BackgroundSyncRegistration("getTags is not a function".to_string())
        })?;

        let promise = get_tags_fn
            .call0(&sync_manager)
            .map_err(|e| PwaError::BackgroundSyncRegistration(format!("{:?}", e)))?;

        let promise = promise.dyn_into::<js_sys::Promise>().map_err(|_| {
            PwaError::BackgroundSyncRegistration("getTags did not return a promise".to_string())
        })?;

        let result = JsFuture::from(promise).await.map_err(|e| {
            PwaError::BackgroundSyncRegistration(format!("Get tags failed: {:?}", e))
        })?;

        let array = js_sys::Array::from(&result);
        let mut tags = Vec::new();

        for i in 0..array.length() {
            if let Some(tag) = array.get(i).as_string() {
                tags.push(tag);
            }
        }

        Ok(tags)
    }

    /// Register a one-time sync.
    pub async fn register_one_time(&self, tag: impl Into<String>) -> Result<()> {
        let options = SyncOptions::new(tag);
        self.register(&options).await
    }

    /// Register a periodic sync for data updates.
    pub async fn register_periodic(&self, tag: impl Into<String>, interval_ms: u64) -> Result<()> {
        let options = SyncOptions::new(tag).with_min_interval(interval_ms);
        self.register(&options).await
    }
}

/// Sync event data for service worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncEventData {
    /// Tag of the sync event
    pub tag: String,

    /// Last sync timestamp
    pub last_sync: Option<i64>,
}

/// Sync queue for queuing operations when offline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncQueue {
    /// Queue name
    pub name: String,

    /// Queued operations
    operations: Vec<QueuedOperation>,
}

/// Queued operation to be synced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedOperation {
    /// Operation ID
    pub id: String,

    /// Operation type
    pub operation_type: String,

    /// Operation data
    pub data: serde_json::Value,

    /// Queued timestamp
    pub queued_at: i64,

    /// Number of retry attempts
    pub retry_count: u32,

    /// Maximum retries
    pub max_retries: u32,
}

impl QueuedOperation {
    /// Create a new queued operation.
    pub fn new(
        id: impl Into<String>,
        operation_type: impl Into<String>,
        data: serde_json::Value,
    ) -> Self {
        Self {
            id: id.into(),
            operation_type: operation_type.into(),
            data,
            queued_at: chrono::Utc::now().timestamp(),
            retry_count: 0,
            max_retries: 3,
        }
    }

    /// Check if operation should be retried.
    pub fn should_retry(&self) -> bool {
        self.retry_count < self.max_retries
    }

    /// Increment retry count.
    pub fn increment_retry(&mut self) {
        self.retry_count += 1;
    }
}

impl SyncQueue {
    /// Create a new sync queue.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            operations: Vec::new(),
        }
    }

    /// Add an operation to the queue.
    pub fn enqueue(&mut self, operation: QueuedOperation) {
        self.operations.push(operation);
    }

    /// Get the next operation to process.
    pub fn dequeue(&mut self) -> Option<QueuedOperation> {
        if self.operations.is_empty() {
            None
        } else {
            Some(self.operations.remove(0))
        }
    }

    /// Peek at the next operation without removing it.
    pub fn peek(&self) -> Option<&QueuedOperation> {
        self.operations.first()
    }

    /// Get the number of queued operations.
    pub fn len(&self) -> usize {
        self.operations.len()
    }

    /// Check if the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    /// Clear all operations from the queue.
    pub fn clear(&mut self) {
        self.operations.clear();
    }

    /// Get all operations.
    pub fn operations(&self) -> &[QueuedOperation] {
        &self.operations
    }

    /// Remove a specific operation by ID.
    pub fn remove(&mut self, id: &str) -> Option<QueuedOperation> {
        if let Some(index) = self.operations.iter().position(|op| op.id == id) {
            Some(self.operations.remove(index))
        } else {
            None
        }
    }

    /// Serialize queue to JSON.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).map_err(|e| PwaError::Serialization(e.to_string()))
    }

    /// Deserialize queue from JSON.
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).map_err(|e| PwaError::Deserialization(e.to_string()))
    }
}

/// Sync manager for coordinating background sync operations.
pub struct SyncCoordinator {
    background_sync: BackgroundSync,
    queues: Vec<SyncQueue>,
}

impl SyncCoordinator {
    /// Create a new sync coordinator.
    pub fn new(registration: ServiceWorkerRegistration) -> Self {
        Self {
            background_sync: BackgroundSync::new(registration),
            queues: Vec::new(),
        }
    }

    /// Add a sync queue.
    pub fn add_queue(&mut self, queue: SyncQueue) {
        self.queues.push(queue);
    }

    /// Get a queue by name.
    pub fn get_queue(&self, name: &str) -> Option<&SyncQueue> {
        self.queues.iter().find(|q| q.name == name)
    }

    /// Get a mutable queue by name.
    pub fn get_queue_mut(&mut self, name: &str) -> Option<&mut SyncQueue> {
        self.queues.iter_mut().find(|q| q.name == name)
    }

    /// Enqueue an operation for background sync.
    pub async fn enqueue_operation(
        &mut self,
        queue_name: &str,
        operation: QueuedOperation,
    ) -> Result<()> {
        // Get or create queue
        if self.get_queue(queue_name).is_none() {
            self.add_queue(SyncQueue::new(queue_name));
        }

        if let Some(queue) = self.get_queue_mut(queue_name) {
            queue.enqueue(operation);

            // Register background sync
            self.background_sync
                .register_one_time(format!("sync-{}", queue_name))
                .await?;
        }

        Ok(())
    }

    /// Process all queues.
    pub async fn process_queues<F>(&mut self, mut processor: F) -> Result<()>
    where
        F: FnMut(&QueuedOperation) -> Result<bool>,
    {
        for queue in &mut self.queues {
            let mut failed_operations = Vec::new();

            while let Some(mut operation) = queue.dequeue() {
                match processor(&operation) {
                    Ok(true) => {
                        // Operation successful, continue
                    }
                    Ok(false) | Err(_) => {
                        // Operation failed, check if should retry
                        if operation.should_retry() {
                            operation.increment_retry();
                            failed_operations.push(operation);
                        }
                    }
                }
            }

            // Re-queue failed operations that should be retried
            for op in failed_operations {
                queue.enqueue(op);
            }
        }

        Ok(())
    }

    /// Get total number of queued operations across all queues.
    pub fn total_queued(&self) -> usize {
        self.queues.iter().map(|q| q.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_options() {
        let options = SyncOptions::new("my-sync").with_min_interval(60000);
        assert_eq!(options.tag, "my-sync");
        assert_eq!(options.min_interval, Some(60000));
    }

    #[test]
    fn test_queued_operation() {
        let mut op =
            QueuedOperation::new("op-1", "upload", serde_json::json!({"file": "test.txt"}));

        assert_eq!(op.retry_count, 0);
        assert!(op.should_retry());

        op.increment_retry();
        assert_eq!(op.retry_count, 1);
    }

    #[test]
    fn test_sync_queue() {
        let mut queue = SyncQueue::new("upload-queue");

        assert!(queue.is_empty());
        assert_eq!(queue.len(), 0);

        let op = QueuedOperation::new("op-1", "upload", serde_json::json!({}));
        queue.enqueue(op);

        assert!(!queue.is_empty());
        assert_eq!(queue.len(), 1);

        let dequeued = queue.dequeue();
        assert!(dequeued.is_some());
        assert!(queue.is_empty());
    }

    #[test]
    fn test_queue_serialization() -> Result<()> {
        let mut queue = SyncQueue::new("test");
        queue.enqueue(QueuedOperation::new("op-1", "test", serde_json::json!({})));

        let json = queue.to_json()?;
        let deserialized = SyncQueue::from_json(&json)?;

        assert_eq!(deserialized.name, "test");
        assert_eq!(deserialized.len(), 1);

        Ok(())
    }
}
