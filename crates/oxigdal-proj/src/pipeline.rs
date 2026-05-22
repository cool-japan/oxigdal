//! Coordinate operation pipeline — chains multiple projection steps.
//!
//! This module mirrors the PROJ `+proj=pipeline +step ...` syntax, allowing
//! sequences of axis-swap, unit-conversion, and projection operations to be
//! composed and executed in order (or reversed).
//!
//! # Example
//!
//! ```
//! use oxigdal_proj::{Pipeline, StepKind, Unit, Coordinate};
//!
//! let pipeline = Pipeline::new()
//!     .step(StepKind::AxisSwap { order: [2, 1, 0, 0] })
//!     .step(StepKind::UnitConvert { from: Unit::M, to: Unit::Km });
//!
//! let coord = Coordinate::new(1000.0, 2000.0);
//! let result = pipeline.transform(&coord).unwrap();
//! // After axisswap: (2000.0, 1000.0); after m->km: (2.0, 1.0)
//! assert!((result.x - 2.0).abs() < 1e-10);
//! assert!((result.y - 1.0).abs() < 1e-10);
//! ```

use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::proj_string::ProjString;
use crate::transform::Coordinate;

/// Type alias for a single parsed pipeline step: `(params map, per-step-inverse flag)`.
pub type ParsedStep = (HashMap<String, Option<String>>, bool);

// ---------------------------------------------------------------------------
// Unit enum
// ---------------------------------------------------------------------------

/// A measurement unit for coordinate values.
///
/// Used in [`StepKind::UnitConvert`] to specify input/output units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    /// Metres (base SI unit for length)
    M,
    /// Kilometres (1000 metres)
    Km,
    /// International feet (0.3048 metres)
    Ft,
    /// US survey feet (0.30480060960121924 metres)
    USFt,
    /// Radians (base SI unit for angle)
    Rad,
    /// Degrees (π/180 radians)
    Deg,
}

impl Unit {
    /// Returns the number of metres per one unit, for length units.
    ///
    /// Panics (well, returns 1.0) for angular units — use [`Self::to_radians_per_unit`]
    /// for those.
    pub fn to_meters_per_unit(self) -> f64 {
        match self {
            Self::M => 1.0,
            Self::Km => 1_000.0,
            Self::Ft => 0.3048,
            Self::USFt => 0.304_800_609_601_219_24,
            // Angular units: return 1.0 as sentinel; callers should dispatch via is_angular()
            Self::Rad | Self::Deg => 1.0,
        }
    }

    /// Returns the number of radians per one unit, for angular units.
    ///
    /// Returns 1.0 for linear units (sentinel).
    pub fn to_radians_per_unit(self) -> f64 {
        match self {
            Self::Rad => 1.0,
            Self::Deg => std::f64::consts::PI / 180.0,
            // Linear units: return 1.0 as sentinel
            Self::M | Self::Km | Self::Ft | Self::USFt => 1.0,
        }
    }

    /// Returns `true` if this is an angular unit (Rad or Deg).
    pub fn is_angular(self) -> bool {
        matches!(self, Self::Rad | Self::Deg)
    }

    /// Parse a unit from a PROJ parameter string (e.g. `"m"`, `"km"`, `"ft"`, `"us-ft"`, `"rad"`, `"deg"`).
    pub fn from_proj_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "m" | "metre" | "meters" | "metres" => Some(Self::M),
            "km" | "kilometre" | "kilometer" | "kilometres" | "kilometers" => Some(Self::Km),
            "ft" | "foot" | "feet" | "intl ft" => Some(Self::Ft),
            "us-ft" | "us_ft" | "survey ft" | "survey_ft" | "us survey foot" => Some(Self::USFt),
            "rad" | "radian" | "radians" => Some(Self::Rad),
            "deg" | "degree" | "degrees" => Some(Self::Deg),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Helmert transform types
// ---------------------------------------------------------------------------

/// Convention for the Helmert 7-parameter similarity transform.
///
/// The two conventions differ in the sign applied to the rotation parameters.
#[derive(Debug, Clone, PartialEq)]
pub enum HelmertConvention {
    /// Position Vector (Bursa-Wolf) convention — used by IERS.
    PositionVector,
    /// Coordinate Frame rotation convention — common in surveying.
    CoordinateFrame,
}

/// Parameters for a 7-parameter Helmert similarity transform.
///
/// Translations are in metres; rotations are in arcseconds; scale is in ppm.
#[derive(Debug, Clone, PartialEq)]
pub struct HelmertParams {
    /// Translation along X axis (m).
    pub tx: f64,
    /// Translation along Y axis (m).
    pub ty: f64,
    /// Translation along Z axis (m).
    pub tz: f64,
    /// Rotation about X axis (arcseconds).
    pub rx: f64,
    /// Rotation about Y axis (arcseconds).
    pub ry: f64,
    /// Rotation about Z axis (arcseconds).
    pub rz: f64,
    /// Scale factor difference (parts per million).
    pub s: f64,
    /// Rotation sign convention.
    pub convention: HelmertConvention,
}

/// Time-rate parameters for a dynamic (time-dependent) Helmert transform.
///
/// Each field represents the annual drift rate of the corresponding Helmert
/// parameter.  Units follow IERS convention:
/// - `dtx/dty/dtz` — translation rates in mm/yr (divide by 1000 for m/yr)
/// - `drx/dry/drz` — rotation rates in milli-arcseconds/yr (mas/yr)
///   (multiply by π/648_000_000 for rad/yr)
/// - `ds`          — scale rate in ppb/yr (divide by 1000 for ppm/yr)
/// - `ref_epoch`   — reference epoch at which `HelmertParams` values are valid
///   (decimal year, e.g. 2010.0)
#[derive(Debug, Clone, PartialEq)]
pub struct HelmertRateParams {
    /// Translation rate along X axis (mm/yr).
    pub dtx: f64,
    /// Translation rate along Y axis (mm/yr).
    pub dty: f64,
    /// Translation rate along Z axis (mm/yr).
    pub dtz: f64,
    /// Rotation rate about X axis (mas/yr).
    pub drx: f64,
    /// Rotation rate about Y axis (mas/yr).
    pub dry: f64,
    /// Rotation rate about Z axis (mas/yr).
    pub drz: f64,
    /// Scale rate (ppb/yr — parts-per-billion per year).
    pub ds: f64,
    /// Reference epoch for the `HelmertParams` values (decimal year).
    pub ref_epoch: f64,
}

// ---------------------------------------------------------------------------
// Cartographic (ECEF) conversion types
// ---------------------------------------------------------------------------

/// Ellipsoid parameters for geographic ↔ geocentric (ECEF) conversion.
///
/// Defined by semi-major axis `a` (metres) and flattening `f`.
#[derive(Debug, Clone, PartialEq)]
pub struct EllipsoidParams {
    /// Semi-major axis in metres.
    pub a: f64,
    /// Flattening  f = (a − b) / a.
    pub f: f64,
}

impl EllipsoidParams {
    /// Semi-minor axis  b = a · (1 − f).
    #[inline]
    pub fn b(&self) -> f64 {
        self.a * (1.0 - self.f)
    }

