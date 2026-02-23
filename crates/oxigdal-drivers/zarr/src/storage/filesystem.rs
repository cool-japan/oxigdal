//! Filesystem storage backend for Zarr arrays
//!
//! This module provides a filesystem-based implementation of the Store trait,
//! allowing Zarr arrays to be stored in a local directory structure.

use super::{Store, StoreKey, StoreMetadata};
use crate::error::{Result, StorageError, ZarrError};
use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Filesystem-based store
#[derive(Debug, Clone)]
pub struct FilesystemStore {
    /// Root directory path
    root: PathBuf,
    /// Whether the store is read-only
    readonly: bool,
    /// Whether to create the directory if it doesn't exist
    #[allow(dead_code)] // Reserved for write operations
    create: bool,
}

impl FilesystemStore {
    /// Opens an existing filesystem store
    ///
    /// # Errors
    /// Returns error if the directory doesn't exist
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let root = path.as_ref().to_path_buf();

        if !root.exists() {
            return Err(ZarrError::Storage(StorageError::StoreNotFound {
                path: root.display().to_string(),
            }));
        }

        if !root.is_dir() {
            return Err(ZarrError::Storage(StorageError::InvalidKey {
                key: format!("Path is not a directory: {}", root.display()),
            }));
        }

        Ok(Self {
            root,
            readonly: false,
            create: false,
        })
    }

    /// Creates a new filesystem store
    ///
    /// # Errors
    /// Returns error if the directory cannot be created
    pub fn create<P: AsRef<Path>>(path: P) -> Result<Self> {
        let root = path.as_ref().to_path_buf();

        fs::create_dir_all(&root).map_err(|e| {
            ZarrError::Io(oxigdal_core::error::IoError::Write {
                message: format!("Failed to create directory {}: {}", root.display(), e),
            })
        })?;

        Ok(Self {
            root,
            readonly: false,
            create: true,
        })
    }

    /// Opens a store as read-only
    ///
    /// # Errors
    /// Returns error if the directory doesn't exist
    pub fn open_readonly<P: AsRef<Path>>(path: P) -> Result<Self> {
        let mut store = Self::open(path)?;
        store.readonly = true;
        Ok(store)
    }

    /// Returns the root path
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Converts a store key to a filesystem path
    fn key_to_path(&self, key: &StoreKey) -> PathBuf {
        let mut path = self.root.clone();
        for segment in key.as_str().split('/') {
            path.push(segment);
        }
        path
    }

    /// Converts a filesystem path to a store key
    fn path_to_key(&self, path: &Path) -> Result<StoreKey> {
        let relative = path.strip_prefix(&self.root).map_err(|_| {
            ZarrError::Storage(StorageError::InvalidKey {
                key: format!("Path is not under root: {}", path.display()),
            })
        })?;

        let key_str = relative
            .to_str()
            .ok_or_else(|| {
                ZarrError::Storage(StorageError::InvalidKey {
                    key: format!("Invalid UTF-8 in path: {}", path.display()),
                })
            })?
            .replace(std::path::MAIN_SEPARATOR, "/");

        Ok(StoreKey::new(key_str))
    }

    /// Ensures the parent directory exists for a key
    fn ensure_parent_dir(&self, key: &StoreKey) -> Result<()> {
        if self.readonly {
            return Err(ZarrError::Storage(StorageError::ReadOnly));
        }

        let path = self.key_to_path(key);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                ZarrError::Io(oxigdal_core::error::IoError::Write {
                    message: format!("Failed to create parent directory: {e}"),
                })
            })?;
        }

        Ok(())
    }

    /// Returns metadata about the store
    #[must_use]
    pub fn metadata(&self) -> StoreMetadata {
        StoreMetadata::new("filesystem", true, !self.readonly, true)
    }

    /// Lists all files recursively
    fn list_recursive(&self, dir: &Path, prefix: &Path, results: &mut Vec<StoreKey>) -> Result<()> {
        let entries = fs::read_dir(dir).map_err(|e| {
            ZarrError::Io(oxigdal_core::error::IoError::Read {
                message: format!("Failed to read directory {}: {}", dir.display(), e),
            })
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| {
                ZarrError::Io(oxigdal_core::error::IoError::Read {
                    message: format!("Failed to read directory entry: {e}"),
                })
            })?;

            let path = entry.path();
            let metadata = entry.metadata().map_err(|e| {
                ZarrError::Io(oxigdal_core::error::IoError::Read {
                    message: format!("Failed to read metadata for {}: {}", path.display(), e),
                })
            })?;

            if metadata.is_dir() {
                self.list_recursive(&path, prefix, results)?;
            } else {
                // Check if this file matches the prefix
                if path.starts_with(prefix) {
                    let key = self.path_to_key(&path)?;
                    results.push(key);
                }
            }
        }

        Ok(())
    }
}

