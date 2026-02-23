//! Query benchmarks.

#![allow(missing_docs)]
#![allow(clippy::panic)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use oxigdal_query::executor::scan::{
    ColumnData, DataType, Field, MemoryDataSource, RecordBatch, Schema,
};
use oxigdal_query::{Optimizer, QueryEngine};
use std::hint::black_box;
use std::sync::Arc;

fn create_benchmark_dataset(size: usize) -> (Arc<Schema>, Vec<RecordBatch>) {
    let schema = Arc::new(Schema::new(vec![
        Field::new("id".to_string(), DataType::Int64, false),
        Field::new("value".to_string(), DataType::Float64, false),
        Field::new("category".to_string(), DataType::String, false),
    ]));

    let ids: Vec<Option<i64>> = (0..size).map(|i| Some(i as i64)).collect();
    let values: Vec<Option<f64>> = (0..size).map(|i| Some((i as f64) * 1.5)).collect();
    let categories: Vec<Option<String>> =
        (0..size).map(|i| Some(format!("cat{}", i % 10))).collect();

    let columns = vec![
        ColumnData::Int64(ids),
        ColumnData::Float64(values),
        ColumnData::String(categories),
    ];

    let batch = RecordBatch::new(schema.clone(), columns, size)
        .ok()
        .unwrap_or_else(|| {
            panic!("Failed to create batch");
        });

    (schema, vec![batch])
}

fn bench_parser(c: &mut Criterion) {
    let mut group = c.benchmark_group("parser");

    let queries = vec![
        "SELECT * FROM test",
        "SELECT id, value FROM test WHERE value > 100",
        "SELECT COUNT(*), AVG(value) FROM test GROUP BY category",
        "SELECT * FROM test ORDER BY value DESC LIMIT 10",
    ];

    for query in queries {
        group.bench_with_input(BenchmarkId::from_parameter(query), query, |b, q| {
            b.iter(|| {
                let _ = oxigdal_query::parse_sql(black_box(q));
            });
        });
    }

    group.finish();
}

fn bench_optimizer(c: &mut Criterion) {
    let mut group = c.benchmark_group("optimizer");

    let optimizer = Optimizer::new();
    let sql = "SELECT * FROM test WHERE 1 + 1 = 2 AND value > 100";
    let stmt = oxigdal_query::parse_sql(sql).ok();

    if let Some(stmt) = stmt {
        group.bench_function("optimize_simple_query", |b| {
            b.iter(|| {
                let _ = optimizer.optimize(black_box(stmt.clone()));
            });
        });
    }

    group.finish();
}

fn bench_executor(c: &mut Criterion) {
    let mut group = c.benchmark_group("executor");

    let runtime = tokio::runtime::Runtime::new().ok();

    if let Some(rt) = runtime {
        for size in [100, 1000, 10000].iter() {
            let (schema, batches) = create_benchmark_dataset(*size);
            let source = Arc::new(MemoryDataSource::new(schema, batches));

            group.bench_with_input(BenchmarkId::new("scan", size), size, |b, _| {
                b.iter(|| {
                    let mut engine = QueryEngine::new();
                    engine.register_data_source("test".to_string(), source.clone());

                    rt.block_on(async {
                        let sql = "SELECT * FROM test";
                        let _ = engine.execute_sql(black_box(sql)).await;
                    });
                });
            });

            group.bench_with_input(BenchmarkId::new("filter", size), size, |b, _| {
                b.iter(|| {
                    let mut engine = QueryEngine::new();
                    engine.register_data_source("test".to_string(), source.clone());

                    rt.block_on(async {
                        let sql = "SELECT * FROM test WHERE value > 100.0";
                        let _ = engine.execute_sql(black_box(sql)).await;
                    });
                });
            });

            group.bench_with_input(BenchmarkId::new("sort", size), size, |b, _| {
                b.iter(|| {
                    let mut engine = QueryEngine::new();
                    engine.register_data_source("test".to_string(), source.clone());

                    rt.block_on(async {
                        let sql = "SELECT * FROM test ORDER BY value DESC";
                        let _ = engine.execute_sql(black_box(sql)).await;
                    });
                });
            });

            group.bench_with_input(BenchmarkId::new("limit", size), size, |b, _| {
                b.iter(|| {
                    let mut engine = QueryEngine::new();
                    engine.register_data_source("test".to_string(), source.clone());

                    rt.block_on(async {
                        let sql = "SELECT * FROM test LIMIT 10";
                        let _ = engine.execute_sql(black_box(sql)).await;
                    });
                });
            });
        }
    }

    group.finish();
}

fn bench_cache(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache");

    let runtime = tokio::runtime::Runtime::new().ok();

    if let Some(rt) = runtime {
        let (schema, batches) = create_benchmark_dataset(1000);
        let source = Arc::new(MemoryDataSource::new(schema, batches));

        group.bench_function("cache_hit", |b| {
            let mut engine = QueryEngine::new();
            engine.register_data_source("test".to_string(), source.clone());

            // Warm up cache
            rt.block_on(async {
                let _ = engine.execute_sql("SELECT * FROM test").await;
            });

            b.iter(|| {
                rt.block_on(async {
                    let sql = "SELECT * FROM test";
                    let _ = engine.execute_sql(black_box(sql)).await;
                });
            });
        });

        group.bench_function("cache_miss", |b| {
            b.iter(|| {
                let mut engine = QueryEngine::new();
                engine.register_data_source("test".to_string(), source.clone());

                rt.block_on(async {
                    let sql = "SELECT * FROM test";
                    let _ = engine.execute_sql(black_box(sql)).await;
                });
            });
        });
    }

    group.finish();
}

fn bench_complex_query(c: &mut Criterion) {
    let mut group = c.benchmark_group("complex_query");

    let runtime = tokio::runtime::Runtime::new().ok();

    if let Some(rt) = runtime {
        let (schema, batches) = create_benchmark_dataset(5000);
        let source = Arc::new(MemoryDataSource::new(schema, batches));

        group.bench_function("complex", |b| {
            b.iter(|| {
                let mut engine = QueryEngine::new();
                engine.register_data_source("test".to_string(), source.clone());

                rt.block_on(async {
                    let sql = "SELECT category, AVG(value) FROM test WHERE value > 100 GROUP BY category ORDER BY AVG(value) DESC LIMIT 5";
                    let _ = engine.execute_sql(black_box(sql)).await;
                });
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_parser,
    bench_optimizer,
    bench_executor,
    bench_cache,
    bench_complex_query,
);
criterion_main!(benches);
