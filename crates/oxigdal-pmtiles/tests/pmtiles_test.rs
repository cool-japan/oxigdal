//! Integration tests for oxigdal-pmtiles.

use oxigdal_pmtiles::{
    Compression, PmTilesHeader, PmTilesReader, TileType, decode_directory, decode_varint,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Build a minimal valid 127-byte PMTiles v3 header.
fn make_pmtiles_header(
    root_dir_offset: u64,
    root_dir_length: u64,
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
    data[7] = 3; // version
    data[8..16].copy_from_slice(&root_dir_offset.to_le_bytes());
    data[16..24].copy_from_slice(&root_dir_length.to_le_bytes());
    // metadata_offset / length at 24-39 → leave zero
    // leaf_dirs at 40-55 → zero
    // tile_data at 56-71 → zero
    // addressed_tiles at 72-79 → zero
    // tile_entries at 80-87 → zero
    // tile_contents at 88-95 → zero
    // clustered = 0
    data[96] = 0;
    // internal_compression = 1 (None)
    data[97] = 1;
    // tile_compression = 1 (None)
    data[98] = 1;
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

fn default_header() -> Vec<u8> {
    make_pmtiles_header(
        127,
        0,
        2, // Png
        0,
        14,
        -1_800_000_000i32, // -180.0
        -900_000_000i32,   // -90.0
        1_800_000_000i32,  // +180.0
        900_000_000i32,    // +90.0
        5,
        0,
        0,
    )
}

// ── Test 1: valid PMTiles magic → Ok ──────────────────────────────────────────

#[test]
fn test_pmtiles_magic_valid() {
    let data = default_header();
    assert!(PmTilesHeader::parse(&data).is_ok());
}

// ── Test 2: wrong magic → error ───────────────────────────────────────────────

#[test]
fn test_pmtiles_magic_invalid() {
    let mut data = default_header();
    data[0] = b'X';
    assert!(PmTilesHeader::parse(&data).is_err());
}

// ── Test 3: version != 3 → UnsupportedVersion ────────────────────────────────

#[test]
fn test_pmtiles_version_unsupported() {
    let mut data = default_header();
    data[7] = 2;
    let err = PmTilesHeader::parse(&data).expect_err("should fail for version 2");
    assert!(matches!(
        err,
        oxigdal_pmtiles::PmTilesError::UnsupportedVersion(2)
    ));
}

// ── Test 4: too short → error ─────────────────────────────────────────────────

#[test]
fn test_pmtiles_too_short() {
    let data = vec![0u8; 50];
    assert!(PmTilesHeader::parse(&data).is_err());
}

// ── Test 5: header fields parsed correctly ────────────────────────────────────

#[test]
fn test_pmtiles_header_fields() {
    let data = make_pmtiles_header(
        512,
        256,
        1,
        2,
        12,
        -1_000_000_000,
        -500_000_000,
        1_000_000_000,
        500_000_000,
        7,
        100_000_000,
        200_000_000,
    );
    let hdr = PmTilesHeader::parse(&data).expect("valid");
    assert_eq!(hdr.root_dir_offset, 512);
    assert_eq!(hdr.root_dir_length, 256);
    assert_eq!(hdr.min_zoom, 2);
    assert_eq!(hdr.max_zoom, 12);
    assert_eq!(hdr.center_zoom, 7);
}

// ── Test 6: min_lon / max_lon from e7 ────────────────────────────────────────

#[test]
fn test_pmtiles_lon_conversion() {
    let data = make_pmtiles_header(
        127,
        0,
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
    );
    let hdr = PmTilesHeader::parse(&data).expect("valid");
    assert!((hdr.min_lon() - (-180.0)).abs() < 1e-5, "min_lon");
    assert!((hdr.max_lon() - 180.0).abs() < 1e-5, "max_lon");
}

// ── Test 7: min_lat / max_lat from e7 ────────────────────────────────────────

#[test]
fn test_pmtiles_lat_conversion() {
    let data = make_pmtiles_header(
        127,
        0,
        2,
        0,
        14,
        0,
        -900_000_000i32,
        0,
        900_000_000i32,
        5,
        0,
        0,
    );
    let hdr = PmTilesHeader::parse(&data).expect("valid");
    assert!((hdr.min_lat() - (-90.0)).abs() < 1e-5, "min_lat");
    assert!((hdr.max_lat() - 90.0).abs() < 1e-5, "max_lat");
}

// ── Test 8: center_lon / center_lat from e7 ───────────────────────────────────

#[test]
fn test_pmtiles_center_conversion() {
    let data = make_pmtiles_header(
        127,
        0,
        2,
        0,
        14,
        0,
        0,
        0,
        0,
        5,
        139_691_700i32, // 13.96917° (Tokyo-ish)
        35_689_487i32,  // 3.5689487°
    );
    let hdr = PmTilesHeader::parse(&data).expect("valid");
    assert!((hdr.center_lon() - 13.96917).abs() < 1e-4, "center_lon");
    assert!((hdr.center_lat() - 3.5689487).abs() < 1e-5, "center_lat");
}

// ── Test 9: bounds() array ────────────────────────────────────────────────────

#[test]
fn test_pmtiles_bounds() {
    let data = make_pmtiles_header(
        127,
        0,
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
    );
    let hdr = PmTilesHeader::parse(&data).expect("valid");
    let b = hdr.bounds();
    assert!((b[0] - (-180.0)).abs() < 1e-5);
    assert!((b[1] - (-90.0)).abs() < 1e-5);
    assert!((b[2] - 180.0).abs() < 1e-5);
    assert!((b[3] - 90.0).abs() < 1e-5);
}

// ── Test 10: TileType::from_u8 all variants ───────────────────────────────────

#[test]
fn test_tile_type_from_u8() {
    assert_eq!(TileType::from_u8(0), TileType::Unknown);
    assert_eq!(TileType::from_u8(1), TileType::Mvt);
    assert_eq!(TileType::from_u8(2), TileType::Png);
    assert_eq!(TileType::from_u8(3), TileType::Jpeg);
    assert_eq!(TileType::from_u8(4), TileType::Webp);
    assert_eq!(TileType::from_u8(5), TileType::Avif);
    assert_eq!(TileType::from_u8(99), TileType::Unknown);
}

// ── Test 11: TileType mime_type ───────────────────────────────────────────────

#[test]
fn test_tile_type_mime_type() {
    assert_eq!(TileType::Mvt.mime_type(), "application/x-protobuf");
    assert_eq!(TileType::Png.mime_type(), "image/png");
    assert_eq!(TileType::Jpeg.mime_type(), "image/jpeg");
    assert_eq!(TileType::Webp.mime_type(), "image/webp");
    assert_eq!(TileType::Avif.mime_type(), "image/avif");
    assert_eq!(TileType::Unknown.mime_type(), "application/octet-stream");
}

// ── Test 12: TileType is_vector ───────────────────────────────────────────────

#[test]
fn test_tile_type_is_vector_raster() {
    assert!(TileType::Mvt.is_vector());
    assert!(!TileType::Png.is_vector());
    assert!(TileType::Png.is_raster());
    assert!(TileType::Jpeg.is_raster());
    assert!(TileType::Webp.is_raster());
    assert!(TileType::Avif.is_raster());
    assert!(!TileType::Mvt.is_raster());
}

// ── Test 13: Compression::from_u8 ────────────────────────────────────────────

#[test]
fn test_compression_from_u8() {
    assert_eq!(Compression::from_u8(0), Compression::Unknown);
    assert_eq!(Compression::from_u8(1), Compression::None);
    assert_eq!(Compression::from_u8(2), Compression::Gzip);
    assert_eq!(Compression::from_u8(3), Compression::Brotli);
    assert_eq!(Compression::from_u8(4), Compression::Zstd);
    assert_eq!(Compression::from_u8(5), Compression::Unknown);
}

// ── Test 14: decode_varint single byte ───────────────────────────────────────

#[test]
fn test_decode_varint_single_byte() {
    let (val, consumed) = decode_varint(&[0x05]).expect("ok");
    assert_eq!(val, 5);
    assert_eq!(consumed, 1);

    let (val, consumed) = decode_varint(&[0x00]).expect("ok");
    assert_eq!(val, 0);
    assert_eq!(consumed, 1);

    let (val, consumed) = decode_varint(&[0x7F]).expect("ok");
    assert_eq!(val, 127);
    assert_eq!(consumed, 1);
}

// ── Test 15: decode_varint multi-byte ────────────────────────────────────────

#[test]
fn test_decode_varint_multi_byte() {
    // 0x80 0x01 → 128 in two bytes
    let (val, consumed) = decode_varint(&[0x80, 0x01]).expect("ok");
    assert_eq!(val, 128);
    assert_eq!(consumed, 2);

    // 0xE5 0x8E 0x26 → 624485
    let (val, consumed) = decode_varint(&[0xE5, 0x8E, 0x26]).expect("ok");
    assert_eq!(val, 624_485);
    assert_eq!(consumed, 3);
}

// ── Test 16: decode_varint overflow → error ───────────────────────────────────

#[test]
fn test_decode_varint_truncated() {
    // All continuation bits set — truncated
    let data = [0x80u8; 5]; // 5 bytes all with continuation bit set, no terminal
    assert!(decode_varint(&data).is_err());
}

// ── Test 17: decode_directory empty data → Ok(empty) ─────────────────────────

#[test]
fn test_decode_directory_empty() {
    let entries = decode_directory(&[]).expect("empty ok");
    assert!(entries.is_empty());
}

// ── Extra: PmTilesReader with empty root directory ────────────────────────────

#[test]
fn test_pmtiles_reader_empty_root_dir() {
    // root_dir_offset=127, root_dir_length=0 → empty directory within the header bytes
    let mut data = default_header();
    // Extend with at least 127 bytes total (header already is 127)
    data.resize(200, 0);
    // Set root_dir_offset=127, root_dir_length=0
    data[8..16].copy_from_slice(&127u64.to_le_bytes());
    data[16..24].copy_from_slice(&0u64.to_le_bytes());

    let reader = PmTilesReader::from_bytes(data).expect("valid");
    let entries = reader.root_directory().expect("root dir ok");
    assert!(entries.is_empty());
}
