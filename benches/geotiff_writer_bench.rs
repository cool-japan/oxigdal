//! Performance benchmarks for GeoTIFF writer
#![allow(
    missing_docs,
    clippy::expect_used,
    clippy::panic,
    clippy::unit_arg,
    clippy::unnecessary_cast,
    clippy::manual_div_ceil,
    clippy::useless_format,
    clippy::needless_range_loop,
    unused_variables
)]
//!
//! This benchmark suite tests:
//! - Write performance for different configurations
//! - Compression performance
//! - Tiling performance
//! - Overview generation performance
//! - COG writing performance

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::env;
use std::hint::black_box;
use std::path::PathBuf;

use oxigdal_core::types::{GeoTransform, RasterDataType};
use oxigdal_geotiff::tiff::Compression;
use oxigdal_geotiff::writer::{
    CogWriter, CogWriterOptions, GeoTiffWriter, GeoTiffWriterOptions, OverviewResampling,
    WriterConfig,
};

/// Helper to create benchmark output path
fn bench_output_path(name: &str) -> PathBuf {
    let mut path = env::temp_dir();
    path.push("oxigdal_bench");
    std::fs::create_dir_all(&path).ok();
    path.push(name);
    path
}

/// Create test data for benchmarking
fn create_test_data_u8(width: u64, height: u64) -> Vec<u8> {
    let mut data = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            data.push(((x + y) % 256) as u8);
        }
    }
    data
}

/// Create test data for RGB benchmarking
fn create_test_data_rgb(width: u64, height: u64) -> Vec<u8> {
    let mut data = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            data.push(((x * 2) % 256) as u8);
            data.push(((y * 2) % 256) as u8);
            data.push(((x + y) % 256) as u8);
        }
    }
    data
}

/// Benchmark writing with different compressions
fn bench_compression_methods(c: &mut Criterion) {
    let mut group = c.benchmark_group("write_compression");

    let width = 512u64;
    let height = 512u64;
    let data = create_test_data_u8(width, height);

    group.throughput(Throughput::Bytes((width * height) as u64));

    // LZW compression
    #[cfg(feature = "lzw")]
    group.bench_function("lzw", |b| {
        b.iter(|| {
            let path = bench_output_path("bench_lzw.tif");
            let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
                .with_compression(Compression::Lzw)
                .with_tile_size(256, 256);

            let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
                .expect("Should create writer");
            writer.write(black_box(&data)).expect("Should write");
            std::fs::remove_file(path).ok();
        });
    });

    // DEFLATE compression
    #[cfg(feature = "deflate")]
    group.bench_function("deflate", |b| {
        b.iter(|| {
            let path = bench_output_path("bench_deflate.tif");
            let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
                .with_compression(Compression::Deflate)
                .with_tile_size(256, 256);

            let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
                .expect("Should create writer");
            writer.write(black_box(&data)).expect("Should write");
            std::fs::remove_file(path).ok();
        });
    });

    // ZSTD compression
    #[cfg(feature = "zstd")]
    group.bench_function("zstd", |b| {
        b.iter(|| {
            let path = bench_output_path("bench_zstd.tif");
            let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
                .with_compression(Compression::Zstd)
                .with_tile_size(256, 256);

            let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
                .expect("Should create writer");
            writer.write(black_box(&data)).expect("Should write");
            std::fs::remove_file(path).ok();
        });
    });

    // PackBits compression
    group.bench_function("packbits", |b| {
        b.iter(|| {
            let path = bench_output_path("bench_packbits.tif");
            let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
                .with_compression(Compression::Packbits)
                .with_tile_size(256, 256);

            let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
                .expect("Should create writer");
            writer.write(black_box(&data)).expect("Should write");
            std::fs::remove_file(path).ok();
        });
    });

    // No compression (baseline)
    group.bench_function("none", |b| {
        b.iter(|| {
            let path = bench_output_path("bench_none.tif");
            let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
                .with_compression(Compression::None)
                .with_tile_size(256, 256);

            let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
                .expect("Should create writer");
            writer.write(black_box(&data)).expect("Should write");
            std::fs::remove_file(path).ok();
        });
    });

    group.finish();
}

