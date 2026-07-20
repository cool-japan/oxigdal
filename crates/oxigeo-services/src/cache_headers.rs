//! CDN-friendly HTTP caching headers for tile servers and geospatial APIs.

use thiserror::Error;

/// Errors produced by cache-header operations.
#[derive(Debug, Error)]
pub enum CacheError {
    /// The ETag string was not in a valid format.
    #[error("invalid ETag: {0}")]
    InvalidETag(String),
    /// The date string was not in a valid format.
    #[error("invalid date: {0}")]
    InvalidDate(String),
}

// ── CachePolicy ───────────────────────────────────────────────────────────────

/// Describes how a response should be cached by browsers and CDNs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CachePolicy {
    /// `no-store` — prohibit all caching.
    NoStore,
    /// `no-cache` — revalidate on every request via ETag/Last-Modified.
    NoCache,
    /// `public, max-age=X, immutable` — ideal for content-addressed URLs.
    Immutable {
        /// Seconds the response may be served from cache without revalidation.
        max_age_secs: u32,
    },
    /// `public, max-age=X [, stale-while-revalidate=Y] [, stale-if-error=Z]`
    Public {
        /// Seconds the response is considered fresh.
        max_age_secs: u32,
        /// Allow stale serving while revalidating in the background.
        stale_while_revalidate_secs: Option<u32>,
        /// Allow stale serving when the origin is returning errors.
        stale_if_error_secs: Option<u32>,
    },
    /// `private, max-age=X` — browser-only; never cached by shared caches.
    Private {
        /// Seconds the response may be served from the private cache.
        max_age_secs: u32,
    },
}

impl CachePolicy {
    /// Formats this policy as a `Cache-Control` header value.
    #[must_use]
    pub fn to_header_value(&self) -> String {
        match self {
            Self::NoStore => "no-store".to_owned(),
            Self::NoCache => "no-cache".to_owned(),
            Self::Immutable { max_age_secs } => {
                format!("public, max-age={max_age_secs}, immutable")
            }
            Self::Public {
                max_age_secs,
                stale_while_revalidate_secs,
                stale_if_error_secs,
            } => {
                let mut s = format!("public, max-age={max_age_secs}");
                if let Some(swr) = stale_while_revalidate_secs {
                    s.push_str(&format!(", stale-while-revalidate={swr}"));
                }
                if let Some(sie) = stale_if_error_secs {
                    s.push_str(&format!(", stale-if-error={sie}"));
                }
                s
            }
            Self::Private { max_age_secs } => {
                format!("private, max-age={max_age_secs}")
            }
        }
    }

    /// Default policy for map tiles: 1 h fresh, 60 s stale-while-revalidate, 24 h stale-if-error.
    #[must_use]
    pub fn tile_default() -> Self {
        Self::Public {
            max_age_secs: 3600,
            stale_while_revalidate_secs: Some(60),
            stale_if_error_secs: Some(86400),
        }
    }

    /// Default policy for dataset/layer metadata: 5 min fresh, 30 s swr, 1 h sie.
    #[must_use]
    pub fn metadata_default() -> Self {
        Self::Public {
            max_age_secs: 300,
            stale_while_revalidate_secs: Some(30),
            stale_if_error_secs: Some(3600),
        }
    }

    /// Policy for static assets referenced by content hash: 1 year, immutable.
    #[must_use]
    pub fn static_asset() -> Self {
        Self::Immutable {
            max_age_secs: 31_536_000,
        }
    }

    /// Policy for dynamic API responses: 60 s fresh, 10 s swr, 10 min sie.
    #[must_use]
    pub fn api_response() -> Self {
        Self::Public {
            max_age_secs: 60,
            stale_while_revalidate_secs: Some(10),
            stale_if_error_secs: Some(600),
        }
    }
}

// ── ETag ──────────────────────────────────────────────────────────────────────

