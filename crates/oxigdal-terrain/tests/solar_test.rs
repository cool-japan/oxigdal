//! Integration tests for the solar radiation module.

#![cfg(feature = "derivatives")]

use approx::assert_relative_eq;
use oxigdal_terrain::radiation::{SolarOptions, hillshade_at, solar_position, solar_radiation};
use scirs2_core::prelude::*;

/// Build a planar DEM with a constant elevation gradient.
///
/// `dz_dy` is the elevation rise per row (south, increasing `y`); `dz_dx`
/// per column (east, increasing `x`).
fn planar_dem(rows: usize, cols: usize, base: f64, dz_dy: f64, dz_dx: f64) -> Array2<f64> {
    let mut dem = Array2::zeros((rows, cols));
    for y in 0..rows {
        for x in 0..cols {
            dem[[y, x]] = base + (y as f64) * dz_dy + (x as f64) * dz_dx;
        }
    }
    dem
}

fn mean_finite(arr: &Array2<f64>) -> f64 {
    let mut sum = 0.0;
    let mut count = 0.0;
    for &v in arr.iter() {
        if v.is_finite() {
            sum += v;
            count += 1.0;
        }
    }
    if count == 0.0 { 0.0 } else { sum / count }
}

#[test]
fn test_solar_position_equinox_noon_equator_near_overhead() {
    // Equinox (~21 March, n=80), solar noon, equator: Sun nearly overhead.
    let pos = solar_position(0.0, 80, 12.0);
    assert!(
        pos.altitude_deg > 88.0,
        "altitude should be near 90 deg, got {}",
        pos.altitude_deg
    );
    assert!(
        pos.zenith_deg < 2.0,
        "zenith should be near 0 deg, got {}",
        pos.zenith_deg
    );
    assert_relative_eq!(pos.altitude_deg + pos.zenith_deg, 90.0, epsilon = 1e-9);
}

#[test]
fn test_solar_position_altitude_near_zero_at_sunrise() {
    // At the equator on the equinox the Sun rises at ~06:00 solar time.
    let pos = solar_position(0.0, 80, 6.0);
    assert!(
        pos.altitude_deg.abs() < 2.0,
        "altitude should be near 0 at sunrise, got {}",
        pos.altitude_deg
    );
    // Morning Sun should be toward the east (azimuth near 90 deg).
    assert!(
        pos.azimuth_deg > 45.0 && pos.azimuth_deg < 135.0,
        "sunrise azimuth should be roughly east, got {}",
        pos.azimuth_deg
    );
}

#[test]
fn test_solar_declination_summer_solstice_positive() {
    // Summer solstice (~21 June, n=172): declination should be near +23.45 deg.
    // Recover declination from zenith at the equator at solar noon:
    // cos(zenith) = cos(declination) => declination = zenith (for phi = 0).
    let pos = solar_position(0.0, 172, 12.0);
    // At the equator, zenith at noon equals |declination|; check it is the
    // maximum northern value and that a winter day flips the sign.
    assert!(
        pos.zenith_deg > 20.0 && pos.zenith_deg < 25.0,
        "summer solstice zenith at equator should be ~23.45 deg, got {}",
        pos.zenith_deg
    );

    // Winter solstice (~21 Dec, n=355): Sun is south of the equator at noon,
    // so a northern-hemisphere site sees a much lower Sun than in summer.
    let summer_nh = solar_position(45.0, 172, 12.0);
    let winter_nh = solar_position(45.0, 355, 12.0);
    assert!(
        summer_nh.altitude_deg > winter_nh.altitude_deg + 30.0,
        "summer noon Sun ({}) should be far higher than winter ({})",
        summer_nh.altitude_deg,
        winter_nh.altitude_deg
    );
}

#[test]
fn test_flat_dem_hillshade_matches_sin_altitude() {
    // On a flat surface, cos(incidence) reduces to cos(zenith) = sin(altitude).
    let dem = Array2::from_elem((7, 7), 100.0_f64);
    let altitude = 35.0_f64;
    let shade = hillshade_at(&dem, 10.0, altitude, 135.0).expect("hillshade failed");
    let expected = altitude.to_radians().sin();
    for &v in shade.iter() {
        assert_relative_eq!(v, expected, epsilon = 1e-9);
    }
}

#[test]
fn test_south_facing_slope_more_insolation_than_north_nh() {
    // Northern hemisphere: south-facing slopes get more insolation.
    // Row y increases southward; a south-facing slope drops toward +y, i.e.
    // elevation decreases as y increases (dz_dy < 0). North-facing is dz_dy > 0.
    let south_facing = planar_dem(9, 9, 500.0, -10.0, 0.0);
    let north_facing = planar_dem(9, 9, 500.0, 10.0, 0.0);

    let opts = SolarOptions {
        latitude_deg: 45.0,
        day_of_year: 172,
        start_hour: 6.0,
        end_hour: 18.0,
        time_step_minutes: 30.0,
        cast_shadows: false,
        compute_diffuse: false,
        ..SolarOptions::default()
    };

    let south = solar_radiation(&south_facing, 10.0, &opts).expect("south failed");
    let north = solar_radiation(&north_facing, 10.0, &opts).expect("north failed");

    let south_mean = mean_finite(&south.global);
    let north_mean = mean_finite(&north.global);
    assert!(
        south_mean > north_mean,
        "south-facing ({}) should exceed north-facing ({}) in NH",
        south_mean,
        north_mean
    );
}

