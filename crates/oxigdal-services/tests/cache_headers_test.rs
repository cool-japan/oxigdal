//! Tests for cache headers functionality.

use oxigdal_services::cache_headers::{
    CacheError, CacheHeaders, CachePolicy, ETag, TileCacheStrategy, VaryHeader, format_http_date,
};

// ── CachePolicy ───────────────────────────────────────────────────────────────

#[test]
fn test_policy_no_store() {
    assert_eq!(CachePolicy::NoStore.to_header_value(), "no-store");
}

#[test]
fn test_policy_no_cache() {
    assert_eq!(CachePolicy::NoCache.to_header_value(), "no-cache");
}

#[test]
fn test_policy_immutable() {
    let p = CachePolicy::Immutable {
        max_age_secs: 31_536_000,
    };
    assert_eq!(p.to_header_value(), "public, max-age=31536000, immutable");
}

#[test]
fn test_policy_immutable_small() {
    let p = CachePolicy::Immutable { max_age_secs: 600 };
    let v = p.to_header_value();
    assert!(v.contains("immutable"), "got: {v}");
    assert!(v.contains("max-age=600"), "got: {v}");
}

#[test]
fn test_policy_public_no_stale() {
    let p = CachePolicy::Public {
        max_age_secs: 3600,
        stale_while_revalidate_secs: None,
        stale_if_error_secs: None,
    };
    assert_eq!(p.to_header_value(), "public, max-age=3600");
}

#[test]
fn test_policy_public_with_swr_only() {
    let p = CachePolicy::Public {
        max_age_secs: 300,
        stale_while_revalidate_secs: Some(30),
        stale_if_error_secs: None,
    };
    let v = p.to_header_value();
    assert!(v.contains("stale-while-revalidate=30"), "got: {v}");
    assert!(!v.contains("stale-if-error"), "got: {v}");
}

#[test]
fn test_policy_public_with_sie_only() {
    let p = CachePolicy::Public {
        max_age_secs: 60,
        stale_while_revalidate_secs: None,
        stale_if_error_secs: Some(600),
    };
    let v = p.to_header_value();
    assert!(v.contains("stale-if-error=600"), "got: {v}");
    assert!(!v.contains("stale-while-revalidate"), "got: {v}");
}

#[test]
fn test_policy_public_both_stale() {
    let p = CachePolicy::Public {
        max_age_secs: 3600,
        stale_while_revalidate_secs: Some(60),
        stale_if_error_secs: Some(86400),
    };
    let v = p.to_header_value();
    assert!(v.contains("stale-while-revalidate=60"), "got: {v}");
    assert!(v.contains("stale-if-error=86400"), "got: {v}");
}

#[test]
fn test_policy_private() {
    let p = CachePolicy::Private { max_age_secs: 900 };
    assert_eq!(p.to_header_value(), "private, max-age=900");
}

#[test]
fn test_policy_private_zero() {
    let p = CachePolicy::Private { max_age_secs: 0 };
    assert_eq!(p.to_header_value(), "private, max-age=0");
}

#[test]
fn test_tile_default_contains_public() {
    let v = CachePolicy::tile_default().to_header_value();
    assert!(v.starts_with("public"), "got: {v}");
}

#[test]
fn test_tile_default_has_max_age_3600() {
    let v = CachePolicy::tile_default().to_header_value();
    assert!(v.contains("max-age=3600"), "got: {v}");
}

#[test]
fn test_tile_default_has_swr() {
    let v = CachePolicy::tile_default().to_header_value();
    assert!(v.contains("stale-while-revalidate"), "got: {v}");
}

#[test]
fn test_tile_default_has_sie() {
    let v = CachePolicy::tile_default().to_header_value();
    assert!(v.contains("stale-if-error=86400"), "got: {v}");
}

#[test]
fn test_metadata_default_max_age() {
    let v = CachePolicy::metadata_default().to_header_value();
    assert!(v.contains("max-age=300"), "got: {v}");
}

#[test]
fn test_metadata_default_swr() {
    let v = CachePolicy::metadata_default().to_header_value();
    assert!(v.contains("stale-while-revalidate=30"), "got: {v}");
}

#[test]
fn test_metadata_default_sie() {
    let v = CachePolicy::metadata_default().to_header_value();
    assert!(v.contains("stale-if-error=3600"), "got: {v}");
}

#[test]
fn test_static_asset_immutable() {
    let v = CachePolicy::static_asset().to_header_value();
    assert!(v.contains("immutable"), "got: {v}");
}

