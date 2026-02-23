//! Benchmarks for oxigdal-kafka

#![allow(missing_docs)]

use criterion::{Criterion, criterion_group, criterion_main};
use oxigdal_kafka::producer::{PartitionerType, create_partitioner};
use std::hint::black_box;
use std::time::Duration;

fn bench_partitioners(c: &mut Criterion) {
    let mut group = c.benchmark_group("partitioners");

    let key = b"test-key-for-partitioning";
    let num_partitions = 10;

    group.bench_function("hash_partitioner", |b| {
        let partitioner = create_partitioner(&PartitionerType::Hash);
        b.iter(|| {
            black_box(partitioner.partition("test-topic", key, num_partitions));
        });
    });

    group.bench_function("round_robin_partitioner", |b| {
        let partitioner = create_partitioner(&PartitionerType::RoundRobin);
        b.iter(|| {
            black_box(partitioner.partition("test-topic", key, num_partitions));
        });
    });

    group.bench_function("consistent_hash_partitioner", |b| {
        let partitioner = create_partitioner(&PartitionerType::ConsistentHash);
        b.iter(|| {
            black_box(partitioner.partition("test-topic", key, num_partitions));
        });
    });

    group.finish();
}

fn bench_message_batching(c: &mut Criterion) {
    use bytes::Bytes;
    use oxigdal_kafka::producer::{BatchManager, MessageBatch};

    let mut group = c.benchmark_group("message_batching");

    group.bench_function("batch_add_message", |b| {
        b.iter(|| {
            let mut batch = MessageBatch::new("test-topic".to_string(), 0);
            for i in 0..100 {
                batch.add_message(
                    Some(Bytes::from(format!("key-{}", i))),
                    Bytes::from(format!("value-{}", i)),
                );
            }
            black_box(batch);
        });
    });

    group.bench_function("batch_manager_add", |b| {
        let manager = BatchManager::new(16384, 1000, Duration::from_millis(100));
        b.iter(|| {
            for i in 0..100 {
                manager.add_message(
                    "test-topic",
                    0,
                    Some(Bytes::from(format!("key-{}", i))),
                    Bytes::from(format!("value-{}", i)),
                );
            }
        });
    });

    group.finish();
}

fn bench_offset_management(c: &mut Criterion) {
    use oxigdal_kafka::consumer::{
        CommitStrategy, InMemoryOffsetStore, OffsetManager, OffsetStore,
    };

    let mut group = c.benchmark_group("offset_management");

    group.bench_function("offset_store_write", |b| {
        let store = InMemoryOffsetStore::new();
        b.iter(|| {
            for i in 0..100i64 {
                store.store("test-topic", (i % 10) as i32, i);
            }
        });
    });

    group.bench_function("offset_store_read", |b| {
        let store = InMemoryOffsetStore::new();
        for i in 0..100i64 {
            store.store("test-topic", (i % 10) as i32, i);
        }
        b.iter(|| {
            for i in 0..100 {
                black_box(store.get("test-topic", i % 10));
            }
        });
    });

    group.bench_function("offset_manager_track", |b| {
        let manager = OffsetManager::new(CommitStrategy::Manual);
        b.iter(|| {
            for i in 0..100i64 {
                manager.track_offset("test-topic", (i % 10) as i32, i);
            }
        });
    });

    group.finish();
}

fn bench_metrics(c: &mut Criterion) {
    use oxigdal_kafka::producer::{AtomicProducerMetrics, ProducerMetrics};

    let mut group = c.benchmark_group("metrics");

    group.bench_function("producer_metrics_record", |b| {
        let mut metrics = ProducerMetrics::new();
        b.iter(|| {
            metrics.record_message_sent(100, Duration::from_micros(1000));
        });
    });

    group.bench_function("atomic_producer_metrics_record", |b| {
        let metrics = AtomicProducerMetrics::new();
        b.iter(|| {
            metrics.record_message_sent(100, Duration::from_micros(1000));
        });
    });

    group.finish();
}

fn bench_schema_compatibility(_c: &mut Criterion) {
    #[cfg(feature = "schema-registry")]
    {
        use oxigdal_kafka::schema_registry::CompatibilityChecker;

        let mut group = _c.benchmark_group("schema_compatibility");

        let schema1 = r#"{"type": "string"}"#;
        let schema2 = r#"{"type": "string"}"#;

        group.bench_function("check_backward_same", |b| {
            b.iter(|| {
                black_box(CompatibilityChecker::check_backward(schema1, schema2));
            });
        });

        let old_schema = r#"{"type": "int"}"#;
        let new_schema = r#"{"type": "long"}"#;

        group.bench_function("check_backward_promotion", |b| {
            b.iter(|| {
                black_box(CompatibilityChecker::check_backward(old_schema, new_schema));
            });
        });

        group.finish();
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .measurement_time(Duration::from_secs(10))
        .sample_size(100);
    targets = bench_partitioners,
        bench_message_batching,
        bench_offset_management,
        bench_metrics,
        bench_schema_compatibility
}

criterion_main!(benches);
