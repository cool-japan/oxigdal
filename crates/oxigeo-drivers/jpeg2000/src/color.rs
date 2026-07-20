//! Color space conversions
//!
//! This module handles color space conversions for JPEG2000 images.

use crate::error::{Jpeg2000Error, Result};
use crate::metadata::EnumeratedColorSpace;

/// Color converter for JP2 images
pub struct ColorConverter {
    /// Source color space
    source: EnumeratedColorSpace,
    /// Number of components
    num_components: usize,
}

impl ColorConverter {
    /// Create new color converter
    pub fn new(source: EnumeratedColorSpace, num_components: usize) -> Self {
        Self {
            source,
            num_components,
        }
    }

    /// Convert to RGB
    pub fn to_rgb(&self, components: &[Vec<u8>]) -> Result<Vec<u8>> {
        if components.len() != self.num_components {
            return Err(Jpeg2000Error::ColorError(format!(
                "Expected {} components, got {}",
                self.num_components,
                components.len()
            )));
        }

        match self.source {
            EnumeratedColorSpace::Srgb => self.srgb_to_rgb(components),
            EnumeratedColorSpace::Grayscale => self.grayscale_to_rgb(components),
            EnumeratedColorSpace::Sycc => self.sycc_to_rgb(components),
            EnumeratedColorSpace::Custom(_) => Err(Jpeg2000Error::UnsupportedFeature(
                "Custom color spaces not yet supported".to_string(),
            )),
        }
    }

    /// sRGB to RGB (identity transform)
    fn srgb_to_rgb(&self, components: &[Vec<u8>]) -> Result<Vec<u8>> {
        if components.len() < 3 {
            return Err(Jpeg2000Error::ColorError(
                "sRGB requires at least 3 components".to_string(),
            ));
        }

        let num_pixels = components[0].len();
        let mut rgb = Vec::with_capacity(num_pixels * 3);

        for ((&r, &g), &b) in components[0].iter().zip(&components[1]).zip(&components[2]) {
            rgb.push(r); // R
            rgb.push(g); // G
            rgb.push(b); // B
        }

        Ok(rgb)
    }

    /// Grayscale to RGB
    fn grayscale_to_rgb(&self, components: &[Vec<u8>]) -> Result<Vec<u8>> {
        if components.is_empty() {
            return Err(Jpeg2000Error::ColorError(
                "Grayscale requires at least 1 component".to_string(),
            ));
        }

        let num_pixels = components[0].len();
        let mut rgb = Vec::with_capacity(num_pixels * 3);

        for &gray in &components[0] {
            rgb.push(gray); // R
            rgb.push(gray); // G
            rgb.push(gray); // B
        }

        Ok(rgb)
    }

    /// sYCC to RGB conversion
    fn sycc_to_rgb(&self, components: &[Vec<u8>]) -> Result<Vec<u8>> {
        if components.len() < 3 {
            return Err(Jpeg2000Error::ColorError(
                "sYCC requires at least 3 components".to_string(),
            ));
        }

        let num_pixels = components[0].len();
        let mut rgb = Vec::with_capacity(num_pixels * 3);

        for ((&y_val, &cb_val), &cr_val) in
            components[0].iter().zip(&components[1]).zip(&components[2])
        {
            let y = f32::from(y_val);
            let cb = f32::from(cb_val) - 128.0;
            let cr = f32::from(cr_val) - 128.0;

            // YCbCr to RGB conversion
            let r = y + 1.402 * cr;
            let g = y - 0.344136 * cb - 0.714136 * cr;
            let b = y + 1.772 * cb;

            // Clamp to [0, 255]
            rgb.push(r.clamp(0.0, 255.0) as u8);
            rgb.push(g.clamp(0.0, 255.0) as u8);
            rgb.push(b.clamp(0.0, 255.0) as u8);
        }

        Ok(rgb)
    }

    /// Get RGBA (adding alpha channel if present)
    pub fn to_rgba(&self, components: &[Vec<u8>]) -> Result<Vec<u8>> {
        let rgb = self.to_rgb(components)?;
        let num_pixels = rgb.len() / 3;

        let mut rgba = Vec::with_capacity(num_pixels * 4);

        // Check if alpha channel exists (4th component)
        let has_alpha = components.len() >= 4;

        for i in 0..num_pixels {
            rgba.push(rgb[i * 3]); // R
            rgba.push(rgb[i * 3 + 1]); // G
            rgba.push(rgb[i * 3 + 2]); // B

            if has_alpha && i < components[3].len() {
                rgba.push(components[3][i]); // A
            } else {
                rgba.push(255); // Opaque
            }
        }

        Ok(rgba)
    }

