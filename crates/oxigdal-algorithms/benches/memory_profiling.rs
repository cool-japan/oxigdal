#![allow(
    missing_docs,
    clippy::expect_used,
    clippy::panic,
    clippy::unit_arg,
    dead_code,
    clippy::unnecessary_cast
)]
use criterion::{
    BenchmarkId, Criterion, Throughput, criterion_group, criterion_main, profiler::Profiler,
};
use std::hint::black_box;
use std::path::Path;

use oxigdal_algorithms::raster::{
    HillshadeParams, compute_statistics, compute_zonal_stats, gaussian_blur, hillshade,
};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

// Custom profiler for memory profiling with pprof
struct PprofProfiler {
    frequency: i32,
}

impl PprofProfiler {
    fn new() -> Self {
        PprofProfiler { frequency: 1000 }
    }
}

impl Profiler for PprofProfiler {
    fn start_profiling(&mut self, _benchmark_id: &str, _benchmark_dir: &Path) {
        // Profiling is started per-iteration in the benchmarks
    }

    fn stop_profiling(&mut self, _benchmark_id: &str, _benchmark_dir: &Path) {
        // Profiling is stopped per-iteration in the benchmarks
    }
}

// Helper function to create test raster data
fn create_test_raster(width: usize, height: usize) -> RasterBuffer {
    let mut buffer = RasterBuffer::zeros(width as u64, height as u64, RasterDataType::Float64);

    for i in 0..width * height {
        let x = i % width;
        let y = i / width;
        let value = (x as f64).sin() * 100.0 + (y as f64).cos() * 50.0 + 1000.0;
        let _ = buffer.set_pixel(x as u64, y as u64, value);
    }

    buffer
}

// Helper function to create test zones
fn create_test_zones(width: usize, height: usize) -> RasterBuffer {
    let mut buffer = RasterBuffer::zeros(width as u64, height as u64, RasterDataType::Int32);

    for i in 0..width * height {
        let x = i % width;
        let y = i / width;
        let zone_id = ((x / 10) + (y / 10) * (width / 10)) as f64;
        let _ = buffer.set_pixel(x as u64, y as u64, zone_id);
    }

    buffer
}

fn bench_memory_hillshade(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_hillshade");

    for size in [512, 1024, 2048].iter() {
        let dem = create_test_raster(*size, *size);

        group.throughput(Throughput::Bytes(
            (size * size * std::mem::size_of::<f64>()) as u64,
        ));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let _guard = pprof::ProfilerGuardBuilder::default()
                    .frequency(1000)
                    .blocklist(&["libc", "libgcc", "pthread", "vdso"])
                    .build();

                let params = HillshadeParams {
                    azimuth: black_box(315.0),
                    altitude: black_box(45.0),
                    z_factor: 1.0,
                    pixel_size: black_box(30.0),
                    scale: 255.0,
                };

                let _ = hillshade(black_box(&dem), params);
            });
        });
    }

    group.finish();
}

fn bench_memory_zonal_stats(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_zonal_stats");

    for size in [512, 1024, 2048].iter() {
        let raster = create_test_raster(*size, *size);
        let zones = create_test_zones(*size, *size);

        group.throughput(Throughput::Bytes(
            (size * size * std::mem::size_of::<f64>()) as u64,
        ));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let _guard = pprof::ProfilerGuardBuilder::default()
                    .frequency(1000)
                    .blocklist(&["libc", "libgcc", "pthread", "vdso"])
                    .build();

                let _ = compute_zonal_stats(black_box(&raster), black_box(&zones));
            });
        });
    }

    group.finish();
}

fn bench_memory_gaussian_blur(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_gaussian_blur");

    for size in [512, 1024, 2048].iter() {
        let raster = create_test_raster(*size, *size);

        group.throughput(Throughput::Bytes(
            (size * size * std::mem::size_of::<f64>()) as u64,
        ));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let _guard = pprof::ProfilerGuardBuilder::default()
                    .frequency(1000)
                    .blocklist(&["libc", "libgcc", "pthread", "vdso"])
                    .build();

                let _ = gaussian_blur(black_box(&raster), black_box(1.5), Some(black_box(5)));
            });
        });
    }

    group.finish();
}

fn bench_memory_simd_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_simd_operations");

    for size in [512, 1024, 2048].iter() {
        let raster1 = create_test_raster(*size, *size);
        let raster2 = create_test_raster(*size, *size);

        group.throughput(Throughput::Bytes(
            (size * size * std::mem::size_of::<f64>() * 2) as u64,
        ));

        group.bench_with_input(BenchmarkId::new("add", size), size, |b, _| {
            b.iter(|| {
                let _guard = pprof::ProfilerGuardBuilder::default()
                    .frequency(1000)
                    .blocklist(&["libc", "libgcc", "pthread", "vdso"])
                    .build();

                let mut result = raster1.clone();
                for y in 0..raster1.height() {
                    for x in 0..raster1.width() {
                        if let (Ok(v1), Ok(v2)) = (
                            black_box(raster1.get_pixel(x, y)),
                            black_box(raster2.get_pixel(x, y)),
                        ) {
                            let _ = result.set_pixel(x, y, v1 + v2);
                        }
                    }
                }
            });
        });

        group.bench_with_input(BenchmarkId::new("multiply", size), size, |b, _| {
            b.iter(|| {
                let _guard = pprof::ProfilerGuardBuilder::default()
                    .frequency(1000)
                    .blocklist(&["libc", "libgcc", "pthread", "vdso"])
                    .build();

                let mut result = raster1.clone();
                for y in 0..raster1.height() {
                    for x in 0..raster1.width() {
                        if let (Ok(v1), Ok(v2)) = (
                            black_box(raster1.get_pixel(x, y)),
                            black_box(raster2.get_pixel(x, y)),
                        ) {
                            let _ = result.set_pixel(x, y, v1 * v2);
                        }
                    }
                }
            });
        });
    }

    group.finish();
}

