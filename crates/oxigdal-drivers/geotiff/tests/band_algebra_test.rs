//! Tests for raster band algebra and spectral indices.

use oxigdal_geotiff::band_algebra::{
    AlgebraError, Band, BandMath, BandStack, NodataMask, SpectralIndex, ThresholdClassifier,
};

// ─── Helpers ──────────────────────────────────────────────────────────────

/// Creates a 2×2 band from four values (row-major).
fn band2x2(a: f64, b: f64, c: f64, d: f64) -> Band {
    Band::new(vec![a, b, c, d], 2, 2)
}

/// Creates a 2×2 band with a nodata sentinel.
fn band2x2_nd(a: f64, b: f64, c: f64, d: f64, nodata: f64) -> Band {
    Band::new(vec![a, b, c, d], 2, 2).with_nodata(nodata)
}

/// Creates a 1×N band (single row).
fn band1xn(values: Vec<f64>) -> Band {
    let n = values.len() as u32;
    Band::new(values, n, 1)
}

const ND: f64 = -9999.0;

// ─── Band basics ──────────────────────────────────────────────────────────

#[test]
fn test_band_new_dimensions() {
    let b = Band::new(vec![1.0; 6], 3, 2);
    assert_eq!(b.width, 3);
    assert_eq!(b.height, 2);
    assert_eq!(b.pixel_count(), 6);
}

#[test]
fn test_band_with_nodata_sets_sentinel() {
    let b = Band::new(vec![1.0, ND, 3.0], 3, 1).with_nodata(ND);
    assert_eq!(b.nodata, Some(ND));
}

#[test]
fn test_band_with_name() {
    let b = Band::new(vec![1.0], 1, 1).with_name("Red");
    assert_eq!(b.name.as_deref(), Some("Red"));
}

#[test]
fn test_band_pixel_count() {
    let b = Band::new(vec![0.0; 12], 4, 3);
    assert_eq!(b.pixel_count(), 12);
}

#[test]
fn test_band_valid_count_no_nodata() {
    let b = band2x2(1.0, 2.0, 3.0, 4.0);
    assert_eq!(b.valid_count(), 4);
}

#[test]
fn test_band_valid_count_with_nodata() {
    let b = band2x2_nd(1.0, ND, 3.0, ND, ND);
    assert_eq!(b.valid_count(), 2);
}

#[test]
fn test_band_valid_count_nan_nodata() {
    // When nodata is None, NaN pixels are treated as nodata.
    let b = Band::new(vec![1.0, f64::NAN, 3.0], 3, 1);
    assert_eq!(b.valid_count(), 2);
}

#[test]
fn test_band_is_nodata_exact_match() {
    let b = Band::new(vec![ND], 1, 1).with_nodata(ND);
    assert!(b.is_nodata(ND));
    assert!(!b.is_nodata(0.0));
}

#[test]
fn test_band_is_nodata_nan_sentinel() {
    let b = Band::new(vec![f64::NAN], 1, 1).with_nodata(f64::NAN);
    assert!(b.is_nodata(f64::NAN));
    assert!(!b.is_nodata(0.0));
}

#[test]
fn test_band_is_nodata_none_only_nan() {
    let b = Band::new(vec![0.0], 1, 1); // no nodata set
    assert!(!b.is_nodata(0.0));
    assert!(b.is_nodata(f64::NAN));
}

#[test]
fn test_band_pixel_at_valid() {
    let b = band2x2(10.0, 20.0, 30.0, 40.0);
    assert_eq!(b.pixel_at(0, 0), Some(10.0));
    assert_eq!(b.pixel_at(1, 0), Some(20.0));
    assert_eq!(b.pixel_at(0, 1), Some(30.0));
    assert_eq!(b.pixel_at(1, 1), Some(40.0));
}

#[test]
fn test_band_pixel_at_out_of_bounds() {
    let b = band2x2(1.0, 2.0, 3.0, 4.0);
    assert_eq!(b.pixel_at(2, 0), None);
    assert_eq!(b.pixel_at(0, 2), None);
}

