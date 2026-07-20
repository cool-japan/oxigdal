//! Integration tests for the SQLite WAL (Write-Ahead Log) overlay.
//!
//! All test data is constructed synthetically in memory; no fixture files are
//! required.  Where temporary files are needed, `std::env::temp_dir()` is used.

use oxigeo_gpkg::error::GpkgError;
use oxigeo_gpkg::wal::{WalReader, overlay_wal};

// ── Test helpers ────────────────────────────────────────────────────────────

/// Checksum algorithm used in `build_wal_frame` and `build_wal_header`.
fn wal_checksum_bytes(data: &[u8], big_endian: bool, s0_init: u32, s1_init: u32) -> (u32, u32) {
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

/// Build a minimal SQLite database image with `n_pages` pages of `page_size` bytes.
///
/// Page 1 begins with the 16-byte SQLite magic and has the page size encoded at
/// offset 16 (big-endian u16).  All remaining pages are zero-filled.
fn build_minimal_sqlite_db(page_size: u32, n_pages: u32) -> Vec<u8> {
    assert!(n_pages >= 1, "need at least one page");
    let total = (page_size * n_pages) as usize;
    let mut db = vec![0u8; total];

    // SQLite magic string at offset 0 of page 1.
    let magic = b"SQLite format 3\x00";
    db[..16].copy_from_slice(magic);

    // Page size at offset 16 (big-endian u16; value 1 encodes 65536, but we
    // only use standard sizes in tests so a plain cast is fine).
    let ps_encoded: u16 = if page_size == 65536 {
        1
    } else {
        page_size as u16
    };
    db[16..18].copy_from_slice(&ps_encoded.to_be_bytes());

    // db_size_pages at offset 28 (big-endian u32).
    db[28..32].copy_from_slice(&n_pages.to_be_bytes());

    db
}

/// Build a 32-byte WAL header with correct checksums.
///
/// `little_endian` selects the checksum variant:
/// - `false` → magic `0x377f0682`, big-endian checksums
/// - `true`  → magic `0x377f0683`, little-endian checksums
///
/// Returns `(header_bytes, salt1, salt2, hdr_s0, hdr_s1)`.
fn build_wal_header(page_size: u32, little_endian: bool) -> (Vec<u8>, u32, u32, u32, u32) {
    let magic: u32 = if little_endian {
        0x377f_0683
    } else {
        0x377f_0682
    };
    let file_format_version: u32 = 3_007_000;
    let ckpt_seq: u32 = 0;
    let salt1: u32 = 0xDEAD_BEEF;
    let salt2: u32 = 0xCAFE_BABE;

    let mut hdr = vec![0u8; 32];
    hdr[0..4].copy_from_slice(&magic.to_be_bytes());
    hdr[4..8].copy_from_slice(&file_format_version.to_be_bytes());
    hdr[8..12].copy_from_slice(&page_size.to_be_bytes());
    hdr[12..16].copy_from_slice(&ckpt_seq.to_be_bytes());
    hdr[16..20].copy_from_slice(&salt1.to_be_bytes());
    hdr[20..24].copy_from_slice(&salt2.to_be_bytes());

    // Checksum of bytes 0-23.
    let big_endian = !little_endian;
    let (s0, s1) = wal_checksum_bytes(&hdr[0..24], big_endian, 0, 0);
    hdr[24..28].copy_from_slice(&s0.to_be_bytes());
    hdr[28..32].copy_from_slice(&s1.to_be_bytes());

    (hdr, salt1, salt2, s0, s1)
}

/// Build a 24-byte frame header + page_size bytes of page data into a single
/// frame blob.
///
/// Returns `(frame_bytes, updated_s0, updated_s1)`.
///
/// # Parameters
/// - `page_no`: 1-indexed database page number
/// - `page_data`: exactly `page_size` bytes to store in this frame
/// - `db_size`: if non-zero this is a commit frame; SQLite uses the database
///   size in pages after this commit
/// - `salt1 / salt2`: must match the WAL header
/// - `prev_s0 / prev_s1`: cumulative checksum state entering this frame
/// - `little_endian`: checksum byte order
#[allow(clippy::too_many_arguments)]
fn build_wal_frame(
    page_no: u32,
    page_data: &[u8],
    db_size: u32,
    salt1: u32,
    salt2: u32,
    prev_s0: u32,
    prev_s1: u32,
    little_endian: bool,
) -> (Vec<u8>, u32, u32) {
    let big_endian = !little_endian;

    let mut frame_hdr = vec![0u8; 24];
    frame_hdr[0..4].copy_from_slice(&page_no.to_be_bytes());
    frame_hdr[4..8].copy_from_slice(&db_size.to_be_bytes());
    frame_hdr[8..12].copy_from_slice(&salt1.to_be_bytes());
    frame_hdr[12..16].copy_from_slice(&salt2.to_be_bytes());

    // Cumulative checksum: first over frame header bytes 0-7, then over page data.
    let (s0_after_hdr, s1_after_hdr) =
        wal_checksum_bytes(&frame_hdr[0..8], big_endian, prev_s0, prev_s1);
    let (s0, s1) = wal_checksum_bytes(page_data, big_endian, s0_after_hdr, s1_after_hdr);

    frame_hdr[16..20].copy_from_slice(&s0.to_be_bytes());
    frame_hdr[20..24].copy_from_slice(&s1.to_be_bytes());

    let mut frame = Vec::with_capacity(24 + page_data.len());
    frame.extend_from_slice(&frame_hdr);
    frame.extend_from_slice(page_data);

    (frame, s0, s1)
}

// ── Magic tests ─────────────────────────────────────────────────────────────

#[test]
fn test_wal_parses_valid_magic_le() {
    let page_size: u32 = 4096;
    let (hdr, _, _, _, _) = build_wal_header(page_size, true);
    // Just the header — no frames — should parse without error.
    let reader = WalReader::from_bytes(&hdr).expect("LE WAL header should parse");
    assert_eq!(reader.page_size(), page_size);
    assert_eq!(reader.committed_page_count(), 0);
}

#[test]
fn test_wal_parses_valid_magic_be() {
    let page_size: u32 = 4096;
    let (hdr, _, _, _, _) = build_wal_header(page_size, false);
    let reader = WalReader::from_bytes(&hdr).expect("BE WAL header should parse");
    assert_eq!(reader.page_size(), page_size);
    assert_eq!(reader.committed_page_count(), 0);
}

#[test]
fn test_wal_invalid_magic_returns_error() {
    let mut hdr = vec![0u8; 32];
    // Write a bogus magic.
    hdr[0..4].copy_from_slice(&0xDEAD_BEEFu32.to_be_bytes());
    let result = WalReader::from_bytes(&hdr);
    assert!(
        matches!(result, Err(GpkgError::InvalidWalMagic(_))),
        "expected InvalidWalMagic, got {result:?}"
    );
}

// ── Overlay tests ────────────────────────────────────────────────────────────

#[test]
fn test_wal_overlay_replaces_main_page() {
    let page_size: u32 = 512;
    let n_pages: u32 = 2;

    let main = build_minimal_sqlite_db(page_size, n_pages);

    // Build a WAL that overwrites page 1 with a recognisable pattern.
    let (mut wal, salt1, salt2, hdr_s0, hdr_s1) = build_wal_header(page_size, false);

    let mut wal_page1 = vec![0u8; page_size as usize];
    // The first 16 bytes of WAL page 1 must still be the SQLite magic for the
    // merged result to pass SqliteReader's magic check — but this test only
    // checks the overlay bytes, not the full parse.  Fill with a pattern.
    for (idx, byte) in wal_page1.iter_mut().enumerate() {
        *byte = (idx as u8).wrapping_add(0x42);
    }

    let (frame1, _, _) = build_wal_frame(
        1, &wal_page1, n_pages, // commit frame: db_size = n_pages
        salt1, salt2, hdr_s0, hdr_s1, false,
    );
    wal.extend_from_slice(&frame1);

    let merged = overlay_wal(&main, &wal).expect("overlay should succeed");

    // The first page of the merged image must be our WAL page 1.
    let ps = page_size as usize;
    assert_eq!(&merged[0..ps], wal_page1.as_slice());

    // Page 2 should be unchanged (all zeros beyond the sqlite header).
    assert_eq!(&merged[ps..ps * 2], &main[ps..ps * 2]);
}

#[test]
fn test_wal_overlay_preserves_unaffected_pages() {
    let page_size: u32 = 512;
    let n_pages: u32 = 2;

    let main = build_minimal_sqlite_db(page_size, n_pages);

    // Build a WAL that only touches page 2.
    let (mut wal, salt1, salt2, hdr_s0, hdr_s1) = build_wal_header(page_size, false);

    let mut wal_page2 = vec![0xABu8; page_size as usize];
    wal_page2[0] = 0x99; // distinguishable sentinel

    let (frame2, _, _) =
        build_wal_frame(2, &wal_page2, n_pages, salt1, salt2, hdr_s0, hdr_s1, false);
    wal.extend_from_slice(&frame2);

    let merged = overlay_wal(&main, &wal).expect("overlay should succeed");

    let ps = page_size as usize;
    // Page 1 must be the original main page 1.
    assert_eq!(&merged[0..ps], &main[0..ps]);
    // Page 2 must be our WAL page.
    assert_eq!(&merged[ps..ps * 2], wal_page2.as_slice());
}

#[test]
fn test_wal_invalid_checksum_skips_frame() {
    let page_size: u32 = 512;
    let n_pages: u32 = 1;

    let main = build_minimal_sqlite_db(page_size, n_pages);

    let (mut wal, salt1, salt2, hdr_s0, hdr_s1) = build_wal_header(page_size, false);

    let wal_page1 = vec![0xFFu8; page_size as usize];
    let (mut frame1, _, _) =
        build_wal_frame(1, &wal_page1, n_pages, salt1, salt2, hdr_s0, hdr_s1, false);

    // Corrupt the frame checksum bytes (frame header bytes 16-19).
    frame1[16] ^= 0xFF;

    wal.extend_from_slice(&frame1);

    // Parsing should succeed but the corrupted frame is silently dropped.
    let reader = WalReader::from_bytes(&wal).expect("parse should succeed despite bad checksum");
    assert_eq!(
        reader.committed_page_count(),
        0,
        "corrupted frame must be dropped"
    );

    // The overlay should return the original main bytes.
    let merged = overlay_wal(&main, &wal).expect("overlay should succeed");
    assert_eq!(merged, main);
}

#[test]
fn test_wal_truncated_returns_only_valid_prefix_frames() {
    let page_size: u32 = 512;
    let n_pages: u32 = 2;

    let main = build_minimal_sqlite_db(page_size, n_pages);

    let (mut wal, salt1, salt2, hdr_s0, hdr_s1) = build_wal_header(page_size, false);

    // Frame 1: a valid commit frame covering page 1.
    let wal_page1 = vec![0x11u8; page_size as usize];
    let (frame1, s0_after1, s1_after1) =
        build_wal_frame(1, &wal_page1, 1, salt1, salt2, hdr_s0, hdr_s1, false);
    wal.extend_from_slice(&frame1);

    // Frame 2: a valid commit frame covering page 2.
    let wal_page2 = vec![0x22u8; page_size as usize];
    let (frame2, _, _) =
        build_wal_frame(2, &wal_page2, 2, salt1, salt2, s0_after1, s1_after1, false);
    // Only write half of frame 2 → truncated WAL.
    wal.extend_from_slice(&frame2[..frame2.len() / 2]);

    let reader = WalReader::from_bytes(&wal).expect("parse should succeed");
    // Only the complete frame 1 should be committed.
    assert_eq!(reader.committed_page_count(), 1);

    let merged = overlay_wal(&main, &wal).expect("overlay should succeed");
    let ps = page_size as usize;
    assert_eq!(
        &merged[0..ps],
        wal_page1.as_slice(),
        "page 1 should be from WAL"
    );
    assert_eq!(
        &merged[ps..ps * 2],
        &main[ps..ps * 2],
        "page 2 should be from main"
    );
}

#[test]
fn test_wal_empty_wal_returns_original_main() {
    let page_size: u32 = 512;
    let n_pages: u32 = 3;

    let main = build_minimal_sqlite_db(page_size, n_pages);

    // Zero-byte WAL.
    let merged = overlay_wal(&main, &[]).expect("empty WAL overlay should succeed");
    assert_eq!(merged, main, "empty WAL must return exact main bytes");
}

// ── GeoPackage::from_files integration test ──────────────────────────────────

#[test]
fn test_from_files_no_wal_passes_main_bytes() {
    use oxigeo_gpkg::gpkg::GeoPackage;

    let page_size: u32 = 4096;
    let n_pages: u32 = 1;
    let main = build_minimal_sqlite_db(page_size, n_pages);

    // Without WAL: should behave identically to from_bytes.
    let gpkg =
        GeoPackage::from_files(main.clone(), None).expect("from_files with no WAL should succeed");
    assert_eq!(gpkg.page_size(), page_size);
}

#[test]
fn test_from_files_with_wal_applies_overlay() {
    use oxigeo_gpkg::gpkg::GeoPackage;

    let page_size: u32 = 4096;
    let n_pages: u32 = 2;

    let mut main = build_minimal_sqlite_db(page_size, n_pages);

    // WAL overwrites page 2 with 0xBB pattern; page 1 keeps the SQLite header.
    let (mut wal, salt1, salt2, hdr_s0, hdr_s1) = build_wal_header(page_size, false);
    let wal_page2 = vec![0xBBu8; page_size as usize];
    let (frame2, _, _) =
        build_wal_frame(2, &wal_page2, n_pages, salt1, salt2, hdr_s0, hdr_s1, false);
    wal.extend_from_slice(&frame2);

    // from_bytes on `main` alone must succeed (to get a baseline).
    let _gpkg_no_wal =
        GeoPackage::from_bytes(main.clone()).expect("main alone should be valid SQLite");

    // Apply the overlay manually and verify page 2 content.
    let merged = overlay_wal(&main, &wal).expect("overlay should succeed");
    let ps = page_size as usize;
    assert_eq!(&merged[ps..ps * 2], wal_page2.as_slice());

    // from_files should produce an equivalent result.
    // We need a fresh `main` because from_files consumes it.
    let gpkg = GeoPackage::from_files(main.clone(), Some(wal))
        .expect("from_files with WAL should succeed");
    assert_eq!(gpkg.page_size(), page_size);

    // Verify the merged bytes via raw_data (reader exposes raw_data).
    let raw = gpkg.reader.raw_data();
    assert_eq!(&raw[ps..ps * 2], wal_page2.as_slice());

    // Suppress the unused-variable warning on the baseline.
    let _ = &mut main;
}

// ── LE checksum variant ──────────────────────────────────────────────────────

#[test]
fn test_wal_le_checksum_overlay_replaces_page() {
    let page_size: u32 = 512;
    let n_pages: u32 = 2;

    let main = build_minimal_sqlite_db(page_size, n_pages);

    let (mut wal, salt1, salt2, hdr_s0, hdr_s1) = build_wal_header(page_size, true); // LE

    let wal_page2 = vec![0x77u8; page_size as usize];
    let (frame2, _, _) =
        build_wal_frame(2, &wal_page2, n_pages, salt1, salt2, hdr_s0, hdr_s1, true);
    wal.extend_from_slice(&frame2);

    let reader = WalReader::from_bytes(&wal).expect("LE WAL should parse");
    assert_eq!(reader.committed_page_count(), 1);

    let merged = overlay_wal(&main, &wal).expect("overlay should succeed");
    let ps = page_size as usize;
    assert_eq!(&merged[0..ps], &main[0..ps]);
    assert_eq!(&merged[ps..ps * 2], wal_page2.as_slice());
}

// ── Multiple frames, latest wins ─────────────────────────────────────────────

#[test]
fn test_wal_later_frame_overrides_earlier_same_page() {
    let page_size: u32 = 512;
    let n_pages: u32 = 1;

    let main = build_minimal_sqlite_db(page_size, n_pages);

    let (mut wal, salt1, salt2, hdr_s0, hdr_s1) = build_wal_header(page_size, false);

    // First commit: page 1 → pattern 0x11.
    let first_data = vec![0x11u8; page_size as usize];
    let (frame_first, s0_a, s1_a) =
        build_wal_frame(1, &first_data, n_pages, salt1, salt2, hdr_s0, hdr_s1, false);
    wal.extend_from_slice(&frame_first);

    // Second commit: page 1 → pattern 0x22.
    let second_data = vec![0x22u8; page_size as usize];
    let (frame_second, _, _) =
        build_wal_frame(1, &second_data, n_pages, salt1, salt2, s0_a, s1_a, false);
    wal.extend_from_slice(&frame_second);

    let merged = overlay_wal(&main, &wal).expect("overlay should succeed");
    let ps = page_size as usize;
    assert_eq!(
        &merged[0..ps],
        second_data.as_slice(),
        "second WAL frame should win for the same page"
    );
}
