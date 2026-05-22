//! Integration tests for PMTiles sub-region extraction.
//!
//! Tests exercise the public API in `oxigdal_pmtiles::extract`:
//! - [`bbox_to_tile_range`]: lon/lat bbox → tile grid range
//! - [`extract_subregion`]: copy tiles within a bbox into a new archive

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

use oxigdal_pmtiles::webmerc::tile_bounds_lonlat;
use oxigdal_pmtiles::{
    ExtractOptions, PmTilesBuilder, PmTilesHeader, PmTilesReader, TileType, bbox_to_tile_range,
    extract_subregion,
};

// ---------------------------------------------------------------------------
// Test helper
// ---------------------------------------------------------------------------

/// Build a minimal valid PMTiles v3 archive from the given set of tiles.
///
/// Zoom range is derived from the tile list; falls back to `(0, 0)` for an
/// empty list so that the archive header is still valid.
fn build_archive(tiles: &[(u8, u32, u32, &[u8])]) -> Vec<u8> {
    let min_z = tiles.iter().map(|t| t.0).min().unwrap_or(0);
    let max_z = tiles.iter().map(|t| t.0).max().unwrap_or(0);
    let mut builder = PmTilesBuilder::new(TileType::Png, min_z, max_z);
    for &(z, x, y, data) in tiles {
        builder.add_tile(z, x, y, data).expect("add_tile ok");
    }
    builder.build().expect("build ok")
}

/// Assert that `bytes` begins with a valid PMTiles v3 header.
fn assert_valid_pmtiles(bytes: &[u8]) {
    assert!(
        bytes.len() >= 127,
        "archive too short: {} bytes",
        bytes.len()
    );
    assert_eq!(&bytes[0..7], b"PMTiles", "magic mismatch");
    assert_eq!(bytes[7], 3, "spec version must be 3");
}

// ---------------------------------------------------------------------------
// Test 1 — full world bbox at z=0 → single tile (0, 0, 0, 0)
// ---------------------------------------------------------------------------

#[test]
fn test_bbox_to_tile_range_full_world_z0() {
    // The entire world maps to the single z=0 tile.
    let (min_x, min_y, max_x, max_y) =
        bbox_to_tile_range(0, -180.0, -85.05, 180.0, 85.05).expect("ok");
    assert_eq!(
        (min_x, min_y, max_x, max_y),
        (0, 0, 0, 0),
        "z=0 must always be a single tile"
    );
}

// ---------------------------------------------------------------------------
// Test 2 — bbox exactly covering one tile at z=2 → returns exactly 1 tile
// ---------------------------------------------------------------------------

#[test]
fn test_bbox_to_tile_range_single_tile_z2() {
    // Obtain the bounds of a known tile (z=2, x=1, y=2) which is in the SW
    // quadrant of the western hemisphere.
    let (tmin_lon, tmin_lat, tmax_lon, tmax_lat) = tile_bounds_lonlat(2, 1, 2);

    // Shrink the bbox slightly inward so we don't pick up neighbours due to
    // floating-point edge effects.
    let eps = 0.001_f64;
    let (min_x, min_y, max_x, max_y) = bbox_to_tile_range(
        2,
        tmin_lon + eps,
        tmin_lat + eps,
        tmax_lon - eps,
        tmax_lat - eps,
    )
    .expect("ok");

    // The range must collapse to a single tile.
    assert_eq!(min_x, max_x, "x range must be 1 tile wide");
    assert_eq!(min_y, max_y, "y range must be 1 tile tall");
    assert_eq!(min_x, 1, "x should be 1");
    assert_eq!(min_y, 2, "y should be 2");
}

// ---------------------------------------------------------------------------
// Test 3 — bbox where no tiles exist → valid archive with zero tiles
// ---------------------------------------------------------------------------

