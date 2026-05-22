# TODO: oxigdal-server

> **Purpose:** WMS 1.3.0 / WMTS 1.0.0 / XYZ tile server for OxiGDAL rasters, built on axum + hyper.
> **Status (2026-05-16):** 7,168 LoC (src) · 66 tests (51 inline + 15 in tests/) · 1 real-code stub
> **Roadmap:** v0.1.5 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [ ] Wire `stats_handler` to the shared `TileCache` instance via Axum `State`.
  - **Verified gap:** `src/server.rs:360` — `// This is a placeholder - in a real implementation, we'd need to access the cache through shared state`
  - **Goal:** `GET /stats` returns real `CacheStats { hits, misses, evictions, size_bytes, entry_count }` JSON from the live tile cache, not the canned `"Cache statistics endpoint - requires state injection"` message.
  - **Design:** `stats_handler` already exists in `server.rs`. Replace `async fn stats_handler()` with `async fn stats_handler(State(cache): State<Arc<TileCache>>) -> Response`. Threading `TileCache` into `TileServer::serve()` Router via `.with_state(...)`. `CacheStats` derive `Serialize`.
  - **Files:** `src/server.rs`, `src/cache.rs` (verify `CacheStats: Serialize`).
  - **Tests:** (proposed) `test_stats_handler_returns_real_counters`, `test_stats_handler_after_cache_hit_miss`.
  - **Risk:** Lock contention on hot stats path — `TileCache` already uses `parking_lot::Mutex`; expose `stats()` returning a snapshot, not a guard.
  - **Prerequisites:** None.

- [ ] Implement WMTS `TileMatrixSetLimits` advertisement in `GetCapabilities` XML.
  - **Verified gap:** `src/handlers/wmts.rs` exposes `get_tile_matrix_set()` and validates against `matrix.matrix_width/matrix_height` at runtime (`src/handlers/wmts.rs:778`) but the generated capabilities XML does not include `<TileMatrixSetLimits>` per WMTS 1.0.0 (07-057r7) §B.5 / 7.1.1.3.
  - **Goal:** Per-layer `<TileMatrixSetLimits>` elements emitted by the WMTS `GetCapabilities` handler, computed from dataset bbox + matrix set, so clients fetch only valid (z,x,y) triples.
  - **Design:** For each `LayerInfo`, project the dataset bbox into the matrix set CRS, walk all `TileMatrix` levels, compute `(MinTileRow, MaxTileRow, MinTileCol, MaxTileCol)` using `tile_to_bbox` inverse. Emit XML inside `<Layer>/<TileMatrixSetLink>/<TileMatrixSetLimits>` (07-057r7 Annex B.5.3 schema).
  - **Files:** `src/handlers/wmts.rs` (extend capabilities writer near line ~330 where `<Layer>` is written).
  - **Tests:** (proposed) `test_wmts_capabilities_includes_tilematrix_set_limits`, `test_wmts_limits_clip_layer_bbox`, `test_wmts_get_tile_outside_limits_returns_404`.
  - **Risk:** Bbox-in-non-Mercator matrix sets needs reprojection; route through `oxigdal-proj` rather than hand-rolled lon/lat math.
  - **Prerequisites:** None.

- [ ] Add OGC API - Tiles (OGC 20-057) endpoints (`/tiles`, `/tileMatrixSets`, `/collections/{id}/tiles/...`).
  - **Goal:** Modern REST-style tile API per OGC 20-057 (with JSON `TileSetMetadata`, conformance declaration), coexisting with legacy WMTS.
  - **Design:** Reuse `oxigdal-services::ogc_tiles::TileSetMetadata` types (already implemented in services crate). Define new module `src/handlers/ogc_tiles.rs` with handlers for `/tiles`, `/tileMatrixSets/{tmsId}`, `/collections/{collectionId}/map/tiles/{tmsId}/{z}/{x}/{y}`. Conformance class URIs per 20-057 §A.
  - **Files:** `src/handlers/ogc_tiles.rs` (new), `src/handlers/mod.rs` (declare), `src/server.rs` (router merge).
  - **Tests:** (proposed) `test_ogc_tiles_root_lists_tilesets`, `test_ogc_tiles_get_tile_png`, `test_conformance_declares_core_class`.
  - **Risk:** Spec overlap with WMTS — keep routes disjoint (`/wmts/*` vs `/tiles/*`).
  - **Prerequisites:** None (depends on already-existing types in `oxigdal-services`).

