//! ML inference benchmarks using Criterion.
//!
//! Every benchmark in this file exercises real `oxigeo-ml` code paths -- there
//! is no hand-rolled arithmetic standing in for the library. Inference
//! benchmarks build a tiny real ONNX model (see
//! [`oxigeo_bench::ml_fixtures`]) and run it through
//! `oxigeo_ml::OnnxModel::from_file` -> `oxionnx`'s actual session/runtime,
//! so what is measured is the real parse + build + forward-pass pipeline.
#![allow(missing_docs, clippy::expect_used, clippy::panic, clippy::unit_arg)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;
use std::time::Duration;

#[cfg(feature = "ml")]
use oxigeo_bench::ml_fixtures::{build_relu_model_bytes, write_temp_model};
#[cfg(feature = "ml")]
use oxigeo_core::buffer::RasterBuffer;
#[cfg(feature = "ml")]
use oxigeo_core::types::RasterDataType;
#[cfg(feature = "ml")]
use oxigeo_ml::OnnxModel;

/// Builds a `size` x `size` float32 [`RasterBuffer`] with distinguishable
/// (mixed positive/negative) pixel values so downstream operations (Relu
/// clamping, normalization, thresholding) have real work to do rather than
/// operating on an all-zero buffer.
#[cfg(feature = "ml")]
fn synthetic_buffer(size: u64) -> RasterBuffer {
    let mut buffer = RasterBuffer::zeros(size, size, RasterDataType::Float32);
    for y in 0..size {
        for x in 0..size {
            let v = ((x + y * size) as f64 * 0.013).sin() * 128.0;
            buffer
                .set_pixel(x, y, v)
                .expect("synthetic_buffer: set_pixel");
        }
    }
    buffer
}

/// Real end-to-end ONNX inference, scaling batch size.
///
/// A batch here is `n` independent [`RasterBuffer`]s run through
/// [`OnnxModel::infer_batch`] -- exactly how `oxigeo-ml` callers drive batch
/// inference today (see `OnnxModel::infer_batch`'s doc comment: ONNX Runtime
/// handles intra-batch parallelism, `oxigeo-ml` loops per-item).
#[cfg(feature = "ml")]
fn bench_inference_batch_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("inference_batch_sizes");

    group.sample_size(10);
    group.measurement_time(Duration::from_secs(15));

    // Keep the tensor small (16x16, single channel) so the fixture stays a
    // few hundred bytes and the benchmark measures per-item session overhead
    // rather than being dominated by a single giant forward pass.
    const SIDE: i64 = 16;
    let model_bytes = build_relu_model_bytes(SIDE, SIDE);
    let model_path =
        write_temp_model(&model_bytes, "batch-sizes").expect("write real ONNX fixture to disk");
    let mut model =
        OnnxModel::from_file(&model_path).expect("load real ONNX fixture for batch-size bench");

    for batch_size in [1, 4, 8, 16, 32].iter() {
        group.throughput(Throughput::Elements(*batch_size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(batch_size),
            batch_size,
            |b, &size| {
                let inputs: Vec<RasterBuffer> =
                    (0..size).map(|_| synthetic_buffer(SIDE as u64)).collect();

                b.iter(|| {
                    let outputs = model
                        .infer_batch(black_box(&inputs))
                        .expect("real oxigeo-ml batch inference");
                    black_box(outputs.len())
                });
            },
        );
    }

    let _ = std::fs::remove_file(&model_path);
    group.finish();
}

/// Real `oxigeo_ml::preprocessing` operations: resize, normalize, and tiling.
#[cfg(feature = "ml")]
fn bench_preprocessing(c: &mut Criterion) {
    use oxigeo_ml::preprocessing::{
        NormalizationParams, TileConfig, normalize, resize_nearest, tile_raster,
    };

    let mut group = c.benchmark_group("preprocessing");
    group.sample_size(20);

    let image = synthetic_buffer(224);

    group.bench_function("resize_nearest_224_to_112", |b| {
        b.iter(|| {
            let resized =
                resize_nearest(black_box(&image), 112, 112).expect("real oxigeo-ml resize_nearest");
            black_box(resized.width())
        });
    });

    group.bench_function("normalize_imagenet_channel0", |b| {
        let params = NormalizationParams::imagenet();
        b.iter(|| {
            let normalized =
                normalize(black_box(&image), &params, 0).expect("real oxigeo-ml normalize");
            black_box(normalized.width())
        });
    });

    group.bench_function("tile_raster_64x64_overlap8", |b| {
        let config = TileConfig {
            tile_width: 64,
            tile_height: 64,
            overlap: 8,
            ..TileConfig::default()
        };
        b.iter(|| {
            let tiles =
                tile_raster(black_box(&image), &config).expect("real oxigeo-ml tile_raster");
            black_box(tiles.len())
        });
    });

    group.finish();
}

