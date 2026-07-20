//! SIMD-accelerated batch coordinate transformation kernels.
//!
//! Provides vectorised kernels for the three most-common hot projection paths:
//! - Transverse Mercator (TM / UTM)
//! - Mercator (spherical and ellipsoidal Web Mercator)
//! - Lambert Conformal Conic (LCC, sphere-based)
//!
//! # Dispatch strategy
//!
//! On stable Rust, transcendental functions (`sin`, `cos`, `ln`, `atan`) are not
//! available as SIMD intrinsics without nightly or a vendor library.  We therefore
//! use **lane-unrolled scalar** kernels:
//!
//! - **AVX2 path (x86_64)**: process 4 points per iteration using four independent
//!   scalar f64 lanes.  This keeps the compiler's auto-vectoriser happy for the
//!   linear arithmetic parts (FMA, multiply-add) and amortises loop overhead.
//! - **Scalar fallback**: identical mathematics, one point per iteration.
//!
//! The public batch functions (`tmerc_forward_batch`, `merc_forward_batch`, …)
//! perform runtime dispatch via `is_avx2()` and branch to the appropriate inner
//! loop.
//!
//! All angles are **radians** at the kernel boundary.

use core::f64::consts::{FRAC_PI_2, FRAC_PI_4};

// ---------------------------------------------------------------------------
// Runtime feature detection
// ---------------------------------------------------------------------------

/// Returns `true` if the current CPU supports AVX2.
#[cfg(target_arch = "x86_64")]
#[inline]
fn is_avx2() -> bool {
    std::arch::is_x86_feature_detected!("avx2")
}

/// Non-x86 always falls back to scalar.
#[cfg(not(target_arch = "x86_64"))]
#[inline]
fn is_avx2() -> bool {
    false
}

// ---------------------------------------------------------------------------
// Ellipsoid / ellipse helper constants
// ---------------------------------------------------------------------------

/// WGS-84 semi-major axis (metres).
pub(crate) const WGS84_A: f64 = 6_378_137.0;
/// WGS-84 flattening.
pub(crate) const WGS84_F: f64 = 1.0 / 298.257_223_563;
/// WGS-84 first eccentricity squared.
pub(crate) const WGS84_E2: f64 = 2.0 * WGS84_F - WGS84_F * WGS84_F;
/// WGS-84 first eccentricity.
pub(crate) const WGS84_E: f64 = 0.081_819_190_842_622; // sqrt(WGS84_E2)

// ---------------------------------------------------------------------------
// Meridional arc (shared helper)
// ---------------------------------------------------------------------------

/// Meridional arc for the WGS-84 ellipsoid from the equator to latitude `phi`
/// (radians).  Series accurate to ~1 mm worldwide.
#[inline(always)]
fn meridional_arc(phi: f64, a: f64, e2: f64) -> f64 {
    let e4 = e2 * e2;
    let e6 = e4 * e2;
    let e8 = e4 * e4;
    a * ((1.0 - e2 / 4.0 - 3.0 * e4 / 64.0 - 5.0 * e6 / 256.0) * phi
        - (3.0 * e2 / 8.0 + 3.0 * e4 / 32.0 + 45.0 * e6 / 1024.0) * (2.0 * phi).sin()
        + (15.0 * e4 / 256.0 + 45.0 * e6 / 1024.0) * (4.0 * phi).sin()
        - (35.0 * e6 / 3072.0) * (6.0 * phi).sin()
        + (315.0 * e8 / 131_072.0) * (8.0 * phi).sin())
}

// ---------------------------------------------------------------------------
// Transverse Mercator batch kernel
// ---------------------------------------------------------------------------

