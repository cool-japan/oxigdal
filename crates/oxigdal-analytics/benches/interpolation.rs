//! Interpolation benchmarks
#![allow(missing_docs, clippy::expect_used, clippy::panic, clippy::unit_arg)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use oxigdal_analytics::interpolation::{
    IdwInterpolator, KrigingInterpolator, KrigingType, Variogram, VariogramModel,
};
use scirs2_core::ndarray::{Array1, Array2};
use std::hint::black_box;

fn generate_random_points(n: usize) -> (Array2<f64>, Array1<f64>) {
    use scirs2_core::random::thread_rng;
    let mut rng = thread_rng();

    let mut points = Vec::with_capacity(n * 2);
    let mut values = Vec::with_capacity(n);

    for _ in 0..n {
        points.push(rng.gen_range(0.0..100.0));
        points.push(rng.gen_range(0.0..100.0));
        values.push(rng.gen_range(0.0..10.0));
    }

    (
        Array2::from_shape_vec((n, 2), points)
            .expect("benchmark points array should be created from valid data"),
        Array1::from_vec(values),
    )
}

fn bench_idw(c: &mut Criterion) {
    let mut group = c.benchmark_group("idw");

    for n_points in [50, 100, 200].iter() {
        let (points, values) = generate_random_points(*n_points);
        let targets = Array2::from_shape_vec(
            (10, 2),
            vec![
                25.0, 25.0, 25.0, 75.0, 75.0, 25.0, 75.0, 75.0, 50.0, 50.0, 20.0, 80.0, 40.0, 60.0,
                60.0, 40.0, 80.0, 20.0, 30.0, 70.0,
            ],
        )
        .expect("benchmark target points array should be created from valid data");

        group.bench_with_input(
            BenchmarkId::new("interpolate", n_points),
            &(points, values, targets),
            |b, (points, values, targets)| {
                let interpolator = IdwInterpolator::new(2.0);
                b.iter(|| {
                    interpolator
                        .interpolate(
                            black_box(points),
                            black_box(&values.view()),
                            black_box(targets),
                        )
                        .expect("IDW interpolation should succeed in benchmark");
                });
            },
        );
    }

    group.finish();
}

fn bench_kriging(c: &mut Criterion) {
    let mut group = c.benchmark_group("kriging");

    for n_points in [20, 50, 100].iter() {
        let (points, values) = generate_random_points(*n_points);
        let targets = Array2::from_shape_vec(
            (5, 2),
            vec![25.0, 25.0, 75.0, 75.0, 50.0, 50.0, 30.0, 70.0, 70.0, 30.0],
        )
        .expect("benchmark target points array should be created from valid data");

        group.bench_with_input(
            BenchmarkId::new("interpolate", n_points),
            &(points, values, targets),
            |b, (points, values, targets)| {
                let variogram = Variogram::new(VariogramModel::Spherical, 0.1, 1.0, 10.0);
                let interpolator = KrigingInterpolator::new(KrigingType::Ordinary, variogram);
                b.iter(|| {
                    interpolator
                        .interpolate(
                            black_box(points),
                            black_box(&values.view()),
                            black_box(targets),
                        )
                        .expect("kriging interpolation should succeed in benchmark");
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_idw, bench_kriging);
criterion_main!(benches);
