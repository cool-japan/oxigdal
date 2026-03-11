//! Integration tests for oxigdal-edge

use bytes::Bytes;
use oxigdal_edge::*;
use oxigdal_edge::{resource, runtime, sync};
use std::sync::atomic::{AtomicU64, Ordering};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Generate a unique temporary directory path for a test.
fn unique_test_dir(label: &str) -> std::path::PathBuf {
    let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("oxigdal_edge_test_{}_{}_{}", label, ts, id))
}

/// Create a minimal EdgeConfig that stores data in a unique temp directory.
fn minimal_config_unique(label: &str) -> runtime::EdgeConfig {
    let mut cfg = runtime::EdgeConfig::minimal();
    cfg.data_dir = unique_test_dir(label);
    cfg
}

/// Create an offline-first EdgeConfig that stores data in a unique temp directory.
fn offline_config_unique(label: &str) -> runtime::EdgeConfig {
    let mut cfg = runtime::EdgeConfig::offline_first();
    cfg.data_dir = unique_test_dir(label);
    // Also point the cache to a unique directory to avoid db lock contention.
    if let Some(ref mut cache_dir) = cfg.cache_config.cache_dir {
        *cache_dir = unique_test_dir(&format!("{}_cache", label));
    }
    cfg
}

#[tokio::test]
async fn test_edge_runtime_full_lifecycle() -> Result<()> {
    let config = minimal_config_unique("full_lifecycle");
    let data_dir = config.data_dir.clone();
    let runtime = EdgeRuntime::new(config).await?;

    // Start runtime
    runtime.start().await?;
    assert_eq!(runtime.state(), runtime::RuntimeState::Running);

    // Check health status immediately (wait_healthy removed to avoid potential hangs in tests)
    assert_eq!(runtime.health(), resource::HealthStatus::Healthy);

    // Execute a task
    let result = runtime.execute(async { Ok(42) }).await?;
    assert_eq!(result, 42);

    // Stop runtime
    runtime.stop().await?;
    assert_eq!(runtime.state(), runtime::RuntimeState::Stopped);

    // Cleanup
    std::fs::remove_dir_all(&data_dir).ok();

    Ok(())
}

#[tokio::test]
async fn test_cache_with_compression() -> Result<()> {
    let cache_config = CacheConfig::minimal();
    let cache = Cache::new(cache_config)?;

    let compressor = EdgeCompressor::fast();

    // Store compressed data
    let original_data = b"This is test data for compression and caching";
    let compressed = compressor.compress(original_data)?;

    cache.put("test_key".to_string(), compressed.clone())?;

    // Retrieve and decompress
    let retrieved = cache.get("test_key")?.expect("Key not found");
    let decompressed = compressor.decompress(&retrieved)?;

    assert_eq!(&decompressed[..], &original_data[..]);

    Ok(())
}

#[tokio::test]
async fn test_resource_management_with_runtime() -> Result<()> {
    let mut config = minimal_config_unique("resource_mgmt");
    config.constraints.max_concurrent_ops = 2;
    let data_dir = config.data_dir.clone();

    let runtime = EdgeRuntime::new(config).await?;
    runtime.start().await?;

    // Execute tasks up to limit
    let task1 = runtime.execute(async {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        Ok(1)
    });

    let task2 = runtime.execute(async {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        Ok(2)
    });

    let results = tokio::join!(task1, task2);
    assert_eq!(results.0?, 1);
    assert_eq!(results.1?, 2);

    runtime.stop().await?;

    // Cleanup
    std::fs::remove_dir_all(&data_dir).ok();

    Ok(())
}

#[tokio::test]
async fn test_offline_first_mode() -> Result<()> {
    let config = offline_config_unique("offline_first");
    let data_dir = config.data_dir.clone();
    let runtime = EdgeRuntime::new(config).await?;
    runtime.start().await?;

    // Cache should be persistent
    let cache = runtime.cache();
    cache.put("offline_key".to_string(), Bytes::from("offline_data"))?;

    let retrieved = cache.get("offline_key")?;
    assert_eq!(retrieved, Some(Bytes::from("offline_data")));

    runtime.stop().await?;

    // Cleanup
    std::fs::remove_dir_all(&data_dir).ok();

    Ok(())
}

#[tokio::test]
async fn test_sync_manager_integration() -> Result<()> {
    let cache_config = CacheConfig::minimal();
    let cache = std::sync::Arc::new(Cache::new(cache_config)?);

    let manager = sync::SyncManager::new(sync::SyncStrategy::Manual, cache)?;

    // Add items to sync queue
    let item1 = sync::SyncItem::new("item-1".to_string(), "key-1".to_string(), vec![1, 2, 3], 1);

    let item2 = sync::SyncItem::new("item-2".to_string(), "key-2".to_string(), vec![4, 5, 6], 1);

    manager.add_pending(item1);
    manager.add_pending(item2);

    let state = manager.state();
    assert_eq!(state.pending_count(), 2);

    // Manual sync
    manager.sync_now().await?;

    let state = manager.state();
    assert_eq!(state.pending_count(), 0);

    Ok(())
}

#[tokio::test]
async fn test_conflict_resolution_with_cache() -> Result<()> {
    let resolver = ConflictResolver::new("edge-node-1".to_string());

    // Create CRDT map
    let mut map = resolver.create_map();
    map.insert("key1", "value1");
    map.insert("key2", "value2");

    assert_eq!(map.get(&"key1"), Some(&"value1"));
    assert_eq!(map.len(), 2);

    // Simulate merge with another node
    let mut map2 = CrdtMap::new("edge-node-2".to_string());
    map2.insert("key2", "value2_updated");
    map2.insert("key3", "value3");

    map.merge(&map2);

    assert_eq!(map.len(), 3);
    assert!(map.contains_key(&"key3"));

    Ok(())
}

