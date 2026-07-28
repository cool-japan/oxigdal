//! HTTP range fetching via `web_sys` with byte/request accounting.
//!
//! Self-contained (deliberately NOT depending on `oxigeo-wasm`):
//! `fetch_range` issues a `Range` request and accepts only 206 responses
//! of the exact requested length, `content_length` probes
//! `bytes=0-0` and parses the `Content-Range` total, and `fetch_ranges`
//! runs bounded-concurrency batches while atomic counters track bytes
//! and request counts for the demo's honesty badges.
//!
//! Every request is timeout-bound via an `AbortController` (a stalled
//! connection is aborted after [`DEFAULT_TIMEOUT_MS`] rather than hanging the
//! query forever) and retried up to [`MAX_ATTEMPTS`] times with exponential
//! backoff on transient failure (network error, non-2xx status, short read).
//!
//! Implemented by WP C2 (GeoParquet Live lane); stub created by WP W0.

// The public surface here (content_length / fetch_ranges / the counter
// accessors) is consumed by the wasm-only `session` bindings landing in WP C4;
// until then some items look unused to the compiler.
#![allow(dead_code)]

use core::sync::atomic::{AtomicU64, Ordering};

use bytes::Bytes;
use futures::future::join_all;
use js_sys::Uint8Array;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen_futures::JsFuture;
use web_sys::{AbortController, Headers, Request, RequestInit, RequestMode, Response, Window};

use crate::coalesce::FetchRange;
use crate::error::GpqLiveError;

/// Default number of range requests kept in flight concurrently.
pub const DEFAULT_CONCURRENCY: usize = 6;

/// Default per-request timeout, in milliseconds, before a stalled range fetch
/// is aborted via `AbortController` rather than hanging the query forever.
pub const DEFAULT_TIMEOUT_MS: i32 = 30_000;

/// Number of attempts made for a single logical range before giving up
/// (i.e. up to `MAX_ATTEMPTS - 1` retries after the first try).
const MAX_ATTEMPTS: u32 = 3;

/// Base backoff delay between retry attempts; doubled on each subsequent
/// retry (200ms, 400ms, ...).
const RETRY_BASE_DELAY_MS: i32 = 200;

/// Cumulative bytes downloaded across every range fetch this session.
static BYTES_FETCHED: AtomicU64 = AtomicU64::new(0);
/// Cumulative number of HTTP requests issued this session (every retry
/// attempt is a real HTTP request and is counted).
static REQUEST_COUNT: AtomicU64 = AtomicU64::new(0);

/// Total bytes downloaded across all range fetches since the last reset.
#[must_use]
pub fn bytes_fetched_total() -> u64 {
    BYTES_FETCHED.load(Ordering::Relaxed)
}

/// Total HTTP requests issued since the last reset.
#[must_use]
pub fn request_count_total() -> u64 {
    REQUEST_COUNT.load(Ordering::Relaxed)
}

/// Resets both accounting counters to zero.
pub fn reset_counters() {
    BYTES_FETCHED.store(0, Ordering::Relaxed);
    REQUEST_COUNT.store(0, Ordering::Relaxed);
}

/// Per-call fetch accounting, local to a single [`fetch_range`] /
/// [`fetch_ranges`] invocation.
///
/// Unlike [`BYTES_FETCHED`] / [`REQUEST_COUNT`] (deliberately global,
/// cumulative counters backing `RemoteGeoParquet::stats()`), a `FetchStats`
/// value is never shared between calls, so it cannot be corrupted by another
/// query's concurrent or interleaved fetch activity — callers that need
/// accurate *per-query* telemetry should read the value returned here instead
/// of diffing the global counters across an `.await` point.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FetchStats {
    /// Bytes downloaded by this call (summed across every attempt, including
    /// retried/failed ones, mirroring how the global counter accrues).
    pub bytes: u64,
    /// HTTP requests issued by this call (one per attempt, including retries).
    pub requests: u64,
}

impl FetchStats {
    /// Folds `other`'s counts into `self` (used to combine per-attempt and
    /// per-range accounting into a single per-query total).
    pub fn merge(&mut self, other: FetchStats) {
        self.bytes += other.bytes;
        self.requests += other.requests;
    }
}

/// Builds a `bytes=start-end` header value for a `len`-byte range.
fn range_header(start: u64, len: u64) -> String {
    format!("bytes={}-{}", start, start + len.max(1) - 1)
}

/// A network / JS-level failure (no meaningful HTTP status) for `url`.
fn net_err(url: &str) -> GpqLiveError {
    GpqLiveError::Fetch {
        status: 0,
        url: url.to_string(),
    }
}

/// Resolves after `ms` milliseconds via `Window::setTimeout`.
async fn sleep_ms(ms: i32) {
    let Some(win) = web_sys::window() else {
        return;
    };
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        // A failure to schedule the timer just means we don't delay; the
        // caller still proceeds (best-effort backoff, not correctness-critical).
        let _ = win.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
    });
    let _ = JsFuture::from(promise).await;
}

