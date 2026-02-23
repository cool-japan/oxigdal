//! Comprehensive benchmarks for spatial clustering algorithms
//!
//! This benchmark suite tests K-means, DBSCAN, and hierarchical clustering
//! algorithms on various point distributions and dataset sizes.
//!
//! ## Performance Characteristics
//!
//! - **K-means**: O(n * k * i) where n=points, k=clusters, i=iterations
//! - **K-means++**: O(n * k) initialization, improves quality
//! - **DBSCAN**: O(n log n) with spatial indexing, O(n²) worst case
//! - **Hierarchical**: O(n² log n) with priority queue, O(n³) naive
//!
//! ## Point Distribution Patterns
//!
//! - **Clustered**: Well-separated gaussian clusters (ideal case)
//! - **Uniform**: Random uniform distribution (challenging)
//! - **Mixed**: Combination of clusters and noise
//! - **Geographic**: Realistic lat/lon coordinates
#![allow(
    missing_docs,
    clippy::expect_used,
    clippy::panic,
    clippy::unit_arg,
    clippy::unnecessary_cast
)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_algorithms::vector::clustering::{
    DbscanOptions, DistanceMetric, HierarchicalOptions, InitMethod, KmeansOptions, LinkageMethod,
    dbscan_cluster, hierarchical_cluster, kmeans_cluster, kmeans_plus_plus_init,
};
use oxigdal_core::vector::Point;
use std::hint::black_box;

/// Point distribution pattern for test data
#[derive(Debug, Clone, Copy)]
enum DistributionPattern {
    /// Well-separated gaussian clusters
    Clustered,
    /// Uniform random distribution
    Uniform,
    /// Mixed clusters with noise
    Mixed,
    /// Geographic coordinates (lat/lon)
    Geographic,
}

/// Creates test point data with specified pattern
fn create_point_distribution(
    point_count: usize,
    num_clusters: usize,
    pattern: DistributionPattern,
) -> Vec<Point> {
    let mut points = Vec::with_capacity(point_count);

    match pattern {
        DistributionPattern::Clustered => {
            // Create well-separated gaussian clusters
            let points_per_cluster = point_count / num_clusters;

            for cluster_id in 0..num_clusters {
                let center_x = (cluster_id as f64 * 100.0) % 1000.0;
                let center_y = ((cluster_id * 73) as f64 * 100.0) % 1000.0;

                for i in 0..points_per_cluster {
                    // Gaussian-like distribution around center
                    let angle = (i as f64 / points_per_cluster as f64) * 2.0 * std::f64::consts::PI;
                    let radius = ((i * 17) % 20) as f64;

                    let x = center_x + radius * angle.cos();
                    let y = center_y + radius * angle.sin();

                    points.push(Point::new(x, y));
                }
            }
        }

        DistributionPattern::Uniform => {
            // Uniform random distribution
            for i in 0..point_count {
                let x = ((i * 173) % 1000) as f64;
                let y = ((i * 271) % 1000) as f64;
                points.push(Point::new(x, y));
            }
        }

        DistributionPattern::Mixed => {
            // Mix of clusters and noise
            let cluster_points = (point_count * 3) / 4;
            let noise_points = point_count - cluster_points;

            // Create clusters
            let points_per_cluster = cluster_points / num_clusters;
            for cluster_id in 0..num_clusters {
                let center_x = (cluster_id as f64 * 150.0) % 1000.0;
                let center_y = ((cluster_id * 97) as f64 * 150.0) % 1000.0;

                for i in 0..points_per_cluster {
                    let angle = (i as f64 / points_per_cluster as f64) * 2.0 * std::f64::consts::PI;
                    let radius = ((i * 13) % 15) as f64;

                    let x = center_x + radius * angle.cos();
                    let y = center_y + radius * angle.sin();

                    points.push(Point::new(x, y));
                }
            }

            // Add noise points
            for i in 0..noise_points {
                let x = ((i * 199) % 1000) as f64;
                let y = ((i * 307) % 1000) as f64;
                points.push(Point::new(x, y));
            }
        }

        DistributionPattern::Geographic => {
            // Realistic geographic coordinates (lat/lon)
            let base_lat = 40.0; // Around NYC
            let base_lon = -74.0;

            for i in 0..point_count {
                let cluster_id = i % num_clusters;
                let offset_lat = (cluster_id as f64 * 0.5) % 2.0;
                let offset_lon = ((cluster_id * 73) as f64 * 0.5) % 2.0;

                // Small variations within cluster
                let noise_lat = ((i * 17) % 100) as f64 / 10000.0;
                let noise_lon = ((i * 23) % 100) as f64 / 10000.0;

                let lat = base_lat + offset_lat + noise_lat;
                let lon = base_lon + offset_lon + noise_lon;

                points.push(Point::new(lon, lat));
            }
        }
    }

    points
}

