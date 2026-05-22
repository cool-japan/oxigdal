//! Tile streaming protocol implementations.

use crate::error::{Result, StreamingError};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Tile coordinate in a tile matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TileCoordinate {
    /// Zoom level
    pub z: u8,

    /// Column (x) index
    pub x: u32,

    /// Row (y) index
    pub y: u32,
}

impl TileCoordinate {
    /// Create a new tile coordinate.
    pub fn new(z: u8, x: u32, y: u32) -> Self {
        Self { z, x, y }
    }

    /// Convert to XYZ format string.
    pub fn to_xyz_string(&self) -> String {
        format!("{}/{}/{}", self.z, self.x, self.y)
    }

    /// Convert to TMS format (flipped Y).
    pub fn to_tms(&self) -> Self {
        let max_y = (1u32 << self.z) - 1;
        Self {
            z: self.z,
            x: self.x,
            y: max_y - self.y,
        }
    }

    /// Get parent tile coordinate.
    pub fn parent(&self) -> Option<Self> {
        if self.z == 0 {
            return None;
        }
        Some(Self {
            z: self.z - 1,
            x: self.x / 2,
            y: self.y / 2,
        })
    }

    /// Get child tile coordinates.
    pub fn children(&self) -> Vec<Self> {
        if self.z >= 31 {
            return vec![];
        }
        let z = self.z + 1;
        let x = self.x * 2;
        let y = self.y * 2;
        vec![
            Self::new(z, x, y),
            Self::new(z, x + 1, y),
            Self::new(z, x, y + 1),
            Self::new(z, x + 1, y + 1),
        ]
    }

    /// Check if this tile is a valid coordinate for the given zoom level.
    pub fn is_valid(&self) -> bool {
        if self.z > 31 {
            return false;
        }
        let max_coord = 1u32 << self.z;
        self.x < max_coord && self.y < max_coord
    }
}

impl fmt::Display for TileCoordinate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}/{}", self.z, self.x, self.y)
    }
}

/// Tile request parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileRequest {
    /// Tile coordinate
    pub coord: TileCoordinate,

    /// Tile format (png, jpg, webp, pbf, etc.)
    pub format: TileFormat,

    /// Optional layer name
    pub layer: Option<String>,

    /// Optional style name
    pub style: Option<String>,

    /// Additional parameters
    pub params: std::collections::HashMap<String, String>,
}

impl TileRequest {
    /// Create a new tile request.
    pub fn new(coord: TileCoordinate, format: TileFormat) -> Self {
        Self {
            coord,
            format,
            layer: None,
            style: None,
            params: std::collections::HashMap::new(),
        }
    }

    /// Set the layer name.
    pub fn with_layer(mut self, layer: String) -> Self {
        self.layer = Some(layer);
        self
    }

    /// Set the style name.
    pub fn with_style(mut self, style: String) -> Self {
        self.style = Some(style);
        self
    }

    /// Add a parameter.
    pub fn with_param(mut self, key: String, value: String) -> Self {
        self.params.insert(key, value);
        self
    }
}

/// Tile response.
#[derive(Debug, Clone)]
pub struct TileResponse {
    /// Tile coordinate
    pub coord: TileCoordinate,

    /// Tile data
    pub data: Bytes,

    /// Content type
    pub content_type: String,

    /// Cache control headers
    pub cache_control: Option<String>,

    /// ETag for cache validation
    pub etag: Option<String>,

    /// Last modified timestamp
    pub last_modified: Option<chrono::DateTime<chrono::Utc>>,
}

impl TileResponse {
    /// Create a new tile response.
    pub fn new(coord: TileCoordinate, data: Bytes, content_type: String) -> Self {
        Self {
            coord,
            data,
            content_type,
            cache_control: None,
            etag: None,
            last_modified: None,
        }
    }

    /// Set cache control.
    pub fn with_cache_control(mut self, cache_control: String) -> Self {
        self.cache_control = Some(cache_control);
        self
    }

    /// Set ETag.
    pub fn with_etag(mut self, etag: String) -> Self {
        self.etag = Some(etag);
        self
    }

