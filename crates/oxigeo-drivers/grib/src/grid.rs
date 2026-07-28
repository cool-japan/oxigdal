//! Grid definitions and coordinate transformations for GRIB formats.
//!
//! This module provides grid definition types and coordinate transformations for various
//! grid types including regular lat/lon, Lambert conformal, Mercator, polar stereographic,
//! and other common GRIB grids.

use crate::error::{GribError, Result};
use serde::{Deserialize, Serialize};

/// Grid definition for GRIB data
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GridDefinition {
    /// Regular latitude/longitude grid (equidistant cylindrical projection)
    LatLon(LatLonGrid),
    /// Rotated latitude/longitude grid
    RotatedLatLon(RotatedLatLonGrid),
    /// Lambert Conformal Conic projection
    LambertConformal(LambertConformalGrid),
    /// Mercator projection
    Mercator(MercatorGrid),
    /// Polar Stereographic projection
    PolarStereographic(PolarStereographicGrid),
    /// Gaussian latitude/longitude grid
    Gaussian(GaussianGrid),
    /// Space view perspective or orthographic
    SpaceView(SpaceViewGrid),
}

/// Regular latitude/longitude grid
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LatLonGrid {
    /// Number of points along a parallel (longitude)
    pub ni: u32,
    /// Number of points along a meridian (latitude)
    pub nj: u32,
    /// Latitude of first grid point (degrees)
    pub la1: f64,
    /// Longitude of first grid point (degrees)
    pub lo1: f64,
    /// Latitude of last grid point (degrees)
    pub la2: f64,
    /// Longitude of last grid point (degrees)
    pub lo2: f64,
    /// i direction increment (degrees)
    pub di: f64,
    /// j direction increment (degrees)
    pub dj: f64,
    /// Scanning mode flags
    pub scan_mode: ScanMode,
}

impl LatLonGrid {
    /// Get total number of grid points
    pub fn num_points(&self) -> usize {
        (self.ni as usize) * (self.nj as usize)
    }

    /// Get latitude for grid point index
    pub fn latitude(&self, j: u32) -> Result<f64> {
        if j >= self.nj {
            return Err(GribError::OutOfRange(format!(
                "j index {} out of range [0, {})",
                j, self.nj
            )));
        }

        let lat = if self.scan_mode.j_positive {
            self.la1 + (j as f64) * self.dj
        } else {
            self.la1 - (j as f64) * self.dj
        };

        Ok(lat)
    }

    /// Get longitude for grid point index
    pub fn longitude(&self, i: u32) -> Result<f64> {
        if i >= self.ni {
            return Err(GribError::OutOfRange(format!(
                "i index {} out of range [0, {})",
                i, self.ni
            )));
        }

        let lon = if self.scan_mode.i_positive {
            self.lo1 + (i as f64) * self.di
        } else {
            self.lo1 - (i as f64) * self.di
        };

        // Normalize to [-180, 180]
        let mut lon = lon;
        while lon > 180.0 {
            lon -= 360.0;
        }
        while lon < -180.0 {
            lon += 360.0;
        }

        Ok(lon)
    }

    /// Get (lat, lon) for grid point (i, j)
    pub fn coordinates(&self, i: u32, j: u32) -> Result<(f64, f64)> {
        Ok((self.latitude(j)?, self.longitude(i)?))
    }
}

/// Rotated latitude/longitude grid
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RotatedLatLonGrid {
    /// Base regular lat/lon grid
    pub base: LatLonGrid,
    /// Latitude of southern pole of rotation (degrees)
    pub lat_south_pole: f64,
    /// Longitude of southern pole of rotation (degrees)
    pub lon_south_pole: f64,
    /// Angle of rotation (degrees)
    pub angle: f64,
}

impl RotatedLatLonGrid {
    /// Get total number of grid points
    pub fn num_points(&self) -> usize {
        self.base.num_points()
    }

