//! Verified EPSG registry data tests — 153 codes, five-point-verified against
//! PROJ 9.5.1.
//!
//! Provenance
//! ----------
//! `tests/data/verified_epsg_5pt.json` embeds 153 EPSG codes, each carrying
//! the authoritative PROJ string plus 5 sample points (the area-of-use
//! centroid and the 4 corners inset 10%). Every `(code, point)` pair was
//! verified against PROJ 9.5.1 (via pyproj, `always_xy=true`) with a residual
//! below 1e-9 m, 2026-08. The set covers the geographic family (WGS 84,
//! NAD83, ETRS89, GDA94/GDA2020, JGD2000/JGD2011), the Japan Plane
//! Rectangular CS family (JGD2000 zones VIII-X, JGD2011 zones I-X), common
//! projected systems (Pseudo-/World Mercator, Lambert-93, LCC/LAEA Europe,
//! TM35FIN, NZTM2000, BC/Conus Albers, ETRS89 + NAD83 UTM) and all 120
//! WGS 84 UTM zones.
//!
//! These tests resolve every code through the public registry API
//! (`lookup_epsg` / `Transformer::from_epsg`) and assert that the registry's
//! definition reproduces the PROJ 9.5.1 coordinates at all five points, in
//! both directions. They exist to keep the embedded registry pinned to
//! externally-verified data: a regression that re-introduces a misassigned
//! row (e.g. the former "JGD2011 / UTM zone 51N-60N" block occupying EPSG
//! 6669-6678, or the `+lat_0=0` Japan Plane Rectangular bug that placed
//! Tokyo 4,007 km into the Pacific) fails loudly here.

#![cfg(feature = "std")]
#![allow(clippy::expect_used, clippy::panic)]

use oxigeo_proj::lookup_epsg;
use oxigeo_proj::transform::{Coordinate, Transformer};

/// Verified ground truth: code -> { name, proj, samples: [{xy, expect}; 5] }.
const VERIFIED_JSON: &str = include_str!("data/verified_epsg_5pt.json");

/// Maximum allowed residual, in metres, between the registry-driven transform
/// and the PROJ 9.5.1 reference coordinates. The measured worst case across
/// all 153 codes x 5 points x 2 directions is 5.9e-9 m; 1e-6 m (1 micrometre)
/// leaves a ~170x margin for platform round-off differences while staying 6+
/// orders of magnitude below any real-world discrepancy.
const METRE_TOL: f64 = 1e-6;

/// WGS 84 equatorial metres per degree (2 * pi * a / 360, a = 6378137).
const M_PER_DEG: f64 = 111_319.490_793_273_57;

struct Sample {
    /// Coordinate in the CRS under test (projected metres, or lon/lat degrees
    /// for geographic codes).
    xy: (f64, f64),
    /// The same point in EPSG:4326 (lon, lat) as computed by PROJ 9.5.1.
    expect: (f64, f64),
}

struct VerifiedCase {
    code: u32,
    name: String,
    proj: String,
    samples: Vec<Sample>,
}

impl VerifiedCase {
    /// A code is geographic when its verified PROJ string is a `longlat` CRS.
    fn is_geographic(&self) -> bool {
        self.proj.contains("+proj=longlat")
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

fn load_verified_cases() -> Vec<VerifiedCase> {
    let root: serde_json::Value =
        serde_json::from_str(VERIFIED_JSON).expect("verified_epsg_5pt.json must parse");
    let map = root.as_object().expect("top level must be an object");

    let mut cases: Vec<VerifiedCase> = map
        .iter()
        .map(|(code, entry)| {
            let code: u32 = code.parse().expect("EPSG code key must be numeric");
            let samples: Vec<Sample> = entry["samples"]
                .as_array()
                .expect("samples must be an array")
                .iter()
                .map(|s| Sample {
                    xy: pair(&s["xy"]),
                    expect: pair(&s["expect"]),
                })
                .collect();
            VerifiedCase {
                code,
                name: entry["name"].as_str().expect("name").to_string(),
                proj: entry["proj"].as_str().expect("proj").to_string(),
                samples,
            }
        })
        .collect();
    cases.sort_by_key(|c| c.code);

    assert_eq!(cases.len(), 153, "the verified data set holds 153 codes");
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

/// Builds a registry-driven transformer with the strict area-of-use policy
/// disabled.
///
/// The verified reference coordinates were produced by PROJ 9.5.1, which
/// transforms points regardless of the CRS's declared area of use, and the
/// sampling grid straddles areas of use that cross the antimeridian (e.g.
/// NAD83's, which spans 167.65°E to 47.74°W). These tests audit the
/// registry's *data*, not the transformer's area-of-use policy, so the
/// policy is turned off.
fn transformer_between(src: u32, dst: u32) -> oxigeo_proj::Result<Transformer> {
    Ok(Transformer::from_epsg(src, dst)?.with_strict(false))
}

/// Residual between `got` and `want`, expressed in metres.
///
/// For geographic coordinates the degree deltas are scaled to metres on the
/// WGS 84 equatorial circle (longitude additionally by cos(latitude)), which
/// keeps the tolerance physically meaningful across the whole latitude range.
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

/// Every verified code must resolve through the public registry API with the
/// authoritative name, and its definition must agree with the verified PROJ
/// string on the CRS class (geographic vs projected) and unit.
#[test]
fn verified_codes_resolve_with_correct_metadata() {
    let cases = load_verified_cases();
    let mut failures: Vec<String> = Vec::new();

    for case in &cases {
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

        let want_geographic = case.is_geographic();
        let got_geographic = def.proj_string.contains("+proj=longlat");
        if got_geographic != want_geographic {
            failures.push(format!(
                "EPSG:{} | class | registry proj: {:?} | verified proj: {:?}",
                case.code, def.proj_string, case.proj
            ));
        }

        let want_unit = if want_geographic { "degree" } else { "metre" };
        if def.unit != want_unit {
            failures.push(format!(
                "EPSG:{} | unit | registry: {:?} | verified: {:?}",
                case.code, def.unit, want_unit
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

/// Forward direction: EPSG:code -> EPSG:4326 through `Transformer::from_epsg`
/// must reproduce the PROJ 9.5.1 lon/lat at all five verified points.
#[test]
fn verified_codes_forward_transform_matches_proj_951() {
    let cases = load_verified_cases();
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
                    let err_m = residual_metres(true, (out.x, out.y), sample.expect);
                    if err_m > METRE_TOL {
                        failures.push(format!(
                            "EPSG:{} -> 4326 | point {idx} | got ({:.9}, {:.9}) | want ({:.9}, {:.9}) | {err_m:.3e} m",
                            case.code, out.x, out.y, sample.expect.0, sample.expect.1
                        ));
                    }
                }
                Err(err) => {
                    failures.push(format!(
                        "EPSG:{} -> 4326 | point {idx} | transform failed: {err}",
                        case.code
                    ));
                }
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

/// Inverse direction: EPSG:4326 -> EPSG:code must reproduce the verified
/// native coordinates at all five points.
#[test]
fn verified_codes_inverse_transform_matches_proj_951() {
    let cases = load_verified_cases();
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
            let input = Coordinate::from_lon_lat(sample.expect.0, sample.expect.1);
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
                Err(err) => {
                    failures.push(format!(
                        "EPSG:4326 -> {} | point {idx} | transform failed: {err}",
                        case.code
                    ));
                }
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
