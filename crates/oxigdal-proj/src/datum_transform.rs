//! Geodetic datum transformation algorithms.
//!
//! This module provides rigorous geodetic datum transformation algorithms including:
//!
//! - Reference ellipsoid definitions and derived parameters
//! - Geographic ↔ ECEF (Earth-Centred, Earth-Fixed) coordinate conversions
//! - Abridged and full Molodensky transformations
//! - Bursa-Wolf / Helmert 7-parameter similarity transformations
//! - ITRF (International Terrestrial Reference Frame) epoch-aware transformations
//! - Unified `DatumTransformer` API
//!
//! # References
//!
//! - EPSG Geodesy Guidance Note 7 Part 2: Coordinate Conversions and Transformations
//! - Bowring, B.R. (1985). The Geodesic Line and Short Geodesics on the Ellipsoid
//! - IERS Conventions (2010), IERS Technical Note 36
//! - OGP/EPSG Transformation Parameters

use core::f64::consts::PI;

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

// Arc-second to radian conversion constant
const ARCSEC_TO_RAD: f64 = PI / (180.0 * 3600.0);

// ─────────────────────────────────────────────────────────────────────────────
// Ellipsoid
// ─────────────────────────────────────────────────────────────────────────────

/// A reference ellipsoid defined by its semi-major axis and inverse flattening.
///
/// The ellipsoid is the fundamental surface from which geodetic coordinates are
/// measured. Different national and international systems use different ellipsoids.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ellipsoid {
    /// Ellipsoid name (informational)
    pub name: &'static str,
    /// Semi-major axis (equatorial radius) in metres
    pub a: f64,
    /// Inverse flattening `1/f`.  Use `0.0` to indicate a sphere.
    pub inv_f: f64,
}

impl Ellipsoid {
    /// Create a new ellipsoid with the given semi-major axis and inverse flattening.
    ///
    /// Pass `inv_f = 0.0` to describe a perfect sphere.
    #[must_use]
    pub const fn new(a: f64, inv_f: f64) -> Self {
        Self {
            name: "custom",
            a,
            inv_f,
        }
    }

    /// Flattening `f = 1 / inv_f`.  Returns `0.0` for a sphere.
    #[must_use]
    pub fn f(&self) -> f64 {
        if self.inv_f == 0.0 {
            0.0
        } else {
            1.0 / self.inv_f
        }
    }

    /// Semi-minor axis `b = a * (1 − f)` in metres.
    #[must_use]
    pub fn b(&self) -> f64 {
        self.a * (1.0 - self.f())
    }

    /// First eccentricity squared `e² = 2f − f²`.
    #[must_use]
    pub fn e2(&self) -> f64 {
        let f = self.f();
        2.0 * f - f * f
    }

    /// Second eccentricity squared `e'² = e² / (1 − e²)`.
    #[must_use]
    pub fn e_prime2(&self) -> f64 {
        let e2 = self.e2();
        e2 / (1.0 - e2)
    }

    /// Radius of curvature in the prime vertical `N(φ)`.
    ///
    /// `N(φ) = a / sqrt(1 − e² sin²φ)`
    ///
    /// # Parameters
    /// * `lat_rad` – geodetic latitude in radians
    #[must_use]
    pub fn n_radius(&self, lat_rad: f64) -> f64 {
        let sin_lat = lat_rad.sin();
        self.a / (1.0 - self.e2() * sin_lat * sin_lat).sqrt()
    }

    /// Radius of curvature in the meridian `M(φ)`.
    ///
    /// `M(φ) = a(1 − e²) / (1 − e² sin²φ)^(3/2)`
    ///
    /// # Parameters
    /// * `lat_rad` – geodetic latitude in radians
    #[must_use]
    pub fn m_radius(&self, lat_rad: f64) -> f64 {
        let sin_lat = lat_rad.sin();
        let w2 = 1.0 - self.e2() * sin_lat * sin_lat;
        self.a * (1.0 - self.e2()) / (w2 * w2.sqrt())
    }

    // ── Well-known ellipsoids ─────────────────────────────────────────────────

    /// WGS 84 — used by GPS and the global default
    pub const WGS84: Ellipsoid = Ellipsoid {
        name: "WGS84",
        a: 6_378_137.0,
        inv_f: 298.257_223_563,
    };

    /// GRS 80 — Geodetic Reference System 1980 (ETRS89, NAD83)
    pub const GRS80: Ellipsoid = Ellipsoid {
        name: "GRS80",
        a: 6_378_137.0,
        inv_f: 298.257_222_101,
    };

