//! Extended integration tests for oxigdal-pmtiles.
//!
//! Brings total test count to 50+.

use oxigdal_pmtiles::{
    Compression, DirectoryEntry, PmTilesError, PmTilesHeader, PmTilesReader, TileType,
    decode_directory, decode_varint,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Build a minimal valid 127-byte PMTiles v3 header.
#[allow(clippy::too_many_arguments)]
fn make_pmtiles_header(
    root_dir_offset: u64,
    root_dir_length: u64,
    metadata_offset: u64,
    metadata_length: u64,
    leaf_dirs_offset: u64,
    leaf_dirs_length: u64,
    tile_data_offset: u64,
    tile_data_length: u64,
    addressed_tiles: u64,
    tile_entries: u64,
    tile_contents: u64,
    clustered: bool,
    internal_compression: u8,
    tile_compression: u8,
    tile_type: u8,
    min_zoom: u8,
    max_zoom: u8,
    min_lon_e7: i32,
    min_lat_e7: i32,
    max_lon_e7: i32,
    max_lat_e7: i32,
    center_zoom: u8,
    center_lon_e7: i32,
    center_lat_e7: i32,
) -> Vec<u8> {
    let mut data = vec![0u8; 127];
    data[0..7].copy_from_slice(b"PMTiles");
    data[7] = 3;
    data[8..16].copy_from_slice(&root_dir_offset.to_le_bytes());
    data[16..24].copy_from_slice(&root_dir_length.to_le_bytes());
    data[24..32].copy_from_slice(&metadata_offset.to_le_bytes());
    data[32..40].copy_from_slice(&metadata_length.to_le_bytes());
    data[40..48].copy_from_slice(&leaf_dirs_offset.to_le_bytes());
    data[48..56].copy_from_slice(&leaf_dirs_length.to_le_bytes());
    data[56..64].copy_from_slice(&tile_data_offset.to_le_bytes());
    data[64..72].copy_from_slice(&tile_data_length.to_le_bytes());
    data[72..80].copy_from_slice(&addressed_tiles.to_le_bytes());
    data[80..88].copy_from_slice(&tile_entries.to_le_bytes());
    data[88..96].copy_from_slice(&tile_contents.to_le_bytes());
    data[96] = if clustered { 1 } else { 0 };
    data[97] = internal_compression;
    data[98] = tile_compression;
    data[99] = tile_type;
    data[100] = min_zoom;
    data[101] = max_zoom;
    data[102..106].copy_from_slice(&min_lon_e7.to_le_bytes());
    data[106..110].copy_from_slice(&min_lat_e7.to_le_bytes());
    data[110..114].copy_from_slice(&max_lon_e7.to_le_bytes());
    data[114..118].copy_from_slice(&max_lat_e7.to_le_bytes());
    data[118] = center_zoom;
    data[119..123].copy_from_slice(&center_lon_e7.to_le_bytes());
    data[123..127].copy_from_slice(&center_lat_e7.to_le_bytes());
    data
}

fn simple_header() -> Vec<u8> {
    make_pmtiles_header(
        127,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        false,
        1,
        1,
        2,
        0,
        14,
        -1_800_000_000i32,
        -900_000_000i32,
        1_800_000_000i32,
        900_000_000i32,
        5,
        0,
        0,
    )
}

/// Encode a slice of u64 values as LEB-128 varints.
fn encode_varint(v: u64) -> Vec<u8> {
    let mut out = Vec::new();
    let mut val = v;
    loop {
        let byte = (val & 0x7F) as u8;
        val >>= 7;
        if val == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
    out
}

/// Build a PMTiles directory with the given entries, using a trivial encoding
/// (no delta compression for simplicity — all offsets are absolute and deltas > 0).
fn build_directory(entries: &[(u64, u32, u32, u32)]) -> Vec<u8> {
    // entries: (tile_id, run_length, length, offset)
    let n = entries.len() as u64;
    let mut out = encode_varint(n);

    // tile_id deltas (cumulative)
    let mut last_id = 0u64;
    for &(tid, _, _, _) in entries {
        let delta = tid.saturating_sub(last_id);
        out.extend(encode_varint(delta));
        last_id = tid;
    }

    // run_lengths
    for &(_, rl, _, _) in entries {
        out.extend(encode_varint(rl as u64));
    }

    // lengths
    for &(_, _, len, _) in entries {
        out.extend(encode_varint(len as u64));
    }

    // offsets — first is absolute, subsequent are delta (use absolute value encoded as delta from last)
    let mut last_off = 0u64;
    for (i, &(_, _, prev_len, off)) in entries.iter().enumerate() {
        if i == 0 {
            out.extend(encode_varint(off as u64));
            last_off = off as u64;
        } else {
            let delta = (off as u64).saturating_sub(last_off);
            if delta == 0 {
                // Use 0 delta means clustered (prev_offset + prev_length)
                out.extend(encode_varint(0));
                last_off += prev_len as u64;
            } else {
                out.extend(encode_varint(delta));
                last_off += delta;
            }
        }
    }

    out
}

// ── PmTilesHeader — additional field coverage ─────────────────────────────────

#[test]
fn test_header_metadata_fields() {
    let data = make_pmtiles_header(
        128, 64, // root_dir
        192, 32, // metadata
        224, 16, // leaf_dirs
        240, 512, // tile_data
        1000, 800, 750, true, 2, 3, 1, 0, 18, 0, 0, 0, 0, 9, 0, 0,
    );
    let hdr = PmTilesHeader::parse(&data).expect("valid");
    assert_eq!(hdr.metadata_offset, 192);
    assert_eq!(hdr.metadata_length, 32);
    assert_eq!(hdr.leaf_dirs_offset, 224);
    assert_eq!(hdr.leaf_dirs_length, 16);
    assert_eq!(hdr.tile_data_offset, 240);
    assert_eq!(hdr.tile_data_length, 512);
}

#[test]
fn test_header_tile_counts() {
    let data = make_pmtiles_header(
        127, 0, 0, 0, 0, 0, 0, 0, 99999, 88888, 77777, false, 1, 1, 1, 0, 14, 0, 0, 0, 0, 5, 0, 0,
    );
    let hdr = PmTilesHeader::parse(&data).expect("valid");
    assert_eq!(hdr.addressed_tiles, 99999);
    assert_eq!(hdr.tile_entries, 88888);
    assert_eq!(hdr.tile_contents, 77777);
}

#[test]
fn test_header_clustered_true() {
    let data = make_pmtiles_header(
        127, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, true, 1, 1, 1, 0, 14, 0, 0, 0, 0, 5, 0, 0,
    );
    let hdr = PmTilesHeader::parse(&data).expect("valid");
    assert!(hdr.clustered);
}

#[test]
fn test_header_clustered_false() {
    let data = make_pmtiles_header(
        127, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, false, 1, 1, 1, 0, 14, 0, 0, 0, 0, 5, 0, 0,
    );
    let hdr = PmTilesHeader::parse(&data).expect("valid");
    assert!(!hdr.clustered);
}

#[test]
fn test_header_internal_compression_gzip() {
    let data = make_pmtiles_header(
        127, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, false, 2, 1, 1, 0, 14, 0, 0, 0, 0, 5, 0, 0,
    );
    let hdr = PmTilesHeader::parse(&data).expect("valid");
    assert_eq!(hdr.internal_compression, Compression::Gzip);
}

#[test]
fn test_header_tile_compression_brotli() {
    let data = make_pmtiles_header(
        127, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, false, 1, 3, 1, 0, 14, 0, 0, 0, 0, 5, 0, 0,
    );
    let hdr = PmTilesHeader::parse(&data).expect("valid");
    assert_eq!(hdr.tile_compression, Compression::Brotli);
}

#[test]
fn test_header_tile_compression_zstd() {
    let data = make_pmtiles_header(
        127, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, false, 1, 4, 1, 0, 14, 0, 0, 0, 0, 5, 0, 0,
    );
    let hdr = PmTilesHeader::parse(&data).expect("valid");
    assert_eq!(hdr.tile_compression, Compression::Zstd);
}

#[test]
fn test_header_tile_type_mvt() {
    let data = make_pmtiles_header(
        127, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, false, 1, 1, 1, 0, 14, 0, 0, 0, 0, 5, 0, 0,
    );
    let hdr = PmTilesHeader::parse(&data).expect("valid");
    assert_eq!(hdr.tile_type, TileType::Mvt);
}

#[test]
fn test_header_tile_type_avif() {
    let data = make_pmtiles_header(
        127, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, false, 1, 1, 5, 0, 14, 0, 0, 0, 0, 5, 0, 0,
    );
    let hdr = PmTilesHeader::parse(&data).expect("valid");
    assert_eq!(hdr.tile_type, TileType::Avif);
}

#[test]
fn test_header_zoom_range() {
    let data = make_pmtiles_header(
        127, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, false, 1, 1, 2, 3, 16, 0, 0, 0, 0, 9, 0, 0,
    );
    let hdr = PmTilesHeader::parse(&data).expect("valid");
    assert_eq!(hdr.min_zoom, 3);
    assert_eq!(hdr.max_zoom, 16);
    assert_eq!(hdr.center_zoom, 9);
}

#[test]
fn test_header_spec_version_is_3() {
    let data = simple_header();
    let hdr = PmTilesHeader::parse(&data).expect("valid");
    assert_eq!(hdr.spec_version, 3);
}

#[test]
fn test_header_version_1_error() {
    let mut data = simple_header();
    data[7] = 1;
    let err = PmTilesHeader::parse(&data).expect_err("unsupported version 1");
    assert!(matches!(err, PmTilesError::UnsupportedVersion(1)));
}

#[test]
fn test_header_version_0_error() {
    let mut data = simple_header();
    data[7] = 0;
    assert!(PmTilesHeader::parse(&data).is_err());
}

#[test]
fn test_header_partial_magic_error() {
    let mut data = simple_header();
    // Corrupt last byte of magic
    data[6] = b'X';
    assert!(PmTilesHeader::parse(&data).is_err());
}

#[test]
fn test_header_126_bytes_error() {
    let data = vec![0u8; 126];
    assert!(PmTilesHeader::parse(&data).is_err());
}

#[test]
fn test_header_exactly_127_bytes_ok() {
    let data = simple_header();
    assert_eq!(data.len(), 127);
    assert!(PmTilesHeader::parse(&data).is_ok());
}

// ── decode_varint — extended ──────────────────────────────────────────────────

#[test]
fn test_decode_varint_zero() {
    let (val, consumed) = decode_varint(&[0x00]).expect("ok");
    assert_eq!(val, 0);
    assert_eq!(consumed, 1);
}

#[test]
fn test_decode_varint_one() {
    let (val, consumed) = decode_varint(&[0x01]).expect("ok");
    assert_eq!(val, 1);
    assert_eq!(consumed, 1);
}

#[test]
fn test_decode_varint_127() {
    let (val, consumed) = decode_varint(&[0x7F]).expect("ok");
    assert_eq!(val, 127);
    assert_eq!(consumed, 1);
}

#[test]
fn test_decode_varint_128_two_bytes() {
    // 0x80 0x01 → 128
    let (val, consumed) = decode_varint(&[0x80, 0x01]).expect("ok");
    assert_eq!(val, 128);
    assert_eq!(consumed, 2);
}

#[test]
fn test_decode_varint_16383_two_bytes() {
    // 0xFF 0x7F → 16383
    let (val, consumed) = decode_varint(&[0xFF, 0x7F]).expect("ok");
    assert_eq!(val, 16383);
    assert_eq!(consumed, 2);
}

#[test]
fn test_decode_varint_16384_three_bytes() {
    // 16384 = 0x4000 → 0x80 0x80 0x01
    let (val, consumed) = decode_varint(&[0x80, 0x80, 0x01]).expect("ok");
    assert_eq!(val, 16384);
    assert_eq!(consumed, 3);
}

#[test]
fn test_decode_varint_max_u32() {
    // 4294967295 = 0xFFFFFFFF encoded in 5 bytes
    let bytes = encode_varint(u32::MAX as u64);
    let (val, _) = decode_varint(&bytes).expect("ok");
    assert_eq!(val, u32::MAX as u64);
}

#[test]
fn test_decode_varint_max_u64() {
    // u64::MAX — encodes in 10 bytes
    let bytes = encode_varint(u64::MAX);
    let (val, consumed) = decode_varint(&bytes).expect("ok");
    assert_eq!(val, u64::MAX);
    assert_eq!(consumed, 10);
}

#[test]
fn test_decode_varint_empty_slice_error() {
    assert!(decode_varint(&[]).is_err());
}

#[test]
fn test_decode_varint_reads_only_needed_bytes() {
    // Put varint 5 (0x05) followed by garbage
    let data = [0x05u8, 0xFF, 0xFF];
    let (val, consumed) = decode_varint(&data).expect("ok");
    assert_eq!(val, 5);
    assert_eq!(consumed, 1);
}

// ── DirectoryEntry ────────────────────────────────────────────────────────────

#[test]
fn test_directory_entry_is_tile_run_length_nonzero() {
    let e = DirectoryEntry {
        tile_id: 0,
        offset: 0,
        length: 100,
        run_length: 1,
    };
    assert!(e.is_tile());
    assert!(!e.is_leaf_directory());
}

#[test]
fn test_directory_entry_is_leaf_directory_run_length_zero() {
    let e = DirectoryEntry {
        tile_id: 0,
        offset: 1024,
        length: 512,
        run_length: 0,
    };
    assert!(e.is_leaf_directory());
    assert!(!e.is_tile());
}

// ── decode_directory — extended ───────────────────────────────────────────────

#[test]
fn test_decode_directory_single_entry() {
    // Encode directory with 1 entry: tile_id=0, run_length=1, length=512, offset=1024
    let dir = build_directory(&[(0, 1, 512, 1024)]);
    let entries = decode_directory(&dir).expect("ok");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].tile_id, 0);
    assert_eq!(entries[0].run_length, 1);
    assert_eq!(entries[0].length, 512);
    assert_eq!(entries[0].offset, 1024);
}