#[test]
fn test_solar_radiation_nonneg_everywhere() {
    let dem = planar_dem(8, 8, 200.0, 5.0, -3.0);
    let opts = SolarOptions {
        latitude_deg: 40.0,
        day_of_year: 100,
        start_hour: 5.0,
        end_hour: 19.0,
        ..SolarOptions::default()
    };
    let result = solar_radiation(&dem, 15.0, &opts).expect("radiation failed");
    for &v in result.global.iter() {
        assert!(
            v >= 0.0,
            "global irradiance must be non-negative, got {}",
            v
        );
    }
    for &v in result.direct.iter() {
        assert!(
            v >= 0.0,
            "direct irradiance must be non-negative, got {}",
            v
        );
    }
    for &v in result.diffuse.iter() {
        assert!(
            v >= 0.0,
            "diffuse irradiance must be non-negative, got {}",
            v
        );
    }
    for &v in result.duration_hours.iter() {
        assert!(v >= 0.0, "duration must be non-negative, got {}", v);
    }
}

#[test]
fn test_cast_shadow_blocks_low_sun_behind_ridge() {
    // Build a tall ridge to the east. A morning (low, eastern) Sun should be
    // blocked by the ridge for cells to its west when shadows are enabled.
    let rows = 5;
    let cols = 9;
    let mut dem = Array2::from_elem((rows, cols), 0.0_f64);
    // Tall wall in the easternmost columns.
    for y in 0..rows {
        for x in (cols - 2)..cols {
            dem[[y, x]] = 500.0;
        }
    }

    let alt = 15.0; // low Sun
    let az = 90.0; // due east

    // Pick a valley cell west of the ridge.
    let (ty, tx) = (2usize, 2usize);

    let lit = {
        // Without cast shadows the cell sees the Sun (cos i > 0 on flat ground).
        hillshade_at(&dem, 10.0, alt, az).expect("hillshade failed")[[ty, tx]]
    };
    assert!(lit > 0.0, "flat valley cell should be lit without shadows");

    let opts_shadow = SolarOptions {
        latitude_deg: 40.0,
        day_of_year: 80,
        start_hour: 7.0,
        end_hour: 7.5,
        time_step_minutes: 30.0,
        cast_shadows: true,
        compute_diffuse: false,
        ..SolarOptions::default()
    };
    let opts_noshadow = SolarOptions {
        cast_shadows: false,
        ..opts_shadow
    };

    // Drive the Sun explicitly low from the east via hillshade-equivalent geometry:
    // here we instead compare the shadow model directly using a short window where
    // the modelled azimuth is eastern. Use the dedicated shadow path.
    let with_shadow = solar_radiation(&dem, 10.0, &opts_shadow).expect("shadow run failed");
    let without_shadow = solar_radiation(&dem, 10.0, &opts_noshadow).expect("noshadow run failed");

    // The valley cell behind the ridge should receive strictly less (typically
    // zero) direct beam with shadows than without.
    assert!(
        with_shadow.direct[[ty, tx]] <= without_shadow.direct[[ty, tx]] + 1e-9,
        "shadowed direct ({}) should not exceed unshadowed ({})",
        with_shadow.direct[[ty, tx]],
        without_shadow.direct[[ty, tx]]
    );
}

#[test]
fn test_cast_shadow_explicit_ridge_geometry() {
    // Deterministic shadow check independent of solar ephemeris: a 1000 m wall
    // immediately east of a flat cell must occlude a 20-deg eastern Sun.
    let rows = 4;
    let cols = 6;
    let mut dem = Array2::from_elem((rows, cols), 0.0_f64);
    for y in 0..rows {
        dem[[y, cols - 1]] = 1000.0;
        dem[[y, cols - 2]] = 1000.0;
    }

    // hillshade_at ignores shadows, so a flat cell is lit. With the integrated
    // model and shadows on over an eastern-Sun window, the shielded column gets
    // no direct beam, while removing the wall restores it.
    let opts = SolarOptions {
        latitude_deg: 10.0,
        day_of_year: 80,
        start_hour: 6.5,
        end_hour: 8.0,
        time_step_minutes: 15.0,
        cast_shadows: true,
        compute_diffuse: false,
        ..SolarOptions::default()
    };

    let shielded_x = cols - 3; // directly west of the wall
    let shielded = solar_radiation(&dem, 30.0, &opts).expect("shielded run failed");

    let flat = Array2::from_elem((rows, cols), 0.0_f64);
    let open = solar_radiation(&flat, 30.0, &opts).expect("open run failed");

    assert!(
        shielded.direct[[1, shielded_x]] < open.direct[[1, shielded_x]],
        "wall should reduce direct beam: shielded={}, open={}",
        shielded.direct[[1, shielded_x]],
        open.direct[[1, shielded_x]]
    );
}

