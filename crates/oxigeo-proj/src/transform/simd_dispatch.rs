//! Dispatch layer routing `Transformer::transform_batch` onto the
//! SIMD-accelerated kernels in [`super::simd`].
//!
//! # Fast-path applicability
//!
//! The kernels project geographic lon/lat **directly onto the target CRS's own
//! ellipsoid**.  They implement the map projection and nothing else: no datum
//! shift, no prime-meridian rotation, no grid interpolation, no axis swap.
//! Anything the kernels cannot represent must therefore make the fast path
//! *decline* (return `None`) so that `transform_batch` falls back to the
//! per-point [`Transformer::transform`] route, which runs the full OxiProj
//! pipeline.  [`fast_path_applicable`] is that gate.
//!
//! Declining is always safe; accepting when we should not have is silently
//! wrong, so every check below errs towards declining.

use super::simd;
use super::{Coordinate, Transformer};
use crate::error::{Error, Result};
use crate::proj_string::ProjString;

/// Semi-major axes closer than this (metres) count as the same ellipsoid.
///
/// WGS84 and GRS80 share `a` exactly; this only absorbs textual round-tripping.
const A_EPS: f64 = 1e-6;

/// First-eccentricity-squared values closer than this count as the same
/// ellipsoid.  WGS84 vs GRS80 differ by ~1.2e-11 in `e²` (≈0.1 mm of northing),
/// which is why the bound is loose enough to admit that pair and nothing wider:
/// the next-closest common pair (GRS80 vs Bessel) differs by ~7e-5.
const E2_EPS: f64 = 1e-9;

/// Helmert parameters closer than this count as identical shifts.
const TOWGS84_EPS: f64 = 1e-9;

impl Transformer {
    /// Attempts to run SIMD-accelerated batch projection.
    ///
    /// Returns `Some(Result<…>)` if the source→target pair maps to a supported
    /// fast-path kernel (TM/UTM forward, Mercator forward, LCC forward).
    /// Returns `None` to signal that the caller should use the scalar fallback.
    pub(super) fn try_simd_batch(&self, coords: &[Coordinate]) -> Option<Result<Vec<Coordinate>>> {
        if coords.is_empty() {
            return Some(Ok(Vec::new()));
        }

        // We only accelerate Geographic → Projected (forward) transforms.
        // Projected → Geographic (inverse) falls back to OxiProj.
        if !self.source_crs.is_geographic() || !self.target_crs.is_projected() {
            return None;
        }

        // Obtain the PROJ strings for both ends.  The source string is needed
        // to prove that no datum change is required (see `fast_path_applicable`).
        let src_str = match self.source_crs.to_proj_string() {
            Ok(s) => s,
            Err(_) => return None,
        };
        let proj_str = match self.target_crs.to_proj_string() {
            Ok(s) => s,
            Err(_) => return None,
        };

        let src_parsed = match ProjString::parse(&src_str) {
            Ok(p) => p,
            Err(_) => return None,
        };
        let parsed = match ProjString::parse(&proj_str) {
            Ok(p) => p,
            Err(_) => return None,
        };

        if !fast_path_applicable(&src_parsed, &parsed) {
            return None;
        }

        let proj_type = parsed.proj()?;

        match proj_type {
            "tmerc" | "utm" => Some(self.simd_tmerc_forward(coords, &parsed)),
            "merc" => Some(self.simd_merc_forward(coords, &parsed)),
            "lcc" => Some(self.simd_lcc_forward(coords, &parsed)),
            _ => None,
        }
    }

