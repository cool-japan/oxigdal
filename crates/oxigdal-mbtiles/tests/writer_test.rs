//! Comprehensive tests for `MBTilesWriter`, `MBTilesData`, `TileScheme`,
//! `TileRange`, bbox utilities, `VectorLayerSpec`, and `TileStatsAggregator`.

use oxigdal_mbtiles::bbox_util::{
    bbox_to_tiles, lonlat_to_tile, tile_count_at_zoom, tile_resolution_degrees,
    tile_resolution_metres, tile_to_bbox, tile_to_lonlat,
};
use oxigdal_mbtiles::tile_coords::{TileCoord, TileFormat};
use oxigdal_mbtiles::writer::{
    FieldType, MBTilesWriter, TileRange, TileScheme, TileStatsAggregator, VectorLayerSpec,
};

// ── helpers ──────────────────────────────────────────────────────────────────

fn sample_tile(size: usize) -> Vec<u8> {
    vec![0xAB_u8; size]
}

// ── MBTilesWriter ─────────────────────────────────────────────────────────────

#[test]
fn writer_new_sets_name_and_format() {
    let w = MBTilesWriter::new("test", TileFormat::Png);
    let data = w.build();
    assert_eq!(data.metadata.name.as_deref(), Some("test"));
}

#[test]
fn writer_add_tile_stores_data() {
    let mut w = MBTilesWriter::new("t", TileFormat::Png);
    w.add_tile(2, 1, 1, sample_tile(512));
    let data = w.build();
    assert_eq!(data.tile_count(), 1);
}

#[test]
fn writer_add_tile_retrieves_correct_data() {
    let mut w = MBTilesWriter::new("t", TileFormat::Png);
    let payload = vec![1_u8, 2, 3, 4];
    w.add_tile(3, 2, 5, payload.clone());
    let data = w.build();
    assert_eq!(data.get_tile(3, 2, 5), Some(&payload));
}

#[test]
fn writer_add_tile_missing_returns_none() {
    let w = MBTilesWriter::new("t", TileFormat::Png);
    let data = w.build();
    assert!(data.get_tile(0, 0, 0).is_none());
}

#[test]
fn writer_add_tile_xyz_flips_y_at_zoom_1() {
    // At z=1: tiles are 0,1 on each axis.  XYZ y=0 → TMS y=1, XYZ y=1 → TMS y=0
    let mut w = MBTilesWriter::new("t", TileFormat::Png);
    let payload = sample_tile(64);
    // XYZ (z=1, x=0, y=0) should land at TMS y=1
    w.add_tile_xyz(1, 0, 0, payload.clone());
    let data = w.build();
    // Internal storage uses TMS, so retrieve with TMS y=1
    assert_eq!(data.get_tile(1, 0, 1), Some(&payload));
    // XYZ y=0 should NOT be there under TMS y=0
    assert!(data.get_tile(1, 0, 0).is_none());
}

#[test]
fn writer_add_tile_xyz_flips_y_at_zoom_2() {
    // At z=2: n=4. XYZ y=0 → TMS y=3
    let mut w = MBTilesWriter::new("t", TileFormat::Png);
    let payload = sample_tile(32);
    w.add_tile_xyz(2, 0, 0, payload.clone());
    let data = w.build();
    assert_eq!(data.get_tile(2, 0, 3), Some(&payload));
}

#[test]
fn writer_add_multiple_tiles_different_zooms() {
    let mut w = MBTilesWriter::new("t", TileFormat::Png);
    w.add_tile(0, 0, 0, sample_tile(100));
    w.add_tile(1, 0, 0, sample_tile(200));
    w.add_tile(1, 1, 0, sample_tile(300));
    w.add_tile(2, 0, 0, sample_tile(400));
    let data = w.build();
    assert_eq!(data.tile_count(), 4);
}