## Medium Priority
- [ ] Disk cache tier for large tile sets (currently `TileCache` is memory-only LRU).
  - **Goal:** Two-tier cache (RAM LRU → disk LRU) with sharded directory layout.
  - **Files:** `src/cache.rs` (add `DiskTier`), `src/config.rs` (`CacheConfig.disk_path`).
  - **Why deferred:** Memory cache covers low-traffic scenarios; disk tier needed when working set exceeds RAM. Requires evicting-to-disk policy spec.

- [ ] Graceful shutdown with in-flight request draining (`tokio::signal::ctrl_c` + `axum::serve::with_graceful_shutdown`).
  - **Goal:** SIGINT/SIGTERM triggers stop-accept + drain with configurable deadline; current `loop { listener.accept() }` cannot be torn down cleanly.
  - **Files:** `src/server.rs`.
  - **Why deferred:** Cleanly bundled with k8s probe work below.

- [ ] `/health` (liveness) and `/ready` (readiness checks all configured datasets) endpoints.
  - **Goal:** Kubernetes-style probes; `/ready` returns 503 until `DatasetRegistry::is_loaded()` true for all layers.
  - **Files:** `src/server.rs`, `src/dataset_registry.rs` (expose readiness API).
  - **Why deferred:** Useful but not blocking deployments; current `/health` exists as fixed 200.

- [ ] MVT vector tile serving from GeoJSON/GeoPackage layers (`oxigdal-services::mvt` integration).
  - **Goal:** `/tiles/{layer}/{z}/{x}/{y}.mvt` returns Mapbox Vector Tile 2.1 protobuf when layer source is vector.
  - **Files:** `src/handlers/tiles.rs`, new vector branch in `LayerConfig`.
  - **Why deferred:** Raster pipeline is the v0.1.5 focus; MVT handler reuses existing `oxigdal-services::mvt` encoder.

- [ ] GetFeatureInfo HTML / JSON / GML response formats (currently only the framework exists).
  - **Files:** `src/handlers/wms.rs` (around line 956, `handle_get_feature_info`).

- [ ] Per-IP rate limiting middleware via `tower::limit` + `governor`.
  - **Files:** `src/server.rs` router stack.

## Low Priority / Future (one-liners)
- [ ] Prometheus `/metrics` endpoint with tile-request histogram and cache-hit ratio.
- [ ] Terrain RGB / Quantized Mesh tile encoding for 3D clients (Cesium, MapLibre Terrain).
- [ ] TLS termination via `tokio-rustls` (currently expects reverse proxy).
- [ ] Redis-backed shared cache for multi-replica deployments.
- [ ] S3/GCS tile cache backend for serverless / CDN warming.
- [ ] Tile pre-seeding CLI subcommand (cache warmup from bbox + zoom range).
- [ ] Multi-layer compositing (alpha blend N rasters in one GetMap response).
- [ ] Hot-reload of `LayerConfig` without server restart (file-watch + atomic registry swap).
- [ ] WMS time dimension (`TIME=` parameter) for temporal raster stacks.

## Cross-crate dependencies
- **Blocks:** None.
- **Blocked by:** `oxigdal-services` (for `TileSetMetadata`, MVT encoder, conformance types).

## Recently completed (verbatim)
*No prior `[x]` entries — slate is empty.*

---
*Last audited: 2026-05-16*
