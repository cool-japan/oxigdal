//! Benchmarks for oxigdal-cluster.

#![allow(missing_docs)]

use criterion::{Criterion, criterion_group, criterion_main};
use oxigdal_cluster::*;
use std::hint::black_box;
use std::time::Duration;

fn bench_task_graph(c: &mut Criterion) {
    c.bench_function("task_graph_add_100", |b| {
        b.iter(|| {
            let graph = TaskGraph::new();
            for i in 0..100 {
                let task = Task {
                    id: TaskId::new(),
                    name: format!("task_{}", i),
                    task_type: "benchmark".to_string(),
                    priority: 0,
                    payload: vec![],
                    dependencies: vec![],
                    estimated_duration: Some(Duration::from_secs(1)),
                    resources: ResourceRequirements::default(),
                    locality_hints: vec![],
                    created_at: std::time::Instant::now(),
                    scheduled_at: None,
                    started_at: None,
                    completed_at: None,
                    status: TaskStatus::Ready,
                    result: None,
                    error: None,
                    retry_count: 0,
                    checkpoint: None,
                };
                black_box(graph.add_task(task).ok());
            }
        });
    });
}

fn bench_cache(c: &mut Criterion) {
    c.bench_function("cache_put_get", |b| {
        let cache = DistributedCache::with_defaults();
        let worker_id = WorkerId::new();

        b.iter(|| {
            let key = CacheKey::new("test".to_string(), "key1".to_string());
            let data = vec![0u8; 1024];
            cache.put(key.clone(), data, worker_id).ok();
            black_box(cache.get(&key).ok());
        });
    });
}

criterion_group!(benches, bench_task_graph, bench_cache);
criterion_main!(benches);