#[test]
fn writer_overwrite_tile_replaces_data() {
    let mut w = MBTilesWriter::new("t", TileFormat::Png);
    w.add_tile(0, 0, 0, vec![1, 2, 3]);
    w.add_tile(0, 0, 0, vec![9, 8, 7]);
    let data = w.build();
    assert_eq!(data.tile_count(), 1);
    assert_eq!(data.get_tile(0, 0, 0), Some(&vec![9_u8, 8, 7]));
}

#[test]
fn writer_remove_tile_returns_true_when_present() {
    let mut w = MBTilesWriter::new("t", TileFormat::Png);
    let coord = TileCoord { z: 1, x: 0, y: 0 };
    w.add_tile(1, 0, 0, sample_tile(10));
    assert!(w.remove_tile(&coord));
}

#[test]
fn writer_remove_tile_returns_false_when_absent() {
    let mut w = MBTilesWriter::new("t", TileFormat::Png);
    let coord = TileCoord { z: 5, x: 3, y: 3 };
    assert!(!w.remove_tile(&coord));
}

#[test]
fn writer_remove_tile_reduces_count() {
    let mut w = MBTilesWriter::new("t", TileFormat::Png);
    w.add_tile(1, 0, 0, sample_tile(10));
    w.add_tile(1, 1, 0, sample_tile(10));
    let coord = TileCoord { z: 1, x: 0, y: 0 };
    w.remove_tile(&coord);
    let data = w.build();
    assert_eq!(data.tile_count(), 1);
}

#[test]
fn writer_count_at_zoom_single_zoom() {
    let mut w = MBTilesWriter::new("t", TileFormat::Png);
    w.add_tile(3, 0, 0, sample_tile(1));
    w.add_tile(3, 1, 0, sample_tile(1));
    w.add_tile(3, 2, 0, sample_tile(1));
    assert_eq!(w.count_at_zoom(3), 3);
    assert_eq!(w.count_at_zoom(4), 0);
}

#[test]
fn writer_count_at_zoom_mixed() {
    let mut w = MBTilesWriter::new("t", TileFormat::Png);
    for x in 0..4_u32 {
        w.add_tile(2, x, 0, sample_tile(1));
    }
    w.add_tile(3, 0, 0, sample_tile(1));
    assert_eq!(w.count_at_zoom(2), 4);
    assert_eq!(w.count_at_zoom(3), 1);
}

#[test]
fn writer_zoom_levels_sorted() {
    let mut w = MBTilesWriter::new("t", TileFormat::Png);
    w.add_tile(5, 0, 0, sample_tile(1));
    w.add_tile(2, 0, 0, sample_tile(1));
    w.add_tile(0, 0, 0, sample_tile(1));
    w.add_tile(3, 0, 0, sample_tile(1));
    let levels = w.zoom_levels();
    assert_eq!(levels, vec![0, 2, 3, 5]);
}

#[test]
fn writer_zoom_levels_deduplicated() {
    let mut w = MBTilesWriter::new("t", TileFormat::Png);
    w.add_tile(1, 0, 0, sample_tile(1));
    w.add_tile(1, 1, 0, sample_tile(1));
    w.add_tile(1, 0, 1, sample_tile(1));
    let levels = w.zoom_levels();
    assert_eq!(levels, vec![1]);
}

#[test]
fn writer_zoom_levels_empty() {
    let w = MBTilesWriter::new("t", TileFormat::Png);
    assert!(w.zoom_levels().is_empty());
}

// ── MBTilesData ───────────────────────────────────────────────────────────────

#[test]
fn data_tile_count_matches_inserted() {
    let mut w = MBTilesWriter::new("t", TileFormat::Pbf);
    for z in 0_u8..=3 {
        for x in 0..(1u32 << z) {
            w.add_tile(z, x, 0, sample_tile(8));
        }
    }
    let expected = (1 + 2 + 4 + 8) as usize; // 15 tiles
    let data = w.build();
    assert_eq!(data.tile_count(), expected);
}

