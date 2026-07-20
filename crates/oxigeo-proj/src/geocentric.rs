//! Geocentric (Earth-Centred, Earth-Fixed) coordinate reference system support.
//!
//! This module provides first-class ECEF (Earth-Centred, Earth-Fixed) types and
//! conversions for the OxiGeo projection library.  All formulas follow published
//! geodetic standards (Bowring 1985, Heiskanen & Moritz 1967).
//!
//! # Key types
//!
//! - [`GeocentricEllipsoid`] — reference ellipsoid parameters and derived quantities.
//! - [`EcefCoordinate`] — an ECEF (X, Y, Z) point in metres.
//! - [`GeocentricCrs`] — a named geocentric CRS tied to an ellipsoid.
//! - [`EcefTransformer`] — transforms coordinates between two geocentric CRS,
//!   optionally applying a Helmert 7-parameter datum shift.
//!
//! # Free-function round-trip
//!
//! ```rust
//! use oxigeo_proj::geocentric::{GeocentricEllipsoid, geographic_to_ecef, ecef_to_geographic};
//!
//! let ell = GeocentricEllipsoid::wgs84();
//! let ecef = geographic_to_ecef(&ell, 0.0, 0.0, 0.0);
//! let (lon, lat, h) = ecef_to_geographic(&ell, &ecef);
//! assert!((lon - 0.0).abs() < 1e-9);
//! assert!((lat - 0.0).abs() < 1e-9);
//! assert!(h.abs() < 1e-6);
//! ```

use crate::grid_shift::Helmert7Params;

// ─────────────────────────────────────────────────────────────────────────────
// GeocentricEllipsoid
// ─────────────────────────────────────────────────────────────────────────────

/// Reference ellipsoid defined by its semi-major axis `a` and flattening `f`.
///
/// All quantities are in SI units (metres for lengths, dimensionless for
/// flattening).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeocentricEllipsoid {
    /// Semi-major (equatorial) axis in metres.
    pub semimajor_a: f64,
    /// Flattening: `f = (a − b) / a`.
    pub flattening_f: f64,
}

impl GeocentricEllipsoid {
    /// First eccentricity squared: `e² = f · (2 − f)`.
    ///
    /// Used extensively in geographic ↔ geocentric conversions.
    #[inline]
    pub fn eccentricity_squared(&self) -> f64 {
        self.flattening_f * (2.0 - self.flattening_f)
    }

    /// Semi-minor (polar) axis in metres: `b = a · (1 − f)`.
    #[inline]
    pub fn semiminor_b(&self) -> f64 {
        self.semimajor_a * (1.0 - self.flattening_f)
    }

    /// WGS 84 ellipsoid (EPSG:7030).
    ///
    /// `a = 6 378 137.0 m`, `f = 1 / 298.257 223 563`.
    pub fn wgs84() -> Self {
        Self {
            semimajor_a: 6_378_137.0,
            flattening_f: 1.0 / 298.257_223_563,
        }
    }

    /// GRS 80 ellipsoid (EPSG:7019).
    ///
    /// `a = 6 378 137.0 m`, `f = 1 / 298.257 222 101`.
    pub fn grs80() -> Self {
        Self {
            semimajor_a: 6_378_137.0,
            flattening_f: 1.0 / 298.257_222_101,
        }
    }

    /// Airy 1830 ellipsoid (EPSG:7001), used by OSGB36.
    ///
    /// `a = 6 377 563.396 m`, `f = 1 / 299.324 9646`.
    pub fn airy1830() -> Self {
        Self {
            semimajor_a: 6_377_563.396,
            flattening_f: 1.0 / 299.324_964_6,
        }
    }

