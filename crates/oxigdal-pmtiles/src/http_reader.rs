//! HTTP range-request reader for PMTiles archives stored on remote HTTP/HTTPS
//! servers.
//!
//! This module is gated behind the `http-range` Cargo feature.  It provides
//! [`HttpPmTilesReader`], a synchronous reader that issues `Range:` HTTP GET
//! requests against a remote PMTiles archive without downloading the whole file.
//!
//! Internally it delegates to [`oxigdal_streaming::cloud::HttpObjectStore`],
//! wrapping async calls with a thread-local Tokio runtime so callers do not
//! need to be inside an async context.
//!
//! # Directory caching
//!
//! The reader caches decoded leaf directories in `HttpPmTilesReader::leaf_cache`
//! keyed by `(leaf_section_offset, leaf_section_length)` within the PMTiles
//! leaf-directory section.  On the first access to a leaf the reader fetches
//! its bytes and decodes them; subsequent accesses to tiles in the same leaf
//! are served from the cache without any network round-trip.

#![cfg(feature = "http-range")]

use std::collections::HashMap;

use oxigdal_streaming::cloud::{CloudCredentials, CloudError, HttpObjectStore, ObjectUrl};

use crate::directory::{DirectoryEntry, decode_directory};
use crate::error::PmTilesError;
use crate::etag_cache::EtagCache;
use crate::header::{Compression, PMTILES_HEADER_SIZE, PMTILES_MAGIC, PmTilesHeader};
use crate::hilbert::zxy_to_tile_id;
use crate::pmtiles::binary_search_entries;

// ─────────────────────────────────────────────────────────────────────────────
// Blocking runtime helper
// ─────────────────────────────────────────────────────────────────────────────

/// Run an async future to completion, blocking the calling thread.
///
/// If the calling thread is already inside a Tokio runtime (e.g. inside
/// `#[tokio::test]`) we use `tokio::task::block_in_place` + `Handle::block_on`
/// to avoid a "cannot block inside async" panic.  Outside a runtime we spin up
/// a single-thread runtime and use that.
fn block_on_async<F, T>(future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            // We are inside an existing runtime.  `block_in_place` moves the
            // current thread out of the async worker pool temporarily so it is
            // safe to call blocking code.
            tokio::task::block_in_place(|| handle.block_on(future))
        }
        Err(_) => {
            // No runtime on this thread — create a minimal single-threaded one.
            // Propagate a build failure as a panic: building a current-thread
            // runtime can only fail due to OS resource exhaustion, which is an
            // unrecoverable condition.
            #[allow(clippy::expect_used)]
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build tokio runtime for HttpPmTilesReader");
            rt.block_on(future)
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// LeafCacheKey
// ─────────────────────────────────────────────────────────────────────────────

/// Cache key for a decoded leaf directory.
///
/// A leaf is uniquely identified by `(byte_offset_within_leaf_section,
/// byte_length_within_leaf_section)`.  Both values come directly from the root
/// directory entry whose `run_length == 0`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LeafCacheKey {
    offset: u64,
    length: u32,
}

// ─────────────────────────────────────────────────────────────────────────────
// HttpPmTilesReader
// ─────────────────────────────────────────────────────────────────────────────

/// Synchronous PMTiles v3 reader that fetches tile data via HTTP Range requests.
///
/// Construct one with [`HttpPmTilesReader::open`].  The constructor performs
/// two HTTP fetches:
///
/// 1. Bytes `0..127` — the 127-byte fixed header.
/// 2. The root directory (offset and length taken from the header).
///
/// All subsequent reads are on-demand: tile data is fetched only when
/// [`get_tile`](HttpPmTilesReader::get_tile) is called, and leaf directories are
/// fetched at most once per distinct leaf (subsequent accesses to tiles in the
/// same leaf are served from the in-memory `leaf_cache`).
///
/// # Credentials
///
/// Requests are sent anonymously (`CloudCredentials::Anonymous`).  Use
/// [`HttpPmTilesReader::open_with_credentials`] to supply a bearer token or
/// other credential type.
pub struct HttpPmTilesReader {
    /// Underlying HTTP transport (with retry policy).
    object_store: HttpObjectStore,
    /// Parsed cloud URL for the remote archive.
    url: ObjectUrl,
    /// Cloud credentials used for every request.
    credentials: CloudCredentials,
    /// Parsed PMTiles v3 header fetched during [`open`](Self::open).
    header: PmTilesHeader,
    /// Decoded root directory fetched during [`open`](Self::open).
    root_dir: Vec<DirectoryEntry>,
    /// Cache of decoded leaf directories, keyed by `(offset, length)` within
    /// the archive's leaf-directory section.
    leaf_cache: HashMap<LeafCacheKey, Vec<DirectoryEntry>>,
    /// Optional ETag-based byte-range cache.  When present, every
    /// [`fetch_range`](Self::fetch_range) call checks this cache first and
    /// populates it on a miss, avoiding redundant HTTP round-trips.
    etag_cache: Option<EtagCache>,
}

