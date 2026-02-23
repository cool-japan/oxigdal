//! Comprehensive benchmarks for GPU advanced features.
#![allow(
    missing_docs,
    clippy::expect_used,
    clippy::panic,
    clippy::unit_arg,
    clippy::manual_div_ceil,
    clippy::useless_vec
)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use oxigdal_gpu_advanced::shader_compiler::ShaderCompiler;
use std::hint::black_box;
use std::time::Duration;

fn bench_shader_compilation(c: &mut Criterion) {
    let mut group = c.benchmark_group("shader_compilation");
    group.measurement_time(Duration::from_secs(10));

    let simple_shader = r#"
@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    // Simple shader
}
    "#;

    let complex_shader = r#"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;
@group(0) @binding(2) var<uniform> params: Params;

struct Params {
    width: u32,
    height: u32,
    channels: u32,
}

@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let x = global_id.x;
    let y = global_id.y;

    if (x >= params.width || y >= params.height) {
        return;
    }

    for (var c = 0u; c < params.channels; c++) {
        let idx = (y * params.width + x) * params.channels + c;
        var sum = 0.0;

        // 3x3 convolution
        for (var dy = -1; dy <= 1; dy++) {
            for (var dx = -1; dx <= 1; dx++) {
                let nx = i32(x) + dx;
                let ny = i32(y) + dy;

                if (nx >= 0 && nx < i32(params.width) && ny >= 0 && ny < i32(params.height)) {
                    let nidx = (u32(ny) * params.width + u32(nx)) * params.channels + c;
                    sum += input[nidx];
                }
            }
        }

        output[idx] = sum / 9.0;
    }
}
    "#;

    group.bench_function("simple", |b| {
        let compiler = ShaderCompiler::new();
        b.iter(|| {
            let _ = compiler.compile(black_box(simple_shader));
        });
    });

    group.bench_function("complex", |b| {
        let compiler = ShaderCompiler::new();
        b.iter(|| {
            let _ = compiler.compile(black_box(complex_shader));
        });
    });

    group.bench_function("with_cache", |b| {
        let compiler = ShaderCompiler::new();
        // Prime the cache
        let _ = compiler.compile(simple_shader);

        b.iter(|| {
            let _ = compiler.compile(black_box(simple_shader));
        });
    });

    group.bench_function("with_optimization", |b| {
        let compiler = ShaderCompiler::new();
        b.iter(|| {
            let _ = compiler.compile_optimized(black_box(simple_shader));
        });
    });

    group.finish();
}

fn bench_shader_preprocessing(c: &mut Criterion) {
    use oxigdal_gpu_advanced::shader_compiler::ShaderPreprocessor;

    let mut group = c.benchmark_group("shader_preprocessing");

    let source_with_macros = r#"
@compute @workgroup_size($WORKGROUP_X, $WORKGROUP_Y, $WORKGROUP_Z)
fn main() {
    var<workgroup> shared: array<f32, $BUFFER_SIZE>;
    let scale = $SCALE_FACTOR;
    let offset = $OFFSET_VALUE;
}
    "#;

    group.bench_function("preprocess", |b| {
        let mut preprocessor = ShaderPreprocessor::new();
        preprocessor.define("WORKGROUP_X", "16");
        preprocessor.define("WORKGROUP_Y", "16");
        preprocessor.define("WORKGROUP_Z", "1");
        preprocessor.define("BUFFER_SIZE", "256");
        preprocessor.define("SCALE_FACTOR", "2.0");
        preprocessor.define("OFFSET_VALUE", "0.5");

        b.iter(|| {
            let _ = preprocessor.preprocess(black_box(source_with_macros));
        });
    });

    group.finish();
}

fn bench_memory_alignment(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_alignment");

    for alignment in [16, 64, 256, 1024].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(alignment),
            alignment,
            |b, &alignment| {
                b.iter(|| {
                    let size = black_box(1000u64);
                    let aligned = ((size + alignment - 1) / alignment) * alignment;
                    black_box(aligned)
                });
            },
        );
    }

    group.finish();
}

fn bench_load_balancing_selection(c: &mut Criterion) {
    let mut group = c.benchmark_group("load_balancing");
    group.measurement_time(Duration::from_secs(5));

    // Simulate device selection decision
    group.bench_function("best_score_calculation", |b| {
        let workloads = vec![0.1f32, 0.5f32, 0.3f32, 0.9f32, 0.2f32];
        let type_scores = vec![1.0f32, 0.7f32, 1.0f32, 0.5f32, 1.0f32];

        b.iter(|| {
            let scores: Vec<f32> = workloads
                .iter()
                .zip(type_scores.iter())
                .map(|(&workload, &type_score)| type_score * (1.0 - workload))
                .collect();

            let max_idx = scores
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(idx, _)| idx);

            black_box(max_idx)
        });
    });

    group.finish();
}

fn bench_hash_calculation(c: &mut Criterion) {
    let mut group = c.benchmark_group("hash_calculation");

    let small_shader = "fn main() {}";
    let medium_shader = r#"
@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let x = global_id.x;
    let y = global_id.y;
}
    "#;
    let large_shader = medium_shader.repeat(10);

    group.bench_function("small", |b| {
        b.iter(|| {
            let hash = blake3::hash(black_box(small_shader.as_bytes()));
            black_box(hash)
        });
    });

    group.bench_function("medium", |b| {
        b.iter(|| {
            let hash = blake3::hash(black_box(medium_shader.as_bytes()));
            black_box(hash)
        });
    });

    group.bench_function("large", |b| {
        b.iter(|| {
            let hash = blake3::hash(black_box(large_shader.as_bytes()));
            black_box(hash)
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_shader_compilation,
    bench_shader_preprocessing,
    bench_memory_alignment,
    bench_load_balancing_selection,
    bench_hash_calculation,
);
criterion_main!(benches);
