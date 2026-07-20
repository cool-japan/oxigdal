//! Tile compositing pipeline for the GPU rendering layer.
//!
//! Provides CPU-side Porter-Duff compositing, multi-layer stacking, 4x5 color
//! matrix transforms, and a high-level [`TileRenderPipeline`] that wires
//! everything together with shader hot-reload support.

use crate::shader_reload::HotReloadRegistry;

// ─── Rgba ─────────────────────────────────────────────────────────────────────

/// A 32-bit RGBA colour in linear-light floating-point space.
///
/// All channels are nominally in `[0.0, 1.0]`.  Arithmetic operations may
/// produce out-of-range values; call [`Rgba::clamp`] to normalise.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    /// Construct from linear-light float channels.
    #[inline]
    pub fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Construct from 8-bit unsigned integer channels (sRGB transfer assumed
    /// by the caller; no gamma conversion is applied here).
    #[inline]
    pub fn from_u8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: a as f32 / 255.0,
        }
    }

    /// Convert back to 8-bit channels, clamping to `[0, 255]`.
    #[inline]
    pub fn to_u8(&self) -> (u8, u8, u8, u8) {
        let clamp_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        (
            clamp_u8(self.r),
            clamp_u8(self.g),
            clamp_u8(self.b),
            clamp_u8(self.a),
        )
    }

    /// Clamp all channels to `[0.0, 1.0]`.
    #[inline]
    pub fn clamp(&self) -> Self {
        Self {
            r: self.r.clamp(0.0, 1.0),
            g: self.g.clamp(0.0, 1.0),
            b: self.b.clamp(0.0, 1.0),
            a: self.a.clamp(0.0, 1.0),
        }
    }

    /// Convert to premultiplied-alpha representation (RGB *= alpha).
    #[inline]
    pub fn premultiply(&self) -> Self {
        Self {
            r: self.r * self.a,
            g: self.g * self.a,
            b: self.b * self.a,
            a: self.a,
        }
    }

    /// Convert from premultiplied-alpha back to straight alpha.
    ///
    /// If `alpha == 0` the RGB channels are left as-is to avoid NaN.
    #[inline]
    pub fn unpremultiply(&self) -> Self {
        if self.a < 1e-8 {
            Self {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            }
        } else {
            Self {
                r: self.r / self.a,
                g: self.g / self.a,
                b: self.b / self.a,
                a: self.a,
            }
        }
    }

    /// Fully transparent black pixel.
    #[inline]
    pub fn transparent() -> Self {
        Self::new(0.0, 0.0, 0.0, 0.0)
    }

    /// Opaque white pixel.
    #[inline]
    pub fn white() -> Self {
        Self::new(1.0, 1.0, 1.0, 1.0)
    }

    /// Opaque black pixel.
    #[inline]
    pub fn black() -> Self {
        Self::new(0.0, 0.0, 0.0, 1.0)
    }
}

// ─── BlendMode ────────────────────────────────────────────────────────────────

/// Porter-Duff and Photoshop blend modes.
///
/// All `blend` operations work in **straight** (non-premultiplied)
/// linear-light colour space.  The internal implementation premultiplies
/// inputs as needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendMode {
    // ── Photoshop-style separable modes ──────────────────────────────────────
    Normal,
    Multiply,
    Screen,
    Overlay,
    HardLight,
    SoftLight,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    Difference,
    Exclusion,
    // ── Porter-Duff compositing operators ────────────────────────────────────
    SrcOver,
    SrcIn,
    SrcOut,
    SrcAtop,
    DstOver,
    DstIn,
    DstOut,
    DstAtop,
    Xor,
    Clear,
}

impl BlendMode {
    /// Blend `src` over `dst` using this mode.
    ///
    /// Inputs and output are in **straight** (non-premultiplied) linear-light
    /// colour space.  Alpha compositing follows the Porter-Duff `SrcOver`
    /// model for all non-Porter-Duff modes.
    pub fn blend(&self, src: Rgba, dst: Rgba) -> Rgba {
        match self {
            BlendMode::Normal | BlendMode::SrcOver => src_over(src, dst),
            BlendMode::Multiply => separable_blend(src, dst, |s, d| s * d),
            BlendMode::Screen => separable_blend(src, dst, |s, d| 1.0 - (1.0 - s) * (1.0 - d)),
            BlendMode::Overlay => separable_blend(src, dst, |s, d| hard_light_channel(d, s)),
            BlendMode::HardLight => separable_blend(src, dst, |s, d| hard_light_channel(s, d)),
            BlendMode::SoftLight => separable_blend(src, dst, soft_light_channel),
            BlendMode::Darken => separable_blend(src, dst, |s, d| s.min(d)),
            BlendMode::Lighten => separable_blend(src, dst, |s, d| s.max(d)),
            BlendMode::ColorDodge => separable_blend(src, dst, color_dodge_channel),
            BlendMode::ColorBurn => separable_blend(src, dst, color_burn_channel),
            BlendMode::Difference => separable_blend(src, dst, |s, d| (s - d).abs()),
            BlendMode::Exclusion => separable_blend(src, dst, |s, d| s + d - 2.0 * s * d),
            BlendMode::SrcIn => porter_duff(src, dst, src.a * dst.a, 0.0),
            BlendMode::SrcOut => porter_duff(src, dst, src.a * (1.0 - dst.a), 0.0),
            BlendMode::SrcAtop => porter_duff(src, dst, src.a * dst.a, dst.a * (1.0 - src.a)),
            BlendMode::DstOver => src_over(dst, src),
            BlendMode::DstIn => porter_duff(src, dst, 0.0, dst.a * src.a),
            BlendMode::DstOut => porter_duff(src, dst, 0.0, dst.a * (1.0 - src.a)),
            BlendMode::DstAtop => porter_duff(src, dst, src.a * (1.0 - dst.a), dst.a * src.a),
            BlendMode::Xor => porter_duff(src, dst, src.a * (1.0 - dst.a), dst.a * (1.0 - src.a)),
            BlendMode::Clear => Rgba::transparent(),
        }
    }
}

// ── Internal compositing helpers ─────────────────────────────────────────────

