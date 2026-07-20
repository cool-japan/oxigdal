# TODO: oxigdal-wasm

> **Purpose:** WebAssembly bindings for browser-based COG viewing, tile streaming, and image processing.
> **Status (2026-05-17):** 17,977 LoC · 407 #[test]/#[wasm_bindgen_test] attributes (403 pass / 3 skipped per MEMORY) · 5 real-code stubs (excluding tests/).
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [ ] Replace `MemoryTracker::record_current` placeholder with `performance.memory` JS heap probe
  - **Verified gap:** `src/profiler.rs:430-432` — `// In WASM, we can't easily get memory info without performance.memory API` / `// For now, use a placeholder` / `let snapshot = MemorySnapshot::new(timestamp, 0);`
  - **Goal:** Real `usedJSHeapSize` / `totalJSHeapSize` capture so the profiler reports non-zero browser memory deltas; gracefully fall back to wasm linear memory page count via `wasm_bindgen::memory().buffer().byte_length()` when `performance.memory` is unavailable (Firefox/Safari).
  - **Design:** Use `js_sys::Reflect::get` on `web_sys::window().performance()` (the `Performance` interface only exposes `memory` in Chromium); when present, read `usedJSHeapSize`. Otherwise read the wasm memory size via `wasm_bindgen::memory()`. Always cache the last good value in `MemoryTracker` so consumers can correlate across browsers.
  - **Files:** `crates/oxigdal-wasm/src/profiler.rs` (replace placeholder block); `crates/oxigdal-wasm/src/wasm_memory.rs` (extend with `current_heap_bytes()` helper).
  - **Tests:** (proposed) `test_memory_tracker_records_nonzero_in_chromium` (gated `wasm_bindgen_test`), `test_memory_tracker_fallback_returns_wasm_memory_size`, `test_memory_tracker_records_two_snapshots_delta`.
  - **Risk:** `performance.memory` is non-standard; never deny tracking when missing. Numbers are JS-engine specific and not comparable across browsers — document.
  - **Prerequisites:** None.

- [ ] Replace `ScopedTimer::drop` placeholder timestamp with `js_sys::Date::now()`
  - **Verified gap:** `src/profiler.rs:622-624` — `// Get current time (in a real WASM environment, use js_sys::Date::now())` / `let current_time = self.start_time; // Placeholder`
  - **Goal:** Scoped timer reports actual elapsed time. Currently every scoped span reports zero duration, making the profiler useless for measuring critical paths.
  - **Design:** In `Drop`, prefer `performance.now()` (sub-ms resolution, monotonic) via `web_sys::window().performance().now()`. Fall back to `js_sys::Date::now()` if the worker context lacks `performance`. Guard against unwind paths — `Drop` must never panic; emit a final NaN duration if both APIs fail.
  - **Files:** `crates/oxigdal-wasm/src/profiler.rs` (Drop impl only).
  - **Tests:** (proposed) `test_scoped_timer_drop_records_positive_elapsed`, `test_scoped_timer_uses_performance_now_when_available`, `test_scoped_timer_no_panic_when_window_missing`.
  - **Risk:** `Date::now()` jumps backwards on NTP correction — prefer `performance.now()` for any monotonic comparisons in downstream code.
  - **Prerequisites:** None.

- [ ] Restore full affine transform in `ViewportTransform::new` (shx/shy honored)
  - **Verified gap:** `src/rendering.rs:271-272` — `pub const fn new(sx: f64, _shy: f64, _shx: f64, sy: f64, tx: f64, ty: f64) -> Self {` / `// Simplified: ignores shear components for now`
  - **Goal:** Honor the shear components of the 2×3 affine matrix `[[sx, shx, tx], [shy, sy, ty]]` so rotated/sheared overlays render correctly. Today the API accepts the parameters but silently discards them, which is a silent-correctness bug.
  - **Design:** Replace the field-level decomposition (tx/ty/sx/sy/rotation) with a `[f64; 6]` matrix (matches Canvas 2D `setTransform` argument order: a, b, c, d, e, f). Re-derive `rotation()` for callers via `atan2(b, a)`. Make `new` non-const if needed; provide `identity()` / `translate()` / `scale()` / `rotate()` constructors and a `compose()` for general affines. Keep the existing field-getter API stable.
  - **Files:** `crates/oxigdal-wasm/src/rendering.rs` (ViewportTransform struct + impl).
  - **Tests:** (proposed) `test_viewport_transform_shear_x_only`, `test_viewport_transform_full_affine_composition_associative`, `test_viewport_transform_identity_then_rotate_matches_rotate`, `test_viewport_transform_canvas2d_setTransform_argument_order`.
  - **Risk:** Public API field signature changes — restrict to internal callers + add a `#[deprecated]` note on legacy `rotation` field.
  - **Prerequisites:** None.