#[test]
fn data_zoom_range_min_max() {
    let mut w = MBTilesWriter::new("t", TileFormat::Png);
    w.add_tile(2, 0, 0, sample_tile(1));
    w.add_tile(4, 0, 0, sample_tile(1));
    w.add_tile(7, 0, 0, sample_tile(1));
    let data = w.build();
    assert_eq!(data.zoom_range(), Some((2, 7)));
}

#[test]
fn data_zoom_range_empty() {
    let w = MBTilesWriter::new("t", TileFormat::Png);
    let data = w.build();
    assert!(data.zoom_range().is_none());
}

#[test]
fn data_zoom_range_single_zoom() {
    let mut w = MBTilesWriter::new("t", TileFormat::Png);
    w.add_tile(5, 0, 0, sample_tile(1));
    w.add_tile(5, 1, 0, sample_tile(1));
    let data = w.build();
    assert_eq!(data.zoom_range(), Some((5, 5)));
}

#[test]
fn data_total_size_bytes_sum() {
    let mut w = MBTilesWriter::new("t", TileFormat::Png);
    w.add_tile(0, 0, 0, sample_tile(100));
    w.add_tile(1, 0, 0, sample_tile(200));
    w.add_tile(1, 1, 0, sample_tile(300));
    let data = w.build();
    assert_eq!(data.total_size_bytes(), 600);
}

#[test]
fn data_total_size_bytes_empty() {
    let w = MBTilesWriter::new("t", TileFormat::Png);
    let data = w.build();
    assert_eq!(data.total_size_bytes(), 0);
}

#[test]
fn data_get_tile_found() {
    let mut w = MBTilesWriter::new("t", TileFormat::Png);
    let payload = vec![42_u8; 50];
    w.add_tile(3, 2, 1, payload.clone());
    let data = w.build();
    assert_eq!(data.get_tile(3, 2, 1), Some(&payload));
}

#[test]
fn data_get_tile_not_found() {
    let w = MBTilesWriter::new("t", TileFormat::Png);
    let data = w.build();
    assert!(data.get_tile(99, 0, 0).is_none());
}

// ── TileScheme ────────────────────────────────────────────────────────────────

#[test]
fn tile_scheme_flip_y_zoom_0_only_tile() {
    let s = TileScheme::Xyz;
    // At zoom 0 there is only tile y=0; flip(0) = 2^0 - 1 - 0 = 0
    assert_eq!(s.flip_y(0, 0), 0);
}

#[test]
fn tile_scheme_flip_y_zoom_1_symmetric() {
    let s = TileScheme::Xyz;
    // At zoom 1: n=2. flip(0)=1, flip(1)=0
    assert_eq!(s.flip_y(0, 1), 1);
    assert_eq!(s.flip_y(1, 1), 0);
}

#[test]
fn tile_scheme_flip_y_zoom_2() {
    let s = TileScheme::Tms;
    // At zoom 2: n=4. flip(0)=3, flip(3)=0, flip(1)=2, flip(2)=1
    assert_eq!(s.flip_y(0, 2), 3);
    assert_eq!(s.flip_y(3, 2), 0);
    assert_eq!(s.flip_y(1, 2), 2);
    assert_eq!(s.flip_y(2, 2), 1);
}

#[test]
fn tile_scheme_flip_y_is_involution() {
    let s = TileScheme::Xyz;
    for z in 0_u8..=4 {
        for y in 0..(1u32 << z) {
            assert_eq!(s.flip_y(s.flip_y(y, z), z), y);
        }
    }
}

// ── TileRange ─────────────────────────────────────────────────────────────────

#[test]
fn tile_range_world_zoom_0_one_tile() {
    let r = TileRange::from_bbox(-180.0, -90.0, 180.0, 90.0, 0, 0);
    assert_eq!(r.min_zoom, 0);
    assert_eq!(r.max_zoom, 0);
    // At zoom 0 there is exactly 1 tile
    let count: u64 = r.iter().count() as u64;
    assert_eq!(count, 1);
}