/// Benchmark writing with different tile sizes
fn bench_tile_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("write_tile_size");

    let width = 1024u64;
    let height = 1024u64;
    let data = create_test_data_u8(width, height);

    group.throughput(Throughput::Bytes((width * height) as u64));

    let tile_sizes = vec![64u32, 128, 256, 512];

    for tile_size in tile_sizes {
        group.bench_with_input(
            BenchmarkId::from_parameter(tile_size),
            &tile_size,
            |b, &ts| {
                b.iter(|| {
                    let path = bench_output_path(&format!("bench_tile_{}.tif", ts));
                    let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
                        .with_compression(Compression::Lzw)
                        .with_tile_size(ts, ts);

                    let mut writer =
                        GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
                            .expect("Should create writer");
                    writer.write(black_box(&data)).expect("Should write");
                    std::fs::remove_file(path).ok();
                });
            },
        );
    }

    group.finish();
}

/// Benchmark writing different image sizes
fn bench_image_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("write_image_size");

    let sizes = vec![(256u64, 256u64), (512, 512), (1024, 1024), (2048, 2048)];

    for (width, height) in sizes {
        let data = create_test_data_u8(width, height);
        group.throughput(Throughput::Bytes((width * height) as u64));

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}x{}", width, height)),
            &(width, height),
            |b, &(w, h)| {
                b.iter(|| {
                    let path = bench_output_path(&format!("bench_size_{}x{}.tif", w, h));
                    let config = WriterConfig::new(w, h, 1, RasterDataType::UInt8)
                        .with_compression(Compression::Lzw)
                        .with_tile_size(256, 256);

                    let mut writer =
                        GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
                            .expect("Should create writer");
                    writer.write(black_box(&data)).expect("Should write");
                    std::fs::remove_file(path).ok();
                });
            },
        );
    }

    group.finish();
}

/// Benchmark writing different data types
fn bench_data_types(c: &mut Criterion) {
    let mut group = c.benchmark_group("write_data_type");

    let width = 512u64;
    let height = 512u64;

    // UInt8
    {
        let data = create_test_data_u8(width, height);
        group.throughput(Throughput::Bytes((width * height) as u64));

        group.bench_function("uint8", |b| {
            b.iter(|| {
                let path = bench_output_path("bench_uint8.tif");
                let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
                    .with_compression(Compression::Lzw)
                    .with_tile_size(256, 256);

                let mut writer =
                    GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
                        .expect("Should create writer");
                writer.write(black_box(&data)).expect("Should write");
                std::fs::remove_file(path).ok();
            });
        });
    }

    // UInt16
    {
        let mut data = Vec::with_capacity((width * height * 2) as usize);
        for i in 0..(width * height) {
            let value = (i % 65536) as u16;
            data.extend_from_slice(&value.to_le_bytes());
        }
        group.throughput(Throughput::Bytes((width * height * 2) as u64));

        group.bench_function("uint16", |b| {
            b.iter(|| {
                let path = bench_output_path("bench_uint16.tif");
                let config = WriterConfig::new(width, height, 1, RasterDataType::UInt16)
                    .with_compression(Compression::Lzw)
                    .with_tile_size(256, 256);

                let mut writer =
                    GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
                        .expect("Should create writer");
                writer.write(black_box(&data)).expect("Should write");
                std::fs::remove_file(path).ok();
            });
        });
    }

    // Float32
    {
        let mut data = Vec::with_capacity((width * height * 4) as usize);
        for i in 0..(width * height) {
            let value = (i as f32) / 1000.0;
            data.extend_from_slice(&value.to_le_bytes());
        }
        group.throughput(Throughput::Bytes((width * height * 4) as u64));

        group.bench_function("float32", |b| {
            b.iter(|| {
                let path = bench_output_path("bench_float32.tif");
                let config = WriterConfig::new(width, height, 1, RasterDataType::Float32)
                    .with_compression(Compression::Lzw)
                    .with_tile_size(256, 256);

                let mut writer =
                    GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
                        .expect("Should create writer");
                writer.write(black_box(&data)).expect("Should write");
                std::fs::remove_file(path).ok();
            });
        });
    }

    group.finish();
}