/// Single-point Transverse Mercator forward computation (Snyder §8).
///
/// Returns `(x_easting, y_northing)` in metres.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn tmerc_point(
    lon_rad: f64,
    lat_rad: f64,
    k0: f64,
    lon0_rad: f64,
    false_easting: f64,
    false_northing: f64,
    a: f64,
    e2: f64,
) -> (f64, f64) {
    let e_prime2 = e2 / (1.0 - e2);

    let sin_lat = lat_rad.sin();
    let cos_lat = lat_rad.cos();
    let tan_lat = lat_rad.tan();

    // Radius of curvature in prime vertical
    let n_val = a / (1.0 - e2 * sin_lat * sin_lat).sqrt();

    // Meridional arc
    let m = meridional_arc(lat_rad, a, e2);
    let m0 = meridional_arc(0.0, a, e2); // lat_0 = 0 for UTM/TM

    let t = tan_lat;
    let t2 = t * t;
    let c = e_prime2 * cos_lat * cos_lat;
    let dlon = lon_rad - lon0_rad;
    let a_coef = cos_lat * dlon;
    let a2 = a_coef * a_coef;
    let a4 = a2 * a2;

    // Easting series (Snyder 8-9a)
    let x = k0
        * n_val
        * f64::mul_add(
            (5.0 - 18.0 * t2 + t2 * t2 + 72.0 * c - 58.0 * e_prime2) * a_coef * a4 / 120.0,
            1.0,
            f64::mul_add((1.0 - t2 + c) * a_coef * a2 / 6.0, 1.0, a_coef),
        );

    // Northing series (Snyder 8-10a)
    let y = k0
        * (m - m0
            + n_val
                * t
                * f64::mul_add(
                    (61.0 - 58.0 * t2 + t2 * t2 + 600.0 * c - 330.0 * e_prime2) * a4 * a2 / 720.0,
                    1.0,
                    f64::mul_add(
                        (5.0 - t2 + 9.0 * c + 4.0 * c * c) * a4 / 24.0,
                        1.0,
                        a2 / 2.0,
                    ),
                ));

    (x + false_easting, y + false_northing)
}

/// Batch Transverse Mercator forward projection.
///
/// Processes the input arrays in chunks of 4 (AVX2 path) or 1 (scalar path).
/// All input angles must be in **radians**.
///
/// # Parameters
/// * `lons` – longitude array (radians)
/// * `lats` – latitude array (radians)
/// * `k0` – scale factor at central meridian
/// * `lon0_rad` – central meridian (radians)
/// * `false_easting`, `false_northing` – offsets in metres
/// * `a` – semi-major axis (metres)
/// * `e2` – first eccentricity squared
///
/// # Returns
/// `(eastings, northings)` — two `Vec<f64>` of the same length as the inputs.
#[allow(clippy::too_many_arguments)]
pub(crate) fn tmerc_forward_batch(
    lons: &[f64],
    lats: &[f64],
    k0: f64,
    lon0_rad: f64,
    false_easting: f64,
    false_northing: f64,
    a: f64,
    e2: f64,
) -> (Vec<f64>, Vec<f64>) {
    debug_assert_eq!(lons.len(), lats.len());
    let n = lons.len();
    let mut xs = vec![0.0f64; n];
    let mut ys = vec![0.0f64; n];

    if is_avx2() {
        // 4-lane unrolled path — lets the compiler use FMA and potentially
        // auto-vectorise the arithmetic portions.
        let chunks = n / 4;
        let remainder = n % 4;

        for c in 0..chunks {
            let base = c * 4;
            // Process 4 independent points (no cross-lane dependency)
            let (x0, y0) = tmerc_point(
                lons[base],
                lats[base],
                k0,
                lon0_rad,
                false_easting,
                false_northing,
                a,
                e2,
            );
            let (x1, y1) = tmerc_point(
                lons[base + 1],
                lats[base + 1],
                k0,
                lon0_rad,
                false_easting,
                false_northing,
                a,
                e2,
            );
            let (x2, y2) = tmerc_point(
                lons[base + 2],
                lats[base + 2],
                k0,
                lon0_rad,
                false_easting,
                false_northing,
                a,
                e2,
            );
            let (x3, y3) = tmerc_point(
                lons[base + 3],
                lats[base + 3],
                k0,
                lon0_rad,
                false_easting,
                false_northing,
                a,
                e2,
            );
            xs[base] = x0;
            xs[base + 1] = x1;
            xs[base + 2] = x2;
            xs[base + 3] = x3;
            ys[base] = y0;
            ys[base + 1] = y1;
            ys[base + 2] = y2;
            ys[base + 3] = y3;
        }

        // Handle the tail (0–3 remaining points)
        for i in 0..remainder {
            let idx = chunks * 4 + i;
            let (x, y) = tmerc_point(
                lons[idx],
                lats[idx],
                k0,
                lon0_rad,
                false_easting,
                false_northing,
                a,
                e2,
            );
            xs[idx] = x;
            ys[idx] = y;
        }
    } else {
        // Scalar fallback: one point per iteration
        for i in 0..n {
            let (x, y) = tmerc_point(
                lons[i],
                lats[i],
                k0,
                lon0_rad,
                false_easting,
                false_northing,
                a,
                e2,
            );
            xs[i] = x;
            ys[i] = y;
        }
    }

    (xs, ys)
}

