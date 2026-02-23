//! Benchmarks for OxiGDAL ETL framework
#![allow(missing_docs, clippy::expect_used)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_etl::operators::{AggregateFunctions, FilterOperator, MapOperator, WindowOperator};
use oxigdal_etl::prelude::*;
use std::hint::black_box;
use std::io::Write;
use tempfile::NamedTempFile;

fn create_test_file(lines: usize) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("Failed to create temp file");
    for i in 0..lines {
        writeln!(file, "{{\"id\": {}, \"value\": {}}}", i, i * 10).expect("Failed to write");
    }
    file
}

fn bench_file_source(c: &mut Criterion) {
    let mut group = c.benchmark_group("file_source");

    for size in [100, 1000, 10000] {
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
            b.iter(|| {
                runtime.block_on(async {
                    let temp_file = create_test_file(size);
                    let path = temp_file.path().to_path_buf();

                    let source = FileSource::new(path).line_based(true);
                    let stream = source.stream().await.expect("Failed to create stream");

                    let _ = black_box(stream);
                })
            });
        });
    }

    group.finish();
}

fn bench_map_operator(c: &mut Criterion) {
    let mut group = c.benchmark_group("map_operator");

    for size in [100, 1000, 10000] {
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
            b.iter(|| {
                runtime.block_on(async {
                    let op = MapOperator::bytes("double".to_string(), |mut bytes| {
                        let copy = bytes.clone();
                        bytes.extend_from_slice(&copy);
                        bytes
                    });

                    for _ in 0..size {
                        let _ = op.transform(vec![1, 2, 3, 4]).await;
                    }
                })
            });
        });
    }

    group.finish();
}

fn bench_filter_operator(c: &mut Criterion) {
    let mut group = c.benchmark_group("filter_operator");

    for size in [100, 1000, 10000] {
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
            b.iter(|| {
                runtime.block_on(async {
                    let filter = FilterOperator::min_size(10);

                    for i in 0..size {
                        let item = vec![i as u8; (i % 20) + 1];
                        let _ = filter.transform(item).await;
                    }
                })
            });
        });
    }

    group.finish();
}

fn bench_json_transformation(c: &mut Criterion) {
    let mut group = c.benchmark_group("json_transformation");

    for size in [100, 1000, 10000] {
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
            b.iter(|| {
                runtime.block_on(async {
                    let op = MapOperator::extract_json_field("id".to_string());

                    for i in 0..size {
                        let json = serde_json::json!({"id": i, "name": "test"});
                        let item = serde_json::to_vec(&json).expect("Failed to serialize");
                        let _ = op.transform(item).await;
                    }
                })
            });
        });
    }

    group.finish();
}

fn bench_window_aggregation(c: &mut Criterion) {
    let mut group = c.benchmark_group("window_aggregation");

    for window_size in [10, 100, 1000] {
        group.throughput(Throughput::Elements(window_size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(window_size),
            &window_size,
            |b, &window_size| {
                let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
                b.iter(|| {
                    runtime.block_on(async move {
                        let window =
                            WindowOperator::tumbling_count(window_size, WindowAggregator::count());

                        for i in 0..window_size {
                            let _ = window.transform(vec![i as u8]).await;
                        }
                    })
                });
            },
        );
    }

    group.finish();
}

fn bench_aggregate_stats(c: &mut Criterion) {
    let mut group = c.benchmark_group("aggregate_stats");

    for size in [100, 1000, 10000] {
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
            b.iter(|| {
                runtime.block_on(async {
                    let items: Vec<_> = (0..size)
                        .map(|i| {
                            serde_json::to_vec(&serde_json::json!({"value": i as f64}))
                                .expect("Failed")
                        })
                        .collect();

                    let _ = AggregateFunctions::stats("value".to_string())(items).await;
                })
            });
        });
    }

    group.finish();
}

fn bench_pipeline_end_to_end(c: &mut Criterion) {
    let mut group = c.benchmark_group("pipeline_end_to_end");

    for size in [100, 1000, 5000] {
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
            b.iter(|| {
                runtime.block_on(async {
                    let temp_input = create_test_file(size);
                    let input_path = temp_input.path().to_path_buf();

                    let temp_output = NamedTempFile::new().expect("Failed to create output");
                    let output_path = temp_output.path().to_path_buf();

                    let pipeline = Pipeline::builder()
                        .source(Box::new(FileSource::new(input_path).line_based(true)))
                        .filter("min_size".to_string(), |item| {
                            let len = item.len();
                            Box::pin(async move { Ok(len > 5) })
                        })
                        .map("uppercase".to_string(), |item| {
                            Box::pin(async move {
                                let s = String::from_utf8(item).map_err(|e| {
                                    oxigdal_etl::error::TransformError::InvalidInput {
                                        message: e.to_string(),
                                    }
                                })?;
                                Ok(s.to_uppercase().into_bytes())
                            })
                        })
                        .sink(Box::new(FileSink::new(output_path)))
                        .build()
                        .expect("Failed to build pipeline");

                    let stats = pipeline.run().await.expect("Failed to run");
                    black_box(stats);
                })
            });
        });
    }

    group.finish();
}

