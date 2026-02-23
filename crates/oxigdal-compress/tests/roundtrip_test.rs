//! Round-trip compression tests
#![allow(clippy::panic)]

use oxigdal_compress::codecs::*;

fn generate_test_data(pattern: &str, size: usize) -> Vec<u8> {
    match pattern {
        "uniform" => vec![42u8; size],
        "random" => (0..size).map(|i| (i % 256) as u8).collect(),
        "text" => b"The quick brown fox jumps over the lazy dog. "
            .iter()
            .cycle()
            .take(size)
            .copied()
            .collect(),
        "binary" => (0..size).map(|i| ((i * 137) % 256) as u8).collect(),
        _ => vec![0u8; size],
    }
}

#[test]
fn test_lz4_roundtrip_various_patterns() {
    let codec = Lz4Codec::new();

    for pattern in ["uniform", "random", "text", "binary"] {
        for size in [100, 1000, 10000] {
            let data = generate_test_data(pattern, size);

            let compressed = codec.compress(&data).unwrap_or_else(|_| {
                panic!(
                    "LZ4 compression failed for pattern {} size {}",
                    pattern, size
                )
            });

            let decompressed = codec
                .decompress(&compressed, Some(size))
                .unwrap_or_else(|_| {
                    panic!(
                        "LZ4 decompression failed for pattern {} size {}",
                        pattern, size
                    )
                });

            assert_eq!(
                decompressed, data,
                "LZ4 roundtrip failed for pattern {} size {}",
                pattern, size
            );
        }
    }
}

#[test]
fn test_zstd_roundtrip_various_patterns() {
    let codec = ZstdCodec::new();

    for pattern in ["uniform", "random", "text", "binary"] {
        for size in [100, 1000, 10000] {
            let data = generate_test_data(pattern, size);

            let compressed = codec.compress(&data).unwrap_or_else(|_| {
                panic!(
                    "Zstd compression failed for pattern {} size {}",
                    pattern, size
                )
            });

            let decompressed = codec
                .decompress(&compressed, Some(size * 2))
                .unwrap_or_else(|_| {
                    panic!(
                        "Zstd decompression failed for pattern {} size {}",
                        pattern, size
                    )
                });

            assert_eq!(
                decompressed, data,
                "Zstd roundtrip failed for pattern {} size {}",
                pattern, size
            );
        }
    }
}

#[test]
fn test_brotli_roundtrip_various_patterns() {
    let codec = BrotliCodec::new();

    for pattern in ["uniform", "text", "binary"] {
        for size in [100, 1000, 10000] {
            let data = generate_test_data(pattern, size);

            let compressed = codec.compress(&data).unwrap_or_else(|_| {
                panic!(
                    "Brotli compression failed for pattern {} size {}",
                    pattern, size
                )
            });

            let decompressed = codec.decompress(&compressed).unwrap_or_else(|_| {
                panic!(
                    "Brotli decompression failed for pattern {} size {}",
                    pattern, size
                )
            });

            assert_eq!(
                decompressed, data,
                "Brotli roundtrip failed for pattern {} size {}",
                pattern, size
            );
        }
    }
}

#[test]
fn test_snappy_roundtrip_various_patterns() {
    let codec = SnappyCodec::new();

    for pattern in ["uniform", "text", "binary"] {
        for size in [100, 1000, 10000] {
            let data = generate_test_data(pattern, size);

            let compressed = codec.compress(&data).unwrap_or_else(|_| {
                panic!(
                    "Snappy compression failed for pattern {} size {}",
                    pattern, size
                )
            });

            let decompressed = codec.decompress(&compressed).unwrap_or_else(|_| {
                panic!(
                    "Snappy decompression failed for pattern {} size {}",
                    pattern, size
                )
            });

            assert_eq!(
                decompressed, data,
                "Snappy roundtrip failed for pattern {} size {}",
                pattern, size
            );
        }
    }
}

#[test]
fn test_deflate_roundtrip_various_patterns() {
    let codec = DeflateCodec::new();

    for pattern in ["uniform", "text", "binary"] {
        for size in [100, 1000, 10000] {
            let data = generate_test_data(pattern, size);

            let compressed = codec.compress(&data).unwrap_or_else(|_| {
                panic!(
                    "Deflate compression failed for pattern {} size {}",
                    pattern, size
                )
            });

            let decompressed = codec.decompress(&compressed).unwrap_or_else(|_| {
                panic!(
                    "Deflate decompression failed for pattern {} size {}",
                    pattern, size
                )
            });

            assert_eq!(
                decompressed, data,
                "Deflate roundtrip failed for pattern {} size {}",
                pattern, size
            );
        }
    }
}

#[test]
fn test_rle_roundtrip() {
    let codec = RleCodec::new();

    let mut data = Vec::new();
    data.extend(vec![1u8; 100]);
    data.extend(vec![2u8; 50]);
    data.extend(vec![3u8; 75]);

    let compressed = codec.compress(&data).expect("RLE compression failed");
    let decompressed = codec
        .decompress(&compressed)
        .expect("RLE decompression failed");

    assert_eq!(decompressed, data);
}

#[test]
fn test_delta_roundtrip_f32() {
    use byteorder::{LittleEndian, WriteBytesExt};

    let config = DeltaConfig::with_data_type(DeltaDataType::F32);
    let codec = DeltaCodec::with_config(config);

    let mut data = Vec::new();
    for i in 0..1000 {
        data.write_f32::<LittleEndian>(i as f32 * 0.1).ok();
    }

    let compressed = codec.compress(&data).expect("Delta compression failed");
    let decompressed = codec
        .decompress(&compressed)
        .expect("Delta decompression failed");

    assert_eq!(decompressed, data);
}

#[test]
fn test_parallel_lz4_roundtrip() {
    use oxigdal_compress::parallel::ParallelCompressor;

    let compressor = ParallelCompressor::new();
    let data = b"Parallel compression test".repeat(100000);

    let (compressed, _metadata) = compressor
        .compress_lz4(&data)
        .expect("Parallel LZ4 compression failed");

    let decompressed = compressor
        .decompress_lz4(&compressed)
        .expect("Parallel LZ4 decompression failed");

    assert_eq!(decompressed, data);
}

#[test]
fn test_parallel_zstd_roundtrip() {
    use oxigdal_compress::parallel::ParallelCompressor;

    let compressor = ParallelCompressor::new();
    let data = b"Parallel compression test".repeat(100000);

    let (compressed, _metadata) = compressor
        .compress_zstd(&data)
        .expect("Parallel Zstd compression failed");

    let decompressed = compressor
        .decompress_zstd(&compressed)
        .expect("Parallel Zstd decompression failed");

    assert_eq!(decompressed, data);
}