    /// Get the size in bytes.
    pub fn size_bytes(&self) -> usize {
        self.data.len()
    }
}

/// Tile format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TileFormat {
    /// PNG format
    Png,

    /// JPEG format
    Jpeg,

    /// WebP format
    WebP,

    /// Protocol Buffer (vector tiles)
    Pbf,

    /// GeoJSON
    GeoJson,

    /// JSON
    Json,
}

impl TileFormat {
    /// Get the MIME type for this format.
    pub fn mime_type(&self) -> &'static str {
        match self {
            TileFormat::Png => "image/png",
            TileFormat::Jpeg => "image/jpeg",
            TileFormat::WebP => "image/webp",
            TileFormat::Pbf => "application/x-protobuf",
            TileFormat::GeoJson => "application/geo+json",
            TileFormat::Json => "application/json",
        }
    }

    /// Get the file extension for this format.
    pub fn extension(&self) -> &'static str {
        match self {
            TileFormat::Png => "png",
            TileFormat::Jpeg => "jpg",
            TileFormat::WebP => "webp",
            TileFormat::Pbf => "pbf",
            TileFormat::GeoJson => "geojson",
            TileFormat::Json => "json",
        }
    }
}

impl fmt::Display for TileFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.extension())
    }
}

/// Tile protocol interface.
#[async_trait::async_trait]
pub trait TileProtocol: Send + Sync {
    /// Get a tile.
    async fn get_tile(&self, request: &TileRequest) -> Result<TileResponse>;

    /// Check if a tile exists.
    async fn has_tile(&self, coord: &TileCoordinate) -> Result<bool>;

    /// Get tile metadata.
    async fn get_tile_metadata(&self, coord: &TileCoordinate) -> Result<TileMetadata>;

    /// Get the supported zoom levels.
    fn zoom_levels(&self) -> (u8, u8);

    /// Get the tile size in pixels.
    fn tile_size(&self) -> (u32, u32);
}

/// Tile metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileMetadata {
    /// Tile coordinate
    pub coord: TileCoordinate,

    /// Size in bytes
    pub size_bytes: usize,

    /// Format
    pub format: TileFormat,

    /// Creation timestamp
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,

    /// Last modified timestamp
    pub modified_at: Option<chrono::DateTime<chrono::Utc>>,

    /// Bounding box
    pub bbox: Option<oxigdal_core::types::BoundingBox>,

    /// Additional metadata
    pub metadata: std::collections::HashMap<String, String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// HTTP retry helper (feature = "tile-http")
// ─────────────────────────────────────────────────────────────────────────────

/// Maximum number of retry attempts for transient 5xx errors.
#[cfg(feature = "tile-http")]
const HTTP_MAX_ATTEMPTS: u32 = 3;

/// Initial backoff delay in milliseconds before the first retry.
#[cfg(feature = "tile-http")]
const HTTP_INITIAL_DELAY_MS: u64 = 100;

/// Parse an RFC-2616 / RFC-7231 `Last-Modified` header value into a UTC datetime.
#[cfg(feature = "tile-http")]
fn parse_http_date(value: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    // Try RFC 2822 (most common in HTTP/1.1)
    if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(value) {
        return Some(dt.with_timezone(&chrono::Utc));
    }
    // Try the older ANSI-C asctime format: "Tue, 15 Nov 1994 08:12:31 GMT"
    // also covered by rfc2822 above. Try ISO 8601 as a fallback.
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(value) {
        return Some(dt.with_timezone(&chrono::Utc));
    }
    None
}

// ─────────────────────────────────────────────────────────────────────────────
// XyzProtocol
// ─────────────────────────────────────────────────────────────────────────────

/// XYZ tile protocol implementation.
pub struct XyzProtocol {
    /// Base URL template (`{z}`, `{x}`, `{y}` are replaced at fetch time)
    url_template: String,

    /// Minimum zoom level
    min_zoom: u8,

    /// Maximum zoom level
    max_zoom: u8,

    /// Tile size
    tile_size: (u32, u32),