#[test]
fn test_extract_empty_bbox_no_tiles() {
    // An archive that only has a tile in the NE quadrant (z=1, x=1, y=0).
    let src = build_archive(&[(1, 1, 0, b"ne-only")]);

    // Extract from the SW quadrant where there are no tiles.
    let opts = ExtractOptions {
        min_zoom: Some(1),
        max_zoom: Some(1),
        preserve_metadata: false,
    };
    let out = extract_subregion(&src, -180.0, -85.0, 0.0, 0.0, &opts).expect("ok");

    assert_valid_pmtiles(&out);
    let hdr = PmTilesHeader::parse(&out).expect("parse header");
    assert_eq!(hdr.addressed_tiles, 0, "no tiles should be in the output");
}

// ---------------------------------------------------------------------------
// Test 4 — single tile round-trip: bbox covers it → output contains that tile
// ---------------------------------------------------------------------------

#[test]
fn test_extract_single_tile_round_trip() {
    // Source contains exactly one tile at z=1, x=0, y=0 (NW quadrant).
    let src = build_archive(&[(1, 0, 0, b"nw-tile-payload")]);

    // Extract using a bbox that covers the NW quadrant only.
    let opts = ExtractOptions {
        min_zoom: Some(1),
        max_zoom: Some(1),
        preserve_metadata: true,
    };
    // NW quadrant: lon [-180..0], lat [0..85]
    let out = extract_subregion(&src, -180.0, 0.0, 0.0, 85.0, &opts).expect("ok");

    assert_valid_pmtiles(&out);

    let reader = PmTilesReader::from_bytes(out).expect("reader ok");

    // The NW tile must be present and have the right payload.
    let got = reader
        .get_tile(1, 0, 0)
        .expect("get_tile ok")
        .expect("tile must exist");
    assert_eq!(got.as_slice(), b"nw-tile-payload", "tile payload mismatch");

    // The NE tile (1, 1, 0) is outside the bbox → must be absent.
    assert!(
        reader.get_tile(1, 1, 0).expect("get_tile ok").is_none(),
        "NE tile must not be in the output"
    );
}

// ---------------------------------------------------------------------------
// Test 5 — multi-zoom archive: extract keeps tiles at all zoom levels in bbox
// ---------------------------------------------------------------------------

#[test]
fn test_extract_multi_zoom_preserves_all_tiles_in_bbox() {
    // Build an archive with tiles at z=0, z=1, z=2, some inside the NW bbox
    // and one outside (z=2, x=2, y=0 is in the NE quadrant east of 0°).
    let src = build_archive(&[
        (0, 0, 0, b"z0-world"), // z=0: whole world → always included
        (1, 0, 0, b"z1-nw"),    // z=1 NW quadrant → inside
        (2, 0, 0, b"z2-nw-00"), // z=2 tile in NW → inside
        (2, 0, 1, b"z2-nw-01"), // z=2 tile in NW (next row) → inside
        (2, 2, 0, b"z2-east"),  // z=2 tile well east of 0° → outside
    ]);

    let opts = ExtractOptions {
        min_zoom: None, // use archive's min_zoom = 0
        max_zoom: None, // use archive's max_zoom = 2
        preserve_metadata: true,
    };
    // Western hemisphere north of equator.  max_lon=-0.001 keeps us strictly
    // west of the prime meridian (lon=0 maps to x=1 at z=1 and x=2 at z=2).
    let out = extract_subregion(&src, -180.0, 0.001, -0.001, 85.0, &opts).expect("ok");

    assert_valid_pmtiles(&out);
    let reader = PmTilesReader::from_bytes(out).expect("reader ok");

    // z=0 is the whole world → must be present.
    assert!(
        reader.get_tile(0, 0, 0).expect("ok").is_some(),
        "z=0 tile must be in output"
    );
    // z=1 NW tile → present.
    assert!(
        reader.get_tile(1, 0, 0).expect("ok").is_some(),
        "z=1 NW tile must be in output"
    );
    // z=2 NW tiles → present.
    assert!(
        reader.get_tile(2, 0, 0).expect("ok").is_some(),
        "z=2 (0,0) must be in output"
    );
    assert!(
        reader.get_tile(2, 0, 1).expect("ok").is_some(),
        "z=2 (0,1) must be in output"
    );
    // z=2 eastern tile → absent.
    assert!(
        reader.get_tile(2, 2, 0).expect("ok").is_none(),
        "eastern tile must NOT be in output"
    );
}

