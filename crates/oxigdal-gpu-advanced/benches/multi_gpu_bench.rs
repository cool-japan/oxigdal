//! Multi-GPU performance benchmarks.
#![allow(missing_docs, clippy::expect_used, clippy::panic, clippy::unit_arg)]

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use std::time::Duration;

fn bench_round_robin_selection(c: &mut Criterion) {
    let mut group = c.benchmark_group("round_robin");
    group.measurement_time(Duration::from_secs(5));

    group.bench_function("selection", |b| {
        let mut counter = 0usize;
        let device_count = 4;

        b.iter(|| {
            let index = counter % device_count;
            counter = counter.wrapping_add(1);
            black_box(index)
        });
    });

    group.finish();
}

fn bench_workload_tracking(c: &mut Criterion) {
    let mut group = c.benchmark_group("workload_tracking");

    group.bench_function("atomic_update", |b| {
        use std::sync::atomic::{AtomicU64, Ordering};
        let counter = AtomicU64::new(0);

        b.iter(|| {
            counter.fetch_add(black_box(1), Ordering::Relaxed);
        });
    });

    group.bench_function("mutex_update", |b| {
        use parking_lot::Mutex;
        let counter = Mutex::new(0u64);

        b.iter(|| {
            let mut count = counter.lock();
            *count = count.saturating_add(black_box(1));
        });
    });

    group.finish();
}

fn bench_affinity_lookup(c: &mut Criterion) {
    use dashmap::DashMap;

    let mut group = c.benchmark_group("affinity_lookup");

    group.bench_function("dashmap", |b| {
        let map = DashMap::new();
        let thread_id = std::thread::current().id();
        map.insert(thread_id, 0usize);

        b.iter(|| {
            let value = map.get(&thread_id);
            black_box(value)
        });
    });

    group.finish();
}

fn bench_device_score_calculation(c: &mut Criterion) {
    let mut group = c.benchmark_group("device_scoring");

    group.bench_function("score_calculation", |b| {
        let type_score = 1.0f32; // DiscreteGpu
        let workload = 0.5f32;

        b.iter(|| {
            let score = black_box(type_score) * (1.0 - black_box(workload));
            black_box(score)
        });
    });

    group.bench_function("multi_device_scoring", |b| {
        let workloads = vec![0.1f32, 0.5f32, 0.3f32, 0.9f32];
        let type_scores = vec![1.0f32, 0.7f32, 1.0f32, 1.0f32];

        b.iter(|| {
            let scores: Vec<f32> = black_box(&workloads)
                .iter()
                .zip(black_box(&type_scores).iter())
                .map(|(&w, &t)| t * (1.0 - w))
                .collect();
            black_box(scores)
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_round_robin_selection,
    bench_workload_tracking,
    bench_affinity_lookup,
    bench_device_score_calculation,
);
criterion_main!(benches);