fn bench_parallel_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("parallel_processing");

    for parallelism in [1, 2, 4, 8] {
        group.bench_with_input(
            BenchmarkId::from_parameter(parallelism),
            &parallelism,
            |b, &parallelism| {
                let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
                b.iter(|| {
                    runtime.block_on(async {
                        let temp_input = create_test_file(1000);
                        let input_path = temp_input.path().to_path_buf();

                        let temp_output = NamedTempFile::new().expect("Failed to create output");
                        let output_path = temp_output.path().to_path_buf();

                        let pipeline = Pipeline::builder()
                            .source(Box::new(FileSource::new(input_path).line_based(true)))
                            .max_parallelism(parallelism)
                            .sink(Box::new(FileSink::new(output_path)))
                            .build()
                            .expect("Failed to build pipeline");

                        let stats = pipeline.run().await.expect("Failed to run");
                        black_box(stats);
                    })
                });
            },
        );
    }

    group.finish();
}

fn bench_checkpointing(c: &mut Criterion) {
    let mut group = c.benchmark_group("checkpointing");

    group.bench_function("with_checkpointing", |b| {
        let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
        b.iter(|| {
            runtime.block_on(async {
                let temp_input = create_test_file(1000);
                let input_path = temp_input.path().to_path_buf();

                let temp_output = NamedTempFile::new().expect("Failed to create output");
                let output_path = temp_output.path().to_path_buf();

                let checkpoint_dir = tempfile::tempdir().expect("Failed to create dir");

                let pipeline = Pipeline::builder()
                    .source(Box::new(FileSource::new(input_path).line_based(true)))
                    .sink(Box::new(FileSink::new(output_path)))
                    .checkpoint_dir(checkpoint_dir.path().to_path_buf())
                    .build()
                    .expect("Failed to build pipeline");

                let stats = pipeline.run().await.expect("Failed to run");
                black_box(stats);
            })
        });
    });

    group.bench_function("without_checkpointing", |b| {
        let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
        b.iter(|| {
            runtime.block_on(async {
                let temp_input = create_test_file(1000);
                let input_path = temp_input.path().to_path_buf();

                let temp_output = NamedTempFile::new().expect("Failed to create output");
                let output_path = temp_output.path().to_path_buf();

                let pipeline = Pipeline::builder()
                    .source(Box::new(FileSource::new(input_path).line_based(true)))
                    .sink(Box::new(FileSink::new(output_path)))
                    .build()
                    .expect("Failed to build pipeline");

                let stats = pipeline.run().await.expect("Failed to run");
                black_box(stats);
            })
        });
    });

    group.finish();
}

fn bench_buffer_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer_sizes");

    for buffer_size in [100, 1000, 10000] {
        group.bench_with_input(
            BenchmarkId::from_parameter(buffer_size),
            &buffer_size,
            |b, &buffer_size| {
                let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
                b.iter(|| {
                    runtime.block_on(async {
                        let temp_input = create_test_file(5000);
                        let input_path = temp_input.path().to_path_buf();

                        let temp_output = NamedTempFile::new().expect("Failed to create output");
                        let output_path = temp_output.path().to_path_buf();

                        let pipeline = Pipeline::builder()
                            .source(Box::new(FileSource::new(input_path).line_based(true)))
                            .buffer_size(buffer_size)
                            .sink(Box::new(FileSink::new(output_path)))
                            .build()
                            .expect("Failed to build pipeline");

                        let stats = pipeline.run().await.expect("Failed to run");
                        black_box(stats);
                    })
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_file_source,
    bench_map_operator,
    bench_filter_operator,
    bench_json_transformation,
    bench_window_aggregation,
    bench_aggregate_stats,
    bench_pipeline_end_to_end,
    bench_parallel_processing,
    bench_checkpointing,
    bench_buffer_sizes
);

criterion_main!(benches);