    /// SIMD-accelerated Transverse Mercator / UTM forward batch.
    fn simd_tmerc_forward(
        &self,
        coords: &[Coordinate],
        parsed: &ProjString,
    ) -> Result<Vec<Coordinate>> {
        use simd::{WGS84_A, WGS84_E2, tmerc_forward_batch};

        // Extract parameters from the PROJ string.
        // `+proj=utm +zone=N` is a shorthand for tmerc with standard parameters.
        let proj_type = parsed.proj().unwrap_or("tmerc");

        let (lon0_rad, lat0_rad, k0, false_easting, false_northing, a, e2) = if proj_type == "utm" {
            // UTM shorthand: zone → central meridian, k0=0.9996, FE=500000, FN=0/10000000.
            // PROJ fixes `phi0 = 0` for `+proj=utm`, so `+lat_0` is ignored here.
            let zone = parsed.zone().unwrap_or(32) as f64;
            let lon0_deg = zone * 6.0 - 183.0;
            let false_northing = if parsed.has("south") {
                10_000_000.0
            } else {
                0.0
            };
            (
                lon0_deg.to_radians(),
                0.0,
                0.9996,
                500_000.0,
                false_northing,
                WGS84_A,
                WGS84_E2,
            )
        } else {
            // Generic tmerc — read all parameters explicitly.
            let lon0_deg = parsed
                .get("lon_0")
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);
            // Latitude of origin.  Zero for UTM-style zones, but non-zero for
            // e.g. the Japan Plane Rectangular CS (EPSG:6669-6687 / 2443-2461),
            // whose zones use `+lat_0=26..44`.  Omitting it offsets every
            // northing by the meridional arc from the equator to `lat_0`
            // (≈ 3 985 144 m for `lat_0=36`).
            let lat0_deg = parsed
                .get("lat_0")
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);
            let k0 = parsed
                .get("k")
                .or_else(|| parsed.get("k_0"))
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(1.0);
            let fe = parsed
                .get("x_0")
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);
            let fn_ = parsed
                .get("y_0")
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);

            // Ellipsoid: prefer explicit +a/+b or +ellps; default to WGS84.
            let (a, e2) = kernel_ellipsoid(parsed)?;
            (
                lon0_deg.to_radians(),
                lat0_deg.to_radians(),
                k0,
                fe,
                fn_,
                a,
                e2,
            )
        };

        // Decompose coordinates into separate lon/lat arrays (degrees → radians).
        let lons: Vec<f64> = coords.iter().map(|c| c.x.to_radians()).collect();
        let lats: Vec<f64> = coords.iter().map(|c| c.y.to_radians()).collect();

        let (xs, ys) = tmerc_forward_batch(
            &lons,
            &lats,
            k0,
            lon0_rad,
            lat0_rad,
            false_easting,
            false_northing,
            a,
            e2,
        );

        finish(xs, ys, parsed, "tmerc_batch: non-finite result")
    }

    /// SIMD-accelerated Mercator forward batch.
    fn simd_merc_forward(
        &self,
        coords: &[Coordinate],
        parsed: &ProjString,
    ) -> Result<Vec<Coordinate>> {
        use simd::merc_forward_batch;

        let lon0_deg = parsed
            .get("lon_0")
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        let fe = parsed
            .get("x_0")
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        let fn_ = parsed
            .get("y_0")
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);

        let (a, e2) = kernel_ellipsoid(parsed)?;
        // Pseudo-Mercator (EPSG:3857) uses a sphere: a=b=6378137, so e=0.
        let e = e2.sqrt();
        let e_eff = if e < 1e-10 { 0.0 } else { e };

        // PROJ (`src/projections/merc.cpp`): when `+lat_ts` is supplied it
        // *derives* the scale factor from the standard parallel and `+k` is
        // ignored — `k0 = cos(lat_ts) / sqrt(1 - e² sin²(lat_ts))` (the
        // spherical form drops the denominator).  Only when `+lat_ts` is absent
        // does `+k` / `+k_0` apply.
        let k0 = match parsed
            .get("lat_ts")
            .and_then(|s| s.parse::<f64>().ok())
            .map(f64::to_radians)
        {
            Some(lat_ts) => {
                let sin_ts = lat_ts.sin();
                lat_ts.cos() / (1.0 - e_eff * e_eff * sin_ts * sin_ts).sqrt()
            }
            None => parsed
                .get("k")
                .or_else(|| parsed.get("k_0"))
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(1.0),
        };

        let lons: Vec<f64> = coords.iter().map(|c| c.x.to_radians()).collect();
        let lats: Vec<f64> = coords.iter().map(|c| c.y.to_radians()).collect();

        let (xs, ys) =
            merc_forward_batch(&lons, &lats, lon0_deg.to_radians(), k0, fe, fn_, a, e_eff);

        finish(xs, ys, parsed, "merc_batch: non-finite result")
    }

    /// SIMD-accelerated Lambert Conformal Conic forward batch.
    fn simd_lcc_forward(
        &self,
        coords: &[Coordinate],
        parsed: &ProjString,
    ) -> Result<Vec<Coordinate>> {
        use simd::{lcc_cone_params, lcc_forward_batch};

        let lon0_deg = parsed
            .get("lon_0")
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        let lat0_deg = parsed
            .get("lat_0")
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        // lat_1 / lat_2: standard parallels.  If only lat_1 is given, use it twice.
        let lat1_deg = parsed
            .get("lat_1")
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(lat0_deg);
        let lat2_deg = parsed
            .get("lat_2")
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(lat1_deg);
        let fe = parsed
            .get("x_0")
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        let fn_ = parsed
            .get("y_0")
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);

        // Scale factor at the standard parallel(s).  LCC_1SP CRSs carry a
        // `+k_0` != 1; PROJ multiplies both rho and rho0 by it, which
        // `lcc_cone_params` folds into the cone constant.
        let k0 = parsed
            .get("k")
            .or_else(|| parsed.get("k_0"))
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(1.0);

        let (a, e2) = kernel_ellipsoid(parsed)?;

        let lat0_rad = lat0_deg.to_radians();
        let lat1_rad = lat1_deg.to_radians();
        let lat2_rad = lat2_deg.to_radians();

        let cone = match lcc_cone_params(lat0_rad, lat1_rad, lat2_rad, e2, k0) {
            Some(p) => p,
            None => {
                return Err(Error::projection_init_error(
                    "lcc_batch: degenerate cone constant",
                ));
            }
        };

        let lons: Vec<f64> = coords.iter().map(|c| c.x.to_radians()).collect();
        let lats: Vec<f64> = coords.iter().map(|c| c.y.to_radians()).collect();

        let (xs, ys) =
            lcc_forward_batch(&lons, &lats, &cone, lon0_deg.to_radians(), fe, fn_, a, e2);

        finish(xs, ys, parsed, "lcc_batch: non-finite result")
    }
}

