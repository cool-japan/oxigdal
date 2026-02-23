//! Geographic transformation (affine transform) for raster georeferencing
//!
//! This module provides the [`GeoTransform`] type which defines the relationship
//! between pixel coordinates and geographic/projected coordinates.

use core::fmt;

use serde::{Deserialize, Serialize};

use crate::error::{OxiGdalError, Result};
use crate::types::BoundingBox;

/// An affine transformation matrix for converting between pixel and world coordinates
///
/// The transformation follows the GDAL convention:
/// ```text
/// x_geo = c0 + pixel_x * c1 + pixel_y * c2
/// y_geo = c3 + pixel_x * c4 + pixel_y * c5
/// ```
///
/// For a north-up image:
/// - `c1` (pixel width) is positive
/// - `c5` (pixel height) is negative (Y increases downward in pixel space)
/// - `c2` and `c4` (rotation terms) are zero
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GeoTransform {
    /// X coordinate of the upper-left corner of the upper-left pixel (origin X)
    pub origin_x: f64,
    /// W-E pixel resolution (pixel width)
    pub pixel_width: f64,
    /// Row rotation (typically zero for north-up images)
    pub row_rotation: f64,
    /// Y coordinate of the upper-left corner of the upper-left pixel (origin Y)
    pub origin_y: f64,
    /// Column rotation (typically zero for north-up images)
    pub col_rotation: f64,
    /// N-S pixel resolution (pixel height, typically negative)
    pub pixel_height: f64,
}

impl GeoTransform {
    /// Creates a new `GeoTransform` from the six coefficients
    ///
    /// # Arguments
    /// * `origin_x` - X coordinate of the upper-left corner
    /// * `pixel_width` - W-E pixel resolution
    /// * `row_rotation` - Row rotation (0 for north-up)
    /// * `origin_y` - Y coordinate of the upper-left corner
    /// * `col_rotation` - Column rotation (0 for north-up)
    /// * `pixel_height` - N-S pixel resolution (negative for north-up)
    #[must_use]
    pub const fn new(
        origin_x: f64,
        pixel_width: f64,
        row_rotation: f64,
        origin_y: f64,
        col_rotation: f64,
        pixel_height: f64,
    ) -> Self {
        Self {
            origin_x,
            pixel_width,
            row_rotation,
            origin_y,
            col_rotation,
            pixel_height,
        }
    }

    /// Creates a north-up `GeoTransform` (no rotation)
    ///
    /// # Arguments
    /// * `origin_x` - X coordinate of the upper-left corner
    /// * `origin_y` - Y coordinate of the upper-left corner
    /// * `pixel_width` - Pixel width (positive)
    /// * `pixel_height` - Pixel height (negative for north-up)
    #[must_use]
    pub const fn north_up(
        origin_x: f64,
        origin_y: f64,
        pixel_width: f64,
        pixel_height: f64,
    ) -> Self {
        Self {
            origin_x,
            pixel_width,
            row_rotation: 0.0,
            origin_y,
            col_rotation: 0.0,
            pixel_height,
        }
    }

    /// Creates a `GeoTransform` from the standard GDAL 6-element array
    ///
    /// The array format is: `[origin_x, pixel_width, row_rotation, origin_y, col_rotation, pixel_height]`
    #[must_use]
    pub const fn from_gdal_array(coeffs: [f64; 6]) -> Self {
        Self {
            origin_x: coeffs[0],
            pixel_width: coeffs[1],
            row_rotation: coeffs[2],
            origin_y: coeffs[3],
            col_rotation: coeffs[4],
            pixel_height: coeffs[5],
        }
    }

    /// Converts to the standard GDAL 6-element array
    #[must_use]
    pub const fn to_gdal_array(&self) -> [f64; 6] {
        [
            self.origin_x,
            self.pixel_width,
            self.row_rotation,
            self.origin_y,
            self.col_rotation,
            self.pixel_height,
        ]
    }

    /// Creates an identity transform (pixel = world)
    #[must_use]
    pub const fn identity() -> Self {
        Self::north_up(0.0, 0.0, 1.0, -1.0)
    }

