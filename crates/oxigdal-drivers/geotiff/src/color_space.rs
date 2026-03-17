//! Color space transformation support for GeoTIFF.
//!
//! Implements TIFF photometric interpretation conversions:
//! - WhiteIsZero / BlackIsZero (grayscale)
//! - RGB
//! - Palette / indexed color
//! - CMYK (Separated)
//! - YCbCr (with Rec. 601 / Rec. 709 luma coefficients)
//! - CIE L*a*b* / ICC L*a*b* / ITU L*a*b*
//!
//! All public conversion types are pure-Rust; no external color-science crates
//! are used.

use thiserror::Error;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors produced by color space conversion operations.
#[derive(Debug, Error)]
pub enum ColorSpaceError {
    /// Unknown TIFF Photometric tag value.
    #[error("unknown photometric interpretation tag value: {0}")]
    UnknownPhotometric(u16),

    /// Pixel slice does not contain enough channel values.
    #[error("insufficient channels: expected {expected}, got {got}")]
    InsufficientChannels { expected: usize, got: usize },

    /// The ICC profile header is malformed or truncated.
    #[error("invalid ICC header: {0}")]
    InvalidIccHeader(String),

    /// Bit-depth not supported for this conversion path.
    #[error("unsupported bit depth: {0}")]
    UnsupportedBitDepth(u8),

    /// A generic conversion failure.
    #[error("conversion error: {0}")]
    ConversionError(String),
}

// ---------------------------------------------------------------------------
// ColorSpace enum
// ---------------------------------------------------------------------------

/// TIFF photometric interpretation, identifying the color space used to encode
/// pixel data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorSpace {
    /// TIFF Photometric 0: WhiteIsZero — minimum value is white.
    WhiteIsZero,
    /// TIFF Photometric 1: BlackIsZero — standard grayscale.
    BlackIsZero,
    /// TIFF Photometric 2: RGB.
    Rgb,
    /// TIFF Photometric 3: Palette / indexed color.
    Palette,
    /// TIFF Photometric 5: CMYK (Separated).
    Cmyk,
    /// TIFF Photometric 6: YCbCr.
    YCbCr,
    /// TIFF Photometric 8: CIE L\*a\*b\*.
    CieLab,
    /// TIFF Photometric 9: ICC L\*a\*b\*.
    IccLab,
    /// TIFF Photometric 10: ITU L\*a\*b\*.
    ItuLab,
}

impl ColorSpace {
    /// Construct from a raw TIFF `PhotometricInterpretation` tag value.
    ///
    /// # Errors
    /// Returns [`ColorSpaceError::UnknownPhotometric`] for unrecognised values.
    pub fn from_photometric(tag_value: u16) -> Result<Self, ColorSpaceError> {
        match tag_value {
            0 => Ok(Self::WhiteIsZero),
            1 => Ok(Self::BlackIsZero),
            2 => Ok(Self::Rgb),
            3 => Ok(Self::Palette),
            5 => Ok(Self::Cmyk),
            6 => Ok(Self::YCbCr),
            8 => Ok(Self::CieLab),
            9 => Ok(Self::IccLab),
            10 => Ok(Self::ItuLab),
            other => Err(ColorSpaceError::UnknownPhotometric(other)),
        }
    }

    /// Return the TIFF `PhotometricInterpretation` tag value for this color
    /// space.
    #[must_use]
    pub const fn to_photometric(&self) -> u16 {
        match self {
            Self::WhiteIsZero => 0,
            Self::BlackIsZero => 1,
            Self::Rgb => 2,
            Self::Palette => 3,
            Self::Cmyk => 5,
            Self::YCbCr => 6,
            Self::CieLab => 8,
            Self::IccLab => 9,
            Self::ItuLab => 10,
        }
    }

    /// Number of color channels (excluding alpha) typical for this color space.
    #[must_use]
    pub const fn channel_count(&self) -> usize {
        match self {
            Self::WhiteIsZero | Self::BlackIsZero => 1,
            Self::Rgb => 3,
            Self::Palette => 1,
            Self::Cmyk => 4,
            Self::YCbCr => 3,
            Self::CieLab | Self::IccLab | Self::ItuLab => 3,
        }
    }

    /// Returns `true` if this color space encodes a single luminance value.
    #[must_use]
    pub const fn is_grayscale(&self) -> bool {
        matches!(self, Self::WhiteIsZero | Self::BlackIsZero)
    }
}

