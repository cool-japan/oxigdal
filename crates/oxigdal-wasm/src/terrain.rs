//! In-browser terrain derivatives (hillshade, slope, aspect, colour relief).
//!
//! This module is deliberately **self-contained**: it does not link
//! `oxigdal-terrain` or `scirs2` into the WASM bundle. Instead it ports the
//! exact scalar Horn's-method kernel from
//! `oxigdal-algorithms/src/raster/hillshade.rs` together with the
//! multidirectional azimuth/weight table from
//! `oxigdal-terrain/src/derivatives/hillshade.rs`, so the numbers produced in
//! the browser match the native crates.
//!
//! # Horn's method
//!
//! For an interior cell the 3×3 elevation window `z` yields gradients
//!
//! ```text
//! dz/dx = ((z02 + 2·z12 + z22) − (z00 + 2·z10 + z20)) · z_factor / (8·cell)
//! dz/dy = ((z20 + 2·z21 + z22) − (z00 + 2·z01 + z02)) · z_factor / (8·cell)
//! ```
//!
//! Edges use replicate-edge (nearest-cell) sampling, so a flat DEM produces a
//! perfectly uniform result across the whole grid.

use wasm_bindgen::prelude::*;
use web_sys::ImageData;

use crate::canvas::Rgb;
use crate::color::ColorPalette;

/// Azimuths (degrees) used for multidirectional hillshade — SW, W, NW, N.
///
/// Taken verbatim from `oxigdal-terrain` for numeric parity.
const MULTIDIRECTIONAL_AZIMUTHS: [f64; 4] = [225.0, 270.0, 315.0, 360.0];

/// Weights matching [`MULTIDIRECTIONAL_AZIMUTHS`] (sum to 1.0).
const MULTIDIRECTIONAL_WEIGHTS: [f64; 4] = [0.3, 0.2, 0.2, 0.3];

/// Replicate-edge sample of the elevation grid at signed `(x, y)`.
///
/// Coordinates outside the grid are clamped to the nearest valid cell.
#[inline]
fn sample(elev: &[f32], width: usize, height: usize, x: isize, y: isize) -> f64 {
    let xc = x.clamp(0, width as isize - 1) as usize;
    let yc = y.clamp(0, height as isize - 1) as usize;
    f64::from(elev[yc * width + xc])
}

/// Computes Horn's 3rd-order finite-difference gradients at cell `(x, y)`.
#[inline]
fn horn_gradients(
    elev: &[f32],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    cell_size: f64,
    z_factor: f64,
) -> (f64, f64) {
    let xi = x as isize;
    let yi = y as isize;

    let z00 = sample(elev, width, height, xi - 1, yi - 1);
    let z01 = sample(elev, width, height, xi, yi - 1);
    let z02 = sample(elev, width, height, xi + 1, yi - 1);
    let z10 = sample(elev, width, height, xi - 1, yi);
    let z12 = sample(elev, width, height, xi + 1, yi);
    let z20 = sample(elev, width, height, xi - 1, yi + 1);
    let z21 = sample(elev, width, height, xi, yi + 1);
    let z22 = sample(elev, width, height, xi + 1, yi + 1);

    let scale = z_factor / (8.0 * cell_size);
    let dzdx = ((z02 + 2.0 * z12 + z22) - (z00 + 2.0 * z10 + z20)) * scale;
    let dzdy = ((z20 + 2.0 * z21 + z22) - (z00 + 2.0 * z01 + z02)) * scale;
    (dzdx, dzdy)
}

/// Illumination value in `[0, 1]` for the given gradients and sun position.
///
/// This is the exact scalar formula from `oxigdal-algorithms`.
#[inline]
fn shade_from_gradients(
    dzdx: f64,
    dzdy: f64,
    cos_zenith: f64,
    sin_zenith: f64,
    azimuth_rad: f64,
) -> f64 {
    let slope_rad = (dzdx * dzdx + dzdy * dzdy).sqrt().atan();
    let aspect_rad = if dzdx.abs() < f64::EPSILON && dzdy.abs() < f64::EPSILON {
        0.0
    } else {
        dzdy.atan2(-dzdx)
    };
    let value = (cos_zenith * slope_rad.cos())
        + (sin_zenith * slope_rad.sin() * (azimuth_rad - aspect_rad).cos());
    value.clamp(0.0, 1.0)
}

