//! Integration tests for PMTiles archive transcoding.
//!
//! Every test in this file is gated behind the `compression` cargo feature
//! because the transcode API is itself gated on it.

#![cfg(feature = "compression")]
#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

use oxigeo_pmtiles::{
    Compression, PmTilesBuilder, PmTilesHeader, PmTilesReader, TileType, TranscodeOptions,
    TranscodeStats, transcode_archive, transcode_archive_with_stats, transcode_tile,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a PMTiles archive whose `tile_compression` byte is `comp` and whose
/// tile payloads are pre-compressed in that algorithm.
///
/// `tiles_raw` supplies the uncompressed tile payloads; this helper handles
/// running them through the appropriate codec before adding to the builder.
fn build_compressed_archive(
    tiles_raw: &[(u8, u32, u32, &[u8])],
    comp: Compression,
    metadata_json: Option<&str>,
) -> Vec<u8> {
    let min_z = tiles_raw.iter().map(|t| t.0).min().unwrap_or(0);
    let max_z = tiles_raw.iter().map(|t| t.0).max().unwrap_or(0);

    let mut builder = PmTilesBuilder::new(TileType::Mvt, min_z, max_z);
    builder.set_tile_compression(comp.clone());
    if let Some(json) = metadata_json {
        builder.set_metadata(json.to_string());
    }

    for &(z, x, y, payload) in tiles_raw {
        let compressed = match comp {
            Compression::None | Compression::Unknown => payload.to_vec(),
            Compression::Gzip => oxiarc_archive::gzip::compress(payload, 6).expect("gzip compress"),
            Compression::Brotli => {
                oxiarc_archive::brotli::compress(payload).expect("brotli compress")
            }
            Compression::Zstd => oxiarc_archive::zstd::compress(payload).expect("zstd compress"),
        };
        builder.add_tile(z, x, y, &compressed).expect("add_tile");
    }

    builder.build().expect("build")
}

/// Manually decompress a tile payload using the given algorithm.
fn decompress(payload: &[u8], comp: Compression) -> Vec<u8> {
    match comp {
        Compression::None | Compression::Unknown => payload.to_vec(),
        Compression::Gzip => {
            let mut reader = std::io::Cursor::new(payload);
            oxiarc_archive::gzip::decompress(&mut reader).expect("gzip decompress")
        }
        Compression::Brotli => {
            oxiarc_archive::brotli::decompress(payload).expect("brotli decompress")
        }
        Compression::Zstd => oxiarc_archive::zstd::decompress(payload).expect("zstd decompress"),
    }
}

// ---------------------------------------------------------------------------
// Test 1 — transcode_tile identity passthrough returns identical bytes
// ---------------------------------------------------------------------------

#[test]
fn test_transcode_tile_identity_passthrough() {
    let raw = b"identity passthrough payload bytes";

    // Identity for every codec slot, including None.
    for comp in [
        Compression::None,
        Compression::Gzip,
        Compression::Brotli,
        Compression::Zstd,
    ] {
        let out = transcode_tile(raw, comp.clone(), comp, None).expect("transcode");
        assert_eq!(out.as_slice(), raw, "identity must be a verbatim copy");
    }
}

// ---------------------------------------------------------------------------
// Test 2 — gzip ⇒ zstd round trip recovers original payload
// ---------------------------------------------------------------------------

#[test]
fn test_transcode_tile_gzip_to_zstd_round_trip() {
    let raw = b"gzip-to-zstd round trip payload that should survive the journey";
    let gzip_payload = oxiarc_archive::gzip::compress(raw, 6).expect("gzip compress");

    let zstd_payload = transcode_tile(&gzip_payload, Compression::Gzip, Compression::Zstd, None)
        .expect("transcode");

    // Output must be valid zstd (i.e. decompress back to the original).
    let decoded = oxiarc_archive::zstd::decompress(&zstd_payload).expect("zstd decompress");
    assert_eq!(
        decoded.as_slice(),
        raw,
        "zstd round-trip must preserve bytes"
    );
}

// ---------------------------------------------------------------------------
// Test 3 — brotli ⇒ gzip round trip recovers original payload
// ---------------------------------------------------------------------------

#[test]
fn test_transcode_tile_brotli_to_gzip_round_trip() {
    let raw = b"brotli-to-gzip round trip payload -- verifies the cross-codec path";
    let brotli_payload = oxiarc_archive::brotli::compress(raw).expect("brotli compress");

    let gzip_payload = transcode_tile(
        &brotli_payload,
        Compression::Brotli,
        Compression::Gzip,
        Some(9),
    )
    .expect("transcode");

    let mut reader = std::io::Cursor::new(&gzip_payload);
    let decoded = oxiarc_archive::gzip::decompress(&mut reader).expect("gzip decompress");
    assert_eq!(decoded.as_slice(), raw);
}

// ---------------------------------------------------------------------------
// Test 4 — empty archive transcodes to empty archive
// ---------------------------------------------------------------------------

#[test]
fn test_transcode_archive_empty() {
    // Build an empty gzip-flagged archive.
    let mut builder = PmTilesBuilder::new(TileType::Mvt, 0, 0);
    builder.set_tile_compression(Compression::Gzip);
    let source = builder.build().expect("build");

    let opts = TranscodeOptions {
        from: Compression::Gzip,
        to: Compression::Zstd,
        level: None,
    };
    let (out, stats) = transcode_archive_with_stats(&source, &opts).expect("transcode");

    let header = PmTilesHeader::parse(&out).expect("parse");
    assert_eq!(header.addressed_tiles, 0);
    assert_eq!(header.tile_entries, 0);
    assert_eq!(header.tile_contents, 0);
    assert_eq!(header.tile_compression, Compression::Zstd);
    assert_eq!(stats.tiles_transcoded, 0);
    assert_eq!(stats.tiles_skipped_identity, 0);
    assert_eq!(stats.bytes_before, 0);
    assert_eq!(stats.bytes_after, 0);
}

// ---------------------------------------------------------------------------
// Test 5 — single-tile gzip ⇒ zstd archive: tile content & header survive
// ---------------------------------------------------------------------------

#[test]
fn test_transcode_archive_single_tile_changes_compression() {
    let raw_tile: &[u8] =
        b"single tile body that is long enough that compression actually does something useful here";
    let source = build_compressed_archive(&[(0, 0, 0, raw_tile)], Compression::Gzip, None);

    // Sanity: source header advertises gzip.
    let src_header = PmTilesHeader::parse(&source).expect("src parse");
    assert_eq!(src_header.tile_compression, Compression::Gzip);

    let opts = TranscodeOptions {
        from: Compression::Unknown, // auto-detect from header
        to: Compression::Zstd,
        level: None,
    };
    let out = transcode_archive(&source, &opts).expect("transcode");

    let dst_header = PmTilesHeader::parse(&out).expect("dst parse");
    assert_eq!(dst_header.tile_compression, Compression::Zstd);
    assert_eq!(dst_header.addressed_tiles, 1);

    // Read the tile back, decompress with zstd, compare to the original raw bytes.
    let reader = PmTilesReader::from_bytes(out).expect("reader");
    let raw_out = reader
        .get_tile(0, 0, 0)
        .expect("get_tile")
        .expect("tile must exist");
    let decoded = decompress(&raw_out, Compression::Zstd);
    assert_eq!(
        decoded.as_slice(),
        raw_tile,
        "tile content must survive transcode"
    );
}

// ---------------------------------------------------------------------------
// Test 6 — output header records the requested target compression
// ---------------------------------------------------------------------------

#[test]
fn test_transcode_archive_header_records_new_compression() {
    let raw_tiles: &[(u8, u32, u32, &[u8])] = &[
        (0, 0, 0, b"z0 tile body for header-compression test"),
        (1, 0, 0, b"z1 tile body that is different from the z0 one"),
    ];
    let source = build_compressed_archive(raw_tiles, Compression::Gzip, None);

    for target in [Compression::Brotli, Compression::Zstd, Compression::Gzip] {
        let opts = TranscodeOptions {
            from: Compression::Gzip,
            to: target.clone(),
            level: None,
        };
        let out = transcode_archive(&source, &opts).expect("transcode");
        let header = PmTilesHeader::parse(&out).expect("parse");
        assert_eq!(
            header.tile_compression, target,
            "header must report target={target:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Test 7 — byte counts in stats reflect actual per-tile sizes
// ---------------------------------------------------------------------------

#[test]
fn test_transcode_archive_stats_byte_counts() {
    let raw_tiles: &[(u8, u32, u32, &[u8])] = &[
        (0, 0, 0, &[0xAAu8; 256]),
        (1, 0, 0, &[0xBBu8; 512]),
        (1, 1, 0, &[0xCCu8; 1024]),
    ];
    let source = build_compressed_archive(raw_tiles, Compression::Gzip, None);

    // Pull the source tile-data section to compute the expected `bytes_before`.
    let reader = PmTilesReader::from_bytes(source.clone()).expect("reader");
    let infos = reader.enumerate_tiles().expect("enumerate");
    let expected_before: u64 = infos.iter().map(|i| u64::from(i.data_length)).sum();

    let opts = TranscodeOptions {
        from: Compression::Gzip,
        to: Compression::Zstd,
        level: None,
    };
    let (out, stats) = transcode_archive_with_stats(&source, &opts).expect("transcode");

    assert_eq!(stats.bytes_before, expected_before);
    assert_eq!(stats.tiles_transcoded, raw_tiles.len() as u64);
    assert_eq!(stats.tiles_skipped_identity, 0);

    // The `bytes_after` should equal the sum of zstd-compressed payload sizes
    // for the unique tile contents.  Easiest check: read each tile from the
    // output and sum its data_length.
    let out_reader = PmTilesReader::from_bytes(out).expect("out reader");
    let out_infos = out_reader.enumerate_tiles().expect("enumerate");
    let expected_after: u64 = out_infos.iter().map(|i| u64::from(i.data_length)).sum();
    assert_eq!(stats.bytes_after, expected_after);

    // Ratio sanity.
    assert!(stats.ratio() > 0.0 && stats.ratio().is_finite());
}

// ---------------------------------------------------------------------------
// Test 8 — metadata (name / description) survives the round trip
// ---------------------------------------------------------------------------

#[test]
fn test_transcode_archive_metadata_preserved() {
    let raw_tile: &[u8] = b"any tile";
    let metadata_json =
        r#"{"name":"my-tileset","description":"Transcode preservation test","extra_key":"hi"}"#;
    let source = build_compressed_archive(
        &[(0, 0, 0, raw_tile)],
        Compression::Gzip,
        Some(metadata_json),
    );

    let opts = TranscodeOptions {
        from: Compression::Gzip,
        to: Compression::Brotli,
        level: None,
    };
    let out = transcode_archive(&source, &opts).expect("transcode");
    let reader = PmTilesReader::from_bytes(out).expect("reader");
    let metadata = reader.metadata().expect("metadata");

    assert_eq!(metadata.name.as_deref(), Some("my-tileset"));
    assert_eq!(
        metadata.description.as_deref(),
        Some("Transcode preservation test")
    );
    assert_eq!(
        metadata.extra.get("extra_key").and_then(|v| v.as_str()),
        Some("hi"),
        "unknown metadata keys must survive via the extra map"
    );
}

// ---------------------------------------------------------------------------
// Test 9 — passing Compression::Unknown as target errors
// ---------------------------------------------------------------------------

#[test]
fn test_transcode_unsupported_format_errors() {
    let raw_tile: &[u8] = b"x";
    let source = build_compressed_archive(&[(0, 0, 0, raw_tile)], Compression::Gzip, None);

    let opts = TranscodeOptions {
        from: Compression::Gzip,
        to: Compression::Unknown,
        level: None,
    };
    let err = transcode_archive(&source, &opts).expect_err("Unknown target must error");
    assert!(matches!(
        err,
        oxigeo_pmtiles::PmTilesError::UnsupportedCompression
    ));
}

// ---------------------------------------------------------------------------
// Test 10 — default options where source and target are identical round-trip
// ---------------------------------------------------------------------------

#[test]
fn test_transcode_archive_default_options_round_trips_when_same_format() {
    // Build a gzip-compressed source.
    let raw_tile: &[u8] = b"default-options identity round trip body bytes here";
    let source = build_compressed_archive(&[(0, 0, 0, raw_tile)], Compression::Gzip, None);

    // Default options: from=Unknown (auto-detect ⇒ Gzip), to=Gzip ⇒ identity.
    let opts = TranscodeOptions::default();
    let (out, stats) = transcode_archive_with_stats(&source, &opts).expect("transcode");

    // The output header still advertises gzip.
    let header = PmTilesHeader::parse(&out).expect("parse");
    assert_eq!(header.tile_compression, Compression::Gzip);

    // Identity path was taken — counted under tiles_skipped_identity.
    assert_eq!(stats.tiles_skipped_identity, 1);
    assert_eq!(stats.tiles_transcoded, 0);

    // The tile content round-trips byte-for-byte (still gzip-encoded).
    let reader = PmTilesReader::from_bytes(out).expect("reader");
    let raw_out = reader
        .get_tile(0, 0, 0)
        .expect("get_tile")
        .expect("tile must exist");
    let decoded = decompress(&raw_out, Compression::Gzip);
    assert_eq!(decoded.as_slice(), raw_tile);
}

// ---------------------------------------------------------------------------
// Bonus — TranscodeStats::ratio() is sensible at the boundaries
// ---------------------------------------------------------------------------

#[test]
fn test_transcode_stats_ratio_boundary() {
    let s = TranscodeStats::default();
    assert!((s.ratio() - 1.0).abs() < f64::EPSILON);

    let s = TranscodeStats {
        tiles_transcoded: 1,
        tiles_skipped_identity: 0,
        bytes_before: 200,
        bytes_after: 50,
    };
    assert!((s.ratio() - 0.25).abs() < f64::EPSILON);
}