/// Standard Porter-Duff `SrcOver` — straight-alpha implementation.
fn src_over(src: Rgba, dst: Rgba) -> Rgba {
    let a_out = src.a + dst.a * (1.0 - src.a);
    if a_out < 1e-8 {
        return Rgba::transparent();
    }
    Rgba {
        r: (src.r * src.a + dst.r * dst.a * (1.0 - src.a)) / a_out,
        g: (src.g * src.a + dst.g * dst.a * (1.0 - src.a)) / a_out,
        b: (src.b * src.a + dst.b * dst.a * (1.0 - src.a)) / a_out,
        a: a_out,
    }
}

/// Apply a separable per-channel blend function with `SrcOver` alpha.
fn separable_blend<F>(src: Rgba, dst: Rgba, f: F) -> Rgba
where
    F: Fn(f32, f32) -> f32,
{
    // Blend RGB channels using the supplied function, then apply SrcOver alpha.
    let blended = Rgba {
        r: f(src.r, dst.r),
        g: f(src.g, dst.g),
        b: f(src.b, dst.b),
        a: src.a,
    };
    src_over(blended, dst)
}

/// Porter-Duff generic compositor with explicit src/dst alpha factors.
fn porter_duff(src: Rgba, dst: Rgba, src_factor: f32, dst_factor: f32) -> Rgba {
    let a_out = src_factor + dst_factor;
    if a_out < 1e-8 {
        return Rgba::transparent();
    }
    Rgba {
        r: (src.r * src_factor + dst.r * dst_factor) / a_out,
        g: (src.g * src_factor + dst.g * dst_factor) / a_out,
        b: (src.b * src_factor + dst.b * dst_factor) / a_out,
        a: a_out,
    }
}

/// Hard-light blend for a single channel (used by Overlay and HardLight).
#[inline]
fn hard_light_channel(src: f32, dst: f32) -> f32 {
    if src <= 0.5 {
        2.0 * src * dst
    } else {
        1.0 - 2.0 * (1.0 - src) * (1.0 - dst)
    }
}

/// Soft-light blend for a single channel (W3C/CSS compositing definition).
#[inline]
fn soft_light_channel(src: f32, dst: f32) -> f32 {
    if src <= 0.5 {
        dst - (1.0 - 2.0 * src) * dst * (1.0 - dst)
    } else {
        let d = if dst <= 0.25 {
            ((16.0 * dst - 12.0) * dst + 4.0) * dst
        } else {
            dst.sqrt()
        };
        dst + (2.0 * src - 1.0) * (d - dst)
    }
}

/// Color-dodge for a single channel.
#[inline]
fn color_dodge_channel(src: f32, dst: f32) -> f32 {
    if (1.0 - src) < 1e-8 {
        1.0
    } else {
        (dst / (1.0 - src)).min(1.0)
    }
}

/// Color-burn for a single channel.
#[inline]
fn color_burn_channel(src: f32, dst: f32) -> f32 {
    if src < 1e-8 {
        0.0
    } else {
        1.0 - ((1.0 - dst) / src).min(1.0)
    }
}

// ─── Layer ────────────────────────────────────────────────────────────────────

/// A compositable image layer with blend settings.
pub struct Layer {
    pub label: String,
    /// Pixel data in row-major order (row 0 at index 0).
    pub pixels: Vec<Rgba>,
    pub width: u32,
    pub height: u32,
    /// Layer-wide opacity multiplier applied to each pixel's alpha.
    pub opacity: f32,
    pub blend_mode: BlendMode,
    pub visible: bool,
    /// Compositing order; layers are sorted ascending before compositing.
    pub z_order: i32,
}

impl Layer {
    /// Create a new, fully transparent layer.
    pub fn new(label: impl Into<String>, width: u32, height: u32) -> Self {
        let count = (width * height) as usize;
        Self {
            label: label.into(),
            pixels: vec![Rgba::transparent(); count],
            width,
            height,
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            visible: true,
            z_order: 0,
        }
    }

    /// Fill every pixel with `color` (builder pattern).
    pub fn fill(mut self, color: Rgba) -> Self {
        self.pixels.fill(color);
        self
    }

    /// Return the pixel at `(x, y)`, or `None` if out of bounds.
    pub fn pixel_at(&self, x: u32, y: u32) -> Option<Rgba> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.pixels.get((y * self.width + x) as usize).copied()
    }

    /// Set the pixel at `(x, y)`.  Returns `true` on success.
    pub fn set_pixel(&mut self, x: u32, y: u32, color: Rgba) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let idx = (y * self.width + x) as usize;
        if let Some(p) = self.pixels.get_mut(idx) {
            *p = color;
            true
        } else {
            false
        }
    }
}

// ─── CompositeStats ───────────────────────────────────────────────────────────

/// Per-channel statistics over a composited image.
#[derive(Debug, Clone)]
pub struct CompositeStats {
    pub min_r: f32,
    pub max_r: f32,
    pub mean_r: f32,
    pub min_g: f32,
    pub max_g: f32,
    pub mean_g: f32,
    pub min_b: f32,
    pub max_b: f32,
    pub mean_b: f32,
    pub min_a: f32,
    pub max_a: f32,
    pub mean_a: f32,
    /// Number of pixels where alpha < `1e-4` (effectively transparent).
    pub transparent_pixel_count: u64,
}

// ─── TileCompositor ───────────────────────────────────────────────────────────

/// Composites a stack of [`Layer`]s into a single image tile.
pub struct TileCompositor {
    pub width: u32,
    pub height: u32,
    /// Background colour drawn beneath all layers.
    pub background: Rgba,
}

impl TileCompositor {
    /// Create a new compositor for tiles of the given dimensions.
    pub fn new(width: u32, height: u32, background: Rgba) -> Self {
        Self {
            width,
            height,
            background,
        }
    }

