# TODO: oxigdal-pwa

> **Purpose:** Progressive Web App primitives — Service Worker registration, Cache API + IndexedDB persistence, Push notifications, Background Sync, Web App Manifest, geospatial tile caching.
> **Status (2026-05-17):** 5,452 LoC · 52 #[test]/#[wasm_bindgen_test] attributes · 1 real-code soft stub.
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [ ] Wait-for-active service worker with real `statechange` event subscription
  - **Verified gap:** `src/service_worker/registration.rs:165-178` — `pub async fn wait_for_active(registration: &ServiceWorkerRegistration) -> Result<ServiceWorker> {` / `// Check if already active` / `... // In a real implementation, we would wait for the state change event` / `// For now, we'll return an error if not active` / `return Err(PwaError::InvalidState("Service worker is not active yet".to_string()));`
  - **Goal:** `wait_for_active` actually waits — subscribes to the `statechange` event on the installing/waiting worker and resolves when it transitions to `Activated`. Today the function returns `InvalidState` immediately for any non-active worker, forcing callers into busy loops.
  - **Design:** Use `wasm_bindgen_futures::JsFuture` + a `Closure<dyn FnMut(web_sys::Event)>` listening to `ServiceWorker.onstatechange`. Resolve when `worker.state() == ServiceWorkerState::Activated`. Reject if state reaches `Redundant`. Provide a timeout via `wasm-timer` (workspace) or `setTimeout` polyfill. Pattern:
    ```rust
    let (sender, receiver) = futures::channel::oneshot::channel();
    let closure = Closure::once(move |_| { /* check state; sender.send(...) */ });
    worker.set_onstatechange(Some(closure.as_ref().unchecked_ref()));
    receiver.await
    ```
  - **Files:** `crates/oxigdal-pwa/src/service_worker/registration.rs` (replace stub block); `crates/oxigdal-pwa/src/service_worker/events.rs` (extract event helpers).
  - **Tests:** (proposed) `test_wait_for_active_resolves_when_worker_activates` (wasm_bindgen_test), `test_wait_for_active_rejects_when_worker_becomes_redundant`, `test_wait_for_active_timeout_after_30s`, `test_wait_for_active_returns_immediately_if_already_active`.
  - **Risk:** Browser GC may drop the closure if not held — store it in a struct member or `mem::forget` it (latter is acceptable for one-shot waiters but `Closure::once` already handles this).
  - **Prerequisites:** None — `web_sys` features already include `ServiceWorkerState`, `ExtendableEvent`.

- [ ] Real response-size estimation by reading body chunks (replace `Not implemented` fallback)
  - **Verified gap:** `src/cache/storage.rs:210-217` — `// Clone and read body to estimate size` / `// Note: Reading the body to estimate size is not implemented` / `// This would require ReadableStream support which may not be available` / `// in all web-sys versions. For now, we return None if Content-Length` / `// header is not present.` / `Ok(None)`
  - **Goal:** When a response has no `Content-Length` (chunked encoding, server omission), still estimate size by streaming the cloned body through `ReadableStreamDefaultReader` and summing chunk byte lengths. Today storage quota decisions silently accept responses of unknown size, leading to surprise quota-exceeded errors mid-write.
  - **Design:** Cargo.toml already enables `ReadableStream` and `ReadableStreamDefaultReader` web-sys features. Clone the response (`response.clone()`), get its body via `body() -> Option<ReadableStream>`, get reader via `get_reader().unchecked_into::<ReadableStreamDefaultReader>()`, then loop `JsFuture::from(reader.read())` accumulating `Uint8Array.byte_length()` until `result.done == true`. Cap at 50 MB to avoid infinite-stream attacks. Return `Some(total)` on success, `None` only if body absent. Return `Err` only on stream error.
  - **Files:** `crates/oxigdal-pwa/src/cache/storage.rs` (replace stub block).
  - **Tests:** (proposed) `test_estimate_response_size_with_content_length_returns_header_value`, `test_estimate_response_size_chunked_sums_chunks`, `test_estimate_response_size_no_body_returns_none`, `test_estimate_response_size_caps_at_50mb`, `test_estimate_response_size_propagates_stream_error`.
  - **Risk:** Cloning the response doubles memory briefly — fine for cache decisions but document. Alternative: don't clone, return `None` and let caller decide.
  - **Prerequisites:** None.

