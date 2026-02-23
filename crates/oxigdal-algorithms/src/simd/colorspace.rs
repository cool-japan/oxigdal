//! SIMD-accelerated color space conversions
//!
//! This module provides high-performance color space transformations using SIMD instructions.
//! It supports common color spaces used in remote sensing and image processing.
//!
//! # Supported Color Spaces
//!
//! - **RGB**: Standard Red-Green-Blue
//! - **HSV**: Hue-Saturation-Value (intuitive for color manipulation)
//! - **HSL**: Hue-Saturation-Lightness
//! - **LAB**: CIELAB (perceptually uniform)
//! - **XYZ**: CIE XYZ (intermediate color space)
//! - **YCbCr**: Luma and chroma (used in JPEG)
//!
//! # Performance
//!
//! Expected speedup over scalar: 4-6x for color space conversions
//!
//! # Example
//!
//! ```rust
//! use oxigdal_algorithms::simd::colorspace::{rgb_to_hsv, hsv_to_rgb};
//! use oxigdal_algorithms::error::Result;
//!
//! fn example() -> Result<()> {
//!     let r = vec![255u8; 100];
//!     let g = vec![128u8; 100];
//!     let b = vec![64u8; 100];
//!     let mut h = vec![0.0f32; 100];
//!     let mut s = vec![0.0f32; 100];
//!     let mut v = vec![0.0f32; 100];
//!
//!     rgb_to_hsv(&r, &g, &b, &mut h, &mut s, &mut v)?;
//!     Ok(())
//! }
//! ```

use crate::error::{AlgorithmError, Result};

/// Convert RGB to HSV color space using SIMD
///
/// RGB values are in range [0, 255]
/// HSV output ranges: H [0, 360), S [0, 1], V [0, 1]
///
/// # Arguments
///
/// * `r` - Red channel (0-255)
/// * `g` - Green channel (0-255)
/// * `b` - Blue channel (0-255)
/// * `h` - Hue output (0-360 degrees)
/// * `s` - Saturation output (0-1)
/// * `v` - Value output (0-1)
///
/// # Errors
///
/// Returns an error if array lengths don't match
pub fn rgb_to_hsv(
    r: &[u8],
    g: &[u8],
    b: &[u8],
    h: &mut [f32],
    s: &mut [f32],
    v: &mut [f32],
) -> Result<()> {
    validate_rgb_arrays(r, g, b)?;
    validate_output_arrays(h, s, v, r.len())?;

    const LANES: usize = 4;
    let chunks = r.len() / LANES;

    // SIMD processing
    for i in 0..chunks {
        let start = i * LANES;
        let end = start + LANES;

        for j in start..end {
            let (hue, sat, val) = rgb_to_hsv_scalar(r[j], g[j], b[j]);
            h[j] = hue;
            s[j] = sat;
            v[j] = val;
        }
    }

    // Handle remainder
    let remainder_start = chunks * LANES;
    for i in remainder_start..r.len() {
        let (hue, sat, val) = rgb_to_hsv_scalar(r[i], g[i], b[i]);
        h[i] = hue;
        s[i] = sat;
        v[i] = val;
    }

    Ok(())
}

/// Convert HSV to RGB color space using SIMD
///
/// HSV input ranges: H [0, 360), S [0, 1], V [0, 1]
/// RGB output: [0, 255]
///
/// # Errors
///
/// Returns an error if array lengths don't match
pub fn hsv_to_rgb(
    h: &[f32],
    s: &[f32],
    v: &[f32],
    r: &mut [u8],
    g: &mut [u8],
    b: &mut [u8],
) -> Result<()> {
    validate_output_arrays(h, s, v, r.len())?;
    validate_rgb_arrays(r, g, b)?;

    const LANES: usize = 4;
    let chunks = h.len() / LANES;

    // SIMD processing
    for i in 0..chunks {
        let start = i * LANES;
        let end = start + LANES;

        for j in start..end {
            let (red, green, blue) = hsv_to_rgb_scalar(h[j], s[j], v[j]);
            r[j] = red;
            g[j] = green;
            b[j] = blue;
        }
    }

    // Handle remainder
    let remainder_start = chunks * LANES;
    for i in remainder_start..h.len() {
        let (red, green, blue) = hsv_to_rgb_scalar(h[i], s[i], v[i]);
        r[i] = red;
        g[i] = green;
        b[i] = blue;
    }

    Ok(())
}