impl HttpPmTilesReader {
    // ── Construction ─────────────────────────────────────────────────────────

    /// Open a remote PMTiles archive at `url_str` using anonymous credentials.
    ///
    /// Parses the URL, fetches the 127-byte header, and fetches and decodes the
    /// root directory.  The root directory may itself be compressed; the
    /// `internal_compression` field of the parsed header controls how it is
    /// decoded.
    ///
    /// # Errors
    /// - [`PmTilesError::HttpError`] — URL parse failure or HTTP error.
    /// - [`PmTilesError::InvalidFormat`] — bad magic bytes or truncated header.
    /// - [`PmTilesError::UnsupportedVersion`] — PMTiles spec version ≠ 3.
    pub fn open(url_str: &str) -> Result<Self, PmTilesError> {
        Self::open_with_credentials(url_str, CloudCredentials::Anonymous)
    }

    /// Open a remote PMTiles archive at `url_str` using the given credentials.
    ///
    /// See [`open`](Self::open) for details; this variant allows bearer tokens,
    /// access-key credentials, or other [`CloudCredentials`] variants.
    pub fn open_with_credentials(
        url_str: &str,
        credentials: CloudCredentials,
    ) -> Result<Self, PmTilesError> {
        let url = ObjectUrl::parse(url_str)
            .map_err(|e: CloudError| PmTilesError::HttpError(e.to_string()))?;
        let object_store = HttpObjectStore::new();

        // ── Fetch header (127 bytes at offset 0) ──────────────────────────
        let header_bytes = Self::fetch_range_impl(
            &object_store,
            &url,
            &credentials,
            0,
            PMTILES_HEADER_SIZE as u64 - 1,
        )?;

        // Validate magic before attempting a full parse (gives a cleaner error
        // message when the URL points to a non-PMTiles resource).
        if !header_bytes.starts_with(PMTILES_MAGIC) {
            return Err(PmTilesError::InvalidArchive(
                "Response does not start with PMTiles magic bytes".into(),
            ));
        }

        let header = PmTilesHeader::parse(&header_bytes)?;

        // ── Fetch root directory ──────────────────────────────────────────
        let root_dir = Self::fetch_and_decode_directory_impl(
            &object_store,
            &url,
            &credentials,
            &header,
            header.root_dir_offset,
            header.root_dir_length,
        )?;

        Ok(Self {
            object_store,
            url,
            credentials,
            header,
            root_dir,
            leaf_cache: HashMap::new(),
            etag_cache: None,
        })
    }

    // ── Tile retrieval ───────────────────────────────────────────────────────

    /// Retrieve the raw tile payload for the tile at `(z, x, y)`.
    ///
    /// Returns `Ok(Some(bytes))` when the tile is present, `Ok(None)` when it
    /// is absent from the archive.  Tiles stored with run-length compression
    /// (consecutive identical tiles) are handled transparently.
    ///
    /// # Two-level directory walk
    ///
    /// The function first binary-searches the root directory for the given tile
    /// ID.  If the matching entry is a leaf-directory pointer (`run_length == 0`)
    /// it fetches and decodes that leaf (or serves it from
    /// `leaf_cache` if already loaded), then binary-searches
    /// the leaf entries for the tile.
    ///
    /// Tile data offsets from directory entries are *relative to the tile-data
    /// section* (`header.tile_data_offset`), so the absolute file offset is
    /// computed as `header.tile_data_offset + entry.offset`.
    ///
    /// # Errors
    /// - [`PmTilesError::InvalidFormat`] — invalid coordinate for zoom level.
    /// - [`PmTilesError::HttpError`] / [`PmTilesError::IoError`] — network failure.
    pub fn get_tile(&mut self, z: u8, x: u32, y: u32) -> Result<Option<Vec<u8>>, PmTilesError> {
        let tile_id = zxy_to_tile_id(z, x, y)?;
        self.resolve_tile(tile_id)
    }