/// Clears a pending `setTimeout` when dropped, so the abort-on-timeout
/// callback armed by [`start_fetch`] never fires once the request has
/// already settled (success or failure).
struct TimeoutGuard {
    window: Window,
    handle: i32,
    // Kept alive only to keep the JS closure it wraps alive until the timer
    // either fires or is cleared; never invoked directly by Rust.
    _closure: Closure<dyn FnMut()>,
}

impl Drop for TimeoutGuard {
    fn drop(&mut self) {
        self.window.clear_timeout_with_handle(self.handle);
    }
}

/// Initiates a GET with the given `Range` header, returning the fetch
/// `Promise` and a [`TimeoutGuard`] that aborts the request via
/// `AbortController` if it has not settled within `timeout_ms`.
///
/// Returning the promise (rather than awaiting it) lets the caller start several
/// requests before awaiting any of them, achieving bounded concurrency without
/// an executor dependency: the browser begins each fetch as soon as it is called.
fn start_fetch(
    url: &str,
    range: &str,
    timeout_ms: i32,
) -> Result<(js_sys::Promise, TimeoutGuard), GpqLiveError> {
    let win = web_sys::window().ok_or_else(|| net_err(url))?;
    let controller = AbortController::new().map_err(|_| net_err(url))?;
    let opts = RequestInit::new();
    opts.set_method("GET");
    opts.set_mode(RequestMode::Cors);
    opts.set_signal(Some(&controller.signal()));
    let headers = Headers::new().map_err(|_| net_err(url))?;
    headers.set("Range", range).map_err(|_| net_err(url))?;
    opts.set_headers(&headers);
    let request = Request::new_with_str_and_init(url, &opts).map_err(|_| net_err(url))?;
    let promise = win.fetch_with_request(&request);

    // Arm a one-shot timer that aborts the in-flight request if it has not
    // settled by `timeout_ms`; `TimeoutGuard::drop` cancels the timer once the
    // caller is done awaiting, so a fast response never triggers a stray abort.
    let closure: Closure<dyn FnMut()> = Closure::once(move || {
        controller.abort();
    });
    let handle = win
        .set_timeout_with_callback_and_timeout_and_arguments_0(
            closure.as_ref().unchecked_ref(),
            timeout_ms,
        )
        .map_err(|_| net_err(url))?;
    let guard = TimeoutGuard {
        window: win,
        handle,
        _closure: closure,
    };
    Ok((promise, guard))
}

/// Awaits a fetch `Promise` and downcasts the result to a [`Response`].
async fn await_response(promise: js_sys::Promise, url: &str) -> Result<Response, GpqLiveError> {
    let value = JsFuture::from(promise).await.map_err(|_| net_err(url))?;
    value.dyn_into::<Response>().map_err(|_| net_err(url))
}

/// Reads a response body to [`Bytes`], adding its length to the byte counter.
async fn read_body(resp: &Response, url: &str) -> Result<Bytes, GpqLiveError> {
    let ab_promise = resp.array_buffer().map_err(|_| net_err(url))?;
    let array_buffer = JsFuture::from(ab_promise).await.map_err(|_| net_err(url))?;
    let bytes = Uint8Array::new(&array_buffer).to_vec();
    BYTES_FETCHED.fetch_add(bytes.len() as u64, Ordering::Relaxed);
    Ok(Bytes::from(bytes))
}

/// Validates a response status, requiring a 206 (or a 200 whole-body).
fn check_status(resp: &Response, url: &str) -> Result<u16, GpqLiveError> {
    let status = resp.status();
    if status != 206 && status != 200 {
        return Err(GpqLiveError::Fetch {
            status,
            url: url.to_string(),
        });
    }
    Ok(status)
}

/// A single fetch attempt for `len` bytes starting at `start`: issues one
/// timeout-bound HTTP range request and validates its status + body length.
/// Always reports the request it made (and any bytes it actually read) via
/// the returned [`FetchStats`], regardless of whether the attempt ultimately
/// succeeded — this mirrors how the global counters accrue, so per-query and
/// cumulative totals stay consistent with each other.
async fn fetch_attempt(
    url: &str,
    start: u64,
    len: u64,
    timeout_ms: i32,
) -> (Result<Bytes, GpqLiveError>, FetchStats) {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut local = FetchStats {
        requests: 1,
        bytes: 0,
    };
    let (promise, guard) = match start_fetch(url, &range_header(start, len), timeout_ms) {
        Ok(v) => v,
        Err(e) => return (Err(e), local),
    };
    let resp = match await_response(promise, url).await {
        Ok(r) => r,
        Err(e) => return (Err(e), local),
    };
    drop(guard); // response settled; cancel the abort timer promptly.
    let status = match check_status(&resp, url) {
        Ok(s) => s,
        Err(e) => return (Err(e), local),
    };
    let body = match read_body(&resp, url).await {
        Ok(b) => b,
        Err(e) => return (Err(e), local),
    };
    local.bytes = body.len() as u64;
    if body.len() as u64 != len {
        return (
            Err(GpqLiveError::Fetch {
                status,
                url: url.to_string(),
            }),
            local,
        );
    }
    (Ok(body), local)
}