#[test]
fn tile_range_world_zoom_1_four_tiles() {
    let r = TileRange::from_bbox(-180.0, -85.0, 180.0, 85.0, 1, 1);
    let coords: Vec<_> = r.iter().collect();
    assert_eq!(coords.len(), 4);
}

#[test]
fn tile_range_tile_count_matches_iter_count() {
    let r = TileRange::from_bbox(-180.0, -85.0, 180.0, 85.0, 0, 2);
    let iter_count = r.iter().count() as u64;
    // tile_count() and iter count must agree
    assert_eq!(r.tile_count(), iter_count);
}

#[test]
fn tile_range_iter_yields_correct_zoom_levels() {
    let r = TileRange::from_bbox(-180.0, -85.0, 180.0, 85.0, 0, 2);
    let zooms: std::collections::HashSet<u8> = r.iter().map(|(z, _, _)| z).collect();
    assert!(zooms.contains(&0));
    assert!(zooms.contains(&1));
    assert!(zooms.contains(&2));
}

#[test]
fn tile_range_iter_no_duplicates() {
    let r = TileRange::from_bbox(-90.0, -45.0, 90.0, 45.0, 0, 3);
    let mut seen = std::collections::HashSet::new();
    for coord in r.iter() {
        assert!(seen.insert(coord), "Duplicate coord {:?}", coord);
    }
}

#[test]
fn tile_range_single_zoom_all_z_match() {
    let r = TileRange::from_bbox(-180.0, -85.0, 180.0, 85.0, 2, 2);
    for (z, _, _) in r.iter() {
        assert_eq!(z, 2);
    }
}

#[test]
fn tile_range_from_bbox_min_le_max() {
    let r = TileRange::from_bbox(-10.0, -10.0, 10.0, 10.0, 3, 5);
    assert!(r.min_x <= r.max_x);
    assert!(r.min_y <= r.max_y);
}

// ── bbox_util ─────────────────────────────────────────────────────────────────

#[test]
fn lonlat_to_tile_origin_zoom_0() {
    // At zoom 0 there is one tile (0, 0)
    let (x, y) = lonlat_to_tile(0.0, 0.0, 0);
    assert_eq!(x, 0);
    assert_eq!(y, 0);
}

#[test]
fn lonlat_to_tile_top_left_zoom_1() {
    // (-180°, 85°) → (0, 0) at zoom 1
    let (x, y) = lonlat_to_tile(-180.0, 85.0, 1);
    assert_eq!(x, 0);
    assert_eq!(y, 0);
}

#[test]
fn lonlat_to_tile_bottom_right_zoom_1() {
    // (180°, -85°) → (1, 1) at zoom 1
    let (x, y) = lonlat_to_tile(179.999, -85.0, 1);
    assert_eq!(x, 1);
    assert_eq!(y, 1);
}

#[test]
fn lonlat_to_tile_greenwich_equator_zoom_2() {
    // (0°, 0°) should be at x=2, y=2 at zoom 2
    let (x, y) = lonlat_to_tile(0.0, 0.0, 2);
    assert_eq!(x, 2);
    assert_eq!(y, 2);
}

#[test]
fn tile_to_lonlat_zoom_0_upper_left() {
    // Tile (0,0) at zoom 0 → upper-left corner of the world (-180°, ~85°)
    let (lon, lat) = tile_to_lonlat(0, 0, 0);
    assert!((lon - (-180.0)).abs() < 1e-9);
    assert!(lat > 80.0); // close to north pole of mercator projection
}

#[test]
fn tile_to_lonlat_zoom_1_tile_1_1() {
    // Tile (1,1) at zoom 1 → (0°, 0°) — bottom-right of top-left quadrant
    let (lon, lat) = tile_to_lonlat(1, 1, 1);
    assert!((lon - 0.0).abs() < 1e-9);
    assert!(lat.abs() < 1e-6);
}