    /// International 1924 — used by ED50 (Europe)
    pub const INTERNATIONAL: Ellipsoid = Ellipsoid {
        name: "International 1924",
        a: 6_378_388.0,
        inv_f: 297.0,
    };

    /// Bessel 1841 — used by DHDN (Germany), Tokyo datum (Japan)
    pub const BESSEL: Ellipsoid = Ellipsoid {
        name: "Bessel 1841",
        a: 6_377_397.155,
        inv_f: 299.152_812_8,
    };

    /// Airy 1830 — used by OSGB36 (Great Britain)
    pub const AIRY: Ellipsoid = Ellipsoid {
        name: "Airy 1830",
        a: 6_377_563.396,
        inv_f: 299.324_964_6,
    };

    /// Clarke 1866 — used by NAD27 (North America)
    pub const CLARKE1866: Ellipsoid = Ellipsoid {
        name: "Clarke 1866",
        a: 6_378_206.4,
        inv_f: 294.978_698_2,
    };

    /// GDA94 ellipsoid (same as GRS80)
    pub const GDA94: Ellipsoid = Ellipsoid {
        name: "GRS80",
        a: 6_378_137.0,
        inv_f: 298.257_222_101,
    };
}

// ─────────────────────────────────────────────────────────────────────────────
// Geographic ↔ ECEF conversions
// ─────────────────────────────────────────────────────────────────────────────

/// Convert geodetic (geographic) coordinates to Earth-Centred Earth-Fixed (ECEF) Cartesian.
///
/// Uses the standard closed-form forward formula from WGS 84 specification.
///
/// # Parameters
/// * `lat` – geodetic latitude in **radians** (positive North)
/// * `lon` – geodetic longitude in **radians** (positive East)
/// * `h`   – ellipsoidal height in **metres** above the reference surface
///
/// # Returns
/// `(X, Y, Z)` in metres, ECEF frame.
#[must_use]
pub fn geodetic_to_ecef(lat: f64, lon: f64, h: f64, ellipsoid: &Ellipsoid) -> (f64, f64, f64) {
    let n = ellipsoid.n_radius(lat);
    let cos_lat = lat.cos();
    let sin_lat = lat.sin();
    let cos_lon = lon.cos();
    let sin_lon = lon.sin();

    let x = (n + h) * cos_lat * cos_lon;
    let y = (n + h) * cos_lat * sin_lon;
    let z = (n * (1.0 - ellipsoid.e2()) + h) * sin_lat;

    (x, y, z)
}

/// Convert ECEF Cartesian coordinates to geodetic (geographic) coordinates.
///
/// Uses Bowring's iterative method, which converges to better than 0.1 mm
/// precision in 3 iterations for all latitudes.
///
/// # Parameters
/// * `x`, `y`, `z` – ECEF coordinates in metres
///
/// # Returns
/// `(lat_rad, lon_rad, h_metres)` where lat/lon are in radians.
#[must_use]
pub fn ecef_to_geodetic(x: f64, y: f64, z: f64, ellipsoid: &Ellipsoid) -> (f64, f64, f64) {
    let a = ellipsoid.a;
    let e2 = ellipsoid.e2();
    let b = ellipsoid.b();

    let lon = y.atan2(x);

    // Bowring's method — start with a reduced-latitude seed
    let p = (x * x + y * y).sqrt();

    // Initial approximation using the parametric latitude
    let mut lat = (z / p * (1.0 - e2)).atan();

    // Iterate (3 iterations are sufficient for all practical purposes)
    for _ in 0..5 {
        let sin_lat = lat.sin();
        let n = a / (1.0 - e2 * sin_lat * sin_lat).sqrt();
        let tan_lat_new = (z + e2 * n * sin_lat) / p;
        let lat_new = tan_lat_new.atan();
        if (lat_new - lat).abs() < 1e-12 {
            lat = lat_new;
            break;
        }
        lat = lat_new;
    }

    // Height above ellipsoid
    let sin_lat = lat.sin();
    let cos_lat = lat.cos();
    let n = a / (1.0 - e2 * sin_lat * sin_lat).sqrt();

    let h = if cos_lat.abs() > 1e-10 {
        p / cos_lat - n
    } else {
        // Near the poles, use the Z component
        z.abs() / sin_lat.abs() - b
    };

    (lat, lon, h)
}

// ─────────────────────────────────────────────────────────────────────────────
// Molodensky transformation
// ─────────────────────────────────────────────────────────────────────────────

