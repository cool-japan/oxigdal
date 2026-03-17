//! STAC Transaction Extension — create / update / upsert / delete items and
//! collections in a STAC API catalog.
//!
//! <https://github.com/stac-extensions/transaction>
//!
//! This module provides:
//!
//! - [`TransactionOp`] – the four mutation operation types.
//! - [`TransactionResult`] – the outcome of a completed operation.
//! - [`StacItemStore`] – an in-memory item store suitable for unit tests;
//!   production deployments would substitute a database-backed implementation.

use std::collections::HashMap;
use std::time::SystemTime;

use serde_json::Value;

use crate::error::{Result, StacError};

// ── Operation type ────────────────────────────────────────────────────────────

/// The mutation operation performed on a STAC item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionOp {
    /// Create a new item (fails if one already exists with the same id).
    Create,
    /// Replace an existing item (fails if not found).
    Update,
    /// Create-or-replace semantics.
    Upsert,
    /// Remove an item permanently.
    Delete,
}

// ── Result ────────────────────────────────────────────────────────────────────

/// The outcome of a completed [`TransactionOp`].
#[derive(Debug)]
pub struct TransactionResult {
    /// Which operation was performed.
    pub op: TransactionOp,
    /// ID of the item that was operated on.
    pub item_id: String,
    /// ID of the collection that contains (or contained) the item.
    pub collection_id: String,
    /// Whether the operation completed successfully.
    pub success: bool,
    /// Wall-clock time at which the operation completed.
    pub created_at: SystemTime,
}

// ── In-memory item store ──────────────────────────────────────────────────────

/// Composite key used to store items: `(collection_id, item_id)`.
type ItemKey = (String, String);

/// An in-memory STAC item store implementing the Transaction Extension
/// semantics.
///
/// # Concurrency
///
/// This store is intentionally single-threaded.  Wrap it in `Arc<Mutex<…>>`
/// if you need shared access across threads.
#[derive(Debug, Default)]
pub struct StacItemStore {
    items: HashMap<ItemKey, Value>,
    collections: HashMap<String, Value>,
    transaction_log: Vec<TransactionResult>,
}

impl StacItemStore {
    /// Creates an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    // ── Item CRUD ─────────────────────────────────────────────────────────

    /// Creates a new item.
    ///
    /// # Errors
    ///
    /// Returns [`StacError::InvalidItem`] when the item JSON does not contain
    /// a string `"id"` field.
    /// Returns [`StacError::AlreadyExists`] when an item with the same id
    /// already exists in the collection.
    pub fn create_item(&mut self, collection: &str, item: Value) -> Result<TransactionResult> {
        let id = extract_id(&item)?;
        let key = make_key(collection, &id);
        if self.items.contains_key(&key) {
            return Err(StacError::AlreadyExists(id));
        }
        self.items.insert(key, item);
        let result = self.record(TransactionOp::Create, id, collection);
        Ok(result)
    }

    /// Updates an existing item.
    ///
    /// # Errors
    ///
    /// Returns [`StacError::NotFound`] when no item with `item_id` exists in
    /// the collection.
    pub fn update_item(
        &mut self,
        collection: &str,
        item_id: &str,
        item: Value,
    ) -> Result<TransactionResult> {
        let key = make_key(collection, item_id);
        if !self.items.contains_key(&key) {
            return Err(StacError::NotFound(item_id.to_string()));
        }
        self.items.insert(key, item);
        let result = self.record(TransactionOp::Update, item_id.to_string(), collection);
        Ok(result)
    }

    /// Creates or replaces an item (upsert semantics).
    ///
    /// # Errors
    ///
    /// Returns [`StacError::InvalidItem`] when the item JSON does not contain
    /// a string `"id"` field.
    pub fn upsert_item(&mut self, collection: &str, item: Value) -> Result<TransactionResult> {
        let id = extract_id(&item)?;
        let key = make_key(collection, &id);
        self.items.insert(key, item);
        let result = self.record(TransactionOp::Upsert, id, collection);
        Ok(result)
    }