    /// Reusable HTTP client (only compiled when tile-http is active)
    #[cfg(feature = "tile-http")]
    http_client: reqwest::Client,
}

impl XyzProtocol {
    /// Create a new XYZ protocol.
    #[cfg(not(feature = "tile-http"))]
    pub fn new(url_template: String, min_zoom: u8, max_zoom: u8) -> Self {
        Self {
            url_template,
            min_zoom,
            max_zoom,
            tile_size: (256, 256),
        }
    }

    /// Create a new XYZ protocol with an HTTP client.
    #[cfg(feature = "tile-http")]
    pub fn new(url_template: String, min_zoom: u8, max_zoom: u8) -> Self {
        Self {
            url_template,
            min_zoom,
            max_zoom,
            tile_size: (256, 256),
            http_client: reqwest::Client::new(),
        }
    }

    /// Set the tile size.
    pub fn with_tile_size(mut self, width: u32, height: u32) -> Self {
        self.tile_size = (width, height);
        self
    }

    /// Build URL for a tile.
    pub fn build_url(&self, coord: &TileCoordinate) -> String {
        self.url_template
            .replace("{z}", &coord.z.to_string())
            .replace("{x}", &coord.x.to_string())
            .replace("{y}", &coord.y.to_string())
    }

    /// Build URL for a tile with an explicit file extension.
    #[cfg(feature = "tile-http")]
    fn build_url_with_ext(&self, coord: &TileCoordinate, ext: &str) -> String {
        // If the template already contains `{ext}` expand it; otherwise fall
        // back to the numeric placeholders only.
        self.url_template
            .replace("{ext}", ext)
            .replace("{z}", &coord.z.to_string())
            .replace("{x}", &coord.x.to_string())
            .replace("{y}", &coord.y.to_string())
    }

    /// Perform a single HTTP GET and return `(status, headers, body)`.
    #[cfg(feature = "tile-http")]
    async fn http_get_raw(
        &self,
        url: &str,
    ) -> std::result::Result<reqwest::Response, reqwest::Error> {
        self.http_client.get(url).send().await
    }

    /// Fetch the tile bytes with automatic retry on transient 5xx errors.
    ///
    /// Returns `(data_bytes, etag, last_modified, cache_control, content_type)`.
    #[cfg(feature = "tile-http")]
    async fn fetch_with_retry(&self, url: &str, expected_mime: &str) -> Result<TileResponse> {
        use std::time::Duration;

        let coord_placeholder = TileCoordinate::new(0, 0, 0); // only used for the error path
        let mut last_err: Option<StreamingError> = None;

        for attempt in 0..HTTP_MAX_ATTEMPTS {
            if attempt > 0 {
                // Simple exponential back-off: 100 ms, 200 ms, …
                let delay_ms = HTTP_INITIAL_DELAY_MS * (1u64 << (attempt - 1));
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }

            let response = match self.http_get_raw(url).await {
                Ok(r) => r,
                Err(e) => {
                    last_err = Some(StreamingError::Reqwest(e));
                    continue;
                }
            };

            let status = response.status();

            // 404 → tile does not exist; do not retry
            if status == reqwest::StatusCode::NOT_FOUND {
                return Err(StreamingError::TileNotFound);
            }

            // 5xx → transient; retry
            if status.is_server_error() {
                last_err = Some(StreamingError::HttpError {
                    status: status.as_u16(),
                    url: url.to_owned(),
                });
                continue;
            }

            // Any other non-2xx → permanent client-side error; fail immediately
            if !status.is_success() {
                return Err(StreamingError::HttpError {
                    status: status.as_u16(),
                    url: url.to_owned(),
                });
            }

            // --- 2xx: extract metadata from headers before consuming body ---

            // ETag
            let etag = response
                .headers()
                .get(reqwest::header::ETAG)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_owned());

            // Last-Modified
            let last_modified = response
                .headers()
                .get(reqwest::header::LAST_MODIFIED)
                .and_then(|v| v.to_str().ok())
                .and_then(parse_http_date);

            // Cache-Control
            let cache_control = response
                .headers()
                .get(reqwest::header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_owned());

            // Content-Type: prefer what the server says; fall back to expected_mime
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_owned())
                .unwrap_or_else(|| expected_mime.to_owned());

