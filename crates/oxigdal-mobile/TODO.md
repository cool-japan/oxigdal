# TODO: oxigdal-mobile

> **Purpose:** C-compatible FFI bindings exposing OxiGDAL to iOS (Swift) and Android (Kotlin/JNI) apps. Includes `cbindgen.toml` header generation and Swift/Kotlin scaffolding under `bindings/`.
> **Status (2026-05-17):** 12,978 LoC · 178 #[test] attributes · 5 real-code placeholders.
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [ ] Replace placeholder Android vector geometry with real feature payload
  - **Verified gap:** `src/android/vector.rs:1187-1189` — `// For now, create a simple point geometry as placeholder` / `// In a full implementation, the feature would contain geometry data` / `let geometry = Geometry::Point(Point2D { x: 0.0, y: 0.0 });`
  - **Goal:** When the Android JNI bridge converts a `*const OxiGdalFeature` to an `android.graphics.Path`, the geometry must come from the feature's actual WKB/WKT payload, not a hard-coded (0,0) point.
  - **Design:** Add a `geometry: *const u8` + `geometry_len: usize` + `geometry_format: u8 (0=WKB,1=WKT,2=GeoJSON)` triple to `OxiGdalFeature` (FFI struct in `src/ffi/vector/feature.rs`). Decode in `oxigdal_mobile_feature_get_geometry()` via `oxigdal_core::geometry::Geometry::from_wkb` then dispatch through `geometry_to_android_path` (already exists below the placeholder). For backward compatibility, if `geometry == NULL`, return null path with `InvalidArgument` error.
  - **Files:** `crates/oxigdal-mobile/src/android/vector.rs` (replace placeholder); `crates/oxigdal-mobile/src/ffi/vector/feature.rs` (extend struct); `crates/oxigdal-mobile/cbindgen.toml` (regenerate header so Kotlin side sees new fields).
  - **Tests:** (proposed) `test_feature_with_wkb_polygon_renders_path`, `test_feature_with_null_geometry_returns_null_path`, `test_feature_with_invalid_wkb_returns_error`, `test_android_path_winding_matches_geometry_orientation`.
  - **Risk:** Adding fields to a public C struct is an ABI break — bump minor and document in CHANGELOG. Provide `oxigdal_mobile_feature_v2` constructor.
  - **Prerequisites:** None.

- [ ] Real tile renderer for `tile_request_handler` placeholder
  - **Verified gap:** `src/common/mod.rs:251-253` — `// Create a placeholder tile with geographic info encoded` / `// The actual rendering would be done by the tile rendering pipeline` / `let tile_data = vec![0u8; tile_data_size];`
  - **Goal:** The tile prefetcher must call into an actual rendering pipeline (raster sampling + reprojection to Web Mercator + encode to PNG/WebP) instead of caching all-zero buffers. Today the cache is populated with black tiles, which then "succeed" downstream and mask real bugs.
  - **Design:** Introduce `trait TileRenderer { fn render(&self, z: u8, x: u32, y: u32, tile_size: u32) -> Result<Vec<u8>, MobileError>; }`. Provide `RasterDatasetTileRenderer` that wraps an `oxigdal_core::Dataset`, uses `oxigdal_geotiff` for reads, `oxigdal-proj` for Web Mercator transform, and oxiarc-deflate/`oxigdal-webp` for encode. Inject the renderer into `prefetch_tiles_for_bbox` via a `TileRendererHandle` opaque FFI type.
  - **Files:** `crates/oxigdal-mobile/src/common/mod.rs` (extract logic); (new) `crates/oxigdal-mobile/src/common/renderer.rs`; (new) `crates/oxigdal-mobile/src/ffi/tiles/renderer.rs` (handle FFI).
  - **Tests:** (proposed) `test_tile_renderer_produces_nonzero_png`, `test_tile_renderer_handles_dateline_crossing_bbox`, `test_prefetch_tiles_calls_renderer_once_per_tile`, `test_tile_renderer_propagates_dataset_error`.
  - **Risk:** PNG encode is allocator-heavy; consider WebP-lossless as the default for mobile (smaller). Coordinate with oxigdal-webp.
  - **Prerequisites:** Stabilization of oxigdal-webp encoder (already in tree).

