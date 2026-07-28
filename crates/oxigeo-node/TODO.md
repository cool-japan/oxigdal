# TODO: oxigeo-node

> **Purpose:** napi-rs v3 (N-API v8) bindings exposing raster/vector ops to Node.js + Deno + Bun with async/await and zero-copy Buffer.
> **Status (2026-07-28):** 3,081 LoC · 84 tests, 0 failed (default and all-features) · progress-callback and slope/aspect pixel_size placeholders resolved; buffer zero-copy and full async-op coverage remain open.
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [x] Wire `set_progress_callback` from placeholder to functioning `ThreadsafeFunction` plumbing
  - **Resolved (2026-07-28):** `src/async_ops.rs` now stores a real `ThreadsafeFunction<f64, (), f64, Status, false, false, 0>` in a `OnceLock<Mutex<Option<ProgressTsfn>>>` (`set_progress_callback`/`clear_progress_callback`/`report_progress`), and `report_progress` is actually invoked from `batch_process_rasters` and `process_raster_parallel` (progress fraction per completed item/chunk, plus 0.0/1.0 start/end markers). No panics on a poisoned lock (returns `INTERNAL_ERROR` instead).
  - **Verified gap (historical):** `src/async_ops.rs:268-275` — `/// Progress callback for long-running operations` / `#[napi(ts_args_type = "callback: (progress: number) => void")] pub fn set_progress_callback(_callback: Function<'_, Unknown<'_>>) -> Result<()> {` / `// Store callback for use in long-running operations` / `// This is a placeholder for actual implementation` / `Ok(())`
  - **Goal:** Real progress reporting where Rust-side long-running tasks (warp, build_overviews, write) invoke a JS callback from a worker thread. Currently the function silently swallows the callback.
  - **Design:** Use `napi::threadsafe_function::ThreadsafeFunction<Args, JsValueOrPromise>` (napi-rs v3 API, N-API v8). Store it in a `Lazy<Mutex<Option<TsFn>>>`. Wrap long-running ops with a `ProgressEmitter::report(percent: f64, message: &str)` that the algorithms layer calls. Avoid `Drop` re-entrancy bugs: the TSFn must outlive the worker job, so reference-count via `Arc<ThreadsafeFunction>`.
  - **Files:** `crates/oxigeo-node/src/async_ops.rs` (replace placeholder); (new) `crates/oxigeo-node/src/progress.rs` (`ProgressEmitter`); modify long-running entry points in `crates/oxigeo-node/src/raster.rs` and `crates/oxigeo-node/src/algorithms.rs`.
  - **Tests:** (proposed) `test_progress_callback_invoked_multiple_times`, `test_progress_callback_receives_monotonic_values`, `test_progress_callback_dropped_without_crash`, `test_progress_callback_concurrent_safe` (parallel ops share emitter).
  - **Risk:** ThreadsafeFunction call-back ordering is not guaranteed under load — document that callback `progress` may decrease if multiple tasks race; recommend per-job emitters in JS docs.
  - **Prerequisites:** None — napi-rs 3.x already in deps.

- [x] Honor `pixel_size` and `z_factor` in `slope()` / `aspect()` instead of hard-coded 1.0
  - **Resolved (2026-07-28):** `src/algorithms.rs` now takes real `pixel_size` (`fn slope(dem, pixel_size, z_factor, as_percent)` / `fn aspect(dem, pixel_size)`), validated via `validate_pixel_size`, and threaded into `compute_slope_advanced`/`compute_aspect_advanced` with an honored `as_percent` unit switch. Covered by `slope_scales_inversely_with_pixel_size` and `slope_as_percent_flag_changes_units` regression tests (their doc comments explicitly call out the old hardcoded-1.0 bug).
  - **Verified gap (historical):** `src/algorithms.rs:223-225` — `pub fn slope(dem: &BufferWrapper, z_factor: f64, _as_percent: bool) -> Result<BufferWrapper> {` / `// Note: pixel_size is assumed to be 1.0 for now` / `let result = compute_slope(dem.inner(), 1.0, z_factor).to_napi()?;`. Same at `src/algorithms.rs:231-233`. The `_as_percent` parameter is also discarded.
  - **Goal:** Slope/aspect compute with real `pixel_size_x`, `pixel_size_y` (from GeoTransform) and an `as_percent: bool` toggle. Discarding these yields incorrect slope magnitudes on non-isotropic CRSs (Web Mercator at high latitude, UTM strips).
  - **Design:** Extend `BufferWrapper` (`src/buffer.rs`) to optionally carry a `GeoTransform`. When the user calls `oxigeo.slope(dem, zFactor, asPercent)`, the wrapper exposes `pixel_size_x()` / `pixel_size_y()` (cell size in CRS units). Slope formula switches between `tan(slope)` (radians, default), `tan*100` (percent), `degrees(atan(slope))`. Aspect honors z_factor scaling and the existing Horn/Z&T choice.
  - **Files:** `crates/oxigeo-node/src/algorithms.rs` (replace assumed-1.0 calls); `crates/oxigeo-node/src/buffer.rs` (add optional geotransform); `crates/oxigeo-node/index.d.ts` (extend typings).
  - **Tests:** (proposed) `test_slope_with_5m_pixels_matches_reference`, `test_slope_as_percent_vs_radians_consistent`, `test_aspect_with_z_factor_zero_returns_zero`, `test_buffer_carries_geotransform_through_chain`.
  - **Risk:** Buffer wrapper API change is observable in TS typings — bump napi minor only.
  - **Prerequisites:** None.

