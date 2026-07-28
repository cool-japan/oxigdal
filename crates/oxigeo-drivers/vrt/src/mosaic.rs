//! VRT mosaicking logic for combining multiple sources

use crate::error::{Result, VrtError};
use oxigeo_core::types::RasterDataType;
use std::str::FromStr;

/// Mosaic compositor for combining data from multiple sources
pub struct MosaicCompositor {
    /// Blending mode
    mode: BlendMode,
}

impl MosaicCompositor {
    /// Creates a new mosaic compositor
    pub fn new() -> Self {
        Self {
            mode: BlendMode::FirstValid,
        }
    }

    /// Creates a compositor with a specific blend mode
    pub fn with_mode(mode: BlendMode) -> Self {
        Self { mode }
    }

    /// Composites source data into a destination buffer, tracking per-pixel
    /// coverage explicitly.
    ///
    /// `coverage` is a parallel mask (one `bool` per destination pixel, indexed
    /// `row * dest_width + col`) recording which pixels have already received a
    /// value from an earlier source. It MUST be shared across the successive
    /// `composite` calls that fill one output window, and it MUST be sized to at
    /// least `dest_width * (dest_y + height)` pixels.
    ///
    /// Tracking coverage explicitly is what makes `FirstValid` correct: the
    /// previous implementation inferred "unwritten" from an all-zero byte pattern
    /// and therefore silently overwrote *legitimate* zero-valued pixels (sea-level
    /// elevation, masked class 0, a raw radiance of 0) contributed by an earlier
    /// source. With a real coverage mask, a genuine zero from the first source is
    /// preserved. The mask likewise lets `Average`/`Min`/`Max` initialise a pixel
    /// on first write instead of blending against an uninitialised zero.
    ///
    /// # Errors
    /// Returns an error if compositing fails (e.g. an unsupported data type for a
    /// blending mode).
    pub fn composite(
        &self,
        source: &[u8],
        dest: &mut [u8],
        coverage: &mut [bool],
        params: &CompositeParams,
    ) -> Result<()> {
        let CompositeParams {
            dest_x,
            dest_y,
            width,
            height,
            dest_width,
            data_type,
        } = *params;
        let bytes_per_pixel = data_type.size_bytes().max(1);

        // Number of columns that actually fit in the destination row.
        let copy_width = width.min(dest_width.saturating_sub(dest_x)) as usize;

        for y in 0..height {
            let dest_row = dest_y + y;
            let src_row_offset = (y * width) as usize * bytes_per_pixel;
            let dest_row_offset = (dest_row * dest_width + dest_x) as usize * bytes_per_pixel;
            let cov_row_base = (dest_row * dest_width + dest_x) as usize;

            for col in 0..copy_width {
                let s = src_row_offset + col * bytes_per_pixel;
                let d = dest_row_offset + col * bytes_per_pixel;
                let cov_idx = cov_row_base + col;

                if s + bytes_per_pixel > source.len()
                    || d + bytes_per_pixel > dest.len()
                    || cov_idx >= coverage.len()
                {
                    continue;
                }

                let already_written = coverage[cov_idx];

                match self.mode {
                    BlendMode::FirstValid => {
                        if !already_written {
                            dest[d..d + bytes_per_pixel]
                                .copy_from_slice(&source[s..s + bytes_per_pixel]);
                            coverage[cov_idx] = true;
                        }
                    }
                    BlendMode::LastValid => {
                        dest[d..d + bytes_per_pixel]
                            .copy_from_slice(&source[s..s + bytes_per_pixel]);
                        coverage[cov_idx] = true;
                    }
                    BlendMode::Average | BlendMode::Min | BlendMode::Max => {
                        if already_written {
                            // Split-borrow: read the source sample first.
                            let (dst_slice, src_slice) = (
                                &mut dest[d..d + bytes_per_pixel],
                                &source[s..s + bytes_per_pixel],
                            );
                            Self::blend_pixel(self.mode, src_slice, dst_slice, data_type)?;
                        } else {
                            dest[d..d + bytes_per_pixel]
                                .copy_from_slice(&source[s..s + bytes_per_pixel]);
                            coverage[cov_idx] = true;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Blends a single already-written pixel with a new source sample according
    /// to `mode`. Uses native-endian byte access (matching the raster's in-memory
    /// layout) rather than `bytemuck` casts so it is safe at any byte offset.
    fn blend_pixel(
        mode: BlendMode,
        src: &[u8],
        dst: &mut [u8],
        data_type: RasterDataType,
    ) -> Result<()> {
        match data_type {
            RasterDataType::UInt8 => {
                let s = src[0];
                let d = dst[0];
                dst[0] = match mode {
                    BlendMode::Average => ((u16::from(s) + u16::from(d)) / 2) as u8,
                    BlendMode::Min => d.min(s),
                    BlendMode::Max => d.max(s),
                    _ => s,
                };
            }
            RasterDataType::UInt16 => {
                let s = u16::from_ne_bytes([src[0], src[1]]);
                let d = u16::from_ne_bytes([dst[0], dst[1]]);
                let r = match mode {
                    BlendMode::Average => ((u32::from(s) + u32::from(d)) / 2) as u16,
                    BlendMode::Min => d.min(s),
                    BlendMode::Max => d.max(s),
                    _ => s,
                };
                dst[0..2].copy_from_slice(&r.to_ne_bytes());
            }
            RasterDataType::Float32 => {
                let s = f32::from_ne_bytes([src[0], src[1], src[2], src[3]]);
                let d = f32::from_ne_bytes([dst[0], dst[1], dst[2], dst[3]]);
                let r = match mode {
                    BlendMode::Average => (s + d) / 2.0,
                    BlendMode::Min => d.min(s),
                    BlendMode::Max => d.max(s),
                    _ => s,
                };
                dst[0..4].copy_from_slice(&r.to_ne_bytes());
            }
            RasterDataType::Float64 => {
                let s = f64::from_ne_bytes([
                    src[0], src[1], src[2], src[3], src[4], src[5], src[6], src[7],
                ]);
                let d = f64::from_ne_bytes([
                    dst[0], dst[1], dst[2], dst[3], dst[4], dst[5], dst[6], dst[7],
                ]);
                let r = match mode {
                    BlendMode::Average => (s + d) / 2.0,
                    BlendMode::Min => d.min(s),
                    BlendMode::Max => d.max(s),
                    _ => s,
                };
                dst[0..8].copy_from_slice(&r.to_ne_bytes());
            }
            _ => {
                return Err(VrtError::invalid_source(
                    "Unsupported data type for blending",
                ));
            }
        }
        Ok(())
    }
}

impl Default for MosaicCompositor {
    fn default() -> Self {
        Self::new()
    }
}

/// Blend mode for mosaicking
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendMode {
    /// Use first valid (non-zero) value
    FirstValid,
    /// Use last valid value (overwrite)
    LastValid,
    /// Average values
    Average,
    /// Take minimum value
    Min,
    /// Take maximum value
    Max,
}

impl BlendMode {
    /// Returns the string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FirstValid => "FirstValid",
            Self::LastValid => "LastValid",
            Self::Average => "Average",
            Self::Min => "Min",
            Self::Max => "Max",
        }
    }
}

impl FromStr for BlendMode {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "first" | "firstvalid" => Ok(Self::FirstValid),
            "last" | "lastvalid" => Ok(Self::LastValid),
            "average" | "avg" => Ok(Self::Average),
            "min" | "minimum" => Ok(Self::Min),
            "max" | "maximum" => Ok(Self::Max),
            _ => Err(format!("Unknown blend mode: {}", s)),
        }
    }
}

/// Parameters for compositing operations
#[derive(Debug, Clone, Copy)]
pub struct CompositeParams {
    /// Destination X offset
    pub dest_x: u64,
    /// Destination Y offset
    pub dest_y: u64,
    /// Width to composite
    pub width: u64,
    /// Height to composite
    pub height: u64,
    /// Destination buffer width
    pub dest_width: u64,
    /// Data type
    pub data_type: RasterDataType,
}

impl CompositeParams {
    /// Creates new composite parameters
    pub fn new(
        dest_x: u64,
        dest_y: u64,
        width: u64,
        height: u64,
        dest_width: u64,
        data_type: RasterDataType,
    ) -> Self {
        Self {
            dest_x,
            dest_y,
            width,
            height,
            dest_width,
            data_type,
        }
    }
}

/// Mosaic builder helper for determining source contributions
pub struct MosaicPlanner;

impl MosaicPlanner {
    /// Determines which sources contribute to a given window
    pub fn find_contributing_sources<'a>(
        sources: &'a [crate::source::VrtSource],
        window: &crate::source::PixelRect,
    ) -> Vec<&'a crate::source::VrtSource> {
        sources
            .iter()
            .filter(|s| s.dst_rect().map(|r| r.intersects(window)).unwrap_or(false))
            .collect()
    }