/// Single-direction hillshade, returning `0..=255` grayscale bytes.
fn compute_hillshade(
    elev: &[f32],
    width: usize,
    height: usize,
    cell_size: f64,
    azimuth: f64,
    altitude: f64,
    z_factor: f64,
) -> Vec<u8> {
    let zenith_rad = (90.0 - altitude).to_radians();
    let azimuth_rad = (360.0 - azimuth + 90.0).to_radians();
    let cos_zenith = zenith_rad.cos();
    let sin_zenith = zenith_rad.sin();

    let mut out = vec![0u8; width * height];
    for y in 0..height {
        for x in 0..width {
            let (dzdx, dzdy) = horn_gradients(elev, width, height, x, y, cell_size, z_factor);
            let shade = shade_from_gradients(dzdx, dzdy, cos_zenith, sin_zenith, azimuth_rad);
            out[y * width + x] = (shade * 255.0).round() as u8;
        }
    }
    out
}

/// Weighted multidirectional shade in `[0, 1]` per cell.
fn compute_multidirectional_shade(
    elev: &[f32],
    width: usize,
    height: usize,
    cell_size: f64,
    altitude: f64,
    z_factor: f64,
) -> Vec<f64> {
    let zenith_rad = (90.0 - altitude).to_radians();
    let cos_zenith = zenith_rad.cos();
    let sin_zenith = zenith_rad.sin();

    let mut out = vec![0.0f64; width * height];
    for (&azimuth, &weight) in MULTIDIRECTIONAL_AZIMUTHS
        .iter()
        .zip(MULTIDIRECTIONAL_WEIGHTS.iter())
    {
        let azimuth_rad = (360.0 - azimuth + 90.0).to_radians();
        for y in 0..height {
            for x in 0..width {
                let (dzdx, dzdy) = horn_gradients(elev, width, height, x, y, cell_size, z_factor);
                let shade = shade_from_gradients(dzdx, dzdy, cos_zenith, sin_zenith, azimuth_rad);
                out[y * width + x] += shade * weight;
            }
        }
    }
    out
}

/// Slope magnitude in degrees per cell.
fn compute_slope(
    elev: &[f32],
    width: usize,
    height: usize,
    cell_size: f64,
    z_factor: f64,
) -> Vec<f32> {
    let mut out = vec![0.0f32; width * height];
    for y in 0..height {
        for x in 0..width {
            let (dzdx, dzdy) = horn_gradients(elev, width, height, x, y, cell_size, z_factor);
            let slope_rad = (dzdx * dzdx + dzdy * dzdy).sqrt().atan();
            out[y * width + x] = slope_rad.to_degrees() as f32;
        }
    }
    out
}

/// Aspect in degrees clockwise from north (`0..360`); flat cells report `0.0`.
fn compute_aspect(elev: &[f32], width: usize, height: usize, cell_size: f64) -> Vec<f32> {
    let mut out = vec![0.0f32; width * height];
    for y in 0..height {
        for x in 0..width {
            // z_factor scales dzdx and dzdy equally and so does not affect the
            // aspect angle; use 1.0.
            let (dzdx, dzdy) = horn_gradients(elev, width, height, x, y, cell_size, 1.0);
            let aspect = if dzdx.abs() < f64::EPSILON && dzdy.abs() < f64::EPSILON {
                0.0
            } else {
                // atan2(dzdy, dzdx) → upslope direction (E=0, CCW); convert to
                // geographic (N=0, CW) as in oxigdal-terrain.
                let mut deg = 90.0 - dzdy.atan2(dzdx).to_degrees();
                if deg < 0.0 {
                    deg += 360.0;
                }
                if deg >= 360.0 {
                    deg -= 360.0;
                }
                deg
            };
            out[y * width + x] = aspect as f32;
        }
    }
    out
}

/// Builds a 256-entry RGB lookup table from a named palette.
fn build_lut(palette: &ColorPalette) -> [Rgb; 256] {
    let default = Rgb::new(0, 0, 0);
    let mut lut = [default; 256];
    for (i, slot) in lut.iter_mut().enumerate() {
        let t = i as f64 / 255.0;
        *slot = palette.interpolate(t).unwrap_or(default);
    }
    lut
}

