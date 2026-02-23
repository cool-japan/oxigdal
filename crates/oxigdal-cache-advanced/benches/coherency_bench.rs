//! Benchmarks for cache coherency protocols
#![allow(missing_docs, clippy::expect_used, clippy::panic)]

use criterion::{Criterion, criterion_group, criterion_main};
use oxigdal_cache_advanced::coherency::protocol::{
    DirectoryCoherency, InvalidationBatcher, MESIProtocol, MSIProtocol,
};
use std::hint::black_box;

fn bench_msi_protocol(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap_or_else(|_| panic!("Failed to create runtime"));

    c.bench_function("msi_protocol_read", |b| {
        b.iter(|| {
            rt.block_on(async {
                let protocol = MSIProtocol::new("node1".to_string());
                protocol.add_peer("node2".to_string()).await;

                let key = black_box("test_key".to_string());
                let _messages = protocol.handle_read(&key).await;
            })
        });
    });

    c.bench_function("msi_protocol_write", |b| {
        b.iter(|| {
            rt.block_on(async {
                let protocol = MSIProtocol::new("node1".to_string());
                protocol.add_peer("node2".to_string()).await;
                protocol.add_peer("node3".to_string()).await;

                let key = black_box("test_key".to_string());
                let _messages = protocol.handle_write(&key).await;
            })
        });
    });
}

fn bench_mesi_protocol(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap_or_else(|_| panic!("Failed to create runtime"));

    c.bench_function("mesi_protocol_read_exclusive", |b| {
        b.iter(|| {
            rt.block_on(async {
                let protocol = MESIProtocol::new("node1".to_string());
                protocol.add_peer("node2".to_string()).await;

                let key = black_box("test_key".to_string());
                let _messages = protocol.handle_read(&key, false).await;
            })
        });
    });

    c.bench_function("mesi_protocol_upgrade", |b| {
        b.iter(|| {
            rt.block_on(async {
                let protocol = MESIProtocol::new("node1".to_string());
                let key = "test_key".to_string();

                // Get exclusive
                let _messages = protocol.handle_read(&key, false).await;

                // Upgrade to modified
                let _messages = protocol.handle_write(&key).await;
            })
        });
    });
}

fn bench_directory_coherency(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap_or_else(|_| panic!("Failed to create runtime"));

    c.bench_function("directory_read", |b| {
        b.iter(|| {
            rt.block_on(async {
                let dir = DirectoryCoherency::new("node1".to_string());
                let key = black_box("test_key".to_string());
                let _messages = dir.handle_read(&key).await;
            })
        });
    });

    c.bench_function("directory_write_with_invalidations", |b| {
        b.iter(|| {
            rt.block_on(async {
                let dir = DirectoryCoherency::new("node1".to_string());
                let key = "test_key".to_string();

                // First read to establish sharers
                let _messages = dir.handle_read(&key).await;

                // Write should invalidate
                let _messages = dir.handle_write(&key).await;
            })
        });
    });
}

fn bench_invalidation_batching(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap_or_else(|_| panic!("Failed to create runtime"));

    c.bench_function("invalidation_batch_add", |b| {
        b.iter(|| {
            rt.block_on(async {
                let batcher = InvalidationBatcher::new(100);

                for i in 0..10 {
                    batcher
                        .add_invalidation(
                            black_box("node1".to_string()),
                            black_box(format!("key{}", i)),
                        )
                        .await;
                }
            })
        });
    });

    c.bench_function("invalidation_batch_flush", |b| {
        b.iter(|| {
            rt.block_on(async {
                let batcher = InvalidationBatcher::new(100);

                // Add many invalidations
                for i in 0..50 {
                    batcher
                        .add_invalidation("node1".to_string(), format!("key{}", i))
                        .await;
                }

                let _batches = batcher.flush().await;
            })
        });
    });
}

fn bench_coherency_overhead(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap_or_else(|_| panic!("Failed to create runtime"));

    let mut group = c.benchmark_group("coherency_overhead");

    group.bench_function("msi_vs_no_coherency", |b| {
        b.iter(|| {
            rt.block_on(async {
                let protocol = MSIProtocol::new("node1".to_string());

                // Simulate 100 cache operations
                for i in 0..100 {
                    let key = format!("key{}", i % 10);
                    if i % 3 == 0 {
                        let _messages = protocol.handle_write(&key).await;
                    } else {
                        let _messages = protocol.handle_read(&key).await;
                    }
                }
            })
        });
    });

    group.bench_function("mesi_vs_msi", |b| {
        b.iter(|| {
            rt.block_on(async {
                let protocol = MESIProtocol::new("node1".to_string());

                // Exclusive state optimization
                for i in 0..100 {
                    let key = format!("key{}", i);
                    let _messages = protocol.handle_read(&key, false).await;
                    let _messages = protocol.handle_write(&key).await;
                }
            })
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_msi_protocol,
    bench_mesi_protocol,
    bench_directory_coherency,
    bench_invalidation_batching,
    bench_coherency_overhead
);

criterion_main!(benches);