    /// Composite `layers` in ascending `z_order` order.
    ///
    /// Invisible layers are skipped.  Each layer's per-pixel alpha is
    /// multiplied by `layer.opacity` before blending.
    pub fn composite(&self, layers: &mut [Layer]) -> Vec<Rgba> {
        let pixel_count = (self.width * self.height) as usize;

        // Initialise with the background colour.
        let mut canvas: Vec<Rgba> = vec![self.background; pixel_count];

        // Sort layers by z_order (stable sort preserves insertion order for ties).
        layers.sort_by_key(|l| l.z_order);

        for layer in layers.iter() {
            if !layer.visible {
                continue;
            }
            if layer.width != self.width || layer.height != self.height {
                // Skip layers with mismatched dimensions.
                continue;
            }

            let opacity = layer.opacity.clamp(0.0, 1.0);

            for (i, canvas_pixel) in canvas.iter_mut().enumerate() {
                let src = match layer.pixels.get(i) {
                    Some(&p) => p,
                    None => continue,
                };
                // Apply layer opacity to the source alpha.
                let src = Rgba {
                    a: src.a * opacity,
                    ..src
                };
                *canvas_pixel = layer.blend_mode.blend(src, *canvas_pixel);
            }
        }

        canvas
    }

    /// Convert a pixel slice to interleaved RGBA bytes (4 bytes per pixel).
    pub fn to_rgba_bytes(pixels: &[Rgba]) -> Vec<u8> {
        let mut out = Vec::with_capacity(pixels.len() * 4);
        for p in pixels {
            let (r, g, b, a) = p.to_u8();
            out.push(r);
            out.push(g);
            out.push(b);
            out.push(a);
        }
        out
    }

    /// Convert a pixel slice to interleaved RGB bytes (3 bytes per pixel),
    /// discarding the alpha channel.
    pub fn to_rgb_bytes(pixels: &[Rgba]) -> Vec<u8> {
        let mut out = Vec::with_capacity(pixels.len() * 3);
        for p in pixels {
            let (r, g, b, _) = p.to_u8();
            out.push(r);
            out.push(g);
            out.push(b);
        }
        out
    }

    /// Compute per-channel statistics on a composited pixel slice.
    pub fn stats(pixels: &[Rgba]) -> CompositeStats {
        if pixels.is_empty() {
            return CompositeStats {
                min_r: 0.0,
                max_r: 0.0,
                mean_r: 0.0,
                min_g: 0.0,
                max_g: 0.0,
                mean_g: 0.0,
                min_b: 0.0,
                max_b: 0.0,
                mean_b: 0.0,
                min_a: 0.0,
                max_a: 0.0,
                mean_a: 0.0,
                transparent_pixel_count: 0,
            };
        }

        let n = pixels.len();
        let mut min_r = f32::MAX;
        let mut max_r = f32::MIN;
        let mut sum_r = 0.0_f64;
        let mut min_g = f32::MAX;
        let mut max_g = f32::MIN;
        let mut sum_g = 0.0_f64;
        let mut min_b = f32::MAX;
        let mut max_b = f32::MIN;
        let mut sum_b = 0.0_f64;
        let mut min_a = f32::MAX;
        let mut max_a = f32::MIN;
        let mut sum_a = 0.0_f64;
        let mut transparent_count: u64 = 0;

        for p in pixels {
            min_r = min_r.min(p.r);
            max_r = max_r.max(p.r);
            sum_r += p.r as f64;
            min_g = min_g.min(p.g);
            max_g = max_g.max(p.g);
            sum_g += p.g as f64;
            min_b = min_b.min(p.b);
            max_b = max_b.max(p.b);
            sum_b += p.b as f64;
            min_a = min_a.min(p.a);
            max_a = max_a.max(p.a);
            sum_a += p.a as f64;
            if p.a < 1e-4 {
                transparent_count += 1;
            }
        }

        let n_f64 = n as f64;
        CompositeStats {
            min_r,
            max_r,
            mean_r: (sum_r / n_f64) as f32,
            min_g,
            max_g,
            mean_g: (sum_g / n_f64) as f32,
            min_b,
            max_b,
            mean_b: (sum_b / n_f64) as f32,
            min_a,
            max_a,
            mean_a: (sum_a / n_f64) as f32,
            transparent_pixel_count: transparent_count,
        }
    }
}

// ─── ColorMatrix ─────────────────────────────────────────────────────────────

/// A 4×5 colour transformation matrix.
///
/// Each output channel is:
/// ```text
/// out_R = m[0][0]*R + m[0][1]*G + m[0][2]*B + m[0][3]*A + m[0][4]
/// out_G = m[1][0]*R + m[1][1]*G + m[1][2]*B + m[1][3]*A + m[1][4]
/// out_B = m[2][0]*R + m[2][1]*G + m[2][2]*B + m[2][3]*A + m[2][4]
/// out_A = m[3][0]*R + m[3][1]*G + m[3][2]*B + m[3][3]*A + m[3][4]
/// ```
#[derive(Debug, Clone, Copy)]
pub struct ColorMatrix {
    /// Row-major 4×5 matrix: rows are [R, G, B, A] output channels.
    pub matrix: [[f32; 5]; 4],
}

impl ColorMatrix {
    /// Identity matrix — no change to colours.
    pub fn identity() -> Self {
        Self {
            matrix: [
                [1.0, 0.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 1.0, 0.0],
            ],
        }
    }

    /// Scale all RGB channels by `factor`.  Alpha is unchanged.
    pub fn brightness(factor: f32) -> Self {
        Self {
            matrix: [
                [factor, 0.0, 0.0, 0.0, 0.0],
                [0.0, factor, 0.0, 0.0, 0.0],
                [0.0, 0.0, factor, 0.0, 0.0],
                [0.0, 0.0, 0.0, 1.0, 0.0],
            ],
        }
    }

    /// Contrast adjustment around the midpoint `0.5`.
    ///
    /// A `factor` of `1.0` is a no-op; values above `1.0` increase contrast.
    pub fn contrast(factor: f32) -> Self {
        let offset = 0.5 * (1.0 - factor);
        Self {
            matrix: [
                [factor, 0.0, 0.0, 0.0, offset],
                [0.0, factor, 0.0, 0.0, offset],
                [0.0, 0.0, factor, 0.0, offset],
                [0.0, 0.0, 0.0, 1.0, 0.0],
            ],
        }
    }

