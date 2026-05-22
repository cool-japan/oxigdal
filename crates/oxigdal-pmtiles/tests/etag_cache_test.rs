//! Tests for [`EtagCache`] (pure unit tests) and ETag-cache integration with
//! [`HttpPmTilesReader`] (mock-server tests).
//!
//! All code in this file is gated behind the `http-range` Cargo feature.

#![cfg(feature = "http-range")]
#![allow(clippy::expect_used)]

// ─────────────────────────────────────────────────────────────────────────────
// Pure unit tests — EtagCache in isolation (no network)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(feature = "http-range")]
mod tests {
    use oxigdal_pmtiles::EtagCache;

    // Helper: build a small payload with a recognisable pattern.
    fn make_payload(seed: u8, len: usize) -> Vec<u8> {
        (0..len).map(|i| seed.wrapping_add(i as u8)).collect()
    }

    /// A freshly-constructed cache must be empty.
    #[test]
    fn test_etag_cache_new_empty() {
        let cache = EtagCache::new(16);
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
        assert_eq!(cache.capacity(), 16);
    }

    /// Inserting a range and then getting it must return identical bytes and
    /// the stored ETag.
    #[test]
    fn test_etag_cache_insert_get_round_trip() {
        let mut cache = EtagCache::new(8);
        let payload = make_payload(0xAB, 64);
        cache.insert(1024, 64, "1024-64".into(), payload.clone());

        assert_eq!(cache.len(), 1);

        let result = cache.get(1024, 64);
        assert!(result.is_some(), "get must return Some after insert");
        let (data, etag) = result.expect("cache hit must return Some");
        assert_eq!(data, payload, "returned data must match inserted payload");
        assert_eq!(etag, "1024-64", "returned ETag must match inserted ETag");
    }

    /// Getting a key that was never inserted must return None.
    #[test]
    fn test_etag_cache_get_miss_returns_none() {
        let mut cache = EtagCache::new(8);
        cache.insert(0, 127, "0-127".into(), vec![0u8; 127]);

        // Offset 500 was never inserted.
        assert!(cache.get(500, 64).is_none());
        // Same offset but different length is also a miss.
        assert!(cache.get(0, 200).is_none());
    }

    /// When the cache is at capacity, inserting a new entry must evict the
    /// least-recently-used entry.
    #[test]
    fn test_etag_cache_eviction_when_at_capacity() {
        let mut cache = EtagCache::new(3);

        // Insert three entries — fills the cache.
        cache.insert(0, 10, "0-10".into(), vec![1u8; 10]);
        cache.insert(10, 10, "10-10".into(), vec![2u8; 10]);
        cache.insert(20, 10, "20-10".into(), vec![3u8; 10]);
        assert_eq!(cache.len(), 3);

        // Insert a fourth entry — must evict the LRU (offset=0, first inserted,
        // not accessed since).
        cache.insert(30, 10, "30-10".into(), vec![4u8; 10]);
        assert_eq!(
            cache.len(),
            3,
            "cache must not grow beyond max_entries after eviction"
        );

        // The first entry (offset=0) must have been evicted.
        assert!(
            cache.get(0, 10).is_none(),
            "LRU entry (offset=0) must be evicted"
        );

        // The newly inserted entry and the two most-recently-inserted survivors
        // must still be present.
        assert!(cache.get(10, 10).is_some(), "entry (10,10) must survive");
        assert!(cache.get(20, 10).is_some(), "entry (20,10) must survive");
        assert!(cache.get(30, 10).is_some(), "entry (30,10) must survive");
    }