#[test]
fn test_static_asset_max_age_one_year() {
    let v = CachePolicy::static_asset().to_header_value();
    assert!(v.contains("31536000"), "got: {v}");
}

#[test]
fn test_api_response_max_age() {
    let v = CachePolicy::api_response().to_header_value();
    assert!(v.contains("max-age=60"), "got: {v}");
}

#[test]
fn test_api_response_swr() {
    let v = CachePolicy::api_response().to_header_value();
    assert!(v.contains("stale-while-revalidate=10"), "got: {v}");
}

#[test]
fn test_api_response_sie() {
    let v = CachePolicy::api_response().to_header_value();
    assert!(v.contains("stale-if-error=600"), "got: {v}");
}

// ── ETag ──────────────────────────────────────────────────────────────────────

#[test]
fn test_etag_from_bytes_deterministic() {
    let a = ETag::from_bytes(b"hello world");
    let b = ETag::from_bytes(b"hello world");
    assert_eq!(a, b);
}

#[test]
fn test_etag_from_bytes_different_inputs_differ() {
    let a = ETag::from_bytes(b"hello");
    let b = ETag::from_bytes(b"world");
    assert_ne!(a.value, b.value);
}

#[test]
fn test_etag_from_bytes_is_strong() {
    let e = ETag::from_bytes(b"data");
    assert!(!e.weak);
}

#[test]
fn test_etag_from_bytes_empty_input() {
    let e = ETag::from_bytes(b"");
    assert!(!e.weak);
    assert!(!e.value.is_empty());
}

#[test]
fn test_etag_from_bytes_value_is_hex() {
    let e = ETag::from_bytes(b"test");
    assert!(
        e.value.chars().all(|c| c.is_ascii_hexdigit()),
        "not hex: {}",
        e.value
    );
}

#[test]
fn test_etag_from_str_value() {
    let e = ETag::from_str_value("abc123");
    assert_eq!(e.value, "abc123");
    assert!(!e.weak);
}

#[test]
fn test_etag_weak_constructor() {
    let e = ETag::weak("xyz");
    assert_eq!(e.value, "xyz");
    assert!(e.weak);
}

#[test]
fn test_etag_to_header_value_strong() {
    let e = ETag::from_str_value("abc");
    assert_eq!(e.to_header_value(), "\"abc\"");
}

#[test]
fn test_etag_to_header_value_weak() {
    let e = ETag::weak("abc");
    assert_eq!(e.to_header_value(), "W/\"abc\"");
}

#[test]
fn test_etag_parse_strong() {
    let e = ETag::parse("\"hello\"").expect("parse strong");
    assert_eq!(e.value, "hello");
    assert!(!e.weak);
}

#[test]
fn test_etag_parse_weak() {
    let e = ETag::parse("W/\"hello\"").expect("parse weak");
    assert_eq!(e.value, "hello");
    assert!(e.weak);
}

#[test]
fn test_etag_parse_invalid_no_quotes() {
    assert!(matches!(
        ETag::parse("hello"),
        Err(CacheError::InvalidETag(_))
    ));
}

#[test]
fn test_etag_parse_invalid_unclosed() {
    assert!(matches!(
        ETag::parse("\"hello"),
        Err(CacheError::InvalidETag(_))
    ));
}

#[test]
fn test_etag_parse_empty_value() {
    let e = ETag::parse("\"\"").expect("empty strong etag");
    assert_eq!(e.value, "");
    assert!(!e.weak);
}

#[test]
fn test_etag_roundtrip_strong() {
    let e = ETag::from_bytes(b"tile data");
    let s = e.to_header_value();
    let parsed = ETag::parse(&s).expect("roundtrip parse");
    assert_eq!(e, parsed);
}

#[test]
fn test_etag_roundtrip_weak() {
    let e = ETag::weak("v42");
    let s = e.to_header_value();
    let parsed = ETag::parse(&s).expect("roundtrip weak parse");
    assert_eq!(e, parsed);
}

// ── VaryHeader ────────────────────────────────────────────────────────────────

#[test]
fn test_vary_accept_encoding() {
    let v = VaryHeader::accept_encoding();
    assert_eq!(v.to_header_value(), "Accept-Encoding");
}

#[test]
fn test_vary_origin_and_encoding_has_origin() {
    let v = VaryHeader::origin_and_encoding();
    assert!(v.to_header_value().contains("Origin"));
}

#[test]
fn test_vary_origin_and_encoding_has_encoding() {
    let v = VaryHeader::origin_and_encoding();
    assert!(v.to_header_value().contains("Accept-Encoding"));
}

