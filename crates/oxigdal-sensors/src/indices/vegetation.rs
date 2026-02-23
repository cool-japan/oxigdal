//! Vegetation spectral indices
//!
//! Comprehensive collection of vegetation indices for monitoring plant health,
//! biomass, chlorophyll content, and water stress.

use crate::error::{Result, SensorError};
use scirs2_core::ndarray::{Array2, ArrayView2, Zip};

/// Normalized Difference Vegetation Index (NDVI)
///
/// Formula: NDVI = (NIR - Red) / (NIR + Red)
///
/// Range: -1 to 1 (typical vegetation: 0.2 to 0.8)
/// Applications: Vegetation health, biomass estimation, crop monitoring
pub fn ndvi(nir: &ArrayView2<f64>, red: &ArrayView2<f64>) -> Result<Array2<f64>> {
    check_dimensions(nir, red, "NIR", "Red")?;

    let mut ndvi = Array2::zeros(nir.dim());

    Zip::from(&mut ndvi)
        .and(nir)
        .and(red)
        .for_each(|out, &n, &r| {
            let sum = n + r;
            *out = if sum.abs() > 1e-10 {
                (n - r) / sum
            } else {
                0.0
            };
        });

    Ok(ndvi)
}

/// Enhanced Vegetation Index (EVI)
///
/// Formula: EVI = G * ((NIR - Red) / (NIR + C1*Red - C2*Blue + L))
///
/// where G=2.5, C1=6, C2=7.5, L=1
///
/// Range: -1 to 1
/// Applications: Improved sensitivity in high biomass regions, reduced atmospheric influence
pub fn evi(
    nir: &ArrayView2<f64>,
    red: &ArrayView2<f64>,
    blue: &ArrayView2<f64>,
) -> Result<Array2<f64>> {
    check_dimensions(nir, red, "NIR", "Red")?;
    check_dimensions(nir, blue, "NIR", "Blue")?;

    const G: f64 = 2.5;
    const C1: f64 = 6.0;
    const C2: f64 = 7.5;
    const L: f64 = 1.0;

    let mut evi = Array2::zeros(nir.dim());

    Zip::from(&mut evi)
        .and(nir)
        .and(red)
        .and(blue)
        .for_each(|out, &n, &r, &b| {
            let denom = n + C1 * r - C2 * b + L;
            *out = if denom.abs() > 1e-10 {
                G * (n - r) / denom
            } else {
                0.0
            };
        });

    Ok(evi)
}

/// Two-band Enhanced Vegetation Index (EVI2)
///
/// Formula: EVI2 = 2.5 * ((NIR - Red) / (NIR + 2.4*Red + 1))
///
/// Applications: EVI without blue band requirement
pub fn evi2(nir: &ArrayView2<f64>, red: &ArrayView2<f64>) -> Result<Array2<f64>> {
    check_dimensions(nir, red, "NIR", "Red")?;

    const G: f64 = 2.5;
    const C: f64 = 2.4;
    const L: f64 = 1.0;

    let mut evi2 = Array2::zeros(nir.dim());

    Zip::from(&mut evi2)
        .and(nir)
        .and(red)
        .for_each(|out, &n, &r| {
            let denom = n + C * r + L;
            *out = if denom.abs() > 1e-10 {
                G * (n - r) / denom
            } else {
                0.0
            };
        });

    Ok(evi2)
}

/// Soil-Adjusted Vegetation Index (SAVI)
///
/// Formula: SAVI = ((NIR - Red) / (NIR + Red + L)) * (1 + L)
///
/// where L = 0.5 (default, can be adjusted based on vegetation density)
///
/// Applications: Reduces soil brightness effects in sparse vegetation
pub fn savi(nir: &ArrayView2<f64>, red: &ArrayView2<f64>, l: f64) -> Result<Array2<f64>> {
    check_dimensions(nir, red, "NIR", "Red")?;

    if !(0.0..=1.0).contains(&l) {
        return Err(SensorError::invalid_parameter(
            "L",
            "must be between 0.0 and 1.0",
        ));
    }

    let mut savi = Array2::zeros(nir.dim());

    Zip::from(&mut savi)
        .and(nir)
        .and(red)
        .for_each(|out, &n, &r| {
            let denom = n + r + l;
            *out = if denom.abs() > 1e-10 {
                ((n - r) / denom) * (1.0 + l)
            } else {
                0.0
            };
        });

    Ok(savi)
}

