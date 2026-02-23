//! Comprehensive tests for Zarr v3 implementation
//!
//! This test suite covers all Zarr v3 features including metadata, codecs,
//! sharding, storage transformers, and round-trip operations.

use oxigdal_zarr::Store;
use oxigdal_zarr::codecs::{CodecChain, NullCodec};
use oxigdal_zarr::metadata::v3::{
    ArrayMetadataV3, ChunkGrid, ChunkKeyEncoding, CodecMetadata, DataType, FillValue, GzipConfig,
    ZstdConfig,
};
use oxigdal_zarr::sharding::{IndexLocation, ShardIndex, ShardIndexEntry, ShardWriter};
use oxigdal_zarr::storage::memory::MemoryStore;
use oxigdal_zarr::transformers::{
    Crc32Transformer, Sha256Transformer, Transformer, TransformerChain,
};
use std::env;

#[test]
fn test_v3_metadata_creation_and_validation() {
    // Test basic metadata creation
    let metadata = ArrayMetadataV3::new(vec![1000, 2000, 3000], vec![100, 200, 300], "float32");

    assert_eq!(metadata.zarr_format, 3);
    assert_eq!(metadata.shape, vec![1000, 2000, 3000]);
    assert_eq!(metadata.ndim(), 3);
    assert_eq!(metadata.size(), 1000 * 2000 * 3000);
    assert!(metadata.validate().is_ok());
}

#[test]
fn test_v3_metadata_with_dimension_names() {
    let metadata = ArrayMetadataV3::new(vec![100, 200], vec![10, 20], "int32")
        .with_dimension_names(vec![Some("x".to_string()), Some("y".to_string())]);

    assert!(metadata.validate().is_ok());
    assert_eq!(metadata.dimension_names.as_ref().map(|d| d.len()), Some(2));
}

#[test]
fn test_v3_metadata_with_invalid_dimension_names() {
    let mut metadata = ArrayMetadataV3::new(vec![100, 200], vec![10, 20], "float32");

    // Wrong number of dimension names (should be 2, but we provide 1)
    metadata.dimension_names = Some(vec![Some("x".to_string())]);

    assert!(metadata.validate().is_err());
}

#[test]
fn test_v3_chunk_grid_regular() {
    let grid = ChunkGrid::regular(vec![10, 20, 30]);
    assert_eq!(
        grid.regular_chunk_shape().expect("chunk shape"),
        &[10, 20, 30]
    );
}

#[test]
fn test_v3_chunk_grid_rectangular() {
    let grid = ChunkGrid::rectangular(vec![vec![10, 15, 20], vec![20, 25, 30]]);
    assert!(grid.regular_chunk_shape().is_err());
}

#[test]
fn test_v3_chunk_grid_variable() {
    let grid = ChunkGrid::variable(vec![vec![0, 10, 25, 50], vec![0, 20, 45, 100]]);
    assert!(grid.regular_chunk_shape().is_err());
}

#[test]
fn test_v3_chunk_key_encoding_default() {
    let encoding = ChunkKeyEncoding::default_with_separator("/");
    assert!(matches!(encoding, ChunkKeyEncoding::Default { .. }));
}

#[test]
fn test_v3_chunk_key_encoding_v2() {
    let encoding = ChunkKeyEncoding::v2_with_separator(".");
    assert!(matches!(encoding, ChunkKeyEncoding::V2 { .. }));
}

#[test]
fn test_v3_fill_value_types() {
    assert!(FillValue::Null.is_null());
    assert!(!FillValue::Int(0).is_null());
    assert!(!FillValue::Float(0.0).is_null());
    assert!(!FillValue::Bool(false).is_null());
}

#[test]
fn test_v3_data_type_simple() {
    let dt = DataType::simple("float32");
    assert_eq!(dt.as_str(), "float32");
    assert_eq!(dt.item_size().expect("item size"), 4);
}

#[test]
fn test_v3_data_type_sizes() {
    assert_eq!(DataType::simple("int8").item_size().expect("int8"), 1);
    assert_eq!(DataType::simple("int16").item_size().expect("int16"), 2);
    assert_eq!(DataType::simple("int32").item_size().expect("int32"), 4);
    assert_eq!(DataType::simple("int64").item_size().expect("int64"), 8);
    assert_eq!(DataType::simple("float32").item_size().expect("float32"), 4);
    assert_eq!(DataType::simple("float64").item_size().expect("float64"), 8);
}

