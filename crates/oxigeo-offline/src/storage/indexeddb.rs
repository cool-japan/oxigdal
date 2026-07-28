//! IndexedDB storage backend for WASM platforms
//!
//! This module provides a full-featured IndexedDB implementation for offline data storage
//! in WebAssembly environments. It supports:
//! - Records storage with versioning and metadata
//! - Operation queue for sync management
//! - Async operations using wasm-bindgen-futures
//! - Transaction-based consistency
//! - Efficient indexing and querying

use crate::error::{Error, Result};
use crate::storage::{StorageBackend, StorageStatistics, serialization};
use crate::types::{Operation, OperationId, Record, RecordId};
use async_trait::async_trait;
use js_sys::Uint8Array;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    DomException, IdbCursorWithValue, IdbDatabase, IdbFactory, IdbObjectStore, IdbOpenDbRequest,
    IdbRequest, IdbTransaction, IdbTransactionMode, IdbVersionChangeEvent,
};

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

/// `true` if `err` is the `DOMException` IndexedDB throws when creating an
/// object store/index that already exists -- i.e. safe to ignore when
/// (re-)opening a database whose schema was already created by a previous
/// session/version.
fn is_already_exists_error(err: &JsValue) -> bool {
    err.dyn_ref::<DomException>()
        .is_some_and(|e| e.name() == "ConstraintError")
}

/// Database version for schema management
const DB_VERSION: u32 = 1;

/// Object store names
const RECORDS_STORE: &str = "records";
const OPERATIONS_STORE: &str = "operations";

/// Index names
const RECORDS_KEY_INDEX: &str = "by_key";
const OPERATIONS_PRIORITY_INDEX: &str = "by_priority";

/// IndexedDB storage backend for WASM platforms
///
/// Provides persistent storage using the browser's IndexedDB API with full support
/// for offline data management, sync queues, and conflict resolution.
pub struct IndexedDbBackend {
    db_name: String,
    db: Option<Rc<IdbDatabase>>,
}

impl IndexedDbBackend {
    /// Create a new IndexedDB backend
    ///
    /// # Arguments
    ///
    /// * `db_name` - Name of the IndexedDB database
    pub fn new(db_name: String) -> Self {
        Self { db_name, db: None }
    }

    /// Get the IndexedDB factory
    fn get_factory() -> Result<IdbFactory> {
        let window = web_sys::window().ok_or_else(|| Error::storage("No window object"))?;
        let idb = window
            .indexed_db()
            .map_err(|e| Error::storage(format!("Failed to get IndexedDB: {e:?}")))?
            .ok_or_else(|| Error::storage("IndexedDB not supported"))?;
        Ok(idb)
    }

