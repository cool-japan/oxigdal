//! Benchmarks for tile processing operations
#![allow(missing_docs, clippy::expect_used)]
//!
//! This benchmark suite measures the performance of:
//! - COG tile reading and decoding
//! - Tile compression and decompression
//! - Tile cache operations
//! - Streaming vs batch tile access
//! - Different tile sizes and patterns
//!
//! Tests various scenarios:
//! - Standard tile sizes: 256x256, 512x512
//! - Different compression methods
//! - Sequential vs random access
//! - Cache hit vs miss patterns

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use std::hint::black_box;

#[cfg(feature = "deflate")]
use oxigdal_geotiff::compression;
#[cfg(feature = "deflate")]
use oxigdal_geotiff::tiff::Compression;

/// Generate a tile with realistic geospatial data
fn generate_tile(width: u64, height: u64, tile_x: u64, tile_y: u64) -> Vec<u8> {
    let mut data = Vec::with_capacity((width * height) as usize);

    for y in 0..height {
        for x in 0..width {
            // Create a pattern based on global position
            let global_x = tile_x * width + x;
            let global_y = tile_y * height + y;

            // Simulate elevation data with gradients and patterns
            let gradient = ((global_x + global_y) % 256) as u8;
            let pattern = ((global_x * 17 + global_y * 13) % 128) as u8;
            let value = gradient.wrapping_add(pattern / 2);

            data.push(value);
        }
    }

    data
}

/// Generate a tile buffer for testing
fn generate_tile_buffer(width: u64, height: u64, dtype: RasterDataType) -> RasterBuffer {
    let mut buffer = RasterBuffer::zeros(width, height, dtype);

    for y in 0..height {
        for x in 0..width {
            let value = ((x + y) % 256) as f64;
            buffer.set_pixel(x, y, value).ok();
        }
    }

    buffer
}