- [ ] iOS `oxigdal_ios_get_documents_path` real platform call instead of `/Documents` literal
  - **Verified gap:** `src/ios/mod.rs:97-100` — `pub extern "C" fn oxigdal_ios_get_documents_path() -> *mut std::os::raw::c_char {` / `// This would use iOS-specific APIs in a real implementation` / `// For now, return a placeholder` / `match std::ffi::CString::new("/Documents") {`
  - **Goal:** Return the actual iOS documents directory (`NSSearchPathForDirectoriesInDomains(NSDocumentDirectory, NSUserDomainMask, YES).firstObject`), not the literal `/Documents` (which is not a real iOS path).
  - **Design:** Add a thin Objective-C runtime call via `objc2 0.6` (workspace-compatible) inside a `#[cfg(target_os = "ios")]` block. Fall back to `std::env::var("HOME") + "/Documents"` when running on the simulator. Cache the result in a `OnceLock<CString>` so repeated calls don't re-FFI. The Swift bindings already wrap this; just have them point to a working impl.
  - **Files:** `crates/oxigdal-mobile/src/ios/mod.rs` (replace placeholder); `crates/oxigdal-mobile/Cargo.toml` (`[target.'cfg(target_os = "ios")'.dependencies] objc2 = "0.6"`).
  - **Tests:** (proposed) `test_ios_documents_path_nonempty_under_simulator`, `test_ios_documents_path_ends_with_documents`, `test_ios_documents_path_cached_pointer_stable_across_calls`.
  - **Risk:** objc2 crate adds an iOS-only dep; gate strictly behind `cfg(target_os = "ios")` and never compile on Linux/macOS host tests.
  - **Prerequisites:** None.

- [ ] iOS raster resampling for display dimensions (replace fixed-resolution read)
  - **Verified gap:** `src/ios/raster.rs:39-40` — `// Read region with resampling for display size` / `// For now, use standard read (would implement resampling in production)`
  - **Goal:** When `oxigdal_ios_raster_read_for_display(dataset, x_off, y_off, width, height, display_width, display_height, buffer)` is called, sample at the source `(width × height)` and resample to `(display_width × display_height)` for retina-aware fast rendering. Today the display_width/display_height parameters are accepted but ignored — caller gets full-res pixels, which is what causes mobile UI freezes on large COGs.
  - **Design:** Inside `oxigdal_ios_raster_read_for_display`, after the existing `oxigdal_dataset_read_region` call, run `oxigdal_algorithms::resampling::resample_buffer(input, src_w, src_h, dst_w, dst_h, ResamplingMethod::Lanczos)`. Expose `method: OxiGdalResamplingMethod` as an extra FFI parameter; default to Bilinear for backward compat.
  - **Files:** `crates/oxigdal-mobile/src/ios/raster.rs` (real resample call); `crates/oxigdal-mobile/src/ffi/raster/mod.rs` (extend FFI struct).
  - **Tests:** (proposed) `test_read_for_display_2x_downsample_bilinear`, `test_read_for_display_3x_upsample_lanczos`, `test_read_for_display_identity_when_dims_match`, `test_read_for_display_invalid_method_returns_error`.
  - **Risk:** Lanczos kernel allocates a temp band; for very large requests document memory cost.
  - **Prerequisites:** None — `oxigdal_algorithms::resampling` already provides the kernels.

- [ ] Replace single-global last-error string with thread-safe error queue
  - **Verified gap:** Existing TODO line — `[ ] Add thread-safe error message queue replacing single last-error global state`. `src/ffi/error.rs` uses a single `Mutex<String>` (verified — single global state pattern leaks errors between threads).
  - **Goal:** Per-thread last-error and an explicit `oxigdal_get_last_error_for_thread(thread_id)` accessor; or thread-local storage so the existing `oxigdal_get_last_error()` returns the calling thread's last error.
  - **Design:** Switch from `Lazy<Mutex<String>>` to `thread_local! { static LAST_ERROR: RefCell<String> }`. Update `set_last_error` and `oxigdal_get_last_error` accordingly. Document that callers should retrieve the error on the thread that triggered it. Provide a "global recent errors" ring buffer (`OnceLock<Mutex<VecDeque<(ThreadId, String)>>>`) for postmortem dumps via `oxigdal_dump_recent_errors`.
  - **Files:** `crates/oxigdal-mobile/src/ffi/error.rs` (entire module); audit all `set_last_error` call sites.
  - **Tests:** (proposed) `test_last_error_isolated_per_thread`, `test_last_error_survives_close_and_reopen`, `test_recent_errors_ring_buffer_caps_at_64`, `test_clear_last_error_only_affects_current_thread`.
  - **Risk:** Existing Swift/Kotlin wrappers may assume the global behavior — release notes must call this out.
  - **Prerequisites:** None.

