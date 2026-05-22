//! Integration tests for tile format auto-detection (magic byte sniffing).
//!
//! Tests numbered 1–10 per the W3/Slice-9 specification:
//!   1.  PNG magic bytes → `Png`
//!   2.  JPEG magic bytes → `Jpeg`
//!   3.  WebP RIFF+WEBP fingerprint → `Webp`
//!   4.  ISO BMFF `ftyp` box → `Avif`
//!   5.  GZip 2-byte ID → `Gzip`
//!   6.  Zstd 4-byte magic → `Zstd`
//!   7.  Protobuf first-byte heuristic → `Mvt`
//!   8.  Empty slice → `Unknown`
//!   9.  MIME types for every variant
//!  10.  `as_tile_type` round-trips to `TileType`

#![allow(clippy::unwrap_used, missing_docs)]

use oxigdal_pmtiles::{
    DetectedTileFormat, PmTilesBuilder, PmTilesReader, TileType, detect_tile_format,
};

// ── Test 1: PNG magic ─────────────────────────────────────────────────────────

/// 8-byte PNG signature must be recognised immediately.
#[test]
fn test_detect_png_magic() {
    // Full PNG magic: \x89PNG\r\n\x1a\n  followed by padding.
    let mut data = vec![0u8; 20];
    data[0..8].copy_from_slice(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
    assert_eq!(
        detect_tile_format(&data),
        DetectedTileFormat::Png,
        "PNG magic should be detected"
    );
}

/// Exactly 8 bytes — minimum input for PNG detection.
#[test]
fn test_detect_png_magic_exact_length() {
    let data: Vec<u8> = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
    assert_eq!(detect_tile_format(&data), DetectedTileFormat::Png);
}

// ── Test 2: JPEG magic ────────────────────────────────────────────────────────

/// SOI marker `\xff\xd8\xff` must be detected as JPEG.
#[test]
fn test_detect_jpeg_magic() {
    // Typical JFIF opening: FF D8 FF E0 …
    let data: Vec<u8> = vec![0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, 0x4a, 0x46];
    assert_eq!(
        detect_tile_format(&data),
        DetectedTileFormat::Jpeg,
        "JPEG SOI prefix must be detected"
    );
}

/// EXIF JPEG variant (`\xff\xd8\xff\xe1`).
#[test]
fn test_detect_jpeg_exif_variant() {
    let data: Vec<u8> = vec![0xff, 0xd8, 0xff, 0xe1, 0x00, 0x18, 0x45, 0x78];
    assert_eq!(detect_tile_format(&data), DetectedTileFormat::Jpeg);
}

// ── Test 3: WebP magic ────────────────────────────────────────────────────────

/// `RIFF` at [0..4] and `WEBP` at [8..12] must yield `Webp`.
#[test]
fn test_detect_webp_magic() {
    let mut data = vec![0u8; 30];
    data[0..4].copy_from_slice(b"RIFF");
    data[4..8].copy_from_slice(&26u32.to_le_bytes()); // file size field
    data[8..12].copy_from_slice(b"WEBP");
    assert_eq!(
        detect_tile_format(&data),
        DetectedTileFormat::Webp,
        "RIFF+WEBP fingerprint must be detected as WebP"
    );
}

/// A RIFF file that is NOT WebP (e.g. WAV) must not be detected as WebP.
#[test]
fn test_detect_riff_non_webp_is_unknown_or_mvt() {
    let mut data = vec![0u8; 30];
    data[0..4].copy_from_slice(b"RIFF");
    data[4..8].copy_from_slice(&26u32.to_le_bytes());
    data[8..12].copy_from_slice(b"WAVE"); // not WebP
    // Must NOT be WebP — the exact result depends on the MVT heuristic.
    let fmt = detect_tile_format(&data);
    assert_ne!(fmt, DetectedTileFormat::Webp);
}

// ── Test 4: AVIF / ISO BMFF ftyp box ─────────────────────────────────────────

/// ISO BMFF `ftyp` box (bytes 4–7 == `ftyp`) must be classified as `Avif`.
#[test]
fn test_detect_avif_ftyp_box() {
    // Box layout: [4-byte size (BE)][4-byte type "ftyp"][4-byte brand "avif"]
    let mut data = vec![0u8; 20];
    data[0..4].copy_from_slice(&20u32.to_be_bytes()); // box size
    data[4..8].copy_from_slice(b"ftyp");
    data[8..12].copy_from_slice(b"avif"); // major brand
    assert_eq!(
        detect_tile_format(&data),
        DetectedTileFormat::Avif,
        "`ftyp` box at offset 4 must be detected as Avif/HEIF"
    );
}

/// HEIC brand also uses the same `ftyp` box — must resolve to `Avif`.
#[test]
fn test_detect_heic_ftyp_box() {
    let mut data = vec![0u8; 20];
    data[0..4].copy_from_slice(&20u32.to_be_bytes());
    data[4..8].copy_from_slice(b"ftyp");
    data[8..12].copy_from_slice(b"heic");
    assert_eq!(detect_tile_format(&data), DetectedTileFormat::Avif);
}

// ── Test 5: GZip magic ────────────────────────────────────────────────────────

/// GZip 2-byte identifier `\x1f\x8b` must be detected as `Gzip`.
#[test]
fn test_detect_gzip_magic() {
    // Typical gzip opening: ID1 ID2 CM …
    let data: Vec<u8> = vec![0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00];
    assert_eq!(
        detect_tile_format(&data),
        DetectedTileFormat::Gzip,
        "GZip magic must be detected"
    );
}

/// Minimum gzip input — just the 2-byte magic.
#[test]
fn test_detect_gzip_minimal() {
    let data: Vec<u8> = vec![0x1f, 0x8b];
    assert_eq!(detect_tile_format(&data), DetectedTileFormat::Gzip);
}

// ── Test 6: Zstd magic ────────────────────────────────────────────────────────

/// Zstandard 4-byte magic `\x28\xb5\x2f\xfd` must be detected as `Zstd`.
#[test]
fn test_detect_zstd_magic() {
    let data: Vec<u8> = vec![0x28, 0xb5, 0x2f, 0xfd, 0x04, 0x00, 0x11, 0x00];
    assert_eq!(
        detect_tile_format(&data),
        DetectedTileFormat::Zstd,
        "Zstd magic must be detected"
    );
}

/// Minimum zstd input — exactly 4 magic bytes.
#[test]
fn test_detect_zstd_minimal() {
    let data: Vec<u8> = vec![0x28, 0xb5, 0x2f, 0xfd];
    assert_eq!(detect_tile_format(&data), DetectedTileFormat::Zstd);
}

// ── Test 7: MVT protobuf heuristic ───────────────────────────────────────────

/// Byte `0x1a` encodes protobuf field=3 (MVT "layers"), wire type=2.
/// This is the canonical first tag of a real MVT tile.
#[test]
fn test_detect_likely_mvt_valid_protobuf_tag() {
    // 0x1a = (field=3 << 3) | wire_type=2
    let data: Vec<u8> = vec![0x1a, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    assert_eq!(
        detect_tile_format(&data),
        DetectedTileFormat::Mvt,
        "Protobuf tag 0x1a (field=3, wire=2) must be heuristically classified as MVT"
    );
}

/// Field=1, wire_type=2 (0x0a) is also a valid first protobuf tag.
#[test]
fn test_detect_mvt_field1_wire2() {
    // 0x0a = (field=1 << 3) | wire_type=2
    let data: Vec<u8> = vec![0x0a, 0x05, 0x68, 0x65, 0x6c, 0x6c, 0x6f];
    assert_eq!(detect_tile_format(&data), DetectedTileFormat::Mvt);
}

/// Wire type 3 (group start, deprecated) must NOT be classified as MVT.
#[test]
fn test_detect_mvt_invalid_wire_type_3_not_mvt() {
    // wire_type = 3 is deprecated and must fail the heuristic.
    // 0x0b = (field=1 << 3) | 3
    let data: Vec<u8> = vec![0x0b, 0x00, 0x00, 0x00, 0x00, 0x00];
    let fmt = detect_tile_format(&data);
    assert_ne!(
        fmt,
        DetectedTileFormat::Mvt,
        "Wire type 3 must not be classified as MVT"
    );
}

/// Wire type 4 (group end, deprecated) must NOT be classified as MVT.
#[test]
fn test_detect_mvt_invalid_wire_type_4_not_mvt() {
    // 0x0c = (field=1 << 3) | 4
    let data: Vec<u8> = vec![0x0c, 0x00, 0x00, 0x00, 0x00, 0x00];
    let fmt = detect_tile_format(&data);
    assert_ne!(fmt, DetectedTileFormat::Mvt);
}

/// Field number 0 is invalid in protobuf — must not be classified as MVT.
#[test]
fn test_detect_mvt_field_number_zero_not_mvt() {
    // 0x02 = (field=0 << 3) | wire_type=2  →  invalid field number
    let data: Vec<u8> = vec![0x02, 0x00, 0x00, 0x00];
    let fmt = detect_tile_format(&data);
    assert_ne!(fmt, DetectedTileFormat::Mvt);
}

// ── Test 8: empty slice → Unknown ────────────────────────────────────────────

/// An empty byte slice must always return `Unknown`.
#[test]
fn test_detect_empty_returns_unknown() {
    assert_eq!(
        detect_tile_format(&[]),
        DetectedTileFormat::Unknown,
        "Empty slice must return Unknown"
    );
}

/// A single-byte slice that does not match any known prefix must return
/// `Unknown` (unless it could pass the MVT heuristic, which requires ≥ 2 bytes).
#[test]
fn test_detect_single_byte_unknown() {
    // A single byte cannot match PNG (8), JPEG (3), WebP (12), AVIF (8),
    // GZip (2), Zstd (4), or MVT (requires ≥ 2 bytes).
    let data: Vec<u8> = vec![0x1a]; // would be MVT if ≥ 2 bytes
    assert_eq!(detect_tile_format(&data), DetectedTileFormat::Unknown);
}

// ── Test 9: MIME types ────────────────────────────────────────────────────────

/// Verify `mime_type()` for every `DetectedTileFormat` variant.
#[test]
fn test_detect_format_mime_types() {
    assert_eq!(DetectedTileFormat::Png.mime_type(), "image/png");
    assert_eq!(DetectedTileFormat::Jpeg.mime_type(), "image/jpeg");
    assert_eq!(DetectedTileFormat::Webp.mime_type(), "image/webp");
    assert_eq!(DetectedTileFormat::Avif.mime_type(), "image/avif");
    assert_eq!(
        DetectedTileFormat::Mvt.mime_type(),
        "application/vnd.mapbox-vector-tile"
    );
    assert_eq!(DetectedTileFormat::Gzip.mime_type(), "application/gzip");
    assert_eq!(DetectedTileFormat::Zstd.mime_type(), "application/zstd");
    assert_eq!(
        DetectedTileFormat::Unknown.mime_type(),
        "application/octet-stream"
    );
}

// ── Test 10: as_tile_type round-trips ────────────────────────────────────────

/// `as_tile_type()` must map each image/vector format to the corresponding
/// `TileType`, and compression/unknown variants must return `None`.
#[test]
fn test_as_tile_type_converts() {
    assert_eq!(
        DetectedTileFormat::Png.as_tile_type(),
        Some(TileType::Png),
        "Png → TileType::Png"
    );
    assert_eq!(
        DetectedTileFormat::Jpeg.as_tile_type(),
        Some(TileType::Jpeg),
        "Jpeg → TileType::Jpeg"
    );
    assert_eq!(
        DetectedTileFormat::Webp.as_tile_type(),
        Some(TileType::Webp),
        "Webp → TileType::Webp"
    );
    assert_eq!(
        DetectedTileFormat::Avif.as_tile_type(),
        Some(TileType::Avif),
        "Avif → TileType::Avif"
    );
    assert_eq!(
        DetectedTileFormat::Mvt.as_tile_type(),
        Some(TileType::Mvt),
        "Mvt → TileType::Mvt"
    );
    assert_eq!(
        DetectedTileFormat::Gzip.as_tile_type(),
        None,
        "Gzip is not a tile type → None"
    );
    assert_eq!(
        DetectedTileFormat::Zstd.as_tile_type(),
        None,
        "Zstd is not a tile type → None"
    );
    assert_eq!(
        DetectedTileFormat::Unknown.as_tile_type(),
        None,
        "Unknown → None"
    );
}

// ── is_raster / is_vector / is_compressed predicates ─────────────────────────

#[test]
fn test_format_predicates() {
    assert!(DetectedTileFormat::Png.is_raster());
    assert!(DetectedTileFormat::Jpeg.is_raster());
    assert!(DetectedTileFormat::Webp.is_raster());
    assert!(DetectedTileFormat::Avif.is_raster());
    assert!(!DetectedTileFormat::Mvt.is_raster());
    assert!(!DetectedTileFormat::Gzip.is_raster());

    assert!(DetectedTileFormat::Mvt.is_vector());
    assert!(!DetectedTileFormat::Png.is_vector());
    assert!(!DetectedTileFormat::Gzip.is_vector());

    assert!(DetectedTileFormat::Gzip.is_compressed());
    assert!(DetectedTileFormat::Zstd.is_compressed());
    assert!(!DetectedTileFormat::Png.is_compressed());
    assert!(!DetectedTileFormat::Mvt.is_compressed());
    assert!(!DetectedTileFormat::Unknown.is_compressed());
}

// ── PmTilesReader::detect_tile_format and detect_dominant_format ──────────────

/// Build a minimal archive containing one PNG-magic tile and verify that
/// `detect_tile_format` returns `Some(DetectedTileFormat::Png)`.
#[test]
fn test_reader_detect_tile_format_png() {
    // PNG tile payload: real magic + padding.
    let mut tile = vec![0u8; 64];
    tile[0..8].copy_from_slice(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);

    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 0);
    builder.add_tile(0, 0, 0, &tile).unwrap();
    let archive_bytes = builder.build().unwrap();

    let reader = PmTilesReader::from_bytes(archive_bytes).unwrap();
    let detected = reader.detect_tile_format(0, 0, 0).unwrap();
    assert_eq!(detected, Some(DetectedTileFormat::Png));
}

/// A tile coordinate that does not exist in the archive must return `None`.
#[test]
fn test_reader_detect_tile_format_missing_tile_returns_none() {
    let builder = PmTilesBuilder::new(TileType::Png, 0, 14);
    let archive_bytes = builder.build().unwrap();

    let reader = PmTilesReader::from_bytes(archive_bytes).unwrap();
    // Tile (0, 0, 0) was never added.
    let detected = reader.detect_tile_format(0, 0, 0).unwrap();
    assert_eq!(detected, None);
}

/// An archive with no tiles must report `Unknown` from `detect_dominant_format`.
#[test]
fn test_reader_detect_dominant_format_empty_archive_returns_unknown() {
    let builder = PmTilesBuilder::new(TileType::Png, 0, 14);
    let archive_bytes = builder.build().unwrap();

    let reader = PmTilesReader::from_bytes(archive_bytes).unwrap();
    let dominant = reader.detect_dominant_format(100).unwrap();
    assert_eq!(dominant, DetectedTileFormat::Unknown);
}

/// An archive filled with PNG-magic tiles must report `Png` as dominant.
#[test]
fn test_reader_detect_dominant_format_all_png() {
    let mut png_tile = vec![0u8; 32];
    png_tile[0..8].copy_from_slice(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);

    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 1);
    // Add tiles at z=0 and z=1.
    builder.add_tile(0, 0, 0, &png_tile).unwrap();
    builder.add_tile(1, 0, 0, &png_tile).unwrap();
    builder.add_tile(1, 1, 0, &png_tile).unwrap();
    builder.add_tile(1, 0, 1, &png_tile).unwrap();
    let archive_bytes = builder.build().unwrap();

    let reader = PmTilesReader::from_bytes(archive_bytes).unwrap();
    let dominant = reader.detect_dominant_format(10).unwrap();
    assert_eq!(dominant, DetectedTileFormat::Png);
}

/// Mixed archive: majority PNG with one JPEG — PNG must win.
#[test]
fn test_reader_detect_dominant_format_majority_wins() {
    let mut png_tile = vec![0u8; 32];
    png_tile[0..8].copy_from_slice(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);

    let mut jpeg_tile = vec![0u8; 32];
    jpeg_tile[0..3].copy_from_slice(&[0xff, 0xd8, 0xff]);

    let mut builder = PmTilesBuilder::new(TileType::Unknown, 0, 1);
    builder.add_tile(0, 0, 0, &png_tile).unwrap();
    builder.add_tile(1, 0, 0, &png_tile).unwrap();
    builder.add_tile(1, 1, 0, &png_tile).unwrap();
    builder.add_tile(1, 0, 1, &jpeg_tile).unwrap(); // minority
    let archive_bytes = builder.build().unwrap();

    let reader = PmTilesReader::from_bytes(archive_bytes).unwrap();
    let dominant = reader.detect_dominant_format(10).unwrap();
    assert_eq!(dominant, DetectedTileFormat::Png);
}