- [ ] Real IndexedDB-backed geospatial tile cache (replace declarative API)
  - **Verified gap:** Existing TODO line — `[ ] Add IndexedDB-backed tile cache for offline geospatial data access`. `src/cache/geospatial.rs` exists (12.4K) with the type definitions, but verify whether persistence layer is real IndexedDB (Cargo.toml lacks `idb`/`rexie` deps).
  - **Goal:** `GeospatialCache::prefetch_tiles(bbox, zoom_range, url_template)` stores tiles in IndexedDB across browser sessions; subsequent `get_tile(z, x, y)` returns the cached blob without network.
  - **Design:** Add `rexie 0.6` (the canonical Rust idiomatic IndexedDB wrapper; Pure Rust; uses web-sys under the hood). Schema: object store `tiles` keyed on `(z, x, y, source_id)` with value `{ data: Vec<u8>, mime: String, etag: Option<String>, fetched_at: u64 }`. Index `(source_id, fetched_at)` for LRU eviction. Use `cache_strategy` field from existing `CacheStrategy` enum to drive cache-first / network-first / stale-while-revalidate behavior.
  - **Files:** `crates/oxigdal-pwa/src/cache/geospatial.rs` (real IDB plumbing); modify `Cargo.toml` to add `rexie` (or `idb`); (new) `crates/oxigdal-pwa/src/cache/indexed_db.rs` (schema + connection helper).
  - **Tests:** (proposed) `test_geospatial_cache_persists_across_page_reload` (wasm_bindgen_test with `fake-indexeddb` polyfill), `test_geospatial_cache_lru_evicts_oldest_when_quota_exceeded`, `test_geospatial_cache_stale_while_revalidate_returns_stale_then_updates`, `test_geospatial_cache_prefetch_5_tiles_zoom_2_to_4`.
  - **Risk:** rexie is a small dep but pulls in `js-sys`/`web-sys` already in tree. Quota Manager API differs Chrome/Firefox/Safari — document.
  - **Prerequisites:** None.

- [ ] Real Cache API integration for network interception (`oninstall`/`onfetch`)
  - **Verified gap:** Existing TODO line — `[ ] Implement Cache API integration for network request interception`. `Cargo.toml` enables `web_sys::Cache`, `CacheStorage`, `FetchEvent`, `ExtendableEvent`. `src/service_worker/messaging.rs` (16.7K) handles `MessageEvent` but the `FetchEvent.respondWith()` flow needs verification.
  - **Goal:** Service Worker script (Rust-compiled-to-wasm) can intercept `fetch` events, look up the requested URL in `caches.open('tiles-v1')`, respond from cache if present, else fall through to network and store the response.
  - **Design:** Provide a `FetchInterceptor` builder pattern: `FetchInterceptor::new().with_strategy(CacheStrategy::CacheFirst("tiles-v1")).match_path("/tiles/*").register_in_worker()`. Internally registers `addEventListener("fetch", e => e.respondWith(...))`. Use `wasm_bindgen::closure::Closure::new` + manual `mem::forget` to keep listeners alive for the worker lifetime.
  - **Files:** (new) `crates/oxigdal-pwa/src/service_worker/fetch_handler.rs`; `crates/oxigdal-pwa/src/cache/strategies.rs` (extend with execution path).
  - **Tests:** (proposed) `test_fetch_handler_cache_first_returns_cached_response`, `test_fetch_handler_network_first_falls_back_to_cache_on_offline`, `test_fetch_handler_stale_while_revalidate_returns_stale_immediately`, `test_fetch_handler_skips_non_matching_paths`.
  - **Risk:** Strict Mode (CSP) blocks dynamic event handler attachment; document inline-eval requirement.
  - **Prerequisites:** Item 3 (IDB cache) for the cache backing store.

- [ ] Push notification subscription with real VAPID key flow
  - **Verified gap:** Existing TODO line — `[ ] Add real push notification subscription with VAPID key support`. `notifications.rs` exists (16.3K) with display-side; push subscription (server-side identification) needs verification — Cargo.toml has `PushManager`, `PushSubscription`, `PushSubscriptionOptions` enabled.
  - **Goal:** `NotificationManager::subscribe_push(vapid_public_key: &str) -> Result<PushSubscriptionInfo>` returns the endpoint URL + auth secret + p256dh key that a backend server can use to send Web Push messages.
  - **Design:** Get `ServiceWorkerRegistration.pushManager`, call `subscribe(PushSubscriptionOptionsInit { user_visible_only: true, application_server_key: vapid_b64 })`. Extract `endpoint`, `keys.auth`, `keys.p256dh` via `js_sys::Reflect`. Return as `PushSubscriptionInfo` struct. Provide `unsubscribe()` returning the previous subscription for revocation. Document the URL-safe base64 encoding required for VAPID public key (uncompressed P-256 point).
  - **Files:** `crates/oxigdal-pwa/src/notifications.rs` (extend with `subscribe_push`); (new) `crates/oxigdal-pwa/src/push/mod.rs`.
  - **Tests:** (proposed) `test_subscribe_push_returns_endpoint_url`, `test_subscribe_push_with_invalid_vapid_returns_error`, `test_unsubscribe_push_succeeds`, `test_push_subscription_info_serde_roundtrip`.
  - **Risk:** VAPID keys are server-side; SDK must not generate them. Document the host-side toolchain (e.g., `web-push-rs`) for key generation.
  - **Prerequisites:** Item 1 (real wait_for_active) so subscribers receive notifications only on activated workers.

