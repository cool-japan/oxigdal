//! Benchmarks for oxigdal-edge
#![allow(missing_docs, clippy::expect_used)]

use bytes::Bytes;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_edge::*;
use std::hint::black_box;

fn cache_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache");

    for size in [1024, 10240, 102400].iter() {
        group.throughput(Throughput::Bytes(*size as u64));

        group.bench_with_input(BenchmarkId::new("put", size), size, |b, &size| {
            let config = CacheConfig::minimal();
            let cache = Cache::new(config).expect("Failed to create cache");
            let data = Bytes::from(vec![0u8; size]);

            b.iter(|| {
                let key = format!("key_{}", fastrand::u64(..));
                cache.put(black_box(key), black_box(data.clone())).ok();
            });
        });

        group.bench_with_input(BenchmarkId::new("get", size), size, |b, &size| {
            let config = CacheConfig::minimal();
            let cache = Cache::new(config).expect("Failed to create cache");
            let data = Bytes::from(vec![0u8; size]);

            // Pre-populate cache
            for i in 0..100 {
                cache.put(format!("key_{}", i), data.clone()).ok();
            }

            b.iter(|| {
                let key = format!("key_{}", fastrand::usize(0..100));
                cache.get(black_box(&key)).ok();
            });
        });
    }

    group.finish();
}

fn compression_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("compression");

    for size in [1024, 10240, 102400].iter() {
        group.throughput(Throughput::Bytes(*size as u64));

        // LZ4 compression
        group.bench_with_input(BenchmarkId::new("lz4_compress", size), size, |b, &size| {
            let compressor =
                EdgeCompressor::new(CompressionStrategy::Lz4, CompressionLevel::Balanced);
            let data = vec![0u8; size];

            b.iter(|| compressor.compress(black_box(&data)));
        });

        group.bench_with_input(
            BenchmarkId::new("lz4_decompress", size),
            size,
            |b, &size| {
                let compressor =
                    EdgeCompressor::new(CompressionStrategy::Lz4, CompressionLevel::Balanced);
                let data = vec![0u8; size];
                let compressed = compressor.compress(&data).expect("Compression failed");

                b.iter(|| compressor.decompress(black_box(&compressed)));
            },
        );

        // Snappy compression
        group.bench_with_input(
            BenchmarkId::new("snappy_compress", size),
            size,
            |b, &size| {
                let compressor =
                    EdgeCompressor::new(CompressionStrategy::Snappy, CompressionLevel::Fast);
                let data = vec![0u8; size];

                b.iter(|| compressor.compress(black_box(&data)));
            },
        );

        group.bench_with_input(
            BenchmarkId::new("snappy_decompress", size),
            size,
            |b, &size| {
                let compressor =
                    EdgeCompressor::new(CompressionStrategy::Snappy, CompressionLevel::Fast);
                let data = vec![0u8; size];
                let compressed = compressor.compress(&data).expect("Compression failed");

                b.iter(|| compressor.decompress(black_box(&compressed)));
            },
        );

        // Adaptive compression
        group.bench_with_input(
            BenchmarkId::new("adaptive_compress", size),
            size,
            |b, &size| {
                let compressor = AdaptiveCompressor::new(CompressionLevel::Balanced);
                let data = vec![0u8; size];

                b.iter(|| compressor.compress(black_box(&data)));
            },
        );
    }

    group.finish();
}

fn resource_management(c: &mut Criterion) {
    let mut group = c.benchmark_group("resource");

    group.bench_function("operation_guard", |b| {
        let constraints = ResourceConstraints::minimal();
        let manager = ResourceManager::new(constraints).expect("Failed to create manager");

        b.iter(|| {
            let _guard = manager.start_operation().ok();
        });
    });

    group.bench_function("memory_allocation", |b| {
        let constraints = ResourceConstraints::minimal();
        let manager = ResourceManager::new(constraints).expect("Failed to create manager");

        b.iter(|| {
            let _guard = manager.allocate_memory(black_box(1024)).ok();
        });
    });

    group.bench_function("metrics_collection", |b| {
        let constraints = ResourceConstraints::minimal();
        let manager = ResourceManager::new(constraints).expect("Failed to create manager");

        b.iter(|| {
            let _ = manager.metrics();
        });
    });

    group.finish();
}