/// Parameters for the standard (full) and abridged Molodensky datum transformations.
///
/// The Molodensky transformation directly converts geographic (lat/lon/h) coordinates
/// between two ellipsoids using translation parameters and ellipsoid differences,
/// avoiding the intermediate ECEF conversion step.  It is less accurate than the
/// Bursa-Wolf 7-parameter method but is computationally cheaper and sufficient for
/// regional work at sub-metre accuracy.
///
/// # References
///
/// - Molodensky, M.S. et al. (1962), Methods for Study of the External Gravitational
///   Field and Figure of the Earth. Translated from Russian by the Israel Program for
///   Scientific Translations, Jerusalem (for US Department of Commerce).
/// - Deakin, R.E. (2004). The Standard and Abridged Molodensky Coordinate Transformation
///   Formulae. Department of Mathematical and Geospatial Sciences, RMIT University.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MolodenskyParams {
    /// X translation from source to target geocentre (metres)
    pub dx: f64,
    /// Y translation from source to target geocentre (metres)
    pub dy: f64,
    /// Z translation from source to target geocentre (metres)
    pub dz: f64,
    /// Semi-major axis difference: `target.a − source.a` (metres)
    pub da: f64,
    /// Flattening difference: `target.f − source.f` (dimensionless)
    pub df: f64,
}

impl MolodenskyParams {
    /// Construct Molodensky parameters from geocentric translation shifts and the
    /// source/target ellipsoids (used to compute `da` and `df` automatically).
    #[must_use]
    pub fn new(dx: f64, dy: f64, dz: f64, source: &Ellipsoid, target: &Ellipsoid) -> Self {
        Self {
            dx,
            dy,
            dz,
            da: target.a - source.a,
            df: target.f() - source.f(),
        }
    }

    /// Apply the **full (standard) Molodensky** transformation.
    ///
    /// Transforms geodetic coordinates on the `source` ellipsoid to geodetic
    /// coordinates on the target ellipsoid (implicitly defined by `source + da/df`).
    ///
    /// Accurate to approximately 1 m for shifts < 1000 m.
    ///
    /// # Parameters
    /// * `lat` – geodetic latitude in radians
    /// * `lon` – geodetic longitude in radians
    /// * `h`   – ellipsoidal height in metres
    ///
    /// # Returns
    /// `(lat_out_rad, lon_out_rad, h_out_metres)`.
    #[must_use]
    pub fn transform(&self, lat: f64, lon: f64, h: f64, source: &Ellipsoid) -> (f64, f64, f64) {
        let a = source.a;
        let e2 = source.e2();
        let da = self.da;
        let df = self.df;

        let sin_lat = lat.sin();
        let cos_lat = lat.cos();
        let sin_lon = lon.sin();
        let cos_lon = lon.cos();
        let sin2_lat = sin_lat * sin_lat;

        let n = a / (1.0 - e2 * sin2_lat).sqrt();
        let m = a * (1.0 - e2) / (1.0 - e2 * sin2_lat).powf(1.5);

        // Full Molodensky latitude shift (radians)
        let d_lat = (-self.dx * sin_lat * cos_lon - self.dy * sin_lat * sin_lon
            + self.dz * cos_lat
            + da * (n * e2 * sin_lat * cos_lat) / a
            + df * (m / (1.0 - source.f()) + n * (1.0 - source.f())) * sin_lat * cos_lat)
            / (m + h);

        // Full Molodensky longitude shift (radians)
        let d_lon = (-self.dx * sin_lon + self.dy * cos_lon) / ((n + h) * cos_lat);

        // Full Molodensky height shift (metres)
        let d_h = self.dx * cos_lat * cos_lon + self.dy * cos_lat * sin_lon + self.dz * sin_lat
            - da * a / n
            + df * n * (1.0 - source.f()) * sin2_lat;

        (lat + d_lat, lon + d_lon, h + d_h)
    }

    /// Apply the **abridged Molodensky** transformation.
    ///
    /// A simplified version that omits the height-dependent terms.  Suitable for
    /// quick lookups where height accuracy is not critical (errors up to ~5 m).
    ///
    /// # Parameters
    /// * `lat` – geodetic latitude in radians
    /// * `lon` – geodetic longitude in radians
    /// * `h`   – ellipsoidal height in metres
    ///
    /// # Returns
    /// `(lat_out_rad, lon_out_rad, h_out_metres)`.
    #[must_use]
    pub fn transform_abridged(
        &self,
        lat: f64,
        lon: f64,
        h: f64,
        source: &Ellipsoid,
    ) -> (f64, f64, f64) {
        let a = source.a;
        let e2 = source.e2();
        let da = self.da;
        let df = self.df;

        let sin_lat = lat.sin();
        let cos_lat = lat.cos();
        let sin_lon = lon.sin();
        let cos_lon = lon.cos();
        let sin2_lat = sin_lat * sin_lat;

        let n = a / (1.0 - e2 * sin2_lat).sqrt();
        let m = a * (1.0 - e2) / (1.0 - e2 * sin2_lat).powf(1.5);

        // Abridged latitude shift (drops the height correction from denominator)
        let d_lat = (-self.dx * sin_lat * cos_lon - self.dy * sin_lat * sin_lon
            + self.dz * cos_lat
            + da * (n * e2 * sin_lat * cos_lat) / a
            + df * (m * (1.0 - source.f()) + n / (1.0 - source.f())) * sin_lat * cos_lat)
            / m;

        // Abridged longitude shift
        let d_lon = (-self.dx * sin_lon + self.dy * cos_lon) / (n * cos_lat);

        // Abridged height shift
        let d_h = self.dx * cos_lat * cos_lon + self.dy * cos_lat * sin_lon + self.dz * sin_lat
            - da * a / n
            + df * n * sin2_lat;

        (lat + d_lat, lon + d_lon, h + d_h)
    }

