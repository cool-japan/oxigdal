//! Memory pool performance benchmarks.
#![allow(
    missing_docs,
    clippy::expect_used,
    clippy::panic,
    clippy::unit_arg,
    clippy::manual_div_ceil,
    clippy::useless_vec,
    unused_variables
)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::collections::BTreeMap;
use std::hint::black_box;
use std::time::Duration;

fn bench_alignment_calculation(c: &mut Criterion) {
    let mut group = c.benchmark_group("alignment");

    for alignment in [16u64, 64, 256, 1024, 4096].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(alignment),
            alignment,
            |b, &alignment| {
                b.iter(|| {
                    let sizes = [100u64, 1000, 10000, 100000];
                    for &size in &sizes {
                        let aligned = ((size + alignment - 1) / alignment) * alignment;
                        black_box(aligned);
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_block_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("block_lookup");
    group.measurement_time(Duration::from_secs(5));

    // Setup test data
    let mut blocks = BTreeMap::new();
    for i in 0..100 {
        blocks.insert(i * 1024u64, i as usize);
    }

    group.bench_function("btreemap_get", |b| {
        b.iter(|| {
            let key = black_box(50 * 1024u64);
            let value = blocks.get(&key);
            black_box(value)
        });
    });

    group.bench_function("btreemap_range", |b| {
        b.iter(|| {
            let key = black_box(50 * 1024u64);
            let range = blocks.range(..key);
            black_box(range.last())
        });
    });

    group.finish();
}

fn bench_fragmentation_calculation(c: &mut Criterion) {
    let mut group = c.benchmark_group("fragmentation");

    let test_cases = vec![
        ("no_fragmentation", vec![1024u64]),
        ("low_fragmentation", vec![512, 256, 256]),
        ("high_fragmentation", vec![128, 64, 64, 32, 32, 16, 16, 16]),
    ];

    for (name, free_blocks) in test_cases {
        group.bench_function(name, |b| {
            b.iter(|| {
                let total_free: u64 = black_box(&free_blocks).iter().sum();
                let largest = black_box(&free_blocks).iter().max().copied().unwrap_or(0);

                let fragmentation = if total_free == 0 {
                    0.0
                } else {
                    1.0 - (largest as f64 / total_free as f64)
                };

                black_box(fragmentation)
            });
        });
    }

    group.finish();
}

fn bench_coalescing(c: &mut Criterion) {
    let mut group = c.benchmark_group("coalescing");
    group.measurement_time(Duration::from_secs(10));

    group.bench_function("find_adjacent", |b| {
        let mut blocks = BTreeMap::new();
        for i in 0..10 {
            blocks.insert(i * 1024u64, (i * 1024, 1024, true)); // (offset, size, is_free)
        }

        b.iter(|| {
            let mut adjacent = Vec::new();
            let mut prev_offset: Option<u64> = None;

            for (offset, (block_offset, size, is_free)) in black_box(&blocks).iter() {
                if *is_free {
                    if let Some(prev_off) = prev_offset {
                        if let Some((_, prev_size, prev_free)) = blocks.get(&prev_off) {
                            if *prev_free && prev_off + prev_size == *offset {
                                adjacent.push(*offset);
                            }
                        }
                    }
                    prev_offset = Some(*offset);
                } else {
                    prev_offset = None;
                }
            }

            black_box(adjacent)
        });
    });

    group.finish();
}

fn bench_allocation_strategy(c: &mut Criterion) {
    let mut group = c.benchmark_group("allocation_strategy");

    // First-fit benchmark
    group.bench_function("first_fit", |b| {
        let free_blocks = vec![(0u64, 1024u64), (2048, 512), (4096, 2048), (8192, 256)];

        b.iter(|| {
            let required_size = black_box(1000u64);
            let result = free_blocks.iter().find(|(_, size)| *size >= required_size);
            black_box(result)
        });
    });

    // Best-fit benchmark
    group.bench_function("best_fit", |b| {
        let free_blocks = vec![(0u64, 1024u64), (2048, 512), (4096, 2048), (8192, 256)];

        b.iter(|| {
            let required_size = black_box(1000u64);
            let result = free_blocks
                .iter()
                .filter(|(_, size)| *size >= required_size)
                .min_by_key(|(_, size)| *size);
            black_box(result)
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_alignment_calculation,
    bench_block_lookup,
    bench_fragmentation_calculation,
    bench_coalescing,
    bench_allocation_strategy,
);
criterion_main!(benches);
