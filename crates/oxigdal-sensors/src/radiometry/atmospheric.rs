//! Atmospheric correction algorithms
//!
//! Provides methods to correct for atmospheric effects in optical remote sensing data.

use crate::error::{Result, SensorError};
use scirs2_core::ndarray::{Array2, ArrayView2, Zip};

/// Atmospheric correction methods
pub trait AtmosphericCorrection {
    /// Apply atmospheric correction to TOA reflectance
    fn correct(&self, toa_reflectance: &ArrayView2<f64>) -> Result<Array2<f64>>;
}

/// Dark Object Subtraction (DOS) atmospheric correction
///
/// A simple empirical method that assumes the darkest pixels in the image
/// should have zero reflectance, and attributes their non-zero values to atmospheric scattering.
pub struct DarkObjectSubtraction {
    /// Threshold for dark object detection (quantile)
    pub dark_object_quantile: f64,

    /// Minimum dark object value (prevents over-correction)
    pub min_dark_value: f64,
}

impl DarkObjectSubtraction {
    /// Create a new DOS corrector
    ///
    /// # Parameters
    /// - `dark_object_quantile`: Quantile for dark object selection (e.g., 0.01 for 1st percentile)
    /// - `min_dark_value`: Minimum value to prevent over-correction
    pub fn new(dark_object_quantile: f64, min_dark_value: f64) -> Result<Self> {
        if !(0.0..=1.0).contains(&dark_object_quantile) {
            return Err(SensorError::invalid_parameter(
                "dark_object_quantile",
                "must be between 0.0 and 1.0",
            ));
        }

        Ok(Self {
            dark_object_quantile,
            min_dark_value,
        })
    }

    /// Create DOS corrector with default parameters
    pub fn default_params() -> Self {
        Self {
            dark_object_quantile: 0.01, // 1st percentile
            min_dark_value: 0.0001,
        }
    }

    /// Estimate dark object value from image
    fn estimate_dark_value(&self, data: &ArrayView2<f64>) -> Result<f64> {
        // Collect all valid (non-NaN) values
        let mut values: Vec<f64> = data
            .iter()
            .filter(|&&v| v.is_finite() && v > 0.0)
            .copied()
            .collect();

        if values.is_empty() {
            return Err(SensorError::atmospheric_correction_error(
                "No valid pixels found for dark object estimation",
            ));
        }

        // Sort to find quantile
        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let index = (values.len() as f64 * self.dark_object_quantile) as usize;
        let index = index.min(values.len() - 1);

        let dark_value = values[index].max(self.min_dark_value);

        Ok(dark_value)
    }
}

impl AtmosphericCorrection for DarkObjectSubtraction {
    fn correct(&self, toa_reflectance: &ArrayView2<f64>) -> Result<Array2<f64>> {
        let dark_value = self.estimate_dark_value(toa_reflectance)?;

        // Subtract dark object value
        let mut corrected = Array2::zeros(toa_reflectance.dim());

        Zip::from(&mut corrected)
            .and(toa_reflectance)
            .for_each(|c, &toa| {
                *c = (toa - dark_value).max(0.0);
            });

        Ok(corrected)
    }
}

/// Cosine correction for topographic illumination effects
///
/// Corrects for terrain slope and aspect effects on illumination
pub struct CosineCorrection {
    /// Solar zenith angle in degrees
    pub solar_zenith: f64,

    /// Solar azimuth angle in degrees
    pub solar_azimuth: f64,
}

impl CosineCorrection {
    /// Create a new cosine correction
    pub fn new(solar_zenith: f64, solar_azimuth: f64) -> Result<Self> {
        if !(0.0..=90.0).contains(&solar_zenith) {
            return Err(SensorError::invalid_parameter(
                "solar_zenith",
                "must be between 0 and 90 degrees",
            ));
        }

        if !(0.0..=360.0).contains(&solar_azimuth) {
            return Err(SensorError::invalid_parameter(
                "solar_azimuth",
                "must be between 0 and 360 degrees",
            ));
        }

        Ok(Self {
            solar_zenith,
            solar_azimuth,
        })
    }