impl Store for FilesystemStore {
    fn exists(&self, key: &StoreKey) -> Result<bool> {
        let path = self.key_to_path(key);
        Ok(path.exists() && path.is_file())
    }

    fn get(&self, key: &StoreKey) -> Result<Vec<u8>> {
        let path = self.key_to_path(key);

        if !path.exists() {
            return Err(ZarrError::Storage(StorageError::KeyNotFound {
                key: key.to_string(),
            }));
        }

        let mut file = fs::File::open(&path).map_err(|e| {
            ZarrError::Io(oxigdal_core::error::IoError::Read {
                message: format!("Failed to open {}: {}", path.display(), e),
            })
        })?;

        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).map_err(|e| {
            ZarrError::Io(oxigdal_core::error::IoError::Read {
                message: format!("Failed to read {}: {}", path.display(), e),
            })
        })?;

        Ok(buffer)
    }

    fn set(&mut self, key: &StoreKey, value: &[u8]) -> Result<()> {
        if self.readonly {
            return Err(ZarrError::Storage(StorageError::ReadOnly));
        }

        self.ensure_parent_dir(key)?;

        let path = self.key_to_path(key);
        let mut file = fs::File::create(&path).map_err(|e| {
            ZarrError::Io(oxigdal_core::error::IoError::Write {
                message: format!("Failed to create {}: {}", path.display(), e),
            })
        })?;

        file.write_all(value).map_err(|e| {
            ZarrError::Io(oxigdal_core::error::IoError::Write {
                message: format!("Failed to write {}: {}", path.display(), e),
            })
        })?;

        file.sync_all().map_err(|e| {
            ZarrError::Io(oxigdal_core::error::IoError::Write {
                message: format!("Failed to sync {}: {}", path.display(), e),
            })
        })?;

        Ok(())
    }

    fn delete(&mut self, key: &StoreKey) -> Result<()> {
        if self.readonly {
            return Err(ZarrError::Storage(StorageError::ReadOnly));
        }

        let path = self.key_to_path(key);

        if !path.exists() {
            return Err(ZarrError::Storage(StorageError::KeyNotFound {
                key: key.to_string(),
            }));
        }

        fs::remove_file(&path).map_err(|e| {
            ZarrError::Io(oxigdal_core::error::IoError::Write {
                message: format!("Failed to delete {}: {}", path.display(), e),
            })
        })?;

        Ok(())
    }

    fn list_prefix(&self, prefix: &StoreKey) -> Result<Vec<StoreKey>> {
        let prefix_path = if prefix.as_str().is_empty() {
            self.root.clone()
        } else {
            self.key_to_path(prefix)
        };

        if !prefix_path.exists() {
            return Ok(Vec::new());
        }

        let mut results = Vec::new();
        self.list_recursive(&self.root, &prefix_path, &mut results)?;

        Ok(results)
    }

    fn is_readonly(&self) -> bool {
        self.readonly
    }

    fn flush(&mut self) -> Result<()> {
        // Filesystem writes are synchronous, nothing to flush
        Ok(())
    }

    fn get_many(&self, keys: &[StoreKey]) -> Result<Vec<Option<Vec<u8>>>> {
        keys.iter()
            .map(|key| match self.get(key) {
                Ok(value) => Ok(Some(value)),
                Err(ZarrError::Storage(StorageError::KeyNotFound { .. })) => Ok(None),
                Err(e) => Err(e),
            })
            .collect()
    }

    fn set_many(&mut self, items: &[(StoreKey, Vec<u8>)]) -> Result<()> {
        if self.readonly {
            return Err(ZarrError::Storage(StorageError::ReadOnly));
        }

        // Collect all unique parent directories
        let mut parent_dirs = HashSet::new();
        for (key, _) in items {
            let path = self.key_to_path(key);
            if let Some(parent) = path.parent() {
                parent_dirs.insert(parent.to_path_buf());
            }
        }

        // Create all parent directories
        for parent in parent_dirs {
            fs::create_dir_all(&parent).map_err(|e| {
                ZarrError::Io(oxigdal_core::error::IoError::Write {
                    message: format!("Failed to create directory {}: {}", parent.display(), e),
                })
            })?;
        }

        // Write all files
        for (key, value) in items {
            let path = self.key_to_path(key);
            let mut file = fs::File::create(&path).map_err(|e| {
                ZarrError::Io(oxigdal_core::error::IoError::Write {
                    message: format!("Failed to create {}: {}", path.display(), e),
                })
            })?;

            file.write_all(value).map_err(|e| {
                ZarrError::Io(oxigdal_core::error::IoError::Write {
                    message: format!("Failed to write {}: {}", path.display(), e),
                })
            })?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filesystem_store_create() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let store = FilesystemStore::create(&temp_dir).expect("create store");

        assert!(store.root().exists());
        assert!(!store.is_readonly());

        // temp_dir automatically cleaned up on drop
    }

    #[test]
    fn test_filesystem_store_set_get() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let mut store = FilesystemStore::create(temp_dir.path()).expect("create store");

        let key = StoreKey::new("test/data".to_string());
        let value = b"Hello, Zarr!";

        store.set(&key, value).expect("set value");

        let retrieved = store.get(&key).expect("get value");
        assert_eq!(retrieved, value);

        assert!(store.exists(&key).expect("check exists"));

        // temp_dir automatically cleaned up on drop
    }

    #[test]
    fn test_filesystem_store_delete() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let temp_path = temp_dir.path();
        let mut store = FilesystemStore::create(temp_path).expect("create store");

        let key = StoreKey::new("test/data".to_string());
        store.set(&key, b"data").expect("set value");

        assert!(store.exists(&key).expect("check exists"));

        store.delete(&key).expect("delete value");

        assert!(!store.exists(&key).expect("check not exists"));

        // temp_dir automatically cleaned up on drop
    }

    #[test]
    fn test_filesystem_store_list() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let temp_path = temp_dir.path();
        let mut store = FilesystemStore::create(temp_path).expect("create store");

        let keys = vec![
            StoreKey::new("a/b/c".to_string()),
            StoreKey::new("a/b/d".to_string()),
            StoreKey::new("a/e".to_string()),
            StoreKey::new("f".to_string()),
        ];

        for key in &keys {
            store.set(key, b"data").expect("set value");
        }

        let all_keys = store.list_all().expect("list all");
        assert_eq!(all_keys.len(), 4);

        let prefix_keys = store
            .list_prefix(&StoreKey::new("a/b".to_string()))
            .expect("list prefix");
        assert_eq!(prefix_keys.len(), 2);

        // temp_dir automatically cleaned up on drop
    }

    #[test]
    fn test_filesystem_store_readonly() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let temp_path = temp_dir.path();

        // Create store and write data
        {
            let mut store = FilesystemStore::create(temp_path).expect("create store");
            let key = StoreKey::new("data".to_string());
            store.set(&key, b"test").expect("set value");
        }

        // Open as readonly
        let mut readonly_store = FilesystemStore::open_readonly(&temp_dir).expect("open readonly");
        assert!(readonly_store.is_readonly());

        let key = StoreKey::new("data".to_string());

        // Read should work
        let value = readonly_store.get(&key).expect("get value");
        assert_eq!(value, b"test");

        // Write should fail
        assert!(readonly_store.set(&key, b"new").is_err());
        assert!(readonly_store.delete(&key).is_err());

        // temp_dir automatically cleaned up on drop
    }
}
