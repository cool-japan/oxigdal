//! Benchmarks for raster operations
#![allow(
    missing_docs,
    clippy::expect_used,
    clippy::panic,
    clippy::unit_arg,
    clippy::unnecessary_cast
)]

use criterion::{Criterion, criterion_group, criterion_main};
use oxigdal_algorithms::raster::{HillshadeParams, hillshade};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use std::hint::black_box;

fn create_test_dem(size: u64) -> RasterBuffer {
    let mut buffer = RasterBuffer::zeros(size, size, RasterDataType::Float32);

    let center = size as f64 / 2.0;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f64 - center;
            let dy = y as f64 - center;
            let dist = (dx * dx + dy * dy).sqrt();
            let elevation = ((size as f64 / 2.0) - dist).max(0.0);
            buffer.set_pixel(x, y, elevation).ok();
        }
    }

    buffer
}

fn bench_hillshade(c: &mut Criterion) {
    let dem = create_test_dem(512);
    let params = HillshadeParams::standard();

    c.bench_function("hillshade_512x512", |b| {
        b.iter(|| {
            black_box(hillshade(&dem, params).ok());
        });
    });
}

criterion_group!(benches, bench_hillshade);
criterion_main!(benches);