#[test]
fn tile_to_bbox_zoom_0_world() {
    let [west, south, east, north] = tile_to_bbox(0, 0, 0);
    // Should cover the full longitude range
    assert!((west - (-180.0)).abs() < 1e-6);
    assert!(east > 179.9);
    // Lat range should span from some large negative to large positive
    assert!(south < -80.0);
    assert!(north > 80.0);
}

#[test]
fn tile_to_bbox_zoom_1_quadrants() {
    // Tile (0,0) at zoom 1 is the NW quadrant
    let [west, _south, east, north] = tile_to_bbox(1, 0, 0);
    assert!((west - (-180.0)).abs() < 1e-6);
    assert!((east - 0.0).abs() < 1e-6);
    assert!(north > 80.0);
}

#[test]
fn tile_count_at_zoom_0() {
    assert_eq!(tile_count_at_zoom(0), 1);
}

#[test]
fn tile_count_at_zoom_1() {
    assert_eq!(tile_count_at_zoom(1), 4);
}

#[test]
fn tile_count_at_zoom_10() {
    // 4^10 = 2^20 = 1_048_576
    assert_eq!(tile_count_at_zoom(10), 1_048_576);
}

#[test]
fn tile_count_at_zoom_2() {
    assert_eq!(tile_count_at_zoom(2), 16);
}

#[test]
fn bbox_to_tiles_roundtrip() {
    // Use a clearly interior bounding box to avoid edge/boundary ambiguity.
    // The bbox covers lon [-45, 45] / lat [-45, 45] at zoom 2.
    // At zoom 2: x in [1,2], y in [0,1] (approx) — just verify min <= max.
    let (min_x, min_y, max_x, max_y) = bbox_to_tiles(-45.0, -45.0, 45.0, 45.0, 2);
    assert!(min_x <= max_x);
    assert!(min_y <= max_y);
    // The tile containing (0°, 0°) at zoom 2 is (2, 2)
    let (cx, cy) = lonlat_to_tile(0.0, 0.0, 2);
    assert!(cx >= min_x && cx <= max_x);
    assert!(cy >= min_y && cy <= max_y);
}

#[test]
fn tile_resolution_degrees_halves_per_zoom() {
    let r0 = tile_resolution_degrees(0);
    let r1 = tile_resolution_degrees(1);
    // Each zoom level halves the resolution
    assert!((r0 / r1 - 2.0).abs() < 1e-10);
}

#[test]
fn tile_resolution_metres_halves_per_zoom() {
    let r0 = tile_resolution_metres(0);
    let r1 = tile_resolution_metres(1);
    assert!((r0 / r1 - 2.0).abs() < 1e-6);
}

#[test]
fn tile_resolution_metres_zoom0_approx() {
    // At zoom 0: ~156,543 m/px
    let res = tile_resolution_metres(0);
    assert!((res - 156_543.0).abs() < 1.0);
}

// ── VectorLayerSpec ───────────────────────────────────────────────────────────

#[test]
fn vector_layer_spec_to_json_has_id() {
    let spec = VectorLayerSpec::new("roads", 0, 14);
    let v = spec.to_json();
    assert_eq!(v["id"], "roads");
}

#[test]
fn vector_layer_spec_to_json_has_zoom_range() {
    let spec = VectorLayerSpec::new("water", 2, 10);
    let v = spec.to_json();
    assert_eq!(v["minzoom"], 2);
    assert_eq!(v["maxzoom"], 10);
}

#[test]
fn vector_layer_spec_to_json_has_fields_key() {
    let spec = VectorLayerSpec::new("poi", 4, 14)
        .with_field("name", FieldType::String)
        .with_field("population", FieldType::Number)
        .with_field("open", FieldType::Boolean);
    let v = spec.to_json();
    assert!(v["fields"].is_object());
    assert_eq!(v["fields"]["name"], "String");
    assert_eq!(v["fields"]["population"], "Number");
    assert_eq!(v["fields"]["open"], "Boolean");
}

