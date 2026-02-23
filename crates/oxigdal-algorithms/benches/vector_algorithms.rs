#![allow(
    missing_docs,
    clippy::expect_used,
    clippy::panic,
    clippy::unit_arg,
    clippy::unnecessary_cast,
    dead_code
)]
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_algorithms::vector::{
    AreaMethod, DistanceMethod, SimplifyMethod, area_polygon, buffer_linestring, buffer_point,
    buffer_polygon, centroid_polygon, difference_polygon, distance_point_to_point,
    intersect_polygons, point_in_polygon, simplify_linestring, union_polygon, validate_linestring,
    validate_polygon,
};
use oxigdal_core::vector::{Coordinate, LineString, Point, Polygon};
use std::hint::black_box;

// Helper function to create a complex polygon
fn create_complex_polygon(num_points: usize) -> Polygon {
    use oxigdal_core::vector::Coordinate;

    let mut coords: Vec<Coordinate> = (0..num_points)
        .map(|i| {
            let angle = 2.0 * std::f64::consts::PI * i as f64 / num_points as f64;
            let radius = 100.0 + 50.0 * (angle * 3.0).sin();
            Coordinate::new_2d(radius * angle.cos(), radius * angle.sin())
        })
        .collect();

    coords.push(coords[0]);
    let ring = LineString::new(coords).expect("Failed to create ring");
    Polygon::new(ring, vec![]).expect("Failed to create polygon")
}

// Helper function to create a complex linestring
fn create_complex_linestring(num_points: usize) -> LineString {
    let coords: Vec<Coordinate> = (0..num_points)
        .map(|i| {
            let x = i as f64;
            let y = (x / 10.0).sin() * 50.0 + (x / 20.0).cos() * 30.0;
            Coordinate::new_2d(x, y)
        })
        .collect();

    LineString::new(coords).expect("Failed to create linestring")
}

// Helper function to create random points
fn create_random_points(num_points: usize) -> Vec<Point> {
    (0..num_points)
        .map(|i| {
            let x = (i as f64 * 1.234567).sin() * 1000.0;
            let y = (i as f64 * 7.654321).cos() * 1000.0;
            Point::new(x, y)
        })
        .collect()
}

fn bench_buffer_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer_operations");

    let point = Point::new(0.0, 0.0);
    let polygon = create_complex_polygon(100);
    let linestring = create_complex_linestring(100);
    let options = oxigdal_algorithms::vector::BufferOptions {
        quadrant_segments: 16,
        ..Default::default()
    };

    group.bench_function("buffer_point", |b| {
        b.iter(|| buffer_point(black_box(&point), black_box(10.0), black_box(&options)));
    });

    group.bench_function("buffer_polygon", |b| {
        b.iter(|| buffer_polygon(black_box(&polygon), black_box(5.0), black_box(&options)));
    });

    group.bench_function("buffer_linestring", |b| {
        b.iter(|| buffer_linestring(black_box(&linestring), black_box(5.0), black_box(&options)));
    });

    group.finish();
}

