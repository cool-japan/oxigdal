# TODO: oxigeo-services

> **Purpose:** OGC Web Services (WFS 2.0.0, WCS 2.0, WPS 2.0, CSW 2.0.2) + OGC API Features/Tiles + MVT encoder for OxiGeo.
> **Status (2026-07-28):** 11,965 LoC (src) · 659 tests · 0 real-code stubs
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [x] Implement real raster read-and-window in WCS `retrieve_coverage_data` (currently returns zeroed buffer).
  - **Done (verified 2026-07-28):** `retrieve_coverage_data` (`src/wcs/coverage.rs:428-466`) now dispatches on `coverage.source`: `CoverageSource::File` opens via `FileDataSource::open` + `decode_geotiff`; `CoverageSource::Url` fetches real bytes via `fetch_url_bytes(url).await?` then decodes; `CoverageSource::Memory` decodes the in-memory buffer directly (erroring if empty) — no more zeroed placeholder. `encode_as_geotiff` (`coverage.rs:567-584`) was also fixed as flagged in the original risk note: it now calls `write_geotiff_bytes(&data, coverage)` for a real GeoTIFF, not a raw `Bytes::from(data.data)` wrap.
  - **Original goal (for reference):** WCS 2.0 (OGC 09-110r4) `GetCoverage` returns real pixel data — opened via OxiGeo, windowed to the `Subset` region, in the requested CRS.

- [x] Wire WFS `count` queries to a real database executor (currently always returns an error).
  - **Done (verified 2026-07-28):** `execute_sql_count` (`src/wfs/database.rs:366-394`) is now `cfg`-dispatched rather than an unconditional error: with the `postgis` feature (`Cargo.toml:20`, Pure Rust — `oxigeo-postgis`/`tokio-postgres`/`deadpool-postgres`) it opens a real `oxigeo_postgis::ConnectionPool`, runs `client.query_one(sql, &[])`, and returns the `BIGINT` count; without the feature it returns a descriptive `ServiceError::Internal` ("PostGIS support is not compiled in...") instead of the old unconditional "Database connection not configured" message. Implemented as an inline `cfg`-gated dispatcher rather than the originally-sketched injectable `FeatureCountExecutor` trait, but the functional goal (real DB-backed count when configured, honest error otherwise) is met.
  - **Original goal (for reference):** WFS 2.0.0 (OGC 09-025r2) `GetFeature?resultType=hits` returns an accurate `<wfs:FeatureCollection numberMatched="..."/>` driven by a SQL count against the live datastore.

- [x] Implement `!=` CQL operator (OGC CQL2, OGC 21-065 §A.5).
  - **Done (verified 2026-07-28):** `src/ogc_features/cql.rs` now has `CqlExpr::Neq { property, value }` (line 27), a real `build_neq` (line 327, no longer `build_neq_placeholder`) registered for both `!=` and the `<>` alias (line 259-260), and evaluated in the filter match arm (line 494).
  - **Original goal (for reference):** `WHERE foo != 'bar'` parses to `CqlExpr::Neq { property, value }` and evaluates via existing filter engine.

- [x] Real-content ETag generation in `tile_cache` (currently FNV-1a key hash, not content hash).
  - **Done (verified 2026-07-28):** `CachedTile::new` (`src/tile_cache/cache.rs:137-144`) now computes `etag` via `compute_etag(&data)` over the actual tile **body** bytes, not the cache key — the RFC 7232 §2.3 correctness gap (ETag must represent the payload) is closed. `CacheHeaders::is_not_modified(if_none_match)` (`src/cache_headers.rs:348-356`) implements the conditional-GET comparison for a 304 response path.
  - **Algorithm note:** the hash is FNV-1a (fast, deterministic per exact byte content), not the BLAKE3/SHA-256 originally sketched in this item's design. FNV-1a still yields a content-derived, change-detecting ETag; upgrading to a cryptographic hash remains a legitimate future hardening step but is no longer a functional gap.
  - **Original goal (for reference):** Strong ETag derived from a content hash of the cached tile body; 304 responses driven by `If-None-Match` per RFC 9110.

## Medium Priority
- [ ] OGC API Features Part 1 (17-069r4) wired to real backends (currently in-memory only via `FeatureSource`).
  - **Goal:** Plug `oxigeo-postgis::PostGisReader` and `oxigeo-geoparquet::GeoParquetReader` as `FeatureSource` impls.
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
  - **Why deferred:** Render integration belongs in oxigeo-server, not services.

- [ ] CORS + security-headers middleware (`tower-http::cors`, `tower-http::set_header`).
  - **Files:** Add `src/middleware.rs`.
  - **Why deferred:** Production deployments typically terminate at gateway.

## Low Priority / Future (one-liners)
- [ ] WMS GetFeatureInfo proxy to underlying raster layer (WMS lives in oxigeo-server; here we'd consume it).
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
- **Blocks:** `oxigeo-server` (consumes `ogc_tiles`, `mvt`, `TileSetMetadata`).
- **Blocked by:** `oxigeo-postgis` (database executor traits), `oxigeo-geoparquet` (real `FeatureSource` impl).

## Recently completed (verbatim)
*No prior `[x]` entries — original TODO had only open items.*

---
*Last audited: 2026-07-28*
