//! Pseudo-cylindrical map projections.
//!
//! This module implements equal-area and compromise pseudo-cylindrical projections:
//!
//! - **Sinusoidal** (`+proj=sinu`): Simple equal-area pseudo-cylindrical used for MODIS data.
//! - **Mollweide** (`+proj=moll`): Equal-area elliptical world projection.
//! - **Robinson** (`+proj=robin`): Compromise projection minimizing overall distortion.
//! - **Eckert IV** (`+proj=eck4`): Equal-area pseudo-cylindrical with rounded poles.
//! - **Eckert VI** (`+proj=eck6`): Equal-area pseudo-cylindrical with sinusoidal meridians.
//!
//! All implementations use sphere-based math (radius `a`). Coordinates are in radians
//! at the internal level; the public API accepts and returns **degrees**.

use crate::error::{Error, Result};

// ---------------------------------------------------------------------------
// Maximum Newton iterations and convergence tolerance
// ---------------------------------------------------------------------------
const MAX_ITER: usize = 100;
const TOLERANCE: f64 = 1e-12;

// WGS-84 semi-major axis (metres) used as the default sphere radius when no
// custom radius is provided.
const DEFAULT_RADIUS: f64 = 6_378_137.0;

// ---------------------------------------------------------------------------
// Robinson lookup table (Snyder 1993, Table 13)
//
// Each row corresponds to 5° intervals from 0° to 90° latitude.
// Columns: [ X_len_factor,  Y_len_factor ]
// X_len_factor: meridional length (PLEN)  — scales the x coordinate
// Y_len_factor: parallel   length (PDFE)  — scales the y coordinate
// ---------------------------------------------------------------------------
// fmt: [[PLEN, PDFE]; 19]  (0°, 5°, 10°, …, 90°)
const ROBINSON_TABLE: [[f64; 2]; 19] = [
    [1.0000, 0.0000], // 0°
    [0.9986, 0.0620], // 5°
    [0.9954, 0.1240], // 10°
    [0.9900, 0.1860], // 15°
    [0.9822, 0.2480], // 20°
    [0.9730, 0.3100], // 25°
    [0.9600, 0.3720], // 30°
    [0.9427, 0.4340], // 35°
    [0.9216, 0.4958], // 40°
    [0.8962, 0.5571], // 45°
    [0.8679, 0.6176], // 50°
    [0.8350, 0.6769], // 55°
    [0.7986, 0.7346], // 60°
    [0.7597, 0.7903], // 65°
    [0.7186, 0.8435], // 70°
    [0.6732, 0.8936], // 75°
    [0.6213, 0.9394], // 80°
    [0.5722, 0.9761], // 85°
    [0.5322, 1.0000], // 90°
];

// Robinson constant: R * 0.8487  (Snyder's FXC constant)
const ROBINSON_FXC: f64 = 0.8487;
// Robinson constant: R * 1.3523  (Snyder's FYC constant)
const ROBINSON_FYC: f64 = 1.3523;

// ---------------------------------------------------------------------------
// Helper: linear interpolation at fractional table index
// ---------------------------------------------------------------------------
fn robinson_interp(lat_abs_deg: f64) -> (f64, f64) {
    // lat_abs_deg is in [0, 90]
    let idx_f = lat_abs_deg / 5.0;
    let lo = idx_f.floor() as usize;
    let hi = (lo + 1).min(18);
    let frac = idx_f - lo as f64;

    let plen = ROBINSON_TABLE[lo][0] + frac * (ROBINSON_TABLE[hi][0] - ROBINSON_TABLE[lo][0]);
    let pdfe = ROBINSON_TABLE[lo][1] + frac * (ROBINSON_TABLE[hi][1] - ROBINSON_TABLE[lo][1]);
    (plen, pdfe)
}

// ---------------------------------------------------------------------------
// Sinusoidal projection
// ---------------------------------------------------------------------------