    // ── Common datum shifts ───────────────────────────────────────────────────

    /// WGS84 → ED50 Molodensky parameters (approximate Europe-wide average).
    ///
    /// Returns `(params, source_ellipsoid, target_ellipsoid)`.
    #[must_use]
    pub fn wgs84_to_ed50() -> (Self, &'static Ellipsoid, &'static Ellipsoid) {
        // dx=89.5, dy=93.8, dz=123.1 (approximate Europe average for ED50)
        let params = Self::new(
            89.5,
            93.8,
            123.1,
            &Ellipsoid::WGS84,
            &Ellipsoid::INTERNATIONAL,
        );
        (params, &Ellipsoid::WGS84, &Ellipsoid::INTERNATIONAL)
    }

    /// WGS84 → Tokyo datum Molodensky parameters (Japan).
    ///
    /// Returns `(params, source_ellipsoid, target_ellipsoid)`.
    #[must_use]
    pub fn wgs84_to_tokyo() -> (Self, &'static Ellipsoid, &'static Ellipsoid) {
        // dx=-148, dy=507, dz=685 (Bessel 1841 ellipsoid)
        let params = Self::new(-148.0, 507.0, 685.0, &Ellipsoid::WGS84, &Ellipsoid::BESSEL);
        (params, &Ellipsoid::WGS84, &Ellipsoid::BESSEL)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Bursa-Wolf 7-parameter transformation
// ─────────────────────────────────────────────────────────────────────────────

/// Parameters for a Bursa-Wolf (Helmert) 7-parameter similarity transformation
/// between two ECEF frames.
///
/// The linearised (small-angle) transformation formula is:
///
/// ```text
/// [X']   [Tx]   [1    Rz  −Ry] [X]
/// [Y'] = [Ty] + (1+S)·[−Rz  1   Rx] [Y]
/// [Z']   [Tz]   [Ry  −Rx  1 ] [Z]
/// ```
///
/// where translations are in metres, rotations in arc-seconds, and the scale
/// factor `S` is in parts per million (ppm).
///
/// # References
///
/// - EPSG Guidance Note 7.2, Coordinate Conversions and Transformations
///   including Formulas (section 4.4)
/// - Bursa, M. (1962). The Theory for the Determination of the Non-parallelism
///   of the Minor Axis of the Reference Ellipsoid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BursaWolfParams {
    /// X translation (metres)
    pub tx: f64,
    /// Y translation (metres)
    pub ty: f64,
    /// Z translation (metres)
    pub tz: f64,
    /// X-axis rotation (arc-seconds)
    pub rx: f64,
    /// Y-axis rotation (arc-seconds)
    pub ry: f64,
    /// Z-axis rotation (arc-seconds)
    pub rz: f64,
    /// Scale difference (parts per million)
    pub ds: f64,
}

impl BursaWolfParams {
    /// Construct a new `BursaWolfParams` from individual components.
    #[must_use]
    pub const fn new(tx: f64, ty: f64, tz: f64, rx: f64, ry: f64, rz: f64, ds: f64) -> Self {
        Self {
            tx,
            ty,
            tz,
            rx,
            ry,
            rz,
            ds,
        }
    }

