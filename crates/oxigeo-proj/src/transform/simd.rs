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

// ---------------------------------------------------------------------------
// Meridional arc (shared helper)
// ---------------------------------------------------------------------------

/// Meridional arc from the equator to latitude `phi` (radians) on the ellipsoid
/// with semi-major axis `a` and first eccentricity squared `e2`.
///
/// Uses the **third-flattening (Helmert) expansion** in
/// `n = f / (2 − f) = (a − b) / (a + b)` rather than a series in `e²`.  For
/// terrestrial ellipsoids `n ≈ 1.7e-3`, so the truncated `n⁴` form is accurate
/// to **~5e-8 m** worldwide.
///
/// This matters: the classic Snyder `e²`-power series previously used here is
/// truncated inconsistently (it carries an `e⁸·sin 8φ` term but omits the `e⁸`
/// corrections to the `φ` and `sin 2φ` coefficients) and is only good to
/// ~3.5e-4 m at 48° N — large enough to show up as a visible batch-vs-scalar
/// disagreement against OxiProj's Poder/Engsager transverse Mercator.
///
/// Reference: Helmert (1880); see also Karney, "Transverse Mercator with an
/// accuracy of a few nanometers" (2011), eq. 8.
#[inline(always)]
fn meridional_arc(phi: f64, a: f64, e2: f64) -> f64 {
    // n = f/(2-f) expressed directly in terms of e²:
    //   b/a = sqrt(1 - e²)  ⇒  n = (1 - b/a) / (1 + b/a)
    let b_over_a = (1.0 - e2).sqrt();
    let n = (1.0 - b_over_a) / (1.0 + b_over_a);
    let n2 = n * n;
    let n3 = n2 * n;
    let n4 = n2 * n2;

    let a0 = 1.0 + n2 / 4.0 + n4 / 64.0;
    let a2 = -1.5 * (n - n3 / 8.0);
    let a4 = (15.0 / 16.0) * (n2 - n4 / 4.0);
    let a6 = -(35.0 / 48.0) * n3;
    let a8 = (315.0 / 512.0) * n4;

    (a / (1.0 + n))
        * (a0 * phi
            + a2 * (2.0 * phi).sin()
            + a4 * (4.0 * phi).sin()
            + a6 * (6.0 * phi).sin()
            + a8 * (8.0 * phi).sin())
}

// ---------------------------------------------------------------------------
// Conformal-latitude helpers (shared by Mercator and LCC)
// ---------------------------------------------------------------------------

/// PROJ's `pj_tsfn`: `t = tan(π/4 − φ/2) / ((1 − e sin φ)/(1 + e sin φ))^(e/2)`.
///
/// `t` equals `exp(−ψ)` where `ψ` is the isometric latitude, so it is the
/// natural building block for every conformal projection (Mercator, LCC).
#[inline(always)]
fn tsfn(phi: f64, e: f64) -> f64 {
    let es = e * phi.sin();
    (FRAC_PI_4 - phi / 2.0).tan() / ((1.0 - es) / (1.0 + es)).powf(0.5 * e)
}

/// PROJ's `pj_msfn`: `m = cos φ / sqrt(1 − e² sin² φ)` — the radius of the
/// parallel at `φ` on the unit-`a` ellipsoid.
#[inline(always)]
fn msfn(phi: f64, e2: f64) -> f64 {
    let sin_phi = phi.sin();
    phi.cos() / (1.0 - e2 * sin_phi * sin_phi).sqrt()
}

// ---------------------------------------------------------------------------
// Transverse Mercator batch kernel
// ---------------------------------------------------------------------------