#[test]
fn test_band_set_pixel_valid() {
    let mut b = band2x2(1.0, 2.0, 3.0, 4.0);
    assert!(b.set_pixel(1, 1, 99.0));
    assert_eq!(b.pixel_at(1, 1), Some(99.0));
}

#[test]
fn test_band_set_pixel_out_of_bounds() {
    let mut b = band2x2(1.0, 2.0, 3.0, 4.0);
    assert!(!b.set_pixel(5, 5, 99.0));
}

// ─── BandMath ─────────────────────────────────────────────────────────────

#[test]
fn test_bandmath_add_values() {
    let a = band2x2(1.0, 2.0, 3.0, 4.0);
    let b = band2x2(10.0, 20.0, 30.0, 40.0);
    let r = BandMath::add(&a, &b).expect("add failed");
    assert_eq!(r.data, vec![11.0, 22.0, 33.0, 44.0]);
}

#[test]
fn test_bandmath_add_nodata_propagation() {
    let a = band2x2_nd(1.0, ND, 3.0, 4.0, ND);
    let b = band2x2_nd(10.0, 20.0, ND, 40.0, ND);
    let r = BandMath::add(&a, &b).expect("add failed");
    // pixel[0]: 1+10=11, pixel[1]: ND (a is nodata), pixel[2]: ND (b is nodata), pixel[3]: 44
    assert!((r.data[0] - 11.0).abs() < 1e-10);
    assert_eq!(r.data[1], ND);
    assert_eq!(r.data[2], ND);
    assert!((r.data[3] - 44.0).abs() < 1e-10);
}

#[test]
fn test_bandmath_add_nodata_propagated_from_first_band() {
    let a = Band::new(vec![1.0, 2.0], 2, 1).with_nodata(ND);
    let b = Band::new(vec![3.0, 4.0], 2, 1).with_nodata(-1.0);
    let r = BandMath::add(&a, &b).expect("add failed");
    // nodata value comes from `a`
    assert_eq!(r.nodata, Some(ND));
}

#[test]
fn test_bandmath_add_dimension_mismatch() {
    let a = Band::new(vec![1.0, 2.0], 2, 1);
    let b = Band::new(vec![1.0, 2.0, 3.0], 3, 1);
    let err = BandMath::add(&a, &b).expect_err("expected dimension mismatch");
    assert!(matches!(err, AlgebraError::DimensionMismatch { .. }));
}

#[test]
fn test_bandmath_subtract_values() {
    let a = band2x2(10.0, 20.0, 30.0, 40.0);
    let b = band2x2(1.0, 2.0, 3.0, 4.0);
    let r = BandMath::subtract(&a, &b).expect("subtract failed");
    assert_eq!(r.data, vec![9.0, 18.0, 27.0, 36.0]);
}

#[test]
fn test_bandmath_subtract_dimension_mismatch() {
    let a = Band::new(vec![1.0, 2.0], 2, 1);
    let b = Band::new(vec![1.0, 2.0, 3.0], 3, 1);
    let err = BandMath::subtract(&a, &b).expect_err("expected dimension mismatch");
    assert!(matches!(err, AlgebraError::DimensionMismatch { .. }));
}

#[test]
fn test_bandmath_multiply_by_zero() {
    let a = band2x2(5.0, 10.0, 15.0, 20.0);
    let b = band2x2(0.0, 0.0, 0.0, 0.0);
    let r = BandMath::multiply(&a, &b).expect("multiply failed");
    assert_eq!(r.data, vec![0.0, 0.0, 0.0, 0.0]);
}

#[test]
fn test_bandmath_multiply_values() {
    let a = band2x2(2.0, 3.0, 4.0, 5.0);
    let b = band2x2(10.0, 10.0, 10.0, 10.0);
    let r = BandMath::multiply(&a, &b).expect("multiply failed");
    assert_eq!(r.data, vec![20.0, 30.0, 40.0, 50.0]);
}