/// Real `oxigeo_ml::classification` and `oxigeo_ml::detection` postprocessing.
#[cfg(feature = "ml")]
fn bench_postprocessing(c: &mut Criterion) {
    use oxigeo_ml::classification::classify_single_label;
    use oxigeo_ml::detection::{BoundingBox, Detection, NmsConfig, non_maximum_suppression};
    use std::collections::HashMap;

    let mut group = c.benchmark_group("postprocessing");
    group.sample_size(20);

    // Classification: a 1000-entry "probability" buffer (32x32 pixels, one
    // probability per pixel), matching the ImageNet class-count convention
    // used elsewhere in the crate's own tests.
    group.bench_function("classify_single_label", |b| {
        let probs = synthetic_buffer(32);
        b.iter(|| {
            let result = classify_single_label(black_box(&probs), None, 0.0)
                .expect("real oxigeo-ml classify_single_label");
            black_box(result.class_id)
        });
    });

    // Object detection: real non_maximum_suppression over 100 overlapping
    // detections, exercising the crate's actual IoU + greedy-suppression path.
    group.bench_function("non_maximum_suppression", |b| {
        let detections: Vec<Detection> = (0..100)
            .map(|i| {
                let jitter = (i % 10) as f32;
                Detection {
                    bbox: BoundingBox::new(jitter, jitter, 100.0, 100.0),
                    class_id: 0,
                    class_label: None,
                    confidence: 0.5 + (i as f32 % 50.0) / 100.0,
                    attributes: HashMap::new(),
                }
            })
            .collect();
        let config = NmsConfig::default();

        b.iter(|| {
            let kept = non_maximum_suppression(black_box(&detections), &config)
                .expect("real oxigeo-ml non_maximum_suppression");
            black_box(kept.len())
        });
    });

    group.finish();
}

/// Real end-to-end pipeline: `oxigeo_ml::preprocessing::normalize` ->
/// `OnnxModel::infer_batch` -> `oxigeo_ml::postprocessing::apply_threshold`.
#[cfg(feature = "ml")]
fn bench_end_to_end_pipeline(c: &mut Criterion) {
    use oxigeo_ml::postprocessing::apply_threshold;
    use oxigeo_ml::preprocessing::{NormalizationParams, normalize};

    let mut group = c.benchmark_group("end_to_end");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(20));

    const SIDE: i64 = 16;
    let model_bytes = build_relu_model_bytes(SIDE, SIDE);
    let model_path =
        write_temp_model(&model_bytes, "end-to-end").expect("write real ONNX fixture to disk");
    let mut model =
        OnnxModel::from_file(&model_path).expect("load real ONNX fixture for end-to-end bench");
    let norm_params = NormalizationParams::zero_mean_unit_variance();

    for batch_size in [1, 4, 8].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(batch_size),
            batch_size,
            |b, &size| {
                let raw_images: Vec<RasterBuffer> =
                    (0..size).map(|_| synthetic_buffer(SIDE as u64)).collect();

                b.iter(|| {
                    // Preprocessing: real per-image normalization.
                    let preprocessed: Vec<RasterBuffer> = raw_images
                        .iter()
                        .map(|img| {
                            normalize(img, &norm_params, 0).expect("real oxigeo-ml normalize")
                        })
                        .collect();

                    // Inference: real ONNX forward pass through oxionnx.
                    let inferred = model
                        .infer_batch(black_box(&preprocessed))
                        .expect("real oxigeo-ml batch inference");

                    // Postprocessing: real thresholding of the model output.
                    let thresholded: Vec<RasterBuffer> = inferred
                        .iter()
                        .map(|out| {
                            apply_threshold(out, 0.0).expect("real oxigeo-ml apply_threshold")
                        })
                        .collect();

                    black_box(thresholded.len())
                });
            },
        );
    }

    let _ = std::fs::remove_file(&model_path);
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
