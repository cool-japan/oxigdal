#![allow(missing_docs, clippy::expect_used)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_core::io::FileDataSource;
use oxigdal_core::types::RasterDataType;
use oxigdal_geojson::{GeoJsonReader, GeoJsonWriter};
use oxigdal_geotiff::{GeoTiffReader, GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};
use std::hint::black_box;
use std::io::Cursor;
use tempfile::NamedTempFile;

// Helper function to create test raster data
fn create_test_raster(width: usize, height: usize) -> Vec<u8> {
    (0..width * height)
        .map(|i| {
            let x = i % width;
            let y = i / width;
            ((x + y) % 256) as u8
        })
        .collect()
}

// Helper function to create test GeoJSON data
fn create_test_geojson_features(num_features: usize) -> String {
    let mut features = Vec::new();
    for i in 0..num_features {
        let lon = -180.0 + (i as f64 / num_features as f64) * 360.0;
        let lat = -90.0 + ((i * 7) % 180) as f64;
        features.push(format!(
            r#"{{
                "type": "Feature",
                "geometry": {{
                    "type": "Point",
                    "coordinates": [{}, {}]
                }},
                "properties": {{
                    "id": {},
                    "name": "Feature {}",
                    "value": {}
                }}
            }}"#,
            lon,
            lat,
            i,
            i,
            (i as f64).sin() * 100.0
        ));
    }

    format!(
        r#"{{
            "type": "FeatureCollection",
            "features": [{}]
        }}"#,
        features.join(",")
    )
}

fn bench_geotiff_read_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("geotiff_read_throughput");

    for size in [256, 512, 1024, 2048].iter() {
        // Create a temporary GeoTIFF file
        let temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let file_path = temp_file.path().to_path_buf();
        let raster_data = create_test_raster(*size, *size);

        // Write test data
        let config = WriterConfig::new(*size as u64, *size as u64, 1, RasterDataType::UInt8);
        let options = GeoTiffWriterOptions::default();
        let mut writer =
            GeoTiffWriter::create(&file_path, config, options).expect("Failed to create writer");
        let _ = writer.write(&raster_data);

        let file_size = raster_data.len();

        group.throughput(Throughput::Bytes(file_size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let source =
                    FileDataSource::open(black_box(&file_path)).expect("Failed to open file");
                let reader = GeoTiffReader::open(source);
                if let Ok(reader) = reader {
                    let _ = reader.read_band(0, 0);
                }
            });
        });
    }

    group.finish();
}

fn bench_geotiff_write_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("geotiff_write_throughput");

    for size in [256, 512, 1024, 2048].iter() {
        let raster_data = create_test_raster(*size, *size);
        let file_size = raster_data.len();

        group.throughput(Throughput::Bytes(file_size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let temp_file = NamedTempFile::new().expect("Failed to create temp file");
                let config =
                    WriterConfig::new(*size as u64, *size as u64, 1, RasterDataType::UInt8);
                let options = GeoTiffWriterOptions::default();
                let mut writer = GeoTiffWriter::create(temp_file.path(), config, options)
                    .expect("Failed to create writer");
                let _ = writer.write(black_box(&raster_data));
            });
        });
    }

    group.finish();
}

fn bench_geotiff_compression_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("geotiff_compression_throughput");

    let size = 1024;
    let raster_data = create_test_raster(size, size);
    let file_size = raster_data.len();

    group.throughput(Throughput::Bytes(file_size as u64));

    // Test different compression methods
    for compression in ["lzw", "deflate", "zstd"].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(compression),
            compression,
            |b, _comp| {
                b.iter(|| {
                    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
                    let config =
                        WriterConfig::new(size as u64, size as u64, 1, RasterDataType::UInt8);
                    // Note: compression is set through config, not after creation
                    let options = GeoTiffWriterOptions::default();
                    let mut writer = GeoTiffWriter::create(temp_file.path(), config, options)
                        .expect("Failed to create writer");
                    let _ = writer.write(black_box(&raster_data));
                });
            },
        );
    }

    group.finish();
}

fn bench_geojson_read_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("geojson_read_throughput");

    for num_features in [100, 500, 1000, 5000].iter() {
        let geojson_data = create_test_geojson_features(*num_features);
        let data_size = geojson_data.len();

        group.throughput(Throughput::Bytes(data_size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(num_features),
            num_features,
            |b, _| {
                b.iter(|| {
                    let cursor = Cursor::new(black_box(geojson_data.as_bytes()));
                    let mut reader = GeoJsonReader::new(cursor);
                    let _ = reader.read_feature_collection();
                });
            },
        );
    }

    group.finish();
}