    /// After a `get` call, the accessed entry must be moved to the
    /// most-recently-used position so it survives the next eviction.
    #[test]
    fn test_etag_cache_lru_order_updated_on_get() {
        let mut cache = EtagCache::new(3);

        cache.insert(0, 10, "0-10".into(), vec![1u8; 10]);
        cache.insert(10, 10, "10-10".into(), vec![2u8; 10]);
        cache.insert(20, 10, "20-10".into(), vec![3u8; 10]);

        // Touch the very first entry to move it to MRU position.
        let hit = cache.get(0, 10);
        assert!(hit.is_some(), "entry (0,10) must be a cache hit");

        // Now insert a fourth entry — the LRU should be (10,10) since (0,10) was
        // refreshed by the get above.
        cache.insert(30, 10, "30-10".into(), vec![4u8; 10]);

        // (0,10) was refreshed and must survive.
        assert!(
            cache.get(0, 10).is_some(),
            "recently-accessed entry (0,10) must not be evicted"
        );

        // (10,10) was the oldest un-accessed entry and must be evicted.
        assert!(
            cache.get(10, 10).is_none(),
            "LRU entry (10,10) must have been evicted"
        );

        // (20,10) and (30,10) must also survive.
        assert!(cache.get(20, 10).is_some(), "entry (20,10) must survive");
        assert!(cache.get(30, 10).is_some(), "entry (30,10) must survive");
    }

    /// After `clear()`, the cache must be empty and new inserts must work.
    #[test]
    fn test_etag_cache_clear_resets_size() {
        let mut cache = EtagCache::new(4);
        cache.insert(0, 50, "0-50".into(), vec![0u8; 50]);
        cache.insert(50, 50, "50-50".into(), vec![1u8; 50]);
        assert_eq!(cache.len(), 2);

        cache.clear();
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());

        // Must still honour capacity and insertions after a clear.
        cache.insert(100, 50, "100-50".into(), vec![2u8; 50]);
        assert_eq!(cache.len(), 1);
        assert!(cache.get(100, 50).is_some());
    }

    /// A zero-capacity cache must never store anything and must always return
    /// `None` from `get`.
    #[test]
    fn test_etag_cache_zero_capacity_never_stores() {
        let mut cache = EtagCache::new(0);
        assert_eq!(cache.capacity(), 0);

        cache.insert(0, 100, "0-100".into(), vec![0u8; 100]);
        assert_eq!(cache.len(), 0, "zero-capacity cache must not store entries");

        let result = cache.get(0, 100);
        assert!(
            result.is_none(),
            "zero-capacity cache must always return None from get"
        );

        // peek must also return None.
        assert!(cache.peek(0, 100).is_none());
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Integration tests — HttpPmTilesReader + EtagCache with a mock HTTP server
// ─────────────────────────────────────────────────────────────────────────────

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

use oxigdal_pmtiles::writer::PmTilesBuilder;
use oxigdal_pmtiles::{HttpPmTilesReader, TileType};

// ── Embedded mock HTTP/1.1 server (same pattern as http_reader_test.rs) ──────

/// Minimal HTTP/1.1 server that serves an in-memory byte slice via Range
/// requests.  Duplicated from `http_reader_test.rs` — the test crate does not
/// expose these helpers, so we reproduce them here.
struct MockServer {
    pub addr: std::net::SocketAddr,
    _accept_handle: thread::JoinHandle<()>,
}

impl MockServer {
    fn new(archive_bytes: Vec<u8>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
        let addr = listener.local_addr().expect("local_addr");
        let shared = Arc::new(archive_bytes);

        let accept_handle = thread::spawn(move || {
            for stream_result in listener.incoming() {
                match stream_result {
                    Ok(stream) => {
                        let data = Arc::clone(&shared);
                        thread::spawn(move || serve_connection(stream, data));
                    }
                    Err(_) => break,
                }
            }
        });

        MockServer {
            addr,
            _accept_handle: accept_handle,
        }
    }

    fn archive_url(&self) -> String {
        format!("http://{}/archive", self.addr)
    }
}

fn serve_connection(stream: TcpStream, data: Arc<Vec<u8>>) {
    let write_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };
    let mut reader = BufReader::new(stream);
    let mut writer = write_stream;

    loop {
        let mut request_line = String::new();
        match reader.read_line(&mut request_line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let request_line = request_line.trim_end().to_owned();
        if request_line.is_empty() {
            return;
        }

        let mut range_header: Option<(u64, u64)> = None;
        let mut connection_close = false;

        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
            let trimmed = line.trim_end();
            if trimmed.is_empty() {
                break;
            }
            let lower = trimmed.to_ascii_lowercase();
            if lower.starts_with("range: bytes=") {
                let rest = &trimmed["range: bytes=".len()..];
                if let Some((s, e)) = rest.split_once('-') {
                    let start: u64 = s.trim().parse().unwrap_or(0);
                    let end: u64 = e
                        .trim()
                        .parse()
                        .unwrap_or(data.len().saturating_sub(1) as u64);
                    range_header = Some((start, end));
                }
            }
            if lower.contains("connection: close") {
                connection_close = true;
            }
        }

        let path = request_line
            .split_whitespace()
            .nth(1)
            .unwrap_or("/")
            .to_owned();

        let keep_alive_hdr = if connection_close {
            "Connection: close\r\n"
        } else {
            "Connection: keep-alive\r\n"
        };

        match path.as_str() {
            "/archive" => {
                if !send_range_response(&mut writer, &data, range_header, keep_alive_hdr) {
                    return;
                }
            }
            _ => {
                let hdr =
                    format!("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n{keep_alive_hdr}\r\n");
                if writer.write_all(hdr.as_bytes()).is_err() || writer.flush().is_err() {
                    return;
                }
            }
        }

        if connection_close {
            return;
        }
    }
}

