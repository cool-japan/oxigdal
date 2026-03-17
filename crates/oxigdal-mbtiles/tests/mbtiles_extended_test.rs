//! Extended integration tests for oxigdal-mbtiles.
//!
//! Brings total test count to 50+.

use std::collections::HashMap;

use oxigdal_mbtiles::{
    MBTiles, MBTilesMetadata, MbTilesError, TileCoord, TileFormat, tms_to_xyz, xyz_to_tms,
};

// ── MBTilesMetadata — additional field coverage ────────────────────────────────

#[test]
fn test_metadata_attribution() {
    let mut map = HashMap::new();
    map.insert(
        "attribution".to_string(),
        "© OpenStreetMap contributors".to_string(),
    );
    let meta = MBTilesMetadata::from_map(map);
    assert_eq!(
        meta.attribution.as_deref(),
        Some("© OpenStreetMap contributors")
    );
}

#[test]
fn test_metadata_description() {
    let mut map = HashMap::new();
    map.insert("description".to_string(), "Road network tiles".to_string());
    let meta = MBTilesMetadata::from_map(map);
    assert_eq!(meta.description.as_deref(), Some("Road network tiles"));
}

#[test]
fn test_metadata_type_overlay() {
    let mut map = HashMap::new();
    map.insert("type".to_string(), "overlay".to_string());
    let meta = MBTilesMetadata::from_map(map);
    assert_eq!(meta.tile_type.as_deref(), Some("overlay"));
}

#[test]
fn test_metadata_type_baselayer() {
    let mut map = HashMap::new();
    map.insert("type".to_string(), "baselayer".to_string());
    let meta = MBTilesMetadata::from_map(map);
    assert_eq!(meta.tile_type.as_deref(), Some("baselayer"));
}

#[test]
fn test_metadata_version_field() {
    let mut map = HashMap::new();
    map.insert("version".to_string(), "1.3.0".to_string());
    let meta = MBTilesMetadata::from_map(map);
    assert_eq!(meta.version.as_deref(), Some("1.3.0"));
}

#[test]
fn test_metadata_json_field() {
    let json = r#"{"vector_layers":[{"id":"water"}]}"#;
    let mut map = HashMap::new();
    map.insert("json".to_string(), json.to_string());
    let meta = MBTilesMetadata::from_map(map);
    assert_eq!(meta.json.as_deref(), Some(json));
}

#[test]
fn test_metadata_all_fields_at_once() {
    let mut map = HashMap::new();
    map.insert("name".to_string(), "Full Tileset".to_string());
    map.insert("format".to_string(), "pbf".to_string());
    map.insert("minzoom".to_string(), "0".to_string());
    map.insert("maxzoom".to_string(), "22".to_string());
    map.insert("bounds".to_string(), "-180.0,-90.0,180.0,90.0".to_string());
    map.insert("center".to_string(), "0.0,0.0,5".to_string());
    map.insert("description".to_string(), "All layers".to_string());
    map.insert("version".to_string(), "2.0".to_string());
    let meta = MBTilesMetadata::from_map(map);
    assert_eq!(meta.name.as_deref(), Some("Full Tileset"));
    assert_eq!(meta.format, Some(TileFormat::Pbf));
    assert_eq!(meta.minzoom, Some(0));
    assert_eq!(meta.maxzoom, Some(22));
    assert!(meta.bounds.is_some());
    assert!(meta.center.is_some());
}

#[test]
fn test_metadata_bounds_with_spaces() {
    // Comma-separated with spaces should still parse
    let mut map = HashMap::new();
    map.insert(
        "bounds".to_string(),
        " -10.0 , -20.0 , 10.0 , 20.0 ".to_string(),
    );
    let meta = MBTilesMetadata::from_map(map);
    let b = meta.bounds.expect("bounds parsed");
    assert!((b[0] - (-10.0)).abs() < f64::EPSILON);
    assert!((b[3] - 20.0).abs() < f64::EPSILON);
}

#[test]
fn test_metadata_bounds_invalid_not_four_parts() {
    let mut map = HashMap::new();
    map.insert("bounds".to_string(), "1.0,2.0".to_string()); // only 2 parts
    let meta = MBTilesMetadata::from_map(map);
    assert!(meta.bounds.is_none());
}

#[test]
fn test_metadata_center_invalid_not_three_parts() {
    let mut map = HashMap::new();
    map.insert("center".to_string(), "10.0,20.0".to_string()); // only 2 parts
    let meta = MBTilesMetadata::from_map(map);
    assert!(meta.center.is_none());
}

#[test]
fn test_metadata_minzoom_nonnumeric_ignored() {
    let mut map = HashMap::new();
    map.insert("minzoom".to_string(), "notanumber".to_string());
    let meta = MBTilesMetadata::from_map(map);
    assert!(meta.minzoom.is_none());
}

