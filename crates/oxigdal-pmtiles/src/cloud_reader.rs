//! Cloud-storage range-request reader for PMTiles archives.
//!
//! Supports S3, Google Cloud Storage, and Azure Blob Storage via URI
//! parsing and credential plumbing (anonymous, presigned-URL, bearer-token).
//! S3 SigV4 signing is explicitly out-of-scope for this module; use presigned
//! URLs or bearer tokens instead.
//!
//! This module is gated behind the `cloud-storage` Cargo feature.

#![cfg(feature = "cloud-storage")]

use url::Url;

use crate::directory::{DirectoryEntry, decode_directory};
use crate::error::PmTilesError;
use crate::header::{PMTILES_HEADER_SIZE, PMTILES_MAGIC, PmTilesHeader};
use crate::hilbert::zxy_to_tile_id;
use crate::pmtiles::{binary_search_entries, decompress_data};

// ─────────────────────────────────────────────────────────────────────────────
// CloudProvider
// ─────────────────────────────────────────────────────────────────────────────

/// Which cloud provider hosts the PMTiles archive.
///
/// The provider determines how `s3://`, `gs://`, and `az://` URIs are
/// translated into canonical HTTPS endpoint URLs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudProvider {
    /// Amazon S3 — `s3://bucket/key`.
    ///
    /// The `region` is used to build the virtual-hosted-style URL:
    /// `https://bucket.s3.{region}.amazonaws.com/key`.
    S3 {
        /// AWS region (e.g. `"us-east-1"`).
        region: String,
    },
    /// Google Cloud Storage — `gs://bucket/key`.
    ///
    /// Maps to `https://storage.googleapis.com/bucket/key`.
    Gcs,
    /// Azure Blob Storage — `az://account/container/blob`.
    ///
    /// Maps to `https://account.blob.core.windows.net/container/blob`.
    AzureBlob {
        /// Storage account name (extracted from the URI host segment).
        account: String,
    },
}

// ─────────────────────────────────────────────────────────────────────────────
// CloudCredentials
// ─────────────────────────────────────────────────────────────────────────────

/// Credential bundle threaded into every range-GET request.
///
/// Only `bearer_token` is used for request signing in this implementation.
/// `access_key` / `secret_key` / `session_token` are stored for future
/// SigV4 or other provider-specific signing support.
#[derive(Debug, Clone, Default)]
pub struct CloudCredentials {
    /// AWS / GCS / Azure access key ID (not yet used for signing).
    pub access_key: Option<String>,
    /// AWS / GCS / Azure secret access key (not yet used for signing).
    pub secret_key: Option<String>,
    /// AWS STS session token (not yet used for signing).
    pub session_token: Option<String>,
    /// OAuth2 / API gateway bearer token.
    ///
    /// When set, every request carries `Authorization: Bearer <token>`.
    pub bearer_token: Option<String>,
}

impl CloudCredentials {
    /// Return credentials that carry no authentication header (public buckets
    /// or presigned-URL access).
    pub fn anonymous() -> Self {
        Self::default()
    }

