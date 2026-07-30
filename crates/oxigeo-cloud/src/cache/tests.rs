//! Cache module tests

#[cfg(test)]
#[cfg(feature = "cache")]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    use bytes::Bytes;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    /// Per-test scratch cache directory inside the system temp dir (house
    /// policy: no hardcoded absolute paths).
    ///
    /// The leaf name embeds the process id and a monotonic counter, so no two
    /// test binaries — nor two concurrent runs of this one — can ever land on
    /// the same directory.  Dropping the guard removes the directory tree, so
    /// a panicking test leaks nothing.
    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
            Self(std::env::temp_dir().join(format!(
                "oxigeo_cloud_cache_{}_{seq}_{name}",
                std::process::id()
            )))
        }
    }

    impl std::ops::Deref for TempDir {
        type Target = std::path::Path;

        fn deref(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl AsRef<std::path::Path> for TempDir {
        fn as_ref(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[tokio::test]
    async fn test_lru_ttl_cache_put_get() {
        let config = CacheConfig::new().with_max_memory_size(1024 * 1024);
        let cache = eviction::LruTtlCache::new(config).expect("Failed to create cache");
        let key = "test-key".to_string();
        let data = Bytes::from("test data");
        cache
            .put(key.clone(), data.clone(), None)
            .await
            .expect("Put failed");
        let retrieved = cache.get(&key).await.expect("Get failed");
        assert_eq!(retrieved, data);
    }

    #[tokio::test]
    async fn test_lru_ttl_cache_expiration() {
        let config = CacheConfig::new()
            .with_max_memory_size(1024 * 1024)
            .with_default_ttl(Duration::from_millis(100));
        let cache = eviction::LruTtlCache::new(config).expect("Failed to create cache");
        let key = "expiring-key".to_string();
        let data = Bytes::from("expiring data");
        cache
            .put(key.clone(), data.clone(), Some(Duration::from_millis(50)))
            .await
            .expect("Put failed");
        let result = cache.get(&key).await;
        assert!(result.is_ok());
        tokio::time::sleep(Duration::from_millis(100)).await;
        let result = cache.get(&key).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_lfu_cache() {
        let config = CacheConfig::new().with_max_memory_size(1024);
        let cache = eviction::LfuCache::new(config);
        cache
            .put("key1".to_string(), Bytes::from("data1"), None)
            .await
            .expect("Put failed");
        cache
            .put("key2".to_string(), Bytes::from("data2"), None)
            .await
            .expect("Put failed");
        for _ in 0..5 {
            cache.get(&"key1".to_string()).await.ok();
        }
        let result = cache.get(&"key1".to_string()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_arc_cache() {
        let config = CacheConfig::new().with_max_entries(100);
        let cache = eviction::ArcCache::new(config);
        cache
            .put("key1".to_string(), Bytes::from("data1"), None)
            .await
            .expect("Put failed");
        let result = cache.get(&"key1".to_string()).await;
        assert!(result.is_ok());
        assert_eq!(result.expect("data"), Bytes::from("data1"));
    }

    #[tokio::test]
    async fn test_arc_cache_oversized_entry_does_not_hang() {
        // Regression test: a single entry larger than max_memory_size on an
        // otherwise empty ArcCache must not spin forever inside `put`.
        let config = CacheConfig::new().with_max_memory_size(16);
        let cache = eviction::ArcCache::new(config);
        let data = Bytes::from(vec![0u8; 1024]);
        let outcome = tokio::time::timeout(
            Duration::from_secs(5),
            cache.put("huge-key".to_string(), data.clone(), None),
        )
        .await;
        let put_result = outcome.expect("ArcCache::put hung on an oversized entry");
        put_result.expect("Put failed");
        let retrieved = cache
            .get(&"huge-key".to_string())
            .await
            .expect("Get failed");
        assert_eq!(retrieved, data);
    }

    #[tokio::test]
    async fn test_tile_cache() {
        let config = CacheConfig::new().with_max_memory_size(1024 * 1024);
        let cache = backends::TileCache::new(config);
        let coord = metadata::TileCoord::new(10, 500, 300);
        let data = Bytes::from(vec![0u8; 256]);
        cache
            .put(coord.clone(), data.clone(), None)
            .await
            .expect("Put failed");
        let result = cache.get(&coord).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_tile_coord_parent_children() {
        let coord = metadata::TileCoord::new(5, 10, 20);
        let parent = coord.parent();
        assert!(parent.is_some());
        let p = parent.expect("parent");
        assert_eq!(p.z, 4);
        assert_eq!(p.x, 5);
        assert_eq!(p.y, 10);
        let children = coord.children();
        assert_eq!(children.len(), 4);
        assert_eq!(children[0].z, 6);
    }

    #[tokio::test]
    async fn test_spatial_info_intersection() {
        let s1 = metadata::SpatialInfo::new((0.0, 0.0, 10.0, 10.0));
        let s2 = metadata::SpatialInfo::new((5.0, 5.0, 15.0, 15.0));
        let s3 = metadata::SpatialInfo::new((20.0, 20.0, 30.0, 30.0));
        assert!(s1.intersects(&s2));
        assert!(!s1.intersects(&s3));
    }

    #[tokio::test]
    async fn test_cache_stats() {
        let config = CacheConfig::new();
        let cache = eviction::LruTtlCache::new(config).expect("Failed to create cache");
        cache
            .put("key".to_string(), Bytes::from("data"), None)
            .await
            .ok();
        cache.get(&"key".to_string()).await.ok();
        cache.get(&"nonexistent".to_string()).await.ok();
        let stats = cache.stats();
        assert_eq!(stats.hits.load(Ordering::Relaxed), 1);
        assert_eq!(stats.misses.load(Ordering::Relaxed), 1);
        assert_eq!(stats.writes.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_multi_level_cache() {
        let temp_dir = TempDir::new("multi-cache-test");
        let config = CacheConfig::new().with_cache_dir(temp_dir.to_path_buf());
        let cache = multi::MultiLevelCache::new(config).expect("Failed to create cache");
        let key = "test-key".to_string();
        let data = Bytes::from("test data");
        cache
            .put(key.clone(), data.clone())
            .await
            .expect("Put failed");
        cache.memory.clear().await.ok();
        let retrieved = cache.get(&key).await.expect("Get failed");
        assert_eq!(retrieved, data);
        cache.clear().await.ok();
    }

    #[tokio::test]
    async fn test_persistent_disk_cache() {
        let temp_dir = TempDir::new("disk-cache-test");
        let config = CacheConfig::new().with_cache_dir(temp_dir.to_path_buf());
        let cache = backends::PersistentDiskCache::new(config).expect("Failed to create cache");
        let key = "disk-key".to_string();
        let data = Bytes::from("disk data");
        cache
            .put(key.clone(), data.clone(), None)
            .await
            .expect("Put failed");
        let retrieved = cache.get(&key).await.expect("Get failed");
        assert_eq!(retrieved, data);
        cache.clear().await.ok();
    }
}
