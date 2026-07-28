# TODO: oxigeo-stac

> **Purpose:** STAC (SpatioTemporal Asset Catalog) 1.0.0 support for OxiGeo — Pure Rust cloud-native geospatial catalog.
> **Status (2026-07-28):** ~8,753 Rust LoC · 415 tests (all-features; 369 with default-features only) · 0 in-source `TODO:` markers.
> **Roadmap:** v0.1.7 → v0.2.0 (current slice) → v1.0.0

## High Priority (next slice — verified gaps)

- [x] HTTP-backed Transaction Extension — `create_item`/`update_item`/`upsert_item`/`delete_item` (completed 2026-05-16)
  - **Done:** Four async methods on `StacClient` (gated on `reqwest+async` features). `HttpTransactionResult { status, location }` struct. HTTP status code mapping (404→NotFound, 409→AlreadyExists, 5xx→ApiResponse). `upsert_item` automatically falls back to `update_item` on 409. `ConformanceMissing(String)` error variant added to `StacError`.
  - **Tests added (14):** create 200/201/404/409/500, update 200/204/404, delete 200/204/404, upsert create/fallback/missing-id. Mock server via `std::net::TcpListener` + `std::thread::spawn`.
  - **Files:** `src/search.rs`, `src/error.rs`, `src/lib.rs`, `tests/transaction_http_test.rs` (new).

- [x] STAC 1.1.0 specification compatibility (completed 2026-05-17)
  - **Done:** New `src/version.rs` — `pub enum StacVersion { V1_0_0, V1_1_0 }` with `as_str() -> &'static str`, `parse(s: &str) -> Result<Self, StacError>` (accepts `"1.0.0"` and `"1.1.0"`, rejects others with `InvalidVersion`), `impl Default` returning `V1_0_0`, `impl Display`, `impl TryFrom<&str> / TryFrom<String>`, `impl From<StacVersion> for String`. Serde via `#[serde(try_from = "String", into = "String")]`. `src/lib.rs`: `STAC_VERSION` const deprecated in favour of `StacVersion::default().as_str()`; `StacVersion` re-exported. `src/collection.rs`: version check relaxed from `!= "1.0.0"` to `StacVersion::parse(...).is_err()`; added `pub assets: Option<HashMap<String, Asset>>` with `#[serde(skip_serializing_if = "Option::is_none")]`. `src/item.rs`: same version check relaxation. `src/asset.rs`: added `pub bands: Option<Vec<Band>>` with `#[serde(skip_serializing_if = "Option::is_none")]`, plus `with_bands()` and `add_band()` builder methods.
  - **Tests added (15):** test_stac_version_parse_accepts_1_0_0_and_1_1_0, test_stac_version_parse_rejects_2_0_0, test_collection_v1_1_0_with_assets_parses_successfully, test_item_v1_1_0_validates_successfully, test_asset_with_bands_shorthand_round_trips, test_collection_assets_optional_field_omitted_in_v1_0_0_output, test_stac_version_default_is_v1_0_0_for_backward_compat, test_stac_version_display, test_stac_version_serde_roundtrip, test_stac_version_try_from_string, test_item_v1_0_0_still_validates, test_collection_missing_assets_parses_successfully, test_asset_bands_none_by_default, test_stac_version_as_str, test_stac_version_invalid_rejected_in_serde. Total: 373 tests pass.

- [x] Lazy pagination stream (completed 2026-05-17)
  - **Done:** `Paginator::stream(self) -> impl Stream<Item = Result<Item>>` added to `src/pagination.rs` (gated `#[cfg(feature = "async")]`). Uses `futures::stream::unfold` with state `(Paginator, Vec<Item>, usize)` — buffer is a full page; items are yielded one at a time; next page is fetched lazily when the buffer is exhausted. Stream terminates when `next_page()` returns `None`. `collect_all` is unchanged. `futures` added to `[dev-dependencies]` for test StreamExt usage.
  - **Tests added (8):** test_stream_single_page_yields_all_features, test_stream_two_pages_yields_all_features, test_stream_terminates_when_no_next_link, test_stream_error_propagates_as_err_item, test_stream_skips_empty_pages_and_continues, test_stream_multiple_pages_ordered, test_stream_take_stops_early, test_stream_empty_single_page_yields_nothing. Total: 380 tests pass.

