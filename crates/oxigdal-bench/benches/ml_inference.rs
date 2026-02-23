//! ML inference benchmarks using Criterion.
#![allow(missing_docs, clippy::expect_used, clippy::panic, clippy::unit_arg)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;
use std::time::Duration;

#[cfg(feature = "ml")]
fn bench_inference_batch_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("inference_batch_sizes");

    group.sample_size(10);
    group.measurement_time(Duration::from_secs(15));

    for batch_size in [1, 4, 8, 16, 32].iter() {
        group.throughput(Throughput::Elements(*batch_size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(batch_size),
            batch_size,
            |b, &size| {
                // Dummy input tensor (NCHW format: batch, channels, height, width)
                let input_size = size * 3 * 224 * 224;
                let input = vec![0.0f32; input_size];

                b.iter(|| {
                    // Placeholder for actual inference
                    black_box(&input);
                });
            },
        );
    }

    group.finish();
}

#[cfg(feature = "ml")]
fn bench_preprocessing(c: &mut Criterion) {
    let mut group = c.benchmark_group("preprocessing");

    group.sample_size(20);

    let image_data = vec![0u8; 224 * 224 * 3]; // RGB image

    group.bench_function("resize", |b| {
        b.iter(|| {
            // Placeholder for resize operation
            black_box(&image_data);
        });
    });

    group.bench_function("normalize", |b| {
        let float_data: Vec<f32> = image_data.iter().map(|&x| x as f32 / 255.0).collect();
        b.iter(|| {
            // Placeholder for normalization
            black_box(&float_data);
        });
    });

    group.bench_function("color_conversion", |b| {
        b.iter(|| {
            // Placeholder for RGB to BGR conversion
            black_box(&image_data);
        });
    });

    group.finish();
}

#[cfg(feature = "ml")]
fn bench_postprocessing(c: &mut Criterion) {
    let mut group = c.benchmark_group("postprocessing");

    group.sample_size(20);

    // Classification
    group.bench_function("softmax_argmax", |b| {
        let logits = vec![0.5f32; 1000]; // ImageNet classes
        b.iter(|| {
            // Placeholder for softmax + argmax
            let max_idx = logits
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(idx, _)| idx);
            black_box(max_idx);
        });
    });

    // Object detection
    group.bench_function("nms", |b| {
        // Dummy bounding boxes
        let boxes = vec![(0.0f32, 0.0f32, 100.0f32, 100.0f32); 100];
        let scores = vec![0.9f32; 100];

        b.iter(|| {
            // Placeholder for NMS
            black_box(&boxes);
            black_box(&scores);
        });
    });

    group.finish();
}

#[cfg(feature = "ml")]
fn bench_end_to_end_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("end_to_end");

    group.sample_size(10);
    group.measurement_time(Duration::from_secs(20));

    for batch_size in [1, 4, 8].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(batch_size),
            batch_size,
            |b, &size| {
                let image_data = vec![0u8; size * 224 * 224 * 3];

                b.iter(|| {
                    // Preprocessing
                    let preprocessed: Vec<f32> =
                        image_data.iter().map(|&x| x as f32 / 255.0).collect();

                    // Inference (placeholder)
                    black_box(&preprocessed);

                    // Postprocessing (placeholder)
                    let result = 0;
                    black_box(result);
                });
            },
        );
    }

    group.finish();
}

#[cfg(feature = "ml")]
criterion_group!(
    ml_benches,
    bench_inference_batch_sizes,
    bench_preprocessing,
    bench_postprocessing,
    bench_end_to_end_pipeline
);

#[cfg(not(feature = "ml"))]
criterion_group!(ml_benches,);

criterion_main!(ml_benches);
