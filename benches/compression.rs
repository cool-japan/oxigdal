//! Benchmarks for compression and decompression operations
#![allow(missing_docs, clippy::expect_used)]
//!
//! This benchmark suite measures the performance of:
//! - DEFLATE compression/decompression
//! - LZW compression/decompression
//! - ZSTD compression/decompression
//! - PackBits compression/decompression
//! - Predictor application
//!
//! Tests are run with various data patterns:
//! - Random data (worst case for compression)
//! - Repeated data (best case for compression)
//! - Structured data (typical geospatial patterns)

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_geotiff::compression::{
    apply_predictor_forward, apply_predictor_reverse, compress, decompress,
};
use oxigdal_geotiff::tiff::{Compression, Predictor};
use std::hint::black_box;

/// Generate random-like data (worst case for compression)
fn generate_random_data(size: usize) -> Vec<u8> {
    (0..size)
        .map(|i| ((i * 1103515245 + 12345) >> 16) as u8)
        .collect()
}

/// Generate repeated data (best case for compression)
fn generate_repeated_data(size: usize) -> Vec<u8> {
    vec![42u8; size]
}

/// Generate structured data (typical geospatial pattern)
fn generate_structured_data(size: usize) -> Vec<u8> {
    let width = (size as f64).sqrt() as usize;
    let mut data = vec![0u8; size];
    for y in 0..width {
        for x in 0..width {
            let idx = y * width + x;
            if idx < size {
                // Create a gradient pattern
                data[idx] = ((x + y) % 256) as u8;
            }
        }
    }
    data
}

fn bench_packbits_compress(c: &mut Criterion) {
    let mut group = c.benchmark_group("compression/packbits/compress");

    let sizes = vec![
        4096,    // 4 KB
        65536,   // 64 KB
        1048576, // 1 MB
    ];

    for size in sizes {
        let data_types = vec![
            ("random", generate_random_data(size)),
            ("repeated", generate_repeated_data(size)),
            ("structured", generate_structured_data(size)),
        ];

        for (pattern, data) in data_types {
            group.throughput(Throughput::Bytes(size as u64));
            group.bench_with_input(
                BenchmarkId::new(pattern, format!("{}KB", size / 1024)),
                &data,
                |b, data| {
                    b.iter(|| {
                        black_box(compress(black_box(data), Compression::Packbits).ok());
                    });
                },
            );
        }
    }

    group.finish();
}

fn bench_packbits_decompress(c: &mut Criterion) {
    let mut group = c.benchmark_group("compression/packbits/decompress");

    let sizes = vec![4096, 65536, 1048576];

    for size in sizes {
        let data = generate_structured_data(size);
        let compressed = compress(&data, Compression::Packbits).expect("compression should work");

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}KB", size / 1024)),
            &(compressed, size),
            |b, (comp, expected_size)| {
                b.iter(|| {
                    black_box(
                        decompress(black_box(comp), Compression::Packbits, *expected_size).ok(),
                    );
                });
            },
        );
    }

    group.finish();
}

#[cfg(feature = "deflate")]
fn bench_deflate_compress(c: &mut Criterion) {
    let mut group = c.benchmark_group("compression/deflate/compress");

    let sizes = vec![4096, 65536, 1048576, 4194304];

    for size in sizes {
        let data_types = vec![
            ("random", generate_random_data(size)),
            ("repeated", generate_repeated_data(size)),
            ("structured", generate_structured_data(size)),
        ];

        for (pattern, data) in data_types {
            group.throughput(Throughput::Bytes(size as u64));
            group.bench_with_input(
                BenchmarkId::new(pattern, format!("{}KB", size / 1024)),
                &data,
                |b, data| {
                    b.iter(|| {
                        black_box(compress(black_box(data), Compression::Deflate).ok());
                    });
                },
            );
        }
    }

    group.finish();
}

#[cfg(feature = "deflate")]
fn bench_deflate_decompress(c: &mut Criterion) {
    let mut group = c.benchmark_group("compression/deflate/decompress");

    let sizes = vec![4096, 65536, 1048576, 4194304];

    for size in sizes {
        let data = generate_structured_data(size);
        let compressed = compress(&data, Compression::Deflate).expect("compression should work");

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}KB", size / 1024)),
            &(compressed, size),
            |b, (comp, expected_size)| {
                b.iter(|| {
                    black_box(
                        decompress(black_box(comp), Compression::Deflate, *expected_size).ok(),
                    );
                });
            },
        );
    }

    group.finish();
}