#[test]
fn test_bandmath_divide_values() {
    let a = band2x2(10.0, 20.0, 30.0, 40.0);
    let b = band2x2(2.0, 4.0, 5.0, 8.0);
    let r = BandMath::divide(&a, &b).expect("divide failed");
    for (actual, expected) in r.data.iter().zip([5.0, 5.0, 6.0, 5.0].iter()) {
        assert!((actual - expected).abs() < 1e-10);
    }
}

#[test]
fn test_bandmath_divide_by_zero_becomes_nodata() {
    let a = Band::new(vec![10.0, 20.0], 2, 1).with_nodata(ND);
    let b = Band::new(vec![0.0, 4.0], 2, 1).with_nodata(ND);
    let r = BandMath::divide(&a, &b).expect("divide failed");
    // pixel[0] denominator=0 → nodata
    assert_eq!(r.data[0], ND);
    // pixel[1] = 20/4 = 5
    assert!((r.data[1] - 5.0).abs() < 1e-10);
}

#[test]
fn test_bandmath_divide_nodata_in_numerator() {
    let a = Band::new(vec![ND, 20.0], 2, 1).with_nodata(ND);
    let b = Band::new(vec![5.0, 4.0], 2, 1).with_nodata(ND);
    let r = BandMath::divide(&a, &b).expect("divide failed");
    assert_eq!(r.data[0], ND);
    assert!((r.data[1] - 5.0).abs() < 1e-10);
}

#[test]
fn test_bandmath_add_scalar_changes_all_values() {
    let a = band1xn(vec![1.0, 2.0, 3.0]);
    let r = BandMath::add_scalar(&a, 10.0);
    assert_eq!(r.data, vec![11.0, 12.0, 13.0]);
}

#[test]
fn test_bandmath_add_scalar_skips_nodata() {
    let a = Band::new(vec![1.0, ND, 3.0], 3, 1).with_nodata(ND);
    let r = BandMath::add_scalar(&a, 10.0);
    assert!((r.data[0] - 11.0).abs() < 1e-10);
    assert_eq!(r.data[1], ND);
    assert!((r.data[2] - 13.0).abs() < 1e-10);
}

#[test]
fn test_bandmath_multiply_scalar() {
    let a = band1xn(vec![1.0, 2.0, 4.0]);
    let r = BandMath::multiply_scalar(&a, 3.0);
    assert_eq!(r.data, vec![3.0, 6.0, 12.0]);
}

#[test]
fn test_bandmath_clamp_clips_below_min() {
    let a = band1xn(vec![-5.0, 0.0, 5.0]);
    let r = BandMath::clamp(&a, 0.0, 3.0);
    assert_eq!(r.data[0], 0.0); // clamped up
    assert_eq!(r.data[1], 0.0); // at boundary
    assert_eq!(r.data[2], 3.0); // clamped down
}

#[test]
fn test_bandmath_clamp_preserves_nodata() {
    let a = Band::new(vec![ND, 0.5], 2, 1).with_nodata(ND);
    let r = BandMath::clamp(&a, 0.0, 1.0);
    assert_eq!(r.data[0], ND);
    assert!((r.data[1] - 0.5).abs() < 1e-10);
}

#[test]
fn test_bandmath_apply_square() {
    let a = band1xn(vec![2.0, 3.0, 4.0]);
    let r = BandMath::apply(&a, |v| v * v);
    assert_eq!(r.data, vec![4.0, 9.0, 16.0]);
}

#[test]
fn test_bandmath_normalize_produces_zero_to_one() {
    let a = band1xn(vec![0.0, 5.0, 10.0]);
    let r = BandMath::normalize(&a).expect("normalize failed");
    assert!((r.data[0] - 0.0).abs() < 1e-10);
    assert!((r.data[1] - 0.5).abs() < 1e-10);
    assert!((r.data[2] - 1.0).abs() < 1e-10);
}

#[test]
fn test_bandmath_normalize_all_equal_returns_error() {
    let a = band1xn(vec![5.0, 5.0, 5.0]);
    let err = BandMath::normalize(&a).expect_err("expected all-equal error");
    assert!(matches!(err, AlgebraError::AllNodata));
}

