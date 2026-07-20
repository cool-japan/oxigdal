//! JPEG2000 Region of Interest (ROI) Tier-2 support
//!
//! JPEG2000 ROI coding uses the MaxShift method (ISO 15444-1:2019 §A.6.4).
//! Wavelet coefficients that belong to the ROI region are upshifted by a
//! fixed number of bit planes, ensuring they survive quantisation and achieve
//! higher quality than the background.
//!
//! This module provides:
//! - [`RoiMap`]: a binary mask of which pixels/code-blocks belong to the ROI.
//! - [`RoiShift`]: helpers to apply and remove the upshift from coefficient arrays.

use crate::error::{Jpeg2000Error, Result};

// ---------------------------------------------------------------------------
// RoiMap
// ---------------------------------------------------------------------------

/// A binary region-of-interest mask for a single component plane.
///
/// The mask covers the full image and records which pixels lie inside
/// at least one defined ROI region.  Multiple regions may overlap;
/// any pixel inside any region is marked as ROI.
#[derive(Debug, Clone)]
pub struct RoiMap {
    width: u32,
    height: u32,
    /// ROI upshift value applied to code blocks fully inside the ROI.
    shift: u8,
    /// Flat boolean mask; `true` = ROI pixel.  Row-major order.
    mask: Vec<bool>,
}

impl RoiMap {
    /// Create a new, empty ROI map (all pixels set to background).
    ///
    /// # Parameters
    /// - `width`, `height`: Image dimensions in pixels.
    /// - `shift`: Upshift value (0–31) applied to ROI code blocks.
    pub fn new(width: u32, height: u32, shift: u8) -> Self {
        let total = width as usize * height as usize;
        Self {
            width,
            height,
            shift,
            mask: vec![false; total],
        }
    }

    /// Mark a rectangular region `[x, x+w) × [y, y+h)` as ROI.
    ///
    /// Coordinates are clamped to the image bounds, so out-of-bounds rectangles
    /// are silently clipped rather than returning an error.
    pub fn add_rect(&mut self, x: u32, y: u32, w: u32, h: u32) {
        let x_end = (x + w).min(self.width);
        let y_end = (y + h).min(self.height);
        for py in y.min(self.height)..y_end {
            for px in x.min(self.width)..x_end {
                let idx = py as usize * self.width as usize + px as usize;
                if idx < self.mask.len() {
                    self.mask[idx] = true;
                }
            }
        }
    }

    /// Mark all pixels within `radius` of `(cx, cy)` as ROI (filled circle).
    ///
    /// Uses integer arithmetic (`dx²+dy² <= radius²`) to avoid floating-point
    /// rounding differences.
    pub fn add_circle(&mut self, cx: u32, cy: u32, radius: u32) {
        if radius == 0 {
            // Mark just the centre pixel
            if cx < self.width && cy < self.height {
                let idx = cy as usize * self.width as usize + cx as usize;
                if idx < self.mask.len() {
                    self.mask[idx] = true;
                }
            }
            return;
        }
        let r2 = radius as i64 * radius as i64;
        let x_lo = cx.saturating_sub(radius);
        let x_hi = (cx + radius + 1).min(self.width);
        let y_lo = cy.saturating_sub(radius);
        let y_hi = (cy + radius + 1).min(self.height);

        for py in y_lo..y_hi {
            for px in x_lo..x_hi {
                let dx = px as i64 - cx as i64;
                let dy = py as i64 - cy as i64;
                if dx * dx + dy * dy <= r2 {
                    let idx = py as usize * self.width as usize + px as usize;
                    if idx < self.mask.len() {
                        self.mask[idx] = true;
                    }
                }
            }
        }
    }

    /// Return `true` if any pixel in the code block `[x, x+w) × [y, y+h)` is ROI.
    pub fn block_in_roi(&self, x: u32, y: u32, w: u32, h: u32) -> bool {
        let x_end = (x + w).min(self.width);
        let y_end = (y + h).min(self.height);
        for py in y.min(self.height)..y_end {
            for px in x.min(self.width)..x_end {
                let idx = py as usize * self.width as usize + px as usize;
                if idx < self.mask.len() && self.mask[idx] {
                    return true;
                }
            }
        }
        false
    }

    /// Return the shift value for a code block.
    ///
    /// Returns `self.shift` if the block is inside the ROI, or `0` otherwise.
    pub fn block_shift(&self, x: u32, y: u32, w: u32, h: u32) -> u8 {
        if self.block_in_roi(x, y, w, h) {
            self.shift
        } else {
            0
        }
    }

    /// Direct read access to the flat pixel mask.
    pub fn mask(&self) -> &[bool] {
        &self.mask
    }

    /// Image width.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Image height.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Configured ROI shift.
    pub fn shift(&self) -> u8 {
        self.shift
    }

