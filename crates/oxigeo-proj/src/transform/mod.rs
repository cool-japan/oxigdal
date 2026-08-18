//! Coordinate transformation operations.
//!
//! This module provides coordinate transformation capabilities between different CRS
//! using the OxiProj library for pure Rust implementations, as well as native pure-Rust
//! implementations of many map projections.
//!
//! # Module Structure
//!
//! - `cylindrical`   — Cylindrical projections (Mercator, Transverse Mercator, Cassini, etc.)
//! - `pseudocylindrical` — Pseudo-cylindrical projections (Sinusoidal, Mollweide, Robinson, Eckert IV/VI)
//! - `conic`         — Conic projections (Lambert Conic, Equidistant Conic, Albers)
//! - `azimuthal`     — Azimuthal projections (Lambert Azimuthal Equal Area, Azimuthal Equidistant, Gnomonic)

#[cfg(feature = "std")]
pub mod azimuthal;
#[cfg(feature = "std")]
pub mod conic;
#[cfg(feature = "std")]
pub mod cylindrical;
#[cfg(feature = "std")]
mod datum_shift;
#[cfg(feature = "std")]
pub mod pseudocylindrical;
#[cfg(feature = "std")]
pub mod simd;
/// Routes `transform_batch` onto the [`simd`] kernels, including the
/// applicability gate that declines the fast path whenever the kernels cannot
/// faithfully reproduce the scalar pipeline.
#[cfg(feature = "std")]
mod simd_dispatch;

#[cfg(feature = "std")]
use crate::area_of_use::area_of_use_for_epsg;
#[cfg(feature = "std")]
use crate::crs::{Crs, CrsSource};
use crate::error::{Error, Result};
use alloc::format;
use core::fmt;
#[cfg(feature = "std")]
use std::sync::Mutex;

// Re-export projection types for easy access (std only — require transcendental float math)
#[cfg(feature = "std")]
pub use azimuthal::{AzimuthalEquidistant, Gnomonic, LambertAzimuthalEqualArea};
#[cfg(feature = "std")]
pub use conic::{EquidistantConic, LambertConformalConic};
#[cfg(feature = "std")]
pub use cylindrical::{
    CassineSoldner, EllipsoidalTransverseMercator, GaussKruger, SphericalTransverseMercator,
    TransverseMercator,
};
#[cfg(feature = "std")]
pub use pseudocylindrical::{EckertIV, EckertVI, Mollweide, Robinson, Sinusoidal};

/// A 2D coordinate (x, y) or (longitude, latitude).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coordinate {
    /// X coordinate (or longitude in geographic CRS)
    pub x: f64,
    /// Y coordinate (or latitude in geographic CRS)
    pub y: f64,
}

impl Coordinate {
    /// Creates a new coordinate.
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Creates a coordinate from longitude and latitude (in degrees).
    pub fn from_lon_lat(lon: f64, lat: f64) -> Self {
        Self::new(lon, lat)
    }

    /// Returns the longitude (assumes geographic CRS).
    pub fn lon(&self) -> f64 {
        self.x
    }

    /// Returns the latitude (assumes geographic CRS).
    pub fn lat(&self) -> f64 {
        self.y
    }

    /// Validates that the coordinate is within valid bounds for a geographic CRS.
    pub fn validate_geographic(&self) -> Result<()> {
        if !(-180.0..=180.0).contains(&self.x) {
            return Err(Error::coordinate_out_of_bounds(self.x, self.y));
        }
        if !(-90.0..=90.0).contains(&self.y) {
            return Err(Error::coordinate_out_of_bounds(self.x, self.y));
        }
        Ok(())
    }

    /// Checks if the coordinate contains valid (finite) values.
    pub fn is_valid(&self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

impl fmt::Display for Coordinate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

/// A 3D coordinate (x, y, z).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coordinate3D {
    /// X coordinate
    pub x: f64,
    /// Y coordinate
    pub y: f64,
    /// Z coordinate (elevation/height)
    pub z: f64,
}

impl Coordinate3D {
    /// Creates a new 3D coordinate.
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// Converts to 2D coordinate (drops Z).
    pub fn to_2d(&self) -> Coordinate {
        Coordinate::new(self.x, self.y)
    }

    /// Checks if the coordinate contains valid (finite) values.
    pub fn is_valid(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

impl From<Coordinate> for Coordinate3D {
    fn from(coord: Coordinate) -> Self {
        Self::new(coord.x, coord.y, 0.0)
    }
}

/// A bounding box defined by minimum and maximum coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingBox {
    /// Minimum X coordinate
    pub min_x: f64,
    /// Minimum Y coordinate
    pub min_y: f64,
    /// Maximum X coordinate
    pub max_x: f64,
    /// Maximum Y coordinate
    pub max_y: f64,
}

impl BoundingBox {
    /// Creates a new bounding box.
    ///
    /// # Errors
    ///
    /// Returns an error if min > max for any dimension.
    pub fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Result<Self> {
        if min_x > max_x {
            return Err(Error::invalid_bounding_box(format!(
                "min_x ({}) > max_x ({})",
                min_x, max_x
            )));
        }
        if min_y > max_y {
            return Err(Error::invalid_bounding_box(format!(
                "min_y ({}) > max_y ({})",
                min_y, max_y
            )));
        }

        Ok(Self {
            min_x,
            min_y,
            max_x,
            max_y,
        })
    }

    /// Creates a bounding box from two coordinates.
    pub fn from_coordinates(c1: Coordinate, c2: Coordinate) -> Result<Self> {
        let min_x = c1.x.min(c2.x);
        let min_y = c1.y.min(c2.y);
        let max_x = c1.x.max(c2.x);
        let max_y = c1.y.max(c2.y);
        Self::new(min_x, min_y, max_x, max_y)
    }

    /// Returns the width of the bounding box.
    pub fn width(&self) -> f64 {
        self.max_x - self.min_x
    }

    /// Returns the height of the bounding box.
    pub fn height(&self) -> f64 {
        self.max_y - self.min_y
    }

    /// Returns the center coordinate of the bounding box.
    pub fn center(&self) -> Coordinate {
        Coordinate::new(
            (self.min_x + self.max_x) / 2.0,
            (self.min_y + self.max_y) / 2.0,
        )
    }

    /// Returns the four corner coordinates.
    pub fn corners(&self) -> [Coordinate; 4] {
        [
            Coordinate::new(self.min_x, self.min_y),
            Coordinate::new(self.max_x, self.min_y),
            Coordinate::new(self.max_x, self.max_y),
            Coordinate::new(self.min_x, self.max_y),
        ]
    }

    /// Checks if a coordinate is within the bounding box.
    pub fn contains(&self, coord: &Coordinate) -> bool {
        coord.x >= self.min_x
            && coord.x <= self.max_x
            && coord.y >= self.min_y
            && coord.y <= self.max_y
    }

    /// Expands the bounding box to include a coordinate.
    pub fn expand_to_include(&mut self, coord: &Coordinate) {
        self.min_x = self.min_x.min(coord.x);
        self.min_y = self.min_y.min(coord.y);
        self.max_x = self.max_x.max(coord.x);
        self.max_y = self.max_y.max(coord.y);
    }
}

/// Opt-in mode controlling how a [`Transformer`] reacts to coordinates that
/// fall outside the source EPSG's registered area-of-use bounding box.
///
/// The check consults [`area_of_use_for_epsg`]; if the lookup returns `None`
/// (no entry registered for that EPSG code) the check is skipped regardless of
/// mode.
#[cfg(feature = "std")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AreaOfUseCheck {
    /// Validation is disabled; out-of-area coordinates pass through silently.
    #[default]
    Off,
    /// Out-of-area coordinates are recorded via [`Transformer::last_warning`]
    /// but the transformation still proceeds.
    Warn,
    /// Out-of-area coordinates abort the transformation with
    /// [`Error::OutsideAreaOfUse`].
    Strict,
}

/// A diagnostic record describing a single point that fell outside the
/// registered area-of-use for the source EPSG.
///
/// Produced by [`Transformer`] when [`AreaOfUseCheck::Warn`] mode is enabled
/// and accessed via [`Transformer::last_warning`].
#[cfg(feature = "std")]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AreaOfUseWarning {
    /// Longitude of the offending point (degrees, WGS84).
    pub lon: f64,
    /// Latitude of the offending point (degrees, WGS84).
    pub lat: f64,
    /// Source EPSG code whose area-of-use was violated.
    pub epsg: u32,
    /// Western bound of the registered area-of-use (degrees).
    pub west: f64,
    /// Southern bound of the registered area-of-use (degrees).
    pub south: f64,
    /// Eastern bound of the registered area-of-use (degrees).
    pub east: f64,
    /// Northern bound of the registered area-of-use (degrees).
    pub north: f64,
}

