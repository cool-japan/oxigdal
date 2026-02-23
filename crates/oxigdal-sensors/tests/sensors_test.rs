//! Integration tests for oxigdal-sensors

use oxigdal_sensors::indices::burn::{d_nbr, nbr};
use oxigdal_sensors::indices::urban::ndbi;
use oxigdal_sensors::indices::vegetation::{evi, msavi, ndvi, savi};
use oxigdal_sensors::indices::water::mndwi;
use oxigdal_sensors::radiometry::atmospheric::{AtmosphericCorrection, DarkObjectSubtraction};
use oxigdal_sensors::radiometry::calibration::{RadiometricCalibration, earth_sun_distance};
use oxigdal_sensors::sensors::landsat::{landsat5_tm, landsat7_etm_plus, landsat8_oli_tirs};
use oxigdal_sensors::sensors::sentinel::sentinel2_msi;
use scirs2_core::ndarray::array;

#[test]
fn test_landsat_sensors() {
    let l5 = landsat5_tm();
    assert_eq!(l5.bands.len(), 7);
    assert!(l5.get_band_by_common_name("NIR").is_some());

    let l7 = landsat7_etm_plus();
    assert_eq!(l7.bands.len(), 8);
    assert!(l7.get_band_by_common_name("Pan").is_some());

    let l8 = landsat8_oli_tirs();
    assert_eq!(l8.bands.len(), 11);
    assert!(l8.get_band_by_common_name("Coastal").is_some());
    assert!(l8.get_band_by_common_name("Cirrus").is_some());
}

#[test]
fn test_sentinel2() {
    let s2 = sentinel2_msi();
    assert_eq!(s2.bands.len(), 13);

    // Check red edge bands (unique to Sentinel-2)
    assert!(s2.get_band_by_common_name("RedEdge1").is_some());
    assert!(s2.get_band_by_common_name("RedEdge2").is_some());
    assert!(s2.get_band_by_common_name("RedEdge3").is_some());

    // Check 10m bands
    let red = s2.get_band_by_common_name("Red");
    assert!(red.is_some());
    if let Some(red) = red {
        assert_eq!(red.spatial_resolution, 10.0);
    }
}

#[test]
fn test_radiometric_calibration() {
    let cal = RadiometricCalibration::new(0.00002, 0.0).with_solar_irradiance(1554.0);

    let dn = array![[1000.0, 2000.0], [3000.0, 4000.0]];
    let radiance = cal.dn_to_radiance(&dn.view());

    assert!(!radiance.is_empty());
    assert!(radiance[[0, 0]] > 0.0);

    // Test TOA reflectance conversion
    let reflectance = cal.radiance_to_reflectance(&radiance.view(), 30.0, 1.0);
    assert!(reflectance.is_ok());
}

#[test]
fn test_earth_sun_distance() {
    let d1 = earth_sun_distance(1);
    assert!(d1.is_ok());

    let d180 = earth_sun_distance(180);
    assert!(d180.is_ok());

    let d365 = earth_sun_distance(365);
    assert!(d365.is_ok());

    // Invalid day
    let invalid = earth_sun_distance(400);
    assert!(invalid.is_err());
}

#[test]
fn test_atmospheric_correction() {
    let dos = DarkObjectSubtraction::default_params();
    let toa = array![[0.05, 0.10, 0.15], [0.06, 0.12, 0.18]];

    let corrected = dos.correct(&toa.view());
    assert!(corrected.is_ok());

    if let Ok(corrected) = corrected {
        // Dark objects should be reduced
        assert!(corrected[[0, 0]] <= toa[[0, 0]]);
    }
}

