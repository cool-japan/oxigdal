//! Transparency-mask (GDAL internal mask) IFD classification.
//!
//! A GDAL internal mask — the IFD chain written by `GDALDataset::CreateMaskBand`
//! with `GDAL_TIFF_INTERNAL_MASK`, and by `gdal_translate -co COPY_SRC_OVERVIEWS`
//! for masked overviews — is stored as an ordinary IFD in the *same* chain as the
//! reduced-resolution overviews. It is a single-bit alpha plane for the image that
//! precedes it, not a pyramid level.
//!
//! A reader that enumerates levels by walking the chain therefore counts every
//! mask as an extra overview: `overview_count` is inflated and every level index
//! past the first mask names the wrong resolution, so a tile read at level *n*
//! silently returns a different image than the caller asked for. The classifier
//! here is what [`CogReader`](crate::cog::CogReader) uses to enumerate levels over
//! non-mask IFDs only, while [`TiffFile`](crate::tiff::TiffFile) keeps exposing the
//! raw chain for consumers that legitimately want the masks.

use super::{ByteOrderType, Ifd, TiffTag};

/// `NewSubfileType` (TIFF 6.0 baseline tag 254). Absent from [`TiffTag`], which
/// only enumerates the tags this crate reads through the typed accessors.
const TAG_NEW_SUBFILE_TYPE: u16 = 254;

/// `NewSubfileType` bit 2: "this image is a transparency mask for another image
/// in this file" (TIFF 6.0, p. 36). GDAL sets it on every internal-mask IFD it
/// writes.
pub const SUBFILE_TYPE_TRANSPARENCY_MASK: u64 = 0x4;

/// `PhotometricInterpretation` (tag 262) value 4: transparency mask. GDAL's
/// internal masks carry this too, and some writers set *only* this.
pub const PHOTOMETRIC_TRANSPARENCY_MASK: u16 = 4;

/// Do these tag values mark a transparency mask rather than a pyramid level?
///
/// Either marker alone is conclusive: `NewSubfileType` bit 2, or
/// `PhotometricInterpretation == 4`. An ordinary reduced-resolution overview
/// (`NewSubfileType == 1`) is never a mask, and neither marker is affected by the
/// other bits of `NewSubfileType`.
///
/// This is the pure core of [`is_mask_ifd`]; it mirrors the classifier the
/// browser-side COG reader in `oxigeo-wasm` applies to the same chain, so both
/// viewers agree on what a level is.
///
/// # Examples
/// ```
/// use oxigeo_geotiff::tiff::is_mask_markers;
///
/// assert!(!is_mask_markers(0, 1)); // plain image
/// assert!(!is_mask_markers(1, 1)); // reduced-resolution overview
/// assert!(is_mask_markers(0x4, 1)); // NewSubfileType bit 2
/// assert!(is_mask_markers(0x5, 1)); // reduced-resolution *and* mask
/// assert!(is_mask_markers(0, 4)); // PhotometricInterpretation only
/// ```
#[must_use]
pub const fn is_mask_markers(new_subfile_type: u64, photometric: u16) -> bool {
    (new_subfile_type & SUBFILE_TYPE_TRANSPARENCY_MASK) != 0
        || photometric == PHOTOMETRIC_TRANSPARENCY_MASK
}

/// Is `ifd` a transparency mask rather than a pyramid level?
///
/// Both tags are single-valued (`NewSubfileType` is a LONG, `Photometric` a
/// SHORT), so they are always stored inline in the entry itself and no data
/// source is needed to read them; an entry that is malformed enough to fail
/// [`IfdEntry::get_u64`](crate::tiff::IfdEntry::get_u64) is treated as absent,
/// i.e. not a mask.
///
/// A missing `PhotometricInterpretation` defaults to 1 (`BlackIsZero`) here
/// rather than to the tag's format-mandated per-compression default, because the
/// only value this predicate cares about is 4.
#[must_use]
pub fn is_mask_ifd(ifd: &Ifd, byte_order: ByteOrderType) -> bool {
    let new_subfile_type = ifd
        .get_entry_raw(TAG_NEW_SUBFILE_TYPE)
        .and_then(|entry| entry.get_u64(byte_order).ok())
        .unwrap_or(0);
    let photometric = ifd
        .get_entry(TiffTag::PhotometricInterpretation)
        .and_then(|entry| entry.get_u64(byte_order).ok())
        .and_then(|value| u16::try_from(value).ok())
        .unwrap_or(1);

    is_mask_markers(new_subfile_type, photometric)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The marker table, pinned value by value. `oxigeo-wasm`'s browser-side
    /// reader classifies the same chain with the same table; nothing native can
    /// compare the two readers end to end, so keeping this table identical is
    /// the parity guarantee.
    #[test]
    fn either_marker_alone_is_conclusive() {
        assert!(!is_mask_markers(0, 1), "plain image");
        assert!(!is_mask_markers(1, 1), "reduced-resolution overview");
        assert!(!is_mask_markers(2, 1), "single page of a multi-page image");
        assert!(is_mask_markers(0x4, 1), "NewSubfileType bit 2 only");
        assert!(is_mask_markers(0x5, 1), "reduced-resolution mask");
        assert!(is_mask_markers(0, PHOTOMETRIC_TRANSPARENCY_MASK));
        assert!(is_mask_markers(
            SUBFILE_TYPE_TRANSPARENCY_MASK,
            PHOTOMETRIC_TRANSPARENCY_MASK
        ));
    }

    /// Photometric values that sit either side of 4 must not be swept up.
    #[test]
    fn neighbouring_photometric_values_are_not_masks() {
        for photometric in [0u16, 1, 2, 3, 5, 6, 8] {
            assert!(
                !is_mask_markers(0, photometric),
                "photometric {photometric} is not a transparency mask"
            );
        }
    }

    /// An IFD with neither tag is an ordinary image.
    #[test]
    fn ifd_without_markers_is_not_a_mask() {
        let ifd = Ifd {
            entries: Vec::new(),
            next_ifd_offset: 0,
        };
        assert!(!is_mask_ifd(&ifd, ByteOrderType::LittleEndian));
    }
}