    /// Return credentials that set `Authorization: Bearer {token}` on every
    /// request.
    pub fn bearer(token: impl Into<String>) -> Self {
        Self {
            bearer_token: Some(token.into()),
            ..Default::default()
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// CloudObjectUri
// ─────────────────────────────────────────────────────────────────────────────

/// A parsed cloud object URI (scheme + bucket + key).
///
/// Supported URI forms:
/// - `s3://bucket/path/to/tiles.pmtiles`
/// - `gs://bucket/path/to/tiles.pmtiles`
/// - `az://account/container/blob/path.pmtiles`
#[derive(Debug, Clone)]
pub struct CloudObjectUri {
    /// Which cloud provider the URI refers to.
    pub provider: CloudProvider,
    /// Bucket (S3/GCS) or container name (Azure, part of the path after the account).
    pub bucket: String,
    /// Object key within the bucket.
    pub key: String,
}

impl CloudObjectUri {
    /// Parse a cloud object URI string into a [`CloudObjectUri`].
    ///
    /// # Supported schemes
    /// - `s3://bucket/key` — Amazon S3 (region defaults to `"us-east-1"`)
    /// - `gs://bucket/key` — Google Cloud Storage
    /// - `az://account/container/blob` — Azure Blob Storage
    ///
    /// # Errors
    /// Returns [`PmTilesError::HttpError`] for unknown schemes, missing
    /// buckets, or missing object keys.
    pub fn parse(s: &str) -> Result<Self, PmTilesError> {
        // Split on "://" to extract the scheme and the rest.
        let (scheme, rest) = s
            .split_once("://")
            .ok_or_else(|| PmTilesError::HttpError(format!("Missing '://' in URI: {s}")))?;

        match scheme {
            "s3" => Self::parse_s3(rest),
            "gs" => Self::parse_gcs(rest),
            "az" => Self::parse_azure(rest),
            other => Err(PmTilesError::HttpError(format!(
                "Unsupported URI scheme '{other}'. Expected s3://, gs://, or az://"
            ))),
        }
    }

    /// Parse `bucket/key/path` portion of an S3 URI.
    fn parse_s3(rest: &str) -> Result<Self, PmTilesError> {
        let (bucket, key) = split_bucket_and_key(rest)?;
        Ok(Self {
            provider: CloudProvider::S3 {
                region: "us-east-1".to_string(),
            },
            bucket,
            key,
        })
    }

    /// Parse `bucket/key/path` portion of a GCS URI.
    fn parse_gcs(rest: &str) -> Result<Self, PmTilesError> {
        let (bucket, key) = split_bucket_and_key(rest)?;
        Ok(Self {
            provider: CloudProvider::Gcs,
            bucket,
            key,
        })
    }

    /// Parse `account/container/blob` portion of an Azure Blob URI.
    ///
    /// The format is `az://account/container/blob-path`.  We treat `account`
    /// as the "host", `container` as the bucket, and the remainder as the key.
    fn parse_azure(rest: &str) -> Result<Self, PmTilesError> {
        // rest = "account/container/blob/path"
        let rest = rest.trim_start_matches('/');
        if rest.is_empty() {
            return Err(PmTilesError::HttpError(
                "Azure URI missing account segment: az://<account>/<container>/<blob>".into(),
            ));
        }

        // First segment is the storage account name.
        let (account, remainder) = rest.split_once('/').ok_or_else(|| {
            PmTilesError::HttpError(
                "Azure URI missing container segment: az://<account>/<container>/<blob>".into(),
            )
        })?;

        // The rest is container + blob path — treat container as bucket.
        let (bucket, key) = split_bucket_and_key(remainder)?;

        Ok(Self {
            provider: CloudProvider::AzureBlob {
                account: account.to_string(),
            },
            bucket,
            key,
        })
    }

    /// Convert this URI to a canonical HTTPS URL suitable for HTTP range-GET
    /// requests.
    ///
    /// | Provider | URL pattern |
    /// |----------|-------------|
    /// | S3 | `https://{bucket}.s3.{region}.amazonaws.com/{key}` |
    /// | GCS | `https://storage.googleapis.com/{bucket}/{key}` |
    /// | Azure | `https://{account}.blob.core.windows.net/{container}/{key}` |
    ///
    /// # Errors
    /// Returns [`PmTilesError::HttpError`] when the resulting URL string is
    /// not parseable by the `url` crate (malformed bucket/key characters).
    pub fn to_https_url(&self) -> Result<Url, PmTilesError> {
        let url_string = match &self.provider {
            CloudProvider::S3 { region } => {
                format!(
                    "https://{}.s3.{}.amazonaws.com/{}",
                    self.bucket, region, self.key
                )
            }
            CloudProvider::Gcs => {
                format!(
                    "https://storage.googleapis.com/{}/{}",
                    self.bucket, self.key
                )
            }
            CloudProvider::AzureBlob { account } => {
                format!(
                    "https://{}.blob.core.windows.net/{}/{}",
                    account, self.bucket, self.key
                )
            }
        };

        Url::parse(&url_string).map_err(|e| {
            PmTilesError::HttpError(format!("Failed to build HTTPS URL from URI: {e}"))
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Split a `bucket/key/path` string into `(bucket, key)`.
///
/// Strips leading slashes, then splits at the first `/`.  Returns an error
/// when either the bucket or key segment is absent.
fn split_bucket_and_key(rest: &str) -> Result<(String, String), PmTilesError> {
    let rest = rest.trim_start_matches('/');
    if rest.is_empty() {
        return Err(PmTilesError::HttpError(
            "URI is missing both bucket and key segments".into(),
        ));
    }

    let (bucket, key) = rest.split_once('/').ok_or_else(|| {
        PmTilesError::HttpError(format!(
            "URI is missing key segment after bucket (got '{rest}')"
        ))
    })?;

    if bucket.is_empty() {
        return Err(PmTilesError::HttpError(
            "URI has an empty bucket segment".into(),
        ));
    }
    if key.is_empty() {
        return Err(PmTilesError::HttpError(
            "URI has an empty key segment after the bucket".into(),
        ));
    }

    Ok((bucket.to_string(), key.to_string()))
}

// ─────────────────────────────────────────────────────────────────────────────
// Blocking runtime helper
// ─────────────────────────────────────────────────────────────────────────────

/// Run an async future to completion on the calling thread.
///
/// Works regardless of whether the calling thread is already inside a Tokio
/// runtime (uses `block_in_place` + `Handle::block_on` in that case) or not
/// (spins up a temporary current-thread runtime).
fn block_on<F, T>(future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(future)),
        Err(_) => {
            #[allow(clippy::expect_used)]
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build tokio runtime for CloudPmTilesReader");
            rt.block_on(future)
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// LeafCacheKey
// ─────────────────────────────────────────────────────────────────────────────

/// Cache key for a decoded leaf directory page.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LeafCacheKey {
    offset: u64,
    length: u32,
}

// ─────────────────────────────────────────────────────────────────────────────
// CloudPmTilesReader
// ─────────────────────────────────────────────────────────────────────────────

/// Synchronous PMTiles v3 reader that fetches tile data from cloud storage
/// (S3, GCS, Azure Blob) via HTTP range-GET requests.
///
/// # Credentials
///
/// Use [`CloudCredentials::anonymous`] for public buckets or presigned URLs.
/// Use [`CloudCredentials::bearer`] to attach an `Authorization: Bearer`
/// header (OAuth2, GCS service accounts via token exchange, Azure SAS bearer).
///
/// S3 SigV4 request signing is **not** implemented in this slice; use
/// presigned S3 URLs (which are plain HTTPS GET-able) instead.
///
/// # Caching
///
/// Decoded leaf directories are cached in memory by `(offset, length)` so
/// that subsequent tile reads within the same leaf incur no extra network
/// round-trips.  The header is cached after the first call to
/// [`read_header`](CloudPmTilesReader::read_header) or implicitly after
/// [`read_tile`](CloudPmTilesReader::read_tile).
pub struct CloudPmTilesReader {
    /// Canonical HTTPS URL for the remote archive.
    base_url: Url,
    /// Credential bundle applied to every request.
    credentials: CloudCredentials,
    /// Parsed PMTiles v3 header, populated on first access.
    header_cache: Option<PmTilesHeader>,
    /// Decoded root directory, populated together with the header.
    root_dir_cache: Option<Vec<DirectoryEntry>>,
    /// Decoded leaf-directory pages keyed by `(offset, length)`.
    leaf_cache: std::collections::HashMap<LeafCacheKey, Vec<DirectoryEntry>>,
}

impl CloudPmTilesReader {
    // ── Construction ─────────────────────────────────────────────────────────

    /// Create a new reader from a parsed [`CloudObjectUri`] and credentials.
    ///
    /// This does **not** perform any network I/O; the header is fetched lazily
    /// on the first call to [`read_header`](Self::read_header) or
    /// [`read_tile`](Self::read_tile).
    ///
    /// # Errors
    /// Returns [`PmTilesError::HttpError`] when [`CloudObjectUri::to_https_url`]
    /// fails.
    pub fn new(uri: CloudObjectUri, credentials: CloudCredentials) -> Result<Self, PmTilesError> {
        let base_url = uri.to_https_url()?;
        Ok(Self {
            base_url,
            credentials,
            header_cache: None,
            root_dir_cache: None,
            leaf_cache: std::collections::HashMap::new(),
        })
    }

    /// Convenience constructor for S3 URIs.
    ///
    /// Parses `uri_str` as an `s3://bucket/key` URI and overrides the default
    /// region with the supplied `region` string.
    ///
    /// # Errors
    /// Propagates errors from [`CloudObjectUri::parse`] or
    /// [`CloudObjectUri::to_https_url`].
    pub fn from_s3_uri(
        uri_str: &str,
        region: &str,
        credentials: CloudCredentials,
    ) -> Result<Self, PmTilesError> {
        let mut uri = CloudObjectUri::parse(uri_str)?;
        if let CloudProvider::S3 { region: ref mut r } = uri.provider {
            *r = region.to_string();
        } else {
            return Err(PmTilesError::HttpError(format!(
                "Expected an S3 URI (s3://…), got a different scheme: {uri_str}"
            )));
        }
        Self::new(uri, credentials)
    }

    /// Convenience constructor for GCS URIs (`gs://bucket/key`).
    ///
    /// # Errors
    /// Propagates errors from [`CloudObjectUri::parse`] or
    /// [`CloudObjectUri::to_https_url`].
    pub fn from_gcs_uri(
        uri_str: &str,
        credentials: CloudCredentials,
    ) -> Result<Self, PmTilesError> {
        let uri = CloudObjectUri::parse(uri_str)?;
        if !matches!(uri.provider, CloudProvider::Gcs) {
            return Err(PmTilesError::HttpError(format!(
                "Expected a GCS URI (gs://…), got a different scheme: {uri_str}"
            )));
        }
        Self::new(uri, credentials)
    }

    /// Convenience constructor for Azure Blob Storage URIs
    /// (`az://account/container/blob`).
    ///
    /// The `_account` parameter is accepted for API symmetry with the S3
    /// constructor but is not used: the account is extracted directly from
    /// the URI.
    ///
    /// # Errors
    /// Propagates errors from [`CloudObjectUri::parse`] or
    /// [`CloudObjectUri::to_https_url`].
    pub fn from_azure_uri(
        uri_str: &str,
        _account: &str,
        credentials: CloudCredentials,
    ) -> Result<Self, PmTilesError> {
        let uri = CloudObjectUri::parse(uri_str)?;
        if !matches!(uri.provider, CloudProvider::AzureBlob { .. }) {
            return Err(PmTilesError::HttpError(format!(
                "Expected an Azure Blob URI (az://…), got a different scheme: {uri_str}"
            )));
        }
        Self::new(uri, credentials)
    }

    // ── Network I/O ──────────────────────────────────────────────────────────

    /// Fetch bytes `[start, start+length)` from the remote archive.
    ///
    /// Issues an HTTP GET with `Range: bytes={start}-{end_inclusive}`.
    /// Adds `Authorization: Bearer <token>` when a bearer token is present in
    /// the credentials.
    ///
    /// # Errors
    /// Returns [`PmTilesError::HttpError`] on any HTTP or network error.
    pub fn read_range(&self, start: u64, length: usize) -> Result<Vec<u8>, PmTilesError> {
        block_on(self.read_range_async(start, length))
    }

    /// Async implementation of [`read_range`](Self::read_range).
    async fn read_range_async(&self, start: u64, length: usize) -> Result<Vec<u8>, PmTilesError> {
        if length == 0 {
            return Ok(Vec::new());
        }

        let end_inclusive = start + length as u64 - 1;
        let range_header = format!("bytes={start}-{end_inclusive}");

        let client = reqwest::Client::new();
        let mut request_builder = client
            .get(self.base_url.as_str())
            .header(reqwest::header::RANGE, &range_header);

        if let Some(ref token) = self.credentials.bearer_token {
            request_builder =
                request_builder.header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"));
        }

        let response = request_builder
            .send()
            .await
            .map_err(|e| PmTilesError::HttpError(format!("Request failed: {e}")))?;

        let status = response.status();
        if !status.is_success() {
            return Err(PmTilesError::HttpError(format!(
                "HTTP {status} for range request [{start}-{end_inclusive}] on {}",
                self.base_url
            )));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| PmTilesError::HttpError(format!("Failed to read response body: {e}")))?;

        Ok(bytes.to_vec())
    }

    // ── Header / directory ───────────────────────────────────────────────────

    /// Return a reference to the parsed PMTiles v3 header, fetching and
    /// caching it from the remote archive on the first call.
    ///
    /// Fetches the first [`PMTILES_HEADER_SIZE`] bytes and also prefetches the
    /// root directory in the same invocation (to avoid a second round-trip on
    /// the first [`read_tile`](Self::read_tile) call).
    ///
    /// # Errors
    /// - [`PmTilesError::HttpError`] — network failure.
    /// - [`PmTilesError::InvalidArchive`] — bad magic bytes.
    /// - [`PmTilesError::InvalidFormat`] — truncated header.
    /// - [`PmTilesError::UnsupportedVersion`] — spec version ≠ 3.
    pub fn read_header(&mut self) -> Result<&PmTilesHeader, PmTilesError> {
        if self.header_cache.is_none() {
            self.fetch_and_cache_header()?;
        }
        // Safety: we just populated the cache in the branch above.
        #[allow(clippy::expect_used)]
        Ok(self.header_cache.as_ref().expect("header_cache populated"))
    }

    /// Fetch the header and root directory from the remote archive and store
    /// them in the in-memory caches.
    fn fetch_and_cache_header(&mut self) -> Result<(), PmTilesError> {
        // ── Step 1: Fetch the 127-byte fixed header ───────────────────────
        let header_bytes = self.read_range(0, PMTILES_HEADER_SIZE)?;

        if !header_bytes.starts_with(PMTILES_MAGIC) {
            return Err(PmTilesError::InvalidArchive(
                "Response does not start with PMTiles magic bytes".into(),
            ));
        }

        let header = PmTilesHeader::parse(&header_bytes)?;

        // ── Step 2: Fetch and decode the root directory ───────────────────
        let root_dir = if header.root_dir_length == 0 {
            Vec::new()
        } else {
            let raw = self.read_range(header.root_dir_offset, header.root_dir_length as usize)?;
            let decompressed = decompress_data(&raw, &header.internal_compression)?;
            decode_directory(&decompressed)?
        };

        self.root_dir_cache = Some(root_dir);
        self.header_cache = Some(header);
        Ok(())
    }

    // ── Tile retrieval ───────────────────────────────────────────────────────

    /// Retrieve the raw tile payload for `(z, x, y)` from the remote archive.
    ///
    /// Returns `Ok(Some(bytes))` when the tile is present, `Ok(None)` when it
    /// is absent from the archive.
    ///
    /// The header and root directory are fetched and cached on the first call.
    /// Leaf directories are also cached; subsequent reads within the same leaf
    /// do not incur extra network round-trips.
    ///
    /// # Errors
    /// - [`PmTilesError::InvalidFormat`] — invalid coordinate for zoom level.
    /// - [`PmTilesError::HttpError`] / [`PmTilesError::IoError`] — network failure.
    /// - [`PmTilesError::InvalidArchive`] — archive structure inconsistency.
    pub fn read_tile(&mut self, z: u8, x: u32, y: u32) -> Result<Option<Vec<u8>>, PmTilesError> {
        let tile_id = zxy_to_tile_id(z, x, y)?;
        self.resolve_tile(tile_id)
    }

    /// Retrieve a tile by its raw Hilbert-curve tile ID.
    ///
    /// Same semantics as [`read_tile`](Self::read_tile) but skips the
    /// coordinate validation and Hilbert encoding step.
    ///
    /// # Errors
    /// Same as [`read_tile`](Self::read_tile).
    pub fn read_tile_by_id(&mut self, tile_id: u64) -> Result<Option<Vec<u8>>, PmTilesError> {
        self.resolve_tile(tile_id)
    }

    /// Core tile-resolution logic: searches the two-level directory tree and
    /// fetches tile data on a hit.
    fn resolve_tile(&mut self, tile_id: u64) -> Result<Option<Vec<u8>>, PmTilesError> {
        // Ensure header + root directory are loaded.
        if self.header_cache.is_none() {
            self.fetch_and_cache_header()?;
        }

        // Phase 1: binary-search the root directory.
        // Clone the entry so we release the borrow on `self` before the next
        // mutable borrow (for leaf fetching / tile fetching).
        let root_dir = self.root_dir_cache.as_deref().ok_or_else(|| {
            PmTilesError::InvalidArchive("root directory cache missing after header fetch".into())
        })?;

        let root_entry = match binary_search_entries(root_dir, tile_id) {
            Some(e) => e.clone(),
            None => return Ok(None),
        };

        if root_entry.is_tile() {
            // Direct tile reference.
            let abs_offset = self
                .header_cache
                .as_ref()
                .map(|h| h.tile_data_offset)
                .ok_or_else(|| PmTilesError::InvalidArchive("header cache missing".into()))?
                + root_entry.offset;
            let data = self.read_range(abs_offset, root_entry.length as usize)?;
            return Ok(Some(data));
        }

        // Phase 2: leaf directory pointer (run_length == 0).
        let leaf_key = LeafCacheKey {
            offset: root_entry.offset,
            length: root_entry.length,
        };

        if !self.leaf_cache.contains_key(&leaf_key) {
            let leaf_dirs_offset = self
                .header_cache
                .as_ref()
                .map(|h| h.leaf_dirs_offset)
                .ok_or_else(|| PmTilesError::InvalidArchive("header cache missing".into()))?;
            let internal_compression = self
                .header_cache
                .as_ref()
                .map(|h| h.internal_compression.clone())
                .ok_or_else(|| PmTilesError::InvalidArchive("header cache missing".into()))?;

            let abs_leaf_offset = leaf_dirs_offset + root_entry.offset;
            let raw_leaf = self.read_range(abs_leaf_offset, root_entry.length as usize)?;
            let decompressed = decompress_data(&raw_leaf, &internal_compression)?;
            let leaf_dir = decode_directory(&decompressed)?;
            self.leaf_cache.insert(leaf_key.clone(), leaf_dir);
        }

        // Phase 3: binary-search the cached leaf directory.
        let leaf_dir = self.leaf_cache.get(&leaf_key).ok_or_else(|| {
            PmTilesError::InvalidArchive(
                "leaf directory vanished from cache immediately after insertion".into(),
            )
        })?;

        match binary_search_entries(leaf_dir, tile_id) {
            Some(leaf_entry) if leaf_entry.is_tile() => {
                let tile_data_offset = self
                    .header_cache
                    .as_ref()
                    .map(|h| h.tile_data_offset)
                    .ok_or_else(|| PmTilesError::InvalidArchive("header cache missing".into()))?;
                let abs_offset = tile_data_offset + leaf_entry.offset;
                let length = leaf_entry.length as usize;
                let data = self.read_range(abs_offset, length)?;
                Ok(Some(data))
            }
            Some(_) => Err(PmTilesError::InvalidFormat(
                "Nested leaf directories are not supported in PMTiles v3".into(),
            )),
            None => Ok(None),
        }
    }

    // ── Accessors ─────────────────────────────────────────────────────────────

    /// Return a reference to the cached PMTiles header.
    ///
    /// Returns `None` when the header has not yet been fetched (i.e. neither
    /// [`read_header`](Self::read_header) nor [`read_tile`](Self::read_tile)
    /// has been called successfully).
    pub fn cached_header(&self) -> Option<&PmTilesHeader> {
        self.header_cache.as_ref()
    }

    /// Return the number of leaf directories currently held in the in-memory
    /// cache.
    pub fn cached_leaf_count(&self) -> usize {
        self.leaf_cache.len()
    }

    /// Return the canonical HTTPS URL for the remote archive.
    pub fn base_url(&self) -> &Url {
        &self.base_url
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests (pure URI parsing and URL construction — no network required)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Test 1 ────────────────────────────────────────────────────────────────

    /// Parse an S3 URI and verify provider, bucket, and key.
    #[test]
    fn test_cloud_object_uri_parse_s3_form() {
        let uri = CloudObjectUri::parse("s3://my-bucket/path/to/tiles.pmtiles").expect("parse ok");
        assert_eq!(uri.bucket, "my-bucket");
        assert_eq!(uri.key, "path/to/tiles.pmtiles");
        assert!(matches!(uri.provider, CloudProvider::S3 { .. }));
    }

    // ── Test 2 ────────────────────────────────────────────────────────────────

    /// Parse a GCS URI and verify the provider is Gcs.
    #[test]
    fn test_cloud_object_uri_parse_gcs_form() {
        let uri = CloudObjectUri::parse("gs://my-bucket/tiles.pmtiles").expect("parse ok");
        assert_eq!(uri.bucket, "my-bucket");
        assert_eq!(uri.key, "tiles.pmtiles");
        assert!(matches!(uri.provider, CloudProvider::Gcs));
    }

    // ── Test 3 ────────────────────────────────────────────────────────────────

    /// Parse an Azure Blob URI and verify account, container, and key.
    #[test]
    fn test_cloud_object_uri_parse_azure_form() {
        let uri =
            CloudObjectUri::parse("az://myaccount/mycontainer/tiles.pmtiles").expect("parse ok");
        assert_eq!(uri.bucket, "mycontainer");
        assert_eq!(uri.key, "tiles.pmtiles");
        assert!(
            matches!(&uri.provider, CloudProvider::AzureBlob { account } if account == "myaccount")
        );
    }

    // ── Test 4 ────────────────────────────────────────────────────────────────

    /// An `http://` URI is not a supported cloud scheme.
    #[test]
    fn test_cloud_object_uri_parse_invalid_scheme_errors() {
        let result = CloudObjectUri::parse("http://example.com/tiles.pmtiles");
        assert!(
            result.is_err(),
            "Expected error for unsupported scheme 'http'"
        );
    }

    // ── Test 5 ────────────────────────────────────────────────────────────────

    /// `to_https_url` for an S3 URI must contain `amazonaws.com`.
    #[test]
    fn test_cloud_object_uri_to_https_s3_virtual_host() {
        let uri = CloudObjectUri::parse("s3://my-bucket/path/tiles.pmtiles").expect("parse ok");
        let url = uri.to_https_url().expect("url ok");
        let url_str = url.as_str();
        assert!(
            url_str.contains("amazonaws.com"),
            "Expected amazonaws.com in '{url_str}'"
        );
        assert!(
            url_str.contains("my-bucket"),
            "Expected bucket name in '{url_str}'"
        );
    }

    // ── Test 6 ────────────────────────────────────────────────────────────────

    /// `to_https_url` for a GCS URI must contain `storage.googleapis.com`.
    #[test]
    fn test_cloud_object_uri_to_https_gcs() {
        let uri = CloudObjectUri::parse("gs://my-bucket/tiles.pmtiles").expect("parse ok");
        let url = uri.to_https_url().expect("url ok");
        let url_str = url.as_str();
        assert!(
            url_str.contains("storage.googleapis.com"),
            "Expected storage.googleapis.com in '{url_str}'"
        );
        assert!(
            url_str.contains("my-bucket"),
            "Expected bucket in '{url_str}'"
        );
    }

    // ── Test 7 ────────────────────────────────────────────────────────────────

    /// `to_https_url` for an Azure Blob URI must contain `blob.core.windows.net`.
    #[test]
    fn test_cloud_object_uri_to_https_azure_blob() {
        let uri =
            CloudObjectUri::parse("az://myaccount/mycontainer/tiles.pmtiles").expect("parse ok");
        let url = uri.to_https_url().expect("url ok");
        let url_str = url.as_str();
        assert!(
            url_str.contains("blob.core.windows.net"),
            "Expected blob.core.windows.net in '{url_str}'"
        );
        assert!(
            url_str.contains("myaccount"),
            "Expected account in '{url_str}'"
        );
        assert!(
            url_str.contains("mycontainer"),
            "Expected container in '{url_str}'"
        );
    }
}
