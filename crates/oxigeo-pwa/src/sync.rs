//! Background sync capabilities for PWA.
//!
//! # Contract with `templates/service-worker.js`
//!
//! Registering a background sync (via [`BackgroundSync::register_one_time`])
//! only tells the browser to fire a `sync` event with a given tag at some
//! point in the future -- it does **not**, by itself, give the service
//! worker (which runs in a separate JS realm with no access to this page's
//! in-memory state) anything to actually replay. [`SyncCoordinator`]
//! therefore also persists each [`QueuedOperation`] to IndexedDB (see
//! [`persistence`]) using a fixed schema that `templates/service-worker.js`'s
//! `handleBackgroundSync` reads from directly. If you change
//! [`persistence::DB_NAME`], [`persistence::DB_VERSION`],
//! [`persistence::STORE_NAME`], or the queue-name/tag naming scheme
//! (`sync-{queue_name}`), update the JS template to match.

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

    /// The URL this operation replays against when a background `sync`
    /// event fires. Required (rather than left to be reconstructed by
    /// application-specific service-worker logic) because the service
    /// worker has no access to this page's in-memory state -- everything
    /// it needs to actually perform the replay (see
    /// `templates/service-worker.js`'s `handleBackgroundSync`) must be
    /// present in the persisted operation itself.
    pub endpoint: String,

    /// HTTP method used to replay this operation (default `"POST"`).
    pub method: String,

    /// Operation data (sent as the JSON request body on replay).
    pub data: serde_json::Value,

    /// Queued timestamp
    pub queued_at: i64,

    /// Number of retry attempts
    pub retry_count: u32,

    /// Maximum retries
    pub max_retries: u32,
}

impl QueuedOperation {
    /// Create a new queued operation that replays as an HTTP `POST` to
    /// `endpoint` with `data` as its JSON body.
    pub fn new(
        id: impl Into<String>,
        operation_type: impl Into<String>,
        endpoint: impl Into<String>,
        data: serde_json::Value,
    ) -> Self {
        Self {
            id: id.into(),
            operation_type: operation_type.into(),
            endpoint: endpoint.into(),
            method: "POST".to_string(),
            data,
            queued_at: chrono::Utc::now().timestamp(),
            retry_count: 0,
            max_retries: 3,
        }
    }