#[test]
fn test_vegetation_indices() {
    let nir = array![[0.5, 0.6], [0.7, 0.8]];
    let red = array![[0.1, 0.1], [0.1, 0.1]];
    let blue = array![[0.05, 0.05], [0.05, 0.05]];

    // NDVI
    let ndvi_result = ndvi(&nir.view(), &red.view());
    assert!(ndvi_result.is_ok());
    if let Ok(ndvi_val) = ndvi_result {
        assert!(ndvi_val[[0, 0]] > 0.0 && ndvi_val[[0, 0]] < 1.0);
    }

    // EVI
    let evi_result = evi(&nir.view(), &red.view(), &blue.view());
    assert!(evi_result.is_ok());
    if let Ok(evi_val) = evi_result {
        assert!(evi_val[[0, 0]].is_finite());
    }

    // SAVI
    let savi_result = savi(&nir.view(), &red.view(), 0.5);
    assert!(savi_result.is_ok());
    if let Ok(savi_val) = savi_result {
        assert!(savi_val[[0, 0]].is_finite());
    }

    // MSAVI
    let msavi_result = msavi(&nir.view(), &red.view());
    assert!(msavi_result.is_ok());
    if let Ok(msavi_val) = msavi_result {
        assert!(msavi_val[[0, 0]] >= 0.0);
    }
}

#[test]
fn test_burn_indices() {
    let nir = array![[0.5, 0.6]];
    let swir2 = array![[0.2, 0.2]];

    // NBR
    let nbr_result = nbr(&nir.view(), &swir2.view());
    assert!(nbr_result.is_ok());
    if let Ok(nbr_val) = nbr_result {
        assert!(nbr_val[[0, 0]] > 0.0);
    }

    // dNBR (change detection)
    let prefire = array![[0.6, 0.7]];
    let postfire = array![[0.1, 0.2]];

    let dnbr_result = d_nbr(&prefire.view(), &postfire.view());
    assert!(dnbr_result.is_ok());
    if let Ok(dnbr_val) = dnbr_result {
        assert!(dnbr_val[[0, 0]] > 0.0); // Indicates burn
    }
}

#[test]
fn test_urban_indices() {
    let swir1 = array![[0.4, 0.4]];
    let nir = array![[0.2, 0.2]];

    let ndbi_result = ndbi(&swir1.view(), &nir.view());
    assert!(ndbi_result.is_ok());
    if let Ok(ndbi_val) = ndbi_result {
        assert!(ndbi_val[[0, 0]] > 0.0); // Built-up areas
    }
}

#[test]
fn test_water_indices() {
    let green = array![[0.3, 0.3]];
    let swir1 = array![[0.1, 0.1]];

    let mndwi_result = mndwi(&green.view(), &swir1.view());
    assert!(mndwi_result.is_ok());
    if let Ok(mndwi_val) = mndwi_result {
        assert!(mndwi_val[[0, 0]] > 0.0); // Water has positive MNDWI
    }
}

#[test]
fn test_complete_workflow() {
    // Simulate a complete Landsat 8 NDVI workflow
    let l8 = landsat8_oli_tirs();

    // Get band information
    let nir_band = l8.get_band_by_common_name("NIR");
    let red_band = l8.get_band_by_common_name("Red");

    assert!(nir_band.is_some());
    assert!(red_band.is_some());

    // Simulate DN values
    let nir_dn = array![[10000.0, 12000.0], [15000.0, 18000.0]];
    let red_dn = array![[8000.0, 8500.0], [9000.0, 9500.0]];

    // Radiometric calibration (simplified - real calibration would use band-specific parameters)
    let cal = RadiometricCalibration::new(0.00002, 0.0).with_solar_irradiance(1554.0);

    let nir_toa = cal.dn_to_reflectance(&nir_dn.view(), 30.0, 1.0);
    let red_toa = cal.dn_to_reflectance(&red_dn.view(), 30.0, 1.0);

    assert!(nir_toa.is_ok());
    assert!(red_toa.is_ok());

    // Atmospheric correction
    let dos = DarkObjectSubtraction::default_params();

    if let (Ok(nir_toa), Ok(red_toa)) = (nir_toa, red_toa) {
        let nir_corrected = dos.correct(&nir_toa.view());
        let red_corrected = dos.correct(&red_toa.view());

        assert!(nir_corrected.is_ok());
        assert!(red_corrected.is_ok());

        if let (Ok(nir_corrected), Ok(red_corrected)) = (nir_corrected, red_corrected) {
            // Calculate NDVI
            let ndvi_result = ndvi(&nir_corrected.view(), &red_corrected.view());
            assert!(ndvi_result.is_ok());

            if let Ok(ndvi_val) = ndvi_result {
                // Healthy vegetation should have NDVI > 0.2
                assert!(ndvi_val.iter().any(|&v| v > 0.2));
            }
        }
    }
}
