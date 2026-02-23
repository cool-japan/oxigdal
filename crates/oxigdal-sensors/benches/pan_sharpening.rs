//! Benchmarks for pan-sharpening algorithms
#![allow(missing_docs, clippy::expect_used, clippy::panic, clippy::unit_arg)]

use criterion::{Criterion, criterion_group, criterion_main};
use oxigdal_sensors::pan_sharpening::{BroveyTransform, IHSPanSharpening, PanSharpening};
use scirs2_core::ndarray::Array2;
use std::hint::black_box;

fn bench_brovey(c: &mut Criterion) {
    let size = 1000;
    let ms = Array2::from_elem((size, size), 0.5);
    let pan = Array2::from_elem((size, size), 0.8);
    let transform = BroveyTransform;

    c.bench_function("brovey_1000x1000", |b| {
        b.iter(|| {
            let result = transform.sharpen(&black_box(ms.clone()), &black_box(pan.clone()));
            black_box(result)
        })
    });
}

fn bench_ihs(c: &mut Criterion) {
    let size = 1000;
    let ms = Array2::from_elem((size, size), 0.5);
    let pan = Array2::from_elem((size, size), 0.8);
    let transform = IHSPanSharpening;

    c.bench_function("ihs_1000x1000", |b| {
        b.iter(|| {
            let result = transform.sharpen(&black_box(ms.clone()), &black_box(pan.clone()));
            black_box(result)
        })
    });
}

criterion_group!(benches, bench_brovey, bench_ihs);
criterion_main!(benches);
