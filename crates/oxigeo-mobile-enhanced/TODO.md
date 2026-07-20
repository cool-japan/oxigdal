# TODO: oxigeo-mobile-enhanced

> **Purpose:** Mobile platform performance optimizations layered on top of oxigeo-mobile — battery awareness, network quality detection, storage compression, background task scheduling.
> **Status (2026-05-17):** 4,872 LoC · 79 #[test] attributes · 8 real-code mock/simulated stubs.
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [ ] Real battery state reading via platform APIs (replace mock `75.0%`)
  - **Verified gap:** `src/battery.rs:247-260` — `// Note: sysinfo doesn't provide battery info directly` / `// In a real implementation, we would use platform-specific APIs` / `// For now, we'll create a mock state` / `let percentage = 75.0; // Mock value` / (state built from `Discharging` / `Some(Duration::from_secs(3600 * 4))` / `Some(35.0)`)
  - **Goal:** `BatteryMonitor::refresh()` returns the device's actual battery percentage, charging state, temperature, and estimated time-remaining instead of fixed `75% / Discharging / 4 h / 35 °C`.
  - **Design:** Split per-OS:
    - **iOS** (`cfg(target_os = "ios")`): bridge to `UIDevice.current` via objc2 — `batteryLevel` (0.0-1.0; -1.0 if monitoring disabled), `batteryState` (Unknown/Unplugged/Charging/Full). Enable monitoring on init; document that `UIDevice.batteryMonitoringEnabled = YES` requires Info.plist.
    - **Android** (`cfg(target_os = "android")`): use JNI to query `BatteryManager` (level, scale, status, temperature) via existing `oxigeo-mobile` JNI bridge.
    - **Linux**: read `/sys/class/power_supply/BAT0/{capacity,status,temp}` (already where battery crate looks).
    - **Other**: keep current synthetic state but return `BatteryLevel::Unknown` and `Err(MobileError::BatteryMonitoringNotSupported)` from `refresh()`.
  - **Files:** `crates/oxigeo-mobile-enhanced/src/battery.rs` (replace mock block); (new) `crates/oxigeo-mobile-enhanced/src/ios/battery.rs`; (new) `crates/oxigeo-mobile-enhanced/src/android/battery.rs`; (new) `crates/oxigeo-mobile-enhanced/src/linux/battery.rs`.
  - **Tests:** (proposed) `test_battery_refresh_returns_real_percentage_on_linux_when_bat0_present`, `test_battery_refresh_returns_err_on_unsupported_platform`, `test_battery_state_charging_detected_on_macos_simulator`, `test_battery_temperature_in_celsius_range`.
  - **Risk:** sysinfo dep is still listed in Cargo.toml under `battery-aware`; either drop it or use it only for cross-platform fallback. Document Info.plist requirement for iOS monitoring.
  - **Prerequisites:** None.

- [ ] Real network type detection (replace `Ok(NetworkType::WiFi)` mock)
  - **Verified gap:** `src/network.rs:199-203` — `pub fn detect_network_type(&self) -> Result<NetworkType> {` / `// In a real implementation, this would use platform-specific APIs` / `// For now, return a mock value` / `Ok(NetworkType::WiFi)`
  - **Goal:** Detect actual connection state — WiFi/4G LTE/5G NSA/5G SA/Cellular/Ethernet/Bluetooth/Offline — and metered status.
  - **Design:** Per-OS:
    - **iOS**: bridge to `NWPathMonitor` (Network.framework) via objc2; query `path.usesInterfaceType(.wifi/.cellular/.wiredEthernet/.loopback)` and `path.isExpensive` (metered).
    - **Android**: JNI to `ConnectivityManager.getActiveNetwork() / getNetworkCapabilities()`; check `TRANSPORT_WIFI` / `TRANSPORT_CELLULAR` and `NET_CAPABILITY_NOT_METERED`.
    - **Linux/macOS host**: parse `/proc/net/route` (Linux) or `route get default` (macOS) for active interface name; map `wlan*` → WiFi, `eth*` → Ethernet, etc. Mark Unknown when uncertain.
  - **Files:** `crates/oxigeo-mobile-enhanced/src/network.rs` (replace mock); (new) `crates/oxigeo-mobile-enhanced/src/ios/network.rs`; (new) `crates/oxigeo-mobile-enhanced/src/android/network.rs`.
  - **Tests:** (proposed) `test_detect_wifi_on_macos_host_when_en0_active`, `test_detect_offline_when_no_default_route`, `test_metered_flag_set_on_cellular`, `test_detect_network_returns_unknown_on_unsupported_platform`.
  - **Risk:** NWPathMonitor is callback-based — we need a snapshot accessor that blocks briefly; cache last-known state and refresh on demand.
  - **Prerequisites:** None.