/// Single-point Transverse Mercator forward computation (Snyder §8).
///
/// `m0` is the meridional arc at the projection's latitude of origin
/// (`+lat_0`).  It is a **per-transformer** constant, so the caller computes it
/// once via [`meridional_arc`] and passes it in rather than recomputing it for
/// every point.  Passing `m0 = 0.0` reproduces the UTM case (`lat_0 = 0`).
///
/// Returns `(x_easting, y_northing)` in metres.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn tmerc_point(
    lon_rad: f64,
    lat_rad: f64,
    k0: f64,
    lon0_rad: f64,
    m0: f64,
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

    // Meridional arc at the point; `m0` (arc at `lat_0`) supplied by the caller.
    let m = meridional_arc(lat_rad, a, e2);

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
/// * `lat0_rad` – latitude of origin `+lat_0` (radians).  Non-zero for e.g. the
///   Japan Plane Rectangular CS (`lat_0 = 33°/36°/…`); zero for UTM.
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
    lat0_rad: f64,
    false_easting: f64,
    false_northing: f64,
    a: f64,
    e2: f64,
) -> (Vec<f64>, Vec<f64>) {
    debug_assert_eq!(lons.len(), lats.len());
    let n = lons.len();
    let mut xs = vec![0.0f64; n];
    let mut ys = vec![0.0f64; n];

    // Meridional arc at the latitude of origin: a per-transformer constant,
    // hoisted out of the per-point loop.
    let m0 = meridional_arc(lat0_rad, a, e2);

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
                m0,
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
                m0,
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
                m0,
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
                m0,
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
                m0,
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
                m0,
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
/// x = k0 * a * (lon − lon0)                                 + false_easting
/// y = k0 * a * ln[ tan(π/4 + φ/2) · ((1 − e·sin φ)/(1 + e·sin φ))^(e/2) ]
///                                                           + false_northing
/// ```
///
/// `false_easting` / `false_northing` are in metres and are added *after* the
/// `k0 · a` scaling, matching PROJ's `fwd_finalize`.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn merc_point_fwd(
    lon_rad: f64,
    lat_rad: f64,
    lon0_rad: f64,
    k0: f64,
    false_easting: f64,
    false_northing: f64,
    a: f64,
    e: f64,
) -> (f64, f64) {
    let x = k0 * a * (lon_rad - lon0_rad);
    let sin_lat = lat_rad.sin();
    // Isometric latitude ψ (Mercator conformal factor)
    let psi = (FRAC_PI_4 + lat_rad / 2.0).tan().ln()
        + (e / 2.0) * ((1.0 - e * sin_lat) / (1.0 + e * sin_lat)).ln();
    let y = k0 * a * psi;
    (x + false_easting, y + false_northing)
}