fn send_range_response(
    writer: &mut TcpStream,
    data: &[u8],
    range: Option<(u64, u64)>,
    extra_header: &str,
) -> bool {
    let total = data.len() as u64;
    let (start, end_inclusive) = match range {
        Some((s, e)) => (s.min(total), e.min(total.saturating_sub(1))),
        None => (0, total.saturating_sub(1)),
    };

    if start > end_inclusive || start >= total {
        let hdr = format!(
            "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\n{extra_header}\r\n"
        );
        return writer.write_all(hdr.as_bytes()).is_ok() && writer.flush().is_ok();
    }

    let body = &data[start as usize..=end_inclusive as usize];
    let status = if range.is_some() {
        "206 Partial Content"
    } else {
        "200 OK"
    };

    let hdr = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end_inclusive}/{total}\r\nContent-Type: application/octet-stream\r\n{extra_header}\r\n",
        body.len()
    );

    writer.write_all(hdr.as_bytes()).is_ok()
        && writer.write_all(body).is_ok()
        && writer.flush().is_ok()
}

// ── Archive helpers ───────────────────────────────────────────────────────────

fn build_single_tile_archive() -> Vec<u8> {
    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 0);
    builder
        .add_tile(0, 0, 0, b"tile-payload-z0")
        .expect("add_tile");
    builder.build().expect("build")
}

/// Build a multi-zoom archive with distinct tiles at z=0 through z=3 so that
/// the reader must issue separate byte-range fetches for header, root dir, and
/// at least one tile payload.
fn build_multi_tile_archive() -> Vec<u8> {
    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 3);
    // z=0
    builder
        .add_tile(0, 0, 0, b"tile-z0-0-0")
        .expect("add_tile z0");
    // z=1
    for x in 0..2u32 {
        for y in 0..2u32 {
            let payload = format!("tile-z1-{x}-{y}");
            builder
                .add_tile(1, x, y, payload.as_bytes())
                .expect("add_tile z1");
        }
    }
    // z=2
    for x in 0..4u32 {
        for y in 0..4u32 {
            let payload = format!("tile-z2-{x}-{y}");
            builder
                .add_tile(2, x, y, payload.as_bytes())
                .expect("add_tile z2");
        }
    }
    builder.build().expect("build")
}