    /// Deletes an item.
    ///
    /// # Errors
    ///
    /// Returns [`StacError::NotFound`] when no item with `item_id` exists in
    /// the collection.
    pub fn delete_item(&mut self, collection: &str, item_id: &str) -> Result<TransactionResult> {
        let key = make_key(collection, item_id);
        if self.items.remove(&key).is_none() {
            return Err(StacError::NotFound(item_id.to_string()));
        }
        let result = self.record(TransactionOp::Delete, item_id.to_string(), collection);
        Ok(result)
    }

    // ── Collection CRUD ───────────────────────────────────────────────────

    /// Stores a collection JSON value.
    pub fn put_collection(&mut self, id: impl Into<String>, collection: Value) {
        self.collections.insert(id.into(), collection);
    }

    /// Retrieves a stored collection by id.
    pub fn get_collection(&self, id: &str) -> Option<&Value> {
        self.collections.get(id)
    }

    /// Removes a collection.  Returns `true` if the collection existed.
    pub fn delete_collection(&mut self, id: &str) -> bool {
        self.collections.remove(id).is_some()
    }

    // ── Read accessors ────────────────────────────────────────────────────

    /// Retrieves an item by collection and item id.
    pub fn get_item(&self, collection: &str, item_id: &str) -> Option<&Value> {
        self.items.get(&make_key(collection, item_id))
    }

    /// Lists all items belonging to `collection`.
    pub fn list_items(&self, collection: &str) -> Vec<&Value> {
        self.items
            .iter()
            .filter(|((col, _), _)| col == collection)
            .map(|(_, v)| v)
            .collect()
    }

    /// Total number of items across all collections.
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// Read-only view of the transaction log.
    pub fn transaction_log(&self) -> &[TransactionResult] {
        &self.transaction_log
    }

    // ── Private helpers ───────────────────────────────────────────────────

    fn record(
        &mut self,
        op: TransactionOp,
        item_id: String,
        collection: &str,
    ) -> TransactionResult {
        let result = TransactionResult {
            op,
            item_id,
            collection_id: collection.to_string(),
            success: true,
            created_at: SystemTime::now(),
        };
        self.transaction_log.push(result.clone());
        result
    }
}

// ── Free helpers ──────────────────────────────────────────────────────────────

fn make_key(collection: &str, item_id: &str) -> ItemKey {
    (collection.to_string(), item_id.to_string())
}

fn extract_id(item: &Value) -> Result<String> {
    item.get("id")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| StacError::InvalidItem("item missing 'id' field".to_string()))
}

// ── Clone for TransactionResult ───────────────────────────────────────────────