    /// Returns the true geographic `(latitude, longitude)` in degrees of grid
    /// point `(i, j)`.
    ///
    /// The base grid supplies the point's coordinates in the *rotated* system;
    /// this method un-rotates them back to geographic coordinates given the
    /// rotated south pole `(lat_south_pole, lon_south_pole)` and the angle of
    /// rotation about the new polar axis (WMO GDT 3.1). The standard
    /// Z-Y'-Z'' spherical rotation is used; for the common `angle == 0` case
    /// this reduces to the classic two-angle un-rotation used by eccodes.
    pub fn coordinates(&self, i: u32, j: u32) -> Result<(f64, f64)> {
        let (rlat_deg, rlon_deg) = self.base.coordinates(i, j)?;
        // Angle of rotation acts about the rotated polar axis: apply it as a
        // longitude offset in the rotated frame before un-rotating.
        let rlon = (rlon_deg - self.angle).to_radians();
        let rlat = rlat_deg.to_radians();

        let theta = -(90.0 + self.lat_south_pole).to_radians();
        let phi = -self.lon_south_pole.to_radians();

        // Rotated-frame unit vector.
        let x = rlon.cos() * rlat.cos();
        let y = rlon.sin() * rlat.cos();
        let z = rlat.sin();

        // Inverse rotation (Y then Z) into the geographic frame.
        let x2 = theta.cos() * phi.cos() * x + phi.sin() * y + theta.sin() * phi.cos() * z;
        let y2 = -theta.cos() * phi.sin() * x + phi.cos() * y - theta.sin() * phi.sin() * z;
        let z2 = -theta.sin() * x + theta.cos() * z;

        let lat = z2.clamp(-1.0, 1.0).asin().to_degrees();
        let lon = normalize_longitude(y2.atan2(x2).to_degrees());
        Ok((lat, lon))
    }
}

/// Lambert Conformal Conic projection grid
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LambertConformalGrid {
    /// Number of points along X-axis
    pub nx: u32,
    /// Number of points along Y-axis
    pub ny: u32,
    /// Latitude of first grid point (degrees)
    pub la1: f64,
    /// Longitude of first grid point (degrees)
    pub lo1: f64,
    /// Orientation of the grid (longitude of meridian parallel to Y-axis)
    pub lov: f64,
    /// X-direction grid length (m)
    pub dx: f64,
    /// Y-direction grid length (m)
    pub dy: f64,
    /// First latitude from pole at which the secant cone cuts the sphere (degrees)
    pub latin1: f64,
    /// Second latitude from pole at which the secant cone cuts the sphere (degrees)
    pub latin2: f64,
    /// Latitude of southern pole (degrees)
    pub lat_south_pole: f64,
    /// Longitude of southern pole (degrees)
    pub lon_south_pole: f64,
    /// Radius of the (spherical) Earth in metres, used by the projection math.
    pub earth_radius_m: f64,
    /// Scanning mode
    pub scan_mode: ScanMode,
}

impl LambertConformalGrid {
    /// Get total number of grid points
    pub fn num_points(&self) -> usize {
        (self.nx as usize) * (self.ny as usize)
    }