    /// Apply the 7-parameter Helmert transformation to ECEF coordinates.
    ///
    /// Uses the small-angle (linearised) rotation matrix.  Errors from the
    /// linearisation are smaller than 0.01 mm for rotation angles < 10 arc-seconds,
    /// which covers all published geodetic datum shifts.
    ///
    /// # Parameters
    /// * `x`, `y`, `z` – ECEF coordinates in metres (source frame)
    ///
    /// # Returns
    /// Transformed ECEF coordinates `(X', Y', Z')` in metres (target frame).
    #[must_use]
    pub fn transform_ecef(&self, x: f64, y: f64, z: f64) -> (f64, f64, f64) {
        let rx = self.rx * ARCSEC_TO_RAD;
        let ry = self.ry * ARCSEC_TO_RAD;
        let rz = self.rz * ARCSEC_TO_RAD;
        let s = self.ds * 1.0e-6; // ppm → dimensionless

        // Small-angle rotation matrix applied with scale factor
        let x_out = self.tx + (1.0 + s) * (x + rz * y - ry * z);
        let y_out = self.ty + (1.0 + s) * (-rz * x + y + rx * z);
        let z_out = self.tz + (1.0 + s) * (ry * x - rx * y + z);

        (x_out, y_out, z_out)
    }

    /// Compute the inverse 7-parameter Helmert transformation.
    ///
    /// The inverse is obtained by negating translations, rotations and scale.
    /// For exact inversion of the linearised model this is only approximate,
    /// but the error is < 0.01 mm for all geodetic datum shifts.
    #[must_use]
    pub fn inverse(&self) -> Self {
        Self {
            tx: -self.tx,
            ty: -self.ty,
            tz: -self.tz,
            rx: -self.rx,
            ry: -self.ry,
            rz: -self.rz,
            ds: -self.ds,
        }
    }

    /// Transform geodetic coordinates (lat/lon/h) from one ellipsoidal datum to another.
    ///
    /// Internally performs:
    /// 1. Geodetic → ECEF on `source` ellipsoid
    /// 2. 7-parameter ECEF rotation/translation/scale
    /// 3. ECEF → Geodetic on `target` ellipsoid
    ///
    /// # Parameters
    /// * `lat`, `lon` – geodetic coordinates in **radians**
    /// * `h`           – ellipsoidal height in **metres**
    /// * `source`      – source reference ellipsoid
    /// * `target`      – target reference ellipsoid
    ///
    /// # Returns
    /// `(lat_out_rad, lon_out_rad, h_out_metres)`.
    #[must_use]
    pub fn transform_geodetic(
        &self,
        lat: f64,
        lon: f64,
        h: f64,
        source: &Ellipsoid,
        target: &Ellipsoid,
    ) -> (f64, f64, f64) {
        let (xe, ye, ze) = geodetic_to_ecef(lat, lon, h, source);
        let (xt, yt, zt) = self.transform_ecef(xe, ye, ze);
        ecef_to_geodetic(xt, yt, zt, target)
    }

    // ── Standard datum shifts (EPSG parameters) ───────────────────────────────

    /// ED50 → WGS84 — approximate Europe-wide transformation (EPSG:1134 class).
    ///
    /// Note: For production use, country-specific parameters (EPSG:1311 etc.)
    /// should be preferred as accuracy varies significantly across Europe.
    #[must_use]
    pub fn ed50_to_wgs84() -> Self {
        Self::new(-89.5, -93.8, -123.1, 0.0, 0.0, 0.156, -1.2)
    }

    /// OSGB36 → WGS84 (EPSG:1314 — British National Grid to WGS84).
    ///
    /// Published by Ordnance Survey of Great Britain.
    /// Accuracy: ~4 m (use OSTN15 grid shift for sub-metre accuracy).
    #[must_use]
    pub fn osgb36_to_wgs84() -> Self {
        Self::new(
            446.448, -125.157, 542.060, -0.1502, -0.2470, -0.8421, -20.4894,
        )
    }

    /// Tokyo → WGS84 (EPSG:1312 — Japan).
    ///
    /// Applies to the Japanese national datum based on the Bessel 1841 ellipsoid.
    #[must_use]
    pub fn tokyo_to_wgs84() -> Self {
        Self::new(-146.414, 507.337, 680.507, 0.0, 0.0, 0.0, 0.0)
    }