// ─────────────────────────────────────────────────────────────────────────────
// Integration tests
// ─────────────────────────────────────────────────────────────────────────────

/// Test 8: After attaching an ETag cache and calling `get_tile` once,
/// `cached_byte_range_count()` must be > 0 (the reader cached the range(s) it
/// fetched).  On the second `get_tile` for the same tile the bytes are served
/// from the cache.
#[test]
fn test_http_reader_with_etag_cache_uses_cache_on_second_fetch() {
    let archive = build_single_tile_archive();
    let server = MockServer::new(archive);

    let mut reader = HttpPmTilesReader::open(&server.archive_url())
        .expect("open should succeed")
        .with_etag_cache(100);

    // Before any get_tile call, the cache must be empty.
    assert_eq!(
        reader.cached_byte_range_count(),
        0,
        "cache must be empty before first get_tile"
    );

    // First fetch — populates the cache with the tile's byte range.
    let tile_first = reader
        .get_tile(0, 0, 0)
        .expect("first get_tile must not error");
    assert!(tile_first.is_some(), "tile (0,0,0) must be present");

    let count_after_first = reader.cached_byte_range_count();
    assert!(
        count_after_first > 0,
        "cache must hold at least one entry after first get_tile (got {})",
        count_after_first
    );

    // Second fetch — served from cache; cache count must not drop.
    let tile_second = reader
        .get_tile(0, 0, 0)
        .expect("second get_tile must not error");
    assert!(tile_second.is_some(), "tile (0,0,0) must still be Some");
    assert_eq!(
        tile_first, tile_second,
        "both fetches must return identical tile bytes"
    );

    assert_eq!(
        reader.cached_byte_range_count(),
        count_after_first,
        "cache count must not change on cache-hit fetch"
    );
}

/// Test 9: A zero-capacity ETag cache must never store any byte ranges, so
/// `cached_byte_range_count()` must remain 0 after `get_tile`.
#[test]
fn test_http_reader_etag_cache_zero_capacity_disables_caching() {
    let archive = build_single_tile_archive();
    let server = MockServer::new(archive);

    let mut reader = HttpPmTilesReader::open(&server.archive_url())
        .expect("open should succeed")
        .with_etag_cache(0);

    let tile = reader.get_tile(0, 0, 0).expect("get_tile must not error");
    assert!(tile.is_some(), "tile (0,0,0) must be present");

    assert_eq!(
        reader.cached_byte_range_count(),
        0,
        "zero-capacity cache must never store entries (got {})",
        reader.cached_byte_range_count()
    );
}

/// Test 10: After fetching the header (at open time) and then at least one
/// tile, `cached_byte_range_count()` must be ≥ 1 (tile data range was
/// cached).  The header and root-dir fetches happen during `open` before the
/// cache is attached, so only ranges fetched via `fetch_range` after
/// `with_etag_cache` are counted.
///
/// We fetch tiles at three distinct coordinates to drive at least three
/// distinct byte-range fetches through the cached reader.
#[test]
fn test_http_reader_etag_cache_count_increases_with_distinct_ranges() {
    let archive = build_multi_tile_archive();
    let server = MockServer::new(archive);

    // Attach the cache after open, so header + root-dir fetches are not counted.
    let mut reader = HttpPmTilesReader::open(&server.archive_url())
        .expect("open should succeed")
        .with_etag_cache(256);

    // Fetch tiles at three distinct coordinates so the reader issues at least
    // three distinct fetch_range calls for their payloads.
    reader.get_tile(0, 0, 0).expect("tile z0");
    reader.get_tile(1, 0, 0).expect("tile z1/0/0");
    reader.get_tile(2, 0, 0).expect("tile z2/0/0");

    let count = reader.cached_byte_range_count();
    assert!(
        count >= 3,
        "cache must hold at least 3 byte ranges after 3 distinct tile fetches (got {})",
        count
    );
}