fn bench_memory_allocation_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_allocation_patterns");

    // Test different allocation sizes
    for size in [256, 512, 1024, 2048, 4096].iter() {
        group.throughput(Throughput::Bytes(
            (size * size * std::mem::size_of::<f64>()) as u64,
        ));

        group.bench_with_input(BenchmarkId::new("allocate_vec", size), size, |b, _| {
            b.iter(|| {
                let _guard = pprof::ProfilerGuardBuilder::default()
                    .frequency(1000)
                    .blocklist(&["libc", "libgcc", "pthread", "vdso"])
                    .build();

                let _data: Vec<f64> = vec![0.0; black_box(*size * *size)];
            });
        });

        group.bench_with_input(
            BenchmarkId::new("allocate_with_capacity", size),
            size,
            |b, _| {
                b.iter(|| {
                    let _guard = pprof::ProfilerGuardBuilder::default()
                        .frequency(1000)
                        .blocklist(&["libc", "libgcc", "pthread", "vdso"])
                        .build();

                    let mut data: Vec<f64> = Vec::with_capacity(black_box(*size * *size));
                    data.resize(*size * *size, 0.0);
                });
            },
        );

        group.bench_with_input(BenchmarkId::new("allocate_and_fill", size), size, |b, _| {
            b.iter(|| {
                let _guard = pprof::ProfilerGuardBuilder::default()
                    .frequency(1000)
                    .blocklist(&["libc", "libgcc", "pthread", "vdso"])
                    .build();

                let data: Vec<f64> = (0..*size * *size).map(|i| black_box(i as f64)).collect();
                let _ = black_box(data);
            });
        });
    }

    group.finish();
}

fn bench_memory_clone_vs_copy(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_clone_vs_copy");

    for size in [512, 1024, 2048].iter() {
        let raster = create_test_raster(*size, *size);

        group.throughput(Throughput::Bytes(
            (size * size * std::mem::size_of::<f64>()) as u64,
        ));

        group.bench_with_input(BenchmarkId::new("clone", size), size, |b, _| {
            b.iter(|| {
                let _guard = pprof::ProfilerGuardBuilder::default()
                    .frequency(1000)
                    .blocklist(&["libc", "libgcc", "pthread", "vdso"])
                    .build();

                let _cloned = black_box(raster.clone());
            });
        });

        group.bench_with_input(BenchmarkId::new("deep_copy", size), size, |b, _| {
            b.iter(|| {
                let _guard = pprof::ProfilerGuardBuilder::default()
                    .frequency(1000)
                    .blocklist(&["libc", "libgcc", "pthread", "vdso"])
                    .build();

                let _copied = black_box(&raster).clone();
            });
        });
    }

    group.finish();
}

fn bench_memory_parallel_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_parallel_operations");

    for size in [1024, 2048].iter() {
        let raster = create_test_raster(*size, *size);

        group.throughput(Throughput::Bytes(
            (size * size * std::mem::size_of::<f64>()) as u64,
        ));

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let _guard = pprof::ProfilerGuardBuilder::default()
                    .frequency(1000)
                    .blocklist(&["libc", "libgcc", "pthread", "vdso"])
                    .build();

                // Element-wise transformation
                let mut result = black_box(&raster).clone();
                for y in 0..result.height() {
                    for x in 0..result.width() {
                        if let Ok(val) = raster.get_pixel(x, y) {
                            let _ = result.set_pixel(x, y, val * 2.0);
                        }
                    }
                }
            });
        });
    }

    group.finish();
}

fn bench_memory_peak_usage(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_peak_usage");

    // Test operations that create multiple temporary allocations
    let size = 1024;
    let raster = create_test_raster(size, size);

    group.throughput(Throughput::Bytes(
        (size * size * std::mem::size_of::<f64>()) as u64,
    ));

    group.bench_function("multi_step_processing", |b| {
        b.iter(|| {
            let _guard = pprof::ProfilerGuardBuilder::default()
                .frequency(1000)
                .blocklist(&["libc", "libgcc", "pthread", "vdso"])
                .build();

            // Simulate a multi-step processing pipeline
            if let Ok(step1) = gaussian_blur(black_box(&raster), 1.5, Some(3)) {
                if let Ok(step2) = gaussian_blur(&step1, 1.5, Some(5)) {
                    if let Ok(stats) = compute_statistics(&step2) {
                        let _ = black_box(stats);
                    }
                }
            }
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_memory_hillshade,
    bench_memory_zonal_stats,
    bench_memory_gaussian_blur,
    bench_memory_simd_operations,
    bench_memory_allocation_patterns,
    bench_memory_clone_vs_copy,
    bench_memory_parallel_operations,
    bench_memory_peak_usage,
);
criterion_main!(benches);
