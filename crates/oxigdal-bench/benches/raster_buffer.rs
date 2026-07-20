//! Benchmarks for RasterBuffer operations
#![allow(missing_docs, clippy::expect_used)]
//!
//! This benchmark suite measures the performance of:
//! - Buffer creation and initialization
//! - Pixel get/set operations
//! - Statistics computation
//! - Type conversion
//! - Fill operations

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{NoDataValue, RasterDataType};
use std::hint::black_box;

fn bench_buffer_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer/creation");

    let sizes = vec![(256, 256), (512, 512), (1024, 1024), (2048, 2048)];

    let data_types = vec![
        ("uint8", RasterDataType::UInt8),
        ("uint16", RasterDataType::UInt16),
        ("float32", RasterDataType::Float32),
        ("float64", RasterDataType::Float64),
    ];

    for (width, height) in &sizes {
        for (type_name, dtype) in &data_types {
            let pixel_count = width * height;
            group.throughput(Throughput::Elements(pixel_count));

            group.bench_with_input(
                BenchmarkId::new(
                    format!("zeros_{}", type_name),
                    format!("{}x{}", width, height),
                ),
                &(*width, *height, *dtype),
                |b, (w, h, dt)| {
                    b.iter(|| {
                        black_box(RasterBuffer::zeros(*w, *h, *dt));
                    });
                },
            );
        }
    }

    group.finish();
}

fn bench_fill_value(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer/fill_value");

    let sizes = vec![(256, 256), (1024, 1024), (2048, 2048)];

    let data_types = vec![
        ("uint8", RasterDataType::UInt8),
        ("float32", RasterDataType::Float32),
        ("float64", RasterDataType::Float64),
    ];

    for (width, height) in &sizes {
        for (type_name, dtype) in &data_types {
            let pixel_count = width * height;
            group.throughput(Throughput::Elements(pixel_count));

            group.bench_with_input(
                BenchmarkId::new(*type_name, format!("{}x{}", width, height)),
                &(*width, *height, *dtype),
                |b, (w, h, dt)| {
                    let mut buffer = RasterBuffer::zeros(*w, *h, *dt);
                    b.iter(|| {
                        buffer.fill_value(black_box(42.0));
                    });
                },
            );
        }
    }

    group.finish();
}

fn bench_get_pixel(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer/get_pixel");

    let data_types = vec![
        ("uint8", RasterDataType::UInt8),
        ("uint16", RasterDataType::UInt16),
        ("int32", RasterDataType::Int32),
        ("float32", RasterDataType::Float32),
        ("float64", RasterDataType::Float64),
    ];

    for (type_name, dtype) in data_types {
        let buffer = RasterBuffer::zeros(1024, 1024, dtype);

        group.throughput(Throughput::Elements(10000));
        group.bench_with_input(BenchmarkId::from_parameter(type_name), &buffer, |b, buf| {
            b.iter(|| {
                for x in 0..100 {
                    for y in 0..100 {
                        black_box(buf.get_pixel(black_box(x), black_box(y)).ok());
                    }
                }
            });
        });
    }

    group.finish();
}

