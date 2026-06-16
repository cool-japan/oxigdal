//! GLCM / Haralick texture metrics for terrain surfaces.
//!
//! Computes six Haralick (1973) texture features from a Grey-Level
//! Co-occurrence Matrix (GLCM) derived from a sliding window over a DEM:
//! entropy, homogeneity (IDM), contrast, energy (ASM), correlation, and
//! dissimilarity.

use crate::error::{Result, TerrainError};
use num_traits::Float;
use scirs2_core::prelude::*;

// ── Public types ────────────────────────────────────────────────────────────

/// Output of GLCM texture analysis: six Haralick feature rasters.
#[derive(Debug, Clone)]
pub struct GlcmTextures {
    /// Entropy: −Σ p(i,j) · log₂(p(i,j))
    pub entropy: Array2<f64>,
    /// Inverse Difference Moment (homogeneity): Σ p(i,j) / (1 + (i−j)²)
    pub homogeneity: Array2<f64>,
    /// Contrast: Σ (i−j)² · p(i,j)
    pub contrast: Array2<f64>,
    /// Energy (Angular Second Moment): Σ p(i,j)²
    pub energy: Array2<f64>,
    /// Correlation: Σ (i·j·p(i,j) − μᵢ·μⱼ) / (σᵢ·σⱼ)
    pub correlation: Array2<f64>,
    /// Dissimilarity: Σ |i−j| · p(i,j)
    pub dissimilarity: Array2<f64>,
}

