//! Comprehensive sharding codec tests for Zarr v3.
//!
//! Tests cover:
//! - ShardIndex construction and coordinate addressing
//! - ShardIndex serialization/deserialization round-trip
//! - ShardWriter / ShardReader encode-decode round-trip with end-of-shard index
//! - ShardWriter with start-of-shard index
//! - Partial read (reading a specific inner chunk by coordinates)
//! - Missing chunk handling (offset = u64::MAX)
//! - Error cases: corrupted index, out-of-bounds coords, wrong dimension count
//! - 1D and higher-dimensional shard layouts
//! - Multiple chunks written, verified in correct positions after read-back

use oxigdal_zarr::codecs::CodecChain;
use oxigdal_zarr::sharding::{
    IndexLocation, ShardIndex, ShardIndexEntry, ShardReader, ShardWriter,
};

// ── Helper ──────────────────────────────────────────────────────────────────

fn null_chain() -> CodecChain {
    CodecChain::empty()
}

// ── ShardIndex construction ──────────────────────────────────────────────────

#[test]
fn test_shard_index_1d_total_chunks() {
    let idx = ShardIndex::new(vec![5]);
    assert_eq!(idx.num_chunks(), 5);
}

#[test]
fn test_shard_index_2d_total_chunks() {
    let idx = ShardIndex::new(vec![3, 4]);
    assert_eq!(idx.num_chunks(), 12);
}

#[test]
fn test_shard_index_3d_total_chunks() {
    let idx = ShardIndex::new(vec![2, 3, 4]);
    assert_eq!(idx.num_chunks(), 24);
}

#[test]
fn test_shard_index_all_entries_missing_by_default() {
    let idx = ShardIndex::new(vec![2, 2]);
    for coords in [[0, 0], [0, 1], [1, 0], [1, 1]] {
        let entry = idx.get(&coords).expect("get");
        assert!(entry.is_missing(), "entry at {coords:?} should be missing");
    }
}

// ── ShardIndex coordinate addressing ────────────────────────────────────────

#[test]
fn test_shard_index_flat_index_order() {
    // Row-major (C-order) indexing: last dimension varies fastest.
    let idx = ShardIndex::new(vec![2, 3]);
    assert_eq!(idx.coords_to_index(&[0, 0]).expect("idx"), 0);
    assert_eq!(idx.coords_to_index(&[0, 1]).expect("idx"), 1);
    assert_eq!(idx.coords_to_index(&[0, 2]).expect("idx"), 2);
    assert_eq!(idx.coords_to_index(&[1, 0]).expect("idx"), 3);
    assert_eq!(idx.coords_to_index(&[1, 2]).expect("idx"), 5);
}

#[test]
fn test_shard_index_out_of_bounds_coord_errors() {
    let idx = ShardIndex::new(vec![2, 2]);
    assert!(idx.coords_to_index(&[2, 0]).is_err());
    assert!(idx.coords_to_index(&[0, 2]).is_err());
}

#[test]
fn test_shard_index_wrong_dim_count_errors() {
    let idx = ShardIndex::new(vec![2, 2]);
    assert!(idx.coords_to_index(&[0]).is_err());
    assert!(idx.coords_to_index(&[0, 0, 0]).is_err());
}

// ── ShardIndex serialization round-trip ─────────────────────────────────────

#[test]
fn test_shard_index_roundtrip_2x2() {
    let mut idx = ShardIndex::new(vec![2, 2]);
    idx.set(&[0, 0], ShardIndexEntry::new(0, 100)).expect("set");
    idx.set(&[0, 1], ShardIndexEntry::new(100, 200))
        .expect("set");
    idx.set(&[1, 1], ShardIndexEntry::new(300, 50))
        .expect("set");
    // [1,0] remains missing

    let encoded = idx.encode().expect("encode");
    let decoded = ShardIndex::decode(&encoded, vec![2, 2]).expect("decode");

    assert_eq!(
        decoded.get(&[0, 0]).expect("get"),
        ShardIndexEntry::new(0, 100)
    );
    assert_eq!(
        decoded.get(&[0, 1]).expect("get"),
        ShardIndexEntry::new(100, 200)
    );
    assert!(decoded.get(&[1, 0]).expect("get").is_missing());
    assert_eq!(
        decoded.get(&[1, 1]).expect("get"),
        ShardIndexEntry::new(300, 50)
    );
}