/// A diagnostic record describing a compound-CRS vertical conversion whose
/// required orthometric↔ellipsoidal height correction was **skipped** because
/// no geoid model was attached to the [`Transformer`].
///
/// Produced by [`Transformer::transform_3d`] and retrieved via
/// [`Transformer::last_vertical_warning`]. Its presence means the returned
/// `z` was passed through unchanged and may be wrong by the local geoid
/// undulation `N` (globally up to ≈ ±100 m). Attach a geoid with
/// [`Transformer::with_geoid`] to perform the correction.
#[cfg(feature = "std")]
#[derive(Debug, Clone, PartialEq)]
pub struct VerticalDatumWarning {
    /// Longitude of the point whose height was left uncorrected (degrees).
    pub lon: f64,
    /// Latitude of the point whose height was left uncorrected (degrees).
    pub lat: f64,
    /// Source vertical datum name (as declared by the source compound CRS).
    pub source_vertical: String,
    /// Target vertical datum name (as declared by the target compound CRS).
    pub target_vertical: String,
}

/// Coordinate transformer that handles transformations between CRS.
#[cfg(feature = "std")]
pub struct Transformer {
    source_crs: Crs,
    target_crs: Crs,
    /// OxiProj source and target CRS pair used to build a transformer on demand.
    /// `None` when the transformation is an identity (same CRS, compound, or engineering).
    oxi_crs_pair: Option<(oxiproj::Crs, oxiproj::Crs)>,
    /// The **built** OxiProj transformer, constructed once in [`Transformer::new`]
    /// and stored here (behind the `Transformer`, which is shared as
    /// `Arc<Transformer>` by [`TransformerCache`]). Storing the engine on the
    /// struct — rather than in a per-thread cache — means that cloning and
    /// sharing an `Arc<Transformer>` across threads reuses this already-built
    /// pipeline instead of paying for a fresh `oxiproj::Transformer::new` on
    /// each thread's first call. `Some` iff `oxi_crs_pair` is `Some`.
    oxi_transformer: Option<oxiproj::Transformer>,
    /// When `true` (the default), `transform` rejects points that lie outside
    /// the source CRS's declared area of use by returning
    /// [`Error::OutOfAreaOfUse`].  When `false`, the check is skipped and the
    /// underlying OxiProj transform is attempted unconditionally.
    strict: bool,
    /// Opt-in per-instance area-of-use validation mode (orthogonal to the
    /// legacy `strict` boolean above which only fires for geographic source
    /// CRS).  Default: [`AreaOfUseCheck::Off`].
    area_of_use_check: AreaOfUseCheck,
    /// Most recent warning recorded under [`AreaOfUseCheck::Warn`] mode.
    ///
    /// Stored in a `Mutex` so that [`Transformer::transform`] can take
    /// `&self` (matching the existing API surface) while still mutating
    /// diagnostic state, and so the whole `Transformer` is `Sync` and
    /// can be safely shared across threads via [`TransformerCache`].
    /// `AreaOfUseWarning` is `Copy`, so locks are held only for the
    /// trivial duration of a load/store.
    last_warning: Mutex<Option<AreaOfUseWarning>>,
    /// Source observation epoch (decimal year) for ITRF epoch-aware transforms.
    source_epoch: Option<f64>,
    /// Target observation epoch (decimal year) for ITRF epoch-aware transforms.
    target_epoch: Option<f64>,
    /// ITRF transformation parameters and reference epoch when `with_epoch` is active.
    ///
    /// Tuple: (params, ref_epoch_decimal_year).
    itrf_params: Option<(crate::datum_transform::ItrfTransformParams, f64)>,
    /// Optional geoid model used by [`Transformer::transform_3d`] when the
    /// source and target compound CRS have vertical components of different
    /// kinds (e.g. orthometric ↔ ellipsoidal).  When `None`, the compound
    /// vertical branch falls through silently (back-compatible behaviour).
    geoid: Option<std::sync::Arc<crate::geoid::GeoidGrid>>,
    /// Most recent diagnostic recorded when a compound-CRS vertical correction
    /// was required but skipped for lack of an attached geoid. Stored in a
    /// `Mutex` (like [`Transformer::last_warning`]) so `transform_3d` can take
    /// `&self` while still recording state and remain `Sync`.
    last_vertical_warning: Mutex<Option<VerticalDatumWarning>>,
    /// Optional horizontal NTv2 grid preferred over the generic `+towgs84=`
    /// Helmert shift for geographic→geographic datum changes (e.g. OSGB36 →
    /// ETRS89 via OSTN15/BETA2007). `(grid, inverse)`: when `inverse` is true
    /// the grid's inverse transform is applied. Attached via
    /// [`Transformer::with_hgrid`]; `None` by default.
    hgrid: Option<(std::sync::Arc<crate::grid_shift::NtV2Grid>, bool)>,
}

#[cfg(feature = "std")]
impl Transformer {
    /// Creates a new transformer.
    ///
    /// # Arguments
    ///
    /// * `source_crs` - Source coordinate reference system
    /// * `target_crs` - Target coordinate reference system
    ///
    /// # Errors
    ///
    /// Returns an error if the transformation cannot be initialized.
    pub fn new(source_crs: Crs, target_crs: Crs) -> Result<Self> {
        // Compound CRS transformations are handled entirely inside `transform_3d`
        // via a dedicated sub-transformer, so no proj4rs initialisation is needed
        // at the outer level.  Skipping it also avoids the "WKT to PROJ conversion
        // not yet implemented" error that WKT-backed sub-CRS would otherwise raise.
        let is_compound = matches!(source_crs.source(), CrsSource::Compound { .. })
            || matches!(target_crs.source(), CrsSource::Compound { .. });

        // Engineering (local) CRS has no geodetic datum: no spatial conversion
        // is possible without user-supplied parameters.  Return a pass-through
        // (identity) transformer so callers receive a usable object rather than
        // an opaque WKT-conversion error.
        let either_engineering = source_crs.is_engineering() || target_crs.is_engineering();

        let (oxi_crs_pair, oxi_transformer) =
            if is_compound || either_engineering || source_crs.is_equivalent(&target_crs) {
                (None, None)
            } else {
                let oxi_src = crs_to_oxi(&source_crs)?;
                let oxi_tgt = crs_to_oxi(&target_crs)?;
                // Build the OxiProj transformer exactly once, here. This both
                // validates that the pipeline can be constructed (errors
                // surface at construction rather than on the first
                // `.transform()` call) and stores the built engine on the
                // struct so that sharing an `Arc<Transformer>` across threads
                // reuses it — no per-thread rebuild.
                let built = oxiproj::Transformer::new(oxi_src.clone(), oxi_tgt.clone())
                    .map_err(crate::error::Error::from)?;
                (Some((oxi_src, oxi_tgt)), Some(built))
            };

        Ok(Self {
            source_crs,
            target_crs,
            oxi_crs_pair,
            oxi_transformer,
            strict: true,
            area_of_use_check: AreaOfUseCheck::default(),
            last_warning: Mutex::new(None),
            source_epoch: None,
            target_epoch: None,
            itrf_params: None,
            geoid: None,
            last_vertical_warning: Mutex::new(None),
            hgrid: None,
        })
    }

    /// Attaches a horizontal NTv2 grid to be used **instead of** the generic
    /// `+towgs84=` Helmert shift when transforming between two **geographic**
    /// CRS.
    ///
    /// This is how a caller opts into the centimetre-accurate national grid
    /// transforms (OSTN15/BETA2007-style `.gsb` files) that a plain Helmert
    /// `+towgs84=` cannot reach: load the grid with
    /// [`NtV2Grid::from_bytes`](crate::NtV2Grid::from_bytes) and hand it here.
    /// When set, [`transform`](Self::transform) applies the grid (forward, or
    /// inverse when `inverse` is `true`) to the lon/lat pair and skips the
    /// OxiProj pipeline entirely. The grid is ignored for projected CRS (whose
    /// `x`/`y` are eastings/northings, not degrees).
    ///
    /// Returns the transformer by value so the call may be chained.
    pub fn with_hgrid(
        mut self,
        grid: std::sync::Arc<crate::grid_shift::NtV2Grid>,
        inverse: bool,
    ) -> Self {
        self.hgrid = Some((grid, inverse));
        self
    }

