//! State backend implementations.

use crate::error::{Result, StreamingError};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Trait for state backends.
#[async_trait]
pub trait StateBackend: Send + Sync {
    /// Get a value from the state.
    async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>>;

    /// Put a value into the state.
    async fn put(&self, key: &[u8], value: &[u8]) -> Result<()>;

    /// Delete a value from the state.
    async fn delete(&self, key: &[u8]) -> Result<()>;

    /// Check if a key exists.
    async fn contains(&self, key: &[u8]) -> Result<bool>;

    /// Clear all state.
    async fn clear(&self) -> Result<()>;

    /// Create a snapshot of the state.
    async fn snapshot(&self) -> Result<Vec<u8>>;

    /// Restore state from a snapshot.
    async fn restore(&self, snapshot: &[u8]) -> Result<()>;

    /// Get all keys.
    async fn keys(&self) -> Result<Vec<Vec<u8>>>;

    /// Get the backend name.
    fn name(&self) -> &str;
}

/// In-memory state backend.
pub struct MemoryStateBackend {
    state: Arc<RwLock<HashMap<Vec<u8>, Vec<u8>>>>,
}

impl MemoryStateBackend {
    /// Create a new memory state backend.
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for MemoryStateBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StateBackend for MemoryStateBackend {
    async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        Ok(self.state.read().await.get(key).cloned())
    }

    async fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
        self.state
            .write()
            .await
            .insert(key.to_vec(), value.to_vec());
        Ok(())
    }

    async fn delete(&self, key: &[u8]) -> Result<()> {
        self.state.write().await.remove(key);
        Ok(())
    }

    async fn contains(&self, key: &[u8]) -> Result<bool> {
        Ok(self.state.read().await.contains_key(key))
    }

    async fn clear(&self) -> Result<()> {
        self.state.write().await.clear();
        Ok(())
    }

    async fn snapshot(&self) -> Result<Vec<u8>> {
        let state = self.state.read().await;
        // Use oxicode for binary serialization since JSON requires string keys
        oxicode::encode_to_vec(&*state)
            .map_err(|e| StreamingError::SerializationError(e.to_string()))
    }

    async fn restore(&self, snapshot: &[u8]) -> Result<()> {
        let (restored, _): (HashMap<Vec<u8>, Vec<u8>>, _) = oxicode::decode_from_slice(snapshot)
            .map_err(|e| StreamingError::SerializationError(e.to_string()))?;
        *self.state.write().await = restored;
        Ok(())
    }

    async fn keys(&self) -> Result<Vec<Vec<u8>>> {
        Ok(self.state.read().await.keys().cloned().collect())
    }

    fn name(&self) -> &str {
        "MemoryStateBackend"
    }
}

/// RocksDB state backend.
#[cfg(feature = "rocksdb-backend")]
pub struct RocksDBStateBackend {
    db: Arc<rocksdb::DB>,
    path: PathBuf,
}

#[cfg(feature = "rocksdb-backend")]
impl RocksDBStateBackend {
    /// Create a new RocksDB state backend.
    pub fn new(path: PathBuf) -> Result<Self> {
        let mut opts = rocksdb::Options::default();
        opts.create_if_missing(true);

        let db = rocksdb::DB::open(&opts, &path)?;

        Ok(Self {
            db: Arc::new(db),
            path,
        })
    }

    /// Get the database path.
    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}