    /// Returns the `(latitude, longitude)` in degrees of grid point `(i, j)`.
    ///
    /// Implements the spherical Lambert Conformal Conic inverse projection
    /// (WMO Manual on Codes Vol. I.2, GDT 3.30 / standard secant-cone map
    /// projection formulas). The grid is regular in projected metres; the
    /// first grid point `(0, 0)` maps back to `(la1, lo1)`.
    pub fn coordinates(&self, i: u32, j: u32) -> Result<(f64, f64)> {
        if i >= self.nx || j >= self.ny {
            return Err(GribError::OutOfRange(format!(
                "index ({i}, {j}) out of range ({}x{})",
                self.nx, self.ny
            )));
        }
        let a = self.earth_radius_m;
        let phi1 = self.latin1.to_radians();
        let phi2 = self.latin2.to_radians();
        let lo_v = self.lov.to_radians();

        // Cone constant n.
        let n = if (phi1 - phi2).abs() < 1e-12 {
            phi1.sin()
        } else {
            (phi1.cos() / phi2.cos()).ln() / ((cone_t(phi2)).ln() - (cone_t(phi1)).ln())
        };
        if n.abs() < 1e-12 {
            return Err(GribError::CoordinateError(
                "Lambert conformal: degenerate cone constant".to_string(),
            ));
        }
        let f = phi1.cos() * cone_t(phi1).powf(n) / n;
        // rho at the first grid point and at the LaD reference (use latin1 as
        // the y origin reference — GRIB defines the grid relative to La1/Lo1).
        let rho1 = a * f / cone_t(self.la1.to_radians()).powf(n);
        let theta1 = n * (self.lo1.to_radians() - lo_v);
        let x1 = rho1 * theta1.sin();
        let y1 = -rho1 * theta1.cos();

        let x = x1 + self.signed_i(i) * self.dx;
        let y = y1 + self.signed_j(j) * self.dy;

        let rho = n.signum() * (x * x + y * y).sqrt();
        if rho.abs() < 1e-12 {
            // At the projection pole longitude is undefined; return LoV.
            let lat = if n > 0.0 { 90.0 } else { -90.0 };
            return Ok((lat, self.lov));
        }
        let theta = x.atan2(-y);
        let lat = 2.0 * ((a * f / rho).powf(1.0 / n)).atan() - std::f64::consts::FRAC_PI_2;
        let lon = lo_v + theta / n;
        Ok((lat.to_degrees(), normalize_longitude(lon.to_degrees())))
    }

    fn signed_i(&self, i: u32) -> f64 {
        if self.scan_mode.i_positive {
            i as f64
        } else {
            -(i as f64)
        }
    }

    fn signed_j(&self, j: u32) -> f64 {
        // In projected grids the +j scan direction increases y (northward).
        if self.scan_mode.j_positive {
            j as f64
        } else {
            -(j as f64)
        }
    }
}

/// `tan(pi/4 + phi/2)`, the recurring Lambert/Mercator conformal factor.
#[inline]
fn cone_t(phi: f64) -> f64 {
    (std::f64::consts::FRAC_PI_4 + phi / 2.0).tan()
}

/// Normalizes a longitude in degrees to the `[-180, 180]` range.
#[inline]
fn normalize_longitude(mut lon: f64) -> f64 {
    while lon > 180.0 {
        lon -= 360.0;
    }
    while lon < -180.0 {
        lon += 360.0;
    }
    lon
}

/// Mercator projection grid
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MercatorGrid {
    /// Number of points along X-axis
    pub ni: u32,
    /// Number of points along Y-axis
    pub nj: u32,
    /// Latitude of first grid point (degrees)
    pub la1: f64,
    /// Longitude of first grid point (degrees)
    pub lo1: f64,
    /// Latitude of last grid point (degrees)
    pub la2: f64,
    /// Longitude of last grid point (degrees)
    pub lo2: f64,
    /// Latitude at which the Mercator projection intersects the Earth
    pub latin: f64,
    /// X-direction grid length (m)
    pub di: f64,
    /// Y-direction grid length (m)
    pub dj: f64,
    /// Radius of the (spherical) Earth in metres, used by the projection math.
    pub earth_radius_m: f64,
    /// Scanning mode
    pub scan_mode: ScanMode,
}

impl MercatorGrid {
    /// Get total number of grid points
    pub fn num_points(&self) -> usize {
        (self.ni as usize) * (self.nj as usize)
    }