#[test]
fn test_vary_add_builder_multiple() {
    let v = VaryHeader::new()
        .add("Accept-Encoding")
        .add("Accept-Language");
    assert_eq!(v.to_header_value(), "Accept-Encoding, Accept-Language");
}

#[test]
fn test_vary_empty() {
    let v = VaryHeader::new();
    assert_eq!(v.to_header_value(), "");
}

#[test]
fn test_vary_single_field() {
    let v = VaryHeader::new().add("Cookie");
    assert_eq!(v.to_header_value(), "Cookie");
}

// ── CacheHeaders ──────────────────────────────────────────────────────────────

#[test]
fn test_cache_headers_new_sets_cache_control() {
    let h = CacheHeaders::new(CachePolicy::NoStore);
    assert_eq!(h.cache_control, "no-store");
    assert!(h.etag.is_none());
}

#[test]
fn test_cache_headers_with_etag_sets_field() {
    let h = CacheHeaders::new(CachePolicy::NoCache).with_etag(ETag::from_str_value("v1"));
    assert_eq!(h.etag.as_deref(), Some("\"v1\""));
}

#[test]
fn test_cache_headers_is_not_modified_match() {
    let h = CacheHeaders::new(CachePolicy::NoCache).with_etag(ETag::from_str_value("abc"));
    assert!(h.is_not_modified(Some("\"abc\"")));
}

#[test]
fn test_cache_headers_is_not_modified_no_match() {
    let h = CacheHeaders::new(CachePolicy::NoCache).with_etag(ETag::from_str_value("abc"));
    assert!(!h.is_not_modified(Some("\"xyz\"")));
}

#[test]
fn test_cache_headers_is_not_modified_none_client() {
    let h = CacheHeaders::new(CachePolicy::NoCache).with_etag(ETag::from_str_value("abc"));
    assert!(!h.is_not_modified(None));
}

#[test]
fn test_cache_headers_is_not_modified_no_etag_set() {
    let h = CacheHeaders::new(CachePolicy::NoCache);
    assert!(!h.is_not_modified(Some("\"abc\"")));
}

#[test]
fn test_cache_headers_to_header_pairs_minimal() {
    let h = CacheHeaders::new(CachePolicy::NoStore);
    let pairs = h.to_header_pairs();
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].0, "Cache-Control");
    assert_eq!(pairs[0].1, "no-store");
}

#[test]
fn test_cache_headers_to_header_pairs_all_fields() {
    let h = CacheHeaders::new(CachePolicy::tile_default())
        .with_etag(ETag::from_str_value("v2"))
        .with_last_modified(0)
        .with_vary(VaryHeader::accept_encoding())
        .with_cdn_override(7200);
    let pairs = h.to_header_pairs();
    let names: Vec<&str> = pairs.iter().map(|(k, _)| k.as_str()).collect();
    assert!(names.contains(&"Cache-Control"));
    assert!(names.contains(&"ETag"));
    assert!(names.contains(&"Last-Modified"));
    assert!(names.contains(&"Vary"));
    assert!(names.contains(&"CDN-Cache-Control"));
    assert!(names.contains(&"Surrogate-Control"));
}

#[test]
fn test_cache_headers_cdn_override_cdn_cache_control() {
    let h = CacheHeaders::new(CachePolicy::NoCache).with_cdn_override(3600);
    assert_eq!(h.cdn_cache_control.as_deref(), Some("public, max-age=3600"));
}

#[test]
fn test_cache_headers_cdn_override_surrogate_control() {
    let h = CacheHeaders::new(CachePolicy::NoCache).with_cdn_override(3600);
    assert_eq!(h.surrogate_control.as_deref(), Some("max-age=3600"));
}

#[test]
fn test_cache_headers_last_modified_epoch() {
    let h = CacheHeaders::new(CachePolicy::NoCache).with_last_modified(0);
    assert_eq!(
        h.last_modified.as_deref(),
        Some("Thu, 01 Jan 1970 00:00:00 GMT")
    );
}

// ── TileCacheStrategy ─────────────────────────────────────────────────────────

#[test]
fn test_tile_strategy_zoom_0_long_ttl() {
    let s = TileCacheStrategy::standard_tile_strategy();
    let v = s.policy_for_zoom(0).to_header_value();
    assert!(v.contains("max-age=86400"), "zoom 0: {v}");
}