#[test]
fn test_bandmath_normalize_all_nodata_returns_error() {
    let a = Band::new(vec![ND, ND, ND], 3, 1).with_nodata(ND);
    let err = BandMath::normalize(&a).expect_err("expected all-nodata error");
    assert!(matches!(err, AlgebraError::AllNodata));
}

#[test]
fn test_bandmath_normalize_empty_band_returns_error() {
    let a = Band::new(vec![], 0, 0);
    let err = BandMath::normalize(&a).expect_err("expected empty band error");
    assert!(matches!(err, AlgebraError::EmptyBand));
}

#[test]
fn test_bandmath_normalize_with_nodata_pixels() {
    let a = Band::new(vec![ND, 0.0, 10.0], 3, 1).with_nodata(ND);
    let r = BandMath::normalize(&a).expect("normalize failed");
    // nodata pixel unchanged, others normalised to [0,1]
    assert_eq!(r.data[0], ND);
    assert!((r.data[1] - 0.0).abs() < 1e-10);
    assert!((r.data[2] - 1.0).abs() < 1e-10);
}

// ─── SpectralIndex ────────────────────────────────────────────────────────

#[test]
fn test_ndvi_known_values() {
    // NIR=0.8, Red=0.2 → NDVI = (0.8-0.2)/(0.8+0.2) = 0.6
    let nir = band1xn(vec![0.8]);
    let red = band1xn(vec![0.2]);
    let r = SpectralIndex::ndvi(&nir, &red).expect("ndvi failed");
    assert!(
        (r.data[0] - 0.6).abs() < 1e-10,
        "expected 0.6, got {}",
        r.data[0]
    );
}

#[test]
fn test_ndvi_nir_equals_red_yields_zero() {
    let nir = band1xn(vec![0.5]);
    let red = band1xn(vec![0.5]);
    let r = SpectralIndex::ndvi(&nir, &red).expect("ndvi failed");
    assert!((r.data[0] - 0.0).abs() < 1e-10);
}

#[test]
fn test_ndvi_denominator_zero_yields_nodata() {
    let nir = Band::new(vec![0.0], 1, 1).with_nodata(ND);
    let red = Band::new(vec![0.0], 1, 1).with_nodata(ND);
    let r = SpectralIndex::ndvi(&nir, &red).expect("ndvi failed");
    assert_eq!(r.data[0], ND);
}

#[test]
fn test_ndvi_result_clamped_to_minus_one_plus_one() {
    // Artificially extreme values: NIR=0, Red=1 → (0-1)/(0+1) = -1
    let nir = band1xn(vec![0.0]);
    let red = band1xn(vec![1.0]);
    let r = SpectralIndex::ndvi(&nir, &red).expect("ndvi failed");
    assert!(r.data[0] >= -1.0);
    assert!(r.data[0] <= 1.0);
}

#[test]
fn test_ndvi_nodata_propagation() {
    let nir = Band::new(vec![ND, 0.8], 2, 1).with_nodata(ND);
    let red = Band::new(vec![0.2, ND], 2, 1).with_nodata(ND);
    let r = SpectralIndex::ndvi(&nir, &red).expect("ndvi failed");
    assert_eq!(r.data[0], ND);
    assert_eq!(r.data[1], ND);
}

#[test]
fn test_ndvi_dimension_mismatch() {
    let nir = Band::new(vec![0.8, 0.8], 2, 1);
    let red = Band::new(vec![0.2], 1, 1);
    let err = SpectralIndex::ndvi(&nir, &red).expect_err("expected dimension mismatch");
    assert!(matches!(err, AlgebraError::DimensionMismatch { .. }));
}

#[test]
fn test_ndvi_band_name_set() {
    let nir = band1xn(vec![0.8]);
    let red = band1xn(vec![0.2]);
    let r = SpectralIndex::ndvi(&nir, &red).expect("ndvi failed");
    assert_eq!(r.name.as_deref(), Some("NDVI"));
}

