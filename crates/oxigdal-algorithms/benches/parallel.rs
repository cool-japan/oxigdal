//! Benchmarks for parallel processing
//!
//! This benchmark suite compares parallel vs single-threaded implementations
//! and measures scaling behavior with different thread counts.
#![allow(
    missing_docs,
    clippy::expect_used,
    clippy::panic,
    clippy::unit_arg,
    clippy::unnecessary_cast
)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_algorithms::parallel::*;
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use std::hint::black_box;

/// Benchmark parallel map vs sequential
fn bench_parallel_map(c: &mut Criterion) {
    let mut group = c.benchmark_group("parallel_map");

    for size in [100, 1000, 2000, 4000].iter() {
        let width = *size;
        let height = *size;
        let total_pixels = width * height;

        group.throughput(Throughput::Elements(total_pixels as u64));

        let input = RasterBuffer::zeros(width, height, RasterDataType::Float32);

        // Sequential baseline
        group.bench_with_input(
            BenchmarkId::new("sequential", size),
            size,
            |bench, &_size| {
                bench.iter(|| {
                    let mut output = RasterBuffer::zeros(width, height, RasterDataType::Float32);
                    for y in 0..height {
                        for x in 0..width {
                            let value = input.get_pixel(x, y).ok().unwrap_or(0.0);
                            let result = value * 2.0 + 1.0;
                            output.set_pixel(x, y, result).ok();
                        }
                    }
                    black_box(output)
                });
            },
        );

        // Parallel version
        group.bench_with_input(BenchmarkId::new("parallel", size), size, |bench, &_size| {
            bench.iter(|| {
                let output = parallel_map_raster(black_box(&input), |pixel| pixel * 2.0 + 1.0).ok();
                black_box(output)
            });
        });
    }

    group.finish();
}