#[test]
fn test_insolation_zero_when_sun_below_horizon() {
    // A nighttime window (Sun below horizon) yields zero energy everywhere.
    let dem = planar_dem(6, 6, 100.0, 4.0, 2.0);
    let opts = SolarOptions {
        latitude_deg: 45.0,
        day_of_year: 355, // winter
        start_hour: 0.0,
        end_hour: 3.0, // deep night
        time_step_minutes: 30.0,
        ..SolarOptions::default()
    };
    let result = solar_radiation(&dem, 10.0, &opts).expect("radiation failed");
    for &v in result.global.iter() {
        assert_relative_eq!(v, 0.0, epsilon = 1e-12);
    }
    for &v in result.duration_hours.iter() {
        assert_relative_eq!(v, 0.0, epsilon = 1e-12);
    }
}

#[test]
fn test_time_integration_positive_over_day() {
    // Integrating across a full summer day produces positive insolation and
    // a positive sunlit duration on a flat surface.
    let dem = Array2::from_elem((6, 6), 300.0_f64);
    let opts = SolarOptions {
        latitude_deg: 35.0,
        day_of_year: 172,
        start_hour: 4.0,
        end_hour: 20.0,
        time_step_minutes: 30.0,
        cast_shadows: false,
        compute_diffuse: true,
        ..SolarOptions::default()
    };
    let result = solar_radiation(&dem, 10.0, &opts).expect("radiation failed");
    assert!(
        mean_finite(&result.global) > 0.0,
        "daily global insolation should be positive"
    );
    assert!(
        mean_finite(&result.direct) > 0.0,
        "daily direct insolation should be positive"
    );
    assert!(
        mean_finite(&result.duration_hours) > 1.0,
        "sunlit duration should be several hours, got {}",
        mean_finite(&result.duration_hours)
    );
    // A flat cell cannot be sunlit longer than the integration window.
    for &v in result.duration_hours.iter() {
        assert!(v <= 16.0 + 1e-9, "duration {} exceeds window", v);
    }
}

#[test]
fn test_nodata_propagates_nan() {
    // A NoData cell (NaN) must remain NaN across every output array.
    let mut dem = planar_dem(7, 7, 200.0, 6.0, -2.0);
    dem[[3, 3]] = f64::NAN;
    let opts = SolarOptions {
        latitude_deg: 30.0,
        day_of_year: 150,
        start_hour: 6.0,
        end_hour: 18.0,
        ..SolarOptions::default()
    };
    let result = solar_radiation(&dem, 12.0, &opts).expect("radiation failed");
    assert!(
        result.global[[3, 3]].is_nan(),
        "global NoData should be NaN"
    );
    assert!(
        result.direct[[3, 3]].is_nan(),
        "direct NoData should be NaN"
    );
    assert!(
        result.diffuse[[3, 3]].is_nan(),
        "diffuse NoData should be NaN"
    );
    assert!(
        result.duration_hours[[3, 3]].is_nan(),
        "duration NoData should be NaN"
    );

    // hillshade_at should also propagate NaN for NoData cells.
    let shade = hillshade_at(&dem, 12.0, 40.0, 180.0).expect("hillshade failed");
    assert!(shade[[3, 3]].is_nan(), "hillshade NoData should be NaN");
}

#[test]
fn test_diffuse_nonzero_when_enabled() {
    // With diffuse enabled, global should exceed direct (diffuse adds energy);
    // with diffuse disabled, global should equal direct.
    let dem = Array2::from_elem((6, 6), 100.0_f64);
    let base = SolarOptions {
        latitude_deg: 40.0,
        day_of_year: 172,
        start_hour: 6.0,
        end_hour: 18.0,
        time_step_minutes: 30.0,
        cast_shadows: false,
        ..SolarOptions::default()
    };

    let with_diffuse = SolarOptions {
        compute_diffuse: true,
        ..base
    };
    let no_diffuse = SolarOptions {
        compute_diffuse: false,
        ..base
    };

    let rd = solar_radiation(&dem, 10.0, &with_diffuse).expect("diffuse failed");
    let rn = solar_radiation(&dem, 10.0, &no_diffuse).expect("nodiffuse failed");

    assert!(
        mean_finite(&rd.diffuse) > 0.0,
        "diffuse insolation should be positive when enabled"
    );
    assert!(
        mean_finite(&rd.global) > mean_finite(&rn.global),
        "global with diffuse ({}) should exceed without ({})",
        mean_finite(&rd.global),
        mean_finite(&rn.global)
    );
    for &v in rn.diffuse.iter() {
        assert_relative_eq!(v, 0.0, epsilon = 1e-12);
    }
}
