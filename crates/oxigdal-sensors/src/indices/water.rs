//! Water indices for water body detection and monitoring

use crate::error::{Result, SensorError};
use scirs2_core::ndarray::{Array2, ArrayView2, Zip};

/// Modified Normalized Difference Water Index (MNDWI)
///
/// Formula: MNDWI = (Green - SWIR1) / (Green + SWIR1)
///
/// Range: -1 to 1 (higher values indicate water)
/// Applications: Water body mapping, wetland monitoring
pub fn mndwi(green: &ArrayView2<f64>, swir1: &ArrayView2<f64>) -> Result<Array2<f64>> {
    if green.dim() != swir1.dim() {
        return Err(SensorError::dimension_mismatch(
            format!("{:?}", green.dim()),
            format!("{:?}", swir1.dim()),
        ));
    }

    let mut mndwi = Array2::zeros(green.dim());

    Zip::from(&mut mndwi)
        .and(green)
        .and(swir1)
        .for_each(|out, &g, &s| {
            let sum = g + s;
            *out = if sum.abs() > 1e-10 {
                (g - s) / sum
            } else {
                0.0
            };
        });

    Ok(mndwi)
}

/// Automated Water Extraction Index (AWEI)
///
/// Formula: AWEI = 4 * (Green - SWIR1) - (0.25*NIR + 2.75*SWIR2)
///
/// Applications: Enhanced water extraction, reduces built-up area confusion
pub fn awei(
    green: &ArrayView2<f64>,
    nir: &ArrayView2<f64>,
    swir1: &ArrayView2<f64>,
    swir2: &ArrayView2<f64>,
) -> Result<Array2<f64>> {
    if green.dim() != nir.dim() || green.dim() != swir1.dim() || green.dim() != swir2.dim() {
        return Err(SensorError::dimension_mismatch(
            format!("{:?}", green.dim()),
            format!(
                "NIR: {:?}, SWIR1: {:?}, SWIR2: {:?}",
                nir.dim(),
                swir1.dim(),
                swir2.dim()
            ),
        ));
    }

    let mut awei = Array2::zeros(green.dim());

    Zip::from(&mut awei)
        .and(green)
        .and(nir)
        .and(swir1)
        .and(swir2)
        .for_each(|out, &g, &n, &s1, &s2| {
            *out = 4.0 * (g - s1) - (0.25 * n + 2.75 * s2);
        });

    Ok(awei)
}

/// Water Ratio Index (WRI)
///
/// Formula: WRI = (Green + Red) / (NIR + SWIR1)
///
/// Applications: Water body detection, complementary to MNDWI
pub fn wri(
    green: &ArrayView2<f64>,
    red: &ArrayView2<f64>,
    nir: &ArrayView2<f64>,
    swir1: &ArrayView2<f64>,
) -> Result<Array2<f64>> {
    if green.dim() != red.dim() || green.dim() != nir.dim() || green.dim() != swir1.dim() {
        return Err(SensorError::dimension_mismatch(
            format!("{:?}", green.dim()),
            format!(
                "Red: {:?}, NIR: {:?}, SWIR1: {:?}",
                red.dim(),
                nir.dim(),
                swir1.dim()
            ),
        ));
    }

    let mut wri = Array2::zeros(green.dim());

    Zip::from(&mut wri)
        .and(green)
        .and(red)
        .and(nir)
        .and(swir1)
        .for_each(|out, &g, &r, &n, &s1| {
            let denom = n + s1;
            *out = if denom.abs() > 1e-10 {
                (g + r) / denom
            } else {
                0.0
            };
        });

    Ok(wri)
}

#[cfg(test)]
mod tests {
    use super::*;
    use scirs2_core::ndarray::array;

    #[test]
    fn test_mndwi() {
        let green = array![[0.3, 0.3]];
        let swir1 = array![[0.1, 0.1]];

        let result = mndwi(&green.view(), &swir1.view());
        assert!(result.is_ok());

        if let Ok(mndwi_result) = result {
            assert!(mndwi_result[[0, 0]] > 0.0); // Water has positive MNDWI
        }
    }

    #[test]
    fn test_awei() {
        let green = array![[0.3, 0.3]];
        let nir = array![[0.1, 0.1]];
        let swir1 = array![[0.05, 0.05]];
        let swir2 = array![[0.03, 0.03]];

        let result = awei(&green.view(), &nir.view(), &swir1.view(), &swir2.view());
        assert!(result.is_ok());

        if let Ok(awei_result) = result {
            assert!(awei_result[[0, 0]].is_finite());
        }
    }

    #[test]
    fn test_wri() {
        let green = array![[0.3, 0.3]];
        let red = array![[0.2, 0.2]];
        let nir = array![[0.1, 0.1]];
        let swir1 = array![[0.05, 0.05]];

        let result = wri(&green.view(), &red.view(), &nir.view(), &swir1.view());
        assert!(result.is_ok());

        if let Ok(wri_result) = result {
            assert!(wri_result[[0, 0]] > 0.0);
        }
    }
}