// ---------------------------------------------------------------------------
// Pixel — normalized [0.0, 1.0] multi-channel representation
// ---------------------------------------------------------------------------

/// A normalized pixel with up to four channels in the `[0.0, 1.0]` range.
///
/// Unused channels are set to `0.0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pixel {
    /// Raw channel values; index meaning depends on the source color space.
    pub channels: [f32; 4],
    /// Number of meaningful channels (1–4).
    pub channel_count: u8,
}

impl Pixel {
    /// Create a grayscale pixel.
    #[must_use]
    pub fn gray(v: f32) -> Self {
        Self {
            channels: [v, 0.0, 0.0, 0.0],
            channel_count: 1,
        }
    }

    /// Create an RGB pixel.
    #[must_use]
    pub fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self {
            channels: [r, g, b, 0.0],
            channel_count: 3,
        }
    }

    /// Create an RGBA pixel.
    #[must_use]
    pub fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self {
            channels: [r, g, b, a],
            channel_count: 4,
        }
    }

    /// Create a CMYK pixel.
    #[must_use]
    pub fn cmyk(c: f32, m: f32, y: f32, k: f32) -> Self {
        Self {
            channels: [c, m, y, k],
            channel_count: 4,
        }
    }

    /// Create a YCbCr pixel.
    #[must_use]
    pub fn ycbcr(y: f32, cb: f32, cr: f32) -> Self {
        Self {
            channels: [y, cb, cr, 0.0],
            channel_count: 3,
        }
    }

    /// Create a CIE L\*a\*b\* pixel.
    #[must_use]
    pub fn lab(l: f32, a: f32, b: f32) -> Self {
        Self {
            channels: [l, a, b, 0.0],
            channel_count: 3,
        }
    }

    /// Red (or first) channel.
    #[must_use]
    pub fn r(&self) -> f32 {
        self.channels[0]
    }

    /// Green (or second) channel.
    #[must_use]
    pub fn g(&self) -> f32 {
        self.channels[1]
    }

    /// Blue (or third) channel.
    #[must_use]
    pub fn b(&self) -> f32 {
        self.channels[2]
    }

    /// Alpha (or fourth) channel.
    #[must_use]
    pub fn a(&self) -> f32 {
        self.channels[3]
    }

    /// Construct from a `u8` slice, interpreting values according to the given
    /// color space (values are normalised to `[0.0, 1.0]`).
    ///
    /// # Errors
    /// Returns an error when the slice is too short for the color space.
    pub fn from_u8_slice(data: &[u8], cs: &ColorSpace) -> Result<Self, ColorSpaceError> {
        let needed = cs.channel_count();
        if data.len() < needed {
            return Err(ColorSpaceError::InsufficientChannels {
                expected: needed,
                got: data.len(),
            });
        }
        let norm = |v: u8| v as f32 / 255.0_f32;
        match cs {
            ColorSpace::WhiteIsZero | ColorSpace::BlackIsZero | ColorSpace::Palette => {
                Ok(Self::gray(norm(data[0])))
            }
            ColorSpace::Rgb => Ok(Self::rgb(norm(data[0]), norm(data[1]), norm(data[2]))),
            ColorSpace::Cmyk => Ok(Self::cmyk(
                norm(data[0]),
                norm(data[1]),
                norm(data[2]),
                norm(data[3]),
            )),
            ColorSpace::YCbCr => Ok(Self::ycbcr(norm(data[0]), norm(data[1]), norm(data[2]))),
            ColorSpace::CieLab | ColorSpace::IccLab | ColorSpace::ItuLab => {
                Ok(Self::lab(norm(data[0]), norm(data[1]), norm(data[2])))
            }
        }
    }

    /// Construct from a `u16` slice, normalised to `[0.0, 1.0]`.
    ///
    /// # Errors
    /// Returns an error when the slice is too short for the color space.
    pub fn from_u16_slice(data: &[u16], cs: &ColorSpace) -> Result<Self, ColorSpaceError> {
        let needed = cs.channel_count();
        if data.len() < needed {
            return Err(ColorSpaceError::InsufficientChannels {
                expected: needed,
                got: data.len(),
            });
        }
        let norm = |v: u16| v as f32 / 65535.0_f32;
        match cs {
            ColorSpace::WhiteIsZero | ColorSpace::BlackIsZero | ColorSpace::Palette => {
                Ok(Self::gray(norm(data[0])))
            }
            ColorSpace::Rgb => Ok(Self::rgb(norm(data[0]), norm(data[1]), norm(data[2]))),
            ColorSpace::Cmyk => Ok(Self::cmyk(
                norm(data[0]),
                norm(data[1]),
                norm(data[2]),
                norm(data[3]),
            )),
            ColorSpace::YCbCr => Ok(Self::ycbcr(norm(data[0]), norm(data[1]), norm(data[2]))),
            ColorSpace::CieLab | ColorSpace::IccLab | ColorSpace::ItuLab => {
                Ok(Self::lab(norm(data[0]), norm(data[1]), norm(data[2])))
            }
        }
    }

    /// Convert the first three channels to a `u8` RGB triple, clamping to
    /// `[0, 255]`.
    #[must_use]
    pub fn to_u8_rgb(&self) -> [u8; 3] {
        let clamp_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        [
            clamp_u8(self.channels[0]),
            clamp_u8(self.channels[1]),
            clamp_u8(self.channels[2]),
        ]
    }

    /// Convert all four channels to a `u8` RGBA quadruple, clamping to
    /// `[0, 255]`.
    #[must_use]
    pub fn to_u8_rgba(&self) -> [u8; 4] {
        let clamp_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        [
            clamp_u8(self.channels[0]),
            clamp_u8(self.channels[1]),
            clamp_u8(self.channels[2]),
            clamp_u8(self.channels[3]),
        ]
    }
}