/// Benchmark parallel reduce operations
fn bench_parallel_reduce(c: &mut Criterion) {
    let mut group = c.benchmark_group("parallel_reduce");

    for size in [100, 1000, 2000, 4000].iter() {
        let width = *size;
        let height = *size;
        let total_pixels = width * height;

        group.throughput(Throughput::Elements(total_pixels as u64));

        let mut input = RasterBuffer::zeros(width, height, RasterDataType::Float32);

        // Fill with test data
        for y in 0..height {
            for x in 0..width {
                input.set_pixel(x, y, (x + y) as f64).ok();
            }
        }

        // Sum
        group.bench_with_input(
            BenchmarkId::new("sum_sequential", size),
            size,
            |bench, &_size| {
                bench.iter(|| {
                    let mut sum = 0.0;
                    for y in 0..height {
                        for x in 0..width {
                            if let Ok(value) = input.get_pixel(x, y) {
                                sum += value;
                            }
                        }
                    }
                    black_box(sum)
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("sum_parallel", size),
            size,
            |bench, &_size| {
                bench.iter(|| {
                    let result = parallel_reduce_raster(black_box(&input), ReduceOp::Sum).ok();
                    black_box(result)
                });
            },
        );

        // Min/Max
        group.bench_with_input(
            BenchmarkId::new("minmax_sequential", size),
            size,
            |bench, &_size| {
                bench.iter(|| {
                    let mut min = f64::MAX;
                    let mut max = f64::MIN;
                    for y in 0..height {
                        for x in 0..width {
                            if let Ok(value) = input.get_pixel(x, y) {
                                min = min.min(value);
                                max = max.max(value);
                            }
                        }
                    }
                    black_box((min, max))
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("min_parallel", size),
            size,
            |bench, &_size| {
                bench.iter(|| {
                    let result = parallel_reduce_raster(black_box(&input), ReduceOp::Min).ok();
                    black_box(result)
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("max_parallel", size),
            size,
            |bench, &_size| {
                bench.iter(|| {
                    let result = parallel_reduce_raster(black_box(&input), ReduceOp::Max).ok();
                    black_box(result)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark thread scaling
fn bench_thread_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("thread_scaling");

    let width = 2000u64;
    let height = 2000u64;
    let total_pixels = width * height;

    group.throughput(Throughput::Elements(total_pixels));

    let input = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    // Test with different thread counts
    for threads in [1, 2, 4, 8].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(threads),
            threads,
            |bench, &thread_count| {
                bench.iter(|| {
                    let config = ChunkConfig::new().with_threads(thread_count);
                    let output =
                        parallel_map_raster_with_config(black_box(&input), &config, |pixel| {
                            pixel * 2.0 + 1.0
                        })
                        .ok();
                    black_box(output)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark chunk size impact
fn bench_chunk_size(c: &mut Criterion) {
    let mut group = c.benchmark_group("chunk_size");

    let width = 2000u64;
    let height = 2000u64;
    let total_pixels = width * height;

    group.throughput(Throughput::Elements(total_pixels));

    let input = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    for chunk_size in [1024, 4096, 16384, 65536].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(chunk_size),
            chunk_size,
            |bench, &size| {
                bench.iter(|| {
                    let config = ChunkConfig::new().with_chunk_size(size);
                    let output =
                        parallel_map_raster_with_config(black_box(&input), &config, |pixel| {
                            pixel * 2.0 + 1.0
                        })
                        .ok();
                    black_box(output)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark parallel focal operations
fn bench_parallel_focal(c: &mut Criterion) {
    let mut group = c.benchmark_group("parallel_focal");

    for size in [100, 500, 1000].iter() {
        let width = *size;
        let height = *size;
        let total_pixels = width * height;

        group.throughput(Throughput::Elements(total_pixels as u64));

        let mut input = RasterBuffer::zeros(width, height, RasterDataType::Float32);

        // Fill with test data
        for y in 0..height {
            for x in 0..width {
                input.set_pixel(x, y, (x % 10) as f64).ok();
            }
        }

        // Focal mean with 3x3 window
        group.bench_with_input(BenchmarkId::new("mean_3x3", size), size, |bench, &_size| {
            bench.iter(|| {
                let output = parallel_focal_mean(black_box(&input), 3).ok();
                black_box(output)
            });
        });

        // Focal median with 3x3 window
        group.bench_with_input(
            BenchmarkId::new("median_3x3", size),
            size,
            |bench, &_size| {
                bench.iter(|| {
                    let output = parallel_focal_median(black_box(&input), 3).ok();
                    black_box(output)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark parallel overview generation
fn bench_parallel_overviews(c: &mut Criterion) {
    let mut group = c.benchmark_group("parallel_overviews");

    for size in [1024, 2048, 4096].iter() {
        let width = *size;
        let height = *size;

        group.throughput(Throughput::Elements((width * height) as u64));

        let input = RasterBuffer::zeros(width, height, RasterDataType::UInt8);

        // Generate 4 overview levels
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |bench, &_size| {
            bench.iter(|| {
                let overviews = parallel_generate_overviews(
                    black_box(&input),
                    &[2, 4, 8, 16],
                    oxigdal_algorithms::resampling::ResamplingMethod::Nearest,
                )
                .ok();
                black_box(overviews)
            });
        });
    }

    group.finish();
}

/// Benchmark parallel batch processing
fn bench_parallel_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("parallel_batch");

    for item_count in [10, 50, 100, 200].iter() {
        group.throughput(Throughput::Elements(*item_count as u64));

        let items: Vec<u64> = (0..*item_count).collect();

        // Sequential baseline
        group.bench_with_input(
            BenchmarkId::new("sequential", item_count),
            item_count,
            |bench, &_count| {
                bench.iter(|| {
                    let results: Vec<u64> = items
                        .iter()
                        .map(|&x| {
                            // Simulate some work
                            let mut sum = 0u64;
                            for i in 0..1000 {
                                sum = sum.wrapping_add(x.wrapping_mul(i));
                            }
                            sum
                        })
                        .collect();
                    black_box(results)
                });
            },
        );

        // Parallel version
        group.bench_with_input(
            BenchmarkId::new("parallel", item_count),
            item_count,
            |bench, &_count| {
                bench.iter(|| {
                    let result = parallel_map(black_box(&items), |&x| {
                        // Simulate some work
                        let mut sum = 0u64;
                        for i in 0..1000 {
                            sum = sum.wrapping_add(x.wrapping_mul(i));
                        }
                        Ok(sum)
                    })
                    .ok();
                    black_box(result)
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_parallel_map,
    bench_parallel_reduce,
    bench_thread_scaling,
    bench_chunk_size,
    bench_parallel_focal,
    bench_parallel_overviews,
    bench_parallel_batch
);
criterion_main!(benches);
