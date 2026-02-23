//! Benchmarks for GPU operations.
#![allow(missing_docs, clippy::expect_used, clippy::panic, clippy::unit_arg)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_gpu::*;
use std::hint::black_box;
use wgpu::BufferUsages;

fn gpu_context() -> Option<GpuContext> {
    pollster::block_on(GpuContext::new()).ok()
}

fn bench_element_wise_operations(c: &mut Criterion) {
    let Some(context) = gpu_context() else {
        eprintln!("GPU not available, skipping GPU benchmarks");
        return;
    };

    let mut group = c.benchmark_group("element_wise");

    for size in [1024, 4096, 16384, 65536] {
        let data_a: Vec<f32> = (0..size).map(|i| i as f32).collect();
        let data_b: Vec<f32> = (0..size).map(|i| (i * 2) as f32).collect();

        let buffer_a = GpuBuffer::from_data(
            &context,
            &data_a,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        )
        .ok();
        let buffer_b = GpuBuffer::from_data(
            &context,
            &data_b,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        )
        .ok();

        if let (Some(buffer_a), Some(buffer_b)) = (buffer_a, buffer_b) {
            group.throughput(Throughput::Elements(size as u64));

            group.bench_with_input(BenchmarkId::new("add", size), &size, |b, _| {
                b.iter(|| {
                    let kernel = RasterKernel::new(&context, ElementWiseOp::Add).ok();
                    if let Some(kernel) = kernel {
                        let mut output = GpuBuffer::new(
                            &context,
                            size as usize,
                            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
                        )
                        .ok();
                        if let Some(ref mut output) = output {
                            let _ = kernel.execute(&buffer_a, &buffer_b, output);
                            context.poll(true);
                        }
                    }
                });
            });

            group.bench_with_input(BenchmarkId::new("multiply", size), &size, |b, _| {
                b.iter(|| {
                    let kernel = RasterKernel::new(&context, ElementWiseOp::Multiply).ok();
                    if let Some(kernel) = kernel {
                        let mut output = GpuBuffer::new(
                            &context,
                            size as usize,
                            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
                        )
                        .ok();
                        if let Some(ref mut output) = output {
                            let _ = kernel.execute(&buffer_a, &buffer_b, output);
                            context.poll(true);
                        }
                    }
                });
            });
        }
    }

    group.finish();
}

fn bench_resampling(c: &mut Criterion) {
    let Some(context) = gpu_context() else {
        eprintln!("GPU not available, skipping GPU benchmarks");
        return;
    };

    let mut group = c.benchmark_group("resampling");

    let sizes = [(512, 256), (1024, 512), (2048, 1024)];

    for (src_size, dst_size) in sizes {
        let data: Vec<f32> = (0..(src_size * src_size)).map(|i| i as f32).collect();

        let buffer = GpuBuffer::from_data(
            &context,
            &data,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        )
        .ok();

        if let Some(buffer) = buffer {
            let elements = (dst_size * dst_size) as u64;
            group.throughput(Throughput::Elements(elements));

            let label = format!("{}x{}_to_{}x{}", src_size, src_size, dst_size, dst_size);

            group.bench_with_input(
                BenchmarkId::new("nearest_neighbor", &label),
                &label,
                |b, _| {
                    b.iter(|| {
                        let _ = resize(
                            &context,
                            &buffer,
                            src_size,
                            src_size,
                            dst_size,
                            dst_size,
                            ResamplingMethod::NearestNeighbor,
                        );
                        context.poll(true);
                    });
                },
            );

            group.bench_with_input(BenchmarkId::new("bilinear", &label), &label, |b, _| {
                b.iter(|| {
                    let _ = resize(
                        &context,
                        &buffer,
                        src_size,
                        src_size,
                        dst_size,
                        dst_size,
                        ResamplingMethod::Bilinear,
                    );
                    context.poll(true);
                });
            });

            group.bench_with_input(BenchmarkId::new("bicubic", &label), &label, |b, _| {
                b.iter(|| {
                    let _ = resize(
                        &context,
                        &buffer,
                        src_size,
                        src_size,
                        dst_size,
                        dst_size,
                        ResamplingMethod::Bicubic,
                    );
                    context.poll(true);
                });
            });
        }
    }

    group.finish();
}