    /// Creates a `GeoTransform` from a bounding box and image dimensions
    ///
    /// # Arguments
    /// * `bbox` - The bounding box of the image
    /// * `width` - Image width in pixels
    /// * `height` - Image height in pixels
    ///
    /// # Errors
    /// Returns an error if width or height is zero
    pub fn from_bounds(bbox: &BoundingBox, width: u64, height: u64) -> Result<Self> {
        if width == 0 {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "width",
                message: "width must be greater than zero".to_string(),
            });
        }
        if height == 0 {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "height",
                message: "height must be greater than zero".to_string(),
            });
        }

        let pixel_width = bbox.width() / width as f64;
        let pixel_height = -bbox.height() / height as f64;

        Ok(Self::north_up(
            bbox.min_x,
            bbox.max_y,
            pixel_width,
            pixel_height,
        ))
    }

    /// Transforms pixel coordinates to world coordinates
    ///
    /// # Arguments
    /// * `pixel_x` - X pixel coordinate (column)
    /// * `pixel_y` - Y pixel coordinate (row)
    ///
    /// # Returns
    /// A tuple of (x, y) world coordinates
    #[must_use]
    pub fn pixel_to_world(&self, pixel_x: f64, pixel_y: f64) -> (f64, f64) {
        let x = self.origin_x + pixel_x * self.pixel_width + pixel_y * self.row_rotation;
        let y = self.origin_y + pixel_x * self.col_rotation + pixel_y * self.pixel_height;
        (x, y)
    }

    /// Transforms world coordinates to pixel coordinates
    ///
    /// # Arguments
    /// * `world_x` - X world coordinate
    /// * `world_y` - Y world coordinate
    ///
    /// # Returns
    /// Ok with a tuple of (`pixel_x`, `pixel_y`), or an error if the transform is singular
    ///
    /// # Errors
    /// Returns an error if the determinant is zero (singular matrix)
    pub fn world_to_pixel(&self, world_x: f64, world_y: f64) -> Result<(f64, f64)> {
        let det = self.pixel_width * self.pixel_height - self.row_rotation * self.col_rotation;

        if det.abs() < f64::EPSILON {
            return Err(OxiGdalError::Internal {
                message: "GeoTransform is singular (determinant is zero)".to_string(),
            });
        }

        let dx = world_x - self.origin_x;
        let dy = world_y - self.origin_y;

        let pixel_x = (self.pixel_height * dx - self.row_rotation * dy) / det;
        let pixel_y = (-self.col_rotation * dx + self.pixel_width * dy) / det;

        Ok((pixel_x, pixel_y))
    }

    /// Returns the center of a pixel in world coordinates
    #[must_use]
    pub fn pixel_center(&self, pixel_x: u64, pixel_y: u64) -> (f64, f64) {
        self.pixel_to_world(pixel_x as f64 + 0.5, pixel_y as f64 + 0.5)
    }

    /// Computes the bounding box for the given raster dimensions
    ///
    /// # Arguments
    /// * `width` - Raster width in pixels
    /// * `height` - Raster height in pixels
    ///
    /// # Returns
    /// The bounding box in world coordinates
    #[must_use]
    pub fn compute_bounds(&self, width: u64, height: u64) -> BoundingBox {
        // Get the four corners
        let (x0, y0) = self.pixel_to_world(0.0, 0.0);
        let (x1, y1) = self.pixel_to_world(width as f64, 0.0);
        let (x2, y2) = self.pixel_to_world(0.0, height as f64);
        let (x3, y3) = self.pixel_to_world(width as f64, height as f64);

        let min_x = x0.min(x1).min(x2).min(x3);
        let max_x = x0.max(x1).max(x2).max(x3);
        let min_y = y0.min(y1).min(y2).min(y3);
        let max_y = y0.max(y1).max(y2).max(y3);

        BoundingBox::new_unchecked(min_x, min_y, max_x, max_y)
    }

    /// Returns true if this is a north-up transform (no rotation)
    #[must_use]
    pub fn is_north_up(&self) -> bool {
        self.row_rotation.abs() < f64::EPSILON
            && self.col_rotation.abs() < f64::EPSILON
            && self.pixel_height < 0.0
    }

    /// Returns true if the transform has rotation
    #[must_use]
    pub fn has_rotation(&self) -> bool {
        self.row_rotation.abs() >= f64::EPSILON || self.col_rotation.abs() >= f64::EPSILON
    }

    /// Returns the absolute pixel resolution (ignoring sign)
    #[must_use]
    pub fn resolution(&self) -> (f64, f64) {
        (self.pixel_width.abs(), self.pixel_height.abs())
    }

    /// Returns the rotation angle in radians (assuming uniform scaling)
    ///
    /// The rotation is calculated from the affine transformation matrix.
    /// For a standard rotation, this extracts the angle from the `col_rotation`
    /// and `pixel_width` terms.
    #[must_use]
    pub fn rotation_radians(&self) -> f64 {
        // For a rotation matrix applied to GeoTransform:
        // pixel_width = scale_x * cos(θ)
        // col_rotation = scale_x * sin(θ)
        // So θ = atan2(col_rotation, pixel_width)
        self.col_rotation.atan2(self.pixel_width)
    }

    /// Returns the rotation angle in degrees
    #[must_use]
    pub fn rotation_degrees(&self) -> f64 {
        self.rotation_radians().to_degrees()
    }

    /// Creates the inverse transform
    ///
    /// # Errors
    /// Returns an error if the transform is singular
    pub fn inverse(&self) -> Result<Self> {
        let det = self.pixel_width * self.pixel_height - self.row_rotation * self.col_rotation;

        if det.abs() < f64::EPSILON {
            return Err(OxiGdalError::Internal {
                message: "GeoTransform is singular (cannot invert)".to_string(),
            });
        }

        let inv_det = 1.0 / det;

        // Compute inverse of the 2x2 linear part
        let inv_pixel_width = self.pixel_height * inv_det;
        let inv_row_rotation = -self.row_rotation * inv_det;
        let inv_col_rotation = -self.col_rotation * inv_det;
        let inv_pixel_height = self.pixel_width * inv_det;

        // Compute new origin
        let inv_origin_x = -inv_pixel_width * self.origin_x - inv_row_rotation * self.origin_y;
        let inv_origin_y = -inv_col_rotation * self.origin_x - inv_pixel_height * self.origin_y;

        Ok(Self {
            origin_x: inv_origin_x,
            pixel_width: inv_pixel_width,
            row_rotation: inv_row_rotation,
            origin_y: inv_origin_y,
            col_rotation: inv_col_rotation,
            pixel_height: inv_pixel_height,
        })
    }

    /// Composes two transforms: self followed by other
    ///
    /// The result transforms from pixel space through self to world space,
    /// then from world space through other.
    #[must_use]
    pub fn compose(&self, other: &Self) -> Self {
        // For affine transforms:
        // [a1 b1 c1]   [a2 b2 c2]
        // [d1 e1 f1] * [d2 e2 f2]
        // [0  0  1 ]   [0  0  1 ]

        let a = self
            .pixel_width
            .mul_add(other.pixel_width, self.row_rotation * other.col_rotation);
        let b = self
            .pixel_width
            .mul_add(other.row_rotation, self.row_rotation * other.pixel_height);
        let c = self.origin_x.mul_add(
            other.pixel_width,
            self.origin_y.mul_add(other.col_rotation, other.origin_x),
        );

        let d = self
            .col_rotation
            .mul_add(other.pixel_width, self.pixel_height * other.col_rotation);
        let e = self
            .col_rotation
            .mul_add(other.row_rotation, self.pixel_height * other.pixel_height);
        let f = self.origin_x.mul_add(
            other.row_rotation,
            self.origin_y.mul_add(other.pixel_height, other.origin_y),
        );

        Self {
            origin_x: c,
            pixel_width: a,
            row_rotation: b,
            origin_y: f,
            col_rotation: d,
            pixel_height: e,
        }
    }

    /// Scales the transform by the given factors
    #[must_use]
    pub const fn scale(&self, scale_x: f64, scale_y: f64) -> Self {
        Self {
            origin_x: self.origin_x,
            pixel_width: self.pixel_width * scale_x,
            row_rotation: self.row_rotation * scale_x,
            origin_y: self.origin_y,
            col_rotation: self.col_rotation * scale_y,
            pixel_height: self.pixel_height * scale_y,
        }
    }

    /// Translates the transform by the given offset
    #[must_use]
    pub const fn translate(&self, offset_x: f64, offset_y: f64) -> Self {
        Self {
            origin_x: self.origin_x + offset_x,
            pixel_width: self.pixel_width,
            row_rotation: self.row_rotation,
            origin_y: self.origin_y + offset_y,
            col_rotation: self.col_rotation,
            pixel_height: self.pixel_height,
        }
    }
}

