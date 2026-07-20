//! PMTiles v3 archive validation.
//!
//! Provides [`validate_archive`] for a full integrity scan that returns a
//! [`ValidationReport`] enumerating every issue found, and
//! [`validate_archive_strict`] which returns `Err` containing all issues when
//! any are present.
//!
//! # Validation passes
//!
//! 1. **Magic / parse** — verifies the 7-byte magic and parses the 127-byte
//!    fixed header.
//! 2. **Version** — spec version must be 3.
//! 3. **Section bounds** — each section (root dir, metadata, leaf dirs, tile
//!    data) must fit within the reported file size.
//! 4. **Root directory decode** — the root directory bytes must decode without
//!    error.
//! 5. **Leaf directory bounds & decode** — every leaf-pointer entry in the
//!    root directory must point within the leaf-dirs section, and each leaf
//!    directory must decode successfully.
//! 6. **Tile data bounds** — every tile entry (in root and leaves) must point
//!    to a byte range within the tile-data section.
//! 7. **Monotonic tile IDs** — within each leaf directory the tile IDs must
//!    be strictly non-decreasing.
//! 8. **Overlapping tile data ranges** — after collecting all tile entries,
//!    their data ranges must not overlap (excluding equal `offset+length`
//!    values that arise from valid deduplication).

use crate::directory::{DirectoryEntry, decode_directory};
use crate::header::{PMTILES_HEADER_SIZE, PMTILES_MAGIC, PmTilesHeader};

// ---------------------------------------------------------------------------
// ValidationIssue
// ---------------------------------------------------------------------------

/// A single integrity issue found during a validation pass over a PMTiles
/// archive.
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationIssue {
    /// The first 7 bytes do not match the PMTiles magic string, or the header
    /// could not be parsed at all.
    HeaderMagicMismatch,

    /// The spec-version byte is not 3.
    UnsupportedVersion(u8),

    /// The root-directory byte range `[offset, offset + length)` exceeds the
    /// reported file size.
    RootDirOutOfBounds {
        /// Byte offset of the root directory.
        offset: u64,
        /// Byte length of the root directory.
        length: u64,
        /// Total file size as reported by the data slice.
        file_size: u64,
    },

    /// A leaf-directory pointer entry's byte range `[offset, offset + length)`
    /// within the leaf-dirs section exceeds the leaf-dirs section length.
    LeafDirOutOfBounds {
        /// Tile ID of the leaf-pointer directory entry.
        tile_id: u64,
        /// Byte offset within the leaf-dirs section.
        offset: u64,
        /// Byte length of the leaf directory.
        length: u64,
        /// Total length of the leaf-dirs section.
        file_size: u64,
    },

    /// A tile entry's data range `[tile_data_offset + offset, + length)`
    /// exceeds the total file size.
    TileDataOutOfBounds {
        /// Tile ID of the offending directory entry.
        tile_id: u64,
        /// Byte offset within the tile-data section.
        offset: u64,
        /// Byte length of the tile.
        length: u64,
        /// Total file size as reported by the data slice.
        file_size: u64,
    },

    /// Two tile entries have data ranges that overlap (excluding the case where
    /// both `offset+length` values are equal, which is valid deduplication).
    OverlappingTiles {
        /// Tile ID of the first (earlier offset) entry.
        tile_id_a: u64,
        /// Tile ID of the second (later offset) entry.
        tile_id_b: u64,
    },

    /// Within a single leaf directory the tile IDs are not non-decreasing.
    NonMonotonicTileIds {
        /// Tile ID of the leaf-pointer directory entry whose decoded leaf
        /// contains non-monotonic IDs.
        leaf_dir_tile_id: u64,
    },

    /// The root or a leaf directory could not be decoded.
    DirectoryDecodeError(String),

    /// A varint in the directory was truncated before its terminal byte.
    VarintTruncated {
        /// Approximate file position at which the truncation was detected.
        at: u64,
    },
}

