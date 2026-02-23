//! Benchmarks for radiometric corrections
#![allow(missing_docs, clippy::expect_used, clippy::panic, clippy::unit_arg)]

use criterion::{Criterion, criterion_group, criterion_main};
use oxigdal_sensors::radiometry::atmospheric::{AtmosphericCorrection, DarkObjectSubtraction};
use oxigdal_sensors::radiometry::calibration::RadiometricCalibration;
use scirs2_core::ndarray::Array2;
use std::hint::black_box;

fn bench_dn_to_radiance(c: &mut Criterion) {
    let size = 1000;
    let dn = Array2::from_elem((size, size), 10000.0);
    let cal = RadiometricCalibration::new(0.00002, 0.0);

    c.bench_function("dn_to_radiance_1000x1000", |b| {
        b.iter(|| {
            let result = cal.dn_to_radiance(&black_box(dn.view()));
            black_box(result)
        })
    });
}

fn bench_atmospheric_correction(c: &mut Criterion) {
    let size = 1000;
    let toa = Array2::from_elem((size, size), 0.15);
    let dos = DarkObjectSubtraction::default_params();

    c.bench_function("dos_correction_1000x1000", |b| {
        b.iter(|| {
            let result = dos.correct(&black_box(toa.view()));
            black_box(result)
        })
    });
}

criterion_group!(benches, bench_dn_to_radiance, bench_atmospheric_correction);
criterion_main!(benches);
