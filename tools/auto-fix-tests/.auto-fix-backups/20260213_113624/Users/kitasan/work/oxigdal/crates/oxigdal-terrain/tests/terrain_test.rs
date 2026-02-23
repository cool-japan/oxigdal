//! Integration tests for oxigdal-terrain.

use oxigdal_terrain::*;
use scirs2_core::prelude::*;

#[test]
fn test_slope_calculation() {
    let mut dem = Array2::zeros((10, 10));
    for y in 0..10 {
        for x in 0..10 {
            dem[[y, x]] = (x as f64) * 10.0;
        }
    }

    let slope_result = derivatives::slope_horn(&dem, 10.0, derivatives::SlopeUnits::Degrees, None);
    assert!(slope_result.is_ok());

    let slope = slope_result.expect("slope calculation failed");
    assert_eq!(slope.dim(), (10, 10));
}

#[test]
fn test_aspect_calculation() {
    let mut dem = Array2::zeros((10, 10));
    for y in 0..10 {
        for x in 0..10 {
            dem[[y, x]] = (y as f64) * 10.0;
        }
    }

    let aspect = derivatives::aspect_horn(&dem, 10.0, derivatives::FlatHandling::NoDirection, None)
        .expect("aspect calculation failed");

    assert_eq!(aspect.dim(), (10, 10));
}

#[test]
fn test_hillshade() {
    let mut dem = Array2::zeros((10, 10));
    for y in 0..10 {
        for x in 0..10 {
            dem[[y, x]] = 100.0 + (x as f64) * 5.0;
        }
    }

    let hillshade = derivatives::hillshade_traditional(&dem, 10.0, 315.0, 45.0, 1.0, None)
        .expect("hillshade calculation failed");

    assert_eq!(hillshade.dim(), (10, 10));
}

#[test]
fn test_curvature() {
    let dem = Array2::from_elem((10, 10), 100.0_f64);

    let profile =
        derivatives::profile_curvature(&dem, 10.0, None).expect("profile curvature failed");
    let plan = derivatives::plan_curvature(&dem, 10.0, None).expect("plan curvature failed");
    let total = derivatives::total_curvature(&dem, 10.0, None).expect("total curvature failed");

    assert_eq!(profile.dim(), (10, 10));
    assert_eq!(plan.dim(), (10, 10));
    assert_eq!(total.dim(), (10, 10));
}

#[test]
fn test_tpi() {
    let dem = Array2::from_elem((10, 10), 100.0_f64);

    let tpi = derivatives::tpi(&dem, 1, None).expect("TPI calculation failed");

    assert_eq!(tpi.dim(), (10, 10));
}

#[test]
fn test_tri() {
    let dem = Array2::from_elem((10, 10), 100.0_f64);

    let tri = derivatives::tri(&dem, None).expect("TRI calculation failed");

    assert_eq!(tri.dim(), (10, 10));
}

#[test]
fn test_roughness() {
    let dem = Array2::from_elem((10, 10), 100.0_f64);

    let rough = derivatives::roughness_stddev(&dem, 1, None).expect("roughness calculation failed");

    assert_eq!(rough.dim(), (10, 10));
}

#[cfg(feature = "hydrology")]
#[test]
fn test_flow_direction() {
    let mut dem = Array2::zeros((10, 10));
    for y in 0..10 {
        for x in 0..10 {
            dem[[y, x]] = 100.0 - (x as f64);
        }
    }

    let flow_dir = hydrology::flow_direction_d8(&dem, 10.0, None).expect("flow direction failed");

    assert_eq!(flow_dir.dim(), (10, 10));
}

#[cfg(feature = "hydrology")]
#[test]
fn test_flow_accumulation() {
    let mut dem = Array2::zeros((5, 5));
    for y in 0..5 {
        for x in 0..5 {
            dem[[y, x]] = 100.0 - (x as f64);
        }
    }

    let accum = hydrology::flow_accumulation(&dem, 10.0, None).expect("flow accumulation failed");

    assert_eq!(accum.dim(), (5, 5));
}

#[cfg(feature = "hydrology")]
#[test]
fn test_sink_fill() {
    let mut dem = Array2::from_elem((5, 5), 100.0_f64);
    dem[[2, 2]] = 50.0; // Create a sink

    let filled = hydrology::fill_sinks(&dem, None).expect("sink fill failed");

    assert_eq!(filled.dim(), (5, 5));
    // Sink should be filled
    assert!(filled[[2, 2]] > 50.0);
}

#[cfg(feature = "visibility")]
#[test]
fn test_viewshed() {
    let dem = Array2::from_elem((20, 20), 100.0_f64);

    let viewshed = visibility::viewshed_binary(&dem, 10.0, 10, 10, 2.0, 0.0, Some(200.0), None)
        .expect("viewshed calculation failed");

    assert_eq!(viewshed.dim(), (20, 20));
    // Observer position should be visible
    assert_eq!(viewshed[[10, 10]], 1);
}

#[cfg(feature = "geomorphometry")]
#[test]
fn test_landform_classification() {
    let dem = Array2::from_elem((20, 20), 100.0_f64);

    let landforms = geomorphometry::classify_weiss(&dem, 10.0, 1, 3, None)
        .expect("landform classification failed");

    assert_eq!(landforms.dim(), (20, 20));
}

#[cfg(feature = "geomorphometry")]
#[test]
fn test_convergence() {
    let dem = Array2::from_elem((10, 10), 100.0_f64);

    let conv = geomorphometry::convergence_index(&dem, 10.0, None)
        .expect("convergence calculation failed");

    assert_eq!(conv.dim(), (10, 10));
}

#[cfg(feature = "geomorphometry")]
#[test]
fn test_openness() {
    let dem = Array2::from_elem((10, 10), 100.0_f64);

    let pos_open =
        geomorphometry::positive_openness(&dem, 10.0, 3, None).expect("positive openness failed");
    let neg_open =
        geomorphometry::negative_openness(&dem, 10.0, 3, None).expect("negative openness failed");

    assert_eq!(pos_open.dim(), (10, 10));
    assert_eq!(neg_open.dim(), (10, 10));
}
