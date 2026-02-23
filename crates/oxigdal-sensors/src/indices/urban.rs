//! Urban indices for built-up area detection and analysis

use crate::error::{Result, SensorError};
use scirs2_core::ndarray::{Array2, ArrayView2, Zip};

/// Normalized Difference Built-up Index (NDBI)
///
/// Formula: NDBI = (SWIR1 - NIR) / (SWIR1 + NIR)
///
/// Range: -1 to 1 (higher values indicate built-up areas)
/// Applications: Urban area mapping, impervious surface detection
pub fn ndbi(swir1: &ArrayView2<f64>, nir: &ArrayView2<f64>) -> Result<Array2<f64>> {
    if swir1.dim() != nir.dim() {
        return Err(SensorError::dimension_mismatch(
            format!("{:?}", swir1.dim()),
            format!("{:?}", nir.dim()),
        ));
    }

    let mut ndbi = Array2::zeros(swir1.dim());

    Zip::from(&mut ndbi)
        .and(swir1)
        .and(nir)
        .for_each(|out, &s, &n| {
            let sum = s + n;
            *out = if sum.abs() > 1e-10 {
                (s - n) / sum
            } else {
                0.0
            };
        });

    Ok(ndbi)
}

/// Urban Index (UI)
///
/// Formula: UI = (SWIR2 - NIR) / (SWIR2 + NIR)
///
/// Applications: Alternative urban index using SWIR2
pub fn ui(swir2: &ArrayView2<f64>, nir: &ArrayView2<f64>) -> Result<Array2<f64>> {
    if swir2.dim() != nir.dim() {
        return Err(SensorError::dimension_mismatch(
            format!("{:?}", swir2.dim()),
            format!("{:?}", nir.dim()),
        ));
    }

    let mut ui = Array2::zeros(swir2.dim());

    Zip::from(&mut ui)
        .and(swir2)
        .and(nir)
        .for_each(|out, &s, &n| {
            let sum = s + n;
            *out = if sum.abs() > 1e-10 {
                (s - n) / sum
            } else {
                0.0
            };
        });

    Ok(ui)
}

/// Index-based Built-up Index (IBI)
///
/// Formula: IBI = (NDBI - (SAVI + MNDWI)/2) / (NDBI + (SAVI + MNDWI)/2)
///
/// Applications: Enhanced built-up area detection combining multiple indices
pub fn ibi(
    ndbi: &ArrayView2<f64>,
    savi: &ArrayView2<f64>,
    mndwi: &ArrayView2<f64>,
) -> Result<Array2<f64>> {
    if ndbi.dim() != savi.dim() || ndbi.dim() != mndwi.dim() {
        return Err(SensorError::dimension_mismatch(
            format!("{:?}", ndbi.dim()),
            format!("SAVI: {:?}, MNDWI: {:?}", savi.dim(), mndwi.dim()),
        ));
    }

    let mut ibi = Array2::zeros(ndbi.dim());

    Zip::from(&mut ibi)
        .and(ndbi)
        .and(savi)
        .and(mndwi)
        .for_each(|out, &nd, &sa, &mw| {
            let avg = (sa + mw) / 2.0;
            let denom = nd + avg;
            *out = if denom.abs() > 1e-10 {
                (nd - avg) / denom
            } else {
                0.0
            };
        });

    Ok(ibi)
}

#[cfg(test)]
mod tests {
    use super::*;
    use scirs2_core::ndarray::array;

    #[test]
    fn test_ndbi() {
        let swir1 = array![[0.4, 0.4]];
        let nir = array![[0.2, 0.2]];

        let result = ndbi(&swir1.view(), &nir.view());
        assert!(result.is_ok());

        if let Ok(ndbi_result) = result {
            assert!(ndbi_result[[0, 0]] > 0.0); // Built-up areas have positive NDBI
        }
    }

    #[test]
    fn test_ui() {
        let swir2 = array![[0.3, 0.3]];
        let nir = array![[0.2, 0.2]];

        let result = ui(&swir2.view(), &nir.view());
        assert!(result.is_ok());
    }

    #[test]
    fn test_ibi() {
        let ndbi_data = array![[0.3, 0.4]];
        let savi = array![[0.1, 0.1]];
        let mndwi = array![[-0.2, -0.2]];

        let result = ibi(&ndbi_data.view(), &savi.view(), &mndwi.view());
        assert!(result.is_ok());

        if let Ok(ibi_result) = result {
            assert!(ibi_result[[0, 0]].is_finite());
        }
    }
}