- [x] Generate `index.d.ts` from napi-derive macros instead of manual maintenance
  - **Resolved:** `index.d.ts` now carries the `/* auto-generated by NAPI-RS */` banner and `package.json`'s `"build"` script (`napi build --platform --release`) regenerates it directly from the `#[napi]` annotations. No longer hand-maintained.
  - **Verified gap (historical):** Existing TODO line — `[ ] Add TypeScript declaration generation from napi-rs annotations`. The current `index.d.ts` (13.7K) is hand-maintained and known to drift; `index.js` (11.6K) similarly hand-rolled.
  - **Goal:** `npm run build` re-generates `index.d.ts` and the platform-specific loader stub `index.js` via `napi build --release --js index.js --dts index.d.ts`. CI fails if a `git diff` after build shows changes.
  - **Design:** Add `@napi-rs/cli` to `package.json` devDependencies (already implicit via `napi build`); add `package.json` script `"build:dts": "napi build --release --js false --dts index.d.ts"`. Move hand-written `index.js` to a generated loader matching napi-rs v3 convention. Annotate every `#[napi]` with explicit `ts_type` / `ts_args_type` where inferred TS is imprecise.
  - **Files:** `crates/oxigeo-node/package.json` (scripts); `crates/oxigeo-node/index.d.ts` (will be overwritten by codegen); `crates/oxigeo-node/index.js` (generated loader); every `#[napi]` site under `crates/oxigeo-node/src/`.
  - **Tests:** (proposed) `test_dts_regen_clean_against_main`, `test_index_js_loader_finds_prebuilt_binary`, `test_typescript_compile_imports_oxigeo_node` (consumer-side tsc test fixture).
  - **Risk:** Existing hand-written types may have richer JSDoc than what napi-rs emits; preserve via `#[napi(ts_args_type = "...")]` attribute strings.
  - **Prerequisites:** None.

- [ ] N-API async worker threads for non-blocking raster I/O (extend `async_ops.rs`)
  - **Verified gap:** Existing TODO line — `[ ] Implement N-API async worker threads for non-blocking raster I/O`. `src/async_ops.rs` exists (419 LoC) but most heavy ops in `src/raster.rs` are still synchronous (block the event loop).
  - **Goal:** Every blocking operation (`openRaster`, `readBand`, `warp`, `hillshade`, `save`) has an `Async`-suffixed counterpart returning a JS Promise. Sync versions remain for back-compat.
  - **Design:** Use `napi::bindgen_prelude::AsyncTask`. For each long-running op, define an `impl Task` with `compute()` running on tokio (already a dep) and `resolve()` building the JS return. Pattern follows the existing `oxigeo.openRasterAsync` if present, or napi-rs v3 docs.
  - **Files:** `crates/oxigeo-node/src/async_ops.rs` (extend); add `*Async` wrappers in `crates/oxigeo-node/src/raster.rs`, `crates/oxigeo-node/src/algorithms.rs`, `crates/oxigeo-node/src/vector.rs`.
  - **Tests:** (proposed) `test_openRasterAsync_does_not_block_event_loop`, `test_two_concurrent_warps_run_in_parallel`, `test_async_error_rejects_promise`, `test_async_progress_callback_fires_during_warp` (with item 1 above).
  - **Risk:** Tokio runtime startup latency adds ~5ms per cold call; share a single `Lazy<Runtime>` at module init.
  - **Prerequisites:** Item 1 (progress callback infra).