// ---------------------------------------------------------------------------
// Mercator forward batch kernel
// ---------------------------------------------------------------------------

/// Single-point ellipsoidal Mercator forward computation.
///
/// Formula (Snyder eq. 7-7 / 15-1):
/// ```text
/// x = k0 * a * (lon − lon0)
/// y = k0 * a * ln[ tan(π/4 + φ/2) · ((1 − e·sin φ) / (1 + e·sin φ))^(e/2) ]
/// ```
#[inline(always)]
fn merc_point_fwd(
    lon_rad: f64,
    lat_rad: f64,
    lon0_rad: f64,
    k0: f64,
    a: f64,
    e: f64,
) -> (f64, f64) {
    let x = k0 * a * (lon_rad - lon0_rad);
    let sin_lat = lat_rad.sin();
    // Isometric latitude ψ (Mercator conformal factor)
    let psi = (FRAC_PI_4 + lat_rad / 2.0).tan().ln()
        + (e / 2.0) * ((1.0 - e * sin_lat) / (1.0 + e * sin_lat)).ln();
    let y = k0 * a * psi;
    (x, y)
}

/// Batch ellipsoidal Mercator forward projection.
///
/// All input angles must be in **radians**.
///
/// # Parameters
/// * `lons` – longitude array (radians)
/// * `lats` – latitude array (radians)
/// * `lon0_rad` – central meridian (radians)
/// * `k0` – scale factor
/// * `a` – semi-major axis (metres)
/// * `e` – first eccentricity (not e²)
pub(crate) fn merc_forward_batch(
    lons: &[f64],
    lats: &[f64],
    lon0_rad: f64,
    k0: f64,
    a: f64,
    e: f64,
) -> (Vec<f64>, Vec<f64>) {
    debug_assert_eq!(lons.len(), lats.len());
    let n = lons.len();
    let mut xs = vec![0.0f64; n];
    let mut ys = vec![0.0f64; n];

    if is_avx2() {
        let chunks = n / 4;
        let remainder = n % 4;

        for c in 0..chunks {
            let base = c * 4;
            let (x0, y0) = merc_point_fwd(lons[base], lats[base], lon0_rad, k0, a, e);
            let (x1, y1) = merc_point_fwd(lons[base + 1], lats[base + 1], lon0_rad, k0, a, e);
            let (x2, y2) = merc_point_fwd(lons[base + 2], lats[base + 2], lon0_rad, k0, a, e);
            let (x3, y3) = merc_point_fwd(lons[base + 3], lats[base + 3], lon0_rad, k0, a, e);
            xs[base] = x0;
            xs[base + 1] = x1;
            xs[base + 2] = x2;
            xs[base + 3] = x3;
            ys[base] = y0;
            ys[base + 1] = y1;
            ys[base + 2] = y2;
            ys[base + 3] = y3;
        }

        for i in 0..remainder {
            let idx = chunks * 4 + i;
            let (x, y) = merc_point_fwd(lons[idx], lats[idx], lon0_rad, k0, a, e);
            xs[idx] = x;
            ys[idx] = y;
        }
    } else {
        for i in 0..n {
            let (x, y) = merc_point_fwd(lons[i], lats[i], lon0_rad, k0, a, e);
            xs[i] = x;
            ys[i] = y;
        }
    }

    (xs, ys)
}

