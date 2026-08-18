//! Extended verified EPSG registry tests — the whole-registry audit.
//!
//! Provenance
//! ----------
//! Two fixtures under `tests/data/`, both generated against **PROJ 9.5.1**
//! (pyproj 3.7.1, `always_xy=true`), 2026-08-12, each carrying the
//! authoritative PROJ string plus 5 sample points per code. The points are the
//! area-of-use centroid plus the four corners inset 10% of the bounds span;
//! the span is deliberately **not** antimeridian-normalised, which reproduces
//! the sampling of the original 153-code fixture (e.g. NAD83's area of use
//! runs 167.65°E → 40.73°W, and the raw negative span is what places its
//! corner points).
//!
//! * `verified_epsg_5pt_extended.json` — codes where the registry reproduces
//!   PROJ end to end. Pivot: `EPSG:4326 <-> code`, both directions.
//!
//! * `verified_epsg_projection_5pt.json` — codes where only the **projection**
//!   agrees. Pivot: the code's own geodetic base CRS `<-> code`, which cancels
//!   the datum shift and leaves the map projection under test. These codes
//!   still differ from PROJ by up to ~423 m end to end (each entry records its
//!   own `e2e_residual_m`) because PROJ selects those datum transformations
//!   from its database rather than from the `+towgs84` in the PROJ string —
//!   `to_proj4()` emits no `+towgs84` for any of them — and
//!   `transform::datum_shift` does not implement database-selected shifts.
//!   **This fixture pins projection math only; it is not an end-to-end
//!   guarantee.**
//!
//! Feature invariance
//! ------------------
//! Both fixtures must hold under **every** feature configuration — in
//! particular with and without `proj-db`. `projection_pivot` deliberately mixes
//! the two ways this crate can name a CRS: the geodetic base comes from a PROJ
//! string (`Crs::from_proj`) and the projected side from an EPSG code
//! (`Crs::from_epsg`). That asymmetry is the point. Until 0.2.4 the `proj-db`
//! feature resolved `CrsSource::Epsg` through oxiproj's bundled authority
//! database while the PROJ-string side kept using the registry definition, so
//! the pair combined a datum-bearing CRS with a datum-less one and the pipeline
//! applied a *one-sided* datum shift — 87 m for `EPSG:2039`, 226 m for
//! `EPSG:2056`, 4.8e5 m for `EPSG:2314`, 560 mismatches in each direction.
//! Real PROJ 9.7.0 composes *both* sides' datum transformations for such a
//! mixed pair (`cs2cs '+proj=longlat +ellps=bessel +towgs84=674.374,15.056,405.346,0,0,0,0'
//! +to EPSG:2056` returns the projection-only result exactly); a one-sided
//! shift is simply wrong. `transform::crs_to_oxi` therefore resolves every CRS
//! through this crate's own PROJ strings, and these tests are the canary for
//! any change that reintroduces a second, divergent CRS source.
//!
//! Together with `epsg_verified_registry_test.rs` (the original 153 codes)
//! these tests cover every code in the registry that PROJ 9.5.1 can resolve
//! and that has a two-dimensional area of use.
//!
//! They exist to keep the registry pinned to externally-verified data. The
//! audit that produced them found, among others: ITRF88–ITRF92 served as
//! Australian UTM grids (3.35e12 m), the CGCS2000 Gauss-Kruger block shifted
//! by a whole sub-family (3.26e7 m), every Gauss-Kruger central meridian 180°
//! out, 16 of 19 JGD2000 Japan Plane Rectangular zones on `+lat_0=0`
//! (4.9e6 m), and 79 State Plane CRSs labelled `metre` while their PROJ
//! strings were in feet.

#![cfg(feature = "std")]
#![allow(clippy::expect_used, clippy::panic)]

use oxigeo_proj::Crs;
use oxigeo_proj::lookup_epsg;
use oxigeo_proj::transform::{Coordinate, Transformer};

/// End-to-end verified codes: `code -> { name, proj, unit, kind, samples }`.
const EXTENDED_JSON: &str = include_str!("data/verified_epsg_5pt_extended.json");