    /// Apply component transformations (MCT - Multiple Component Transform)
    pub fn apply_mct(components: &mut [Vec<i32>]) -> Result<()> {
        if components.len() < 3 {
            return Err(Jpeg2000Error::ColorError(
                "MCT requires at least 3 components".to_string(),
            ));
        }

        let (first, rest) = components.split_at_mut(1);
        let (second, third) = rest.split_at_mut(1);

        for ((r, g), b) in first[0]
            .iter_mut()
            .zip(second[0].iter_mut())
            .zip(third[0].iter_mut())
        {
            let y = *r;
            let cb = *g;
            let cr = *b;

            // Inverse Irreversible MCT (ICT)
            // R = Y + 1.402 * Cr
            // G = Y - 0.34413 * Cb - 0.71414 * Cr
            // B = Y + 1.772 * Cb

            let r_new = y + ((1.402 * f64::from(cr)) as i32);
            let g_new = y - ((0.34413 * f64::from(cb)) as i32) - ((0.71414 * f64::from(cr)) as i32);
            let b_new = y + ((1.772 * f64::from(cb)) as i32);

            *r = r_new;
            *g = g_new;
            *b = b_new;
        }

        Ok(())
    }

    /// Apply reversible MCT (RCT)
    pub fn apply_rct(components: &mut [Vec<i32>]) -> Result<()> {
        if components.len() < 3 {
            return Err(Jpeg2000Error::ColorError(
                "RCT requires at least 3 components".to_string(),
            ));
        }

        let (first, rest) = components.split_at_mut(1);
        let (second, third) = rest.split_at_mut(1);

        for ((r, g), b) in first[0]
            .iter_mut()
            .zip(second[0].iter_mut())
            .zip(third[0].iter_mut())
        {
            let y = *r;
            let cb = *g;
            let cr = *b;

            // Inverse Reversible MCT (RCT)
            // G = Y - floor((Cb + Cr) / 4)
            // R = Cr + G
            // B = Cb + G

            let g_new = y - ((cb + cr) >> 2);
            let r_new = cr + g_new;
            let b_new = cb + g_new;

            *r = r_new;
            *g = g_new;
            *b = b_new;
        }

        Ok(())
    }
}

/// Level shift to convert signed to unsigned
pub fn level_shift(data: &[i32], precision: u8, is_signed: bool) -> Vec<u8> {
    let shift = if is_signed { 1 << (precision - 1) } else { 0 };

    let max_val = (1 << precision) - 1;

    data.iter()
        .map(|&val| {
            let shifted = val + shift;
            shifted.clamp(0, max_val) as u8
        })
        .collect()
}

/// Dequantization for lossy compression
pub fn dequantize(coefficients: &[i32], step_size: f32) -> Vec<f32> {
    coefficients
        .iter()
        .map(|&c| (c as f32) * step_size)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grayscale_to_rgb() {
        let converter = ColorConverter::new(EnumeratedColorSpace::Grayscale, 1);
        let gray = vec![vec![128u8, 64, 192]];

        let rgb = converter.to_rgb(&gray).expect("conversion failed");

        assert_eq!(rgb.len(), 9); // 3 pixels * 3 channels
        assert_eq!(rgb[0], 128);
        assert_eq!(rgb[1], 128);
        assert_eq!(rgb[2], 128);
    }

    #[test]
    fn test_srgb_to_rgb() {
        let converter = ColorConverter::new(EnumeratedColorSpace::Srgb, 3);
        let components = vec![
            vec![255u8, 128], // R
            vec![0, 64],      // G
            vec![127, 192],   // B
        ];

        let rgb = converter.to_rgb(&components).expect("conversion failed");

        assert_eq!(rgb.len(), 6); // 2 pixels * 3 channels
        assert_eq!(rgb[0], 255);
        assert_eq!(rgb[1], 0);
        assert_eq!(rgb[2], 127);
    }

    #[test]
    fn test_level_shift() {
        let data = vec![-128, 0, 127];
        let result = level_shift(&data, 8, true);

        assert_eq!(result[0], 0);
        assert_eq!(result[1], 128);
        assert_eq!(result[2], 255);
    }

    #[test]
    fn test_dequantize() {
        let coefficients = vec![10, 20, 30];
        let step_size = 0.5;

        let result = dequantize(&coefficients, step_size);

        assert_eq!(result[0], 5.0);
        assert_eq!(result[1], 10.0);
        assert_eq!(result[2], 15.0);
    }

    #[test]
    fn test_to_rgba() {
        let converter = ColorConverter::new(EnumeratedColorSpace::Srgb, 4);
        let components = vec![
            vec![255u8], // R
            vec![0],     // G
            vec![127],   // B
            vec![200],   // A
        ];

        let rgba = converter.to_rgba(&components).expect("conversion failed");

        assert_eq!(rgba.len(), 4);
        assert_eq!(rgba[0], 255); // R
        assert_eq!(rgba[1], 0); // G
        assert_eq!(rgba[2], 127); // B
        assert_eq!(rgba[3], 200); // A
    }
}