#[test]
fn test_metadata_zoom_range_only_min_no_range() {
    let mut map = HashMap::new();
    map.insert("minzoom".to_string(), "3".to_string());
    // maxzoom not set
    let meta = MBTilesMetadata::from_map(map);
    assert!(meta.zoom_range().is_none());
}

#[test]
fn test_metadata_zoom_range_only_max_no_range() {
    let mut map = HashMap::new();
    map.insert("maxzoom".to_string(), "14".to_string());
    // minzoom not set
    let meta = MBTilesMetadata::from_map(map);
    assert!(meta.zoom_range().is_none());
}

#[test]
fn test_metadata_default_all_none() {
    let meta = MBTilesMetadata::default();
    assert!(meta.name.is_none());
    assert!(meta.format.is_none());
    assert!(meta.bounds.is_none());
    assert!(meta.center.is_none());
    assert!(meta.minzoom.is_none());
    assert!(meta.maxzoom.is_none());
    assert!(meta.attribution.is_none());
    assert!(meta.description.is_none());
    assert!(meta.tile_type.is_none());
    assert!(meta.version.is_none());
    assert!(meta.json.is_none());
    assert!(meta.extra.is_empty());
}

#[test]
fn test_metadata_multiple_extra_fields() {
    let mut map = HashMap::new();
    map.insert("custom_a".to_string(), "value_a".to_string());
    map.insert("custom_b".to_string(), "value_b".to_string());
    let meta = MBTilesMetadata::from_map(map);
    assert_eq!(
        meta.extra.get("custom_a").map(|s| s.as_str()),
        Some("value_a")
    );
    assert_eq!(
        meta.extra.get("custom_b").map(|s| s.as_str()),
        Some("value_b")
    );
}

// ── TileFormat — additional coverage ──────────────────────────────────────────

#[test]
fn test_tile_format_case_insensitive_png() {
    assert_eq!(TileFormat::parse_format("PNG"), TileFormat::Png);
    assert_eq!(TileFormat::parse_format("Png"), TileFormat::Png);
}

#[test]
fn test_tile_format_case_insensitive_jpeg() {
    assert_eq!(TileFormat::parse_format("JPEG"), TileFormat::Jpeg);
    assert_eq!(TileFormat::parse_format("JPG"), TileFormat::Jpeg);
}

#[test]
fn test_tile_format_case_insensitive_pbf() {
    assert_eq!(TileFormat::parse_format("PBF"), TileFormat::Pbf);
}

#[test]
fn test_tile_format_unknown_string() {
    let fmt = TileFormat::parse_format("geotiff");
    assert!(matches!(fmt, TileFormat::Unknown(_)));
    if let TileFormat::Unknown(s) = fmt {
        assert_eq!(s, "geotiff");
    }
}

#[test]
fn test_tile_format_unknown_mime_type() {
    let fmt = TileFormat::Unknown("custom".to_string());
    assert_eq!(fmt.mime_type(), "application/octet-stream");
}

#[test]
fn test_tile_format_unknown_is_not_vector_or_raster() {
    let fmt = TileFormat::Unknown("x".to_string());
    assert!(!fmt.is_vector());
    assert!(!fmt.is_raster());
}

#[test]
fn test_tile_format_webp_mime() {
    assert_eq!(TileFormat::Webp.mime_type(), "image/webp");
}

// ── TileCoord ─────────────────────────────────────────────────────────────────

#[test]
fn test_tile_coord_equality() {
    let c1 = TileCoord { z: 5, x: 10, y: 20 };
    let c2 = TileCoord { z: 5, x: 10, y: 20 };
    let c3 = TileCoord { z: 5, x: 11, y: 20 };
    assert_eq!(c1, c2);
    assert_ne!(c1, c3);
}

#[test]
fn test_tile_coord_hash_as_map_key() {
    use std::collections::HashMap;
    let mut map: HashMap<TileCoord, &str> = HashMap::new();
    let coord = TileCoord { z: 3, x: 1, y: 2 };
    map.insert(coord.clone(), "tile_data");
    assert_eq!(map.get(&coord), Some(&"tile_data"));
}

#[test]
fn test_tile_coord_zoom_zero() {
    let c = TileCoord { z: 0, x: 0, y: 0 };
    assert_eq!(c.z, 0);
    assert_eq!(c.x, 0);
    assert_eq!(c.y, 0);
}

// ── tms_to_xyz / xyz_to_tms — extended ────────────────────────────────────────

#[test]
fn test_tms_xyz_at_zoom2() {
    // At zoom 2: 4 rows (0..3)
    // TMS y=0 (south) ↔ XYZ y=3 (south is bottom in XYZ)
    assert_eq!(tms_to_xyz(2, 0), 3);
    assert_eq!(tms_to_xyz(2, 3), 0);
    assert_eq!(xyz_to_tms(2, 0), 3);
    assert_eq!(xyz_to_tms(2, 3), 0);
}