#[test]
fn test_evi_known_values() {
    // NIR=0.5, Red=0.1, Blue=0.02
    // denom = 0.5 + 6*0.1 - 7.5*0.02 + 1 = 0.5+0.6-0.15+1 = 1.95
    // EVI = 2.5 * (0.5 - 0.1) / 1.95 ≈ 0.5128
    let nir = band1xn(vec![0.5]);
    let red = band1xn(vec![0.1]);
    let blue = band1xn(vec![0.02]);
    let r = SpectralIndex::evi(&nir, &red, &blue).expect("evi failed");
    let expected = 2.5 * (0.5 - 0.1) / (0.5 + 6.0 * 0.1 - 7.5 * 0.02 + 1.0);
    assert!(
        (r.data[0] - expected).abs() < 1e-10,
        "expected {expected}, got {}",
        r.data[0]
    );
}

#[test]
fn test_evi_nodata_propagation() {
    let nir = Band::new(vec![ND], 1, 1).with_nodata(ND);
    let red = Band::new(vec![0.1], 1, 1).with_nodata(ND);
    let blue = Band::new(vec![0.02], 1, 1).with_nodata(ND);
    let r = SpectralIndex::evi(&nir, &red, &blue).expect("evi failed");
    assert_eq!(r.data[0], ND);
}

#[test]
fn test_evi_dimension_mismatch() {
    let nir = Band::new(vec![0.5, 0.5], 2, 1);
    let red = Band::new(vec![0.1], 1, 1);
    let blue = Band::new(vec![0.02, 0.02], 2, 1);
    let err = SpectralIndex::evi(&nir, &red, &blue).expect_err("expected dimension mismatch");
    assert!(matches!(err, AlgebraError::DimensionMismatch { .. }));
}

#[test]
fn test_ndwi_positive_for_water() {
    // Water: Green > NIR → positive NDWI
    let green = band1xn(vec![0.7]);
    let nir = band1xn(vec![0.1]);
    let r = SpectralIndex::ndwi(&green, &nir).expect("ndwi failed");
    assert!(
        r.data[0] > 0.0,
        "expected positive NDWI for water, got {}",
        r.data[0]
    );
}

#[test]
fn test_ndwi_negative_for_vegetation() {
    // Vegetation: NIR > Green → negative NDWI
    let green = band1xn(vec![0.2]);
    let nir = band1xn(vec![0.8]);
    let r = SpectralIndex::ndwi(&green, &nir).expect("ndwi failed");
    assert!(r.data[0] < 0.0);
}

#[test]
fn test_ndsi_known_values() {
    // Snow: Green > SWIR → positive NDSI
    let green = band1xn(vec![0.5]);
    let swir = band1xn(vec![0.1]);
    let r = SpectralIndex::ndsi(&green, &swir).expect("ndsi failed");
    let expected = (0.5 - 0.1) / (0.5 + 0.1);
    assert!((r.data[0] - expected).abs() < 1e-10);
}

#[test]
fn test_savi_known_values() {
    // NIR=0.8, Red=0.2, L=0.5
    // denom = 0.8+0.2+0.5 = 1.5
    // SAVI = ((0.8-0.2)/1.5) * 1.5 = 0.6
    let nir = band1xn(vec![0.8]);
    let red = band1xn(vec![0.2]);
    let r = SpectralIndex::savi(&nir, &red, 0.5).expect("savi failed");
    let expected = ((0.8 - 0.2) / (0.8 + 0.2 + 0.5)) * (1.0 + 0.5);
    assert!(
        (r.data[0] - expected).abs() < 1e-10,
        "expected {expected}, got {}",
        r.data[0]
    );
}

#[test]
fn test_savi_invalid_l_parameter() {
    let nir = band1xn(vec![0.8]);
    let red = band1xn(vec![0.2]);
    let err = SpectralIndex::savi(&nir, &red, -0.1).expect_err("expected invalid parameter");
    assert!(matches!(err, AlgebraError::InvalidParameter(_)));
}

#[test]
fn test_nbr_known_values() {
    // NIR=0.9, SWIR=0.1 → NBR = (0.9-0.1)/(0.9+0.1) = 0.8
    let nir = band1xn(vec![0.9]);
    let swir = band1xn(vec![0.1]);
    let r = SpectralIndex::nbr(&nir, &swir).expect("nbr failed");
    assert!((r.data[0] - 0.8).abs() < 1e-10);
}

