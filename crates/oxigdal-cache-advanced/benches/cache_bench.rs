//! Cache performance benchmarks
#![allow(missing_docs, clippy::expect_used)]

use bytes::Bytes;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_cache_advanced::{
    CacheConfig, CacheStats,
    analytics::CacheAnalytics,
    compression::{AdaptiveCompressor, CompressionCodec, DataType},
    eviction::{EvictionPolicy, LfuEviction, LruEviction},
    multi_tier::*,
    predictive::*,
    warming::*,
};
use std::hint::black_box;
use std::sync::Arc;
use tokio::runtime::Runtime;

fn bench_l1_memory_tier(c: &mut Criterion) {
    let rt = Runtime::new().expect("runtime creation failed");

    let mut group = c.benchmark_group("l1_memory_tier");

    for size in [1024, 10240, 102400].iter() {
        group.throughput(Throughput::Bytes(*size as u64));

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let tier = L1MemoryTier::new(10 * 1024 * 1024); // 10MB

            b.iter(|| {
                rt.block_on(async {
                    let key = format!("key_{}", size);
                    let value = CacheValue::new(Bytes::from(vec![0u8; size]), DataType::Binary);

                    tier.put(key.clone(), value.clone())
                        .await
                        .expect("put failed");

                    let result = tier.get(&key).await.expect("get failed");
                    black_box(result);
                })
            });
        });
    }

    group.finish();
}

fn bench_l2_disk_tier(c: &mut Criterion) {
    let rt = Runtime::new().expect("runtime creation failed");

    let mut group = c.benchmark_group("l2_disk_tier");
    group.sample_size(10); // Disk I/O is slower

    for size in [1024, 10240].iter() {
        group.throughput(Throughput::Bytes(*size as u64));

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter(|| {
                rt.block_on(async {
                    let temp_dir = std::env::temp_dir().join(format!("bench_l2_{}", size));
                    let tier = L2DiskTier::new(temp_dir.clone(), 10 * 1024 * 1024)
                        .await
                        .expect("tier creation failed");

                    let key = format!("key_{}", size);
                    let value = CacheValue::new(Bytes::from(vec![0u8; size]), DataType::Binary);

                    tier.put(key.clone(), value.clone())
                        .await
                        .expect("put failed");

                    let result = tier.get(&key).await.expect("get failed");
                    black_box(result);

                    // Cleanup
                    let _ = tier.clear().await;
                    let _ = tokio::fs::remove_dir_all(temp_dir).await;
                })
            });
        });
    }

    group.finish();
}

fn bench_multi_tier_cache(c: &mut Criterion) {
    let rt = Runtime::new().expect("runtime creation failed");

    let mut group = c.benchmark_group("multi_tier_cache");

    for size in [1024, 10240, 102400].iter() {
        group.throughput(Throughput::Bytes(*size as u64));

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter(|| {
                rt.block_on(async {
                    let temp_dir = std::env::temp_dir().join(format!("bench_multi_{}", size));
                    let config = CacheConfig {
                        l1_size: 1024 * 1024,
                        l2_size: 10 * 1024 * 1024,
                        l3_size: 0,
                        enable_compression: true,
                        enable_prefetch: false,
                        enable_distributed: false,
                        cache_dir: Some(temp_dir.clone()),
                    };

                    let cache = MultiTierCache::new(config)
                        .await
                        .expect("cache creation failed");

                    let key = format!("key_{}", size);
                    let value = CacheValue::new(Bytes::from(vec![0u8; size]), DataType::Binary);

                    cache
                        .put(key.clone(), value.clone())
                        .await
                        .expect("put failed");

                    let result = cache.get(&key).await.expect("get failed");
                    black_box(result);

                    // Cleanup
                    let _ = cache.clear().await;
                    let _ = tokio::fs::remove_dir_all(temp_dir).await;
                })
            });
        });
    }

    group.finish();
}

fn bench_compression(c: &mut Criterion) {
    let mut group = c.benchmark_group("compression");

    for size in [1024, 10240, 102400].iter() {
        group.throughput(Throughput::Bytes(*size as u64));

        group.bench_with_input(BenchmarkId::new("lz4", size), size, |b, &size| {
            let mut compressor = AdaptiveCompressor::new();
            let data = vec![0u8; size];

            b.iter(|| {
                let result = compressor
                    .compress(&data, CompressionCodec::Lz4, DataType::Binary)
                    .expect("compression failed");
                black_box(result);
            });
        });

        group.bench_with_input(BenchmarkId::new("zstd", size), size, |b, &size| {
            let mut compressor = AdaptiveCompressor::new();
            let data = vec![0u8; size];

            b.iter(|| {
                let result = compressor
                    .compress(&data, CompressionCodec::Zstd, DataType::Binary)
                    .expect("compression failed");
                black_box(result);
            });
        });

        group.bench_with_input(BenchmarkId::new("snappy", size), size, |b, &size| {
            let mut compressor = AdaptiveCompressor::new();
            let data = vec![0u8; size];

            b.iter(|| {
                let result = compressor
                    .compress(&data, CompressionCodec::Snappy, DataType::Binary)
                    .expect("compression failed");
                black_box(result);
            });
        });
    }

    group.finish();
}