    /// Returns the `(latitude, longitude)` in degrees of grid point `(i, j)`.
    ///
    /// Implements the spherical Mercator inverse projection with true-scale
    /// latitude `latin` (WMO GDT 3.10). Longitude is uniform in `i`; latitude
    /// is recovered from the projected `y` coordinate. The first grid point
    /// `(0, 0)` maps back to `(la1, lo1)`.
    pub fn coordinates(&self, i: u32, j: u32) -> Result<(f64, f64)> {
        if i >= self.ni || j >= self.nj {
            return Err(GribError::OutOfRange(format!(
                "index ({i}, {j}) out of range ({}x{})",
                self.ni, self.nj
            )));
        }
        let a = self.earth_radius_m;
        let cos_latin = self.latin.to_radians().cos();
        if cos_latin.abs() < 1e-12 {
            return Err(GribError::CoordinateError(
                "Mercator: true-scale latitude too close to a pole".to_string(),
            ));
        }
        let scale = a * cos_latin;

        // Projected coordinates of the first grid point.
        let y1 = scale * cone_t(self.la1.to_radians()).ln();
        let lon1 = self.lo1.to_radians();

        let di_signed = if self.scan_mode.i_positive {
            i as f64
        } else {
            -(i as f64)
        };
        let dj_signed = if self.scan_mode.j_positive {
            j as f64
        } else {
            -(j as f64)
        };

        let lon = lon1 + di_signed * self.di / scale;
        let y = y1 + dj_signed * self.dj;
        let lat = 2.0 * (y / scale).exp().atan() - std::f64::consts::FRAC_PI_2;
        Ok((lat.to_degrees(), normalize_longitude(lon.to_degrees())))
    }
}

/// Polar Stereographic projection grid
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolarStereographicGrid {
    /// Number of points along X-axis
    pub nx: u32,
    /// Number of points along Y-axis
    pub ny: u32,
    /// Latitude of first grid point (degrees)
    pub la1: f64,
    /// Longitude of first grid point (degrees)
    pub lo1: f64,
    /// Orientation of the grid (longitude where dx and dy are specified)
    pub lov: f64,
    /// X-direction grid length (m)
    pub dx: f64,
    /// Y-direction grid length (m)
    pub dy: f64,
    /// Latitude at which the grid lengths dx/dy are specified (true-scale
    /// latitude, degrees). GRIB2 historically uses 60° for the standard grids.
    pub lad: f64,
    /// Projection center flag (0 = North Pole, 1 = South Pole)
    pub projection_center: u8,
    /// Radius of the (spherical) Earth in metres, used by the projection math.
    pub earth_radius_m: f64,
    /// Scanning mode
    pub scan_mode: ScanMode,
}

impl PolarStereographicGrid {
    /// Get total number of grid points
    pub fn num_points(&self) -> usize {
        (self.nx as usize) * (self.ny as usize)
    }

    /// Check if projection is centered on North Pole
    pub fn is_north_pole(&self) -> bool {
        self.projection_center & 0x80 == 0
    }

    /// Returns the `(latitude, longitude)` in degrees of grid point `(i, j)`.
    ///
    /// Implements the spherical polar-stereographic inverse projection with
    /// true-scale latitude `lad` (WMO GDT 3.20). The first grid point `(0, 0)`
    /// maps back to `(la1, lo1)`.
    pub fn coordinates(&self, i: u32, j: u32) -> Result<(f64, f64)> {
        if i >= self.nx || j >= self.ny {
            return Err(GribError::OutOfRange(format!(
                "index ({i}, {j}) out of range ({}x{})",
                self.nx, self.ny
            )));
        }
        let a = self.earth_radius_m;
        let north = self.is_north_pole();
        // Hemisphere sign: +1 for north, -1 for south.
        let hemi = if north { 1.0 } else { -1.0 };
        // Scale factor at the pole for a true-scale latitude `lad`.
        let phi_c = self.lad.to_radians().abs();
        let k0 = (1.0 + phi_c.sin()) / 2.0;
        let lov = self.lov.to_radians();

        // rho as a function of latitude (measured from the projection pole).
        let rho_of_lat = |lat_deg: f64| -> f64 {
            let lat = lat_deg.to_radians();
            // Colatitude from the projection pole.
            let t = (std::f64::consts::FRAC_PI_4 - hemi * lat / 2.0).tan();
            2.0 * a * k0 * t
        };

        // Projected coordinates of the first grid point.
        let rho1 = rho_of_lat(self.la1);
        let ang1 = self.lo1.to_radians() - lov;
        let (x1, y1) = if north {
            (rho1 * ang1.sin(), -rho1 * ang1.cos())
        } else {
            (rho1 * ang1.sin(), rho1 * ang1.cos())
        };

        let di_signed = if self.scan_mode.i_positive {
            i as f64
        } else {
            -(i as f64)
        };
        let dj_signed = if self.scan_mode.j_positive {
            j as f64
        } else {
            -(j as f64)
        };
        let x = x1 + di_signed * self.dx;
        let y = y1 + dj_signed * self.dy;

        let rho = (x * x + y * y).sqrt();
        if rho < 1e-9 {
            let lat = if north { 90.0 } else { -90.0 };
            return Ok((lat, self.lov));
        }
        let c = 2.0 * (rho / (2.0 * a * k0)).atan();
        let lat = hemi * (std::f64::consts::FRAC_PI_2 - c);
        let lon = if north {
            lov + x.atan2(-y)
        } else {
            lov + x.atan2(y)
        };
        Ok((lat.to_degrees(), normalize_longitude(lon.to_degrees())))
    }
}