## Medium Priority
- [ ] Background sync queue with retry-when-online semantics
  - **Goal:** Failed uploads queued in IDB; SyncManager retries via `SyncEvent` when network returns.
  - **Files:** `crates/oxigdal-pwa/src/sync.rs` (extend).
  - **Why deferred:** Pending Item 3 (IDB cache).

- [ ] Bandwidth estimation for tile quality selection
  - **Goal:** Measure recent fetch RTTs, pick low/medium/high tile quality.
  - **Files:** (new) `crates/oxigdal-pwa/src/bandwidth.rs`.
  - **Why deferred:** Coordinated with oxigdal-mobile-enhanced bandwidth detection.

- [ ] Periodic background sync for STAC catalog updates
  - **Goal:** `periodicSync` API integration to refresh catalog daily.
  - **Files:** `crates/oxigdal-pwa/src/sync.rs` (extend).
  - **Why deferred:** Periodic Sync is Chrome-only (no Firefox/Safari).

- [ ] App update detection (`updatefound` event) with user prompt
  - **Goal:** Surface a "New version available, reload?" banner when SW updates.
  - **Files:** `crates/oxigdal-pwa/src/lifecycle.rs` (extend).
  - **Why deferred:** Quick win; pair with Item 1 (statechange).

- [ ] Share Target API to receive .tif / .geojson from share sheet
  - **Goal:** Manifest `share_target` config + handler that ingests shared files into IDB.
  - **Files:** `crates/oxigdal-pwa/src/manifest.rs` (extend); (new) `crates/oxigdal-pwa/src/share.rs`.
  - **Why deferred:** Lower priority than core caching.

- [ ] File Handling API for OS file-picker associations (.tif/.geojson)
  - **Goal:** Register the PWA as the handler for geospatial MIME types.
  - **Files:** `crates/oxigdal-pwa/src/manifest.rs` (file_handlers field).
  - **Why deferred:** Bundle with Share Target.

- [ ] Web Share API for sharing map views and screenshots
  - **Goal:** `navigator.share({ files: [pngBlob] })` wrapper.
  - **Files:** (new) `crates/oxigdal-pwa/src/web_share.rs`.
  - **Why deferred:** Low priority.

- [ ] Storage estimate + quota cleanup policies
  - **Goal:** When quota >80%, evict oldest LRU tiles.
  - **Files:** `crates/oxigdal-pwa/src/cache/storage.rs` (extend cleanup).
  - **Why deferred:** Coordinated with Item 3 (IDB schema includes index for eviction).

- [ ] Tile prefetch on idle (requestIdleCallback)
  - **Goal:** Prefetch tiles for adjacent zoom levels during browser idle.
  - **Files:** `crates/oxigdal-pwa/src/cache/geospatial.rs` (extend).
  - **Why deferred:** Pending Item 3.

## Low Priority / Future (one-liners)
- [ ] Workbox-style declarative caching route configuration.
- [ ] Payment Request API for premium tile subscriptions.
- [ ] Web Bluetooth for field sensor data collection.
- [ ] Credential Management API for tile-server authentication.
- [ ] Screen Wake Lock for continuous GPS tracking mode.
- [ ] Content Indexing API for offline search of cached datasets.
- [ ] Badging API to show unsynced edit count on icon.

## Cross-crate dependencies
- **Blocks:** Browser apps using both oxigdal-wasm and oxigdal-pwa (shared IDB cache layer).
- **Blocked by:** oxigdal-wasm (Service Worker compiled from wasm).

## Recently completed (verbatim)
- (no `[x]` entries in prior TODO.md — see README.md for the PWA architecture)

---
*Last audited: 2026-05-17*