/// Resolves a palette by name, defaulting to `terrain`.
fn palette_by_name(name: &str) -> ColorPalette {
    match name {
        "viridis" => ColorPalette::viridis(),
        "plasma" => ColorPalette::plasma(),
        "inferno" => ColorPalette::inferno(),
        "terrain" => ColorPalette::terrain(),
        "rainbow" => ColorPalette::rainbow(),
        "rdylbu" => ColorPalette::rdylbu(),
        "grayscale" | "gray" | "grey" => ColorPalette::grayscale(),
        _ => ColorPalette::terrain(),
    }
}

/// Validates dimensions against the buffer length, returning `(width, height)`.
///
/// Errors are plain `String`s (not `JsValue`) so this is safe to call from
/// native unit tests — constructing a `JsValue` aborts off-wasm.
fn checked_dims(elev: &[f32], width: u32, height: u32) -> Result<(usize, usize), String> {
    if width == 0 || height == 0 {
        return Err("WasmTerrain: width and height must be non-zero".to_string());
    }
    let w = width as usize;
    let h = height as usize;
    let n = w
        .checked_mul(h)
        .ok_or_else(|| "WasmTerrain: dimensions overflow".to_string())?;
    if elev.len() < n {
        return Err(format!(
            "WasmTerrain: elevation buffer too small ({} < {}x{}={})",
            elev.len(),
            width,
            height,
            n
        ));
    }
    Ok((w, h))
}

/// Browser-side terrain analysis over a flat `f32` elevation grid.
///
/// All methods are static: they take a row-major `elev` buffer plus `width`
/// and `height`, and never retain state. This keeps the WASM surface small and
/// avoids pulling the native terrain/scirs2 stack into the bundle.
#[wasm_bindgen]
pub struct WasmTerrain;

#[wasm_bindgen]
impl WasmTerrain {
    /// Single-direction hillshade as `0..=255` grayscale bytes.
    #[wasm_bindgen]
    pub fn hillshade(
        elev: &[f32],
        width: u32,
        height: u32,
        cell_size: f64,
        azimuth: f64,
        altitude: f64,
        z_factor: f64,
    ) -> Result<Vec<u8>, JsValue> {
        let (w, h) = checked_dims(elev, width, height).map_err(|e| JsValue::from_str(&e))?;
        Ok(compute_hillshade(
            elev, w, h, cell_size, azimuth, altitude, z_factor,
        ))
    }

    /// Multidirectional hillshade (SW/W/NW/N) as `0..=255` grayscale bytes.
    #[wasm_bindgen(js_name = hillshadeMultidirectional)]
    pub fn hillshade_multidirectional(
        elev: &[f32],
        width: u32,
        height: u32,
        cell_size: f64,
        altitude: f64,
        z_factor: f64,
    ) -> Result<Vec<u8>, JsValue> {
        let (w, h) = checked_dims(elev, width, height).map_err(|e| JsValue::from_str(&e))?;
        let shade = compute_multidirectional_shade(elev, w, h, cell_size, altitude, z_factor);
        Ok(shade
            .into_iter()
            .map(|s| (s.clamp(0.0, 1.0) * 255.0).round() as u8)
            .collect())
    }

    /// Slope magnitude in degrees. Returns an empty vector on invalid input.
    #[wasm_bindgen]
    pub fn slope(elev: &[f32], width: u32, height: u32, cell_size: f64, z_factor: f64) -> Vec<f32> {
        match checked_dims(elev, width, height) {
            Ok((w, h)) => compute_slope(elev, w, h, cell_size, z_factor),
            Err(_) => Vec::new(),
        }
    }

    /// Aspect in degrees clockwise from north (`0..360`, flat = `0`).
    /// Returns an empty vector on invalid input.
    #[wasm_bindgen]
    pub fn aspect(elev: &[f32], width: u32, height: u32, cell_size: f64) -> Vec<f32> {
        match checked_dims(elev, width, height) {
            Ok((w, h)) => compute_aspect(elev, w, h, cell_size),
            Err(_) => Vec::new(),
        }
    }

