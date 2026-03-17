//! Tests for the tile_cache module: TileKey, CachedTile, TileCache,
//! TilePrefetcher, PushHint, PushPolicy, ETagValidator, TileServer.

use oxigdal_services::tile_cache::{
    CachedTile, ETagValidator, PushHint, PushPolicy, PushRel, TileCache, TileEncoding, TileFormat,
    TileKey, TilePrefetcher, TileResponseStatus, TileServer,
};

// ── helpers ───────────────────────────────────────────────────────────────────

fn make_key(z: u8, x: u32, y: u32) -> TileKey {
    TileKey::new(z, x, y, "test", TileFormat::Mvt)
}

fn make_tile(z: u8, x: u32, y: u32, data: Vec<u8>) -> CachedTile {
    CachedTile::new(make_key(z, x, y), data, 1_000)
}

// ── TileKey tests ─────────────────────────────────────────────────────────────

#[test]
fn test_tile_key_path_string_mvt() {
    let key = TileKey::new(10, 512, 384, "roads", TileFormat::Mvt);
    assert_eq!(key.path_string(), "roads/10/512/384.mvt");
}

#[test]
fn test_tile_key_path_string_png() {
    let key = TileKey::new(5, 1, 2, "sat", TileFormat::Png);
    assert!(key.path_string().ends_with(".png"));
}

#[test]
fn test_tile_key_path_string_jpeg() {
    let key = TileKey::new(5, 1, 2, "sat", TileFormat::Jpeg);
    assert!(key.path_string().ends_with(".jpg"));
}

#[test]
fn test_tile_key_path_string_webp() {
    let key = TileKey::new(5, 1, 2, "sat", TileFormat::Webp);
    assert!(key.path_string().ends_with(".webp"));
}

#[test]
fn test_tile_key_path_string_json() {
    let key = TileKey::new(5, 1, 2, "grid", TileFormat::Json);
    assert!(key.path_string().ends_with(".json"));
}

#[test]
fn test_tile_key_content_type_mvt() {
    let key = TileKey::new(0, 0, 0, "l", TileFormat::Mvt);
    assert_eq!(key.content_type(), "application/vnd.mapbox-vector-tile");
}

#[test]
fn test_tile_key_content_type_png() {
    let key = TileKey::new(0, 0, 0, "l", TileFormat::Png);
    assert_eq!(key.content_type(), "image/png");
}

#[test]
fn test_tile_key_content_type_jpeg() {
    let key = TileKey::new(0, 0, 0, "l", TileFormat::Jpeg);
    assert_eq!(key.content_type(), "image/jpeg");
}

#[test]
fn test_tile_key_content_type_webp() {
    let key = TileKey::new(0, 0, 0, "l", TileFormat::Webp);
    assert_eq!(key.content_type(), "image/webp");
}

#[test]
fn test_tile_key_content_type_json() {
    let key = TileKey::new(0, 0, 0, "l", TileFormat::Json);
    assert_eq!(key.content_type(), "application/json");
}

#[test]
fn test_tile_key_equality() {
    let a = TileKey::new(3, 4, 5, "base", TileFormat::Mvt);
    let b = TileKey::new(3, 4, 5, "base", TileFormat::Mvt);
    assert_eq!(a, b);
}

#[test]
fn test_tile_key_hash_in_hashmap() {
    let mut map = std::collections::HashMap::new();
    let key = TileKey::new(1, 2, 3, "layer", TileFormat::Png);
    map.insert(key.clone(), 42u32);
    assert_eq!(*map.get(&key).expect("tile key should be in hashmap"), 42);
}

// ── CachedTile tests ──────────────────────────────────────────────────────────

#[test]
fn test_cached_tile_new_fields() {
    let data = vec![1u8, 2, 3, 4, 5];
    let tile = CachedTile::new(make_key(0, 0, 0), data.clone(), 500);
    assert_eq!(tile.size_bytes, data.len() as u64);
    assert_eq!(tile.access_count, 1);
    assert_eq!(tile.encoding, TileEncoding::Identity);
    assert_eq!(tile.created_at, 500);
    assert_eq!(tile.accessed_at, 500);
}

#[test]
fn test_cached_tile_etag_deterministic() {
    let data = b"hello tile".to_vec();
    let t1 = CachedTile::new(make_key(0, 0, 0), data.clone(), 0);
    let t2 = CachedTile::new(make_key(0, 0, 0), data, 0);
    assert_eq!(t1.etag, t2.etag);
}