/// `parse_ellipsoid` for the kernels.
///
/// [`fast_path_applicable`] has already proved the ellipsoid is recognised
/// before any kernel runs, so `None` is unreachable here; it is turned into an
/// error rather than an `unwrap` so the function stays total.
fn kernel_ellipsoid(parsed: &ProjString) -> Result<(f64, f64)> {
    parse_ellipsoid(parsed).ok_or_else(|| {
        Error::projection_init_error("simd_batch: unrecognised ellipsoid for the target CRS")
    })
}

/// Applies the CRS's linear unit and packages the kernel output as
/// [`Coordinate`]s, rejecting any non-finite result.
///
/// PROJ's `fwd_finalize` is `out = fr_meter * (projected + false_offset)`, i.e.
/// the false easting/northing are expressed in **metres** and the *sum* is
/// converted to the CRS's linear unit.  The kernels already added the false
/// offsets, so only the reciprocal unit scale remains.
fn finish(
    xs: Vec<f64>,
    ys: Vec<f64>,
    parsed: &ProjString,
    non_finite_msg: &'static str,
) -> Result<Vec<Coordinate>> {
    // `fast_path_applicable` already rejected unknown units, so the fallback
    // here is unreachable in practice; 1.0 keeps it total without `expect`.
    let inv_unit = 1.0 / linear_unit_to_metre(parsed).unwrap_or(1.0);
    xs.into_iter()
        .zip(ys)
        .map(|(x, y)| {
            let c = Coordinate::new(x * inv_unit, y * inv_unit);
            if c.is_valid() {
                Ok(c)
            } else {
                Err(Error::transformation_error(non_finite_msg))
            }
        })
        .collect()
}

/// Size of one CRS linear unit in metres, or `None` when the `+units` token is
/// not one we know how to convert.
///
/// `+to_meter` wins over `+units`, matching PROJ.  An unknown `+units` returns
/// `None` so that [`fast_path_applicable`] can decline rather than silently
/// emit metres for a CRS measured in something else.
fn linear_unit_to_metre(parsed: &ProjString) -> Option<f64> {
    if let Some(tm) = parsed.get("to_meter").and_then(|s| s.parse::<f64>().ok()) {
        return if tm.is_finite() && tm > 0.0 {
            Some(tm)
        } else {
            None
        };
    }
    match parsed.get("units") {
        None => Some(1.0),
        // Values and spellings from PROJ's `pj_units.cpp` unit table.
        Some("m") => Some(1.0),
        Some("km") => Some(1000.0),
        Some("ft") => Some(0.3048),
        Some("us-ft") => Some(1200.0 / 3937.0),
        Some("yd") => Some(0.9144),
        Some("us-yd") => Some(3600.0 / 3937.0),
        Some("in") => Some(0.0254),
        Some("us-in") => Some(1.0 / 39.37),
        Some("mi") => Some(1609.344),
        Some("us-mi") => Some(6336000.0 / 3937.0),
        Some("fath") => Some(1.8288),
        Some("ch") => Some(20.1168),
        Some("us-ch") => Some(792.0 / 39.37),
        Some("link") => Some(0.201168),
        Some("kmi") => Some(1852.0),
        Some("dm") => Some(0.1),
        Some("cm") => Some(0.01),
        Some("mm") => Some(0.001),
        Some(_) => None,
    }
}