    /// Returns the attached horizontal grid and its inverse flag, if any.
    pub fn hgrid(&self) -> Option<(&std::sync::Arc<crate::grid_shift::NtV2Grid>, bool)> {
        self.hgrid.as_ref().map(|(g, inv)| (g, *inv))
    }

    /// Returns the most recent [`VerticalDatumWarning`] recorded by
    /// [`transform_3d`](Self::transform_3d) when a required compound-CRS
    /// orthometric↔ellipsoidal height correction was skipped because no geoid
    /// was attached.
    ///
    /// Returns `None` if no such condition has occurred (including the normal
    /// case where a geoid *was* attached and the correction was applied).
    pub fn last_vertical_warning(&self) -> Option<VerticalDatumWarning> {
        self.last_vertical_warning
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    /// Clears any previously-recorded vertical-datum warning.
    pub fn clear_vertical_warning(&self) {
        if let Ok(mut slot) = self.last_vertical_warning.lock() {
            *slot = None;
        }
    }

    /// Records a vertical-datum warning (internal helper).
    fn record_vertical_warning(&self, lon: f64, lat: f64, src_v: &Crs, dst_v: &Crs) {
        if let Ok(mut slot) = self.last_vertical_warning.lock() {
            *slot = Some(VerticalDatumWarning {
                lon,
                lat,
                source_vertical: src_v.name().unwrap_or("").to_string(),
                target_vertical: dst_v.name().unwrap_or("").to_string(),
            });
        }
    }

    /// Attaches a geoid model to this transformer.
    ///
    /// When both the source and target CRS are [`CrsSource::Compound`] and
    /// their vertical components are of different kinds (one ellipsoidal,
    /// one orthometric), [`Transformer::transform_3d`] uses the attached
    /// grid to apply the corresponding height correction (`N` added or
    /// subtracted depending on direction).  Without a geoid attached the
    /// vertical conversion silently passes through, preserving the legacy
    /// behaviour of pre-Slice-14 builds.
    ///
    /// Returns the transformer by value so the call may be chained:
    ///
    /// ```no_run
    /// use std::sync::Arc;
    /// use oxigeo_proj::{Crs, Transformer};
    /// use oxigeo_proj::geoid::{GeoidModel, synthetic_grid};
    ///
    /// let t = Transformer::new(Crs::wgs84(), Crs::wgs84())
    ///     .expect("ok")
    ///     .with_geoid(Arc::new(synthetic_grid(GeoidModel::Egm96)));
    /// assert!(t.geoid().is_some());
    /// ```
    pub fn with_geoid(mut self, grid: std::sync::Arc<crate::geoid::GeoidGrid>) -> Self {
        self.geoid = Some(grid);
        self
    }

    /// Returns the attached geoid model, if any.
    pub fn geoid(&self) -> Option<&std::sync::Arc<crate::geoid::GeoidGrid>> {
        self.geoid.as_ref()
    }

    /// Sets strict area-of-use validation.
    ///
    /// When `strict` is `true` (the default), [`Transformer::transform`] returns
    /// [`Error::OutOfAreaOfUse`] for any point that lies outside the source CRS's
    /// declared area of use.  When `false`, the check is skipped silently.
    pub fn with_strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }

    /// Returns whether strict area-of-use validation is enabled.
    pub fn is_strict(&self) -> bool {
        self.strict
    }

    /// Configures the per-instance area-of-use validation mode.
    ///
    /// This is independent of the legacy `with_strict`/`is_strict` switch and
    /// applies in [`transform`](Self::transform) and
    /// [`transform_batch`](Self::transform_batch) **before** the underlying
    /// projection runs.  The bounds are looked up via
    /// [`area_of_use_for_epsg`] on the **source EPSG**; if that lookup
    /// returns `None`, the check is skipped silently regardless of mode.
    ///
    /// Behaviour:
    /// * [`AreaOfUseCheck::Off`] (default) — no validation is performed.
    /// * [`AreaOfUseCheck::Warn`] — out-of-area points are recorded via
    ///   [`last_warning`](Self::last_warning) but the transform proceeds.
    /// * [`AreaOfUseCheck::Strict`] — out-of-area points abort the transform
    ///   with [`Error::OutsideAreaOfUse`].
    pub fn with_area_of_use_check(mut self, mode: AreaOfUseCheck) -> Self {
        self.area_of_use_check = mode;
        if let Ok(mut slot) = self.last_warning.lock() {
            *slot = None;
        }
        self
    }

    /// Returns the currently configured area-of-use check mode.
    pub fn area_of_use_check(&self) -> AreaOfUseCheck {
        self.area_of_use_check
    }

    /// Returns the most recent area-of-use warning recorded under
    /// [`AreaOfUseCheck::Warn`] mode, if any.
    ///
    /// Returns `None` when the check mode is [`AreaOfUseCheck::Off`], when no
    /// out-of-area point has been seen yet, or when the source EPSG has no
    /// registered area-of-use entry.
    pub fn last_warning(&self) -> Option<AreaOfUseWarning> {
        // `AreaOfUseWarning` is `Copy`, so we can simply dereference
        // the guard.  Poisoning is treated as "no warning" — the
        // accessor never panics on a poisoned lock.
        self.last_warning.lock().ok().and_then(|guard| *guard)
    }

    /// Clears any previously-recorded area-of-use warning.
    pub fn clear_warning(&self) {
        if let Ok(mut slot) = self.last_warning.lock() {
            *slot = None;
        }
    }

    /// Internal helper: runs the area-of-use check for `(lon, lat)` against the
    /// source EPSG (only if one is available).
    ///
    /// * In [`AreaOfUseCheck::Strict`] mode, returns
    ///   `Err(Error::OutsideAreaOfUse { .. })` on violation.
    /// * In [`AreaOfUseCheck::Warn`] mode, records the violation via
    ///   `self.last_warning` and returns `Ok(())`.
    /// * In [`AreaOfUseCheck::Off`] mode, returns `Ok(())` immediately.
    /// * When the source CRS has no EPSG code or no registered area-of-use,
    ///   returns `Ok(())` regardless of mode.
    fn check_area_of_use(&self, lon: f64, lat: f64) -> Result<()> {
        if self.area_of_use_check == AreaOfUseCheck::Off {
            return Ok(());
        }
        let epsg = match self.source_crs.epsg_code() {
            Some(c) => c,
            None => return Ok(()),
        };
        let aou = match area_of_use_for_epsg(epsg) {
            Some(a) => a,
            None => return Ok(()),
        };
        if aou.contains(lon, lat) {
            return Ok(());
        }
        match self.area_of_use_check {
            AreaOfUseCheck::Off => Ok(()),
            AreaOfUseCheck::Warn => {
                if let Ok(mut slot) = self.last_warning.lock() {
                    *slot = Some(AreaOfUseWarning {
                        lon,
                        lat,
                        epsg,
                        west: aou.west,
                        south: aou.south,
                        east: aou.east,
                        north: aou.north,
                    });
                }
                Ok(())
            }
            AreaOfUseCheck::Strict => Err(Error::OutsideAreaOfUse {
                lon,
                lat,
                epsg,
                west: aou.west,
                south: aou.south,
                east: aou.east,
                north: aou.north,
            }),
        }
    }

    /// Configure a time-dependent ITRF epoch transformation.
    ///
    /// Both the source and target CRS must be ITRF-based (recognised by EPSG code or
    /// by frame name in the datum/CRS name), and a preset must exist for that pair in
    /// the built-in IERS table.
    ///
    /// The Bursa-Wolf parameters are extrapolated linearly from the published reference
    /// epoch to the requested observation epochs before the Helmert transformation is
    /// applied.  When `source_epoch == target_epoch` the correction is zero and the
    /// output equals the input.
    ///
    /// # Parameters
    /// * `source_epoch` – observation epoch of the input coordinates (decimal year, e.g. 2015.0)
    /// * `target_epoch` – desired output epoch (decimal year, e.g. 2020.75)
    ///
    /// # Errors
    ///
    /// Returns `Err` if:
    /// - either CRS is not an ITRF realisation, or
    /// - no registered IERS preset exists for the source→target frame pair.
    pub fn with_epoch(mut self, source_epoch: f64, target_epoch: f64) -> Result<Self> {
        let src_itrf = self.source_crs.itrf_name().ok_or_else(|| {
            Error::transformation_error(
                "with_epoch requires the source CRS to be an ITRF realisation",
            )
        })?;
        let dst_itrf = self.target_crs.itrf_name().ok_or_else(|| {
            Error::transformation_error(
                "with_epoch requires the target CRS to be an ITRF realisation",
            )
        })?;

        let params_ref = crate::datum_transform::find_itrf_params(&src_itrf, &dst_itrf)
            .ok_or_else(|| {
                Error::transformation_error(format!(
                    "no ITRF parameters registered for {src_itrf} \u{2192} {dst_itrf}"
                ))
            })?;

        self.source_epoch = Some(source_epoch);
        self.target_epoch = Some(target_epoch);
        self.itrf_params = Some(params_ref);
        Ok(self)
    }

