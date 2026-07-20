# TODO: oxigdal-services

> **Purpose:** OGC Web Services (WFS 2.0.0, WCS 2.0, WPS 2.0, CSW 2.0.2) + OGC API Features/Tiles + MVT encoder for OxiGDAL.
> **Status (2026-05-16):** 11,691 LoC (src) · 578 tests (205 inline + 373 in tests/) · 3 real-code stubs
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [ ] Implement real raster read-and-window in WCS `retrieve_coverage_data` (currently returns zeroed buffer).
  - **Verified gap:** `src/wcs/coverage.rs:333` — `// For now, return placeholder data` followed by `Ok(CoverageData { data: vec![0u8; coverage.grid_size.0 * coverage.grid_size.1 * coverage.band_count], ... })`
  - **Goal:** WCS 2.0 (OGC 09-110r4) `GetCoverage` returns real pixel data — opened via OxiGDAL, windowed to the `Subset` region, in the requested CRS.
  - **Design:** Replace the placeholder branch with `oxigdal_core::Dataset::open(path)?`, derive read window from `Subset` (axis-based subsetting per 09-147r3), apply scale + sample-type conversion, fill `CoverageData`. For `CoverageSource::Url` use `oxigdal_core::ObjectStore` if scheme is `s3://`/`gs://`/`https://`; else return `ServiceError::UnsupportedFormat`. Wire `Format` parameter to existing `encode_as_geotiff/png/jpeg`.
  - **Files:** `src/wcs/coverage.rs:322-348` (replace `retrieve_coverage_data`), `src/wcs/mod.rs` (extend `CoverageInfo` with band data-type).
  - **Tests:** (proposed) `test_wcs_get_coverage_real_pixels`, `test_wcs_subset_window_clipped_to_bbox`, `test_wcs_url_source_via_object_store`, `test_wcs_unsupported_url_scheme_errors`.
  - **Risk:** GeoTIFF round-trip for floating-point bands needs sample-format tags; ensure WCS-reported band datatype matches encoder. Current `encode_as_geotiff` (`coverage.rs:365`) just wraps `Bytes::from(data.data)` — it does **not** produce a real GeoTIFF; fold that into this fix.
  - **Prerequisites:** None.

- [ ] Wire WFS `count` queries to a real database executor (currently always returns an error).
  - **Verified gap:** `src/wfs/database.rs:422` — `// This is a placeholder for actual database execution` and returns `Err(ServiceError::Internal("Database connection not configured. ..."))` unconditionally.
  - **Goal:** WFS 2.0.0 (OGC 09-025r2) `GetFeature?resultType=hits` returns an accurate `<wfs:FeatureCollection numberMatched="..."/>` driven by a SQL count against the live datastore.
  - **Design:** Inject an `Arc<dyn FeatureCountExecutor>` into `CountCache::new(...)`. Implementations: `PostGisCountExecutor` (calls `oxigdal-postgis::SpatialQuery::count`), `GeoJsonCountExecutor` (in-memory length). `execute_sql_count` becomes a dispatcher; the explicit "not configured" error stays as `NoOpExecutor` for tests.
  - **Files:** `src/wfs/database.rs:402-436`, `src/wfs/mod.rs` (re-export trait).
  - **Tests:** (proposed) `test_wfs_count_via_postgis_executor`, `test_wfs_count_cache_hit_skips_executor`, `test_wfs_count_noop_executor_returns_error`.
  - **Risk:** Cross-crate cyclic dep — keep `FeatureCountExecutor` as a trait in services; only the binary wiring imports oxigdal-postgis.
  - **Prerequisites:** Resolution of `oxigdal-postgis` writer COPY work (sibling crate) for symmetry, but not strictly blocking.

- [ ] Implement `!=` CQL operator (OGC CQL2, OGC 21-065 §A.5).
  - **Verified gap:** `src/ogc_features/cql.rs:247` — `fn build_neq_placeholder(_property: &str, _value_str: &str) -> Result<CqlExpr, FeaturesError> { Err(FeaturesError::CqlParseError("!= operator is not yet supported".to_string())) }`
  - **Goal:** `WHERE foo != 'bar'` parses to `CqlExpr::Neq { property, value }` and evaluates via existing filter engine.
  - **Design:** Add `Neq { property, value: CqlValue }` variant to `CqlExpr` (parallel to existing `Eq`), wire evaluation as `!eq`, rename `build_neq_placeholder` → `build_neq`. Also add `<>` alias per CQL2.
  - **Files:** `src/ogc_features/cql.rs:180,247` and types.rs `CqlExpr` enum.
  - **Tests:** (proposed) `test_cql_neq_string`, `test_cql_neq_number`, `test_cql_angle_bracket_alias`.
  - **Risk:** Trivial; just verify parse precedence vs `<=`/`>=` doesn't shadow `<>`.
  - **Prerequisites:** None.