    /// Count the number of ROI pixels.
    pub fn roi_pixel_count(&self) -> usize {
        self.mask.iter().filter(|&&b| b).count()
    }

    /// Return `true` if the mask is entirely background (no ROI pixels).
    pub fn is_empty(&self) -> bool {
        self.mask.iter().all(|&b| !b)
    }
}

// ---------------------------------------------------------------------------
// RoiShift
// ---------------------------------------------------------------------------

/// Applies and removes the MaxShift ROI coefficient scaling.
///
/// ROI pixels are upshifted (multiplied by `2^shift`) during encoding so that
/// quantisation preserves them at higher quality.  The decoder identifies these
/// coefficients by their elevated magnitude and shifts them back down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoiShift {
    /// Upshift amount (0–31).
    pub shift: u8,
}

impl RoiShift {
    /// Create a new `RoiShift`.
    pub fn new(shift: u8) -> Result<Self> {
        if shift > 31 {
            return Err(Jpeg2000Error::Other(
                "ROI shift must be in range 0..=31".to_string(),
            ));
        }
        Ok(Self { shift })
    }

    /// Apply the upshift to coefficients that have the corresponding mask pixel set.
    ///
    /// Each `coeffs[i]` with `roi_mask[i] == true` is multiplied by `2^shift`.
    /// Saturation arithmetic prevents overflow.
    ///
    /// # Errors
    /// Returns an error if `coeffs` and `roi_mask` have different lengths.
    pub fn apply_upshift(&self, coeffs: &mut [i32], roi_mask: &[bool]) -> Result<()> {
        if coeffs.len() != roi_mask.len() {
            return Err(Jpeg2000Error::WaveletError(format!(
                "Buffer length mismatch: coeffs={} roi_mask={}",
                coeffs.len(),
                roi_mask.len()
            )));
        }
        if self.shift == 0 {
            return Ok(());
        }
        let scale = 1i32.checked_shl(self.shift as u32).unwrap_or(i32::MAX);
        for (c, &in_roi) in coeffs.iter_mut().zip(roi_mask.iter()) {
            if in_roi {
                *c = c.saturating_mul(scale);
            }
        }
        Ok(())
    }