    /// Returns the configured source epoch, if any.
    pub fn source_epoch(&self) -> Option<f64> {
        self.source_epoch
    }

    /// Returns the configured target epoch, if any.
    pub fn target_epoch(&self) -> Option<f64> {
        self.target_epoch
    }

    /// Creates a transformer from EPSG codes.
    ///
    /// # Arguments
    ///
    /// * `source_epsg` - Source EPSG code
    /// * `target_epsg` - Target EPSG code
    ///
    /// # Errors
    ///
    /// Returns an error if the EPSG codes are invalid or transformation cannot be initialized.
    pub fn from_epsg(source_epsg: u32, target_epsg: u32) -> Result<Self> {
        let source_crs = Crs::from_epsg(source_epsg)?;
        let target_crs = Crs::from_epsg(target_epsg)?;
        Self::new(source_crs, target_crs)
    }

    /// Returns the source CRS.
    pub fn source_crs(&self) -> &Crs {
        &self.source_crs
    }

    /// Returns the target CRS.
    pub fn target_crs(&self) -> &Crs {
        &self.target_crs
    }

    /// Transforms a single coordinate.
    ///
    /// # Arguments
    ///
    /// * `coord` - Input coordinate in source CRS
    ///
    /// # Errors
    ///
    /// Returns an error if the transformation fails, or if `self.strict` is
    /// `true` and the point lies outside the source CRS's declared area of use.
    pub fn transform(&self, coord: &Coordinate) -> Result<Coordinate> {
        // Opt-in per-instance area-of-use check (independent of the legacy
        // `strict` flag below): runs first so that even no-op same-CRS pairs
        // honour the configured policy.
        self.check_area_of_use(coord.x, coord.y)?;

        // If no transformation needed, return as-is
        if self.oxi_crs_pair.is_none() {
            return Ok(*coord);
        }

        // Validate input
        if !coord.is_valid() {
            return Err(Error::invalid_coordinate(
                "Coordinate contains non-finite values",
            ));
        }

        // Area-of-use check: only when strict mode is active, the source CRS
        // actually declares bounds, AND the source CRS is geographic (lon/lat in
        // degrees).  For projected CRS, the input coordinates are in metres, so
        // comparing them against degree-based AoU bounds is meaningless.
        // `area_of_use()` returns None for CRS created from PROJ strings, WKT, or
        // custom definitions, in which case the check is skipped silently.
        if self.strict
            && self.source_crs.is_geographic()
            && let Some(aou) = self.source_crs.area_of_use()
            && !aou.contains(coord.x, coord.y)
        {
            return Err(Error::out_of_area_of_use(
                coord.x,
                coord.y,
                self.source_crs.to_string(),
            ));
        }

        // Prefer an attached NTv2 grid over the generic Helmert shift when both
        // CRS are geographic (so x/y really are lon/lat degrees). This is the
        // path that delivers OSTN15/BETA2007-grade accuracy for national datum
        // changes such as OSGB36 → ETRS89.
        if let Some((grid, inverse)) = &self.hgrid
            && self.source_crs.is_geographic()
            && self.target_crs.is_geographic()
        {
            let (lon, lat) = if *inverse {
                grid.inverse_transform(coord.x, coord.y)?
            } else {
                grid.transform(coord.x, coord.y)?
            };
            let result = Coordinate::new(lon, lat);
            if !result.is_valid() {
                return Err(Error::transformation_error(
                    "Grid-shifted transformation resulted in non-finite values",
                ));
            }
            return Ok(result);
        }

        // Perform transformation using OxiProj
        self.transform_impl(coord)
    }