fn conflict_resolution(c: &mut Criterion) {
    let mut group = c.benchmark_group("conflict");

    group.bench_function("vector_clock_increment", |b| {
        let mut clock = VectorClock::new();

        b.iter(|| {
            clock.increment(black_box("node1"));
        });
    });

    group.bench_function("vector_clock_merge", |b| {
        let mut clock1 = VectorClock::new();
        clock1.increment("node1");

        let mut clock2 = VectorClock::new();
        clock2.increment("node2");

        b.iter(|| {
            clock1.merge(black_box(&clock2));
        });
    });

    group.bench_function("crdt_map_insert", |b| {
        let mut map = CrdtMap::new("node1".to_string());

        b.iter(|| {
            map.insert(black_box("key"), black_box(42));
        });
    });

    group.bench_function("crdt_map_merge", |b| {
        let mut map1 = CrdtMap::new("node1".to_string());
        for i in 0..100 {
            map1.insert(format!("key_{}", i), i);
        }

        let mut map2 = CrdtMap::new("node2".to_string());
        for i in 50..150 {
            map2.insert(format!("key_{}", i), i);
        }

        b.iter(|| {
            map1.merge(black_box(&map2));
        });
    });

    group.bench_function("crdt_set_insert", |b| {
        let mut set = CrdtSet::new();

        b.iter(|| {
            set.insert(black_box(42));
        });
    });

    group.finish();
}

fn edge_vs_cloud_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("edge_vs_cloud");

    // Simulate edge processing (local with compression)
    group.bench_function("edge_processing", |b| {
        let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");

        b.iter(|| {
            runtime.block_on(async {
                let config = EdgeConfig::minimal();
                let edge_runtime = EdgeRuntime::new(config)
                    .await
                    .expect("Failed to create runtime");
                edge_runtime.start().await.ok();

                let compressor = edge_runtime.compressor();
                let data = vec![0u8; 10240];

                let (compressed, strategy) =
                    compressor.compress(&data).expect("Compression failed");

                let cache = edge_runtime.cache();
                cache.put("data".to_string(), compressed.clone()).ok();

                let retrieved = cache.get("data").ok().flatten();
                if let Some(data) = retrieved {
                    compressor.decompress(&data, strategy).ok();
                }

                edge_runtime.stop().await.ok();
            })
        });
    });

    // Simulate cloud processing (no local cache, larger data)
    group.bench_function("cloud_processing", |b| {
        let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");

        b.iter(|| {
            runtime.block_on(async {
                // Simulate network latency
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;

                let data = vec![0u8; 10240];
                // No local caching, direct processing
                let _ = black_box(data);
            })
        });
    });

    group.finish();
}

fn sync_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("sync");

    group.bench_function("sync_item_creation", |b| {
        b.iter(|| {
            let _ = sync::SyncItem::new(
                black_box("item-1".to_string()),
                black_box("key-1".to_string()),
                black_box(vec![1, 2, 3, 4, 5]),
                black_box(1),
            );
        });
    });

    group.bench_function("sync_batch_creation", |b| {
        b.iter(|| {
            let mut batch = sync::SyncBatch::new(black_box("batch-1".to_string()));
            for i in 0..100 {
                let item = sync::SyncItem::new(
                    format!("item-{}", i),
                    format!("key-{}", i),
                    vec![0u8; 100],
                    1,
                );
                batch.add_item(item);
            }
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    cache_operations,
    compression_benchmarks,
    resource_management,
    conflict_resolution,
    edge_vs_cloud_comparison,
    sync_operations
);
criterion_main!(benches);
