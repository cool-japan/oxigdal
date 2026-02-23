//! Comprehensive codec tests

use oxigdal_compress::codecs::*;

#[test]
fn test_lz4_various_sizes() {
    let codec = Lz4Codec::new();

    for size in [0, 1, 100, 1000, 10000, 100000] {
        let data = vec![42u8; size];

        if size == 0 {
            let compressed = codec.compress(&data).expect("Compression failed");
            assert_eq!(compressed.len(), 0);
            continue;
        }

        let compressed = codec.compress(&data).expect("Compression failed");
        assert!(compressed.len() < data.len() || size < 100);

        let decompressed = codec
            .decompress(&compressed, Some(size))
            .expect("Decompression failed");
        assert_eq!(decompressed, data);
    }
}

#[test]
fn test_zstd_compression_levels() {
    for level in [1, 3, 9, 15, 22] {
        let config = ZstdConfig::with_level(level).expect("Config creation failed");
        let codec = ZstdCodec::with_config(config);

        let data = b"Test data".repeat(1000);

        let compressed = codec.compress(&data).expect("Compression failed");
        let decompressed = codec
            .decompress(&compressed, Some(data.len() * 2))
            .expect("Decompression failed");

        assert_eq!(decompressed, data);
    }
}

#[test]
fn test_brotli_roundtrip() {
    let codec = BrotliCodec::new();
    let data = b"Brotli compression test data".repeat(100);

    let compressed = codec.compress(&data).expect("Compression failed");
    assert!(compressed.len() < data.len());

    let decompressed = codec.decompress(&compressed).expect("Decompression failed");
    assert_eq!(decompressed, data);
}

#[test]
fn test_snappy_roundtrip() {
    let codec = SnappyCodec::new();
    let data = b"Snappy compression test data".repeat(100);

    let compressed = codec.compress(&data).expect("Compression failed");
    assert!(compressed.len() < data.len());

    let decompressed = codec.decompress(&compressed).expect("Decompression failed");
    assert_eq!(decompressed, data);
}

#[test]
fn test_deflate_formats() {
    for format in [DeflateFormat::Zlib, DeflateFormat::Gzip] {
        let config = DeflateConfig::with_level(6)
            .expect("Config creation failed")
            .with_format(format);
        let codec = DeflateCodec::with_config(config);

        let data = b"Deflate test".repeat(100);

        let compressed = codec.compress(&data).expect("Compression failed");
        let decompressed = codec.decompress(&compressed).expect("Decompression failed");

        assert_eq!(decompressed, data);
    }
}

#[test]
fn test_delta_encoding() {
    use byteorder::{LittleEndian, WriteBytesExt};

    let config = DeltaConfig::with_data_type(DeltaDataType::I32);
    let codec = DeltaCodec::with_config(config);

    let mut data = Vec::new();
    for i in 0..100 {
        data.write_i32::<LittleEndian>(i * 10).ok();
    }

    let compressed = codec.compress(&data).expect("Compression failed");
    let decompressed = codec.decompress(&compressed).expect("Decompression failed");

    assert_eq!(decompressed, data);
}

#[test]
fn test_rle_compression() {
    let codec = RleCodec::new();

    // Highly compressible data
    let data = vec![5u8; 10000];

    let compressed = codec.compress(&data).expect("Compression failed");
    assert!(compressed.len() < data.len() / 10); // Should compress very well

    let decompressed = codec.decompress(&compressed).expect("Decompression failed");
    assert_eq!(decompressed, data);
}

#[test]
fn test_dictionary_compression() {
    let config = DictionaryConfig::with_symbol_size(4);
    let codec = DictionaryCodec::with_config(config);

    let mut data = Vec::new();
    // Repeated 4-byte patterns
    for _ in 0..100 {
        data.extend_from_slice(&[1u8, 2, 3, 4]);
    }
    for _ in 0..100 {
        data.extend_from_slice(&[5u8, 6, 7, 8]);
    }

    let compressed = codec.compress(&data).expect("Compression failed");
    assert!(compressed.len() < data.len());

    let decompressed = codec.decompress(&compressed).expect("Decompression failed");
    assert_eq!(decompressed, data);
}

#[test]
fn test_all_codecs_empty_data() {
    let empty: &[u8] = b"";

    // LZ4
    let codec = Lz4Codec::new();
    let compressed = codec.compress(empty).expect("Compression failed");
    assert_eq!(compressed.len(), 0);

    // Zstd
    let codec = ZstdCodec::new();
    let compressed = codec.compress(empty).expect("Compression failed");
    assert_eq!(compressed.len(), 0);

    // Brotli
    let codec = BrotliCodec::new();
    let compressed = codec.compress(empty).expect("Compression failed");
    assert_eq!(compressed.len(), 0);

    // Snappy
    let codec = SnappyCodec::new();
    let compressed = codec.compress(empty).expect("Compression failed");
    assert_eq!(compressed.len(), 0);

    // Deflate
    let codec = DeflateCodec::new();
    let compressed = codec.compress(empty).expect("Compression failed");
    assert_eq!(compressed.len(), 0);

    // RLE
    let codec = RleCodec::new();
    let compressed = codec.compress(empty).expect("Compression failed");
    assert_eq!(compressed.len(), 0);

    // Dictionary
    let codec = DictionaryCodec::new();
    let compressed = codec.compress(empty).expect("Compression failed");
    assert_eq!(compressed.len(), 0);
}
