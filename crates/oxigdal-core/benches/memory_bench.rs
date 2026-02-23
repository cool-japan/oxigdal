//! Benchmarks for memory module
#![allow(
    missing_docs,
    clippy::expect_used,
    clippy::panic,
    clippy::unit_arg,
    unsafe_code
)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_core::memory::*;
use std::hint::black_box;

fn bench_slab_allocator(c: &mut Criterion) {
    let mut group = c.benchmark_group("slab_allocator");

    for size in [256, 1024, 4096, 16384].iter() {
        group.throughput(Throughput::Bytes(*size as u64));

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let allocator = SlabAllocator::new();
            b.iter(|| {
                let ptr = allocator.allocate(size).expect("slab allocation failed");
                black_box(&ptr);
                allocator
                    .deallocate(ptr, size)
                    .expect("slab deallocation failed");
            });
        });
    }

    group.finish();
}

fn bench_buddy_allocator(c: &mut Criterion) {
    let mut group = c.benchmark_group("buddy_allocator");

    for size in [1024, 4096, 16384, 65536].iter() {
        group.throughput(Throughput::Bytes(*size as u64));

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let allocator =
                BuddyAllocator::with_defaults().expect("buddy allocator creation failed");
            b.iter(|| {
                let ptr = allocator.allocate(size).expect("buddy allocation failed");
                black_box(&ptr);
                allocator
                    .deallocate(ptr, size)
                    .expect("buddy deallocation failed");
            });
        });
    }

    group.finish();
}

fn bench_shared_buffer(c: &mut Criterion) {
    let mut group = c.benchmark_group("shared_buffer");

    group.bench_function("create", |b| {
        b.iter(|| {
            let buffer = SharedBuffer::new(4096).expect("shared buffer creation failed");
            black_box(&buffer);
        });
    });

    group.bench_function("share", |b| {
        let buffer = SharedBuffer::new(4096).expect("shared buffer creation failed");
        b.iter(|| {
            let shared = buffer.share();
            black_box(&shared);
        });
    });

    group.bench_function("cow", |b| {
        b.iter(|| {
            let mut buffer = SharedBuffer::new(4096).expect("shared buffer creation failed");
            let _shared = buffer.share();
            let slice = buffer.as_mut_slice().expect("COW slice failed");
            slice[0] = 42;
            black_box(&buffer);
        });
    });

    group.finish();
}

fn bench_zero_copy_buffer(c: &mut Criterion) {
    let mut group = c.benchmark_group("zero_copy_buffer");

    group.bench_function("create_u32", |b| {
        b.iter(|| {
            let buffer: ZeroCopyBuffer<u32> =
                ZeroCopyBuffer::new(1024).expect("zero-copy buffer creation failed");
            black_box(&buffer);
        });
    });

    group.bench_function("read", |b| {
        let buffer: ZeroCopyBuffer<u32> =
            ZeroCopyBuffer::new(1024).expect("zero-copy buffer creation failed");
        b.iter(|| {
            let slice = buffer.as_slice();
            black_box(slice[512]);
        });
    });

    group.bench_function("write", |b| {
        let mut buffer: ZeroCopyBuffer<u32> =
            ZeroCopyBuffer::new(1024).expect("zero-copy buffer creation failed");
        b.iter(|| {
            let slice = buffer.as_mut_slice().expect("zero-copy mut slice failed");
            slice[512] = 42;
        });
    });

    group.finish();
}

fn bench_arena(c: &mut Criterion) {
    let mut group = c.benchmark_group("arena");

    group.bench_function("allocate_small", |b| {
        let arena = Arena::with_capacity(1024 * 1024).expect("arena creation failed");
        b.iter(|| {
            arena.reset();
            for _ in 0..100 {
                let ptr = arena.allocate(256).expect("arena allocation failed");
                black_box(&ptr);
            }
        });
    });

    group.bench_function("allocate_large", |b| {
        let arena = Arena::with_capacity(16 * 1024 * 1024).expect("arena creation failed");
        b.iter(|| {
            arena.reset();
            for _ in 0..10 {
                let ptr = arena.allocate(65536).expect("arena allocation failed");
                black_box(&ptr);
            }
        });
    });

    group.bench_function("reset", |b| {
        let arena = Arena::with_capacity(1024 * 1024).expect("arena creation failed");
        for _ in 0..100 {
            arena.allocate(4096).expect("arena allocation failed");
        }
        b.iter(|| {
            arena.reset();
        });
    });

    group.finish();
}