    /// NAD27 → WGS84, CONUS average (EPSG:1173 class).
    ///
    /// This is an approximate continental average; for production work use
    /// the NADCON5 grid shift file.
    #[must_use]
    pub fn nad27_to_wgs84_conus() -> Self {
        Self::new(-8.0, 160.0, 176.0, 0.0, 0.0, 0.0, 0.0)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ITRF epoch-aware transformation
// ─────────────────────────────────────────────────────────────────────────────

/// Identifies a specific International Terrestrial Reference Frame realisation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItRfFrame {
    /// ITRF2020 — most recent realisation (reference epoch 2015.0)
    Itrf2020,
    /// ITRF2014 — reference epoch 2010.0
    Itrf2014,
    /// ITRF2008 — reference epoch 2005.0
    Itrf2008,
    /// ITRF2005 — reference epoch 2000.0
    Itrf2005,
    /// ITRF2000 — reference epoch 1997.0
    Itrf2000,
    /// ITRF97 — reference epoch 1997.0
    Itrf97,
    /// ITRF96 — reference epoch 1997.0
    Itrf96,
    /// GDA2020 — aligned to ITRF2014 at epoch 2020.0
    Gda2020,
    /// GDA94 — aligned to ITRF1992 at epoch 1994.0
    Gda94,
}

impl ItRfFrame {
    /// Return the human-readable name of this ITRF realisation.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Itrf2020 => "ITRF2020",
            Self::Itrf2014 => "ITRF2014",
            Self::Itrf2008 => "ITRF2008",
            Self::Itrf2005 => "ITRF2005",
            Self::Itrf2000 => "ITRF2000",
            Self::Itrf97 => "ITRF97",
            Self::Itrf96 => "ITRF96",
            Self::Gda2020 => "GDA2020",
            Self::Gda94 => "GDA94",
        }
    }

    /// Return the reference epoch for this ITRF realisation (decimal year).
    ///
    /// The reference epoch is the epoch at which the published Bursa-Wolf
    /// parameters apply without any rate correction.
    #[must_use]
    pub fn reference_epoch(&self) -> f64 {
        match self {
            Self::Itrf2020 => 2015.0,
            Self::Itrf2014 => 2010.0,
            Self::Itrf2008 => 2005.0,
            Self::Itrf2005 => 2000.0,
            Self::Itrf2000 => 1997.0,
            Self::Itrf97 => 1997.0,
            Self::Itrf96 => 1997.0,
            Self::Gda2020 => 2020.0,
            Self::Gda94 => 1994.0,
        }
    }
}

/// An ITRF frame identifier combined with an observation epoch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItrfEpoch {
    /// The ITRF frame realisation
    pub frame: ItRfFrame,
    /// Observation epoch in decimal years (e.g. 2005.5 = 1 July 2005)
    pub epoch: f64,
}

impl ItrfEpoch {
    /// Create a new `ItrfEpoch`.
    #[must_use]
    pub const fn new(frame: ItRfFrame, epoch: f64) -> Self {
        Self { frame, epoch }
    }
}

/// Arguments for [`ItrfTransformParams::transform_at_epoch`].
///
/// Bundling the arguments into a struct keeps the method signature within
/// clippy's `too-many-arguments` lint limit.
#[derive(Debug, Clone, Copy)]
pub struct EpochTransformArgs<'e> {
    /// Geodetic latitude in radians
    pub lat: f64,
    /// Geodetic longitude in radians
    pub lon: f64,
    /// Ellipsoidal height in metres
    pub h: f64,
    /// Source reference ellipsoid
    pub source: &'e Ellipsoid,
    /// Target reference ellipsoid
    pub target: &'e Ellipsoid,
    /// Reference epoch of the published Bursa-Wolf parameters (decimal year)
    pub ref_epoch: f64,
    /// Observation epoch to extrapolate to (decimal year)
    pub epoch: f64,
}

impl<'e> EpochTransformArgs<'e> {
    /// Construct all arguments for an epoch-aware transformation.
    #[must_use]
    pub const fn new(
        lat: f64,
        lon: f64,
        h: f64,
        source: &'e Ellipsoid,
        target: &'e Ellipsoid,
        ref_epoch: f64,
        epoch: f64,
    ) -> Self {
        Self {
            lat,
            lon,
            h,
            source,
            target,
            ref_epoch,
            epoch,
        }
    }
}

/// Parameters for a time-dependent (epoch-aware) ITRF-to-ITRF transformation.
///
/// The transformation parameters at epoch `t` are computed by linear extrapolation
/// from the reference epoch `t₀`:
///
/// ```text
/// param(t) = param(t₀) + rate × (t − t₀)
/// ```
///
/// All seven Bursa-Wolf parameters and their rates are stored explicitly.
///
/// # References
///
/// - IERS Conventions (2010), Chapter 4 — Terrestrial Reference Systems and Frames
/// - Altamimi, Z. et al. (2016), ITRF2014: A new release of the International
///   Terrestrial Reference Frame modeling nonlinear station motions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItrfTransformParams {
    /// Bursa-Wolf parameters at the reference epoch
    pub bursa_wolf: BursaWolfParams,
    /// Annual rates of change: `[dtx, dty, dtz, drx, dry, drz, dds]`
    ///
    /// Units match the corresponding `bursa_wolf` fields (m/yr, arcsec/yr, ppm/yr).
    pub rates: [f64; 7],
}