// ---------------------------------------------------------------------------
// Mercator inverse batch kernel
// ---------------------------------------------------------------------------

/// Single-point ellipsoidal Mercator inverse computation.
///
/// Recovers `(lon_rad, lat_rad)` from projected `(x, y)`.
/// Uses iterative inversion of the isometric latitude (converges in ~5 iterations).
#[inline(always)]
#[allow(dead_code)]
fn merc_point_inv(x: f64, y: f64, lon0_rad: f64, k0: f64, a: f64, e: f64) -> (f64, f64) {
    let lon_rad = x / (k0 * a) + lon0_rad;
    // Iterative inversion: start from spherical approximation
    let t = (-y / (k0 * a)).exp();
    let mut lat_rad = FRAC_PI_2 - 2.0 * t.atan();
    for _ in 0..15 {
        let sin_lat = lat_rad.sin();
        let factor = ((1.0 - e * sin_lat) / (1.0 + e * sin_lat)).powf(e / 2.0);
        let lat_new = FRAC_PI_2 - 2.0 * (t * factor).atan();
        let delta = (lat_new - lat_rad).abs();
        lat_rad = lat_new;
        if delta < 1e-12 {
            break;
        }
    }
    (lon_rad, lat_rad)
}

/// Batch ellipsoidal Mercator inverse projection.
///
/// # Parameters
/// * `xs`, `ys` – projected coordinates (metres)
/// * `lon0_rad` – central meridian (radians)
/// * `k0` – scale factor
/// * `a` – semi-major axis (metres)
/// * `e` – first eccentricity
#[allow(dead_code)]
pub(crate) fn merc_inverse_batch(
    xs: &[f64],
    ys: &[f64],
    lon0_rad: f64,
    k0: f64,
    a: f64,
    e: f64,
) -> (Vec<f64>, Vec<f64>) {
    debug_assert_eq!(xs.len(), ys.len());
    let n = xs.len();
    let mut lons = vec![0.0f64; n];
    let mut lats = vec![0.0f64; n];

    if is_avx2() {
        let chunks = n / 4;
        let remainder = n % 4;

        for c in 0..chunks {
            let base = c * 4;
            let (lon0, lat0) = merc_point_inv(xs[base], ys[base], lon0_rad, k0, a, e);
            let (lon1, lat1) = merc_point_inv(xs[base + 1], ys[base + 1], lon0_rad, k0, a, e);
            let (lon2, lat2) = merc_point_inv(xs[base + 2], ys[base + 2], lon0_rad, k0, a, e);
            let (lon3, lat3) = merc_point_inv(xs[base + 3], ys[base + 3], lon0_rad, k0, a, e);
            lons[base] = lon0;
            lons[base + 1] = lon1;
            lons[base + 2] = lon2;
            lons[base + 3] = lon3;
            lats[base] = lat0;
            lats[base + 1] = lat1;
            lats[base + 2] = lat2;
            lats[base + 3] = lat3;
        }

        for i in 0..remainder {
            let idx = chunks * 4 + i;
            let (lon, lat) = merc_point_inv(xs[idx], ys[idx], lon0_rad, k0, a, e);
            lons[idx] = lon;
            lats[idx] = lat;
        }
    } else {
        for i in 0..n {
            let (lon, lat) = merc_point_inv(xs[i], ys[i], lon0_rad, k0, a, e);
            lons[i] = lon;
            lats[i] = lat;
        }
    }

    (lons, lats)
}

// ---------------------------------------------------------------------------
// Lambert Conformal Conic forward batch kernel
// ---------------------------------------------------------------------------