// ---------------------------------------------------------------------------
// YCbCrConverter
// ---------------------------------------------------------------------------

/// Converts between YCbCr and RGB according to the TIFF specification, which
/// follows ITU-R BT.601 by default.
///
/// The conversion formulae used are:
/// ```text
/// R = Y + 1.402 * (Cr - 128)
/// G = Y - 0.344136 * (Cb - 128) - 0.714136 * (Cr - 128)
/// B = Y + 1.772 * (Cb - 128)
/// ```
pub struct YCbCrConverter {
    /// Luma coefficients `[LumaRed, LumaGreen, LumaBlue]`.
    /// Default: `[0.299, 0.587, 0.114]` (Rec. 601).
    pub luma_coefficients: [f32; 3],
    /// Reference black/white levels:
    /// `[Y_black, Y_white, Cb_black, Cb_white, Cr_black, Cr_white]`.
    /// Default: `[0.0, 255.0, 128.0, 255.0, 128.0, 255.0]`.
    pub reference_black_white: [f32; 6],
}

impl YCbCrConverter {
    /// ITU-R BT.601 luma coefficients (TIFF / JPEG default).
    #[must_use]
    pub fn rec601() -> Self {
        Self {
            luma_coefficients: [0.299, 0.587, 0.114],
            reference_black_white: [0.0, 255.0, 128.0, 255.0, 128.0, 255.0],
        }
    }

    /// ITU-R BT.709 luma coefficients (HDTV).
    #[must_use]
    pub fn rec709() -> Self {
        Self {
            luma_coefficients: [0.2126, 0.7152, 0.0722],
            reference_black_white: [0.0, 255.0, 128.0, 255.0, 128.0, 255.0],
        }
    }

    /// Construct from optional TIFF tag values.  Missing values fall back to
    /// the Rec. 601 / TIFF defaults.
    #[must_use]
    pub fn from_tiff_tags(luma: Option<[f32; 3]>, ref_bw: Option<[f32; 6]>) -> Self {
        let default = Self::rec601();
        Self {
            luma_coefficients: luma.unwrap_or(default.luma_coefficients),
            reference_black_white: ref_bw.unwrap_or(default.reference_black_white),
        }
    }

    /// Convert a YCbCr pixel (all three values normalised to `[0.0, 1.0]`,
    /// i.e. raw byte value divided by 255) to linear RGB in `[0.0, 1.0]`.
    ///
    /// The standard BT.601 / TIFF integer-domain formula is applied after
    /// re-scaling to the `[0, 255]` byte range:
    ///
    /// ```text
    /// R = Y + 1.402 * (Cr − 128)
    /// G = Y − 0.344136 * (Cb − 128) − 0.714136 * (Cr − 128)
    /// B = Y + 1.772 * (Cb − 128)
    /// ```
    #[must_use]
    pub fn to_rgb(&self, y_norm: f32, cb_norm: f32, cr_norm: f32) -> [f32; 3] {
        // Re-scale normalised values back to the byte domain [0, 255].
        let y = y_norm * 255.0;
        let cb = cb_norm * 255.0;
        let cr = cr_norm * 255.0;

        // BT.601 / TIFF standard matrix (byte-domain).
        let r = y + 1.402_f32 * (cr - 128.0);
        let g = y - 0.344_136_f32 * (cb - 128.0) - 0.714_136_f32 * (cr - 128.0);
        let b = y + 1.772_f32 * (cb - 128.0);

        [
            (r / 255.0).clamp(0.0, 1.0),
            (g / 255.0).clamp(0.0, 1.0),
            (b / 255.0).clamp(0.0, 1.0),
        ]
    }