/// Benchmark tiled vs striped writing
fn bench_tiled_vs_striped(c: &mut Criterion) {
    let mut group = c.benchmark_group("write_layout");

    let width = 512u64;
    let height = 512u64;
    let data = create_test_data_u8(width, height);

    group.throughput(Throughput::Bytes((width * height) as u64));

    // Tiled
    group.bench_function("tiled_256x256", |b| {
        b.iter(|| {
            let path = bench_output_path("bench_tiled.tif");
            let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
                .with_compression(Compression::Lzw)
                .with_tile_size(256, 256);

            let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
                .expect("Should create writer");
            writer.write(black_box(&data)).expect("Should write");
            std::fs::remove_file(path).ok();
        });
    });

    // Striped
    group.bench_function("striped", |b| {
        b.iter(|| {
            let path = bench_output_path("bench_striped.tif");
            let mut config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
                .with_compression(Compression::Lzw);
            config.tile_width = None;
            config.tile_height = None;

            let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
                .expect("Should create writer");
            writer.write(black_box(&data)).expect("Should write");
            std::fs::remove_file(path).ok();
        });
    });

    group.finish();
}

/// Benchmark multi-band writing
fn bench_multi_band(c: &mut Criterion) {
    let mut group = c.benchmark_group("write_bands");

    let width = 512u64;
    let height = 512u64;

    // Single band
    {
        let data = create_test_data_u8(width, height);
        group.throughput(Throughput::Bytes((width * height) as u64));

        group.bench_function("1_band", |b| {
            b.iter(|| {
                let path = bench_output_path("bench_1band.tif");
                let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
                    .with_compression(Compression::Lzw)
                    .with_tile_size(256, 256);

                let mut writer =
                    GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
                        .expect("Should create writer");
                writer.write(black_box(&data)).expect("Should write");
                std::fs::remove_file(path).ok();
            });
        });
    }

    // RGB (3 bands)
    {
        let data = create_test_data_rgb(width, height);
        group.throughput(Throughput::Bytes((width * height * 3) as u64));

        group.bench_function("3_bands_rgb", |b| {
            b.iter(|| {
                let path = bench_output_path("bench_3band.tif");
                let config = WriterConfig::new(width, height, 3, RasterDataType::UInt8)
                    .with_compression(Compression::Lzw)
                    .with_tile_size(256, 256);

                let mut writer =
                    GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
                        .expect("Should create writer");
                writer.write(black_box(&data)).expect("Should write");
                std::fs::remove_file(path).ok();
            });
        });
    }

    group.finish();
}

/// Benchmark COG writing with overviews
fn bench_cog_with_overviews(c: &mut Criterion) {
    let mut group = c.benchmark_group("cog_overviews");

    let width = 1024u64;
    let height = 1024u64;
    let data = create_test_data_u8(width, height);

    group.throughput(Throughput::Bytes((width * height) as u64));

    // No overviews
    group.bench_function("no_overviews", |b| {
        b.iter(|| {
            let path = bench_output_path("bench_cog_no_ov.tif");
            let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
                .with_compression(Compression::Lzw)
                .with_tile_size(256, 256)
                .with_overviews(false, OverviewResampling::Average);

            let mut writer = CogWriter::create(&path, config, CogWriterOptions::default())
                .expect("Should create writer");
            writer.write(black_box(&data)).expect("Should write");
            std::fs::remove_file(path).ok();
        });
    });

    // With 2 overview levels
    group.bench_function("2_overviews", |b| {
        b.iter(|| {
            let path = bench_output_path("bench_cog_2ov.tif");
            let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
                .with_compression(Compression::Lzw)
                .with_tile_size(256, 256)
                .with_overviews(true, OverviewResampling::Average)
                .with_overview_levels(vec![2, 4]);

            let mut writer = CogWriter::create(&path, config, CogWriterOptions::default())
                .expect("Should create writer");
            writer.write(black_box(&data)).expect("Should write");
            std::fs::remove_file(path).ok();
        });
    });

    // With 4 overview levels
    group.bench_function("4_overviews", |b| {
        b.iter(|| {
            let path = bench_output_path("bench_cog_4ov.tif");
            let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
                .with_compression(Compression::Lzw)
                .with_tile_size(256, 256)
                .with_overviews(true, OverviewResampling::Average)
                .with_overview_levels(vec![2, 4, 8, 16]);

            let mut writer = CogWriter::create(&path, config, CogWriterOptions::default())
                .expect("Should create writer");
            writer.write(black_box(&data)).expect("Should write");
            std::fs::remove_file(path).ok();
        });
    });

    group.finish();
}