/// Modified Soil-Adjusted Vegetation Index (MSAVI)
///
/// Formula: MSAVI = (2*NIR + 1 - sqrt((2*NIR + 1)^2 - 8*(NIR - Red))) / 2
///
/// Applications: Self-adjusting L factor, better performance across varying vegetation densities
pub fn msavi(nir: &ArrayView2<f64>, red: &ArrayView2<f64>) -> Result<Array2<f64>> {
    check_dimensions(nir, red, "NIR", "Red")?;

    let mut msavi = Array2::zeros(nir.dim());

    Zip::from(&mut msavi)
        .and(nir)
        .and(red)
        .for_each(|out, &n, &r| {
            let term1 = 2.0 * n + 1.0;
            let term2 = term1 * term1 - 8.0 * (n - r);
            *out = if term2 >= 0.0 {
                (term1 - term2.sqrt()) / 2.0
            } else {
                0.0
            };
        });

    Ok(msavi)
}

/// Optimized Soil-Adjusted Vegetation Index (OSAVI)
///
/// Formula: OSAVI = (NIR - Red) / (NIR + Red + 0.16)
///
/// Applications: Improved soil adjustment with optimized L=0.16
pub fn osavi(nir: &ArrayView2<f64>, red: &ArrayView2<f64>) -> Result<Array2<f64>> {
    savi(nir, red, 0.16)
}

/// Normalized Difference Water Index (NDWI)
///
/// Formula: NDWI = (NIR - SWIR) / (NIR + SWIR)
///
/// Applications: Vegetation water content, drought monitoring
pub fn ndwi(nir: &ArrayView2<f64>, swir: &ArrayView2<f64>) -> Result<Array2<f64>> {
    check_dimensions(nir, swir, "NIR", "SWIR")?;

    let mut ndwi = Array2::zeros(nir.dim());

    Zip::from(&mut ndwi)
        .and(nir)
        .and(swir)
        .for_each(|out, &n, &s| {
            let sum = n + s;
            *out = if sum.abs() > 1e-10 {
                (n - s) / sum
            } else {
                0.0
            };
        });

    Ok(ndwi)
}

/// Normalized Difference Moisture Index (NDMI)
///
/// Formula: NDMI = (NIR - SWIR1) / (NIR + SWIR1)
///
/// Applications: Vegetation water stress, fire risk assessment
pub fn ndmi(nir: &ArrayView2<f64>, swir1: &ArrayView2<f64>) -> Result<Array2<f64>> {
    ndwi(nir, swir1) // Same formula as NDWI
}

/// Green Normalized Difference Vegetation Index (GNDVI)
///
/// Formula: GNDVI = (NIR - Green) / (NIR + Green)
///
/// Applications: Chlorophyll content estimation, nitrogen assessment
pub fn gndvi(nir: &ArrayView2<f64>, green: &ArrayView2<f64>) -> Result<Array2<f64>> {
    check_dimensions(nir, green, "NIR", "Green")?;

    let mut gndvi = Array2::zeros(nir.dim());

    Zip::from(&mut gndvi)
        .and(nir)
        .and(green)
        .for_each(|out, &n, &g| {
            let sum = n + g;
            *out = if sum.abs() > 1e-10 {
                (n - g) / sum
            } else {
                0.0
            };
        });

    Ok(gndvi)
}

/// Green Ratio Vegetation Index (GRVI)
///
/// Formula: GRVI = NIR / Green
///
/// Applications: Alternative to GNDVI for chlorophyll monitoring
pub fn grvi(nir: &ArrayView2<f64>, green: &ArrayView2<f64>) -> Result<Array2<f64>> {
    check_dimensions(nir, green, "NIR", "Green")?;

    let mut grvi = Array2::zeros(nir.dim());

    Zip::from(&mut grvi)
        .and(nir)
        .and(green)
        .for_each(|out, &n, &g| {
            *out = if g.abs() > 1e-10 { n / g } else { 0.0 };
        });

    Ok(grvi)
}

