//! Comprehensive GPU benchmarks covering all operations.
#![allow(missing_docs, clippy::expect_used, clippy::panic, clippy::unit_arg)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_gpu::*;
use std::hint::black_box;
use wgpu::BufferUsages;

fn gpu_context() -> Option<GpuContext> {
    pollster::block_on(GpuContext::new()).ok()
}

// Memory management benchmarks
fn bench_memory_pool(c: &mut Criterion) {
    let Some(context) = gpu_context() else {
        return;
    };

    let mut group = c.benchmark_group("memory_pool");

    for size in [1024, 4096, 16384, 65536] {
        let config = MemoryPoolConfig {
            initial_size: 64 * 1024 * 1024,
            max_size: 256 * 1024 * 1024,
            ..Default::default()
        };

        let mut pool = MemoryPool::new(&context, config).ok();

        if let Some(ref mut pool) = pool {
            group.throughput(Throughput::Bytes(size));

            group.bench_with_input(BenchmarkId::new("allocate", size), &size, |b, &size| {
                b.iter(|| {
                    if let Ok(alloc) = pool.allocate(size, 256) {
                        let _ = pool.free(alloc);
                    }
                });
            });
        }
    }

    group.finish();
}

// Staging buffer benchmarks
fn bench_staging_buffers(c: &mut Criterion) {
    let Some(context) = gpu_context() else {
        return;
    };

    let mut group = c.benchmark_group("staging_buffers");

    for size in [1024, 4096, 16384, 65536] {
        let mut manager = StagingBufferManager::new(&context, size, 10);

        group.throughput(Throughput::Bytes(size));

        group.bench_with_input(BenchmarkId::new("upload", size), &size, |b, _| {
            b.iter(|| {
                if let Ok(buffer) = manager.get_upload_buffer() {
                    manager.return_upload_buffer(buffer);
                }
            });
        });

        group.bench_with_input(BenchmarkId::new("download", size), &size, |b, _| {
            b.iter(|| {
                if let Ok(buffer) = manager.get_download_buffer() {
                    manager.return_download_buffer(buffer);
                }
            });
        });
    }

    group.finish();
}

// Pipeline benchmarks
fn bench_compute_pipeline(c: &mut Criterion) {
    let Some(context) = gpu_context() else {
        return;
    };

    let mut group = c.benchmark_group("compute_pipeline");

    for size in [256, 512, 1024, 2048] {
        let data: Vec<f32> = vec![1.0; size * size];

        group.throughput(Throughput::Elements((size * size) as u64));

        group.bench_with_input(BenchmarkId::new("chain_ops", size), &size, |b, &size| {
            b.iter(|| {
                if let Ok(pipeline) =
                    ComputePipeline::from_data(&context, &data, size as u32, size as u32)
                {
                    let result = pipeline
                        .add(5.0)
                        .and_then(|p| p.multiply(2.0))
                        .and_then(|p| p.clamp(0.0, 255.0));

                    if let Ok(pipeline) = result {
                        let _ = pipeline.finish();
                    }
                }
            });
        });
    }

    group.finish();
}

// Buffer transfer benchmarks
fn bench_buffer_transfers(c: &mut Criterion) {
    let Some(context) = gpu_context() else {
        return;
    };

    let mut group = c.benchmark_group("buffer_transfers");

    for size in [1024, 4096, 16384, 65536] {
        let data: Vec<f32> = vec![1.0; size];

        group.throughput(Throughput::Bytes((size * 4) as u64));

        group.bench_with_input(BenchmarkId::new("upload", size), &size, |b, _| {
            b.iter(|| {
                let _ = GpuBuffer::from_data(
                    &context,
                    &data,
                    BufferUsages::STORAGE | BufferUsages::COPY_SRC,
                );
            });
        });

        if let Ok(buffer) = GpuBuffer::from_data(
            &context,
            &data,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        ) {
            if let Ok(staging) = GpuBuffer::staging(&context, size) {
                let mut staging_mut = staging.clone();

                group.bench_with_input(BenchmarkId::new("download", size), &size, |b, _| {
                    b.iter(|| {
                        let _ = staging_mut.copy_from(&buffer);
                        let _ = pollster::block_on(staging.read());
                    });
                });
            }
        }
    }

    group.finish();
}