/// Projection-verified codes: adds `base` and `e2e_residual_m`; each sample is
/// `{ xy, ll }` where `ll` is a coordinate of the code's geodetic base CRS.
const PROJECTION_JSON: &str = include_str!("data/verified_epsg_projection_5pt.json");

/// Maximum allowed residual, in metres. Matches the 153-code test exactly:
/// the measured worst case is far below this, and 1e-6 m stays six orders of
/// magnitude below any real-world discrepancy.
const METRE_TOL: f64 = 1e-6;

/// WGS 84 equatorial metres per degree (2 * pi * a / 360, a = 6378137).
const M_PER_DEG: f64 = 111_319.490_793_273_57;

/// Metadata key carrying the fixture's provenance rather than a CRS.
const PROVENANCE_KEY: &str = "__provenance__";

struct Sample {
    /// Coordinate in the CRS under test.
    xy: (f64, f64),
    /// The paired geographic coordinate (lon, lat) computed by PROJ 9.5.1 —
    /// in EPSG:4326 for the end-to-end fixture, in the code's own geodetic
    /// base CRS for the projection-only fixture.
    geographic: (f64, f64),
}

struct VerifiedCase {
    code: u32,
    name: String,
    unit: String,
    kind: String,
    samples: Vec<Sample>,
}

impl VerifiedCase {
    fn is_geographic(&self) -> bool {
        self.kind == "geographic"
    }
}

fn pair(v: &serde_json::Value) -> (f64, f64) {
    let arr = v.as_array().expect("coordinate pair must be an array");
    assert_eq!(arr.len(), 2, "coordinate pair must have exactly 2 entries");
    (
        arr[0].as_f64().expect("x must be a number"),
        arr[1].as_f64().expect("y must be a number"),
    )
}

/// Parses a fixture. `geographic_key` names the per-sample field holding the
/// geographic partner coordinate (`expect` end-to-end, `ll` projection-only).
fn load(raw: &str, geographic_key: &str) -> Vec<VerifiedCase> {
    let root: serde_json::Value = serde_json::from_str(raw).expect("fixture must parse");
    let map = root.as_object().expect("top level must be an object");

    let mut cases: Vec<VerifiedCase> = map
        .iter()
        .filter(|(key, _)| key.as_str() != PROVENANCE_KEY)
        .map(|(code, entry)| {
            let code: u32 = code.parse().expect("EPSG code key must be numeric");
            let samples: Vec<Sample> = entry["samples"]
                .as_array()
                .expect("samples must be an array")
                .iter()
                .map(|s| Sample {
                    xy: pair(&s["xy"]),
                    geographic: pair(&s[geographic_key]),
                })
                .collect();
            VerifiedCase {
                code,
                name: entry["name"].as_str().expect("name").to_string(),
                unit: entry["unit"].as_str().expect("unit").to_string(),
                kind: entry["kind"].as_str().expect("kind").to_string(),
                samples,
            }
        })
        .collect();
    cases.sort_by_key(|c| c.code);

    assert!(!cases.is_empty(), "fixture must not be empty");
    for case in &cases {
        assert_eq!(
            case.samples.len(),
            5,
            "EPSG:{} must carry 5 verified points",
            case.code
        );
    }
    cases
}

fn extended_cases() -> Vec<VerifiedCase> {
    load(EXTENDED_JSON, "expect")
}

fn projection_cases() -> Vec<VerifiedCase> {
    load(PROJECTION_JSON, "ll")
}

/// Residual between `got` and `want`, in metres (see the 153-code test).
fn residual_metres(geographic: bool, got: (f64, f64), want: (f64, f64)) -> f64 {
    if geographic {
        let coslat = want.1.to_radians().cos().abs().max(1e-12);
        let dx = (got.0 - want.0) * M_PER_DEG * coslat;
        let dy = (got.1 - want.1) * M_PER_DEG;
        dx.hypot(dy)
    } else {
        (got.0 - want.0).hypot(got.1 - want.1)
    }
}

