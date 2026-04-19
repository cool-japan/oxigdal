//! Sub-region window into a raster dataset.

use crate::error::{OxiGdalError, Result};
use core::fmt;

/// A sub-region window into a raster dataset.
/// Defines a rectangular area for reading/writing a portion of raster data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RasterWindow {
    /// Column offset from the left edge (0-based)
    pub col_off: u32,
    /// Row offset from the top edge (0-based)
    pub row_off: u32,
    /// Width of the window in pixels
    pub width: u32,
    /// Height of the window in pixels
    pub height: u32,
}

impl RasterWindow {
    /// Create a new window. Validates that width and height are > 0.
    pub fn new(col_off: u32, row_off: u32, width: u32, height: u32) -> Result<Self> {
        if width == 0 {
            return Err(OxiGdalError::invalid_parameter(
                "width",
                "window width must be greater than 0",
            ));
        }
        if height == 0 {
            return Err(OxiGdalError::invalid_parameter(
                "height",
                "window height must be greater than 0",
            ));
        }
        Ok(Self {
            col_off,
            row_off,
            width,
            height,
        })
    }

    /// Create a window covering the full extent of a raster.
    pub fn full(raster_width: u32, raster_height: u32) -> Result<Self> {
        Self::new(0, 0, raster_width, raster_height)
    }

    /// Check if this window fits within a raster of the given dimensions.
    pub fn fits_within(&self, raster_width: u32, raster_height: u32) -> bool {
        let col_end = u64::from(self.col_off) + u64::from(self.width);
        let row_end = u64::from(self.row_off) + u64::from(self.height);
        col_end <= u64::from(raster_width) && row_end <= u64::from(raster_height)
    }

    /// Validate that this window fits within the given raster dimensions.
    /// Returns an error if any part is out of bounds.
    pub fn validate_bounds(&self, raster_width: u32, raster_height: u32) -> Result<()> {
        if !self.fits_within(raster_width, raster_height) {
            return Err(OxiGdalError::OutOfBounds {
                message: format!(
                    "Window (col_off={}, row_off={}, width={}, height={}) exceeds raster bounds ({}x{})",
                    self.col_off,
                    self.row_off,
                    self.width,
                    self.height,
                    raster_width,
                    raster_height
                ),
            });
        }
        Ok(())
    }

