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

    /// Saves consolidated metadata to a store (Zarr v2 `.zmetadata` file).
    ///
    /// # Errors
    /// Returns error if serialization or writing fails
    pub fn save_to_store(&self, store: &mut impl Store) -> Result<()> {
        let key = StoreKey::new(".zmetadata".to_string());
        let data = self.to_json()?;
        store.set(&key, &data)
    }

    /// Returns the root `zarr.json` key for a (possibly empty) root path.
    fn v3_root_key(root_path: &str) -> StoreKey {
        if root_path.is_empty() {
            StoreKey::new("zarr.json".to_string())
        } else {
            StoreKey::new(format!("{}/zarr.json", root_path.trim_end_matches('/')))
        }
    }

    /// Saves consolidated metadata using the Zarr **v3** extension: the
    /// `consolidated_metadata` object is embedded inside the root group's
    /// `zarr.json` (there is no separate `.zmetadata` file in v3).
    ///
    /// An existing root `zarr.json` is preserved and augmented; if none
    /// exists a minimal group `zarr.json` is created.
    ///
    /// # Errors
    /// Returns error if reading/serializing/writing the root `zarr.json` fails.
    pub fn save_to_store_v3(&self, store: &mut impl Store, root_path: &str) -> Result<()> {
        let root_key = Self::v3_root_key(root_path);

        let mut root: serde_json::Value = match store.get(&root_key) {
            Ok(data) => serde_json::from_slice(&data).map_err(|e| {
                ZarrError::Metadata(MetadataError::InvalidJson {
                    message: format!("Failed to parse root zarr.json: {e}"),
                })
            })?,
            Err(ZarrError::Storage(StorageError::KeyNotFound { .. })) => {
                serde_json::json!({ "zarr_format": 3, "node_type": "group" })
            }
            Err(e) => return Err(e),
        };

        let obj = root.as_object_mut().ok_or_else(|| {
            ZarrError::Metadata(MetadataError::InvalidJson {
                message: "root zarr.json is not a JSON object".to_string(),
            })
        })?;

        let metadata_value = serde_json::to_value(&self.metadata).map_err(|e| {
            ZarrError::Metadata(MetadataError::InvalidJson {
                message: format!("Failed to serialize consolidated metadata: {e}"),
            })
        })?;

        obj.insert(
            "consolidated_metadata".to_string(),
            serde_json::json!({
                "kind": "inline",
                "must_understand": false,
                "metadata": metadata_value,
            }),
        );

        let data = serde_json::to_vec_pretty(&root).map_err(|e| {
            ZarrError::Metadata(MetadataError::InvalidJson {
                message: format!("Failed to serialize root zarr.json: {e}"),
            })
        })?;
        store.set(&root_key, &data)
    }

    /// Loads consolidated metadata from the Zarr **v3** extension embedded in
    /// the root group's `zarr.json`.
    ///
    /// # Errors
    /// Returns [`MetadataError::MissingField`] if the root `zarr.json` (or its
    /// `consolidated_metadata` object) is absent.
    pub fn load_from_store_v3(store: &impl Store, root_path: &str) -> Result<Self> {
        let root_key = Self::v3_root_key(root_path);
        let data = store.get(&root_key).map_err(|e| match e {
            ZarrError::Storage(StorageError::KeyNotFound { .. }) => {
                ZarrError::Metadata(MetadataError::MissingField { field: "zarr.json" })
            }
            other => other,
        })?;

        let root: serde_json::Value = serde_json::from_slice(&data).map_err(|e| {
            ZarrError::Metadata(MetadataError::InvalidJson {
                message: format!("Failed to parse root zarr.json: {e}"),
            })
        })?;

        let cm = root.get("consolidated_metadata").ok_or({
            ZarrError::Metadata(MetadataError::MissingField {
                field: "consolidated_metadata",
            })
        })?;

        let metadata_map = cm.get("metadata").and_then(|m| m.as_object()).ok_or({
            ZarrError::Metadata(MetadataError::MissingField {
                field: "consolidated_metadata.metadata",
            })
        })?;

        let mut metadata = HashMap::new();
        for (k, v) in metadata_map {
            metadata.insert(k.clone(), v.clone());
        }

        Ok(Self {
            zarr_format: 3,
            metadata_version: "1".to_string(),
            metadata,
            extra: HashMap::new(),
        })
    }
}

/// Returns true if `key` names a metadata file for the given Zarr format.
///
/// * v2 keeps per-node metadata in `.zarray`/`.zgroup`/`.zattrs` files.
/// * v3 keeps all node metadata in `zarr.json` files.
fn is_metadata_key(key: &str, zarr_format: u8) -> bool {
    match zarr_format {
        3 => key == "zarr.json" || key.ends_with("/zarr.json"),
        _ => key.ends_with(".zarray") || key.ends_with(".zgroup") || key.ends_with(".zattrs"),
    }
}