/// Sinusoidal equal-area pseudo-cylindrical projection (`+proj=sinu`).
///
/// The Sinusoidal is one of the oldest known projections and is still widely
/// used for MODIS satellite data products. It preserves area faithfully but
/// produces severe angular distortion at high latitudes and near the
/// antimeridian.
///
/// **Forward equations (sphere):**
/// ```text
/// x = (λ − λ₀) · cos(φ) · R
/// y = φ · R
/// ```
///
/// **Inverse equations (sphere):**
/// ```text
/// φ = y / R
/// λ = λ₀ + x / (R · cos(φ))
/// ```
#[derive(Debug, Clone)]
pub struct Sinusoidal {
    /// Central meridian λ₀ (degrees).
    pub lon_0: f64,
    /// Sphere radius (metres). Defaults to WGS-84 semi-major axis.
    pub radius: f64,
}

impl Default for Sinusoidal {
    fn default() -> Self {
        Self {
            lon_0: 0.0,
            radius: DEFAULT_RADIUS,
        }
    }
}

impl Sinusoidal {
    /// Creates a Sinusoidal projection with a central meridian and sphere radius.
    pub fn new(lon_0_deg: f64, radius: f64) -> Self {
        Self {
            lon_0: lon_0_deg,
            radius,
        }
    }

    /// Projects a geographic coordinate (degrees) to projected metres.
    ///
    /// # Errors
    ///
    /// Returns `NumericalError` if the projection would produce a non-finite result
    /// (e.g., latitude > 90°).
    pub fn forward(&self, lon_deg: f64, lat_deg: f64) -> Result<(f64, f64)> {
        let lat = lat_deg.to_radians();
        let d_lon = (lon_deg - self.lon_0).to_radians();

        let x = self.radius * d_lon * lat.cos();
        let y = self.radius * lat;

        if !x.is_finite() || !y.is_finite() {
            return Err(Error::numerical_error(
                "sinusoidal forward: non-finite result",
            ));
        }
        Ok((x, y))
    }

    /// Unprojects projected metres back to geographic degrees.
    ///
    /// # Errors
    ///
    /// Returns `NumericalError` if `cos(lat) ≈ 0` (at the poles).
    pub fn inverse(&self, x: f64, y: f64) -> Result<(f64, f64)> {
        let lat = y / self.radius;
        let cos_lat = lat.cos();

        if cos_lat.abs() < TOLERANCE {
            return Err(Error::numerical_error(
                "sinusoidal inverse: latitude at pole — longitude undefined",
            ));
        }

        let lon = self.lon_0 + (x / (self.radius * cos_lat)).to_degrees();
        Ok((lon, lat.to_degrees()))
    }
}

// ---------------------------------------------------------------------------
// Mollweide projection
// ---------------------------------------------------------------------------

/// Mollweide equal-area pseudo-cylindrical projection (`+proj=moll`).
///
/// Preserves area; all parallels are straight and spaced along central meridian
/// in true proportion. Meridians are elliptical arcs. Used for whole-world
/// thematic maps.
///
/// **Auxiliary angle θ** is found iteratively from:
/// ```text
/// 2θ + sin(2θ) = π · sin(φ)
/// ```
///
/// **Forward:**
/// ```text
/// x = (2√2 / π) · (λ − λ₀) · cos(θ) · R
/// y = √2 · sin(θ) · R
/// ```
///
/// **Inverse:**
/// ```text
/// θ = arcsin(y / (√2 · R))
/// φ = arcsin((2θ + sin(2θ)) / π)
/// λ = λ₀ + π · x / (2√2 · R · cos(θ))
/// ```
#[derive(Debug, Clone)]
pub struct Mollweide {
    /// Central meridian λ₀ (degrees).
    pub lon_0: f64,
    /// Sphere radius (metres).
    pub radius: f64,
}

impl Default for Mollweide {
    fn default() -> Self {
        Self {
            lon_0: 0.0,
            radius: DEFAULT_RADIUS,
        }
    }
}

impl Mollweide {
    /// Creates a Mollweide projection.
    pub fn new(lon_0_deg: f64, radius: f64) -> Self {
        Self {
            lon_0: lon_0_deg,
            radius,
        }
    }