#[test]
fn test_decode_directory_multiple_entries() {
    let dir = build_directory(&[(0, 1, 100, 500), (1, 1, 200, 600), (2, 1, 300, 800)]);
    let entries = decode_directory(&dir).expect("ok");
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].tile_id, 0);
    assert_eq!(entries[1].tile_id, 1);
    assert_eq!(entries[2].tile_id, 2);
}

#[test]
fn test_decode_directory_tile_id_delta_encoding() {
    // Non-consecutive tile IDs
    let dir = build_directory(&[(0, 1, 100, 0), (10, 1, 100, 100), (100, 1, 100, 200)]);
    let entries = decode_directory(&dir).expect("ok");
    assert_eq!(entries[0].tile_id, 0);
    assert_eq!(entries[1].tile_id, 10);
    assert_eq!(entries[2].tile_id, 100);
}

#[test]
fn test_decode_directory_leaf_entry() {
    // run_length=0 means leaf directory
    let dir = build_directory(&[(0, 0, 4096, 2048)]);
    let entries = decode_directory(&dir).expect("ok");
    assert_eq!(entries.len(), 1);
    assert!(entries[0].is_leaf_directory());
}

// ── PmTilesReader ─────────────────────────────────────────────────────────────

#[test]
fn test_reader_from_bytes_valid() {
    let mut data = simple_header();
    data.resize(256, 0);
    let reader = PmTilesReader::from_bytes(data).expect("valid");
    assert_eq!(reader.header.spec_version, 3);
}

