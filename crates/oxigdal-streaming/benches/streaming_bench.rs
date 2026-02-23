//! Benchmarks for streaming operations.
#![allow(missing_docs, clippy::expect_used, clippy::panic, clippy::unit_arg)]

use chrono::Utc;
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use oxigdal_streaming::core::stream::{Stream, StreamElement, StreamMessage};
use oxigdal_streaming::core::{
    BackpressureConfig, BackpressureManager, FlowControlConfig, FlowController,
};
use oxigdal_streaming::state::{MemoryStateBackend, StateBackend};
use oxigdal_streaming::transformations::{AggregateOperator, CountAggregate};
use oxigdal_streaming::windowing::{TumblingAssigner, WindowAssigner};
use std::hint::black_box;

fn bench_stream_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("stream_throughput");

    for size in [100, 1000, 10000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let rt =
                tokio::runtime::Runtime::new().expect("runtime should be created for benchmark");

            b.iter(|| {
                rt.block_on(async {
                    let stream = Stream::new();

                    for i in 0..size {
                        let elem = StreamElement::new(vec![i as u8], Utc::now());
                        stream
                            .send(StreamMessage::Data(elem))
                            .await
                            .expect("stream should send data successfully");
                    }

                    for _ in 0..size {
                        let _ = stream
                            .recv()
                            .await
                            .expect("stream should receive data successfully");
                    }
                });
            });
        });
    }

    group.finish();
}

fn bench_backpressure(c: &mut Criterion) {
    let mut group = c.benchmark_group("backpressure");

    for capacity in [100, 1000, 10000].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(capacity),
            capacity,
            |b, &capacity| {
                let rt = tokio::runtime::Runtime::new()
                    .expect("runtime should be created for benchmark");

                b.iter(|| {
                    rt.block_on(async {
                        let config = BackpressureConfig::default();
                        let manager = BackpressureManager::new(config, capacity);

                        for _ in 0..capacity / 2 {
                            let _ = manager.handle_element_arrival().await;
                        }

                        for _ in 0..capacity / 2 {
                            manager
                                .handle_element_processed(std::time::Duration::from_millis(1))
                                .await;
                        }
                    });
                });
            },
        );
    }

    group.finish();
}

fn bench_flow_control(c: &mut Criterion) {
    let mut group = c.benchmark_group("flow_control");

    for rate in [100.0, 1000.0, 10000.0].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(rate), rate, |b, &rate| {
            let rt =
                tokio::runtime::Runtime::new().expect("runtime should be created for benchmark");

            b.iter(|| {
                rt.block_on(async {
                    let config = FlowControlConfig {
                        enable_rate_limiting: true,
                        max_rate: Some(rate),
                        ..Default::default()
                    };

                    let controller = FlowController::new(config);

                    for _ in 0..100 {
                        let _ = controller.acquire(1).await;
                    }
                });
            });
        });
    }

    group.finish();
}

fn bench_windowing(c: &mut Criterion) {
    let mut group = c.benchmark_group("windowing");

    for window_size in [1, 10, 60].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(window_size),
            window_size,
            |b, &window_size| {
                b.iter(|| {
                    let assigner = TumblingAssigner::new(chrono::Duration::seconds(window_size));
                    let elem = StreamElement::new(vec![1, 2, 3], Utc::now());

                    black_box(
                        assigner
                            .assign_windows(&elem)
                            .expect("window assignment should succeed in benchmark"),
                    );
                });
            },
        );
    }

    group.finish();
}

fn bench_aggregation(c: &mut Criterion) {
    let mut group = c.benchmark_group("aggregation");

    for count in [100, 1000, 10000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(count), count, |b, &count| {
            let rt =
                tokio::runtime::Runtime::new().expect("runtime should be created for benchmark");

            b.iter(|| {
                rt.block_on(async {
                    let operator = AggregateOperator::new(CountAggregate);

                    for i in 0..count {
                        let elem = StreamElement::new(vec![i as u8], Utc::now());
                        let _ = operator.process(elem).await;
                    }
                });
            });
        });
    }

    group.finish();
}

fn bench_state_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("state_operations");

    for ops in [100, 1000, 10000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(ops), ops, |b, &ops| {
            let rt =
                tokio::runtime::Runtime::new().expect("runtime should be created for benchmark");

            b.iter(|| {
                rt.block_on(async {
                    let backend = MemoryStateBackend::new();

                    for i in 0..ops {
                        let key = format!("key{}", i).into_bytes();
                        let value = vec![i as u8];
                        backend
                            .put(&key, &value)
                            .await
                            .expect("state backend should store value successfully");
                    }

                    for i in 0..ops {
                        let key = format!("key{}", i).into_bytes();
                        let _ = backend
                            .get(&key)
                            .await
                            .expect("state backend should retrieve value successfully");
                    }
                });
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_stream_throughput,
    bench_backpressure,
    bench_flow_control,
    bench_windowing,
    bench_aggregation,
    bench_state_operations
);

criterion_main!(benches);
