//! Benchmarks for vector operations
#![allow(
    missing_docs,
    clippy::expect_used,
    clippy::panic,
    clippy::unit_arg,
    clippy::unnecessary_cast
)]

use criterion::{Criterion, criterion_group, criterion_main};
use oxigdal_algorithms::vector::{Coordinate, LineString, SimplifyMethod, simplify_linestring};
use std::hint::black_box;

fn create_test_line(num_points: usize) -> Result<LineString, Box<dyn std::error::Error>> {
    let mut coords = Vec::with_capacity(num_points);

    for i in 0..num_points {
        let x = i as f64;
        let y = (i as f64 * 0.1).sin() * 10.0;
        coords.push(Coordinate::new_2d(x, y));
    }

    LineString::new(coords).map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
}

fn bench_douglas_peucker(c: &mut Criterion) {
    let line = create_test_line(10000).expect("Failed to create test line");

    c.bench_function("douglas_peucker_10k_points", |b| {
        b.iter(|| {
            let _ = black_box(simplify_linestring(&line, 0.5, SimplifyMethod::DouglasPeucker).ok());
        });
    });
}

criterion_group!(benches, bench_douglas_peucker);
criterion_main!(benches);