#[test]
fn test_cached_tile_etag_changes() {
    let t1 = CachedTile::new(make_key(0, 0, 0), b"aaa".to_vec(), 0);
    let t2 = CachedTile::new(make_key(0, 0, 0), b"bbb".to_vec(), 0);
    assert_ne!(t1.etag, t2.etag);
}

#[test]
fn test_cached_tile_etag_format() {
    let tile = CachedTile::new(make_key(0, 0, 0), b"data".to_vec(), 0);
    assert!(tile.etag.starts_with('"'), "ETag should start with quote");
    assert!(tile.etag.ends_with('"'), "ETag should end with quote");
}

#[test]
fn test_cached_tile_not_stale() {
    let tile = CachedTile::new(make_key(0, 0, 0), vec![0], 100);
    assert!(!tile.is_stale(3600, 200));
}

#[test]
fn test_cached_tile_stale() {
    let tile = CachedTile::new(make_key(0, 0, 0), vec![0], 100);
    assert!(tile.is_stale(3600, 3701));
}

#[test]
fn test_cached_tile_stale_at_boundary() {
    let tile = CachedTile::new(make_key(0, 0, 0), vec![0], 100);
    // now == created_at + max_age → stale
    assert!(tile.is_stale(3600, 3700));
}

// ── TileCache tests ───────────────────────────────────────────────────────────

#[test]
fn test_tile_cache_miss() {
    let mut cache = TileCache::new(100, 1_000_000);
    let result = cache.get(&make_key(0, 0, 0), 0);
    assert!(result.is_none());
    assert_eq!(cache.miss_count, 1);
}

#[test]
fn test_tile_cache_insert_then_hit() {
    let mut cache = TileCache::new(100, 1_000_000);
    cache.insert(make_tile(1, 1, 1, vec![42]));
    let result = cache.get(&make_key(1, 1, 1), 10);
    assert!(result.is_some());
    assert_eq!(cache.hit_count, 1);
}

#[test]
fn test_tile_cache_get_updates_access() {
    let mut cache = TileCache::new(100, 1_000_000);
    cache.insert(make_tile(0, 0, 0, vec![1]));
    // First get: access_count goes from 1 → 2
    cache.get(&make_key(0, 0, 0), 10);
    // Second get: access_count goes 2 → 3
    let tile = cache
        .get(&make_key(0, 0, 0), 20)
        .expect("cached tile should exist");
    assert_eq!(tile.access_count, 3);
}

#[test]
fn test_tile_cache_get_updates_accessed_at() {
    let mut cache = TileCache::new(100, 1_000_000);
    cache.insert(make_tile(0, 0, 0, vec![1]));
    cache.get(&make_key(0, 0, 0), 9999);
    let tile = cache
        .get(&make_key(0, 0, 0), 9999)
        .expect("cached tile should exist");
    assert_eq!(tile.accessed_at, 9999);
}

#[test]
fn test_tile_cache_hit_rate_zero() {
    let cache = TileCache::new(100, 1_000_000);
    assert_eq!(cache.hit_rate(), 0.0);
}

#[test]
fn test_tile_cache_hit_rate_calculation() {
    let mut cache = TileCache::new(100, 1_000_000);
    cache.insert(make_tile(0, 0, 0, vec![1]));
    // 3 hits
    cache.get(&make_key(0, 0, 0), 1);
    cache.get(&make_key(0, 0, 0), 2);
    cache.get(&make_key(0, 0, 0), 3);
    // 1 miss
    cache.get(&make_key(9, 9, 9), 4);
    assert!((cache.hit_rate() - 0.75).abs() < 1e-9);
}

#[test]
fn test_tile_cache_invalidate_existing() {
    let mut cache = TileCache::new(100, 1_000_000);
    cache.insert(make_tile(0, 0, 0, vec![1]));
    assert!(cache.invalidate(&make_key(0, 0, 0)));
}

#[test]
fn test_tile_cache_invalidate_missing() {
    let mut cache = TileCache::new(100, 1_000_000);
    assert!(!cache.invalidate(&make_key(0, 0, 0)));
}

#[test]
fn test_tile_cache_invalidate_reduces_bytes() {
    let mut cache = TileCache::new(100, 1_000_000);
    cache.insert(make_tile(0, 0, 0, vec![1; 64]));
    let before = cache.current_bytes;
    cache.invalidate(&make_key(0, 0, 0));
    assert!(cache.current_bytes < before);
    assert_eq!(cache.current_bytes, 0);
}