/// Batch ellipsoidal Mercator forward projection.
///
/// All input angles must be in **radians**.
///
/// # Parameters
/// * `lons` – longitude array (radians)
/// * `lats` – latitude array (radians)
/// * `lon0_rad` – central meridian (radians)
/// * `k0` – scale factor (already derived from `+lat_ts` by the caller when
///   that parameter is present — PROJ ignores `+k` in that case)
/// * `false_easting`, `false_northing` – offsets in metres
/// * `a` – semi-major axis (metres)
/// * `e` – first eccentricity (not e²)
#[allow(clippy::too_many_arguments)]
pub(crate) fn merc_forward_batch(
    lons: &[f64],
    lats: &[f64],
    lon0_rad: f64,
    k0: f64,
    false_easting: f64,
    false_northing: f64,
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
            let (x0, y0) = merc_point_fwd(
                lons[base],
                lats[base],
                lon0_rad,
                k0,
                false_easting,
                false_northing,
                a,
                e,
            );
            let (x1, y1) = merc_point_fwd(
                lons[base + 1],
                lats[base + 1],
                lon0_rad,
                k0,
                false_easting,
                false_northing,
                a,
                e,
            );
            let (x2, y2) = merc_point_fwd(
                lons[base + 2],
                lats[base + 2],
                lon0_rad,
                k0,
                false_easting,
                false_northing,
                a,
                e,
            );
            let (x3, y3) = merc_point_fwd(
                lons[base + 3],
                lats[base + 3],
                lon0_rad,
                k0,
                false_easting,
                false_northing,
                a,
                e,
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
            let (x, y) = merc_point_fwd(
                lons[idx],
                lats[idx],
                lon0_rad,
                k0,
                false_easting,
                false_northing,
                a,
                e,
            );
            xs[idx] = x;
            ys[idx] = y;
        }
    } else {
        for i in 0..n {
            let (x, y) = merc_point_fwd(
                lons[i],
                lats[i],
                lon0_rad,
                k0,
                false_easting,
                false_northing,
                a,
                e,
            );
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
/// Recovers `(lon_rad, lat_rad)` from projected `(x, y)`.  The false easting /
/// northing (metres) are removed **before** un-scaling by `k0 · a`, mirroring
/// [`merc_point_fwd`].
/// Uses iterative inversion of the isometric latitude (converges in ~5 iterations).
#[inline(always)]
#[allow(dead_code)]
#[allow(clippy::too_many_arguments)]
fn merc_point_inv(
    x: f64,
    y: f64,
    lon0_rad: f64,
    k0: f64,
    false_easting: f64,
    false_northing: f64,
    a: f64,
    e: f64,
) -> (f64, f64) {
    let lon_rad = (x - false_easting) / (k0 * a) + lon0_rad;
    // Iterative inversion: start from spherical approximation
    let t = (-(y - false_northing) / (k0 * a)).exp();
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
/// * `k0` – scale factor (derived from `+lat_ts` when present, as in the
///   forward direction)
/// * `false_easting`, `false_northing` – offsets in metres, removed first
/// * `a` – semi-major axis (metres)
/// * `e` – first eccentricity
#[allow(dead_code)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn merc_inverse_batch(
    xs: &[f64],
    ys: &[f64],
    lon0_rad: f64,
    k0: f64,
    false_easting: f64,
    false_northing: f64,
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
            let (lon0, lat0) = merc_point_inv(
                xs[base],
                ys[base],
                lon0_rad,
                k0,
                false_easting,
                false_northing,
                a,
                e,
            );
            let (lon1, lat1) = merc_point_inv(
                xs[base + 1],
                ys[base + 1],
                lon0_rad,
                k0,
                false_easting,
                false_northing,
                a,
                e,
            );
            let (lon2, lat2) = merc_point_inv(
                xs[base + 2],
                ys[base + 2],
                lon0_rad,
                k0,
                false_easting,
                false_northing,
                a,
                e,
            );
            let (lon3, lat3) = merc_point_inv(
                xs[base + 3],
                ys[base + 3],
                lon0_rad,
                k0,
                false_easting,
                false_northing,
                a,
                e,
            );
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
            let (lon, lat) = merc_point_inv(
                xs[idx],
                ys[idx],
                lon0_rad,
                k0,
                false_easting,
                false_northing,
                a,
                e,
            );
            lons[idx] = lon;
            lats[idx] = lat;
        }
    } else {
        for i in 0..n {
            let (lon, lat) = merc_point_inv(
                xs[i],
                ys[i],
                lon0_rad,
                k0,
                false_easting,
                false_northing,
                a,
                e,
            );
            lons[i] = lon;
            lats[i] = lat;
        }
    }

    (lons, lats)
}

// ---------------------------------------------------------------------------
// Lambert Conformal Conic forward batch kernel
// ---------------------------------------------------------------------------

/// Per-transformer LCC cone constants, in **normalised (a = 1)** units.
///
/// Built once by [`lcc_cone_params`] and reused for every point in a batch.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LccCone {
    /// Cone constant `n` (= sin of the standard parallel for the tangent case).
    pub(crate) n: f64,
    /// `c = m1 · t1^(−n) / n` — PROJ's `P->c`.
    pub(crate) c: f64,
    /// Radius at the latitude of origin, `ρ0 = c · t(φ0)^n`.
    pub(crate) rho0: f64,
    /// `true` when the ellipsoidal branch is in use (`e² > 0`).
    pub(crate) ellipsoidal: bool,
}