#[test]
fn test_tms_xyz_round_trip_z5() {
    let n = 1u32 << 5;
    for y in 0..n {
        assert_eq!(
            xyz_to_tms(5, tms_to_xyz(5, y)),
            y,
            "round-trip failed at z=5, y={y}"
        );
    }
}

#[test]
fn test_tms_xyz_symmetry_property() {
    // The conversion is its own inverse
    for z in 0u8..=8 {
        let n = 1u32 << z;
        for y in 0..n {
            assert_eq!(
                tms_to_xyz(z, xyz_to_tms(z, y)),
                y,
                "symmetry failed z={z} y={y}"
            );
        }
    }
}

#[test]
fn test_tms_xyz_middle_tile() {
    // At zoom 3 (8 rows), middle tile at TMS y=3 → XYZ y=4
    assert_eq!(tms_to_xyz(3, 3), 4);
    assert_eq!(tms_to_xyz(3, 4), 3);
}

// ── MBTiles in-memory store — extended ────────────────────────────────────────

#[test]
fn test_mbtiles_overwrite_tile() {
    let mut store = MBTiles::new(MBTilesMetadata::default());
    let coord = TileCoord { z: 0, x: 0, y: 0 };
    store.insert_tile(coord.clone(), vec![1, 2, 3]);
    store.insert_tile(coord.clone(), vec![9, 8, 7]);
    let tile = store.get_tile(&coord).expect("tile exists");
    assert_eq!(tile, &vec![9u8, 8, 7]);
}

#[test]
fn test_mbtiles_get_nonexistent_tile_returns_none() {
    let store = MBTiles::new(MBTilesMetadata::default());
    let coord = TileCoord {
        z: 7,
        x: 100,
        y: 200,
    };
    assert!(store.get_tile(&coord).is_none());
}

#[test]
fn test_mbtiles_empty_store() {
    let store = MBTiles::new(MBTilesMetadata::default());
    assert_eq!(store.tile_count(), 0);
    assert!(store.zoom_levels().is_empty());
    assert!(store.tiles_at_zoom(0).is_empty());
}

#[test]
fn test_mbtiles_large_tile_payload() {
    let mut store = MBTiles::new(MBTilesMetadata::default());
    let coord = TileCoord {
        z: 14,
        x: 8192,
        y: 4096,
    };
    let payload = vec![0u8; 65536]; // 64 KiB
    store.insert_tile(coord.clone(), payload.clone());
    let retrieved = store.get_tile(&coord).expect("tile present");
    assert_eq!(retrieved.len(), 65536);
}

#[test]
fn test_mbtiles_tiles_at_zoom_filters_correctly() {
    let mut store = MBTiles::new(MBTilesMetadata::default());
    for x in 0..4u32 {
        store.insert_tile(TileCoord { z: 3, x, y: 0 }, vec![x as u8]);
    }
    store.insert_tile(TileCoord { z: 4, x: 0, y: 0 }, vec![99]);

    let zoom3 = store.tiles_at_zoom(3);
    assert_eq!(zoom3.len(), 4);
    assert_eq!(store.tiles_at_zoom(4).len(), 1);
    assert_eq!(store.tiles_at_zoom(5).len(), 0);
}

#[test]
fn test_mbtiles_zoom_levels_sorted_and_deduped() {
    let mut store = MBTiles::new(MBTilesMetadata::default());
    // Insert tiles at zoom levels 5, 3, 5, 1 (5 appears twice)
    store.insert_tile(TileCoord { z: 5, x: 0, y: 0 }, vec![]);
    store.insert_tile(TileCoord { z: 3, x: 0, y: 0 }, vec![]);
    store.insert_tile(TileCoord { z: 5, x: 1, y: 0 }, vec![]);
    store.insert_tile(TileCoord { z: 1, x: 0, y: 0 }, vec![]);
    let levels = store.zoom_levels();
    assert_eq!(levels, vec![1, 3, 5]);
}

#[test]
fn test_mbtiles_has_tile_true_after_insert() {
    let mut store = MBTiles::new(MBTilesMetadata::default());
    let coord = TileCoord {
        z: 10,
        x: 500,
        y: 300,
    };
    assert!(!store.has_tile(&coord));
    store.insert_tile(coord.clone(), vec![42]);
    assert!(store.has_tile(&coord));
}

// ── MbTilesError ──────────────────────────────────────────────────────────────

#[test]
fn test_mbtiles_error_tile_not_found_display() {
    let err = MbTilesError::TileNotFound(3, 4, 5);
    let msg = format!("{err}");
    assert!(msg.contains('3'));
    assert!(msg.contains('4'));
    assert!(msg.contains('5'));
}

#[test]
fn test_mbtiles_error_invalid_format_display() {
    let err = MbTilesError::InvalidFormat("bad sqlite".to_string());
    let msg = format!("{err}");
    assert!(msg.contains("bad sqlite"));
}
