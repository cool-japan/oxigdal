//! Multi-tier cache integration tests

use bytes::Bytes;
use oxigdal_cache_advanced::{CacheConfig, compression::DataType, multi_tier::*};
use std::sync::Arc;

#[tokio::test]
async fn test_l1_memory_tier_basic() {
    let tier = L1MemoryTier::new(1024 * 1024); // 1MB

    let key = "test_key".to_string();
    let value = CacheValue::new(Bytes::from("test data"), DataType::Binary);

    // Put and get
    tier.put(key.clone(), value.clone())
        .await
        .expect("put failed");
    let retrieved = tier.get(&key).await.expect("get failed");

    assert!(retrieved.is_some());
    let retrieved_value = retrieved.expect("value should exist");
    assert_eq!(retrieved_value.data, value.data);

    // Check stats
    let stats = tier.stats().await;
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.misses, 0);
    assert_eq!(stats.item_count, 1);
}

#[tokio::test]
async fn test_l1_eviction_on_full() {
    let tier = L1MemoryTier::new(200); // Very small cache

    let value1 = CacheValue::new(Bytes::from("a".repeat(80)), DataType::Binary);
    let value2 = CacheValue::new(Bytes::from("b".repeat(80)), DataType::Binary);
    let value3 = CacheValue::new(Bytes::from("c".repeat(80)), DataType::Binary);

    tier.put("key1".to_string(), value1)
        .await
        .expect("put failed");
    tier.put("key2".to_string(), value2)
        .await
        .expect("put failed");

    // This should trigger eviction
    tier.put("key3".to_string(), value3)
        .await
        .expect("put failed");

    let stats = tier.stats().await;
    assert!(stats.evictions > 0);
    assert!(stats.item_count <= 3);
}

#[tokio::test]
async fn test_l1_remove() {
    let tier = L1MemoryTier::new(1024 * 1024);

    let key = "test_key".to_string();
    let value = CacheValue::new(Bytes::from("test data"), DataType::Binary);

    tier.put(key.clone(), value).await.expect("put failed");
    assert!(tier.contains(&key).await);

    let removed = tier.remove(&key).await.expect("remove failed");
    assert!(removed);
    assert!(!tier.contains(&key).await);
}