    /// Convert a linear RGB pixel (`[0.0, 1.0]`) to YCbCr, returning
    /// normalised `[0.0, 1.0]` values (i.e. byte-domain divided by 255).
    ///
    /// Uses the luma coefficients stored in `self`.
    #[must_use]
    pub fn from_rgb(&self, r: f32, g: f32, b: f32) -> [f32; 3] {
        let [lr, lg, lb] = self.luma_coefficients;

        // Scale to byte domain [0, 255].
        let r255 = r * 255.0;
        let g255 = g * 255.0;
        let b255 = b * 255.0;

        let y = lr * r255 + lg * g255 + lb * b255;
        let cb = 128.0 - (lr / (2.0 * (1.0 - lb))) * r255 - (lg / (2.0 * (1.0 - lb))) * g255
            + 0.5 * b255;
        let cr = 128.0 + 0.5 * r255
            - (lg / (2.0 * (1.0 - lr))) * g255
            - (lb / (2.0 * (1.0 - lr))) * b255;

        // Normalise back to [0, 1].
        [
            (y / 255.0).clamp(0.0, 1.0),
            (cb / 255.0).clamp(0.0, 1.0),
            (cr / 255.0).clamp(0.0, 1.0),
        ]
    }

    /// Convert an interleaved YCbCr `u8` buffer (3 bytes per pixel) to an
    /// interleaved RGB `u8` buffer of the same length.
    ///
    /// Bytes that do not form a complete triple are silently dropped.
    #[must_use]
    pub fn buffer_to_rgb(&self, ycbcr: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(ycbcr.len());
        for chunk in ycbcr.chunks_exact(3) {
            let yn = chunk[0] as f32 / 255.0;
            let cbn = chunk[1] as f32 / 255.0;
            let crn = chunk[2] as f32 / 255.0;
            let [r, g, b] = self.to_rgb(yn, cbn, crn);
            out.push((r * 255.0).round() as u8);
            out.push((g * 255.0).round() as u8);
            out.push((b * 255.0).round() as u8);
        }
        out
    }

