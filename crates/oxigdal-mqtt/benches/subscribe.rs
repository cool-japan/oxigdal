//! Benchmark for MQTT subscription operations
#![allow(missing_docs)]

use criterion::{Criterion, criterion_group, criterion_main};
use oxigdal_mqtt::types::{QoS, TopicFilter};
use std::hint::black_box;

fn benchmark_filter_validation(c: &mut Criterion) {
    c.bench_function("validate_simple_topic", |b| {
        b.iter(|| {
            let filter = TopicFilter::new(black_box("sensor/temperature"), QoS::AtMostOnce);
            filter.validate()
        })
    });

    c.bench_function("validate_wildcard_topic", |b| {
        b.iter(|| {
            let filter = TopicFilter::new(black_box("sensor/+/temperature"), QoS::AtMostOnce);
            filter.validate()
        })
    });

    c.bench_function("validate_multi_wildcard", |b| {
        b.iter(|| {
            let filter = TopicFilter::new(black_box("sensor/#"), QoS::AtMostOnce);
            filter.validate()
        })
    });
}

criterion_group!(benches, benchmark_filter_validation);
criterion_main!(benches);