/// Decides whether the SIMD kernels can faithfully reproduce the transform that
/// the scalar OxiProj pipeline would perform for this `source → target` pair.
///
/// The kernels are *pure map projections*.  They therefore only apply when the
/// source and target describe the **same geodetic datum**, the target's figure
/// of the Earth is one we can pin down exactly, and the target uses a linear
/// unit we can convert with no axis reordering.  Everything else falls back to
/// the scalar path.
///
/// Note the asymmetry between a *datum* and a projection's *computational
/// figure*: EPSG:3857 (Web Mercator) deliberately applies spherical Mercator
/// formulas to WGS-84 geodetic latitudes.  That is not a datum change and must
/// not be declined — see the `spherical_target` branch below.
fn fast_path_applicable(src: &ProjString, dst: &ProjString) -> bool {
    // Unknown linear unit → we cannot scale the kernel's metres correctly.
    if linear_unit_to_metre(dst).is_none() {
        return false;
    }

    for p in [src, dst] {
        // Axis reordering / non-standard axis directions are not modelled.
        if let Some(axis) = p.get("axis")
            && axis != "enu"
        {
            return false;
        }
        // A non-Greenwich prime meridian rotates longitudes before projection.
        if let Some(pm) = p.get("pm")
            && !pm.eq_ignore_ascii_case("greenwich")
        {
            return false;
        }
        // Grid-based datum shifts cannot be expressed by a projection kernel.
        // `@null` is PROJ's explicit "no shift" marker (used by EPSG:3857).
        if let Some(grids) = p.get("nadgrids")
            && grids != "@null"
        {
            return false;
        }
        // A named datum other than the two null-shift ones may imply a Helmert
        // shift or a grid that we would silently drop.
        if let Some(datum) = p.get("datum")
            && !is_null_shift_datum(datum)
        {
            return false;
        }
    }

    // The figure of the Earth must be *known*, not guessed.  `parse_ellipsoid`
    // returns `None` for an `+ellps` outside PROJ's table; before this gate
    // existed such a CRS was silently projected on WGS-84 (a 2.1e5 m error for
    // `+ellps=clrk66`), so an unknown ellipsoid must decline, never default.
    let (a_s, e2_s) = match parse_ellipsoid(src) {
        Some(v) => v,
        None => return false,
    };
    let (a_d, e2_d) = match parse_ellipsoid(dst) {
        Some(v) => v,
        None => return false,
    };

    // Either both ends use the same ellipsoid (the kernel may consume the input
    // lon/lat directly), or the target is a sphere — a projection parameter,
    // not a datum, so PROJ applies no shift either and the kernels reproduce it
    // exactly with `e = 0`.
    //
    // Deliberately conservative: because the checks above already established
    // that neither end declares a datum shift, a *differing* ellipsoid would in
    // fact also reproduce the scalar path (measured: `+ellps=clrk66`, `intl`
    // and `bessel` targets from a WGS-84 source all agree to <5e-8 m once
    // `parse_ellipsoid` knows the ellipsoid).  We still decline those, because
    // a CRS naming a foreign ellipsoid with no `+towgs84` is under-specified,
    // and the cost of declining is a slower-but-correct scalar loop while the
    // cost of wrongly accepting is a silent error of hundreds of metres.  Every
    // CRS family that actually matters here — UTM, JPR, Web Mercator,
    // Lambert-93, State Plane — passes this check.
    let same_figure = (a_s - a_d).abs() <= A_EPS && (e2_s - e2_d).abs() <= E2_EPS;
    let spherical_target = e2_d.abs() <= E2_EPS;
    if !(same_figure || spherical_target) {
        return false;
    }

    // Same Helmert shift on both ends (usually "none on either end"): a shift
    // present on only one side is a real datum change the kernels do not apply.
    let t_s = src.towgs84().unwrap_or([0.0; 7]);
    let t_d = dst.towgs84().unwrap_or([0.0; 7]);
    if t_s
        .iter()
        .zip(t_d.iter())
        .any(|(a, b)| (a - b).abs() > TOWGS84_EPS)
    {
        return false;
    }

    true
}