    /// Open or create the IndexedDB database
    async fn open_database(&self) -> Result<IdbDatabase> {
        let factory = Self::get_factory()?;

        let open_request = factory
            .open_with_u32(&self.db_name, DB_VERSION)
            .map_err(|e| Error::storage(format!("Failed to open database: {e:?}")))?;

        // The `onupgradeneeded` closure runs synchronously as a DOM event
        // callback and cannot itself return a `Result` to this async
        // function. Schema-creation failures (quota, disallowed
        // characters, an object store/index that already exists with
        // different parameters, ...) are captured here instead of being
        // silently discarded via `.ok()`/`let _ =`, so a failure surfaces
        // as a real `Error::storage(...)` after the open request settles
        // rather than leaving the database silently missing a store or
        // index that later query methods depend on.
        let schema_error: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let schema_error_handle = Rc::clone(&schema_error);

        /// Record a schema-creation failure: log it immediately (the only
        /// way to surface it while still inside the synchronous DOM
        /// callback) and stash the first failure so `open_database` can
        /// turn it into a real error once the open request settles.
        fn record_schema_error(handle: &Rc<RefCell<Option<String>>>, message: String) {
            web_sys::console::error_1(&JsValue::from_str(&format!(
                "IndexedDB schema creation failed: {message}"
            )));
            let mut slot = handle.borrow_mut();
            if slot.is_none() {
                *slot = Some(message);
            }
        }

        // Set up upgrade handler for schema creation/migration
        let upgrade_handler = Closure::once(move |event: IdbVersionChangeEvent| {
            let target = event
                .target()
                .and_then(|t| t.dyn_into::<IdbOpenDbRequest>().ok())
                .and_then(|r| r.result().ok())
                .and_then(|r| r.dyn_into::<IdbDatabase>().ok());

            let Some(db) = target else {
                record_schema_error(
                    &schema_error_handle,
                    "onupgradeneeded fired but no IdbDatabase result was available".to_string(),
                );
                return;
            };

            // Create records object store. Values are stored as opaque
            // serialized byte blobs (see `bytes_to_js`/`js_to_bytes`), so
            // the "key field" index below is created with a placeholder
            // keyPath (it currently indexes no real property on the
            // stored blob values) -- it exists so the store's schema
            // matches its documented name, but no query in this file uses
            // it yet; real per-field indexing would require storing
            // structured JS objects instead of raw bytes.
            //
            // `create_object_store` throws a `ConstraintError` if the store
            // already exists (e.g. this handler ran again after a future
            // `DB_VERSION` bump) -- that's expected/idempotent, not a real
            // failure, so it's the only error swallowed without being
            // recorded.
            match db.create_object_store(RECORDS_STORE) {
                Ok(store) => {
                    if let Err(e) =
                        store.create_index_with_str(RECORDS_KEY_INDEX, RECORDS_KEY_INDEX)
                    {
                        if !is_already_exists_error(&e) {
                            record_schema_error(
                                &schema_error_handle,
                                format!(
                                    "failed to create index {RECORDS_KEY_INDEX} on \
                                     {RECORDS_STORE}: {e:?}"
                                ),
                            );
                        }
                    }
                }
                Err(e) if !is_already_exists_error(&e) => {
                    record_schema_error(
                        &schema_error_handle,
                        format!("failed to create object store {RECORDS_STORE}: {e:?}"),
                    );
                }
                Err(_) => {}
            }

            // Create operations object store (same opaque-blob caveat as above).
            match db.create_object_store(OPERATIONS_STORE) {
                Ok(store) => {
                    if let Err(e) = store
                        .create_index_with_str(OPERATIONS_PRIORITY_INDEX, OPERATIONS_PRIORITY_INDEX)
                    {
                        if !is_already_exists_error(&e) {
                            record_schema_error(
                                &schema_error_handle,
                                format!(
                                    "failed to create index {OPERATIONS_PRIORITY_INDEX} on \
                                     {OPERATIONS_STORE}: {e:?}"
                                ),
                            );
                        }
                    }
                }
                Err(e) if !is_already_exists_error(&e) => {
                    record_schema_error(
                        &schema_error_handle,
                        format!("failed to create object store {OPERATIONS_STORE}: {e:?}"),
                    );
                }
                Err(_) => {}
            }
        });

        open_request.set_onupgradeneeded(Some(upgrade_handler.as_ref().unchecked_ref()));
        upgrade_handler.forget();

        // Wait for database to open. `IdbOpenDbRequest` is event-based
        // (onsuccess/onerror), not `Promise`-returning, hence
        // `await_idb_request` rather than `JsFuture::from(open_request)`.
        let result = await_idb_request(&open_request)
            .await
            .map_err(|e| Error::storage(format!("Database open failed: {e:?}")))?;

        // `onupgradeneeded` (if it fired at all) has already run to
        // completion by the time the open request's success/error settles,
        // so any schema-creation failure captured above is visible here.
        if let Some(message) = schema_error.borrow_mut().take() {
            return Err(Error::storage(format!(
                "IndexedDB schema creation failed during upgrade: {message}"
            )));
        }

        let db = result
            .dyn_into::<IdbDatabase>()
            .map_err(|_| Error::storage("Failed to cast to IdbDatabase"))?;

        Ok(db)
    }

    /// Get the database handle (lazy initialization)
    async fn db(&mut self) -> Result<Rc<IdbDatabase>> {
        if self.db.is_none() {
            let db = self.open_database().await?;
            self.db = Some(Rc::new(db));
        }
        self.db
            .clone()
            .ok_or_else(|| Error::internal("Database not initialized"))
    }

    /// Create a read-only transaction
    async fn read_transaction(&mut self, store_name: &str) -> Result<IdbTransaction> {
        let db = self.db().await?;
        db.transaction_with_str_and_mode(store_name, IdbTransactionMode::Readonly)
            .map_err(|e| Error::storage(format!("Failed to create read transaction: {e:?}")))
    }

    /// Create a read-write transaction
    async fn write_transaction(&mut self, store_name: &str) -> Result<IdbTransaction> {
        let db = self.db().await?;
        db.transaction_with_str_and_mode(store_name, IdbTransactionMode::Readwrite)
            .map_err(|e| Error::storage(format!("Failed to create write transaction: {e:?}")))
    }