            // Consume body
            let data = response.bytes().await.map_err(StreamingError::Reqwest)?;

            let mut tile_resp = TileResponse::new(coord_placeholder, data, content_type);

            if let Some(e) = etag {
                tile_resp = tile_resp.with_etag(e);
            }
            if let Some(cc) = cache_control {
                tile_resp = tile_resp.with_cache_control(cc);
            }
            tile_resp.last_modified = last_modified;

            return Ok(tile_resp);
        }

        // All attempts exhausted
        Err(last_err.unwrap_or_else(|| StreamingError::HttpError {
            status: 500,
            url: url.to_owned(),
        }))
    }
}

#[async_trait::async_trait]
impl TileProtocol for XyzProtocol {
    async fn get_tile(&self, request: &TileRequest) -> Result<TileResponse> {
        if request.coord.z < self.min_zoom || request.coord.z > self.max_zoom {
            return Err(StreamingError::InvalidOperation(format!(
                "Zoom level {} out of range [{}, {}]",
                request.coord.z, self.min_zoom, self.max_zoom
            )));
        }

        #[cfg(feature = "tile-http")]
        {
            let ext = request.format.extension();
            let url = self.build_url_with_ext(&request.coord, ext);
            let expected_mime = request.format.mime_type();

            let mut tile_resp = self.fetch_with_retry(&url, expected_mime).await?;
            // Stamp the actual requested coordinate (the placeholder in fetch_with_retry
            // is only used before we know which coord is being requested).
            tile_resp.coord = request.coord;
            return Ok(tile_resp);
        }

        #[cfg(not(feature = "tile-http"))]
        {
            let _ = request; // suppress unused warning
            Ok(TileResponse::new(
                request.coord,
                Bytes::new(),
                request.format.mime_type().to_string(),
            ))
        }
    }

    async fn has_tile(&self, coord: &TileCoordinate) -> Result<bool> {
        Ok(coord.z >= self.min_zoom && coord.z <= self.max_zoom && coord.is_valid())
    }

    async fn get_tile_metadata(&self, coord: &TileCoordinate) -> Result<TileMetadata> {
        #[cfg(feature = "tile-http")]
        {
            // Perform a HEAD-like request (GET is the safest fallback since many
            // tile CDNs do not implement HEAD).  We use a PNG request by default.
            let url = self.build_url(coord);
            let response = self
                .http_get_raw(&url)
                .await
                .map_err(StreamingError::Reqwest)?;

            let status = response.status();
            if status == reqwest::StatusCode::NOT_FOUND {
                return Err(StreamingError::TileNotFound);
            }
            if !status.is_success() {
                return Err(StreamingError::HttpError {
                    status: status.as_u16(),
                    url: url.clone(),
                });
            }

            // Extract Last-Modified and ETag from headers.
            let modified_at = response
                .headers()
                .get(reqwest::header::LAST_MODIFIED)
                .and_then(|v| v.to_str().ok())
                .and_then(parse_http_date);

            let size_bytes = response
                .headers()
                .get(reqwest::header::CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0);

            let mut metadata_map = std::collections::HashMap::new();
            if let Some(etag) = response
                .headers()
                .get(reqwest::header::ETAG)
                .and_then(|v| v.to_str().ok())
            {
                metadata_map.insert("etag".to_owned(), etag.to_owned());
            }

            return Ok(TileMetadata {
                coord: *coord,
                size_bytes,
                format: TileFormat::Png,
                created_at: None,
                modified_at,
                bbox: None,
                metadata: metadata_map,
            });
        }

        #[cfg(not(feature = "tile-http"))]
        {
            let _ = coord;
            Ok(TileMetadata {
                coord: *coord,
                size_bytes: 0,
                format: TileFormat::Png,
                created_at: None,
                modified_at: None,
                bbox: None,
                metadata: std::collections::HashMap::new(),
            })
        }
    }

    fn zoom_levels(&self) -> (u8, u8) {
        (self.min_zoom, self.max_zoom)
    }

