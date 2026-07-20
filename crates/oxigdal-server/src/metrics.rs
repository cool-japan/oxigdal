//! Prometheus-compatible metrics registry and text-exposition encoder.
//!
//! This module is the single source of truth for every metric name the ops tooling shipped
//! under `monitoring/` (Prometheus scrape config, alert rules, Grafana dashboard) and `k8s/`
//! (Deployment `prometheus.io/*` annotations, the `metrics` Service port) assumes the server
//! emits. If a metric name is added here it MUST also be documented in
//! `monitoring/alerts.yml` / `monitoring/grafana-dashboard.json`, and vice versa - do not let
//! the two drift apart again.
//!
//! Design notes:
//! - HTTP-level metrics (`http_requests_total`, `http_request_duration_seconds_*`,
//!   `http_connections_active`) are recorded centrally by [`track_http_metrics`], an Axum
//!   middleware applied to every route via `Router::route_layer`. This avoids scattering
//!   instrumentation across the WMS/WMTS/XYZ handler modules.
//! - Business metrics (`tile_generation_total`, `tile_generation_failures_total`,
//!   `wms_requests_total`) are derived from the same middleware by classifying the matched
//!   route template: `/wms*` counts as a WMS request, `/wmts*` and `/tiles*` count as tile
//!   generation. This is a deliberate simplification - it counts at the HTTP boundary rather
//!   than inside each renderer - but it is accurate for the failure/success signal the alert
//!   rules key on (`HighTileGenerationFailures`, `HighWMSFailures`).
//! - Cache metrics (`cache_hits_total`, `cache_requests_total`) are read directly from
//!   [`crate::cache::TileCache::stats`] at scrape time rather than duplicated into a second
//!   counter, so there is exactly one source of truth for cache accounting.
//! - Process metrics (`process_cpu_seconds_total`, `process_resident_memory_bytes`, ...) are
//!   sampled from the OS at scrape time via `sysinfo`, which is Pure Rust and already a
//!   workspace dependency used elsewhere in the ecosystem.

