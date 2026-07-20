#![allow(missing_docs, clippy::expect_used)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigeo_algorithms::raster::{
    FocalBoundaryMode, HillshadeParams, WindowShape, aspect, compute_statistics, focal_mean,
    gaussian_blur, hillshade, slope,
};
use oxigeo_algorithms::resampling::{Resampler, ResamplingMethod};
use oxigeo_algorithms::simd::raster::{add_f32, mul_f32, sub_f32};
use oxigeo_geotiff::tiff::Compression;
use oxigeo_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};
use std::hint::black_box;
use std::path::PathBuf;

use oxigeo_core::buffer::RasterBuffer;
use oxigeo_core::types::RasterDataType;

/// Builds a scratch output path under the OS temp directory for write benchmarks.
/// Never hardcodes a user-specific absolute path.
fn bench_output_path(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push("oxigeo_bench_gdal_comparison");
    std::fs::create_dir_all(&path).ok();
    path.push(name);
    path
}

// Helper function to create test raster data as RasterBuffer
fn create_test_raster_buffer(width: u64, height: u64) -> RasterBuffer {
    let data: Vec<f32> = (0..width * height)
        .map(|i| {
            let x = (i % width) as f32;
            let y = (i / width) as f32;
            x.sin() * 100.0 + y.cos() * 50.0 + 1000.0
        })
        .collect();
    let byte_data: Vec<u8> = data.iter().flat_map(|v| v.to_le_bytes()).collect();
    RasterBuffer::new(
        byte_data,
        width,
        height,
        RasterDataType::Float32,
        Default::default(),
    )
    .expect("Failed to create test raster")
}

// Note: These benchmarks compare OxiGeo performance against expected GDAL baselines.
// The baselines are approximate and based on typical GDAL performance characteristics.
// For actual comparison, GDAL would need to be installed and benchmarked separately.

const GDAL_BASELINE_HILLSHADE_MS_PER_MEGAPIXEL: f64 = 50.0;
const GDAL_BASELINE_SLOPE_MS_PER_MEGAPIXEL: f64 = 45.0;
const GDAL_BASELINE_ASPECT_MS_PER_MEGAPIXEL: f64 = 45.0;
const GDAL_BASELINE_WARP_MS_PER_MEGAPIXEL: f64 = 120.0;
const GDAL_BASELINE_GAUSSIAN_MS_PER_MEGAPIXEL: f64 = 60.0;
const GDAL_BASELINE_STATS_MS_PER_MEGAPIXEL: f64 = 10.0;

fn bench_hillshade_vs_gdal(c: &mut Criterion) {
    let mut group = c.benchmark_group("hillshade_vs_gdal");

    for size in [512u64, 1024, 2048].iter() {
        let dem = create_test_raster_buffer(*size, *size);
        let megapixels = (size * size) as f64 / 1_000_000.0;

        group.throughput(Throughput::Elements(size * size));
        group.bench_with_input(BenchmarkId::new("oxigeo", size), size, |b, _| {
            b.iter(|| {
                let dem_clone = black_box(dem.clone());
                hillshade(&dem_clone, HillshadeParams::standard())
            });
        });

        // Baseline comparison note
        let baseline_ms = megapixels * GDAL_BASELINE_HILLSHADE_MS_PER_MEGAPIXEL;
        println!(
            "GDAL baseline for {} hillshade: ~{:.2} ms",
            size, baseline_ms
        );
    }

    group.finish();
}

fn bench_slope_vs_gdal(c: &mut Criterion) {
    let mut group = c.benchmark_group("slope_vs_gdal");

    for size in [512u64, 1024, 2048].iter() {
        let dem = create_test_raster_buffer(*size, *size);
        let megapixels = (size * size) as f64 / 1_000_000.0;

        group.throughput(Throughput::Elements(size * size));
        group.bench_with_input(BenchmarkId::new("oxigeo", size), size, |b, _| {
            b.iter(|| {
                let dem_clone = black_box(dem.clone());
                slope(&dem_clone, 30.0, 1.0)
            });
        });

        let baseline_ms = megapixels * GDAL_BASELINE_SLOPE_MS_PER_MEGAPIXEL;
        println!("GDAL baseline for {} slope: ~{:.2} ms", size, baseline_ms);
    }

    group.finish();
}