/// Gaussian latitude/longitude grid
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GaussianGrid {
    /// Number of points along a parallel
    pub ni: u32,
    /// Number of points along a meridian
    pub nj: u32,
    /// Latitude of first grid point (degrees)
    pub la1: f64,
    /// Longitude of first grid point (degrees)
    pub lo1: f64,
    /// Latitude of last grid point (degrees)
    pub la2: f64,
    /// Longitude of last grid point (degrees)
    pub lo2: f64,
    /// i direction increment (degrees)
    pub di: f64,
    /// Number of latitude circles between pole and equator
    pub n: u32,
    /// Scanning mode
    pub scan_mode: ScanMode,
}

impl GaussianGrid {
    /// Get total number of grid points
    pub fn num_points(&self) -> usize {
        (self.ni as usize) * (self.nj as usize)
    }

    /// Longitude (degrees) of column `i`, normalized to `[-180, 180]`.
    pub fn longitude(&self, i: u32) -> Result<f64> {
        if i >= self.ni {
            return Err(GribError::OutOfRange(format!(
                "i index {i} out of range [0, {})",
                self.ni
            )));
        }
        let lon = if self.scan_mode.i_positive {
            self.lo1 + (i as f64) * self.di
        } else {
            self.lo1 - (i as f64) * self.di
        };
        Ok(normalize_longitude(lon))
    }

    /// Latitude (degrees) of row `j`.
    ///
    /// The row latitudes are the Gaussian latitudes: the arcsines of the roots
    /// of the Legendre polynomial `P_{2N}` (a global Gaussian grid has
    /// `nj = 2N` rows). They are ordered north-to-south when the grid scans
    /// with `j_positive == false` (the GRIB default), matching `la1` at the
    /// northern edge.
    pub fn latitude(&self, j: u32) -> Result<f64> {
        if j >= self.nj {
            return Err(GribError::OutOfRange(format!(
                "j index {j} out of range [0, {})",
                self.nj
            )));
        }
        let lats = gaussian_latitudes(self.nj as usize);
        // `gaussian_latitudes` returns north-to-south (descending). If the grid
        // scans south-to-north, reverse the row order.
        let idx = if self.scan_mode.j_positive {
            self.nj as usize - 1 - j as usize
        } else {
            j as usize
        };
        lats.get(idx)
            .copied()
            .ok_or_else(|| GribError::OutOfRange(format!("gaussian latitude row {j} unavailable")))
    }

    /// Returns the `(latitude, longitude)` in degrees of grid point `(i, j)`.
    pub fn coordinates(&self, i: u32, j: u32) -> Result<(f64, f64)> {
        Ok((self.latitude(j)?, self.longitude(i)?))
    }
}

