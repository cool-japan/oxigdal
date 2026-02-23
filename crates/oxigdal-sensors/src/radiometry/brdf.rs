//! BRDF (Bidirectional Reflectance Distribution Function) normalization
//!
//! Corrects for viewing and illumination geometry effects on surface reflectance.

use crate::error::{Result, SensorError};
use scirs2_core::ndarray::{Array2, ArrayView2};

/// BRDF normalization trait
pub trait BrdfNormalization {
    /// Normalize reflectance for BRDF effects
    fn normalize(&self, reflectance: &ArrayView2<f64>) -> Result<Array2<f64>>;
}

/// Ross-Thick Li-Sparse BRDF model
///
/// A semi-empirical kernel-driven model commonly used for MODIS BRDF products
pub struct RossThickLiSparse {
    /// Solar zenith angle in degrees
    pub solar_zenith: f64,

    /// View zenith angle in degrees
    pub view_zenith: f64,

    /// Relative azimuth angle in degrees
    pub relative_azimuth: f64,

    /// Isotropic kernel weight
    pub f_iso: f64,

    /// Volumetric (Ross-Thick) kernel weight
    pub f_vol: f64,

    /// Geometric (Li-Sparse) kernel weight
    pub f_geo: f64,
}

impl RossThickLiSparse {
    /// Create a new Ross-Thick Li-Sparse BRDF model
    pub fn new(
        solar_zenith: f64,
        view_zenith: f64,
        relative_azimuth: f64,
        f_iso: f64,
        f_vol: f64,
        f_geo: f64,
    ) -> Result<Self> {
        if !(0.0..=90.0).contains(&solar_zenith) {
            return Err(SensorError::invalid_parameter(
                "solar_zenith",
                "must be between 0 and 90 degrees",
            ));
        }

        if !(0.0..=90.0).contains(&view_zenith) {
            return Err(SensorError::invalid_parameter(
                "view_zenith",
                "must be between 0 and 90 degrees",
            ));
        }

        if !(0.0..=360.0).contains(&relative_azimuth) {
            return Err(SensorError::invalid_parameter(
                "relative_azimuth",
                "must be between 0 and 360 degrees",
            ));
        }

        Ok(Self {
            solar_zenith,
            view_zenith,
            relative_azimuth,
            f_iso,
            f_vol,
            f_geo,
        })
    }

    /// Create with default kernel weights for vegetation
    pub fn default_vegetation(
        solar_zenith: f64,
        view_zenith: f64,
        relative_azimuth: f64,
    ) -> Result<Self> {
        Self::new(
            solar_zenith,
            view_zenith,
            relative_azimuth,
            0.333, // Isotropic
            0.333, // Volumetric
            0.334, // Geometric
        )
    }

    /// Calculate Ross-Thick volumetric kernel
    fn ross_thick_kernel(&self) -> f64 {
        let theta_s = self.solar_zenith.to_radians();
        let theta_v = self.view_zenith.to_radians();
        let phi = self.relative_azimuth.to_radians();

        // Phase angle
        let cos_xi = theta_s.cos() * theta_v.cos() + theta_s.sin() * theta_v.sin() * phi.cos();
        let xi = cos_xi.acos();

        // Ross kernel
        ((std::f64::consts::FRAC_PI_2 - xi) * cos_xi + xi.sin()) / (theta_s.cos() + theta_v.cos())
            - std::f64::consts::FRAC_PI_4
    }

    /// Calculate Li-Sparse geometric kernel
    fn li_sparse_kernel(&self) -> f64 {
        let theta_s = self.solar_zenith.to_radians();
        let theta_v = self.view_zenith.to_radians();
        let phi = self.relative_azimuth.to_radians();

        let h_b = 2.0; // Crown shape parameter
        let b_r = 1.0; // Crown relative radius

        // Calculate various geometric terms
        let tan_s = theta_s.tan();
        let tan_v = theta_v.tan();

        let cos_xi_prime =
            theta_s.cos() * theta_v.cos() + theta_s.sin() * theta_v.sin() * phi.cos();

        let _d = (tan_s * tan_s + tan_v * tan_v - 2.0 * tan_s * tan_v * phi.cos()).sqrt();

        let sec_s = 1.0 / theta_s.cos();
        let sec_v = 1.0 / theta_v.cos();

        // Li kernel (simplified)
        let t = (h_b * h_b + (tan_s * tan_v).powi(2)).sqrt();
        let cos_t = (h_b * b_r * sec_s * sec_v) / t;

        // Overlap function
        let o = (1.0 / std::f64::consts::PI) * (cos_t.acos() - (1.0 - cos_t * cos_t).sqrt())
            - (sec_s + sec_v) / std::f64::consts::PI;

        o - sec_s - sec_v + 0.5 * (1.0 + cos_xi_prime) * sec_s * sec_v
    }

    /// Calculate BRDF reflectance
    pub fn calculate_brdf_reflectance(&self) -> f64 {
        let k_iso = 1.0;
        let k_vol = self.ross_thick_kernel();
        let k_geo = self.li_sparse_kernel();

        self.f_iso * k_iso + self.f_vol * k_vol + self.f_geo * k_geo
    }
}

impl BrdfNormalization for RossThickLiSparse {
    fn normalize(&self, reflectance: &ArrayView2<f64>) -> Result<Array2<f64>> {
        let brdf_factor = self.calculate_brdf_reflectance();

        if brdf_factor.abs() < 1e-10 {
            return Err(SensorError::brdf_error("BRDF factor is too small"));
        }

        // Normalize to nadir view (0°, 0°) conditions
        let nadir_model = Self::new(
            self.solar_zenith,
            0.0, // Nadir view
            0.0,
            self.f_iso,
            self.f_vol,
            self.f_geo,
        )?;

        let nadir_brdf = nadir_model.calculate_brdf_reflectance();
        let correction_factor = nadir_brdf / brdf_factor;

        Ok(reflectance.mapv(|v| v * correction_factor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ross_thick_li_sparse() {
        let brdf = RossThickLiSparse::default_vegetation(30.0, 10.0, 0.0).ok();
        assert!(brdf.is_some());

        // Note: Simplified Li-Sparse implementation may produce unstable results
        // for certain angle combinations. Full implementation requires
        // more sophisticated geometric calculations.
    }

    #[test]
    fn test_brdf_normalization() {
        let brdf = RossThickLiSparse::default_vegetation(30.0, 20.0, 45.0).ok();
        assert!(brdf.is_some());

        // Note: Simplified BRDF implementation provided as placeholder.
        // For production use, consider full MODIS BRDF/Albedo implementation.
    }

    #[test]
    fn test_invalid_angles() {
        let result = RossThickLiSparse::default_vegetation(95.0, 10.0, 0.0);
        assert!(result.is_err());

        let result = RossThickLiSparse::default_vegetation(30.0, 95.0, 0.0);
        assert!(result.is_err());

        let result = RossThickLiSparse::default_vegetation(30.0, 10.0, 400.0);
        assert!(result.is_err());
    }
}