#[test]
fn test_v3_codec_metadata_gzip() {
    let codec = CodecMetadata::Gzip {
        configuration: Some(GzipConfig { level: Some(6) }),
    };

    assert!(matches!(codec, CodecMetadata::Gzip { .. }));
}

#[test]
fn test_v3_codec_metadata_zstd() {
    let codec = CodecMetadata::Zstd {
        configuration: Some(ZstdConfig {
            level: Some(3),
            checksum: Some(true),
        }),
    };

    assert!(matches!(codec, CodecMetadata::Zstd { .. }));
}

#[test]
fn test_shard_index_entry_encode_decode() {
    let entry = ShardIndexEntry::new(12345, 6789);
    let encoded = entry.encode().expect("encode");
    assert_eq!(encoded.len(), 16);

    let decoded = ShardIndexEntry::decode(&encoded).expect("decode");
    assert_eq!(decoded, entry);
}

#[test]
fn test_shard_index_entry_missing() {
    let missing = ShardIndexEntry::missing();
    assert!(missing.is_missing());
    assert_eq!(missing.offset, u64::MAX);
    assert_eq!(missing.size, u64::MAX);
}

#[test]
fn test_shard_index_coords_to_index() {
    let index = ShardIndex::new(vec![2, 3, 4]);

    // Test various coordinate conversions
    assert_eq!(index.coords_to_index(&[0, 0, 0]).expect("idx"), 0);
    assert_eq!(index.coords_to_index(&[0, 0, 1]).expect("idx"), 1);
    assert_eq!(index.coords_to_index(&[0, 0, 2]).expect("idx"), 2);
    assert_eq!(index.coords_to_index(&[0, 1, 0]).expect("idx"), 4);
    assert_eq!(index.coords_to_index(&[0, 2, 0]).expect("idx"), 8);
    assert_eq!(index.coords_to_index(&[1, 0, 0]).expect("idx"), 12);
    assert_eq!(index.coords_to_index(&[1, 2, 3]).expect("idx"), 23);
}

#[test]
fn test_shard_index_set_get() {
    let mut index = ShardIndex::new(vec![3, 3]);
    let entry1 = ShardIndexEntry::new(100, 50);
    let entry2 = ShardIndexEntry::new(200, 75);

    index.set(&[0, 1], entry1).expect("set");
    index.set(&[2, 2], entry2).expect("set");

    assert_eq!(index.get(&[0, 1]).expect("get"), entry1);
    assert_eq!(index.get(&[2, 2]).expect("get"), entry2);
}

#[test]
fn test_shard_index_out_of_bounds() {
    let index = ShardIndex::new(vec![2, 2]);

    // Out of bounds coordinates
    assert!(index.coords_to_index(&[2, 0]).is_err());
    assert!(index.coords_to_index(&[0, 2]).is_err());
    assert!(index.coords_to_index(&[2, 2]).is_err());
}

#[test]
fn test_shard_index_encode_decode() {
    let mut index = ShardIndex::new(vec![2, 3]);

    index
        .set(&[0, 0], ShardIndexEntry::new(0, 100))
        .expect("set");
    index
        .set(&[0, 1], ShardIndexEntry::new(100, 150))
        .expect("set");
    index
        .set(&[1, 2], ShardIndexEntry::new(250, 200))
        .expect("set");

    let encoded = index.encode().expect("encode");
    assert_eq!(encoded.len(), 2 * 3 * 16); // 2x3 entries * 16 bytes each

    let decoded = ShardIndex::decode(&encoded, vec![2, 3]).expect("decode");

    assert_eq!(
        decoded.get(&[0, 0]).expect("get"),
        ShardIndexEntry::new(0, 100)
    );
    assert_eq!(
        decoded.get(&[0, 1]).expect("get"),
        ShardIndexEntry::new(100, 150)
    );
    assert_eq!(
        decoded.get(&[1, 2]).expect("get"),
        ShardIndexEntry::new(250, 200)
    );
}