    /// Calculates the overlap percentage between a source and window
    pub fn calculate_overlap(
        source_rect: &crate::source::PixelRect,
        window: &crate::source::PixelRect,
    ) -> f64 {
        if let Some(intersection) = source_rect.intersect(window) {
            let intersection_area = (intersection.x_size * intersection.y_size) as f64;
            let window_area = (window.x_size * window.y_size) as f64;
            intersection_area / window_area
        } else {
            0.0
        }
    }

    /// Sorts sources by priority (can be extended with custom priority logic)
    pub fn prioritize_sources<'a>(
        sources: Vec<&'a crate::source::VrtSource>,
        _window: &crate::source::PixelRect,
    ) -> Vec<&'a crate::source::VrtSource> {
        // For now, just return in original order
        // Future: could implement priority based on overlap, quality, etc.
        sources
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coverage_for(params: &CompositeParams) -> Vec<bool> {
        // Large enough to index `row * dest_width + col` for the whole dest grid.
        let rows = params.dest_y + params.height;
        vec![false; (params.dest_width * rows) as usize]
    }

    #[test]
    fn test_mosaic_compositor() {
        let compositor = MosaicCompositor::new();
        let source = vec![1u8, 2, 3, 4];
        let mut dest = vec![0u8; 16];
        let params = CompositeParams::new(0, 0, 2, 2, 4, RasterDataType::UInt8);
        let mut coverage = coverage_for(&params);

        let result = compositor.composite(&source, &mut dest, &mut coverage, &params);

        assert!(result.is_ok());
        assert_eq!(dest[0], 1);
        assert_eq!(dest[1], 2);
        assert_eq!(dest[4], 3);
        assert_eq!(dest[5], 4);
    }

    /// Regression: `FirstValid` must preserve a *legitimate* zero-valued pixel
    /// written by the first source instead of treating all-zero bytes as
    /// "unset" and letting a later overlapping source clobber it (sea-level
    /// elevation, masked class 0, radiance 0, ...).
    #[test]
    fn test_first_valid_preserves_legitimate_zero() {
        let compositor = MosaicCompositor::new(); // FirstValid
        // Two UInt16 pixels; dest_width = 2.
        let params = CompositeParams::new(0, 0, 2, 1, 2, RasterDataType::UInt16);
        let mut coverage = coverage_for(&params);
        let mut dest = vec![0u8; 4];

        // First source writes pixel 0 = 0 (a REAL zero data value) and pixel 1 = 5.
        let mut src1 = Vec::new();
        src1.extend_from_slice(&0u16.to_le_bytes());
        src1.extend_from_slice(&5u16.to_le_bytes());
        compositor
            .composite(&src1, &mut dest, &mut coverage, &params)
            .expect("composite source 1");

        // Second source overlaps both pixels with non-zero values.
        let mut src2 = Vec::new();
        src2.extend_from_slice(&900u16.to_le_bytes());
        src2.extend_from_slice(&999u16.to_le_bytes());
        compositor
            .composite(&src2, &mut dest, &mut coverage, &params)
            .expect("composite source 2");

        // Pixel 0 must remain the first source's genuine 0, NOT be overwritten.
        assert_eq!(
            u16::from_le_bytes([dest[0], dest[1]]),
            0,
            "FirstValid clobbered a legitimate zero pixel"
        );
        assert_eq!(u16::from_le_bytes([dest[2], dest[3]]), 5);
    }

    /// Regression: `FirstValid` must treat whole multi-byte samples atomically.
    /// A UInt16 pixel written by the first source (value 256) must not be
    /// touched at all by a later overlapping source once its pixel is covered.
    #[test]
    fn test_first_valid_preserves_multibyte_sample() {
        let compositor = MosaicCompositor::new(); // defaults to FirstValid
        let params = CompositeParams::new(0, 0, 1, 1, 1, RasterDataType::UInt16);
        let mut coverage = coverage_for(&params);
        let mut dest = vec![0u8; 2];

        let src1 = 256u16.to_le_bytes().to_vec();
        compositor
            .composite(&src1, &mut dest, &mut coverage, &params)
            .expect("composite source 1");
        assert_eq!(u16::from_le_bytes([dest[0], dest[1]]), 256);

        let src2 = 2u16.to_le_bytes().to_vec();
        compositor
            .composite(&src2, &mut dest, &mut coverage, &params)
            .expect("composite source 2");

        assert_eq!(
            u16::from_le_bytes([dest[0], dest[1]]),
            256,
            "FirstValid corrupted a covered multi-byte sample"
        );
    }

    /// Regression: a Float32 sample already written must not be altered by a
    /// later source under `FirstValid`.
    #[test]
    fn test_first_valid_preserves_float_sample() {
        let compositor = MosaicCompositor::new();
        let params = CompositeParams::new(0, 0, 1, 1, 1, RasterDataType::Float32);
        let mut coverage = coverage_for(&params);
        let mut dest = vec![0u8; 4];

        let src1 = 1.0f32.to_le_bytes().to_vec();
        compositor
            .composite(&src1, &mut dest, &mut coverage, &params)
            .expect("composite source 1");
        assert_eq!(
            f32::from_le_bytes([dest[0], dest[1], dest[2], dest[3]]),
            1.0
        );

        let src2 = 42.0f32.to_le_bytes().to_vec();
        compositor
            .composite(&src2, &mut dest, &mut coverage, &params)
            .expect("composite source 2");

        assert_eq!(
            f32::from_le_bytes([dest[0], dest[1], dest[2], dest[3]]),
            1.0,
            "FirstValid altered a covered float sample"
        );
    }

    /// Sanity: a genuinely *uncovered* pixel (one the first source never wrote,
    /// because it lay outside that source's footprint) is still filled by a
    /// later source.
    #[test]
    fn test_first_valid_fills_uncovered_pixel() {
        let compositor = MosaicCompositor::new();

        let mut dest = vec![0u8; 4]; // two UInt16 pixels, dest_width = 2
        let mut coverage = vec![false; 2];

        // First source covers ONLY pixel 0 (width = 1) with value 700.
        let params1 = CompositeParams::new(0, 0, 1, 1, 2, RasterDataType::UInt16);
        let src1 = 700u16.to_le_bytes().to_vec();
        compositor
            .composite(&src1, &mut dest, &mut coverage, &params1)
            .expect("composite source 1");

        // Second source covers both pixels.
        let params2 = CompositeParams::new(0, 0, 2, 1, 2, RasterDataType::UInt16);
        let mut src2 = Vec::new();
        src2.extend_from_slice(&1u16.to_le_bytes());
        src2.extend_from_slice(&999u16.to_le_bytes());
        compositor
            .composite(&src2, &mut dest, &mut coverage, &params2)
            .expect("composite source 2");

        // Pixel 0 keeps the first source's value; pixel 1 (never covered) is filled.
        assert_eq!(u16::from_le_bytes([dest[0], dest[1]]), 700);
        assert_eq!(u16::from_le_bytes([dest[2], dest[3]]), 999);
    }

    /// `Average` initialises an uncovered pixel on first write, then averages on
    /// the second (covered) write — never blending against an uninitialised zero.
    #[test]
    fn test_average_initialises_then_blends() {
        let compositor = MosaicCompositor::with_mode(BlendMode::Average);
        let params = CompositeParams::new(0, 0, 1, 1, 1, RasterDataType::UInt8);
        let mut coverage = coverage_for(&params);
        let mut dest = vec![0u8; 1];

        compositor
            .composite(&[100u8], &mut dest, &mut coverage, &params)
            .expect("first");
        assert_eq!(
            dest[0], 100,
            "first write must initialise, not average with 0"
        );

        compositor
            .composite(&[200u8], &mut dest, &mut coverage, &params)
            .expect("second");
        assert_eq!(dest[0], 150, "second write averages (100 + 200) / 2");
    }

    #[test]
    fn test_blend_mode_parsing() {
        assert_eq!("first".parse::<BlendMode>(), Ok(BlendMode::FirstValid));
        assert_eq!("average".parse::<BlendMode>(), Ok(BlendMode::Average));
        assert_eq!("min".parse::<BlendMode>(), Ok(BlendMode::Min));
        assert!("invalid".parse::<BlendMode>().is_err());
    }

    #[test]
    fn test_blend_average_on_covered_pixels() {
        let compositor = MosaicCompositor::with_mode(BlendMode::Average);
        // Two UInt8 pixels, both already covered so the second source averages.
        let params = CompositeParams::new(0, 0, 2, 1, 2, RasterDataType::UInt8);
        let mut coverage = vec![true, true];
        let mut dest = vec![50u8, 100];

        let result = compositor.composite(&[100u8, 200], &mut dest, &mut coverage, &params);
        assert!(result.is_ok());
        assert_eq!(dest[0], 75); // (100 + 50) / 2
        assert_eq!(dest[1], 150); // (200 + 100) / 2
    }

    #[test]
    fn test_mosaic_planner() {
        use crate::source::{PixelRect, SourceWindow, VrtSource};

        let src1 = VrtSource::simple("/test1.tif", 1).with_window(SourceWindow::new(
            PixelRect::new(0, 0, 256, 256),
            PixelRect::new(0, 0, 256, 256),
        ));

        let src2 = VrtSource::simple("/test2.tif", 1).with_window(SourceWindow::new(
            PixelRect::new(0, 0, 256, 256),
            PixelRect::new(256, 0, 256, 256),
        ));

        let sources = vec![src1, src2];
        let window = PixelRect::new(0, 0, 512, 256);

        let contributing = MosaicPlanner::find_contributing_sources(&sources, &window);
        assert_eq!(contributing.len(), 2);

        let window_partial = PixelRect::new(100, 100, 100, 100);
        let contributing_partial =
            MosaicPlanner::find_contributing_sources(&sources, &window_partial);
        assert_eq!(contributing_partial.len(), 1);
    }

    #[test]
    fn test_overlap_calculation() {
        use crate::source::PixelRect;

        let source_rect = PixelRect::new(0, 0, 100, 100);
        let window = PixelRect::new(50, 50, 100, 100);

        let overlap = MosaicPlanner::calculate_overlap(&source_rect, &window);
        assert!((overlap - 0.25).abs() < 0.01); // 50x50 / 100x100 = 0.25
    }
}