#[cfg(feature = "lzw")]
fn bench_lzw_compress(c: &mut Criterion) {
    let mut group = c.benchmark_group("compression/lzw/compress");

    let sizes = vec![4096, 65536, 1048576, 4194304];

    for size in sizes {
        let data_types = vec![
            ("random", generate_random_data(size)),
            ("repeated", generate_repeated_data(size)),
            ("structured", generate_structured_data(size)),
        ];

        for (pattern, data) in data_types {
            group.throughput(Throughput::Bytes(size as u64));
            group.bench_with_input(
                BenchmarkId::new(pattern, format!("{}KB", size / 1024)),
                &data,
                |b, data| {
                    b.iter(|| {
                        black_box(compress(black_box(data), Compression::Lzw).ok());
                    });
                },
            );
        }
    }

    group.finish();
}

#[cfg(feature = "lzw")]
fn bench_lzw_decompress(c: &mut Criterion) {
    let mut group = c.benchmark_group("compression/lzw/decompress");

    let sizes = vec![4096, 65536, 1048576, 4194304];

    for size in sizes {
        let data = generate_structured_data(size);
        let compressed = compress(&data, Compression::Lzw).expect("compression should work");

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}KB", size / 1024)),
            &(compressed, size),
            |b, (comp, expected_size)| {
                b.iter(|| {
                    black_box(decompress(black_box(comp), Compression::Lzw, *expected_size).ok());
                });
            },
        );
    }

    group.finish();
}

#[cfg(feature = "zstd")]
fn bench_zstd_compress(c: &mut Criterion) {
    let mut group = c.benchmark_group("compression/zstd/compress");

    let sizes = vec![4096, 65536, 1048576, 4194304];

    for size in sizes {
        let data_types = vec![
            ("random", generate_random_data(size)),
            ("repeated", generate_repeated_data(size)),
            ("structured", generate_structured_data(size)),
        ];

        for (pattern, data) in data_types {
            group.throughput(Throughput::Bytes(size as u64));
            group.bench_with_input(
                BenchmarkId::new(pattern, format!("{}KB", size / 1024)),
                &data,
                |b, data| {
                    b.iter(|| {
                        black_box(compress(black_box(data), Compression::Zstd).ok());
                    });
                },
            );
        }
    }

    group.finish();
}

#[cfg(feature = "zstd")]
fn bench_zstd_decompress(c: &mut Criterion) {
    let mut group = c.benchmark_group("compression/zstd/decompress");

    let sizes = vec![4096, 65536, 1048576, 4194304];

    for size in sizes {
        let data = generate_structured_data(size);
        let compressed = compress(&data, Compression::Zstd).expect("compression should work");

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}KB", size / 1024)),
            &(compressed, size),
            |b, (comp, expected_size)| {
                b.iter(|| {
                    black_box(decompress(black_box(comp), Compression::Zstd, *expected_size).ok());
                });
            },
        );
    }

    group.finish();
}

fn bench_predictor_forward(c: &mut Criterion) {
    let mut group = c.benchmark_group("compression/predictor/forward");

    let sizes = vec![4096, 65536, 1048576];

    for size in sizes {
        let data = generate_structured_data(size);
        let width = (size as f64).sqrt() as usize;

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}KB", size / 1024)),
            &data,
            |b, input_data| {
                b.iter(|| {
                    let mut data_copy = black_box(input_data.clone());
                    apply_predictor_forward(
                        &mut data_copy,
                        Predictor::HorizontalDifferencing,
                        1,
                        1,
                        width,
                    );
                });
            },
        );
    }

    group.finish();
}

fn bench_predictor_reverse(c: &mut Criterion) {
    let mut group = c.benchmark_group("compression/predictor/reverse");

    let sizes = vec![4096, 65536, 1048576];

    for size in sizes {
        let mut data = generate_structured_data(size);
        let width = (size as f64).sqrt() as usize;

        // Apply forward first to create appropriate test data
        apply_predictor_forward(&mut data, Predictor::HorizontalDifferencing, 1, 1, width);

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}KB", size / 1024)),
            &data,
            |b, input_data| {
                b.iter(|| {
                    let mut data_copy = black_box(input_data.clone());
                    apply_predictor_reverse(
                        &mut data_copy,
                        Predictor::HorizontalDifferencing,
                        1,
                        1,
                        width,
                    );
                });
            },
        );
    }

    group.finish();
}