#[test]
fn test_shard_index_decode_wrong_size_errors() {
    // 2×2 shard needs exactly 4×16 = 64 bytes.
    let short_data = vec![0u8; 48];
    assert!(ShardIndex::decode(&short_data, vec![2, 2]).is_err());

    let long_data = vec![0u8; 80];
    assert!(ShardIndex::decode(&long_data, vec![2, 2]).is_err());
}

// ── ShardIndexEntry ──────────────────────────────────────────────────────────

#[test]
fn test_shard_index_entry_missing_sentinel() {
    let m = ShardIndexEntry::missing();
    assert_eq!(m.offset, u64::MAX);
    assert_eq!(m.size, u64::MAX);
    assert!(m.is_missing());
}

#[test]
fn test_shard_index_entry_encode_decode_roundtrip() {
    for (offset, size) in [(0u64, 1u64), (u64::MAX - 1, 42), (999_999, 1_234_567)] {
        let entry = ShardIndexEntry::new(offset, size);
        let encoded = entry.encode().expect("encode");
        assert_eq!(encoded.len(), 16, "entry should be 16 bytes");
        let decoded = ShardIndexEntry::decode(&encoded).expect("decode");
        assert_eq!(decoded.offset, offset);
        assert_eq!(decoded.size, size);
    }
}

#[test]
fn test_shard_index_entry_decode_too_short_errors() {
    assert!(ShardIndexEntry::decode(&[0u8; 15]).is_err());
    assert!(ShardIndexEntry::decode(&[0u8; 8]).is_err());
    assert!(ShardIndexEntry::decode(&[]).is_err());
}

// ── ShardWriter / ShardReader end-of-shard round-trip ───────────────────────

#[test]
fn test_shard_roundtrip_end_index_single_chunk() {
    let mut writer = ShardWriter::new(vec![1, 1], null_chain(), null_chain(), IndexLocation::End);

    let payload = b"hello_zarr_shard";
    writer
        .write_chunk(vec![0, 0], payload.to_vec())
        .expect("write");

    let shard_data = writer.finalize().expect("finalize");
    assert!(!shard_data.is_empty());

    let reader = ShardReader::new(
        shard_data,
        vec![1, 1],
        null_chain(),
        null_chain(),
        IndexLocation::End,
    )
    .expect("reader");

    let chunk = reader.read_chunk(&[0, 0]).expect("read").expect("present");
    assert_eq!(chunk, payload.to_vec());
}

#[test]
fn test_shard_roundtrip_end_index_multiple_chunks() {
    let chunks_per_shard = vec![2, 2]; // 4 inner chunks
    let mut writer = ShardWriter::new(
        chunks_per_shard.clone(),
        null_chain(),
        null_chain(),
        IndexLocation::End,
    );

    let payloads: &[(&[usize], &[u8])] = &[
        (&[0, 0], b"chunk-00"),
        (&[0, 1], b"chunk-01-longer"),
        (&[1, 0], b"chunk-10"),
        (&[1, 1], b"chunk-11-longest-payload"),
    ];

    for &(coords, data) in payloads {
        writer
            .write_chunk(coords.to_vec(), data.to_vec())
            .expect("write");
    }

    let shard_data = writer.finalize().expect("finalize");

    let reader = ShardReader::new(
        shard_data,
        chunks_per_shard,
        null_chain(),
        null_chain(),
        IndexLocation::End,
    )
    .expect("reader");

    for &(coords, expected) in payloads {
        let chunk = reader
            .read_chunk(coords)
            .expect("read")
            .expect("chunk present");
        assert_eq!(chunk, expected.to_vec(), "mismatch for {coords:?}");
    }
}

#[test]
fn test_shard_missing_chunk_returns_none() {
    let chunks_per_shard = vec![2, 2];
    let mut writer = ShardWriter::new(
        chunks_per_shard.clone(),
        null_chain(),
        null_chain(),
        IndexLocation::End,
    );

    // Only write one of the four inner chunks.
    writer
        .write_chunk(vec![0, 0], b"only-chunk".to_vec())
        .expect("write");

    let shard_data = writer.finalize().expect("finalize");

    let reader = ShardReader::new(
        shard_data,
        chunks_per_shard,
        null_chain(),
        null_chain(),
        IndexLocation::End,
    )
    .expect("reader");

    // Written chunk should be present.
    assert!(reader.read_chunk(&[0, 0]).expect("read").is_some());

    // Others should be absent (missing sentinel).
    assert!(reader.read_chunk(&[0, 1]).expect("read").is_none());
    assert!(reader.read_chunk(&[1, 0]).expect("read").is_none());
    assert!(reader.read_chunk(&[1, 1]).expect("read").is_none());
}

