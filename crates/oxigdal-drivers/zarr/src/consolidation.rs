//! Consolidated metadata support for Zarr arrays
//!
//! This module provides support for consolidated metadata (.zmetadata files),
//! which combines all array and group metadata into a single file to reduce
//! the number of storage operations needed to read a Zarr hierarchy.
//!
//! Consolidation is especially beneficial for cloud storage where each file
//! access has significant latency overhead.

use crate::error::{MetadataError, Result, StorageError, ZarrError};
use crate::storage::{Store, StoreKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Consolidated metadata for a Zarr store
///
/// The .zmetadata file contains all metadata for arrays and groups in a
/// Zarr hierarchy, allowing efficient access without multiple storage operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidatedMetadata {
    /// Zarr format version
    pub zarr_format: u8,

    /// Metadata version
    pub metadata_version: String,

    /// Metadata for all arrays and groups
    pub metadata: HashMap<String, serde_json::Value>,

    /// Additional attributes
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

impl ConsolidatedMetadata {
    /// Creates a new consolidated metadata structure
    #[must_use]
    pub fn new(zarr_format: u8) -> Self {
        Self {
            zarr_format,
            metadata_version: "1".to_string(),
            metadata: HashMap::new(),
            extra: HashMap::new(),
        }
    }

    /// Adds metadata for a key
    pub fn add_metadata(&mut self, key: impl Into<String>, metadata: serde_json::Value) {
        self.metadata.insert(key.into(), metadata);
    }

    /// Gets metadata for a key
    #[must_use]
    pub fn get_metadata(&self, key: &str) -> Option<&serde_json::Value> {
        self.metadata.get(key)
    }

    /// Checks if metadata exists for a key
    #[must_use]
    pub fn has_metadata(&self, key: &str) -> bool {
        self.metadata.contains_key(key)
    }

    /// Returns all metadata keys
    #[must_use]
    pub fn keys(&self) -> Vec<&str> {
        self.metadata.keys().map(String::as_str).collect()
    }

    /// Serializes the consolidated metadata to JSON
    ///
    /// # Errors
    /// Returns error if serialization fails
    pub fn to_json(&self) -> Result<Vec<u8>> {
        serde_json::to_vec_pretty(self).map_err(|e| {
            ZarrError::Metadata(MetadataError::InvalidJson {
                message: format!("Failed to serialize consolidated metadata: {e}"),
            })
        })
    }

    /// Deserializes consolidated metadata from JSON
    ///
    /// # Errors
    /// Returns error if deserialization fails
    pub fn from_json(data: &[u8]) -> Result<Self> {
        serde_json::from_slice(data).map_err(|e| {
            ZarrError::Metadata(MetadataError::InvalidJson {
                message: format!("Failed to parse consolidated metadata: {e}"),
            })
        })
    }

    /// Loads consolidated metadata from a store
    ///
    /// # Errors
    /// Returns error if the .zmetadata file doesn't exist or is invalid
    pub fn load_from_store(store: &impl Store) -> Result<Self> {
        let key = StoreKey::new(".zmetadata".to_string());
        let data = store.get(&key).map_err(|e| match e {
            ZarrError::Storage(StorageError::KeyNotFound { .. }) => {
                ZarrError::Metadata(MetadataError::MissingField {
                    field: ".zmetadata",
                })
            }
            other => other,
        })?;

        Self::from_json(&data)
    }

    /// Saves consolidated metadata to a store
    ///
    /// # Errors
    /// Returns error if serialization or writing fails
    pub fn save_to_store(&self, store: &mut impl Store) -> Result<()> {
        let key = StoreKey::new(".zmetadata".to_string());
        let data = self.to_json()?;
        store.set(&key, &data)
    }
}

/// Consolidates metadata from a Zarr store
///
/// This function walks through a Zarr store and collects all metadata files
/// (.zarray, .zgroup, .zattrs) into a consolidated metadata structure.
///
/// # Arguments
/// * `store` - The store to consolidate
/// * `zarr_format` - The Zarr format version (2 or 3)
///
/// # Errors
/// Returns error if reading metadata fails
pub fn consolidate_metadata(store: &impl Store, zarr_format: u8) -> Result<ConsolidatedMetadata> {
    let mut consolidated = ConsolidatedMetadata::new(zarr_format);

    // List all keys in the store
    let all_keys = store.list_all()?;

    // Process metadata files
    for key in &all_keys {
        let key_str = key.as_str();

        // Check if this is a metadata file
        if key_str.ends_with(".zarray")
            || key_str.ends_with(".zgroup")
            || key_str.ends_with(".zattrs")
        {
            // Read the metadata
            let data = store.get(key)?;

            // Parse as JSON
            let json: serde_json::Value = serde_json::from_slice(&data).map_err(|e| {
                ZarrError::Metadata(MetadataError::InvalidJson {
                    message: format!("Failed to parse metadata for '{key_str}': {e}"),
                })
            })?;

            // Add to consolidated metadata
            consolidated.add_metadata(key_str, json);
        }
    }

    Ok(consolidated)
}

/// Store wrapper that uses consolidated metadata when available
///
/// This wrapper first checks consolidated metadata for metadata files
/// before falling back to the underlying store.
pub struct ConsolidatedStore<S: Store> {
    /// Underlying store
    store: S,
    /// Consolidated metadata
    consolidated: Option<ConsolidatedMetadata>,
}

impl<S: Store> ConsolidatedStore<S> {
    /// Creates a new consolidated store wrapper
    #[must_use]
    pub fn new(store: S) -> Self {
        Self {
            store,
            consolidated: None,
        }
    }

    /// Attempts to load consolidated metadata
    ///
    /// # Errors
    /// Returns error if loading fails (but silently continues if .zmetadata doesn't exist)
    pub fn load_consolidated(&mut self) -> Result<bool> {
        match ConsolidatedMetadata::load_from_store(&self.store) {
            Ok(consolidated) => {
                self.consolidated = Some(consolidated);
                Ok(true)
            }
            Err(ZarrError::Metadata(MetadataError::MissingField { .. })) => {
                // .zmetadata doesn't exist, that's okay
                Ok(false)
            }
            Err(e) => Err(e),
        }
    }

    /// Returns true if consolidated metadata is loaded
    #[must_use]
    pub fn is_consolidated(&self) -> bool {
        self.consolidated.is_some()
    }

    /// Gets the underlying store
    #[must_use]
    pub fn inner(&self) -> &S {
        &self.store
    }

    /// Gets the underlying store mutably
    pub fn inner_mut(&mut self) -> &mut S {
        &mut self.store
    }

    /// Consumes this wrapper and returns the underlying store
    #[must_use]
    pub fn into_inner(self) -> S {
        self.store
    }
}

impl<S: Store> Store for ConsolidatedStore<S> {
    fn get(&self, key: &StoreKey) -> Result<Vec<u8>> {
        // Check if this is a metadata file and we have it consolidated
        if let Some(ref consolidated) = self.consolidated {
            let key_str = key.as_str();
            if key_str.ends_with(".zarray")
                || key_str.ends_with(".zgroup")
                || key_str.ends_with(".zattrs")
            {
                if let Some(metadata) = consolidated.get_metadata(key_str) {
                    // Return the consolidated metadata as JSON
                    return serde_json::to_vec(metadata).map_err(|e| {
                        ZarrError::Metadata(MetadataError::InvalidJson {
                            message: format!("Failed to serialize metadata: {e}"),
                        })
                    });
                }
            }
        }

        // Fall back to the underlying store
        self.store.get(key)
    }

    fn set(&mut self, key: &StoreKey, value: &[u8]) -> Result<()> {
        self.store.set(key, value)
    }

    fn delete(&mut self, key: &StoreKey) -> Result<()> {
        self.store.delete(key)
    }

    fn exists(&self, key: &StoreKey) -> Result<bool> {
        // Check consolidated metadata first
        if let Some(ref consolidated) = self.consolidated {
            let key_str = key.as_str();
            if (key_str.ends_with(".zarray")
                || key_str.ends_with(".zgroup")
                || key_str.ends_with(".zattrs"))
                && consolidated.has_metadata(key_str)
            {
                return Ok(true);
            }
        }

        self.store.exists(key)
    }

    fn list_prefix(&self, prefix: &StoreKey) -> Result<Vec<StoreKey>> {
        self.store.list_prefix(prefix)
    }

    fn is_readonly(&self) -> bool {
        self.store.is_readonly()
    }

    fn flush(&mut self) -> Result<()> {
        self.store.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::memory::MemoryStore;

    #[test]
    fn test_consolidated_metadata_new() {
        let meta = ConsolidatedMetadata::new(2);
        assert_eq!(meta.zarr_format, 2);
        assert_eq!(meta.metadata_version, "1");
        assert!(meta.metadata.is_empty());
    }

    #[test]
    fn test_consolidated_metadata_add_get() {
        let mut meta = ConsolidatedMetadata::new(2);

        let array_meta = serde_json::json!({
            "chunks": [100, 100],
            "compressor": null,
            "dtype": "<f8",
            "fill_value": 0.0,
            "order": "C",
            "shape": [1000, 1000],
            "zarr_format": 2
        });

        meta.add_metadata("array/.zarray", array_meta.clone());

        let retrieved = meta.get_metadata("array/.zarray").expect("Should exist");
        assert_eq!(retrieved, &array_meta);

        assert!(meta.has_metadata("array/.zarray"));
        assert!(!meta.has_metadata("other/.zarray"));
    }

    #[test]
    fn test_consolidated_metadata_keys() {
        let mut meta = ConsolidatedMetadata::new(2);

        meta.add_metadata("array1/.zarray", serde_json::json!({}));
        meta.add_metadata("array2/.zarray", serde_json::json!({}));
        meta.add_metadata(".zgroup", serde_json::json!({}));

        let mut keys = meta.keys();
        keys.sort();

        assert_eq!(keys.len(), 3);
        assert!(keys.contains(&".zgroup"));
        assert!(keys.contains(&"array1/.zarray"));
        assert!(keys.contains(&"array2/.zarray"));
    }

    #[test]
    fn test_consolidated_metadata_roundtrip() {
        let mut meta = ConsolidatedMetadata::new(2);

        meta.add_metadata(
            "array/.zarray",
            serde_json::json!({
                "chunks": [10, 10],
                "dtype": "<f4",
                "shape": [100, 100],
                "zarr_format": 2
            }),
        );

        let json = meta.to_json().expect("Should serialize");
        let parsed = ConsolidatedMetadata::from_json(&json).expect("Should deserialize");

        assert_eq!(parsed.zarr_format, 2);
        assert_eq!(parsed.metadata.len(), 1);
        assert!(parsed.has_metadata("array/.zarray"));
    }

    #[test]
    fn test_consolidate_metadata() {
        let mut store = MemoryStore::new();

        // Add some metadata files
        store
            .set(
                &StoreKey::new(".zgroup".to_string()),
                b"{\"zarr_format\": 2}",
            )
            .expect("set");

        store
            .set(
                &StoreKey::new("array/.zarray".to_string()),
                br#"{"chunks": [10], "dtype": "<f4", "shape": [100], "zarr_format": 2}"#,
            )
            .expect("set");

        store
            .set(
                &StoreKey::new("array/.zattrs".to_string()),
                b"{\"description\": \"test array\"}",
            )
            .expect("set");

        // Add a data file (should not be included)
        store
            .set(&StoreKey::new("array/0".to_string()), b"chunk data")
            .expect("set");

        let consolidated = consolidate_metadata(&store, 2).expect("Should consolidate");

        assert_eq!(consolidated.zarr_format, 2);
        assert_eq!(consolidated.metadata.len(), 3);
        assert!(consolidated.has_metadata(".zgroup"));
        assert!(consolidated.has_metadata("array/.zarray"));
        assert!(consolidated.has_metadata("array/.zattrs"));
        assert!(!consolidated.has_metadata("array/0"));
    }

    #[test]
    fn test_consolidated_store() {
        let mut store = MemoryStore::new();

        // Add metadata
        store
            .set(
                &StoreKey::new("array/.zarray".to_string()),
                br#"{"chunks": [10], "dtype": "<f4", "shape": [100], "zarr_format": 2}"#,
            )
            .expect("set");

        // Create consolidated metadata
        let mut consolidated = ConsolidatedMetadata::new(2);
        consolidated.add_metadata(
            "array/.zarray",
            serde_json::json!({
                "chunks": [10],
                "dtype": "<f4",
                "shape": [100],
                "zarr_format": 2
            }),
        );

        consolidated.save_to_store(&mut store).expect("save");

        // Create consolidated store wrapper
        let mut cs = ConsolidatedStore::new(store);
        let loaded = cs.load_consolidated().expect("load");
        assert!(loaded);
        assert!(cs.is_consolidated());

        // Should read from consolidated metadata
        let data = cs
            .get(&StoreKey::new("array/.zarray".to_string()))
            .expect("get");
        let json: serde_json::Value = serde_json::from_slice(&data).expect("parse");

        assert_eq!(json["chunks"], serde_json::json!([10]));
        assert_eq!(json["dtype"], "<f4");
    }

    #[test]
    fn test_consolidated_store_fallback() {
        let mut store = MemoryStore::new();

        // Add a data file (not metadata)
        store
            .set(&StoreKey::new("array/0".to_string()), b"chunk data")
            .expect("set");

        let mut cs = ConsolidatedStore::new(store);

        // No consolidated metadata available
        let loaded = cs.load_consolidated().expect("load");
        assert!(!loaded);
        assert!(!cs.is_consolidated());

        // Should still be able to read from underlying store
        let data = cs.get(&StoreKey::new("array/0".to_string())).expect("get");
        assert_eq!(data, b"chunk data");
    }
}