    /// Remove the upshift from coefficients that have the corresponding mask pixel set.
    ///
    /// Each `coeffs[i]` with `roi_mask[i] == true` is right-shifted by `shift` bits
    /// (arithmetic shift, preserving sign).
    ///
    /// # Errors
    /// Returns an error if `coeffs` and `roi_mask` have different lengths.
    pub fn remove_upshift(&self, coeffs: &mut [i32], roi_mask: &[bool]) -> Result<()> {
        if coeffs.len() != roi_mask.len() {
            return Err(Jpeg2000Error::WaveletError(format!(
                "Buffer length mismatch: coeffs={} roi_mask={}",
                coeffs.len(),
                roi_mask.len()
            )));
        }
        if self.shift == 0 {
            return Ok(());
        }
        for (c, &in_roi) in coeffs.iter_mut().zip(roi_mask.iter()) {
            if in_roi {
                *c >>= self.shift;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // RoiMap tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_roi_map_new_is_empty() {
        let map = RoiMap::new(16, 16, 4);
        assert!(map.is_empty());
        assert_eq!(map.roi_pixel_count(), 0);
    }

    #[test]
    fn test_roi_map_add_rect() {
        let mut map = RoiMap::new(8, 8, 3);
        map.add_rect(2, 2, 4, 4);
        assert_eq!(map.roi_pixel_count(), 16); // 4x4
        assert!(map.mask()[2 * 8 + 2]); // (2,2)
        assert!(map.mask()[5 * 8 + 5]); // (5,5)
        assert!(!map.mask()[0]); // (0,0)
    }

    #[test]
    fn test_roi_map_add_rect_out_of_bounds_clip() {
        let mut map = RoiMap::new(4, 4, 2);
        // Rectangle starting inside but extending beyond boundary
        map.add_rect(2, 2, 10, 10);
        // Only the 2x2 area [2..4,2..4] is valid
        assert_eq!(map.roi_pixel_count(), 4);
    }

    #[test]
    fn test_roi_map_add_circle_radius1() {
        let mut map = RoiMap::new(10, 10, 5);
        map.add_circle(5, 5, 1);
        // Centre + up to 4 neighbours with dx²+dy²<=1: (5,5),(4,5),(6,5),(5,4),(5,6) = 5
        assert_eq!(map.roi_pixel_count(), 5);
        assert!(map.mask()[5 * 10 + 5]); // centre
    }

    #[test]
    fn test_roi_map_add_circle_radius0() {
        let mut map = RoiMap::new(8, 8, 5);
        map.add_circle(3, 3, 0);
        assert_eq!(map.roi_pixel_count(), 1);
        assert!(map.mask()[3 * 8 + 3]);
    }

    #[test]
    fn test_roi_map_block_in_roi_true() {
        let mut map = RoiMap::new(16, 16, 4);
        map.add_rect(4, 4, 8, 8);
        assert!(map.block_in_roi(4, 4, 4, 4));
        assert!(map.block_in_roi(8, 8, 4, 4));
        assert!(map.block_in_roi(0, 0, 16, 16)); // covers the ROI
    }

    #[test]
    fn test_roi_map_block_in_roi_false() {
        let mut map = RoiMap::new(16, 16, 4);
        map.add_rect(8, 8, 4, 4);
        assert!(!map.block_in_roi(0, 0, 4, 4));
        assert!(!map.block_in_roi(4, 4, 4, 4));
    }

    #[test]
    fn test_roi_map_block_shift() {
        let mut map = RoiMap::new(16, 16, 7);
        map.add_rect(0, 0, 4, 4);
        assert_eq!(map.block_shift(0, 0, 4, 4), 7);
        assert_eq!(map.block_shift(8, 8, 4, 4), 0);
    }

    #[test]
    fn test_roi_map_overlap_adds_once() {
        let mut map = RoiMap::new(4, 4, 1);
        map.add_rect(0, 0, 2, 2);
        map.add_rect(0, 0, 2, 2); // same region again
        assert_eq!(map.roi_pixel_count(), 4); // not 8
    }

    #[test]
    fn test_roi_map_dimensions() {
        let map = RoiMap::new(100, 200, 3);
        assert_eq!(map.width(), 100);
        assert_eq!(map.height(), 200);
        assert_eq!(map.shift(), 3);
    }

    // -----------------------------------------------------------------------
    // RoiShift tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_roi_shift_new_valid() {
        assert!(RoiShift::new(0).is_ok());
        assert!(RoiShift::new(31).is_ok());
    }

    #[test]
    fn test_roi_shift_new_invalid() {
        assert!(RoiShift::new(32).is_err());
        assert!(RoiShift::new(255).is_err());
    }

    #[test]
    fn test_roi_shift_apply_upshift() {
        let rs = RoiShift::new(3).expect("create roi shift 3"); // shift by 3 → ×8
        let mut coeffs = vec![1i32, 2, 3, 4];
        let mask = vec![true, false, true, false];
        rs.apply_upshift(&mut coeffs, &mask).expect("apply upshift");
        assert_eq!(coeffs, vec![8, 2, 24, 4]);
    }

    #[test]
    fn test_roi_shift_remove_upshift() {
        let rs = RoiShift::new(3).expect("create roi shift 3");
        let mut coeffs = vec![8i32, 2, 24, 4];
        let mask = vec![true, false, true, false];
        rs.remove_upshift(&mut coeffs, &mask)
            .expect("remove upshift");
        assert_eq!(coeffs, vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_roi_shift_round_trip() {
        let rs = RoiShift::new(4).expect("create roi shift 4");
        let original = vec![10i32, 20, 30, 40, 50];
        let mask = vec![true, false, true, true, false];
        let mut coeffs = original.clone();

        rs.apply_upshift(&mut coeffs, &mask)
            .expect("apply upshift round trip");
        rs.remove_upshift(&mut coeffs, &mask)
            .expect("remove upshift round trip");

        assert_eq!(coeffs, original);
    }

    #[test]
    fn test_roi_shift_zero_shift_noop() {
        let rs = RoiShift::new(0).expect("create roi shift 0");
        let mut coeffs = vec![5i32, 10, 15];
        let original = coeffs.clone();
        let mask = vec![true, true, true];
        rs.apply_upshift(&mut coeffs, &mask)
            .expect("apply zero upshift");
        assert_eq!(coeffs, original);
        rs.remove_upshift(&mut coeffs, &mask)
            .expect("remove zero upshift");
        assert_eq!(coeffs, original);
    }

    #[test]
    fn test_roi_shift_length_mismatch() {
        let rs = RoiShift::new(2).expect("create roi shift 2");
        let mut coeffs = vec![1i32, 2, 3];
        let mask = vec![true, false]; // shorter
        assert!(rs.apply_upshift(&mut coeffs, &mask).is_err());
        assert!(rs.remove_upshift(&mut coeffs, &mask).is_err());
    }

    #[test]
    fn test_roi_shift_negative_coefficients() {
        let rs = RoiShift::new(2).expect("create roi shift 2 for negatives");
        let mut coeffs = vec![-4i32, -8, 4];
        let mask = vec![true, true, true];
        rs.apply_upshift(&mut coeffs, &mask)
            .expect("apply upshift to negatives");
        assert_eq!(coeffs, vec![-16, -32, 16]);
        rs.remove_upshift(&mut coeffs, &mask)
            .expect("remove upshift from negatives");
        assert_eq!(coeffs, vec![-4, -8, 4]);
    }
}