    /// Get an object store from a transaction
    fn get_store(transaction: &IdbTransaction, store_name: &str) -> Result<IdbObjectStore> {
        transaction
            .object_store(store_name)
            .map_err(|e| Error::storage(format!("Failed to get object store: {e:?}")))
    }

    /// Convert bytes to JsValue for storage
    fn bytes_to_js(data: &[u8]) -> JsValue {
        let array = Uint8Array::new_with_length(data.len() as u32);
        array.copy_from(data);
        array.into()
    }

    /// Convert JsValue to bytes
    fn js_to_bytes(value: JsValue) -> Result<Vec<u8>> {
        let array = Uint8Array::new(&value);
        Ok(array.to_vec())
    }

    /// Execute a request and wait for completion. `IdbRequest` is
    /// event-based (onsuccess/onerror), not `Promise`-returning, hence
    /// [`await_idb_request`] rather than `JsFuture::from(request)`.
    async fn execute_request(request: &IdbRequest) -> Result<JsValue> {
        await_idb_request(request)
            .await
            .map_err(|e| Error::storage(format!("Request failed: {e:?}")))
    }

    /// Get a value from the object store
    async fn get_value(&mut self, store_name: &str, key: &str) -> Result<Option<JsValue>> {
        let transaction = self.read_transaction(store_name).await?;
        let store = Self::get_store(&transaction, store_name)?;

        let request = store
            .get(&JsValue::from_str(key))
            .map_err(|e| Error::storage(format!("Failed to get value: {e:?}")))?;

        let result = Self::execute_request(&request).await?;

        Ok(if result.is_undefined() || result.is_null() {
            None
        } else {
            Some(result)
        })
    }

    /// Put a value into the object store
    async fn put_value(&mut self, store_name: &str, key: &str, value: JsValue) -> Result<()> {
        let transaction = self.write_transaction(store_name).await?;
        let store = Self::get_store(&transaction, store_name)?;

        let request = store
            .put_with_key(&value, &JsValue::from_str(key))
            .map_err(|e| Error::storage(format!("Failed to put value: {e:?}")))?;

        Self::execute_request(&request).await?;
        Ok(())
    }

    /// Delete a value from the object store
    async fn delete_value(&mut self, store_name: &str, key: &str) -> Result<()> {
        let transaction = self.write_transaction(store_name).await?;
        let store = Self::get_store(&transaction, store_name)?;

        let request = store
            .delete(&JsValue::from_str(key))
            .map_err(|e| Error::storage(format!("Failed to delete value: {e:?}")))?;

        Self::execute_request(&request).await?;
        Ok(())
    }

    /// List all values from the object store
    async fn list_values(&mut self, store_name: &str) -> Result<Vec<JsValue>> {
        let transaction = self.read_transaction(store_name).await?;
        let store = Self::get_store(&transaction, store_name)?;

        let request = store
            .open_cursor()
            .map_err(|e| Error::storage(format!("Failed to open cursor: {e:?}")))?;

        let mut values = Vec::new();

        // `IdbCursor::continue_()` doesn't return a new request -- it just
        // re-arms the *same* underlying cursor request, which then fires
        // another `success` event (with the next value, or `null` once
        // exhausted). So we keep awaiting the one `request` object rather
        // than chasing a new one each iteration.
        loop {
            let result = Self::execute_request(&request).await?;

            if result.is_null() || result.is_undefined() {
                break;
            }

            let cursor = result.dyn_into::<IdbCursorWithValue>().map_err(|_| {
                Error::storage("Cursor result was not an IdbCursorWithValue".to_string())
            })?;

            let value = cursor
                .value()
                .map_err(|e| Error::storage(format!("Failed to read cursor value: {e:?}")))?;
            values.push(value);

            cursor
                .continue_()
                .map_err(|e| Error::storage(format!("Failed to continue cursor: {e:?}")))?;
        }

        Ok(values)
    }

    /// Count values in the object store
    async fn count_values(&mut self, store_name: &str) -> Result<usize> {
        let transaction = self.read_transaction(store_name).await?;
        let store = Self::get_store(&transaction, store_name)?;

        let request = store
            .count()
            .map_err(|e| Error::storage(format!("Failed to count: {e:?}")))?;

        let result = Self::execute_request(&request).await?;

        // Extract count from JsValue
        if let Some(count) = result.as_f64() {
            Ok(count as usize)
        } else {
            Ok(0)
        }
    }

