#![allow(
    missing_docs,
    clippy::expect_used,
    clippy::panic,
    clippy::unit_arg,
    clippy::clone_on_copy,
    clippy::unnecessary_cast
)]
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_algorithms::raster::{
    HillshadeParams, RasterCalculator, StructuringElement, aspect, close, compute_histogram,
    compute_statistics, dilate, erode, gaussian_blur, hillshade, median_filter, open, slope,
};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use std::hint::black_box;

// Helper function to create test raster data
fn create_test_raster(width: usize, height: usize) -> RasterBuffer {
    let mut raster = RasterBuffer::zeros(width as u64, height as u64, RasterDataType::Float64);
    for y in 0..height {
        for x in 0..width {
            let value = (x as f64).sin() * 100.0 + (y as f64).cos() * 50.0 + 1000.0;
            let _ = raster.set_pixel(x as u64, y as u64, value);
        }
    }
    raster
}

fn bench_hillshade(c: &mut Criterion) {
    let mut group = c.benchmark_group("hillshade");

    for size in [256, 512, 1024].iter() {
        let dem = create_test_raster(*size, *size);
        let params = HillshadeParams::default();

        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let _ = hillshade(black_box(&dem), black_box(params.clone()));
            });
        });
    }

    group.finish();
}

fn bench_slope(c: &mut Criterion) {
    let mut group = c.benchmark_group("slope");

    for size in [256, 512, 1024].iter() {
        let dem = create_test_raster(*size, *size);
        let pixel_size = 30.0;

        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let _ = slope(black_box(&dem), black_box(pixel_size), black_box(1.0));
            });
        });
    }

    group.finish();
}

fn bench_aspect(c: &mut Criterion) {
    let mut group = c.benchmark_group("aspect");

    for size in [256, 512, 1024].iter() {
        let dem = create_test_raster(*size, *size);
        let pixel_size = 30.0;

        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let _ = aspect(black_box(&dem), black_box(pixel_size), black_box(1.0));
            });
        });
    }

    group.finish();
}

fn bench_gaussian_blur(c: &mut Criterion) {
    let mut group = c.benchmark_group("gaussian_blur");

    for size in [256, 512, 1024].iter() {
        let raster = create_test_raster(*size, *size);

        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let _ = gaussian_blur(black_box(&raster), black_box(1.5), black_box(None));
            });
        });
    }

    group.finish();
}

fn bench_median_filter(c: &mut Criterion) {
    let mut group = c.benchmark_group("median_filter");

    for size in [256, 512, 1024].iter() {
        let raster = create_test_raster(*size, *size);

        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let _ = median_filter(black_box(&raster), black_box(3));
            });
        });
    }

    group.finish();
}

fn bench_morphological_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("morphological_operations");

    let size = 512;
    let mut raster = RasterBuffer::zeros(size as u64, size as u64, RasterDataType::UInt8);
    for y in 0..size {
        for x in 0..size {
            if (x + y) % 3 == 0 {
                let _ = raster.set_pixel(x as u64, y as u64, 255.0);
            }
        }
    }

    let element = StructuringElement::Square { size: 3 };
    group.throughput(Throughput::Elements((size * size) as u64));

    group.bench_function("erode", |b| {
        b.iter(|| {
            let _ = erode(black_box(&raster), black_box(element.clone()));
        });
    });

    group.bench_function("dilate", |b| {
        b.iter(|| {
            let _ = dilate(black_box(&raster), black_box(element.clone()));
        });
    });

    group.bench_function("open", |b| {
        b.iter(|| {
            let _ = open(black_box(&raster), black_box(element.clone()));
        });
    });

    group.bench_function("close", |b| {
        b.iter(|| {
            let _ = close(black_box(&raster), black_box(element.clone()));
        });
    });

    group.finish();
}

fn bench_statistics(c: &mut Criterion) {
    let mut group = c.benchmark_group("statistics");

    for size in [256, 512, 1024, 2048].iter() {
        let raster = create_test_raster(*size, *size);

        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let _ = compute_statistics(black_box(&raster));
            });
        });
    }

    group.finish();
}

fn bench_histogram(c: &mut Criterion) {
    let mut group = c.benchmark_group("histogram");

    for size in [256, 512, 1024, 2048].iter() {
        let raster = create_test_raster(*size, *size);

        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let _ = compute_histogram(
                    black_box(&raster),
                    black_box(256),
                    black_box(None),
                    black_box(None),
                );
            });
        });
    }

    group.finish();
}

fn bench_raster_calculator(c: &mut Criterion) {
    let mut group = c.benchmark_group("raster_calculator");

    let size = 512;
    let band1 = create_test_raster(size, size);
    let band2 = create_test_raster(size, size);

    group.throughput(Throughput::Elements((size * size) as u64));

    group.bench_function("ndvi", |b| {
        b.iter(|| {
            let _ = RasterCalculator::evaluate(
                black_box("(a - b) / (a + b)"),
                black_box(&[band1.clone(), band2.clone()]),
            );
        });
    });

    group.bench_function("evi", |b| {
        b.iter(|| {
            let _ = RasterCalculator::evaluate(
                black_box("2.5 * (a - b) / (a + 6.0 * b + 1.0)"),
                black_box(&[band1.clone(), band2.clone()]),
            );
        });
    });

    group.bench_function("savi", |b| {
        b.iter(|| {
            let _ = RasterCalculator::evaluate(
                black_box("1.5 * (a - b) / (a + b + 0.5)"),
                black_box(&[band1.clone(), band2.clone()]),
            );
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_hillshade,
    bench_slope,
    bench_aspect,
    bench_gaussian_blur,
    bench_median_filter,
    bench_morphological_operations,
    bench_statistics,
    bench_histogram,
    bench_raster_calculator,
);
criterion_main!(benches);