    fn tile_size(&self) -> (u32, u32) {
        self.tile_size
    }
}

/// TMS (Tile Map Service) protocol implementation.
pub struct TmsProtocol {
    inner: XyzProtocol,
}

impl TmsProtocol {
    /// Create a new TMS protocol.
    pub fn new(url_template: String, min_zoom: u8, max_zoom: u8) -> Self {
        Self {
            inner: XyzProtocol::new(url_template, min_zoom, max_zoom),
        }
    }
}

#[async_trait::async_trait]
impl TileProtocol for TmsProtocol {
    async fn get_tile(&self, request: &TileRequest) -> Result<TileResponse> {
        // Convert to TMS coordinates (flip Y)
        let tms_coord = request.coord.to_tms();
        let tms_request = TileRequest {
            coord: tms_coord,
            ..request.clone()
        };
        self.inner.get_tile(&tms_request).await
    }

    async fn has_tile(&self, coord: &TileCoordinate) -> Result<bool> {
        let tms_coord = coord.to_tms();
        self.inner.has_tile(&tms_coord).await
    }

    async fn get_tile_metadata(&self, coord: &TileCoordinate) -> Result<TileMetadata> {
        let tms_coord = coord.to_tms();
        self.inner.get_tile_metadata(&tms_coord).await
    }

    fn zoom_levels(&self) -> (u8, u8) {
        self.inner.zoom_levels()
    }

    fn tile_size(&self) -> (u32, u32) {
        self.inner.tile_size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tile_coordinate() {
        let coord = TileCoordinate::new(10, 512, 384);
        assert_eq!(coord.z, 10);
        assert_eq!(coord.x, 512);
        assert_eq!(coord.y, 384);
        assert!(coord.is_valid());
    }

    #[test]
    fn test_tile_parent() {
        let coord = TileCoordinate::new(10, 512, 384);
        let parent = coord.parent();
        assert!(parent.is_some());
        let parent = parent.expect("parent tile should exist for non-zero zoom level");
        assert_eq!(parent.z, 9);
        assert_eq!(parent.x, 256);
        assert_eq!(parent.y, 192);
    }

    #[test]
    fn test_tile_children() {
        let coord = TileCoordinate::new(10, 512, 384);
        let children = coord.children();
        assert_eq!(children.len(), 4);
        assert_eq!(children[0], TileCoordinate::new(11, 1024, 768));
        assert_eq!(children[1], TileCoordinate::new(11, 1025, 768));
        assert_eq!(children[2], TileCoordinate::new(11, 1024, 769));
        assert_eq!(children[3], TileCoordinate::new(11, 1025, 769));
    }

    #[test]
    fn test_tms_conversion() {
        let coord = TileCoordinate::new(10, 512, 384);
        let tms = coord.to_tms();
        assert_eq!(tms.z, 10);
        assert_eq!(tms.x, 512);
        assert_eq!(tms.y, 639); // 1024 - 384 - 1
    }

    #[test]
    fn test_tile_format() {
        assert_eq!(TileFormat::Png.mime_type(), "image/png");
        assert_eq!(TileFormat::Jpeg.extension(), "jpg");
        assert_eq!(TileFormat::WebP.to_string(), "webp");
    }

    #[test]
    fn test_build_url() {
        let proto = XyzProtocol::new(
            "https://tiles.example.com/{z}/{x}/{y}.png".to_string(),
            0,
            18,
        );
        let coord = TileCoordinate::new(10, 512, 384);
        let url = proto.build_url(&coord);
        assert_eq!(url, "https://tiles.example.com/10/512/384.png");
    }

    #[cfg(feature = "tile-http")]
    #[test]
    fn test_build_url_with_ext() {
        let proto = XyzProtocol::new(
            "https://tiles.example.com/{z}/{x}/{y}.{ext}".to_string(),
            0,
            18,
        );
        let coord = TileCoordinate::new(7, 63, 42);
        let url = proto.build_url_with_ext(&coord, "pbf");
        assert_eq!(url, "https://tiles.example.com/7/63/42.pbf");
    }
}