impl Default for GeoTransform {
    fn default() -> Self {
        Self::identity()
    }
}

impl fmt::Display for GeoTransform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "GeoTransform(origin=({}, {}), size=({}, {}), rotation=({}, {}))",
            self.origin_x,
            self.origin_y,
            self.pixel_width,
            self.pixel_height,
            self.row_rotation,
            self.col_rotation
        )
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    #![allow(clippy::float_cmp)]

    use super::*;

    const EPSILON: f64 = 1e-10;

    fn approx_eq(a: f64, b: f64) -> bool {
        (a - b).abs() < EPSILON
    }

    #[test]
    fn test_north_up_transform() {
        // A simple 1x1 degree/pixel transform starting at (0, 90)
        let gt = GeoTransform::north_up(0.0, 90.0, 1.0, -1.0);

        assert!(gt.is_north_up());
        assert!(!gt.has_rotation());

        let (x, y) = gt.pixel_to_world(0.0, 0.0);
        assert!(approx_eq(x, 0.0));
        assert!(approx_eq(y, 90.0));

        let (x, y) = gt.pixel_to_world(180.0, 90.0);
        assert!(approx_eq(x, 180.0));
        assert!(approx_eq(y, 0.0));
    }

    #[test]
    fn test_pixel_world_roundtrip() {
        let gt = GeoTransform::north_up(-180.0, 90.0, 0.1, -0.1);

        for px in [0.0, 10.0, 100.0, 500.0] {
            for py in [0.0, 10.0, 100.0, 500.0] {
                let (wx, wy) = gt.pixel_to_world(px, py);
                let (rpx, rpy) = gt.world_to_pixel(wx, wy).expect("inverse should work");
                assert!(
                    approx_eq(px, rpx),
                    "pixel_x roundtrip failed: {px} -> {rpx}"
                );
                assert!(
                    approx_eq(py, rpy),
                    "pixel_y roundtrip failed: {py} -> {rpy}"
                );
            }
        }
    }

    #[test]
    fn test_from_bounds() {
        let bbox = BoundingBox::new(-180.0, -90.0, 180.0, 90.0).expect("valid bbox");
        let gt = GeoTransform::from_bounds(&bbox, 360, 180).expect("valid transform");

        assert!(approx_eq(gt.pixel_width, 1.0));
        assert!(approx_eq(gt.pixel_height, -1.0));
        assert!(approx_eq(gt.origin_x, -180.0));
        assert!(approx_eq(gt.origin_y, 90.0));

        let bounds = gt.compute_bounds(360, 180);
        assert!(approx_eq(bounds.min_x, bbox.min_x));
        assert!(approx_eq(bounds.max_x, bbox.max_x));
        assert!(approx_eq(bounds.min_y, bbox.min_y));
        assert!(approx_eq(bounds.max_y, bbox.max_y));
    }

    #[test]
    fn test_rotated_transform() {
        // 45-degree rotation
        let angle = core::f64::consts::PI / 4.0;
        let cos_a = angle.cos();
        let sin_a = angle.sin();

        let gt = GeoTransform::new(0.0, cos_a, -sin_a, 0.0, sin_a, cos_a);

        assert!(gt.has_rotation());
        assert!(!gt.is_north_up());

        // Verify rotation angle
        assert!(approx_eq(gt.rotation_degrees(), 45.0));
    }

    #[test]
    fn test_inverse() {
        let gt = GeoTransform::north_up(100.0, 200.0, 0.5, -0.5);
        let inv = gt.inverse().expect("should be invertible");

        // Verify that compose gives identity-like behavior
        let px = 50.0;
        let py = 75.0;
        let (wx, wy) = gt.pixel_to_world(px, py);
        let (rpx, rpy) = inv.pixel_to_world(wx, wy);

        assert!(approx_eq(rpx, px));
        assert!(approx_eq(rpy, py));
    }

    #[test]
    fn test_singular_transform() {
        // A singular transform (zero determinant)
        let gt = GeoTransform::new(0.0, 1.0, 2.0, 0.0, 0.5, 1.0);
        // det = 1 * 1 - 2 * 0.5 = 0

        assert!(gt.inverse().is_err());
        assert!(gt.world_to_pixel(10.0, 10.0).is_err());
    }

    #[test]
    fn test_pixel_center() {
        let gt = GeoTransform::north_up(0.0, 100.0, 10.0, -10.0);

        let (cx, cy) = gt.pixel_center(0, 0);
        assert!(approx_eq(cx, 5.0));
        assert!(approx_eq(cy, 95.0));
    }

    #[test]
    fn test_gdal_array_roundtrip() {
        let original = GeoTransform::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
        let array = original.to_gdal_array();
        let recovered = GeoTransform::from_gdal_array(array);

        assert_eq!(original, recovered);
    }

    #[test]
    fn test_scale_and_translate() {
        let gt = GeoTransform::north_up(0.0, 0.0, 1.0, -1.0);

        let scaled = gt.scale(2.0, 2.0);
        assert!(approx_eq(scaled.pixel_width, 2.0));
        assert!(approx_eq(scaled.pixel_height, -2.0));

        let translated = gt.translate(10.0, 20.0);
        assert!(approx_eq(translated.origin_x, 10.0));
        assert!(approx_eq(translated.origin_y, 20.0));
    }
}