/// Benchmark K-means clustering with varying data sizes
///
/// Time complexity: O(n * k * i)
fn bench_kmeans_size(c: &mut Criterion) {
    let mut group = c.benchmark_group("kmeans_size");

    for &point_count in &[100, 1000, 10000, 100000] {
        let points = create_point_distribution(point_count, 5, DistributionPattern::Clustered);

        let options = KmeansOptions {
            k: 5,
            max_iterations: 100,
            tolerance: 1e-6,
            metric: DistanceMetric::Euclidean,
            init_method: InitMethod::KMeansPlusPlus,
            seed: Some(42),
        };

        group.throughput(Throughput::Elements(point_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(point_count),
            &point_count,
            |b, _| {
                b.iter(|| {
                    kmeans_cluster(black_box(&points), black_box(&options)).expect("K-means failed")
                });
            },
        );
    }

    group.finish();
}

/// Benchmark K-means with varying number of clusters
fn bench_kmeans_k(c: &mut Criterion) {
    let mut group = c.benchmark_group("kmeans_k_values");

    let point_count = 10000;
    let points = create_point_distribution(point_count, 10, DistributionPattern::Clustered);

    for &k in &[3, 5, 10, 20, 50] {
        let options = KmeansOptions {
            k,
            max_iterations: 100,
            tolerance: 1e-6,
            metric: DistanceMetric::Euclidean,
            init_method: InitMethod::KMeansPlusPlus,
            seed: Some(42),
        };

        group.throughput(Throughput::Elements(point_count as u64));
        group.bench_with_input(BenchmarkId::from_parameter(k), &k, |b, _| {
            b.iter(|| {
                kmeans_cluster(black_box(&points), black_box(&options)).expect("K-means failed")
            });
        });
    }

    group.finish();
}

/// Benchmark K-means initialization methods
fn bench_kmeans_init(c: &mut Criterion) {
    let mut group = c.benchmark_group("kmeans_init_methods");

    let point_count = 10000;
    let points = create_point_distribution(point_count, 5, DistributionPattern::Clustered);
    let k = 5;

    group.throughput(Throughput::Elements(point_count as u64));

    // Random initialization
    let random_options = KmeansOptions {
        k,
        max_iterations: 100,
        tolerance: 1e-6,
        metric: DistanceMetric::Euclidean,
        init_method: InitMethod::Random,
        seed: Some(42),
    };

    group.bench_function("random", |b| {
        b.iter(|| {
            kmeans_cluster(black_box(&points), black_box(&random_options)).expect("K-means failed")
        });
    });

    // K-means++ initialization
    let plusplus_options = KmeansOptions {
        k,
        max_iterations: 100,
        tolerance: 1e-6,
        metric: DistanceMetric::Euclidean,
        init_method: InitMethod::KMeansPlusPlus,
        seed: Some(42),
    };

    group.bench_function("kmeans++", |b| {
        b.iter(|| {
            kmeans_cluster(black_box(&points), black_box(&plusplus_options))
                .expect("K-means failed")
        });
    });

    // Just initialization (no iterations)
    group.bench_function("plusplus_init_only", |b| {
        b.iter(|| {
            kmeans_plus_plus_init(
                black_box(&points),
                black_box(k),
                black_box(DistanceMetric::Euclidean),
            )
            .expect("Init failed")
        });
    });

    group.finish();
}

/// Benchmark DBSCAN with varying data sizes
///
/// Time complexity: O(n log n) with spatial indexing
fn bench_dbscan_size(c: &mut Criterion) {
    let mut group = c.benchmark_group("dbscan_size");

    for &point_count in &[100, 1000, 10000, 50000] {
        let points = create_point_distribution(point_count, 5, DistributionPattern::Mixed);

        let options = DbscanOptions {
            epsilon: 25.0,
            min_points: 5,
            metric: DistanceMetric::Euclidean,
        };

        group.throughput(Throughput::Elements(point_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(point_count),
            &point_count,
            |b, _| {
                b.iter(|| {
                    dbscan_cluster(black_box(&points), black_box(&options)).expect("DBSCAN failed")
                });
            },
        );
    }

    group.finish();
}

/// Benchmark DBSCAN with varying epsilon values
fn bench_dbscan_epsilon(c: &mut Criterion) {
    let mut group = c.benchmark_group("dbscan_epsilon");

    let point_count = 5000;
    let points = create_point_distribution(point_count, 5, DistributionPattern::Mixed);

    for &epsilon in &[10.0, 25.0, 50.0, 100.0] {
        let options = DbscanOptions {
            epsilon,
            min_points: 5,
            metric: DistanceMetric::Euclidean,
        };

        group.throughput(Throughput::Elements(point_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(epsilon as usize),
            &epsilon,
            |b, _| {
                b.iter(|| {
                    dbscan_cluster(black_box(&points), black_box(&options)).expect("DBSCAN failed")
                });
            },
        );
    }

    group.finish();
}

/// Benchmark DBSCAN with different min_points values
fn bench_dbscan_minpts(c: &mut Criterion) {
    let mut group = c.benchmark_group("dbscan_minpts");

    let point_count = 5000;
    let points = create_point_distribution(point_count, 5, DistributionPattern::Mixed);

    for &min_points in &[3, 5, 10, 20] {
        let options = DbscanOptions {
            epsilon: 25.0,
            min_points,
            metric: DistanceMetric::Euclidean,
        };

        group.throughput(Throughput::Elements(point_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(min_points),
            &min_points,
            |b, _| {
                b.iter(|| {
                    dbscan_cluster(black_box(&points), black_box(&options)).expect("DBSCAN failed")
                });
            },
        );
    }

    group.finish();
}

/// Benchmark hierarchical clustering with varying data sizes
///
/// Time complexity: O(n² log n)
/// Note: Hierarchical is computationally expensive, using smaller datasets
fn bench_hierarchical_size(c: &mut Criterion) {
    let mut group = c.benchmark_group("hierarchical_size");
    group.sample_size(10); // Reduce sample size due to computational cost

    for &point_count in &[50, 100, 250, 500] {
        let points = create_point_distribution(point_count, 5, DistributionPattern::Clustered);

        let options = HierarchicalOptions {
            num_clusters: 5,
            linkage: LinkageMethod::Average,
            metric: DistanceMetric::Euclidean,
            distance_threshold: None,
        };

        group.throughput(Throughput::Elements(point_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(point_count),
            &point_count,
            |b, _| {
                b.iter(|| {
                    hierarchical_cluster(black_box(&points), black_box(&options))
                        .expect("Hierarchical failed")
                });
            },
        );
    }

    group.finish();
}

/// Benchmark hierarchical clustering linkage methods
fn bench_hierarchical_linkage(c: &mut Criterion) {
    let mut group = c.benchmark_group("hierarchical_linkage");
    group.sample_size(10);

    let point_count = 200;
    let points = create_point_distribution(point_count, 5, DistributionPattern::Clustered);

    let linkage_methods = [
        ("single", LinkageMethod::Single),
        ("complete", LinkageMethod::Complete),
        ("average", LinkageMethod::Average),
        ("ward", LinkageMethod::Ward),
    ];

    group.throughput(Throughput::Elements(point_count as u64));

    for (name, linkage) in linkage_methods.iter() {
        let options = HierarchicalOptions {
            num_clusters: 5,
            linkage: *linkage,
            metric: DistanceMetric::Euclidean,
            distance_threshold: None,
        };

        group.bench_function(*name, |b| {
            b.iter(|| {
                hierarchical_cluster(black_box(&points), black_box(&options))
                    .expect("Hierarchical failed")
            });
        });
    }

    group.finish();
}

/// Benchmark distance metrics comparison
fn bench_distance_metrics(c: &mut Criterion) {
    let mut group = c.benchmark_group("distance_metrics");

    let point_count = 5000;

    // Euclidean metric
    let euclidean_points =
        create_point_distribution(point_count, 5, DistributionPattern::Clustered);
    let euclidean_options = KmeansOptions {
        k: 5,
        max_iterations: 100,
        tolerance: 1e-6,
        metric: DistanceMetric::Euclidean,
        init_method: InitMethod::KMeansPlusPlus,
        seed: Some(42),
    };

    group.throughput(Throughput::Elements(point_count as u64));
    group.bench_function("euclidean", |b| {
        b.iter(|| {
            kmeans_cluster(black_box(&euclidean_points), black_box(&euclidean_options))
                .expect("K-means failed")
        });
    });

    // Haversine metric (for geographic data)
    let geographic_points =
        create_point_distribution(point_count, 5, DistributionPattern::Geographic);
    let haversine_options = KmeansOptions {
        k: 5,
        max_iterations: 100,
        tolerance: 1e-6,
        metric: DistanceMetric::Haversine,
        init_method: InitMethod::KMeansPlusPlus,
        seed: Some(42),
    };

    group.bench_function("haversine", |b| {
        b.iter(|| {
            kmeans_cluster(black_box(&geographic_points), black_box(&haversine_options))
                .expect("K-means failed")
        });
    });

    group.finish();
}

/// Benchmark different point distribution patterns
fn bench_distribution_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("distribution_patterns");

    let point_count = 10000;
    let k = 5;

    let patterns = [
        ("clustered", DistributionPattern::Clustered),
        ("uniform", DistributionPattern::Uniform),
        ("mixed", DistributionPattern::Mixed),
        ("geographic", DistributionPattern::Geographic),
    ];

    group.throughput(Throughput::Elements(point_count as u64));

    for (name, pattern) in patterns.iter() {
        let points = create_point_distribution(point_count, k, *pattern);
        let options = KmeansOptions {
            k,
            max_iterations: 100,
            tolerance: 1e-6,
            metric: if matches!(pattern, DistributionPattern::Geographic) {
                DistanceMetric::Haversine
            } else {
                DistanceMetric::Euclidean
            },
            init_method: InitMethod::KMeansPlusPlus,
            seed: Some(42),
        };

        group.bench_function(*name, |b| {
            b.iter(|| {
                kmeans_cluster(black_box(&points), black_box(&options)).expect("K-means failed")
            });
        });
    }

    group.finish();
}

/// Benchmark algorithm comparison on the same dataset
fn bench_algorithm_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("clustering_algorithm_comparison");

    let point_count = 2000;
    let points = create_point_distribution(point_count, 5, DistributionPattern::Clustered);

    group.throughput(Throughput::Elements(point_count as u64));

    // K-means
    group.bench_function("kmeans", |b| {
        let options = KmeansOptions {
            k: 5,
            max_iterations: 100,
            tolerance: 1e-6,
            metric: DistanceMetric::Euclidean,
            init_method: InitMethod::KMeansPlusPlus,
            seed: Some(42),
        };
        b.iter(|| kmeans_cluster(black_box(&points), black_box(&options)).expect("K-means failed"));
    });

    // DBSCAN
    group.bench_function("dbscan", |b| {
        let options = DbscanOptions {
            epsilon: 25.0,
            min_points: 5,
            metric: DistanceMetric::Euclidean,
        };
        b.iter(|| dbscan_cluster(black_box(&points), black_box(&options)).expect("DBSCAN failed"));
    });

    // Hierarchical (smaller sample size)
    group.sample_size(10);
    let small_points = create_point_distribution(200, 5, DistributionPattern::Clustered);
    group.bench_function("hierarchical", |b| {
        let options = HierarchicalOptions {
            num_clusters: 5,
            linkage: LinkageMethod::Average,
            metric: DistanceMetric::Euclidean,
            distance_threshold: None,
        };
        b.iter(|| {
            hierarchical_cluster(black_box(&small_points), black_box(&options))
                .expect("Hierarchical failed")
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_kmeans_size,
    bench_kmeans_k,
    bench_kmeans_init,
    bench_dbscan_size,
    bench_dbscan_epsilon,
    bench_dbscan_minpts,
    bench_hierarchical_size,
    bench_hierarchical_linkage,
    bench_distance_metrics,
    bench_distribution_patterns,
    bench_algorithm_comparison,
);

criterion_main!(benches);
