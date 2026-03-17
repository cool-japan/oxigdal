//! HTTP/2 server push hints, ETag validation, and unified tile serving logic.
//!
//! Provides `PushHint` (Link header builder), `PushPolicy` (neighbour-based
//! push hint generator), `ETagValidator` (conditional-request helpers), and
//! `TileServer` (cache + push policy integration).

use super::cache::{CacheStats, CachedTile, TileCache, TileFormat, TileKey, TilePrefetcher};

// ── PushRel ───────────────────────────────────────────────────────────────────

/// The `rel` attribute of an HTTP Link header push hint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushRel {
    /// `rel=preload` — browser should load this resource soon.
    Preload,
    /// `rel=prefetch` — browser may load this resource in the background.
    Prefetch,
    /// `rel=preconnect` — browser should open a connection to the origin.
    Preconnect,
}

impl PushRel {
    fn as_str(&self) -> &'static str {
        match self {
            PushRel::Preload => "preload",
            PushRel::Prefetch => "prefetch",
            PushRel::Preconnect => "preconnect",
        }
    }
}

// ── PushHint ──────────────────────────────────────────────────────────────────

/// An HTTP/2 server push hint represented as a `Link` header entry.
#[derive(Debug, Clone)]
pub struct PushHint {
    /// The URL of the resource to push.
    pub url: String,
    /// The link relation type.
    pub rel: PushRel,
    /// Optional MIME type (`type` attribute).
    pub type_: Option<String>,
    /// Optional resource type (`as` attribute: `"image"`, `"fetch"`, etc.).
    pub as_: Option<String>,
    /// Whether to add the `crossorigin` attribute.
    pub crossorigin: bool,
    /// If `true`, add `nopush` (preload without an actual server push).
    pub nopush: bool,
}

impl PushHint {
    /// Creates a minimal push hint with all optional fields unset.
    pub fn new(url: impl Into<String>, rel: PushRel) -> Self {
        Self {
            url: url.into(),
            rel,
            type_: None,
            as_: None,
            crossorigin: false,
            nopush: false,
        }
    }

    /// Creates a `Preload` push hint for a tile, setting `as_` and `type_`
    /// according to the tile format.
    pub fn preload_tile(url: impl Into<String>, format: &TileFormat) -> Self {
        let as_ = match format {
            TileFormat::Png | TileFormat::Jpeg | TileFormat::Webp => "image",
            TileFormat::Mvt | TileFormat::Json => "fetch",
        };
        let type_ = format.content_type().to_owned();
        Self {
            url: url.into(),
            rel: PushRel::Preload,
            type_: Some(type_),
            as_: Some(as_.to_owned()),
            crossorigin: false,
            nopush: false,
        }
    }

    /// Serialises this hint as a single `Link` header value entry.
    ///
    /// Example: `</tiles/roads/10/512/384.mvt>; rel=preload; as=fetch; type="application/vnd.mapbox-vector-tile"`
    #[must_use]
    pub fn to_link_header(&self) -> String {
        let mut s = format!("<{}>; rel={}", self.url, self.rel.as_str());
        if let Some(ref as_) = self.as_ {
            s.push_str(&format!("; as={as_}"));
        }
        if let Some(ref type_) = self.type_ {
            s.push_str(&format!("; type=\"{type_}\""));
        }
        if self.crossorigin {
            s.push_str("; crossorigin");
        }
        if self.nopush {
            s.push_str("; nopush");
        }
        s
    }
}

// ── PushPolicy ────────────────────────────────────────────────────────────────

/// Decides which neighbouring tiles to push alongside a tile request.
pub struct PushPolicy {
    /// Maximum number of push hints to generate per request.
    pub max_push_count: u8,
    /// Minimum zoom level to consider for push hints.
    pub min_zoom: u8,
    /// Maximum zoom level to consider for push hints.
    pub max_zoom: u8,
    /// Tile formats to include in push hints.
    pub formats: Vec<TileFormat>,
    /// Base URL prepended to each tile path.
    pub base_url: String,
}