    /// Clear all values from the object store
    async fn clear_store(&mut self, store_name: &str) -> Result<()> {
        let transaction = self.write_transaction(store_name).await?;
        let store = Self::get_store(&transaction, store_name)?;

        let request = store
            .clear()
            .map_err(|e| Error::storage(format!("Failed to clear store: {e:?}")))?;

        Self::execute_request(&request).await?;
        Ok(())
    }
}

#[async_trait(?Send)]
impl StorageBackend for IndexedDbBackend {
    async fn initialize(&mut self) -> Result<()> {
        // Open database (this will create it if it doesn't exist)
        self.db().await?;
        tracing::info!("IndexedDB backend initialized: {}", self.db_name);
        Ok(())
    }

    async fn put_record(&mut self, record: &Record) -> Result<()> {
        let serialized = serialization::serialize_record(record)?;
        let js_value = Self::bytes_to_js(&serialized);
        self.put_value(RECORDS_STORE, &record.key, js_value).await?;
        Ok(())
    }

    async fn get_record(&self, key: &str) -> Result<Option<Record>> {
        let mut backend = Self {
            db_name: self.db_name.clone(),
            db: self.db.clone(),
        };

        let value = backend.get_value(RECORDS_STORE, key).await?;

        match value {
            Some(js_val) => {
                let bytes = Self::js_to_bytes(js_val)?;
                let record = serialization::deserialize_record(&bytes)?;
                Ok(if record.deleted { None } else { Some(record) })
            }
            None => Ok(None),
        }
    }

    async fn get_record_by_id(&self, id: &RecordId) -> Result<Option<Record>> {
        // IndexedDB doesn't have efficient ID lookups without an index
        // For now, we scan all records (could be optimized with a secondary index)
        let mut backend = Self {
            db_name: self.db_name.clone(),
            db: self.db.clone(),
        };

        let values = backend.list_values(RECORDS_STORE).await?;

        for js_val in values {
            let bytes = Self::js_to_bytes(js_val)?;
            let record = serialization::deserialize_record(&bytes)?;

            if record.id == *id && !record.deleted {
                return Ok(Some(record));
            }
        }

        Ok(None)
    }

    async fn delete_record(&mut self, key: &str) -> Result<()> {
        // Mark as deleted rather than removing (tombstone)
        if let Some(mut record) = self.get_record(key).await? {
            record.mark_deleted();
            self.put_record(&record).await?;
        }
        Ok(())
    }

    async fn list_records(&self) -> Result<Vec<Record>> {
        let mut backend = Self {
            db_name: self.db_name.clone(),
            db: self.db.clone(),
        };

        let values = backend.list_values(RECORDS_STORE).await?;
        let mut records = Vec::new();

        for js_val in values {
            let bytes = Self::js_to_bytes(js_val)?;
            let record = serialization::deserialize_record(&bytes)?;

            if !record.deleted {
                records.push(record);
            }
        }

        // Sort by updated_at descending
        records.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

        Ok(records)
    }

    async fn count_records(&self) -> Result<usize> {
        let mut backend = Self {
            db_name: self.db_name.clone(),
            db: self.db.clone(),
        };

        let values = backend.list_values(RECORDS_STORE).await?;
        let mut count = 0;

        for js_val in values {
            let bytes = Self::js_to_bytes(js_val)?;
            let record = serialization::deserialize_record(&bytes)?;

            if !record.deleted {
                count += 1;
            }
        }

        Ok(count)
    }

    async fn clear_records(&mut self) -> Result<()> {
        self.clear_store(RECORDS_STORE).await
    }

    async fn enqueue_operation(&mut self, operation: &Operation) -> Result<()> {
        let serialized = serialization::serialize_operation(operation)?;
        let js_value = Self::bytes_to_js(&serialized);
        self.put_value(OPERATIONS_STORE, &operation.id.to_string(), js_value)
            .await?;
        Ok(())
    }