#[cfg(feature = "rocksdb-backend")]
#[async_trait]
impl StateBackend for RocksDBStateBackend {
    async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        Ok(self.db.get(key)?)
    }

    async fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
        self.db.put(key, value)?;
        Ok(())
    }

    async fn delete(&self, key: &[u8]) -> Result<()> {
        self.db.delete(key)?;
        Ok(())
    }

    async fn contains(&self, key: &[u8]) -> Result<bool> {
        Ok(self.db.get(key)?.is_some())
    }

    async fn clear(&self) -> Result<()> {
        let keys: Vec<Vec<u8>> = self
            .db
            .iterator(rocksdb::IteratorMode::Start)
            .map(|item| {
                let (key, _) = item.map_err(|e| StreamingError::StateError(e.to_string()))?;
                Ok(key.to_vec())
            })
            .collect::<Result<Vec<_>>>()?;

        for key in keys {
            self.db.delete(&key)?;
        }

        Ok(())
    }

    async fn snapshot(&self) -> Result<Vec<u8>> {
        let snapshot = self.db.snapshot();
        let mut data = Vec::new();

        for item in snapshot.iterator(rocksdb::IteratorMode::Start) {
            let (key, value) = item?;
            let entry = (key.to_vec(), value.to_vec());
            // Use oxicode for binary serialization
            let serialized = oxicode::encode_to_vec(&entry)
                .map_err(|e| StreamingError::SerializationError(e.to_string()))?;
            data.extend_from_slice(&(serialized.len() as u32).to_le_bytes());
            data.extend_from_slice(&serialized);
        }

        Ok(data)
    }

    async fn restore(&self, snapshot: &[u8]) -> Result<()> {
        self.clear().await?;

        let mut offset = 0;
        while offset < snapshot.len() {
            if offset + 4 > snapshot.len() {
                break;
            }

            let len = u32::from_le_bytes([
                snapshot[offset],
                snapshot[offset + 1],
                snapshot[offset + 2],
                snapshot[offset + 3],
            ]) as usize;

            offset += 4;

            if offset + len > snapshot.len() {
                break;
            }

            let entry_data = &snapshot[offset..offset + len];
            let ((key, value), _): ((Vec<u8>, Vec<u8>), _) = oxicode::decode_from_slice(entry_data)
                .map_err(|e| StreamingError::SerializationError(e.to_string()))?;
            self.db.put(&key, &value)?;

            offset += len;
        }

        Ok(())
    }

    async fn keys(&self) -> Result<Vec<Vec<u8>>> {
        let keys: Vec<Vec<u8>> = self
            .db
            .iterator(rocksdb::IteratorMode::Start)
            .map(|item| {
                let (key, _) = item?;
                Ok(key.to_vec())
            })
            .collect::<std::result::Result<Vec<_>, rocksdb::Error>>()?;

        Ok(keys)
    }

    fn name(&self) -> &str {
        "RocksDBStateBackend"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_backend() -> Result<()> {
        let backend = MemoryStateBackend::new();

        backend.put(b"key1", b"value1").await?;
        let value = backend.get(b"key1").await?;
        assert_eq!(value, Some(b"value1".to_vec()));

        assert!(backend.contains(b"key1").await?);
        assert!(!backend.contains(b"key2").await?);

        backend.delete(b"key1").await?;
        assert!(!backend.contains(b"key1").await?);

        Ok(())
    }

    #[tokio::test]
    async fn test_memory_backend_snapshot() -> Result<()> {
        let backend = MemoryStateBackend::new();

        backend.put(b"key1", b"value1").await?;
        backend.put(b"key2", b"value2").await?;

        let snapshot = backend.snapshot().await?;

        let backend2 = MemoryStateBackend::new();
        backend2.restore(&snapshot).await?;

        assert_eq!(backend2.get(b"key1").await?, Some(b"value1".to_vec()));
        assert_eq!(backend2.get(b"key2").await?, Some(b"value2".to_vec()));

        Ok(())
    }

    #[cfg(feature = "rocksdb-backend")]
    #[tokio::test]
    async fn test_rocksdb_backend() -> Result<()> {
        let temp_dir = tempfile::tempdir()
            .map_err(|e| StreamingError::StateError(format!("Failed to create temp dir: {}", e)))?;
        let backend = RocksDBStateBackend::new(temp_dir.path().to_path_buf())?;

        backend.put(b"key1", b"value1").await?;
        let value = backend.get(b"key1").await?;
        assert_eq!(value, Some(b"value1".to_vec()));

        assert!(backend.contains(b"key1").await?);

        backend.delete(b"key1").await?;
        assert!(!backend.contains(b"key1").await?);

        Ok(())
    }
}