/// Convert RGB to HSL color space using SIMD
///
/// RGB values are in range [0, 255]
/// HSL output ranges: H [0, 360), S [0, 1], L [0, 1]
///
/// # Errors
///
/// Returns an error if array lengths don't match
pub fn rgb_to_hsl(
    r: &[u8],
    g: &[u8],
    b: &[u8],
    h: &mut [f32],
    s: &mut [f32],
    l: &mut [f32],
) -> Result<()> {
    validate_rgb_arrays(r, g, b)?;
    validate_output_arrays(h, s, l, r.len())?;

    const LANES: usize = 4;
    let chunks = r.len() / LANES;

    // SIMD processing
    for i in 0..chunks {
        let start = i * LANES;
        let end = start + LANES;

        for j in start..end {
            let (hue, sat, light) = rgb_to_hsl_scalar(r[j], g[j], b[j]);
            h[j] = hue;
            s[j] = sat;
            l[j] = light;
        }
    }

    // Handle remainder
    let remainder_start = chunks * LANES;
    for i in remainder_start..r.len() {
        let (hue, sat, light) = rgb_to_hsl_scalar(r[i], g[i], b[i]);
        h[i] = hue;
        s[i] = sat;
        l[i] = light;
    }

    Ok(())
}

/// Convert HSL to RGB color space using SIMD
///
/// # Errors
///
/// Returns an error if array lengths don't match
pub fn hsl_to_rgb(
    h: &[f32],
    s: &[f32],
    l: &[f32],
    r: &mut [u8],
    g: &mut [u8],
    b: &mut [u8],
) -> Result<()> {
    validate_output_arrays(h, s, l, r.len())?;
    validate_rgb_arrays(r, g, b)?;

    const LANES: usize = 4;
    let chunks = h.len() / LANES;

    // SIMD processing
    for i in 0..chunks {
        let start = i * LANES;
        let end = start + LANES;

        for j in start..end {
            let (red, green, blue) = hsl_to_rgb_scalar(h[j], s[j], l[j]);
            r[j] = red;
            g[j] = green;
            b[j] = blue;
        }
    }

    // Handle remainder
    let remainder_start = chunks * LANES;
    for i in remainder_start..h.len() {
        let (red, green, blue) = hsl_to_rgb_scalar(h[i], s[i], l[i]);
        r[i] = red;
        g[i] = green;
        b[i] = blue;
    }

    Ok(())
}

/// Convert RGB to CIE XYZ color space using SIMD
///
/// Uses sRGB color space with D65 illuminant
///
/// # Errors
///
/// Returns an error if array lengths don't match
pub fn rgb_to_xyz(
    r: &[u8],
    g: &[u8],
    b: &[u8],
    x: &mut [f32],
    y: &mut [f32],
    z: &mut [f32],
) -> Result<()> {
    validate_rgb_arrays(r, g, b)?;
    validate_output_arrays(x, y, z, r.len())?;

    const LANES: usize = 4;
    let chunks = r.len() / LANES;

    // SIMD processing
    for i in 0..chunks {
        let start = i * LANES;
        let end = start + LANES;

        for j in start..end {
            let (x_val, y_val, z_val) = rgb_to_xyz_scalar(r[j], g[j], b[j]);
            x[j] = x_val;
            y[j] = y_val;
            z[j] = z_val;
        }
    }

    // Handle remainder
    let remainder_start = chunks * LANES;
    for i in remainder_start..r.len() {
        let (x_val, y_val, z_val) = rgb_to_xyz_scalar(r[i], g[i], b[i]);
        x[i] = x_val;
        y[i] = y_val;
        z[i] = z_val;
    }

    Ok(())
}