    async fn get_pending_operations(&self, limit: usize) -> Result<Vec<Operation>> {
        let mut backend = Self {
            db_name: self.db_name.clone(),
            db: self.db.clone(),
        };

        let values = backend.list_values(OPERATIONS_STORE).await?;
        let mut operations = Vec::new();

        for js_val in values {
            let bytes = Self::js_to_bytes(js_val)?;
            let operation = serialization::deserialize_operation(&bytes)?;
            operations.push(operation);
        }

        // Sort by priority (desc) and created_at (asc)
        operations.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then_with(|| a.created_at.cmp(&b.created_at))
        });

        // Apply limit
        operations.truncate(limit);

        Ok(operations)
    }

    async fn dequeue_operation(&mut self, operation_id: &OperationId) -> Result<()> {
        self.delete_value(OPERATIONS_STORE, &operation_id.to_string())
            .await
    }

    async fn update_operation(&mut self, operation: &Operation) -> Result<()> {
        // Re-serialize and store the updated operation
        self.enqueue_operation(operation).await
    }

    async fn count_pending_operations(&self) -> Result<usize> {
        let mut backend = Self {
            db_name: self.db_name.clone(),
            db: self.db.clone(),
        };

        backend.count_values(OPERATIONS_STORE).await
    }

    async fn clear_operations(&mut self) -> Result<()> {
        self.clear_store(OPERATIONS_STORE).await
    }

    async fn get_statistics(&self) -> Result<StorageStatistics> {
        let record_count = self.count_records().await?;
        let pending_operations = self.count_pending_operations().await?;

        // Calculate storage sizes (approximate)
        let mut backend = Self {
            db_name: self.db_name.clone(),
            db: self.db.clone(),
        };

        let record_values = backend.list_values(RECORDS_STORE).await?;
        let mut record_size_bytes = 0u64;

        for js_val in record_values {
            let bytes = Self::js_to_bytes(js_val)?;
            record_size_bytes += bytes.len() as u64;
        }

        let operation_values = backend.list_values(OPERATIONS_STORE).await?;
        let mut operations_size_bytes = 0u64;

        for js_val in operation_values {
            let bytes = Self::js_to_bytes(js_val)?;
            operations_size_bytes += bytes.len() as u64;
        }

        let mut stats = StorageStatistics {
            record_count,
            record_size_bytes,
            pending_operations,
            operations_size_bytes,
            backend_type: "IndexedDB".to_string(),
            custom_metrics: Vec::new(),
        };

        stats.add_metric("database_name".to_string(), self.db_name.clone());
        stats.add_metric("database_version".to_string(), DB_VERSION.to_string());

        Ok(stats)
    }

    async fn compact(&mut self) -> Result<()> {
        // IndexedDB doesn't have explicit compaction
        // We could implement vacuum by recreating stores, but it's not necessary
        tracing::debug!("IndexedDB compact is a no-op");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    async fn test_indexeddb_initialization() {
        let mut backend = IndexedDbBackend::new("test_db".to_string());
        let result = backend.initialize().await;
        assert!(result.is_ok());
    }

    #[wasm_bindgen_test]
    async fn test_record_operations() {
        let mut backend = IndexedDbBackend::new("test_records".to_string());
        backend.initialize().await.ok();

        // Create and store a record
        let record = Record::new("test_key".to_string(), Bytes::from("test_data"));
        let put_result = backend.put_record(&record).await;
        assert!(put_result.is_ok());

        // Retrieve the record
        let retrieved = backend.get_record("test_key").await;
        assert!(retrieved.is_ok());
        let retrieved_record = retrieved.ok().and_then(|r| r);
        assert!(retrieved_record.is_some());
        assert_eq!(
            retrieved_record.as_ref().map(|r| &r.key),
            Some(&"test_key".to_string())
        );

        // Count records
        let count = backend.count_records().await;
        assert!(count.is_ok());
        assert_eq!(count.ok(), Some(1));

        // Delete record
        let delete_result = backend.delete_record("test_key").await;
        assert!(delete_result.is_ok());

        // Verify deletion
        let count_after = backend.count_records().await;
        assert_eq!(count_after.ok(), Some(0));

        // Cleanup
        let _ = backend.clear_records().await;
    }

    #[wasm_bindgen_test]
    async fn test_operation_queue() {
        let mut backend = IndexedDbBackend::new("test_operations".to_string());
        backend.initialize().await.ok();

        // Create and enqueue operation
        let record = Record::new("op_test".to_string(), Bytes::from("data"));
        let operation = Operation::insert(&record);

        let enqueue_result = backend.enqueue_operation(&operation).await;
        assert!(enqueue_result.is_ok());

        // Get pending operations
        let pending = backend.get_pending_operations(10).await;
        assert!(pending.is_ok());
        assert_eq!(pending.as_ref().ok().map(|p| p.len()), Some(1));

        // Dequeue operation
        let dequeue_result = backend.dequeue_operation(&operation.id).await;
        assert!(dequeue_result.is_ok());

        // Verify empty queue
        let pending_after = backend.get_pending_operations(10).await;
        assert_eq!(pending_after.ok().map(|p| p.len()), Some(0));

        // Cleanup
        let _ = backend.clear_operations().await;
    }

    #[wasm_bindgen_test]
    async fn test_statistics() {
        let mut backend = IndexedDbBackend::new("test_stats".to_string());
        backend.initialize().await.ok();

        let stats = backend.get_statistics().await;
        assert!(stats.is_ok());

        let statistics = stats.ok();
        assert!(statistics.is_some());
        assert_eq!(
            statistics.as_ref().map(|s| &s.backend_type),
            Some(&"IndexedDB".to_string())
        );

        // Cleanup
        let _ = backend.clear_records().await;
        let _ = backend.clear_operations().await;
    }

    #[wasm_bindgen_test]
    async fn test_list_records() {
        let mut backend = IndexedDbBackend::new("test_list".to_string());
        backend.initialize().await.ok();

        // Add multiple records
        for i in 0..5 {
            let record = Record::new(format!("key_{}", i), Bytes::from(format!("data_{}", i)));
            let _ = backend.put_record(&record).await;
        }

        // List all records
        let records = backend.list_records().await;
        assert!(records.is_ok());
        assert_eq!(records.ok().map(|r| r.len()), Some(5));

        // Cleanup
        let _ = backend.clear_records().await;
    }

    #[wasm_bindgen_test]
    async fn test_operation_priority() {
        let mut backend = IndexedDbBackend::new("test_priority".to_string());
        backend.initialize().await.ok();

        // Create operations with different priorities
        let record1 = Record::new("key1".to_string(), Bytes::from("data1"));
        let mut op1 = Operation::insert(&record1);
        op1.priority = 3;

        let record2 = Record::new("key2".to_string(), Bytes::from("data2"));
        let mut op2 = Operation::insert(&record2);
        op2.priority = 8;

        let record3 = Record::new("key3".to_string(), Bytes::from("data3"));
        let mut op3 = Operation::insert(&record3);
        op3.priority = 5;

        // Enqueue in random order
        let _ = backend.enqueue_operation(&op1).await;
        let _ = backend.enqueue_operation(&op2).await;
        let _ = backend.enqueue_operation(&op3).await;

        // Get pending operations (should be sorted by priority)
        let pending = backend.get_pending_operations(10).await;
        assert!(pending.is_ok());

        if let Ok(ops) = pending {
            assert_eq!(ops.len(), 3);
            // Highest priority first
            assert_eq!(ops[0].priority, 8);
            assert_eq!(ops[1].priority, 5);
            assert_eq!(ops[2].priority, 3);
        }

        // Cleanup
        let _ = backend.clear_operations().await;
    }

    #[wasm_bindgen_test]
    async fn test_update_operation() {
        let mut backend = IndexedDbBackend::new("test_update_op".to_string());
        backend.initialize().await.ok();

        // Create and enqueue operation
        let record = Record::new("update_test".to_string(), Bytes::from("data"));
        let mut operation = Operation::insert(&record);

        let _ = backend.enqueue_operation(&operation).await;

        // Update operation (increment retry count)
        operation.increment_retry();
        let update_result = backend.update_operation(&operation).await;
        assert!(update_result.is_ok());

        // Verify update
        let pending = backend.get_pending_operations(10).await;
        if let Ok(ops) = pending {
            assert_eq!(ops.len(), 1);
            assert_eq!(ops[0].retry_count, 1);
            assert!(ops[0].last_retry.is_some());
        }

        // Cleanup
        let _ = backend.clear_operations().await;
    }

    #[wasm_bindgen_test]
    async fn test_record_by_id() {
        let mut backend = IndexedDbBackend::new("test_by_id".to_string());
        backend.initialize().await.ok();

        // Create and store a record
        let record = Record::new("id_test".to_string(), Bytes::from("test_data"));
        let record_id = record.id;

        let _ = backend.put_record(&record).await;

        // Retrieve by ID
        let retrieved = backend.get_record_by_id(&record_id).await;
        assert!(retrieved.is_ok());

        if let Ok(Some(rec)) = retrieved {
            assert_eq!(rec.id, record_id);
            assert_eq!(rec.key, "id_test");
        }

        // Cleanup
        let _ = backend.clear_records().await;
    }
}