/// PROJ parameters that describe the *projection* rather than the datum.
///
/// Dropping exactly these from a projected CRS's PROJ string, and forcing
/// `+proj=longlat`, yields that CRS's geodetic base while keeping every
/// ellipsoid and datum-shift token byte-identical. A transform between the two
/// therefore performs no datum shift at all, which is what isolates the
/// projection for `verified_epsg_projection_5pt.json`.
const PROJECTION_ONLY_PARAMS: &[&str] = &[
    "proj", "lat_0", "lon_0", "lat_1", "lat_2", "lat_ts", "lat_b", "k", "k_0", "x_0", "y_0",
    "zone", "south", "alpha", "gamma", "lonc", "azi", "units", "to_meter", "vunits", "axis",
    "o_lat_p", "o_lon_p", "o_alpha", "o_lon_c", "o_lat_c", "o_lon_1", "o_lat_1", "o_lon_2",
    "o_lat_2", "czech", "approx", "n", "m", "q", "h", "sweep",
];

/// Builds the same-datum geographic CRS underlying a projected PROJ string.
fn geodetic_base_of(proj_string: &str) -> String {
    let mut out = String::from("+proj=longlat");
    for token in proj_string.split_whitespace() {
        let Some(body) = token.strip_prefix('+') else {
            continue;
        };
        let key = body.split('=').next().unwrap_or(body);
        if key == "no_defs" || PROJECTION_ONLY_PARAMS.contains(&key) {
            continue;
        }
        out.push(' ');
        out.push_str(token);
    }
    out.push_str(" +no_defs");
    out
}

/// Registry-driven transformer with the strict area-of-use policy disabled —
/// the reference coordinates come from PROJ, which ignores areas of use, and
/// the sampling grid straddles areas that cross the antimeridian.
fn transformer_between(src: u32, dst: u32) -> oxigeo_proj::Result<Transformer> {
    Ok(Transformer::from_epsg(src, dst)?.with_strict(false))
}