impl ItrfTransformParams {
    /// Construct new ITRF transformation parameters.
    #[must_use]
    pub const fn new(bursa_wolf: BursaWolfParams, rates: [f64; 7]) -> Self {
        Self { bursa_wolf, rates }
    }

    /// Compute the Bursa-Wolf parameters extrapolated to the given epoch.
    ///
    /// # Parameters
    /// * `epoch`     – target observation epoch (decimal year)
    /// * `ref_epoch` – reference epoch of the published parameters (decimal year)
    #[must_use]
    pub fn params_at_epoch(&self, epoch: f64, ref_epoch: f64) -> BursaWolfParams {
        let dt = epoch - ref_epoch;
        BursaWolfParams {
            tx: self.bursa_wolf.tx + self.rates[0] * dt,
            ty: self.bursa_wolf.ty + self.rates[1] * dt,
            tz: self.bursa_wolf.tz + self.rates[2] * dt,
            rx: self.bursa_wolf.rx + self.rates[3] * dt,
            ry: self.bursa_wolf.ry + self.rates[4] * dt,
            rz: self.bursa_wolf.rz + self.rates[5] * dt,
            ds: self.bursa_wolf.ds + self.rates[6] * dt,
        }
    }

    /// Transform geodetic coordinates between ITRF frames at the specified epoch.
    ///
    /// The Bursa-Wolf parameters are first extrapolated to `epoch` before the
    /// transformation is applied.
    ///
    /// # Parameters
    /// * `args` – all inputs bundled as [`EpochTransformArgs`]
    ///
    /// # Returns
    /// `(lat_out_rad, lon_out_rad, h_out_metres)`.
    #[must_use]
    pub fn transform_at_epoch(&self, args: EpochTransformArgs<'_>) -> (f64, f64, f64) {
        let bw = self.params_at_epoch(args.epoch, args.ref_epoch);
        bw.transform_geodetic(args.lat, args.lon, args.h, args.source, args.target)
    }

    // ── Standard ITRF transformation sets ────────────────────────────────────

    /// ITRF2014 → ITRF2008 transformation parameters (IERS published).
    ///
    /// Reference epoch: ITRF2014 reference epoch = 2010.0.
    /// Translations in metres; rotations in arc-seconds; scale in ppm.
    ///
    /// Source: IERS Technical Note 61, Table 3.
    #[must_use]
    pub fn itrf2014_to_itrf2008() -> Self {
        let bw = BursaWolfParams::new(
            1.6e-3,   // tx  (m)
            1.9e-3,   // ty  (m)
            2.4e-3,   // tz  (m)
            0.0,      // rx  (arcsec)
            0.0,      // ry  (arcsec)
            0.0,      // rz  (arcsec)
            -0.02e-3, // ds (ppm) — ~0.02 ppb
        );
        // Rates are negligible at millimetre/sub-ppb level
        let rates = [0.0e-4, 0.0e-4, -0.1e-4, 0.0, 0.0, 0.0, 0.003e-3];
        Self::new(bw, rates)
    }