- [ ] Wire `WkbProjection::from_epsg` placeholder WKT to a real EPSG→WKT2 lookup
  - **Verified gap:** `src/component/projection.rs:96-97` — `/// Only a small set of well-known codes are pre-populated; others receive` / `/// a placeholder WKT.  For full WKT, use an external PROJ/WKT database.`
  - **Goal:** Replace the hard-coded match arm with a real EPSG→WKT2 table covering at least the 200 most-used CRS (EPSG:4326, 3857, 3035, 4269, 25832, UTM zones 1N-60S, JGD2011 series, NAD83(2011) series). Out-of-table codes get a structured `Err`, not a fake string.
  - **Design:** Embed `proj.db` lookups via `oxigdal-proj` (a workspace crate). For the WASM build, gate behind a `proj-table` feature that ships an EPSG→WKT2 table generated at build time (~80 KB after gzip). Loadable on demand via dynamic import to keep the default bundle under the 1 MB target.
  - **Files:** `crates/oxigdal-wasm/src/component/projection.rs` (replace placeholder branch); (new) `crates/oxigdal-wasm/build.rs` (codegen lookup table from oxigdal-proj's EPSG database).
  - **Tests:** (proposed) `test_from_epsg_4326_full_wkt2`, `test_from_epsg_25832_returns_utm32n_etrs89`, `test_from_epsg_unknown_returns_err`, `test_table_size_under_100kb`.
  - **Risk:** Adding 200 WKT strings could bloat the .wasm bundle (~150 KB raw). Mitigate with run-length WKT compression (CRS share common substrings) — target +50 KB compressed.
  - **Prerequisites:** None — oxigdal-proj already exposes EPSG resolution.

- [ ] Real `wasm_bindgen_futures` Fetch backend for HTTP range-request COG tile loading
  - **Verified gap:** Existing TODO line: `[ ] Implement actual HTTP range-request fetching for COG tiles via web_sys Fetch API`. `src/fetch.rs` exists (29.9K) with the `DataSource` trait and retry/parallel scaffolding, but lib-level COG reader still backed by an in-memory buffer.
  - **Goal:** End-to-end COG tile reads driven by `web_sys::Request` with `headers["Range"] = "bytes=N-M"`, exposed to JS as `WasmCogViewer.read_tile_remote(url, z, x, y)`.
  - **Design:** Build on existing `fetch::FetchClient` (retry/backoff/statistics already done); add `RemoteCogDataSource` implementing `oxigdal_core::io::AsyncDataSource`. Use `wasm_bindgen_futures::JsFuture::from(window.fetch_with_request(&req))`. Honor 206 Partial Content; degrade to full-GET when server returns 200. Cache directory IFD entries in IDB across reloads (interfaces with item 4 below).
  - **Files:** `crates/oxigdal-wasm/src/fetch.rs` (extend); (new) `crates/oxigdal-wasm/src/remote_cog.rs` (RemoteCogDataSource); `crates/oxigdal-wasm/src/cog_reader.rs` (add `WasmCogViewer::open_remote_url`).
  - **Tests:** (proposed) `test_remote_cog_range_request_returns_206`, `test_remote_cog_falls_back_to_full_get_on_200`, `test_remote_cog_retries_on_503_with_backoff`, `test_remote_cog_propagates_cors_error`.
  - **Risk:** CORS misconfiguration is the most common failure mode in production COG hosting; ensure error messages preserve the upstream HTTP status and any `Access-Control-Allow-Origin` mismatch so developers can diagnose.
  - **Prerequisites:** None (fetch.rs scaffolding already present).

## Medium Priority
- [ ] SharedArrayBuffer + Web Worker pool for true multi-threaded tile decode
  - **Goal:** Worker pool decodes JPEG/WebP/Deflate tiles in parallel using shared memory rather than postMessage copy.
  - **Files:** `crates/oxigdal-wasm/src/worker.rs` (extend).
  - **Why deferred:** Requires `crossOriginIsolated` (COOP/COEP headers); deployment caveat dominates implementation cost.

- [ ] WebGPU compute pipeline for hillshade/slope/aspect on GPU
  - **Goal:** Offload morphometry kernels (Horn / Zevenbergen & Thorne) to WGSL compute shaders; wire into JS via WebGPU `GPUDevice` queue.
  - **Files:** (new) `crates/oxigdal-wasm/src/webgpu/mod.rs`; (new) `crates/oxigdal-wasm/src/webgpu/hillshade.wgsl`.
  - **Why deferred:** WebGPU only at ~95% of evergreen browsers as of 2026-05; Safari 18.4 still partial.

- [ ] IndexedDB-backed persistent tile cache
  - **Goal:** Replace in-memory `WasmTileCache` with idb-backed store; cache COG IFDs across sessions.
  - **Files:** (new) `crates/oxigdal-wasm/src/idb_cache.rs`; modify `tile.rs`.
  - **Why deferred:** Coordinated with oxigdal-pwa cache layer.

- [ ] OffscreenCanvas rendering inside Web Worker
  - **Goal:** Tile compositing in a worker thread so main thread stays at 60fps during large pan/zoom.
  - **Files:** `crates/oxigdal-wasm/src/rendering.rs` (extend); `crates/oxigdal-wasm/src/worker.rs`.
  - **Why deferred:** Needs SharedArrayBuffer first.

- [ ] wasm128 SIMD intrinsics for inner pixel loops
  - **Goal:** Vectorize color-space convert, gamma, contrast kernels using `std::arch::wasm32::v128`.
  - **Files:** `crates/oxigdal-wasm/src/color.rs`, `crates/oxigdal-wasm/src/canvas.rs`.
  - **Why deferred:** Requires `+simd128` target feature plumbing in wasm-pack invocation.

- [ ] Streaming decode for GeoTIFF exceeding 2 GB browser address space
  - **Goal:** Incremental tile-by-tile decode rather than buffering the whole file.
  - **Files:** `crates/oxigdal-wasm/src/streaming.rs` (extend).
  - **Why deferred:** Blocked on remote COG reader (Item 5 above) landing first.

- [ ] WebCodecs `VideoDecoder`/`ImageDecoder` integration for hardware JPEG/AVIF decode
  - **Goal:** Hand JPEG/WebP/AVIF tile payloads to the browser's hardware-accelerated decoder.
  - **Files:** (new) `crates/oxigdal-wasm/src/webcodecs.rs`.
  - **Why deferred:** WebCodecs absent on Firefox stable until 130.

- [ ] Drag-and-drop local file handling (`File`, `FileSystemFileHandle`)
  - **Goal:** Accept GeoTIFF/GeoJSON via drop or File System Access API.
  - **Files:** (new) `crates/oxigdal-wasm/src/file_api.rs`.
  - **Why deferred:** Pending Item 5 (HTTP backend) to share the same `DataSource` abstraction.

## Low Priority / Future (one-liners)
- [ ] wasm32-wasip2 Component Model build target verification (WASI Preview 2 proposal).
- [ ] WebXR integration for immersive 3D terrain (WebXR Device API L1).
- [ ] Comlink-style transparent worker proxy (`@cooljapan/oxigdal-worker-proxy`).
- [ ] OPFS (Origin Private File System) backend for >100 MB local datasets.
- [ ] Progressive mesh loading for 3D terrain (TIN level-of-detail).
- [ ] Emscripten-free pure wasm-bindgen build (already wasm-bindgen, ensure no -lc deps).
- [ ] WebTransport (HTTP/3) tile streaming.
- [ ] WebSocket push for real-time tile invalidation.

## Cross-crate dependencies
- **Blocks:** oxigdal-pwa (shared IDB cache), oxigdal-mobile (uses bundled .wasm in WKWebView/WebView).
- **Blocked by:** oxigdal-proj (EPSG→WKT2 table generation for projection item), oxigdal-geotiff (remote tile reader trait).

## Recently completed (verbatim)
- (no `[x]` entries in prior TODO.md — see `crates/oxigdal-wasm/README.md` and MEMORY.md for the 403-tests-passing milestone)

---
*Last audited: 2026-05-17*