/// `true` for the `+datum` names whose PROJ definition carries an all-zero
/// Helmert shift and no grid, i.e. those that need no datum transformation.
fn is_null_shift_datum(datum: &str) -> bool {
    matches!(datum, "WGS84" | "wgs84" | "NAD83" | "nad83")
}

/// How an entry in [`PROJ_ELLIPSOIDS`] states its second defining parameter.
#[derive(Clone, Copy)]
enum EllpsShape {
    /// Reciprocal flattening `1/f`.
    Rf(f64),
    /// Semi-minor axis `b`, in metres.
    B(f64),
}

/// PROJ's built-in ellipsoid table (`src/ellps.cpp`, `pj_ellps[]`).
///
/// Transcribed in full rather than partially on purpose: a partial table plus a
/// WGS-84 fallback silently projects e.g. `+ellps=clrk66` onto the wrong figure.
/// [`parse_ellipsoid`] returns `None` for anything not listed here, and
/// [`fast_path_applicable`] then declines the fast path.
const PROJ_ELLIPSOIDS: &[(&str, f64, EllpsShape)] = &[
    ("MERIT", 6_378_137.0, EllpsShape::Rf(298.257)),
    ("SGS85", 6_378_136.0, EllpsShape::Rf(298.257)),
    ("GRS80", 6_378_137.0, EllpsShape::Rf(298.257_222_101)),
    ("IAU76", 6_378_140.0, EllpsShape::Rf(298.257)),
    ("airy", 6_377_563.396, EllpsShape::B(6_356_256.910)),
    ("APL4.9", 6_378_137.0, EllpsShape::Rf(298.25)),
    ("NWL9D", 6_378_145.0, EllpsShape::Rf(298.25)),
    ("mod_airy", 6_377_340.189, EllpsShape::B(6_356_034.446)),
    ("andrae", 6_377_104.43, EllpsShape::Rf(300.0)),
    ("danish", 6_377_019.256_3, EllpsShape::Rf(300.0)),
    ("aust_SA", 6_378_160.0, EllpsShape::Rf(298.25)),
    ("GRS67", 6_378_160.0, EllpsShape::Rf(298.247_167_427)),
    ("GSK2011", 6_378_136.5, EllpsShape::Rf(298.256_415_1)),
    ("bessel", 6_377_397.155, EllpsShape::Rf(299.152_812_8)),
    ("bess_nam", 6_377_483.865, EllpsShape::Rf(299.152_812_8)),
    ("clrk66", 6_378_206.4, EllpsShape::B(6_356_583.8)),
    ("clrk80", 6_378_249.145, EllpsShape::Rf(293.466_3)),
    (
        "clrk80ign",
        6_378_249.2,
        EllpsShape::Rf(293.466_021_293_627),
    ),
    ("CPM", 6_375_738.7, EllpsShape::Rf(334.29)),
    ("delmbr", 6_376_428.0, EllpsShape::Rf(311.5)),
    ("engelis", 6_378_136.05, EllpsShape::Rf(298.2566)),
    ("evrst30", 6_377_276.345, EllpsShape::Rf(300.801_7)),
    ("evrst48", 6_377_304.063, EllpsShape::Rf(300.801_7)),
    ("evrst56", 6_377_301.243, EllpsShape::Rf(300.801_7)),
    ("evrst69", 6_377_295.664, EllpsShape::Rf(300.801_7)),
    ("evrstSS", 6_377_298.556, EllpsShape::Rf(300.801_7)),
    ("fschr60", 6_378_166.0, EllpsShape::Rf(298.3)),
    ("fschr60m", 6_378_155.0, EllpsShape::Rf(298.3)),
    ("fschr68", 6_378_150.0, EllpsShape::Rf(298.3)),
    ("helmert", 6_378_200.0, EllpsShape::Rf(298.3)),
    ("hough", 6_378_270.0, EllpsShape::Rf(297.0)),
    ("intl", 6_378_388.0, EllpsShape::Rf(297.0)),
    ("krass", 6_378_245.0, EllpsShape::Rf(298.3)),
    ("kaula", 6_378_163.0, EllpsShape::Rf(298.24)),
    ("lerch", 6_378_139.0, EllpsShape::Rf(298.257)),
    ("mprts", 6_397_300.0, EllpsShape::Rf(191.0)),
    ("new_intl", 6_378_157.5, EllpsShape::B(6_356_772.2)),
    ("plessis", 6_376_523.0, EllpsShape::B(6_355_863.0)),
    ("PZ90", 6_378_136.0, EllpsShape::Rf(298.257_84)),
    ("SEasia", 6_378_155.0, EllpsShape::B(6_356_773.320_5)),
    ("walbeck", 6_376_896.0, EllpsShape::B(6_355_834.846_7)),
    ("WGS60", 6_378_165.0, EllpsShape::Rf(298.3)),
    ("WGS66", 6_378_145.0, EllpsShape::Rf(298.25)),
    ("WGS72", 6_378_135.0, EllpsShape::Rf(298.26)),
    ("WGS84", 6_378_137.0, EllpsShape::Rf(298.257_223_563)),
    ("sphere", 6_370_997.0, EllpsShape::B(6_370_997.0)),
];