/// Computes the `n` Gaussian latitudes (in degrees) ordered north-to-south.
///
/// The Gaussian latitudes are `asin(x_k)` where `x_k` are the roots of the
/// Legendre polynomial `P_n`. Each root is found by Newton-Raphson from the
/// standard initial guess `cos(pi*(k+0.75)/(n+0.5))`, which converges
/// quadratically and to full `f64` precision in a handful of iterations.
fn gaussian_latitudes(n: usize) -> Vec<f64> {
    if n == 0 {
        return Vec::new();
    }
    let mut roots = Vec::with_capacity(n);
    // Roots are symmetric; compute the positive half and mirror.
    let half = n.div_ceil(2);
    for k in 0..half {
        // Initial guess for the (k+1)-th root (descending in x).
        let mut x = (std::f64::consts::PI * (k as f64 + 0.75) / (n as f64 + 0.5)).cos();
        for _ in 0..100 {
            let (p, dp) = legendre_p_and_deriv(n, x);
            let dx = -p / dp;
            x += dx;
            if dx.abs() < 1e-15 {
                break;
            }
        }
        roots.push(x);
    }
    // `roots` holds the largest positive roots first (descending). Build the
    // full descending list by mirroring: [+roots..., (0 if odd), -roots...].
    let mut all = Vec::with_capacity(n);
    for &r in &roots {
        all.push(r);
    }
    // Mirror negatives (skip the central root for odd n, already included).
    let start = if n % 2 == 1 { half - 1 } else { half };
    for k in (0..start).rev() {
        all.push(-roots[k]);
    }
    all.truncate(n);
    all.into_iter().map(|x| x.asin().to_degrees()).collect()
}

/// Evaluates the Legendre polynomial `P_n(x)` and its derivative `P_n'(x)` via
/// the standard three-term recurrence.
fn legendre_p_and_deriv(n: usize, x: f64) -> (f64, f64) {
    // P_0 = 1, P_1 = x.
    let mut p_prev = 1.0f64;
    let mut p_curr = x;
    if n == 0 {
        return (1.0, 0.0);
    }
    if n == 1 {
        return (x, 1.0);
    }
    for k in 2..=n {
        let kf = k as f64;
        let p_next = ((2.0 * kf - 1.0) * x * p_curr - (kf - 1.0) * p_prev) / kf;
        p_prev = p_curr;
        p_curr = p_next;
    }
    // Derivative: P_n'(x) = n (x P_n - P_{n-1}) / (x^2 - 1).
    let denom = x * x - 1.0;
    let deriv = if denom.abs() < 1e-14 {
        0.0
    } else {
        n as f64 * (x * p_curr - p_prev) / denom
    };
    (p_curr, deriv)
}

/// Space view perspective or orthographic grid
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpaceViewGrid {
    /// Number of points along X-axis
    pub nx: u32,
    /// Number of points along Y-axis
    pub ny: u32,
    /// Latitude of sub-satellite point (degrees)
    pub lap: f64,
    /// Longitude of sub-satellite point (degrees)
    pub lop: f64,
    /// X-direction grid length (m)
    pub dx: f64,
    /// Y-direction grid length (m)
    pub dy: f64,
    /// Altitude of camera from Earth's center (m)
    pub altitude: f64,
    /// Scanning mode
    pub scan_mode: ScanMode,
}

impl SpaceViewGrid {
    /// Get total number of grid points
    pub fn num_points(&self) -> usize {
        (self.nx as usize) * (self.ny as usize)
    }
}

/// Scanning mode flags
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanMode {
    /// Points scan in +i direction (true) or -i direction (false)
    pub i_positive: bool,
    /// Points scan in +j direction (true) or -j direction (false)
    pub j_positive: bool,
    /// Adjacent points in i direction are consecutive (true) or j direction (false)
    pub consecutive_i: bool,
}

impl ScanMode {
    /// Parse scanning mode from GRIB flags byte
    pub fn from_flags(flags: u8) -> Self {
        Self {
            i_positive: (flags & 0b1000_0000) == 0,
            j_positive: (flags & 0b0100_0000) != 0,
            consecutive_i: (flags & 0b0010_0000) == 0,
        }
    }