/// Every verified code must resolve through the public registry API with the
/// authoritative EPSG name, CRS class and unit.
#[test]
fn extended_verified_codes_resolve_with_correct_metadata() {
    let mut failures: Vec<String> = Vec::new();

    for case in extended_cases().iter().chain(projection_cases().iter()) {
        let def = match lookup_epsg(case.code) {
            Ok(def) => def,
            Err(err) => {
                failures.push(format!("EPSG:{} | missing from registry: {err}", case.code));
                continue;
            }
        };

        if def.name != case.name {
            failures.push(format!(
                "EPSG:{} | name | registry: {:?} | verified: {:?}",
                case.code, def.name, case.name
            ));
        }

        let got_geographic = def.proj_string.contains("+proj=longlat");
        if got_geographic != case.is_geographic() {
            failures.push(format!(
                "EPSG:{} | class | registry proj: {:?} | verified kind: {:?}",
                case.code, def.proj_string, case.kind
            ));
        }

        if def.unit != case.unit {
            failures.push(format!(
                "EPSG:{} | unit | registry: {:?} | verified: {:?}",
                case.code, def.unit, case.unit
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} metadata mismatches against the verified data set:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Forward: EPSG:code -> EPSG:4326 must reproduce PROJ 9.5.1 at all 5 points.
#[test]
fn extended_verified_codes_forward_transform_matches_proj_951() {
    let cases = extended_cases();
    let mut failures: Vec<String> = Vec::new();

    for case in &cases {
        let transformer = match transformer_between(case.code, 4326) {
            Ok(t) => t,
            Err(err) => {
                failures.push(format!(
                    "EPSG:{} -> 4326 | transformer construction failed: {err}",
                    case.code
                ));
                continue;
            }
        };

        for (idx, sample) in case.samples.iter().enumerate() {
            let input = Coordinate::new(sample.xy.0, sample.xy.1);
            match transformer.transform(&input) {
                Ok(out) => {
                    let err_m = residual_metres(true, (out.x, out.y), sample.geographic);
                    if err_m > METRE_TOL {
                        failures.push(format!(
                            "EPSG:{} -> 4326 | point {idx} | got ({:.9}, {:.9}) | want ({:.9}, {:.9}) | {err_m:.3e} m",
                            case.code, out.x, out.y, sample.geographic.0, sample.geographic.1
                        ));
                    }
                }
                Err(err) => failures.push(format!(
                    "EPSG:{} -> 4326 | point {idx} | transform failed: {err}",
                    case.code
                )),
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} forward-transform mismatches against PROJ 9.5.1 (tolerance {METRE_TOL:.0e} m):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Inverse: EPSG:4326 -> EPSG:code must reproduce the verified native
/// coordinates at all 5 points.
#[test]
fn extended_verified_codes_inverse_transform_matches_proj_951() {
    let cases = extended_cases();
    let mut failures: Vec<String> = Vec::new();

    for case in &cases {
        let transformer = match transformer_between(4326, case.code) {
            Ok(t) => t,
            Err(err) => {
                failures.push(format!(
                    "EPSG:4326 -> {} | transformer construction failed: {err}",
                    case.code
                ));
                continue;
            }
        };

        let geographic = case.is_geographic();
        for (idx, sample) in case.samples.iter().enumerate() {
            let input = Coordinate::from_lon_lat(sample.geographic.0, sample.geographic.1);
            match transformer.transform(&input) {
                Ok(out) => {
                    let err_m = residual_metres(geographic, (out.x, out.y), sample.xy);
                    if err_m > METRE_TOL {
                        failures.push(format!(
                            "EPSG:4326 -> {} | point {idx} | got ({:.9}, {:.9}) | want ({:.9}, {:.9}) | {err_m:.3e} m",
                            case.code, out.x, out.y, sample.xy.0, sample.xy.1
                        ));
                    }
                }
                Err(err) => failures.push(format!(
                    "EPSG:4326 -> {} | point {idx} | transform failed: {err}",
                    case.code
                )),
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} inverse-transform mismatches against PROJ 9.5.1 (tolerance {METRE_TOL:.0e} m):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Builds the projection-only transformer pair for a code: its own geodetic
/// base CRS (derived from the registry's PROJ string, so datum tokens match
/// exactly and no shift is applied) against the registry's projected CRS.
///
/// The two sides are named differently on purpose — `Crs::from_proj` for the
/// base, `Crs::from_epsg` for the projected CRS. Both must resolve to the same
/// datum in every feature configuration; see the "Feature invariance" section
/// of the module documentation for the `proj-db` regression this guards.
fn projection_pivot(code: u32) -> Result<(Crs, Crs), String> {
    let def = lookup_epsg(code).map_err(|e| format!("missing from registry: {e}"))?;
    let base =
        Crs::from_proj(geodetic_base_of(&def.proj_string)).map_err(|e| format!("base CRS: {e}"))?;
    let projected = Crs::from_epsg(code).map_err(|e| format!("projected CRS: {e}"))?;
    Ok((base, projected))
}

/// Projection-only forward: geodetic base -> code.
///
/// These codes cannot reach the tolerance through EPSG:4326 because PROJ
/// applies a database-selected datum shift that the registry's PROJ string
/// does not carry; this pivot removes the datum from the comparison so the
/// map projection itself stays pinned.
#[test]
fn projection_verified_codes_forward_matches_proj_951() {
    let cases = projection_cases();
    let mut failures: Vec<String> = Vec::new();

    for case in &cases {
        let (base, projected) = match projection_pivot(case.code) {
            Ok(pair) => pair,
            Err(err) => {
                failures.push(format!("EPSG:{} | {err}", case.code));
                continue;
            }
        };
        let transformer = match Transformer::new(base, projected) {
            Ok(t) => t.with_strict(false),
            Err(err) => {
                failures.push(format!(
                    "EPSG:{} | base -> code transformer failed: {err}",
                    case.code
                ));
                continue;
            }
        };

        for (idx, sample) in case.samples.iter().enumerate() {
            let input = Coordinate::from_lon_lat(sample.geographic.0, sample.geographic.1);
            match transformer.transform(&input) {
                Ok(out) => {
                    let err_m = residual_metres(false, (out.x, out.y), sample.xy);
                    if err_m > METRE_TOL {
                        failures.push(format!(
                            "EPSG:{} | base -> code | point {idx} | got ({:.6}, {:.6}) | want ({:.6}, {:.6}) | {err_m:.3e} m",
                            case.code, out.x, out.y, sample.xy.0, sample.xy.1
                        ));
                    }
                }
                Err(err) => failures.push(format!(
                    "EPSG:{} | base -> code | point {idx} | transform failed: {err}",
                    case.code
                )),
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} projection mismatches against PROJ 9.5.1 (tolerance {METRE_TOL:.0e} m):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Projection-only inverse: code -> geodetic base.
#[test]
fn projection_verified_codes_inverse_matches_proj_951() {
    let cases = projection_cases();
    let mut failures: Vec<String> = Vec::new();

    for case in &cases {
        let (base, projected) = match projection_pivot(case.code) {
            Ok(pair) => pair,
            Err(err) => {
                failures.push(format!("EPSG:{} | {err}", case.code));
                continue;
            }
        };
        let transformer = match Transformer::new(projected, base) {
            Ok(t) => t.with_strict(false),
            Err(err) => {
                failures.push(format!(
                    "EPSG:{} | code -> base transformer failed: {err}",
                    case.code
                ));
                continue;
            }
        };

        for (idx, sample) in case.samples.iter().enumerate() {
            let input = Coordinate::new(sample.xy.0, sample.xy.1);
            match transformer.transform(&input) {
                Ok(out) => {
                    let err_m = residual_metres(true, (out.x, out.y), sample.geographic);
                    if err_m > METRE_TOL {
                        failures.push(format!(
                            "EPSG:{} | code -> base | point {idx} | got ({:.9}, {:.9}) | want ({:.9}, {:.9}) | {err_m:.3e} m",
                            case.code, out.x, out.y, sample.geographic.0, sample.geographic.1
                        ));
                    }
                }
                Err(err) => failures.push(format!(
                    "EPSG:{} | code -> base | point {idx} | transform failed: {err}",
                    case.code
                )),
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} inverse projection mismatches against PROJ 9.5.1 (tolerance {METRE_TOL:.0e} m):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The Japan Plane Rectangular families are the reason this audit exists: a
/// `+lat_0=0` bug and a whole-family misassignment both landed here. Pin the
/// invariant directly so a regression names itself.
#[test]
fn japan_plane_rectangular_families_are_complete_and_share_zone_geometry() {
    for (index, zone) in [
        "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI", "XII", "XIII", "XIV",
        "XV", "XVI", "XVII", "XVIII", "XIX",
    ]
    .iter()
    .enumerate()
    {
        let jgd2000 = lookup_epsg(2443 + index as u32)
            .unwrap_or_else(|_| panic!("JGD2000 zone {zone} must be registered"));
        let jgd2011 = lookup_epsg(6669 + index as u32)
            .unwrap_or_else(|_| panic!("JGD2011 zone {zone} must be registered"));

        assert_eq!(
            jgd2000.name,
            format!("JGD2000 / Japan Plane Rectangular CS {zone}")
        );
        assert_eq!(
            jgd2011.name,
            format!("JGD2011 / Japan Plane Rectangular CS {zone}")
        );

        // Same zone geometry in both realizations — the drift that produced
        // the original bug is exactly this assertion failing.
        assert_eq!(
            jgd2000.proj_string, jgd2011.proj_string,
            "zone {zone}: JGD2000 and JGD2011 must share the projection"
        );
        assert!(
            !jgd2000.proj_string.contains("+lat_0=0 "),
            "zone {zone}: Japan Plane Rectangular CS never has +lat_0=0"
        );
    }

    // The UTM zones the Plane Rectangular block used to squat on.
    for zone in 51u32..=55 {
        let def = lookup_epsg(6637 + zone)
            .unwrap_or_else(|_| panic!("JGD2011 / UTM zone {zone}N must be registered"));
        assert_eq!(def.name, format!("JGD2011 / UTM zone {zone}N"));
        assert!(def.proj_string.contains(&format!("+zone={zone} ")));
    }
}