use crate::cache::CacheStats;
use axum::extract::{MatchedPath, Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

/// Histogram bucket upper bounds (seconds) for `http_request_duration_seconds`.
///
/// Matches the default bucket layout used by most Prometheus client libraries, which is
/// what the `histogram_quantile(...)` expressions in `monitoring/alerts.yml` and
/// `monitoring/grafana-dashboard.json` are tuned against.
const DURATION_BUCKETS: [f64; 11] = [
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

/// Cumulative-bucket histogram for request-duration observations.
#[derive(Debug, Default, Clone)]
struct DurationHistogram {
    /// Cumulative count of observations `<= DURATION_BUCKETS[i]`, one slot per bucket.
    bucket_counts: [u64; DURATION_BUCKETS.len()],
    /// Sum of all observed durations, in seconds.
    sum: f64,
    /// Total number of observations (equivalent to the `+Inf` bucket).
    count: u64,
}

impl DurationHistogram {
    fn observe(&mut self, seconds: f64) {
        for (bound, bucket) in DURATION_BUCKETS.iter().zip(self.bucket_counts.iter_mut()) {
            if seconds <= *bound {
                *bucket += 1;
            }
        }
        self.sum += seconds;
        self.count += 1;
    }
}

/// Route classification used to attribute business-level metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RouteClass {
    Wms,
    Tile,
    Other,
}

fn classify_route(path: &str) -> RouteClass {
    if path.starts_with("/wms") {
        RouteClass::Wms
    } else if path.starts_with("/wmts") || path.starts_with("/tiles") {
        RouteClass::Tile
    } else {
        RouteClass::Other
    }
}

/// Mutable metric state, guarded by a single mutex.
///
/// Request volume on a tile server is dominated by I/O (raster decode, PNG/JPEG encode), so a
/// coarse-grained mutex around bookkeeping counters is not a meaningful bottleneck; it keeps
/// the implementation simple and, crucially, panic-free (`Mutex::lock` errors are handled
/// explicitly rather than via `.unwrap()`).
#[derive(Debug, Default)]
struct MetricsInner {
    /// `(method, matched_path, status_code) -> count`
    http_requests_total: HashMap<(String, String, u16), u64>,
    /// `(method, matched_path) -> histogram`
    http_request_duration_seconds: HashMap<(String, String), DurationHistogram>,
    /// `status ("ok" | "error") -> count`
    wms_requests_total: HashMap<&'static str, u64>,
    tile_generation_total: u64,
    tile_generation_failures_total: u64,
}

/// Application-wide Prometheus metrics registry.
///
/// Cheap to clone (internally `Arc`-backed), so it can be stored directly as Axum router
/// state and shared between the main app router and the dedicated `/metrics` router.
#[derive(Debug, Clone)]
pub struct AppMetrics {
    inner: Arc<Mutex<MetricsInner>>,
    connections_active: Arc<AtomicI64>,
    process_start: Instant,
    process_start_unix: u64,
}

impl Default for AppMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl AppMetrics {
    /// Create a new, empty metrics registry, capturing the current time as process start.
    pub fn new() -> Self {
        let process_start_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Self {
            inner: Arc::new(Mutex::new(MetricsInner::default())),
            connections_active: Arc::new(AtomicI64::new(0)),
            process_start: Instant::now(),
            process_start_unix,
        }
    }

    /// How long the process has been running.
    pub fn uptime(&self) -> std::time::Duration {
        self.process_start.elapsed()
    }

    fn record_http_request(&self, method: &str, path: &str, status: u16, duration_secs: f64) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };

        *inner
            .http_requests_total
            .entry((method.to_string(), path.to_string(), status))
            .or_insert(0) += 1;

        inner
            .http_request_duration_seconds
            .entry((method.to_string(), path.to_string()))
            .or_default()
            .observe(duration_secs);
    }

    fn record_route_outcome(&self, class: RouteClass, success: bool) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };

        match class {
            RouteClass::Wms => {
                let key = if success { "ok" } else { "error" };
                *inner.wms_requests_total.entry(key).or_insert(0) += 1;
            }
            RouteClass::Tile => {
                inner.tile_generation_total += 1;
                if !success {
                    inner.tile_generation_failures_total += 1;
                }
            }
            RouteClass::Other => {}
        }
    }

    fn connections_inc(&self) {
        self.connections_active.fetch_add(1, Ordering::Relaxed);
    }

    fn connections_dec(&self) {
        self.connections_active.fetch_sub(1, Ordering::Relaxed);
    }

    /// Render the full Prometheus text-exposition payload, folding in the live cache
    /// statistics supplied by the caller (the cache itself is not owned by this registry).
    pub fn render_prometheus(&self, cache_stats: &CacheStats) -> String {
        let mut out = String::new();

        write_process_metrics(&mut out, self.process_start_unix);

        let _ = writeln!(out, "# HELP oxigdal_version_info Build version metadata.");
        let _ = writeln!(out, "# TYPE oxigdal_version_info gauge");
        let _ = writeln!(
            out,
            "oxigdal_version_info{{version=\"{}\"}} 1",
            escape_label(env!("CARGO_PKG_VERSION"))
        );

        let _ = writeln!(
            out,
            "# HELP http_connections_active Number of in-flight HTTP requests."
        );
        let _ = writeln!(out, "# TYPE http_connections_active gauge");
        let _ = writeln!(
            out,
            "http_connections_active {}",
            self.connections_active.load(Ordering::Relaxed).max(0)
        );

        if let Ok(inner) = self.inner.lock() {
            render_http_requests_total(&mut out, &inner.http_requests_total);
            render_http_duration(&mut out, &inner.http_request_duration_seconds);
            render_wms_requests_total(&mut out, &inner.wms_requests_total);

            let _ = writeln!(
                out,
                "# HELP tile_generation_total Total number of tile render attempts."
            );
            let _ = writeln!(out, "# TYPE tile_generation_total counter");
            let _ = writeln!(out, "tile_generation_total {}", inner.tile_generation_total);

            let _ = writeln!(
                out,
                "# HELP tile_generation_failures_total Total number of failed tile render attempts."
            );
            let _ = writeln!(out, "# TYPE tile_generation_failures_total counter");
            let _ = writeln!(
                out,
                "tile_generation_failures_total {}",
                inner.tile_generation_failures_total
            );
        }

        let _ = writeln!(
            out,
            "# HELP cache_hits_total Total number of tile cache hits."
        );
        let _ = writeln!(out, "# TYPE cache_hits_total counter");
        let _ = writeln!(out, "cache_hits_total {}", cache_stats.hits);

        let _ = writeln!(
            out,
            "# HELP cache_requests_total Total number of tile cache lookups (hits + misses)."
        );
        let _ = writeln!(out, "# TYPE cache_requests_total counter");
        let _ = writeln!(
            out,
            "cache_requests_total {}",
            cache_stats.hits + cache_stats.misses
        );

        out
    }
}

