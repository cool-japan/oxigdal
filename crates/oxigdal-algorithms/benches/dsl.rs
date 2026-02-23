//! Benchmarks for the Raster Algebra DSL
#![allow(
    missing_docs,
    clippy::expect_used,
    clippy::panic,
    clippy::unit_arg,
    clippy::unnecessary_cast
)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_algorithms::dsl::{OptLevel, RasterDsl};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use std::hint::black_box;

fn create_test_bands(width: u64, height: u64, num_bands: usize) -> Vec<RasterBuffer> {
    let mut bands = Vec::new();
    for i in 0..num_bands {
        let mut band = RasterBuffer::zeros(width, height, RasterDataType::Float32);
        for y in 0..height {
            for x in 0..width {
                let value = ((x + y + i as u64) % 100) as f64 + 1.0;
                let _ = band.set_pixel(x, y, value);
            }
        }
        bands.push(band);
    }
    bands
}

fn bench_simple_arithmetic(c: &mut Criterion) {
    let mut group = c.benchmark_group("simple_arithmetic");

    for size in [100, 500, 1000].iter() {
        let bands = create_test_bands(*size, *size, 2);
        let dsl = RasterDsl::new();

        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_with_input(BenchmarkId::new("add", size), size, |b, _| {
            b.iter(|| {
                let _ = dsl.execute(black_box("B1 + B2"), black_box(&bands));
            });
        });
    }

    group.finish();
}

fn bench_ndvi(c: &mut Criterion) {
    let mut group = c.benchmark_group("ndvi");

    for size in [100, 500, 1000].iter() {
        let bands = create_test_bands(*size, *size, 2);
        let dsl = RasterDsl::new();

        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_with_input(BenchmarkId::new("calculate", size), size, |b, _| {
            b.iter(|| {
                let _ = dsl.execute(black_box("(B1 - B2) / (B1 + B2)"), black_box(&bands));
            });
        });
    }

    group.finish();
}

fn bench_complex_expression(c: &mut Criterion) {
    let bands = create_test_bands(500, 500, 8);
    let dsl = RasterDsl::new();

    let program = r#"
        let ndvi = (B8 - B4) / (B8 + B4);
        let evi = 2.5 * ((B8 - B4) / (B8 + 6*B4 - 7.5*B2 + 1));
        if ndvi > 0.6 && evi > 0.5 then 1.0 else if ndvi > 0.3 then 0.5 else 0.0
    "#;

    c.bench_function("complex_vegetation_analysis", |b| {
        b.iter(|| {
            let _ = dsl.execute(black_box(program), black_box(&bands));
        });
    });
}

fn bench_optimization_levels(c: &mut Criterion) {
    let mut group = c.benchmark_group("optimization_levels");
    let bands = create_test_bands(500, 500, 1);

    for level in [
        OptLevel::None,
        OptLevel::Basic,
        OptLevel::Standard,
        OptLevel::Aggressive,
    ]
    .iter()
    {
        let mut dsl = RasterDsl::new();
        dsl.set_opt_level(*level);

        group.bench_with_input(
            BenchmarkId::new("expression", format!("{:?}", level)),
            level,
            |b, _| {
                b.iter(|| {
                    let _ = dsl.execute(black_box("B1 * 2 + 5 - 3"), black_box(&bands));
                });
            },
        );
    }

    group.finish();
}

fn bench_compile_vs_interpret(c: &mut Criterion) {
    let mut group = c.benchmark_group("compile_vs_interpret");
    let bands = create_test_bands(500, 500, 2);
    let dsl = RasterDsl::new();
    let expr = "(B1 - B2) / (B1 + B2)";

    group.bench_function("interpret_each_time", |b| {
        b.iter(|| {
            let _ = dsl.execute(black_box(expr), black_box(&bands));
        });
    });

    let compiled = dsl.compile(expr).expect("Should compile");
    group.bench_function("pre_compiled", |b| {
        b.iter(|| {
            let _ = compiled.execute(black_box(&bands));
        });
    });

    group.finish();
}

fn bench_mathematical_functions(c: &mut Criterion) {
    let mut group = c.benchmark_group("mathematical_functions");
    let bands = create_test_bands(500, 500, 1);
    let dsl = RasterDsl::new();

    for func in ["sqrt", "sin", "cos", "log", "exp"].iter() {
        let expr = format!("{}(B1)", func);
        group.bench_with_input(BenchmarkId::new("function", func), func, |b, _| {
            b.iter(|| {
                let _ = dsl.execute(black_box(&expr), black_box(&bands));
            });
        });
    }

    group.finish();
}

fn bench_conditional_expressions(c: &mut Criterion) {
    let bands = create_test_bands(500, 500, 1);
    let dsl = RasterDsl::new();

    c.bench_function("simple_conditional", |b| {
        b.iter(|| {
            let _ = dsl.execute(black_box("if B1 > 50 then 1 else 0"), black_box(&bands));
        });
    });

    c.bench_function("nested_conditional", |b| {
        b.iter(|| {
            let _ = dsl.execute(
                black_box("if B1 > 75 then 1 else if B1 > 50 then 0.5 else 0"),
                black_box(&bands),
            );
        });
    });
}

fn bench_variable_declarations(c: &mut Criterion) {
    let bands = create_test_bands(500, 500, 2);
    let dsl = RasterDsl::new();

    let program = r#"
        let a = B1 + B2;
        let b = B1 - B2;
        let c = a * b;
        c;
    "#;

    c.bench_function("variable_declarations", |b| {
        b.iter(|| {
            let _ = dsl.execute(black_box(program), black_box(&bands));
        });
    });
}

fn bench_parser_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("parser_overhead");
    let dsl = RasterDsl::new();

    group.bench_function("parse_simple", |b| {
        b.iter(|| {
            let _ = dsl.compile(black_box("B1 + B2"));
        });
    });

    group.bench_function("parse_complex", |b| {
        b.iter(|| {
            let _ = dsl.compile(black_box("(B1 - B2) / (B1 + B2)"));
        });
    });

    let program = r#"
        let ndvi = (B8 - B4) / (B8 + B4);
        let evi = 2.5 * ((B8 - B4) / (B8 + 6*B4 - 7.5*B2 + 1));
        if ndvi > 0.6 && evi > 0.5 then 1.0 else 0.0
    "#;

    group.bench_function("parse_program", |b| {
        b.iter(|| {
            let _ = dsl.compile(black_box(program));
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_simple_arithmetic,
    bench_ndvi,
    bench_complex_expression,
    bench_optimization_levels,
    bench_compile_vs_interpret,
    bench_mathematical_functions,
    bench_conditional_expressions,
    bench_variable_declarations,
    bench_parser_overhead,
);

criterion_main!(benches);
