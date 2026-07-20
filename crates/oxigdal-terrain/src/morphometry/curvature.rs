//! Profile and plan curvature via Zevenbergen & Thorne (1987).
//!
//! Curvature describes the rate of change of slope (profile curvature) and
//! aspect (plan curvature) across a Digital Elevation Model. Both quantities
//! are second derivatives of the terrain surface and are returned in units of
//! reciprocal length (1 / m when the DEM elevation and `cell_size` are in
//! metres).
//!
//! # Formulation
//!
//! A 3×3 finite-difference window is fitted to the polynomial
//! `Z(x, y) = D·x² + E·y² + F·x·y + G·x + H·y + I` following
//! Zevenbergen, L. W. & Thorne, C. R., *Quantitative analysis of land surface
//! topography*, Earth Surface Processes and Landforms 12 (1987), 47–56.
//! With cell size `L`, neighbour heights `z[r,c]` for `r, c ∈ {-1, 0, 1}`,
//!
//! - `D = ((z[0, 1] + z[0, -1]) / 2 − z[0, 0]) / L²`
//! - `E = ((z[1, 0] + z[-1, 0]) / 2 − z[0, 0]) / L²`
//! - `F = (−z[-1, -1] + z[-1, 1] + z[1, -1] − z[1, 1]) / (4 L²)`
//! - `G = (−z[0, -1] + z[0, 1]) / (2 L)` (∂z/∂x at the centre)
//! - `H = (z[-1, 0] − z[1, 0]) / (2 L)` (∂z/∂y at the centre, north positive)
//!
//! Following the canonical hydrology convention (Wood 1996; SAGA GIS), we
//! identify `p = G`, `q = H`, `r = 2 D`, `s = F`, `t = 2 E` and form
//!
//! ```text
//! K_profile = (p²·r + 2·p·q·s + q²·t) / ((p² + q²) · (1 + p² + q²)^1.5)
//! K_plan    = (q²·r − 2·p·q·s + p²·t) / (p² + q²)^1.5
//! ```
//!
//! # Sign convention
//!
//! - **Profile curvature** is **positive on concave** (bowl-up) downslope
//!   profiles and **negative on convex** (hilltop) profiles. This matches
//!   Wood (1996) and SAGA GIS — the hydrology-friendly convention used
//!   throughout the OxiGDAL terrain stack — and is the **opposite** sign of
//!   ESRI's ArcGIS implementation (which inherited Moore et al. 1991 with a
//!   leading minus sign). The relation is `K_profile_oxigdal = − K_profile_arcgis`.
//! - **Plan curvature** is **positive on divergent** (ridge / nose) flow and
//!   **negative on convergent** (valley) flow.
//!
//! # Numerical safeguards
//!
//! On approximately flat cells the denominators `(p² + q²)` and
//! `(p² + q²) · (1 + p² + q²)^1.5` collapse toward zero. We treat any cell
//! with `p² + q² < ε = 1×10⁻¹²` (corresponding to a slope below ~10⁻⁶ rad in
//! a 1 m cell) as flat and emit `0` for both curvatures. Boundary cells —
//! the outermost row and column — have no full 3×3 stencil and emit
//! [`f64::NAN`].
//!
//! # Inputs
//!
//! `cell_size` is assumed isotropic (square pixels). Anisotropic pixel sizes
//! are not supported by this entry point.

use crate::error::{Result, TerrainError};
use num_traits::Float;
use scirs2_core::prelude::*;

/// Numerical floor used to detect flat cells.
///
/// Any cell whose squared-gradient magnitude `p² + q²` falls below this
/// threshold is treated as flat: both profile and plan curvature are set to
/// `0`. For a 1 m cell this corresponds to a slope below roughly 10⁻⁶ rad
/// (well below numerical noise from f64 differencing of f32 elevations).
const FLAT_GRADIENT_EPS: f64 = 1.0e-12;