- [ ] Real bandwidth measurement to replace `Mock quality metrics` block
  - **Verified gap:** `src/network.rs:210-217` — `// Mock quality metrics` / `let quality = NetworkQuality { network_type, download_speed: Some(10_000_000), upload_speed: Some(5_000_000), latency: Some(Duration::from_millis(20)), packet_loss: Some(0.5), timestamp: Instant::now() };`
  - **Goal:** Measure actual bandwidth + latency + packet loss instead of always reporting 10 MB/s down / 5 MB/s up / 20 ms RTT / 0.5% loss.
  - **Design:** Active probe to a tiny CDN URL (configurable via `NetworkOptimizer::with_probe_url(...)`). Default to `https://www.gstatic.com/generate_204` (4-byte response, ~50ms RTT). Run two parallel `HEAD` requests via reqwest/ureq, measure connect time → first byte (latency) and total transfer (throughput estimate). Run every `probe_interval` (default 60s). Passive monitoring path: track bytes transferred during recent calls to `compress_for_transfer` / `decompress_from_transfer` via the existing `DataUsageTracker`. Combine active+passive into `NetworkQuality`.
  - **Files:** `crates/oxigeo-mobile-enhanced/src/network.rs` (replace mock); add `reqwest = { workspace = true, default-features = false, features = ["rustls-tls"], optional = true }` gated by feature `bandwidth-probe`.
  - **Tests:** (proposed) `test_measure_quality_returns_realistic_latency_to_local_server`, `test_measure_quality_throughput_decreases_with_throttled_link` (uses tokio-test-util slow IO), `test_passive_monitoring_updates_data_usage_tracker`, `test_measure_quality_falls_back_when_probe_url_unreachable`.
  - **Risk:** Active probing on cellular costs bytes; gate behind `should_probe()` that respects metered status. Document.
  - **Prerequisites:** Item 2 (network type detection) for metered check.

- [ ] LZ4 compression real implementation (currently falls back to deflate)
  - **Verified gap:** `src/network.rs:251-254` — `CompressionMethod::Lz4 => {` / `// LZ4 compression would go here` / `// For now, use deflate as fallback` / `return self.compress_with_method(data, CompressionMethod::Deflate);`. Symmetric stub at lines 280-284 for decompress.
  - **Goal:** When the user requests `CompressionMethod::Lz4`, actually use LZ4 (~3-5x faster than deflate, similar ratio for binary tile data) rather than silently switching to deflate.
  - **Design:** Use `oxiarc-lz4` (already in workspace per `oxigeo-edge/Cargo.toml`). Add `oxiarc-lz4 = { workspace = true }` to `oxigeo-mobile-enhanced/Cargo.toml`. Call `oxiarc_lz4::compress(data, level)` and `oxiarc_lz4::decompress(data)`. Compression level 1-12 (LZ4 frame format default 1; HC mode 4+).
  - **Files:** `crates/oxigeo-mobile-enhanced/src/network.rs` (real call); `crates/oxigeo-mobile-enhanced/Cargo.toml` (add dep).
  - **Tests:** (proposed) `test_lz4_compression_returns_smaller_than_input_for_redundant_data`, `test_lz4_roundtrip_byte_identical`, `test_lz4_faster_than_deflate_on_64kb_random` (criterion), `test_lz4_decompress_rejects_truncated_input`.
  - **Risk:** None significant — oxiarc-lz4 is already a stable workspace dep used by oxigeo-edge.
  - **Prerequisites:** None.

