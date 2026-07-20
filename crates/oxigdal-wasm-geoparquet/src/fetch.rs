//! HTTP range fetching via `web_sys` with byte/request accounting.
//!
//! Self-contained (deliberately NOT depending on `oxigdal-wasm`):
//! `fetch_range` issues a `Range` request and accepts only 206 responses
//! of the exact requested length, `content_length` probes
//! `bytes=0-0` and parses the `Content-Range` total, and `fetch_ranges`
//! runs bounded-concurrency batches while atomic counters track bytes
//! and request counts for the demo's honesty badges.
//!
//! Implemented by WP C2 (GeoParquet Live lane); stub created by WP W0.

// The public surface here (content_length / fetch_ranges / the counter
// accessors) is consumed by the wasm-only `session` bindings landing in WP C4;
// until then some items look unused to the compiler.
#![allow(dead_code)]

use core::sync::atomic::{AtomicU64, Ordering};

use bytes::Bytes;
use js_sys::Uint8Array;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Headers, Request, RequestInit, RequestMode, Response};

use crate::coalesce::FetchRange;
use crate::error::GpqLiveError;

/// Default number of range requests kept in flight concurrently.
pub const DEFAULT_CONCURRENCY: usize = 6;

/// Cumulative bytes downloaded across every range fetch this session.
static BYTES_FETCHED: AtomicU64 = AtomicU64::new(0);
/// Cumulative number of HTTP requests issued this session.
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

/// Initiates a GET with the given `Range` header, returning the fetch `Promise`.
///
/// Returning the promise (rather than awaiting it) lets the caller start several
/// requests before awaiting any of them, achieving bounded concurrency without
/// an executor dependency: the browser begins each fetch as soon as it is called.
fn start_fetch(url: &str, range: &str) -> Result<js_sys::Promise, GpqLiveError> {
    let win = web_sys::window().ok_or_else(|| net_err(url))?;
    let opts = RequestInit::new();
    opts.set_method("GET");
    opts.set_mode(RequestMode::Cors);
    let headers = Headers::new().map_err(|_| net_err(url))?;
    headers.set("Range", range).map_err(|_| net_err(url))?;
    opts.set_headers(&headers);
    let request = Request::new_with_str_and_init(url, &opts).map_err(|_| net_err(url))?;
    Ok(win.fetch_with_request(&request))
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

/// Fetches exactly `len` bytes starting at `start` via an HTTP range request.
///
/// Accepts a `206 Partial Content` response of the exact requested length. A
/// `200 OK` (server ignored `Range` and returned the whole file) whose body
/// does not match `len` is rejected, as is any other status.
///
/// # Errors
/// Returns [`GpqLiveError::Fetch`] on a network failure, a rejected status, or a
/// body whose length differs from `len`.
pub async fn fetch_range(url: &str, start: u64, len: u64) -> Result<Bytes, GpqLiveError> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    let promise = start_fetch(url, &range_header(start, len))?;
    let resp = await_response(promise, url).await?;
    let status = check_status(&resp, url)?;
    let body = read_body(&resp, url).await?;
    if body.len() as u64 != len {
        return Err(GpqLiveError::Fetch {
            status,
            url: url.to_string(),
        });
    }
    Ok(body)
}

/// Fetches many byte ranges, keeping up to `max_concurrency` requests in flight.
///
/// Ranges are processed in batches; every request in a batch is started before
/// any is awaited, so the browser runs them concurrently. Results are returned
/// in the same order as `ranges`.
///
/// # Errors
/// Returns [`GpqLiveError::Fetch`] if any range fails to fetch or returns the
/// wrong number of bytes.
pub async fn fetch_ranges(
    url: &str,
    ranges: &[FetchRange],
    max_concurrency: usize,
) -> Result<Vec<Bytes>, GpqLiveError> {
    let concurrency = max_concurrency.max(1);
    let mut out = Vec::with_capacity(ranges.len());
    for batch in ranges.chunks(concurrency) {
        // Start every request in the batch so they overlap on the network.
        let mut pending = Vec::with_capacity(batch.len());
        for range in batch {
            REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
            let promise = start_fetch(url, &range_header(range.start, range.len))?;
            pending.push((promise, range.len));
        }
        for (promise, len) in pending {
            let resp = await_response(promise, url).await?;
            let status = check_status(&resp, url)?;
            let body = read_body(&resp, url).await?;
            if body.len() as u64 != len {
                return Err(GpqLiveError::Fetch {
                    status,
                    url: url.to_string(),
                });
            }
            out.push(body);
        }
    }
    Ok(out)
}

/// Probes the total resource size via a `bytes=0-0` range request.
///
/// Parses the total from the `Content-Range: bytes 0-0/<total>` header, falling
/// back to `Content-Length` when the server answered a whole-body `200 OK`.
///
/// # Errors
/// Returns [`GpqLiveError::Fetch`] on a network failure, a rejected status, or
/// when neither header yields a total size.
pub async fn content_length(url: &str) -> Result<u64, GpqLiveError> {
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    let promise = start_fetch(url, "bytes=0-0")?;
    let resp = await_response(promise, url).await?;
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