/// `(a, e2)` from a reciprocal flattening.
#[inline]
fn from_rf(a: f64, rf: f64) -> (f64, f64) {
    let f = 1.0 / rf;
    (a, 2.0 * f - f * f)
}

/// `(a, e2)` from a semi-minor axis.
#[inline]
fn from_b(a: f64, b: f64) -> (f64, f64) {
    let f = 1.0 - b / a;
    (a, 2.0 * f - f * f)
}

/// Parse ellipsoid parameters from a `ProjString`.
///
/// Returns `Some((a, e2))` — semi-major axis in metres and first eccentricity
/// squared — or **`None` when the figure of the Earth cannot be determined**.
///
/// Returning `None` rather than defaulting to WGS-84 is the whole point: the
/// caller ([`fast_path_applicable`]) turns `None` into "decline the fast path",
/// so an unrecognised `+ellps` costs a little speed instead of silently
/// projecting onto the wrong ellipsoid.
///
/// Priority order (PROJ's own):
/// 1. Explicit `+a` and (`+b` / `+f` / `+rf`), or `+a` alone (sphere).
/// 2. `+R` (spherical radius).
/// 3. Named ellipsoid `+ellps` from [`PROJ_ELLIPSOIDS`].
/// 4. Named datum `+datum` (only the null-shift datums, whose ellipsoid is
///    unambiguous).
/// 5. No figure given at all: PROJ's default, WGS-84.
fn parse_ellipsoid(parsed: &ProjString) -> Option<(f64, f64)> {
    use simd::{WGS84_A, WGS84_E2};

    // 1. Explicit semi-major axis.
    if let Some(a_val) = parsed.get("a").and_then(|s| s.parse::<f64>().ok()) {
        if let Some(b_val) = parsed.get("b").and_then(|s| s.parse::<f64>().ok()) {
            return Some(from_b(a_val, b_val));
        }
        if let Some(f_val) = parsed.get("f").and_then(|s| s.parse::<f64>().ok()) {
            return Some((a_val, 2.0 * f_val - f_val * f_val));
        }
        if let Some(rf) = parsed.get("rf").and_then(|s| s.parse::<f64>().ok()) {
            return Some(from_rf(a_val, rf));
        }
        // Semi-major only: a sphere.
        return Some((a_val, 0.0));
    }

    // 2. Spherical radius.
    if let Some(r) = parsed.get("R").and_then(|s| s.parse::<f64>().ok()) {
        return Some((r, 0.0));
    }

    // 3. Named ellipsoid — unknown names decline rather than default.
    if let Some(ellps) = parsed.get("ellps") {
        return PROJ_ELLIPSOIDS
            .iter()
            .find(|(name, _, _)| name.eq_ignore_ascii_case(ellps))
            .map(|&(_, a, shape)| match shape {
                EllpsShape::Rf(rf) => from_rf(a, rf),
                EllpsShape::B(b) => from_b(a, b),
            });
    }

    // 4. Named datum.  Only the null-shift datums are recognised; every other
    //    name already made `fast_path_applicable` decline, and guessing an
    //    ellipsoid for it here would be exactly the bug this function avoids.
    if let Some(datum) = parsed.get("datum") {
        return match datum {
            "WGS84" | "wgs84" => Some((WGS84_A, WGS84_E2)),
            // NAD83 uses GRS80.
            "NAD83" | "nad83" => Some(from_rf(6_378_137.0, 298.257_222_101)),
            _ => None,
        };
    }

    // 5. Nothing specified: PROJ defaults to WGS-84.
    Some((WGS84_A, WGS84_E2))
}

