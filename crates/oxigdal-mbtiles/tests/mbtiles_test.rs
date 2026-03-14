//! Integration tests for oxigdal-mbtiles.

use std::collections::HashMap;

use oxigdal_mbtiles::{MBTiles, MBTilesMetadata, TileCoord, TileFormat, tms_to_xyz, xyz_to_tms};

// ── Test 1: MBTilesMetadata from_map with name ────────────────────────────────

#[test]
fn test_metadata_name() {
    let mut map = HashMap::new();
    map.insert("name".to_string(), "My Tiles".to_string());
    let meta = MBTilesMetadata::from_map(map);
    assert_eq!(meta.name.as_deref(), Some("My Tiles"));
}

// ── Test 2: format="png" → TileFormat::Png ───────────────────────────────────

#[test]
fn test_metadata_format_png() {
    let mut map = HashMap::new();
    map.insert("format".to_string(), "png".to_string());
    let meta = MBTilesMetadata::from_map(map);
    assert_eq!(meta.format, Some(TileFormat::Png));
}

// ── Test 3: bounds parsing ────────────────────────────────────────────────────

#[test]
fn test_metadata_bounds() {
    let mut map = HashMap::new();
    map.insert("bounds".to_string(), "1.0,2.0,3.0,4.0".to_string());
    let meta = MBTilesMetadata::from_map(map);
    let b = meta.bounds.expect("bounds present");
    assert!((b[0] - 1.0).abs() < f64::EPSILON);
    assert!((b[1] - 2.0).abs() < f64::EPSILON);
    assert!((b[2] - 3.0).abs() < f64::EPSILON);
    assert!((b[3] - 4.0).abs() < f64::EPSILON);
}

// ── Test 4: center parsing ────────────────────────────────────────────────────

#[test]
fn test_metadata_center() {
    let mut map = HashMap::new();
    map.insert("center".to_string(), "10.0,20.0,5".to_string());
    let meta = MBTilesMetadata::from_map(map);
    let c = meta.center.expect("center present");
    assert!((c[0] - 10.0).abs() < f64::EPSILON);
    assert!((c[1] - 20.0).abs() < f64::EPSILON);
    assert!((c[2] - 5.0).abs() < f64::EPSILON);
}

// ── Test 5: minzoom / maxzoom parsing ────────────────────────────────────────

#[test]
fn test_metadata_zoom_levels() {
    let mut map = HashMap::new();
    map.insert("minzoom".to_string(), "3".to_string());
    map.insert("maxzoom".to_string(), "14".to_string());
    let meta = MBTilesMetadata::from_map(map);
    assert_eq!(meta.minzoom, Some(3));
    assert_eq!(meta.maxzoom, Some(14));
}

// ── Test 6: zoom_range() ──────────────────────────────────────────────────────

#[test]
fn test_metadata_zoom_range() {
    let mut map = HashMap::new();
    map.insert("minzoom".to_string(), "2".to_string());
    map.insert("maxzoom".to_string(), "8".to_string());
    let meta = MBTilesMetadata::from_map(map);
    assert_eq!(meta.zoom_range(), Some((2, 8)));
}

// ── Test 7: TileFormat::from_str("png") ──────────────────────────────────────

#[test]
fn test_tile_format_png() {
    assert_eq!(TileFormat::from_str("png"), TileFormat::Png);
}

// ── Test 8: TileFormat::from_str("jpg") ──────────────────────────────────────

#[test]
fn test_tile_format_jpg() {
    assert_eq!(TileFormat::from_str("jpg"), TileFormat::Jpeg);
    assert_eq!(TileFormat::from_str("jpeg"), TileFormat::Jpeg);
}

// ── Test 9: TileFormat::from_str("webp") ─────────────────────────────────────

#[test]
fn test_tile_format_webp() {
    assert_eq!(TileFormat::from_str("webp"), TileFormat::Webp);
}

// ── Test 10: TileFormat::from_str("pbf") ─────────────────────────────────────