    /// Bessel 1841 ellipsoid (EPSG:7004), used by DHDN / RD New.
    ///
    /// `a = 6 377 397.155 m`, `f = 1 / 299.152 8128`.
    pub fn bessel1841() -> Self {
        Self {
            semimajor_a: 6_377_397.155,
            flattening_f: 1.0 / 299.152_812_8,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// EcefCoordinate
// ─────────────────────────────────────────────────────────────────────────────

/// An Earth-Centred, Earth-Fixed (ECEF) coordinate triple `(X, Y, Z)` in metres.
///
/// The origin is at the Earth's centre of mass.  The Z axis points toward the
/// conventional terrestrial North Pole, and the X axis lies in the Greenwich
/// meridian plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EcefCoordinate {
    /// X component in metres (towards intersection of equator and prime meridian).
    pub x: f64,
    /// Y component in metres (towards intersection of equator and 90°E meridian).
    pub y: f64,
    /// Z component in metres (towards North Pole).
    pub z: f64,
}

impl EcefCoordinate {
    /// Construct a new ECEF coordinate from its three Cartesian components.
    #[inline]
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// Euclidean magnitude (distance from the Earth's centre) in metres.
    #[inline]
    pub fn magnitude(&self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    /// Vector difference `self − other`, returning a new [`EcefCoordinate`].
    #[inline]
    pub fn subtract(&self, other: &EcefCoordinate) -> EcefCoordinate {
        EcefCoordinate {
            x: self.x - other.x,
            y: self.y - other.y,
            z: self.z - other.z,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// GeocentricCrs
// ─────────────────────────────────────────────────────────────────────────────

/// A named geocentric (ECEF) Coordinate Reference System.
///
/// Associates a human-readable name and optional EPSG code with the underlying
/// reference ellipsoid.
#[derive(Debug, Clone)]
pub struct GeocentricCrs {
    /// Human-readable name of the CRS (e.g. `"WGS 84"`).
    pub name: String,
    /// EPSG code, if applicable.
    pub epsg_code: Option<u32>,
    /// Reference ellipsoid.
    pub ellipsoid: GeocentricEllipsoid,
}

impl GeocentricCrs {
    /// WGS 84 geocentric CRS (EPSG:4978).
    pub fn wgs84() -> Self {
        Self {
            name: String::from("WGS 84"),
            epsg_code: Some(4978),
            ellipsoid: GeocentricEllipsoid::wgs84(),
        }
    }

    /// ETRS89 geocentric CRS (EPSG:4936), using the GRS80 ellipsoid.
    pub fn etrs89() -> Self {
        Self {
            name: String::from("ETRS89"),
            epsg_code: Some(4936),
            ellipsoid: GeocentricEllipsoid::grs80(),
        }
    }

    /// NAD83 geocentric CRS (EPSG:4978 realization), using the GRS80 ellipsoid.
    ///
    /// Note: NAD83 and WGS84 share the same EPSG geocentric code at the metre
    /// level; the EPSG code stored here follows the specification.
    pub fn nad83() -> Self {
        Self {
            name: String::from("NAD83"),
            epsg_code: Some(4978),
            ellipsoid: GeocentricEllipsoid::grs80(),
        }
    }

    /// Construct a custom geocentric CRS with a user-supplied name and ellipsoid.
    ///
    /// The EPSG code will be `None`.
    pub fn custom<S: Into<String>>(name: S, ellipsoid: GeocentricEllipsoid) -> Self {
        Self {
            name: name.into(),
            epsg_code: None,
            ellipsoid,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// geographic_to_ecef
// ─────────────────────────────────────────────────────────────────────────────

/// Convert geodetic geographic coordinates to ECEF Cartesian coordinates.
///
/// # Parameters
/// - `ellipsoid` — reference ellipsoid
/// - `lon_deg` — geodetic longitude in decimal degrees (−180 … +180)
/// - `lat_deg` — geodetic latitude in decimal degrees (−90 … +90)
/// - `h_m` — ellipsoidal height above the reference surface, in metres
///
/// # Returns
/// An [`EcefCoordinate`] `(X, Y, Z)` in metres.
///
/// # Algorithm
/// Standard closed-form using the prime vertical radius of curvature `N`:
/// ```text
/// e² = f · (2 − f)
/// φ  = lat_deg.to_radians();   λ = lon_deg.to_radians()
/// N  = a / √(1 − e² · sin²φ)
/// X  = (N + h) · cos φ · cos λ
/// Y  = (N + h) · cos φ · sin λ
/// Z  = (N · (1 − e²) + h) · sin φ
/// ```
pub fn geographic_to_ecef(
    ellipsoid: &GeocentricEllipsoid,
    lon_deg: f64,
    lat_deg: f64,
    h_m: f64,
) -> EcefCoordinate {
    let a = ellipsoid.semimajor_a;
    let e2 = ellipsoid.eccentricity_squared();

    let phi = lat_deg.to_radians();
    let lambda = lon_deg.to_radians();

    let sin_phi = phi.sin();
    let cos_phi = phi.cos();
    let sin_lambda = lambda.sin();
    let cos_lambda = lambda.cos();

    // Prime vertical radius of curvature
    let n = a / (1.0 - e2 * sin_phi * sin_phi).sqrt();

    let x = (n + h_m) * cos_phi * cos_lambda;
    let y = (n + h_m) * cos_phi * sin_lambda;
    let z = (n * (1.0 - e2) + h_m) * sin_phi;

    EcefCoordinate { x, y, z }
}

// ─────────────────────────────────────────────────────────────────────────────
// ecef_to_geographic  (Bowring 1985 closed-form)
// ─────────────────────────────────────────────────────────────────────────────

/// Convert ECEF Cartesian coordinates to geodetic geographic coordinates.
///
/// Uses the Bowring (1985) parametric-latitude closed-form, which is
/// essentially one-shot and gives sub-millimetre accuracy for typical
/// Earth-surface points.  For points very close to the Z-axis (polar caps,
/// |λ| > 89.9°), consider [`ecef_to_geographic_iterative`] instead.
///
/// # Returns
/// `(lon_deg, lat_deg, h_m)` — longitude, latitude (decimal degrees) and
/// ellipsoidal height (metres).
///
/// # Algorithm  (Bowring 1985)
/// ```text
/// p   = √(X² + Y²)
/// λ   = atan2(Y, X)
/// b   = a · (1 − f)
/// ep² = (a² − b²) / b²
/// θ   = atan2(Z · a, p · b)
/// φ   = atan2(Z + ep² · b · sin³θ, p − e² · a · cos³θ)
/// N   = a / √(1 − e² · sin²φ)
/// h   = p / cos φ − N       (if |φ| < π/4, else use Z formula)
/// ```
pub fn ecef_to_geographic(
    ellipsoid: &GeocentricEllipsoid,
    ecef: &EcefCoordinate,
) -> (f64, f64, f64) {
    let a = ellipsoid.semimajor_a;
    let f = ellipsoid.flattening_f;
    let e2 = ellipsoid.eccentricity_squared();
    let b = ellipsoid.semiminor_b();

    let x = ecef.x;
    let y = ecef.y;
    let z = ecef.z;

    // Longitude is straightforward.
    let lambda = y.atan2(x);

    // Distance from Z-axis (equatorial plane distance).
    let p = (x * x + y * y).sqrt();

    // Second eccentricity squared: e'² = (a² − b²) / b²
    let ep2 = (a * a - b * b) / (b * b);

    // Parametric latitude θ (Bowring's auxiliary angle).
    let theta = (z * a).atan2(p * b);

    let sin_theta = theta.sin();
    let cos_theta = theta.cos();

    // Geodetic latitude φ via Bowring's formula.
    let phi_numer = z + ep2 * b * sin_theta * sin_theta * sin_theta;
    let phi_denom = p - e2 * a * cos_theta * cos_theta * cos_theta;
    let phi = phi_numer.atan2(phi_denom);

    let sin_phi = phi.sin();
    let cos_phi = phi.cos();

    // Prime vertical radius of curvature at φ.
    let n = a / (1.0 - e2 * sin_phi * sin_phi).sqrt();

    // Ellipsoidal height — use the formula that avoids division by zero.
    let h = if cos_phi.abs() > f {
        // Away from the poles: h = p / cos φ − N
        p / cos_phi - n
    } else {
        // Near the poles: h = Z / sin φ − N(1 − e²)
        z / sin_phi - n * (1.0 - e2)
    };

    (lambda.to_degrees(), phi.to_degrees(), h)
}

// ─────────────────────────────────────────────────────────────────────────────
// ecef_to_geographic_iterative  (Heiskanen-Moritz)
// ─────────────────────────────────────────────────────────────────────────────

/// Convert ECEF coordinates to geodetic geographic coordinates using the
/// Heiskanen-Moritz iterative method.
///
/// Iterates until convergence (|Δφ| < 1 × 10⁻¹²) or 10 iterations.
///
/// # Returns
/// `(lon_deg, lat_deg, h_m)`.
pub fn ecef_to_geographic_iterative(
    ellipsoid: &GeocentricEllipsoid,
    ecef: &EcefCoordinate,
) -> (f64, f64, f64) {
    let a = ellipsoid.semimajor_a;
    let e2 = ellipsoid.eccentricity_squared();

    let x = ecef.x;
    let y = ecef.y;
    let z = ecef.z;

    let lambda = y.atan2(x);
    let p = (x * x + y * y).sqrt();

    // Initial latitude estimate (Heiskanen-Moritz).
    let mut phi = (z / (p * (1.0 - e2))).atan();

    for _ in 0..10 {
        let sin_phi = phi.sin();
        let n = a / (1.0 - e2 * sin_phi * sin_phi).sqrt();
        let phi_new = (z + e2 * n * sin_phi).atan2(p);
        let delta = (phi_new - phi).abs();
        phi = phi_new;
        if delta < 1e-12 {
            break;
        }
    }

    let sin_phi = phi.sin();
    let cos_phi = phi.cos();
    let n = a / (1.0 - e2 * sin_phi * sin_phi).sqrt();

    let h = if cos_phi.abs() > 1e-10 {
        p / cos_phi - n
    } else {
        z / sin_phi - n * (1.0 - e2)
    };

    (lambda.to_degrees(), phi.to_degrees(), h)
}

// ─────────────────────────────────────────────────────────────────────────────
// EcefTransformer
// ─────────────────────────────────────────────────────────────────────────────

/// Transforms ECEF coordinates from one geocentric CRS to another.
///
/// If the source and destination share the same ellipsoid, and no Helmert
/// parameters are provided, the transform is effectively an identity.
///
/// Optionally applies a [`Helmert7Params`] datum shift between the two
/// geocentric frames.
#[derive(Debug, Clone)]
pub struct EcefTransformer {
    /// Source geocentric CRS.
    pub src: GeocentricCrs,
    /// Destination geocentric CRS.
    pub dst: GeocentricCrs,
    /// Optional Helmert 7-parameter datum shift from `src` to `dst`.
    pub helmert: Option<Helmert7Params>,
}

impl EcefTransformer {
    /// Create a new transformer between two geocentric CRS without a datum shift.
    pub fn new(src: GeocentricCrs, dst: GeocentricCrs) -> Self {
        Self {
            src,
            dst,
            helmert: None,
        }
    }

    /// Attach a Helmert 7-parameter datum shift to this transformer (builder style).
    pub fn with_helmert(mut self, params: Helmert7Params) -> Self {
        self.helmert = Some(params);
        self
    }

    /// Transform a single ECEF coordinate from `src` to `dst`.
    ///
    /// If a Helmert transformation is configured, it is applied to the coordinate
    /// before returning.  Otherwise the coordinate is returned unchanged.
    pub fn transform(&self, coord: &EcefCoordinate) -> EcefCoordinate {
        match &self.helmert {
            Some(h) => {
                let (x2, y2, z2) = h.apply(coord.x, coord.y, coord.z);
                EcefCoordinate::new(x2, y2, z2)
            }
            None => *coord,
        }
    }

    /// Transform a batch of ECEF coordinates.
    ///
    /// Each coordinate is transformed independently via [`EcefTransformer::transform`].
    pub fn transform_batch(&self, coords: &[EcefCoordinate]) -> Vec<EcefCoordinate> {
        coords.iter().map(|c| self.transform(c)).collect()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// geographic_to_geographic_via_ecef
// ─────────────────────────────────────────────────────────────────────────────

/// Convert geographic coordinates from one ellipsoid to another via ECEF.
///
/// This is the classic three-step datum transformation:
/// 1. Project from `src_ell` geographic → ECEF.
/// 2. Optionally apply the Helmert datum shift.
/// 3. Convert the shifted ECEF → `dst_ell` geographic.
///
/// # Parameters
/// - `src_ell` — source reference ellipsoid
/// - `dst_ell` — destination reference ellipsoid
/// - `helmert` — optional Helmert 7-parameter shift from `src` to `dst`
/// - `lon_deg`, `lat_deg` — geodetic coordinates on `src_ell` (decimal degrees)
/// - `h_m` — ellipsoidal height above `src_ell` surface (metres)
///
/// # Returns
/// `(lon_deg, lat_deg, h_m)` expressed on `dst_ell`.
pub fn geographic_to_geographic_via_ecef(
    src_ell: &GeocentricEllipsoid,
    dst_ell: &GeocentricEllipsoid,
    helmert: Option<&Helmert7Params>,
    lon_deg: f64,
    lat_deg: f64,
    h_m: f64,
) -> (f64, f64, f64) {
    // Step 1: geographic → ECEF on the source ellipsoid.
    let ecef_src = geographic_to_ecef(src_ell, lon_deg, lat_deg, h_m);

    // Step 2: apply datum shift (if any).
    let ecef_dst = match helmert {
        Some(h) => {
            let (x2, y2, z2) = h.apply(ecef_src.x, ecef_src.y, ecef_src.z);
            EcefCoordinate::new(x2, y2, z2)
        }
        None => ecef_src,
    };

    // Step 3: ECEF → geographic on the destination ellipsoid.
    ecef_to_geographic(dst_ell, &ecef_dst)
}
