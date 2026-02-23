//! Clustering benchmarks
#![allow(missing_docs, clippy::expect_used, clippy::panic, clippy::unit_arg)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use oxigdal_analytics::clustering::{DbscanClusterer, KMeansClusterer};
use scirs2_core::ndarray::Array2;
use scirs2_core::random::thread_rng;
use std::hint::black_box;

fn generate_clustered_data(
    n_clusters: usize,
    points_per_cluster: usize,
    spread: f64,
) -> Array2<f64> {
    let mut rng = thread_rng();
    let n_points = n_clusters * points_per_cluster;
    let mut data = Vec::with_capacity(n_points * 2);

    for cluster_id in 0..n_clusters {
        let center_x = (cluster_id as f64) * 10.0;
        let center_y = (cluster_id as f64) * 10.0;

        for _ in 0..points_per_cluster {
            let x = center_x + rng.gen_range(-spread..spread);
            let y = center_y + rng.gen_range(-spread..spread);
            data.push(x);
            data.push(y);
        }
    }

    Array2::from_shape_vec((n_points, 2), data)
        .expect("benchmark clustered data array should be created from valid data")
}

fn bench_kmeans(c: &mut Criterion) {
    let mut group = c.benchmark_group("kmeans");

    for n_points in [100, 500, 1000].iter() {
        let data = generate_clustered_data(3, n_points / 3, 1.0);

        group.bench_with_input(BenchmarkId::new("fit", n_points), &data, |b, data| {
            let clusterer = KMeansClusterer::new(3, 100, 1e-4);
            b.iter(|| {
                clusterer
                    .fit(&black_box(data.view()))
                    .expect("k-means clustering should succeed in benchmark");
            });
        });
    }

    group.finish();
}

fn bench_dbscan(c: &mut Criterion) {
    let mut group = c.benchmark_group("dbscan");

    for n_points in [100, 200, 500].iter() {
        let data = generate_clustered_data(3, n_points / 3, 1.0);

        group.bench_with_input(BenchmarkId::new("fit", n_points), &data, |b, data| {
            let clusterer = DbscanClusterer::new(2.0, 3);
            b.iter(|| {
                clusterer
                    .fit(&black_box(data.view()))
                    .expect("DBSCAN clustering should succeed in benchmark");
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_kmeans, bench_dbscan);
criterion_main!(benches);
