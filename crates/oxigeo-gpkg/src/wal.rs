//! SQLite Write-Ahead Log (WAL) parser and page overlay.
//!
//! QGIS and other SQLite clients may operate a database in WAL mode.  In that
//! mode the most recently committed pages live in the sibling `<file>-wal`
//! file rather than in the main `.gpkg` file.  Reading only the main file
//! returns **stale data** for any page that has been modified since the last
//! WAL checkpoint.
//!
//! This module parses the WAL binary format (as specified in
//! <https://www.sqlite.org/walformat.html>) and produces a merged byte vector
//! where WAL pages are substituted into the main database image, making the
//! combined result safe to hand to [`SqliteReader`](crate::sqlite_reader::SqliteReader).
//!
//! # WAL binary layout
//!
//! ```text
//! WAL file
//! ├── 32-byte WAL header
//! └── N × (24-byte frame header  +  page_size bytes of page data)
//! ```
//!
//! Frames are validated by their cumulative checksums.  Only committed frames
//! (those whose `db_size` header field is non-zero) are retained, and later
//! frames always override earlier frames for the same page number.

use std::collections::HashMap;

use crate::error::GpkgError;

// ── WAL header constants ────────────────────────────────────────────────────

/// Big-endian checksum variant magic (`0x377f0682` read as a big-endian u32).
const WAL_MAGIC_BE: u32 = 0x377f_0682;
/// Little-endian checksum variant magic (`0x377f0683` read as a big-endian u32).
const WAL_MAGIC_LE: u32 = 0x377f_0683;

const WAL_HEADER_SIZE: usize = 32;
const FRAME_HEADER_SIZE: usize = 24;

// ── Checksum algorithm ──────────────────────────────────────────────────────

/// Accumulate the SQLite WAL checksum over `data` starting from `(s0, s1)`.
///
/// When `big_endian` is `true` each 4-byte word is interpreted as big-endian;
/// otherwise as little-endian.  The input slice **must** be a multiple of 8
/// bytes (the algorithm processes pairs of 32-bit words).
///
/// This mirrors the `walChecksumBytes` function from the SQLite source code.
fn wal_checksum(data: &[u8], big_endian: bool, s0_init: u32, s1_init: u32) -> (u32, u32) {
    debug_assert!(
        data.len().is_multiple_of(8),
        "checksum input must be a multiple of 8 bytes"
    );

    let mut s0 = s0_init;
    let mut s1 = s1_init;

    let mut i = 0;
    while i + 8 <= data.len() {
        let word_a = if big_endian {
            u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]])
        } else {
            u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]])
        };
        let word_b = if big_endian {
            u32::from_be_bytes([data[i + 4], data[i + 5], data[i + 6], data[i + 7]])
        } else {
            u32::from_le_bytes([data[i + 4], data[i + 5], data[i + 6], data[i + 7]])
        };

        s0 = s0.wrapping_add(word_a).wrapping_add(s1);
        s1 = s1.wrapping_add(word_b).wrapping_add(s0);

        i += 8;
    }

    (s0, s1)
}

// ── WAL reader ──────────────────────────────────────────────────────────────

/// Parsed representation of a SQLite WAL file.
///
/// After construction via [`WalReader::from_bytes`] the `committed_pages` map
/// holds the **last valid committed frame** for each page number (1-indexed,
/// matching the SQLite convention).
#[derive(Debug)]
pub struct WalReader {
    /// Page size in bytes (must match the main database).
    page_size: u32,
    /// Map from 1-indexed page number to the most-recent committed page bytes.
    committed_pages: HashMap<u32, Vec<u8>>,
}