    /// Total number of pixels in this window.
    pub fn pixel_count(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    /// Intersect this window with another, returning the overlapping region.
    /// Returns None if they don't overlap.
    pub fn intersection(&self, other: &RasterWindow) -> Option<RasterWindow> {
        let left = self.col_off.max(other.col_off);
        let top = self.row_off.max(other.row_off);

        let self_right = self.col_off.checked_add(self.width)?;
        let other_right = other.col_off.checked_add(other.width)?;
        let right = self_right.min(other_right);

        let self_bottom = self.row_off.checked_add(self.height)?;
        let other_bottom = other.row_off.checked_add(other.height)?;
        let bottom = self_bottom.min(other_bottom);

        if left >= right || top >= bottom {
            return None;
        }

        // Safety: width/height are > 0 since left < right and top < bottom
        Some(RasterWindow {
            col_off: left,
            row_off: top,
            width: right - left,
            height: bottom - top,
        })
    }

    /// Compute the union bounding box of this window and another.
    pub fn union_bounds(&self, other: &RasterWindow) -> RasterWindow {
        let left = self.col_off.min(other.col_off);
        let top = self.row_off.min(other.row_off);

        let self_right = u64::from(self.col_off) + u64::from(self.width);
        let other_right = u64::from(other.col_off) + u64::from(other.width);
        let right = self_right.max(other_right);

        let self_bottom = u64::from(self.row_off) + u64::from(self.height);
        let other_bottom = u64::from(other.row_off) + u64::from(other.height);
        let bottom = self_bottom.max(other_bottom);

        // Widths/heights saturate to u32::MAX if they overflow
        let width = u32::try_from(right - u64::from(left)).unwrap_or(u32::MAX);
        let height = u32::try_from(bottom - u64::from(top)).unwrap_or(u32::MAX);

        RasterWindow {
            col_off: left,
            row_off: top,
            width,
            height,
        }
    }

    /// Check if this window contains the given pixel coordinates.
    pub fn contains_pixel(&self, col: u32, row: u32) -> bool {
        col >= self.col_off
            && row >= self.row_off
            && u64::from(col) < u64::from(self.col_off) + u64::from(self.width)
            && u64::from(row) < u64::from(self.row_off) + u64::from(self.height)
    }

    /// Subdivide this window into tiles of the given size.
    /// The last tiles in each row/column may be smaller.
    pub fn subdivide(&self, tile_width: u32, tile_height: u32) -> Result<Vec<RasterWindow>> {
        if tile_width == 0 {
            return Err(OxiGdalError::invalid_parameter(
                "tile_width",
                "tile width must be greater than 0",
            ));
        }
        if tile_height == 0 {
            return Err(OxiGdalError::invalid_parameter(
                "tile_height",
                "tile height must be greater than 0",
            ));
        }

        let cols = self.width.div_ceil(tile_width);
        let rows = self.height.div_ceil(tile_height);
        let capacity = u64::from(cols) * u64::from(rows);

        let mut tiles = Vec::with_capacity(usize::try_from(capacity).map_err(|_| {
            OxiGdalError::invalid_parameter("tile_size", "too many tiles would be generated")
        })?);

        for row_idx in 0..rows {
            for col_idx in 0..cols {
                let c = self.col_off + col_idx * tile_width;
                let r = self.row_off + row_idx * tile_height;
                let w = tile_width.min(self.col_off + self.width - c);
                let h = tile_height.min(self.row_off + self.height - r);
                tiles.push(RasterWindow {
                    col_off: c,
                    row_off: r,
                    width: w,
                    height: h,
                });
            }
        }

        Ok(tiles)
    }

    /// Convert a pixel coordinate from window-local to global raster coordinates.
    pub fn to_global(&self, local_col: u32, local_row: u32) -> Result<(u32, u32)> {
        if local_col >= self.width || local_row >= self.height {
            return Err(OxiGdalError::OutOfBounds {
                message: format!(
                    "Local coordinate ({}, {}) is outside window dimensions ({}x{})",
                    local_col, local_row, self.width, self.height
                ),
            });
        }
        let global_col =
            self.col_off
                .checked_add(local_col)
                .ok_or_else(|| OxiGdalError::OutOfBounds {
                    message: "Global column coordinate overflow".to_string(),
                })?;
        let global_row =
            self.row_off
                .checked_add(local_row)
                .ok_or_else(|| OxiGdalError::OutOfBounds {
                    message: "Global row coordinate overflow".to_string(),
                })?;
        Ok((global_col, global_row))
    }

    /// Convert a pixel coordinate from global raster to window-local coordinates.
    /// Returns None if the global coordinate is outside this window.
    pub fn to_local(&self, global_col: u32, global_row: u32) -> Option<(u32, u32)> {
        if !self.contains_pixel(global_col, global_row) {
            return None;
        }
        Some((global_col - self.col_off, global_row - self.row_off))
    }

    /// Extract the bytes for this window from a full-raster band buffer.
    /// Assumes row-major layout with `bytes_per_pixel` bytes per pixel and
    /// `raster_width` pixels per row in the source buffer.
    pub fn extract_from_buffer(
        &self,
        source: &[u8],
        raster_width: u32,
        bytes_per_pixel: u32,
    ) -> Result<Vec<u8>> {
        if bytes_per_pixel == 0 {
            return Err(OxiGdalError::invalid_parameter(
                "bytes_per_pixel",
                "must be greater than 0",
            ));
        }

        let rw = u64::from(raster_width);
        let bpp = u64::from(bytes_per_pixel);
        let row_stride = rw.checked_mul(bpp).ok_or_else(|| {
            OxiGdalError::invalid_parameter("raster_width", "row stride overflow")
        })?;

        // Validate source buffer size: we need at least (row_off + height) rows
        let last_row = u64::from(self.row_off) + u64::from(self.height);
        let required_len = last_row.checked_mul(row_stride).ok_or_else(|| {
            OxiGdalError::invalid_parameter("raster_width", "buffer size calculation overflow")
        })?;
        if u64::try_from(source.len()).unwrap_or(0) < required_len {
            return Err(OxiGdalError::OutOfBounds {
                message: format!(
                    "Source buffer too small: need {} bytes, got {}",
                    required_len,
                    source.len()
                ),
            });
        }

        // Validate column bounds
        let col_end = u64::from(self.col_off) + u64::from(self.width);
        if col_end > rw {
            return Err(OxiGdalError::OutOfBounds {
                message: format!(
                    "Window column extent {} exceeds raster width {}",
                    col_end, raster_width
                ),
            });
        }

        let win_row_bytes = u64::from(self.width) * bpp;
        let total_bytes = win_row_bytes * u64::from(self.height);
        let total_usize = usize::try_from(total_bytes).map_err(|_| {
            OxiGdalError::invalid_parameter("window", "window data size exceeds addressable memory")
        })?;

        let mut result = Vec::with_capacity(total_usize);

        for row in 0..self.height {
            let src_row = u64::from(self.row_off + row);
            let src_offset = src_row * row_stride + u64::from(self.col_off) * bpp;
            let start = src_offset as usize;
            let end = start + win_row_bytes as usize;
            result.extend_from_slice(&source[start..end]);
        }

        Ok(result)
    }

    /// Write window data back into a full-raster band buffer.
    pub fn write_to_buffer(
        &self,
        window_data: &[u8],
        dest: &mut [u8],
        raster_width: u32,
        bytes_per_pixel: u32,
    ) -> Result<()> {
        if bytes_per_pixel == 0 {
            return Err(OxiGdalError::invalid_parameter(
                "bytes_per_pixel",
                "must be greater than 0",
            ));
        }

        let rw = u64::from(raster_width);
        let bpp = u64::from(bytes_per_pixel);
        let row_stride = rw.checked_mul(bpp).ok_or_else(|| {
            OxiGdalError::invalid_parameter("raster_width", "row stride overflow")
        })?;

        let win_row_bytes = u64::from(self.width) * bpp;
        let expected_data_len = win_row_bytes * u64::from(self.height);
        if u64::try_from(window_data.len()).unwrap_or(0) != expected_data_len {
            return Err(OxiGdalError::invalid_parameter(
                "window_data",
                format!(
                    "expected {} bytes, got {}",
                    expected_data_len,
                    window_data.len()
                ),
            ));
        }

        // Validate dest buffer size
        let last_row = u64::from(self.row_off) + u64::from(self.height);
        let required_len = last_row.checked_mul(row_stride).ok_or_else(|| {
            OxiGdalError::invalid_parameter("raster_width", "buffer size calculation overflow")
        })?;
        if u64::try_from(dest.len()).unwrap_or(0) < required_len {
            return Err(OxiGdalError::OutOfBounds {
                message: format!(
                    "Destination buffer too small: need {} bytes, got {}",
                    required_len,
                    dest.len()
                ),
            });
        }

        // Validate column bounds
        let col_end = u64::from(self.col_off) + u64::from(self.width);
        if col_end > rw {
            return Err(OxiGdalError::OutOfBounds {
                message: format!(
                    "Window column extent {} exceeds raster width {}",
                    col_end, raster_width
                ),
            });
        }

        for row in 0..self.height {
            let dst_row = u64::from(self.row_off + row);
            let dst_offset = dst_row * row_stride + u64::from(self.col_off) * bpp;
            let dst_start = dst_offset as usize;
            let dst_end = dst_start + win_row_bytes as usize;

            let src_offset = u64::from(row) * win_row_bytes;
            let src_start = src_offset as usize;
            let src_end = src_start + win_row_bytes as usize;

            dest[dst_start..dst_end].copy_from_slice(&window_data[src_start..src_end]);
        }

        Ok(())
    }
}

impl fmt::Display for RasterWindow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Window(col_off={}, row_off={}, width={}, height={})",
            self.col_off, self.row_off, self.width, self.height
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_valid() {
        let w = RasterWindow::new(0, 0, 100, 100).expect("should create valid window");
        assert_eq!(w.col_off, 0);
        assert_eq!(w.row_off, 0);
        assert_eq!(w.width, 100);
        assert_eq!(w.height, 100);
    }