#[tokio::test]
async fn test_l2_disk_tier_basic() {
    use std::time::{SystemTime, UNIX_EPOCH};

    // Create unique temp directory for this test run to avoid conflicts
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_micros();
    let temp_dir = std::env::temp_dir().join(format!("oxigdal_cache_l2_test_{}", timestamp));

    // Ensure clean state - remove directory if it exists
    let _ = tokio::fs::remove_dir_all(&temp_dir).await;

    let tier = L2DiskTier::new(temp_dir.clone(), 1024 * 1024)
        .await
        .expect("L2 tier creation failed");

    let key = "test_key".to_string();
    let value = CacheValue::new(Bytes::from("test data for L2"), DataType::Text);

    // Put and get
    tier.put(key.clone(), value.clone())
        .await
        .expect("put failed");
    let retrieved = tier.get(&key).await.expect("get failed");

    assert!(retrieved.is_some());

    // Clean up
    tier.clear().await.expect("clear failed");
    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_l2_persistence() {
    let temp_dir = std::env::temp_dir().join("oxigdal_cache_l2_persist_test");

    // Create tier and add data
    {
        let tier = L2DiskTier::new(temp_dir.clone(), 1024 * 1024)
            .await
            .expect("L2 tier creation failed");

        let key = "persist_key".to_string();
        let value = CacheValue::new(Bytes::from("persistent data"), DataType::Binary);

        tier.put(key, value).await.expect("put failed");
    }

    // Create new tier instance and check if data persists
    {
        let tier = L2DiskTier::new(temp_dir.clone(), 1024 * 1024)
            .await
            .expect("L2 tier creation failed");

        let stats = tier.stats().await;
        assert!(stats.item_count > 0);
    }

    // Clean up
    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_multi_tier_cache_get_from_l1() {
    let temp_dir = std::env::temp_dir().join("oxigdal_cache_multi_test1");
    let config = CacheConfig {
        l1_size: 1024 * 1024,
        l2_size: 4096 * 1024,
        l3_size: 0,
        enable_compression: true,
        enable_prefetch: false,
        enable_distributed: false,
        cache_dir: Some(temp_dir.clone()),
    };

    let cache = MultiTierCache::new(config)
        .await
        .expect("cache creation failed");

    let key = "test_multi_l1".to_string();
    let value = CacheValue::new(Bytes::from("multi-tier test data"), DataType::Text);

    // Put
    cache
        .put(key.clone(), value.clone())
        .await
        .expect("put failed");

    // Get (should hit L1)
    let retrieved = cache.get(&key).await.expect("get failed");
    assert!(retrieved.is_some());
    assert_eq!(retrieved.as_ref().map(|v| &v.data), Some(&value.data));

    let stats = cache.stats().await;
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.misses, 0);

    // Clean up
    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_multi_tier_cache_promotion() {
    let temp_dir = std::env::temp_dir().join("oxigdal_cache_multi_test2");
    let config = CacheConfig {
        l1_size: 100, // Very small L1
        l2_size: 1024 * 1024,
        l3_size: 0,
        enable_compression: true,
        enable_prefetch: false,
        enable_distributed: false,
        cache_dir: Some(temp_dir.clone()),
    };

    let cache = MultiTierCache::new(config)
        .await
        .expect("cache creation failed");

    let key = "promote_key".to_string();
    let value = CacheValue::new(Bytes::from("data for promotion test"), DataType::Binary);

    // Put (will be in both L1 and L2)
    cache
        .put(key.clone(), value.clone())
        .await
        .expect("put failed");

    // Get (should retrieve from cache)
    let retrieved = cache.get(&key).await.expect("get failed");
    assert!(retrieved.is_some());

    // Clean up
    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_multi_tier_cache_remove() {
    let temp_dir = std::env::temp_dir().join("oxigdal_cache_multi_test3");
    let config = CacheConfig {
        l1_size: 1024 * 1024,
        l2_size: 4096 * 1024,
        l3_size: 0,
        enable_compression: true,
        enable_prefetch: false,
        enable_distributed: false,
        cache_dir: Some(temp_dir.clone()),
    };

    let cache = MultiTierCache::new(config)
        .await
        .expect("cache creation failed");

    let key = "remove_key".to_string();
    let value = CacheValue::new(Bytes::from("data to remove"), DataType::Binary);

    cache.put(key.clone(), value).await.expect("put failed");
    assert!(cache.contains(&key).await);

    let removed = cache.remove(&key).await.expect("remove failed");
    assert!(removed);
    assert!(!cache.contains(&key).await);

    // Clean up
    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_multi_tier_cache_tier_stats() {
    let temp_dir = std::env::temp_dir().join("oxigdal_cache_multi_test4");
    let config = CacheConfig {
        l1_size: 1024 * 1024,
        l2_size: 4096 * 1024,
        l3_size: 0,
        enable_compression: true,
        enable_prefetch: false,
        enable_distributed: false,
        cache_dir: Some(temp_dir.clone()),
    };

    let cache = MultiTierCache::new(config)
        .await
        .expect("cache creation failed");

    // Add some data
    for i in 0..10 {
        let key = format!("key{}", i);
        let value = CacheValue::new(Bytes::from(format!("value{}", i)), DataType::Text);
        cache.put(key, value).await.expect("put failed");
    }

    let tier_stats = cache.tier_stats().await;
    assert!(tier_stats.contains_key("L1-Memory"));
    assert!(tier_stats.contains_key("L2-Disk"));

    // Clean up
    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[tokio::test]
async fn test_cache_value_access_tracking() {
    let mut value = CacheValue::new(Bytes::from("test"), DataType::Binary);

    assert_eq!(value.access_count, 0);

    value.record_access();
    assert_eq!(value.access_count, 1);

    value.record_access();
    assert_eq!(value.access_count, 2);

    assert!(value.age_seconds() >= 0);
    assert!(value.idle_seconds() >= 0);
}

#[tokio::test]
async fn test_concurrent_access() {
    let temp_dir = std::env::temp_dir().join("oxigdal_cache_concurrent_test");
    let config = CacheConfig {
        l1_size: 10 * 1024 * 1024,
        l2_size: 0,
        l3_size: 0,
        enable_compression: false,
        enable_prefetch: false,
        enable_distributed: false,
        cache_dir: Some(temp_dir.clone()),
    };

    let cache = Arc::new(
        MultiTierCache::new(config)
            .await
            .expect("cache creation failed"),
    );

    let mut handles = Vec::new();

    // Spawn multiple tasks
    for i in 0..10 {
        let cache_clone = Arc::clone(&cache);
        let handle = tokio::spawn(async move {
            let key = format!("key{}", i);
            let value = CacheValue::new(Bytes::from(format!("value{}", i)), DataType::Binary);

            cache_clone
                .put(key.clone(), value)
                .await
                .expect("put failed");

            let retrieved = cache_clone.get(&key).await.expect("get failed");
            assert!(retrieved.is_some());
        });

        handles.push(handle);
    }

    // Wait for all tasks
    for handle in handles {
        handle.await.expect("task failed");
    }

    // Clean up
    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}