// ---------------------------------------------------------------------------
// Test 6 — tiles outside bbox are dropped from output
// ---------------------------------------------------------------------------

#[test]
fn test_extract_drops_tiles_outside_bbox() {
    // One tile in each of the four z=1 quadrants.
    let src = build_archive(&[
        (1, 0, 0, b"NW"), // lon [-180..0], lat [0..85]
        (1, 1, 0, b"NE"), // lon [0..180],  lat [0..85]
        (1, 0, 1, b"SW"), // lon [-180..0], lat [-85..0]
        (1, 1, 1, b"SE"), // lon [0..180],  lat [-85..0]
    ]);

    // Extract only the NW tile.  At z=1, lon=0 maps to tile x=1, so we use
    // max_lon=-0.001 to stay strictly inside tile x=0 (the western half).
    // Similarly, lat=0 maps to y=1, so min_lat=0.001 keeps us in the northern
    // half (tile y=0).
    let opts = ExtractOptions {
        min_zoom: Some(1),
        max_zoom: Some(1),
        preserve_metadata: false,
    };
    let out = extract_subregion(&src, -180.0, 0.001, -0.001, 85.0, &opts).expect("ok");

    assert_valid_pmtiles(&out);
    let hdr = PmTilesHeader::parse(&out).expect("parse header");
    assert_eq!(
        hdr.addressed_tiles, 1,
        "exactly one tile should be extracted"
    );

    let reader = PmTilesReader::from_bytes(out).expect("reader ok");
    assert!(
        reader.get_tile(1, 0, 0).expect("ok").is_some(),
        "NW must be present"
    );
    assert!(
        reader.get_tile(1, 1, 0).expect("ok").is_none(),
        "NE must be absent"
    );
    assert!(
        reader.get_tile(1, 0, 1).expect("ok").is_none(),
        "SW must be absent"
    );
    assert!(
        reader.get_tile(1, 1, 1).expect("ok").is_none(),
        "SE must be absent"
    );
}

// ---------------------------------------------------------------------------
// Test 7 — preserve_metadata = true copies zoom range to output header
// ---------------------------------------------------------------------------

#[test]
fn test_extract_preserve_metadata_copies_min_max_zoom() {
    // Source archive spans z=0 to z=3.
    let src = build_archive(&[
        (0, 0, 0, b"z0"),
        (1, 0, 0, b"z1"),
        (2, 0, 0, b"z2"),
        (3, 0, 0, b"z3"),
    ]);

    // Extract with a narrower zoom range (z=1..=2) and preserve_metadata=true.
    let opts = ExtractOptions {
        min_zoom: Some(1),
        max_zoom: Some(2),
        preserve_metadata: true,
    };
    // Full world bbox so we are not restricting by geography.
    let out = extract_subregion(&src, -180.0, -85.05, 180.0, 85.05, &opts).expect("ok");

    assert_valid_pmtiles(&out);
    let hdr = PmTilesHeader::parse(&out).expect("parse header");

    // The builder was constructed with zoom range [1..=2].
    assert_eq!(hdr.min_zoom, 1, "min_zoom must be 1");
    assert_eq!(hdr.max_zoom, 2, "max_zoom must be 2");

    // Verify tile presence/absence via the reader.
    let reader = PmTilesReader::from_bytes(out).expect("reader ok");

    assert!(
        reader.get_tile(0, 0, 0).expect("ok").is_none(),
        "z=0 tile must be absent (outside effective zoom)"
    );
    assert!(
        reader.get_tile(1, 0, 0).expect("ok").is_some(),
        "z=1 tile must be present"
    );
    assert!(
        reader.get_tile(2, 0, 0).expect("ok").is_some(),
        "z=2 tile must be present"
    );
    assert!(
        reader.get_tile(3, 0, 0).expect("ok").is_none(),
        "z=3 tile must be absent (outside effective zoom)"
    );
}