## Medium Priority
- [ ] XCFramework build script (iOS + iOS-simulator + macOS Catalyst, lipo'd)
  - **Goal:** A `build_xcframework.sh` that produces `OxiGDALMobile.xcframework` consumable via SPM.
  - **Files:** (new) `crates/oxigdal-mobile/bindings/ios/build_xcframework.sh`.
  - **Why deferred:** Manual today; CI matrix in `npm-publish.yml`/`pypi-publish.yml` only per CLAUDE.md.

- [ ] Android AAR packaging via Gradle (`./gradlew :oxigdal-mobile:assembleRelease`)
  - **Goal:** Pre-built `.aar` shipping `libs/{arm64-v8a,armeabi-v7a,x86_64}/liboxigdal_mobile.so` + Kotlin wrappers.
  - **Files:** (new) `crates/oxigdal-mobile/bindings/android/build.gradle.kts`.
  - **Why deferred:** Manual today.

- [ ] Streaming raster read FFI for memory-constrained devices
  - **Goal:** `oxigdal_dataset_read_stream(dataset, callback, user_data)` — push tiles one at a time to the caller.
  - **Files:** `crates/oxigdal-mobile/src/ffi/raster/mod.rs` (extend).
  - **Why deferred:** Pending decision on callback ABI (block vs async).

- [ ] COG range-request reader exposed via FFI
  - **Goal:** Open `https://.../*.tif` from Swift/Kotlin without staging the file locally.
  - **Files:** (new) `crates/oxigdal-mobile/src/ffi/remote.rs`.
  - **Why deferred:** Pending oxigdal-cloud `HttpDataSource` (workspace).

- [ ] Zero-copy buffer sharing with Metal (iOS) / Vulkan (Android)
  - **Goal:** Hand off raw GPU-mapped pixel pointers to the platform graphics API for zero-copy display.
  - **Files:** (new) `crates/oxigdal-mobile/src/ffi/gpu_interop.rs`.
  - **Why deferred:** Requires investigation of Metal `MTLBuffer`/AHardwareBuffer ABI.

- [ ] Real progress callback support across FFI for long-running ops
  - **Goal:** `oxigdal_set_progress_callback(void (*)(double, const char*, void*))`.
  - **Files:** (new) `crates/oxigdal-mobile/src/ffi/progress.rs`.
  - **Why deferred:** Coordinated with oxigdal-node analogue.

- [ ] Cancellation token FFI for async operations
  - **Goal:** `OxiGdalCancellationToken* token = oxigdal_token_create(); ...; oxigdal_token_cancel(token);`.
  - **Files:** (new) `crates/oxigdal-mobile/src/ffi/cancel.rs`.
  - **Why deferred:** Coordinates with progress callback infrastructure.

- [ ] Memory-mapped local file access for large local raster datasets
  - **Goal:** Use `memmap2` for read-only local opens, reduce RAM use on mobile.
  - **Files:** `crates/oxigdal-mobile/src/ffi/raster/mod.rs` (extend).
  - **Why deferred:** iOS sandboxing rules around mmap need verification.

- [ ] Coordinate transformation FFI functions (`oxigdal_proj_transform_points`)
  - **Goal:** Thin wrapper around oxigdal-proj `Transformer` accepting flat `[lon0, lat0, lon1, lat1, ...]` arrays.
  - **Files:** (new) `crates/oxigdal-mobile/src/ffi/proj.rs`.
  - **Why deferred:** Easy; gated on FFI ABI stabilization.

- [ ] GeoJSON read/write through FFI
  - **Goal:** `oxigdal_geojson_open(path, layer_handle*)` / `oxigdal_geojson_write(features, path)`.
  - **Files:** (new) `crates/oxigdal-mobile/src/ffi/vector/geojson.rs`.
  - **Why deferred:** Vector FFI churning (Item 1 above) settles first.

## Low Priority / Future (one-liners)
- [ ] React Native bridge module (`react-native-oxigdal`).
- [ ] Flutter plugin via dart:ffi.
- [ ] .NET MAUI / Xamarin bindings via P/Invoke.
- [ ] On-device ML inference passthrough (CoreML on iOS, NNAPI on Android).
- [ ] ARKit/ARCore spatial anchor support for geo-registered AR.
- [ ] MapLibre/Mapbox integration layer for native map display.
- [ ] Automated cbindgen header generation in CI (guard against drift).
- [ ] Fuzz testing for all FFI entry points (cargo-fuzz already in workspace).

## Cross-crate dependencies
- **Blocks:** oxigdal-mobile-enhanced (uses this crate's FFI).
- **Blocked by:** oxigdal-cloud (remote COG reader), oxigdal-webp (tile encoding), oxigdal-algorithms::resampling (display resample), oxigdal-proj (transform FFI).

## Recently completed (verbatim)
- (no `[x]` entries in prior TODO.md — see README.md for the FFI architecture)

---
*Last audited: 2026-05-17*