/// Single-point Lambert Conformal Conic forward computation.
///
/// Ellipsoidal form (Snyder §15, "Lambert Conformal Conic — ellipsoid"; matches
/// PROJ's `src/projections/lcc.cpp`):
///
/// ```text
/// t  = tan(π/4 − φ/2) / ((1 − e sin φ)/(1 + e sin φ))^(e/2)
/// ρ  = a · k0 · c · t^n
/// θ  = n · (λ − λ₀)
/// x  = ρ · sin θ            + false_easting
/// y  = a · k0 · ρ₀ − ρ·cos θ + false_northing
/// ```
///
/// The spherical branch (`e = 0`) reduces to `t = tan(π/4 − φ/2)`, so both
/// branches share one code path via [`tsfn`].
///
/// `cone.c` / `cone.rho0` are normalised to `a = 1`; the caller's `a` (and the
/// projection's `k0`) scale them here, matching PROJ's `fwd_finalize`.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn lcc_point_fwd(
    lon_rad: f64,
    lat_rad: f64,
    cone: &LccCone,
    lon0_rad: f64,
    false_easting: f64,
    false_northing: f64,
    a: f64,
    e: f64,
) -> (f64, f64) {
    // At the pole on the far side of the cone `t → 0` and `ρ → 0`; at the near
    // pole `t → ∞`.  `powf` handles both, producing 0 or +inf, and the caller
    // rejects non-finite results.
    let t = tsfn(lat_rad, e);
    let rho = a * cone.c * t.powf(cone.n);
    let theta = cone.n * (lon_rad - lon0_rad);
    let (sin_theta, cos_theta) = theta.sin_cos();
    let x = rho * sin_theta + false_easting;
    let y = a * cone.rho0 - rho * cos_theta + false_northing;
    (x, y)
}