    /// Saturation adjustment.
    ///
    /// Uses ITU-R BT.601 luma weights.  `factor = 0` → greyscale, `factor = 1`
    /// → no change, `factor > 1` → more saturated.
    pub fn saturation(factor: f32) -> Self {
        // BT.601 luma weights
        let lr = 0.2126_f32;
        let lg = 0.7152_f32;
        let lb = 0.0722_f32;

        let sr = (1.0 - factor) * lr;
        let sg = (1.0 - factor) * lg;
        let sb = (1.0 - factor) * lb;

        Self {
            matrix: [
                [sr + factor, sg, sb, 0.0, 0.0],
                [sr, sg + factor, sb, 0.0, 0.0],
                [sr, sg, sb + factor, 0.0, 0.0],
                [0.0, 0.0, 0.0, 1.0, 0.0],
            ],
        }
    }

    /// Hue rotation by `degrees`.
    ///
    /// Implemented as a rotation in the colour-opponent plane after projecting
    /// out the luminance axis (Hacker-level approximation suitable for
    /// real-time use).
    pub fn hue_rotate(degrees: f32) -> Self {
        let rad = degrees.to_radians();
        let cos = rad.cos();
        let sin = rad.sin();

        // BT.601 luma weights.
        let lr = 0.2126_f32;
        let lg = 0.7152_f32;
        let lb = 0.0722_f32;

        Self {
            matrix: [
                [
                    lr + cos * (1.0 - lr) - sin * lr,
                    lg + cos * (-lg) - sin * lg,
                    lb + cos * (-lb) - sin * (1.0 - lb),
                    0.0,
                    0.0,
                ],
                [
                    lr + cos * (-lr) + sin * 0.143,
                    lg + cos * (1.0 - lg) + sin * 0.140,
                    lb + cos * (-lb) - sin * 0.283,
                    0.0,
                    0.0,
                ],
                [
                    lr + cos * (-lr) - sin * (1.0 - lr),
                    lg + cos * (-lg) + sin * lg,
                    lb + cos * (1.0 - lb) + sin * lb,
                    0.0,
                    0.0,
                ],
                [0.0, 0.0, 0.0, 1.0, 0.0],
            ],
        }
    }

    /// Invert all RGB channels (`1.0 - channel`).  Alpha is unchanged.
    pub fn invert() -> Self {
        Self {
            matrix: [
                [-1.0, 0.0, 0.0, 0.0, 1.0],
                [0.0, -1.0, 0.0, 0.0, 1.0],
                [0.0, 0.0, -1.0, 0.0, 1.0],
                [0.0, 0.0, 0.0, 1.0, 0.0],
            ],
        }
    }

    /// Greyscale conversion using ITU-R BT.601 luma weights.
    pub fn grayscale() -> Self {
        let lr = 0.2126_f32;
        let lg = 0.7152_f32;
        let lb = 0.0722_f32;
        Self {
            matrix: [
                [lr, lg, lb, 0.0, 0.0],
                [lr, lg, lb, 0.0, 0.0],
                [lr, lg, lb, 0.0, 0.0],
                [0.0, 0.0, 0.0, 1.0, 0.0],
            ],
        }
    }

    /// Sepia-tone matrix (classic film emulation).
    pub fn sepia() -> Self {
        Self {
            matrix: [
                [0.393, 0.769, 0.189, 0.0, 0.0],
                [0.349, 0.686, 0.168, 0.0, 0.0],
                [0.272, 0.534, 0.131, 0.0, 0.0],
                [0.0, 0.0, 0.0, 1.0, 0.0],
            ],
        }
    }

    /// Apply the matrix to a single pixel, clamping the result to `[0, 1]`.
    pub fn apply(&self, pixel: Rgba) -> Rgba {
        let m = &self.matrix;
        let r =
            m[0][0] * pixel.r + m[0][1] * pixel.g + m[0][2] * pixel.b + m[0][3] * pixel.a + m[0][4];
        let g =
            m[1][0] * pixel.r + m[1][1] * pixel.g + m[1][2] * pixel.b + m[1][3] * pixel.a + m[1][4];
        let b =
            m[2][0] * pixel.r + m[2][1] * pixel.g + m[2][2] * pixel.b + m[2][3] * pixel.a + m[2][4];
        let a =
            m[3][0] * pixel.r + m[3][1] * pixel.g + m[3][2] * pixel.b + m[3][3] * pixel.a + m[3][4];

        Rgba::new(r, g, b, a).clamp()
    }

    /// Compose two matrices: `self ∘ other` (apply `other` first, then `self`).
    ///
    /// The 4×5 matrices are augmented to 5×5 (with an implicit row \[0,0,0,0,1\])
    /// for standard homogeneous composition, then the result is trimmed back
    /// to 4×5.
    pub fn compose(&self, other: &ColorMatrix) -> ColorMatrix {
        let a = &self.matrix;
        let b = &other.matrix;

        let mut out = [[0.0_f32; 5]; 4];

        for i in 0..4 {
            for j in 0..5 {
                // The 5th implicit row of `b` is [0,0,0,0,1].
                let mut sum = 0.0_f32;
                for k in 0..4 {
                    sum += a[i][k] * b[k][j];
                }
                // j == 4 is the offset column; the implicit row contributes
                // a[i][4] * 1.0 for j == 4, zero otherwise.
                if j == 4 {
                    sum += a[i][4];
                }
                out[i][j] = sum;
            }
        }

        ColorMatrix { matrix: out }
    }
}

// ─── TileRenderPipeline ───────────────────────────────────────────────────────

/// High-level CPU rendering pipeline: shader hot-reload + layer compositing
/// + optional colour-matrix post-processing.
pub struct TileRenderPipeline {
    pub compositor: TileCompositor,
    /// Default colour matrix applied by [`Self::render`] (identity by default).
    pub color_matrix: ColorMatrix,
    pub shader_registry: HotReloadRegistry,
}

