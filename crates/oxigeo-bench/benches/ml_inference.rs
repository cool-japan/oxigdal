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
                    // Simulate one forward pass: dot-product layer (input→hidden) + ReLU
                    let hidden_size = 512usize;
                    let hidden: Vec<f32> = (0..hidden_size)
                        .map(|j| {
                            let dot: f32 = input
                                .iter()
                                .enumerate()
                                .take(1024)
                                .fold(0.0_f32, |acc, (i, &x)| {
                                    acc + x * ((i + j) as f32 * 0.001_f32).sin()
                                });
                            dot.max(0.0) // ReLU
                        })
                        .collect();
                    black_box(hidden.len())
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
            // Simulate bilinear resize from 224x224 to 112x112
            let src_w = 224usize;
            let src_h = 224usize;
            let dst_w = 112usize;
            let dst_h = 112usize;
            let channels = 3usize;
            let resized: Vec<u8> = (0..dst_h * dst_w * channels)
                .map(|i| {
                    let c = i % channels;
                    let px = (i / channels) % dst_w;
                    let py = i / channels / dst_w;
                    let src_x = px * src_w / dst_w;
                    let src_y = py * src_h / dst_h;
                    image_data[(src_y * src_w + src_x) * channels + c]
                })
                .collect();
            black_box(resized.len())
        });
    });

    group.bench_function("normalize", |b| {
        let float_data: Vec<f32> = image_data.iter().map(|&x| x as f32 / 255.0).collect();
        b.iter(|| {
            // ImageNet normalization: (x/255 - mean) / std
            let mean = [0.485_f32, 0.456, 0.406];
            let std_v = [0.229_f32, 0.224, 0.225];
            let normalized: Vec<f32> = float_data
                .iter()
                .enumerate()
                .map(|(i, &v)| {
                    let c = i % 3;
                    (v - mean[c]) / std_v[c]
                })
                .collect();
            black_box(normalized.len())
        });
    });

    group.bench_function("color_conversion", |b| {
        b.iter(|| {
            // RGB to BGR in-place swap (every 3rd element)
            let mut bgr = image_data.clone();
            for px in 0..(bgr.len() / 3) {
                bgr.swap(px * 3, px * 3 + 2);
            }
            black_box(bgr.len())
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
            // Greedy NMS over `boxes` with `scores`
            let threshold = 0.5_f32;
            let mut kept = vec![true; boxes.len()];
            for i in 0..boxes.len() {
                if !kept[i] {
                    continue;
                }
                for j in (i + 1)..boxes.len() {
                    if !kept[j] {
                        continue;
                    }
                    let (ax1, ay1, ax2, ay2) = boxes[i];
                    let (bx1, by1, bx2, by2) = boxes[j];
                    let ix1 = ax1.max(bx1);
                    let iy1 = ay1.max(by1);
                    let ix2 = ax2.min(bx2);
                    let iy2 = ay2.min(by2);
                    let inter = (ix2 - ix1).max(0.0) * (iy2 - iy1).max(0.0);
                    let area_a = (ax2 - ax1) * (ay2 - ay1);
                    let area_b = (bx2 - bx1) * (by2 - by1);
                    let iou = inter / (area_a + area_b - inter).max(1e-6);
                    if iou > threshold {
                        if scores[i] >= scores[j] {
                            kept[j] = false;
                        } else {
                            kept[i] = false;
                            break;
                        }
                    }
                }
            }
            let n_kept: usize = kept.iter().filter(|&&k| k).count();
            black_box(n_kept)
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
