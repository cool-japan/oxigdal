//! Integration tests for [`HttpPmTilesReader`].
//!
//! A minimal HTTP/1.1 server is embedded in this file.  It binds to
//! `127.0.0.1:0` (kernel-assigned port), parses `Range:` headers, and
//! serves bytes from an in-memory PMTiles archive.  Each TCP connection is
//! served in a dedicated thread with keep-alive loop support so that
//! `reqwest`'s connection-pool reuse works correctly.

#![cfg(feature = "http-range")]
#![allow(clippy::expect_used)]

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

use oxigdal_pmtiles::writer::PmTilesBuilder;
use oxigdal_pmtiles::{HttpPmTilesReader, PmTilesHeader, TileType};

// ─────────────────────────────────────────────────────────────────────────────
// Minimal embedded HTTP/1.1 server
// ─────────────────────────────────────────────────────────────────────────────

/// An embedded HTTP/1.1 server serving an in-memory byte slice.
///
/// Each incoming TCP connection is handled in a dedicated thread.  Within each
/// connection the server loops, handling one HTTP/1.1 request at a time, until
/// the peer closes the connection or a read error occurs.  This is necessary
/// because `reqwest` uses HTTP keep-alive by default and will send multiple
/// sequential requests on the same TCP connection.
///
/// Endpoints:
/// - `GET /archive` — returns the archive bytes or a sub-range (`Range:`).
/// - `GET /garbage` — returns 200 with 512 bytes of `0xFF`.
/// - Anything else — returns `404 Not Found`.
struct MockServer {
    /// The socket address the server is listening on.
    pub addr: std::net::SocketAddr,
    // Keeps the accept-loop thread alive for the server's lifetime.
    _accept_handle: thread::JoinHandle<()>,
}