    /// Solves 2θ + sin(2θ) = π·sin(φ) for θ via Newton–Raphson.
    fn solve_theta(lat_rad: f64) -> Result<f64> {
        let target = core::f64::consts::PI * lat_rad.sin();
        let mut theta = lat_rad; // Initial estimate

        for i in 0..MAX_ITER {
            let f = 2.0 * theta + (2.0 * theta).sin() - target;
            let df = 2.0 + 2.0 * (2.0 * theta).cos();

            if df.abs() < TOLERANCE {
                return Err(Error::numerical_error(
                    "mollweide: derivative near zero in Newton iteration",
                ));
            }

            let d_theta = f / df;
            theta -= d_theta;

            if d_theta.abs() < TOLERANCE {
                return Ok(theta);
            }

            if i == MAX_ITER - 1 {
                return Err(Error::convergence_error(MAX_ITER));
            }
        }
        Ok(theta)
    }

    /// Projects geographic coordinate (degrees) to projected metres.
    pub fn forward(&self, lon_deg: f64, lat_deg: f64) -> Result<(f64, f64)> {
        let lat = lat_deg.to_radians();
        let d_lon = (lon_deg - self.lon_0).to_radians();

        let theta = Self::solve_theta(lat)?;
        let sqrt2 = core::f64::consts::SQRT_2;

        let x = self.radius * (2.0 * sqrt2 / core::f64::consts::PI) * d_lon * theta.cos();
        let y = self.radius * sqrt2 * theta.sin();

        if !x.is_finite() || !y.is_finite() {
            return Err(Error::numerical_error(
                "mollweide forward: non-finite result",
            ));
        }
        Ok((x, y))
    }

    /// Unprojects projected metres to geographic degrees.
    pub fn inverse(&self, x: f64, y: f64) -> Result<(f64, f64)> {
        let sqrt2 = core::f64::consts::SQRT_2;

        let sin_theta = y / (self.radius * sqrt2);
        if sin_theta.abs() > 1.0 + TOLERANCE {
            return Err(Error::numerical_error(
                "mollweide inverse: y out of valid range",
            ));
        }
        let sin_theta = sin_theta.clamp(-1.0, 1.0);
        let theta = sin_theta.asin();

        let cos_theta = theta.cos();
        if cos_theta.abs() < TOLERANCE {
            // At the poles, longitude is indeterminate — return central meridian
            let sin_phi = (2.0 * theta + (2.0 * theta).sin()) / core::f64::consts::PI;
            let lat = sin_phi.clamp(-1.0, 1.0).asin().to_degrees();
            return Ok((self.lon_0, lat));
        }

        let sin_phi = (2.0 * theta + (2.0 * theta).sin()) / core::f64::consts::PI;
        let lat = sin_phi.clamp(-1.0, 1.0).asin().to_degrees();
        let lon = self.lon_0
            + (core::f64::consts::PI * x / (2.0 * sqrt2 * self.radius * cos_theta)).to_degrees();

        Ok((lon, lat))
    }
}

// ---------------------------------------------------------------------------
// Robinson projection
// ---------------------------------------------------------------------------

/// Robinson compromise world projection (`+proj=robin`).
///
/// Not equal-area or conformal; it minimises overall distortion using a lookup
/// table of pseudocylindrical scaling factors. Used extensively for world maps.
///
/// **Algorithm (Snyder 1993 p.376):**
/// ```text
/// x = R · ROBINSON_FXC · PLEN(φ) · (λ − λ₀)
/// y = R · ROBINSON_FYC · PDFE(φ)
/// ```
/// where PLEN and PDFE are interpolated from the standard 5° lookup table.
#[derive(Debug, Clone)]
pub struct Robinson {
    /// Central meridian λ₀ (degrees).
    pub lon_0: f64,
    /// Sphere radius (metres).
    pub radius: f64,
}

impl Default for Robinson {
    fn default() -> Self {
        Self {
            lon_0: 0.0,
            radius: DEFAULT_RADIUS,
        }
    }
}

impl Robinson {
    /// Creates a Robinson projection.
    pub fn new(lon_0_deg: f64, radius: f64) -> Self {
        Self {
            lon_0: lon_0_deg,
            radius,
        }
    }

