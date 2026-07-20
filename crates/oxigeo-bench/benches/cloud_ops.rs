//! Cloud storage operations benchmarks using Criterion.
//!
//! # Scope: no real network I/O
//!
//! Every benchmark in this file is deterministic and offline: it synthesizes
//! in-memory byte buffers and measures local serialization/checksum/caching
//! overhead representative of the named cloud I/O pattern (range requests,
//! caching, prefetching, multipart upload). None of them construct an
//! `oxigeo-cloud` client or perform any network call, so the reported
//! durations reflect only local CPU cost, not real S3/GCS/Azure network
//! latency. See `oxigeo_bench::scenarios::cloud` for the scenario-level
//! equivalents and the same caveat.
#![allow(missing_docs, clippy::expect_used, clippy::panic, clippy::unit_arg)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;
use std::time::Duration;

#[cfg(feature = "cloud")]
fn bench_range_requests(c: &mut Criterion) {
    let mut group = c.benchmark_group("range_requests");

    group.sample_size(10);

    for range_size in [64 * 1024, 256 * 1024, 1024 * 1024].iter() {
        group.throughput(Throughput::Bytes(*range_size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(range_size),
            range_size,
            |b, &size| {
                b.iter(|| {
                    // Simulate parsing a range-request response of `size` bytes
                    // (HTTP chunked transfer decode + checksum verify)
                    let data: Vec<u8> = (0..size)
                        .map(|i| ((i * 6364136223846793005usize + 1442695040888963407) % 256) as u8)
                        .collect();
                    let checksum: u32 = data.chunks(4).fold(0u32, |acc, chunk| {
                        let word = chunk
                            .iter()
                            .enumerate()
                            .fold(0u32, |w, (i, &b)| w | ((b as u32) << (i * 8)));
                        acc.wrapping_add(word)
                    });
                    black_box(checksum)
                });
            },
        );
    }

    group.finish();
}

#[cfg(feature = "cloud")]
fn bench_caching_strategies(c: &mut Criterion) {
    let mut group = c.benchmark_group("caching");

    group.sample_size(10);

    let data = vec![0u8; 1024 * 1024]; // 1MB of data

    group.bench_function("no_cache", |b| {
        b.iter(|| {
            // Simulate fetching from cloud without cache: full copy + hash
            let fetched = data.clone();
            let hash: u64 = fetched.iter().enumerate().fold(0u64, |acc, (i, &b)| {
                acc.wrapping_add((b as u64).wrapping_mul(i as u64 + 1))
            });
            black_box(hash)
        });
    });

    group.bench_function("with_cache", |b| {
        b.iter(|| {
            // Simulate cache hit: just compute a checksum without copying
            let hash: u64 = data
                .iter()
                .fold(0u64, |acc, &b| acc.wrapping_add(b as u64).rotate_left(1));
            black_box(hash)
        });
    });

    group.finish();
}

#[cfg(feature = "cloud")]
fn bench_prefetching(c: &mut Criterion) {
    let mut group = c.benchmark_group("prefetching");

    group.sample_size(10);

    for parallel_requests in [1, 2, 4, 8].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(parallel_requests),
            parallel_requests,
            |b, &count| {
                b.iter(|| {
                    // Simulate parallel prefetch: create `count` chunks and compute checksums
                    let chunks: Vec<u64> = (0..count)
                        .map(|worker_id| {
                            let data: Vec<u8> = (0..64 * 1024)
                                .map(|i| ((worker_id * 12345 + i) % 256) as u8)
                                .collect();
                            data.iter().fold(0u64, |acc, &b| acc.wrapping_add(b as u64))
                        })
                        .collect();
                    black_box(chunks.iter().sum::<u64>())
                });
            },
        );
    }

    group.finish();
}

#[cfg(feature = "cloud")]
fn bench_multipart_upload(c: &mut Criterion) {
    let mut group = c.benchmark_group("multipart_upload");

    group.sample_size(5);
    group.measurement_time(Duration::from_secs(15));

    for part_size in [5 * 1024 * 1024, 10 * 1024 * 1024].iter() {
        group.throughput(Throughput::Bytes(*part_size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(part_size),
            part_size,
            |b, &size| {
                let data = vec![0u8; size];
                b.iter(|| {
                    // Simulate multipart upload: split data into parts, compute per-part ETag (MD5-like)
                    let parts: Vec<u32> = data
                        .chunks(1024 * 1024)
                        .map(|part| {
                            // Polynomial rolling hash as MD5 stand-in
                            part.iter().fold(0u32, |acc, &b| {
                                acc.wrapping_mul(16777619).wrapping_add(b as u32)
                            })
                        })
                        .collect();
                    // Simulate completing multipart: hash of ETags
                    let final_etag: u32 = parts.iter().fold(2166136261u32, |acc, &p| {
                        acc.wrapping_mul(16777619).wrapping_add(p)
                    });
                    black_box(final_etag)
                });
            },
        );
    }

    group.finish();
}

#[cfg(feature = "cloud")]
criterion_group!(
    cloud_benches,
    bench_range_requests,
    bench_caching_strategies,
    bench_prefetching,
    bench_multipart_upload
);

#[cfg(not(feature = "cloud"))]
criterion_group!(cloud_benches,);

criterion_main!(cloud_benches);