/// Direction for GLCM co-occurrence offset.
#[derive(Debug, Clone, Copy)]
pub enum GlcmOffset {
    /// Single direction (dy, dx).
    Single {
        /// Row offset
        dy: isize,
        /// Column offset
        dx: isize,
    },
    /// Average over 4 directions (0°, 45°, 90°, 135° and their reverses).
    AllDirections,
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Compute GLCM-based Haralick texture metrics over a sliding window.
///
/// # Arguments
/// * `dem`           - Input DEM.
/// * `window_radius` - Half-width of the square analysis window
///   (window = `2·window_radius+1 × 2·window_radius+1`).
/// * `levels`        - Number of quantisation grey levels (must be ≥ 2).
/// * `offset`        - Co-occurrence direction.
/// * `nodata`        - Optional NoData sentinel.
///
/// # Returns
/// [`GlcmTextures`] with one raster per Haralick feature.
///
/// # Errors
/// Returns [`TerrainError`] if the DEM is empty, `levels < 2`, or
/// `window_radius` is too large relative to the DEM dimensions.
pub fn glcm_texture<T>(
    dem: &Array2<T>,
    window_radius: usize,
    levels: usize,
    offset: GlcmOffset,
    nodata: Option<T>,
) -> Result<GlcmTextures>
where
    T: Float + Into<f64> + Copy,
{
    validate_glcm_inputs(dem, window_radius, levels)?;

    let (height, width) = dem.dim();
    let n = height * width;

    // Global min / max for quantisation (valid cells only)
    let (global_min, global_max) = compute_global_range(dem, nodata)?;
    let range = global_max - global_min;

    let mut entropy_arr = Array2::zeros((height, width));
    let mut homogeneity_arr = Array2::zeros((height, width));
    let mut contrast_arr = Array2::zeros((height, width));
    let mut energy_arr = Array2::zeros((height, width));
    let mut correlation_arr = Array2::zeros((height, width));
    let mut dissimilarity_arr = Array2::zeros((height, width));

    // Immutable parameters bundled to keep fill_glcm's arg count below 7.
    let glcm_params = GlcmParams {
        wr: window_radius,
        levels,
        offset,
        nodata,
        range,
        global_min,
    };

    // Scratch GLCM buffer reused for every window (avoids heap thrash)
    let mut glcm = vec![0_u64; levels * levels];

    for idx in 0..n {
        let y = idx / width;
        let x = idx % width;

        let center = dem[[y, x]];
        if let Some(nd) = nodata {
            if is_nodata(center, nd) {
                // Leave all features at 0.0
                continue;
            }
        }

        // Build GLCM for this window
        let total_pairs = fill_glcm(dem, y, x, &glcm_params, &mut glcm);

        if total_pairs == 0 {
            // Window fully outside or no valid pairs
            glcm.fill(0);
            continue;
        }

        let features = compute_haralick(&glcm, levels, total_pairs);

        entropy_arr[[y, x]] = features.entropy;
        homogeneity_arr[[y, x]] = features.homogeneity;
        contrast_arr[[y, x]] = features.contrast;
        energy_arr[[y, x]] = features.energy;
        correlation_arr[[y, x]] = features.correlation;
        dissimilarity_arr[[y, x]] = features.dissimilarity;

        // Reset scratch buffer
        glcm.fill(0);
    }

    Ok(GlcmTextures {
        entropy: entropy_arr,
        homogeneity: homogeneity_arr,
        contrast: contrast_arr,
        energy: energy_arr,
        correlation: correlation_arr,
        dissimilarity: dissimilarity_arr,
    })
}

// ── Private helpers ──────────────────────────────────────────────────────────

struct HaralickFeatures {
    entropy: f64,
    homogeneity: f64,
    contrast: f64,
    energy: f64,
    correlation: f64,
    dissimilarity: f64,
}

/// Immutable parameters shared by every `fill_glcm` call inside a single
/// `glcm_texture` run.  Grouping them reduces `fill_glcm`'s argument count
/// below Clippy's 7-argument limit.
struct GlcmParams<T: Float + Into<f64> + Copy> {
    wr: usize,
    levels: usize,
    offset: GlcmOffset,
    nodata: Option<T>,
    range: f64,
    global_min: f64,
}

/// Populate `glcm` (already zeroed) and return the total number of valid
/// co-occurrence pairs recorded.
fn fill_glcm<T>(
    dem: &Array2<T>,
    cy: usize,
    cx: usize,
    params: &GlcmParams<T>,
    glcm: &mut [u64],
) -> u64
where
    T: Float + Into<f64> + Copy,
{
    let GlcmParams {
        wr,
        levels,
        offset,
        nodata,
        range,
        global_min,
    } = *params;
    let (height, width) = dem.dim();

    let directions: &[(isize, isize)] = match offset {
        GlcmOffset::Single { dy, dx } => &[(dy, dx)],
        GlcmOffset::AllDirections => &[
            (0, 1),   // 0°
            (-1, 1),  // 45°
            (-1, 0),  // 90°
            (-1, -1), // 135°
        ],
    };

    let mut total = 0_u64;

    let wr_i = wr as isize;

    for &(ody, odx) in directions {
        // Iterate over all cells in the window
        for dy in -wr_i..=wr_i {
            for dx in -wr_i..=wr_i {
                let iy = cy as isize + dy;
                let ix = cx as isize + dx;
                let jy = iy + ody;
                let jx = ix + odx;

                // Both cells must be inside the DEM
                if iy < 0 || iy >= height as isize || ix < 0 || ix >= width as isize {
                    continue;
                }
                if jy < 0 || jy >= height as isize || jx < 0 || jx >= width as isize {
                    continue;
                }

                let vi = dem[[iy as usize, ix as usize]];
                let vj = dem[[jy as usize, jx as usize]];

                if let Some(nd) = nodata {
                    if is_nodata(vi, nd) || is_nodata(vj, nd) {
                        continue;
                    }
                }

                let qi = quantise(vi.into(), global_min, range, levels);
                let qj = quantise(vj.into(), global_min, range, levels);

                glcm[qi * levels + qj] += 1;
                // Symmetric: also record the reverse co-occurrence
                glcm[qj * levels + qi] += 1;
                total += 2;
            }
        }
    }

    total
}

/// Quantise an elevation value into a grey level in `[0, levels-1]`.
#[inline]
fn quantise(v: f64, min: f64, range: f64, levels: usize) -> usize {
    if range < 1e-10 {
        return 0;
    }
    let q = ((v - min) / range * (levels - 1) as f64).round() as isize;
    q.clamp(0, levels as isize - 1) as usize
}

/// Compute all six Haralick features from a (not yet normalised) GLCM.
fn compute_haralick(glcm: &[u64], levels: usize, total_pairs: u64) -> HaralickFeatures {
    let n = total_pairs as f64;

    // Normalise to probability matrix P
    let p: Vec<f64> = glcm.iter().map(|&v| v as f64 / n).collect();

    // Marginal probabilities px[i] = Σ_j P[i][j]
    let mut px = vec![0.0_f64; levels];
    let mut py = vec![0.0_f64; levels];
    for i in 0..levels {
        for j in 0..levels {
            let pij = p[i * levels + j];
            px[i] += pij;
            py[j] += pij;
        }
    }

    // Marginal means and standard deviations
    let mu_x: f64 = (0..levels).map(|i| i as f64 * px[i]).sum();
    let mu_y: f64 = (0..levels).map(|j| j as f64 * py[j]).sum();
    let var_x: f64 = (0..levels).map(|i| (i as f64 - mu_x).powi(2) * px[i]).sum();
    let var_y: f64 = (0..levels).map(|j| (j as f64 - mu_y).powi(2) * py[j]).sum();
    let sigma_x = var_x.sqrt();
    let sigma_y = var_y.sqrt();

    let mut entropy = 0.0_f64;
    let mut homogeneity = 0.0_f64;
    let mut contrast = 0.0_f64;
    let mut energy = 0.0_f64;
    let mut correlation_num = 0.0_f64;
    let mut dissimilarity = 0.0_f64;

    for i in 0..levels {
        for j in 0..levels {
            let pij = p[i * levels + j];
            if pij == 0.0 {
                continue;
            }
            let diff = i as f64 - j as f64;

            entropy -= pij * (pij + 1e-10_f64).log2();
            homogeneity += pij / (1.0 + diff * diff);
            contrast += diff * diff * pij;
            energy += pij * pij;
            correlation_num += i as f64 * j as f64 * pij;
            dissimilarity += diff.abs() * pij;
        }
    }

    let correlation = (correlation_num - mu_x * mu_y) / (sigma_x * sigma_y + 1e-10);

    HaralickFeatures {
        entropy,
        homogeneity,
        contrast,
        energy,
        correlation,
        dissimilarity,
    }
}

/// Find global min and max among valid DEM cells.
fn compute_global_range<T>(dem: &Array2<T>, nodata: Option<T>) -> Result<(f64, f64)>
where
    T: Float + Into<f64> + Copy,
{
    let mut min_val = f64::INFINITY;
    let mut max_val = f64::NEG_INFINITY;

    for &v in dem.iter() {
        if let Some(nd) = nodata {
            if is_nodata(v, nd) {
                continue;
            }
        }
        let f: f64 = v.into();
        if f < min_val {
            min_val = f;
        }
        if f > max_val {
            max_val = f;
        }
    }

    if min_val.is_infinite() {
        return Err(TerrainError::ComputationError {
            message: "DEM contains no valid (non-nodata) cells".to_string(),
        });
    }

    Ok((min_val, max_val))
}

/// Validate inputs for GLCM texture computation.
fn validate_glcm_inputs<T>(dem: &Array2<T>, window_radius: usize, levels: usize) -> Result<()> {
    let (height, width) = dem.dim();

    if height == 0 || width == 0 {
        return Err(TerrainError::InvalidDimensions { width, height });
    }

    if levels < 2 {
        return Err(TerrainError::InvalidThreshold {
            threshold: levels as f64,
            message: "levels must be at least 2".to_string(),
        });
    }

    // window_radius must be strictly less than half of each dimension
    if window_radius >= height / 2 || window_radius >= width / 2 {
        return Err(TerrainError::InvalidRadius {
            radius: window_radius as f64,
        });
    }

    Ok(())
}

/// Check whether a value matches the nodata sentinel.
fn is_nodata<T: Float>(value: T, nodata: T) -> bool {
    if value.is_nan() && nodata.is_nan() {
        true
    } else {
        (value - nodata).abs() < T::epsilon()
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    // Helper: build a small DEM filled with a constant value.
    fn flat_dem(size: usize, val: f64) -> Array2<f64> {
        Array2::from_elem((size, size), val)
    }

    #[test]
    fn test_glcm_uniform() {
        // Uniform DEM: all cells have the same elevation.
        // → single GLCM bin at (0, 0) with probability 1.0.
        // Features: energy ≈ 1.0, entropy ≈ 0.0, homogeneity ≈ 1.0.
        let dem = flat_dem(10, 100.0_f64);
        let tex = glcm_texture(&dem, 2, 16, GlcmOffset::AllDirections, None)
            .expect("glcm_texture failed");

        let (h, w) = (10_usize, 10_usize);
        for y in 2..(h - 2) {
            for x in 2..(w - 2) {
                assert!(
                    (tex.energy[[y, x]] - 1.0).abs() < 1e-6,
                    "energy should be 1 for uniform DEM at ({y},{x})"
                );
                assert!(
                    tex.entropy[[y, x]].abs() < 1e-4,
                    "entropy should be ~0 for uniform DEM at ({y},{x})"
                );
                assert!(
                    (tex.homogeneity[[y, x]] - 1.0).abs() < 1e-6,
                    "homogeneity should be 1 for uniform DEM at ({y},{x})"
                );
                assert!(
                    tex.contrast[[y, x]].abs() < 1e-6,
                    "contrast should be 0 for uniform DEM at ({y},{x})"
                );
                assert!(
                    tex.dissimilarity[[y, x]].abs() < 1e-6,
                    "dissimilarity should be 0 for uniform DEM at ({y},{x})"
                );
            }
        }
    }

    #[test]
    fn test_glcm_checkerboard() {
        // Checkerboard: values alternate between 0.0 and 100.0.
        // Transitions are always max→0 or 0→max → high contrast.
        let size = 12_usize;
        let mut dem = Array2::zeros((size, size));
        for y in 0..size {
            for x in 0..size {
                dem[[y, x]] = if (y + x) % 2 == 0 { 100.0_f64 } else { 0.0_f64 };
            }
        }

        let tex =
            glcm_texture(&dem, 2, 4, GlcmOffset::AllDirections, None).expect("glcm_texture failed");

        // Interior cell should exhibit high contrast
        let contrast_centre = tex.contrast[[6, 6]];
        assert!(
            contrast_centre > 0.1,
            "checkerboard should have non-trivial contrast, got {contrast_centre}"
        );

        // Homogeneity should be lower than for the uniform case
        let hom_centre = tex.homogeneity[[6, 6]];
        assert!(
            hom_centre < 1.0,
            "checkerboard should reduce homogeneity, got {hom_centre}"
        );
    }

    #[test]
    fn test_glcm_gradient() {
        // Linearly increasing DEM → intermediate texture values.
        let size = 12_usize;
        let mut dem = Array2::zeros((size, size));
        for y in 0..size {
            for x in 0..size {
                dem[[y, x]] = (x as f64) * 10.0;
            }
        }

        let tex = glcm_texture(&dem, 2, 16, GlcmOffset::Single { dy: 0, dx: 1 }, None)
            .expect("glcm_texture failed");

        // Interior cells: energy must be strictly positive, dissimilarity > 0
        for y in 2..(size - 2) {
            for x in 2..(size - 2) {
                assert!(tex.energy[[y, x]] >= 0.0, "energy must be non-negative");
                assert!(
                    tex.dissimilarity[[y, x]] >= 0.0,
                    "dissimilarity must be non-negative"
                );
            }
        }

        // Gradient: consecutive cells differ by one level → dissimilarity > 0
        let d = tex.dissimilarity[[6, 5]];
        assert!(d > 0.0, "gradient dissimilarity must be > 0, got {d}");
    }

    #[test]
    fn test_glcm_all_directions() {
        // AllDirections should produce output with the same shape as Single.
        let size = 12_usize;
        let mut dem = Array2::zeros((size, size));
        for y in 0..size {
            for x in 0..size {
                dem[[y, x]] = (y * 5 + x * 3) as f64;
            }
        }

        let tex_single = glcm_texture(&dem, 2, 8, GlcmOffset::Single { dy: 0, dx: 1 }, None)
            .expect("single failed");
        let tex_all = glcm_texture(&dem, 2, 8, GlcmOffset::AllDirections, None)
            .expect("all directions failed");

        assert_eq!(tex_single.entropy.dim(), tex_all.entropy.dim());
        assert_eq!(tex_single.contrast.dim(), tex_all.contrast.dim());
    }

    #[test]
    fn test_glcm_invalid_levels() {
        // levels = 1 must return Err
        let dem = flat_dem(10, 50.0_f64);
        let result = glcm_texture(&dem, 2, 1, GlcmOffset::AllDirections, None);
        assert!(result.is_err(), "levels=1 should return Err");
    }

    #[test]
    fn test_glcm_invalid_window_radius() {
        // window_radius >= dim/2 must return Err
        let dem = flat_dem(6, 50.0_f64);
        // 6/2 = 3, so radius = 3 is too large
        let result = glcm_texture(&dem, 3, 8, GlcmOffset::AllDirections, None);
        assert!(result.is_err(), "window_radius >= dim/2 should return Err");
    }

    #[test]
    fn test_glcm_nodata_handling() {
        // Cells with nodata value should produce 0.0 output (not NaN/panic).
        let mut dem = Array2::from_elem((10, 10), 100.0_f64);
        dem[[5, 5]] = -9999.0; // nodata sentinel at centre

        let tex = glcm_texture(&dem, 2, 8, GlcmOffset::AllDirections, Some(-9999.0_f64))
            .expect("nodata handling failed");

        // Centre cell was nodata → all features 0.0
        assert_relative_eq!(tex.entropy[[5, 5]], 0.0, epsilon = 1e-10);
        assert_relative_eq!(tex.energy[[5, 5]], 0.0, epsilon = 1e-10);
    }
}
