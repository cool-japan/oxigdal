//! Cached tile/strip location index for one image level.
//!
//! # Why this exists (cool-japan/oxigeo#14)
//!
//! [`CogReader::tile_byte_range`](super::CogReader::tile_byte_range) used to
//! re-read *and* re-parse the whole `TileOffsets`/`TileByteCounts` (or
//! `StripOffsets`/`StripByteCounts`) array on **every single tile lookup**:
//! `IfdEntry::get_u64_vec` → `get_value_bytes` → `DataSource::read_range`, i.e.
//! two real seek+read syscalls plus two full `Vec<u64>` parses per block. Reading
//! a whole band walks every block, so the cost of a band read grew as O(n²) in the
//! number of blocks — measured at 190 ms of a 248 ms band read for a file with
//! 8000 strips (77 %), versus 0.93 ms for 500 blocks and 2.6 ms for 1024.
//!
//! [`BlockIndex`] parses each level's two arrays exactly once (at
//! [`CogReader::open`](super::CogReader::open)) and turns the lookup into an O(1)
//! index. Memory is one `u64` per block per array — 128 KiB for the 8000-strip
//! file above, and bounded by [`MAX_BLOCK_INDEX_ENTRIES`] for hostile headers.

use oxigeo_core::error::{FormatError, OxiGeoError, Result};
use oxigeo_core::io::{ByteRange, DataSource};

use crate::tiff::{ByteOrderType, Ifd, IfdEntry, TiffTag, TiffVariant};

/// Largest block count this index will pre-read.
///
/// The declared element count of an IFD entry is untrusted: a malformed or
/// hostile header can claim billions of entries, and pre-reading it would attempt
/// a multi-gigabyte allocation inside the data source. 64 Mi blocks is far past
/// any real raster (a 1 000 000 x 1 000 000 image tiled 256x256 has ~15 Mi tiles)
/// while bounding the pre-read to 512 MiB per array. Anything larger simply is
/// not cached: the lookup then falls back to the original on-demand path, whose
/// behaviour (including its errors) is unchanged.
const MAX_BLOCK_INDEX_ENTRIES: u64 = 64 * 1024 * 1024;

/// Parsed tile/strip offsets and byte counts for one image level.
#[derive(Debug, Clone)]
pub(crate) struct BlockIndex {
    /// File offset of each tile/strip, in block order.
    offsets: Vec<u64>,
    /// Compressed byte length of each tile/strip, in block order.
    byte_counts: Vec<u64>,
}

impl BlockIndex {
    /// Parses the offset/byte-count arrays of one IFD.
    ///
    /// `is_tiled` selects between the tiled (`TileOffsets`/`TileByteCounts`) and
    /// striped (`StripOffsets`/`StripByteCounts`) tag pairs, exactly as the
    /// per-lookup code did.
    ///
    /// # Errors
    /// Returns the same errors the on-demand lookup produced: a
    /// [`FormatError::MissingTag`] naming the absent tag, or whatever
    /// `IfdEntry::get_u64_vec` fails with (short read, incompatible field type).
    pub(crate) fn parse<S: DataSource>(
        ifd: &Ifd,
        source: &S,
        byte_order: ByteOrderType,
        variant: TiffVariant,
        is_tiled: bool,
    ) -> Result<Self> {
        let (offsets_tag, counts_tag) = if is_tiled {
            (TiffTag::TileOffsets, TiffTag::TileByteCounts)
        } else {
            (TiffTag::StripOffsets, TiffTag::StripByteCounts)
        };
        let (offsets_name, counts_name) = if is_tiled {
            ("TileOffsets", "TileByteCounts")
        } else {
            ("StripOffsets", "StripByteCounts")
        };

        let offsets_entry =
            ifd.get_entry(offsets_tag)
                .ok_or(OxiGeoError::Format(FormatError::MissingTag {
                    tag: offsets_name,
                }))?;
        let counts_entry =
            ifd.get_entry(counts_tag)
                .ok_or(OxiGeoError::Format(FormatError::MissingTag {
                    tag: counts_name,
                }))?;

        Ok(Self {
            offsets: offsets_entry.get_u64_vec(source, byte_order, variant)?,
            byte_counts: counts_entry.get_u64_vec(source, byte_order, variant)?,
        })
    }