    /// Convert an interleaved RGB `u8` buffer (3 bytes per pixel) to an
    /// interleaved YCbCr `u8` buffer of the same length.
    ///
    /// Bytes that do not form a complete triple are silently dropped.
    #[must_use]
    pub fn rgb_to_buffer(&self, rgb: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(rgb.len());
        for chunk in rgb.chunks_exact(3) {
            let r = chunk[0] as f32 / 255.0;
            let g = chunk[1] as f32 / 255.0;
            let b = chunk[2] as f32 / 255.0;
            let [yn, cbn, crn] = self.from_rgb(r, g, b);
            out.push((yn * 255.0).round() as u8);
            out.push((cbn * 255.0).round() as u8);
            out.push((crn * 255.0).round() as u8);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// CmykConverter
// ---------------------------------------------------------------------------

/// Converts between CMYK and RGB using the ink-subtractive model.
pub struct CmykConverter {
    /// When `true`, ICC-based conversion is preferred (currently falls through
    /// to the simple formula; a full ICC engine is out of scope for this crate).
    pub use_icc: bool,
}

impl Default for CmykConverter {
    fn default() -> Self {
        Self::new()
    }
}

impl CmykConverter {
    /// Create a new `CmykConverter` using the simple ink-subtractive formula.
    #[must_use]
    pub const fn new() -> Self {
        Self { use_icc: false }
    }

    /// Convert CMYK (`[0.0, 1.0]` each) to RGB (`[0.0, 1.0]` each) using
    /// the simple ink-subtractive model:
    ///
    /// ```text
    /// R = (1 - C) * (1 - K)
    /// G = (1 - M) * (1 - K)
    /// B = (1 - Y) * (1 - K)
    /// ```
    #[must_use]
    pub fn to_rgb(&self, c: f32, m: f32, y: f32, k: f32) -> [f32; 3] {
        let r = (1.0 - c) * (1.0 - k);
        let g = (1.0 - m) * (1.0 - k);
        let b = (1.0 - y) * (1.0 - k);
        [r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0)]
    }

    /// Convert RGB (`[0.0, 1.0]`) to CMYK (`[0.0, 1.0]` each).
    ///
    /// Returns `[C, M, Y, K]`.
    #[must_use]
    pub fn from_rgb(&self, r: f32, g: f32, b: f32) -> [f32; 4] {
        let k = 1.0 - r.max(g).max(b);
        if (1.0 - k).abs() < f32::EPSILON {
            // Pure black.
            return [0.0, 0.0, 0.0, 1.0];
        }
        let denom = 1.0 - k;
        let c = (1.0 - r - k) / denom;
        let m = (1.0 - g - k) / denom;
        let y = (1.0 - b - k) / denom;
        [
            c.clamp(0.0, 1.0),
            m.clamp(0.0, 1.0),
            y.clamp(0.0, 1.0),
            k.clamp(0.0, 1.0),
        ]
    }

    /// Convert an interleaved CMYK `u8` buffer (4 bytes per pixel) to an
    /// interleaved RGB `u8` buffer (3 bytes per pixel).
    ///
    /// Incomplete pixel groups are silently dropped.
    #[must_use]
    pub fn buffer_to_rgb(&self, cmyk: &[u8]) -> Vec<u8> {
        let pixel_count = cmyk.len() / 4;
        let mut out = Vec::with_capacity(pixel_count * 3);
        for chunk in cmyk.chunks_exact(4) {
            let c = chunk[0] as f32 / 255.0;
            let m = chunk[1] as f32 / 255.0;
            let y = chunk[2] as f32 / 255.0;
            let k = chunk[3] as f32 / 255.0;
            let [r, g, b] = self.to_rgb(c, m, y, k);
            out.push((r * 255.0).round() as u8);
            out.push((g * 255.0).round() as u8);
            out.push((b * 255.0).round() as u8);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// CieLabConverter
// ---------------------------------------------------------------------------

/// Converts between CIE L\*a\*b\* and sRGB via the CIE XYZ intermediate space.
pub struct CieLabConverter {
    /// X component of the reference white point.
    pub white_x: f32,
    /// Y component of the reference white point (luminance, always 1.0 for a
    /// normalised white point).
    pub white_y: f32,
    /// Z component of the reference white point.
    pub white_z: f32,
}

impl CieLabConverter {
    /// D65 illuminant — the standard for most digital imaging workflows.
    #[must_use]
    pub fn d65() -> Self {
        Self {
            white_x: 0.950_456,
            white_y: 1.0,
            white_z: 1.088_754,
        }
    }

    /// D50 illuminant — used in printing and ICC profiles.
    #[must_use]
    pub fn d50() -> Self {
        Self {
            white_x: 0.964_212,
            white_y: 1.0,
            white_z: 0.825_188,
        }
    }

    // CIE Lab auxiliary function (cube root or linear approximation).
    fn f_inv(t: f32) -> f32 {
        const DELTA: f32 = 6.0 / 29.0;
        if t > DELTA {
            t * t * t
        } else {
            3.0 * DELTA * DELTA * (t - 4.0 / 29.0)
        }
    }

    // Inverse: the forward f(t) function used in XYZ→Lab.
    fn f_fwd(t: f32) -> f32 {
        const DELTA: f32 = 6.0 / 29.0;
        if t > DELTA * DELTA * DELTA {
            t.cbrt()
        } else {
            t / (3.0 * DELTA * DELTA) + 4.0 / 29.0
        }
    }

    /// Convert CIE L\*a\*b\* (`L` in `[0, 100]`, `a*`, `b*` in `[-128, 127]`)
    /// to XYZ (normalised to the white point).
    #[must_use]
    pub fn lab_to_xyz(&self, l: f32, a: f32, b: f32) -> [f32; 3] {
        let fy = (l + 16.0) / 116.0;
        let fx = a / 500.0 + fy;
        let fz = fy - b / 200.0;

        let x = self.white_x * Self::f_inv(fx);
        let y = self.white_y * Self::f_inv(fy);
        let z = self.white_z * Self::f_inv(fz);

        [x, y, z]
    }

    /// Convert CIE XYZ to sRGB in `[0.0, 1.0]`, applying the sRGB gamma curve.
    ///
    /// Uses the standard D65-referenced 3×3 matrix regardless of white point
    /// (chromatic adaptation is out of scope here).
    #[must_use]
    pub fn xyz_to_srgb(&self, x: f32, y: f32, z: f32) -> [f32; 3] {
        // Standard XYZ→linear-sRGB matrix (D65 reference white).
        let r_lin = 3.240_97 * x - 1.537_383 * y - 0.498_611 * z;
        let g_lin = -0.969_244 * x + 1.875_968 * y + 0.041_555 * z;
        let b_lin = 0.055_630 * x - 0.203_977 * y + 1.056_972 * z;

        // Apply sRGB gamma.
        let srgb_gamma = |c: f32| -> f32 {
            let c = c.clamp(0.0, 1.0);
            if c <= 0.003_130_8 {
                12.92 * c
            } else {
                1.055 * c.powf(1.0 / 2.4) - 0.055
            }
        };

        [
            srgb_gamma(r_lin).clamp(0.0, 1.0),
            srgb_gamma(g_lin).clamp(0.0, 1.0),
            srgb_gamma(b_lin).clamp(0.0, 1.0),
        ]
    }

    /// Convert CIE L\*a\*b\* to sRGB in `[0.0, 1.0]` (combined pipeline).
    #[must_use]
    pub fn lab_to_rgb(&self, l: f32, a: f32, b: f32) -> [f32; 3] {
        let [x, y, z] = self.lab_to_xyz(l, a, b);
        self.xyz_to_srgb(x, y, z)
    }

    /// Convert sRGB in `[0.0, 1.0]` to CIE L\*a\*b\*.
    ///
    /// Applies the inverse sRGB gamma and standard linear-sRGB→XYZ matrix.
    #[must_use]
    pub fn rgb_to_lab(&self, r: f32, g: f32, b: f32) -> [f32; 3] {
        // Inverse sRGB gamma → linear.
        let linearize = |c: f32| -> f32 {
            let c = c.clamp(0.0, 1.0);
            if c <= 0.040_45 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };

        let r_lin = linearize(r);
        let g_lin = linearize(g);
        let b_lin = linearize(b);

        // Linear-sRGB → XYZ (D65).
        let x = 0.412_391 * r_lin + 0.357_584 * g_lin + 0.180_481 * b_lin;
        let y = 0.212_639 * r_lin + 0.715_169 * g_lin + 0.072_192 * b_lin;
        let z = 0.019_330 * r_lin + 0.119_195 * g_lin + 0.950_532 * b_lin;

        // XYZ → Lab.
        let fx = Self::f_fwd(x / self.white_x);
        let fy = Self::f_fwd(y / self.white_y);
        let fz = Self::f_fwd(z / self.white_z);

        let l_star = 116.0 * fy - 16.0;
        let a_star = 500.0 * (fx - fy);
        let b_star = 200.0 * (fy - fz);

        [l_star, a_star, b_star]
    }

    /// Convert an interleaved CIE L\*a\*b\* `u8` buffer to an interleaved RGB
    /// `u8` buffer.
    ///
    /// **TIFF encoding convention for CIE Lab (Photometric 8):**
    /// - `L*` byte → `L* = byte * (100.0 / 255.0)`
    /// - `a*` byte → `a* = byte − 128`  (signed, −128 … +127)
    /// - `b*` byte → `b* = byte − 128`  (signed, −128 … +127)
    ///
    /// Incomplete pixel groups are silently dropped.
    #[must_use]
    pub fn buffer_to_rgb(&self, lab: &[u8]) -> Vec<u8> {
        let pixel_count = lab.len() / 3;
        let mut out = Vec::with_capacity(pixel_count * 3);
        for chunk in lab.chunks_exact(3) {
            let l = chunk[0] as f32 * (100.0 / 255.0);
            let a = chunk[1] as f32 - 128.0;
            let b = chunk[2] as f32 - 128.0;
            let [r, g, bv] = self.lab_to_rgb(l, a, b);
            out.push((r * 255.0).round() as u8);
            out.push((g * 255.0).round() as u8);
            out.push((bv * 255.0).round() as u8);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// IccProfile — minimal header parsing
// ---------------------------------------------------------------------------

/// Color space signature parsed from an ICC profile header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColorSpaceSig {
    /// `'RGB '` — RGB color space.
    Rgb,
    /// `'CMYK'` — CMYK color space.
    Cmyk,
    /// `'GRAY'` — grayscale.
    Gray,
    /// `'Lab '` — CIE L\*a\*b\*.
    Lab,
    /// `'XYZ '` — CIE XYZ.
    Xyz,
    /// An unrecognised four-byte signature.
    Unknown([u8; 4]),
}

impl ColorSpaceSig {
    fn from_bytes(b: &[u8]) -> Self {
        match b {
            b"RGB " => Self::Rgb,
            b"CMYK" => Self::Cmyk,
            b"GRAY" => Self::Gray,
            b"Lab " => Self::Lab,
            b"XYZ " => Self::Xyz,
            other => {
                let mut arr = [0u8; 4];
                arr.copy_from_slice(&other[..4]);
                Self::Unknown(arr)
            }
        }
    }
}

/// Device class parsed from an ICC profile header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceClass {
    /// `'scnr'` — input device (scanner).
    Input,
    /// `'mntr'` — display device (monitor).
    Display,
    /// `'prtr'` — output device (printer).
    Output,
    /// `'link'` — device link profile.
    Link,
    /// `'abst'` — abstract profile.
    Abstract,
    /// `'spac'` — color space conversion profile.
    ColorSpace,
    /// `'nmcl'` — named color profile.
    Named,
    /// An unrecognised four-byte class signature.
    Unknown([u8; 4]),
}

impl DeviceClass {
    fn from_bytes(b: &[u8]) -> Self {
        match b {
            b"scnr" => Self::Input,
            b"mntr" => Self::Display,
            b"prtr" => Self::Output,
            b"link" => Self::Link,
            b"abst" => Self::Abstract,
            b"spac" => Self::ColorSpace,
            b"nmcl" => Self::Named,
            other => {
                let mut arr = [0u8; 4];
                arr.copy_from_slice(&other[..4]);
                Self::Unknown(arr)
            }
        }
    }
}

/// Key information decoded from the 128-byte ICC profile header.
pub struct IccProfile {
    /// Total byte length of the profile as stated in the header.
    pub profile_size: u32,
    /// Color space of the profile data.
    pub color_space_sig: ColorSpaceSig,
    /// Profile Connection Space (PCS).
    pub pcs: ColorSpaceSig,
    /// Device class of this profile.
    pub device_class: DeviceClass,
    /// Rendering intent (ICC Table 19).
    pub rendering_intent: u32,
    /// Number of tags recorded in the tag table.
    pub tag_count: u32,
}

impl IccProfile {
    /// Parse the first 128 bytes of an ICC profile.
    ///
    /// Layout (big-endian integers):
    /// - bytes 0–3:   profile size (u32)
    /// - bytes 16–19: device class signature
    /// - bytes 20–23: color space signature
    /// - bytes 24–27: PCS signature
    /// - bytes 64–67: rendering intent (u32)
    /// - bytes 128–131: tag count (u32) — only present if `data.len() >= 132`
    ///
    /// # Errors
    /// Returns [`ColorSpaceError::InvalidIccHeader`] when `data` is shorter
    /// than 128 bytes or when mandatory fields are missing.
    pub fn parse_header(data: &[u8]) -> Result<Self, ColorSpaceError> {
        if data.len() < 128 {
            return Err(ColorSpaceError::InvalidIccHeader(format!(
                "data too short: {} bytes (minimum 128 required)",
                data.len()
            )));
        }

        let profile_size = u32::from_be_bytes(data[0..4].try_into().map_err(|_| {
            ColorSpaceError::InvalidIccHeader("cannot read profile size".to_string())
        })?);

        let device_class = DeviceClass::from_bytes(&data[16..20]);
        let color_space_sig = ColorSpaceSig::from_bytes(&data[20..24]);
        let pcs = ColorSpaceSig::from_bytes(&data[24..28]);

        let rendering_intent = u32::from_be_bytes(data[64..68].try_into().map_err(|_| {
            ColorSpaceError::InvalidIccHeader("cannot read rendering intent".to_string())
        })?);

        // Tag count sits immediately after the 128-byte header.
        let tag_count = if data.len() >= 132 {
            u32::from_be_bytes(data[128..132].try_into().map_err(|_| {
                ColorSpaceError::InvalidIccHeader("cannot read tag count".to_string())
            })?)
        } else {
            0
        };

        Ok(Self {
            profile_size,
            color_space_sig,
            pcs,
            device_class,
            rendering_intent,
            tag_count,
        })
    }

    /// Returns `true` when the profile's stated size is at least 128 and the
    /// color space and PCS are not `Unknown`.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.profile_size >= 128
            && !matches!(self.color_space_sig, ColorSpaceSig::Unknown(_))
            && !matches!(self.pcs, ColorSpaceSig::Unknown(_))
    }
}

// ---------------------------------------------------------------------------
// ColorSpaceConverter — unified façade
// ---------------------------------------------------------------------------

/// Unified color space converter: converts pixel buffers or individual pixels
/// to linear RGB.
pub struct ColorSpaceConverter;

impl ColorSpaceConverter {
    /// Convert a `u8` pixel buffer from `source_cs` to interleaved RGB `u8`.
    ///
    /// Supported bit depths: 8.  Use `pixel_to_rgb` for already-normalised
    /// `f32` data.
    ///
    /// # Errors
    /// Returns an error for unsupported bit depths or conversion failures.
    pub fn to_rgb(
        data: &[u8],
        source_cs: &ColorSpace,
        bits_per_sample: u8,
    ) -> Result<Vec<u8>, ColorSpaceError> {
        if bits_per_sample != 8 {
            return Err(ColorSpaceError::UnsupportedBitDepth(bits_per_sample));
        }

        match source_cs {
            ColorSpace::WhiteIsZero => {
                // Invert luminance: white = minimum, black = maximum.
                let out = data
                    .chunks_exact(1)
                    .flat_map(|ch| {
                        let v = 255 - ch[0];
                        [v, v, v]
                    })
                    .collect();
                Ok(out)
            }
            ColorSpace::BlackIsZero => {
                // Passthrough: replicate single gray channel to RGB.
                let out = data
                    .chunks_exact(1)
                    .flat_map(|ch| [ch[0], ch[0], ch[0]])
                    .collect();
                Ok(out)
            }
            ColorSpace::Rgb => Ok(data.to_vec()),
            ColorSpace::Palette => {
                // Passthrough index as gray (caller must apply palette lookup).
                let out = data
                    .chunks_exact(1)
                    .flat_map(|ch| [ch[0], ch[0], ch[0]])
                    .collect();
                Ok(out)
            }
            ColorSpace::Cmyk => {
                let conv = CmykConverter::new();
                Ok(conv.buffer_to_rgb(data))
            }
            ColorSpace::YCbCr => {
                let conv = YCbCrConverter::rec601();
                Ok(conv.buffer_to_rgb(data))
            }
            ColorSpace::CieLab | ColorSpace::IccLab | ColorSpace::ItuLab => {
                let conv = CieLabConverter::d65();
                Ok(conv.buffer_to_rgb(data))
            }
        }
    }

    /// Convert a single pixel (normalised `f32` channels) to RGB `[0.0, 1.0]`.
    ///
    /// # Errors
    /// Returns an error when `channels` is too short or conversion fails.
    pub fn pixel_to_rgb(
        channels: &[f32],
        source_cs: &ColorSpace,
    ) -> Result<[f32; 3], ColorSpaceError> {
        let needed = source_cs.channel_count();
        if channels.len() < needed {
            return Err(ColorSpaceError::InsufficientChannels {
                expected: needed,
                got: channels.len(),
            });
        }

        match source_cs {
            ColorSpace::WhiteIsZero => {
                let v = 1.0 - channels[0].clamp(0.0, 1.0);
                Ok([v, v, v])
            }
            ColorSpace::BlackIsZero | ColorSpace::Palette => {
                let v = channels[0].clamp(0.0, 1.0);
                Ok([v, v, v])
            }
            ColorSpace::Rgb => Ok([
                channels[0].clamp(0.0, 1.0),
                channels[1].clamp(0.0, 1.0),
                channels[2].clamp(0.0, 1.0),
            ]),
            ColorSpace::Cmyk => {
                let conv = CmykConverter::new();
                Ok(conv.to_rgb(channels[0], channels[1], channels[2], channels[3]))
            }
            ColorSpace::YCbCr => {
                let conv = YCbCrConverter::rec601();
                Ok(conv.to_rgb(channels[0], channels[1], channels[2]))
            }
            ColorSpace::CieLab | ColorSpace::IccLab | ColorSpace::ItuLab => {
                // channels expected as [L (0–1), a* (0–1), b* (0–1)] normalised.
                // Decode: L = ch[0]*100, a* = ch[1]*255 - 128, b* = ch[2]*255 - 128
                let conv = CieLabConverter::d65();
                let l = channels[0] * 100.0;
                let a = channels[1] * 255.0 - 128.0;
                let b = channels[2] * 255.0 - 128.0;
                Ok(conv.lab_to_rgb(l, a, b))
            }
        }
    }
}