- [x] Conformance-class auto-detection from landing page (completed 2026-05-16)
  - **Verified gap:** `src/client/conformance.rs` (5.1 KB) defines `ConformanceDeclaration` but `StacClient::new` (search.rs:39-51) never fetches `/` to read it; the client behaves identically against any endpoint, even when CQL2, Sort, or Transaction support is missing server-side.
  - **Done (2026-05-16):** `StacClient::with_conformance(self) -> Result<Self>` fetches `{base_url}/`, parses `conformsTo` array, caches in `Arc<Mutex<Option<HashSet<String>>>>`. `supports(&self, uri) -> bool` advisory check. Tolerates all network/HTTP/JSON failures. `ConformanceMissing(String)` error variant added.
  - **Tests added (9):** caching, true/false lookups, case-sensitivity, 404/malformed-json/connection-refused tolerance.
  - **Files:** `src/search.rs`, `src/error.rs`, `tests/conformance_test.rs` (new).

## Medium Priority (planned — design sketched)

- [ ] Bulk item ingest with batch validation and per-item error reporting
  - **Goal:** `Vec<Item> -> BulkResult { succeeded: usize, failed: Vec<(usize, StacError)> }`; useful for catalog migrations.
  - **Files:** `(new) src/bulk_ingest.rs`.
  - **Why deferred:** Awaits the HTTP Transaction client.

- [ ] STAC Filter Extension — full CQL2 with spatial/temporal/array ops
  - **Goal:** Wire `cql2::Cql2Filter` (which already enumerates `s_intersects`, `s_contains`, `s_within`) through the search request body and validate before send.
  - **Files:** `src/cql2.rs` (extend tests), `src/search.rs` (add `.filter_cql2(filter)`).
  - **Why deferred:** Server-side coverage varies — wait for conformance auto-detect.

- [ ] Cross-collection search with deduplication
  - **Goal:** Single `client.cross_collection_search(query)` fan-out + merge-sort by `datetime`.
  - **Files:** `(new) src/multi_collection.rs`.
  - **Why deferred:** Multi-tenancy ordering corner cases.

- [ ] Pointcloud, Raster, and Label STAC extensions
  - **Goal:** Typed struct fields parallel to the existing `eo`, `projection`, `sar`, `scientific`, `timestamps`, `version`, `view` modules.
  - **Files:** `(new) src/extensions/pointcloud.rs`, `(new) src/extensions/raster.rs`, `(new) src/extensions/label.rs`.
  - **Why deferred:** Each extension brings nuanced field semantics.

- [ ] Collection-level asset management (`Collection.assets`)
  - **Goal:** Pair with the 1.1.0 spec work above — separate slice once that lands.
  - **Files:** `src/collection.rs`.
  - **Why deferred:** Folded into 1.1.0 milestone.

- [ ] Authentication for private STAC APIs (Bearer / API-key / OAuth2 client credentials)
  - **Goal:** `StacClient::with_bearer(token)` / `with_api_key(header, value)` builders.
  - **Files:** modify `src/search.rs`.
  - **Why deferred:** Auth design needs cross-team review (token refresh semantics).

- [ ] STAC Sorting Extension with multi-field sort directives
  - **Goal:** `.sort_by([SortField::Field("datetime", Desc), SortField::Field("cloud_cover", Asc)])`.
  - **Files:** `src/api/request.rs` (already has `SortField` — wire to query).
  - **Why deferred:** Awaits conformance probe.

## Low Priority / Future (speculative — one-liners only)

- [ ] STAC ↔ GeoParquet conversion (round-trip via `oxigeo-geoparquet`)
- [ ] Local file-based static catalog generator
- [ ] STAC catalog crawler / harvester
- [ ] STAC item change detection (semantic diff between versions)
- [ ] Query-cost estimator from spatial / temporal extent
- [ ] Item thumbnail generation from COG assets
- [ ] JSON-Schema validation of catalog documents
- [ ] STAC Aggregation Extension (date histogram, geohash grid) — `aggregation.rs` already has scaffolding

## Cross-crate dependencies
- **Blocks:** `oxigeo-oxigeo` (umbrella STAC streaming), `oxigeo-services` (catalog endpoints)
- **Blocked by:** None — all High Priority items are internal

## Recently completed (kept verbatim)
- [x] Wire StacClient to perform real HTTP requests against STAC API endpoints (verified 2026-05-16: `src/search.rs:75` posts to `{base_url}/search` via `reqwest::Client`)
- [x] Implement STAC API item download (fetch asset bytes via href) (verified 2026-05-16: `StacClient::get_item` at `src/search.rs:101-119`)
- [x] Add CQL2-JSON filter support (verified 2026-05-16: `src/cql2.rs` `Cql2Filter` enum with `#[serde(tag = "op")]`, full spatial/temporal operator coverage)

---
*Last audited: 2026-07-28*
