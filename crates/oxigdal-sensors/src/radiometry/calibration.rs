//! Radiometric calibration
//!
//! Converts Digital Numbers (DN) to physical units:
//! - DN → At-sensor radiance
//! - Radiance → Top-of-Atmosphere (TOA) reflectance
//! - Thermal calibration for TIR bands

use crate::error::{Result, SensorError};
use crate::sensors::Band;
use scirs2_core::ndarray::{Array2, ArrayView2, Zip};

/// Radiometric calibration for optical bands
pub struct RadiometricCalibration {
    /// Multiplicative rescaling factor (gain)
    pub mult: f64,

    /// Additive rescaling factor (bias)
    pub add: f64,

    /// Solar irradiance at top of atmosphere (W/m²/μm)
    pub esun: Option<f64>,
}

impl RadiometricCalibration {
    /// Create a new radiometric calibration
    pub fn new(mult: f64, add: f64) -> Self {
        Self {
            mult,
            add,
            esun: None,
        }
    }

    /// Create from band metadata
    pub fn from_band(band: &Band) -> Result<Self> {
        let mult = band
            .gain
            .ok_or_else(|| SensorError::calibration_error("Gain not set for band"))?;
        let add = band
            .bias
            .ok_or_else(|| SensorError::calibration_error("Bias not set for band"))?;

        Ok(Self {
            mult,
            add,
            esun: band.solar_irradiance,
        })
    }

    /// Set solar irradiance
    pub fn with_solar_irradiance(mut self, esun: f64) -> Self {
        self.esun = Some(esun);
        self
    }

    /// Convert DN to radiance
    ///
    /// Formula: L = mult * DN + add
    pub fn dn_to_radiance(&self, dn: &ArrayView2<f64>) -> Array2<f64> {
        dn.mapv(|v| self.mult * v + self.add)
    }

    /// Convert radiance to TOA reflectance
    ///
    /// Formula: ρ = (π * L * d²) / (ESUN * cos(θs))
    ///
    /// # Parameters
    /// - `radiance`: At-sensor radiance (W/m²/sr/μm)
    /// - `solar_zenith`: Solar zenith angle in degrees
    /// - `earth_sun_distance`: Earth-Sun distance in AU
    pub fn radiance_to_reflectance(
        &self,
        radiance: &ArrayView2<f64>,
        solar_zenith: f64,
        earth_sun_distance: f64,
    ) -> Result<Array2<f64>> {
        let esun = self
            .esun
            .ok_or_else(|| SensorError::calibration_error("Solar irradiance not set"))?;

        if !(0.0..=90.0).contains(&solar_zenith) {
            return Err(SensorError::invalid_parameter(
                "solar_zenith",
                "must be between 0 and 90 degrees",
            ));
        }

        let sz_rad = solar_zenith.to_radians();
        let cos_sz = sz_rad.cos();

        if cos_sz.abs() < 1e-10 {
            return Err(SensorError::division_by_zero("cos(solar_zenith) is zero"));
        }

        let factor =
            (std::f64::consts::PI * earth_sun_distance * earth_sun_distance) / (esun * cos_sz);

        Ok(radiance.mapv(|v| v * factor))
    }

    /// Convert DN to TOA reflectance (one-step conversion)
    pub fn dn_to_reflectance(
        &self,
        dn: &ArrayView2<f64>,
        solar_zenith: f64,
        earth_sun_distance: f64,
    ) -> Result<Array2<f64>> {
        let radiance = self.dn_to_radiance(dn);
        self.radiance_to_reflectance(&radiance.view(), solar_zenith, earth_sun_distance)
    }
}

/// Thermal calibration for thermal infrared bands
pub struct ThermalCalibration {
    /// Radiance multiplicative factor
    pub mult: f64,

    /// Radiance additive factor
    pub add: f64,

    /// Thermal constant K1
    pub k1: f64,

    /// Thermal constant K2
    pub k2: f64,
}

impl ThermalCalibration {
    /// Create a new thermal calibration
    pub fn new(mult: f64, add: f64, k1: f64, k2: f64) -> Self {
        Self { mult, add, k1, k2 }
    }

    /// Landsat 8 Band 10 thermal calibration
    pub fn landsat8_b10() -> Self {
        Self::new(0.0003342, 0.1, 774.8853, 1321.0789)
    }

    /// Landsat 8 Band 11 thermal calibration
    pub fn landsat8_b11() -> Self {
        Self::new(0.0003342, 0.1, 480.8883, 1201.1442)
    }

    /// Convert DN to radiance
    pub fn dn_to_radiance(&self, dn: &ArrayView2<f64>) -> Array2<f64> {
        dn.mapv(|v| self.mult * v + self.add)
    }