    /// Pre-parses the index for one level, or returns `None` when it must not be
    /// cached (missing/undersized tags, or an implausible declared entry count).
    ///
    /// Never propagates an error: a level that cannot be pre-parsed simply falls
    /// back to the original on-demand lookup, which reproduces the original error
    /// verbatim at the point of use. This keeps [`CogReader::open`] as tolerant of
    /// malformed files as it has always been, and stops a hostile entry count from
    /// turning `open()` into a huge speculative read.
    pub(crate) fn try_parse<S: DataSource>(
        ifd: &Ifd,
        source: &S,
        byte_order: ByteOrderType,
        variant: TiffVariant,
        is_tiled: bool,
    ) -> Option<Self> {
        let (offsets_tag, counts_tag) = if is_tiled {
            (TiffTag::TileOffsets, TiffTag::TileByteCounts)
        } else {
            (TiffTag::StripOffsets, TiffTag::StripByteCounts)
        };
        let source_size = source.size().ok();
        for tag in [offsets_tag, counts_tag] {
            let entry = ifd.get_entry(tag)?;
            if !is_prereadable(entry, variant, source_size) {
                return None;
            }
        }
        Self::parse(ifd, source, byte_order, variant, is_tiled).ok()
    }

    /// Number of blocks the shorter of the two arrays describes.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.offsets.len().min(self.byte_counts.len())
    }

    /// Returns the byte range of block `index`, or `None` if either array is too
    /// short to describe it (a truncated or malformed index).
    pub(crate) fn byte_range(&self, index: usize) -> Option<ByteRange> {
        let offset = *self.offsets.get(index)?;
        let length = *self.byte_counts.get(index)?;
        Some(ByteRange::from_offset_length(offset, length))
    }
}

/// Decides whether an entry's value array is safe to pre-read at open time.
///
/// Rejects counts beyond [`MAX_BLOCK_INDEX_ENTRIES`] and value arrays that claim
/// more bytes than the data source holds, both of which indicate a malformed or
/// hostile header rather than a real block index.
fn is_prereadable(entry: &IfdEntry, variant: TiffVariant, source_size: Option<u64>) -> bool {
    if entry.count > MAX_BLOCK_INDEX_ENTRIES {
        return false;
    }
    if entry.is_inline(variant) {
        return true;
    }
    match source_size {
        Some(size) => entry
            .value_offset
            .checked_add(entry.value_size())
            .is_some_and(|end| end <= size),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::tiff::FieldType;

    fn entry(count: u64, field_type: FieldType, value_offset: u64) -> IfdEntry {
        IfdEntry {
            tag: TiffTag::StripOffsets as u16,
            field_type,
            count,
            value_offset,
            inline_value: None,
        }
    }

    #[test]
    fn test_issue_14_prereadable_rejects_implausible_counts() {
        // A count beyond the cap must not be pre-read.
        let huge = entry(MAX_BLOCK_INDEX_ENTRIES + 1, FieldType::Long, 1024);
        assert!(!is_prereadable(&huge, TiffVariant::Classic, Some(u64::MAX)));

        // A plausible count whose value array runs past the end of the file must
        // not be pre-read either.
        let past_eof = entry(1024, FieldType::Long, 900);
        assert!(!is_prereadable(&past_eof, TiffVariant::Classic, Some(1000)));

        // The same array inside a large enough file is fine.
        assert!(is_prereadable(
            &past_eof,
            TiffVariant::Classic,
            Some(1_000_000)
        ));

        // Unknown source size: allow (the read itself still validates).
        assert!(is_prereadable(&past_eof, TiffVariant::Classic, None));
    }

    #[test]
    fn test_issue_14_block_index_byte_range_bounds() {
        let index = BlockIndex {
            offsets: vec![100, 200, 300],
            byte_counts: vec![10, 20],
        };
        assert_eq!(index.len(), 2);
        let range = index.byte_range(0).expect("block 0");
        assert_eq!(range.start, 100);
        assert_eq!(range.len(), 10);
        let range = index.byte_range(1).expect("block 1");
        assert_eq!(range.start, 200);
        assert_eq!(range.len(), 20);
        // Block 2 has an offset but no byte count: not describable.
        assert!(index.byte_range(2).is_none());
        assert!(index.byte_range(99).is_none());
    }
}
