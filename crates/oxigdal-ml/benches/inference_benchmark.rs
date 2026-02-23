//! Comprehensive ML inference benchmarks
#![allow(missing_docs, clippy::expect_used, clippy::unnecessary_cast)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use oxigdal_ml::batch::{BatchConfig, BatchProcessor};
use oxigdal_ml::inference::{InferenceConfig, InferenceEngine};
use oxigdal_ml::models::{Model, ModelMetadata};
use oxigdal_ml::preprocessing::{
    NormalizationParams, PaddingStrategy, TileConfig, normalize, tile_raster,
};
use std::hint::black_box;

// Mock model for benchmarking
struct MockModel {
    metadata: ModelMetadata,
}

impl MockModel {
    fn new() -> Self {
        Self {
            metadata: ModelMetadata {
                name: "mock_model".to_string(),
                version: "1.0".to_string(),
                description: "Mock model for benchmarking".to_string(),
                input_names: vec!["input".to_string()],
                output_names: vec!["output".to_string()],
                input_shape: (3, 256, 256),
                output_shape: (2, 256, 256),
                class_labels: None,
            },
        }
    }
}

impl Model for MockModel {
    fn metadata(&self) -> &ModelMetadata {
        &self.metadata
    }

    fn predict(&mut self, _input: &RasterBuffer) -> oxigdal_ml::error::Result<RasterBuffer> {
        // Simulate some computation
        Ok(RasterBuffer::zeros(256, 256, RasterDataType::Float32))
    }

    fn predict_batch(
        &mut self,
        inputs: &[RasterBuffer],
    ) -> oxigdal_ml::error::Result<Vec<RasterBuffer>> {
        inputs.iter().map(|input| self.predict(input)).collect()
    }

    fn input_shape(&self) -> (usize, usize, usize) {
        (3, 256, 256)
    }

    fn output_shape(&self) -> (usize, usize, usize) {
        (2, 256, 256)
    }
}

fn bench_single_inference(c: &mut Criterion) {
    let model = MockModel::new();
    let config = InferenceConfig::default();
    let mut engine = InferenceEngine::new(model, config);
    let input = RasterBuffer::zeros(256, 256, RasterDataType::Float32);

    c.bench_function("single_inference_256x256", |b| {
        b.iter(|| engine.predict(black_box(&input)).ok());
    });
}

fn bench_batch_inference(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_inference");

    for batch_size in [1, 4, 8, 16, 32].iter() {
        let model = MockModel::new();
        let config = BatchConfig::builder()
            .max_batch_size(*batch_size)
            .parallel_batches(4)
            .build();
        let processor = BatchProcessor::new(model, config);

        let inputs: Vec<_> = (0..*batch_size)
            .map(|_| RasterBuffer::zeros(256, 256, RasterDataType::Float32))
            .collect();

        group.throughput(Throughput::Elements(*batch_size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(batch_size),
            batch_size,
            |b, &_size| {
                b.iter(|| processor.infer_batch(black_box(inputs.clone())).ok());
            },
        );
    }

    group.finish();
}

fn bench_preprocessing(c: &mut Criterion) {
    let mut group = c.benchmark_group("preprocessing");

    // Normalization benchmark
    let input = RasterBuffer::zeros(512, 512, RasterDataType::Float32);
    let params = NormalizationParams::imagenet();

    group.bench_function("normalize_512x512", |b| {
        b.iter(|| normalize(black_box(&input), black_box(&params)).ok());
    });

    // Tiling benchmark
    let config = TileConfig {
        tile_width: 256,
        tile_height: 256,
        overlap: 32,
        padding: PaddingStrategy::Reflect,
    };

    group.bench_function("tile_1024x1024", |b| {
        let large_input = RasterBuffer::zeros(1024, 1024, RasterDataType::Float32);
        b.iter(|| tile_raster(black_box(&large_input), black_box(&config)).ok());
    });

    group.finish();
}

fn bench_tiled_inference(c: &mut Criterion) {
    let mut group = c.benchmark_group("tiled_inference");

    for size in [512, 1024, 2048].iter() {
        let model = MockModel::new();
        let config = InferenceConfig {
            normalization: Some(NormalizationParams::imagenet()),
            tiling: Some(TileConfig {
                tile_width: 256,
                tile_height: 256,
                overlap: 32,
                padding: PaddingStrategy::Reflect,
            }),
            confidence_threshold: 0.5,
        };
        let mut engine = InferenceEngine::new(model, config);
        let input = RasterBuffer::zeros(*size, *size, RasterDataType::Float32);

        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &_s| {
            b.iter(|| engine.predict(black_box(&input)).ok());
        });
    }

    group.finish();
}

fn bench_memory_usage(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_usage");

    // Benchmark different buffer sizes
    for size in [256, 512, 1024].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &s| {
            b.iter(|| {
                let buffer = RasterBuffer::zeros(s, s, RasterDataType::Float32);
                black_box(buffer)
            });
        });
    }

    group.finish();
}

fn bench_parallel_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("parallel_throughput");

    for num_threads in [1, 2, 4, 8].iter() {
        let model = MockModel::new();
        let config = BatchConfig::builder()
            .max_batch_size(32)
            .parallel_batches(*num_threads)
            .build();
        let processor = BatchProcessor::new(model, config);

        let inputs: Vec<_> = (0..32)
            .map(|_| RasterBuffer::zeros(256, 256, RasterDataType::Float32))
            .collect();

        group.throughput(Throughput::Elements(32));
        group.bench_with_input(
            BenchmarkId::from_parameter(num_threads),
            num_threads,
            |b, &_threads| {
                b.iter(|| processor.infer_batch(black_box(inputs.clone())).ok());
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_single_inference,
    bench_batch_inference,
    bench_preprocessing,
    bench_tiled_inference,
    bench_memory_usage,
    bench_parallel_throughput
);

criterion_main!(benches);