- [ ] Node.js `Buffer` / `ArrayBuffer` zero-copy for large raster transfer
  - **Verified gap:** Existing TODO line — `[ ] Add Node.js Buffer zero-copy integration for large raster data transfer`. Today `BufferWrapper::to_buffer()` copies pixels into a fresh `Vec<u8>`.
  - **Goal:** Return a `napi::bindgen_prelude::Buffer` that references the underlying `RasterBuffer<u8>` storage directly (no copy on the hot path), and accept `Buffer`/`ArrayBuffer` parameters by ref for writes.
  - **Design:** Use `napi::Env::create_buffer_with_borrowed_data` to wrap a stable raw pointer + length pair; the buffer ownership stays with Rust for the buffer's JS lifetime. Hand back a `Buffer` whose finalizer drops the Rust-side `RasterBuffer`. For non-u8 dtypes, expose `Float32Array`, `Int16Array`, etc. via N-API typed array constructors. Document that the underlying memory must not be mutated after handoff.
  - **Files:** `crates/oxigeo-node/src/buffer.rs` (add `to_buffer_zero_copy`); `crates/oxigeo-node/src/raster.rs` (use the zero-copy path); `crates/oxigeo-node/index.d.ts` (TS typings).
  - **Tests:** (proposed) `test_zero_copy_buffer_round_trip_no_pixel_diff`, `test_zero_copy_buffer_finalizer_runs`, `test_typed_float32_array_correct_dtype`, `test_concurrent_zero_copy_reads_safe`.
  - **Risk:** N-API finalizer must run on the same env as creation; verify under worker_threads.
  - **Prerequisites:** None.

## Medium Priority
- [ ] HTTP range-request COG reader (`oxigeo.openCog(url)`)
  - **Goal:** Node-side COG fetcher backed by undici/`globalThis.fetch` for remote tile reads.
  - **Files:** (new) `crates/oxigeo-node/src/remote.rs`.
  - **Why deferred:** Pending oxigeo-cloud HTTP data source.

- [ ] GeoPackage + MBTiles read/write bindings
  - **Goal:** `oxigeo.openGpkg(path, layer)`, `oxigeo.openMbtiles(path)`.
  - **Files:** (new) `crates/oxigeo-node/src/gpkg.rs`, (new) `crates/oxigeo-node/src/mbtiles.rs`.
  - **Why deferred:** Pending oxigeo-gpkg/oxigeo-mbtiles API stabilization.

- [ ] MVT vector tile generation bindings
  - **Goal:** Server-side MVT slicing for tile-server middleware.
  - **Files:** (new) `crates/oxigeo-node/src/mvt.rs`.
  - **Why deferred:** Pending oxigeo-mvt extraction from oxigeo-services.

- [ ] EPSG coordinate transformation bindings (`oxigeo.transform([lon, lat], 4326, 3857)`)
  - **Goal:** Thin wrapper over oxigeo-proj's `Transformer`.
  - **Files:** (new) `crates/oxigeo-node/src/proj_bindings.rs`.
  - **Why deferred:** Easy but low-priority vs items above.

- [ ] Prebuilt binary distribution via npm (`prebuild-install` pattern)
  - **Goal:** Ship platform-tagged `.node` binaries (`oxigeo-node-darwin-arm64.node`, etc.) so end users don't need a Rust toolchain.
  - **Files:** `crates/oxigeo-node/package.json` (`napi.triples`); `.github/workflows/npm-publish.yml` (per CLAUDE.md, allowed yaml).
  - **Why deferred:** Blocks on stabilizing default feature set + ABI.

- [ ] worker_threads parallel batch processing API
  - **Goal:** `oxigeo.batch([{op: "warp", ...}, ...], {threads: 4})`.
  - **Files:** (new) `crates/oxigeo-node/src/batch.rs`.
  - **Why deferred:** Needs async I/O (Item 4 above) first.

- [ ] gdal-style CLI tool (`npx oxigeo warp ...`)
  - **Goal:** Drop-in `gdal_translate` / `gdalwarp` CLI in Node, useful for CI scripts.
  - **Files:** (new) `crates/oxigeo-node/bin/oxigeo-node.js`.
  - **Why deferred:** Pure Node code; can ship without Rust work.

## Low Priority / Future (one-liners)
- [ ] Express/Fastify middleware exposing `/{z}/{x}/{y}.png` tile endpoint.
- [ ] MapLibre GL JS data source plugin (`oxigeo-source` package).
- [ ] gRPC server bindings via `@grpc/grpc-js`.
- [ ] Deno + Bun npm compat shim (already partially via N-API v8).
- [ ] PM2 cluster mode with shared SharedArrayBuffer tile cache.
- [ ] STAC API client bindings (delegates to oxigeo-stac).
- [ ] sharp-compatible image processing API surface.
- [ ] GeoArrow / GeoParquet bindings (delegate to oxigeo-geoparquet).

## Cross-crate dependencies
- **Blocks:** None directly (downstream is Node ecosystem).
- **Blocked by:** oxigeo-cloud (HTTP COG reader), oxigeo-gpkg, oxigeo-mbtiles, oxigeo-mvt extraction.

## Recently completed (verbatim)
- (no `[x]` entries in prior TODO.md — `oxigeo.darwin-arm64.node` 1.9 MB present in tree as local-build artifact)

---
*Last audited: 2026-07-28*