/// Single-point spherical Lambert Conformal Conic forward computation.
///
/// Formula (Snyder §15, sphere):
/// ```text
/// ρ  = R · F / tan^n(π/4 + φ/2)   = R · F · exp(−n · ln tan(π/4+φ/2))
/// θ  = n · (λ − λ₀)
/// x  = ρ · sin θ
/// y  = ρ₀ − ρ · cos θ
/// ```
#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn lcc_point_fwd(
    lon_rad: f64,
    lat_rad: f64,
    n: f64,
    big_f: f64,
    rho0: f64,
    lon0_rad: f64,
    false_easting: f64,
    false_northing: f64,
    a: f64,
) -> (f64, f64) {
    let t = (FRAC_PI_4 + lat_rad / 2.0).tan().ln();
    let rho = a * big_f / (n * t).exp();
    let theta = n * (lon_rad - lon0_rad);
    let (sin_theta, cos_theta) = theta.sin_cos();
    let x = rho * sin_theta + false_easting;
    let y = rho0 - rho * cos_theta + false_northing;
    (x, y)
}

/// Batch spherical Lambert Conformal Conic forward projection.
///
/// All input angles must be in **radians**.
///
/// # Parameters
/// * `lons` – longitude array (radians)
/// * `lats` – latitude array (radians)
/// * `n` – cone constant (dimensionless)
/// * `big_f` – F parameter (dimensionless)
/// * `rho0` – origin radius (dimensionless; caller must pre-multiply by `a`)
/// * `lon0_rad` – central meridian (radians)
/// * `false_easting`, `false_northing` – offsets (metres)
/// * `a` – sphere radius (metres)
#[allow(clippy::too_many_arguments)]
pub(crate) fn lcc_forward_batch(
    lons: &[f64],
    lats: &[f64],
    n: f64,
    big_f: f64,
    rho0: f64,
    lon0_rad: f64,
    false_easting: f64,
    false_northing: f64,
    a: f64,
) -> (Vec<f64>, Vec<f64>) {
    debug_assert_eq!(lons.len(), lats.len());
    let len = lons.len();
    let mut xs = vec![0.0f64; len];
    let mut ys = vec![0.0f64; len];

    if is_avx2() {
        let chunks = len / 4;
        let remainder = len % 4;

        for c in 0..chunks {
            let base = c * 4;
            let (x0, y0) = lcc_point_fwd(
                lons[base],
                lats[base],
                n,
                big_f,
                rho0,
                lon0_rad,
                false_easting,
                false_northing,
                a,
            );
            let (x1, y1) = lcc_point_fwd(
                lons[base + 1],
                lats[base + 1],
                n,
                big_f,
                rho0,
                lon0_rad,
                false_easting,
                false_northing,
                a,
            );
            let (x2, y2) = lcc_point_fwd(
                lons[base + 2],
                lats[base + 2],
                n,
                big_f,
                rho0,
                lon0_rad,
                false_easting,
                false_northing,
                a,
            );
            let (x3, y3) = lcc_point_fwd(
                lons[base + 3],
                lats[base + 3],
                n,
                big_f,
                rho0,
                lon0_rad,
                false_easting,
                false_northing,
                a,
            );
            xs[base] = x0;
            xs[base + 1] = x1;
            xs[base + 2] = x2;
            xs[base + 3] = x3;
            ys[base] = y0;
            ys[base + 1] = y1;
            ys[base + 2] = y2;
            ys[base + 3] = y3;
        }

        for i in 0..remainder {
            let idx = chunks * 4 + i;
            let (x, y) = lcc_point_fwd(
                lons[idx],
                lats[idx],
                n,
                big_f,
                rho0,
                lon0_rad,
                false_easting,
                false_northing,
                a,
            );
            xs[idx] = x;
            ys[idx] = y;
        }
    } else {
        for i in 0..len {
            let (x, y) = lcc_point_fwd(
                lons[i],
                lats[i],
                n,
                big_f,
                rho0,
                lon0_rad,
                false_easting,
                false_northing,
                a,
            );
            xs[i] = x;
            ys[i] = y;
        }
    }

    (xs, ys)
}

