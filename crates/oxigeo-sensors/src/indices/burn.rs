//! Burn indices for fire and burn scar analysis

use crate::error::{Result, SensorError};
use scirs2_core::ndarray::{Array2, ArrayView2, Zip};

/// Normalized Burn Ratio (NBR)
///
/// Formula: NBR = (NIR - SWIR2) / (NIR + SWIR2)
///
/// Range: -1 to 1
/// Applications: Burn severity mapping, fire damage assessment
pub fn nbr(nir: &ArrayView2<f64>, swir2: &ArrayView2<f64>) -> Result<Array2<f64>> {
    if nir.dim() != swir2.dim() {
        return Err(SensorError::dimension_mismatch(
            format!("{:?}", nir.dim()),
            format!("{:?}", swir2.dim()),
        ));
    }

    let mut nbr = Array2::zeros(nir.dim());

    Zip::from(&mut nbr)
        .and(nir)
        .and(swir2)
        .for_each(|out, &n, &s| {
            let sum = n + s;
            *out = if sum.abs() > 1e-10 {
                (n - s) / sum
            } else {
                0.0
            };
        });

    Ok(nbr)
}

/// Differenced Normalized Burn Ratio (dNBR)
///
/// Formula: dNBR = NBR_prefire - NBR_postfire
///
/// Range: -2 to 2 (higher values indicate more severe burns)
/// Applications: Burn severity classification, change detection
pub fn d_nbr(nbr_prefire: &ArrayView2<f64>, nbr_postfire: &ArrayView2<f64>) -> Result<Array2<f64>> {
    if nbr_prefire.dim() != nbr_postfire.dim() {
        return Err(SensorError::dimension_mismatch(
            format!("{:?}", nbr_prefire.dim()),
            format!("{:?}", nbr_postfire.dim()),
        ));
    }

    Ok(nbr_prefire - nbr_postfire)
}

/// Normalized Burn Ratio 2 (NBR2)
///
/// Formula: NBR2 = (SWIR1 - SWIR2) / (SWIR1 + SWIR2)
///
/// Applications: Alternative burn index using SWIR bands
pub fn nbr2(swir1: &ArrayView2<f64>, swir2: &ArrayView2<f64>) -> Result<Array2<f64>> {
    if swir1.dim() != swir2.dim() {
        return Err(SensorError::dimension_mismatch(
            format!("{:?}", swir1.dim()),
            format!("{:?}", swir2.dim()),
        ));
    }

    let mut nbr2 = Array2::zeros(swir1.dim());

    Zip::from(&mut nbr2)
        .and(swir1)
        .and(swir2)
        .for_each(|out, &s1, &s2| {
            let sum = s1 + s2;
            *out = if sum.abs() > 1e-10 {
                (s1 - s2) / sum
            } else {
                0.0
            };
        });

    Ok(nbr2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use scirs2_core::ndarray::array;

    #[test]
    fn test_nbr() {
        let nir = array![[0.5, 0.6]];
        let swir2 = array![[0.2, 0.2]];

        let result = nbr(&nir.view(), &swir2.view());
        assert!(result.is_ok());

        if let Ok(nbr_result) = result {
            assert!(nbr_result[[0, 0]] > 0.0); // Healthy vegetation has positive NBR
        }
    }

    #[test]
    fn test_dnbr() {
        let prefire = array![[0.6, 0.7]];
        let postfire = array![[0.1, 0.2]];

        let result = d_nbr(&prefire.view(), &postfire.view());
        assert!(result.is_ok());

        if let Ok(dnbr_result) = result {
            assert!(dnbr_result[[0, 0]] > 0.0); // Positive dNBR indicates burn
        }
    }

    #[test]
    fn test_nbr2() {
        let swir1 = array![[0.3, 0.3]];
        let swir2 = array![[0.2, 0.2]];

        let result = nbr2(&swir1.view(), &swir2.view());
        assert!(result.is_ok());
    }
}