    /// Projects geographic coordinate (degrees) to projected metres.
    pub fn forward(&self, lon_deg: f64, lat_deg: f64) -> Result<(f64, f64)> {
        let lat_abs = lat_deg.abs().min(90.0);
        let sign = if lat_deg < 0.0 { -1.0_f64 } else { 1.0_f64 };
        let d_lon = (lon_deg - self.lon_0).to_radians();

        let (plen, pdfe) = robinson_interp(lat_abs);

        let x = self.radius * ROBINSON_FXC * plen * d_lon;
        let y = self.radius * ROBINSON_FYC * pdfe * sign;

        if !x.is_finite() || !y.is_finite() {
            return Err(Error::numerical_error(
                "robinson forward: non-finite result",
            ));
        }
        Ok((x, y))
    }

    /// Unprojects projected metres to geographic degrees (Newton–Raphson on PDFE table).
    pub fn inverse(&self, x: f64, y: f64) -> Result<(f64, f64)> {
        let sign = if y < 0.0 { -1.0_f64 } else { 1.0_f64 };
        let y_abs = y.abs();

        // Normalised PDFE value we want to match
        let pdfe_target = y_abs / (self.radius * ROBINSON_FYC);

        if pdfe_target > 1.0 + TOLERANCE {
            return Err(Error::numerical_error(
                "robinson inverse: y out of valid range",
            ));
        }
        let pdfe_target = pdfe_target.min(1.0);

        // Binary search for table index
        let mut lo_idx = 0usize;
        let mut hi_idx = 18usize;
        while hi_idx - lo_idx > 1 {
            let mid = (lo_idx + hi_idx) / 2;
            if ROBINSON_TABLE[mid][1] <= pdfe_target {
                lo_idx = mid;
            } else {
                hi_idx = mid;
            }
        }

        // Linear interpolation within the bracket
        let pdfe_lo = ROBINSON_TABLE[lo_idx][1];
        let pdfe_hi = ROBINSON_TABLE[hi_idx][1];
        let plen_lo = ROBINSON_TABLE[lo_idx][0];
        let plen_hi = ROBINSON_TABLE[hi_idx][0];

        let frac = if (pdfe_hi - pdfe_lo).abs() < TOLERANCE {
            0.0
        } else {
            (pdfe_target - pdfe_lo) / (pdfe_hi - pdfe_lo)
        };

        let lat_abs_deg = (lo_idx as f64 + frac) * 5.0;
        let lat = sign * lat_abs_deg;

        let plen = plen_lo + frac * (plen_hi - plen_lo);
        if plen.abs() < TOLERANCE {
            return Err(Error::numerical_error("robinson inverse: plen near zero"));
        }

        let d_lon_rad = x / (self.radius * ROBINSON_FXC * plen);
        let lon = self.lon_0 + d_lon_rad.to_degrees();

        Ok((lon, lat))
    }
}

// ---------------------------------------------------------------------------
// Eckert IV projection
// ---------------------------------------------------------------------------

/// Eckert IV equal-area pseudo-cylindrical projection (`+proj=eck4`).
///
/// Has rounded pole lines (half the length of the equator) and unequally
/// spaced parallels. Aesthetically pleasing for world maps.
///
/// **Auxiliary angle θ** (half the latitude circle) satisfies:
/// ```text
/// θ + sin(θ)·cos(θ) + 2·sin(θ) = (2 + π/2)·sin(φ)
/// ```
///
/// **Forward:**
/// ```text
/// C = 2 / √(π · (4 + π))
/// x = 2R · C · (λ − λ₀) · (1 + cos(θ))
/// y = 2R · C · √π · sin(θ)
/// ```
#[derive(Debug, Clone)]
pub struct EckertIV {
    /// Central meridian λ₀ (degrees).
    pub lon_0: f64,
    /// Sphere radius (metres).
    pub radius: f64,
}

impl Default for EckertIV {
    fn default() -> Self {
        Self {
            lon_0: 0.0,
            radius: DEFAULT_RADIUS,
        }
    }
}

impl EckertIV {
    /// Creates an Eckert IV projection.
    pub fn new(lon_0_deg: f64, radius: f64) -> Self {
        Self {
            lon_0: lon_0_deg,
            radius,
        }
    }

    /// Snyder constant C = 2 / sqrt(π(4+π))
    fn c_const() -> f64 {
        2.0 / (core::f64::consts::PI * (4.0 + core::f64::consts::PI)).sqrt()
    }