    /// Transforms a 3D coordinate.
    ///
    /// When both the source and target CRS are `CrsSource::Compound`, the
    /// horizontal pair (x, y) is transformed independently using a sub-
    /// transformer, and the vertical component (z) is handled as follows:
    ///
    /// * If the source and target vertical CRS are equivalent, `z` is passed
    ///   through unchanged.
    /// * Otherwise, the vertical datums of source and target are classified
    ///   via [`crate::geoid::classify_vertical_datum`].  When a geoid model
    ///   has been attached via [`Transformer::with_geoid`] and the pair is
    ///   `orthometric ↔ ellipsoidal`, the undulation correction is applied
    ///   (`h_ellip = h_ortho + N` or `h_ortho = h_ellip − N`).
    /// * In any other case (no geoid attached, or one side `Unknown`), `z`
    ///   is silently passed through unchanged to preserve back-compat with
    ///   pre-Slice-14 builds.
    ///
    /// When `with_epoch` has been called, the ITRF epoch correction is applied
    /// using the Bursa-Wolf parameters extrapolated to the requested epochs.
    /// The coordinate convention is: `coord.x` = geodetic longitude (degrees),
    /// `coord.y` = geodetic latitude (degrees), `coord.z` = ellipsoidal height
    /// (metres).
    ///
    /// ## Ordinary (non-compound, non-ITRF) datum-changing 3-D transforms
    ///
    /// For a plain horizontal-datum change between two **geographic** CRS
    /// (e.g. NAD27 → WGS84, ED50 → WGS84, Tokyo → WGS84, OSGB36 → WGS84)
    /// where [`Crs::datum`] resolves to one of the datum names this crate
    /// ships a published Bursa-Wolf preset for (see
    /// [`crate::datum_transform::BursaWolfParams`]), the point is routed
    /// through the full geodetic → ECEF → 7-parameter Helmert → ECEF →
    /// geodetic round trip, so the returned height is adjusted consistently
    /// with the horizontal shift (mirroring the ITRF-epoch branch above).
    ///
    /// **For every other CRS pair** — an unrecognised/custom datum name, a
    /// projected CRS on either side (where `x`/`y` are eastings/northings,
    /// not lon/lat, so no geodetic Helmert shift can be applied without
    /// first un-projecting), or CRS sourced from a PROJ string/WKT/custom
    /// definition (which carry no datum name at all) — only the horizontal
    /// `(x, y)` is transformed and `z` is passed through **unchanged**.
    /// This is a known, deliberate limitation, not a bug: a horizontal
    /// datum shift generally *does* change ellipsoidal height, so treat the
    /// returned height as unreliable whenever the source/target datum names
    /// are not both one of the recognised pairs above. Callers that need a
    /// hard guarantee should either restrict themselves to the recognised
    /// datum pairs, or perform their own geodetic Helmert shift via
    /// [`crate::datum_transform::BursaWolfParams::transform_geodetic`].
    pub fn transform_3d(&self, coord: &Coordinate3D) -> Result<Coordinate3D> {
        // ITRF epoch-aware branch: applies before all other transformations.
        if let (Some((params, ref_epoch)), Some(t0), Some(t1)) =
            (&self.itrf_params, self.source_epoch, self.target_epoch)
        {
            if !coord.is_valid() {
                return Err(Error::invalid_coordinate(
                    "Coordinate contains non-finite values",
                ));
            }

            // Short-circuit: zero epoch difference → exact identity (avoids
            // floating-point rounding from ECEF round-trips).
            if (t1 - t0).abs() < f64::EPSILON {
                return Ok(*coord);
            }

            // `EpochTransformArgs` expects lat/lon in **radians**.
            // `Coordinate3D` stores geographic degrees: x=lon, y=lat, z=height.
            let lat_rad = coord.y.to_radians();
            let lon_rad = coord.x.to_radians();

            // Use the GRS80 ellipsoid (shared by all ITRF realisations).
            let ellipsoid = crate::datum_transform::Ellipsoid::GRS80;

            // Extrapolate Bursa-Wolf params to the observation epoch, then transform.
            // For epoch-aware ITRF transforms we interpret:
            //   source_epoch = t0 (epoch of input coords)
            //   target_epoch = t1 (desired output epoch)
            // The delta epoch used for extrapolation is (t1 − ref_epoch).
            // The source epoch is used to interpolate a second Helmert set which is
            // then composed: net displacement = bw(t1) − bw(t0), applied in one pass.
            let bw_t1 = params.params_at_epoch(t1, *ref_epoch);
            let bw_t0 = params.params_at_epoch(t0, *ref_epoch);

            // Net Bursa-Wolf parameters representing the coordinate change
            // from epoch t0 to epoch t1.
            let net_bw = crate::datum_transform::BursaWolfParams {
                tx: bw_t1.tx - bw_t0.tx,
                ty: bw_t1.ty - bw_t0.ty,
                tz: bw_t1.tz - bw_t0.tz,
                rx: bw_t1.rx - bw_t0.rx,
                ry: bw_t1.ry - bw_t0.ry,
                rz: bw_t1.rz - bw_t0.rz,
                ds: bw_t1.ds - bw_t0.ds,
            };

            // Apply the net Bursa-Wolf correction.  Source and target share the same
            // GRS80 ellipsoid (all ITRF realisations are defined on GRS80).
            let (lat_out_rad, lon_out_rad, h_out) =
                net_bw.transform_geodetic(lat_rad, lon_rad, coord.z, &ellipsoid, &ellipsoid);

            return Ok(Coordinate3D::new(
                lon_out_rad.to_degrees(),
                lat_out_rad.to_degrees(),
                h_out,
            ));
        }

        // Compound-CRS branch: split horizontal and vertical transformations.
        if let (
            CrsSource::Compound {
                horizontal: src_h,
                vertical: src_v,
            },
            CrsSource::Compound {
                horizontal: dst_h,
                vertical: dst_v,
            },
        ) = (self.source_crs.source(), self.target_crs.source())
        {
            if !coord.is_valid() {
                return Err(Error::invalid_coordinate(
                    "Coordinate contains non-finite values",
                ));
            }

            // Transform the horizontal (x, y) pair.
            let h_transformer = Transformer::new((**src_h).clone(), (**dst_h).clone())?;
            let xy_2d = Coordinate::new(coord.x, coord.y);
            let transformed_xy = h_transformer.transform(&xy_2d)?;

            // Transform the vertical (z):
            //   1. If source and target vertical datums match, passthrough.
            //   2. Otherwise classify each vertical CRS and, if an undulation
            //      shift is required AND a geoid model is attached, apply it.
            //   3. When the shift is required but no geoid is attached, fall
            //      through with z unchanged BUT record a
            //      [`VerticalDatumWarning`] (retrievable via
            //      `last_vertical_warning`).  This preserves the contract that
            //      `transform_3d` does not fail in the default (no-geoid)
            //      configuration, while giving callers a runtime signal that
            //      the height was left uncorrected — instead of the previous
            //      wholly-silent fall-through.  Callers needing a hard
            //      guarantee should attach a geoid via `with_geoid`.
            let z = if src_v.is_equivalent(dst_v) {
                coord.z
            } else {
                use crate::geoid::{VerticalDatumKind, classify_vertical_datum};
                let src_kind = classify_vertical_datum(src_v.name().unwrap_or(""));
                let dst_kind = classify_vertical_datum(dst_v.name().unwrap_or(""));
                match (src_kind, dst_kind, self.geoid.as_ref()) {
                    // Orthometric → ellipsoidal: h_ellip = h_ortho + N
                    (
                        VerticalDatumKind::Orthometric,
                        VerticalDatumKind::Ellipsoidal,
                        Some(grid),
                    ) => grid.orthometric_to_ellipsoidal(coord.y, coord.x, coord.z),
                    // Ellipsoidal → orthometric: h_ortho = h_ellip − N
                    (
                        VerticalDatumKind::Ellipsoidal,
                        VerticalDatumKind::Orthometric,
                        Some(grid),
                    ) => grid.ellipsoidal_to_orthometric(coord.y, coord.x, coord.z),
                    // Correction required (orthometric↔ellipsoidal) but no
                    // geoid attached: record a diagnostic, then pass z through.
                    (VerticalDatumKind::Orthometric, VerticalDatumKind::Ellipsoidal, None)
                    | (VerticalDatumKind::Ellipsoidal, VerticalDatumKind::Orthometric, None) => {
                        self.record_vertical_warning(coord.x, coord.y, src_v, dst_v);
                        coord.z
                    }
                    // Any other case (same kind, or unclassifiable): passthrough.
                    _ => coord.z,
                }
            };

            return Ok(Coordinate3D::new(transformed_xy.x, transformed_xy.y, z));
        }

        if self.oxi_crs_pair.is_none() {
            return Ok(*coord);
        }

        if !coord.is_valid() {
            return Err(Error::invalid_coordinate(
                "Coordinate contains non-finite values",
            ));
        }

        // Height-consistent horizontal datum shift: when both CRS are
        // geographic (so `coord.x`/`coord.y` are genuinely lon/lat degrees,
        // not projected eastings/northings) and their datum names resolve
        // to a known published Bursa-Wolf preset, route the point through
        // the full geodetic -> ECEF -> 7-parameter Helmert -> ECEF ->
        // geodetic round trip. This adjusts height consistently with the
        // horizontal shift instead of leaving it untouched.
        if self.source_crs.is_geographic()
            && self.target_crs.is_geographic()
            && let (Some(src_datum), Some(dst_datum)) =
                (self.source_crs.datum(), self.target_crs.datum())
            && let Some(shift) = datum_shift::known_horizontal_datum_shift(src_datum, dst_datum)
        {
            let lat_rad = coord.y.to_radians();
            let lon_rad = coord.x.to_radians();
            let (lat_out_rad, lon_out_rad, h_out) = shift.params.transform_geodetic(
                lat_rad,
                lon_rad,
                coord.z,
                &shift.source_ellipsoid,
                &shift.target_ellipsoid,
            );
            return Ok(Coordinate3D::new(
                lon_out_rad.to_degrees(),
                lat_out_rad.to_degrees(),
                h_out,
            ));
        }

        // Transform 2D part
        let coord_2d = coord.to_2d();
        let transformed_2d = self.transform_impl(&coord_2d)?;

        // No known height-consistent datum-shift preset applies to this CRS
        // pair (see the "Ordinary ... 3-D transforms" section of this
        // method's doc comment for the exact scope): Z is passed through
        // unchanged rather than fabricated.
        Ok(Coordinate3D::new(
            transformed_2d.x,
            transformed_2d.y,
            coord.z,
        ))
    }

    /// Transforms multiple coordinates in batch.
    ///
    /// Attempts SIMD-accelerated kernels for Transverse Mercator, Mercator, and
    /// Lambert Conformal Conic projections first.  Falls back to scalar
    /// point-by-point transformation for any other projection or when the
    /// projection parameters cannot be extracted from the PROJ string.
    ///
    /// # Arguments
    ///
    /// * `coords` - Input coordinates in source CRS
    ///
    /// # Errors
    ///
    /// Returns an error if any transformation fails.
    pub fn transform_batch(&self, coords: &[Coordinate]) -> Result<Vec<Coordinate>> {
        // Opt-in area-of-use check runs once over the whole batch so that the
        // SIMD fast-path also honours the configured policy.  In `Warn` mode
        // `last_warning` reflects the *last* offending point in the input
        // sequence (consistent with the scalar `transform` path).
        if self.area_of_use_check != AreaOfUseCheck::Off {
            for c in coords {
                self.check_area_of_use(c.x, c.y)?;
            }
        }
        if let Some(result) = self.try_simd_batch(coords) {
            return result;
        }
        coords.iter().map(|c| self.transform(c)).collect()
    }

    /// Transforms a bounding box.
    ///
    /// This transforms all four corners and creates a new bounding box from the results.
    ///
    /// # Arguments
    ///
    /// * `bbox` - Input bounding box in source CRS
    ///
    /// # Errors
    ///
    /// Returns an error if the transformation fails.
    pub fn transform_bbox(&self, bbox: &BoundingBox) -> Result<BoundingBox> {
        if self.oxi_crs_pair.is_none() {
            return Ok(*bbox);
        }

        // Transform all four corners
        let corners = bbox.corners();
        let transformed_corners = self.transform_batch(&corners)?;

        // Find new bounds
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;

        for corner in &transformed_corners {
            min_x = min_x.min(corner.x);
            min_y = min_y.min(corner.y);
            max_x = max_x.max(corner.x);
            max_y = max_y.max(corner.y);
        }

        BoundingBox::new(min_x, min_y, max_x, max_y)
    }