/// Chlorophyll Index (CI)
///
/// Formula: CI = (NIR / RedEdge) - 1
///
/// Applications: Chlorophyll content, plant health (requires red edge band)
pub fn ci(nir: &ArrayView2<f64>, red_edge: &ArrayView2<f64>) -> Result<Array2<f64>> {
    check_dimensions(nir, red_edge, "NIR", "RedEdge")?;

    let mut ci = Array2::zeros(nir.dim());

    Zip::from(&mut ci)
        .and(nir)
        .and(red_edge)
        .for_each(|out, &n, &re| {
            *out = if re.abs() > 1e-10 {
                (n / re) - 1.0
            } else {
                0.0
            };
        });

    Ok(ci)
}

/// Check that two arrays have the same dimensions
fn check_dimensions(
    a: &ArrayView2<f64>,
    b: &ArrayView2<f64>,
    name_a: &str,
    name_b: &str,
) -> Result<()> {
    if a.dim() != b.dim() {
        return Err(SensorError::dimension_mismatch(
            format!("{}: {:?}", name_a, a.dim()),
            format!("{}: {:?}", name_b, b.dim()),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use scirs2_core::ndarray::array;

    #[test]
    fn test_ndvi() {
        let nir = array![[0.5, 0.6], [0.7, 0.8]];
        let red = array![[0.1, 0.1], [0.1, 0.1]];

        let result = ndvi(&nir.view(), &red.view());
        assert!(result.is_ok());

        if let Ok(ndvi_result) = result {
            // NDVI should be positive for healthy vegetation
            assert!(ndvi_result[[0, 0]] > 0.0);
            assert!(ndvi_result[[0, 0]] < 1.0);
        }
    }

    #[test]
    fn test_evi() {
        let nir = array![[0.5, 0.6]];
        let red = array![[0.1, 0.1]];
        let blue = array![[0.05, 0.05]];

        let result = evi(&nir.view(), &red.view(), &blue.view());
        assert!(result.is_ok());

        if let Ok(evi_result) = result {
            assert!(evi_result[[0, 0]].is_finite());
        }
    }

    #[test]
    fn test_savi() {
        let nir = array![[0.5, 0.6]];
        let red = array![[0.1, 0.1]];

        let result = savi(&nir.view(), &red.view(), 0.5);
        assert!(result.is_ok());

        if let Ok(savi_result) = result {
            assert!(savi_result[[0, 0]].is_finite());
        }
    }

    #[test]
    fn test_msavi() {
        let nir = array![[0.5, 0.6]];
        let red = array![[0.1, 0.1]];

        let result = msavi(&nir.view(), &red.view());
        assert!(result.is_ok());

        if let Ok(msavi_result) = result {
            assert!(msavi_result[[0, 0]].is_finite());
            assert!(msavi_result[[0, 0]] >= 0.0);
        }
    }

    #[test]
    fn test_ndwi() {
        let nir = array![[0.5, 0.6]];
        let swir = array![[0.2, 0.2]];

        let result = ndwi(&nir.view(), &swir.view());
        assert!(result.is_ok());

        if let Ok(ndwi_result) = result {
            assert!(ndwi_result[[0, 0]] > 0.0);
        }
    }

    #[test]
    fn test_gndvi() {
        let nir = array![[0.5, 0.6]];
        let green = array![[0.15, 0.15]];

        let result = gndvi(&nir.view(), &green.view());
        assert!(result.is_ok());

        if let Ok(gndvi_result) = result {
            assert!(gndvi_result[[0, 0]] > 0.0);
        }
    }

    #[test]
    fn test_dimension_mismatch() {
        let nir = array![[0.5, 0.6]];
        let red = array![[0.1], [0.1]];

        let result = ndvi(&nir.view(), &red.view());
        assert!(result.is_err());
    }
}