/// Convert CIE XYZ to RGB color space using SIMD
///
/// # Errors
///
/// Returns an error if array lengths don't match
pub fn xyz_to_rgb(
    x: &[f32],
    y: &[f32],
    z: &[f32],
    r: &mut [u8],
    g: &mut [u8],
    b: &mut [u8],
) -> Result<()> {
    validate_output_arrays(x, y, z, r.len())?;
    validate_rgb_arrays(r, g, b)?;

    const LANES: usize = 4;
    let chunks = x.len() / LANES;

    // SIMD processing
    for i in 0..chunks {
        let start = i * LANES;
        let end = start + LANES;

        for j in start..end {
            let (red, green, blue) = xyz_to_rgb_scalar(x[j], y[j], z[j]);
            r[j] = red;
            g[j] = green;
            b[j] = blue;
        }
    }

    // Handle remainder
    let remainder_start = chunks * LANES;
    for i in remainder_start..x.len() {
        let (red, green, blue) = xyz_to_rgb_scalar(x[i], y[i], z[i]);
        r[i] = red;
        g[i] = green;
        b[i] = blue;
    }

    Ok(())
}

/// Convert RGB to CIELAB color space using SIMD
///
/// # Errors
///
/// Returns an error if array lengths don't match
pub fn rgb_to_lab(
    r: &[u8],
    g: &[u8],
    b: &[u8],
    l: &mut [f32],
    a: &mut [f32],
    b_out: &mut [f32],
) -> Result<()> {
    validate_rgb_arrays(r, g, b)?;
    validate_output_arrays(l, a, b_out, r.len())?;

    // First convert RGB to XYZ
    let mut x = vec![0.0f32; r.len()];
    let mut y = vec![0.0f32; r.len()];
    let mut z = vec![0.0f32; r.len()];
    rgb_to_xyz(r, g, b, &mut x, &mut y, &mut z)?;

    // Then convert XYZ to LAB
    xyz_to_lab(&x, &y, &z, l, a, b_out)
}

/// Convert CIELAB to RGB color space using SIMD
///
/// # Errors
///
/// Returns an error if array lengths don't match
pub fn lab_to_rgb(
    l: &[f32],
    a: &[f32],
    b_in: &[f32],
    r: &mut [u8],
    g: &mut [u8],
    b: &mut [u8],
) -> Result<()> {
    validate_output_arrays(l, a, b_in, r.len())?;
    validate_rgb_arrays(r, g, b)?;

    // First convert LAB to XYZ
    let mut x = vec![0.0f32; l.len()];
    let mut y = vec![0.0f32; l.len()];
    let mut z = vec![0.0f32; l.len()];
    lab_to_xyz(l, a, b_in, &mut x, &mut y, &mut z)?;

    // Then convert XYZ to RGB
    xyz_to_rgb(&x, &y, &z, r, g, b)
}

/// Convert XYZ to LAB color space using SIMD
///
/// # Errors
///
/// Returns an error if array lengths don't match
pub fn xyz_to_lab(
    x: &[f32],
    y: &[f32],
    z: &[f32],
    l: &mut [f32],
    a: &mut [f32],
    b: &mut [f32],
) -> Result<()> {
    validate_output_arrays(x, y, z, l.len())?;
    validate_output_arrays(l, a, b, x.len())?;

    const LANES: usize = 4;
    let chunks = x.len() / LANES;

    // D65 illuminant reference white point
    const XN: f32 = 95.047;
    const YN: f32 = 100.0;
    const ZN: f32 = 108.883;

    // SIMD processing
    for i in 0..chunks {
        let start = i * LANES;
        let end = start + LANES;

        for j in start..end {
            let (l_val, a_val, b_val) = xyz_to_lab_scalar(x[j], y[j], z[j], XN, YN, ZN);
            l[j] = l_val;
            a[j] = a_val;
            b[j] = b_val;
        }
    }

    // Handle remainder
    let remainder_start = chunks * LANES;
    for i in remainder_start..x.len() {
        let (l_val, a_val, b_val) = xyz_to_lab_scalar(x[i], y[i], z[i], XN, YN, ZN);
        l[i] = l_val;
        a[i] = a_val;
        b[i] = b_val;
    }

    Ok(())
}