/// Batch Lambert Conformal Conic forward projection.
///
/// All input angles must be in **radians**.
///
/// # Parameters
/// * `lons` – longitude array (radians)
/// * `lats` – latitude array (radians)
/// * `cone` – precomputed cone constants from [`lcc_cone_params`]
/// * `lon0_rad` – central meridian (radians)
/// * `false_easting`, `false_northing` – offsets (metres)
/// * `a` – semi-major axis (metres)
/// * `e2` – first eccentricity squared (0 for a sphere)
#[allow(clippy::too_many_arguments)]
pub(crate) fn lcc_forward_batch(
    lons: &[f64],
    lats: &[f64],
    cone: &LccCone,
    lon0_rad: f64,
    false_easting: f64,
    false_northing: f64,
    a: f64,
    e2: f64,
) -> (Vec<f64>, Vec<f64>) {
    debug_assert_eq!(lons.len(), lats.len());
    let len = lons.len();
    let mut xs = vec![0.0f64; len];
    let mut ys = vec![0.0f64; len];

    // Per-transformer constant, hoisted out of the per-point loop.
    let e = if cone.ellipsoidal { e2.sqrt() } else { 0.0 };

    if is_avx2() {
        let chunks = len / 4;
        let remainder = len % 4;

        for c in 0..chunks {
            let base = c * 4;
            let (x0, y0) = lcc_point_fwd(
                lons[base],
                lats[base],
                cone,
                lon0_rad,
                false_easting,
                false_northing,
                a,
                e,
            );
            let (x1, y1) = lcc_point_fwd(
                lons[base + 1],
                lats[base + 1],
                cone,
                lon0_rad,
                false_easting,
                false_northing,
                a,
                e,
            );
            let (x2, y2) = lcc_point_fwd(
                lons[base + 2],
                lats[base + 2],
                cone,
                lon0_rad,
                false_easting,
                false_northing,
                a,
                e,
            );
            let (x3, y3) = lcc_point_fwd(
                lons[base + 3],
                lats[base + 3],
                cone,
                lon0_rad,
                false_easting,
                false_northing,
                a,
                e,
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
                cone,
                lon0_rad,
                false_easting,
                false_northing,
                a,
                e,
            );
            xs[idx] = x;
            ys[idx] = y;
        }
    } else {
        for i in 0..len {
            let (x, y) = lcc_point_fwd(
                lons[i],
                lats[i],
                cone,
                lon0_rad,
                false_easting,
                false_northing,
                a,
                e,
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

/// Precompute the LCC cone constants from the standard parallels.
///
/// Uses the **ellipsoidal** formulae (Snyder §15) whenever `e2 > 0`, and the
/// spherical ones otherwise.  A sphere-only kernel is not adequate here: for
/// EPSG:2154 (RGF93 / Lambert-93, GRS80) the spherical approximation is off by
/// ~214 m only 1.5° from the origin.
///
/// # Parameters
/// * `lat0_rad` – latitude of origin (radians)
/// * `lat1_rad` – first standard parallel (radians)
/// * `lat2_rad` – second standard parallel (radians)
/// * `e2` – first eccentricity squared of the target ellipsoid (0 for a sphere)
/// * `k0` – scale factor (`+k_0`, 1.0 for the usual LCC_2SP definitions).
///   PROJ multiplies both `ρ` and `ρ₀` by `k0`, which is equivalent to scaling
///   the constant `c` — so it is folded in here, once per transformer, instead
///   of costing a multiply per point.
///
/// Returns `None` if the standard parallels produce a degenerate cone
/// (`n ≈ 0`, i.e. `φ1 ≈ −φ2`) or a non-finite constant.
pub(crate) fn lcc_cone_params(
    lat0_rad: f64,
    lat1_rad: f64,
    lat2_rad: f64,
    e2: f64,
    k0: f64,
) -> Option<LccCone> {
    /// PROJ's `EPS10` — the tolerance it uses for "parallels coincide" and
    /// "origin is at a pole".
    const EPS10: f64 = 1e-10;

    let ellipsoidal = e2 > 0.0;
    let e = if ellipsoidal { e2.sqrt() } else { 0.0 };

    // Secant (two distinct parallels) vs tangent (one parallel).
    let secant = (lat1_rad - lat2_rad).abs() >= EPS10;

    let t1 = tsfn(lat1_rad, e);
    let n = if secant {
        let t2 = tsfn(lat2_rad, e);
        let (m1, m2) = if ellipsoidal {
            (msfn(lat1_rad, e2), msfn(lat2_rad, e2))
        } else {
            (lat1_rad.cos(), lat2_rad.cos())
        };
        let denom = (t1 / t2).ln();
        if denom.abs() < EPS10 {
            return None;
        }
        (m1 / m2).ln() / denom
    } else {
        lat1_rad.sin()
    };

    if !n.is_finite() || n.abs() < 1e-12 {
        return None;
    }

    let m1 = if ellipsoidal {
        msfn(lat1_rad, e2)
    } else {
        lat1_rad.cos()
    };
    let c = k0 * m1 * t1.powf(-n) / n;

    // At a polar origin ρ₀ is 0 (PROJ applies the same EPS10 test).
    let rho0 = if (lat0_rad.abs() - FRAC_PI_2).abs() < EPS10 {
        0.0
    } else {
        c * tsfn(lat0_rad, e).powf(n)
    };

    if !c.is_finite() || !rho0.is_finite() {
        return None;
    }

    Some(LccCone {
        n,
        c,
        rho0,
        ellipsoidal,
    })
}

// ---------------------------------------------------------------------------
// Unit tests for the SIMD kernels
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-6;

    /// WGS-84 first eccentricity, derived from `WGS84_E2` for the Mercator
    /// kernels (which take `e`, not `e²`).
    fn wgs84_e() -> f64 {
        WGS84_E2.sqrt()
    }

    // UTM zone 32N parameters
    const UTM32_LON0: f64 = 9.0_f64 * core::f64::consts::PI / 180.0;
    const UTM_K0: f64 = 0.9996;
    const UTM_FE: f64 = 500_000.0;
    const UTM_FN: f64 = 0.0;
    /// UTM fixes `+lat_0 = 0`, so the meridional arc at the origin is zero.
    const UTM_LAT0: f64 = 0.0;
    const UTM_M0: f64 = 0.0;

    #[test]
    fn test_tmerc_batch_scalar_consistency() {
        let lons: Vec<f64> = (0..8)
            .map(|i| (9.0 + i as f64 * 0.1).to_radians())
            .collect();
        let lats: Vec<f64> = (0..8)
            .map(|i| (48.0 + i as f64 * 0.1).to_radians())
            .collect();

        let (xs, ys) = tmerc_forward_batch(
            &lons, &lats, UTM_K0, UTM32_LON0, UTM_LAT0, UTM_FE, UTM_FN, WGS84_A, WGS84_E2,
        );

        for i in 0..8 {
            let (x_scalar, y_scalar) = tmerc_point(
                lons[i], lats[i], UTM_K0, UTM32_LON0, UTM_M0, UTM_FE, UTM_FN, WGS84_A, WGS84_E2,
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

        let (xs, ys) = merc_forward_batch(&lons, &lats, 0.0, 1.0, 0.0, 0.0, WGS84_A, wgs84_e());

        for i in 0..8 {
            let (xr, yr) = merc_point_fwd(lons[i], lats[i], 0.0, 1.0, 0.0, 0.0, WGS84_A, wgs84_e());
            assert!((xs[i] - xr).abs() < TOL, "x mismatch at i={i}");
            assert!((ys[i] - yr).abs() < TOL, "y mismatch at i={i}");
        }
    }

    #[test]
    fn test_merc_roundtrip() {
        let lons: Vec<f64> = (0..8).map(|i| (i as f64 * 10.0).to_radians()).collect();
        let lats: Vec<f64> = (0..8).map(|i| (i as f64 * 5.0).to_radians()).collect();

        // Non-zero false easting/northing: the round trip must undo them.
        let (xs, ys) = merc_forward_batch(
            &lons,
            &lats,
            0.0,
            1.0,
            5_000_000.0,
            10_000_000.0,
            WGS84_A,
            wgs84_e(),
        );
        let (lons2, lats2) = merc_inverse_batch(
            &xs,
            &ys,
            0.0,
            1.0,
            5_000_000.0,
            10_000_000.0,
            WGS84_A,
            wgs84_e(),
        );

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

        let cone = lcc_cone_params(lat0, lat1, lat2, WGS84_E2, 1.0).expect("valid params");

        let lons: Vec<f64> = (0..8)
            .map(|i| (5.0 + i as f64 * 2.0).to_radians())
            .collect();
        let lats: Vec<f64> = (0..8)
            .map(|i| (40.0 + i as f64 * 2.0).to_radians())
            .collect();

        let (xs, ys) = lcc_forward_batch(&lons, &lats, &cone, lon0, 0.0, 0.0, WGS84_A, WGS84_E2);

        for i in 0..8 {
            let (xr, yr) = lcc_point_fwd(
                lons[i],
                lats[i],
                &cone,
                lon0,
                0.0,
                0.0,
                WGS84_A,
                WGS84_E2.sqrt(),
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
            &lons, &lats, UTM_K0, UTM32_LON0, UTM_LAT0, UTM_FE, UTM_FN, WGS84_A, WGS84_E2,
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
            UTM_LAT0,
            UTM_FE,
            UTM_FN,
            WGS84_A,
            WGS84_E2,
        );
        assert!(xs.is_empty());
        assert!(ys.is_empty());

        let (xs2, ys2) = merc_forward_batch(&[], &[], 0.0, 1.0, 0.0, 0.0, WGS84_A, wgs84_e());
        assert!(xs2.is_empty());
        assert!(ys2.is_empty());

        let cone = lcc_cone_params(
            52.0_f64.to_radians(),
            35.0_f64.to_radians(),
            65.0_f64.to_radians(),
            WGS84_E2,
            1.0,
        )
        .expect("valid");
        let (xs3, ys3) = lcc_forward_batch(&[], &[], &cone, 0.0, 0.0, 0.0, WGS84_A, WGS84_E2);
        assert!(xs3.is_empty());
        assert!(ys3.is_empty());
    }
}