- [ ] Real Android RAM availability replacing mocked `total_ram / 3`
  - **Verified gap:** `src/android/memory.rs:160` — `let available_ram = total_ram / 3; // Mock: 33% available`. Also `src/android/mod.rs:117` — `// For now, return mock values` (Android performance metrics); `src/ios/mod.rs:108` — same; `src/ios/memory.rs:106` — `// For now, create mock statistics`; `src/storage/mod.rs:66` — `// For now, return mock values`.
  - **Goal:** All mobile telemetry returns real values: Android via `ActivityManager.getMemoryInfo()`, iOS via `mach_task_info` + `host_statistics64`, storage via `statvfs`. Today the values are constants that the adaptive algorithms downstream then make decisions on — garbage-in-garbage-out.
  - **Design:** Same JNI/objc2 pattern as Items 1-2. For Android memory: bridge to `ActivityManager.MemoryInfo` (totalMem, availMem, threshold, lowMemory). For iOS: `host_statistics64(mach_host_self(), HOST_VM_INFO64, ...)` for system-wide pages; `task_info(mach_task_self(), TASK_VM_INFO, ...)` for current task. For storage: `statvfs("/data/data/<package>")` on Android, `[NSFileManager.default attributesOfFileSystemForPath:]` on iOS, `statvfs` on host.
  - **Files:** `crates/oxigeo-mobile-enhanced/src/android/memory.rs`, `crates/oxigeo-mobile-enhanced/src/ios/memory.rs`, `crates/oxigeo-mobile-enhanced/src/storage/mod.rs`, `crates/oxigeo-mobile-enhanced/src/android/mod.rs`, `crates/oxigeo-mobile-enhanced/src/ios/mod.rs`.
  - **Tests:** (proposed) `test_android_memory_info_returns_positive_avail_ram_on_emulator`, `test_ios_task_info_reports_resident_size_under_heap_size`, `test_storage_statvfs_reports_nonzero_free_bytes`, `test_low_memory_threshold_triggers_processing_mode_change`.
  - **Risk:** Per-test scaffolding needs CI runners with emulator/simulator access — Linux host tests must mark these `#[cfg(target_os = "android")]` etc.
  - **Prerequisites:** None.

- [ ] Background task `enqueue` placeholder needs WorkManager/BGTaskScheduler integration
  - **Verified gap:** `src/background.rs:216` — `// For now, just mark as queued`. Existing TODO line — `[ ] Add background task scheduling that integrates with iOS BGTaskScheduler / Android WorkManager`.
  - **Goal:** Tasks queued via `BackgroundTaskManager::schedule` actually run on the platform's background scheduler — Android `WorkManager` + iOS `BGTaskScheduler` — instead of being marked "queued" and forgotten.
  - **Design:** Define `trait BackgroundExecutor { fn submit(&self, task: BackgroundTask) -> Result<TaskId>; fn cancel(&self, id: TaskId) -> Result<()>; }`. Platform impls: `AndroidWorkManagerExecutor` (JNI; serialize the task to `WorkRequest` `setInputData`), `IosBgTaskExecutor` (objc2; register `BGTaskScheduler.shared.register` at app launch, submit `BGProcessingTaskRequest`). Host/Linux: in-process tokio runtime (already in deps as optional `tokio` under `background-tasks` feature). The Rust task receives a `wake_up()` callback when the platform fires the work.
  - **Files:** `crates/oxigeo-mobile-enhanced/src/background.rs` (extract trait); (new) `crates/oxigeo-mobile-enhanced/src/android/background.rs`; (new) `crates/oxigeo-mobile-enhanced/src/ios/background.rs`.
  - **Tests:** (proposed) `test_background_executor_in_process_runs_tokio_task`, `test_background_executor_cancellation_propagates`, `test_android_workmanager_submit_returns_id_via_jni` (requires Android emulator), `test_ios_bgtask_register_at_launch_succeeds` (simulator).
  - **Risk:** iOS BGTaskScheduler requires Info.plist entitlement (`BGTaskSchedulerPermittedIdentifiers`). Document. Android WorkManager has its own backoff policy that may conflict with our retry — document the precedence.
  - **Prerequisites:** Item 2 (network detection for "WiFi-only" task constraints).

