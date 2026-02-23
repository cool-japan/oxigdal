//! Comprehensive compression benchmarks
#![allow(missing_docs, clippy::expect_used)]

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_compress::codecs::*;
use std::hint::black_box;

fn generate_test_data(size: usize, pattern: &str) -> Vec<u8> {
    match pattern {
        "uniform" => vec![42u8; size],
        "random" => (0..size).map(|i| (i % 256) as u8).collect(),
        "text" => b"The quick brown fox jumps over the lazy dog. "
            .iter()
            .cycle()
            .take(size)
            .copied()
            .collect(),
        _ => vec![0u8; size],
    }
}

fn bench_lz4(c: &mut Criterion) {
    let mut group = c.benchmark_group("lz4");

    for size in [1024, 10240, 102400] {
        group.throughput(Throughput::Bytes(size as u64));

        let data = generate_test_data(size, "text");

        group.bench_function(format!("compress_{}", size), |b| {
            let codec = Lz4Codec::new();
            b.iter(|| {
                let _ = codec.compress(black_box(&data));
            });
        });

        let codec = Lz4Codec::new();
        let compressed = codec.compress(&data).expect("Compression failed");

        group.bench_function(format!("decompress_{}", size), |b| {
            b.iter(|| {
                let _ = codec.decompress(black_box(&compressed), Some(size));
            });
        });
    }

    group.finish();
}

fn bench_zstd(c: &mut Criterion) {
    let mut group = c.benchmark_group("zstd");

    for size in [1024, 10240, 102400] {
        group.throughput(Throughput::Bytes(size as u64));

        let data = generate_test_data(size, "text");

        group.bench_function(format!("compress_{}", size), |b| {
            let codec = ZstdCodec::new();
            b.iter(|| {
                let _ = codec.compress(black_box(&data));
            });
        });

        let codec = ZstdCodec::new();
        let compressed = codec.compress(&data).expect("Compression failed");

        group.bench_function(format!("decompress_{}", size), |b| {
            b.iter(|| {
                let _ = codec.decompress(black_box(&compressed), Some(size * 2));
            });
        });
    }

    group.finish();
}

fn bench_snappy(c: &mut Criterion) {
    let mut group = c.benchmark_group("snappy");

    for size in [1024, 10240, 102400] {
        group.throughput(Throughput::Bytes(size as u64));

        let data = generate_test_data(size, "text");

        group.bench_function(format!("compress_{}", size), |b| {
            let codec = SnappyCodec::new();
            b.iter(|| {
                let _ = codec.compress(black_box(&data));
            });
        });

        let codec = SnappyCodec::new();
        let compressed = codec.compress(&data).expect("Compression failed");

        group.bench_function(format!("decompress_{}", size), |b| {
            b.iter(|| {
                let _ = codec.decompress(black_box(&compressed));
            });
        });
    }

    group.finish();
}

fn bench_parallel(c: &mut Criterion) {
    use oxigdal_compress::parallel::ParallelCompressor;

    let mut group = c.benchmark_group("parallel");

    let size = 1_048_576; // 1 MB
    group.throughput(Throughput::Bytes(size as u64));

    let data = generate_test_data(size, "text");

    group.bench_function("parallel_lz4_compress", |b| {
        let compressor = ParallelCompressor::new();
        b.iter(|| {
            let _ = compressor.compress_lz4(black_box(&data));
        });
    });

    let compressor = ParallelCompressor::new();
    let (compressed, _) = compressor.compress_lz4(&data).expect("Compression failed");

    group.bench_function("parallel_lz4_decompress", |b| {
        b.iter(|| {
            let _ = compressor.decompress_lz4(black_box(&compressed));
        });
    });

    group.finish();
}

criterion_group!(benches, bench_lz4, bench_zstd, bench_snappy, bench_parallel);
criterion_main!(benches);