    /// Solves  θ + sin(θ)cos(θ) + 2sin(θ) = (2 + π/2)·sin(φ)  for θ.
    fn solve_theta(lat_rad: f64) -> Result<f64> {
        let rhs = (2.0 + core::f64::consts::FRAC_PI_2) * lat_rad.sin();
        let mut theta = lat_rad; // decent initial guess

        for i in 0..MAX_ITER {
            let f = theta + theta.sin() * theta.cos() + 2.0 * theta.sin() - rhs;
            let df = 1.0 + 2.0 * theta.cos() + 2.0 * theta.cos().powi(2);

            if df.abs() < TOLERANCE {
                return Err(Error::numerical_error(
                    "eckert iv: derivative near zero in Newton iteration",
                ));
            }

            let d_theta = f / df;
            theta -= d_theta;

            if d_theta.abs() < TOLERANCE {
                return Ok(theta);
            }

            if i == MAX_ITER - 1 {
                return Err(Error::convergence_error(MAX_ITER));
            }
        }
        Ok(theta)
    }

    /// Projects geographic coordinate (degrees) to projected metres.
    pub fn forward(&self, lon_deg: f64, lat_deg: f64) -> Result<(f64, f64)> {
        let lat = lat_deg.to_radians();
        let d_lon = (lon_deg - self.lon_0).to_radians();
        let c = Self::c_const();

        let theta = Self::solve_theta(lat)?;
        let x = 2.0 * self.radius * c * d_lon * (1.0 + theta.cos());
        let y = 2.0 * self.radius * c * core::f64::consts::PI.sqrt() * theta.sin();

        if !x.is_finite() || !y.is_finite() {
            return Err(Error::numerical_error(
                "eckert iv forward: non-finite result",
            ));
        }
        Ok((x, y))
    }

    /// Unprojects projected metres to geographic degrees.
    pub fn inverse(&self, x: f64, y: f64) -> Result<(f64, f64)> {
        let c = Self::c_const();
        let sin_theta = y / (2.0 * self.radius * c * core::f64::consts::PI.sqrt());
        if sin_theta.abs() > 1.0 + TOLERANCE {
            return Err(Error::numerical_error(
                "eckert iv inverse: y out of valid range",
            ));
        }
        let theta = sin_theta.clamp(-1.0, 1.0).asin();
        let cos_theta = theta.cos();

        // Recover φ from  θ + sin(θ)cos(θ) + 2sin(θ) = (2+π/2)sin(φ)
        let sin_phi = (theta + theta.sin() * cos_theta + 2.0 * theta.sin())
            / (2.0 + core::f64::consts::FRAC_PI_2);
        let lat = sin_phi.clamp(-1.0, 1.0).asin().to_degrees();

        if (1.0 + cos_theta).abs() < TOLERANCE {
            return Ok((self.lon_0, lat));
        }

        let d_lon_rad = x / (2.0 * self.radius * c * (1.0 + cos_theta));
        let lon = self.lon_0 + d_lon_rad.to_degrees();

        Ok((lon, lat))
    }
}

// ---------------------------------------------------------------------------
// Eckert VI projection
// ---------------------------------------------------------------------------

/// Eckert VI equal-area pseudo-cylindrical projection (`+proj=eck6`).
///
/// Sinusoidal meridians; uses simpler auxiliary angle θ satisfying:
/// ```text
/// θ + sin(θ) = (1 + π/2) · sin(φ)
/// ```
///
/// **Forward:**
/// ```text
/// C = √(2 / (π(π + 4)))
/// x = R · C · (λ − λ₀) · (1 + cos(θ))
/// y = 2R · C · θ
/// ```
/// Note: These scale factors make the projection exactly equal-area.
#[derive(Debug, Clone)]
pub struct EckertVI {
    /// Central meridian λ₀ (degrees).
    pub lon_0: f64,
    /// Sphere radius (metres).
    pub radius: f64,
}

impl Default for EckertVI {
    fn default() -> Self {
        Self {
            lon_0: 0.0,
            radius: DEFAULT_RADIUS,
        }
    }
}

impl EckertVI {
    /// Creates an Eckert VI projection.
    pub fn new(lon_0_deg: f64, radius: f64) -> Self {
        Self {
            lon_0: lon_0_deg,
            radius,
        }
    }