fn bench_tile_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("tile/creation");

    let tile_sizes = vec![
        (128, 128, "128x128"),
        (256, 256, "256x256"),
        (512, 512, "512x512"),
        (1024, 1024, "1024x1024"),
    ];

    for (width, height, label) in tile_sizes {
        let pixel_count = width * height;
        group.throughput(Throughput::Elements(pixel_count));

        group.bench_with_input(
            BenchmarkId::new("uint8", label),
            &(width, height),
            |b, (w, h)| {
                b.iter(|| {
                    black_box(generate_tile_buffer(*w, *h, RasterDataType::UInt8));
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("float32", label),
            &(width, height),
            |b, (w, h)| {
                b.iter(|| {
                    black_box(generate_tile_buffer(*w, *h, RasterDataType::Float32));
                });
            },
        );
    }

    group.finish();
}

#[cfg(feature = "deflate")]
fn bench_tile_compression(c: &mut Criterion) {
    let mut group = c.benchmark_group("tile/compression");

    let tile_sizes = vec![(256, 256), (512, 512)];

    for (width, height) in tile_sizes {
        let tile_data = generate_tile(width, height, 0, 0);
        let label = format!("{}x{}", width, height);

        group.throughput(Throughput::Bytes(tile_data.len() as u64));

        // Deflate compression
        group.bench_with_input(
            BenchmarkId::new("deflate_compress", &label),
            &tile_data,
            |b, data| {
                b.iter(|| {
                    black_box(compression::compress(black_box(data), Compression::Deflate).ok());
                });
            },
        );

        // Deflate decompression
        let compressed =
            compression::compress(&tile_data, Compression::Deflate).expect("should compress");

        group.bench_with_input(
            BenchmarkId::new("deflate_decompress", &label),
            &(compressed, tile_data.len()),
            |b, (comp, size)| {
                b.iter(|| {
                    black_box(
                        compression::decompress(black_box(comp), Compression::Deflate, *size).ok(),
                    );
                });
            },
        );
    }

    group.finish();
}

#[cfg(feature = "lzw")]
fn bench_tile_lzw(c: &mut Criterion) {
    use oxigdal_geotiff::compression;
    use oxigdal_geotiff::tiff::Compression;

    let mut group = c.benchmark_group("tile/lzw");

    let tile_sizes = vec![(256, 256), (512, 512)];

    for (width, height) in tile_sizes {
        let tile_data = generate_tile(width, height, 0, 0);
        let label = format!("{}x{}", width, height);

        group.throughput(Throughput::Bytes(tile_data.len() as u64));

        // LZW compression
        group.bench_with_input(
            BenchmarkId::new("compress", &label),
            &tile_data,
            |b, data| {
                b.iter(|| {
                    black_box(compression::compress(black_box(data), Compression::Lzw).ok());
                });
            },
        );

        // LZW decompression
        let compressed =
            compression::compress(&tile_data, Compression::Lzw).expect("should compress");

        group.bench_with_input(
            BenchmarkId::new("decompress", &label),
            &(compressed, tile_data.len()),
            |b, (comp, size)| {
                b.iter(|| {
                    black_box(
                        compression::decompress(black_box(comp), Compression::Lzw, *size).ok(),
                    );
                });
            },
        );
    }

    group.finish();
}

fn bench_tile_access_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("tile/access_patterns");

    let tile_width = 256u64;
    let tile_height = 256u64;
    let grid_width = 10; // 10x10 grid of tiles
    let grid_height = 10;

    // Pre-generate all tiles
    let mut tiles: Vec<Vec<u8>> = Vec::new();
    for ty in 0..grid_height {
        for tx in 0..grid_width {
            tiles.push(generate_tile(tile_width, tile_height, tx, ty));
        }
    }

    // Sequential access (row-major)
    group.throughput(Throughput::Elements(100)); // 100 tiles
    group.bench_function("sequential_access", |b| {
        b.iter(|| {
            for tile in &tiles {
                black_box(tile.as_slice());
            }
        });
    });

    // Random access pattern
    let random_indices: Vec<usize> = (0..100).map(|i| (i * 13 + 7) % tiles.len()).collect();

    group.bench_function("random_access", |b| {
        b.iter(|| {
            for &idx in &random_indices {
                black_box(tiles[idx].as_slice());
            }
        });
    });

    // Sparse access (every 10th tile)
    group.bench_function("sparse_access", |b| {
        b.iter(|| {
            for i in (0..tiles.len()).step_by(10) {
                black_box(tiles[i].as_slice());
            }
        });
    });

    group.finish();
}

fn bench_tile_pyramid(c: &mut Criterion) {
    let mut group = c.benchmark_group("tile/pyramid");

    // Simulate reading tiles from different overview levels
    let levels = vec![
        (512, 512, "level_0_full"),
        (256, 256, "level_1_half"),
        (128, 128, "level_2_quarter"),
        (64, 64, "level_3_eighth"),
    ];

    for (width, height, label) in levels {
        let tile = generate_tile(width, height, 0, 0);
        group.throughput(Throughput::Bytes(tile.len() as u64));

        group.bench_with_input(BenchmarkId::from_parameter(label), &tile, |b, tile| {
            b.iter(|| {
                black_box(tile.as_slice());
            });
        });
    }

    group.finish();
}

fn bench_tile_batch_vs_stream(c: &mut Criterion) {
    let mut group = c.benchmark_group("tile/batch_vs_stream");

    let tile_width = 256u64;
    let tile_height = 256u64;
    let tile_count = 100;

    // Generate tiles
    let tiles: Vec<Vec<u8>> = (0..tile_count)
        .map(|i| generate_tile(tile_width, tile_height, i % 10, i / 10))
        .collect();

    let total_bytes: usize = tiles.iter().map(|t| t.len()).sum();
    group.throughput(Throughput::Bytes(total_bytes as u64));

    // Batch processing (all at once)
    group.bench_function("batch_all", |b| {
        b.iter(|| {
            for tile in &tiles {
                black_box(tile.as_slice());
            }
        });
    });

    // Streaming (process one at a time with breaks)
    group.bench_function("stream_chunked_10", |b| {
        b.iter(|| {
            for chunk in tiles.chunks(10) {
                for tile in chunk {
                    black_box(tile.as_slice());
                }
                // Simulate small processing delay between chunks
                black_box(std::hint::black_box(1));
            }
        });
    });

    group.finish();
}

fn bench_tile_copy_vs_reference(c: &mut Criterion) {
    let mut group = c.benchmark_group("tile/copy_vs_reference");

    let tile_width = 512;
    let tile_height = 512;
    let tile = generate_tile(tile_width, tile_height, 0, 0);

    group.throughput(Throughput::Bytes(tile.len() as u64));

    // By reference (zero-copy)
    group.bench_function("by_reference", |b| {
        b.iter(|| {
            black_box(tile.as_slice());
        });
    });

    // By clone (copy)
    group.bench_function("by_clone", |b| {
        b.iter(|| {
            black_box(tile.clone());
        });
    });

    // Partial copy (sub-tile)
    let sub_size = (tile_width * tile_height / 4) as usize;
    group.bench_function("partial_copy", |b| {
        b.iter(|| {
            black_box(tile[..sub_size].to_vec());
        });
    });

    group.finish();
}

fn bench_tile_decoding_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("tile/decoding_pipeline");

    let tile_width = 256;
    let tile_height = 256;
    let tile_data = generate_tile(tile_width, tile_height, 0, 0);

    group.throughput(Throughput::Bytes(tile_data.len() as u64));

    // Raw data -> Buffer
    group.bench_function("raw_to_buffer", |b| {
        b.iter(|| {
            let mut buffer = RasterBuffer::zeros(tile_width, tile_height, RasterDataType::UInt8);
            for (i, &value) in tile_data.iter().enumerate() {
                let x = (i % tile_width as usize) as u64;
                let y = (i / tile_width as usize) as u64;
                buffer.set_pixel(x, y, value as f64).ok();
            }
            black_box(buffer);
        });
    });

    #[cfg(feature = "deflate")]
    {
        // Compressed -> Decompressed -> Buffer
        let compressed =
            compression::compress(&tile_data, Compression::Deflate).expect("should compress");

        group.bench_function("decompress_to_buffer", |b| {
            b.iter(|| {
                if let Ok(decompressed) = compression::decompress(
                    black_box(&compressed),
                    Compression::Deflate,
                    tile_data.len(),
                ) {
                    let mut buffer =
                        RasterBuffer::zeros(tile_width, tile_height, RasterDataType::UInt8);
                    for (i, &value) in decompressed.iter().enumerate() {
                        let x = (i % tile_width as usize) as u64;
                        let y = (i / tile_width as usize) as u64;
                        buffer.set_pixel(x, y, value as f64).ok();
                    }
                    black_box(buffer);
                }
            });
        });
    }

    group.finish();
}

