//! Integration tests for PROJ-pipeline grid-shift steps
//! (`+proj=hgridshift` / `+proj=vgridshift`), exercising the public
//! [`Pipeline::with_hgrid`] / [`Pipeline::with_vgrid`] API.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use oxigeo_proj::grid_shift::ntv2::{NtV2Grid, NtV2Header, NtV2Record, NtV2SubGrid};
use oxigeo_proj::{Coordinate, Coordinate3D, Pipeline};

/// A tiny 2×2 NTv2 grid (positive-west convention) covering ~10°W, 60°N with a
/// uniform +10" latitude / +20" positive-west longitude shift.
fn tiny_ntv2_grid() -> Arc<NtV2Grid> {
    let rec = NtV2Record {
        lat_shift: 10.0,
        lon_shift: 20.0,
        lat_accuracy: 0.01,
        lon_accuracy: 0.01,
    };
    let sub = NtV2SubGrid {
        name: "TINY".into(),
        parent: "NONE".into(),
        south_lat: 216_000.0,
        north_lat: 216_060.0,
        east_lon: 36_000.0,
        west_lon: 36_060.0,
        lat_inc: 60.0,
        lon_inc: 60.0,
        gs_count: 4,
        records: vec![rec; 4],
        children: vec![],
    };
    Arc::new(NtV2Grid {
        overview: NtV2Header {
            num_file: 1,
            gs_type: "SECONDS".into(),
            version: "NTv2.0".into(),
            system_f: "NAD27".into(),
            system_t: "NAD83".into(),
            major_f: 6_378_206.4,
            minor_f: 6_356_583.8,
            major_t: 6_378_137.0,
            minor_t: 6_356_752.314_14,
        },
        sub_grids: vec![sub],
    })
}

#[test]
fn hgridshift_errors_without_registered_grid() {
    let p = Pipeline::from_proj_string("+proj=pipeline +step +proj=hgridshift +grids=TINY.gsb")
        .expect("parse");
    let err = p.transform(&Coordinate::new(-10.0, 60.0)).unwrap_err();
    assert!(
        format!("{err}").contains("not loaded"),
        "expected 'not loaded' error, got: {err}"
    );
}

#[test]
fn hgridshift_applies_registered_grid() {
    let p = Pipeline::from_proj_string("+proj=pipeline +step +proj=hgridshift +grids=TINY.gsb")
        .expect("parse")
        .with_hgrid("TINY.gsb", tiny_ntv2_grid());

    let out = p
        .transform(&Coordinate::new(-10.0, 60.0))
        .expect("hgridshift applies");
    // +10" lat, +20" positive-west lon → −20" east.
    assert!(
        (out.y - (60.0 + 10.0 / 3600.0)).abs() < 1e-9,
        "lat {}",
        out.y
    );
    assert!(
        (out.x - (-10.0 - 20.0 / 3600.0)).abs() < 1e-9,
        "lon {}",
        out.x
    );
}

#[test]
fn hgridshift_inverse_roundtrip() {
    let grid = tiny_ntv2_grid();
    let fwd = Pipeline::from_proj_string("+proj=pipeline +step +proj=hgridshift +grids=TINY.gsb")
        .expect("parse")
        .with_hgrid("TINY.gsb", Arc::clone(&grid));
    let inv =
        Pipeline::from_proj_string("+proj=pipeline +step +inv +proj=hgridshift +grids=TINY.gsb")
            .expect("parse")
            .with_hgrid("TINY.gsb", grid);

    let start = Coordinate::new(-10.005, 60.005);
    let shifted = fwd.transform(&start).expect("fwd");
    let back = inv.transform(&shifted).expect("inv");
    assert!((back.x - start.x).abs() < 1e-8, "lon roundtrip {}", back.x);
    assert!((back.y - start.y).abs() < 1e-8, "lat roundtrip {}", back.y);
}

#[test]
fn vgridshift_shifts_height() {
    use oxigeo_proj::{GeoidModel, synthetic_grid};
    let geoid = Arc::new(synthetic_grid(GeoidModel::Egm96));
    let lon = 5.0_f64;
    let lat = 45.0_f64;
    let n = geoid.geoid_height_m(lat, lon);

    let p = Pipeline::from_proj_string("+proj=pipeline +step +proj=vgridshift +grids=egm96.gtx")
        .expect("parse")
        .with_vgrid("egm96.gtx", geoid);

    // Forward: ellipsoidal → orthometric, h_out = h_in − N.
    let out = p
        .transform_3d(&Coordinate3D::new(lon, lat, 100.0))
        .expect("vgridshift applies");
    assert!(
        (out.z - (100.0 - n)).abs() < 1e-9,
        "z {} vs {}",
        out.z,
        100.0 - n
    );
    assert!((out.x - lon).abs() < 1e-12 && (out.y - lat).abs() < 1e-12);
}

#[test]
fn vgridshift_2d_passthrough_when_grid_loaded() {
    use oxigeo_proj::{GeoidModel, synthetic_grid};
    // With the grid loaded, a 2-D coordinate has no height to shift, so the
    // horizontal position passes through unchanged.
    let p = Pipeline::from_proj_string("+proj=pipeline +step +proj=vgridshift +grids=egm96.gtx")
        .expect("parse")
        .with_vgrid("egm96.gtx", Arc::new(synthetic_grid(GeoidModel::Egm96)));
    let out = p
        .transform(&Coordinate::new(5.0, 45.0))
        .expect("2-D vgridshift passthrough");
    assert!((out.x - 5.0).abs() < 1e-12 && (out.y - 45.0).abs() < 1e-12);
}

#[test]
fn vgridshift_2d_errors_when_grid_unloaded() {
    // A reference to an unregistered vertical grid must error (non-silent),
    // consistent with hgridshift.
    let p = Pipeline::from_proj_string("+proj=pipeline +step +proj=vgridshift +grids=egm96.gtx")
        .expect("parse");
    let err = p.transform(&Coordinate::new(5.0, 45.0)).unwrap_err();
    assert!(format!("{err}").contains("not loaded"), "got: {err}");
}