impl Clone for TransactionResult {
    fn clone(&self) -> Self {
        Self {
            op: self.op,
            item_id: self.item_id.clone(),
            collection_id: self.collection_id.clone(),
            success: self.success,
            created_at: self.created_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn item(id: &str) -> Value {
        json!({ "id": id, "type": "Feature", "stac_version": "1.0.0" })
    }

    #[test]
    fn test_create_item_success() {
        let mut store = StacItemStore::new();
        let r = store.create_item("col1", item("item-a")).expect("create");
        assert_eq!(r.op, TransactionOp::Create);
        assert_eq!(r.item_id, "item-a");
        assert!(r.success);
    }

    #[test]
    fn test_create_duplicate_returns_error() {
        let mut store = StacItemStore::new();
        store
            .create_item("col1", item("item-a"))
            .expect("first create");
        let err = store.create_item("col1", item("item-a"));
        assert!(err.is_err());
        assert!(matches!(err, Err(StacError::AlreadyExists(_))));
        if let Err(StacError::AlreadyExists(id)) = err {
            assert_eq!(id, "item-a");
        }
    }

    #[test]
    fn test_update_existing() {
        let mut store = StacItemStore::new();
        store.create_item("col1", item("item-b")).expect("create");
        let updated =
            json!({ "id": "item-b", "type": "Feature", "stac_version": "1.0.0", "updated": true });
        let r = store
            .update_item("col1", "item-b", updated)
            .expect("update");
        assert_eq!(r.op, TransactionOp::Update);
    }

    #[test]
    fn test_update_non_existent_fails() {
        let mut store = StacItemStore::new();
        let err = store.update_item("col1", "ghost", item("ghost"));
        assert!(err.is_err());
        assert!(matches!(err, Err(StacError::NotFound(_))));
    }

    #[test]
    fn test_upsert_creates_new() {
        let mut store = StacItemStore::new();
        let r = store.upsert_item("col1", item("item-c")).expect("upsert");
        assert_eq!(r.op, TransactionOp::Upsert);
        assert!(store.get_item("col1", "item-c").is_some());
    }

    #[test]
    fn test_upsert_replaces_existing() {
        let mut store = StacItemStore::new();
        store.create_item("col1", item("item-d")).expect("create");
        let v2 = json!({ "id": "item-d", "version": 2 });
        store.upsert_item("col1", v2).expect("upsert");
        let stored = store.get_item("col1", "item-d").expect("get");
        assert_eq!(stored["version"], 2);
    }

    #[test]
    fn test_delete_existing() {
        let mut store = StacItemStore::new();
        store.create_item("col1", item("item-e")).expect("create");
        let r = store.delete_item("col1", "item-e").expect("delete");
        assert_eq!(r.op, TransactionOp::Delete);
        assert!(store.get_item("col1", "item-e").is_none());
    }

    #[test]
    fn test_delete_non_existent_fails() {
        let mut store = StacItemStore::new();
        let err = store.delete_item("col1", "no-such-item");
        assert!(err.is_err());
        assert!(matches!(err, Err(StacError::NotFound(_))));
    }

    #[test]
    fn test_get_item() {
        let mut store = StacItemStore::new();
        store.create_item("col1", item("item-f")).expect("create");
        assert!(store.get_item("col1", "item-f").is_some());
        assert!(store.get_item("col1", "other").is_none());
    }

    #[test]
    fn test_list_items_by_collection() {
        let mut store = StacItemStore::new();
        store.create_item("col-a", item("x1")).expect("create");
        store.create_item("col-a", item("x2")).expect("create");
        store.create_item("col-b", item("y1")).expect("create");
        assert_eq!(store.list_items("col-a").len(), 2);
        assert_eq!(store.list_items("col-b").len(), 1);
    }

    #[test]
    fn test_transaction_log_populated() {
        let mut store = StacItemStore::new();
        store.create_item("col1", item("log-item")).expect("create");
        store.upsert_item("col1", item("log-item")).expect("upsert");
        assert_eq!(store.transaction_log().len(), 2);
        assert_eq!(store.transaction_log()[0].op, TransactionOp::Create);
        assert_eq!(store.transaction_log()[1].op, TransactionOp::Upsert);
    }

    #[test]
    fn test_missing_id_field() {
        let mut store = StacItemStore::new();
        let no_id = json!({ "type": "Feature" });
        let err = store.create_item("col1", no_id);
        assert!(err.is_err());
        assert!(matches!(err, Err(StacError::InvalidItem(_))));
    }

    #[test]
    fn test_item_count() {
        let mut store = StacItemStore::new();
        assert_eq!(store.item_count(), 0);
        store.create_item("col1", item("cnt-1")).expect("create");
        store.create_item("col1", item("cnt-2")).expect("create");
        assert_eq!(store.item_count(), 2);
        store.delete_item("col1", "cnt-1").expect("delete");
        assert_eq!(store.item_count(), 1);
    }

    #[test]
    fn test_collection_operations() {
        let mut store = StacItemStore::new();
        let col = json!({ "id": "test-col", "type": "Collection" });
        store.put_collection("test-col", col.clone());
        assert!(store.get_collection("test-col").is_some());
        assert!(store.delete_collection("test-col"));
        assert!(store.get_collection("test-col").is_none());
    }
}
