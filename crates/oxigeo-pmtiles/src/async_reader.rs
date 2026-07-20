//! Async PMTiles v3 reader backed by `tokio::io::AsyncRead + AsyncSeek`.
//!
//! This module is gated behind the `async` Cargo feature.  It provides
//! [`AsyncPmTilesReader`], a reader that performs all I/O asynchronously using
//! the Tokio runtime.  Any type implementing both [`AsyncRead`] and [`AsyncSeek`]
//! — including `tokio::fs::File`, `tokio::io::BufReader<std::io::Cursor<Vec<u8>>>`,
//! and custom in-memory sources — can serve as the backing store.
//!
//! # Directory caching
//!
//! Decoded leaf directory pages are cached in `AsyncPmTilesReader::leaf_cache`
//! keyed by `(leaf_section_relative_offset, leaf_section_byte_length)`.  The
//! first access to a leaf fetches and decodes it; subsequent accesses to tiles
//! in the same leaf are served from the cache without further I/O.
//!
//! # Usage
//!
//! ```rust,no_run
//! # #[cfg(feature = "async")]
//! # {
//! use oxigeo_pmtiles::AsyncPmTilesReader;
//! use tokio::io::BufReader;
//!
//! # async fn run() -> Result<(), oxigeo_pmtiles::PmTilesError> {
//! let file = tokio::fs::File::open("tiles.pmtiles").await?;
//! let mut reader = AsyncPmTilesReader::open(BufReader::new(file)).await?;
//! if let Some(data) = reader.get_tile(2, 1, 0).await? {
//!     println!("tile is {} bytes", data.len());
//! }
//! # Ok(())
//! # }
//! # }
//! ```

use std::collections::HashMap;
use std::io::SeekFrom;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt};

use crate::directory::{DirectoryEntry, decode_directory};
use crate::error::PmTilesError;
use crate::header::{Compression, PMTILES_HEADER_SIZE, PmTilesHeader};
use crate::hilbert::{tile_id_to_zxy, zxy_to_tile_id};
use crate::pmtiles::binary_search_entries;
use crate::pmtiles::decompress_data;

// ─────────────────────────────────────────────────────────────────────────────
// LeafCacheKey
// ─────────────────────────────────────────────────────────────────────────────

/// Cache key identifying a decoded leaf directory page.
///
/// Leaf pages are uniquely addressed by their position within the archive's
/// leaf-directory section (i.e. values taken directly from the root entry that
/// points to them, *before* adding `header.leaf_dirs_offset`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LeafCacheKey {
    /// Byte offset within the leaf-directory section.
    section_offset: u64,
    /// Byte length of this leaf page (possibly compressed).
    section_length: u32,
}

// ─────────────────────────────────────────────────────────────────────────────
// AsyncPmTilesReader
// ─────────────────────────────────────────────────────────────────────────────

/// Async PMTiles v3 reader backed by any `AsyncRead + AsyncSeek + Unpin` source.
///
/// Construct with [`AsyncPmTilesReader::open`], then retrieve tiles with
/// [`get_tile`](Self::get_tile) or [`get_tile_by_id`](Self::get_tile_by_id).
pub struct AsyncPmTilesReader<R>
where
    R: AsyncRead + AsyncSeek + Unpin,
{
    /// Underlying byte source.
    reader: R,
    /// Parsed 127-byte PMTiles v3 header.
    header: PmTilesHeader,
    /// Decoded root directory entries (possibly decompressed).
    root_dir: Vec<DirectoryEntry>,
    /// In-memory cache of decoded leaf directory pages, keyed by
    /// (section_relative_offset, compressed_byte_length).
    leaf_cache: HashMap<LeafCacheKey, Vec<DirectoryEntry>>,
}

