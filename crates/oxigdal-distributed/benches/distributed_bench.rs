//! Benchmarks for distributed processing operations.
#![allow(missing_docs, clippy::expect_used, clippy::panic, clippy::unit_arg)]

use arrow::array::{Float64Array, Int32Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_distributed::*;
use std::hint::black_box;
use std::sync::Arc;

fn create_test_batch(rows: usize) -> RecordBatch {
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int32, false),
        Field::new("value", DataType::Float64, false),
        Field::new("name", DataType::Utf8, false),
    ]));

    let id_array = Int32Array::from((0..rows as i32).collect::<Vec<_>>());
    let value_array = Float64Array::from((0..rows).map(|i| i as f64 * 10.0).collect::<Vec<_>>());
    let name_array =
        StringArray::from((0..rows).map(|i| format!("name_{}", i)).collect::<Vec<_>>());

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(id_array),
            Arc::new(value_array),
            Arc::new(name_array),
        ],
    )
    .expect("valid batch")
}

fn bench_tile_partitioning(c: &mut Criterion) {
    let mut group = c.benchmark_group("tile_partitioning");

    for grid_size in [2, 4, 8, 16].iter() {
        let extent = SpatialExtent::new(0.0, 0.0, 1000.0, 1000.0).expect("valid extent");
        let partitioner = TilePartitioner::new(extent, *grid_size, *grid_size).expect("valid");

        group.throughput(Throughput::Elements((grid_size * grid_size) as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}x{}", grid_size, grid_size)),
            grid_size,
            |b, _| {
                b.iter(|| {
                    let partitions = partitioner.partition();
                    black_box(partitions)
                });
            },
        );
    }

    group.finish();
}

fn bench_hash_shuffle(c: &mut Criterion) {
    let mut group = c.benchmark_group("hash_shuffle");

    for rows in [1000, 10000, 100000].iter() {
        let batch = create_test_batch(*rows);
        let shuffle = HashShuffle::new("id".to_string(), 4).expect("valid shuffle");

        group.throughput(Throughput::Elements(*rows as u64));
        group.bench_with_input(BenchmarkId::from_parameter(rows), rows, |b, _| {
            b.iter(|| {
                let partitions = shuffle.shuffle(&batch).expect("shuffle succeeds");
                black_box(partitions)
            });
        });
    }

    group.finish();
}

fn bench_range_shuffle(c: &mut Criterion) {
    let mut group = c.benchmark_group("range_shuffle");

    for rows in [1000, 10000, 100000].iter() {
        let batch = create_test_batch(*rows);
        let boundaries = vec![250.0, 500.0, 750.0];
        let shuffle = RangeShuffle::new("value".to_string(), boundaries).expect("valid shuffle");

        group.throughput(Throughput::Elements(*rows as u64));
        group.bench_with_input(BenchmarkId::from_parameter(rows), rows, |b, _| {
            b.iter(|| {
                let partitions = shuffle.shuffle(&batch).expect("shuffle succeeds");
                black_box(partitions)
            });
        });
    }

    group.finish();
}

fn bench_broadcast_shuffle(c: &mut Criterion) {
    let mut group = c.benchmark_group("broadcast_shuffle");

    for rows in [1000, 10000, 100000].iter() {
        let batch = create_test_batch(*rows);
        let shuffle = BroadcastShuffle::new(4).expect("valid shuffle");

        group.throughput(Throughput::Elements(*rows as u64));
        group.bench_with_input(BenchmarkId::from_parameter(rows), rows, |b, _| {
            b.iter(|| {
                let partitions = shuffle.shuffle(&batch);
                black_box(partitions)
            });
        });
    }

    group.finish();
}

fn bench_task_scheduler(c: &mut Criterion) {
    let mut group = c.benchmark_group("task_scheduler");

    for num_tasks in [100, 1000, 10000].iter() {
        group.throughput(Throughput::Elements(*num_tasks as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(num_tasks),
            num_tasks,
            |b, &num_tasks| {
                b.iter(|| {
                    let mut scheduler = TaskScheduler::new();

                    // Add tasks
                    for i in 0..num_tasks {
                        let task = Task::new(
                            TaskId(i as u64),
                            PartitionId(i as u64),
                            TaskOperation::Filter {
                                expression: format!("value > {}", i),
                            },
                        );
                        scheduler.add_task(task);
                    }

                    // Process tasks
                    while let Some(task) = scheduler.next_task() {
                        scheduler.mark_running(task, "worker-1".to_string());
                    }

                    black_box(scheduler)
                });
            },
        );
    }

    group.finish();
}

fn bench_hash_partitioner(c: &mut Criterion) {
    let mut group = c.benchmark_group("hash_partitioner");

    let partitioner = HashPartitioner::new(16).expect("valid partitioner");
    let keys: Vec<Vec<u8>> = (0..10000)
        .map(|i| format!("key_{}", i).into_bytes())
        .collect();

    group.throughput(Throughput::Elements(keys.len() as u64));
    group.bench_function("hash_10000_keys", |b| {
        b.iter(|| {
            for key in &keys {
                let partition = partitioner.partition_for_key(key);
                black_box(partition);
            }
        });
    });

    group.finish();
}

fn bench_range_partitioner(c: &mut Criterion) {
    let mut group = c.benchmark_group("range_partitioner");

    let boundaries = vec![
        100.0, 200.0, 300.0, 400.0, 500.0, 600.0, 700.0, 800.0, 900.0,
    ];
    let partitioner = RangePartitioner::new(boundaries).expect("valid partitioner");
    let values: Vec<f64> = (0..10000).map(|i| i as f64).collect();

    group.throughput(Throughput::Elements(values.len() as u64));
    group.bench_function("partition_10000_values", |b| {
        b.iter(|| {
            for &value in &values {
                let partition = partitioner.partition_for_value(value);
                black_box(partition);
            }
        });
    });

    group.finish();
}

fn bench_coordinator_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("coordinator");

    group.bench_function("create_and_add_workers", |b| {
        b.iter(|| {
            let config = CoordinatorConfig::new("localhost:50051".to_string());
            let coordinator = Coordinator::new(config);

            for i in 0..10 {
                coordinator
                    .add_worker(format!("worker-{}", i), format!("localhost:{}", 50052 + i))
                    .expect("add worker");
            }

            black_box(coordinator)
        });
    });

    group.bench_function("submit_100_tasks", |b| {
        b.iter(|| {
            let config = CoordinatorConfig::new("localhost:50051".to_string());
            let coordinator = Coordinator::new(config);

            for i in 0..100 {
                coordinator
                    .submit_task(
                        PartitionId(i),
                        TaskOperation::Filter {
                            expression: format!("value > {}", i),
                        },
                    )
                    .expect("submit task");
            }

            black_box(coordinator)
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_tile_partitioning,
    bench_hash_shuffle,
    bench_range_shuffle,
    bench_broadcast_shuffle,
    bench_task_scheduler,
    bench_hash_partitioner,
    bench_range_partitioner,
    bench_coordinator_operations,
);
criterion_main!(benches);