    /// First eccentricity squared  e² = 1 − (b/a)².
    #[inline]
    pub fn e2(&self) -> f64 {
        let b = self.b();
        1.0 - (b / self.a) * (b / self.a)
    }

    /// Prime vertical radius of curvature  N(φ) = a / √(1 − e²·sin²φ).
    #[inline]
    pub fn prime_vertical_radius(&self, lat_rad: f64) -> f64 {
        let e2 = self.e2();
        let sin_lat = lat_rad.sin();
        self.a / (1.0 - e2 * sin_lat * sin_lat).sqrt()
    }
}

// ---------------------------------------------------------------------------
// Grid shift direction
// ---------------------------------------------------------------------------

/// Direction of a grid-based shift operation.
#[derive(Debug, Clone, PartialEq)]
pub enum ShiftDirection {
    /// Apply the grid shift in the forward direction.
    Forward,
    /// Apply the grid shift in the inverse direction.
    Inverse,
}

// ---------------------------------------------------------------------------
// StepKind enum
// ---------------------------------------------------------------------------

/// Describes the operation performed by a single pipeline step.
#[derive(Debug, Clone)]
pub enum StepKind {
    /// Reorder coordinate axes.
    ///
    /// `order` is a 1-indexed array of up to four axis positions.
    /// Trailing zeros indicate unused positions.
    ///
    /// Example: `order = [2, 1, 0, 0]` swaps X and Y.
    AxisSwap {
        /// 1-indexed axis reordering (zeros are unused).
        order: [u8; 4],
    },

    /// Convert coordinate values between units.
    ///
    /// The scale factor applied is `from.to_meters_per_unit() / to.to_meters_per_unit()`
    /// for length units, or `from.to_radians_per_unit() / to.to_radians_per_unit()` for
    /// angular units.
    UnitConvert {
        /// Input unit.
        from: Unit,
        /// Output unit.
        to: Unit,
    },

    /// Apply a projection using a PROJ string (delegates to proj4rs).
    Project {
        /// PROJ string for the projection step (e.g. `"+proj=merc +R=6378137"`).
        proj_string: String,
    },

    /// Pass coordinates through unchanged.
    Passthrough,

    /// 7-parameter Helmert similarity transform (Bursa-Wolf or Coordinate Frame convention).
    ///
    /// Operates on geocentric (X, Y, Z) Cartesian coordinates.  For a 2-D pipeline the
    /// step treats the coordinate as (X, Y) with Z = 0; this is sufficient for testing
    /// the linearised rotation/translation arithmetic.
    Helmert {
        /// Helmert transform parameters.
        params: HelmertParams,
    },

    /// Geographic ↔ geocentric (ECEF) conversion using a specified ellipsoid.
    ///
    /// Forward: geographic (lon°, lat°) → ECEF (X, Y).
    ///   Z is implicit (height = 0); the pipeline Coordinate carries (X_ecef, Y_ecef).
    /// Inverse: ECEF (X, Y) → geographic (lon°, lat°) via Bowring 1985 iteration.
    Cart {
        /// Ellipsoid parameters for the conversion.
        ellipsoid: EllipsoidParams,
    },

    /// Vertical grid shift (e.g. ellipsoidal ↔ orthometric height).
    ///
    /// Full implementation is deferred — grid loading infrastructure is not yet available.
    /// The parser accepts and round-trips the step without panicking; applying it returns
    /// an error indicating the grid is not loaded.
    Vgridshift {
        /// Grid file identifier (e.g. `"egm96_15.gtx"`).
        grid_id: String,
        /// Direction of the shift.
        direction: ShiftDirection,
    },

    /// Horizontal NTv2 grid shift.
    ///
    /// Full implementation deferred — NTv2 grid parser available in `grid_shift` mod.
    /// The parser accepts and round-trips the step without panicking; applying it returns
    /// an error indicating the grid is not loaded.
    Hgridshift {
        /// Grid file identifier (e.g. `"ntv2_0.gsb"`).
        grid_id: String,
        /// Direction of the shift.
        direction: ShiftDirection,
    },

    /// Time-dependent (dynamic) 7-parameter Helmert similarity transform.
    ///
    /// Evaluates a Helmert transform at a specific target epoch by linearly
    /// extrapolating each parameter from its value at `rates.ref_epoch` using
    /// the supplied annual drift rates.  The effective parameters at `epoch` are:
    ///
    /// ```text
    /// tx_eff = params.tx + rates.dtx / 1000  × (epoch − ref_epoch)   [m]
    /// rz_eff = params.rz + rates.drz × π/648_000_000 × (epoch − ref_epoch)  [rad arcsec equiv]
    /// s_eff  = params.s  + rates.ds  / 1000  × (epoch − ref_epoch)   [ppm]
    /// ```
    ///
    /// The effective parameters are then forwarded to the same `apply_helmert`
    /// function used by the static `Helmert` variant.
    HelmertTemporal {
        /// Static Helmert parameters valid at `rates.ref_epoch`.
        params: HelmertParams,
        /// Annual drift rates for each Helmert parameter.
        rates: HelmertRateParams,
        /// Target evaluation epoch (decimal year).
        epoch: f64,
    },
}

// ---------------------------------------------------------------------------
// PipelineStep
// ---------------------------------------------------------------------------

/// A single step in a coordinate transformation pipeline.
#[derive(Debug, Clone)]
pub struct PipelineStep {
    /// The kind of operation this step performs.
    pub kind: StepKind,
    /// If `true`, the step is executed in its inverse direction.
    pub inverse: bool,
}

impl PipelineStep {
    /// Creates a forward step.
    pub fn new(kind: StepKind) -> Self {
        Self {
            kind,
            inverse: false,
        }
    }

    /// Creates an inverse step.
    pub fn new_inverse(kind: StepKind) -> Self {
        Self {
            kind,
            inverse: true,
        }
    }