// ---------------------------------------------------------------------------
// LCC cone-parameter precomputation
// ---------------------------------------------------------------------------

/// Precompute LCC cone parameters `(n, F, rho0)` from standard parallels.
///
/// Uses the sphere-based formulae (Snyder §15).
///
/// # Parameters
/// * `lat0_rad` – latitude of origin (radians)
/// * `lat1_rad` – first standard parallel (radians)
/// * `lat2_rad` – second standard parallel (radians)
///
/// Returns `None` if the standard parallels produce a degenerate cone (`n ≈ 0`).
pub(crate) fn lcc_cone_params(
    lat0_rad: f64,
    lat1_rad: f64,
    lat2_rad: f64,
) -> Option<(f64, f64, f64)> {
    let t0 = (FRAC_PI_4 + lat0_rad / 2.0).tan().ln();
    let t1 = (FRAC_PI_4 + lat1_rad / 2.0).tan().ln();
    let t2 = (FRAC_PI_4 + lat2_rad / 2.0).tan().ln();

    let n = if (lat1_rad - lat2_rad).abs() < 1e-12 {
        lat1_rad.sin()
    } else {
        (lat1_rad.cos().ln() - lat2_rad.cos().ln()) / (t2 - t1)
    };

    if n.abs() < 1e-12 {
        return None;
    }

    let big_f = lat1_rad.cos() * (n * t1).exp() / n;
    let rho0 = if t0.is_finite() {
        big_f / (n * t0).exp()
    } else {
        0.0
    };

    Some((n, big_f, rho0))
}