    /// ITRF2008 → ITRF2005 transformation parameters (IERS published).
    ///
    /// Reference epoch: 2005.0.
    #[must_use]
    pub fn itrf2008_to_itrf2005() -> Self {
        let bw = BursaWolfParams::new(
            -2.0e-3, // tx
            -0.9e-3, // ty
            -4.7e-3, // tz
            0.0,     // rx
            0.0,     // ry
            0.0,     // rz
            0.94e-3, // ds (ppm)
        );
        let rates = [0.3e-4, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        Self::new(bw, rates)
    }

    /// ITRF2000 → ITRF97 transformation parameters (IERS published).
    ///
    /// Reference epoch: 1997.0.
    #[must_use]
    pub fn itrf2000_to_itrf97() -> Self {
        let bw = BursaWolfParams::new(
            6.7e-3,   // tx
            6.1e-3,   // ty
            -18.5e-3, // tz
            0.0,      // rx
            0.0,      // ry
            0.0,      // rz
            1.55e-3,  // ds
        );
        let rates = [0.0, -0.6e-4, -1.4e-4, 0.0, 0.0, 0.0, 0.01e-3];
        Self::new(bw, rates)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Unified DatumTransformer API
// ─────────────────────────────────────────────────────────────────────────────

/// The transformation method and associated parameters to use.
#[derive(Debug, Clone)]
pub enum TransformMethod {
    /// Full or abridged Molodensky transformation (3-parameter)
    Molodensky(MolodenskyParams),
    /// Bursa-Wolf / Helmert 7-parameter transformation
    BursaWolf(BursaWolfParams),
    /// Time-dependent ITRF transformation: parameters + observation epoch (decimal year)
    Itrf(ItrfTransformParams, f64),
    /// No transformation — pass coordinates through unchanged
    Identity,
}

/// High-level datum transformation API.
///
/// Wraps the underlying transformation method (Molodensky, Bursa-Wolf, ITRF, or
/// identity) together with source and target ellipsoids, and exposes a simple
/// degrees-in / degrees-out interface.
///
/// # Example
///
/// ```
/// use oxigdal_proj::datum_transform::{
///     BursaWolfParams, DatumTransformer, Ellipsoid, TransformMethod,
/// };
///
/// let bw = BursaWolfParams::osgb36_to_wgs84();
/// let transformer = DatumTransformer::new(
///     TransformMethod::BursaWolf(bw),
///     Ellipsoid::AIRY,
///     Ellipsoid::WGS84,
/// );
///
/// // Greenwich Observatory: approximately (51.4778°N, 0.0°E)
/// let (lat_out, lon_out, _h) = transformer.transform_degrees(51.4778, 0.0, 0.0);
/// println!("WGS84: {lat_out:.4}°N  {lon_out:.4}°E");
/// ```
#[derive(Debug, Clone)]
pub struct DatumTransformer {
    /// The transformation method and parameters
    pub method: TransformMethod,
    /// Source (input) ellipsoid
    pub source: Ellipsoid,
    /// Target (output) ellipsoid
    pub target: Ellipsoid,
}

impl DatumTransformer {
    /// Create a new `DatumTransformer`.
    #[must_use]
    pub const fn new(method: TransformMethod, source: Ellipsoid, target: Ellipsoid) -> Self {
        Self {
            method,
            source,
            target,
        }
    }

    /// Transform a single point given in **decimal degrees** and metres.
    ///
    /// # Parameters
    /// * `lat` – geodetic latitude in decimal degrees (positive North)
    /// * `lon` – geodetic longitude in decimal degrees (positive East)
    /// * `h`   – ellipsoidal height in metres
    ///
    /// # Returns
    /// `(lat_deg, lon_deg, h_m)` in the target datum.
    #[must_use]
    pub fn transform_degrees(&self, lat: f64, lon: f64, h: f64) -> (f64, f64, f64) {
        let lat_r = lat.to_radians();
        let lon_r = lon.to_radians();

        let (lat_out_r, lon_out_r, h_out) = match &self.method {
            TransformMethod::Identity => (lat_r, lon_r, h),

            TransformMethod::Molodensky(params) => params.transform(lat_r, lon_r, h, &self.source),

            TransformMethod::BursaWolf(params) => {
                params.transform_geodetic(lat_r, lon_r, h, &self.source, &self.target)
            }

            TransformMethod::Itrf(params, epoch) => {
                // Use ITRF2014 reference epoch as the default reference when
                // the caller does not specify a source frame.
                let ref_epoch = ItRfFrame::Itrf2014.reference_epoch();
                params.transform_at_epoch(EpochTransformArgs::new(
                    lat_r,
                    lon_r,
                    h,
                    &self.source,
                    &self.target,
                    ref_epoch,
                    *epoch,
                ))
            }
        };

        (lat_out_r.to_degrees(), lon_out_r.to_degrees(), h_out)
    }

    /// Transform multiple points given in decimal degrees.
    ///
    /// Processes each `(lat_deg, lon_deg, h_m)` tuple through
    /// [`transform_degrees`](Self::transform_degrees) and collects the results.
    #[must_use]
    pub fn transform_batch(&self, points: &[(f64, f64, f64)]) -> Vec<(f64, f64, f64)> {
        points
            .iter()
            .map(|&(lat, lon, h)| self.transform_degrees(lat, lon, h))
            .collect()
    }

    /// Return an approximate accuracy figure (in metres) for this transformation.
    ///
    /// These are conservative, order-of-magnitude estimates based on the known
    /// characteristics of each method:
    ///
    /// | Method | Approximate accuracy |
    /// |---|---|
    /// | Identity | 0 m |
    /// | Molodensky | ~1 m (for typical shifts < 1 km) |
    /// | Bursa-Wolf | ~0.1–1 m depending on published parameters |
    /// | ITRF | ~0.01 m (millimetre-level) |
    #[must_use]
    pub fn accuracy_meters(&self) -> f64 {
        match &self.method {
            TransformMethod::Identity => 0.0,
            TransformMethod::Molodensky(_) => 1.0,
            TransformMethod::BursaWolf(_) => 0.5,
            TransformMethod::Itrf(_, _) => 0.01,
        }
    }
}