    /// Override the HTTP method used to replay this operation.
    #[must_use]
    pub fn with_method(mut self, method: impl Into<String>) -> Self {
        self.method = method.into();
        self
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

/// IndexedDB persistence for queued sync operations.
///
/// The service worker's `sync` event handler runs in a separate JS realm
/// with no access to this page's in-memory [`SyncQueue`]s, so
/// [`SyncCoordinator::enqueue_operation`] must write each operation
/// somewhere the service worker can independently read it back from. This
/// module owns that shared schema; `templates/service-worker.js`'s
/// `handleBackgroundSync` reads/deletes from the exact same database, store,
/// and record shape.
pub mod persistence {
    use super::{PwaError, QueuedOperation, Result};
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::*;
    use wasm_bindgen_futures::JsFuture;
    use web_sys::{
        DomException, IdbDatabase, IdbObjectStore, IdbOpenDbRequest, IdbRequest, IdbTransaction,
        IdbTransactionMode, IdbVersionChangeEvent,
    };

    /// IndexedDB database name. Keep in sync with `templates/service-worker.js`.
    pub const DB_NAME: &str = "oxigeo-pwa-sync";
    /// IndexedDB database schema version. Keep in sync with the JS template.
    pub const DB_VERSION: u32 = 1;
    /// Object store holding one record per queued operation, keyed by
    /// [`QueuedOperation::id`]. Each record is the JSON-shaped serialization
    /// of `QueuedOperation` plus a `queue_name` field. Keep in sync with the
    /// JS template.
    pub const STORE_NAME: &str = "sync_operations";

    /// Background-sync tag for a given queue name. A tag `sync-{queue_name}`
    /// is registered with the browser's SyncManager and, on firing, the
    /// service worker recovers `queue_name` by stripping this same prefix
    /// (see `handleBackgroundSync` in the JS template) to find the matching
    /// persisted operations.
    pub fn tag_for_queue(queue_name: &str) -> String {
        format!("sync-{queue_name}")
    }

    fn map_js_err(context: &str) -> impl Fn(JsValue) -> PwaError + '_ {
        move |e| PwaError::SyncQueuePersistenceFailed(format!("{context}: {e:?}"))
    }

    /// Bridge an event-based [`IdbRequest`] (IndexedDB requests fire
    /// `onsuccess`/`onerror` events; they do not return a `Promise`) into an
    /// awaitable `Result`. Accepts anything derefable to `IdbRequest`
    /// (including [`IdbOpenDbRequest`]).
    async fn await_idb_request(request: &IdbRequest) -> std::result::Result<JsValue, JsValue> {
        let request_for_success = request.clone();
        let request_for_error = request.clone();

        let promise = js_sys::Promise::new(&mut |resolve, reject| {
            let request_for_success = request_for_success.clone();
            let on_success = Closure::once(move |_evt: web_sys::Event| {
                let result = request_for_success.result().unwrap_or(JsValue::UNDEFINED);
                let _ = resolve.call1(&JsValue::NULL, &result);
            });
            request.set_onsuccess(Some(on_success.as_ref().unchecked_ref()));
            on_success.forget();

            let request_for_error = request_for_error.clone();
            let on_error = Closure::once(move |_evt: web_sys::Event| {
                let err: JsValue = request_for_error
                    .error()
                    .ok()
                    .flatten()
                    .map(Into::into)
                    .unwrap_or_else(|| JsValue::from_str("IndexedDB request failed"));
                let _ = reject.call1(&JsValue::NULL, &err);
            });
            request.set_onerror(Some(on_error.as_ref().unchecked_ref()));
            on_error.forget();
        });

        JsFuture::from(promise).await
    }

    /// `true` if `err` is the `DOMException` IndexedDB throws when creating
    /// an object store/index that already exists -- i.e. safe to ignore
    /// when (re-)opening a database whose schema was already created by a
    /// previous session.
    fn is_already_exists_error(err: &JsValue) -> bool {
        err.dyn_ref::<DomException>()
            .is_some_and(|e| e.name() == "ConstraintError")
    }

    async fn open_db() -> Result<IdbDatabase> {
        let window = web_sys::window()
            .ok_or_else(|| PwaError::SyncQueuePersistenceFailed("no window object".to_string()))?;
        let idb = window
            .indexed_db()
            .map_err(map_js_err("failed to get indexedDB"))?
            .ok_or_else(|| {
                PwaError::SyncQueuePersistenceFailed("IndexedDB not supported".to_string())
            })?;

        let open_request = idb
            .open_with_u32(DB_NAME, DB_VERSION)
            .map_err(map_js_err("failed to open sync queue database"))?;

        let upgrade_handler = Closure::once(move |event: IdbVersionChangeEvent| {
            let target = event
                .target()
                .and_then(|t| t.dyn_into::<IdbOpenDbRequest>().ok())
                .and_then(|r| r.result().ok())
                .and_then(|r| r.dyn_into::<IdbDatabase>().ok());

            if let Some(db) = target {
                // `create_object_store` throws a `ConstraintError` if the
                // store already exists (e.g. this closure ran again after a
                // future `DB_VERSION` bump) -- that's expected and fine, so
                // only genuinely unexpected failures are logged.
                if let Err(e) = db.create_object_store(STORE_NAME)
                    && !is_already_exists_error(&e)
                {
                    web_sys::console::error_1(&JsValue::from_str(&format!(
                        "failed to create {STORE_NAME} object store: {e:?}"
                    )));
                }
            }
        });

        open_request.set_onupgradeneeded(Some(upgrade_handler.as_ref().unchecked_ref()));
        upgrade_handler.forget();

        let result = await_idb_request(&open_request)
            .await
            .map_err(map_js_err("sync queue database open failed"))?;

        result
            .dyn_into::<IdbDatabase>()
            .map_err(|_| PwaError::SyncQueuePersistenceFailed("failed to cast IdbDatabase".into()))
    }

    fn transaction(db: &IdbDatabase, mode: IdbTransactionMode) -> Result<IdbTransaction> {
        db.transaction_with_str_and_mode(STORE_NAME, mode)
            .map_err(map_js_err("failed to open sync queue transaction"))
    }

    fn store(transaction: &IdbTransaction) -> Result<IdbObjectStore> {
        transaction
            .object_store(STORE_NAME)
            .map_err(map_js_err("failed to get sync queue object store"))
    }

    async fn settle(request: IdbRequest) -> Result<JsValue> {
        await_idb_request(&request)
            .await
            .map_err(map_js_err("sync queue request failed"))
    }

    /// Persist a queued operation so the service worker can replay it later,
    /// even if this page closes before the `sync` event fires.
    pub async fn persist_operation(queue_name: &str, operation: &QueuedOperation) -> Result<()> {
        let db = open_db().await?;
        let txn = transaction(&db, IdbTransactionMode::Readwrite)?;
        let object_store = store(&txn)?;

        // Store the operation as a JS object with an extra `queue_name`
        // field so the service worker can filter by it directly.
        let js_value = serde_wasm_bindgen::to_value(operation).map_err(|e| {
            PwaError::SyncQueuePersistenceFailed(format!(
                "failed to serialize queued operation: {e}"
            ))
        })?;
        js_sys::Reflect::set(
            &js_value,
            &JsValue::from_str("queue_name"),
            &JsValue::from_str(queue_name),
        )
        .map_err(map_js_err("failed to attach queue_name"))?;

        let request = object_store
            .put_with_key(&js_value, &JsValue::from_str(&operation.id))
            .map_err(map_js_err("failed to put queued operation"))?;

        settle(request).await?;
        Ok(())
    }

    /// Remove a previously-persisted operation (e.g. after it has been
    /// successfully replayed, or its retries are exhausted).
    pub async fn delete_operation(operation_id: &str) -> Result<()> {
        let db = open_db().await?;
        let txn = transaction(&db, IdbTransactionMode::Readwrite)?;
        let object_store = store(&txn)?;

        let request = object_store
            .delete(&JsValue::from_str(operation_id))
            .map_err(map_js_err("failed to delete queued operation"))?;

        settle(request).await?;
        Ok(())
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
    ///
    /// The operation is persisted to IndexedDB (via [`persistence`]) *before*
    /// the background sync is registered, so that even if the browser fires
    /// the `sync` event after this page has already closed, the service
    /// worker (`templates/service-worker.js`'s `handleBackgroundSync`) has
    /// something real to read and replay -- it cannot see `self.queues`,
    /// which lives only in this page's WASM memory.
    pub async fn enqueue_operation(
        &mut self,
        queue_name: &str,
        operation: QueuedOperation,
    ) -> Result<()> {
        // Get or create queue
        if self.get_queue(queue_name).is_none() {
            self.add_queue(SyncQueue::new(queue_name));
        }

        // Persist first: if this fails, we haven't yet registered a sync
        // the service worker would wake up for with nothing to do.
        persistence::persist_operation(queue_name, &operation).await?;

        if let Some(queue) = self.get_queue_mut(queue_name) {
            queue.enqueue(operation);

            // Register background sync
            self.background_sync
                .register_one_time(persistence::tag_for_queue(queue_name))
                .await?;
        }

        Ok(())
    }

    /// Process all queues (page-side fallback/complement to the service
    /// worker's own background sync replay -- e.g. for use while the page
    /// is open and connectivity is available immediately).
    ///
    /// Successfully processed operations (and ones whose retries are
    /// exhausted) are also removed from the IndexedDB persistence layer, so
    /// a later `sync` event doesn't redundantly replay work this page
    /// already completed.
    pub async fn process_queues<F>(&mut self, mut processor: F) -> Result<()>
    where
        F: FnMut(&QueuedOperation) -> Result<bool>,
    {
        for queue in &mut self.queues {
            let mut failed_operations = Vec::new();

            while let Some(mut operation) = queue.dequeue() {
                match processor(&operation) {
                    Ok(true) => {
                        // Operation successful: drop the persisted copy too.
                        if let Err(e) = persistence::delete_operation(&operation.id).await {
                            web_sys::console::warn_1(&JsValue::from_str(&format!(
                                "failed to remove persisted sync operation {}: {e}",
                                operation.id
                            )));
                        }
                    }
                    Ok(false) | Err(_) => {
                        // Operation failed, check if should retry
                        if operation.should_retry() {
                            operation.increment_retry();
                            failed_operations.push(operation);
                        } else {
                            // Retries exhausted: give up and remove the
                            // persisted copy so it isn't replayed forever.
                            if let Err(e) = persistence::delete_operation(&operation.id).await {
                                web_sys::console::warn_1(&JsValue::from_str(&format!(
                                    "failed to remove exhausted sync operation {}: {e}",
                                    operation.id
                                )));
                            }
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
        let mut op = QueuedOperation::new(
            "op-1",
            "upload",
            "/api/upload",
            serde_json::json!({"file": "test.txt"}),
        );

        assert_eq!(op.retry_count, 0);
        assert_eq!(op.endpoint, "/api/upload");
        assert_eq!(op.method, "POST");
        assert!(op.should_retry());

        op.increment_retry();
        assert_eq!(op.retry_count, 1);
    }

    #[test]
    fn test_queued_operation_with_method() {
        let op = QueuedOperation::new("op-1", "delete", "/api/items/op-1", serde_json::json!({}))
            .with_method("DELETE");
        assert_eq!(op.method, "DELETE");
    }

    #[test]
    fn test_sync_queue() {
        let mut queue = SyncQueue::new("upload-queue");

        assert!(queue.is_empty());
        assert_eq!(queue.len(), 0);

        let op = QueuedOperation::new("op-1", "upload", "/api/upload", serde_json::json!({}));
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
        queue.enqueue(QueuedOperation::new(
            "op-1",
            "test",
            "/api/test",
            serde_json::json!({}),
        ));

        let json = queue.to_json()?;
        let deserialized = SyncQueue::from_json(&json)?;

        assert_eq!(deserialized.name, "test");
        assert_eq!(deserialized.len(), 1);
        assert_eq!(deserialized.operations()[0].endpoint, "/api/test");

        Ok(())
    }

    #[test]
    fn test_tag_for_queue_matches_service_worker_naming_scheme() {
        // This prefix must match the JS template's
        // `tag.replace(/^sync-/, '')` logic exactly.
        assert_eq!(persistence::tag_for_queue("uploads"), "sync-uploads");
    }
}