fn bench_set_pixel(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer/set_pixel");

    let data_types = vec![
        ("uint8", RasterDataType::UInt8),
        ("uint16", RasterDataType::UInt16),
        ("int32", RasterDataType::Int32),
        ("float32", RasterDataType::Float32),
        ("float64", RasterDataType::Float64),
    ];

    for (type_name, dtype) in data_types {
        group.throughput(Throughput::Elements(10000));
        group.bench_with_input(
            BenchmarkId::from_parameter(type_name),
            &dtype,
            |b, dtype| {
                let mut buffer = RasterBuffer::zeros(1024, 1024, *dtype);
                b.iter(|| {
                    for x in 0..100 {
                        for y in 0..100 {
                            black_box(
                                buffer
                                    .set_pixel(black_box(x), black_box(y), black_box(42.5))
                                    .ok(),
                            );
                        }
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_get_set_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer/get_set_roundtrip");

    let data_types = vec![
        ("uint8", RasterDataType::UInt8),
        ("float32", RasterDataType::Float32),
        ("float64", RasterDataType::Float64),
    ];

    for (type_name, dtype) in data_types {
        group.throughput(Throughput::Elements(10000));
        group.bench_with_input(
            BenchmarkId::from_parameter(type_name),
            &dtype,
            |b, dtype| {
                let mut buffer = RasterBuffer::zeros(1024, 1024, *dtype);
                b.iter(|| {
                    for x in 0..100 {
                        for y in 0..100 {
                            if let Ok(value) = buffer.get_pixel(x, y) {
                                black_box(buffer.set_pixel(x, y, value + 1.0).ok());
                            }
                        }
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_statistics(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer/statistics");

    let sizes = vec![(256, 256), (512, 512), (1024, 1024), (2048, 2048)];

    for (width, height) in sizes {
        let pixel_count = width * height;
        group.throughput(Throughput::Elements(pixel_count));

        // Create buffer with varying values
        let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::Float32);
        for y in 0..height {
            for x in 0..width {
                let value = ((x + y) % 256) as f64;
                buffer.set_pixel(x, y, value).ok();
            }
        }

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}x{}", width, height)),
            &buffer,
            |b, buf| {
                b.iter(|| {
                    black_box(buf.compute_statistics().ok());
                });
            },
        );
    }

    group.finish();
}

fn bench_statistics_with_nodata(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer/statistics_nodata");

    let width = 1024u64;
    let height = 1024u64;
    group.throughput(Throughput::Elements(width * height));

    // Create buffer with 10% nodata values
    let mut buffer = RasterBuffer::nodata_filled(
        width,
        height,
        RasterDataType::Float32,
        NoDataValue::Float(-9999.0),
    );

    for y in 0..height {
        for x in 0..width {
            if (x + y) % 10 != 0 {
                let value = ((x + y) % 256) as f64;
                buffer.set_pixel(x, y, value).ok();
            }
        }
    }

    group.bench_function("10pct_nodata", |b| {
        b.iter(|| {
            black_box(buffer.compute_statistics().ok());
        });
    });

    group.finish();
}

fn bench_convert_type(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer/convert_type");

    let width = 1024u64;
    let height = 1024u64;
    group.throughput(Throughput::Elements(width * height));

    let conversions = vec![
        (
            "uint8_to_float32",
            RasterDataType::UInt8,
            RasterDataType::Float32,
        ),
        (
            "uint16_to_float32",
            RasterDataType::UInt16,
            RasterDataType::Float32,
        ),
        (
            "float32_to_float64",
            RasterDataType::Float32,
            RasterDataType::Float64,
        ),
        (
            "float64_to_float32",
            RasterDataType::Float64,
            RasterDataType::Float32,
        ),
        (
            "int32_to_float32",
            RasterDataType::Int32,
            RasterDataType::Float32,
        ),
    ];

    for (name, from_type, to_type) in conversions {
        let buffer = RasterBuffer::zeros(width, height, from_type);

        group.bench_with_input(
            BenchmarkId::from_parameter(name),
            &(buffer, to_type),
            |b, (buf, to_type)| {
                b.iter(|| {
                    black_box(buf.convert_to(black_box(*to_type)).ok());
                });
            },
        );
    }

    group.finish();
}

fn bench_sequential_access(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer/sequential_access");

    let width = 2048u64;
    let height = 2048u64;
    let pixel_count = width * height;
    group.throughput(Throughput::Elements(pixel_count));

    let buffer = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    group.bench_function("row_major_read", |b| {
        b.iter(|| {
            for y in 0..height {
                for x in 0..width {
                    black_box(buffer.get_pixel(x, y).ok());
                }
            }
        });
    });

    group.bench_function("column_major_read", |b| {
        b.iter(|| {
            for x in 0..width {
                for y in 0..height {
                    black_box(buffer.get_pixel(x, y).ok());
                }
            }
        });
    });

    group.finish();
}

fn bench_random_access(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer/random_access");

    let width = 2048u64;
    let height = 2048u64;
    let buffer = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    // Pre-generate random positions
    let positions: Vec<(u64, u64)> = (0..10000)
        .map(|i| {
            let x = (i * 13 + 7) % width;
            let y = (i * 17 + 11) % height;
            (x, y)
        })
        .collect();

    group.throughput(Throughput::Elements(positions.len() as u64));

    group.bench_function("read", |b| {
        b.iter(|| {
            for (x, y) in &positions {
                black_box(buffer.get_pixel(*x, *y).ok());
            }
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_buffer_creation,
    bench_fill_value,
    bench_get_pixel,
    bench_set_pixel,
    bench_get_set_roundtrip,
    bench_statistics,
    bench_statistics_with_nodata,
    bench_convert_type,
    bench_sequential_access,
    bench_random_access
);
criterion_main!(benches);