impl std::fmt::Display for ValidationIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HeaderMagicMismatch => {
                write!(f, "Header magic mismatch: not a valid PMTiles file")
            }
            Self::UnsupportedVersion(v) => {
                write!(
                    f,
                    "Unsupported PMTiles spec version: {v} (only v3 is supported)"
                )
            }
            Self::RootDirOutOfBounds {
                offset,
                length,
                file_size,
            } => write!(
                f,
                "Root directory [{offset}, {end}) is out of bounds (file size {file_size})",
                end = offset.saturating_add(*length),
            ),
            Self::LeafDirOutOfBounds {
                tile_id,
                offset,
                length,
                file_size,
            } => write!(
                f,
                "Leaf directory for tile_id={tile_id} [{offset}, {end}) exceeds \
                 leaf-dirs section length {file_size}",
                end = offset.saturating_add(*length),
            ),
            Self::TileDataOutOfBounds {
                tile_id,
                offset,
                length,
                file_size,
            } => write!(
                f,
                "Tile data for tile_id={tile_id} [{offset}, {end}) is out of bounds \
                 (file size {file_size})",
                end = offset.saturating_add(*length),
            ),
            Self::OverlappingTiles {
                tile_id_a,
                tile_id_b,
            } => write!(
                f,
                "Overlapping tile data ranges between tile_id={tile_id_a} and tile_id={tile_id_b}"
            ),
            Self::NonMonotonicTileIds { leaf_dir_tile_id } => write!(
                f,
                "Non-monotonic tile IDs in leaf directory pointed to by tile_id={leaf_dir_tile_id}"
            ),
            Self::DirectoryDecodeError(msg) => {
                write!(f, "Directory decode error: {msg}")
            }
            Self::VarintTruncated { at } => {
                write!(f, "Truncated varint at approximate byte position {at}")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ValidationReport
// ---------------------------------------------------------------------------

/// The result of a full validation pass over a PMTiles archive.
///
/// `passed` is `true` iff `issues` is empty.
#[derive(Debug, Clone)]
pub struct ValidationReport {
    /// `true` when no issues were found.
    pub passed: bool,
    /// All issues discovered during the validation pass.
    pub issues: Vec<ValidationIssue>,
}

impl ValidationReport {
    /// Construct a report for a fully valid archive (no issues).
    pub fn ok() -> Self {
        Self {
            passed: true,
            issues: vec![],
        }
    }

    /// Construct a report from a (potentially empty) list of issues.
    ///
    /// `passed` is set to `true` when `issues` is empty.
    pub fn with_issues(issues: Vec<ValidationIssue>) -> Self {
        Self {
            passed: issues.is_empty(),
            issues,
        }
    }
}

impl std::fmt::Display for ValidationReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.passed {
            write!(f, "PMTiles archive is valid")
        } else {
            write!(f, "PMTiles archive has {} issue(s):", self.issues.len())?;
            for issue in &self.issues {
                write!(f, "\n  - {issue}")?;
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// A compact representation of a single tile data range used for overlap
/// detection.
#[derive(Debug, Clone)]
struct TileRange {
    /// Absolute byte offset within the tile-data section (not file-absolute).
    offset: u64,
    /// Byte length of the tile.
    length: u64,
    /// Tile ID of the directory entry.
    tile_id: u64,
}

/// Check whether the data for `bytes` starts with the PMTiles magic bytes and
/// has at least [`PMTILES_HEADER_SIZE`] bytes.
fn has_valid_magic(bytes: &[u8]) -> bool {
    bytes.len() >= PMTILES_HEADER_SIZE && bytes.starts_with(PMTILES_MAGIC)
}

/// Verify that `offset + length <= limit`, pushing an issue if not.
///
/// Returns `true` when the range is in bounds.
fn check_section_bounds(
    offset: u64,
    length: u64,
    limit: u64,
    make_issue: impl FnOnce() -> ValidationIssue,
    issues: &mut Vec<ValidationIssue>,
) -> bool {
    let end = match offset.checked_add(length) {
        Some(e) => e,
        None => {
            // Arithmetic overflow — definitely out of bounds.
            issues.push(make_issue());
            return false;
        }
    };
    if end > limit {
        issues.push(make_issue());
        false
    } else {
        true
    }
}

/// Decode the root directory from its (uncompressed) raw bytes, pushing a
/// [`ValidationIssue::DirectoryDecodeError`] on failure.
///
/// Returns `Some(entries)` on success, `None` on failure.
fn try_decode_directory(
    bytes: &[u8],
    issues: &mut Vec<ValidationIssue>,
) -> Option<Vec<DirectoryEntry>> {
    match decode_directory(bytes) {
        Ok(entries) => Some(entries),
        Err(e) => {
            issues.push(ValidationIssue::DirectoryDecodeError(e.to_string()));
            None
        }
    }
}

/// Check that tile IDs within a single slice of entries are non-decreasing.
///
/// Returns `true` when monotonic, `false` when a violation is found (an issue
/// is pushed).
fn check_monotonic_ids(
    entries: &[DirectoryEntry],
    leaf_ptr_tile_id: u64,
    issues: &mut Vec<ValidationIssue>,
) -> bool {
    let mut last: u64 = 0;
    let mut first = true;
    for entry in entries {
        if !first && entry.tile_id < last {
            issues.push(ValidationIssue::NonMonotonicTileIds {
                leaf_dir_tile_id: leaf_ptr_tile_id,
            });
            return false;
        }
        last = entry.tile_id;
        first = false;
    }
    true
}

/// Validate the tile data ranges collected from all tile entries for overlaps.
///
/// Ranges with `offset_a + length_a == offset_b` are valid (adjacent), and
/// ranges sharing the exact same `(offset, length)` pair are valid
/// (deduplicated identical tile content).  Only cases where one range partially
/// intrudes into another are flagged.
fn check_overlapping_ranges(ranges: &mut [TileRange], issues: &mut Vec<ValidationIssue>) {
    if ranges.len() < 2 {
        return;
    }

    // Sort by (offset, length) so equal-range pairs are adjacent and we can
    // detect them easily.
    ranges.sort_unstable_by(|a, b| a.offset.cmp(&b.offset).then(a.length.cmp(&b.length)));

    let mut i = 0usize;
    while i < ranges.len() {
        let current = &ranges[i];
        let current_end = match current.offset.checked_add(current.length) {
            Some(e) => e,
            None => {
                // Arithmetic overflow — can't sensibly check further.
                i += 1;
                continue;
            }
        };

        let j = i + 1;
        if j >= ranges.len() {
            break;
        }
        let next = &ranges[j];

        // Skip over ranges that share the exact same (offset, length) — these
        // are deduplicated tile data (valid by spec).
        if next.offset == current.offset && next.length == current.length {
            i += 1;
            continue;
        }

        // A real overlap occurs when the next range starts before the current
        // one ends.
        if next.offset < current_end {
            issues.push(ValidationIssue::OverlappingTiles {
                tile_id_a: current.tile_id,
                tile_id_b: next.tile_id,
            });
        }

        i += 1;
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Perform a non-strict validation pass over a PMTiles v3 archive.
///
/// Every issue found is recorded in the returned [`ValidationReport`]; the
/// function does not short-circuit on the first problem (except when the header
/// is completely unreadable, since later checks depend on the parsed header).
///
/// # Short-circuit behaviour
/// When the magic bytes are wrong or the header cannot be parsed, a single
/// [`ValidationIssue::HeaderMagicMismatch`] is returned immediately because
/// all subsequent checks depend on the parsed header fields.
///
/// # Compression
/// Directories are expected to be uncompressed (i.e. `internal_compression ==
/// None`).  Archives with compressed directories will have their directory data
/// decoded as-is; if that fails, a [`ValidationIssue::DirectoryDecodeError`]
/// is emitted.  For proper compressed-directory validation, decompress first
/// before calling this function.
pub fn validate_archive(bytes: &[u8]) -> ValidationReport {
    let mut issues: Vec<ValidationIssue> = Vec::new();
    let file_size = bytes.len() as u64;

    // ── Pass 1: magic + header parse ─────────────────────────────────────────
    if !has_valid_magic(bytes) {
        return ValidationReport::with_issues(vec![ValidationIssue::HeaderMagicMismatch]);
    }

    // At this point we know the magic is correct; attempt a full header parse.
    // Header::parse also checks the version, so capture that separately below.
    let header = match PmTilesHeader::parse(bytes) {
        Ok(h) => h,
        Err(e) => {
            // Determine whether this is a version error or a format error.
            let issue = if bytes.len() >= 8 && bytes[7] != 3 {
                ValidationIssue::UnsupportedVersion(bytes[7])
            } else {
                // Treat parse errors as a magic / format mismatch for simplicity.
                ValidationIssue::DirectoryDecodeError(e.to_string())
            };
            return ValidationReport::with_issues(vec![issue]);
        }
    };

    // ── Pass 2: version ───────────────────────────────────────────────────────
    if header.spec_version != 3 {
        issues.push(ValidationIssue::UnsupportedVersion(header.spec_version));
        // Continue — we can still check section bounds.
    }

    // ── Pass 3: section bounds ────────────────────────────────────────────────
    // Root directory.
    let root_in_bounds = check_section_bounds(
        header.root_dir_offset,
        header.root_dir_length,
        file_size,
        || ValidationIssue::RootDirOutOfBounds {
            offset: header.root_dir_offset,
            length: header.root_dir_length,
            file_size,
        },
        &mut issues,
    );

    // Metadata.
    check_section_bounds(
        header.metadata_offset,
        header.metadata_length,
        file_size,
        || {
            // Metadata doesn't have a dedicated variant; reuse TileDataOutOfBounds
            // with tile_id=u64::MAX as a sentinel isn't ideal, so we emit a
            // DirectoryDecodeError with a descriptive message instead.
            ValidationIssue::DirectoryDecodeError(format!(
                "Metadata section [{}, {}) is out of bounds (file size {file_size})",
                header.metadata_offset,
                header
                    .metadata_offset
                    .saturating_add(header.metadata_length),
            ))
        },
        &mut issues,
    );

    // Leaf dirs section.
    let leaf_section_in_bounds = check_section_bounds(
        header.leaf_dirs_offset,
        header.leaf_dirs_length,
        file_size,
        || {
            ValidationIssue::DirectoryDecodeError(format!(
                "Leaf-dirs section [{}, {}) is out of bounds (file size {file_size})",
                header.leaf_dirs_offset,
                header
                    .leaf_dirs_offset
                    .saturating_add(header.leaf_dirs_length),
            ))
        },
        &mut issues,
    );

    // Tile data section.
    check_section_bounds(
        header.tile_data_offset,
        header.tile_data_length,
        file_size,
        || ValidationIssue::TileDataOutOfBounds {
            // No specific tile_id for the section-level check.
            tile_id: 0,
            offset: header.tile_data_offset,
            length: header.tile_data_length,
            file_size,
        },
        &mut issues,
    );

    // If the root directory section itself is out of bounds, we cannot decode
    // the directory at all, so stop here.
    if !root_in_bounds {
        return ValidationReport::with_issues(issues);
    }

    // ── Pass 4: root directory decode ─────────────────────────────────────────
    let root_start = header.root_dir_offset as usize;
    let root_end = root_start + header.root_dir_length as usize;
    let root_bytes = &bytes[root_start..root_end];

    let root_entries = match try_decode_directory(root_bytes, &mut issues) {
        Some(e) => e,
        None => {
            // Cannot proceed with tile/leaf checks.
            return ValidationReport::with_issues(issues);
        }
    };

    // Accumulate all tile ranges for overlap detection.
    let mut all_tile_ranges: Vec<TileRange> = Vec::new();

    // ── Pass 5: leaf directory bounds, decode, and tile bounds ────────────────
    for root_entry in &root_entries {
        if root_entry.is_leaf_directory() {
            // Validate the leaf pointer's byte range within the leaf-dirs section.
            let leaf_offset = root_entry.offset;
            let leaf_length = u64::from(root_entry.length);

            let leaf_range_ok = if leaf_section_in_bounds {
                check_section_bounds(
                    leaf_offset,
                    leaf_length,
                    header.leaf_dirs_length,
                    || ValidationIssue::LeafDirOutOfBounds {
                        tile_id: root_entry.tile_id,
                        offset: leaf_offset,
                        length: leaf_length,
                        file_size: header.leaf_dirs_length,
                    },
                    &mut issues,
                )
            } else {
                // Leaf section itself is out of bounds — skip individual leaf checks.
                false
            };

            if !leaf_range_ok {
                continue;
            }

            // Decode the leaf directory.
            let abs_leaf_start = (header.leaf_dirs_offset + leaf_offset) as usize;
            let abs_leaf_end = abs_leaf_start + root_entry.length as usize;

            // Guard against the file being smaller than we expect (should have
            // been caught above, but be defensive).
            if abs_leaf_end > bytes.len() {
                issues.push(ValidationIssue::LeafDirOutOfBounds {
                    tile_id: root_entry.tile_id,
                    offset: leaf_offset,
                    length: leaf_length,
                    file_size: header.leaf_dirs_length,
                });
                continue;
            }

            let leaf_bytes = &bytes[abs_leaf_start..abs_leaf_end];
            let leaf_entries = match try_decode_directory(leaf_bytes, &mut issues) {
                Some(e) => e,
                None => continue,
            };

            // Check monotonicity of tile IDs within this leaf.
            check_monotonic_ids(&leaf_entries, root_entry.tile_id, &mut issues);

            // Validate each tile entry inside the leaf.
            for leaf_entry in &leaf_entries {
                if !leaf_entry.is_tile() {
                    // Nested leaf pointers are not supported in PMTiles v3;
                    // just skip without flagging here (the reader will error
                    // on access).
                    continue;
                }

                let tile_abs_offset = header.tile_data_offset.saturating_add(leaf_entry.offset);
                let tile_length = u64::from(leaf_entry.length);

                let tile_in_bounds = check_section_bounds(
                    tile_abs_offset,
                    tile_length,
                    file_size,
                    || ValidationIssue::TileDataOutOfBounds {
                        tile_id: leaf_entry.tile_id,
                        offset: tile_abs_offset,
                        length: tile_length,
                        file_size,
                    },
                    &mut issues,
                );

                if tile_in_bounds {
                    all_tile_ranges.push(TileRange {
                        offset: leaf_entry.offset,
                        length: tile_length,
                        tile_id: leaf_entry.tile_id,
                    });
                }
            }
        } else {
            // Tile entry in root directory — validate data range.
            let tile_abs_offset = header.tile_data_offset.saturating_add(root_entry.offset);
            let tile_length = u64::from(root_entry.length);

            let tile_in_bounds = check_section_bounds(
                tile_abs_offset,
                tile_length,
                file_size,
                || ValidationIssue::TileDataOutOfBounds {
                    tile_id: root_entry.tile_id,
                    offset: tile_abs_offset,
                    length: tile_length,
                    file_size,
                },
                &mut issues,
            );

            if tile_in_bounds {
                all_tile_ranges.push(TileRange {
                    offset: root_entry.offset,
                    length: tile_length,
                    tile_id: root_entry.tile_id,
                });
            }
        }
    }

    // ── Pass 6: monotonicity in root (when no leaves) ─────────────────────────
    // For archives without leaf directories, also verify root tile-ID order.
    if header.leaf_dirs_length == 0 {
        // We reuse 0 as the "root" leaf_dir_tile_id sentinel.
        check_monotonic_ids(&root_entries, 0, &mut issues);
    }

    // ── Pass 7: overlapping tile data ranges ──────────────────────────────────
    check_overlapping_ranges(&mut all_tile_ranges, &mut issues);

    ValidationReport::with_issues(issues)
}

/// Strict validation: returns `Ok(())` when the archive is valid, or
/// `Err(issues)` containing every discovered issue when it is not.
///
/// Internally delegates to [`validate_archive`].
pub fn validate_archive_strict(bytes: &[u8]) -> Result<(), Vec<ValidationIssue>> {
    let report = validate_archive(bytes);
    if report.passed {
        Ok(())
    } else {
        Err(report.issues)
    }
}

// ---------------------------------------------------------------------------
// Unit tests (inline)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_report_ok_constructor() {
        let r = ValidationReport::ok();
        assert!(r.passed);
        assert!(r.issues.is_empty());
    }

    #[test]
    fn test_validate_report_with_empty_issues_passes() {
        let r = ValidationReport::with_issues(vec![]);
        assert!(r.passed);
    }

    #[test]
    fn test_validate_report_with_issue_fails() {
        let r = ValidationReport::with_issues(vec![ValidationIssue::HeaderMagicMismatch]);
        assert!(!r.passed);
        assert_eq!(r.issues.len(), 1);
    }

    #[test]
    fn test_issue_display_header_magic() {
        let s = format!("{}", ValidationIssue::HeaderMagicMismatch);
        assert!(s.contains("magic") || s.contains("PMTiles"));
    }

    #[test]
    fn test_issue_display_unsupported_version() {
        let s = format!("{}", ValidationIssue::UnsupportedVersion(2));
        assert!(s.contains('2'));
    }

    #[test]
    fn test_check_section_bounds_ok() {
        let mut issues = Vec::new();
        let in_bounds = check_section_bounds(
            0,
            100,
            200,
            || ValidationIssue::HeaderMagicMismatch,
            &mut issues,
        );
        assert!(in_bounds);
        assert!(issues.is_empty());
    }

    #[test]
    fn test_check_section_bounds_exact_edge() {
        let mut issues = Vec::new();
        let in_bounds = check_section_bounds(
            100,
            100,
            200,
            || ValidationIssue::HeaderMagicMismatch,
            &mut issues,
        );
        assert!(in_bounds);
        assert!(issues.is_empty());
    }

    #[test]
    fn test_check_section_bounds_out_of_bounds() {
        let mut issues = Vec::new();
        let in_bounds = check_section_bounds(
            150,
            100,
            200,
            || ValidationIssue::HeaderMagicMismatch,
            &mut issues,
        );
        assert!(!in_bounds);
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn test_check_monotonic_ids_ok() {
        let entries = vec![
            DirectoryEntry {
                tile_id: 1,
                offset: 0,
                length: 10,
                run_length: 1,
            },
            DirectoryEntry {
                tile_id: 5,
                offset: 10,
                length: 10,
                run_length: 1,
            },
            DirectoryEntry {
                tile_id: 5,
                offset: 20,
                length: 10,
                run_length: 1,
            }, // equal is ok
            DirectoryEntry {
                tile_id: 9,
                offset: 30,
                length: 10,
                run_length: 1,
            },
        ];
        let mut issues = Vec::new();
        let ok = check_monotonic_ids(&entries, 0, &mut issues);
        assert!(ok);
        assert!(issues.is_empty());
    }

    #[test]
    fn test_check_monotonic_ids_violation() {
        let entries = vec![
            DirectoryEntry {
                tile_id: 5,
                offset: 0,
                length: 10,
                run_length: 1,
            },
            DirectoryEntry {
                tile_id: 3,
                offset: 10,
                length: 10,
                run_length: 1,
            }, // 3 < 5 → violation
        ];
        let mut issues = Vec::new();
        let ok = check_monotonic_ids(&entries, 42, &mut issues);
        assert!(!ok);
        assert_eq!(issues.len(), 1);
        assert!(matches!(
            &issues[0],
            ValidationIssue::NonMonotonicTileIds {
                leaf_dir_tile_id: 42
            }
        ));
    }

    #[test]
    fn test_check_overlapping_ranges_no_overlap() {
        let mut ranges = vec![
            TileRange {
                offset: 0,
                length: 10,
                tile_id: 1,
            },
            TileRange {
                offset: 10,
                length: 20,
                tile_id: 2,
            },
            TileRange {
                offset: 30,
                length: 5,
                tile_id: 3,
            },
        ];
        let mut issues = Vec::new();
        check_overlapping_ranges(&mut ranges, &mut issues);
        assert!(issues.is_empty());
    }

    #[test]
    fn test_check_overlapping_ranges_with_overlap() {
        let mut ranges = vec![
            TileRange {
                offset: 0,
                length: 15,
                tile_id: 1,
            },
            TileRange {
                offset: 10,
                length: 10,
                tile_id: 2,
            }, // starts before offset 15
        ];
        let mut issues = Vec::new();
        check_overlapping_ranges(&mut ranges, &mut issues);
        assert_eq!(issues.len(), 1);
        assert!(matches!(
            &issues[0],
            ValidationIssue::OverlappingTiles { .. }
        ));
    }

    #[test]
    fn test_check_overlapping_ranges_dedup_same_range() {
        // Two entries pointing to the exact same range — deduplication, not an overlap.
        let mut ranges = vec![
            TileRange {
                offset: 0,
                length: 10,
                tile_id: 1,
            },
            TileRange {
                offset: 0,
                length: 10,
                tile_id: 2,
            },
        ];
        let mut issues = Vec::new();
        check_overlapping_ranges(&mut ranges, &mut issues);
        assert!(
            issues.is_empty(),
            "identical ranges (dedup) should not be flagged"
        );
    }
}