fn bench_pool(c: &mut Criterion) {
    let mut group = c.benchmark_group("pool");

    group.bench_function("allocate_4kb", |b| {
        let pool = Pool::new().expect("pool creation failed");
        b.iter(|| {
            let buffer = pool.allocate(4096).expect("pool allocation failed");
            black_box(&buffer);
        });
    });

    group.bench_function("allocate_reuse", |b| {
        let pool = Pool::new().expect("pool creation failed");
        // Prime the pool
        {
            let _buffer = pool.allocate(4096).expect("pool allocation failed");
        }

        b.iter(|| {
            let buffer = pool.allocate(4096).expect("pool allocation failed");
            black_box(&buffer);
        });
    });

    group.bench_function("allocate_multiple_sizes", |b| {
        let pool = Pool::new().expect("pool creation failed");
        b.iter(|| {
            let _b1 = pool.allocate(1024).expect("pool allocation failed");
            let _b2 = pool.allocate(4096).expect("pool allocation failed");
            let _b3 = pool.allocate(16384).expect("pool allocation failed");
        });
    });

    group.finish();
}

fn bench_mmap(c: &mut Criterion) {
    use std::io::Write;

    let mut group = c.benchmark_group("mmap");

    // Create test file
    let mut file = tempfile::NamedTempFile::new().expect("temp file creation failed");
    let data = vec![0u8; 16 * 1024 * 1024]; // 16MB
    file.write_all(&data).expect("file write failed");
    file.flush().expect("file flush failed");
    let path = file.path().to_path_buf();

    group.bench_function("map_readonly", |b| {
        b.iter(|| {
            let map = MemoryMap::new(&path).expect("memory map creation failed");
            black_box(&map);
        });
    });

    group.bench_function("read_sequential", |b| {
        let map = MemoryMap::new(&path).expect("memory map creation failed");
        b.iter(|| {
            let slice = map.as_slice();
            let mut sum = 0u64;
            for &byte in slice.iter().step_by(4096) {
                sum = sum.wrapping_add(byte as u64);
            }
            black_box(sum);
        });
    });

    group.bench_function("prefetch", |b| {
        let map = MemoryMap::new(&path).expect("memory map creation failed");
        b.iter(|| {
            map.prefetch(0, 1024 * 1024).expect("prefetch failed");
        });
    });

    group.finish();
}

fn bench_numa(c: &mut Criterion) {
    let mut group = c.benchmark_group("numa");

    group.bench_function("allocate", |b| {
        let allocator = NumaAllocator::new().expect("NUMA allocator creation failed");
        b.iter(|| {
            let ptr = allocator.allocate(4096).expect("NUMA allocation failed");
            black_box(&ptr);
            allocator
                .deallocate(ptr, 4096)
                .expect("NUMA deallocation failed");
        });
    });

    group.finish();
}

fn bench_huge_pages(c: &mut Criterion) {
    let mut group = c.benchmark_group("huge_pages");

    let config = HugePageConfig::new().with_fallback(true);

    group.bench_function("allocate_2mb", |b| {
        let allocator = HugePageAllocator::with_config(config.clone())
            .expect("huge page allocator creation failed");
        b.iter(|| {
            let size = 2 * 1024 * 1024;
            let ptr = allocator
                .allocate(size)
                .expect("huge page allocation failed");
            black_box(&ptr);
            allocator
                .deallocate(ptr, size)
                .expect("huge page deallocation failed");
        });
    });

    group.finish();
}

fn bench_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("allocator_comparison");

    let size = 4096;

    group.bench_function("std_alloc", |b| {
        b.iter(|| {
            let layout =
                std::alloc::Layout::from_size_align(size, 16).expect("layout creation failed");
            unsafe {
                let ptr = std::alloc::alloc(layout);
                black_box(&ptr);
                std::alloc::dealloc(ptr, layout);
            }
        });
    });

    group.bench_function("slab_alloc", |b| {
        let allocator = SlabAllocator::new();
        b.iter(|| {
            let ptr = allocator.allocate(size).expect("slab allocation failed");
            black_box(&ptr);
            allocator
                .deallocate(ptr, size)
                .expect("slab deallocation failed");
        });
    });

    group.bench_function("buddy_alloc", |b| {
        let allocator = BuddyAllocator::with_defaults().expect("buddy allocator creation failed");
        b.iter(|| {
            let ptr = allocator.allocate(size).expect("buddy allocation failed");
            black_box(&ptr);
            allocator
                .deallocate(ptr, size)
                .expect("buddy deallocation failed");
        });
    });

    group.bench_function("pool_alloc", |b| {
        let pool = Pool::new().expect("pool creation failed");
        b.iter(|| {
            let buffer = pool.allocate(size).expect("pool allocation failed");
            black_box(&buffer);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_slab_allocator,
    bench_buddy_allocator,
    bench_shared_buffer,
    bench_zero_copy_buffer,
    bench_arena,
    bench_pool,
    bench_mmap,
    bench_numa,
    bench_huge_pages,
    bench_comparison,
);
criterion_main!(benches);