    /// Apply this step to a coordinate, respecting the `inverse` flag.
    pub fn apply(&self, coord: &Coordinate) -> Result<Coordinate> {
        apply_step_kind(&self.kind, coord, self.inverse)
    }

    /// Apply this step to a coordinate with an additional inversion on top of `self.inverse`.
    pub fn apply_flipped(&self, coord: &Coordinate) -> Result<Coordinate> {
        apply_step_kind(&self.kind, coord, !self.inverse)
    }
}

// ---------------------------------------------------------------------------
// Pipeline
// ---------------------------------------------------------------------------

/// A chained sequence of coordinate transformation steps.
///
/// Steps are applied in the order they were added (or in reverse when the
/// pipeline itself has `inverse = true`).
///
/// # Building a pipeline
///
/// ```
/// use oxigdal_proj::{Pipeline, StepKind, Unit};
///
/// let p = Pipeline::new()
///     .step(StepKind::AxisSwap { order: [2, 1, 0, 0] })
///     .step(StepKind::UnitConvert { from: Unit::M, to: Unit::Km });
/// ```
///
/// # Parsing from a PROJ pipeline string
///
/// ```
/// use oxigdal_proj::Pipeline;
///
/// let s = "+proj=pipeline +step +proj=axisswap +order=2,1 +step +proj=axisswap +order=2,1";
/// let p = Pipeline::from_proj_string(s).unwrap();
/// ```
#[derive(Debug, Clone)]
pub struct Pipeline {
    steps: Vec<PipelineStep>,
    /// If `true`, steps are executed in reverse order and each step's
    /// `inverse` flag is logically flipped.
    inverse: bool,
}

impl Default for Pipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Pipeline {
    /// Creates an empty forward pipeline.
    pub fn new() -> Self {
        Self {
            steps: Vec::new(),
            inverse: false,
        }
    }

    /// Appends a forward step to the pipeline (builder method).
    pub fn step(mut self, kind: StepKind) -> Self {
        self.steps.push(PipelineStep::new(kind));
        self
    }

    /// Appends an inverse step to the pipeline (builder method).
    pub fn step_inv(mut self, kind: StepKind) -> Self {
        self.steps.push(PipelineStep::new_inverse(kind));
        self
    }

    /// Sets the global `inverse` flag (builder method).
    ///
    /// When `inv = true`, `transform` will iterate steps in reverse order and
    /// logically flip each step's own `inverse` flag.
    pub fn with_inverse(mut self, inv: bool) -> Self {
        self.inverse = inv;
        self
    }

    /// Returns the number of steps in this pipeline.
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Returns `true` if the pipeline has no steps.
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Returns whether the global inverse flag is set.
    pub fn is_inverse(&self) -> bool {
        self.inverse
    }

    // -----------------------------------------------------------------------
    // Transform
    // -----------------------------------------------------------------------

    /// Transforms a single coordinate through the pipeline.
    ///
    /// If `self.inverse` is set, steps are iterated in reverse order and each
    /// step's direction is logically flipped.
    ///
    /// # Errors
    ///
    /// Returns [`Error::PipelineStepError`] if any step fails, wrapping the
    /// step index and inner error message.
    pub fn transform(&self, coord: &Coordinate) -> Result<Coordinate> {
        let mut current = *coord;

        if self.inverse {
            // Reverse iteration, flip each step's direction
            for (idx, step) in self.steps.iter().enumerate().rev() {
                current = step
                    .apply_flipped(&current)
                    .map_err(|e| Error::PipelineStepError {
                        step: idx,
                        inner: format!("{}", e),
                    })?;
            }
        } else {
            for (idx, step) in self.steps.iter().enumerate() {
                current = step.apply(&current).map_err(|e| Error::PipelineStepError {
                    step: idx,
                    inner: format!("{}", e),
                })?;
            }
        }

        Ok(current)
    }

    /// Transforms a slice of coordinates through the pipeline.
    ///
    /// # Errors
    ///
    /// Returns the first error encountered, if any.
    pub fn transform_many(&self, coords: &[Coordinate]) -> Result<Vec<Coordinate>> {
        coords.iter().map(|c| self.transform(c)).collect()
    }

    // -----------------------------------------------------------------------
    // Parsing
    // -----------------------------------------------------------------------

