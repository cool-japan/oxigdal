//! Benchmark for MQTT publishing operations
#![allow(missing_docs)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use oxigdal_mqtt::types::{Message, QoS};
use std::hint::black_box;

fn benchmark_message_creation(c: &mut Criterion) {
    c.bench_function("message_new", |b| {
        b.iter(|| Message::new(black_box("sensor/temperature"), black_box(b"25.5".to_vec())))
    });

    c.bench_function("message_with_qos", |b| {
        b.iter(|| {
            Message::new(black_box("sensor/temperature"), black_box(b"25.5".to_vec()))
                .with_qos(black_box(QoS::AtLeastOnce))
        })
    });
}

fn benchmark_topic_matching(c: &mut Criterion) {
    use oxigdal_mqtt::types::TopicFilter;

    let filter = TopicFilter::new("sensor/+/temperature", QoS::AtMostOnce);

    c.bench_function("topic_match_single_wildcard", |b| {
        b.iter(|| filter.matches(black_box("sensor/1/temperature")))
    });

    let multi_filter = TopicFilter::new("sensor/#", QoS::AtMostOnce);

    c.bench_function("topic_match_multi_wildcard", |b| {
        b.iter(|| multi_filter.matches(black_box("sensor/1/2/temperature")))
    });
}

fn benchmark_message_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("message_sizes");

    for size in [100, 1000, 10000, 100000].iter() {
        let payload = vec![0u8; *size];
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| Message::new(black_box("test/topic"), black_box(payload.clone())))
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    benchmark_message_creation,
    benchmark_topic_matching,
    benchmark_message_sizes
);
criterion_main!(benches);