#[test]
fn test_tile_format_pbf() {
    assert_eq!(TileFormat::from_str("pbf"), TileFormat::Pbf);
}

// ── Test 11: mime_type() ──────────────────────────────────────────────────────

#[test]
fn test_tile_format_mime_type() {
    assert_eq!(TileFormat::Png.mime_type(), "image/png");
    assert_eq!(TileFormat::Jpeg.mime_type(), "image/jpeg");
    assert_eq!(TileFormat::Webp.mime_type(), "image/webp");
    assert_eq!(TileFormat::Pbf.mime_type(), "application/x-protobuf");
}

// ── Test 12: is_vector / is_raster ───────────────────────────────────────────

#[test]
fn test_tile_format_vector_raster() {
    assert!(TileFormat::Pbf.is_vector());
    assert!(!TileFormat::Pbf.is_raster());
    assert!(TileFormat::Png.is_raster());
    assert!(!TileFormat::Png.is_vector());
    assert!(TileFormat::Jpeg.is_raster());
    assert!(TileFormat::Webp.is_raster());
}

// ── Test 13: tms_to_xyz / xyz_to_tms round-trip ──────────────────────────────

#[test]
fn test_tms_xyz_round_trip() {
    // At zoom 0 there is only tile (0,0); TMS and XYZ are identical.
    assert_eq!(tms_to_xyz(0, 0), 0);
    assert_eq!(xyz_to_tms(0, 0), 0);

    // At zoom 1 there are 2 rows (0 and 1).
    // TMS y=0 (south) ↔ XYZ y=1 (south, since XYZ counts from north).
    assert_eq!(tms_to_xyz(1, 0), 1);
    assert_eq!(xyz_to_tms(1, 1), 0);

    // Round-trip property: xyz_to_tms(z, tms_to_xyz(z, y)) == y
    for z in 0u8..=4 {
        let n = 1u32 << z;
        for y in 0..n {
            assert_eq!(
                xyz_to_tms(z, tms_to_xyz(z, y)),
                y,
                "round-trip failed at z={z} y={y}"
            );
        }
    }
}

// ── Test 14: insert_tile / get_tile / has_tile ────────────────────────────────

#[test]
fn test_mbtiles_insert_get_has() {
    let mut store = MBTiles::new(MBTilesMetadata::default());
    let coord = TileCoord { z: 3, x: 1, y: 2 };
    let payload = vec![0u8, 1, 2, 3];

    assert!(!store.has_tile(&coord));
    store.insert_tile(coord.clone(), payload.clone());
    assert!(store.has_tile(&coord));
    assert_eq!(store.get_tile(&coord), Some(&payload));
}

// ── Test 15: tile_count / tiles_at_zoom / zoom_levels ────────────────────────

#[test]
fn test_mbtiles_counts_and_zoom_levels() {
    let mut store = MBTiles::new(MBTilesMetadata::default());
    store.insert_tile(TileCoord { z: 0, x: 0, y: 0 }, vec![0]);
    store.insert_tile(TileCoord { z: 1, x: 0, y: 0 }, vec![1]);
    store.insert_tile(TileCoord { z: 1, x: 1, y: 0 }, vec![2]);
    store.insert_tile(TileCoord { z: 2, x: 0, y: 0 }, vec![3]);

    assert_eq!(store.tile_count(), 4);
    assert_eq!(store.tiles_at_zoom(1).len(), 2);
    assert_eq!(store.tiles_at_zoom(3).len(), 0);

    let zooms = store.zoom_levels();
    assert_eq!(zooms, vec![0, 1, 2]);
}

// ── Extra: extra fields stored in metadata ────────────────────────────────────

#[test]
fn test_metadata_extra_fields() {
    let mut map = HashMap::new();
    map.insert("custom_key".to_string(), "custom_value".to_string());
    let meta = MBTilesMetadata::from_map(map);
    assert_eq!(
        meta.extra.get("custom_key").map(|s| s.as_str()),
        Some("custom_value")
    );
}