/// Fetches `len` bytes starting at `start`, retrying transient failures
/// (network error, rejected status, short read) up to [`MAX_ATTEMPTS`] times
/// with exponential backoff between attempts.
///
/// # Errors
/// Returns the last [`GpqLiveError`] encountered once every attempt has failed.
async fn fetch_with_retry(
    url: &str,
    start: u64,
    len: u64,
    timeout_ms: i32,
) -> Result<(Bytes, FetchStats), GpqLiveError> {
    let mut total = FetchStats::default();
    let mut last_err = None;
    for attempt in 0..MAX_ATTEMPTS {
        let (result, local) = fetch_attempt(url, start, len, timeout_ms).await;
        total.merge(local);
        match result {
            Ok(bytes) => return Ok((bytes, total)),
            Err(e) => {
                last_err = Some(e);
                if attempt + 1 < MAX_ATTEMPTS {
                    let delay = RETRY_BASE_DELAY_MS.saturating_mul(1 << attempt);
                    sleep_ms(delay).await;
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| net_err(url)))
}

/// Fetches exactly `len` bytes starting at `start` via an HTTP range request.
///
/// Accepts a `206 Partial Content` response of the exact requested length. A
/// `200 OK` (server ignored `Range` and returned the whole file) whose body
/// does not match `len` is rejected, as is any other status. Transient
/// failures are retried (see module docs); a request that never settles is
/// aborted after [`DEFAULT_TIMEOUT_MS`].
///
/// # Errors
/// Returns [`GpqLiveError::Fetch`] on a network failure, a rejected status, or a
/// body whose length differs from `len`, after exhausting all retry attempts.
pub async fn fetch_range(url: &str, start: u64, len: u64) -> Result<Bytes, GpqLiveError> {
    let (body, _stats) = fetch_with_retry(url, start, len, DEFAULT_TIMEOUT_MS).await?;
    Ok(body)
}

/// Fetches many byte ranges, keeping up to `max_concurrency` requests in flight.
///
/// Ranges are processed in batches; every request in a batch is polled at
/// least once (via `join_all`, which drives every future forward on the same
/// poll) before any is awaited to completion, so the browser runs them
/// concurrently. Results are returned in the same order as `ranges`, alongside
/// the [`FetchStats`] accrued by this call alone (immune to any other
/// concurrent query's fetch activity).
///
/// # Errors
/// Returns [`GpqLiveError::Fetch`] if any range fails to fetch or returns the
/// wrong number of bytes after exhausting all retry attempts.
pub async fn fetch_ranges(
    url: &str,
    ranges: &[FetchRange],
    max_concurrency: usize,
) -> Result<(Vec<Bytes>, FetchStats), GpqLiveError> {
    let concurrency = max_concurrency.max(1);
    let mut out = Vec::with_capacity(ranges.len());
    let mut stats = FetchStats::default();
    for batch in ranges.chunks(concurrency) {
        let futs = batch
            .iter()
            .map(|range| fetch_with_retry(url, range.start, range.len, DEFAULT_TIMEOUT_MS));
        for result in join_all(futs).await {
            let (body, local) = result?;
            stats.merge(local);
            out.push(body);
        }
    }
    Ok((out, stats))
}

/// Probes the total resource size via a `bytes=0-0` range request.
///
/// Parses the total from the `Content-Range: bytes 0-0/<total>` header, falling
/// back to `Content-Length` when the server answered a whole-body `200 OK`.
/// Transient failures are retried and the request is timeout-bound, matching
/// [`fetch_range`]'s reliability behaviour.
///
/// # Errors
/// Returns [`GpqLiveError::Fetch`] on a network failure, a rejected status, or
/// when neither header yields a total size, after exhausting all retry attempts.
pub async fn content_length(url: &str) -> Result<u64, GpqLiveError> {
    let mut last_err = None;
    for attempt in 0..MAX_ATTEMPTS {
        match content_length_attempt(url).await {
            Ok(total) => return Ok(total),
            Err(e) => {
                last_err = Some(e);
                if attempt + 1 < MAX_ATTEMPTS {
                    let delay = RETRY_BASE_DELAY_MS.saturating_mul(1 << attempt);
                    sleep_ms(delay).await;
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| net_err(url)))
}

/// A single `bytes=0-0` size-probe attempt.
async fn content_length_attempt(url: &str) -> Result<u64, GpqLiveError> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    let (promise, guard) = start_fetch(url, "bytes=0-0", DEFAULT_TIMEOUT_MS)?;
    let resp = await_response(promise, url).await?;
    drop(guard);
    let status = check_status(&resp, url)?;
    let headers = resp.headers();

    if let Ok(Some(content_range)) = headers.get("content-range")
        && let Some(total) = content_range
            .rsplit('/')
            .next()
            .and_then(|s| s.trim().parse::<u64>().ok())
    {
        return Ok(total);
    }

    if status == 200
        && let Ok(Some(content_length)) = headers.get("content-length")
        && let Ok(total) = content_length.trim().parse::<u64>()
    {
        return Ok(total);
    }

    Err(GpqLiveError::Fetch {
        status,
        url: url.to_string(),
    })
}