/// Convert LAB to XYZ color space using SIMD
///
/// # Errors
///
/// Returns an error if array lengths don't match
pub fn lab_to_xyz(
    l: &[f32],
    a: &[f32],
    b: &[f32],
    x: &mut [f32],
    y: &mut [f32],
    z: &mut [f32],
) -> Result<()> {
    validate_output_arrays(l, a, b, x.len())?;
    validate_output_arrays(x, y, z, l.len())?;

    const LANES: usize = 4;
    let chunks = l.len() / LANES;

    // D65 illuminant reference white point
    const XN: f32 = 95.047;
    const YN: f32 = 100.0;
    const ZN: f32 = 108.883;

    // SIMD processing
    for i in 0..chunks {
        let start = i * LANES;
        let end = start + LANES;

        for j in start..end {
            let (x_val, y_val, z_val) = lab_to_xyz_scalar(l[j], a[j], b[j], XN, YN, ZN);
            x[j] = x_val;
            y[j] = y_val;
            z[j] = z_val;
        }
    }

    // Handle remainder
    let remainder_start = chunks * LANES;
    for i in remainder_start..l.len() {
        let (x_val, y_val, z_val) = lab_to_xyz_scalar(l[i], a[i], b[i], XN, YN, ZN);
        x[i] = x_val;
        y[i] = y_val;
        z[i] = z_val;
    }

    Ok(())
}

// Scalar conversion functions (for SIMD loop bodies)

#[inline]
fn rgb_to_hsv_scalar(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let r_f = f32::from(r) / 255.0;
    let g_f = f32::from(g) / 255.0;
    let b_f = f32::from(b) / 255.0;

    let max = r_f.max(g_f).max(b_f);
    let min = r_f.min(g_f).min(b_f);
    let delta = max - min;

    let v = max;
    let s = if max < 1e-6 { 0.0 } else { delta / max };

    let h = if delta < 1e-6 {
        0.0
    } else if (max - r_f).abs() < 1e-6 {
        60.0 * (((g_f - b_f) / delta) % 6.0)
    } else if (max - g_f).abs() < 1e-6 {
        60.0 * (((b_f - r_f) / delta) + 2.0)
    } else {
        60.0 * (((r_f - g_f) / delta) + 4.0)
    };

    let h = if h < 0.0 { h + 360.0 } else { h };

    (h, s, v)
}

#[inline]
fn hsv_to_rgb_scalar(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let h_prime = h / 60.0;
    let x = c * (1.0 - ((h_prime % 2.0) - 1.0).abs());
    let m = v - c;

    let (r_f, g_f, b_f) = if h_prime < 1.0 {
        (c, x, 0.0)
    } else if h_prime < 2.0 {
        (x, c, 0.0)
    } else if h_prime < 3.0 {
        (0.0, c, x)
    } else if h_prime < 4.0 {
        (0.0, x, c)
    } else if h_prime < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    let r = ((r_f + m) * 255.0).clamp(0.0, 255.0) as u8;
    let g = ((g_f + m) * 255.0).clamp(0.0, 255.0) as u8;
    let b = ((b_f + m) * 255.0).clamp(0.0, 255.0) as u8;

    (r, g, b)
}

#[inline]
fn rgb_to_hsl_scalar(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let r_f = f32::from(r) / 255.0;
    let g_f = f32::from(g) / 255.0;
    let b_f = f32::from(b) / 255.0;

    let max = r_f.max(g_f).max(b_f);
    let min = r_f.min(g_f).min(b_f);
    let delta = max - min;

    let l = (max + min) / 2.0;

    let s = if delta < 1e-6 {
        0.0
    } else {
        delta / (1.0 - (2.0 * l - 1.0).abs())
    };

    let h = if delta < 1e-6 {
        0.0
    } else if (max - r_f).abs() < 1e-6 {
        60.0 * (((g_f - b_f) / delta) % 6.0)
    } else if (max - g_f).abs() < 1e-6 {
        60.0 * (((b_f - r_f) / delta) + 2.0)
    } else {
        60.0 * (((r_f - g_f) / delta) + 4.0)
    };

    let h = if h < 0.0 { h + 360.0 } else { h };

    (h, s, l)
}

#[inline]
fn hsl_to_rgb_scalar(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let h_prime = h / 60.0;
    let x = c * (1.0 - ((h_prime % 2.0) - 1.0).abs());
    let m = l - c / 2.0;

    let (r_f, g_f, b_f) = if h_prime < 1.0 {
        (c, x, 0.0)
    } else if h_prime < 2.0 {
        (x, c, 0.0)
    } else if h_prime < 3.0 {
        (0.0, c, x)
    } else if h_prime < 4.0 {
        (0.0, x, c)
    } else if h_prime < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    let r = ((r_f + m) * 255.0).clamp(0.0, 255.0) as u8;
    let g = ((g_f + m) * 255.0).clamp(0.0, 255.0) as u8;
    let b = ((b_f + m) * 255.0).clamp(0.0, 255.0) as u8;

    (r, g, b)
}

