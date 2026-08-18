//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use super::types_3::CogMetadata;

/// Per-IFD metadata for one pyramid level (full resolution or an overview).
///
/// Every entry carries the tile geometry, sample layout, decompression codec,
/// Predictor (TIFF tag 317) and tile directory required to fetch and correctly
/// decode a tile at that resolution. `levels[0]` in [`CogMetadata`] is the
/// full-resolution image; subsequent entries are the reduced-resolution
/// overviews in file order (typically halving each dimension).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct IfdMetadata {
    pub width: u64,
    pub height: u64,
    pub tile_width: u32,
    pub tile_height: u32,
    pub bits_per_sample: u16,
    pub samples_per_pixel: u16,
    /// TIFF SampleFormat (tag 339): 1=unsigned int, 2=signed int, 3=IEEE float
    pub sample_format: u16,
    pub compression: u16,
    pub photometric_interpretation: u16,
    /// TIFF Predictor (tag 317): 1=none, 2=horizontal differencing.
    pub predictor: u16,
    pub tile_offsets: Vec<u64>,
    pub tile_byte_counts: Vec<u64>,
    pub pixel_scale_x: Option<f64>,
    pub pixel_scale_y: Option<f64>,
    pub tiepoint_pixel_x: Option<f64>,
    pub tiepoint_pixel_y: Option<f64>,
    pub tiepoint_geo_x: Option<f64>,
    pub tiepoint_geo_y: Option<f64>,
    pub epsg_code: Option<u32>,
}
impl IfdMetadata {
    /// Build a per-level record from a freshly parsed [`CogMetadata`] IFD.
    pub(super) fn from_cog(meta: &CogMetadata) -> Self {
        Self {
            width: meta.width,
            height: meta.height,
            tile_width: meta.tile_width,
            tile_height: meta.tile_height,
            bits_per_sample: meta.bits_per_sample,
            samples_per_pixel: meta.samples_per_pixel,
            sample_format: meta.sample_format,
            compression: meta.compression,
            photometric_interpretation: meta.photometric_interpretation,
            predictor: meta.predictor,
            tile_offsets: meta.tile_offsets.clone(),
            tile_byte_counts: meta.tile_byte_counts.clone(),
            pixel_scale_x: meta.pixel_scale_x,
            pixel_scale_y: meta.pixel_scale_y,
            tiepoint_pixel_x: meta.tiepoint_pixel_x,
            tiepoint_pixel_y: meta.tiepoint_pixel_y,
            tiepoint_geo_x: meta.tiepoint_geo_x,
            tiepoint_geo_y: meta.tiepoint_geo_y,
            epsg_code: meta.epsg_code,
        }
    }
}
/// Overview/pyramid level metadata
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct OverviewMetadata {
    pub width: u64,
    pub height: u64,
    pub tile_width: u32,
    pub tile_height: u32,
}