#[test]
fn test_nbr_nodata_propagation() {
    let nir = Band::new(vec![ND], 1, 1).with_nodata(ND);
    let swir = Band::new(vec![0.1], 1, 1).with_nodata(ND);
    let r = SpectralIndex::nbr(&nir, &swir).expect("nbr failed");
    assert_eq!(r.data[0], ND);
}

#[test]
fn test_bsi_known_values() {
    // Red=0.3, SWIR=0.4, NIR=0.5, Blue=0.1
    // pos = 0.3+0.4 = 0.7
    // neg = 0.5+0.1 = 0.6
    // BSI = (0.7-0.6)/(0.7+0.6) = 0.1/1.3
    let red = band1xn(vec![0.3]);
    let swir = band1xn(vec![0.4]);
    let nir = band1xn(vec![0.5]);
    let blue = band1xn(vec![0.1]);
    let r = SpectralIndex::bsi(&red, &swir, &nir, &blue).expect("bsi failed");
    let expected = (0.7 - 0.6) / (0.7 + 0.6);
    assert!((r.data[0] - expected).abs() < 1e-10);
}

#[test]
fn test_bsi_nodata_propagation_blue() {
    let swir = band1xn(vec![0.4]);
    let nir = band1xn(vec![0.5]);
    let blue = Band::new(vec![ND], 1, 1).with_nodata(ND);
    // red must also carry the nodata value for propagation to work correctly
    let red_nd = Band::new(vec![0.3], 1, 1).with_nodata(ND);
    let r = SpectralIndex::bsi(&red_nd, &swir, &nir, &blue).expect("bsi failed");
    assert_eq!(r.data[0], ND);
}

#[test]
fn test_mndwi_uses_swir() {
    // Green=0.6, SWIR=0.1 → positive (water)
    let green = band1xn(vec![0.6]);
    let swir = band1xn(vec![0.1]);
    let r = SpectralIndex::mndwi(&green, &swir).expect("mndwi failed");
    assert!(r.data[0] > 0.0);
    assert_eq!(r.name.as_deref(), Some("MNDWI"));
}

// ─── NodataMask ───────────────────────────────────────────────────────────

#[test]
fn test_nodata_mask_from_band_marks_nodata_pixels() {
    let b = Band::new(vec![1.0, ND, 3.0, ND], 4, 1).with_nodata(ND);
    let m = NodataMask::from_band(&b);
    assert_eq!(m.mask, vec![true, false, true, false]);
}

#[test]
fn test_nodata_mask_all_valid() {
    let m = NodataMask::all_valid(3, 2);
    assert_eq!(m.valid_count(), 6);
    assert!(m.mask.iter().all(|&v| v));
}

#[test]
fn test_nodata_mask_all_invalid() {
    let m = NodataMask::all_invalid(3, 2);
    assert_eq!(m.valid_count(), 0);
    assert!(m.mask.iter().all(|&v| !v));
}

#[test]
fn test_nodata_mask_from_value() {
    let data = vec![1.0, ND, 3.0];
    let m = NodataMask::from_value(&data, ND);
    assert_eq!(m.mask, vec![true, false, true]);
}

#[test]
fn test_nodata_mask_valid_count() {
    let m = NodataMask {
        mask: vec![true, false, true, true],
        width: 4,
        height: 1,
    };
    assert_eq!(m.valid_count(), 3);
}

#[test]
fn test_nodata_mask_and_intersection() {
    let a = NodataMask {
        mask: vec![true, true, false, false],
        width: 4,
        height: 1,
    };
    let b = NodataMask {
        mask: vec![true, false, true, false],
        width: 4,
        height: 1,
    };
    let r = a.and(&b);
    assert_eq!(r.mask, vec![true, false, false, false]);
}

