//! In-memory storage backend for Zarr arrays
//!
//! This module provides an in-memory implementation of the Store trait,
//! useful for testing and temporary arrays.

use super::{Store, StoreKey, StoreMetadata};
use crate::error::{Result, StorageError, ZarrError};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// In-memory store implementation
#[derive(Debug, Clone)]
pub struct MemoryStore {
    /// Internal storage map
    data: Arc<RwLock<HashMap<String, Vec<u8>>>>,
    /// Whether the store is read-only
    readonly: bool,
}

impl MemoryStore {
    /// Creates a new empty memory store
    #[must_use]
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
            readonly: false,
        }
    }

    /// Creates a memory store from existing data
    #[must_use]
    pub fn from_data(data: HashMap<String, Vec<u8>>) -> Self {
        Self {
            data: Arc::new(RwLock::new(data)),
            readonly: false,
        }
    }

    /// Creates a read-only memory store
    #[must_use]
    pub fn readonly(data: HashMap<String, Vec<u8>>) -> Self {
        Self {
            data: Arc::new(RwLock::new(data)),
            readonly: true,
        }
    }

    /// Returns the number of keys in the store
    pub fn len(&self) -> Result<usize> {
        let data = self.data.read().map_err(|e| {
            ZarrError::Storage(StorageError::Cache {
                message: format!("Lock poisoned: {e}"),
            })
        })?;
        Ok(data.len())
    }

    /// Checks if the store is empty
    pub fn is_empty(&self) -> Result<bool> {
        Ok(self.len()? == 0)
    }

    /// Clears all data from the store
    pub fn clear(&mut self) -> Result<()> {
        if self.readonly {
            return Err(ZarrError::Storage(StorageError::ReadOnly));
        }

        let mut data = self.data.write().map_err(|e| {
            ZarrError::Storage(StorageError::Cache {
                message: format!("Lock poisoned: {e}"),
            })
        })?;
        data.clear();
        Ok(())
    }

    /// Returns a clone of all data
    pub fn to_hashmap(&self) -> Result<HashMap<String, Vec<u8>>> {
        let data = self.data.read().map_err(|e| {
            ZarrError::Storage(StorageError::Cache {
                message: format!("Lock poisoned: {e}"),
            })
        })?;
        Ok(data.clone())
    }

    /// Returns metadata about the store
    #[must_use]
    pub fn metadata(&self) -> StoreMetadata {
        StoreMetadata::new("memory", true, !self.readonly, true)
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl Store for MemoryStore {
    fn exists(&self, key: &StoreKey) -> Result<bool> {
        let data = self.data.read().map_err(|e| {
            ZarrError::Storage(StorageError::Cache {
                message: format!("Lock poisoned: {e}"),
            })
        })?;
        Ok(data.contains_key(key.as_str()))
    }

    fn get(&self, key: &StoreKey) -> Result<Vec<u8>> {
        let data = self.data.read().map_err(|e| {
            ZarrError::Storage(StorageError::Cache {
                message: format!("Lock poisoned: {e}"),
            })
        })?;

        data.get(key.as_str()).cloned().ok_or_else(|| {
            ZarrError::Storage(StorageError::KeyNotFound {
                key: key.to_string(),
            })
        })
    }

    fn set(&mut self, key: &StoreKey, value: &[u8]) -> Result<()> {
        if self.readonly {
            return Err(ZarrError::Storage(StorageError::ReadOnly));
        }

        let mut data = self.data.write().map_err(|e| {
            ZarrError::Storage(StorageError::Cache {
                message: format!("Lock poisoned: {e}"),
            })
        })?;

        data.insert(key.as_str().to_string(), value.to_vec());
        Ok(())
    }

    fn delete(&mut self, key: &StoreKey) -> Result<()> {
        if self.readonly {
            return Err(ZarrError::Storage(StorageError::ReadOnly));
        }

        let mut data = self.data.write().map_err(|e| {
            ZarrError::Storage(StorageError::Cache {
                message: format!("Lock poisoned: {e}"),
            })
        })?;

        data.remove(key.as_str()).ok_or_else(|| {
            ZarrError::Storage(StorageError::KeyNotFound {
                key: key.to_string(),
            })
        })?;

        Ok(())
    }

    fn list_prefix(&self, prefix: &StoreKey) -> Result<Vec<StoreKey>> {
        let data = self.data.read().map_err(|e| {
            ZarrError::Storage(StorageError::Cache {
                message: format!("Lock poisoned: {e}"),
            })
        })?;

        let prefix_str = prefix.as_str();
        let mut keys: Vec<_> = data
            .keys()
            .filter(|k| prefix_str.is_empty() || k.starts_with(prefix_str))
            .map(|k| StoreKey::new(k.clone()))
            .collect();

        keys.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        Ok(keys)
    }

    fn is_readonly(&self) -> bool {
        self.readonly
    }

    fn get_many(&self, keys: &[StoreKey]) -> Result<Vec<Option<Vec<u8>>>> {
        let data = self.data.read().map_err(|e| {
            ZarrError::Storage(StorageError::Cache {
                message: format!("Lock poisoned: {e}"),
            })
        })?;

        Ok(keys
            .iter()
            .map(|key| data.get(key.as_str()).cloned())
            .collect())
    }

    fn set_many(&mut self, items: &[(StoreKey, Vec<u8>)]) -> Result<()> {
        if self.readonly {
            return Err(ZarrError::Storage(StorageError::ReadOnly));
        }

        let mut data = self.data.write().map_err(|e| {
            ZarrError::Storage(StorageError::Cache {
                message: format!("Lock poisoned: {e}"),
            })
        })?;

        for (key, value) in items {
            data.insert(key.as_str().to_string(), value.clone());
        }

        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        // Nothing to flush in memory
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_store_new() {
        let store = MemoryStore::new();
        assert!(store.is_empty().expect("check empty"));
        assert!(!store.is_readonly());
    }

    #[test]
    fn test_memory_store_set_get() {
        let mut store = MemoryStore::new();
        let key = StoreKey::new("test/key".to_string());
        let value = b"Hello, Memory!";

        store.set(&key, value).expect("set value");
        assert!(store.exists(&key).expect("check exists"));

        let retrieved = store.get(&key).expect("get value");
        assert_eq!(retrieved, value);

        assert_eq!(store.len().expect("check len"), 1);
    }

    #[test]
    fn test_memory_store_delete() {
        let mut store = MemoryStore::new();
        let key = StoreKey::new("test/key".to_string());

        store.set(&key, b"data").expect("set value");
        assert!(store.exists(&key).expect("check exists"));

        store.delete(&key).expect("delete value");
        assert!(!store.exists(&key).expect("check not exists"));
        assert!(store.is_empty().expect("check empty"));
    }

    #[test]
    fn test_memory_store_list() {
        let mut store = MemoryStore::new();

        store
            .set(&StoreKey::new("a/b/c".to_string()), b"1")
            .expect("set");
        store
            .set(&StoreKey::new("a/b/d".to_string()), b"2")
            .expect("set");
        store
            .set(&StoreKey::new("a/e".to_string()), b"3")
            .expect("set");
        store
            .set(&StoreKey::new("f".to_string()), b"4")
            .expect("set");

        let all_keys = store.list_all().expect("list all");
        assert_eq!(all_keys.len(), 4);

        let prefix_keys = store
            .list_prefix(&StoreKey::new("a/b".to_string()))
            .expect("list prefix");
        assert_eq!(prefix_keys.len(), 2);

        let prefix_keys2 = store
            .list_prefix(&StoreKey::new("a".to_string()))
            .expect("list prefix");
        assert_eq!(prefix_keys2.len(), 3);
    }

    #[test]
    fn test_memory_store_readonly() {
        let mut data = HashMap::new();
        data.insert("key".to_string(), b"value".to_vec());

        let mut store = MemoryStore::readonly(data);
        assert!(store.is_readonly());

        let key = StoreKey::new("key".to_string());
        let value = store.get(&key).expect("get value");
        assert_eq!(value, b"value");

        // Write operations should fail
        assert!(store.set(&key, b"new").is_err());
        assert!(store.delete(&key).is_err());
        assert!(store.clear().is_err());
    }

    #[test]
    fn test_memory_store_clear() {
        let mut store = MemoryStore::new();

        store
            .set(&StoreKey::new("key1".to_string()), b"val1")
            .expect("set");
        store
            .set(&StoreKey::new("key2".to_string()), b"val2")
            .expect("set");
        assert_eq!(store.len().expect("check len"), 2);

        store.clear().expect("clear");
        assert!(store.is_empty().expect("check empty"));
    }

    #[test]
    fn test_memory_store_get_many() {
        let mut store = MemoryStore::new();

        store
            .set(&StoreKey::new("k1".to_string()), b"v1")
            .expect("set");
        store
            .set(&StoreKey::new("k2".to_string()), b"v2")
            .expect("set");
        store
            .set(&StoreKey::new("k3".to_string()), b"v3")
            .expect("set");

        let keys = vec![
            StoreKey::new("k1".to_string()),
            StoreKey::new("k2".to_string()),
            StoreKey::new("missing".to_string()),
            StoreKey::new("k3".to_string()),
        ];

        let values = store.get_many(&keys).expect("get many");
        assert_eq!(values.len(), 4);
        assert_eq!(values[0], Some(b"v1".to_vec()));
        assert_eq!(values[1], Some(b"v2".to_vec()));
        assert_eq!(values[2], None);
        assert_eq!(values[3], Some(b"v3".to_vec()));
    }

    #[test]
    fn test_memory_store_set_many() {
        let mut store = MemoryStore::new();

        let items = vec![
            (StoreKey::new("k1".to_string()), b"v1".to_vec()),
            (StoreKey::new("k2".to_string()), b"v2".to_vec()),
            (StoreKey::new("k3".to_string()), b"v3".to_vec()),
        ];

        store.set_many(&items).expect("set many");

        assert_eq!(store.len().expect("check len"), 3);
        assert_eq!(
            store.get(&StoreKey::new("k1".to_string())).expect("get"),
            b"v1"
        );
        assert_eq!(
            store.get(&StoreKey::new("k2".to_string())).expect("get"),
            b"v2"
        );
        assert_eq!(
            store.get(&StoreKey::new("k3".to_string())).expect("get"),
            b"v3"
        );
    }

    #[test]
    fn test_memory_store_clone() {
        let mut store1 = MemoryStore::new();
        store1
            .set(&StoreKey::new("key".to_string()), b"value")
            .expect("set");

        let store2 = store1.clone();
        assert_eq!(
            store2.get(&StoreKey::new("key".to_string())).expect("get"),
            b"value"
        );

        // Both stores share the same data
        store1
            .set(&StoreKey::new("key2".to_string()), b"value2")
            .expect("set");
        assert!(
            store2
                .exists(&StoreKey::new("key2".to_string()))
                .expect("exists")
        );
    }
}