#[test]
fn test_reader_raw_root_directory_within_bounds() {
    // root_dir at offset 127, length 10
    let mut data = make_pmtiles_header(
        127, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, false, 1, 1, 2, 0, 14, 0, 0, 0, 0, 5, 0, 0,
    );
    data.resize(200, 0xAB);
    let reader = PmTilesReader::from_bytes(data).expect("valid");
    let raw = reader.raw_root_directory().expect("raw root dir ok");
    assert_eq!(raw.len(), 10);
}

#[test]
fn test_reader_raw_root_directory_out_of_bounds_error() {
    // root_dir_offset=127, root_dir_length=1000 but file is only 127 bytes
    let data = make_pmtiles_header(
        127, 1000, 0, 0, 0, 0, 0, 0, 0, 0, 0, false, 1, 1, 2, 0, 14, 0, 0, 0, 0, 5, 0, 0,
    );
    let reader = PmTilesReader::from_bytes(data).expect("header parses ok");
    assert!(reader.raw_root_directory().is_err());
}

#[test]
fn test_reader_root_directory_with_one_entry() {
    // Build header pointing to a real directory
    let dir_data = build_directory(&[(42, 1, 256, 8192)]);
    let dir_len = dir_data.len() as u64;

    let mut data = make_pmtiles_header(
        127, dir_len, 0, 0, 0, 0, 0, 0, 0, 0, 0, false, 1, 1, 2, 0, 14, 0, 0, 0, 0, 5, 0, 0,
    );
    data.extend_from_slice(&dir_data);

    let reader = PmTilesReader::from_bytes(data).expect("valid");
    let entries = reader.root_directory().expect("root dir ok");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].tile_id, 42);
    assert_eq!(entries[0].length, 256);
}

// ── PmTilesError ──────────────────────────────────────────────────────────────

#[test]
fn test_pmtiles_error_invalid_format_display() {
    let err = PmTilesError::InvalidFormat("test error".to_string());
    let msg = format!("{err}");
    assert!(msg.contains("test error"));
}

#[test]
fn test_pmtiles_error_unsupported_version_display() {
    let err = PmTilesError::UnsupportedVersion(2);
    let msg = format!("{err}");
    assert!(msg.contains('2'));
}

// ── Compression variants ──────────────────────────────────────────────────────

#[test]
fn test_compression_none_variant() {
    assert_eq!(Compression::from_u8(1), Compression::None);
}

#[test]
fn test_compression_unknown_high_value() {
    assert_eq!(Compression::from_u8(255), Compression::Unknown);
}