    /// C = sqrt(2 / (π(π+4)))
    fn c_const() -> f64 {
        (2.0 / (core::f64::consts::PI * (core::f64::consts::PI + 4.0))).sqrt()
    }

    /// Solves θ + sin(θ) = (1 + π/2)·sin(φ) for θ via Newton–Raphson.
    fn solve_theta(lat_rad: f64) -> Result<f64> {
        let rhs = (1.0 + core::f64::consts::FRAC_PI_2) * lat_rad.sin();
        let mut theta = lat_rad;

        for i in 0..MAX_ITER {
            let f = theta + theta.sin() - rhs;
            let df = 1.0 + theta.cos();

            if df.abs() < TOLERANCE {
                return Err(Error::numerical_error(
                    "eckert vi: derivative near zero in Newton iteration",
                ));
            }

            let d_theta = f / df;
            theta -= d_theta;

            if d_theta.abs() < TOLERANCE {
                return Ok(theta);
            }

            if i == MAX_ITER - 1 {
                return Err(Error::convergence_error(MAX_ITER));
            }
        }
        Ok(theta)
    }

    /// Projects geographic coordinate (degrees) to projected metres.
    pub fn forward(&self, lon_deg: f64, lat_deg: f64) -> Result<(f64, f64)> {
        let lat = lat_deg.to_radians();
        let d_lon = (lon_deg - self.lon_0).to_radians();
        let c = Self::c_const();

        let theta = Self::solve_theta(lat)?;
        let x = self.radius * c * d_lon * (1.0 + theta.cos());
        let y = 2.0 * self.radius * c * theta;

        if !x.is_finite() || !y.is_finite() {
            return Err(Error::numerical_error(
                "eckert vi forward: non-finite result",
            ));
        }
        Ok((x, y))
    }