fn bench_geojson_write_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("geojson_write_throughput");

    for num_features in [100, 500, 1000, 5000].iter() {
        // Create test features
        let geojson_data = create_test_geojson_features(*num_features);
        let data_size = geojson_data.len();

        group.throughput(Throughput::Bytes(data_size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(num_features),
            num_features,
            |b, _| {
                b.iter(|| {
                    let buffer = Vec::new();
                    let mut writer = GeoJsonWriter::new(buffer);
                    // Parse and re-write features
                    let cursor = Cursor::new(black_box(geojson_data.as_bytes()));
                    let mut reader = GeoJsonReader::new(cursor);
                    if let Ok(fc) = reader.read_feature_collection() {
                        let _ = writer.write_feature_collection(&fc);
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_concurrent_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_read");

    let size = 1024;
    let num_readers = 4;

    // Create a temporary GeoTIFF file
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let file_path = temp_file.path().to_path_buf();
    let raster_data = create_test_raster(size, size);

    let config = WriterConfig::new(size as u64, size as u64, 1, RasterDataType::UInt8);
    let options = GeoTiffWriterOptions::default();
    let mut writer =
        GeoTiffWriter::create(&file_path, config, options).expect("Failed to create writer");
    let _ = writer.write(&raster_data);

    let file_size = raster_data.len();

    group.throughput(Throughput::Bytes((file_size * num_readers) as u64));
    group.bench_function("geotiff_4_readers", |b| {
        b.iter(|| {
            use rayon::prelude::*;

            (0..num_readers).into_par_iter().for_each(|_| {
                if let Ok(source) = FileDataSource::open(black_box(&file_path)) {
                    let reader = GeoTiffReader::open(source);
                    if let Ok(reader) = reader {
                        let _ = reader.read_band(0, 0);
                    }
                }
            });
        });
    });

    group.finish();
}

fn bench_streaming_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("streaming_read");

    for chunk_size in [256, 512, 1024].iter() {
        let total_size = 4096;
        let raster_data = create_test_raster(total_size, total_size);

        group.throughput(Throughput::Bytes((total_size * total_size) as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(chunk_size),
            chunk_size,
            |b, cs| {
                b.iter(|| {
                    // Simulate streaming by reading in chunks
                    for y in (0..total_size).step_by(*cs) {
                        for x in (0..total_size).step_by(*cs) {
                            let chunk_h = (*cs).min(total_size - y);
                            let chunk_w = (*cs).min(total_size - x);

                            let mut chunk = Vec::with_capacity(chunk_h * chunk_w);
                            for dy in 0..chunk_h {
                                for dx in 0..chunk_w {
                                    let idx = (y + dy) * total_size + (x + dx);
                                    chunk.push(black_box(raster_data[idx]));
                                }
                            }
                        }
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_large_file_handling(c: &mut Criterion) {
    let mut group = c.benchmark_group("large_file_handling");
    group.sample_size(10); // Reduce sample size for large files

    for size in [2048, 4096].iter() {
        let raster_data = create_test_raster(*size, *size);
        let file_size = raster_data.len();

        group.throughput(Throughput::Bytes(file_size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let temp_file = NamedTempFile::new().expect("Failed to create temp file");
                let config =
                    WriterConfig::new(*size as u64, *size as u64, 1, RasterDataType::UInt8);
                let options = GeoTiffWriterOptions::default();
                let mut writer = GeoTiffWriter::create(temp_file.path(), config, options)
                    .expect("Failed to create writer");
                let _ = writer.write(black_box(&raster_data));
            });
        });
    }

    group.finish();
}

fn bench_multiband_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("multiband_operations");

    let size = 1024;

    for bands in [1, 3, 4, 8].iter() {
        let total_size = size * size * bands;
        let raster_data = create_test_raster(size, size);

        group.throughput(Throughput::Bytes(total_size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(bands), bands, |b, num_bands| {
            b.iter(|| {
                let temp_file = NamedTempFile::new().expect("Failed to create temp file");
                let config = WriterConfig::new(
                    size as u64,
                    size as u64,
                    *num_bands as u16,
                    RasterDataType::UInt8,
                );
                let options = GeoTiffWriterOptions::default();
                let mut writer = GeoTiffWriter::create(temp_file.path(), config, options)
                    .expect("Failed to create writer");
                let _ = writer.write(black_box(&raster_data));
            });
        });
    }

    group.finish();
}

fn bench_random_access_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("random_access_patterns");

    let size = 2048;
    let raster_data = create_test_raster(size, size);

    group.throughput(Throughput::Bytes((size * size) as u64));

    group.bench_function("sequential_access", |b| {
        b.iter(|| {
            let mut sum: u64 = 0;
            for item in &raster_data {
                sum = sum.wrapping_add(black_box(*item) as u64);
            }
            black_box(sum);
        });
    });

    group.bench_function("random_access", |b| {
        b.iter(|| {
            let mut sum: u64 = 0;
            for i in 0..raster_data.len() {
                let idx = (i * 7919) % raster_data.len(); // Prime number for pseudo-random
                sum = sum.wrapping_add(black_box(raster_data[idx]) as u64);
            }
            black_box(sum);
        });
    });

    group.bench_function("strided_access", |b| {
        b.iter(|| {
            let mut sum: u64 = 0;
            for i in (0..raster_data.len()).step_by(16) {
                sum = sum.wrapping_add(black_box(raster_data[i]) as u64);
            }
            black_box(sum);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_geotiff_read_throughput,
    bench_geotiff_write_throughput,
    bench_geotiff_compression_throughput,
    bench_geojson_read_throughput,
    bench_geojson_write_throughput,
    bench_concurrent_read,
    bench_streaming_read,
    bench_large_file_handling,
    bench_multiband_operations,
    bench_random_access_patterns,
);
criterion_main!(benches);