    /// Single-direction hillshade as grayscale RGBA [`ImageData`].
    #[wasm_bindgen(js_name = hillshadeImageData)]
    pub fn hillshade_image_data(
        elev: &[f32],
        width: u32,
        height: u32,
        cell_size: f64,
        azimuth: f64,
        altitude: f64,
        z_factor: f64,
    ) -> Result<ImageData, JsValue> {
        let (w, h) = checked_dims(elev, width, height).map_err(|e| JsValue::from_str(&e))?;
        let shade = compute_hillshade(elev, w, h, cell_size, azimuth, altitude, z_factor);
        let mut rgba = vec![0u8; w * h * 4];
        for (i, &s) in shade.iter().enumerate() {
            rgba[i * 4] = s;
            rgba[i * 4 + 1] = s;
            rgba[i * 4 + 2] = s;
            rgba[i * 4 + 3] = 255;
        }
        let clamped = wasm_bindgen::Clamped(rgba.as_slice());
        ImageData::new_with_u8_clamped_array_and_sh(clamped, width, height)
    }

    /// Colour-relief image: a palette lookup of elevation modulated by the
    /// multidirectional shade, returned as RGBA [`ImageData`].
    ///
    /// `min`/`max` define the elevation range mapped across the palette.
    #[wasm_bindgen(js_name = colorReliefShaded)]
    #[allow(clippy::too_many_arguments)]
    pub fn color_relief_shaded(
        elev: &[f32],
        width: u32,
        height: u32,
        cell_size: f64,
        palette: &str,
        min: f32,
        max: f32,
        altitude: f64,
        z_factor: f64,
    ) -> Result<ImageData, JsValue> {
        let (w, h) = checked_dims(elev, width, height).map_err(|e| JsValue::from_str(&e))?;
        let shade = compute_multidirectional_shade(elev, w, h, cell_size, altitude, z_factor);
        let lut = build_lut(&palette_by_name(palette));

        let min_f = f64::from(min);
        let range = f64::from((max - min).max(f32::EPSILON));

        let mut rgba = vec![0u8; w * h * 4];
        for i in 0..(w * h) {
            let value = f64::from(elev[i]);
            let t = ((value - min_f) / range).clamp(0.0, 1.0);
            let idx = ((t * 255.0).round() as usize).min(255);
            let color = lut[idx];
            let s = shade[i].clamp(0.0, 1.0);
            rgba[i * 4] = (f64::from(color.r) * s).round().clamp(0.0, 255.0) as u8;
            rgba[i * 4 + 1] = (f64::from(color.g) * s).round().clamp(0.0, 255.0) as u8;
            rgba[i * 4 + 2] = (f64::from(color.b) * s).round().clamp(0.0, 255.0) as u8;
            rgba[i * 4 + 3] = 255;
        }
        let clamped = wasm_bindgen::Clamped(rgba.as_slice());
        ImageData::new_with_u8_clamped_array_and_sh(clamped, width, height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 5×5 DEM that ramps east: every column `x` has elevation `x`.
    fn ramp_5x5() -> Vec<f32> {
        let mut dem = vec![0.0f32; 25];
        for y in 0..5 {
            for x in 0..5 {
                dem[y * 5 + x] = x as f32;
            }
        }
        dem
    }

    #[test]
    fn hillshade_ramp_center_matches_hand_computed_horn() {
        // For the east-ramp with cell_size=1, z_factor=1 an interior cell has
        // dz/dx = 1.0, dz/dy = 0.0.  With azimuth=315, altitude=45:
        //   slope_rad   = atan(1)            = π/4
        //   aspect_rad  = atan2(0, -1)       = π
        //   zenith_rad  = 45°  → cos=sin=√½
        //   azimuth_rad = 135°
        //   shade = √½·√½ + √½·√½·cos(135°-180°)
        //         = 0.5 + 0.5·cos(-45°) = 0.5 + 0.5·√½ = 0.853553…
        //   byte  = round(0.853553… · 255) = 218
        let dem = ramp_5x5();
        let out = WasmTerrain::hillshade(&dem, 5, 5, 1.0, 315.0, 45.0, 1.0).expect("hillshade");
        assert_eq!(out.len(), 25);
        assert_eq!(out[2 * 5 + 2], 218, "center pixel Horn parity");
        // Interior columns (x = 1..=3) share the same gradient and value.
        for y in 1..4 {
            assert_eq!(out[y * 5 + 1], 218);
            assert_eq!(out[y * 5 + 3], 218);
        }
    }

    #[test]
    fn hillshade_flat_dem_is_uniform() {
        // Flat DEM → dz/dx = dz/dy = 0 → shade = cos(zenith).
        // altitude=45 → cos(45°)=√½ → round(√½·255)=180 everywhere.
        let dem = vec![100.0f32; 25];
        let out = WasmTerrain::hillshade(&dem, 5, 5, 1.0, 315.0, 45.0, 1.0).expect("hillshade");
        assert!(
            out.iter().all(|&v| v == 180),
            "expected uniform 180, got {out:?}"
        );
    }

    #[test]
    fn multidirectional_flat_dem_is_uniform() {
        // Weights sum to 1.0, so the flat-DEM multidirectional value equals the
        // single-direction one: 180.
        let dem = vec![-5.0f32; 25];
        let out = WasmTerrain::hillshade_multidirectional(&dem, 5, 5, 1.0, 45.0, 1.0)
            .expect("multidirectional");
        assert!(
            out.iter().all(|&v| v == 180),
            "expected uniform 180, got {out:?}"
        );
    }

    #[test]
    fn slope_ramp_center_is_45_degrees() {
        // dz/dx = 1, dz/dy = 0 → slope = atan(1) = 45°.
        let dem = ramp_5x5();
        let slopes = WasmTerrain::slope(&dem, 5, 5, 1.0, 1.0);
        assert_eq!(slopes.len(), 25);
        assert!(
            (slopes[2 * 5 + 2] - 45.0).abs() < 1e-4,
            "slope = {}",
            slopes[2 * 5 + 2]
        );
    }

    #[test]
    fn slope_flat_dem_is_zero() {
        let dem = vec![7.0f32; 25];
        let slopes = WasmTerrain::slope(&dem, 5, 5, 1.0, 1.0);
        assert!(slopes.iter().all(|&s| s.abs() < 1e-6));
    }

    #[test]
    fn aspect_ramp_center_faces_east() {
        // Surface rises to the east → upslope aspect = 90° (E).
        let dem = ramp_5x5();
        let aspects = WasmTerrain::aspect(&dem, 5, 5, 1.0);
        assert_eq!(aspects.len(), 25);
        assert!(
            (aspects[2 * 5 + 2] - 90.0).abs() < 1e-3,
            "aspect = {}",
            aspects[2 * 5 + 2]
        );
    }

    #[test]
    fn aspect_flat_dem_reports_zero() {
        let dem = vec![3.0f32; 25];
        let aspects = WasmTerrain::aspect(&dem, 5, 5, 1.0);
        assert!(aspects.iter().all(|&a| a == 0.0));
    }

    #[test]
    fn invalid_dims_are_rejected_gracefully() {
        // NB: assert on the native-safe `checked_dims` and the Vec-returning
        // wrappers only — exercising a `JsValue`-returning error path aborts
        // off-wasm, so we avoid calling `hillshade` with bad dims here.
        let dem = vec![0.0f32; 4];
        // Buffer too small for 5×5.
        assert!(checked_dims(&dem, 5, 5).is_err());
        // Zero dimension.
        assert!(checked_dims(&dem, 0, 5).is_err());
        // Vec-returning wrappers degrade to empty output (no JsValue built).
        assert!(WasmTerrain::slope(&dem, 5, 5, 1.0, 1.0).is_empty());
        assert!(WasmTerrain::aspect(&dem, 5, 5, 1.0).is_empty());
        // Valid dims resolve.
        let ok = vec![0.0f32; 25];
        assert_eq!(checked_dims(&ok, 5, 5), Ok((5, 5)));
    }

    #[test]
    fn palette_lookup_covers_named_and_default() {
        // Named palettes resolve; unknown falls back to terrain (non-empty LUT).
        for name in [
            "viridis",
            "plasma",
            "inferno",
            "terrain",
            "rainbow",
            "rdylbu",
            "grayscale",
        ] {
            let lut = build_lut(&palette_by_name(name));
            assert_eq!(lut.len(), 256);
        }
        let fallback = build_lut(&palette_by_name("does-not-exist"));
        let terrain = build_lut(&palette_by_name("terrain"));
        assert_eq!(fallback[128], terrain[128]);
    }
}