#[test]
fn vector_layer_spec_description_when_set() {
    let mut spec = VectorLayerSpec::new("admin", 0, 12);
    spec.description = Some("Administrative boundaries".into());
    let v = spec.to_json();
    assert_eq!(v["description"], "Administrative boundaries");
}

#[test]
fn vector_layer_spec_no_description_by_default() {
    let spec = VectorLayerSpec::new("land", 0, 8);
    let v = spec.to_json();
    assert!(v["description"].is_null());
}

#[test]
fn vector_layer_spec_with_field_builder() {
    let spec = VectorLayerSpec::new("transit", 6, 14).with_field("route_type", FieldType::Number);
    assert!(spec.fields.contains_key("route_type"));
}

// ── TileStatsAggregator ───────────────────────────────────────────────────────

#[test]
fn stats_empty_mean_is_zero() {
    let s = TileStatsAggregator::new();
    assert_eq!(s.mean_bytes(), 0.0);
}

#[test]
fn stats_single_tile_accumulates() {
    let mut s = TileStatsAggregator::new();
    s.add_tile(3, 1024);
    assert_eq!(s.total_tiles, 1);
    assert_eq!(s.total_bytes, 1024);
    assert_eq!(s.min_bytes, 1024);
    assert_eq!(s.max_bytes, 1024);
}

#[test]
fn stats_mean_bytes_correct() {
    let mut s = TileStatsAggregator::new();
    s.add_tile(0, 100);
    s.add_tile(0, 200);
    s.add_tile(0, 300);
    assert!((s.mean_bytes() - 200.0).abs() < 1e-9);
}

#[test]
fn stats_min_max_correct() {
    let mut s = TileStatsAggregator::new();
    s.add_tile(1, 500);
    s.add_tile(1, 50);
    s.add_tile(1, 1000);
    assert_eq!(s.min_bytes, 50);
    assert_eq!(s.max_bytes, 1000);
}

#[test]
fn stats_per_zoom_mean() {
    let mut s = TileStatsAggregator::new();
    s.add_tile(2, 400);
    s.add_tile(2, 600);
    s.add_tile(3, 800);
    let z2 = s.per_zoom.get(&2).expect("zoom 2 stats");
    assert!((z2.mean_bytes - 500.0).abs() < 1e-6);
    let z3 = s.per_zoom.get(&3).expect("zoom 3 stats");
    assert!((z3.mean_bytes - 800.0).abs() < 1e-6);
}

#[test]
fn stats_compression_ratio_gt_one_when_compressed() {
    let mut s = TileStatsAggregator::new();
    s.add_tile(5, 100); // 100 bytes stored
    let ratio = s.compression_ratio(500); // 500 uncompressed
    assert!(ratio > 1.0);
}

#[test]
fn stats_compression_ratio_one_when_no_tiles() {
    let s = TileStatsAggregator::new();
    assert_eq!(s.compression_ratio(0), 1.0);
}

#[test]
fn stats_per_zoom_tile_count() {
    let mut s = TileStatsAggregator::new();
    for _ in 0..5 {
        s.add_tile(4, 256);
    }
    for _ in 0..3 {
        s.add_tile(5, 512);
    }
    assert_eq!(s.per_zoom[&4].tile_count, 5);
    assert_eq!(s.per_zoom[&5].tile_count, 3);
}

#[test]
fn stats_total_tiles_and_bytes() {
    let mut s = TileStatsAggregator::new();
    s.add_tile(0, 100);
    s.add_tile(1, 200);
    s.add_tile(2, 300);
    assert_eq!(s.total_tiles, 3);
    assert_eq!(s.total_bytes, 600);
}

#[test]
fn stats_default_is_empty() {
    let s: TileStatsAggregator = Default::default();
    assert_eq!(s.total_tiles, 0);
    assert_eq!(s.total_bytes, 0);
}