impl TileRenderPipeline {
    /// Create a new pipeline for a tile of the given size.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            compositor: TileCompositor::new(width, height, Rgba::transparent()),
            color_matrix: ColorMatrix::identity(),
            shader_registry: HotReloadRegistry::new(),
        }
    }

    /// Register a WGSL shader source with the hot-reload registry.
    pub fn add_shader(&mut self, label: impl Into<String>, wgsl: impl Into<String>) {
        let lbl: String = label.into();
        self.shader_registry.watcher.add_inline(lbl, wgsl);
    }

    /// Update a registered shader source, bumping its version.
    ///
    /// Returns `true` if the label existed; `false` otherwise.
    pub fn update_shader(&mut self, label: &str, new_wgsl: impl Into<String>) -> bool {
        self.shader_registry.watcher.update_source(label, new_wgsl)
    }

    /// Composite `layers`, apply the pipeline's default colour matrix, and
    /// return the result as interleaved RGBA bytes.
    pub fn render(&self, layers: &mut [Layer]) -> Vec<u8> {
        self.render_with_matrix(layers, &self.color_matrix)
    }

    /// Composite `layers`, apply `matrix`, and return RGBA bytes.
    pub fn render_with_matrix(&self, layers: &mut [Layer], matrix: &ColorMatrix) -> Vec<u8> {
        let pixels = self.compositor.composite(layers);
        let transformed: Vec<Rgba> = pixels.iter().map(|p| matrix.apply(*p)).collect();
        TileCompositor::to_rgba_bytes(&transformed)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f32 = 1e-4;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    fn rgba_approx(a: Rgba, b: Rgba) -> bool {
        approx_eq(a.r, b.r) && approx_eq(a.g, b.g) && approx_eq(a.b, b.b) && approx_eq(a.a, b.a)
    }

    // ── Rgba ──────────────────────────────────────────────────────────────────

    #[test]
    fn test_rgba_from_u8_round_trip() {
        let (r, g, b, a) = (128_u8, 64_u8, 255_u8, 200_u8);
        let px = Rgba::from_u8(r, g, b, a);
        let (ro, go, bo, ao) = px.to_u8();
        assert_eq!(ro, r);
        assert_eq!(go, g);
        assert_eq!(bo, b);
        assert_eq!(ao, a);
    }

    #[test]
    fn test_rgba_from_u8_black() {
        let px = Rgba::from_u8(0, 0, 0, 255);
        assert!(approx_eq(px.r, 0.0));
        assert!(approx_eq(px.a, 1.0));
    }

    #[test]
    fn test_rgba_from_u8_white() {
        let px = Rgba::from_u8(255, 255, 255, 255);
        assert!(approx_eq(px.r, 1.0));
        assert!(approx_eq(px.g, 1.0));
    }

    #[test]
    fn test_rgba_to_u8_clamps_over() {
        let px = Rgba::new(1.5, -0.5, 0.5, 1.0);
        let (r, _g, b, _a) = px.to_u8();
        assert_eq!(r, 255);
        assert_eq!(b, 128);
    }

    #[test]
    fn test_rgba_clamp() {
        let px = Rgba::new(1.5, -0.1, 0.5, 2.0).clamp();
        assert!(approx_eq(px.r, 1.0));
        assert!(approx_eq(px.g, 0.0));
        assert!(approx_eq(px.b, 0.5));
        assert!(approx_eq(px.a, 1.0));
    }

    #[test]
    fn test_rgba_premultiply() {
        let px = Rgba::new(1.0, 0.5, 0.25, 0.5);
        let pre = px.premultiply();
        assert!(approx_eq(pre.r, 0.5));
        assert!(approx_eq(pre.g, 0.25));
        assert!(approx_eq(pre.b, 0.125));
        assert!(approx_eq(pre.a, 0.5));
    }

    #[test]
    fn test_rgba_premultiply_full_alpha() {
        let px = Rgba::new(0.3, 0.6, 0.9, 1.0);
        let pre = px.premultiply();
        assert!(approx_eq(pre.r, 0.3));
        assert!(approx_eq(pre.g, 0.6));
        assert!(approx_eq(pre.b, 0.9));
    }

    #[test]
    fn test_rgba_unpremultiply() {
        let px = Rgba::new(0.5, 0.25, 0.125, 0.5);
        let straight = px.unpremultiply();
        assert!(approx_eq(straight.r, 1.0));
        assert!(approx_eq(straight.g, 0.5));
        assert!(approx_eq(straight.b, 0.25));
    }

    #[test]
    fn test_rgba_unpremultiply_zero_alpha() {
        let px = Rgba::new(0.5, 0.5, 0.5, 0.0);
        let straight = px.unpremultiply();
        assert!(approx_eq(straight.r, 0.0));
        assert!(approx_eq(straight.a, 0.0));
    }

    #[test]
    fn test_rgba_premultiply_unpremultiply_round_trip() {
        let px = Rgba::new(0.8, 0.4, 0.2, 0.6);
        let recovered = px.premultiply().unpremultiply();
        assert!(rgba_approx(px, recovered));
    }

    // ── BlendMode ─────────────────────────────────────────────────────────────

    #[test]
    fn test_blend_normal_src_over() {
        let src = Rgba::new(1.0, 0.0, 0.0, 1.0);
        let dst = Rgba::new(0.0, 1.0, 0.0, 1.0);
        let result = BlendMode::Normal.blend(src, dst);
        // Fully opaque src → result equals src.
        assert!(rgba_approx(result, src));
    }

    #[test]
    fn test_blend_src_over_transparent_src() {
        let src = Rgba::new(1.0, 0.0, 0.0, 0.0);
        let dst = Rgba::new(0.0, 1.0, 0.0, 1.0);
        let result = BlendMode::SrcOver.blend(src, dst);
        assert!(rgba_approx(result, dst));
    }

    #[test]
    fn test_blend_src_over_half_alpha() {
        let src = Rgba::new(1.0, 0.0, 0.0, 0.5);
        let dst = Rgba::new(0.0, 0.0, 1.0, 1.0);
        let result = BlendMode::SrcOver.blend(src, dst);
        // a_out = 0.5 + 1.0 * 0.5 = 1.0
        assert!(approx_eq(result.a, 1.0));
        // r_out = (1.0 * 0.5 + 0.0 * 1.0 * 0.5) / 1.0 = 0.5
        assert!(approx_eq(result.r, 0.5));
        // b_out = (0.0 * 0.5 + 1.0 * 1.0 * 0.5) / 1.0 = 0.5
        assert!(approx_eq(result.b, 0.5));
    }

    #[test]
    fn test_blend_multiply_black_src() {
        let src = Rgba::new(0.0, 0.0, 0.0, 1.0);
        let dst = Rgba::new(0.8, 0.5, 0.3, 1.0);
        let result = BlendMode::Multiply.blend(src, dst);
        assert!(approx_eq(result.r, 0.0));
        assert!(approx_eq(result.g, 0.0));
        assert!(approx_eq(result.b, 0.0));
    }

    #[test]
    fn test_blend_multiply_white_src() {
        let src = Rgba::new(1.0, 1.0, 1.0, 1.0);
        let dst = Rgba::new(0.5, 0.5, 0.5, 1.0);
        let result = BlendMode::Multiply.blend(src, dst);
        // Multiply with 1.0 is identity-ish via src_over; final blend ≈ dst.
        assert!(approx_eq(result.r, 0.5));
    }

    #[test]
    fn test_blend_screen_white_src() {
        let src = Rgba::new(1.0, 1.0, 1.0, 1.0);
        let dst = Rgba::new(0.5, 0.5, 0.5, 1.0);
        // Screen with fully opaque white → result is white (1,1,1).
        let result = BlendMode::Screen.blend(src, dst);
        assert!(approx_eq(result.r, 1.0));
    }

    #[test]
    fn test_blend_darken_picks_darker() {
        let src = Rgba::new(0.3, 0.7, 0.2, 1.0);
        let dst = Rgba::new(0.5, 0.4, 0.8, 1.0);
        let result = BlendMode::Darken.blend(src, dst);
        // Each channel should be ≤ both inputs.
        assert!(result.r <= src.r + EPSILON && result.r <= dst.r + EPSILON);
        assert!(result.g <= src.g + EPSILON && result.g <= dst.g + EPSILON);
        assert!(result.b <= src.b + EPSILON && result.b <= dst.b + EPSILON);
    }

    #[test]
    fn test_blend_lighten_picks_lighter() {
        let src = Rgba::new(0.3, 0.7, 0.2, 1.0);
        let dst = Rgba::new(0.5, 0.4, 0.8, 1.0);
        let result = BlendMode::Lighten.blend(src, dst);
        assert!(result.r >= src.r - EPSILON && result.r >= dst.r - EPSILON);
        assert!(result.g >= src.g - EPSILON && result.g >= dst.g - EPSILON);
        assert!(result.b >= src.b - EPSILON && result.b >= dst.b - EPSILON);
    }

    #[test]
    fn test_blend_difference_same_color() {
        let color = Rgba::new(0.5, 0.3, 0.8, 1.0);
        let result = BlendMode::Difference.blend(color, color);
        // Difference of identical colours → black (RGB all 0).
        assert!(approx_eq(result.r, 0.0));
        assert!(approx_eq(result.g, 0.0));
        assert!(approx_eq(result.b, 0.0));
    }

    #[test]
    fn test_blend_clear() {
        let src = Rgba::new(1.0, 0.5, 0.0, 1.0);
        let dst = Rgba::new(0.0, 1.0, 0.0, 1.0);
        let result = BlendMode::Clear.blend(src, dst);
        assert!(approx_eq(result.a, 0.0));
    }

    #[test]
    fn test_blend_src_in() {
        // SrcIn: only where both src and dst exist.
        let src = Rgba::new(1.0, 0.0, 0.0, 0.5);
        let dst = Rgba::new(0.0, 1.0, 0.0, 0.6);
        let result = BlendMode::SrcIn.blend(src, dst);
        // a_out = src.a * dst.a = 0.3
        assert!(approx_eq(result.a, 0.3));
    }

    #[test]
    fn test_blend_src_out() {
        let src = Rgba::new(1.0, 0.0, 0.0, 1.0);
        let dst = Rgba::new(0.0, 1.0, 0.0, 1.0);
        let result = BlendMode::SrcOut.blend(src, dst);
        // SrcOut: src.a * (1 - dst.a) → 0 if dst is fully opaque.
        assert!(approx_eq(result.a, 0.0));
    }

    #[test]
    fn test_blend_dst_over() {
        // DstOver: dst paints over src (swap).
        let src = Rgba::new(1.0, 0.0, 0.0, 0.5);
        let dst = Rgba::new(0.0, 0.0, 1.0, 1.0);
        let result = BlendMode::DstOver.blend(src, dst);
        // dst is opaque → result is dst.
        assert!(rgba_approx(result, dst));
    }

    #[test]
    fn test_blend_xor() {
        // Xor with fully opaque src and dst → transparent.
        let src = Rgba::new(1.0, 0.0, 0.0, 1.0);
        let dst = Rgba::new(0.0, 1.0, 0.0, 1.0);
        let result = BlendMode::Xor.blend(src, dst);
        // Both alphas = 1 → 1*(1-1) + 1*(1-1) = 0.
        assert!(approx_eq(result.a, 0.0));
    }

    #[test]
    fn test_blend_exclusion() {
        let src = Rgba::new(0.5, 0.5, 0.5, 1.0);
        let dst = Rgba::new(0.5, 0.5, 0.5, 1.0);
        // Exclusion with same colour: s + d - 2sd = 0.5 + 0.5 - 2*0.25 = 0.5.
        let result = BlendMode::Exclusion.blend(src, dst);
        assert!(approx_eq(result.r, 0.5));
    }

    // ── Layer ─────────────────────────────────────────────────────────────────

    #[test]
    fn test_layer_new_transparent() {
        let layer = Layer::new("base", 4, 4);
        for px in &layer.pixels {
            assert!(approx_eq(px.a, 0.0));
        }
    }

    #[test]
    fn test_layer_fill() {
        let color = Rgba::new(0.5, 0.0, 1.0, 1.0);
        let layer = Layer::new("l", 2, 2).fill(color);
        for px in &layer.pixels {
            assert!(rgba_approx(*px, color));
        }
    }

    #[test]
    fn test_layer_pixel_at_in_bounds() {
        let layer = Layer::new("l", 4, 4).fill(Rgba::white());
        let px = layer.pixel_at(2, 3);
        assert!(px.is_some());
        assert!(rgba_approx(px.expect("should be Some"), Rgba::white()));
    }

    #[test]
    fn test_layer_pixel_at_out_of_bounds() {
        let layer = Layer::new("l", 4, 4);
        assert!(layer.pixel_at(4, 0).is_none());
        assert!(layer.pixel_at(0, 4).is_none());
    }

    #[test]
    fn test_layer_set_pixel() {
        let mut layer = Layer::new("l", 4, 4);
        let ok = layer.set_pixel(1, 2, Rgba::white());
        assert!(ok);
        assert!(rgba_approx(
            layer.pixel_at(1, 2).expect("should be Some"),
            Rgba::white()
        ));
    }

    #[test]
    fn test_layer_set_pixel_out_of_bounds() {
        let mut layer = Layer::new("l", 4, 4);
        assert!(!layer.set_pixel(10, 10, Rgba::white()));
    }

    // ── TileCompositor ────────────────────────────────────────────────────────

    #[test]
    fn test_compositor_empty_layers() {
        let comp = TileCompositor::new(2, 2, Rgba::black());
        let mut layers: Vec<Layer> = vec![];
        let out = comp.composite(&mut layers);
        assert_eq!(out.len(), 4);
        for px in out {
            assert!(rgba_approx(px, Rgba::black()));
        }
    }

    #[test]
    fn test_compositor_single_opaque_layer() {
        let comp = TileCompositor::new(2, 2, Rgba::transparent());
        let layer = Layer::new("l", 2, 2).fill(Rgba::white());
        let mut layers = vec![layer];
        let out = comp.composite(&mut layers);
        for px in out {
            assert!(rgba_approx(px, Rgba::white()));
        }
    }

    #[test]
    fn test_compositor_z_order_respected() {
        let comp = TileCompositor::new(1, 1, Rgba::transparent());
        let bottom = Layer::new("bottom", 1, 1).fill(Rgba::new(0.0, 0.0, 1.0, 1.0));
        let top = {
            let mut l = Layer::new("top", 1, 1);
            l.z_order = 1;
            l.fill(Rgba::new(1.0, 0.0, 0.0, 1.0))
        };
        let mut layers = vec![top, bottom]; // note reversed insertion order
        let out = comp.composite(&mut layers);
        // Top (red, z=1) should completely cover bottom (blue, z=0).
        assert!(approx_eq(out[0].r, 1.0));
        assert!(approx_eq(out[0].b, 0.0));
    }

    #[test]
    fn test_compositor_invisible_layer_skipped() {
        let comp = TileCompositor::new(1, 1, Rgba::black());
        let mut invisible = Layer::new("inv", 1, 1).fill(Rgba::white());
        invisible.visible = false;
        let mut layers = vec![invisible];
        let out = comp.composite(&mut layers);
        // Background should remain black.
        assert!(rgba_approx(out[0], Rgba::black()));
    }

    #[test]
    fn test_compositor_opacity_scales_alpha() {
        let comp = TileCompositor::new(1, 1, Rgba::transparent());
        let mut layer = Layer::new("l", 1, 1).fill(Rgba::new(1.0, 0.0, 0.0, 1.0));
        layer.opacity = 0.5;
        let mut layers = vec![layer];
        let out = comp.composite(&mut layers);
        // Red channel will be 0.5 * 1.0 (src.a = 0.5 → src_over with transparent).
        assert!(approx_eq(out[0].a, 0.5));
    }

    #[test]
    fn test_compositor_to_rgba_bytes_length() {
        let pixels = vec![Rgba::white(); 10];
        let bytes = TileCompositor::to_rgba_bytes(&pixels);
        assert_eq!(bytes.len(), 40);
    }

    #[test]
    fn test_compositor_to_rgb_bytes_length() {
        let pixels = vec![Rgba::white(); 10];
        let bytes = TileCompositor::to_rgb_bytes(&pixels);
        assert_eq!(bytes.len(), 30);
    }

    #[test]
    fn test_compositor_to_rgba_bytes_values() {
        let pixels = vec![Rgba::from_u8(100, 150, 200, 255)];
        let bytes = TileCompositor::to_rgba_bytes(&pixels);
        assert_eq!(bytes[0], 100);
        assert_eq!(bytes[1], 150);
        assert_eq!(bytes[2], 200);
        assert_eq!(bytes[3], 255);
    }

    #[test]
    fn test_compositor_stats_mean_r() {
        let pixels = vec![Rgba::new(0.0, 0.0, 0.0, 1.0), Rgba::new(1.0, 0.0, 0.0, 1.0)];
        let s = TileCompositor::stats(&pixels);
        assert!(approx_eq(s.mean_r, 0.5));
    }

    #[test]
    fn test_compositor_stats_transparent_count() {
        let pixels = vec![
            Rgba::new(0.0, 0.0, 0.0, 0.0),
            Rgba::new(0.0, 0.0, 0.0, 0.0),
            Rgba::new(1.0, 1.0, 1.0, 1.0),
        ];
        let s = TileCompositor::stats(&pixels);
        assert_eq!(s.transparent_pixel_count, 2);
    }

    #[test]
    fn test_compositor_stats_empty() {
        let s = TileCompositor::stats(&[]);
        assert!(approx_eq(s.min_r, 0.0));
        assert!(approx_eq(s.mean_r, 0.0));
    }

    #[test]
    fn test_compositor_stats_min_max() {
        let pixels = vec![Rgba::new(0.1, 0.2, 0.3, 0.4), Rgba::new(0.9, 0.8, 0.7, 0.6)];
        let s = TileCompositor::stats(&pixels);
        assert!(approx_eq(s.min_r, 0.1));
        assert!(approx_eq(s.max_r, 0.9));
        assert!(approx_eq(s.min_g, 0.2));
        assert!(approx_eq(s.max_g, 0.8));
    }

    // ── ColorMatrix ───────────────────────────────────────────────────────────

    #[test]
    fn test_color_matrix_identity_noop() {
        let m = ColorMatrix::identity();
        let px = Rgba::new(0.4, 0.6, 0.2, 0.8);
        let out = m.apply(px);
        assert!(rgba_approx(out, px));
    }

    #[test]
    fn test_color_matrix_brightness_doubles() {
        let m = ColorMatrix::brightness(2.0);
        let px = Rgba::new(0.2, 0.3, 0.4, 1.0);
        let out = m.apply(px);
        // 0.4, 0.6, 0.8 (clamped at 1.0 if needed)
        assert!(approx_eq(out.r, 0.4));
        assert!(approx_eq(out.g, 0.6));
        assert!(approx_eq(out.b, 0.8));
    }

    #[test]
    fn test_color_matrix_brightness_zero() {
        let m = ColorMatrix::brightness(0.0);
        let px = Rgba::new(1.0, 1.0, 1.0, 1.0);
        let out = m.apply(px);
        assert!(approx_eq(out.r, 0.0));
        assert!(approx_eq(out.g, 0.0));
        assert!(approx_eq(out.b, 0.0));
        assert!(approx_eq(out.a, 1.0));
    }

    #[test]
    fn test_color_matrix_invert() {
        let m = ColorMatrix::invert();
        let px = Rgba::new(0.2, 0.5, 0.8, 1.0);
        let out = m.apply(px);
        assert!(approx_eq(out.r, 0.8));
        assert!(approx_eq(out.g, 0.5));
        assert!(approx_eq(out.b, 0.2));
        assert!(approx_eq(out.a, 1.0));
    }

    #[test]
    fn test_color_matrix_invert_twice_is_identity() {
        let m = ColorMatrix::invert().compose(&ColorMatrix::invert());
        let px = Rgba::new(0.3, 0.6, 0.9, 0.7);
        let out = m.apply(px);
        assert!(rgba_approx(out, px));
    }

    #[test]
    fn test_color_matrix_grayscale_equal_channels() {
        let m = ColorMatrix::grayscale();
        let px = Rgba::new(0.6, 0.4, 0.2, 1.0);
        let out = m.apply(px);
        // All output RGB channels equal (grayscale).
        assert!(approx_eq(out.r, out.g));
        assert!(approx_eq(out.g, out.b));
    }

    #[test]
    fn test_color_matrix_grayscale_white_stays_white() {
        let m = ColorMatrix::grayscale();
        let out = m.apply(Rgba::white());
        assert!(approx_eq(out.r, 1.0));
        assert!(approx_eq(out.g, 1.0));
        assert!(approx_eq(out.b, 1.0));
    }

    #[test]
    fn test_color_matrix_compose_identity() {
        let any_m = ColorMatrix::brightness(1.5);
        let composed = any_m.compose(&ColorMatrix::identity());
        let px = Rgba::new(0.3, 0.4, 0.5, 1.0);
        let a = any_m.apply(px);
        let b = composed.apply(px);
        assert!(rgba_approx(a, b));
    }

    #[test]
    fn test_color_matrix_compose_identity_left() {
        let any_m = ColorMatrix::contrast(1.5);
        let composed = ColorMatrix::identity().compose(&any_m);
        let px = Rgba::new(0.3, 0.4, 0.5, 1.0);
        let a = any_m.apply(px);
        let b = composed.apply(px);
        assert!(rgba_approx(a, b));
    }

    #[test]
    fn test_color_matrix_saturation_zero_is_grayscale() {
        let gray = ColorMatrix::grayscale();
        let sat0 = ColorMatrix::saturation(0.0);
        let px = Rgba::new(0.5, 0.3, 0.7, 1.0);
        let a = gray.apply(px);
        let b = sat0.apply(px);
        // Both should produce the same luma value in all channels.
        assert!(approx_eq(a.r, b.r));
    }

    #[test]
    fn test_color_matrix_contrast_identity() {
        let m = ColorMatrix::contrast(1.0);
        let px = Rgba::new(0.4, 0.6, 0.8, 1.0);
        assert!(rgba_approx(m.apply(px), px));
    }

    #[test]
    fn test_color_matrix_sepia_non_zero() {
        let m = ColorMatrix::sepia();
        let px = Rgba::new(0.5, 0.5, 0.5, 1.0);
        let out = m.apply(px);
        assert!(out.r > 0.0);
        assert!(out.g > 0.0);
        assert!(out.b > 0.0);
    }

    // ── TileRenderPipeline ─────────────────────────────────────────────────

    #[test]
    fn test_pipeline_render_byte_length() {
        let pipeline = TileRenderPipeline::new(4, 4);
        let mut layers = vec![Layer::new("l", 4, 4).fill(Rgba::white())];
        let bytes = pipeline.render(&mut layers);
        assert_eq!(bytes.len(), 4 * 4 * 4); // 64
    }

    #[test]
    fn test_pipeline_render_with_matrix() {
        let pipeline = TileRenderPipeline::new(2, 2);
        let mut layers = vec![Layer::new("l", 2, 2).fill(Rgba::new(0.5, 0.5, 0.5, 1.0))];
        let matrix = ColorMatrix::brightness(2.0);
        let bytes = pipeline.render_with_matrix(&mut layers, &matrix);
        // Bytes should be 16 (2x2 * 4 channels).
        assert_eq!(bytes.len(), 16);
        // First pixel R should be clamped to 255.
        assert_eq!(bytes[0], 255);
    }

    #[test]
    fn test_pipeline_add_shader() {
        let mut pipeline = TileRenderPipeline::new(4, 4);
        pipeline.add_shader("my_shader", "@compute fn main() {}");
        assert!(
            pipeline
                .shader_registry
                .watcher
                .get_source("my_shader")
                .is_some()
        );
    }

    #[test]
    fn test_pipeline_update_shader() {
        let mut pipeline = TileRenderPipeline::new(4, 4);
        pipeline.add_shader("s", "@compute fn main() {}");
        let ok = pipeline.update_shader("s", "@compute fn main_v2() {}");
        assert!(ok);
        assert_eq!(
            pipeline.shader_registry.watcher.source_version("s"),
            Some(2)
        );
    }

    #[test]
    fn test_pipeline_update_unknown_shader() {
        let mut pipeline = TileRenderPipeline::new(4, 4);
        assert!(!pipeline.update_shader("ghost", "@compute fn x() {}"));
    }

    #[test]
    fn test_pipeline_render_empty_layers() {
        let pipeline = TileRenderPipeline::new(3, 3);
        let mut layers: Vec<Layer> = vec![];
        let bytes = pipeline.render(&mut layers);
        assert_eq!(bytes.len(), 3 * 3 * 4); // 36
    }
}