## Medium Priority
- [ ] Storage compression entropy estimator: replace "simplified" with Shannon entropy
  - **Goal:** Pick compression algorithm based on real Shannon entropy of input, not a heuristic.
  - **Verified gap:** `src/storage/compression.rs:136` — `// Calculate entropy (simplified)`.
  - **Files:** `crates/oxigeo-mobile-enhanced/src/storage/compression.rs`.
  - **Why deferred:** Quick win once Item 4 above lands.

- [ ] iOS Metal GPU acceleration hints for raster processing
  - **Goal:** `MetalAccelerationHint` enum drives Metal Performance Shaders (MPS) selection via objc2.
  - **Files:** (new) `crates/oxigeo-mobile-enhanced/src/ios/metal.rs`.
  - **Why deferred:** Cross-cuts with oxigeo-gpu.

- [ ] Android RenderScript fallback for image processing on legacy devices
  - **Goal:** Use deprecated-but-still-supported RenderScript for pre-Vulkan Android (API 24+).
  - **Files:** (new) `crates/oxigeo-mobile-enhanced/src/android/renderscript.rs`.
  - **Why deferred:** RenderScript deprecated since Android 12; low priority unless we target API 24.

- [ ] Memory pressure handler for iOS `didReceiveMemoryWarning` and Android `onTrimMemory`
  - **Goal:** Hook OS memory pressure callbacks → trigger cache eviction.
  - **Files:** `crates/oxigeo-mobile-enhanced/src/android/memory.rs`, `crates/oxigeo-mobile-enhanced/src/ios/memory.rs`.
  - **Why deferred:** Pending Item 5 (real memory readings) so eviction decisions are meaningful.

- [ ] Geofencing-aware prefetch (download tiles for known routes)
  - **Goal:** Subscribe to Geofence transitions; prefetch tiles for the destination region.
  - **Files:** (new) `crates/oxigeo-mobile-enhanced/src/geofencing.rs`.
  - **Why deferred:** Needs platform Location API integration.

- [ ] Thermal state monitoring (throttle on `processInfo.thermalState` / `ThermalService`)
  - **Goal:** Pause CPU-heavy work when device thermal state ≥ `serious`.
  - **Files:** (new) `crates/oxigeo-mobile-enhanced/src/thermal.rs`.
  - **Why deferred:** Lower priority than battery/network items.

- [ ] Adaptive sync protocol for partial dataset updates
  - **Goal:** Delta-sync rather than full re-download when STAC catalog changes.
  - **Files:** (new) `crates/oxigeo-mobile-enhanced/src/sync.rs`.
  - **Why deferred:** Coordinated with oxigeo-edge sync work.

- [ ] Offline-first vector tile rendering with local cache
  - **Goal:** MVT rendering using cached tiles only; no network fall-through.
  - **Files:** (new) `crates/oxigeo-mobile-enhanced/src/offline_mvt.rs`.
  - **Why deferred:** Pending oxigeo-mvt extraction.

## Low Priority / Future (one-liners)
- [ ] Bluetooth/UWB peer-to-peer data sharing between devices.
- [ ] AR overlay pipeline for camera-based geospatial visualization (ARKit/ARCore).
- [ ] On-device ML model management (download, update, rollback).
- [ ] Location-aware cache prewarming using movement prediction.
- [ ] Accessibility features (VoiceOver/TalkBack hooks for map data).
- [ ] Delta sync for large raster datasets over cellular.
- [ ] watchOS/Wear OS companion data relay.

## Cross-crate dependencies
- **Blocks:** Mobile apps using both oxigeo-mobile and oxigeo-mobile-enhanced.
- **Blocked by:** oxigeo-mobile (FFI base layer), oxigeo-edge (sync infra), oxigeo-mvt (planned extraction), oxigeo-gpu (Metal interop).

## Recently completed (verbatim)
- (no `[x]` entries in prior TODO.md — see README.md for the optimization architecture)

---
*Last audited: 2026-05-17*