fn escape_label(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn render_http_requests_total(out: &mut String, counters: &HashMap<(String, String, u16), u64>) {
    let _ = writeln!(
        out,
        "# HELP http_requests_total Total number of HTTP requests."
    );
    let _ = writeln!(out, "# TYPE http_requests_total counter");
    let mut rows: Vec<_> = counters.iter().collect();
    rows.sort_by(|a, b| a.0.cmp(b.0));
    for ((method, path, status), count) in rows {
        let _ = writeln!(
            out,
            "http_requests_total{{method=\"{}\",path=\"{}\",status=\"{}\"}} {}",
            escape_label(method),
            escape_label(path),
            status,
            count
        );
    }
}

fn render_http_duration(
    out: &mut String,
    histograms: &HashMap<(String, String), DurationHistogram>,
) {
    let _ = writeln!(
        out,
        "# HELP http_request_duration_seconds HTTP request duration in seconds."
    );
    let _ = writeln!(out, "# TYPE http_request_duration_seconds histogram");
    let mut rows: Vec<_> = histograms.iter().collect();
    rows.sort_by(|a, b| a.0.cmp(b.0));
    for ((method, path), hist) in rows {
        for (bound, cumulative) in DURATION_BUCKETS.iter().zip(hist.bucket_counts.iter()) {
            let _ = writeln!(
                out,
                "http_request_duration_seconds_bucket{{method=\"{}\",path=\"{}\",le=\"{}\"}} {}",
                escape_label(method),
                escape_label(path),
                bound,
                cumulative
            );
        }
        let _ = writeln!(
            out,
            "http_request_duration_seconds_bucket{{method=\"{}\",path=\"{}\",le=\"+Inf\"}} {}",
            escape_label(method),
            escape_label(path),
            hist.count
        );
        let _ = writeln!(
            out,
            "http_request_duration_seconds_sum{{method=\"{}\",path=\"{}\"}} {}",
            escape_label(method),
            escape_label(path),
            hist.sum
        );
        let _ = writeln!(
            out,
            "http_request_duration_seconds_count{{method=\"{}\",path=\"{}\"}} {}",
            escape_label(method),
            escape_label(path),
            hist.count
        );
    }
}

fn render_wms_requests_total(out: &mut String, counters: &HashMap<&'static str, u64>) {
    let _ = writeln!(
        out,
        "# HELP wms_requests_total Total number of WMS requests by outcome."
    );
    let _ = writeln!(out, "# TYPE wms_requests_total counter");
    for status in ["ok", "error"] {
        let count = counters.get(status).copied().unwrap_or(0);
        let _ = writeln!(out, "wms_requests_total{{status=\"{}\"}} {}", status, count);
    }
}

/// Sample current-process OS metrics via `sysinfo` and render them.
///
/// Never panics: if the current PID or process table lookup fails (which should not happen
/// in practice, but `sysinfo` returns `Result`/`Option` rather than guaranteeing success), the
/// process metric families are simply omitted from this scrape rather than crashing the
/// `/metrics` endpoint.
fn write_process_metrics(out: &mut String, process_start_unix: u64) {
    let Ok(pid) = sysinfo::get_current_pid() else {
        return;
    };

    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(std::slice::from_ref(&pid)),
        true,
        ProcessRefreshKind::everything(),
    );

    let Some(process) = system.process(pid) else {
        return;
    };

    let cpu_seconds_total = process.accumulated_cpu_time() as f64 / 1000.0;
    let resident_memory_bytes = process.memory();
    let virtual_memory_bytes = process.virtual_memory();
    let open_fds = process.open_files();
    let max_fds = process.open_files_limit();

    let _ = writeln!(
        out,
        "# HELP process_cpu_seconds_total Total user and system CPU time spent, in seconds."
    );
    let _ = writeln!(out, "# TYPE process_cpu_seconds_total counter");
    let _ = writeln!(out, "process_cpu_seconds_total {}", cpu_seconds_total);

    let _ = writeln!(
        out,
        "# HELP process_resident_memory_bytes Resident memory size in bytes."
    );
    let _ = writeln!(out, "# TYPE process_resident_memory_bytes gauge");
    let _ = writeln!(
        out,
        "process_resident_memory_bytes {}",
        resident_memory_bytes
    );

    let _ = writeln!(
        out,
        "# HELP process_virtual_memory_bytes Virtual memory size in bytes."
    );
    let _ = writeln!(out, "# TYPE process_virtual_memory_bytes gauge");
    let _ = writeln!(out, "process_virtual_memory_bytes {}", virtual_memory_bytes);

    let _ = writeln!(
        out,
        "# HELP process_start_time_seconds Start time of the process since unix epoch, in seconds."
    );
    let _ = writeln!(out, "# TYPE process_start_time_seconds gauge");
    let _ = writeln!(out, "process_start_time_seconds {}", process_start_unix);

    if let Some(open_fds) = open_fds {
        let _ = writeln!(
            out,
            "# HELP process_open_fds Number of open file descriptors."
        );
        let _ = writeln!(out, "# TYPE process_open_fds gauge");
        let _ = writeln!(out, "process_open_fds {}", open_fds);
    }

    if let Some(max_fds) = max_fds {
        let _ = writeln!(
            out,
            "# HELP process_max_fds Maximum number of open file descriptors."
        );
        let _ = writeln!(out, "# TYPE process_max_fds gauge");
        let _ = writeln!(out, "process_max_fds {}", max_fds);
    }
}