fn bench_aspect_vs_gdal(c: &mut Criterion) {
    let mut group = c.benchmark_group("aspect_vs_gdal");

    for size in [512u64, 1024, 2048].iter() {
        let dem = create_test_raster_buffer(*size, *size);
        let megapixels = (size * size) as f64 / 1_000_000.0;

        group.throughput(Throughput::Elements(size * size));
        group.bench_with_input(BenchmarkId::new("oxigeo", size), size, |b, _| {
            b.iter(|| {
                let dem_clone = black_box(dem.clone());
                aspect(&dem_clone, 30.0, 1.0)
            });
        });

        let baseline_ms = megapixels * GDAL_BASELINE_ASPECT_MS_PER_MEGAPIXEL;
        println!("GDAL baseline for {} aspect: ~{:.2} ms", size, baseline_ms);
    }

    group.finish();
}

fn bench_resampling_vs_gdal(c: &mut Criterion) {
    let mut group = c.benchmark_group("resampling_vs_gdal");

    let src_size = 2048u64;
    let dst_size = 1024u64;
    let src_data = create_test_raster_buffer(src_size, src_size);
    let megapixels = (dst_size * dst_size) as f64 / 1_000_000.0;

    group.throughput(Throughput::Elements(dst_size * dst_size));

    let methods = [
        ("nearest", ResamplingMethod::Nearest),
        ("bilinear", ResamplingMethod::Bilinear),
        ("bicubic", ResamplingMethod::Bicubic),
        ("lanczos", ResamplingMethod::Lanczos),
    ];

    for (name, method) in &methods {
        group.bench_with_input(BenchmarkId::new("oxigeo", name), name, |b, _| {
            b.iter(|| {
                let src_clone = black_box(src_data.clone());
                let resampler = Resampler::new(*method);
                resampler.resample(&src_clone, dst_size, dst_size)
            });
        });

        let baseline_ms = megapixels * GDAL_BASELINE_WARP_MS_PER_MEGAPIXEL;
        println!(
            "GDAL baseline for {} resampling: ~{:.2} ms",
            name, baseline_ms
        );
    }

    group.finish();
}

fn bench_filters_vs_gdal(c: &mut Criterion) {
    let mut group = c.benchmark_group("filters_vs_gdal");

    let size = 1024u64;
    let raster = create_test_raster_buffer(size, size);
    let megapixels = (size * size) as f64 / 1_000_000.0;

    group.throughput(Throughput::Elements(size * size));

    group.bench_function("oxigeo_gaussian", |b| {
        b.iter(|| {
            let raster_clone = black_box(raster.clone());
            gaussian_blur(&raster_clone, 1.5, None)
        });
    });

    // Create window and boundary mode for focal mean
    let window = WindowShape::rectangular(3, 3).expect("Failed to create window");
    let boundary = FocalBoundaryMode::Reflect;

    group.bench_function("oxigeo_focal_mean", |b| {
        b.iter(|| {
            let raster_clone = black_box(raster.clone());
            focal_mean(&raster_clone, &window, &boundary)
        });
    });

    let baseline_ms = megapixels * GDAL_BASELINE_GAUSSIAN_MS_PER_MEGAPIXEL;
    println!("GDAL baseline for filtering: ~{:.2} ms", baseline_ms);

    group.finish();
}