fn bench_tile_size_impact(c: &mut Criterion) {
    let mut group = c.benchmark_group("tile/size_impact");

    // Test various tile sizes and their impact on I/O patterns
    let sizes = vec![(64, 64), (128, 128), (256, 256), (512, 512), (1024, 1024)];

    // Simulate reading a 4K x 4K image with different tile sizes
    let image_size = 4096u64;

    for (tile_w, tile_h) in sizes {
        let tiles_per_row = image_size.div_ceil(tile_w);
        let tiles_per_col = image_size.div_ceil(tile_h);
        let total_tiles = tiles_per_row * tiles_per_col;

        let label = format!("{}x{}", tile_w, tile_h);

        group.bench_with_input(
            BenchmarkId::new("tiles_for_4k_image", &label),
            &(tile_w, tile_h, total_tiles),
            |b, (tw, th, count)| {
                b.iter(|| {
                    let tile = generate_tile(*tw, *th, 0, 0);
                    for _ in 0..*count {
                        black_box(tile.as_slice());
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_tile_alignment(c: &mut Criterion) {
    let mut group = c.benchmark_group("tile/alignment");

    let tile_width = 256u64;
    let tile_height = 256u64;

    // Aligned access (reading exactly one tile)
    let aligned_tile = generate_tile(tile_width, tile_height, 0, 0);

    group.throughput(Throughput::Bytes(aligned_tile.len() as u64));
    group.bench_function("aligned_access", |b| {
        b.iter(|| {
            black_box(aligned_tile.as_slice());
        });
    });

    // Unaligned access (reading across tile boundaries)
    // Would require reading 4 tiles to get the data
    group.bench_function("unaligned_cross_4_tiles", |b| {
        b.iter(|| {
            let tile1 = generate_tile(tile_width, tile_height, 0, 0);
            let tile2 = generate_tile(tile_width, tile_height, 1, 0);
            let tile3 = generate_tile(tile_width, tile_height, 0, 1);
            let tile4 = generate_tile(tile_width, tile_height, 1, 1);

            black_box(&tile1);
            black_box(&tile2);
            black_box(&tile3);
            black_box(&tile4);
        });
    });

    group.finish();
}

#[cfg(all(feature = "deflate", feature = "lzw"))]
criterion_group!(
    benches,
    bench_tile_creation,
    bench_tile_compression,
    bench_tile_lzw,
    bench_tile_access_patterns,
    bench_tile_pyramid,
    bench_tile_batch_vs_stream,
    bench_tile_copy_vs_reference,
    bench_tile_decoding_pipeline,
    bench_tile_size_impact,
    bench_tile_alignment
);

#[cfg(all(feature = "deflate", not(feature = "lzw")))]
criterion_group!(
    benches,
    bench_tile_creation,
    bench_tile_compression,
    bench_tile_access_patterns,
    bench_tile_pyramid,
    bench_tile_batch_vs_stream,
    bench_tile_copy_vs_reference,
    bench_tile_decoding_pipeline,
    bench_tile_size_impact,
    bench_tile_alignment
);

#[cfg(all(not(feature = "deflate"), feature = "lzw"))]
criterion_group!(
    benches,
    bench_tile_creation,
    bench_tile_lzw,
    bench_tile_access_patterns,
    bench_tile_pyramid,
    bench_tile_batch_vs_stream,
    bench_tile_copy_vs_reference,
    bench_tile_decoding_pipeline,
    bench_tile_size_impact,
    bench_tile_alignment
);

#[cfg(not(any(feature = "deflate", feature = "lzw")))]
criterion_group!(
    benches,
    bench_tile_creation,
    bench_tile_access_patterns,
    bench_tile_pyramid,
    bench_tile_batch_vs_stream,
    bench_tile_copy_vs_reference,
    bench_tile_decoding_pipeline,
    bench_tile_size_impact,
    bench_tile_alignment
);

criterion_main!(benches);