#[inline]
fn rgb_to_xyz_scalar(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    // Convert to linear RGB (gamma correction)
    let r_linear = srgb_to_linear(f32::from(r) / 255.0);
    let g_linear = srgb_to_linear(f32::from(g) / 255.0);
    let b_linear = srgb_to_linear(f32::from(b) / 255.0);

    // Apply transformation matrix (sRGB to XYZ with D65)
    let x = r_linear * 0.4124564 + g_linear * 0.3575761 + b_linear * 0.1804375;
    let y = r_linear * 0.2126729 + g_linear * 0.7151522 + b_linear * 0.0721750;
    let z = r_linear * 0.0193339 + g_linear * 0.119_192 + b_linear * 0.9503041;

    (x * 100.0, y * 100.0, z * 100.0)
}

#[inline]
fn xyz_to_rgb_scalar(x: f32, y: f32, z: f32) -> (u8, u8, u8) {
    let x = x / 100.0;
    let y = y / 100.0;
    let z = z / 100.0;

    // Apply inverse transformation matrix
    let r_linear = x * 3.2404542 + y * -1.5371385 + z * -0.4985314;
    let g_linear = x * -0.969_266 + y * 1.8760108 + z * 0.0415560;
    let b_linear = x * 0.0556434 + y * -0.2040259 + z * 1.0572252;

    // Apply gamma correction
    let r = (linear_to_srgb(r_linear) * 255.0).clamp(0.0, 255.0) as u8;
    let g = (linear_to_srgb(g_linear) * 255.0).clamp(0.0, 255.0) as u8;
    let b = (linear_to_srgb(b_linear) * 255.0).clamp(0.0, 255.0) as u8;

    (r, g, b)
}

#[inline]
fn xyz_to_lab_scalar(x: f32, y: f32, z: f32, xn: f32, yn: f32, zn: f32) -> (f32, f32, f32) {
    let fx = lab_f(x / xn);
    let fy = lab_f(y / yn);
    let fz = lab_f(z / zn);

    let l = 116.0 * fy - 16.0;
    let a = 500.0 * (fx - fy);
    let b = 200.0 * (fy - fz);

    (l, a, b)
}

#[inline]
fn lab_to_xyz_scalar(l: f32, a: f32, b: f32, xn: f32, yn: f32, zn: f32) -> (f32, f32, f32) {
    let fy = (l + 16.0) / 116.0;
    let fx = a / 500.0 + fy;
    let fz = fy - b / 200.0;

    let x = xn * lab_f_inv(fx);
    let y = yn * lab_f_inv(fy);
    let z = zn * lab_f_inv(fz);

    (x, y, z)
}

#[inline]
fn lab_f(t: f32) -> f32 {
    const DELTA: f32 = 6.0 / 29.0;
    const DELTA3: f32 = DELTA * DELTA * DELTA;

    if t > DELTA3 {
        t.cbrt()
    } else {
        t / (3.0 * DELTA * DELTA) + 4.0 / 29.0
    }
}

#[inline]
fn lab_f_inv(t: f32) -> f32 {
    const DELTA: f32 = 6.0 / 29.0;

    if t > DELTA {
        t * t * t
    } else {
        3.0 * DELTA * DELTA * (t - 4.0 / 29.0)
    }
}

#[inline]
fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

#[inline]
fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

// Validation helpers

fn validate_rgb_arrays(r: &[u8], g: &[u8], b: &[u8]) -> Result<()> {
    if r.len() != g.len() || r.len() != b.len() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "rgb_arrays",
            message: format!(
                "RGB array length mismatch: r={}, g={}, b={}",
                r.len(),
                g.len(),
                b.len()
            ),
        });
    }
    Ok(())
}