fn bench_statistics_vs_gdal(c: &mut Criterion) {
    let mut group = c.benchmark_group("statistics_vs_gdal");

    for size in [512u64, 1024, 2048, 4096].iter() {
        let raster = create_test_raster_buffer(*size, *size);
        let megapixels = (size * size) as f64 / 1_000_000.0;

        group.throughput(Throughput::Elements(size * size));
        group.bench_with_input(BenchmarkId::new("oxigeo", size), size, |b, _| {
            b.iter(|| {
                let raster_clone = black_box(raster.clone());
                compute_statistics(&raster_clone)
            });
        });

        let baseline_ms = megapixels * GDAL_BASELINE_STATS_MS_PER_MEGAPIXEL;
        println!(
            "GDAL baseline for {} statistics: ~{:.2} ms",
            size, baseline_ms
        );
    }

    group.finish();
}

fn bench_simd_raster_ops_vs_gdal(c: &mut Criterion) {
    let mut group = c.benchmark_group("simd_raster_ops_vs_gdal");

    let size = 2048u64;
    let raster1 = create_test_raster_buffer(size, size);
    let raster2 = create_test_raster_buffer(size, size);
    let megapixels = (size * size) as f64 / 1_000_000.0;
    let pixel_count = (size * size) as usize;

    // Real SIMD-accelerated raster algebra: operate on the actual pixel data,
    // not just clone the buffers.
    let data1: &[f32] = raster1.as_slice().expect("raster1 is Float32");
    let data2: &[f32] = raster2.as_slice().expect("raster2 is Float32");
    let mut out = vec![0.0f32; pixel_count];

    group.throughput(Throughput::Elements(size * size));

    group.bench_function("oxigeo_add", |b| {
        b.iter(|| {
            add_f32(black_box(data1), black_box(data2), black_box(&mut out))
                .expect("add_f32 failed");
        });
    });

    group.bench_function("oxigeo_multiply", |b| {
        b.iter(|| {
            mul_f32(black_box(data1), black_box(data2), black_box(&mut out))
                .expect("mul_f32 failed");
        });
    });

    group.bench_function("oxigeo_subtract", |b| {
        b.iter(|| {
            sub_f32(black_box(data1), black_box(data2), black_box(&mut out))
                .expect("sub_f32 failed");
        });
    });

    // GDAL raster algebra is typically slower due to less aggressive SIMD optimization
    let baseline_ms = megapixels * 15.0; // Approximate baseline
    println!("GDAL baseline for raster algebra: ~{:.2} ms", baseline_ms);

    group.finish();
}

fn bench_io_throughput_vs_gdal(c: &mut Criterion) {
    let mut group = c.benchmark_group("io_throughput_vs_gdal");

    for size in [512u32, 1024, 2048].iter() {
        let megapixels = (size * size) as f64 / 1_000_000.0;
        let raster = create_test_raster_buffer(u64::from(*size), u64::from(*size));
        let data = raster.as_bytes().to_vec();

        // 4 bytes/pixel (Float32)
        group.throughput(Throughput::Bytes(u64::from(size * size) * 4));

        group.bench_with_input(BenchmarkId::new("oxigeo_write", size), size, |b, &sz| {
            b.iter(|| {
                // Real GeoTIFF write: full header + tiled data + overviews, uncompressed.
                let path = bench_output_path(&format!("bench_io_{sz}.tif"));
                let config =
                    WriterConfig::new(u64::from(sz), u64::from(sz), 1, RasterDataType::Float32)
                        .with_compression(Compression::None)
                        .with_tile_size(256, 256);
                let mut writer =
                    GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
                        .expect("Should create writer");
                writer.write(black_box(&data)).expect("Should write");
                std::fs::remove_file(&path).ok();
            });
        });

        // GDAL write baseline (varies by compression and format)
        let baseline_ms = megapixels * 80.0; // Approximate for uncompressed
        println!("GDAL baseline for {} write: ~{:.2} ms", size, baseline_ms);
    }

    group.finish();
}

