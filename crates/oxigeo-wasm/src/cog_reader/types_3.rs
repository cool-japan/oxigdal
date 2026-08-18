//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use super::types::{IfdMetadata, OverviewMetadata};

/// TIFF byte order
#[derive(Debug, Clone, Copy)]
pub enum ByteOrder {
    LittleEndian,
    BigEndian,
}
/// One IFD as returned by the parser, before it is classified into the pyramid.
///
/// `is_mask` is carried here rather than on [`CogMetadata`]/[`IfdMetadata`] on
/// purpose: those are all-`pub` structs with no constructor, so adding a field
/// would break anyone building one by literal. The flag is only needed while
/// walking the chain, which is entirely private.
pub(super) struct ParsedIfd {
    /// Tags parsed from this IFD (`levels`/`overviews` not yet populated).
    pub(super) metadata: CogMetadata,
    /// Absolute file offset of the next IFD, or 0 at the end of the chain.
    pub(super) next_ifd_offset: u64,
    /// This IFD is a transparency mask, not a pyramid level.
    pub(super) is_mask: bool,
}
/// TIFF/COG metadata extracted from IFD
#[derive(Debug, Clone)]
pub struct CogMetadata {
    pub width: u64,
    pub height: u64,
    pub tile_width: u32,
    pub tile_height: u32,
    pub bits_per_sample: u16,
    pub samples_per_pixel: u16,
    /// TIFF SampleFormat (tag 339): 1=unsigned int, 2=signed int, 3=IEEE float
    pub sample_format: u16,
    pub compression: u16,
    #[allow(dead_code)]
    pub photometric_interpretation: u16,
    /// TIFF Predictor (tag 317) for the full-resolution IFD: 1=none, 2=horizontal.
    pub predictor: u16,
    pub tile_offsets: Vec<u64>,
    pub tile_byte_counts: Vec<u64>,
    pub pixel_scale_x: Option<f64>,
    pub pixel_scale_y: Option<f64>,
    pub tiepoint_pixel_x: Option<f64>,
    pub tiepoint_pixel_y: Option<f64>,
    pub tiepoint_geo_x: Option<f64>,
    pub tiepoint_geo_y: Option<f64>,
    pub overview_count: usize,
    pub overviews: Vec<OverviewMetadata>,
    pub epsg_code: Option<u32>,
    /// All pyramid levels in file order; `levels[0]` is the full-resolution IFD
    /// and `levels[1..]` are the overviews. Each carries its own tile directory,
    /// predictor and sample layout for `read_tile_level` / `read_window_*`.
    pub levels: Vec<IfdMetadata>,
}