fn bench_convolution(c: &mut Criterion) {
    let Some(context) = gpu_context() else {
        eprintln!("GPU not available, skipping GPU benchmarks");
        return;
    };

    let mut group = c.benchmark_group("convolution");

    for size in [256, 512, 1024] {
        let data: Vec<f32> = (0..(size * size)).map(|i| i as f32).collect();

        let buffer = GpuBuffer::from_data(
            &context,
            &data,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        )
        .ok();

        if let Some(buffer) = buffer {
            group.throughput(Throughput::Elements((size * size) as u64));

            group.bench_with_input(
                BenchmarkId::new("gaussian_blur_3x3", size),
                &size,
                |b, _| {
                    b.iter(|| {
                        let _ = gaussian_blur(&context, &buffer, size, size, 1.0);
                        context.poll(true);
                    });
                },
            );

            group.bench_with_input(
                BenchmarkId::new("gaussian_blur_5x5", size),
                &size,
                |b, _| {
                    b.iter(|| {
                        let _ = gaussian_blur(&context, &buffer, size, size, 2.0);
                        context.poll(true);
                    });
                },
            );
        }
    }

    group.finish();
}

fn bench_statistics(c: &mut Criterion) {
    let Some(context) = gpu_context() else {
        eprintln!("GPU not available, skipping GPU benchmarks");
        return;
    };

    let mut group = c.benchmark_group("statistics");

    for size in [1024, 16384, 65536, 262144] {
        let data: Vec<f32> = (0..size).map(|i| i as f32).collect();

        let buffer = GpuBuffer::from_data(
            &context,
            &data,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        )
        .ok();

        if let Some(buffer) = buffer {
            group.throughput(Throughput::Elements(size as u64));

            group.bench_with_input(BenchmarkId::new("sum", size), &size, |b, _| {
                let data_vec: Vec<f32> = (0..size).map(|i| i as f32).collect();
                b.iter(|| {
                    if let Ok(pipeline) =
                        ComputePipeline::from_data(&context, &data_vec, size as u32, 1)
                    {
                        let _ = pollster::block_on(pipeline.reduce(ReductionOp::Sum));
                    }
                });
            });

            group.bench_with_input(BenchmarkId::new("min_max", size), &size, |b, _| {
                b.iter(|| {
                    let _ = pollster::block_on(compute_statistics(&context, &buffer));
                });
            });

            group.bench_with_input(BenchmarkId::new("histogram", size), &size, |b, _| {
                let data_vec: Vec<f32> = (0..size).map(|i| i as f32).collect();
                b.iter(|| {
                    if let Ok(pipeline) =
                        ComputePipeline::from_data(&context, &data_vec, size as u32, 1)
                    {
                        let _ = pollster::block_on(pipeline.histogram(256, 0.0, size as f32));
                    }
                });
            });
        }
    }

    group.finish();
}

fn bench_pipeline(c: &mut Criterion) {
    let Some(context) = gpu_context() else {
        eprintln!("GPU not available, skipping GPU benchmarks");
        return;
    };

    let mut group = c.benchmark_group("pipeline");

    for size in [256, 512, 1024] {
        let data: Vec<f32> = vec![1.0; (size * size) as usize];

        group.throughput(Throughput::Elements((size * size) as u64));

        group.bench_with_input(BenchmarkId::new("chain_3_ops", size), &size, |b, _| {
            b.iter(|| {
                if let Ok(pipeline) = ComputePipeline::from_data(&context, &data, size, size) {
                    let result = pipeline
                        .add(10.0)
                        .and_then(|p| p.multiply(2.0))
                        .and_then(|p| p.clamp(0.0, 100.0));

                    if let Ok(result) = result {
                        black_box(result.finish());
                    }
                }
            });
        });

        group.bench_with_input(BenchmarkId::new("chain_with_blur", size), &size, |b, _| {
            b.iter(|| {
                if let Ok(pipeline) = ComputePipeline::from_data(&context, &data, size, size) {
                    let result = pipeline
                        .gaussian_blur(2.0)
                        .and_then(|p| p.multiply(1.5))
                        .and_then(|p| p.clamp(0.0, 255.0));

                    if let Ok(result) = result {
                        black_box(result.finish());
                    }
                }
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_element_wise_operations,
    bench_resampling,
    bench_convolution,
    bench_statistics,
    bench_pipeline
);
criterion_main!(benches);