impl WalReader {
    /// Parse a WAL file from its raw bytes.
    ///
    /// Returns an empty `WalReader` (no committed pages) when `data` is empty,
    /// so that callers can pass a zero-length slice to mean "no WAL present".
    ///
    /// # Errors
    /// - [`GpkgError::InvalidWalMagic`] when the WAL header magic is unrecognised.
    /// - [`GpkgError::InvalidFormat`] when the WAL header is truncated.
    ///
    /// Individual frames with invalid checksums or mismatching salts are
    /// silently dropped (per the SQLite specification: parsing stops at the
    /// first invalid frame).
    pub fn from_bytes(data: &[u8]) -> Result<Self, GpkgError> {
        // An empty WAL file is a valid no-op.
        if data.is_empty() {
            return Ok(Self {
                page_size: 0,
                committed_pages: HashMap::new(),
            });
        }

        if data.len() < WAL_HEADER_SIZE {
            return Err(GpkgError::InvalidFormat(format!(
                "WAL data too short: need {WAL_HEADER_SIZE} bytes for header, got {}",
                data.len()
            )));
        }

        // ── Parse WAL header ────────────────────────────────────────────────
        let magic = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        let big_endian = match magic {
            WAL_MAGIC_BE => true,
            WAL_MAGIC_LE => false,
            other => return Err(GpkgError::InvalidWalMagic(other)),
        };

        // Bytes 8-11: page size.
        let page_size = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);