    /// Parse a PROJ pipeline string into a `Pipeline`.
    ///
    /// The input must start with `+proj=pipeline`.  Steps are delimited by
    /// `+step` tokens.  Each step chunk is parsed via [`ProjString::parse`]
    /// and then classified by its `+proj=` value.
    ///
    /// # Errors
    ///
    /// Returns [`Error::PipelineParseError`] if the string is not a valid
    /// pipeline, or if a step cannot be parsed.
    pub fn from_proj_string(s: &str) -> Result<Self> {
        let step_chunks = parse_pipeline(s)?;

        // First chunk describes the pipeline itself; determine global +inv.
        // parse_pipeline already consumed it and returned per-step chunks.
        // The global inverse flag was extracted during parse.
        // We need to re-run the detection here.
        let global_inv = detect_global_inv(s);

        let mut pipeline = Self::new().with_inverse(global_inv);

        for (idx, (params, step_inv)) in step_chunks.into_iter().enumerate() {
            let kind = params_to_step_kind(params, idx)?;
            if step_inv {
                pipeline.steps.push(PipelineStep::new_inverse(kind));
            } else {
                pipeline.steps.push(PipelineStep::new(kind));
            }
        }

        Ok(pipeline)
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Detect whether the pipeline-level `+inv` flag is present in the header.
fn detect_global_inv(s: &str) -> bool {
    // Split off the part before the first +step
    let header = if let Some(pos) = s.find("+step") {
        &s[..pos]
    } else {
        s
    };
    // Look for bare "+inv" token in the header
    for token in header.split_whitespace() {
        if token == "+inv" {
            return true;
        }
    }
    false
}

/// Apply a `StepKind` to a coordinate, either forward or inverse.
fn apply_step_kind(kind: &StepKind, coord: &Coordinate, inverse: bool) -> Result<Coordinate> {
    match kind {
        StepKind::Passthrough => Ok(*coord),

        StepKind::AxisSwap { order } => apply_axis_swap(coord, order, inverse),

        StepKind::UnitConvert { from, to } => apply_unit_convert(coord, *from, *to, inverse),

        StepKind::Project { proj_string } => apply_project(coord, proj_string, inverse),

        StepKind::Helmert { params } => apply_helmert(coord, params, inverse),

        StepKind::Cart { ellipsoid } => apply_cart(coord, ellipsoid, inverse),

        StepKind::Vgridshift { grid_id, .. } => Err(Error::PipelineParseError(format!(
            "vgridshift: grid '{}' not loaded",
            grid_id
        ))),

        StepKind::Hgridshift { grid_id, .. } => {
            // Full implementation deferred — NTv2 grid parser available in grid_shift mod
            Err(Error::PipelineParseError(format!(
                "hgridshift: grid '{}' not loaded",
                grid_id
            )))
        }

        StepKind::HelmertTemporal {
            params,
            rates,
            epoch,
        } => apply_helmert_temporal(coord, params, rates, *epoch, inverse),
    }
}

/// Apply (or invert) an axis-swap step.
///
/// `order` is 1-indexed: `order[i] = k` means output axis `i+1` = input axis `k`.
/// A value of 0 means "unused" — that output axis keeps its original value.
///
/// Forward: reorder axes according to `order`.
/// Inverse: compute the reverse permutation and reorder.
fn apply_axis_swap(coord: &Coordinate, order: &[u8; 4], inverse: bool) -> Result<Coordinate> {
    // Collect the active (non-zero) entries.
    let active: Vec<u8> = order.iter().copied().filter(|&v| v != 0).collect();

    // We operate on a small array: [x, y] (only 2D in Coordinate).
    let input = [coord.x, coord.y];

    if active.is_empty() {
        return Ok(*coord);
    }

    let effective_order: Vec<u8> = if inverse {
        // Compute the inverse permutation of `active`.
        // `active[i] = k` → in forward direction output[i] = input[k-1].
        // For inverse: output[k-1] = input[i].
        let n = active.len();
        let mut inv_order = vec![0u8; n];
        for (i, &k) in active.iter().enumerate() {
            let ki = k as usize;
            if ki == 0 || ki > n {
                return Err(Error::PipelineParseError(format!(
                    "axis order value {} is out of range 1..={}",
                    k, n
                )));
            }
            inv_order[ki - 1] = (i + 1) as u8;
        }
        inv_order
    } else {
        active.clone()
    };

    // Build output by reading from input per effective_order.
    let n = effective_order.len().min(2);
    let mut out_x = coord.x;
    let mut out_y = coord.y;

    if n >= 1 {
        let src_idx = (effective_order[0] as usize).saturating_sub(1);
        if src_idx < input.len() {
            out_x = input[src_idx];
        }
    }
    if n >= 2 {
        let src_idx = (effective_order[1] as usize).saturating_sub(1);
        if src_idx < input.len() {
            out_y = input[src_idx];
        }
    }

    Ok(Coordinate::new(out_x, out_y))
}

/// Apply (or invert) a unit-conversion step.
fn apply_unit_convert(
    coord: &Coordinate,
    from: Unit,
    to: Unit,
    inverse: bool,
) -> Result<Coordinate> {
    // Determine whether we deal with angular or linear units.
    let (actual_from, actual_to) = if inverse { (to, from) } else { (from, to) };

    let scale = if actual_from.is_angular() || actual_to.is_angular() {
        // Angular conversion
        actual_from.to_radians_per_unit() / actual_to.to_radians_per_unit()
    } else {
        // Linear conversion
        actual_from.to_meters_per_unit() / actual_to.to_meters_per_unit()
    };

    Ok(Coordinate::new(coord.x * scale, coord.y * scale))
}

/// Apply (or invert) a projection step using proj4rs.
fn apply_project(coord: &Coordinate, proj_string: &str, inverse: bool) -> Result<Coordinate> {
    // For a Project step we need both a geographic (longlat) CRS and the
    // target projection CRS.  Forward: longlat → projected.
    // Inverse: projected → longlat.

    let geo_proj = proj4rs::Proj::from_proj_string("+proj=longlat +datum=WGS84")
        .map_err(|e| Error::PipelineParseError(format!("geo proj init failed: {:?}", e)))?;

    let step_proj = proj4rs::Proj::from_proj_string(proj_string)
        .map_err(|e| Error::PipelineParseError(format!("step proj init failed: {:?}", e)))?;

    let (src_proj, dst_proj, src_is_geo, dst_is_geo) = if inverse {
        // projected → geographic
        (&step_proj, &geo_proj, false, true)
    } else {
        // geographic → projected
        (&geo_proj, &step_proj, true, false)
    };

    let mut x = coord.x;
    let mut y = coord.y;

    if src_is_geo {
        x = x.to_radians();
        y = y.to_radians();
    }

    let mut points = [(x, y)];
    proj4rs::transform::transform(src_proj, dst_proj, &mut points[..])
        .map_err(|e| Error::transformation_error(format!("{:?}", e)))?;

    let (mut rx, mut ry) = points[0];

    if dst_is_geo {
        rx = rx.to_degrees();
        ry = ry.to_degrees();
    }

    let result = Coordinate::new(rx, ry);
    if !result.is_valid() {
        return Err(Error::transformation_error(
            "Project step produced non-finite coordinate",
        ));
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// Helmert 7-parameter similarity transform
// ---------------------------------------------------------------------------

/// Apply (or invert) a 7-parameter Helmert similarity transform.
///
/// The transform operates on Cartesian geocentric (X, Y, Z) coordinates.
/// In this 2-D pipeline, Z is taken as 0 on the way in; only (X, Y) are
/// returned.  This is intentional for testing the linear arithmetic.
///
/// # Position Vector convention (Bursa-Wolf)
///
/// ```text
/// X' = tx + (1+ds)·X + dRz·Y − dRy·Z
/// Y' = ty − dRz·X + (1+ds)·Y + dRx·Z
/// Z' = tz + dRy·X − dRx·Y + (1+ds)·Z
/// ```
///
/// where dRx/dRy/dRz are in radians and ds = s × 10⁻⁶.
///
/// # Coordinate Frame convention
///
/// Identical to Position Vector but with the signs of dRx, dRy, dRz negated
/// before applying.
///
/// # Inverse
///
/// Computed analytically from the 2×2 sub-system (Z=0 makes rx/ry terms
/// vanish), yielding an exact closed-form inverse.
fn apply_helmert(coord: &Coordinate, params: &HelmertParams, inverse: bool) -> Result<Coordinate> {
    // Arcseconds → radians: 1 arcsec = π / 648 000 rad.
    let arc_sec_to_rad = std::f64::consts::PI / 648_000.0;

    // Raw rotation parameters in radians.
    let drx_raw = params.rx * arc_sec_to_rad;
    let dry_raw = params.ry * arc_sec_to_rad;
    let drz_raw = params.rz * arc_sec_to_rad;

    // Coordinate Frame convention: negate all rotations before applying.
    let (drx, dry, drz) = if params.convention == HelmertConvention::CoordinateFrame {
        (-drx_raw, -dry_raw, -drz_raw)
    } else {
        (drx_raw, dry_raw, drz_raw)
    };

    // drx / dry only appear in Z-dependent terms; with Z=0 they cancel out.
    // Bind them to suppress the unused-variable warning.
    let _drx = drx;
    let _dry = dry;

    // Scale factor from ppm.
    let ds = params.s * 1.0e-6;
    let scale = 1.0 + ds;

    if !inverse {
        // Forward transform (Z=0 sub-system):
        //   X' = tx + scale·X + drz·Y
        //   Y' = ty − drz·X + scale·Y
        let x = coord.x;
        let y = coord.y;

        let out_x = params.tx + scale * x + drz * y;
        let out_y = params.ty - drz * x + scale * y;

        Ok(Coordinate::new(out_x, out_y))
    } else {
        // Exact closed-form inverse of the 2×2 forward system.
        //
        // From:
        //   X' = tx + scale·X + drz·Y   →   ΔX = X'−tx = scale·X + drz·Y
        //   Y' = ty − drz·X + scale·Y   →   ΔY = Y'−ty = −drz·X + scale·Y
        //
        // Solving:
        //   det = scale² + drz²
        //   X = (scale·ΔX − drz·ΔY) / det
        //   Y = (drz·ΔX  + scale·ΔY) / det
        let dx = coord.x - params.tx;
        let dy = coord.y - params.ty;
        let det = scale * scale + drz * drz;

        if det.abs() < 1e-30 {
            return Err(Error::transformation_error(
                "Helmert inverse: degenerate scale/rotation combination",
            ));
        }

        let out_x = (scale * dx - drz * dy) / det;
        let out_y = (drz * dx + scale * dy) / det;

        Ok(Coordinate::new(out_x, out_y))
    }
}

// ---------------------------------------------------------------------------
// Time-dependent Helmert transform
// ---------------------------------------------------------------------------

/// Apply (or invert) a time-dependent 7-parameter Helmert transform.
///
/// The method linearly extrapolates each Helmert parameter from its reference
/// value (`params`) at `rates.ref_epoch` to the requested `epoch` using the
/// corresponding annual drift rate from `rates`.  Unit conversions applied
/// before the linear extrapolation:
///
/// - `dtx/dty/dtz` : mm/yr  → m/yr  (÷ 1000)
/// - `drx/dry/drz` : mas/yr → rad-equivalent arcseconds/yr; because
///   `HelmertParams.rx/ry/rz` are stored in arcseconds we instead convert
///   mas/yr → arcsec/yr (÷ 1000) so the addition is dimensionally consistent.
/// - `ds`           : ppb/yr → ppm/yr (÷ 1000)
///
/// The resulting effective `HelmertParams` are then forwarded to
/// [`apply_helmert`], reusing the full Bursa-Wolf / Coordinate-Frame logic.
fn apply_helmert_temporal(
    coord: &Coordinate,
    params: &HelmertParams,
    rates: &HelmertRateParams,
    epoch: f64,
    inverse: bool,
) -> Result<Coordinate> {
    // dt: signed elapsed years from reference epoch to evaluation epoch.
    let dt = epoch - rates.ref_epoch;

    // Translate rates to the same units as the static params:
    //   translations  : mm/yr  → m/yr  via ÷ 1000
    //   rotations     : mas/yr → arcsec/yr via ÷ 1000
    //     (HelmertParams.rx/ry/rz are in arcseconds, so this keeps units consistent)
    //   scale         : ppb/yr → ppm/yr via ÷ 1000
    let effective = HelmertParams {
        tx: params.tx + rates.dtx / 1_000.0 * dt,
        ty: params.ty + rates.dty / 1_000.0 * dt,
        tz: params.tz + rates.dtz / 1_000.0 * dt,
        rx: params.rx + rates.drx / 1_000.0 * dt,
        ry: params.ry + rates.dry / 1_000.0 * dt,
        rz: params.rz + rates.drz / 1_000.0 * dt,
        s: params.s + rates.ds / 1_000.0 * dt,
        convention: params.convention.clone(),
    };

    apply_helmert(coord, &effective, inverse)
}

// ---------------------------------------------------------------------------
// Cartographic (ECEF) conversion
// ---------------------------------------------------------------------------

/// Apply (or invert) a geographic ↔ ECEF conversion.
///
/// # Forward direction: geographic → ECEF
///
/// Input: (lon_deg, lat_deg).  Height is assumed 0 m.
/// Output: (X_ecef, Y_ecef).  Z_ecef is discarded (2-D pipeline limitation).
///
/// # Inverse direction: ECEF → geographic
///
/// Input: (X_ecef, Y_ecef).  Z_ecef is assumed 0.
/// Output: (lon_deg, lat_deg).
///
/// Uses Bowring (1985) iterative method — 3 iterations give millimetre accuracy.
fn apply_cart(coord: &Coordinate, ell: &EllipsoidParams, inverse: bool) -> Result<Coordinate> {
    if !inverse {
        // Forward: geographic (deg) → ECEF (m).
        let lon_rad = coord.x.to_radians();
        let lat_rad = coord.y.to_radians();
        let h = 0.0_f64; // height above ellipsoid (assumed zero)

        let n = ell.prime_vertical_radius(lat_rad);
        let e2 = ell.e2();

        let cos_lat = lat_rad.cos();
        let sin_lat = lat_rad.sin();

        let x_ecef = (n + h) * cos_lat * lon_rad.cos();
        let y_ecef = (n + h) * cos_lat * lon_rad.sin();
        // Z_ecef is correct but is dropped from the 2-D Coordinate:
        //   z_ecef = (n * (1.0 - e2) + h) * sin_lat
        let _z_ecef = (n * (1.0 - e2) + h) * sin_lat;

        if !x_ecef.is_finite() || !y_ecef.is_finite() {
            return Err(Error::transformation_error(
                "Cart forward produced non-finite value",
            ));
        }

        Ok(Coordinate::new(x_ecef, y_ecef))
    } else {
        // Inverse: ECEF (m) → geographic (deg), via Bowring 1985 iteration.
        // Treat incoming coord as (X, Y); Z = 0.
        let x_ecef = coord.x;
        let y_ecef = coord.y;
        let z_ecef = 0.0_f64;

        let a = ell.a;
        let b = ell.b();
        let e2 = ell.e2();
        let ep2 = (a * a - b * b) / (b * b); // e'² second eccentricity squared

        let p = (x_ecef * x_ecef + y_ecef * y_ecef).sqrt();
        let lon_rad = y_ecef.atan2(x_ecef);

        // Initial approximation (Bowring 1985).
        let theta = (z_ecef * a).atan2(p * b);
        let sin_theta = theta.sin();
        let cos_theta = theta.cos();
        let mut lat_rad = (z_ecef + ep2 * b * sin_theta * sin_theta * sin_theta)
            .atan2(p - e2 * a * cos_theta * cos_theta * cos_theta);

        // Iterate 3 times for millimetre accuracy.
        for _ in 0..3 {
            let sin_lat = lat_rad.sin();
            let n = a / (1.0 - e2 * sin_lat * sin_lat).sqrt();
            lat_rad = (z_ecef + e2 * n * sin_lat).atan2(p);
        }

        if !lon_rad.is_finite() || !lat_rad.is_finite() {
            return Err(Error::transformation_error(
                "Cart inverse (Bowring) produced non-finite value",
            ));
        }

        Ok(Coordinate::new(lon_rad.to_degrees(), lat_rad.to_degrees()))
    }
}

// ---------------------------------------------------------------------------
// Ellipsoid lookup table
// ---------------------------------------------------------------------------

/// Look up a well-known ellipsoid by its PROJ identifier string.
///
/// Returns `None` for unknown identifiers.
fn ellipsoid_from_name(name: &str) -> Option<EllipsoidParams> {
    match name.to_lowercase().as_str() {
        "wgs84" | "wgs_84" => Some(EllipsoidParams {
            a: 6_378_137.0,
            f: 1.0 / 298.257_223_563,
        }),
        "grs80" | "grs_80" => Some(EllipsoidParams {
            a: 6_378_137.0,
            f: 1.0 / 298.257_222_101,
        }),
        "bessel" => Some(EllipsoidParams {
            a: 6_377_397.155,
            f: 1.0 / 299.152_843_4,
        }),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Step-kind parsers
// ---------------------------------------------------------------------------

/// Parse a `+proj=helmert` step from its parameter map.
fn parse_helmert_step(
    params: HashMap<String, Option<String>>,
    step_idx: usize,
) -> Result<StepKind> {
    let parse_f64 = |key: &str, default: f64| -> Result<f64> {
        match params.get(key).and_then(|v| v.as_deref()) {
            None => Ok(default),
            Some(s) => s.parse::<f64>().map_err(|_| {
                Error::PipelineParseError(format!(
                    "step {}: helmert +{} '{}' is not a valid f64",
                    step_idx, key, s
                ))
            }),
        }
    };

    let tx = parse_f64("x", 0.0)?;
    let ty = parse_f64("y", 0.0)?;
    let tz = parse_f64("z", 0.0)?;
    let rx = parse_f64("rx", 0.0)?;
    let ry = parse_f64("ry", 0.0)?;
    let rz = parse_f64("rz", 0.0)?;
    let s = parse_f64("s", 0.0)?;

    let convention_str = params
        .get("convention")
        .and_then(|v| v.as_deref())
        .unwrap_or("position_vector");

    let convention = match convention_str.to_lowercase().as_str() {
        "position_vector" | "bursa_wolf" | "bursawolf" => HelmertConvention::PositionVector,
        "coordinate_frame" | "coordinateframe" => HelmertConvention::CoordinateFrame,
        other => {
            return Err(Error::PipelineParseError(format!(
                "step {}: helmert unknown +convention '{}'",
                step_idx, other
            )));
        }
    };

    Ok(StepKind::Helmert {
        params: HelmertParams {
            tx,
            ty,
            tz,
            rx,
            ry,
            rz,
            s,
            convention,
        },
    })
}

/// Parse a `+proj=helmert_temporal` step from its parameter map.
///
/// Expects the static Helmert parameters (`+x=`, `+y=`, `+z=`, `+rx=`,
/// `+ry=`, `+rz=`, `+s=`, `+convention=`) plus rate parameters
/// (`+dtx=`, `+dty=`, `+dtz=`, `+drx=`, `+dry=`, `+drz=`, `+ds=`,
/// `+ref_epoch=`) and the evaluation epoch (`+t_epoch=`).
fn parse_helmert_temporal_step(
    params: HashMap<String, Option<String>>,
    step_idx: usize,
) -> Result<StepKind> {
    let parse_f64 = |key: &str, default: f64| -> Result<f64> {
        match params.get(key).and_then(|v| v.as_deref()) {
            None => Ok(default),
            Some(s) => s.parse::<f64>().map_err(|_| {
                Error::PipelineParseError(format!(
                    "step {}: helmert_temporal +{} '{}' is not a valid f64",
                    step_idx, key, s
                ))
            }),
        }
    };

    // Static parameters at ref_epoch (same as helmert step).
    let tx = parse_f64("x", 0.0)?;
    let ty = parse_f64("y", 0.0)?;
    let tz = parse_f64("z", 0.0)?;
    let rx = parse_f64("rx", 0.0)?;
    let ry = parse_f64("ry", 0.0)?;
    let rz = parse_f64("rz", 0.0)?;
    let s = parse_f64("s", 0.0)?;

    let convention_str = params
        .get("convention")
        .and_then(|v| v.as_deref())
        .unwrap_or("position_vector");

    let convention = match convention_str.to_lowercase().as_str() {
        "position_vector" | "bursa_wolf" | "bursawolf" => HelmertConvention::PositionVector,
        "coordinate_frame" | "coordinateframe" => HelmertConvention::CoordinateFrame,
        other => {
            return Err(Error::PipelineParseError(format!(
                "step {}: helmert_temporal unknown +convention '{}'",
                step_idx, other
            )));
        }
    };

    // Rate parameters.
    let dtx = parse_f64("dtx", 0.0)?;
    let dty = parse_f64("dty", 0.0)?;
    let dtz = parse_f64("dtz", 0.0)?;
    let drx = parse_f64("drx", 0.0)?;
    let dry = parse_f64("dry", 0.0)?;
    let drz = parse_f64("drz", 0.0)?;
    let ds_rate = parse_f64("ds", 0.0)?;
    let ref_epoch = parse_f64("ref_epoch", 0.0)?;

    // Evaluation epoch (required for meaningful time-dependent transforms).
    let epoch = parse_f64("t_epoch", ref_epoch)?;

    Ok(StepKind::HelmertTemporal {
        params: HelmertParams {
            tx,
            ty,
            tz,
            rx,
            ry,
            rz,
            s,
            convention,
        },
        rates: HelmertRateParams {
            dtx,
            dty,
            dtz,
            drx,
            dry,
            drz,
            ds: ds_rate,
            ref_epoch,
        },
        epoch,
    })
}

/// Parse a `+proj=cart` step from its parameter map.
fn parse_cart_step(params: HashMap<String, Option<String>>, step_idx: usize) -> Result<StepKind> {
    // Prefer explicit +a= and +rf= if given.
    let a_opt = params
        .get("a")
        .and_then(|v| v.as_deref())
        .and_then(|s| s.parse::<f64>().ok());
    let rf_opt = params
        .get("rf")
        .and_then(|v| v.as_deref())
        .and_then(|s| s.parse::<f64>().ok());

    let ellipsoid = if let (Some(a), Some(rf)) = (a_opt, rf_opt) {
        EllipsoidParams { a, f: 1.0 / rf }
    } else {
        // Fall back to +ellps= named lookup.
        let ellps_str = params
            .get("ellps")
            .and_then(|v| v.as_deref())
            .unwrap_or("WGS84");

        ellipsoid_from_name(ellps_str).ok_or_else(|| {
            Error::PipelineParseError(format!(
                "step {}: cart unknown +ellps '{}'",
                step_idx, ellps_str
            ))
        })?
    };

    Ok(StepKind::Cart { ellipsoid })
}

/// Parse a `+proj=vgridshift` step from its parameter map.
fn parse_vgridshift_step(
    params: HashMap<String, Option<String>>,
    step_idx: usize,
) -> Result<StepKind> {
    let grid_id = params
        .get("grids")
        .and_then(|v| v.as_deref())
        .ok_or_else(|| {
            Error::PipelineParseError(format!(
                "step {}: vgridshift missing +grids parameter",
                step_idx
            ))
        })?
        .to_string();

    let direction = parse_shift_direction(
        params.get("direction").and_then(|v| v.as_deref()),
        step_idx,
        "vgridshift",
    )?;

    Ok(StepKind::Vgridshift { grid_id, direction })
}

/// Parse a `+proj=hgridshift` step from its parameter map.
fn parse_hgridshift_step(
    params: HashMap<String, Option<String>>,
    step_idx: usize,
) -> Result<StepKind> {
    let grid_id = params
        .get("grids")
        .and_then(|v| v.as_deref())
        .ok_or_else(|| {
            Error::PipelineParseError(format!(
                "step {}: hgridshift missing +grids parameter",
                step_idx
            ))
        })?
        .to_string();

    let direction = parse_shift_direction(
        params.get("direction").and_then(|v| v.as_deref()),
        step_idx,
        "hgridshift",
    )?;

    Ok(StepKind::Hgridshift { grid_id, direction })
}

/// Parse a direction string into a [`ShiftDirection`].
fn parse_shift_direction(
    s: Option<&str>,
    step_idx: usize,
    step_name: &str,
) -> Result<ShiftDirection> {
    match s.unwrap_or("forward").to_lowercase().as_str() {
        "forward" | "fwd" => Ok(ShiftDirection::Forward),
        "inverse" | "inv" => Ok(ShiftDirection::Inverse),
        other => Err(Error::PipelineParseError(format!(
            "step {}: {} unknown +direction '{}'",
            step_idx, step_name, other
        ))),
    }
}

/// Convert a parsed parameter map into a [`StepKind`].
fn params_to_step_kind(
    params: HashMap<String, Option<String>>,
    step_idx: usize,
) -> Result<StepKind> {
    let proj_val = params.get("proj").and_then(|v| v.as_deref()).unwrap_or("");

    match proj_val {
        "axisswap" => {
            let order_str = params
                .get("order")
                .and_then(|v| v.as_deref())
                .ok_or_else(|| {
                    Error::PipelineParseError(format!(
                        "step {}: axisswap missing +order parameter",
                        step_idx
                    ))
                })?;
            let order = parse_axis_order(order_str, step_idx)?;
            Ok(StepKind::AxisSwap { order })
        }

        "unitconvert" => {
            // Accept xy_in / xy_out (or just units if only one is given)
            let from_str = params
                .get("xy_in")
                .and_then(|v| v.as_deref())
                .or_else(|| params.get("from").and_then(|v| v.as_deref()))
                .ok_or_else(|| {
                    Error::PipelineParseError(format!(
                        "step {}: unitconvert missing +xy_in parameter",
                        step_idx
                    ))
                })?;
            let to_str = params
                .get("xy_out")
                .and_then(|v| v.as_deref())
                .or_else(|| params.get("to").and_then(|v| v.as_deref()))
                .ok_or_else(|| {
                    Error::PipelineParseError(format!(
                        "step {}: unitconvert missing +xy_out parameter",
                        step_idx
                    ))
                })?;
            let from = Unit::from_proj_str(from_str).ok_or_else(|| {
                Error::PipelineParseError(format!("step {}: unknown unit '{}'", step_idx, from_str))
            })?;
            let to = Unit::from_proj_str(to_str).ok_or_else(|| {
                Error::PipelineParseError(format!("step {}: unknown unit '{}'", step_idx, to_str))
            })?;
            Ok(StepKind::UnitConvert { from, to })
        }

        "passthrough" | "" => Ok(StepKind::Passthrough),

        "helmert" => parse_helmert_step(params, step_idx),

        "helmert_temporal" => parse_helmert_temporal_step(params, step_idx),

        "cart" => parse_cart_step(params, step_idx),

        "vgridshift" => parse_vgridshift_step(params, step_idx),

        "hgridshift" => parse_hgridshift_step(params, step_idx),

        _ => {
            // Reassemble a PROJ string from the params map and delegate to proj4rs.
            let ps = ProjString { params };
            Ok(StepKind::Project {
                proj_string: ps.to_proj_string(),
            })
        }
    }
}

/// Parse a comma-separated axis order string (e.g. `"2,1"`) into `[u8; 4]`.
fn parse_axis_order(s: &str, step_idx: usize) -> Result<[u8; 4]> {
    let mut order = [0u8; 4];
    for (i, part) in s.split(',').enumerate() {
        if i >= 4 {
            return Err(Error::PipelineParseError(format!(
                "step {}: axisswap order has more than 4 axes",
                step_idx
            )));
        }
        let v: u8 = part.trim().parse().map_err(|_| {
            Error::PipelineParseError(format!(
                "step {}: axisswap order '{}' is not a valid u8",
                step_idx, part
            ))
        })?;
        order[i] = v;
    }
    Ok(order)
}

// ---------------------------------------------------------------------------
// Public parse_pipeline helper (also used by from_proj_string)
// ---------------------------------------------------------------------------

/// Parse a PROJ pipeline string into per-step parameter maps.
///
/// Returns a `Vec` of `(params, inverse_flag)` pairs — one per step,
/// *not* including the pipeline header itself.
///
/// # Errors
///
/// Returns [`Error::PipelineParseError`] if the string does not start with
/// `+proj=pipeline` or if any step chunk cannot be parsed.
pub fn parse_pipeline(s: &str) -> Result<Vec<ParsedStep>> {
    let s = s.trim();

    // Quick sanity check: must contain +proj=pipeline
    if !s.contains("+proj=pipeline") {
        return Err(Error::PipelineParseError(
            "pipeline string must contain +proj=pipeline".to_string(),
        ));
    }

    // Split on "+step" boundary (case-sensitive, per PROJ convention).
    // We prepend a sentinel so the first chunk is always the pipeline header.
    let raw_chunks: Vec<&str> = s.split("+step").map(str::trim).collect();

    // raw_chunks[0] is the pipeline header (e.g. "+proj=pipeline +inv")
    // raw_chunks[1..] are the step chunks.
    let step_chunks = &raw_chunks[1..];

    let mut result: Vec<ParsedStep> = Vec::with_capacity(step_chunks.len());

    for (idx, chunk) in step_chunks.iter().enumerate() {
        if chunk.is_empty() {
            continue;
        }

        // Detect per-step +inv flag.
        let step_inv = has_inv_flag(chunk);

        // Strip the +inv token before passing to ProjString::parse so it
        // doesn't corrupt the params map (inv has no =value).
        let cleaned = strip_inv_flag(chunk);

        if cleaned.trim().is_empty() {
            // A bare "+step +inv" with no projection — treat as passthrough.
            result.push((HashMap::new(), step_inv));
            continue;
        }

        let ps = ProjString::parse(cleaned.trim())
            .map_err(|e| Error::PipelineParseError(format!("step {}: {}", idx, e)))?;

        result.push((ps.params, step_inv));
    }

    Ok(result)
}

/// Returns `true` if the chunk contains a bare `+inv` token.
fn has_inv_flag(chunk: &str) -> bool {
    for token in chunk.split_whitespace() {
        if token == "+inv" {
            return true;
        }
    }
    false
}

/// Return the chunk with the bare `+inv` token removed.
fn strip_inv_flag(chunk: &str) -> String {
    chunk
        .split_whitespace()
        .filter(|t| *t != "+inv")
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_unit_meters_per_unit() {
        assert!((Unit::M.to_meters_per_unit() - 1.0).abs() < 1e-15);
        assert!((Unit::Km.to_meters_per_unit() - 1000.0).abs() < 1e-15);
        assert!((Unit::Ft.to_meters_per_unit() - 0.3048).abs() < 1e-15);
    }

    #[test]
    fn test_unit_radians_per_unit() {
        assert!((Unit::Rad.to_radians_per_unit() - 1.0).abs() < 1e-15);
        let deg_rad = std::f64::consts::PI / 180.0;
        assert!((Unit::Deg.to_radians_per_unit() - deg_rad).abs() < 1e-20);
    }

    #[test]
    fn test_unit_from_proj_str() {
        assert_eq!(Unit::from_proj_str("m"), Some(Unit::M));
        assert_eq!(Unit::from_proj_str("km"), Some(Unit::Km));
        assert_eq!(Unit::from_proj_str("ft"), Some(Unit::Ft));
        assert_eq!(Unit::from_proj_str("us-ft"), Some(Unit::USFt));
        assert_eq!(Unit::from_proj_str("rad"), Some(Unit::Rad));
        assert_eq!(Unit::from_proj_str("deg"), Some(Unit::Deg));
        assert_eq!(Unit::from_proj_str("parsec"), None);
    }

    #[test]
    fn test_axis_swap_forward() {
        let coord = Coordinate::new(10.0, 20.0);
        let result =
            apply_axis_swap(&coord, &[2, 1, 0, 0], false).expect("axisswap should succeed");
        assert!((result.x - 20.0).abs() < 1e-10);
        assert!((result.y - 10.0).abs() < 1e-10);
    }

    #[test]
    fn test_axis_swap_inverse() {
        let coord = Coordinate::new(10.0, 20.0);
        // Forward [2,1]: (x,y) → (y,x); inverse should also be [2,1] for a swap.
        let result =
            apply_axis_swap(&coord, &[2, 1, 0, 0], true).expect("axisswap inv should succeed");
        assert!((result.x - 20.0).abs() < 1e-10);
        assert!((result.y - 10.0).abs() < 1e-10);
    }

    #[test]
    fn test_unit_convert_m_to_km() {
        let coord = Coordinate::new(1000.0, 2000.0);
        let result = apply_unit_convert(&coord, Unit::M, Unit::Km, false).expect("unit convert ok");
        assert!((result.x - 1.0).abs() < 1e-10);
        assert!((result.y - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_unit_convert_km_to_m_inverse() {
        let coord = Coordinate::new(1.0, 2.0);
        // inverse of M→Km is Km→M
        let result =
            apply_unit_convert(&coord, Unit::M, Unit::Km, true).expect("unit convert inverse ok");
        assert!((result.x - 1000.0).abs() < 1e-10);
        assert!((result.y - 2000.0).abs() < 1e-10);
    }

    #[test]
    fn test_parse_axis_order_valid() {
        let order = parse_axis_order("2,1", 0).expect("valid order");
        assert_eq!(order, [2, 1, 0, 0]);
    }

    #[test]
    fn test_parse_pipeline_basic() {
        let s = "+proj=pipeline +step +proj=axisswap +order=2,1";
        let steps = parse_pipeline(s).expect("valid pipeline string");
        assert_eq!(steps.len(), 1);
        let (params, inv) = &steps[0];
        assert!(!inv);
        assert_eq!(
            params.get("proj").and_then(|v| v.as_deref()),
            Some("axisswap")
        );
    }

    #[test]
    fn test_parse_pipeline_step_inv() {
        let s = "+proj=pipeline +step +inv +proj=axisswap +order=2,1";
        let steps = parse_pipeline(s).expect("valid pipeline string");
        assert_eq!(steps.len(), 1);
        let (_params, inv) = &steps[0];
        assert!(inv);
    }
}