    /// Apply correction given slope and aspect
    ///
    /// # Parameters
    /// - `reflectance`: Input reflectance
    /// - `slope`: Terrain slope in degrees
    /// - `aspect`: Terrain aspect in degrees (0 = North, 90 = East, etc.)
    pub fn correct_with_terrain(
        &self,
        reflectance: &ArrayView2<f64>,
        slope: &ArrayView2<f64>,
        aspect: &ArrayView2<f64>,
    ) -> Result<Array2<f64>> {
        if reflectance.dim() != slope.dim() || reflectance.dim() != aspect.dim() {
            return Err(SensorError::dimension_mismatch(
                format!("{:?}", reflectance.dim()),
                format!("slope: {:?}, aspect: {:?}", slope.dim(), aspect.dim()),
            ));
        }

        let sz_rad = self.solar_zenith.to_radians();
        let sa_rad = self.solar_azimuth.to_radians();

        let mut corrected = Array2::zeros(reflectance.dim());

        Zip::from(&mut corrected)
            .and(reflectance)
            .and(slope)
            .and(aspect)
            .for_each(|c, &refl, &slp, &asp| {
                let slp_rad = slp.to_radians();
                let asp_rad = asp.to_radians();

                // Calculate illumination angle (incidence angle)
                let cos_i = sz_rad.cos() * slp_rad.cos()
                    + sz_rad.sin() * slp_rad.sin() * (sa_rad - asp_rad).cos();

                // Apply correction (avoid division by zero)
                if cos_i.abs() > 1e-6 {
                    *c = refl * sz_rad.cos() / cos_i;
                } else {
                    *c = refl;
                }
            });

        Ok(corrected)
    }
}

/// Simple haze removal using band subtraction
pub struct HazeRemoval {
    /// Haze value to subtract
    pub haze_value: f64,
}

impl HazeRemoval {
    /// Create a new haze removal corrector
    pub fn new(haze_value: f64) -> Self {
        Self { haze_value }
    }

    /// Auto-detect haze from blue band
    pub fn auto_detect(blue_band: &ArrayView2<f64>) -> Result<Self> {
        // Use minimum value in blue band as haze estimate
        let haze_value = blue_band
            .iter()
            .filter(|&&v| v.is_finite() && v > 0.0)
            .fold(f64::INFINITY, |min, &v| min.min(v));

        if haze_value.is_infinite() {
            return Err(SensorError::atmospheric_correction_error(
                "Could not auto-detect haze value",
            ));
        }

        Ok(Self { haze_value })
    }

    /// Apply haze removal
    pub fn remove(&self, reflectance: &ArrayView2<f64>) -> Array2<f64> {
        reflectance.mapv(|v| (v - self.haze_value).max(0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use scirs2_core::ndarray::array;

    #[test]
    fn test_dos_correction() {
        let dos = DarkObjectSubtraction::default_params();

        let toa = array![[0.05, 0.10, 0.15], [0.06, 0.12, 0.18], [0.07, 0.14, 0.21],];

        let corrected = dos.correct(&toa.view());
        assert!(corrected.is_ok());

        if let Ok(corrected) = corrected {
            // Dark object (minimum) should be close to zero after correction
            assert!(corrected[[0, 0]] < toa[[0, 0]]);
        }
    }

    #[test]
    fn test_cosine_correction() {
        let corrector = CosineCorrection::new(30.0, 180.0);
        assert!(corrector.is_ok());

        if let Ok(corrector) = corrector {
            let refl = array![[0.1, 0.2], [0.3, 0.4]];
            let slope = array![[10.0, 15.0], [20.0, 25.0]];
            let aspect = array![[180.0, 180.0], [180.0, 180.0]];

            let corrected =
                corrector.correct_with_terrain(&refl.view(), &slope.view(), &aspect.view());
            assert!(corrected.is_ok());
        }
    }

    #[test]
    fn test_haze_removal() {
        let haze = HazeRemoval::new(0.05);

        let refl = array![[0.10, 0.20], [0.30, 0.40]];
        let corrected = haze.remove(&refl.view());

        assert_relative_eq!(corrected[[0, 0]], 0.05, epsilon = 1e-6);
        assert_relative_eq!(corrected[[0, 1]], 0.15, epsilon = 1e-6);
    }

    #[test]
    fn test_haze_auto_detect() {
        let blue = array![[0.08, 0.12], [0.10, 0.15]];

        let haze = HazeRemoval::auto_detect(&blue.view());
        assert!(haze.is_ok());

        if let Ok(haze) = haze {
            assert_relative_eq!(haze.haze_value, 0.08, epsilon = 1e-6);
        }
    }

    #[test]
    fn test_invalid_parameters() {
        let result = DarkObjectSubtraction::new(1.5, 0.0);
        assert!(result.is_err());

        let result = CosineCorrection::new(95.0, 180.0);
        assert!(result.is_err());

        let result = CosineCorrection::new(30.0, 400.0);
        assert!(result.is_err());
    }
}