    /// Unprojects projected metres to geographic degrees.
    pub fn inverse(&self, x: f64, y: f64) -> Result<(f64, f64)> {
        let c = Self::c_const();
        let theta = y / (2.0 * self.radius * c);
        let cos_theta = theta.cos();

        let sin_phi = (theta + theta.sin()) / (1.0 + core::f64::consts::FRAC_PI_2);
        let lat = sin_phi.clamp(-1.0, 1.0).asin().to_degrees();

        if (1.0 + cos_theta).abs() < TOLERANCE {
            return Ok((self.lon_0, lat));
        }

        let d_lon_rad = x / (self.radius * c * (1.0 + cos_theta));
        let lon = self.lon_0 + d_lon_rad.to_degrees();

        Ok((lon, lat))
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    const ROUND_TRIP_TOL: f64 = 1e-6; // degrees

    fn round_trip_sinu(lon: f64, lat: f64) {
        let proj = Sinusoidal::default();
        let (x, y) = proj.forward(lon, lat).expect("forward ok");
        let (lon2, lat2) = proj.inverse(x, y).expect("inverse ok");
        assert!(
            (lon - lon2).abs() < ROUND_TRIP_TOL,
            "sinusoidal lon: {} vs {}",
            lon,
            lon2
        );
        assert!(
            (lat - lat2).abs() < ROUND_TRIP_TOL,
            "sinusoidal lat: {} vs {}",
            lat,
            lat2
        );
    }

    fn round_trip_moll(lon: f64, lat: f64) {
        let proj = Mollweide::default();
        let (x, y) = proj.forward(lon, lat).expect("forward ok");
        let (lon2, lat2) = proj.inverse(x, y).expect("inverse ok");
        assert!(
            (lon - lon2).abs() < ROUND_TRIP_TOL,
            "mollweide lon: {} vs {}",
            lon,
            lon2
        );
        assert!(
            (lat - lat2).abs() < ROUND_TRIP_TOL,
            "mollweide lat: {} vs {}",
            lat,
            lat2
        );
    }

    #[test]
    fn test_sinusoidal_origin() {
        let proj = Sinusoidal::default();
        let (x, y) = proj.forward(0.0, 0.0).expect("ok");
        assert!(x.abs() < 1e-9);
        assert!(y.abs() < 1e-9);
    }

    #[test]
    fn test_sinusoidal_round_trips() {
        round_trip_sinu(0.0, 0.0);
        round_trip_sinu(10.0, 20.0);
        round_trip_sinu(-100.0, -45.0);
        round_trip_sinu(170.0, 60.0);
        round_trip_sinu(-170.0, -60.0);
    }

    #[test]
    fn test_sinusoidal_pole_error() {
        // inverse at pole should fail (cos(90°)=0)
        let proj = Sinusoidal::default();
        let (_, y_pole) = proj.forward(0.0, 90.0).expect("forward ok");
        let result = proj.inverse(1e6, y_pole);
        assert!(result.is_err(), "expected error at pole");
    }

    #[test]
    fn test_mollweide_origin() {
        let proj = Mollweide::default();
        let (x, y) = proj.forward(0.0, 0.0).expect("ok");
        assert!(x.abs() < 1e-6);
        assert!(y.abs() < 1e-6);
    }

    #[test]
    fn test_mollweide_round_trips() {
        round_trip_moll(0.0, 0.0);
        round_trip_moll(20.0, 30.0);
        round_trip_moll(-150.0, 60.0);
        round_trip_moll(90.0, -45.0);
    }

    #[test]
    fn test_robinson_origin() {
        let proj = Robinson::default();
        let (x, y) = proj.forward(0.0, 0.0).expect("ok");
        assert!(x.abs() < 1.0); // should be very close to 0
        assert!(y.abs() < 1.0);
    }

    #[test]
    fn test_robinson_round_trip() {
        let proj = Robinson::default();
        let test_cases = [(0.0, 0.0), (10.0, 20.0), (-90.0, 45.0), (150.0, -30.0)];
        for (lon, lat) in test_cases {
            let (x, y) = proj.forward(lon, lat).expect("forward ok");
            let (lon2, lat2) = proj.inverse(x, y).expect("inverse ok");
            assert!(
                (lon - lon2).abs() < 1e-3,
                "robinson lon: {} vs {}",
                lon,
                lon2
            );
            assert!(
                (lat - lat2).abs() < 1e-3,
                "robinson lat: {} vs {}",
                lat,
                lat2
            );
        }
    }

    #[test]
    fn test_eckert4_origin() {
        let proj = EckertIV::default();
        let (x, y) = proj.forward(0.0, 0.0).expect("ok");
        assert!(x.abs() < 1e-6);
        assert!(y.abs() < 1e-6);
    }

    #[test]
    fn test_eckert4_round_trip() {
        let proj = EckertIV::default();
        let test_cases = [(0.0, 0.0), (30.0, 45.0), (-120.0, -30.0), (10.0, 80.0)];
        for (lon, lat) in test_cases {
            let (x, y) = proj.forward(lon, lat).expect("forward ok");
            let (lon2, lat2) = proj.inverse(x, y).expect("inverse ok");
            assert!(
                (lon - lon2).abs() < ROUND_TRIP_TOL,
                "eckert iv lon: {} vs {}",
                lon,
                lon2
            );
            assert!(
                (lat - lat2).abs() < ROUND_TRIP_TOL,
                "eckert iv lat: {} vs {}",
                lat,
                lat2
            );
        }
    }

    #[test]
    fn test_eckert6_origin() {
        let proj = EckertVI::default();
        let (x, y) = proj.forward(0.0, 0.0).expect("ok");
        assert!(x.abs() < 1e-6);
        assert!(y.abs() < 1e-6);
    }

    #[test]
    fn test_eckert6_round_trip() {
        let proj = EckertVI::default();
        let test_cases = [(0.0, 0.0), (45.0, 60.0), (-30.0, -45.0), (100.0, 20.0)];
        for (lon, lat) in test_cases {
            let (x, y) = proj.forward(lon, lat).expect("forward ok");
            let (lon2, lat2) = proj.inverse(x, y).expect("inverse ok");
            assert!(
                (lon - lon2).abs() < ROUND_TRIP_TOL,
                "eckert vi lon: {} vs {}",
                lon,
                lon2
            );
            assert!(
                (lat - lat2).abs() < ROUND_TRIP_TOL,
                "eckert vi lat: {} vs {}",
                lat,
                lat2
            );
        }
    }
}
