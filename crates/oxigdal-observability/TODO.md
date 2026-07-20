# TODO: oxigdal-observability

> **Purpose:** OpenTelemetry-based observability — distributed tracing, metrics, logs, anomaly detection, SLO/alerting — for OxiGDAL.
> **Status (2026-05-16):** 20,096 LoC (src) · 157 tests (141 inline + 16 in tests/) · 0 real-code stubs
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [ ] Drop the crate-wide `#![allow(clippy::unwrap_used)]` and audit residual `unwrap_or` patterns.
  - **Verified gap:** `src/lib.rs:32-33` — `#![allow(clippy::unwrap_used)]` and `#![allow(clippy::panic)]`. These lint suppressions exist crate-wide despite the COOLJAPAN no-unwrap policy.
  - **Goal:** Remove both `allow` attributes; project compiles under `#![deny(clippy::unwrap_used)]` and `#![deny(clippy::panic)]`.
  - **Design:** `grep -rn "\.unwrap()" src --include='*.rs'` currently returns no naked `unwrap()` calls; only `partial_cmp(...).unwrap_or(std::cmp::Ordering::Equal)` patterns in `anomaly/advanced.rs` (lines 72, 80, 1120, 1121, 1137, 1173, 1217). Those are `unwrap_or`, which is policy-compliant. Removing the lint allow attributes should be effectively a no-op build-wise; verify by running `cargo clippy -p oxigdal-observability -- -D clippy::unwrap_used -D clippy::panic`.
  - **Files:** `src/lib.rs:31-33` (delete the two `allow` lines, keep `#![warn(missing_docs)]`).
  - **Tests:** None — covered by clippy.
  - **Risk:** A latent `.unwrap()` may surface in a code path the grep missed (e.g., inside a macro expansion); fix as it appears.
  - **Prerequisites:** None.

- [ ] W3C Trace Context propagation (`traceparent` / `tracestate`) wired into outgoing reqwest calls.
  - **Verified gap:** `opentelemetry::global` is used in `src/telemetry/traces.rs` for span export, but no module installs a `TraceContextPropagator` nor injects `traceparent` headers into the `reqwest::Client` used in `src/exporters/*`. Per W3C Trace Context Level 2 (W3C TR-trace-context-2) and OpenTelemetry semantic conventions §http.client.* a compliant client MUST inject these headers.
  - **Goal:** Outgoing HTTP requests from the crate's exporters and any caller-supplied `reqwest::Client` adapter carry valid `traceparent: 00-<trace_id>-<span_id>-<flags>` plus optional `tracestate`. Incoming HTTP context extracted symmetrically.
  - **Design:** On telemetry init (`telemetry/mod.rs`), call `opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new())`. Provide `pub fn inject_context_into_headers(span: &Context, headers: &mut HeaderMap)` and `extract_context_from_headers(headers: &HeaderMap) -> Context`. Document a `reqwest_middleware::Middleware` integration.
  - **Files:** `src/telemetry/mod.rs`, new `src/tracing/distributed/propagator.rs` (the `tracing/distributed/` directory already exists with type traits).
  - **Tests:** (proposed) `test_traceparent_header_format_w3c_compliant`, `test_inject_and_extract_roundtrip`, `test_tracestate_preserved_through_proxy`.
  - **Risk:** Verify `tracing/distributed/` directory is not already wired — file scan shows trait shells (`baggagemetadata_traits.rs`, `otelheaderinjector_traits.rs`, `baggagemanager_traits.rs`, `alwaysonsampler_traits.rs`) but no concrete propagator impl visible from `lib.rs` re-exports.
  - **Prerequisites:** None.

- [ ] Health-check endpoint with dependency status (PostGIS, S3, Redis...).
  - **Goal:** `HealthCheckManager::aggregate()` returns structured `HealthStatus { service, dependencies: Vec<DependencyHealth> }` JSON suitable for `/health` probe.
  - **Design:** Existing `src/health` module returns `HealthStatus`; extend with `Dependency { name, kind: enum, status, latency_ms, last_check }`. Concrete probes: HTTP (via reqwest), TCP (tokio), database ping (via crate-specific adapters). Optional dependency probes feature-flagged.
  - **Files:** `src/health/mod.rs` and new `src/health/dependencies.rs`.
  - **Tests:** (proposed) `test_health_aggregator_all_healthy`, `test_health_one_dependency_degraded_returns_degraded`, `test_health_critical_dependency_down_returns_unhealthy`.
  - **Risk:** Probe storms — rate-limit checks via `last_check` + TTL.
  - **Prerequisites:** None.

## Medium Priority
- [ ] Prometheus exposition format `/metrics` HTTP endpoint with geospatial-specific gauges.
  - **Goal:** axum handler returning text/plain per Prometheus exposition format 0.0.4 with raster_ops_total, vector_ops_total, cache_hit_ratio, tile_render_seconds histogram.
  - **Files:** New `src/exporters/prometheus_http.rs`. `prometheus` crate already in deps.
  - **Why deferred:** Stand-alone HTTP responder; consumers usually mount via their own router (`oxigdal-gateway` for instance).

- [ ] Structured logging with correlation IDs across crate boundaries.
  - **Goal:** `tracing` events emit `trace_id`/`span_id`/`request_id` consistently.
  - **Files:** `src/correlation/mod.rs` (already exists; verify field naming matches OTel semconv).

- [ ] Grafana dashboard JSON templates for raster/vector/I/O metrics.
  - **Files:** New `dashboards/grafana/*.json`. Documentation crate.

- [ ] Multi-window SLO burn-rate alerting (Google SRE workbook §5).
  - **Files:** `src/slo/` (exists; needs window-pair calculator).

- [ ] Alert routing webhook layer (PagerDuty Events API v2, Slack incoming webhook, generic JSON).
  - **Files:** `src/alerting/` (alerting framework exists; concrete senders needed).

- [ ] StatsD line-protocol exporter.
  - **Files:** New `src/exporters/statsd.rs`.

- [ ] InfluxDB line-protocol exporter.
  - **Files:** New `src/exporters/influxdb.rs`.

- [ ] Flamegraph profiling integration (existing `src/profiling/`).

- [ ] Metric cardinality cap (label-explosion guard).

- [ ] Log sampling for high-throughput operations (rate-based + adaptive).

## Low Priority / Future (one-liners)
- [ ] CloudWatch Embedded Metrics Format exporter.
- [ ] Jaeger-compatible export via OTLP (post RUSTSEC-2025-0123; see Cargo.toml note).
- [ ] Custom Grafana plugin for geospatial map metric visualization.
- [ ] Per-operation cost attribution (cloud resource tracking).
- [ ] OpenTelemetry Collector configuration auto-generation.
- [ ] Continuous profiling (pprof-compatible format).

## Cross-crate dependencies
- **Blocks:** All other crates (universal consumer of tracing/metrics).
- **Blocked by:** None.

## Recently completed (verbatim)
*No prior `[x]` entries — slate was empty.*

---
*Last audited: 2026-05-16*