/// An HTTP ETag header value, either strong or weak.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ETag {
    /// The opaque tag value (without surrounding quotes or `W/` prefix).
    pub value: String,
    /// `true` if this is a weak ETag (`W/"…"`).
    pub weak: bool,
}

const FNV_OFFSET: u64 = 14_695_981_039_346_656_037;
const FNV_PRIME: u64 = 1_099_511_628_211;

fn fnv1a_64(data: &[u8]) -> u64 {
    data.iter().fold(FNV_OFFSET, |acc, &b| {
        (acc ^ b as u64).wrapping_mul(FNV_PRIME)
    })
}

impl ETag {
    /// Creates a strong ETag whose value is the FNV-1a 64-bit hash of `data`.
    #[must_use]
    pub fn from_bytes(data: &[u8]) -> Self {
        let hash = fnv1a_64(data);
        Self {
            value: format!("{hash:016x}"),
            weak: false,
        }
    }

    /// Creates a strong ETag with an explicit string value.
    #[must_use]
    pub fn from_str_value(s: &str) -> Self {
        Self {
            value: s.to_owned(),
            weak: false,
        }
    }

    /// Creates a weak ETag with the given value.
    #[must_use]
    pub fn weak(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            weak: true,
        }
    }

    /// Formats the ETag as an HTTP header value: `"value"` or `W/"value"`.
    #[must_use]
    pub fn to_header_value(&self) -> String {
        if self.weak {
            format!("W/\"{}\"", self.value)
        } else {
            format!("\"{}\"", self.value)
        }
    }

    /// Parses an ETag from an HTTP header value.
    ///
    /// Accepts `"value"` (strong) and `W/"value"` (weak).
    ///
    /// # Errors
    /// Returns [`CacheError::InvalidETag`] if the string is not a valid ETag.
    pub fn parse(s: &str) -> Result<Self, CacheError> {
        let s = s.trim();
        if let Some(rest) = s.strip_prefix("W/\"") {
            let value = rest
                .strip_suffix('"')
                .ok_or_else(|| CacheError::InvalidETag(s.to_owned()))?;
            return Ok(Self::weak(value));
        }
        if let Some(inner) = s.strip_prefix('"') {
            let value = inner
                .strip_suffix('"')
                .ok_or_else(|| CacheError::InvalidETag(s.to_owned()))?;
            return Ok(Self::from_str_value(value));
        }
        Err(CacheError::InvalidETag(s.to_owned()))
    }
}

// ── VaryHeader ────────────────────────────────────────────────────────────────

/// Builder for the HTTP `Vary` response header.
#[derive(Debug, Clone, Default)]
pub struct VaryHeader {
    /// The list of request header field names that affect the response.
    pub fields: Vec<String>,
}

impl VaryHeader {
    /// Creates an empty `VaryHeader`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a field name (builder-style).
    #[must_use]
    #[allow(clippy::should_implement_trait)]
    pub fn add(mut self, field: impl Into<String>) -> Self {
        self.fields.push(field.into());
        self
    }

    /// Returns a `Vary` header containing only `Accept-Encoding`.
    #[must_use]
    pub fn accept_encoding() -> Self {
        Self::new().add("Accept-Encoding")
    }

    /// Returns a `Vary` header containing `Origin` and `Accept-Encoding`.
    #[must_use]
    pub fn origin_and_encoding() -> Self {
        Self::new().add("Origin").add("Accept-Encoding")
    }

    /// Formats the header value as a comma-separated list of field names.
    #[must_use]
    pub fn to_header_value(&self) -> String {
        self.fields.join(", ")
    }
}

// ── HTTP date formatting ──────────────────────────────────────────────────────

