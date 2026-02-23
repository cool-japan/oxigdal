//! Cache module tests

#[cfg(test)]
#[cfg(feature = "cache")]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    use bytes::Bytes;
    use std::sync::atomic::Ordering;
    use std::time::Duration;

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
        let temp_dir = std::env::temp_dir().join("oxigdal-multi-cache-test");
        let config = CacheConfig::new().with_cache_dir(&temp_dir);
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
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[tokio::test]
    async fn test_persistent_disk_cache() {
        let temp_dir = std::env::temp_dir().join("oxigdal-disk-cache-test");
        let config = CacheConfig::new().with_cache_dir(&temp_dir);
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
        std::fs::remove_dir_all(&temp_dir).ok();
    }
}