/// Axum middleware that records HTTP-level and derived business metrics for every request.
///
/// Must be attached via [`axum::Router::route_layer`] (not `.layer`) so that
/// [`MatchedPath`] is populated - `.layer` wraps the router from the outside, before route
/// matching happens, and would only ever see `None`.
pub async fn track_http_metrics(
    State(metrics): State<AppMetrics>,
    matched_path: Option<MatchedPath>,
    req: Request,
    next: Next,
) -> Response {
    let method = req.method().to_string();
    let path = matched_path
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| req.uri().path().to_string());
    let class = classify_route(&path);

    metrics.connections_inc();
    let start = Instant::now();
    let response = next.run(req).await;
    let elapsed = start.elapsed();
    metrics.connections_dec();

    let status = response.status().as_u16();
    metrics.record_http_request(&method, &path, status, elapsed.as_secs_f64());
    metrics.record_route_outcome(class, response.status().is_success());

    response
}

/// Handler for `GET /metrics`, serving the Prometheus text-exposition payload.
pub async fn metrics_handler(
    State((metrics, cache)): State<(AppMetrics, crate::cache::TileCache)>,
) -> Response {
    let body = metrics.render_prometheus(&cache.stats());
    (
        axum::http::StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4",
        )],
        body,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::{CacheKey, TileCache, TileCacheConfig};

    #[test]
    fn test_duration_histogram_buckets_are_cumulative() {
        let mut hist = DurationHistogram::default();
        hist.observe(0.02);
        hist.observe(0.2);
        hist.observe(20.0);

        // 0.02 lands in every bucket >= 0.025.
        assert_eq!(hist.bucket_counts[2], 1); // 0.025
        // 0.2 additionally lands in every bucket >= 0.25.
        assert_eq!(hist.bucket_counts[6], 2); // 0.5
        // 20.0 never lands in any finite bucket.
        assert_eq!(hist.bucket_counts[10], 2); // 10.0
        assert_eq!(hist.count, 3);
        assert!((hist.sum - 20.22).abs() < 1e-9);
    }

    #[test]
    fn test_classify_route() {
        assert_eq!(classify_route("/wms"), RouteClass::Wms);
        assert_eq!(classify_route("/wms/capabilities"), RouteClass::Wms);
        assert_eq!(classify_route("/wmts"), RouteClass::Tile);
        assert_eq!(
            classify_route("/tiles/{layer}/{z}/{x}/{y}"),
            RouteClass::Tile
        );
        assert_eq!(classify_route("/health"), RouteClass::Other);
        assert_eq!(classify_route("/"), RouteClass::Other);
    }

    #[test]
    fn test_record_http_request_and_render_contains_metric_names() {
        let metrics = AppMetrics::new();
        metrics.record_http_request("GET", "/wms", 200, 0.01);
        metrics.record_route_outcome(RouteClass::Wms, true);

        let cache = TileCache::new(TileCacheConfig::default());
        let rendered = metrics.render_prometheus(&cache.stats());

        assert!(rendered.contains("http_requests_total"));
        assert!(rendered.contains("http_request_duration_seconds_bucket"));
        assert!(rendered.contains("http_request_duration_seconds_sum"));
        assert!(rendered.contains("http_request_duration_seconds_count"));
        assert!(rendered.contains("wms_requests_total{status=\"ok\"} 1"));
        assert!(rendered.contains("wms_requests_total{status=\"error\"} 0"));
        assert!(rendered.contains("cache_hits_total"));
        assert!(rendered.contains("cache_requests_total"));
        assert!(rendered.contains("tile_generation_total"));
        assert!(rendered.contains("tile_generation_failures_total"));
        assert!(rendered.contains("oxigdal_version_info"));
        assert!(rendered.contains("http_connections_active"));
    }

    #[test]
    fn test_tile_generation_counters() {
        let metrics = AppMetrics::new();
        metrics.record_route_outcome(RouteClass::Tile, true);
        metrics.record_route_outcome(RouteClass::Tile, false);

        let inner = metrics.inner.lock().expect("lock");
        assert_eq!(inner.tile_generation_total, 2);
        assert_eq!(inner.tile_generation_failures_total, 1);
    }

    #[test]
    fn test_cache_metrics_reflect_live_cache_stats() {
        let metrics = AppMetrics::new();
        let cache = TileCache::new(TileCacheConfig::default());
        let key = CacheKey::new("layer".to_string(), 0, 0, 0, "png".to_string());
        cache
            .put(key.clone(), bytes::Bytes::from_static(b"data"))
            .expect("put should succeed");
        cache.get(&key);
        cache.get(&CacheKey::new(
            "missing".to_string(),
            0,
            0,
            0,
            "png".to_string(),
        ));

        let rendered = metrics.render_prometheus(&cache.stats());
        assert!(rendered.contains("cache_hits_total 1"));
        assert!(rendered.contains("cache_requests_total 2"));
    }

    #[test]
    fn test_connections_active_gauge_never_reports_negative() {
        let metrics = AppMetrics::new();
        metrics.connections_dec(); // Would go negative without the floor.
        let cache = TileCache::new(TileCacheConfig::default());
        let rendered = metrics.render_prometheus(&cache.stats());
        assert!(rendered.contains("http_connections_active 0"));
    }
}
