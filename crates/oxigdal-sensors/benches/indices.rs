//! Benchmarks for spectral indices
#![allow(missing_docs, clippy::expect_used, clippy::panic, clippy::unit_arg)]

use criterion::{Criterion, criterion_group, criterion_main};
use oxigdal_sensors::indices::vegetation::{evi, msavi, ndvi, savi};
use scirs2_core::ndarray::Array2;
use std::hint::black_box;

fn bench_ndvi(c: &mut Criterion) {
    let size = 1000;
    let nir = Array2::from_elem((size, size), 0.5);
    let red = Array2::from_elem((size, size), 0.1);

    c.bench_function("ndvi_1000x1000", |b| {
        b.iter(|| {
            let result = ndvi(&black_box(nir.view()), &black_box(red.view()));
            black_box(result)
        })
    });
}

fn bench_evi(c: &mut Criterion) {
    let size = 1000;
    let nir = Array2::from_elem((size, size), 0.5);
    let red = Array2::from_elem((size, size), 0.1);
    let blue = Array2::from_elem((size, size), 0.05);

    c.bench_function("evi_1000x1000", |b| {
        b.iter(|| {
            let result = evi(
                &black_box(nir.view()),
                &black_box(red.view()),
                &black_box(blue.view()),
            );
            black_box(result)
        })
    });
}

fn bench_savi(c: &mut Criterion) {
    let size = 1000;
    let nir = Array2::from_elem((size, size), 0.5);
    let red = Array2::from_elem((size, size), 0.1);

    c.bench_function("savi_1000x1000", |b| {
        b.iter(|| {
            let result = savi(
                &black_box(nir.view()),
                &black_box(red.view()),
                black_box(0.5),
            );
            black_box(result)
        })
    });
}

fn bench_msavi(c: &mut Criterion) {
    let size = 1000;
    let nir = Array2::from_elem((size, size), 0.5);
    let red = Array2::from_elem((size, size), 0.1);

    c.bench_function("msavi_1000x1000", |b| {
        b.iter(|| {
            let result = msavi(&black_box(nir.view()), &black_box(red.view()));
            black_box(result)
        })
    });
}

criterion_group!(benches, bench_ndvi, bench_evi, bench_savi, bench_msavi);
criterion_main!(benches);
