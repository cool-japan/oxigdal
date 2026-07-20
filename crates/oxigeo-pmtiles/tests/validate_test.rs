//! Integration tests for PMTiles v3 archive validation.
//!
//! Tests are numbered 1–7 per the specification in the implementation task.
//! Each test either exercises a specific error path (tests 1–4, 6–7) or
//! verifies a structurally valid archive (test 5).

#![allow(clippy::expect_used)]

use oxigeo_pmtiles::{
    PmTilesBuilder, TileType, ValidationIssue, validate_archive, validate_archive_strict,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Build a correctly formed 127-byte PMTiles v3 header.
///
/// All section lengths and offsets default to zero (empty sections); callers
/// can overwrite specific bytes after calling this helper.
fn make_valid_header() -> Vec<u8> {
    let mut buf = vec![0u8; 127];
    buf[0..7].copy_from_slice(b"PMTiles");
    buf[7] = 3; // spec_version
    // root_dir_offset = 127 (immediately after the header), root_dir_length = 0
    buf[8..16].copy_from_slice(&127u64.to_le_bytes());
    buf[16..24].copy_from_slice(&0u64.to_le_bytes());
    // metadata_offset = 127, metadata_length = 0
    buf[24..32].copy_from_slice(&127u64.to_le_bytes());
    buf[32..40].copy_from_slice(&0u64.to_le_bytes());
    // leaf_dirs_offset = 127, leaf_dirs_length = 0
    buf[40..48].copy_from_slice(&127u64.to_le_bytes());
    buf[48..56].copy_from_slice(&0u64.to_le_bytes());
    // tile_data_offset = 127, tile_data_length = 0
    buf[56..64].copy_from_slice(&127u64.to_le_bytes());
    buf[64..72].copy_from_slice(&0u64.to_le_bytes());
    // clustered = 0; internal_compression = 1 (None); tile_compression = 1 (None)
    buf[96] = 0;
    buf[97] = 1;
    buf[98] = 1;
    buf[99] = 2; // TileType::Png
    buf
}

/// Write a u64 little-endian value at `offset` inside `buf`.
fn write_u64_le(buf: &mut [u8], offset: usize, value: u64) {
    buf[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

/// Build a complete minimal PMTiles v3 archive with no tiles using the
/// official PmTilesBuilder so that the byte layout is guaranteed valid.
fn build_empty_archive() -> Vec<u8> {
    let builder = PmTilesBuilder::new(TileType::Png, 0, 0);
    builder.build().expect("build empty archive")
}

/// Build a complete PMTiles v3 archive with a given set of tiles.
fn build_archive(tiles: &[(u8, u32, u32, &[u8])]) -> Vec<u8> {
    let min_z = tiles.iter().map(|t| t.0).min().unwrap_or(0);
    let max_z = tiles.iter().map(|t| t.0).max().unwrap_or(0);
    let mut builder = PmTilesBuilder::new(TileType::Png, min_z, max_z);
    for &(z, x, y, data) in tiles {
        builder.add_tile(z, x, y, data).expect("add_tile");
    }
    builder.build().expect("build archive")
}

// ── Test 1: bad magic ─────────────────────────────────────────────────────────

/// A zero-filled 127-byte buffer has no valid PMTiles magic, so validation
/// must report exactly `HeaderMagicMismatch`.
#[test]
fn test_validate_bad_magic_reports_issue() {
    let data = [0u8; 127];
    let report = validate_archive(&data);
    assert!(!report.passed, "zeroed buffer should fail validation");
    assert!(
        report
            .issues
            .contains(&ValidationIssue::HeaderMagicMismatch),
        "expected HeaderMagicMismatch, got {:?}",
        report.issues
    );
}

// ── Test 2: correct magic, wrong version ──────────────────────────────────────

/// A header with the correct 7-byte magic but version byte = 2 must report
/// `UnsupportedVersion(2)`.
#[test]
fn test_validate_unsupported_version_reports_issue() {
    let mut data = make_valid_header();
    // Overwrite version byte from 3 to 2.
    data[7] = 2;

    let report = validate_archive(&data);
    assert!(!report.passed, "version 2 should fail validation");
    assert!(
        report
            .issues
            .contains(&ValidationIssue::UnsupportedVersion(2)),
        "expected UnsupportedVersion(2), got {:?}",
        report.issues
    );
}

// ── Test 3: truncated root directory ─────────────────────────────────────────

/// If the header claims `root_dir_length = 1000` but the file is only 200
/// bytes total, `RootDirOutOfBounds` must be reported.
#[test]
fn test_validate_truncated_root_dir_reports_out_of_bounds() {
    let mut data = make_valid_header();
    // Extend to 200 bytes total (header is 127 bytes).
    data.resize(200, 0);

    // root_dir_offset = 127, root_dir_length = 1000 → 127 + 1000 = 1127 > 200
    write_u64_le(&mut data, 8, 127);
    write_u64_le(&mut data, 16, 1000);

    let report = validate_archive(&data);
    assert!(!report.passed, "out-of-bounds root dir should fail");
    let has_issue = report.issues.iter().any(|i| {
        matches!(
            i,
            ValidationIssue::RootDirOutOfBounds {
                offset: 127,
                length: 1000,
                ..
            }
        )
    });
    assert!(
        has_issue,
        "expected RootDirOutOfBounds(127, 1000), got {:?}",
        report.issues
    );
}

// ── Test 4: tile data section out of bounds ───────────────────────────────────

/// If the header's tile-data section `(offset + length)` exceeds the file
/// size, `TileDataOutOfBounds` must be reported.
#[test]
fn test_validate_truncated_tile_data_reports_out_of_bounds() {
    let mut data = make_valid_header();
    // Extend to 300 bytes total.
    data.resize(300, 0);

    // tile_data_offset = 127, tile_data_length = 50_000 → clearly out of bounds
    write_u64_le(&mut data, 56, 127);
    write_u64_le(&mut data, 64, 50_000);

    let report = validate_archive(&data);
    assert!(!report.passed, "out-of-bounds tile data should fail");
    let has_issue = report
        .issues
        .iter()
        .any(|i| matches!(i, ValidationIssue::TileDataOutOfBounds { .. }));
    assert!(
        has_issue,
        "expected TileDataOutOfBounds, got {:?}",
        report.issues
    );
}

// ── Test 5: well-formed empty archive passes ──────────────────────────────────

/// A PMTiles archive produced by `PmTilesBuilder` with no tiles must pass
/// validation cleanly.
#[test]
fn test_validate_well_formed_empty_archive_passes() {
    let archive = build_empty_archive();
    let report = validate_archive(&archive);
    assert!(
        report.passed,
        "empty archive produced by builder should be valid, issues: {:?}",
        report.issues
    );
}

/// `validate_archive_strict` on a valid archive must return `Ok(())`.
#[test]
fn test_validate_strict_ok_on_valid_archive() {
    let archive = build_empty_archive();
    assert!(
        validate_archive_strict(&archive).is_ok(),
        "strict validation of valid archive should return Ok"
    );
}

/// `validate_archive_strict` on an invalid archive must return `Err(issues)`.
#[test]
fn test_validate_strict_err_on_invalid_archive() {
    let data = [0u8; 127];
    let result = validate_archive_strict(&data);
    assert!(
        result.is_err(),
        "strict validation of zeroed buffer must return Err"
    );
    let issues = result.expect_err("should be Err");
    assert!(
        issues.contains(&ValidationIssue::HeaderMagicMismatch),
        "expected HeaderMagicMismatch in strict error, got {:?}",
        issues
    );
}

/// A multi-tile archive produced by the builder also passes validation.
#[test]
fn test_validate_well_formed_multi_tile_archive_passes() {
    // Use arithmetic sequences for tile data to avoid the rand crate.
    let tile_a: Vec<u8> = (0u8..64).collect();
    let tile_b: Vec<u8> = (64u8..128).collect();
    let tile_c: Vec<u8> = (0u8..32).map(|i| i * 2).collect();

    let archive = build_archive(&[(0, 0, 0, &tile_a), (1, 0, 0, &tile_b), (1, 1, 0, &tile_c)]);
    let report = validate_archive(&archive);
    assert!(
        report.passed,
        "multi-tile archive should be valid, issues: {:?}",
        report.issues
    );
}

// ── Test 6: overlapping tile offsets ─────────────────────────────────────────

/// Construct a header + directory + tile-data blob in which two tile entries
/// claim overlapping byte ranges.  The validator must report `OverlappingTiles`.
///
/// # Layout
/// ```text
/// [127-byte header]
/// [root directory bytes]
/// [tile data: 50 bytes total]
///   entry A: offset=0,  length=30  → bytes [0..30)
///   entry B: offset=20, length=10  → bytes [20..30) — overlaps A by 10 bytes
/// ```
#[test]
fn test_validate_overlapping_tile_offsets_detected() {
    use oxigeo_pmtiles::varint::encode_varint;

    // Build the root directory with two tile entries whose data ranges overlap.
    //
    // PMTiles v3 directory wire format (column-oriented):
    //   1. num_entries (varint)
    //   2. tile_id deltas (varints, cumulative-sum encoded from 0)
    //   3. run_lengths (varints)
    //   4. lengths (varints)
    //   5. offsets (varints, `absolute_offset + 1` encoding;
    //               0 reserved for "immediately follows previous" shorthand)
    //
    // Entry A: tile_id=0, run_length=1, length=30, offset=0  → encoded as 0+1=1
    // Entry B: tile_id=1, run_length=1, length=10, offset=20 → encoded as 20+1=21
    let mut dir: Vec<u8> = Vec::new();

    // num_entries = 2
    dir.extend_from_slice(&encode_varint(2));

    // Tile ID column (delta-encoded from 0):
    //   tile_id[0] = delta[0] = 0
    //   tile_id[1] = tile_id[0] + delta[1] = 0 + 1 = 1
    dir.extend_from_slice(&encode_varint(0)); // tile_id 0
    dir.extend_from_slice(&encode_varint(1)); // tile_id 1

    // Run-length column: both entries are tile entries (run_length=1)
    dir.extend_from_slice(&encode_varint(1));
    dir.extend_from_slice(&encode_varint(1));

    // Length column: 30 bytes and 10 bytes
    dir.extend_from_slice(&encode_varint(30));
    dir.extend_from_slice(&encode_varint(10));

    // Offset column (absolute_offset + 1 encoding):
    //   Entry A: offset=0  → 0+1 = 1
    //   Entry B: offset=20 → 20+1 = 21  (does NOT use the clustered-shorthand
    //                                     because 0 + 30 ≠ 20)
    dir.extend_from_slice(&encode_varint(1)); // offset 0
    dir.extend_from_slice(&encode_varint(21)); // offset 20  — overlaps [0..30)

    // Tile data blob: 50 bytes using an arithmetic sequence.
    let tile_data: Vec<u8> = (0u8..50).collect();

    // Assemble layout: [header][root dir][tile data]
    // (no metadata, no leaf dirs)
    let root_dir_offset: u64 = 127;
    let root_dir_length: u64 = dir.len() as u64;
    let leaf_dirs_offset: u64 = root_dir_offset + root_dir_length;
    let tile_data_offset: u64 = leaf_dirs_offset; // leaf_dirs_length = 0
    let tile_data_length: u64 = tile_data.len() as u64;

    let mut header = vec![0u8; 127];
    header[0..7].copy_from_slice(b"PMTiles");
    header[7] = 3;
    header[8..16].copy_from_slice(&root_dir_offset.to_le_bytes());
    header[16..24].copy_from_slice(&root_dir_length.to_le_bytes());
    // metadata_offset = root_dir_offset + root_dir_length, metadata_length = 0
    header[24..32].copy_from_slice(&leaf_dirs_offset.to_le_bytes());
    header[32..40].copy_from_slice(&0u64.to_le_bytes());
    // leaf_dirs: offset = tile_data_offset, length = 0
    header[40..48].copy_from_slice(&tile_data_offset.to_le_bytes());
    header[48..56].copy_from_slice(&0u64.to_le_bytes());
    // tile_data section
    header[56..64].copy_from_slice(&tile_data_offset.to_le_bytes());
    header[64..72].copy_from_slice(&tile_data_length.to_le_bytes());
    header[97] = 1; // internal_compression = None
    header[98] = 1; // tile_compression = None
    header[99] = 2; // TileType::Png

    let mut archive: Vec<u8> = Vec::new();
    archive.extend_from_slice(&header);
    archive.extend_from_slice(&dir);
    archive.extend_from_slice(&tile_data);

    let report = validate_archive(&archive);
    assert!(
        !report.passed,
        "overlapping tile offsets should fail validation"
    );
    let has_overlap = report
        .issues
        .iter()
        .any(|i| matches!(i, ValidationIssue::OverlappingTiles { .. }));
    assert!(
        has_overlap,
        "expected OverlappingTiles issue, got {:?}",
        report.issues
    );
}

// ── Test 7: corrupt leaf directory detected ───────────────────────────────────

/// Construct an archive where the root directory has a leaf pointer entry, but
/// the leaf directory bytes are garbage (all 0x80, i.e. truncated varints).
///
/// This exercises the leaf-directory decode path and ensures the validator
/// reports `DirectoryDecodeError` (or `LeafDirOutOfBounds`) for corrupt leaf
/// contents.
///
/// Additionally verify that a genuine leaf-based archive produced by
/// `PmTilesBuilder` (which forces leaf directories via a large tile count)
/// passes validation cleanly — confirming the positive path through the
/// leaf validation code.
///
/// # Note on `NonMonotonicTileIds` path
/// PMTiles v3 directories are delta-encoded with unsigned varints and
/// `saturating_add`, which means the decoded tile IDs always form a
/// non-decreasing sequence when using the standard encoder.  The
/// `NonMonotonicTileIds` invariant is therefore verified in the unit tests
/// of `validate.rs`, where `check_monotonic_ids` can be called directly
/// with hand-constructed `DirectoryEntry` slices containing decreasing IDs.
/// The integration test focuses on the positive (valid) and negative (corrupt)
/// leaf-directory paths that are reachable through the public API.
#[test]
fn test_validate_non_monotonic_tile_ids_in_leaf_detected() {
    use oxigeo_pmtiles::varint::encode_varint;

    // ── Part A: builder-produced leaf-based archive passes ────────────────────
    //
    // Build enough tiles to exceed LEAF_SPLIT_THRESHOLD (16 384 bytes) and
    // force the builder to emit leaf directories.  Tile data is an arithmetic
    // sequence to avoid the rand crate.
    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 7);
    let mut count = 0u32;
    'outer: for z in 0u8..=7 {
        let max_coord = 1u32 << z;
        for x in 0..max_coord {
            for y in 0..max_coord {
                // Arithmetic byte pattern that varies by position.
                let byte_val = ((x * 31).wrapping_add(y * 17).wrapping_add(u32::from(z))) as u8;
                let tile_data = vec![byte_val; 32];
                builder.add_tile(z, x, y, &tile_data).expect("add_tile");
                count += 1;
                if count >= 2000 {
                    break 'outer;
                }
            }
        }
    }
    let leaf_archive = builder.build().expect("build leaf archive");

    let report_a = validate_archive(&leaf_archive);
    assert!(
        report_a.passed,
        "builder-produced leaf-based archive ({count} tiles) should be valid, \
         issues: {:?}",
        report_a.issues
    );

    // ── Part B: archive with a corrupt leaf directory fails ───────────────────
    //
    // Layout:
    //   [127-byte header]
    //   [root directory: 1 leaf-pointer entry]
    //   [leaf-dirs section: 20 bytes of 0x80 (truncated-varint garbage)]
    //   [tile data: 0 bytes]

    // Root directory: 1 entry, leaf pointer (run_length = 0).
    //   tile_id = 0 (delta = 0)
    //   run_length = 0  → leaf pointer
    //   length = 20     → size of the leaf dir below
    //   offset = 0      → start of leaf-dirs section (encoded as 0+1 = 1)
    let mut root_dir: Vec<u8> = Vec::new();
    root_dir.extend_from_slice(&encode_varint(1)); // num_entries = 1
    root_dir.extend_from_slice(&encode_varint(0)); // tile_id delta = 0
    root_dir.extend_from_slice(&encode_varint(0)); // run_length = 0 (leaf pointer)
    root_dir.extend_from_slice(&encode_varint(20)); // length = 20
    root_dir.extend_from_slice(&encode_varint(1)); // offset 0 → encoded as 1

    let root_dir_offset: u64 = 127;
    let root_dir_length: u64 = root_dir.len() as u64;
    let leaf_dirs_offset: u64 = root_dir_offset + root_dir_length;
    let leaf_dirs_length: u64 = 20;
    let tile_data_offset: u64 = leaf_dirs_offset + leaf_dirs_length;
    let tile_data_length: u64 = 0;
    let total_size: usize = tile_data_offset as usize;

    let mut header = vec![0u8; 127];
    header[0..7].copy_from_slice(b"PMTiles");
    header[7] = 3;
    header[8..16].copy_from_slice(&root_dir_offset.to_le_bytes());
    header[16..24].copy_from_slice(&root_dir_length.to_le_bytes());
    // metadata: offset = leaf_dirs_offset, length = 0
    header[24..32].copy_from_slice(&leaf_dirs_offset.to_le_bytes());
    header[32..40].copy_from_slice(&0u64.to_le_bytes());
    // leaf dirs
    header[40..48].copy_from_slice(&leaf_dirs_offset.to_le_bytes());
    header[48..56].copy_from_slice(&leaf_dirs_length.to_le_bytes());
    // tile data
    header[56..64].copy_from_slice(&tile_data_offset.to_le_bytes());
    header[64..72].copy_from_slice(&tile_data_length.to_le_bytes());
    header[97] = 1; // internal_compression = None
    header[98] = 1; // tile_compression = None
    header[99] = 2; // TileType::Png

    let mut archive: Vec<u8> = Vec::with_capacity(total_size);
    archive.extend_from_slice(&header);
    archive.extend_from_slice(&root_dir);
    // Garbage leaf directory: 20 bytes, all 0x80 (continuation bit always set
    // → the varint decoder will see a truncated varint and return an error).
    archive.extend(std::iter::repeat_n(0x80u8, 20));
    // No tile data bytes needed (tile_data_length = 0).

    let report_b = validate_archive(&archive);
    assert!(
        !report_b.passed,
        "archive with corrupt leaf directory should fail validation"
    );
    let has_leaf_err = report_b.issues.iter().any(|i| {
        matches!(
            i,
            ValidationIssue::DirectoryDecodeError(_) | ValidationIssue::LeafDirOutOfBounds { .. }
        )
    });
    assert!(
        has_leaf_err,
        "expected DirectoryDecodeError or LeafDirOutOfBounds for corrupt leaf, \
         got {:?}",
        report_b.issues
    );
}