    /// Internal implementation of coordinate transformation using OxiProj.
    ///
    /// Uses the [`oxiproj::Transformer`] built once in [`Transformer::new`] and
    /// stored on the struct, so every call — from any thread sharing this
    /// `Arc<Transformer>` — reuses the same pipeline with no rebuild.
    fn transform_impl(&self, coord: &Coordinate) -> Result<Coordinate> {
        match &self.oxi_transformer {
            Some(t) => {
                let oxi_coord = oxiproj::Coordinate {
                    x: coord.x,
                    y: coord.y,
                };
                let out = t.transform(&oxi_coord).map_err(crate::error::Error::from)?;

                let result = Coordinate { x: out.x, y: out.y };
                if !result.is_valid() {
                    return Err(Error::transformation_error(
                        "Transformation resulted in non-finite values",
                    ));
                }
                Ok(result)
            }
            None => Ok(*coord),
        }
    }
}

/// Converts an [`oxigeo_proj::Crs`] to an [`oxiproj::Crs`] for use with the
/// OxiProj transformation engine.
///
/// # Feature-invariant resolution
///
/// Every CRS is resolved through *this crate's* PROJ string
/// ([`Crs::to_proj_string`] → the embedded, PROJ-verified EPSG registry) in
/// **every** feature configuration, so a given `(source, target)` pair produces
/// the same numbers with and without `proj-db`.
///
/// Until 0.2.4 the `proj-db` feature instead routed an EPSG-sourced CRS to
/// `oxiproj::Crs::from_epsg` (oxiproj's bundled authority database). Because
/// [`Crs::from_epsg`] itself goes through `lookup_epsg`, that branch could only
/// ever fire for codes the embedded registry *already* carries — it added no
/// coverage, only a second, divergent definition of the same code. Two failure
/// modes followed:
///
/// * **Asymmetric pairs.** A PROJ-string CRS transformed against an
///   EPSG-sourced one mixed a datum-bearing definition (`+towgs84` from the
///   registry string) with a datum-less authority one, and the resulting
///   pipeline applied a *one-sided* datum shift — e.g. a code's own geodetic
///   base → the code came out 87 m off for `EPSG:2039` and 226 m off for
///   `EPSG:2056`, where PROJ 9.7.0 returns the projection alone.
/// * **Divergent definitions.** oxiproj 0.1.4's authority definitions disagree
///   with PROJ for several codes even when both sides are EPSG-sourced
///   (`EPSG:2314`/`EPSG:24382` emit the ellipsoid's semi-major axis in the
///   CRS's own linear unit while still saying `+units=m`; `EPSG:6933` emits
///   `+lat_1` instead of `+lat_ts`; `EPSG:2062`/`EPSG:5469`/`EPSG:24382` fail
///   to build a transformer at all).
///
/// `oxiproj::Crs::from_epsg` is therefore kept only as a **fallback** for the
/// one case the registry cannot serve: a [`CrsSource::Epsg`] holding a code
/// that is absent from the embedded registry (reachable via `Deserialize`, and
/// otherwise not constructible). That keeps `proj-db` strictly additive — it
/// widens coverage, it never changes an answer the default build already gives.
#[cfg(feature = "std")]
fn crs_to_oxi(crs: &Crs) -> Result<oxiproj::Crs> {
    match crs.to_proj_string() {
        Ok(proj_str) => oxiproj::Crs::from_proj(&proj_str).map_err(crate::error::Error::from),
        Err(err) => crs_to_oxi_unregistered(crs, err),
    }
}

/// Fallback for a [`Crs`] this crate's registry cannot turn into a PROJ string.
///
/// With `proj-db` (hence `oxiproj/epsg`) an EPSG-sourced code outside the
/// embedded registry is looked up in oxiproj's bundled authority database;
/// anything else propagates the original registry error. See [`crs_to_oxi`].
#[cfg(all(feature = "std", feature = "proj-db"))]
fn crs_to_oxi_unregistered(crs: &Crs, err: Error) -> Result<oxiproj::Crs> {
    match crs.source() {
        CrsSource::Epsg(code) => oxiproj::Crs::from_epsg(*code).map_err(crate::error::Error::from),
        _ => Err(err),
    }
}

/// Fallback for a [`Crs`] this crate's registry cannot turn into a PROJ string.
///
/// Without `proj-db` there is no second CRS source to consult — `oxiproj/epsg`
/// (and with it `oxiproj-db`, which is not wasm-safe) is deliberately not
/// linked — so the registry error is final. See [`crs_to_oxi`].
#[cfg(all(feature = "std", not(feature = "proj-db")))]
fn crs_to_oxi_unregistered(_crs: &Crs, err: Error) -> Result<oxiproj::Crs> {
    Err(err)
}

/// Transforms a coordinate from one CRS to another (convenience function).
#[cfg(feature = "std")]
///
/// # Arguments
///
/// * `coord` - Input coordinate
/// * `source_crs` - Source CRS
/// * `target_crs` - Target CRS
///
/// # Errors
///
/// Returns an error if the transformation fails.
pub fn transform_coordinate(
    coord: &Coordinate,
    source_crs: &Crs,
    target_crs: &Crs,
) -> Result<Coordinate> {
    let transformer = Transformer::new(source_crs.clone(), target_crs.clone())?;
    transformer.transform(coord)
}

/// Process-wide cache of [`Transformer`] instances keyed by
/// `(src_epsg, dst_epsg)`, backing [`transform_epsg`].
///
/// Without this cache, every call to `transform_epsg` paid the full cost of
/// [`Transformer::from_epsg`] — `Crs::from_epsg` × 2, `crs_to_oxi` × 2 (PROJ
/// string parsing, and EPSG-database resolution when the opt-in `proj-db`
/// feature is active), plus an `oxiproj::Transformer` construction — on
/// *every single coordinate*. For bulk workloads (one coordinate at a time
/// through a dense GeoJSON geometry, a tile pipeline, …) this dominated
/// runtime. Reusing the same `Arc<Transformer>` across calls for a repeated
/// EPSG pair amortises that cost to a single build.
///
/// Capacity is a pragmatic default sized for typical ETL/tile pipelines that
/// cycle through a modest number of distinct EPSG pairs. Callers with wider
/// fan-out, or who want explicit control over the cache's lifetime/capacity,
/// should build their own [`crate::cache::TransformerCache`] (or construct
/// and reuse a bare [`Transformer`] directly) instead of calling
/// `transform_epsg` in a hot loop.
#[cfg(feature = "std")]
static GLOBAL_TRANSFORMER_CACHE: once_cell::sync::Lazy<crate::cache::TransformerCache> =
    once_cell::sync::Lazy::new(|| crate::cache::TransformerCache::new(64));

/// Transforms coordinates from one EPSG code to another (convenience function).
///
/// Internally reuses a process-wide, thread-safe cache of already-built
/// [`Transformer`] instances keyed by `(source_epsg, target_epsg)` — see
/// `GLOBAL_TRANSFORMER_CACHE` — so repeated calls with the same EPSG pair
/// do not re-resolve the CRS or reinitialise the underlying `oxiproj`
/// transformer pipeline. For transforming many coordinates through the same
/// pair, prefer building a [`Transformer`] once (via
/// [`Transformer::from_epsg`]) and calling [`Transformer::transform`]
/// directly, or reuse a [`crate::cache::TransformerCache`] handle — both
/// avoid this function's per-call cache lookup overhead.
#[cfg(feature = "std")]
///
/// # Arguments
///
/// * `coord` - Input coordinate
/// * `source_epsg` - Source EPSG code
/// * `target_epsg` - Target EPSG code
///
/// # Errors
///
/// Returns an error if the transformation fails.
pub fn transform_epsg(
    coord: &Coordinate,
    source_epsg: u32,
    target_epsg: u32,
) -> Result<Coordinate> {
    let transformer = GLOBAL_TRANSFORMER_CACHE.get_or_build(source_epsg, target_epsg)?;
    transformer.transform(coord)
}