fn bench_simplification(c: &mut Criterion) {
    let mut group = c.benchmark_group("simplification");

    for num_points in [100, 500, 1000, 5000].iter() {
        let linestring = create_complex_linestring(*num_points);

        group.throughput(Throughput::Elements(*num_points as u64));

        group.bench_with_input(
            BenchmarkId::new("douglas_peucker", num_points),
            num_points,
            |b, _| {
                b.iter(|| {
                    simplify_linestring(
                        black_box(&linestring),
                        black_box(2.0),
                        SimplifyMethod::DouglasPeucker,
                    )
                });
            },
        );

        group.bench_with_input(BenchmarkId::new("rdp", num_points), num_points, |b, _| {
            b.iter(|| {
                simplify_linestring(
                    black_box(&linestring),
                    black_box(2.0),
                    SimplifyMethod::DouglasPeucker,
                )
            });
        });

        group.bench_with_input(
            BenchmarkId::new("visvalingam_whyatt", num_points),
            num_points,
            |b, _| {
                b.iter(|| {
                    simplify_linestring(
                        black_box(&linestring),
                        black_box(0.95),
                        SimplifyMethod::VisvalingamWhyatt,
                    )
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("topology_preserving", num_points),
            num_points,
            |b, _| {
                b.iter(|| {
                    simplify_linestring(
                        black_box(&linestring),
                        black_box(2.0),
                        SimplifyMethod::TopologyPreserving,
                    )
                });
            },
        );
    }

    group.finish();
}

fn bench_polygon_intersection(c: &mut Criterion) {
    let mut group = c.benchmark_group("polygon_intersection");

    for num_points in [50, 100, 200].iter() {
        let poly1 = create_complex_polygon(*num_points);
        let poly2 = create_complex_polygon(*num_points);

        group.throughput(Throughput::Elements(*num_points as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(num_points),
            num_points,
            |b, _| {
                b.iter(|| intersect_polygons(black_box(&poly1), black_box(&poly2)));
            },
        );
    }

    group.finish();
}

fn bench_polygon_union(c: &mut Criterion) {
    let mut group = c.benchmark_group("polygon_union");

    for num_points in [50, 100, 200].iter() {
        let poly1 = create_complex_polygon(*num_points);
        let poly2 = create_complex_polygon(*num_points);

        group.throughput(Throughput::Elements(*num_points as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(num_points),
            num_points,
            |b, _| {
                b.iter(|| union_polygon(black_box(&poly1), black_box(&poly2)));
            },
        );
    }

    group.finish();
}

fn bench_polygon_difference(c: &mut Criterion) {
    let mut group = c.benchmark_group("polygon_difference");

    for num_points in [50, 100, 200].iter() {
        let poly1 = create_complex_polygon(*num_points);
        let poly2 = create_complex_polygon(*num_points);

        group.throughput(Throughput::Elements(*num_points as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(num_points),
            num_points,
            |b, _| {
                b.iter(|| difference_polygon(black_box(&poly1), black_box(&poly2)));
            },
        );
    }

    group.finish();
}

fn bench_area_calculation(c: &mut Criterion) {
    let mut group = c.benchmark_group("area_calculation");

    for num_points in [50, 100, 500, 1000].iter() {
        let polygon = create_complex_polygon(*num_points);

        group.throughput(Throughput::Elements(*num_points as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(num_points),
            num_points,
            |b, _| {
                b.iter(|| area_polygon(black_box(&polygon), black_box(AreaMethod::Planar)));
            },
        );
    }

    group.finish();
}

fn bench_centroid_calculation(c: &mut Criterion) {
    let mut group = c.benchmark_group("centroid_calculation");

    for num_points in [50, 100, 500, 1000].iter() {
        let polygon = create_complex_polygon(*num_points);

        group.throughput(Throughput::Elements(*num_points as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(num_points),
            num_points,
            |b, _| {
                b.iter(|| centroid_polygon(black_box(&polygon)));
            },
        );
    }

    group.finish();
}

fn bench_distance_calculation(c: &mut Criterion) {
    let mut group = c.benchmark_group("distance_calculation");

    let point1 = Point::new(-122.4194, 37.7749);
    let point2 = Point::new(-74.0060, 40.7128);

    group.bench_function("haversine", |b| {
        b.iter(|| {
            distance_point_to_point(
                black_box(&point1),
                black_box(&point2),
                DistanceMethod::Haversine,
            )
        });
    });

    group.finish();
}

fn bench_point_in_polygon(c: &mut Criterion) {
    let mut group = c.benchmark_group("point_in_polygon");

    for num_points in [50, 100, 500, 1000].iter() {
        let polygon = create_complex_polygon(*num_points);
        let test_point = Point::new(50.0, 50.0);

        group.throughput(Throughput::Elements(*num_points as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(num_points),
            num_points,
            |b, _| {
                b.iter(|| point_in_polygon(black_box(&test_point.coord), black_box(&polygon)));
            },
        );
    }

    group.finish();
}

fn bench_geometry_validation(c: &mut Criterion) {
    let mut group = c.benchmark_group("geometry_validation");

    for num_points in [50, 100, 500].iter() {
        let polygon = create_complex_polygon(*num_points);
        let linestring = create_complex_linestring(*num_points);

        group.bench_with_input(
            BenchmarkId::new("validate_polygon", num_points),
            num_points,
            |b, _| {
                b.iter(|| validate_polygon(black_box(&polygon)));
            },
        );

        group.bench_with_input(
            BenchmarkId::new("validate_linestring", num_points),
            num_points,
            |b, _| {
                b.iter(|| validate_linestring(black_box(&linestring)));
            },
        );
    }

    group.finish();
}

fn bench_batch_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_operations");

    let options = oxigdal_algorithms::vector::BufferOptions {
        quadrant_segments: 16,
        ..Default::default()
    };

    for num_geometries in [100, 500, 1000].iter() {
        let polygons: Vec<Polygon> = (0..*num_geometries)
            .map(|_| create_complex_polygon(50))
            .collect();

        group.throughput(Throughput::Elements(*num_geometries as u64));

        group.bench_with_input(
            BenchmarkId::new("batch_area", num_geometries),
            num_geometries,
            |b, _| {
                b.iter(|| {
                    polygons
                        .iter()
                        .map(|p| area_polygon(black_box(p), black_box(AreaMethod::Planar)))
                        .collect::<Vec<_>>()
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("batch_centroid", num_geometries),
            num_geometries,
            |b, _| {
                b.iter(|| {
                    polygons
                        .iter()
                        .map(|p| centroid_polygon(black_box(p)))
                        .collect::<Vec<_>>()
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("batch_buffer", num_geometries),
            num_geometries,
            |b, _| {
                b.iter(|| {
                    polygons
                        .iter()
                        .map(|p| buffer_polygon(black_box(p), 5.0, black_box(&options)))
                        .collect::<Vec<_>>()
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_buffer_operations,
    bench_simplification,
    bench_polygon_intersection,
    bench_polygon_union,
    bench_polygon_difference,
    bench_area_calculation,
    bench_centroid_calculation,
    bench_distance_calculation,
    bench_point_in_polygon,
    bench_geometry_validation,
    bench_batch_operations,
);
criterion_main!(benches);