    /// Retrieve a tile by its raw Hilbert-curve tile ID.
    ///
    /// Same semantics as [`get_tile`](Self::get_tile) but skips the coordinate
    /// validation and Hilbert encoding step.
    pub fn get_tile_by_id(&mut self, tile_id: u64) -> Result<Option<Vec<u8>>, PmTilesError> {
        self.resolve_tile(tile_id)
    }

    // ── Accessors ─────────────────────────────────────────────────────────────

    /// Return a reference to the parsed PMTiles header.
    pub fn header(&self) -> &PmTilesHeader {
        &self.header
    }

    /// Return a reference to the decoded root directory.
    pub fn root_directory(&self) -> &[DirectoryEntry] {
        &self.root_dir
    }

    /// Return the number of distinct leaf directories currently cached in
    /// memory.
    pub fn cached_leaf_count(&self) -> usize {
        self.leaf_cache.len()
    }

    /// Attach an ETag byte-range cache to this reader.
    ///
    /// After attachment every call to the internal `fetch_range` helper first
    /// checks the cache.  On a hit the cached bytes are returned directly,
    /// with no network request.  On a miss the bytes are fetched from the
    /// remote archive and the result is stored in the cache under the
    /// synthetic ETag `"<offset>-<length>"`.
    ///
    /// Setting `max_entries` to `0` effectively disables caching (the
    /// [`EtagCache::new(0)`](EtagCache::new) zero-capacity special case).
    ///
    /// # Example
    ///
    /// ```no_run
    /// # #[cfg(feature = "http-range")]
    /// # {
    /// use oxigdal_pmtiles::HttpPmTilesReader;
    ///
    /// let reader = HttpPmTilesReader::open("http://example.com/tiles.pmtiles")
    ///     .unwrap()
    ///     .with_etag_cache(128);
    /// # }
    /// ```
    pub fn with_etag_cache(mut self, max_entries: usize) -> Self {
        self.etag_cache = Some(EtagCache::new(max_entries));
        self
    }

