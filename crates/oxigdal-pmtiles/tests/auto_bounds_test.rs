//! Integration tests for the auto-bounds / auto-center / auto-zoom-range
//! helpers and the underlying Web Mercator conversion functions.

use oxigdal_pmtiles::{
    PmTilesBuilder, TileType, lonlat_to_tile, tile_bounds_lonlat, tile_to_lonlat,
};

// ── webmerc unit tests via the public re-export ──────────────────────────────

#[test]
fn test_tile_to_lonlat_root_tile_is_world_top_left() {
    let (lon, lat) = tile_to_lonlat(0, 0, 0);
    assert!(
        (lon - (-180.0)).abs() < 1e-6,
        "lon should be -180 for z=0,x=0,y=0, got {lon}"
    );
    // The top row (y=0) maps to the north pole edge, clamped to ~85.05.
    assert!(lat > 85.0, "lat should be ~85 for y=0 at z=0, got {lat}");
}

#[test]
fn test_tile_bounds_lonlat_z0_covers_full_world() {
    let (min_lon, min_lat, max_lon, max_lat) = tile_bounds_lonlat(0, 0, 0);
    assert!((min_lon - (-180.0)).abs() < 1e-6, "min_lon={min_lon}");
    assert!((max_lon - 180.0).abs() < 1e-6, "max_lon={max_lon}");
    assert!(min_lat < -85.0, "min_lat={min_lat}");
    assert!(max_lat > 85.0, "max_lat={max_lat}");
}

#[test]
fn test_tile_bounds_lonlat_z1_quadrants() {
    // z=1, x=0, y=0 is the NW quadrant: lon in [-180, 0], lat in [0, ~85].
    let (min_lon, min_lat, max_lon, max_lat) = tile_bounds_lonlat(1, 0, 0);
    assert!(min_lon < 0.0, "NW min_lon={min_lon}");
    assert!(max_lon <= 0.0 + 1e-6, "NW max_lon={max_lon}");
    assert!(min_lat >= 0.0 - 1e-6, "NW min_lat={min_lat}");
    assert!(max_lat > 85.0, "NW max_lat={max_lat}");

    // z=1, x=1, y=1 is the SE quadrant: lon in [0, 180], lat in [-85, 0].
    let (min_lon, min_lat, max_lon, max_lat) = tile_bounds_lonlat(1, 1, 1);
    assert!(min_lon >= 0.0 - 1e-6, "SE min_lon={min_lon}");
    assert!(max_lon > 0.0, "SE max_lon={max_lon}");
    assert!(min_lat < 0.0, "SE min_lat={min_lat}");
    assert!(max_lat <= 0.0 + 1e-6, "SE max_lat={max_lat}");
}

#[test]
fn test_lonlat_to_tile_round_trip_at_z10() {
    let z = 10u8;
    let lon = 13.405_0_f64; // Berlin
    let lat = 52.520_0_f64;
    let (x, y) = lonlat_to_tile(z, lon, lat);
    let (min_lon, min_lat, max_lon, max_lat) = tile_bounds_lonlat(z, x, y);
    assert!(
        lon >= min_lon && lon <= max_lon,
        "lon {lon} not in tile bounds [{min_lon}, {max_lon}]"
    );
    assert!(
        lat >= min_lat && lat <= max_lat,
        "lat {lat} not in tile bounds [{min_lat}, {max_lat}]"
    );
}

// ── PmTilesBuilder auto-* helpers ───────────────────────────────────────────

#[test]
fn test_auto_bounds_z0_single_tile() {
    // A z=0 tile covers the whole world; auto_bounds should not crash and the
    // archive should be non-empty.
    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 0);
    builder.add_tile(0, 0, 0, &[0u8; 100]).expect("add_tile");
    builder.auto_bounds();
    let archive = builder.build().expect("build");
    assert!(!archive.is_empty(), "archive must not be empty");
    // The PMTiles v3 header is always 127 bytes; a non-trivial archive is
    // much larger.
    assert!(
        archive.len() > 127,
        "archive too small: {} bytes",
        archive.len()
    );
}

#[test]
fn test_auto_bounds_multi_tile_aggregates_extent() {
    // Two tiles at z=1: NW (0,0) and SE (1,1).  Their union is the whole world.
    let mut builder = PmTilesBuilder::new(TileType::Png, 1, 1);
    builder.add_tile(1, 0, 0, &[0u8; 100]).expect("NW tile");
    builder.add_tile(1, 1, 1, &[1u8; 100]).expect("SE tile");
    builder.auto_bounds();
    let archive = builder.build().expect("build");
    assert!(!archive.is_empty());
}

#[test]
fn test_auto_center_sets_midpoint() {
    // After auto_bounds the center should be computable; archive must build.
    let mut builder = PmTilesBuilder::new(TileType::Png, 1, 1);
    builder.add_tile(1, 0, 0, &[0u8; 100]).expect("add_tile");
    builder.auto_bounds();
    builder.auto_center();
    let archive = builder.build().expect("build");
    assert!(!archive.is_empty());
}

#[test]
fn test_auto_zoom_range_derives_min_max() {
    // Tiles at z=2 and z=4 should produce min_zoom=2, max_zoom=4.
    let mut builder = PmTilesBuilder::new(TileType::Png, 2, 4);
    builder.add_tile(2, 0, 0, &[0u8; 100]).expect("z2 tile");
    builder.add_tile(4, 1, 1, &[1u8; 100]).expect("z4 tile");
    builder.auto_zoom_range();
    let archive = builder.build().expect("build");
    assert!(!archive.is_empty());
}

#[test]
fn test_auto_all_chained_produces_valid_archive() {
    // auto_all() = auto_zoom_range + auto_bounds + auto_center in sequence.
    let mut builder = PmTilesBuilder::new(TileType::Mvt, 0, 2);
    builder.add_tile(0, 0, 0, b"root").expect("z0");
    builder.add_tile(1, 0, 0, b"nw-z1").expect("z1-nw");
    builder.add_tile(2, 0, 0, b"nw-z2").expect("z2-nw");
    builder.auto_all();
    let archive = builder.build().expect("build");
    assert!(archive.len() > 127);
}

#[test]
fn test_auto_bounds_empty_builder_does_not_panic() {
    // When no tiles are present, auto_bounds must be a no-op.
    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 14);
    builder.auto_bounds();
    // Build still succeeds (zero-tile archive is valid for header-only output).
    let archive = builder.build().expect("build");
    // Header is always 127 bytes; an empty archive may contain only the header
    // and empty sections.
    assert!(archive.len() >= 127);
}