#[test]
fn test_tile_cache_invalidate_layer() {
    let mut cache = TileCache::new(100, 1_000_000);
    cache.insert(CachedTile::new(
        TileKey::new(5, 0, 0, "roads", TileFormat::Mvt),
        vec![1],
        0,
    ));
    cache.insert(CachedTile::new(
        TileKey::new(5, 1, 0, "roads", TileFormat::Mvt),
        vec![2],
        0,
    ));
    cache.insert(CachedTile::new(
        TileKey::new(5, 0, 0, "water", TileFormat::Mvt),
        vec![3],
        0,
    ));
    let removed = cache.invalidate_layer("roads");
    assert_eq!(removed, 2);
    assert_eq!(cache.stats().entry_count, 1);
}

#[test]
fn test_tile_cache_invalidate_zoom_range() {
    let mut cache = TileCache::new(100, 1_000_000);
    for z in [5u8, 6, 7] {
        cache.insert(CachedTile::new(
            TileKey::new(z, 0, 0, "l", TileFormat::Mvt),
            vec![z],
            0,
        ));
    }
    let removed = cache.invalidate_zoom_range(5, 6);
    assert_eq!(removed, 2);
    assert_eq!(cache.stats().entry_count, 1);
}

#[test]
fn test_tile_cache_evict_lru_on_max_entries() {
    let mut cache = TileCache::new(2, 1_000_000);
    cache.insert(make_tile(0, 0, 0, vec![1]));
    cache.insert(make_tile(0, 0, 1, vec![2]));
    cache.insert(make_tile(0, 0, 2, vec![3])); // triggers eviction of (0,0,0)
    assert_eq!(cache.eviction_count, 1);
    assert!(cache.get(&make_key(0, 0, 0), 0).is_none());
    assert!(cache.get(&make_key(0, 0, 1), 0).is_some());
    assert!(cache.get(&make_key(0, 0, 2), 0).is_some());
}

#[test]
fn test_tile_cache_evict_on_byte_budget() {
    // max_bytes=100; first tile is 60 bytes, second tile is 60 bytes → first evicted
    let mut cache = TileCache::new(1000, 100);
    cache.insert(make_tile(0, 0, 0, vec![0u8; 60]));
    cache.insert(make_tile(0, 0, 1, vec![0u8; 60]));
    assert_eq!(cache.eviction_count, 1);
    assert!(
        cache.get(&make_key(0, 0, 0), 0).is_none(),
        "first tile should be evicted"
    );
    assert!(cache.get(&make_key(0, 0, 1), 0).is_some());
}

#[test]
fn test_tile_cache_stats() {
    let mut cache = TileCache::new(100, 1_000_000);
    cache.insert(make_tile(0, 0, 0, vec![1; 32]));
    cache.get(&make_key(0, 0, 0), 1);
    let stats = cache.stats();
    assert_eq!(stats.entry_count, 1);
    assert_eq!(stats.total_bytes, 32);
    assert_eq!(stats.hit_count, 1);
}

// ── TilePrefetcher tests ──────────────────────────────────────────────────────

#[test]
fn test_prefetcher_neighbors_radius1_count() {
    let pf = TilePrefetcher::new(1);
    let key = TileKey::new(5, 10, 10, "l", TileFormat::Mvt);
    let neighbors = pf.neighbors(&key);
    // At same zoom: 8 (3x3 - 1); plus up to zoom ±1 tiles (may add more)
    // At minimum we expect exactly 8 same-zoom neighbors when away from boundary
    let same_zoom: Vec<_> = neighbors.iter().filter(|t| t.z == 5).collect();
    assert_eq!(
        same_zoom.len(),
        8,
        "Should have 8 same-zoom neighbors for radius=1"
    );
}

#[test]
fn test_prefetcher_neighbors_no_self() {
    let pf = TilePrefetcher::new(1);
    let key = TileKey::new(5, 10, 10, "l", TileFormat::Mvt);
    let neighbors = pf.neighbors(&key);
    assert!(
        !neighbors.contains(&key),
        "Result should not contain the key itself"
    );
}

#[test]
fn test_prefetcher_neighbors_boundary_x0_y0() {
    let pf = TilePrefetcher::new(1);
    let key = TileKey::new(5, 0, 0, "l", TileFormat::Mvt);
    // Should not panic; all x, y must be >= 0
    let neighbors = pf.neighbors(&key);
    for n in &neighbors {
        // u32 is always >= 0; just ensure no overflow
        let _ = n.x;
        let _ = n.y;
    }
    assert!(!neighbors.is_empty());
}