// Convolution benchmarks
fn bench_convolution(c: &mut Criterion) {
    let Some(context) = gpu_context() else {
        return;
    };

    let mut group = c.benchmark_group("convolution");

    for size in [256, 512, 1024] {
        let data: Vec<f32> = vec![1.0; size * size];

        group.throughput(Throughput::Elements((size * size) as u64));

        for sigma in [1.0, 2.0, 4.0] {
            let buffer = GpuBuffer::from_data(
                &context,
                &data,
                BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
            );

            if let Ok(buffer) = buffer {
                group.bench_with_input(
                    BenchmarkId::new(
                        format!("gaussian_blur_{}x{}_sigma{}", size, size, sigma),
                        size,
                    ),
                    &size,
                    |b, &size| {
                        b.iter(|| {
                            let _ =
                                gaussian_blur(&context, &buffer, size as u32, size as u32, sigma);
                        });
                    },
                );
            }
        }
    }

    group.finish();
}

// Resampling benchmarks
fn bench_resampling(c: &mut Criterion) {
    let Some(context) = gpu_context() else {
        return;
    };

    let mut group = c.benchmark_group("resampling");

    for input_size in [256, 512] {
        let data: Vec<f32> = vec![1.0; input_size * input_size];

        let buffer = GpuBuffer::from_data(
            &context,
            &data,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        );

        if let Ok(buffer) = buffer {
            for output_size in [512, 1024] {
                group.throughput(Throughput::Elements((output_size * output_size) as u64));

                for method in [
                    ResamplingMethod::NearestNeighbor,
                    ResamplingMethod::Bilinear,
                    ResamplingMethod::Bicubic,
                ] {
                    group.bench_with_input(
                        BenchmarkId::new(
                            format!(
                                "{:?}_{}x{}_to_{}x{}",
                                method, input_size, input_size, output_size, output_size
                            ),
                            output_size,
                        ),
                        &output_size,
                        |b, &output_size| {
                            b.iter(|| {
                                let _ = resize(
                                    &context,
                                    &buffer,
                                    input_size as u32,
                                    input_size as u32,
                                    output_size as u32,
                                    output_size as u32,
                                    method,
                                );
                            });
                        },
                    );
                }
            }
        }
    }

    group.finish();
}

// Statistics benchmarks
fn bench_statistics(c: &mut Criterion) {
    let Some(context) = gpu_context() else {
        return;
    };

    let mut group = c.benchmark_group("statistics");

    for size in [1024, 4096, 16384, 65536] {
        let data: Vec<f32> = (0..size).map(|i| (i % 100) as f32).collect();

        let buffer = GpuBuffer::from_data(
            &context,
            &data,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        );

        if let Ok(_buffer) = buffer {
            group.throughput(Throughput::Elements(size as u64));

            for op in [
                ReductionOp::Sum,
                ReductionOp::Min,
                ReductionOp::Max,
                ReductionOp::Product,
            ] {
                group.bench_with_input(
                    BenchmarkId::new(format!("{:?}", op), size),
                    &size,
                    |b, _| {
                        b.iter(|| {
                            if let Ok(pipeline) =
                                ComputePipeline::from_data(&context, &data, size as u32, 1)
                            {
                                let _ = pollster::block_on(pipeline.reduce(op));
                            }
                        });
                    },
                );
            }

            // Histogram benchmark
            group.bench_with_input(BenchmarkId::new("histogram", size), &size, |b, _| {
                b.iter(|| {
                    if let Ok(pipeline) =
                        ComputePipeline::from_data(&context, &data, size as u32, 1)
                    {
                        let _ = pollster::block_on(pipeline.histogram(100, 0.0, 100.0));
                    }
                });
            });
        }
    }

    group.finish();
}