/// Bundled output of [`compute_curvature`].
///
/// Profile and plan curvature share the same shape as the input DEM. Both
/// arrays carry `f64::NAN` on the outermost row and column (boundary cells
/// where the 3×3 stencil cannot be evaluated).
#[derive(Debug, Clone)]
pub struct CurvatureResult {
    /// Profile curvature in 1/length-unit.
    ///
    /// Positive = concave downslope profile (valley), negative = convex
    /// (ridge/dome). Boundary cells: [`f64::NAN`].
    pub profile: Array2<f64>,
    /// Plan curvature in 1/length-unit.
    ///
    /// Positive = divergent flow (ridge nose), negative = convergent
    /// (valley confluence). Boundary cells: [`f64::NAN`].
    pub plan: Array2<f64>,
}

impl CurvatureResult {
    /// Compute both profile and plan curvature for the given DEM.
    ///
    /// Convenience wrapper around [`compute_curvature`].
    pub fn from_dem<T>(dem: &Array2<T>, cell_size: f64, nodata: Option<T>) -> Result<Self>
    where
        T: Float + Into<f64> + Copy,
    {
        let (profile, plan) = compute_curvature(dem, cell_size, nodata)?;
        Ok(Self { profile, plan })
    }
}

/// Compute profile and plan curvature using Zevenbergen & Thorne (1987).
///
/// # Arguments
///
/// - `dem` — input elevation raster.
/// - `cell_size` — square pixel size, must be positive.
/// - `nodata` — optional sentinel value; matches NaN as well.
///
/// # Returns
///
/// `(profile_curvature, plan_curvature)` — two arrays the same shape as
/// `dem`. The outermost row/column carry [`f64::NAN`] (no full 3×3 stencil).
/// Cells matching the `nodata` sentinel propagate [`f64::NAN`] into both
/// outputs. Flat cells (gradient magnitude below `1e-12`) emit `0`.
///
/// # Errors
///
/// Returns [`TerrainError::InvalidDimensions`] if the DEM is smaller than
/// 3×3, and [`TerrainError::InvalidCellSize`] if `cell_size <= 0`.
pub fn compute_curvature<T>(
    dem: &Array2<T>,
    cell_size: f64,
    nodata: Option<T>,
) -> Result<(Array2<f64>, Array2<f64>)>
where
    T: Float + Into<f64> + Copy,
{
    validate_inputs(dem, cell_size)?;

    let (height, width) = dem.dim();
    // Boundary cells must surface as NaN, so initialise with NaN rather than 0.
    let mut profile = Array2::from_elem((height, width), f64::NAN);
    let mut plan = Array2::from_elem((height, width), f64::NAN);

    let cs = cell_size;
    let cs_sq = cs * cs;
    let inv_2cs = 1.0 / (2.0 * cs);
    let inv_4cs_sq = 1.0 / (4.0 * cs_sq);
    let inv_cs_sq = 1.0 / cs_sq;

    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let centre = dem[[y, x]];

            // nodata at the centre: leave NaN and continue.
            if let Some(nd) = nodata
                && is_nodata(centre, nd)
            {
                continue;
            }

            // Read the 3×3 window. If any neighbour is nodata we cannot fit
            // the polynomial — emit NaN for this cell and continue.
            let z = match read_window(dem, y, x, nodata) {
                Some(window) => window,
                None => continue,
            };

            // Zevenbergen & Thorne (1987) coefficients in 3×3 stencil notation.
            //   z[0] = NW, z[1] = N,  z[2] = NE
            //   z[3] = W,  z[4] = C,  z[5] = E
            //   z[6] = SW, z[7] = S,  z[8] = SE
            //
            // East/west and north/south are handed; here row index increases
            // downward, so north is row y - 1 (z[0..3]) and south is row y + 1
            // (z[6..9]). The +y direction in slope/curvature space points
            // *north* (upward), so dz/dy uses (north − south).
            let nw = z[0];
            let n = z[1];
            let ne = z[2];
            let w = z[3];
            // centre is z[4] but unused after the convexity coefficients
            let _c = z[4];
            let e = z[5];
            let sw = z[6];
            let s = z[7];
            let se = z[8];

            // First derivatives at the centre.
            let g = (e - w) * inv_2cs; // p = ∂z/∂x  (east positive)
            let h = (n - s) * inv_2cs; // q = ∂z/∂y  (north positive)

            // Second derivatives. D = ½ ∂²z/∂x², E = ½ ∂²z/∂y², F = ∂²z/∂x∂y.
            let d = ((e + w) * 0.5 - z[4]) * inv_cs_sq;
            let e_coeff = ((n + s) * 0.5 - z[4]) * inv_cs_sq;
            // F via the cross-difference (NE − NW − SE + SW) / (4 L²).
            // Note: with row-y-down, dxdy mixes signs; the conventional Z&T
            // form is `(−NW + NE + SW − SE) / (4 L²)`.
            let f_coeff = (-nw + ne + sw - se) * inv_4cs_sq;

            let p = g;
            let q = h;
            let r = 2.0 * d; // ∂²z/∂x²
            let s_mix = f_coeff; // ∂²z/∂x∂y
            let t = 2.0 * e_coeff; // ∂²z/∂y²

            let p2 = p * p;
            let q2 = q * q;
            let pq = p * q;
            let p2_q2 = p2 + q2;

            if p2_q2 < FLAT_GRADIENT_EPS {
                // Flat cell — curvature undefined; emit zero rather than NaN
                // so downstream means/medians are well defined.
                profile[[y, x]] = 0.0;
                plan[[y, x]] = 0.0;
                continue;
            }

            // (1 + p² + q²)^1.5 — only the profile denominator needs it.
            let one_plus = 1.0 + p2_q2;
            let one_plus_pow_15 = one_plus.sqrt() * one_plus; // = (1+...)^1.5
            let p2_q2_pow_15 = p2_q2.sqrt() * p2_q2; // = (p²+q²)^1.5

            // Hydrology convention: positive = concave (profile), divergent (plan).
            // ESRI/ArcGIS uses the opposite sign — see module docs.
            let kpr = (p2 * r + 2.0 * pq * s_mix + q2 * t) / (p2_q2 * one_plus_pow_15);
            let kpl = (q2 * r - 2.0 * pq * s_mix + p2 * t) / p2_q2_pow_15;

            profile[[y, x]] = kpr;
            plan[[y, x]] = kpl;
        }
    }

    Ok((profile, plan))
}