    #[test]
    fn test_new_zero_width_error() {
        let result = RasterWindow::new(0, 0, 0, 100);
        assert!(result.is_err());
    }

    #[test]
    fn test_new_zero_height_error() {
        let result = RasterWindow::new(0, 0, 100, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_full() {
        let w = RasterWindow::full(256, 256).expect("should create full window");
        assert_eq!(w.col_off, 0);
        assert_eq!(w.row_off, 0);
        assert_eq!(w.width, 256);
        assert_eq!(w.height, 256);
    }

    #[test]
    fn test_fits_within() {
        let w = RasterWindow::new(10, 10, 50, 50).expect("valid window");
        assert!(w.fits_within(100, 100));
        assert!(!w.fits_within(30, 30));
    }

    #[test]
    fn test_validate_bounds_ok() {
        let w = RasterWindow::new(0, 0, 100, 100).expect("valid window");
        assert!(w.validate_bounds(100, 100).is_ok());
    }

    #[test]
    fn test_validate_bounds_overflow() {
        let w = RasterWindow::new(50, 50, 100, 100).expect("valid window");
        assert!(w.validate_bounds(100, 100).is_err());
    }

    #[test]
    fn test_pixel_count() {
        let w = RasterWindow::new(0, 0, 50, 50).expect("valid window");
        assert_eq!(w.pixel_count(), 2500);
    }

    #[test]
    fn test_intersection_overlap() {
        let a = RasterWindow::new(0, 0, 100, 100).expect("valid window");
        let b = RasterWindow::new(50, 50, 100, 100).expect("valid window");
        let isect = a.intersection(&b).expect("should overlap");
        assert_eq!(isect.col_off, 50);
        assert_eq!(isect.row_off, 50);
        assert_eq!(isect.width, 50);
        assert_eq!(isect.height, 50);
    }

    #[test]
    fn test_intersection_no_overlap() {
        let a = RasterWindow::new(0, 0, 50, 50).expect("valid window");
        let b = RasterWindow::new(100, 100, 50, 50).expect("valid window");
        assert!(a.intersection(&b).is_none());
    }

    #[test]
    fn test_union_bounds() {
        let a = RasterWindow::new(10, 10, 40, 40).expect("valid window");
        let b = RasterWindow::new(30, 30, 60, 60).expect("valid window");
        let u = a.union_bounds(&b);
        assert_eq!(u.col_off, 10);
        assert_eq!(u.row_off, 10);
        assert_eq!(u.width, 80);
        assert_eq!(u.height, 80);
    }

    #[test]
    fn test_contains_pixel() {
        let w = RasterWindow::new(10, 10, 50, 50).expect("valid window");
        assert!(w.contains_pixel(10, 10));
        assert!(w.contains_pixel(59, 59));
        assert!(!w.contains_pixel(60, 60));
        assert!(!w.contains_pixel(9, 9));
    }

    #[test]
    fn test_subdivide() {
        let w = RasterWindow::new(0, 0, 100, 100).expect("valid window");
        let tiles = w.subdivide(30, 30).expect("should subdivide");
        // 4 cols (30, 30, 30, 10) * 4 rows (30, 30, 30, 10) = 16 tiles
        assert_eq!(tiles.len(), 16);

        // First tile
        assert_eq!(tiles[0].col_off, 0);
        assert_eq!(tiles[0].row_off, 0);
        assert_eq!(tiles[0].width, 30);
        assert_eq!(tiles[0].height, 30);

        // Last tile in first row (partial width)
        assert_eq!(tiles[3].col_off, 90);
        assert_eq!(tiles[3].row_off, 0);
        assert_eq!(tiles[3].width, 10);
        assert_eq!(tiles[3].height, 30);

        // Last tile (bottom-right, partial both)
        assert_eq!(tiles[15].col_off, 90);
        assert_eq!(tiles[15].row_off, 90);
        assert_eq!(tiles[15].width, 10);
        assert_eq!(tiles[15].height, 10);
    }

    #[test]
    fn test_to_global_and_local() {
        let w = RasterWindow::new(10, 20, 50, 50).expect("valid window");

        // Local (5, 3) -> global (15, 23)
        let (gc, gr) = w.to_global(5, 3).expect("should convert");
        assert_eq!((gc, gr), (15, 23));

        // Global (15, 23) -> local (5, 3)
        let local = w.to_local(15, 23).expect("should be inside");
        assert_eq!(local, (5, 3));

        // Outside global coord
        assert!(w.to_local(0, 0).is_none());

        // Out-of-bounds local coord
        assert!(w.to_global(50, 50).is_err());
    }

    #[test]
    fn test_extract_from_buffer() {
        // 4x4 raster, 1 byte per pixel, values 0..16
        let source: Vec<u8> = (0u8..16).collect();
        let w = RasterWindow::new(1, 1, 2, 2).expect("valid window");
        let extracted = w
            .extract_from_buffer(&source, 4, 1)
            .expect("should extract");

        // Row 1: pixels at col 1,2 → values 5, 6
        // Row 2: pixels at col 1,2 → values 9, 10
        assert_eq!(extracted, vec![5, 6, 9, 10]);
    }

    #[test]
    fn test_write_to_buffer() {
        // 4x4 raster, 1 byte per pixel, all zeros
        let mut dest = vec![0u8; 16];
        let w = RasterWindow::new(1, 1, 2, 2).expect("valid window");
        let window_data = vec![0xAA, 0xBB, 0xCC, 0xDD];
        w.write_to_buffer(&window_data, &mut dest, 4, 1)
            .expect("should write");

        // Check the 4x4 buffer
        #[rustfmt::skip]
        let expected: Vec<u8> = vec![
            0x00, 0x00, 0x00, 0x00,
            0x00, 0xAA, 0xBB, 0x00,
            0x00, 0xCC, 0xDD, 0x00,
            0x00, 0x00, 0x00, 0x00,
        ];
        assert_eq!(dest, expected);
    }

    #[test]
    fn test_display() {
        let w = RasterWindow::new(5, 10, 100, 200).expect("valid window");
        assert_eq!(
            w.to_string(),
            "Window(col_off=5, row_off=10, width=100, height=200)"
        );
    }
}