#[test]
fn test_shard_writer_basic() {
    let codec = CodecChain::empty();
    let index_codec = CodecChain::empty();
    let mut writer = ShardWriter::new(vec![2, 2], codec, index_codec, IndexLocation::End);

    // Write some chunks
    writer
        .write_chunk(vec![0, 0], vec![1, 2, 3, 4])
        .expect("write chunk");
    writer
        .write_chunk(vec![0, 1], vec![5, 6, 7, 8])
        .expect("write chunk");
    writer
        .write_chunk(vec![1, 0], vec![9, 10, 11, 12])
        .expect("write chunk");

    assert_eq!(writer.num_chunks(), 3);

    // Finalize and get shard data
    let shard_data = writer.finalize().expect("finalize");
    assert!(!shard_data.is_empty());
}

#[test]
fn test_shard_writer_invalid_coords() {
    let codec = CodecChain::empty();
    let index_codec = CodecChain::empty();
    let mut writer = ShardWriter::new(vec![2, 2], codec, index_codec, IndexLocation::End);

    // Wrong number of dimensions
    assert!(writer.write_chunk(vec![0], vec![1, 2, 3]).is_err());

    // Out of bounds
    assert!(writer.write_chunk(vec![2, 0], vec![1, 2, 3]).is_err());
    assert!(writer.write_chunk(vec![0, 2], vec![1, 2, 3]).is_err());
}

#[test]
fn test_crc32_transformer_round_trip() {
    let transformer = Crc32Transformer::new(true);
    let data = b"Test data for CRC32 checksum verification";

    let encoded = transformer.encode(data).expect("encode");
    assert_eq!(encoded.len(), data.len() + 4); // Original data + 4 bytes checksum

    let decoded = transformer.decode(&encoded).expect("decode");
    assert_eq!(decoded, data);
}

#[test]
fn test_crc32_transformer_checksum_mismatch() {
    let transformer = Crc32Transformer::new(true);
    let data = b"Test data";

    let mut encoded = transformer.encode(data).expect("encode");

    // Corrupt the data
    encoded[0] ^= 0xFF;

    let result = transformer.decode(&encoded);
    assert!(result.is_err());
}

#[test]
fn test_sha256_transformer_round_trip() {
    let transformer = Sha256Transformer::new(false);
    let data = b"Test data for SHA256 hash verification";

    let encoded = transformer.encode(data).expect("encode");
    assert_eq!(encoded.len(), data.len() + 32); // Original data + 32 bytes hash

    let decoded = transformer.decode(&encoded).expect("decode");
    assert_eq!(decoded, data);
}

#[test]
fn test_transformer_chain_multiple() {
    let mut chain = TransformerChain::empty();
    chain.add(Box::new(Crc32Transformer::new(true)));
    chain.add(Box::new(Sha256Transformer::new(true)));

    assert_eq!(chain.len(), 2);
    assert!(!chain.is_empty());

    let data = b"Test data for transformer chain".to_vec();
    let encoded = chain.encode(data.clone()).expect("encode");
    let decoded = chain.decode(encoded).expect("decode");

    assert_eq!(decoded, data);
}

#[test]
fn test_codec_chain_empty() {
    let chain = CodecChain::empty();
    assert!(chain.is_empty());
    assert_eq!(chain.len(), 0);

    let data = vec![1, 2, 3, 4, 5];
    let encoded = chain.encode(data.clone()).expect("encode");
    assert_eq!(encoded, data);

    let decoded = chain.decode(encoded).expect("decode");
    assert_eq!(decoded, data);
}

#[test]
fn test_codec_chain_single() {
    let mut chain = CodecChain::empty();
    chain.add(Box::new(NullCodec));

    assert_eq!(chain.len(), 1);

    let data = vec![1, 2, 3, 4, 5];
    let encoded = chain.encode(data.clone()).expect("encode");
    let decoded = chain.decode(encoded).expect("decode");

    assert_eq!(decoded, data);
}

#[test]
fn test_metadata_serialization() {
    let metadata = ArrayMetadataV3::new(vec![100, 200], vec![10, 20], "float32")
        .with_fill_value(FillValue::Float(0.0))
        .with_dimension_names(vec![Some("x".to_string()), Some("y".to_string())]);

    // Serialize to JSON
    let json = serde_json::to_string_pretty(&metadata).expect("serialize");
    assert!(!json.is_empty());
    assert!(json.contains("zarr_format"));
    assert!(json.contains("float32"));

    // Deserialize back
    let deserialized: ArrayMetadataV3 = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(deserialized.zarr_format, 3);
    assert_eq!(deserialized.shape, vec![100, 200]);
}