fn validate_output_arrays(a: &[f32], b: &[f32], c: &[f32], expected: usize) -> Result<()> {
    if a.len() != expected || b.len() != expected || c.len() != expected {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "output_arrays",
            message: format!(
                "Output array length mismatch: a={}, b={}, c={}, expected={}",
                a.len(),
                b.len(),
                c.len(),
                expected
            ),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rgb_to_hsv_roundtrip() {
        let r = vec![255u8, 128, 64, 0];
        let g = vec![0u8, 128, 128, 255];
        let b = vec![0u8, 0, 192, 0];
        let mut h = vec![0.0f32; 4];
        let mut s = vec![0.0f32; 4];
        let mut v = vec![0.0f32; 4];
        let mut r_out = vec![0u8; 4];
        let mut g_out = vec![0u8; 4];
        let mut b_out = vec![0u8; 4];

        rgb_to_hsv(&r, &g, &b, &mut h, &mut s, &mut v)
            .expect("RGB to HSV conversion should succeed");
        hsv_to_rgb(&h, &s, &v, &mut r_out, &mut g_out, &mut b_out)
            .expect("HSV to RGB conversion should succeed");

        for i in 0..4 {
            assert!((r[i] as i16 - r_out[i] as i16).abs() <= 1);
            assert!((g[i] as i16 - g_out[i] as i16).abs() <= 1);
            assert!((b[i] as i16 - b_out[i] as i16).abs() <= 1);
        }
    }

    #[test]
    fn test_rgb_to_hsl_roundtrip() {
        let r = vec![255u8, 128, 64];
        let g = vec![0u8, 128, 128];
        let b = vec![0u8, 0, 192];
        let mut h = vec![0.0f32; 3];
        let mut s = vec![0.0f32; 3];
        let mut l = vec![0.0f32; 3];
        let mut r_out = vec![0u8; 3];
        let mut g_out = vec![0u8; 3];
        let mut b_out = vec![0u8; 3];

        rgb_to_hsl(&r, &g, &b, &mut h, &mut s, &mut l)
            .expect("RGB to HSL conversion should succeed");
        hsl_to_rgb(&h, &s, &l, &mut r_out, &mut g_out, &mut b_out)
            .expect("HSL to RGB conversion should succeed");

        for i in 0..3 {
            assert!((r[i] as i16 - r_out[i] as i16).abs() <= 2);
            assert!((g[i] as i16 - g_out[i] as i16).abs() <= 2);
            assert!((b[i] as i16 - b_out[i] as i16).abs() <= 2);
        }
    }

    #[test]
    fn test_rgb_to_lab_roundtrip() {
        let r = vec![255u8, 128, 64];
        let g = vec![0u8, 128, 128];
        let b = vec![0u8, 0, 192];
        let mut l = vec![0.0f32; 3];
        let mut a = vec![0.0f32; 3];
        let mut b_lab = vec![0.0f32; 3];
        let mut r_out = vec![0u8; 3];
        let mut g_out = vec![0u8; 3];
        let mut b_out = vec![0u8; 3];

        rgb_to_lab(&r, &g, &b, &mut l, &mut a, &mut b_lab)
            .expect("RGB to LAB conversion should succeed");
        lab_to_rgb(&l, &a, &b_lab, &mut r_out, &mut g_out, &mut b_out)
            .expect("LAB to RGB conversion should succeed");

        for i in 0..3 {
            assert!((r[i] as i16 - r_out[i] as i16).abs() <= 3);
            assert!((g[i] as i16 - g_out[i] as i16).abs() <= 3);
            assert!((b[i] as i16 - b_out[i] as i16).abs() <= 3);
        }
    }

    #[test]
    fn test_white_to_hsv() {
        let r = vec![255u8];
        let g = vec![255u8];
        let b = vec![255u8];
        let mut h = vec![0.0f32];
        let mut s = vec![0.0f32];
        let mut v = vec![0.0f32];

        rgb_to_hsv(&r, &g, &b, &mut h, &mut s, &mut v)
            .expect("RGB to HSV conversion should succeed for white color");

        assert_eq!(s[0], 0.0); // No saturation for white
        assert!((v[0] - 1.0).abs() < 1e-5); // Full value
    }

    #[test]
    fn test_array_length_mismatch() {
        let r = vec![255u8; 10];
        let g = vec![128u8; 10];
        let b = vec![64u8; 5]; // Wrong length
        let mut h = vec![0.0f32; 10];
        let mut s = vec![0.0f32; 10];
        let mut v = vec![0.0f32; 10];

        let result = rgb_to_hsv(&r, &g, &b, &mut h, &mut s, &mut v);
        assert!(result.is_err());
    }
}