/// Benchmark overview resampling methods
fn bench_overview_resampling(c: &mut Criterion) {
    let mut group = c.benchmark_group("overview_resampling");

    let width = 1024u64;
    let height = 1024u64;
    let data = create_test_data_u8(width, height);

    group.throughput(Throughput::Bytes((width * height) as u64));

    // AVERAGE resampling
    group.bench_function("average", |b| {
        b.iter(|| {
            let path = bench_output_path("bench_ov_avg.tif");
            let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
                .with_compression(Compression::Lzw)
                .with_tile_size(256, 256)
                .with_overviews(true, OverviewResampling::Average)
                .with_overview_levels(vec![2, 4]);

            let mut writer = CogWriter::create(&path, config, CogWriterOptions::default())
                .expect("Should create writer");
            writer.write(black_box(&data)).expect("Should write");
            std::fs::remove_file(path).ok();
        });
    });

    // NEAREST resampling
    group.bench_function("nearest", |b| {
        b.iter(|| {
            let path = bench_output_path("bench_ov_nearest.tif");
            let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
                .with_compression(Compression::Lzw)
                .with_tile_size(256, 256)
                .with_overviews(true, OverviewResampling::Nearest)
                .with_overview_levels(vec![2, 4]);

            let mut writer = CogWriter::create(&path, config, CogWriterOptions::default())
                .expect("Should create writer");
            writer.write(black_box(&data)).expect("Should write");
            std::fs::remove_file(path).ok();
        });
    });

    group.finish();
}

/// Benchmark georeferenced writing
fn bench_georeferenced(c: &mut Criterion) {
    let mut group = c.benchmark_group("write_georeferenced");

    let width = 512u64;
    let height = 512u64;
    let data = create_test_data_u8(width, height);

    group.throughput(Throughput::Bytes((width * height) as u64));

    // Without georeferencing
    group.bench_function("no_georeference", |b| {
        b.iter(|| {
            let path = bench_output_path("bench_no_geo.tif");
            let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
                .with_compression(Compression::Lzw)
                .with_tile_size(256, 256);

            let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
                .expect("Should create writer");
            writer.write(black_box(&data)).expect("Should write");
            std::fs::remove_file(path).ok();
        });
    });

    // With georeferencing
    group.bench_function("with_georeference", |b| {
        b.iter(|| {
            let path = bench_output_path("bench_with_geo.tif");
            let geo_transform = GeoTransform::north_up(0.0, 0.0, 1.0, -1.0);

            let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
                .with_compression(Compression::Lzw)
                .with_tile_size(256, 256)
                .with_geo_transform(geo_transform)
                .with_epsg_code(4326);

            let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
                .expect("Should create writer");
            writer.write(black_box(&data)).expect("Should write");
            std::fs::remove_file(path).ok();
        });
    });

    group.finish();
}

criterion_group!(
    writer_benches,
    bench_compression_methods,
    bench_tile_sizes,
    bench_image_sizes,
    bench_data_types,
    bench_tiled_vs_striped,
    bench_multi_band,
    bench_cog_with_overviews,
    bench_overview_resampling,
    bench_georeferenced,
);

criterion_main!(writer_benches);