    /// Convert radiance to brightness temperature (Kelvin)
    ///
    /// Formula: T = K2 / ln((K1 / L) + 1)
    pub fn radiance_to_temperature(&self, radiance: &ArrayView2<f64>) -> Result<Array2<f64>> {
        let mut temp = Array2::zeros(radiance.dim());

        Zip::from(&mut temp).and(radiance).for_each(|t, &l| {
            if l <= 0.0 {
                *t = 0.0; // Invalid radiance
            } else {
                let ratio = self.k1 / l + 1.0;
                if ratio > 0.0 {
                    *t = self.k2 / ratio.ln();
                } else {
                    *t = 0.0;
                }
            }
        });

        Ok(temp)
    }

    /// Convert DN to brightness temperature (one-step conversion)
    pub fn dn_to_temperature(&self, dn: &ArrayView2<f64>) -> Result<Array2<f64>> {
        let radiance = self.dn_to_radiance(dn);
        self.radiance_to_temperature(&radiance.view())
    }

    /// Convert brightness temperature to radiance
    ///
    /// Formula: L = K1 / (exp(K2 / T) - 1)
    pub fn temperature_to_radiance(&self, temperature: &ArrayView2<f64>) -> Result<Array2<f64>> {
        let mut radiance = Array2::zeros(temperature.dim());

        Zip::from(&mut radiance).and(temperature).for_each(|l, &t| {
            if t <= 0.0 {
                *l = 0.0;
            } else {
                let exp_term = (self.k2 / t).exp();
                *l = self.k1 / (exp_term - 1.0);
            }
        });

        Ok(radiance)
    }
}

/// Calculate Earth-Sun distance for a given day of year
///
/// Returns distance in Astronomical Units (AU)
pub fn earth_sun_distance(day_of_year: u32) -> Result<f64> {
    if !(1..=366).contains(&day_of_year) {
        return Err(SensorError::invalid_parameter(
            "day_of_year",
            "must be between 1 and 366",
        ));
    }

    let d = day_of_year as f64;
    let distance = 1.0 - 0.01672 * (0.9856 * (d - 4.0) * std::f64::consts::PI / 180.0).cos();

    Ok(distance)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use scirs2_core::ndarray::array;

    #[test]
    fn test_dn_to_radiance() {
        let cal = RadiometricCalibration::new(0.00002, 0.0);
        let dn = array![[1000.0, 2000.0], [3000.0, 4000.0]];

        let radiance = cal.dn_to_radiance(&dn.view());

        assert_relative_eq!(radiance[[0, 0]], 0.02, epsilon = 1e-6);
        assert_relative_eq!(radiance[[0, 1]], 0.04, epsilon = 1e-6);
        assert_relative_eq!(radiance[[1, 0]], 0.06, epsilon = 1e-6);
        assert_relative_eq!(radiance[[1, 1]], 0.08, epsilon = 1e-6);
    }

    #[test]
    fn test_radiance_to_reflectance() {
        let cal = RadiometricCalibration::new(1.0, 0.0).with_solar_irradiance(1554.0);
        let radiance = array![[100.0, 200.0], [300.0, 400.0]];

        let reflectance = cal.radiance_to_reflectance(&radiance.view(), 30.0, 1.0);
        assert!(reflectance.is_ok());

        if let Ok(reflectance) = reflectance {
            assert!(reflectance[[0, 0]] > 0.0);
        }
    }

    #[test]
    fn test_thermal_calibration() {
        let cal = ThermalCalibration::landsat8_b10();
        let dn = array![[10000.0, 11000.0], [12000.0, 13000.0]];

        let radiance = cal.dn_to_radiance(&dn.view());
        assert!(radiance[[0, 0]] > 0.0);

        let temp = cal.dn_to_temperature(&dn.view());
        assert!(temp.is_ok());

        if let Ok(temp) = temp {
            // Temperature should be reasonable (in Kelvin)
            assert!(temp[[0, 0]] > 200.0 && temp[[0, 0]] < 400.0);
        }
    }

    #[test]
    fn test_earth_sun_distance() {
        // Winter solstice (approx day 355)
        let d1 = earth_sun_distance(355);
        assert!(d1.is_ok());

        // Summer solstice (approx day 172)
        let d2 = earth_sun_distance(172);
        assert!(d2.is_ok());

        // Earth is closer in winter (northern hemisphere)
        if let (Ok(d1), Ok(d2)) = (d1, d2) {
            assert!(d1 < d2);
        }
    }

    #[test]
    fn test_invalid_solar_zenith() {
        let cal = RadiometricCalibration::new(1.0, 0.0).with_solar_irradiance(1554.0);
        let radiance = array![[100.0]];

        let result = cal.radiance_to_reflectance(&radiance.view(), 95.0, 1.0);
        assert!(result.is_err());
    }
}