    /// Return the number of byte ranges currently held in the ETag cache.
    ///
    /// Returns `0` when no cache has been attached via
    /// [`with_etag_cache`](Self::with_etag_cache) or when the attached cache
    /// has zero capacity.
    pub fn cached_byte_range_count(&self) -> usize {
        self.etag_cache.as_ref().map_or(0, EtagCache::len)
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    /// Core tile-resolution logic shared by [`get_tile`] and [`get_tile_by_id`].
    fn resolve_tile(&mut self, tile_id: u64) -> Result<Option<Vec<u8>>, PmTilesError> {
        // Phase 1: search root directory.
        let root_entry = match binary_search_entries(&self.root_dir, tile_id) {
            Some(e) => e.clone(),
            None => return Ok(None),
        };

        if root_entry.is_tile() {
            // Direct tile reference — data offset is relative to tile_data_offset.
            let abs_offset = self.header.tile_data_offset + root_entry.offset;
            let abs_end = abs_offset + u64::from(root_entry.length) - 1;
            let data = self.fetch_range(abs_offset, abs_end)?;
            return Ok(Some(data));
        }

        // Phase 2: leaf directory pointer (run_length == 0).
        // Offset & length are relative to the leaf_dirs section start.
        let leaf_key = LeafCacheKey {
            offset: root_entry.offset,
            length: root_entry.length,
        };

        if !self.leaf_cache.contains_key(&leaf_key) {
            // Fetch and decode this leaf directory page.
            let abs_leaf_offset = self.header.leaf_dirs_offset + root_entry.offset;
            let abs_leaf_end = abs_leaf_offset + u64::from(root_entry.length) - 1;
            let leaf_bytes = self.fetch_range(abs_leaf_offset, abs_leaf_end)?;
            let leaf_dir =
                Self::decode_possibly_compressed(&leaf_bytes, &self.header.internal_compression)?;
            self.leaf_cache.insert(leaf_key.clone(), leaf_dir);
        }

        // Phase 3: search within the cached leaf directory.
        // Safety: we inserted the leaf above in this function; the only way it
        // could be absent is a HashMap bug, which is not a recoverable error.
        let leaf_dir = match self.leaf_cache.get(&leaf_key) {
            Some(dir) => dir,
            None => {
                return Err(PmTilesError::InvalidArchive(
                    "leaf directory vanished from cache immediately after insertion".into(),
                ));
            }
        };

        match binary_search_entries(leaf_dir, tile_id) {
            Some(leaf_entry) if leaf_entry.is_tile() => {
                let abs_offset = self.header.tile_data_offset + leaf_entry.offset;
                let abs_end = abs_offset + u64::from(leaf_entry.length) - 1;
                let data = self.fetch_range(abs_offset, abs_end)?;
                Ok(Some(data))
            }
            Some(_) => {
                // Nested leaf pointer — not supported in PMTiles v3.
                Err(PmTilesError::InvalidFormat(
                    "Nested leaf directories are not supported in PMTiles v3".into(),
                ))
            }
            None => Ok(None),
        }
    }

    /// Fetch a byte range from the remote archive and decode it through the
    /// archive's `internal_compression` to produce a decoded directory.
    ///
    /// This is used for both the root directory and leaf directory pages.
    fn fetch_and_decode_directory_impl(
        store: &HttpObjectStore,
        url: &ObjectUrl,
        credentials: &CloudCredentials,
        header: &PmTilesHeader,
        section_offset: u64,
        section_length: u64,
    ) -> Result<Vec<DirectoryEntry>, PmTilesError> {
        if section_length == 0 {
            return Ok(Vec::new());
        }
        let raw = Self::fetch_range_impl(
            store,
            url,
            credentials,
            section_offset,
            section_offset + section_length - 1,
        )?;
        Self::decode_possibly_compressed(&raw, &header.internal_compression)
    }

    /// Decompress `data` according to `compression` and then decode the
    /// directory entries.
    fn decode_possibly_compressed(
        data: &[u8],
        compression: &Compression,
    ) -> Result<Vec<DirectoryEntry>, PmTilesError> {
        // Use the existing pmtiles::decompress_data helper which honours the
        // `compression` feature flag correctly.
        let decompressed = crate::pmtiles::decompress_data(data, compression)?;
        decode_directory(&decompressed)
    }

    /// Fetch a byte range from the remote archive, using `self`'s store, URL,
    /// and credentials.
    ///
    /// When an [`EtagCache`] is attached (via [`with_etag_cache`](Self::with_etag_cache))
    /// this method first checks whether the requested range is already cached.
    /// On a cache hit the stored bytes are returned immediately with no network
    /// request.  On a miss the bytes are fetched, stored in the cache under the
    /// synthetic ETag `"<start>-<length>"`, and then returned to the caller.
    fn fetch_range(&mut self, start: u64, end_inclusive: u64) -> Result<Vec<u8>, PmTilesError> {
        // Derive the canonical (offset, length) pair for cache keying.
        let length = end_inclusive.saturating_sub(start) + 1;

        // Fast path: ETag cache hit — no network call needed.
        if let Some(ref mut cache) = self.etag_cache {
            if let Some((cached_data, _etag)) = cache.get(start, length) {
                return Ok(cached_data);
            }
        }

        // Slow path: network fetch.
        let data = Self::fetch_range_impl(
            &self.object_store,
            &self.url,
            &self.credentials,
            start,
            end_inclusive,
        )?;

        // Populate the cache with the freshly fetched bytes.
        if let Some(ref mut cache) = self.etag_cache {
            let synthetic_etag = format!("{start}-{length}");
            cache.insert(start, length, synthetic_etag, data.clone());
        }

        Ok(data)
    }

    /// Static variant of [`fetch_range`] used before `Self` is fully
    /// constructed (i.e. during [`open`](Self::open)).
    fn fetch_range_impl(
        store: &HttpObjectStore,
        url: &ObjectUrl,
        credentials: &CloudCredentials,
        start: u64,
        end_inclusive: u64,
    ) -> Result<Vec<u8>, PmTilesError> {
        let bytes_result = block_on_async(store.get_range(url, start, end_inclusive, credentials));
        match bytes_result {
            Ok(b) => Ok(b.to_vec()),
            Err(e) => Err(PmTilesError::IoError(e.to_string())),
        }
    }
}