/// Formats a Unix timestamp (seconds since 1970-01-01T00:00:00Z) as an HTTP
/// date string, e.g. `"Thu, 01 Jan 1970 00:00:00 GMT"`.
///
/// Implemented without any date-time library using the civil-calendar algorithm
/// by Howard Hinnant.
#[must_use]
pub fn format_http_date(unix_secs: u64) -> String {
    const DAY_NAMES: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTH_NAMES: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    // Day of week: epoch (1970-01-01) was a Thursday (index 0 in our array).
    let day_of_week = DAY_NAMES[(unix_secs / 86400 % 7) as usize];

    let secs_of_day = unix_secs % 86400;
    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;

    // Civil calendar from Unix days (Hinnant's algorithm, integer arithmetic only).
    // Shift epoch to 0000-03-01 to simplify leap-year handling.
    let z = unix_secs / 86400 + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097; // day of era [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // year of era [0, 399]
    let y = yoe + era * 400; // year
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // day of year [0, 365]
    let mp = (5 * doy + 2) / 153; // month of year [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // day [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // month [1, 12]
    let y = if m <= 2 { y + 1 } else { y }; // adjust year

    format!(
        "{}, {:02} {} {:04} {:02}:{:02}:{:02} GMT",
        day_of_week,
        d,
        MONTH_NAMES[(m - 1) as usize],
        y,
        hour,
        minute,
        second
    )
}

// ── CacheHeaders ──────────────────────────────────────────────────────────────

/// A complete set of cache-related HTTP response headers.
#[derive(Debug, Clone, Default)]
pub struct CacheHeaders {
    /// Value of the `Cache-Control` header.
    pub cache_control: String,
    /// Value of the `ETag` header, if any.
    pub etag: Option<String>,
    /// Value of the `Last-Modified` header (HTTP date), if any.
    pub last_modified: Option<String>,
    /// Value of the `Vary` header, if any.
    pub vary: Option<String>,
    /// Value of the `CDN-Cache-Control` header, if any.
    pub cdn_cache_control: Option<String>,
    /// Value of the `Surrogate-Control` header (Varnish/Fastly), if any.
    pub surrogate_control: Option<String>,
}

impl CacheHeaders {
    /// Creates a new `CacheHeaders` from a [`CachePolicy`].
    #[must_use]
    pub fn new(policy: CachePolicy) -> Self {
        Self {
            cache_control: policy.to_header_value(),
            ..Self::default()
        }
    }

    /// Attaches an ETag header (builder-style).
    #[must_use]
    pub fn with_etag(mut self, etag: ETag) -> Self {
        self.etag = Some(etag.to_header_value());
        self
    }

    /// Attaches a `Last-Modified` header derived from a Unix timestamp
    /// (builder-style).
    #[must_use]
    pub fn with_last_modified(mut self, unix_secs: u64) -> Self {
        self.last_modified = Some(format_http_date(unix_secs));
        self
    }

    /// Attaches a `Vary` header (builder-style).
    #[must_use]
    pub fn with_vary(mut self, vary: VaryHeader) -> Self {
        self.vary = Some(vary.to_header_value());
        self
    }

    /// Adds CDN-specific override headers (`CDN-Cache-Control` and
    /// `Surrogate-Control`) with a custom max-age (builder-style).
    #[must_use]
    pub fn with_cdn_override(mut self, cdn_max_age_secs: u32) -> Self {
        self.cdn_cache_control = Some(format!("public, max-age={cdn_max_age_secs}"));
        self.surrogate_control = Some(format!("max-age={cdn_max_age_secs}"));
        self
    }

    /// Returns `true` if the request's `If-None-Match` value matches this
    /// response's ETag, indicating the client's copy is still valid (→ 304).
    #[must_use]
    pub fn is_not_modified(&self, if_none_match: Option<&str>) -> bool {
        match (&self.etag, if_none_match) {
            (Some(our_etag), Some(client_val)) => our_etag == client_val,
            _ => false,
        }
    }