fn bench_compression_vs_gdal(c: &mut Criterion) {
    let mut group = c.benchmark_group("compression_vs_gdal");

    let size = 1024u32;
    let megapixels = (size * size) as f64 / 1_000_000.0;
    let raster = create_test_raster_buffer(u64::from(size), u64::from(size));
    let data = raster.as_bytes().to_vec();
    // Keep `data` observably used even when none of the deflate/lzw/zstd
    // codec features are enabled (avoids an unused-variable warning).
    let _ = black_box(&data);

    group.throughput(Throughput::Bytes(u64::from(size * size) * 4));

    // Real GeoTIFF writes driving the actual codecs, one bench per compression scheme.
    #[cfg(feature = "deflate")]
    group.bench_with_input(BenchmarkId::new("oxigeo", "deflate"), &data, |b, data| {
        b.iter(|| {
            let path = bench_output_path("bench_compression_deflate.tif");
            let config =
                WriterConfig::new(u64::from(size), u64::from(size), 1, RasterDataType::Float32)
                    .with_compression(Compression::Deflate)
                    .with_tile_size(256, 256);
            let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
                .expect("Should create writer");
            writer.write(black_box(data)).expect("Should write");
            std::fs::remove_file(&path).ok();
        });
    });

    #[cfg(feature = "lzw")]
    group.bench_with_input(BenchmarkId::new("oxigeo", "lzw"), &data, |b, data| {
        b.iter(|| {
            let path = bench_output_path("bench_compression_lzw.tif");
            let config =
                WriterConfig::new(u64::from(size), u64::from(size), 1, RasterDataType::Float32)
                    .with_compression(Compression::Lzw)
                    .with_tile_size(256, 256);
            let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
                .expect("Should create writer");
            writer.write(black_box(data)).expect("Should write");
            std::fs::remove_file(&path).ok();
        });
    });

    #[cfg(feature = "zstd")]
    group.bench_with_input(BenchmarkId::new("oxigeo", "zstd"), &data, |b, data| {
        b.iter(|| {
            let path = bench_output_path("bench_compression_zstd.tif");
            let config =
                WriterConfig::new(u64::from(size), u64::from(size), 1, RasterDataType::Float32)
                    .with_compression(Compression::Zstd)
                    .with_tile_size(256, 256);
            let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
                .expect("Should create writer");
            writer.write(black_box(data)).expect("Should write");
            std::fs::remove_file(&path).ok();
        });
    });

    for compression in &["deflate", "lzw", "zstd"] {
        // GDAL compression baselines (approximate)
        let compression_overhead = match *compression {
            "deflate" => 120.0,
            "lzw" => 100.0,
            "zstd" => 90.0,
            _ => 80.0,
        };
        let baseline_ms = megapixels * compression_overhead;
        println!(
            "GDAL baseline for {} compression: ~{:.2} ms",
            compression, baseline_ms
        );
    }

    group.finish();
}

fn bench_parallel_vs_gdal(c: &mut Criterion) {
    let mut group = c.benchmark_group("parallel_vs_gdal");

    let size = 2048u64;
    let raster = create_test_raster_buffer(size, size);
    let megapixels = (size * size) as f64 / 1_000_000.0;

    group.throughput(Throughput::Elements(size * size));

    group.bench_function("oxigeo_parallel_slope", |b| {
        b.iter(|| {
            let raster_clone = black_box(raster.clone());
            // Test slope computation which uses parallel processing internally
            slope(&raster_clone, 30.0, 1.0)
        });
    });

    // GDAL has limited built-in parallelism for terrain operations
    let baseline_ms = megapixels * GDAL_BASELINE_SLOPE_MS_PER_MEGAPIXEL;
    println!("GDAL baseline (single-threaded): ~{:.2} ms", baseline_ms);

    group.finish();
}

criterion_group!(
    benches,
    bench_hillshade_vs_gdal,
    bench_slope_vs_gdal,
    bench_aspect_vs_gdal,
    bench_resampling_vs_gdal,
    bench_filters_vs_gdal,
    bench_statistics_vs_gdal,
    bench_simd_raster_ops_vs_gdal,
    bench_io_throughput_vs_gdal,
    bench_compression_vs_gdal,
    bench_parallel_vs_gdal,
);
criterion_main!(benches);