fn bench_eviction_policies(c: &mut Criterion) {
    let mut group = c.benchmark_group("eviction_policies");

    let num_keys = 1000;

    group.bench_function("lru", |b| {
        b.iter(|| {
            let mut policy = LruEviction::new();

            for i in 0..num_keys {
                policy.on_insert(format!("key{}", i), 1024);
            }

            for i in 0..num_keys {
                policy.on_access(&format!("key{}", i));
            }

            for _ in 0..100 {
                black_box(policy.select_victim());
            }
        });
    });

    group.bench_function("lfu", |b| {
        b.iter(|| {
            let mut policy = LfuEviction::new();

            for i in 0..num_keys {
                policy.on_insert(format!("key{}", i), 1024);
            }

            for i in 0..num_keys {
                policy.on_access(&format!("key{}", i));
            }

            for _ in 0..100 {
                black_box(policy.select_victim());
            }
        });
    });

    group.finish();
}

fn bench_markov_predictor(c: &mut Criterion) {
    let mut group = c.benchmark_group("markov_predictor");

    group.bench_function("record_access", |b| {
        b.iter(|| {
            let mut predictor = MarkovPredictor::new(2);

            for i in 0..1000 {
                let key = format!("key{}", i % 10);
                predictor.record_access(key);
            }

            black_box(predictor);
        });
    });

    group.bench_function("predict", |b| {
        let mut predictor = MarkovPredictor::new(2);

        // Build up state
        for i in 0..1000 {
            let key = format!("key{}", i % 10);
            predictor.record_access(key);
        }

        b.iter(|| {
            let predictions = predictor.predict(5);
            black_box(predictions);
        });
    });

    group.finish();
}

fn bench_cache_analytics(c: &mut Criterion) {
    let rt = Runtime::new().expect("runtime creation failed");

    let mut group = c.benchmark_group("cache_analytics");

    group.bench_function("record_access", |b| {
        b.iter(|| {
            rt.block_on(async {
                let analytics = CacheAnalytics::new();

                for i in 0..1000 {
                    analytics.record_access(format!("key{}", i)).await;
                }

                black_box(analytics);
            })
        });
    });

    group.bench_function("analyze_patterns", |b| {
        b.iter(|| {
            rt.block_on(async {
                let analytics = CacheAnalytics::new();

                // Record some accesses
                for i in 0..100 {
                    analytics.record_access(format!("key{}", i % 10)).await;
                }

                let patterns = analytics.analyze_patterns().await;
                black_box(patterns);
            })
        });
    });

    group.bench_function("generate_recommendations", |b| {
        b.iter(|| {
            rt.block_on(async {
                let analytics = CacheAnalytics::new();

                // Record some stats
                for _ in 0..20 {
                    let stats = CacheStats {
                        hits: 80,
                        misses: 20,
                        evictions: 5,
                        bytes_stored: 1024 * 1024,
                        item_count: 100,
                    };
                    analytics.record_stats(stats).await;
                }

                let recommendations = analytics.generate_recommendations().await;
                black_box(recommendations);
            })
        });
    });

    group.finish();
}

fn bench_cache_warming(c: &mut Criterion) {
    let rt = Runtime::new().expect("runtime creation failed");

    let mut group = c.benchmark_group("cache_warming");
    group.sample_size(10);

    group.bench_function("sequential_warming", |b| {
        b.iter(|| {
            rt.block_on(async {
                let temp_dir = std::env::temp_dir().join("bench_warming_seq");
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

                let data_source = Arc::new(InMemoryDataSource::new());

                // Add test data
                for i in 0..10 {
                    let key = format!("key{}", i);
                    let value =
                        CacheValue::new(Bytes::from(format!("value{}", i)), DataType::Binary);
                    data_source.add(key.clone(), value).await;
                }

                let keys: Vec<_> = (0..10).map(|i| format!("key{}", i)).collect();
                let strategy = Box::new(SequentialWarming::new(keys));

                let warmer =
                    Arc::new(CacheWarmer::new(cache, data_source, strategy, 10).with_batch_size(5));

                warmer.start().await.expect("warming failed");

                let _ = tokio::fs::remove_dir_all(temp_dir).await;
            })
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_l1_memory_tier,
    bench_l2_disk_tier,
    bench_multi_tier_cache,
    bench_compression,
    bench_eviction_policies,
    bench_markov_predictor,
    bench_cache_analytics,
    bench_cache_warming,
);

criterion_main!(benches);