#[test]
fn test_prefetcher_ring_at_zoom() {
    let pf = TilePrefetcher::new(1);
    let key = TileKey::new(5, 10, 10, "myLayer", TileFormat::Png);
    let ring = pf.ring_at_zoom(&key, 5, 1);
    // 3x3 = 9 tiles
    assert_eq!(ring.len(), 9);
    for t in &ring {
        assert_eq!(t.layer, "myLayer");
        assert_eq!(t.format, TileFormat::Png);
    }
}

#[test]
fn test_prefetcher_radius2_more_neighbors() {
    let pf1 = TilePrefetcher::new(1);
    let pf2 = TilePrefetcher::new(2);
    let key = TileKey::new(5, 20, 20, "l", TileFormat::Mvt);
    let n1 = pf1.neighbors(&key);
    let n2 = pf2.neighbors(&key);
    assert!(
        n2.len() > n1.len(),
        "radius=2 should have more neighbors than radius=1"
    );
}

// ── PushHint tests ────────────────────────────────────────────────────────────

#[test]
fn test_push_hint_to_link_header_preload() {
    let hint = PushHint::new("/tiles/test/10/1/2.mvt", PushRel::Preload);
    let header = hint.to_link_header();
    assert!(header.contains("rel=preload"));
    assert!(header.contains("</tiles/test/10/1/2.mvt>"));
}

#[test]
fn test_push_hint_to_link_header_with_as() {
    let mut hint = PushHint::new("/tile.png", PushRel::Preload);
    hint.as_ = Some("image".to_owned());
    let header = hint.to_link_header();
    assert!(header.contains("; as=image"));
}

#[test]
fn test_push_hint_to_link_header_with_type() {
    let mut hint = PushHint::new("/tile.png", PushRel::Preload);
    hint.type_ = Some("image/png".to_owned());
    let header = hint.to_link_header();
    assert!(header.contains("; type=\"image/png\""));
}

#[test]
fn test_push_hint_preload_tile_mvt() {
    let hint = PushHint::preload_tile("/t.mvt", &TileFormat::Mvt);
    assert_eq!(hint.as_.as_deref(), Some("fetch"));
    assert_eq!(hint.rel, PushRel::Preload);
}

#[test]
fn test_push_hint_preload_tile_png() {
    let hint = PushHint::preload_tile("/t.png", &TileFormat::Png);
    assert_eq!(hint.as_.as_deref(), Some("image"));
}

#[test]
fn test_push_hint_nopush_flag() {
    let mut hint = PushHint::new("/t.mvt", PushRel::Preload);
    hint.nopush = true;
    let header = hint.to_link_header();
    assert!(header.contains("; nopush"));
}

// ── PushPolicy tests ──────────────────────────────────────────────────────────

#[test]
fn test_push_policy_generate_hints_count() {
    let policy = PushPolicy::new("https://tiles.example.com");
    let key = TileKey::new(10, 512, 384, "roads", TileFormat::Mvt);
    let hints = policy.generate_hints(&key);
    assert!(hints.len() <= policy.max_push_count as usize);
}

#[test]
fn test_push_policy_to_link_header_value() {
    let hints = vec![
        PushHint::new("/a.mvt", PushRel::Preload),
        PushHint::new("/b.mvt", PushRel::Preload),
    ];
    let value = PushPolicy::to_link_header_value(&hints);
    assert!(value.contains(", "), "Should be comma-separated");
    assert!(value.contains("/a.mvt"));
    assert!(value.contains("/b.mvt"));
}

#[test]
fn test_push_policy_parse_tile_url_roundtrip() {
    let base = "https://tiles.example.com";
    let key = TileKey::new(10, 512, 384, "roads", TileFormat::Mvt);
    let url = format!("{}/{}", base, key.path_string());
    let parsed = PushPolicy::parse_tile_url(&url, base);
    assert_eq!(parsed, Some(key));
}

#[test]
fn test_push_policy_parse_tile_url_invalid() {
    let result = PushPolicy::parse_tile_url("not-a-tile-url", "https://example.com");
    assert!(result.is_none());
}

// ── ETagValidator tests ───────────────────────────────────────────────────────

#[test]
fn test_etag_check_none_match_no_match() {
    // ETag not in list → true (send full response)
    let result = ETagValidator::check_none_match("\"other\"", "\"abc123\"");
    assert!(result, "Should return true when etag is not in the list");
}