// ── ShardWriter start-of-shard index ────────────────────────────────────────

#[test]
fn test_shard_writer_start_index_produces_non_empty_output() {
    let mut writer = ShardWriter::new(vec![2, 2], null_chain(), null_chain(), IndexLocation::Start);

    writer
        .write_chunk(vec![0, 0], b"data".to_vec())
        .expect("write");
    writer
        .write_chunk(vec![1, 1], b"more-data".to_vec())
        .expect("write");

    let shard_data = writer.finalize().expect("finalize");
    // Start-of-shard: first 8 bytes are the index size.
    assert!(shard_data.len() >= 8);
}

// ── IndexLocation ────────────────────────────────────────────────────────────

#[test]
fn test_index_location_default_is_end() {
    assert_eq!(IndexLocation::default(), IndexLocation::End);
}

#[test]
fn test_index_location_str_roundtrip() {
    assert_eq!(IndexLocation::Start.as_str(), "start");
    assert_eq!(IndexLocation::End.as_str(), "end");
    assert_eq!(
        IndexLocation::from_str("start").expect("parse"),
        IndexLocation::Start
    );
    assert_eq!(
        IndexLocation::from_str("end").expect("parse"),
        IndexLocation::End
    );
    assert!(IndexLocation::from_str("unknown").is_err());
}

// ── ShardWriter validation ───────────────────────────────────────────────────

#[test]
fn test_shard_writer_wrong_dim_count_errors() {
    let mut writer = ShardWriter::new(vec![2, 2], null_chain(), null_chain(), IndexLocation::End);
    assert!(writer.write_chunk(vec![0], b"data".to_vec()).is_err());
    assert!(writer.write_chunk(vec![0, 0, 0], b"data".to_vec()).is_err());
}

#[test]
fn test_shard_writer_out_of_bounds_coord_errors() {
    let mut writer = ShardWriter::new(vec![2, 2], null_chain(), null_chain(), IndexLocation::End);
    assert!(writer.write_chunk(vec![2, 0], b"data".to_vec()).is_err());
    assert!(writer.write_chunk(vec![0, 3], b"data".to_vec()).is_err());
}

#[test]
fn test_shard_writer_num_chunks_counter() {
    let mut writer = ShardWriter::new(vec![3, 3], null_chain(), null_chain(), IndexLocation::End);
    assert_eq!(writer.num_chunks(), 0);
    writer
        .write_chunk(vec![0, 0], b"a".to_vec())
        .expect("write");
    assert_eq!(writer.num_chunks(), 1);
    writer
        .write_chunk(vec![2, 2], b"b".to_vec())
        .expect("write");
    assert_eq!(writer.num_chunks(), 2);
}

// ── ShardIndex iterator ──────────────────────────────────────────────────────

#[test]
fn test_shard_index_iter_covers_all_coords() {
    let mut idx = ShardIndex::new(vec![2, 3]);
    idx.set(&[0, 2], ShardIndexEntry::new(50, 10)).expect("set");

    let entries: Vec<(Vec<usize>, ShardIndexEntry)> = idx.iter().collect();
    assert_eq!(entries.len(), 6); // 2×3 = 6 total

    // Find the entry we set
    let found = entries
        .iter()
        .find(|(coords, _)| coords == &vec![0, 2])
        .expect("entry [0,2] must be present");
    assert_eq!(found.1, ShardIndexEntry::new(50, 10));
}

// ── ShardReader index accessor ───────────────────────────────────────────────

#[test]
fn test_shard_reader_exposes_index() {
    let mut writer = ShardWriter::new(vec![1, 2], null_chain(), null_chain(), IndexLocation::End);
    writer
        .write_chunk(vec![0, 0], b"first".to_vec())
        .expect("write");
    writer
        .write_chunk(vec![0, 1], b"second".to_vec())
        .expect("write");
    let shard_data = writer.finalize().expect("finalize");

    let reader = ShardReader::new(
        shard_data,
        vec![1, 2],
        null_chain(),
        null_chain(),
        IndexLocation::End,
    )
    .expect("reader");

    let idx = reader.index();
    assert_eq!(idx.num_chunks(), 2);
    assert!(!idx.get(&[0, 0]).expect("get").is_missing());
    assert!(!idx.get(&[0, 1]).expect("get").is_missing());
}
