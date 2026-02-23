//! Benchmarks for OxiGDAL ML operations
#![allow(missing_docs, clippy::expect_used)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use oxigdal_ml::detection::{BoundingBox, Detection, NmsConfig, non_maximum_suppression};
use oxigdal_ml::postprocessing::{apply_threshold, mask_to_polygons};
use oxigdal_ml::preprocessing::{NormalizationParams, TileConfig, normalize, tile_raster};
use oxigdal_ml::segmentation::{find_connected_components, probability_to_mask};
use std::collections::HashMap;
use std::hint::black_box;

fn benchmark_normalization(c: &mut Criterion) {
    let mut group = c.benchmark_group("normalization");

    for size in [256, 512, 1024].iter() {
        let buffer = RasterBuffer::zeros(*size, *size, RasterDataType::Float32);
        let params = NormalizationParams::imagenet();

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let _ = normalize(black_box(&buffer), black_box(&params));
            });
        });
    }

    group.finish();
}

fn benchmark_tiling(c: &mut Criterion) {
    let mut group = c.benchmark_group("tiling");

    for size in [512, 1024, 2048].iter() {
        let buffer = RasterBuffer::zeros(*size, *size, RasterDataType::Float32);
        let config = TileConfig::default();

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let _ = tile_raster(black_box(&buffer), black_box(&config));
            });
        });
    }

    group.finish();
}

fn benchmark_probability_to_mask(c: &mut Criterion) {
    let mut group = c.benchmark_group("probability_to_mask");

    for size in [256, 512, 1024].iter() {
        let buffer = RasterBuffer::zeros(*size, *size, RasterDataType::Float32);

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let _ = probability_to_mask(black_box(&buffer), black_box(2), black_box(0.5));
            });
        });
    }

    group.finish();
}

fn benchmark_connected_components(c: &mut Criterion) {
    let mut group = c.benchmark_group("connected_components");

    for size in [256, 512].iter() {
        let mut mask = RasterBuffer::zeros(*size, *size, RasterDataType::Float32);

        // Create some components
        for y in 10..20 {
            for x in 10..20 {
                let _ = mask.set_pixel(x, y, 1.0);
            }
        }

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let _ = find_connected_components(black_box(&mask), black_box(10));
            });
        });
    }

    group.finish();
}

fn benchmark_nms(c: &mut Criterion) {
    let mut group = c.benchmark_group("nms");

    for num_detections in [10, 100, 1000].iter() {
        let detections: Vec<Detection> = (0..*num_detections)
            .map(|i| Detection {
                bbox: BoundingBox::new(
                    (i as f32 * 10.0) % 1000.0,
                    (i as f32 * 10.0) % 1000.0,
                    50.0,
                    50.0,
                ),
                class_id: i % 10,
                class_label: None,
                confidence: 0.5 + (i as f32 / *num_detections as f32) * 0.5,
                attributes: HashMap::new(),
            })
            .collect();

        let config = NmsConfig::default();

        group.bench_with_input(
            BenchmarkId::from_parameter(num_detections),
            num_detections,
            |b, _| {
                b.iter(|| {
                    let _ = non_maximum_suppression(black_box(&detections), black_box(&config));
                });
            },
        );
    }

    group.finish();
}

fn benchmark_threshold(c: &mut Criterion) {
    let mut group = c.benchmark_group("threshold");

    for size in [256, 512, 1024].iter() {
        let buffer = RasterBuffer::zeros(*size, *size, RasterDataType::Float32);

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let _ = apply_threshold(black_box(&buffer), black_box(0.5));
            });
        });
    }

    group.finish();
}

fn benchmark_mask_to_polygons(c: &mut Criterion) {
    let mut group = c.benchmark_group("mask_to_polygons");

    for size in [128, 256].iter() {
        let mut mask = RasterBuffer::zeros(*size, *size, RasterDataType::Float32);

        // Create some regions
        for y in 10..20 {
            for x in 10..20 {
                let _ = mask.set_pixel(x, y, 1.0);
            }
        }

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let _ = mask_to_polygons(black_box(&mask), black_box(10.0));
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    benchmark_normalization,
    benchmark_tiling,
    benchmark_probability_to_mask,
    benchmark_connected_components,
    benchmark_nms,
    benchmark_threshold,
    benchmark_mask_to_polygons
);

criterion_main!(benches);
