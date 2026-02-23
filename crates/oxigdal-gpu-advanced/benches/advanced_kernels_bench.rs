//! Benchmarks for advanced GPU kernels
#![allow(missing_docs, clippy::expect_used, clippy::unit_arg)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use oxigdal_gpu_advanced::{AdaptiveSelector, WorkloadInfo};
use std::hint::black_box;
use std::sync::Arc;
use std::time::Duration;

fn benchmark_adaptive_selection(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");

    rt.block_on(async {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let adapter = match instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
        {
            Ok(adapter) => adapter,
            Err(_) => {
                println!("No GPU available, skipping benchmarks");
                return;
            }
        };

        let info = adapter.get_info();

        let (device, _queue) = match adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("bench_device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                experimental_features: Default::default(),
                trace: Default::default(),
            })
            .await
        {
            Ok((device, queue)) => (device, queue),
            Err(e) => {
                println!("Failed to request device: {}", e);
                return;
            }
        };

        let device = Arc::new(device);
        let selector = AdaptiveSelector::new(device, info.device_type);

        let mut group = c.benchmark_group("adaptive_selection");

        // Benchmark different workload sizes
        for size in [1024, 4096, 16384, 65536].iter() {
            let workload = WorkloadInfo {
                data_size: size * size * 4,
                dimensions: vec![(*size) as u32, (*size) as u32],
                element_size: 4,
            };

            group.bench_with_input(
                BenchmarkId::new("matmul", size),
                &workload,
                |b, workload| {
                    b.iter(|| black_box(selector.select_algorithm("matrix_multiply", workload)));
                },
            );

            group.bench_with_input(
                BenchmarkId::new("convolution", size),
                &workload,
                |b, workload| {
                    b.iter(|| black_box(selector.select_algorithm("convolution", workload)));
                },
            );
        }

        group.finish();
    });
}

fn benchmark_profiling_overhead(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");

    rt.block_on(async {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let adapter = match instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
        {
            Ok(adapter) => adapter,
            Err(_) => {
                println!("No GPU available, skipping benchmarks");
                return;
            }
        };

        let (device, queue) = match adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("bench_device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                experimental_features: Default::default(),
                trace: Default::default(),
            })
            .await
        {
            Ok((device, queue)) => (device, queue),
            Err(e) => {
                println!("Failed to request device: {}", e);
                return;
            }
        };

        let device = Arc::new(device);
        let queue = Arc::new(queue);

        use oxigdal_gpu_advanced::{GpuProfiler, ProfilingConfig};

        let config = ProfilingConfig::default();
        let profiler = match GpuProfiler::new(device, queue, config) {
            Ok(p) => p,
            Err(e) => {
                println!("Failed to create profiler: {}", e);
                return;
            }
        };

        c.bench_function("profiling_session", |b| {
            b.iter(|| {
                let session = profiler.begin_profile("test_kernel");
                black_box(session.end(1024, 8));
            });
        });

        c.bench_function("profiling_record", |b| {
            b.iter(|| {
                profiler.record_kernel_execution(
                    "test_kernel",
                    Duration::from_micros(100),
                    black_box(1024),
                    black_box(8),
                );
            });
        });
    });
}

fn benchmark_memory_compaction(c: &mut Criterion) {
    use oxigdal_gpu_advanced::{CompactionConfig, MemoryCompactor};

    let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");

    rt.block_on(async {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let adapter = match instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
        {
            Ok(adapter) => adapter,
            Err(_) => {
                println!("No GPU available, skipping benchmarks");
                return;
            }
        };

        let (device, queue) = match adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("bench_device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                experimental_features: Default::default(),
                trace: Default::default(),
            })
            .await
        {
            Ok((device, queue)) => (device, queue),
            Err(e) => {
                println!("Failed to request device: {}", e);
                return;
            }
        };

        let device = Arc::new(device);
        let queue = Arc::new(queue);

        let config = CompactionConfig::default();
        let compactor = MemoryCompactor::new(device, queue, config);

        // Register some allocations
        for i in 0..100 {
            compactor.register_allocation(i, i * 1024, 512, true);
        }

        c.bench_function("fragmentation_detection", |b| {
            b.iter(|| {
                black_box(compactor.detect_fragmentation());
            });
        });

        c.bench_function("needs_compaction", |b| {
            b.iter(|| {
                black_box(compactor.needs_compaction());
            });
        });
    });
}

criterion_group!(
    benches,
    benchmark_adaptive_selection,
    benchmark_profiling_overhead,
    benchmark_memory_compaction
);
criterion_main!(benches);