fn bench_compression_ratio(c: &mut Criterion) {
    let mut group = c.benchmark_group("compression/ratio");

    let size = 1048576; // 1 MB
    let data = generate_structured_data(size);

    #[cfg(feature = "deflate")]
    {
        let compressed = compress(&data, Compression::Deflate).expect("should compress");
        let ratio = data.len() as f64 / compressed.len() as f64;
        println!("DEFLATE ratio: {:.2}:1", ratio);
        group.bench_function("deflate_ratio", |b| {
            b.iter(|| {
                black_box(compress(&data, Compression::Deflate).ok());
            });
        });
    }

    #[cfg(feature = "lzw")]
    {
        let compressed = compress(&data, Compression::Lzw).expect("should compress");
        let ratio = data.len() as f64 / compressed.len() as f64;
        println!("LZW ratio: {:.2}:1", ratio);
        group.bench_function("lzw_ratio", |b| {
            b.iter(|| {
                black_box(compress(&data, Compression::Lzw).ok());
            });
        });
    }

    #[cfg(feature = "zstd")]
    {
        let compressed = compress(&data, Compression::Zstd).expect("should compress");
        let ratio = data.len() as f64 / compressed.len() as f64;
        println!("ZSTD ratio: {:.2}:1", ratio);
        group.bench_function("zstd_ratio", |b| {
            b.iter(|| {
                black_box(compress(&data, Compression::Zstd).ok());
            });
        });
    }

    let compressed = compress(&data, Compression::Packbits).expect("should compress");
    let ratio = data.len() as f64 / compressed.len() as f64;
    println!("PackBits ratio: {:.2}:1", ratio);
    group.bench_function("packbits_ratio", |b| {
        b.iter(|| {
            black_box(compress(&data, Compression::Packbits).ok());
        });
    });

    group.finish();
}

#[cfg(all(feature = "deflate", feature = "lzw", feature = "zstd"))]
criterion_group!(
    benches,
    bench_packbits_compress,
    bench_packbits_decompress,
    bench_deflate_compress,
    bench_deflate_decompress,
    bench_lzw_compress,
    bench_lzw_decompress,
    bench_zstd_compress,
    bench_zstd_decompress,
    bench_predictor_forward,
    bench_predictor_reverse,
    bench_compression_ratio
);

#[cfg(all(feature = "deflate", feature = "lzw", not(feature = "zstd")))]
criterion_group!(
    benches,
    bench_packbits_compress,
    bench_packbits_decompress,
    bench_deflate_compress,
    bench_deflate_decompress,
    bench_lzw_compress,
    bench_lzw_decompress,
    bench_predictor_forward,
    bench_predictor_reverse,
    bench_compression_ratio
);

#[cfg(all(feature = "deflate", not(feature = "lzw"), feature = "zstd"))]
criterion_group!(
    benches,
    bench_packbits_compress,
    bench_packbits_decompress,
    bench_deflate_compress,
    bench_deflate_decompress,
    bench_zstd_compress,
    bench_zstd_decompress,
    bench_predictor_forward,
    bench_predictor_reverse,
    bench_compression_ratio
);

#[cfg(all(not(feature = "deflate"), feature = "lzw", feature = "zstd"))]
criterion_group!(
    benches,
    bench_packbits_compress,
    bench_packbits_decompress,
    bench_lzw_compress,
    bench_lzw_decompress,
    bench_zstd_compress,
    bench_zstd_decompress,
    bench_predictor_forward,
    bench_predictor_reverse,
    bench_compression_ratio
);

#[cfg(all(feature = "deflate", not(feature = "lzw"), not(feature = "zstd")))]
criterion_group!(
    benches,
    bench_packbits_compress,
    bench_packbits_decompress,
    bench_deflate_compress,
    bench_deflate_decompress,
    bench_predictor_forward,
    bench_predictor_reverse,
    bench_compression_ratio
);

#[cfg(all(not(feature = "deflate"), feature = "lzw", not(feature = "zstd")))]
criterion_group!(
    benches,
    bench_packbits_compress,
    bench_packbits_decompress,
    bench_lzw_compress,
    bench_lzw_decompress,
    bench_predictor_forward,
    bench_predictor_reverse,
    bench_compression_ratio
);

#[cfg(all(not(feature = "deflate"), not(feature = "lzw"), feature = "zstd"))]
criterion_group!(
    benches,
    bench_packbits_compress,
    bench_packbits_decompress,
    bench_zstd_compress,
    bench_zstd_decompress,
    bench_predictor_forward,
    bench_predictor_reverse,
    bench_compression_ratio
);

#[cfg(not(any(feature = "deflate", feature = "lzw", feature = "zstd")))]
criterion_group!(
    benches,
    bench_packbits_compress,
    bench_packbits_decompress,
    bench_predictor_forward,
    bench_predictor_reverse,
    bench_compression_ratio
);

criterion_main!(benches);