#[test]
fn test_tile_strategy_zoom_7_long_ttl() {
    let s = TileCacheStrategy::standard_tile_strategy();
    let v = s.policy_for_zoom(7).to_header_value();
    assert!(v.contains("max-age=86400"), "zoom 7: {v}");
}

#[test]
fn test_tile_strategy_zoom_8_medium_ttl() {
    let s = TileCacheStrategy::standard_tile_strategy();
    let v = s.policy_for_zoom(8).to_header_value();
    assert!(v.contains("max-age=3600"), "zoom 8: {v}");
}

#[test]
fn test_tile_strategy_zoom_12_medium_ttl() {
    let s = TileCacheStrategy::standard_tile_strategy();
    let v = s.policy_for_zoom(12).to_header_value();
    assert!(v.contains("max-age=3600"), "zoom 12: {v}");
}

#[test]
fn test_tile_strategy_zoom_13_short_ttl() {
    let s = TileCacheStrategy::standard_tile_strategy();
    let v = s.policy_for_zoom(13).to_header_value();
    assert!(v.contains("max-age=300"), "zoom 13: {v}");
}

#[test]
fn test_tile_strategy_zoom_16_short_ttl() {
    let s = TileCacheStrategy::standard_tile_strategy();
    let v = s.policy_for_zoom(16).to_header_value();
    assert!(v.contains("max-age=300"), "zoom 16: {v}");
}

#[test]
fn test_tile_strategy_zoom_17_no_cache() {
    let s = TileCacheStrategy::standard_tile_strategy();
    assert_eq!(s.policy_for_zoom(17).to_header_value(), "no-cache");
}

#[test]
fn test_tile_strategy_zoom_22_no_cache() {
    let s = TileCacheStrategy::standard_tile_strategy();
    assert_eq!(s.policy_for_zoom(22).to_header_value(), "no-cache");
}

#[test]
fn test_tile_strategy_zoom_25_fallback_no_cache() {
    let s = TileCacheStrategy::standard_tile_strategy();
    assert_eq!(s.policy_for_zoom(25).to_header_value(), "no-cache");
}

#[test]
fn test_tile_strategy_headers_for_tile_has_etag() {
    let s = TileCacheStrategy::standard_tile_strategy();
    let h = s.headers_for_tile(10, b"tile bytes");
    let pairs = h.to_header_pairs();
    assert!(pairs.iter().any(|(k, _)| k == "ETag"));
}

#[test]
fn test_tile_strategy_headers_for_tile_has_vary() {
    let s = TileCacheStrategy::standard_tile_strategy();
    let h = s.headers_for_tile(10, b"data");
    let pairs = h.to_header_pairs();
    assert!(pairs.iter().any(|(k, _)| k == "Vary"));
}

#[test]
fn test_tile_strategy_headers_etag_differs_by_data() {
    let s = TileCacheStrategy::standard_tile_strategy();
    let h1 = s.headers_for_tile(5, b"tile_a");
    let h2 = s.headers_for_tile(5, b"tile_b");
    assert_ne!(h1.etag, h2.etag);
}

// ── format_http_date ──────────────────────────────────────────────────────────

#[test]
fn test_format_http_date_epoch() {
    assert_eq!(format_http_date(0), "Thu, 01 Jan 1970 00:00:00 GMT");
}

#[test]
fn test_format_http_date_one_day() {
    assert_eq!(format_http_date(86400), "Fri, 02 Jan 1970 00:00:00 GMT");
}

#[test]
fn test_format_http_date_known_2021() {
    // 2021-01-01T00:00:00Z = unix 1609459200
    assert_eq!(
        format_http_date(1_609_459_200),
        "Fri, 01 Jan 2021 00:00:00 GMT"
    );
}

#[test]
fn test_format_http_date_ends_with_gmt() {
    assert!(format_http_date(1_000_000).ends_with("GMT"));
}

#[test]
fn test_format_http_date_epoch_month_is_jan() {
    let s = format_http_date(0);
    assert!(s.contains("Jan"), "got: {s}");
}

#[test]
fn test_format_http_date_time_component() {
    // Unix 3661 = 1970-01-01T01:01:01Z
    let s = format_http_date(3661);
    assert!(s.contains("01:01:01"), "got: {s}");
}

#[test]
fn test_format_http_date_midyear() {
    // 2000-06-15T00:00:00Z = unix 961027200
    // Just verify year and month are in the output
    let s = format_http_date(961_027_200);
    assert!(s.contains("2000"), "got: {s}");
    assert!(s.contains("Jun"), "got: {s}");
}
