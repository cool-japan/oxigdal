//! Storage backends for Zarr arrays
//!
//! This module provides abstract storage traits and concrete implementations
//! for various storage backends including filesystem, S3, HTTP, and in-memory.

#[cfg(feature = "filesystem")]
pub mod filesystem;

#[cfg(feature = "s3")]
pub mod s3;

#[cfg(feature = "http")]
pub mod http;

#[cfg(feature = "memory")]
pub mod memory;

#[cfg(feature = "cache")]
pub mod cache;

pub mod cloud;

use crate::error::{Result, StorageError};
use serde::{Deserialize, Serialize};

/// Storage key - identifies a value in the store
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StoreKey(String);

impl StoreKey {
    /// Creates a new store key
    #[must_use]
    pub fn new(key: String) -> Self {
        Self(key)
    }

    /// Returns the key as a string slice
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Joins path segments into a key
    #[must_use]
    pub fn join(segments: &[&str]) -> Self {
        Self(segments.join("/"))
    }

    /// Returns the parent key
    #[must_use]
    pub fn parent(&self) -> Option<Self> {
        self.0
            .rsplit_once('/')
            .map(|(parent, _)| Self(parent.to_string()))
    }

    /// Returns the file name part of the key
    #[must_use]
    pub fn file_name(&self) -> Option<&str> {
        self.0
            .rsplit_once('/')
            .map(|(_, name)| name)
            .or(Some(self.0.as_str()))
    }

    /// Appends a segment to the key
    #[must_use]
    pub fn append(&self, segment: &str) -> Self {
        if self.0.is_empty() {
            Self(segment.to_string())
        } else {
            Self(format!("{}/{}", self.0, segment))
        }
    }
}

impl From<String> for StoreKey {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for StoreKey {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl AsRef<str> for StoreKey {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for StoreKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Trait for synchronous key-value storage
pub trait Store: Send + Sync {
    /// Checks if a key exists in the store
    fn exists(&self, key: &StoreKey) -> Result<bool>;

    /// Gets a value from the store
    ///
    /// # Errors
    /// Returns `StorageError::KeyNotFound` if the key doesn't exist
    fn get(&self, key: &StoreKey) -> Result<Vec<u8>>;

    /// Sets a value in the store
    ///
    /// # Errors
    /// Returns error if the store is read-only or the write fails
    fn set(&mut self, key: &StoreKey, value: &[u8]) -> Result<()>;

    /// Deletes a key from the store
    ///
    /// # Errors
    /// Returns error if the store is read-only or the delete fails
    fn delete(&mut self, key: &StoreKey) -> Result<()>;

    /// Lists all keys with a given prefix
    fn list_prefix(&self, prefix: &StoreKey) -> Result<Vec<StoreKey>>;

    /// Lists all keys in the store
    fn list_all(&self) -> Result<Vec<StoreKey>> {
        self.list_prefix(&StoreKey::new(String::new()))
    }

    /// Gets multiple values efficiently
    fn get_many(&self, keys: &[StoreKey]) -> Result<Vec<Option<Vec<u8>>>> {
        keys.iter()
            .map(|key| match self.get(key) {
                Ok(value) => Ok(Some(value)),
                Err(crate::error::ZarrError::Storage(StorageError::KeyNotFound { .. })) => Ok(None),
                Err(e) => Err(e),
            })
            .collect()
    }

    /// Sets multiple values efficiently
    fn set_many(&mut self, items: &[(StoreKey, Vec<u8>)]) -> Result<()> {
        for (key, value) in items {
            self.set(key, value)?;
        }
        Ok(())
    }

    /// Checks if the store is read-only
    fn is_readonly(&self) -> bool {
        false
    }

    /// Checks if the store is write-only
    fn is_writeonly(&self) -> bool {
        false
    }

    /// Flushes any pending writes
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Trait for async key-value storage
#[cfg(feature = "async")]
#[async_trait::async_trait]
pub trait AsyncStore: Send + Sync {
    /// Checks if a key exists in the store
    async fn exists(&self, key: &StoreKey) -> Result<bool>;

    /// Gets a value from the store
    async fn get(&self, key: &StoreKey) -> Result<Vec<u8>>;

    /// Sets a value in the store
    async fn set(&mut self, key: &StoreKey, value: &[u8]) -> Result<()>;

    /// Deletes a key from the store
    async fn delete(&mut self, key: &StoreKey) -> Result<()>;

    /// Lists all keys with a given prefix
    async fn list_prefix(&self, prefix: &StoreKey) -> Result<Vec<StoreKey>>;

    /// Lists all keys in the store
    async fn list_all(&self) -> Result<Vec<StoreKey>> {
        self.list_prefix(&StoreKey::new(String::new())).await
    }

    /// Gets multiple values efficiently (concurrent)
    async fn get_many(&self, keys: &[StoreKey]) -> Result<Vec<Option<Vec<u8>>>> {
        let mut results = Vec::with_capacity(keys.len());
        for key in keys {
            match self.get(key).await {
                Ok(value) => results.push(Some(value)),
                Err(crate::error::ZarrError::Storage(StorageError::KeyNotFound { .. })) => {
                    results.push(None);
                }
                Err(e) => return Err(e),
            }
        }
        Ok(results)
    }

    /// Sets multiple values efficiently
    async fn set_many(&mut self, items: &[(StoreKey, Vec<u8>)]) -> Result<()> {
        for (key, value) in items {
            self.set(key, value).await?;
        }
        Ok(())
    }

    /// Checks if the store is read-only
    fn is_readonly(&self) -> bool {
        false
    }

    /// Flushes any pending writes
    async fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Store metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreMetadata {
    /// Store type identifier
    pub store_type: String,
    /// Whether the store is readable
    pub readable: bool,
    /// Whether the store is writable
    pub writable: bool,
    /// Whether the store supports listing
    pub listable: bool,
    /// Additional metadata
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl StoreMetadata {
    /// Creates new store metadata
    #[must_use]
    pub fn new(
        store_type: impl Into<String>,
        readable: bool,
        writable: bool,
        listable: bool,
    ) -> Self {
        Self {
            store_type: store_type.into(),
            readable,
            writable,
            listable,
            extra: serde_json::Map::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_key() {
        let key = StoreKey::new("path/to/data".to_string());
        assert_eq!(key.as_str(), "path/to/data");

        let joined = StoreKey::join(&["path", "to", "data"]);
        assert_eq!(joined.as_str(), "path/to/data");
    }

    #[test]
    fn test_store_key_parent() {
        let key = StoreKey::new("path/to/data".to_string());
        let parent = key.parent().expect("has parent");
        assert_eq!(parent.as_str(), "path/to");

        let root = StoreKey::new("data".to_string());
        assert!(root.parent().is_none());
    }

    #[test]
    fn test_store_key_file_name() {
        let key = StoreKey::new("path/to/data".to_string());
        assert_eq!(key.file_name(), Some("data"));

        let root = StoreKey::new("data".to_string());
        assert_eq!(root.file_name(), Some("data"));
    }

    #[test]
    fn test_store_key_append() {
        let key = StoreKey::new("path".to_string());
        let appended = key.append("to");
        assert_eq!(appended.as_str(), "path/to");

        let empty = StoreKey::new(String::new());
        let appended2 = empty.append("data");
        assert_eq!(appended2.as_str(), "data");
    }
}