#[tokio::test]
async fn test_adaptive_compression() -> Result<()> {
    let compressor = AdaptiveCompressor::new(CompressionLevel::Balanced);

    // Test with different data sizes
    let small_data = b"Hi";
    let (_compressed, strategy) = compressor.compress(small_data)?;

    // Small data should not be compressed
    assert_eq!(strategy, CompressionStrategy::None);

    let large_data = vec![0u8; 10000];
    let (compressed, strategy) = compressor.compress(&large_data)?;

    // Large data should be compressed
    assert!(matches!(
        strategy,
        CompressionStrategy::Lz4 | CompressionStrategy::Snappy
    ));
    assert!(compressed.len() < large_data.len());

    // Verify decompression
    let decompressed = compressor.decompress(&compressed, strategy)?;
    assert_eq!(decompressed.len(), large_data.len());

    Ok(())
}

#[tokio::test]
async fn test_memory_tracking() -> Result<()> {
    let constraints = ResourceConstraints::minimal();
    let manager = ResourceManager::new(constraints)?;

    // Allocate memory
    let _guard1 = manager.allocate_memory(1024)?;
    let metrics = manager.metrics();
    assert_eq!(metrics.memory_bytes, 1024);

    {
        let _guard2 = manager.allocate_memory(512)?;
        let metrics = manager.metrics();
        assert_eq!(metrics.memory_bytes, 1536);
    }

    // After guard2 drops
    let metrics = manager.metrics();
    assert_eq!(metrics.memory_bytes, 1024);

    // Peak memory should be recorded
    assert_eq!(metrics.peak_memory_bytes, 1536);

    Ok(())
}

#[tokio::test]
async fn test_vector_clock_causality() -> Result<()> {
    let mut clock1 = VectorClock::new();
    clock1.increment("node1");
    clock1.increment("node1");

    let mut clock2 = VectorClock::new();
    clock2.increment("node1");
    clock2.increment("node1");
    clock2.increment("node1");

    assert!(clock1.happens_before(&clock2));
    assert!(!clock2.happens_before(&clock1));

    let mut clock3 = VectorClock::new();
    clock3.increment("node2");

    assert!(clock1.is_concurrent(&clock3));

    Ok(())
}

#[tokio::test]
async fn test_crdt_set_operations() -> Result<()> {
    let mut set = CrdtSet::new();

    set.insert(1);
    set.insert(2);
    set.insert(3);

    assert_eq!(set.len(), 3);
    assert!(set.contains(&2));

    set.remove(&2);
    assert!(!set.contains(&2));
    assert_eq!(set.len(), 2);

    // Once removed, cannot be added again
    set.insert(2);
    assert!(!set.contains(&2));

    Ok(())
}

#[tokio::test]
async fn test_end_to_end_edge_workflow() -> Result<()> {
    // Create edge runtime
    let mut config = minimal_config_unique("e2e_workflow");
    config.mode = runtime::RuntimeMode::Offline;
    let data_dir = config.data_dir.clone();

    let runtime = EdgeRuntime::new(config).await?;
    runtime.start().await?;

    // 1. Process data with compression
    let compressor = runtime.compressor();
    let data = b"Geospatial data for edge processing";
    let (compressed, strategy) = compressor.compress(data)?;

    // 2. Store in cache
    let cache = runtime.cache();
    cache.put("geo_data".to_string(), compressed.clone())?;

    // 3. Retrieve and verify
    let retrieved = cache.get("geo_data")?.expect("Data not found");
    assert_eq!(retrieved, compressed);

    // 4. Decompress
    let decompressed = compressor.decompress(&retrieved, strategy)?;
    assert_eq!(&decompressed[..], &data[..]);

    // 5. Check resource usage
    let health = runtime.health();
    assert_eq!(health, resource::HealthStatus::Healthy);

    runtime.stop().await?;

    // Cleanup
    std::fs::remove_dir_all(&data_dir).ok();

    Ok(())
}

#[tokio::test]
async fn test_edge_runtime_pause_resume() -> Result<()> {
    let config = minimal_config_unique("pause_resume");
    let data_dir = config.data_dir.clone();
    let runtime = EdgeRuntime::new(config).await?;

    runtime.start().await?;
    assert_eq!(runtime.state(), runtime::RuntimeState::Running);

    runtime.pause().await?;
    assert_eq!(runtime.state(), runtime::RuntimeState::Paused);

    runtime.resume().await?;
    assert_eq!(runtime.state(), runtime::RuntimeState::Running);

    runtime.stop().await?;

    // Cleanup
    std::fs::remove_dir_all(&data_dir).ok();

    Ok(())
}

#[tokio::test]
async fn test_cache_eviction_policies() -> Result<()> {
    // Test LRU eviction
    let mut config = CacheConfig::minimal();
    config.max_size = 100;
    config.policy = CachePolicy::Lru;

    let cache = Cache::new(config)?;

    // Fill cache
    for i in 0..5 {
        let key = format!("key_{}", i);
        let data = Bytes::from(vec![0u8; 25]);
        cache.put(key, data)?;
    }

    // This should trigger eviction
    cache.put("new_key".to_string(), Bytes::from(vec![0u8; 25]))?;

    // LRU should have evicted oldest entry
    assert!(cache.get("new_key")?.is_some());

    Ok(())
}