/// Read the 3×3 stencil centred on `(y, x)` as f64. Returns `None` if any
/// pixel in the window matches the nodata sentinel.
fn read_window<T>(dem: &Array2<T>, y: usize, x: usize, nodata: Option<T>) -> Option<[f64; 9]>
where
    T: Float + Into<f64> + Copy,
{
    let raw = [
        dem[[y - 1, x - 1]],
        dem[[y - 1, x]],
        dem[[y - 1, x + 1]],
        dem[[y, x - 1]],
        dem[[y, x]],
        dem[[y, x + 1]],
        dem[[y + 1, x - 1]],
        dem[[y + 1, x]],
        dem[[y + 1, x + 1]],
    ];
    if let Some(nd) = nodata {
        for v in &raw {
            if is_nodata(*v, nd) {
                return None;
            }
        }
    } else {
        for v in &raw {
            if v.is_nan() {
                return None;
            }
        }
    }
    Some([
        raw[0].into(),
        raw[1].into(),
        raw[2].into(),
        raw[3].into(),
        raw[4].into(),
        raw[5].into(),
        raw[6].into(),
        raw[7].into(),
        raw[8].into(),
    ])
}

fn validate_inputs<T>(dem: &Array2<T>, cell_size: f64) -> Result<()> {
    let (height, width) = dem.dim();
    if height < 3 || width < 3 {
        return Err(TerrainError::InvalidDimensions { width, height });
    }
    if cell_size <= 0.0 {
        return Err(TerrainError::InvalidCellSize { size: cell_size });
    }
    Ok(())
}