impl<R> AsyncPmTilesReader<R>
where
    R: AsyncRead + AsyncSeek + Unpin,
{
    // ── Construction ──────────────────────────────────────────────────────────

    /// Open a PMTiles v3 archive from an async byte source.
    ///
    /// Reads the 127-byte header from the beginning of the stream, validates
    /// the magic bytes and version, then reads and decodes the root directory.
    ///
    /// # Errors
    /// - [`PmTilesError::InvalidFormat`] if the header is too short, the magic
    ///   is wrong, or the spec version is not 3.
    /// - [`PmTilesError::Io`] on underlying I/O failures.
    /// - [`PmTilesError::Decompression`] if the root directory is compressed
    ///   and decompression fails.
    pub async fn open(mut reader: R) -> Result<Self, PmTilesError> {
        // ── 1. Read the fixed 127-byte header ─────────────────────────────
        let mut header_bytes = vec![0u8; PMTILES_HEADER_SIZE];
        reader
            .read_exact(&mut header_bytes)
            .await
            .map_err(PmTilesError::Io)?;

        let header = PmTilesHeader::parse(&header_bytes)?;

        // ── 2. Read and decode the root directory ──────────────────────────
        reader
            .seek(SeekFrom::Start(header.root_dir_offset))
            .await
            .map_err(PmTilesError::Io)?;

        let mut root_bytes = vec![0u8; header.root_dir_length as usize];
        reader
            .read_exact(&mut root_bytes)
            .await
            .map_err(PmTilesError::Io)?;

        let root_decompressed = decompress_data(&root_bytes, &header.internal_compression)?;
        let root_dir = decode_directory(&root_decompressed)?;

        Ok(Self {
            reader,
            header,
            root_dir,
            leaf_cache: HashMap::new(),
        })
    }

    // ── Tile retrieval ────────────────────────────────────────────────────────

    /// Retrieve tile data for `(z, x, y)` coordinates.
    ///
    /// Returns `Ok(Some(bytes))` if the tile is present, `Ok(None)` if it is
    /// not in the archive.
    ///
    /// The returned bytes are the raw (possibly compressed) tile payload.
    ///
    /// # Errors
    /// - [`PmTilesError::InvalidFormat`] if coordinates are out of range.
    /// - [`PmTilesError::Io`] on underlying I/O failures.
    /// - [`PmTilesError::Decompression`] if a leaf directory cannot be
    ///   decompressed.
    pub async fn get_tile(
        &mut self,
        z: u8,
        x: u32,
        y: u32,
    ) -> Result<Option<Vec<u8>>, PmTilesError> {
        let tile_id = zxy_to_tile_id(z, x, y)?;
        self.get_tile_by_id(tile_id).await
    }

    /// Retrieve tile data by its Hilbert-curve tile ID.
    ///
    /// Returns `Ok(Some(bytes))` if the tile is present, `Ok(None)` if not.
    ///
    /// Performs a two-level directory traversal matching the PMTiles v3 spec:
    /// root directory → optional leaf directory → tile data bytes.
    ///
    /// # Errors
    /// Same as [`get_tile`](Self::get_tile).
    pub async fn get_tile_by_id(&mut self, tile_id: u64) -> Result<Option<Vec<u8>>, PmTilesError> {
        // ── Walk root directory ────────────────────────────────────────────
        let root_entry = binary_search_entries(&self.root_dir, tile_id);

        match root_entry {
            None => Ok(None),
            Some(entry) if entry.is_tile() => {
                // Direct tile reference: read from tile-data section.
                let abs_offset = self.header.tile_data_offset + entry.offset;
                let length = u64::from(entry.length);
                let data = self.read_range(abs_offset, length).await?;
                Ok(Some(data))
            }
            Some(entry) if entry.is_leaf_directory() => {
                // Leaf pointer: copy the fields we need before any mutable borrow.
                let leaf_section_offset = entry.offset;
                let leaf_section_length = entry.length;

                // Resolve and cache the leaf directory.
                self.ensure_leaf_cached(leaf_section_offset, leaf_section_length)
                    .await?;

                // Re-borrow the now-cached leaf entries.
                let cache_key = LeafCacheKey {
                    section_offset: leaf_section_offset,
                    section_length: leaf_section_length,
                };
                // SAFETY: we just inserted this key above via ensure_leaf_cached.
                let leaf_entry = self
                    .leaf_cache
                    .get(&cache_key)
                    .and_then(|entries| binary_search_entries(entries, tile_id))
                    .map(|e| (e.offset, e.length, e.run_length));

                match leaf_entry {
                    None => Ok(None),
                    Some((offset, length, run_length)) if run_length > 0 => {
                        let abs_offset = self.header.tile_data_offset + offset;
                        let data = self.read_range(abs_offset, u64::from(length)).await?;
                        Ok(Some(data))
                    }
                    Some(_) => {
                        // Nested leaf directories are not supported in PMTiles v3.
                        Err(PmTilesError::InvalidFormat(
                            "Nested leaf directories are not supported in PMTiles v3".into(),
                        ))
                    }
                }
            }
            Some(_) => Ok(None),
        }
    }

    // ── Metadata accessors ────────────────────────────────────────────────────

    /// Return a reference to the parsed PMTiles v3 header.
    pub fn header(&self) -> &PmTilesHeader {
        &self.header
    }

    /// Return a slice of the decoded root directory entries.
    pub fn root_directory(&self) -> &[DirectoryEntry] {
        &self.root_dir
    }

    /// Return the number of leaf directory pages currently cached in memory.
    pub fn cached_leaf_count(&self) -> usize {
        self.leaf_cache.len()
    }

    // ── Tile enumeration ──────────────────────────────────────────────────────

    /// Enumerate all logical tiles stored in the archive, in tile-ID order.
    ///
    /// Performs a two-level traversal of the directory structure.  Entries with
    /// `run_length > 1` are expanded: each of the `run_length` consecutive tile
    /// IDs shares the same data payload.  Leaf pointers (`run_length == 0`) are
    /// fetched and decoded.
    ///
    /// Returns a `Vec` of `(tile_id, z, x, y)` tuples.
    ///
    /// # Errors
    /// - [`PmTilesError::Io`] on underlying I/O failures.
    /// - [`PmTilesError::Decompression`] if a leaf directory cannot be
    ///   decompressed.
    /// - [`PmTilesError::InvalidFormat`] if a tile ID cannot be converted to
    ///   `(z, x, y)`.
    pub async fn enumerate_tiles(&mut self) -> Result<Vec<(u64, u8, u32, u32)>, PmTilesError> {
        // Collect root entries by value to avoid holding borrows across awaits.
        let root_snapshot: Vec<(u64, u64, u32, u32)> = self
            .root_dir
            .iter()
            .map(|e| (e.tile_id, e.offset, e.length, e.run_length))
            .collect();

        let mut tiles: Vec<(u64, u8, u32, u32)> = Vec::new();

        for (base_tile_id, offset, length, run_length) in root_snapshot {
            if run_length > 0 {
                // Tile entry — expand run.
                for i in 0..u64::from(run_length) {
                    let current_id = base_tile_id + i;
                    let (z, x, y) = tile_id_to_zxy(current_id)?;
                    tiles.push((current_id, z, x, y));
                }
            } else {
                // Leaf pointer — fetch, cache, and expand.
                self.ensure_leaf_cached(offset, length).await?;

                let cache_key = LeafCacheKey {
                    section_offset: offset,
                    section_length: length,
                };
                // Snapshot the leaf entries to avoid borrow issues.
                let leaf_snapshot: Vec<(u64, u32)> = self
                    .leaf_cache
                    .get(&cache_key)
                    .map(|entries| entries.iter().map(|e| (e.tile_id, e.run_length)).collect())
                    .unwrap_or_default();

                for (leaf_tile_id, leaf_run_length) in leaf_snapshot {
                    if leaf_run_length > 0 {
                        for i in 0..u64::from(leaf_run_length) {
                            let current_id = leaf_tile_id + i;
                            let (z, x, y) = tile_id_to_zxy(current_id)?;
                            tiles.push((current_id, z, x, y));
                        }
                    }
                    // Nested leaf directories (run_length == 0 inside a leaf)
                    // are not supported in PMTiles v3 — skip silently.
                }
            }
        }

        tiles.sort_by_key(|t| t.0);
        Ok(tiles)
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    /// Ensure the leaf directory at `(section_offset, section_length)` is
    /// decoded and present in `self.leaf_cache`.
    ///
    /// If the entry is already cached this is a no-op.
    async fn ensure_leaf_cached(
        &mut self,
        section_offset: u64,
        section_length: u32,
    ) -> Result<(), PmTilesError> {
        let cache_key = LeafCacheKey {
            section_offset,
            section_length,
        };
        if self.leaf_cache.contains_key(&cache_key) {
            return Ok(());
        }

        // Fetch compressed (or uncompressed) leaf bytes from the archive.
        let abs_offset = self.header.leaf_dirs_offset + section_offset;
        let raw_bytes = self
            .read_range(abs_offset, u64::from(section_length))
            .await?;

        // Decompress the leaf directory bytes.
        let decompressed = decompress_data(&raw_bytes, &self.header.internal_compression)?;
        let leaf_entries = decode_directory(&decompressed)?;

        self.leaf_cache.insert(cache_key, leaf_entries);
        Ok(())
    }

    /// Read exactly `length` bytes starting at absolute file `offset`.
    async fn read_range(&mut self, offset: u64, length: u64) -> Result<Vec<u8>, PmTilesError> {
        self.reader
            .seek(SeekFrom::Start(offset))
            .await
            .map_err(PmTilesError::Io)?;

        let mut buf = vec![0u8; length as usize];
        self.reader
            .read_exact(&mut buf)
            .await
            .map_err(PmTilesError::Io)?;

        Ok(buf)
    }

    // ── Compression passthrough ───────────────────────────────────────────────

    /// Return the tile payload compression algorithm declared by the archive.
    pub fn tile_compression(&self) -> &Compression {
        &self.header.tile_compression
    }

    /// Decompress a raw tile payload using the archive's `tile_compression`.
    ///
    /// # Errors
    /// Returns [`PmTilesError::Decompression`] on failure.
    pub fn decompress_tile(&self, raw_data: &[u8]) -> Result<Vec<u8>, PmTilesError> {
        decompress_data(raw_data, &self.header.tile_compression)
    }
}