// These unit tests exercise std-only API (`Transformer`, `Crs::compound`,
// `lookup_epsg`, …), so they are gated with the `std` feature.
#[cfg(all(test, feature = "std"))]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_coordinate_creation() {
        let coord = Coordinate::new(10.0, 20.0);
        assert_eq!(coord.x, 10.0);
        assert_eq!(coord.y, 20.0);
    }

    #[test]
    fn test_coordinate_from_lon_lat() {
        let coord = Coordinate::from_lon_lat(-122.4194, 37.7749);
        assert_eq!(coord.lon(), -122.4194);
        assert_eq!(coord.lat(), 37.7749);
    }

    #[test]
    fn test_coordinate_validation() {
        let valid = Coordinate::new(0.0, 0.0);
        assert!(valid.validate_geographic().is_ok());

        let invalid_lon = Coordinate::new(200.0, 0.0);
        assert!(invalid_lon.validate_geographic().is_err());

        let invalid_lat = Coordinate::new(0.0, 100.0);
        assert!(invalid_lat.validate_geographic().is_err());
    }

    #[test]
    fn test_coordinate_is_valid() {
        let valid = Coordinate::new(1.0, 2.0);
        assert!(valid.is_valid());

        let invalid = Coordinate::new(f64::NAN, 2.0);
        assert!(!invalid.is_valid());

        let infinite = Coordinate::new(f64::INFINITY, 2.0);
        assert!(!infinite.is_valid());
    }

    #[test]
    fn test_coordinate3d() {
        let coord = Coordinate3D::new(1.0, 2.0, 3.0);
        assert_eq!(coord.x, 1.0);
        assert_eq!(coord.y, 2.0);
        assert_eq!(coord.z, 3.0);

        let coord_2d = coord.to_2d();
        assert_eq!(coord_2d.x, 1.0);
        assert_eq!(coord_2d.y, 2.0);
    }

    #[test]
    fn test_bounding_box() {
        let bbox = BoundingBox::new(0.0, 0.0, 10.0, 20.0);
        assert!(bbox.is_ok());

        let bbox = bbox.expect("should be valid");
        assert_eq!(bbox.width(), 10.0);
        assert_eq!(bbox.height(), 20.0);

        let center = bbox.center();
        assert_eq!(center.x, 5.0);
        assert_eq!(center.y, 10.0);
    }

    #[test]
    fn test_bounding_box_invalid() {
        let result = BoundingBox::new(10.0, 0.0, 0.0, 20.0);
        assert!(result.is_err());

        let result = BoundingBox::new(0.0, 20.0, 10.0, 0.0);
        assert!(result.is_err());
    }

    #[test]
    fn test_bounding_box_contains() {
        let bbox = BoundingBox::new(0.0, 0.0, 10.0, 10.0).expect("valid bbox");

        assert!(bbox.contains(&Coordinate::new(5.0, 5.0)));
        assert!(bbox.contains(&Coordinate::new(0.0, 0.0)));
        assert!(bbox.contains(&Coordinate::new(10.0, 10.0)));
        assert!(!bbox.contains(&Coordinate::new(-1.0, 5.0)));
        assert!(!bbox.contains(&Coordinate::new(5.0, 11.0)));
    }

    #[test]
    fn test_bounding_box_expand() {
        let mut bbox = BoundingBox::new(0.0, 0.0, 10.0, 10.0).expect("valid bbox");

        bbox.expand_to_include(&Coordinate::new(15.0, 5.0));
        assert_eq!(bbox.max_x, 15.0);

        bbox.expand_to_include(&Coordinate::new(5.0, -5.0));
        assert_eq!(bbox.min_y, -5.0);
    }

    #[test]
    fn test_transformer_same_crs() {
        let wgs84 = Crs::wgs84();
        let transformer = Transformer::new(wgs84.clone(), wgs84.clone());
        assert!(transformer.is_ok());

        let transformer = transformer.expect("should create transformer");
        let coord = Coordinate::new(10.0, 20.0);
        let result = transformer.transform(&coord);
        assert!(result.is_ok());

        let result = result.expect("should transform");
        assert_eq!(result, coord);
    }

    #[test]
    fn test_transformer_wgs84_to_web_mercator() {
        let transformer = Transformer::from_epsg(4326, 3857);
        assert!(transformer.is_ok());

        let transformer = transformer.expect("should create transformer");

        // Transform London coordinates (0.0, 51.5)
        let london = Coordinate::from_lon_lat(0.0, 51.5);
        let result = transformer.transform(&london);
        assert!(result.is_ok());

        let result = result.expect("should transform");
        // Web Mercator should give us meters from equator
        // X should be close to 0 (prime meridian)
        assert_relative_eq!(result.x, 0.0, epsilon = 1.0);
        // Y should be positive (northern hemisphere)
        assert!(result.y > 6_000_000.0 && result.y < 7_000_000.0);
    }

    #[test]
    fn test_transform_batch() {
        let transformer = Transformer::from_epsg(4326, 4326).expect("same CRS");

        let coords = vec![
            Coordinate::new(0.0, 0.0),
            Coordinate::new(10.0, 10.0),
            Coordinate::new(20.0, 20.0),
        ];

        let result = transformer.transform_batch(&coords);
        assert!(result.is_ok());

        let result = result.expect("should transform");
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], coords[0]);
        assert_eq!(result[1], coords[1]);
        assert_eq!(result[2], coords[2]);
    }

    #[test]
    fn test_transform_bbox() {
        let transformer = Transformer::from_epsg(4326, 4326).expect("same CRS");

        let bbox = BoundingBox::new(0.0, 0.0, 10.0, 10.0).expect("valid bbox");
        let result = transformer.transform_bbox(&bbox);
        assert!(result.is_ok());

        let result = result.expect("should transform");
        assert_eq!(result, bbox);
    }

    #[test]
    fn test_convenience_functions() {
        let wgs84 = Crs::wgs84();
        let coord = Coordinate::new(0.0, 0.0);

        let result = transform_coordinate(&coord, &wgs84, &wgs84);
        assert!(result.is_ok());
        assert_eq!(result.expect("should transform"), coord);

        let result = transform_epsg(&coord, 4326, 4326);
        assert!(result.is_ok());
        assert_eq!(result.expect("should transform"), coord);
    }

    /// Regression test for the `transform_epsg` reusable-Transformer cache:
    /// repeated calls for the same `(src, dst)` EPSG pair must all return
    /// bit-for-bit identical results — this is a pure performance refactor
    /// (route through `GLOBAL_TRANSFORMER_CACHE` instead of rebuilding a
    /// fresh `Transformer` on every call), not a behavior change.
    #[test]
    fn test_transform_epsg_repeated_calls_are_consistent() {
        let coord = Coordinate::new(139.7671, 35.6812);

        let first = transform_epsg(&coord, 4326, 32654).expect("first call");
        for _ in 0..25 {
            let repeat = transform_epsg(&coord, 4326, 32654).expect("repeat call");
            assert_eq!(
                repeat, first,
                "cached transform_epsg must be deterministic across calls"
            );
        }
    }

    /// `transform_epsg` (cached) must produce the exact same result as
    /// explicitly building a fresh [`Transformer`] via [`Transformer::from_epsg`]
    /// for the same EPSG pair — proving the cache reuse introduced no
    /// numerical drift relative to the uncached path.
    #[test]
    fn test_transform_epsg_matches_fresh_transformer_from_epsg() {
        let coord = Coordinate::new(-122.4194, 37.7749);

        let via_cache = transform_epsg(&coord, 4326, 32610).expect("cached path");
        let fresh_transformer = Transformer::from_epsg(4326, 32610).expect("fresh transformer");
        let via_fresh = fresh_transformer
            .transform(&coord)
            .expect("fresh transform");

        assert!(
            (via_cache.x - via_fresh.x).abs() < 1e-9,
            "x mismatch: cached={} fresh={}",
            via_cache.x,
            via_fresh.x
        );
        assert!(
            (via_cache.y - via_fresh.y).abs() < 1e-9,
            "y mismatch: cached={} fresh={}",
            via_cache.y,
            via_fresh.y
        );
    }

    /// The cache is keyed by `(src_epsg, dst_epsg)`: interleaving calls for
    /// several distinct EPSG pairs must not corrupt or cross-contaminate
    /// results for any one pair.
    #[test]
    fn test_transform_epsg_distinct_pairs_do_not_interfere() {
        let tokyo = Coordinate::new(139.7671, 35.6812);
        let sf = Coordinate::new(-122.4194, 37.7749);

        let tokyo_expected = transform_epsg(&tokyo, 4326, 32654).expect("tokyo baseline");
        let sf_expected = transform_epsg(&sf, 4326, 32610).expect("sf baseline");

        for _ in 0..10 {
            let tokyo_out = transform_epsg(&tokyo, 4326, 32654).expect("tokyo repeat");
            let sf_out = transform_epsg(&sf, 4326, 32610).expect("sf repeat");
            assert_eq!(tokyo_out, tokyo_expected);
            assert_eq!(sf_out, sf_expected);
        }
    }

    #[test]
    fn test_transform_invalid_coordinate() {
        let transformer = Transformer::from_epsg(4326, 3857).expect("should create");

        let invalid = Coordinate::new(f64::NAN, 0.0);
        let result = transformer.transform(&invalid);
        assert!(result.is_err());
    }

    // =========================================================================
    // Compound CRS transform_3d tests
    // =========================================================================

    /// Build a compound CRS using programmatic constructor.  Reused across several tests.
    ///
    /// Using `Crs::compound` with an EPSG-backed horizontal ensures the sub-transformer
    /// can obtain a PROJ string from the horizontal component.
    fn make_compound_wgs84_egm96() -> crate::crs::Crs {
        let horiz = Crs::wgs84(); // EPSG:4326
        let vert_wkt = r#"VERTCRS["EGM96 height",VDATUM["EGM96 geoid"],UNIT["metre",1]]"#;
        let vert = crate::crs::Crs::from_wkt(vert_wkt).expect("vert parse");
        crate::crs::Crs::compound(horiz, vert).expect("compound CRS should build")
    }

    #[test]
    fn test_transform_3d_compound_same_vertical_datum_passes_z_through() {
        // When source == target compound CRS (same vertical datum), z must come through unchanged.
        let crs = make_compound_wgs84_egm96();
        let transformer = Transformer::new(crs.clone(), crs).expect("same-CRS transformer");
        let input = Coordinate3D::new(13.4050, 52.5200, 34.567);
        let output = transformer
            .transform_3d(&input)
            .expect("transform should succeed");
        // Horizontal: same CRS → passthrough.
        assert!((output.x - input.x).abs() < 1e-9, "x should be unchanged");
        assert!((output.y - input.y).abs() < 1e-9, "y should be unchanged");
        // Vertical: same vertical datum → z unchanged.
        assert!(
            (output.z - input.z).abs() < 1e-9,
            "z should be passed through"
        );
    }

    #[test]
    fn test_transform_3d_compound_different_vertical_datum_silently_passes_through_when_no_geoid() {
        // Slice-14 W1 contract: when the vertical datums differ AND no geoid model
        // has been attached via `with_geoid`, `transform_3d` must silently let `z`
        // pass through unchanged (back-compat with pre-Slice-14 builds).  A hard
        // error is only emitted by the explicit `Error::geoid_not_available`
        // constructor or via callers that opt-in to a stricter validation path.
        let horiz = Crs::wgs84(); // EPSG:4326 — has a PROJ string

        let vert1_wkt = r#"VERTCRS["EGM96 height",VDATUM["EGM96 geoid"],UNIT["metre",1]]"#;
        let vert2_wkt = r#"VERTCRS["EGM2008 height",VDATUM["EGM2008 geoid"],UNIT["metre",1]]"#;

        let vert1 = Crs::from_wkt(vert1_wkt).expect("vert1 parse");
        let vert2 = Crs::from_wkt(vert2_wkt).expect("vert2 parse");

        let crs1 = Crs::compound(horiz.clone(), vert1).expect("compound crs1");
        let crs2 = Crs::compound(horiz, vert2).expect("compound crs2");

        let transformer = Transformer::new(crs1, crs2).expect("different-vertical transformer");
        let input = Coordinate3D::new(0.0, 51.5, 50.0);
        let output = transformer
            .transform_3d(&input)
            .expect("must succeed without geoid (silent passthrough)");
        assert!((output.x - input.x).abs() < 1e-9);
        assert!((output.y - input.y).abs() < 1e-9);
        assert!(
            (output.z - input.z).abs() < 1e-9,
            "z must pass through when no geoid attached"
        );
    }

    // Cross-thread reuse and vertical-datum-warning tests live in
    // `tests/transformer_grid_thread.rs` (public-API only) to keep this file
    // under the 2000-line refactoring limit.

    #[test]
    fn test_transform_3d_simple_non_compound_crs_unaffected() {
        // Ordinary EPSG-based transformer must still work exactly as before.
        let transformer = Transformer::from_epsg(4326, 4326).expect("same EPSG");
        let input = Coordinate3D::new(10.0, 50.0, 100.0);
        let output = transformer.transform_3d(&input).expect("should transform");
        assert!((output.x - input.x).abs() < 1e-9);
        assert!((output.y - input.y).abs() < 1e-9);
        assert!((output.z - input.z).abs() < 1e-9);
    }

    #[test]
    fn test_transform_3d_compound_to_horizontal_still_works() {
        // When only one side is Compound (non-compound target), the code should
        // fall through to the normal transform_impl path.
        // We just verify no panic / unexpected error for a same-EPSG pair.
        let non_compound = Crs::wgs84();
        let transformer = Transformer::new(non_compound.clone(), non_compound).expect("same CRS");
        let input = Coordinate3D::new(5.0, 45.0, 200.0);
        let output = transformer.transform_3d(&input).expect("should transform");
        assert!((output.x - input.x).abs() < 1e-9);
        assert!((output.y - input.y).abs() < 1e-9);
        assert!((output.z - input.z).abs() < 1e-9);
    }

    /// Regression test: an ordinary (non-compound, non-ITRF) cross-datum 3-D
    /// transform between two geographic CRS with a recognised named datum
    /// pair (NAD27 → WGS84) must adjust height consistently with the
    /// horizontal shift, instead of silently leaving `z` untouched.
    #[test]
    fn test_transform_3d_nad27_to_wgs84_adjusts_height() {
        // EPSG:4267 = NAD27 geographic, EPSG:4326 = WGS84 geographic.
        let transformer = Transformer::from_epsg(4267, 4326).expect("NAD27 -> WGS84");
        let input = Coordinate3D::new(-100.0, 40.0, 200.0);
        let output = transformer
            .transform_3d(&input)
            .expect("should transform NAD27 -> WGS84 3D");

        // The NAD27 CONUS Bursa-Wolf preset has translations on the order of
        // tens to hundreds of metres, so the resulting height must move by
        // a physically significant amount (bounded well below the ~200 m
        // scale of the translation itself, but unmistakably non-zero).
        assert!(
            (output.z - input.z).abs() > 1.0,
            "expected height to change measurably under NAD27 -> WGS84, got {} vs {}",
            output.z,
            input.z
        );
        // Horizontal position must also move (NAD27 and WGS84 differ).
        assert!(
            (output.x - input.x).abs() > 1e-6 || (output.y - input.y).abs() > 1e-6,
            "expected horizontal position to change under NAD27 -> WGS84"
        );
        assert!(output.is_valid());
    }

    /// The inverse direction (WGS84 → NAD27) must also apply the
    /// height-consistent shift (using the preset's `.inverse()`).
    #[test]
    fn test_transform_3d_wgs84_to_nad27_adjusts_height() {
        let transformer = Transformer::from_epsg(4326, 4267).expect("WGS84 -> NAD27");
        let input = Coordinate3D::new(-100.0, 40.0, 200.0);
        let output = transformer
            .transform_3d(&input)
            .expect("should transform WGS84 -> NAD27 3D");

        assert!(
            (output.z - input.z).abs() > 1.0,
            "expected height to change measurably under WGS84 -> NAD27, got {} vs {}",
            output.z,
            input.z
        );
        assert!(output.is_valid());
    }

    /// Round-tripping NAD27 -> WGS84 -> NAD27 through `transform_3d` must
    /// recover the original (lon, lat, height) to within the Bursa-Wolf
    /// linearisation + Bowring iteration error budget.
    #[test]
    fn test_transform_3d_nad27_wgs84_round_trip() {
        let fwd = Transformer::from_epsg(4267, 4326).expect("NAD27 -> WGS84");
        let inv = Transformer::from_epsg(4326, 4267).expect("WGS84 -> NAD27");

        let original = Coordinate3D::new(-100.0, 40.0, 200.0);
        let transformed = fwd.transform_3d(&original).expect("forward NAD27 -> WGS84");
        let recovered = inv
            .transform_3d(&transformed)
            .expect("inverse WGS84 -> NAD27");

        assert!((recovered.x - original.x).abs() < 1e-6);
        assert!((recovered.y - original.y).abs() < 1e-6);
        assert!((recovered.z - original.z).abs() < 1e-2);
    }

    /// A datum pair with no known Bursa-Wolf preset (e.g. WGS84 ->
    /// EPSG:4283, GDA94 — no preset registered) must keep the documented
    /// passthrough behavior: `z` unchanged. This is the intentional
    /// remaining scope of the limitation, not a regression.
    #[test]
    fn test_transform_3d_unknown_datum_pair_still_passes_z_through() {
        let transformer = Transformer::from_epsg(4326, 4283).expect("WGS84 -> GDA94");
        let input = Coordinate3D::new(140.0, -30.0, 123.456);
        let output = transformer
            .transform_3d(&input)
            .expect("should transform WGS84 -> GDA94 3D");
        assert!(
            (output.z - input.z).abs() < 1e-9,
            "z must still pass through for datum pairs without a known preset"
        );
    }
}