// ---------------------------------------------------------------------------
// Unit tests for the SIMD kernels
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-6;

    // UTM zone 32N parameters
    const UTM32_LON0: f64 = 9.0_f64 * core::f64::consts::PI / 180.0;
    const UTM_K0: f64 = 0.9996;
    const UTM_FE: f64 = 500_000.0;
    const UTM_FN: f64 = 0.0;

    #[test]
    fn test_tmerc_batch_scalar_consistency() {
        let lons: Vec<f64> = (0..8)
            .map(|i| (9.0 + i as f64 * 0.1).to_radians())
            .collect();
        let lats: Vec<f64> = (0..8)
            .map(|i| (48.0 + i as f64 * 0.1).to_radians())
            .collect();

        let (xs, ys) = tmerc_forward_batch(
            &lons, &lats, UTM_K0, UTM32_LON0, UTM_FE, UTM_FN, WGS84_A, WGS84_E2,
        );

        for i in 0..8 {
            let (x_scalar, y_scalar) = tmerc_point(
                lons[i], lats[i], UTM_K0, UTM32_LON0, UTM_FE, UTM_FN, WGS84_A, WGS84_E2,
            );
            assert!((xs[i] - x_scalar).abs() < TOL, "x mismatch at i={i}");
            assert!((ys[i] - y_scalar).abs() < TOL, "y mismatch at i={i}");
        }
    }

    #[test]
    fn test_merc_forward_batch_scalar_consistency() {
        let lons: Vec<f64> = (0..8)
            .map(|i| (0.0 + i as f64 * 5.0).to_radians())
            .collect();
        let lats: Vec<f64> = (0..8)
            .map(|i| (0.0 + i as f64 * 5.0).to_radians())
            .collect();

        let (xs, ys) = merc_forward_batch(&lons, &lats, 0.0, 1.0, WGS84_A, WGS84_E);

        for i in 0..8 {
            let (xr, yr) = merc_point_fwd(lons[i], lats[i], 0.0, 1.0, WGS84_A, WGS84_E);
            assert!((xs[i] - xr).abs() < TOL, "x mismatch at i={i}");
            assert!((ys[i] - yr).abs() < TOL, "y mismatch at i={i}");
        }
    }

    #[test]
    fn test_merc_roundtrip() {
        let lons: Vec<f64> = (0..8).map(|i| (i as f64 * 10.0).to_radians()).collect();
        let lats: Vec<f64> = (0..8).map(|i| (i as f64 * 5.0).to_radians()).collect();

        let (xs, ys) = merc_forward_batch(&lons, &lats, 0.0, 1.0, WGS84_A, WGS84_E);
        let (lons2, lats2) = merc_inverse_batch(&xs, &ys, 0.0, 1.0, WGS84_A, WGS84_E);

        for i in 0..8 {
            assert!(
                (lons[i] - lons2[i]).abs() < 1e-10,
                "lon roundtrip mismatch at i={i}"
            );
            assert!(
                (lats[i] - lats2[i]).abs() < 1e-10,
                "lat roundtrip mismatch at i={i}"
            );
        }
    }

    #[test]
    fn test_lcc_batch_scalar_consistency() {
        let lat0 = 52.0_f64.to_radians();
        let lat1 = 35.0_f64.to_radians();
        let lat2 = 65.0_f64.to_radians();
        let lon0 = 10.0_f64.to_radians();

        let (n, big_f, rho0) = lcc_cone_params(lat0, lat1, lat2).expect("valid params");

        let lons: Vec<f64> = (0..8)
            .map(|i| (5.0 + i as f64 * 2.0).to_radians())
            .collect();
        let lats: Vec<f64> = (0..8)
            .map(|i| (40.0 + i as f64 * 2.0).to_radians())
            .collect();

        let (xs, ys) = lcc_forward_batch(
            &lons,
            &lats,
            n,
            big_f,
            rho0 * WGS84_A,
            lon0,
            0.0,
            0.0,
            WGS84_A,
        );

        for i in 0..8 {
            let (xr, yr) = lcc_point_fwd(
                lons[i],
                lats[i],
                n,
                big_f,
                rho0 * WGS84_A,
                lon0,
                0.0,
                0.0,
                WGS84_A,
            );
            assert!((xs[i] - xr).abs() < TOL, "x mismatch at i={i}");
            assert!((ys[i] - yr).abs() < TOL, "y mismatch at i={i}");
        }
    }

    #[test]
    fn test_tmerc_partial_tail() {
        // 5 points — exercises the remainder (5 % 4 == 1)
        let lons: Vec<f64> = (0..5)
            .map(|i| (9.0 + i as f64 * 0.1).to_radians())
            .collect();
        let lats: Vec<f64> = (0..5)
            .map(|i| (48.0 + i as f64 * 0.1).to_radians())
            .collect();

        let (xs, ys) = tmerc_forward_batch(
            &lons, &lats, UTM_K0, UTM32_LON0, UTM_FE, UTM_FN, WGS84_A, WGS84_E2,
        );

        assert_eq!(xs.len(), 5);
        assert_eq!(ys.len(), 5);
        for i in 0..5 {
            assert!(xs[i].is_finite(), "x[{i}] not finite");
            assert!(ys[i].is_finite(), "y[{i}] not finite");
        }
    }

    #[test]
    fn test_empty_batch() {
        let (xs, ys) = tmerc_forward_batch(
            &[],
            &[],
            UTM_K0,
            UTM32_LON0,
            UTM_FE,
            UTM_FN,
            WGS84_A,
            WGS84_E2,
        );
        assert!(xs.is_empty());
        assert!(ys.is_empty());

        let (xs2, ys2) = merc_forward_batch(&[], &[], 0.0, 1.0, WGS84_A, WGS84_E);
        assert!(xs2.is_empty());
        assert!(ys2.is_empty());

        let params = lcc_cone_params(
            52.0_f64.to_radians(),
            35.0_f64.to_radians(),
            65.0_f64.to_radians(),
        )
        .expect("valid");
        let (xs3, ys3) = lcc_forward_batch(
            &[],
            &[],
            params.0,
            params.1,
            params.2,
            0.0,
            0.0,
            0.0,
            WGS84_A,
        );
        assert!(xs3.is_empty());
        assert!(ys3.is_empty());
    }
}