#[test]
fn test_nodata_mask_or_union() {
    let a = NodataMask {
        mask: vec![true, true, false, false],
        width: 4,
        height: 1,
    };
    let b = NodataMask {
        mask: vec![true, false, true, false],
        width: 4,
        height: 1,
    };
    let r = a.or(&b);
    assert_eq!(r.mask, vec![true, true, true, false]);
}

#[test]
fn test_nodata_mask_invert() {
    let m = NodataMask {
        mask: vec![true, false, true],
        width: 3,
        height: 1,
    };
    let inv = m.invert();
    assert_eq!(inv.mask, vec![false, true, false]);
}

#[test]
fn test_nodata_mask_apply_to_band() {
    let mut b = Band::new(vec![1.0, 2.0, 3.0, 4.0], 4, 1).with_nodata(ND);
    let m = NodataMask {
        mask: vec![true, false, true, false],
        width: 4,
        height: 1,
    };
    m.apply_to_band(&mut b, ND);
    assert!((b.data[0] - 1.0).abs() < 1e-10);
    assert_eq!(b.data[1], ND);
    assert!((b.data[2] - 3.0).abs() < 1e-10);
    assert_eq!(b.data[3], ND);
}

// ─── BandStack ────────────────────────────────────────────────────────────

#[test]
fn test_bandstack_push_band_dimension_mismatch() {
    let mut stack = BandStack::new(2, 2);
    let b = Band::new(vec![1.0, 2.0, 3.0], 3, 1);
    let err = stack.push_band(b).expect_err("expected dimension mismatch");
    assert!(matches!(err, AlgebraError::DimensionMismatch { .. }));
}

#[test]
fn test_bandstack_band_count() {
    let mut stack = BandStack::new(2, 2);
    stack
        .push_band(band2x2(1.0, 2.0, 3.0, 4.0))
        .expect("push band 0");
    stack
        .push_band(band2x2(5.0, 6.0, 7.0, 8.0))
        .expect("push band 1");
    assert_eq!(stack.band_count(), 2);
}

#[test]
fn test_bandstack_get_band_by_index() {
    let mut stack = BandStack::new(2, 2);
    let b = band2x2(1.0, 2.0, 3.0, 4.0);
    stack.push_band(b).expect("push band");
    assert!(stack.get_band(0).is_some());
    assert!(stack.get_band(1).is_none());
}

#[test]
fn test_bandstack_get_band_by_name() {
    let mut stack = BandStack::new(2, 2);
    let b = band2x2(1.0, 2.0, 3.0, 4.0).with_name("NIR");
    stack.push_band(b).expect("push band");
    assert!(stack.get_band_by_name("NIR").is_some());
    assert!(stack.get_band_by_name("Red").is_none());
}

#[test]
fn test_bandstack_pixel_mean() {
    let mut stack = BandStack::new(2, 2);
    // pixel (0,0) = 10 in band0, 20 in band1 → mean=15
    stack
        .push_band(band2x2(10.0, 0.0, 0.0, 0.0))
        .expect("push band 0");
    stack
        .push_band(band2x2(20.0, 0.0, 0.0, 0.0))
        .expect("push band 1");
    let mean = stack.pixel_mean(0, 0).expect("mean failed");
    assert!((mean - 15.0).abs() < 1e-10);
}

#[test]
fn test_bandstack_pixel_min_max() {
    let mut stack = BandStack::new(1, 1);
    stack
        .push_band(Band::new(vec![3.0], 1, 1))
        .expect("push band 0");
    stack
        .push_band(Band::new(vec![7.0], 1, 1))
        .expect("push band 1");
    stack
        .push_band(Band::new(vec![1.0], 1, 1))
        .expect("push band 2");
    assert_eq!(stack.pixel_min(0, 0), Some(1.0));
    assert_eq!(stack.pixel_max(0, 0), Some(7.0));
}

#[test]
fn test_bandstack_pixel_mean_with_nodata_excluded() {
    let mut stack = BandStack::new(1, 1);
    stack
        .push_band(Band::new(vec![ND], 1, 1).with_nodata(ND))
        .expect("push band 0");
    stack
        .push_band(Band::new(vec![10.0], 1, 1).with_nodata(ND))
        .expect("push band 1");
    let mean = stack
        .pixel_mean(0, 0)
        .expect("mean should have valid pixel");
    assert!((mean - 10.0).abs() < 1e-10);
}