impl PushPolicy {
    /// Creates a `PushPolicy` with sensible defaults:
    /// `max_push_count=8`, zoom 0–22, MVT format.
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            max_push_count: 8,
            min_zoom: 0,
            max_zoom: 22,
            formats: vec![TileFormat::Mvt],
            base_url: base_url.into(),
        }
    }

    /// Generates push hints for the neighbours of `requested`.
    ///
    /// Uses a `TilePrefetcher` with radius 1, filters by zoom range and format,
    /// and caps the result at `max_push_count`.
    pub fn generate_hints(&self, requested: &TileKey) -> Vec<PushHint> {
        let prefetcher = TilePrefetcher::new(1);
        let neighbours = prefetcher.neighbors(requested);
        let mut hints = Vec::new();
        for neighbour in neighbours {
            if hints.len() >= self.max_push_count as usize {
                break;
            }
            if neighbour.z < self.min_zoom || neighbour.z > self.max_zoom {
                continue;
            }
            if !self.formats.contains(&neighbour.format) {
                continue;
            }
            let url = format!(
                "{}/{}",
                self.base_url.trim_end_matches('/'),
                neighbour.path_string()
            );
            hints.push(PushHint::preload_tile(url, &neighbour.format));
        }
        hints
    }

    /// Joins multiple hints into a single `Link` header value (comma-separated).
    #[must_use]
    pub fn to_link_header_value(hints: &[PushHint]) -> String {
        hints
            .iter()
            .map(PushHint::to_link_header)
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Parses a tile URL of the form `{base_url}/{layer}/{z}/{x}/{y}.{ext}`
    /// back into a `TileKey`.  Returns `None` if the URL is malformed.
    #[must_use]
    pub fn parse_tile_url(url: &str, base_url: &str) -> Option<TileKey> {
        let base = base_url.trim_end_matches('/');
        let path = url.strip_prefix(base)?.trim_start_matches('/');
        // path should now be "{layer}/{z}/{x}/{y}.{ext}"
        let parts: Vec<&str> = path.splitn(4, '/').collect();
        if parts.len() != 4 {
            return None;
        }
        let layer = parts[0];
        let z: u8 = parts[1].parse().ok()?;
        let x: u32 = parts[2].parse().ok()?;
        // parts[3] is "{y}.{ext}"
        let (y_str, ext) = parts[3].rsplit_once('.')?;
        let y: u32 = y_str.parse().ok()?;
        let format = match ext {
            "mvt" => TileFormat::Mvt,
            "png" => TileFormat::Png,
            "jpg" => TileFormat::Jpeg,
            "webp" => TileFormat::Webp,
            "json" => TileFormat::Json,
            _ => return None,
        };
        Some(TileKey::new(z, x, y, layer, format))
    }
}

// ── ETagValidator ─────────────────────────────────────────────────────────────

/// Helpers for evaluating HTTP conditional request headers (`If-None-Match`,
/// `If-Match`).
pub struct ETagValidator;

impl ETagValidator {
    /// Checks whether the tile should be sent in full.
    ///
    /// Returns `true` (send 200) if `tile_etag` is **not** matched by
    /// `if_none_match`.  Returns `false` (send 304) if matched or if the
    /// header is the wildcard `*`.
    #[must_use]
    pub fn check_none_match(if_none_match: &str, tile_etag: &str) -> bool {
        let trimmed = if_none_match.trim();
        if trimmed == "*" {
            return false; // wildcard matches → 304
        }
        let list = Self::parse_etag_list(trimmed);
        let normalized = Self::normalize_etag(tile_etag);
        // If any entry in the list matches, return false (304)
        !list.iter().any(|e| Self::normalize_etag(e) == normalized)
    }

    /// Returns `true` if `tile_etag` is present in the `If-Match` list or the
    /// list is the wildcard `*`.
    #[must_use]
    pub fn check_match(if_match: &str, tile_etag: &str) -> bool {
        let trimmed = if_match.trim();
        if trimmed == "*" {
            return true;
        }
        let list = Self::parse_etag_list(trimmed);
        let normalized = Self::normalize_etag(tile_etag);
        list.iter().any(|e| Self::normalize_etag(e) == normalized)
    }