/// Consolidates metadata from a Zarr store.
///
/// Walks the store and collects every metadata file for the requested format
/// (v2: `.zarray`/`.zgroup`/`.zattrs`; v3: `zarr.json`) into a consolidated
/// metadata structure.
///
/// # Arguments
/// * `store` - The store to consolidate
/// * `zarr_format` - The Zarr format version (2 or 3)
///
/// # Errors
/// Returns [`MetadataError::MissingField`] when the store contains no metadata
/// files for the requested format (previously this silently returned an empty
/// structure, which masked a wrong-format or empty store), and propagates any
/// storage/parse error.
pub fn consolidate_metadata(store: &impl Store, zarr_format: u8) -> Result<ConsolidatedMetadata> {
    let mut consolidated = ConsolidatedMetadata::new(zarr_format);

    // List all keys in the store
    let all_keys = store.list_all()?;

    // Process metadata files for the requested format.
    let mut found = 0usize;
    for key in &all_keys {
        let key_str = key.as_str();

        if is_metadata_key(key_str, zarr_format) {
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
            found += 1;
        }
    }

    if found == 0 {
        return Err(ZarrError::Metadata(MetadataError::MissingField {
            field: if zarr_format == 3 {
                "zarr.json"
            } else {
                ".zarray/.zgroup/.zattrs"
            },
        }));
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
            if (key_str.ends_with(".zarray")
                || key_str.ends_with(".zgroup")
                || key_str.ends_with(".zattrs"))
                && let Some(metadata) = consolidated.get_metadata(key_str)
            {
                // Return the consolidated metadata as JSON
                return serde_json::to_vec(metadata).map_err(|e| {
                    ZarrError::Metadata(MetadataError::InvalidJson {
                        message: format!("Failed to serialize metadata: {e}"),
                    })
                });
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
    fn test_consolidate_metadata_v3() {
        let mut store = MemoryStore::new();
        store
            .set(
                &StoreKey::new("zarr.json".to_string()),
                br#"{"zarr_format":3,"node_type":"group"}"#,
            )
            .expect("set root");
        store
            .set(
                &StoreKey::new("arr/zarr.json".to_string()),
                br#"{"zarr_format":3,"node_type":"array","shape":[4],"data_type":"float32"}"#,
            )
            .expect("set array");

        let consolidated = consolidate_metadata(&store, 3).expect("consolidate v3");
        assert_eq!(consolidated.metadata.len(), 2);
        assert!(consolidated.has_metadata("zarr.json"));
        assert!(consolidated.has_metadata("arr/zarr.json"));
    }

    #[test]
    fn test_consolidate_metadata_errors_when_empty() {
        // A v3 store consolidated with the wrong (v2) format finds zero
        // metadata files and must error, not silently return an empty struct.
        let mut store = MemoryStore::new();
        store
            .set(
                &StoreKey::new("arr/zarr.json".to_string()),
                br#"{"zarr_format":3,"node_type":"array"}"#,
            )
            .expect("set");

        let result = consolidate_metadata(&store, 2);
        assert!(
            matches!(
                result,
                Err(ZarrError::Metadata(MetadataError::MissingField { .. }))
            ),
            "consolidating with no matching metadata must error, got {result:?}"
        );
    }

    #[test]
    fn test_consolidated_metadata_v3_roundtrip_embedded() {
        let mut store = MemoryStore::new();
        store
            .set(
                &StoreKey::new("zarr.json".to_string()),
                br#"{"zarr_format":3,"node_type":"group","attributes":{"k":"v"}}"#,
            )
            .expect("set root");

        let mut cm = ConsolidatedMetadata::new(3);
        cm.add_metadata("arr/zarr.json", serde_json::json!({"shape":[8]}));
        cm.save_to_store_v3(&mut store, "").expect("save v3");

        // The root zarr.json is preserved and augmented, not overwritten.
        let raw = store
            .get(&StoreKey::new("zarr.json".to_string()))
            .expect("get root");
        let root: serde_json::Value = serde_json::from_slice(&raw).expect("parse");
        assert_eq!(root["attributes"]["k"], "v");
        assert!(root.get("consolidated_metadata").is_some());

        let loaded = ConsolidatedMetadata::load_from_store_v3(&store, "").expect("load v3");
        assert_eq!(loaded.zarr_format, 3);
        assert_eq!(
            loaded.get_metadata("arr/zarr.json"),
            Some(&serde_json::json!({"shape":[8]}))
        );
    }

    #[test]
    fn test_load_from_store_v3_missing_errors() {
        let store = MemoryStore::new();
        let result = ConsolidatedMetadata::load_from_store_v3(&store, "");
        assert!(matches!(
            result,
            Err(ZarrError::Metadata(MetadataError::MissingField { .. }))
        ));
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