        // Bytes 16-19 / 20-23: salt values.
        let salt1 = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
        let salt2 = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);

        // Validate the header checksum (covers bytes 0-23, i.e. 24 bytes = 3 pairs of 8).
        let (hdr_s0, hdr_s1) = wal_checksum(&data[0..24], big_endian, 0, 0);
        let stored_hdr_s0 = u32::from_be_bytes([data[24], data[25], data[26], data[27]]);
        let stored_hdr_s1 = u32::from_be_bytes([data[28], data[29], data[30], data[31]]);
        if hdr_s0 != stored_hdr_s0 || hdr_s1 != stored_hdr_s1 {
            // Treat a bad header checksum as an invalid magic (corrupted file).
            return Err(GpkgError::InvalidFormat(
                "WAL header checksum mismatch".into(),
            ));
        }

        // ── Iterate frames ──────────────────────────────────────────────────
        let frame_size = FRAME_HEADER_SIZE + page_size as usize;

        // Pending frames: we accumulate all frames from the current "generation"
        // (same salt pair) and only promote them to committed_pages when we
        // encounter a commit frame (db_size != 0).  This faithfully mirrors
        // SQLite's behaviour: all frames since the last commit are either all
        // applied or all discarded.
        //
        // Map: page_no → (generation_index, page_data)  — generation_index is
        // the ordinal of the frame within the current (non-committed) run.
        let mut committed_pages: HashMap<u32, Vec<u8>> = HashMap::new();
        let mut pending: HashMap<u32, Vec<u8>> = HashMap::new();

        let mut cumulative_s0: u32 = hdr_s0;
        let mut cumulative_s1: u32 = hdr_s1;

        let mut offset = WAL_HEADER_SIZE;

        while offset + frame_size <= data.len() {
            let frame_hdr = &data[offset..offset + FRAME_HEADER_SIZE];
            let page_data = &data[offset + FRAME_HEADER_SIZE..offset + frame_size];

            // Frame header fields.
            let frame_page_no =
                u32::from_be_bytes([frame_hdr[0], frame_hdr[1], frame_hdr[2], frame_hdr[3]]);
            let db_size_after =
                u32::from_be_bytes([frame_hdr[4], frame_hdr[5], frame_hdr[6], frame_hdr[7]]);
            let frame_salt1 =
                u32::from_be_bytes([frame_hdr[8], frame_hdr[9], frame_hdr[10], frame_hdr[11]]);
            let frame_salt2 =
                u32::from_be_bytes([frame_hdr[12], frame_hdr[13], frame_hdr[14], frame_hdr[15]]);
            let stored_s0 =
                u32::from_be_bytes([frame_hdr[16], frame_hdr[17], frame_hdr[18], frame_hdr[19]]);
            let stored_s1 =
                u32::from_be_bytes([frame_hdr[20], frame_hdr[21], frame_hdr[22], frame_hdr[23]]);

            // Salt mismatch → stop (SQLite spec: frames after a salt change
            // belong to the next WAL generation and are not yet valid).
            if frame_salt1 != salt1 || frame_salt2 != salt2 {
                break;
            }

            // Accumulate checksum over frame header bytes 0-7, then over page data.
            // The checksum covers (frame_hdr[0..8]) followed by the page content.
            // Page data size must be a multiple of 8 for wal_checksum; page sizes
            // are always powers of two (≥ 512), so this is guaranteed.
            let (after_hdr_s0, after_hdr_s1) =
                wal_checksum(&frame_hdr[0..8], big_endian, cumulative_s0, cumulative_s1);
            let (frame_s0, frame_s1) =
                wal_checksum(page_data, big_endian, after_hdr_s0, after_hdr_s1);

            // Checksum mismatch → stop (per SQLite spec: truncate here).
            if frame_s0 != stored_s0 || frame_s1 != stored_s1 {
                break;
            }

            // Update cumulative checksum.
            cumulative_s0 = frame_s0;
            cumulative_s1 = frame_s1;

            // Record this frame in the pending set.
            pending.insert(frame_page_no, page_data.to_vec());

            // Commit frame: db_size field is non-zero.
            if db_size_after != 0 {
                // Move all pending pages into the committed set.
                for (pg_no, pg_data) in pending.drain() {
                    committed_pages.insert(pg_no, pg_data);
                }
            }

            offset += frame_size;
        }

        Ok(Self {
            page_size,
            committed_pages,
        })
    }

    /// Apply the WAL overlay to `main` database bytes.
    ///
    /// Returns a new `Vec<u8>` where every page covered by a committed WAL
    /// frame has been substituted.  Pages not mentioned in the WAL are copied
    /// verbatim from `main`.
    ///
    /// If there are no committed pages the returned vector is a clone of `main`.
    ///
    /// # Errors
    /// Returns [`GpkgError::InvalidFormat`] when the `page_size` stored in
    /// `self` is zero (which would arise only from an empty WAL, which should
    /// have no committed pages anyway).
    pub fn apply_to_main(&self, main: &[u8]) -> Result<Vec<u8>, GpkgError> {
        if self.committed_pages.is_empty() {
            return Ok(main.to_vec());
        }

        if self.page_size == 0 {
            return Err(GpkgError::InvalidFormat(
                "WAL page size is zero; cannot apply overlay".into(),
            ));
        }

        let page_size = self.page_size as usize;

        // Determine the minimum required length: main.len() or the highest
        // page referenced by the WAL, whichever is greater.
        let max_wal_page = self.committed_pages.keys().copied().max().unwrap_or(0);
        let required_len = (max_wal_page as usize) * page_size;
        let output_len = main.len().max(required_len);

        let mut output = Vec::with_capacity(output_len);
        output.extend_from_slice(main);
        // Pad with zeroes if WAL references pages beyond the main file.
        if output.len() < required_len {
            output.resize(required_len, 0u8);
        }

        // Overwrite each WAL page.
        for (&page_no, page_bytes) in &self.committed_pages {
            debug_assert!(page_no >= 1, "page numbers are 1-indexed");
            let start = (page_no as usize - 1) * page_size;
            let end = start + page_size;
            // Ensure the output buffer is large enough (should already be from above).
            if output.len() < end {
                output.resize(end, 0u8);
            }
            output[start..end].copy_from_slice(page_bytes);
        }

        Ok(output)
    }

    /// Return the page size recorded in the WAL header (0 for an empty WAL).
    pub fn page_size(&self) -> u32 {
        self.page_size
    }

    /// Return the number of distinct pages present in committed WAL frames.
    pub fn committed_page_count(&self) -> usize {
        self.committed_pages.len()
    }
}

// ── Convenience function ────────────────────────────────────────────────────

/// Parse `wal` bytes and overlay their committed pages onto `main`, returning
/// the merged database image.
///
/// This is the primary public entry point for WAL support.  Pass the raw
/// bytes of the `.gpkg` file and its sibling `-wal` file; receive a single
/// byte vector that can be handed directly to
/// [`SqliteReader::from_bytes`](crate::sqlite_reader::SqliteReader::from_bytes)
/// or [`GeoPackage::from_bytes`](crate::gpkg::GeoPackage::from_bytes).
///
/// # Errors
/// Propagates errors from [`WalReader::from_bytes`] or
/// [`WalReader::apply_to_main`].
pub fn overlay_wal(main: &[u8], wal: &[u8]) -> Result<Vec<u8>, GpkgError> {
    let reader = WalReader::from_bytes(wal)?;
    reader.apply_to_main(main)
}