- [ ] Real-content ETag generation in `tile_cache` (currently FNV-1a key hash, not content hash).
  - **Verified gap:** `src/tile_cache/cache.rs` uses cache-key hash for ETag; per RFC 7232 §2.3 an ETag must represent the response payload, not the request key. (The prior TODO line acknowledged this — keeping as verified gap.)
  - **Goal:** Strong ETag derived from BLAKE3 (or SHA-256) of the cached tile body; 304 responses driven by `If-None-Match` per RFC 9110.
  - **Design:** Add `etag: String` field to `CachedTile`. On insert, compute `blake3::hash(body).to_hex()`. ETag format: `"<algo>:<hex16>"`. `If-None-Match` handler short-circuits with 304 + `Cache-Control: max-age=...`.
  - **Files:** `src/tile_cache/cache.rs`, `src/cache_headers.rs`.
  - **Tests:** (proposed) `test_etag_changes_when_tile_changes`, `test_if_none_match_returns_304`, `test_etag_format_is_strong`.
  - **Risk:** BLAKE3 already in workspace deps (gateway uses it).
  - **Prerequisites:** None.

## Medium Priority
- [ ] OGC API Features Part 1 (17-069r4) wired to real backends (currently in-memory only via `FeatureSource`).
  - **Goal:** Plug `oxigdal-postgis::PostGisReader` and `oxigdal-geoparquet::GeoParquetReader` as `FeatureSource` impls.
  - **Files:** `src/ogc_features/server.rs`, `src/ogc_features/mod.rs`.
  - **Why deferred:** Trait surface is already correct; impls need cross-crate plumbing.

- [ ] WFS 2.0.0 Transaction (`Insert`/`Update`/`Delete`) execution against PostGIS.
  - **Files:** `src/wfs/transactions.rs` (already 525 LoC of parsing).
  - **Why deferred:** Requires authn/authz layer; v0.2.0 milestone.

- [ ] OGC API Processes Part 1 (REST WPS) alongside legacy `wps/` SOAP-style.
  - **Files:** `src/wps/` (new `rest.rs` module).
  - **Why deferred:** WPS 2.0 ExecuteRequest still serves existing clients.

- [ ] Mapbox GL Style Spec rendering (currently `src/style.rs` parses but does not rasterize).
  - **Goal:** Apply parsed style to incoming render request when serving `/styles/{id}/maps/...`.
  - **Files:** `src/style.rs:1099 lines` (already has the parser).
  - **Why deferred:** Render integration belongs in oxigdal-server, not services.

- [ ] CORS + security-headers middleware (`tower-http::cors`, `tower-http::set_header`).
  - **Files:** Add `src/middleware.rs`.
  - **Why deferred:** Production deployments typically terminate at gateway.

## Low Priority / Future (one-liners)
- [ ] WMS GetFeatureInfo proxy to underlying raster layer (WMS lives in oxigdal-server; here we'd consume it).
- [ ] OGC API Records (replaces CSW 2.0.2).
- [ ] OGC API Styles (style upload/download REST).
- [ ] OGC API EDR (Environmental Data Retrieval).
- [ ] OGC API Coverages (modern WCS replacement).
- [ ] CSW GetRecords Dublin Core + ISO 19115 output format implementation.
- [ ] WMS time dimension support across handlers.
- [ ] 3D Tiles (OGC Community Standard) glTF tile serving.
- [ ] SensorThings API for IoT geospatial.
- [ ] HTTP/2 Server Push for predicted tile prefetch.
- [ ] Tile cache invalidation on source-data update (file-watch hook).
- [ ] StarSearch / OGC API - Records full-text index.

## Cross-crate dependencies
- **Blocks:** `oxigdal-server` (consumes `ogc_tiles`, `mvt`, `TileSetMetadata`).
- **Blocked by:** `oxigdal-postgis` (database executor traits), `oxigdal-geoparquet` (real `FeatureSource` impl).

## Recently completed (verbatim)
*No prior `[x]` entries — original TODO had only open items.*

---
*Last audited: 2026-05-16*
