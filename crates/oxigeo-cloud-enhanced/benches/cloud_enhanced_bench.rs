//! Benchmarks for oxigeo-cloud-enhanced.
#![allow(missing_docs)]

use criterion::{Criterion, criterion_group, criterion_main};
use oxigeo_cloud_enhanced::*;
use std::hint::black_box;

fn bench_cloud_provider_display(c: &mut Criterion) {
    c.bench_function("cloud_provider_display", |b| {
        b.iter(|| {
            let provider = black_box(CloudProvider::Aws);
            let _name = provider.name();
        });
    });
}

fn bench_resource_type_display(c: &mut Criterion) {
    c.bench_function("resource_type_display", |b| {
        b.iter(|| {
            let resource = black_box(ResourceType::Analytics);
            let _name = resource.name();
        });
    });
}

fn bench_error_creation(c: &mut Criterion) {
    c.bench_function("error_creation", |b| {
        b.iter(|| {
            let _err = CloudEnhancedError::aws_service(black_box("test error"));
        });
    });
}

#[cfg(feature = "gcp")]
fn bench_gcp_config_creation(c: &mut Criterion) {
    c.bench_function("gcp_config_creation", |b| {
        b.iter(|| {
            let _config = gcp::GcpConfig::new(
                black_box("test-project".to_string()),
                black_box(Some("us-central1".to_string())),
            );
        });
    });
}

#[cfg(not(feature = "gcp"))]
criterion_group!(
    benches,
    bench_cloud_provider_display,
    bench_resource_type_display,
    bench_error_creation,
);

#[cfg(feature = "gcp")]
criterion_group!(
    benches,
    bench_cloud_provider_display,
    bench_resource_type_display,
    bench_error_creation,
    bench_gcp_config_creation,
);

criterion_main!(benches);