    /// Convert to GRIB flags byte
    pub fn to_flags(&self) -> u8 {
        let mut flags = 0u8;
        if !self.i_positive {
            flags |= 0b1000_0000;
        }
        if self.j_positive {
            flags |= 0b0100_0000;
        }
        if !self.consecutive_i {
            flags |= 0b0010_0000;
        }
        flags
    }
}

impl Default for ScanMode {
    fn default() -> Self {
        Self {
            i_positive: true,
            j_positive: false,
            consecutive_i: true,
        }
    }
}

impl GridDefinition {
    /// Get the total number of grid points
    pub fn num_points(&self) -> usize {
        match self {
            Self::LatLon(g) => g.num_points(),
            Self::RotatedLatLon(g) => g.base.num_points(),
            Self::LambertConformal(g) => g.num_points(),
            Self::Mercator(g) => g.num_points(),
            Self::PolarStereographic(g) => g.num_points(),
            Self::Gaussian(g) => g.num_points(),
            Self::SpaceView(g) => g.num_points(),
        }
    }

    /// Get grid dimensions (ni/nx, nj/ny)
    pub fn dimensions(&self) -> (u32, u32) {
        match self {
            Self::LatLon(g) => (g.ni, g.nj),
            Self::RotatedLatLon(g) => (g.base.ni, g.base.nj),
            Self::LambertConformal(g) => (g.nx, g.ny),
            Self::Mercator(g) => (g.ni, g.nj),
            Self::PolarStereographic(g) => (g.nx, g.ny),
            Self::Gaussian(g) => (g.ni, g.nj),
            Self::SpaceView(g) => (g.nx, g.ny),
        }
    }

    /// Get grid type name
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::LatLon(_) => "Regular Lat/Lon",
            Self::RotatedLatLon(_) => "Rotated Lat/Lon",
            Self::LambertConformal(_) => "Lambert Conformal",
            Self::Mercator(_) => "Mercator",
            Self::PolarStereographic(_) => "Polar Stereographic",
            Self::Gaussian(_) => "Gaussian",
            Self::SpaceView(_) => "Space View",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_latlon_grid() {
        let grid = LatLonGrid {
            ni: 360,
            nj: 181,
            la1: 90.0,
            lo1: 0.0,
            la2: -90.0,
            lo2: 359.0,
            di: 1.0,
            dj: 1.0,
            scan_mode: ScanMode {
                i_positive: true,
                j_positive: false,
                consecutive_i: true,
            },
        };

        assert_eq!(grid.num_points(), 360 * 181);

        let lat = grid.latitude(0).expect("latitude failed");
        assert!((lat - 90.0).abs() < 1e-6);

        let lon = grid.longitude(0).expect("longitude failed");
        assert!((lon - 0.0).abs() < 1e-6);

        let (lat, lon) = grid.coordinates(180, 90).expect("coordinates failed");
        assert!((lat - 0.0).abs() < 1.1); // ~0 degrees latitude
        assert!((lon - 180.0).abs() < 1.1); // ~180 degrees longitude
    }

    #[test]
    fn test_scan_mode() {
        let mode = ScanMode::from_flags(0b0100_0000);
        assert!(mode.i_positive);
        assert!(mode.j_positive);
        assert!(mode.consecutive_i);

        let flags = mode.to_flags();
        assert_eq!(flags, 0b0100_0000);
    }

    #[test]
    fn test_grid_dimensions() {
        let grid = GridDefinition::LatLon(LatLonGrid {
            ni: 720,
            nj: 361,
            la1: 90.0,
            lo1: 0.0,
            la2: -90.0,
            lo2: 359.5,
            di: 0.5,
            dj: 0.5,
            scan_mode: ScanMode::default(),
        });

        assert_eq!(grid.dimensions(), (720, 361));
        assert_eq!(grid.num_points(), 720 * 361);
        assert_eq!(grid.type_name(), "Regular Lat/Lon");
    }
}