#[test]
fn test_bandstack_pixel_mean_all_nodata_returns_none() {
    let mut stack = BandStack::new(1, 1);
    stack
        .push_band(Band::new(vec![ND], 1, 1).with_nodata(ND))
        .expect("push band");
    assert_eq!(stack.pixel_mean(0, 0), None);
}

#[test]
fn test_bandstack_reduce_sum() {
    let mut stack = BandStack::new(1, 1);
    stack
        .push_band(Band::new(vec![3.0], 1, 1))
        .expect("push band 0");
    stack
        .push_band(Band::new(vec![7.0], 1, 1))
        .expect("push band 1");
    let result = stack.reduce(|vals| vals.iter().sum());
    assert!((result.data[0] - 10.0).abs() < 1e-10);
}

#[test]
fn test_bandstack_reduce_empty_slice_for_all_nodata() {
    let mut stack = BandStack::new(1, 1);
    stack
        .push_band(Band::new(vec![ND], 1, 1).with_nodata(ND))
        .expect("push band");
    let result = stack.reduce(|vals| if vals.is_empty() { ND } else { vals[0] });
    assert_eq!(result.data[0], ND);
}

// ─── ThresholdClassifier ──────────────────────────────────────────────────

#[test]
fn test_threshold_classifier_ndvi_water() {
    let clf = ThresholdClassifier::ndvi_classes();
    let b = band1xn(vec![-0.5]);
    let r = clf.classify(&b);
    assert_eq!(r.data[0] as u32, 0); // water/barren
}

#[test]
fn test_threshold_classifier_ndvi_sparse() {
    let clf = ThresholdClassifier::ndvi_classes();
    let b = band1xn(vec![0.05]);
    let r = clf.classify(&b);
    assert_eq!(r.data[0] as u32, 1); // sparse
}

#[test]
fn test_threshold_classifier_ndvi_moderate() {
    let clf = ThresholdClassifier::ndvi_classes();
    let b = band1xn(vec![0.35]);
    let r = clf.classify(&b);
    assert_eq!(r.data[0] as u32, 2); // moderate
}

#[test]
fn test_threshold_classifier_ndvi_dense() {
    let clf = ThresholdClassifier::ndvi_classes();
    let b = band1xn(vec![0.6]);
    let r = clf.classify(&b);
    assert_eq!(r.data[0] as u32, 3); // dense (default class)
}

#[test]
fn test_threshold_classifier_custom_thresholds() {
    let clf = ThresholdClassifier::new(99)
        .add_class(0.0, 0)
        .add_class(1.0, 1);
    let b = band1xn(vec![-1.0, 0.5, 2.0]);
    let r = clf.classify(&b);
    assert_eq!(r.data[0] as u32, 0); // < 0.0 → class 0
    assert_eq!(r.data[1] as u32, 1); // < 1.0 → class 1
    assert_eq!(r.data[2] as u32, 99); // >= 1.0 → default
}

#[test]
fn test_threshold_classifier_nodata_maps_to_default() {
    let clf = ThresholdClassifier::ndvi_classes();
    let b = Band::new(vec![ND, 0.6], 2, 1).with_nodata(ND);
    let r = clf.classify(&b);
    assert_eq!(r.data[0] as u32, 3); // nodata → default class
    assert_eq!(r.data[1] as u32, 3); // 0.6 → default (dense)
}

#[test]
fn test_threshold_classifier_add_class_keeps_sorted() {
    let clf = ThresholdClassifier::new(9)
        .add_class(5.0, 2)
        .add_class(1.0, 0)
        .add_class(3.0, 1);
    // thresholds should be sorted: 1.0, 3.0, 5.0
    let thresholds: Vec<f64> = clf.thresholds.iter().map(|&(t, _)| t).collect();
    assert_eq!(thresholds, vec![1.0, 3.0, 5.0]);
}