impl MockServer {
    /// Spawn a mock server that serves `archive_bytes` on `/archive`.
    fn new(archive_bytes: Vec<u8>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
        let addr = listener.local_addr().expect("local_addr");
        let shared = Arc::new(archive_bytes);

        let accept_handle = thread::spawn(move || {
            for stream_result in listener.incoming() {
                match stream_result {
                    Ok(stream) => {
                        let data = Arc::clone(&shared);
                        // Serve each connection in its own thread so the accept
                        // loop is never blocked.
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

    /// Return the base URL (e.g. `http://127.0.0.1:PORT`).
    fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Return the URL for the archive endpoint.
    fn archive_url(&self) -> String {
        format!("{}/archive", self.base_url())
    }
}

/// Serve all HTTP/1.1 requests arriving on a single TCP connection, looping
/// until the peer closes or a read error occurs.
///
/// Responses include `Connection: keep-alive` unless the request contains
/// `Connection: close`, in which case the loop terminates after the response.
fn serve_connection(stream: TcpStream, data: Arc<Vec<u8>>) {
    // Clone the stream so we can own both a `BufReader` for reads and the raw
    // stream for writes.
    let write_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };
    let mut reader = BufReader::new(stream);
    let mut writer = write_stream;

    loop {
        // ── Parse request line ────────────────────────────────────────────────
        let mut request_line = String::new();
        match reader.read_line(&mut request_line) {
            Ok(0) | Err(_) => return, // EOF or error — close connection
            Ok(_) => {}
        }
        let request_line = request_line.trim_end().to_owned();
        if request_line.is_empty() {
            return; // Spurious blank line — close
        }

        // ── Parse headers ─────────────────────────────────────────────────────
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
                break; // End of headers
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

        // ── Dispatch ──────────────────────────────────────────────────────────
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
            "/garbage" => {
                let body: Vec<u8> = vec![0xFFu8; 512];
                let hdr = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\n{keep_alive_hdr}\r\n",
                    body.len()
                );
                if writer.write_all(hdr.as_bytes()).is_err()
                    || writer.write_all(&body).is_err()
                    || writer.flush().is_err()
                {
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

/// Write a 206 (or 200 for no Range header) partial-content response.
///
/// Returns `true` if the write succeeded, `false` on I/O error.
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

// ─────────────────────────────────────────────────────────────────────────────
// Archive builders
// ─────────────────────────────────────────────────────────────────────────────

/// Build a minimal archive with exactly one tile at `(z=0, x=0, y=0)`.
fn build_single_tile_archive() -> Vec<u8> {
    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 0);
    builder
        .add_tile(0, 0, 0, b"tile-payload-z0")
        .expect("add_tile");
    builder.build().expect("build")
}

/// Build a large archive with unique tiles across z=7 (16 384 tiles) that
/// forces leaf-directory creation (root directory exceeds 16 KiB threshold).
fn build_large_archive() -> Vec<u8> {
    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 7);
    let dim: u32 = 1 << 7; // 128 × 128 = 16 384 tiles at z=7
    for x in 0..dim {
        for y in 0..dim {
            // Unique payload per tile prevents run-length deduplication.
            let payload = format!("tile-z7-{x:04}-{y:04}");
            builder
                .add_tile(7, x, y, payload.as_bytes())
                .expect("add_tile");
        }
    }
    builder.build().expect("build")
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

/// Opening a remote archive should fetch the 127-byte header and produce a
/// reader whose `spec_version` equals 3.
#[test]
fn test_http_reader_open_fetches_header_bytes_0_to_127() {
    let archive = build_single_tile_archive();
    let server = MockServer::new(archive);

    let reader = HttpPmTilesReader::open(&server.archive_url())
        .expect("HttpPmTilesReader::open should succeed");

    assert_eq!(
        reader.header().spec_version,
        3,
        "spec_version must be 3 for a PMTiles v3 archive"
    );
}

/// After `open`, `root_directory()` must return at least one entry for a
/// non-empty archive.
#[test]
fn test_http_reader_open_fetches_root_directory_after_header() {
    let archive = build_single_tile_archive();
    let server = MockServer::new(archive);

    let reader = HttpPmTilesReader::open(&server.archive_url()).expect("open should succeed");

    assert!(
        !reader.root_directory().is_empty(),
        "root_directory() must be non-empty for a non-empty archive"
    );
}

/// Opening a URL that returns garbage bytes (no PMTiles magic) must produce
/// an error, not a panic.
#[test]
fn test_http_reader_open_invalid_magic_returns_error() {
    let garbage = vec![0xFFu8; 512];
    let server = MockServer::new(garbage);

    // `/garbage` path returns the 0xFF bytes.
    let url = format!("{}/garbage", server.base_url());
    let result = HttpPmTilesReader::open(&url);

    assert!(
        result.is_err(),
        "opening a non-PMTiles URL must return Err, got Ok"
    );
}

/// `get_tile(0, 0, 0)` must return `Some(payload)` for a tile that was added.
#[test]
fn test_http_reader_get_tile_returns_some_for_present_tile() {
    let expected = b"tile-payload-z0";
    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 0);
    builder.add_tile(0, 0, 0, expected).expect("add_tile");
    let archive = builder.build().expect("build");

    let server = MockServer::new(archive);
    let mut reader = HttpPmTilesReader::open(&server.archive_url()).expect("open should succeed");

    let tile = reader.get_tile(0, 0, 0).expect("get_tile should not error");
    assert!(tile.is_some(), "tile (0,0,0) must be Some");
    assert_eq!(
        tile.as_deref(),
        Some(expected.as_ref()),
        "tile payload must match original"
    );
}

/// `get_tile` for a coordinate that was never added must return `None`.
#[test]
fn test_http_reader_get_tile_returns_none_for_missing_tile() {
    // Archive has only z=0 tiles.
    let archive = build_single_tile_archive();
    let server = MockServer::new(archive);

    let mut reader = HttpPmTilesReader::open(&server.archive_url()).expect("open should succeed");

    let result = reader.get_tile(5, 10, 20).expect("get_tile must not error");
    assert!(
        result.is_none(),
        "tile (5,10,20) was never added; get_tile must return None"
    );
}

/// A large archive forces leaf-directory creation.  `get_tile` must still be
/// able to retrieve a high-zoom tile via the two-level directory walk.
#[test]
fn test_http_reader_get_tile_walks_leaf_directory_for_high_zoom() {
    let archive = build_large_archive();

    // Sanity: the archive must actually contain leaf directories.
    let header = PmTilesHeader::parse(&archive).expect("parse header");
    assert!(
        header.leaf_dirs_length > 0,
        "archive must contain leaf directories (leaf_dirs_length = {})",
        header.leaf_dirs_length
    );

    let server = MockServer::new(archive);
    let mut reader = HttpPmTilesReader::open(&server.archive_url()).expect("open should succeed");

    let tile = reader
        .get_tile(7, 63, 63)
        .expect("get_tile should not error");

    assert!(tile.is_some(), "tile (7,63,63) must be present");
    let tile_bytes = tile.expect("tile (7,63,63) is Some, checked above");
    assert!(!tile_bytes.is_empty(), "tile payload must be non-empty");
}

/// Accessing tiles in distinct leaf directories must grow `cached_leaf_count`.
#[test]
fn test_http_reader_leaf_cache_grows_with_distinct_leaf_fetches() {
    let archive = build_large_archive();
    let server = MockServer::new(archive);

    let mut reader = HttpPmTilesReader::open(&server.archive_url()).expect("open should succeed");

    // Tiles spread across the Hilbert curve so they likely sit in distinct
    // leaf pages.
    reader.get_tile(7, 0, 0).expect("tile 7/0/0");
    reader.get_tile(7, 127, 127).expect("tile 7/127/127");
    reader.get_tile(7, 64, 0).expect("tile 7/64/0");
    reader.get_tile(7, 0, 64).expect("tile 7/0/64");

    assert!(
        reader.cached_leaf_count() > 1,
        "cached_leaf_count must grow when distinct leaves are accessed; got {}",
        reader.cached_leaf_count()
    );
}

/// Fetching the same tile twice must not grow the leaf cache beyond its
/// count after the first fetch.
#[test]
fn test_http_reader_leaf_cache_does_not_grow_on_repeat_fetch() {
    let archive = build_large_archive();
    let server = MockServer::new(archive);

    let mut reader = HttpPmTilesReader::open(&server.archive_url()).expect("open should succeed");

    reader.get_tile(7, 10, 10).expect("first fetch");
    let count_after_first = reader.cached_leaf_count();
    assert!(
        count_after_first >= 1,
        "leaf cache must contain at least one entry after first fetch"
    );

    reader.get_tile(7, 10, 10).expect("second fetch");
    assert_eq!(
        reader.cached_leaf_count(),
        count_after_first,
        "leaf cache must not grow on repeated access to the same tile"
    );
}

/// Opening a URL that resolves to a 404 response must return `Err`.
#[test]
fn test_http_reader_fetch_byte_range_returns_io_error_on_404() {
    let archive = build_single_tile_archive();
    let server = MockServer::new(archive);

    let url = format!("{}/nonexistent", server.base_url());
    let result = HttpPmTilesReader::open(&url);

    assert!(result.is_err(), "opening a 404 URL must return Err, got Ok");
}

/// `header()` must return the parsed header with `root_dir_offset >= 127`
/// (the header occupies bytes 0..127 so the root directory must start after).
#[test]
fn test_http_reader_header_accessor_returns_parsed_header() {
    let archive = build_single_tile_archive();
    let server = MockServer::new(archive);

    let reader = HttpPmTilesReader::open(&server.archive_url()).expect("open should succeed");

    let hdr = reader.header();

    assert!(
        hdr.root_dir_offset >= 127,
        "root_dir_offset ({}) must be >= 127 (header size)",
        hdr.root_dir_offset
    );

    assert!(
        hdr.root_dir_length > 0,
        "root_dir_length must be > 0 for a non-empty archive"
    );
}