// ---------------------------------------------------------------------------
// Unit tests for the applicability gate
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Every numeric test elsewhere in the tree compares the batch path against
    /// the scalar path — and those comparisons pass *trivially* when the gate
    /// declines, because declining makes `transform_batch` run the scalar loop.
    /// So the gate needs its own direct tests, or an over-restrictive gate
    /// silently disables the whole fast path with a fully green suite.
    fn gate(src: &str, dst: &str) -> bool {
        let s = ProjString::parse(src).expect("source PROJ string parses");
        let d = ProjString::parse(dst).expect("target PROJ string parses");
        fast_path_applicable(&s, &d)
    }

    const WGS84_LL: &str = "+proj=longlat +datum=WGS84 +no_defs";

    #[test]
    fn accepts_jpr_zones() {
        for dst in [
            "+proj=tmerc +lat_0=33 +lon_0=129.5 +k=0.9999 +x_0=0 +y_0=0 +ellps=GRS80 +units=m +no_defs",
            "+proj=tmerc +lat_0=36 +lon_0=136 +k=0.9999 +x_0=0 +y_0=0 +ellps=GRS80 +units=m +no_defs",
            "+proj=tmerc +lat_0=36 +lon_0=139.833333333333 +k=0.9999 +x_0=0 +y_0=0 +ellps=GRS80 +units=m +no_defs",
        ] {
            assert!(
                gate(WGS84_LL, dst),
                "JPR zone must take the fast path: {dst}"
            );
        }
    }

    #[test]
    fn accepts_utm_and_web_mercator() {
        assert!(gate(
            WGS84_LL,
            "+proj=utm +zone=32 +datum=WGS84 +units=m +no_defs"
        ));
        // EPSG:3857 — spherical formulas on WGS-84 latitudes.  `+a == +b` makes
        // the target's e² zero while the source's is not; that is a projection
        // choice, not a datum change, and `+nadgrids=@null` says so explicitly.
        assert!(
            gate(
                WGS84_LL,
                "+proj=merc +a=6378137 +b=6378137 +lat_ts=0 +lon_0=0 +x_0=0 +y_0=0 +k=1 \
                 +units=m +nadgrids=@null +wktext +no_defs"
            ),
            "Web Mercator (EPSG:3857) must keep the Mercator fast path"
        );
    }

    #[test]
    fn accepts_ellipsoidal_mercator_and_lcc() {
        // EPSG:3395 — World Mercator.
        assert!(gate(
            WGS84_LL,
            "+proj=merc +lon_0=0 +k=1 +x_0=0 +y_0=0 +datum=WGS84 +units=m +no_defs"
        ));
        // EPSG:5641 — SIRGAS 2000 / Brazil Mercator: GRS80 vs WGS84 differ by
        // ~3e-11 in e², inside `E2_EPS`.
        assert!(gate(
            WGS84_LL,
            "+proj=merc +lon_0=-43 +lat_ts=-2 +x_0=5000000 +y_0=10000000 +ellps=GRS80 \
             +units=m +no_defs"
        ));
        // EPSG:2154 — RGF93 / Lambert-93.
        assert!(gate(
            WGS84_LL,
            "+proj=lcc +lat_0=46.5 +lon_0=3 +lat_1=49 +lat_2=44 +x_0=700000 +y_0=6600000 \
             +ellps=GRS80 +units=m +no_defs"
        ));
    }

    #[test]
    fn accepts_us_survey_foot() {
        assert!(gate(
            WGS84_LL,
            "+proj=tmerc +lat_0=38.83333333333334 +lon_0=-77 +k=0.9999 \
             +x_0=399999.9998983998 +y_0=0 +datum=NAD83 +units=us-ft +no_defs"
        ));
        assert!(gate(
            WGS84_LL,
            "+proj=lcc +lat_0=39.3333333333333 +lon_0=-122 +lat_1=41.6666666666667 +lat_2=40 \
             +x_0=2000000.0001016 +y_0=500000.0001016 +datum=NAD83 +units=us-ft +no_defs"
        ));
    }

    #[test]
    fn declines_datum_shifts() {
        // Tokyo datum JPR IX — Bessel + ~600 m Helmert shift.
        assert!(!gate(
            WGS84_LL,
            "+proj=tmerc +lat_0=36 +lon_0=139.833333333333 +k=0.9999 +x_0=0 +y_0=0 \
             +ellps=bessel +towgs84=-146.414,507.337,680.507,0,0,0,0 +units=m +no_defs"
        ));
        // OSGB36 — Airy + 7-parameter shift.
        assert!(!gate(
            WGS84_LL,
            "+proj=tmerc +lat_0=49 +lon_0=-2 +k=0.9996012717 +x_0=400000 +y_0=-100000 \
             +ellps=airy +towgs84=446.448,-125.157,542.06,0.15,0.247,0.842,-20.489 \
             +units=m +no_defs"
        ));
        // A different ellipsoid with no shift at all is still a different
        // figure of the Earth for the input latitudes.
        assert!(!gate(
            WGS84_LL,
            "+proj=tmerc +lat_0=0 +lon_0=9 +k=0.9996 +x_0=500000 +y_0=0 +ellps=intl \
             +units=m +no_defs"
        ));
    }

    #[test]
    fn declines_unknown_ellipsoid_instead_of_defaulting_to_wgs84() {
        // `+ellps=nosuchellipsoid` must NOT silently read as WGS-84.
        assert_eq!(
            parse_ellipsoid(
                &ProjString::parse("+proj=tmerc +ellps=nosuchellipsoid").expect("parse")
            ),
            None,
        );
        assert!(!gate(
            WGS84_LL,
            "+proj=tmerc +lat_0=0 +lon_0=9 +k=0.9996 +ellps=nosuchellipsoid +units=m +no_defs"
        ));
        // clrk66 IS in PROJ's table, so it parses — and then declines because
        // it is a different figure from the WGS-84 source (this pair used to
        // be projected on WGS-84 with a 2.1e5 m error).
        let clrk66 = ProjString::parse("+proj=tmerc +ellps=clrk66").expect("parse");
        let (a, _) = parse_ellipsoid(&clrk66).expect("clrk66 is a known ellipsoid");
        assert!(
            (a - 6_378_206.4).abs() < 1e-6,
            "clrk66 semi-major axis: {a}"
        );
        assert!(!gate(
            WGS84_LL,
            "+proj=tmerc +lat_0=0 +lon_0=-75 +k=0.9996 +x_0=500000 +y_0=0 +ellps=clrk66 \
             +units=m +no_defs"
        ));
    }

    #[test]
    fn declines_unknown_units_prime_meridian_grids_and_axis_order() {
        assert!(!gate(
            WGS84_LL,
            "+proj=tmerc +lat_0=0 +lon_0=9 +k=0.9996 +ellps=WGS84 +units=furlong +no_defs"
        ));
        assert!(!gate(
            WGS84_LL,
            "+proj=tmerc +lat_0=0 +lon_0=9 +k=0.9996 +ellps=WGS84 +pm=paris +units=m +no_defs"
        ));
        assert!(!gate(
            WGS84_LL,
            "+proj=tmerc +lat_0=0 +lon_0=9 +k=0.9996 +ellps=WGS84 +nadgrids=BETA2007.gsb \
             +units=m +no_defs"
        ));
        assert!(!gate(
            WGS84_LL,
            "+proj=tmerc +lat_0=0 +lon_0=9 +k=0.9996 +ellps=WGS84 +axis=neu +units=m +no_defs"
        ));
    }

    #[test]
    fn linear_units_match_proj_table() {
        let unit = |s: &str| linear_unit_to_metre(&ProjString::parse(s).expect("parse"));
        assert_eq!(unit("+proj=tmerc"), Some(1.0));
        assert_eq!(unit("+proj=tmerc +units=m"), Some(1.0));
        assert_eq!(unit("+proj=tmerc +units=ft"), Some(0.3048));
        assert_eq!(unit("+proj=tmerc +units=us-ft"), Some(1200.0 / 3937.0));
        assert_eq!(unit("+proj=tmerc +units=km"), Some(1000.0));
        // `+to_meter` wins over `+units`, as in PROJ.
        assert_eq!(unit("+proj=tmerc +units=ft +to_meter=2.5"), Some(2.5));
        // Unknown or nonsensical values decline.
        assert_eq!(unit("+proj=tmerc +units=furlong"), None);
        assert_eq!(unit("+proj=tmerc +to_meter=0"), None);
    }
}