    /// Returns all set headers as `(name, value)` pairs.
    ///
    /// `Cache-Control` is always included.  All other headers are omitted when
    /// not set.
    #[must_use]
    pub fn to_header_pairs(&self) -> Vec<(String, String)> {
        let mut pairs = vec![("Cache-Control".to_owned(), self.cache_control.clone())];
        if let Some(v) = &self.etag {
            pairs.push(("ETag".to_owned(), v.clone()));
        }
        if let Some(v) = &self.last_modified {
            pairs.push(("Last-Modified".to_owned(), v.clone()));
        }
        if let Some(v) = &self.vary {
            pairs.push(("Vary".to_owned(), v.clone()));
        }
        if let Some(v) = &self.cdn_cache_control {
            pairs.push(("CDN-Cache-Control".to_owned(), v.clone()));
        }
        if let Some(v) = &self.surrogate_control {
            pairs.push(("Surrogate-Control".to_owned(), v.clone()));
        }
        pairs
    }
}

// ── TileCacheStrategy ─────────────────────────────────────────────────────────

/// Zoom-level-aware caching strategy for map tiles.
#[derive(Debug, Clone)]
pub struct TileCacheStrategy {
    /// Ordered list of `(min_zoom, max_zoom, policy)` bands.
    pub zoom_policies: Vec<(u8, u8, CachePolicy)>,
    /// Fallback policy when no band matches the requested zoom level.
    pub default_policy: CachePolicy,
}

impl TileCacheStrategy {
    /// Creates an empty strategy with `NoCache` as the fallback.
    #[must_use]
    pub fn new() -> Self {
        Self {
            zoom_policies: Vec::new(),
            default_policy: CachePolicy::NoCache,
        }
    }

    /// Returns the standard multi-tier tile caching strategy:
    ///
    /// | Zoom  | Policy                                          |
    /// |-------|-------------------------------------------------|
    /// | 0–7   | public, 24 h, swr 1 h, sie 7 d                 |
    /// | 8–12  | public, 1 h, swr 60 s, sie 24 h                |
    /// | 13–16 | public, 5 min, swr 30 s, sie 1 h               |
    /// | 17–22 | no-cache                                        |
    #[must_use]
    pub fn standard_tile_strategy() -> Self {
        Self {
            zoom_policies: vec![
                (
                    0,
                    7,
                    CachePolicy::Public {
                        max_age_secs: 86400,
                        stale_while_revalidate_secs: Some(3600),
                        stale_if_error_secs: Some(604_800),
                    },
                ),
                (
                    8,
                    12,
                    CachePolicy::Public {
                        max_age_secs: 3600,
                        stale_while_revalidate_secs: Some(60),
                        stale_if_error_secs: Some(86400),
                    },
                ),
                (
                    13,
                    16,
                    CachePolicy::Public {
                        max_age_secs: 300,
                        stale_while_revalidate_secs: Some(30),
                        stale_if_error_secs: Some(3600),
                    },
                ),
                (17, 22, CachePolicy::NoCache),
            ],
            default_policy: CachePolicy::NoCache,
        }
    }

    /// Returns the [`CachePolicy`] appropriate for the given zoom level.
    #[must_use]
    pub fn policy_for_zoom(&self, zoom: u8) -> &CachePolicy {
        for (min, max, policy) in &self.zoom_policies {
            if zoom >= *min && zoom <= *max {
                return policy;
            }
        }
        &self.default_policy
    }

    /// Builds [`CacheHeaders`] for a tile at `zoom` with content `tile_data`.
    ///
    /// The ETag is derived from the tile bytes; `Vary: Accept-Encoding` is
    /// always set.
    #[must_use]
    pub fn headers_for_tile(&self, zoom: u8, tile_data: &[u8]) -> CacheHeaders {
        let policy = self.policy_for_zoom(zoom).clone();
        CacheHeaders::new(policy)
            .with_etag(ETag::from_bytes(tile_data))
            .with_vary(VaryHeader::accept_encoding())
    }
}

impl Default for TileCacheStrategy {
    fn default() -> Self {
        Self::new()
    }
}