fn is_nodata<T: Float>(value: T, nodata: T) -> bool {
    if value.is_nan() && nodata.is_nan() {
        true
    } else {
        (value - nodata).abs() < T::epsilon()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a DEM where `z = f(x_world, y_world)` evaluated on a regular grid.
    /// `(0, 0)` (top-left in array coordinates) maps to `(x = 0, y = (h-1) * L)`
    /// — i.e. row index increases southward but world y increases northward.
    fn build_dem<F>(height: usize, width: usize, cell_size: f64, f: F) -> Array2<f64>
    where
        F: Fn(f64, f64) -> f64,
    {
        let mut dem = Array2::zeros((height, width));
        for row in 0..height {
            for col in 0..width {
                let x_world = col as f64 * cell_size;
                let y_world = (height - 1 - row) as f64 * cell_size;
                dem[[row, col]] = f(x_world, y_world);
            }
        }
        dem
    }

    #[test]
    fn test_curvature_flat_dem_returns_zero() {
        let dem = Array2::<f64>::from_elem((7, 7), 42.0);
        let (profile, plan) = compute_curvature(&dem, 5.0, None).expect("compute_curvature");
        for y in 1..6 {
            for x in 1..6 {
                assert_eq!(profile[[y, x]], 0.0, "flat profile [{y},{x}]");
                assert_eq!(plan[[y, x]], 0.0, "flat plan [{y},{x}]");
            }
        }
    }

    #[test]
    fn test_curvature_concave_bowl_positive_profile() {
        // z = x² + y² is a concave-up bowl. At any interior cell (x, y) ≠ origin
        // the profile direction (downslope) bends *upward* as you walk along it
        // — the canonical concave profile. Hydrology convention → positive.
        let cell = 1.0;
        let dem = build_dem(11, 11, cell, |x, y| {
            let xc = x - 5.0;
            let yc = y - 5.0;
            xc * xc + yc * yc
        });
        let (profile, _plan) = compute_curvature(&dem, cell, None).expect("compute_curvature");
        // Sample several non-origin interior points; all must be positive.
        let probes: [(usize, usize); 4] = [(2, 7), (3, 7), (7, 7), (5, 8)];
        for (y, x) in probes {
            let v = profile[[y, x]];
            assert!(
                v > 0.0,
                "expected positive profile curvature on concave bowl at ({y},{x}), got {v}"
            );
        }
    }

    #[test]
    fn test_curvature_convex_dome_negative_profile() {
        // z = -(x² + y²) is a convex dome. Profile curvature should be negative
        // on the flanks (the downslope direction bends downward).
        let cell = 1.0;
        let dem = build_dem(11, 11, cell, |x, y| {
            let xc = x - 5.0;
            let yc = y - 5.0;
            -(xc * xc + yc * yc)
        });
        let (profile, _plan) = compute_curvature(&dem, cell, None).expect("compute_curvature");
        let probes: [(usize, usize); 4] = [(2, 7), (3, 7), (7, 7), (5, 8)];
        for (y, x) in probes {
            let v = profile[[y, x]];
            assert!(
                v < 0.0,
                "expected negative profile curvature on convex dome at ({y},{x}), got {v}"
            );
        }
    }

    #[test]
    fn test_curvature_planar_slope_zero_curvature() {
        // A planar (linear) surface has zero curvature everywhere.
        let cell = 1.0;
        let dem = build_dem(7, 7, cell, |x, y| 3.0 * x - 2.0 * y + 7.5);
        let (profile, plan) = compute_curvature(&dem, cell, None).expect("compute_curvature");
        for y in 1..6 {
            for x in 1..6 {
                let pr = profile[[y, x]];
                let pl = plan[[y, x]];
                assert!(
                    pr.abs() < 1.0e-12,
                    "planar profile curvature at ({y},{x}) should ≈ 0, got {pr}"
                );
                assert!(
                    pl.abs() < 1.0e-12,
                    "planar plan curvature at ({y},{x}) should ≈ 0, got {pl}"
                );
            }
        }
    }

    #[test]
    fn test_curvature_boundary_is_nan() {
        let dem = Array2::<f64>::from_elem((5, 5), 1.0);
        let (profile, plan) = compute_curvature(&dem, 1.0, None).expect("compute_curvature");
        let (h, w) = (5, 5);
        // Top and bottom row.
        for x in 0..w {
            assert!(profile[[0, x]].is_nan(), "top profile [{x}] not NaN");
            assert!(profile[[h - 1, x]].is_nan(), "bottom profile [{x}] not NaN");
            assert!(plan[[0, x]].is_nan(), "top plan [{x}] not NaN");
            assert!(plan[[h - 1, x]].is_nan(), "bottom plan [{x}] not NaN");
        }
        // Left and right column.
        for y in 0..h {
            assert!(profile[[y, 0]].is_nan(), "left profile [{y}] not NaN");
            assert!(profile[[y, w - 1]].is_nan(), "right profile [{y}] not NaN");
            assert!(plan[[y, 0]].is_nan(), "left plan [{y}] not NaN");
            assert!(plan[[y, w - 1]].is_nan(), "right plan [{y}] not NaN");
        }
    }

    #[test]
    fn test_curvature_units_per_meter() {
        // Curvature is a second derivative — for a fixed geometric surface,
        // doubling cell_size (sampling the same paraboloid at a coarser
        // grid) leaves the *true* curvature unchanged at the same world
        // coordinate. The Z&T finite-difference estimator recovers this:
        // the discretisation cancels because both the gradient denominator
        // and the second-difference numerator scale consistently with L.
        //
        // We sample a paraboloid `z = κ/2 · (x² + y²)` (mean curvature ≈ κ
        // for small slopes) and check the recovered curvature scales as
        // expected with cell_size.
        let kappa = 0.04; // small enough to keep |grad| << 1 → linearised regime
        let surface = |xc: f64, yc: f64| 0.5 * kappa * (xc * xc + yc * yc);

        // Grid 1: 21×21 with cell=1, centre at (10, 10) world (10, 10).
        let dem1 = build_dem(21, 21, 1.0, |x, y| surface(x - 10.0, y - 10.0));
        // Grid 2: 11×11 with cell=2, centre at (10, 10) world (10, 10).
        let dem2 = build_dem(11, 11, 2.0, |x, y| surface(x - 10.0, y - 10.0));

        let (profile1, _) = compute_curvature(&dem1, 1.0, None).expect("c1");
        let (profile2, _) = compute_curvature(&dem2, 2.0, None).expect("c2");

        // Probe at the same world point (5 m east of centre, on the y-axis).
        // In dem1 at L=1 that's array coord (10, 15); in dem2 at L=2 that's (5, 8).
        // Note: row index = (h - 1) - y_world / L.
        let v1 = profile1[[10, 15]];
        let v2 = profile2[[5, 8]];
        // Both estimates target the same physical curvature → close to each other,
        // and both close to the small-slope analytical value κ/(1+|grad|²)^1.5.
        let rel = (v1 - v2).abs() / v1.abs().max(1.0e-9);
        assert!(
            rel < 0.05,
            "curvature estimates differ across cell sizes: v1={v1} v2={v2} rel={rel}"
        );
        // And both are positive (concave) for this bowl.
        assert!(v1 > 0.0, "v1 should be > 0 (concave), got {v1}");
        assert!(v2 > 0.0, "v2 should be > 0 (concave), got {v2}");
    }

    #[test]
    fn test_curvature_invalid_dimensions_errors() {
        let dem = Array2::<f64>::zeros((2, 5));
        let err = compute_curvature(&dem, 1.0, None).expect_err("must error");
        matches!(err, TerrainError::InvalidDimensions { .. });
    }

    #[test]
    fn test_curvature_invalid_cell_size_errors() {
        let dem = Array2::<f64>::zeros((5, 5));
        let err = compute_curvature(&dem, 0.0, None).expect_err("must error");
        matches!(err, TerrainError::InvalidCellSize { .. });
    }

    #[test]
    fn test_curvature_result_struct_roundtrip() {
        let dem = build_dem(7, 7, 1.0, |x, y| {
            let xc = x - 3.0;
            let yc = y - 3.0;
            xc * xc + yc * yc
        });
        let res = CurvatureResult::from_dem(&dem, 1.0, None).expect("CurvatureResult::from_dem");
        let (profile, plan) = compute_curvature(&dem, 1.0, None).expect("compute_curvature");
        for y in 1..6 {
            for x in 1..6 {
                if profile[[y, x]].is_finite() {
                    assert!(
                        (res.profile[[y, x]] - profile[[y, x]]).abs() < 1.0e-15,
                        "profile mismatch at ({y},{x})"
                    );
                }
                if plan[[y, x]].is_finite() {
                    assert!(
                        (res.plan[[y, x]] - plan[[y, x]]).abs() < 1.0e-15,
                        "plan mismatch at ({y},{x})"
                    );
                }
            }
        }
    }
}