#[test]
fn test_etag_check_none_match_match() {
    // ETag in list → false (304)
    let result = ETagValidator::check_none_match("\"abc123\"", "\"abc123\"");
    assert!(!result, "Should return false (304) when etag matches");
}

#[test]
fn test_etag_check_none_match_wildcard() {
    // Wildcard → false (304)
    let result = ETagValidator::check_none_match("*", "\"abc123\"");
    assert!(!result, "Wildcard should match everything (304)");
}

#[test]
fn test_etag_check_match_found() {
    let result = ETagValidator::check_match("\"abc123\"", "\"abc123\"");
    assert!(result);
}

#[test]
fn test_etag_check_match_wildcard() {
    let result = ETagValidator::check_match("*", "\"anything\"");
    assert!(result);
}

#[test]
fn test_etag_parse_etag_list_single() {
    let list = ETagValidator::parse_etag_list("\"abc\"");
    assert_eq!(list, vec!["\"abc\""]);
}

#[test]
fn test_etag_parse_etag_list_multiple() {
    let list = ETagValidator::parse_etag_list("\"a\", \"b\"");
    assert_eq!(list.len(), 2);
    assert!(list.contains(&"\"a\"".to_owned()));
    assert!(list.contains(&"\"b\"".to_owned()));
}

#[test]
fn test_etag_is_weak_true() {
    assert!(ETagValidator::is_weak("W/\"abc\""));
}

#[test]
fn test_etag_is_weak_false() {
    assert!(!ETagValidator::is_weak("\"abc\""));
}

// ── TileServer tests ──────────────────────────────────────────────────────────

#[test]
fn test_tile_server_serve_miss() {
    let mut server = TileServer::new("https://tiles.example.com");
    let key = make_key(5, 1, 1);
    let response = server.serve(&key, None, 1000);
    assert_eq!(response.status, TileResponseStatus::NotFound);
    assert!(response.data.is_none());
}

#[test]
fn test_tile_server_cache_then_serve_ok() {
    let mut server = TileServer::new("https://tiles.example.com");
    let key = make_key(5, 1, 1);
    let data = vec![10u8, 20, 30];
    server.cache_tile(key.clone(), data.clone(), 1000);
    let response = server.serve(&key, None, 1001);
    assert_eq!(response.status, TileResponseStatus::Ok);
    assert_eq!(response.data, Some(data));
}

#[test]
fn test_tile_server_serve_not_modified() {
    let mut server = TileServer::new("https://tiles.example.com");
    let key = make_key(5, 1, 1);
    server.cache_tile(key.clone(), vec![1, 2, 3], 1000);
    // Get the ETag first
    let resp_ok = server.serve(&key, None, 1001);
    let etag = resp_ok
        .headers
        .iter()
        .find(|(k, _)| k == "ETag")
        .map(|(_, v)| v.clone())
        .expect("ETag header should be present");
    // Now serve with matching If-None-Match
    let response = server.serve(&key, Some(&etag), 1002);
    assert_eq!(response.status, TileResponseStatus::NotModified);
    assert!(response.data.is_none());
}

#[test]
fn test_tile_server_serve_headers_present() {
    let mut server = TileServer::new("https://tiles.example.com");
    let key = make_key(5, 1, 1);
    server.cache_tile(key.clone(), vec![0], 1000);
    let response = server.serve(&key, None, 1001);
    assert_eq!(response.status, TileResponseStatus::Ok);
    let header_names: Vec<&str> = response.headers.iter().map(|(k, _)| k.as_str()).collect();
    assert!(
        header_names.contains(&"Cache-Control"),
        "Missing Cache-Control"
    );
    assert!(header_names.contains(&"ETag"), "Missing ETag");
    assert!(
        header_names.contains(&"Content-Type"),
        "Missing Content-Type"
    );
}

#[test]
fn test_tile_server_serve_push_hints() {
    let mut server = TileServer::new("https://tiles.example.com");
    let key = TileKey::new(10, 512, 384, "roads", TileFormat::Mvt);
    server.cache_tile(key.clone(), vec![1, 2, 3], 1000);
    let response = server.serve(&key, None, 1001);
    assert_eq!(response.status, TileResponseStatus::Ok);
    assert!(
        !response.push_hints.is_empty(),
        "Should have push hints for neighbouring tiles"
    );
}

#[test]
fn test_tile_server_cache_stats() {
    let mut server = TileServer::new("https://tiles.example.com");
    server.cache_tile(make_key(1, 0, 0), vec![0], 0);
    let stats = server.cache_stats();
    assert_eq!(stats.entry_count, 1);
}