// Multi-band operations benchmarks
fn bench_multiband_operations(c: &mut Criterion) {
    let Some(context) = gpu_context() else {
        return;
    };

    let mut group = c.benchmark_group("multiband");

    for size in [256, 512] {
        let bands: Vec<Vec<f32>> = vec![
            vec![10.0; size * size], // Red
            vec![20.0; size * size], // Green
            vec![30.0; size * size], // Blue
            vec![40.0; size * size], // NIR
        ];

        let raster = GpuRasterBuffer::from_bands(
            &context,
            size as u32,
            size as u32,
            &bands,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        );

        if let Ok(raster) = raster {
            group.throughput(Throughput::Elements((size * size) as u64));

            group.bench_with_input(BenchmarkId::new("ndvi", size), &size, |b, _| {
                b.iter(|| {
                    if let Ok(pipeline) = MultibandPipeline::new(&context, &raster) {
                        let _ = pipeline.ndvi();
                    }
                });
            });

            group.bench_with_input(BenchmarkId::new("map_all_bands", size), &size, |b, _| {
                b.iter(|| {
                    if let Ok(pipeline) = MultibandPipeline::new(&context, &raster) {
                        let _ = pipeline.map(|band| band.multiply(1.5));
                    }
                });
            });
        }
    }

    group.finish();
}

// VRAM budget management benchmarks
fn bench_vram_budget(c: &mut Criterion) {
    let mut group = c.benchmark_group("vram_budget");

    let manager = VramBudgetManager::new(1024 * 1024 * 1024); // 1 GB

    for size in [1024, 4096, 16384, 65536] {
        group.throughput(Throughput::Bytes(size));

        group.bench_with_input(
            BenchmarkId::new("allocate_free", size),
            &size,
            |b, &size| {
                b.iter(|| {
                    if let Ok(id) = manager.allocate(size) {
                        let _ = manager.free(id);
                    }
                });
            },
        );
    }

    group.finish();
}

// Scalar operations benchmarks
fn bench_scalar_operations(c: &mut Criterion) {
    let Some(context) = gpu_context() else {
        return;
    };

    let mut group = c.benchmark_group("scalar_ops");

    for size in [1024, 4096, 16384, 65536] {
        let data: Vec<f32> = vec![1.0; size];

        group.throughput(Throughput::Elements(size as u64));

        for (name, op) in [
            ("add", ScalarOp::Add(5.0)),
            ("multiply", ScalarOp::Multiply(2.0)),
            (
                "clamp",
                ScalarOp::Clamp {
                    min: 0.0,
                    max: 100.0,
                },
            ),
        ] {
            group.bench_with_input(BenchmarkId::new(name, size), &size, |b, _| {
                b.iter(|| {
                    if let Ok(pipeline) =
                        ComputePipeline::from_data(&context, &data, size as u32, 1)
                    {
                        let _ = pipeline.scalar(op);
                    }
                });
            });
        }
    }

    group.finish();
}

// Unary operations benchmarks
fn bench_unary_operations(c: &mut Criterion) {
    let Some(context) = gpu_context() else {
        return;
    };

    let mut group = c.benchmark_group("unary_ops");

    for size in [1024, 4096, 16384, 65536] {
        let data: Vec<f32> = vec![2.0; size];

        group.throughput(Throughput::Elements(size as u64));

        for (name, op) in [
            ("abs", UnaryOp::Abs),
            ("sqrt", UnaryOp::Sqrt),
            ("log", UnaryOp::Log),
            ("exp", UnaryOp::Exp),
        ] {
            group.bench_with_input(BenchmarkId::new(name, size), &size, |b, _| {
                b.iter(|| {
                    if let Ok(pipeline) =
                        ComputePipeline::from_data(&context, &data, size as u32, 1)
                    {
                        let _ = pipeline.unary(op);
                    }
                });
            });
        }
    }

    group.finish();
}

// Backend-specific benchmarks
fn bench_backend_detection(c: &mut Criterion) {
    let Some(context) = gpu_context() else {
        return;
    };

    let mut group = c.benchmark_group("backend_detection");

    group.bench_function("get_backend", |b| {
        b.iter(|| {
            black_box(context.backend());
        });
    });

    group.bench_function("check_features", |b| {
        b.iter(|| {
            let _ = black_box(backends::query_capabilities(context.backend()));
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_memory_pool,
    bench_staging_buffers,
    bench_compute_pipeline,
    bench_buffer_transfers,
    bench_convolution,
    bench_resampling,
    bench_statistics,
    bench_multiband_operations,
    bench_vram_budget,
    bench_scalar_operations,
    bench_unary_operations,
    bench_backend_detection,
);

criterion_main!(benches);
