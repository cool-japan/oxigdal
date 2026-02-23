//! Benchmarks for Rs3gwDataSource concurrent reads
//!
//! This benchmark suite measures the performance of concurrent tile reads
//! with various configurations and compares them to sequential reads.
#![allow(missing_docs, clippy::expect_used)]

use bytes::Bytes;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_core::io::{ByteRange, DataSource};
use oxigdal_rs3gw::datasource::{ConcurrentReadConfig, Rs3gwDataSource};
use rs3gw::storage::backend::{BackendConfig, BackendType, create_backend_from_config};
use std::collections::HashMap;
use std::hint::black_box;
use tempfile::TempDir;

/// Create a test backend with a temporary directory
async fn create_test_backend() -> (rs3gw::storage::backend::DynBackend, TempDir) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let storage_root = temp_dir.path().to_path_buf();

    let config = BackendConfig {
        backend_type: BackendType::Local,
        endpoint: None,
        access_key: None,
        secret_key: None,
        region: None,
        use_ssl: false,
        extra: HashMap::new(),
    };

    let backend = create_backend_from_config(config, Some(storage_root))
        .await
        .expect("Failed to create backend");

    (backend, temp_dir)
}

/// Create a test file with specified size
async fn create_test_file(
    backend: &rs3gw::storage::backend::DynBackend,
    bucket: &str,
    key: &str,
    size_kb: usize,
) {
    backend.create_bucket(bucket).await.ok(); // Ignore if already exists

    let data: Vec<u8> = (0..size_kb * 1024).map(|i| (i % 256) as u8).collect();
    backend
        .put_object(bucket, key, Bytes::from(data), HashMap::new())
        .await
        .expect("Failed to put object");
}

/// Benchmark sequential reads
fn bench_sequential_reads(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");

    let (backend, _temp_dir) = rt.block_on(create_test_backend());
    let bucket = "bench-bucket";
    let key = "test-1mb.bin";

    rt.block_on(create_test_file(&backend, bucket, key, 1024)); // 1MB file

    let config = ConcurrentReadConfig::new()
        .with_concurrency_limit(1) // Sequential
        .with_cache(false);

    let source = rt
        .block_on(Rs3gwDataSource::new_with_config(
            backend,
            bucket.to_string(),
            key.to_string(),
            config,
        ))
        .expect("Failed to create data source");

    let mut group = c.benchmark_group("sequential_reads");
    group.throughput(Throughput::Bytes(256 * 1024)); // 256KB per read

    group.bench_function("read_single_256kb", |b| {
        b.iter(|| {
            let range = ByteRange::new(0, 256 * 1024);
            black_box(source.read_range(range).expect("Read failed"));
        });
    });

    group.bench_function("read_10_ranges_256kb", |b| {
        b.iter(|| {
            let ranges: Vec<ByteRange> = (0..10)
                .map(|i| ByteRange::new(i * 100 * 1024, i * 100 * 1024 + 256 * 1024))
                .collect();
            black_box(source.read_ranges(&ranges).expect("Read failed"));
        });
    });

    group.finish();
}

/// Benchmark concurrent reads with different concurrency levels
fn bench_concurrent_reads(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");

    let mut group = c.benchmark_group("concurrent_reads");
    group.throughput(Throughput::Bytes(256 * 1024 * 10)); // 10 * 256KB

    for concurrency in [2, 4, 8, 16].iter() {
        let (backend, _temp_dir) = rt.block_on(create_test_backend());
        let bucket = "bench-bucket";
        let key = format!("test-concurrent-{}.bin", concurrency);

        rt.block_on(create_test_file(&backend, bucket, &key, 4096)); // 4MB file

        let config = ConcurrentReadConfig::new()
            .with_concurrency_limit(*concurrency)
            .with_cache(false);

        let source = rt
            .block_on(Rs3gwDataSource::new_with_config(
                backend,
                bucket.to_string(),
                key,
                config,
            ))
            .expect("Failed to create data source");

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("concurrency_{}", concurrency)),
            concurrency,
            |b, _| {
                b.iter(|| {
                    let ranges: Vec<ByteRange> = (0..10)
                        .map(|i| ByteRange::new(i * 256 * 1024, (i + 1) * 256 * 1024))
                        .collect();
                    black_box(source.read_ranges(&ranges).expect("Read failed"));
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_sequential_reads, bench_concurrent_reads);
criterion_main!(benches);
