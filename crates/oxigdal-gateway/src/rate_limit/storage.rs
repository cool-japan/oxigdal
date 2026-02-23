//! Storage backends for rate limiting.
//!
//! Provides in-memory and distributed Redis storage for rate limit counters.

#[cfg(feature = "redis")]
use crate::error::GatewayError;
use crate::error::Result;
use async_trait::async_trait;
use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Storage trait for rate limit data.
#[async_trait]
pub trait Storage: Send + Sync {
    /// Gets a value from storage.
    async fn get(&self, key: &str) -> Result<Option<u64>>;

    /// Sets a value in storage with optional TTL.
    async fn set(&self, key: &str, value: u64, ttl: Option<Duration>) -> Result<()>;

    /// Increments a value in storage.
    async fn increment(&self, key: &str, ttl: Option<Duration>) -> Result<u64>;

    /// Deletes a key from storage.
    async fn delete(&self, key: &str) -> Result<()>;

    /// Checks if a key exists.
    async fn exists(&self, key: &str) -> Result<bool>;
}

/// Entry in memory storage with expiration.
#[derive(Debug, Clone)]
struct MemoryEntry {
    value: u64,
    expires_at: Option<u64>,
}

impl MemoryEntry {
    fn new(value: u64, ttl: Option<Duration>) -> Self {
        let expires_at = ttl.map(|ttl| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() + ttl.as_secs())
                .unwrap_or(0)
        });

        Self { value, expires_at }
    }

    fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            now >= expires_at
        } else {
            false
        }
    }
}

/// In-memory storage implementation.
#[derive(Clone)]
pub struct MemoryStorage {
    data: Arc<DashMap<String, MemoryEntry>>,
}

impl MemoryStorage {
    /// Creates a new memory storage.
    pub fn new() -> Self {
        Self {
            data: Arc::new(DashMap::new()),
        }
    }

    /// Cleans up expired entries.
    fn cleanup(&self) {
        self.data.retain(|_, entry| !entry.is_expired());
    }
}

impl Default for MemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Storage for MemoryStorage {
    async fn get(&self, key: &str) -> Result<Option<u64>> {
        self.cleanup();

        Ok(self.data.get(key).and_then(|entry| {
            if entry.is_expired() {
                None
            } else {
                Some(entry.value)
            }
        }))
    }

    async fn set(&self, key: &str, value: u64, ttl: Option<Duration>) -> Result<()> {
        let entry = MemoryEntry::new(value, ttl);
        self.data.insert(key.to_string(), entry);
        Ok(())
    }

    async fn increment(&self, key: &str, ttl: Option<Duration>) -> Result<u64> {
        self.cleanup();

        let new_value = if let Some(mut entry) = self.data.get_mut(key) {
            if entry.is_expired() {
                // Reset if expired
                let new_entry = MemoryEntry::new(1, ttl);
                *entry = new_entry;
                1
            } else {
                entry.value += 1;
                entry.value
            }
        } else {
            let entry = MemoryEntry::new(1, ttl);
            self.data.insert(key.to_string(), entry);
            1
        };

        Ok(new_value)
    }

    async fn delete(&self, key: &str) -> Result<()> {
        self.data.remove(key);
        Ok(())
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        self.cleanup();
        Ok(self
            .data
            .get(key)
            .map(|entry| !entry.is_expired())
            .unwrap_or(false))
    }
}

/// Redis storage implementation for distributed rate limiting.
#[cfg(feature = "redis")]
pub struct RedisStorage {
    client: redis::Client,
}

#[cfg(feature = "redis")]
impl RedisStorage {
    /// Creates a new Redis storage.
    pub fn new(redis_url: &str) -> Result<Self> {
        let client = redis::Client::open(redis_url)
            .map_err(|e| GatewayError::InternalError(format!("Failed to connect to Redis: {e}")))?;

        Ok(Self { client })
    }

    /// Gets connection manager.
    async fn get_connection(&self) -> Result<redis::aio::ConnectionManager> {
        redis::aio::ConnectionManager::new(self.client.clone())
            .await
            .map_err(Into::into)
    }
}

#[cfg(feature = "redis")]
#[async_trait]
impl Storage for RedisStorage {
    async fn get(&self, key: &str) -> Result<Option<u64>> {
        use redis::AsyncCommands;

        let mut conn = self.get_connection().await?;
        let value: Option<String> = conn.get(key).await?;

        Ok(value.and_then(|v| v.parse().ok()))
    }

    async fn set(&self, key: &str, value: u64, ttl: Option<Duration>) -> Result<()> {
        use redis::AsyncCommands;

        let mut conn = self.get_connection().await?;

        if let Some(ttl) = ttl {
            let _: () = conn.set_ex(key, value, ttl.as_secs()).await?;
        } else {
            let _: () = conn.set(key, value).await?;
        }

        Ok(())
    }

    async fn increment(&self, key: &str, ttl: Option<Duration>) -> Result<u64> {
        use redis::AsyncCommands;

        let mut conn = self.get_connection().await?;

        let new_value: u64 = conn.incr(key, 1).await?;

        if let Some(ttl) = ttl {
            if new_value == 1 {
                // Set TTL only on first increment
                let _: bool = conn.expire(key, ttl.as_secs() as i64).await?;
            }
        }

        Ok(new_value)
    }

    async fn delete(&self, key: &str) -> Result<()> {
        use redis::AsyncCommands;

        let mut conn = self.get_connection().await?;
        let _: () = conn.del(key).await?;
        Ok(())
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        use redis::AsyncCommands;

        let mut conn = self.get_connection().await?;
        let exists: bool = conn.exists(key).await?;
        Ok(exists)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_storage_get_set() {
        let storage = MemoryStorage::new();

        storage.set("key1", 42, None).await.ok();
        let value = storage.get("key1").await.ok().flatten();
        assert_eq!(value, Some(42));
    }

    #[tokio::test]
    async fn test_memory_storage_increment() {
        let storage = MemoryStorage::new();

        let val1 = storage.increment("counter", None).await.ok();
        assert_eq!(val1, Some(1));

        let val2 = storage.increment("counter", None).await.ok();
        assert_eq!(val2, Some(2));
    }

    #[tokio::test]
    async fn test_memory_storage_delete() {
        let storage = MemoryStorage::new();

        storage.set("key1", 42, None).await.ok();
        assert!(storage.exists("key1").await.unwrap_or(false));

        storage.delete("key1").await.ok();
        assert!(!storage.exists("key1").await.unwrap_or(true));
    }

    #[tokio::test]
    async fn test_memory_storage_ttl() {
        let storage = MemoryStorage::new();

        // Set with very short TTL
        storage
            .set("key1", 42, Some(Duration::from_millis(1)))
            .await
            .ok();

        // Wait for expiration
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Should be expired
        let value = storage.get("key1").await.ok().flatten();
        assert_eq!(value, None);
    }

    #[tokio::test]
    async fn test_memory_storage_exists() {
        let storage = MemoryStorage::new();

        assert!(!storage.exists("nonexistent").await.unwrap_or(true));

        storage.set("key1", 42, None).await.ok();
        assert!(storage.exists("key1").await.unwrap_or(false));
    }
}