    /// Parses a comma-separated ETag list header value.
    ///
    /// Each entry is trimmed of whitespace but kept with its quotes.  Weak
    /// indicators (`W/`) are preserved.
    #[must_use]
    pub fn parse_etag_list(header: &str) -> Vec<String> {
        header
            .split(',')
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// Returns `true` if `etag` is a weak ETag (starts with `W/`).
    #[must_use]
    pub fn is_weak(etag: &str) -> bool {
        etag.starts_with("W/")
    }

    /// Strips the `W/` prefix for comparison purposes.
    fn normalize_etag(etag: &str) -> &str {
        etag.strip_prefix("W/").unwrap_or(etag)
    }
}

// ── TileResponseStatus ────────────────────────────────────────────────────────

/// HTTP status code category for a tile response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TileResponseStatus {
    /// 200 OK — tile found and returned.
    Ok,
    /// 304 Not Modified — ETag matched, no body needed.
    NotModified,
    /// 404 Not Found — tile not in cache.
    NotFound,
}

// ── TileResponse ──────────────────────────────────────────────────────────────

/// The result of a `TileServer::serve` call.
#[derive(Debug)]
pub struct TileResponse {
    /// HTTP status category.
    pub status: TileResponseStatus,
    /// Tile bytes (present for `Ok` responses only).
    pub data: Option<Vec<u8>>,
    /// Response headers as `(name, value)` pairs.
    pub headers: Vec<(String, String)>,
    /// HTTP/2 push hints for neighbouring tiles.
    pub push_hints: Vec<PushHint>,
}

// ── TileServer ────────────────────────────────────────────────────────────────

/// Unified tile serving combining an LRU cache, push policy, and prefetcher.
pub struct TileServer {
    /// The underlying LRU tile cache.
    pub cache: TileCache,
    /// Policy for generating HTTP/2 push hints.
    pub push_policy: PushPolicy,
    /// Prefetcher for enumerating neighbouring tiles.
    pub prefetcher: TilePrefetcher,
}

impl TileServer {
    /// Creates a `TileServer` with a 1 GiB / 1 024-entry cache.
    pub fn new(base_url: impl Into<String>) -> Self {
        let base_url = base_url.into();
        Self {
            cache: TileCache::new(1024, 256 * 1024 * 1024),
            push_policy: PushPolicy::new(base_url),
            prefetcher: TilePrefetcher::new(1),
        }
    }

    /// Serves a tile request, returning data, response headers, and push hints.
    ///
    /// * Cache miss → `NotFound`.
    /// * Hit + matching `If-None-Match` → `NotModified` (304).
    /// * Hit → `Ok` with data, `Cache-Control`, `ETag`, `Content-Type`, `Vary`,
    ///   and HTTP/2 push hints.
    pub fn serve(&mut self, key: &TileKey, if_none_match: Option<&str>, now: u64) -> TileResponse {
        // Perform the cache lookup and extract owned values to avoid borrow conflicts.
        let cached: Option<(Vec<u8>, String)> = self
            .cache
            .get(key, now)
            .map(|t: &CachedTile| (t.data.clone(), t.etag.clone()));

        match cached {
            None => TileResponse {
                status: TileResponseStatus::NotFound,
                data: None,
                headers: vec![],
                push_hints: vec![],
            },
            Some((data, etag)) => {
                // Check conditional request
                if let Some(inm) = if_none_match {
                    if !ETagValidator::check_none_match(inm, &etag) {
                        // ETag matched → 304
                        let headers = vec![
                            ("ETag".to_owned(), etag),
                            (
                                "Cache-Control".to_owned(),
                                "public, max-age=3600".to_owned(),
                            ),
                        ];
                        return TileResponse {
                            status: TileResponseStatus::NotModified,
                            data: None,
                            headers,
                            push_hints: vec![],
                        };
                    }
                }

                // Full 200 response
                let content_type = key.content_type().to_owned();
                let headers = vec![
                    (
                        "Cache-Control".to_owned(),
                        "public, max-age=3600".to_owned(),
                    ),
                    ("ETag".to_owned(), etag),
                    ("Content-Type".to_owned(), content_type),
                    ("Vary".to_owned(), "Accept-Encoding".to_owned()),
                ];

                let push_hints = self.push_policy.generate_hints(key);

                TileResponse {
                    status: TileResponseStatus::Ok,
                    data: Some(data),
                    headers,
                    push_hints,
                }
            }
        }
    }

    /// Stores a tile in the cache.
    pub fn cache_tile(&mut self, key: TileKey, data: Vec<u8>, now: u64) {
        let tile = CachedTile::new(key, data, now);
        self.cache.insert(tile);
    }

    /// Returns a snapshot of cache statistics.
    #[must_use]
    pub fn cache_stats(&self) -> CacheStats {
        self.cache.stats()
    }
}
