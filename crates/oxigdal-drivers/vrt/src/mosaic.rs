//! VRT mosaicking logic for combining multiple sources

use crate::error::{Result, VrtError};
use oxigdal_core::types::RasterDataType;
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

    /// Composites source data into destination buffer
    ///
    /// # Errors
    /// Returns an error if compositing fails
    pub fn composite(
        &self,
        source: &[u8],
        dest: &mut [u8],
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
        let bytes_per_pixel = data_type.size_bytes();

        for y in 0..height {
            let dest_row = dest_y + y;
            let src_row_offset = (y * width) as usize * bytes_per_pixel;
            let dest_row_offset = (dest_row * dest_width + dest_x) as usize * bytes_per_pixel;

            let copy_width = width.min(dest_width - dest_x) as usize;
            let copy_bytes = copy_width * bytes_per_pixel;

            let src_start = src_row_offset;
            let src_end = src_start + copy_bytes;
            let dest_start = dest_row_offset;
            let dest_end = dest_start + copy_bytes;

            if src_end <= source.len() && dest_end <= dest.len() {
                match self.mode {
                    BlendMode::FirstValid => {
                        // Copy only if destination is zero (not set)
                        for i in 0..copy_bytes {
                            if dest[dest_start + i] == 0 {
                                dest[dest_start + i] = source[src_start + i];
                            }
                        }
                    }
                    BlendMode::LastValid => {
                        // Always overwrite
                        dest[dest_start..dest_end].copy_from_slice(&source[src_start..src_end]);
                    }
                    BlendMode::Average => {
                        // Average with existing value
                        self.blend_average(
                            &source[src_start..src_end],
                            &mut dest[dest_start..dest_end],
                            data_type,
                        )?;
                    }
                    BlendMode::Min => {
                        self.blend_min(
                            &source[src_start..src_end],
                            &mut dest[dest_start..dest_end],
                            data_type,
                        )?;
                    }
                    BlendMode::Max => {
                        self.blend_max(
                            &source[src_start..src_end],
                            &mut dest[dest_start..dest_end],
                            data_type,
                        )?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Blends using average mode
    fn blend_average(
        &self,
        source: &[u8],
        dest: &mut [u8],
        data_type: RasterDataType,
    ) -> Result<()> {
        match data_type {
            RasterDataType::UInt8 => {
                for i in 0..source.len() {
                    let src = source[i] as u16;
                    let dst = dest[i] as u16;
                    dest[i] = ((src + dst) / 2) as u8;
                }
            }
            RasterDataType::UInt16 => {
                let src_u16 = bytemuck::cast_slice::<u8, u16>(source);
                let dst_u16 = bytemuck::cast_slice_mut::<u8, u16>(dest);
                for i in 0..src_u16.len() {
                    let src = src_u16[i] as u32;
                    let dst = dst_u16[i] as u32;
                    dst_u16[i] = ((src + dst) / 2) as u16;
                }
            }
            RasterDataType::Float32 => {
                let src_f32 = bytemuck::cast_slice::<u8, f32>(source);
                let dst_f32 = bytemuck::cast_slice_mut::<u8, f32>(dest);
                for i in 0..src_f32.len() {
                    dst_f32[i] = (src_f32[i] + dst_f32[i]) / 2.0;
                }
            }
            RasterDataType::Float64 => {
                let src_f64 = bytemuck::cast_slice::<u8, f64>(source);
                let dst_f64 = bytemuck::cast_slice_mut::<u8, f64>(dest);
                for i in 0..src_f64.len() {
                    dst_f64[i] = (src_f64[i] + dst_f64[i]) / 2.0;
                }
            }
            _ => {
                return Err(VrtError::invalid_source(
                    "Unsupported data type for averaging",
                ));
            }
        }
        Ok(())
    }

    /// Blends using min mode
    fn blend_min(&self, source: &[u8], dest: &mut [u8], data_type: RasterDataType) -> Result<()> {
        match data_type {
            RasterDataType::UInt8 => {
                for i in 0..source.len() {
                    dest[i] = dest[i].min(source[i]);
                }
            }
            RasterDataType::UInt16 => {
                let src_u16 = bytemuck::cast_slice::<u8, u16>(source);
                let dst_u16 = bytemuck::cast_slice_mut::<u8, u16>(dest);
                for i in 0..src_u16.len() {
                    dst_u16[i] = dst_u16[i].min(src_u16[i]);
                }
            }
            RasterDataType::Float32 => {
                let src_f32 = bytemuck::cast_slice::<u8, f32>(source);
                let dst_f32 = bytemuck::cast_slice_mut::<u8, f32>(dest);
                for i in 0..src_f32.len() {
                    dst_f32[i] = dst_f32[i].min(src_f32[i]);
                }
            }
            RasterDataType::Float64 => {
                let src_f64 = bytemuck::cast_slice::<u8, f64>(source);
                let dst_f64 = bytemuck::cast_slice_mut::<u8, f64>(dest);
                for i in 0..src_f64.len() {
                    dst_f64[i] = dst_f64[i].min(src_f64[i]);
                }
            }
            _ => {
                return Err(VrtError::invalid_source("Unsupported data type for min"));
            }
        }
        Ok(())
    }

    /// Blends using max mode
    fn blend_max(&self, source: &[u8], dest: &mut [u8], data_type: RasterDataType) -> Result<()> {
        match data_type {
            RasterDataType::UInt8 => {
                for i in 0..source.len() {
                    dest[i] = dest[i].max(source[i]);
                }
            }
            RasterDataType::UInt16 => {
                let src_u16 = bytemuck::cast_slice::<u8, u16>(source);
                let dst_u16 = bytemuck::cast_slice_mut::<u8, u16>(dest);
                for i in 0..src_u16.len() {
                    dst_u16[i] = dst_u16[i].max(src_u16[i]);
                }
            }
            RasterDataType::Float32 => {
                let src_f32 = bytemuck::cast_slice::<u8, f32>(source);
                let dst_f32 = bytemuck::cast_slice_mut::<u8, f32>(dest);
                for i in 0..src_f32.len() {
                    dst_f32[i] = dst_f32[i].max(src_f32[i]);
                }
            }
            RasterDataType::Float64 => {
                let src_f64 = bytemuck::cast_slice::<u8, f64>(source);
                let dst_f64 = bytemuck::cast_slice_mut::<u8, f64>(dest);
                for i in 0..src_f64.len() {
                    dst_f64[i] = dst_f64[i].max(src_f64[i]);
                }
            }
            _ => {
                return Err(VrtError::invalid_source("Unsupported data type for max"));
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

    #[test]
    fn test_mosaic_compositor() {
        let compositor = MosaicCompositor::new();
        let source = vec![1u8, 2, 3, 4];
        let mut dest = vec![0u8; 16];
        let params = CompositeParams::new(0, 0, 2, 2, 4, RasterDataType::UInt8);

        let result = compositor.composite(&source, &mut dest, &params);

        assert!(result.is_ok());
        assert_eq!(dest[0], 1);
        assert_eq!(dest[1], 2);
        assert_eq!(dest[4], 3);
        assert_eq!(dest[5], 4);
    }

    #[test]
    fn test_blend_mode_parsing() {
        assert_eq!("first".parse::<BlendMode>(), Ok(BlendMode::FirstValid));
        assert_eq!("average".parse::<BlendMode>(), Ok(BlendMode::Average));
        assert_eq!("min".parse::<BlendMode>(), Ok(BlendMode::Min));
        assert!("invalid".parse::<BlendMode>().is_err());
    }

    #[test]
    fn test_blend_average() {
        let compositor = MosaicCompositor::with_mode(BlendMode::Average);
        let source = vec![100u8, 200];
        let mut dest = vec![50u8, 100];

        let result = compositor.blend_average(&source, &mut dest, RasterDataType::UInt8);
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