#[test]
fn test_memory_store_integration() {
    let mut store = MemoryStore::new();
    let key = oxigdal_zarr::storage::StoreKey::new("test/array/data".to_string());
    let data = vec![1, 2, 3, 4, 5];

    // Write data
    store.set(&key, &data).expect("set");

    // Read data back
    let retrieved = store.get(&key).expect("get");
    assert_eq!(retrieved, data);
}

#[test]
fn test_index_location_from_str() {
    assert_eq!(
        IndexLocation::from_str("start").expect("start"),
        IndexLocation::Start
    );
    assert_eq!(
        IndexLocation::from_str("end").expect("end"),
        IndexLocation::End
    );
    assert!(IndexLocation::from_str("middle").is_err());
    assert!(IndexLocation::from_str("unknown").is_err());
}

#[test]
fn test_index_location_as_str() {
    assert_eq!(IndexLocation::Start.as_str(), "start");
    assert_eq!(IndexLocation::End.as_str(), "end");
}

#[test]
fn test_index_location_default() {
    let default_location = IndexLocation::default();
    assert_eq!(default_location, IndexLocation::End);
}

#[test]
fn test_large_shard_index() {
    // Test with a larger shard
    let mut index = ShardIndex::new(vec![10, 10, 10]);
    assert_eq!(index.num_chunks(), 1000);

    // Set some entries
    for i in 0..10 {
        for j in 0..10 {
            let entry = ShardIndexEntry::new((i * 100 + j * 10) as u64, 100);
            index.set(&[i, j, 0], entry).expect("set");
        }
    }

    // Verify entries
    for i in 0..10 {
        for j in 0..10 {
            let entry = index.get(&[i, j, 0]).expect("get");
            assert_eq!(entry.offset, (i * 100 + j * 10) as u64);
            assert_eq!(entry.size, 100);
        }
    }
}

#[test]
fn test_shard_index_iterator() {
    let mut index = ShardIndex::new(vec![2, 2]);

    index
        .set(&[0, 0], ShardIndexEntry::new(0, 10))
        .expect("set");
    index
        .set(&[0, 1], ShardIndexEntry::new(10, 20))
        .expect("set");
    index
        .set(&[1, 0], ShardIndexEntry::new(30, 15))
        .expect("set");
    index
        .set(&[1, 1], ShardIndexEntry::new(45, 25))
        .expect("set");

    let entries: Vec<_> = index.iter().collect();
    assert_eq!(entries.len(), 4);

    // Verify all entries are present
    assert_eq!(entries[0].0, vec![0, 0]);
    assert_eq!(entries[0].1, ShardIndexEntry::new(0, 10));
}

#[test]
fn test_create_temp_zarr_v3_array() {
    use std::fs;

    let temp_dir = env::temp_dir().join(format!("zarr_v3_test_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    // Create metadata
    let metadata = ArrayMetadataV3::new(vec![100, 200], vec![10, 20], "float32");
    let metadata_json = serde_json::to_vec_pretty(&metadata).expect("serialize");

    // Write metadata file
    let metadata_path = temp_dir.join("zarr.json");
    fs::write(&metadata_path, metadata_json).expect("write metadata");

    // Verify file exists
    assert!(metadata_path.exists());

    // Clean up
    fs::remove_dir_all(&temp_dir).expect("cleanup");
}

#[test]
fn test_data_type_conversion() {
    let dt: DataType = "int32".into();
    assert_eq!(dt.as_str(), "int32");

    let dt: DataType = String::from("float64").into();
    assert_eq!(dt.as_str(), "float64");
}

#[test]
fn test_fill_value_default() {
    let fill = FillValue::default();
    assert!(fill.is_null());
}

#[test]
fn test_chunk_key_encoding_default() {
    let encoding = ChunkKeyEncoding::default();
    assert!(matches!(encoding, ChunkKeyEncoding::Default { .. }));
}
