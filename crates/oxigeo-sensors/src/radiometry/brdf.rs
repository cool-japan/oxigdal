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

    /// Calculate the Li-Sparse-Reciprocal geometric-optical kernel.
    ///
    /// Implements the kernel exactly as published in Wanner, Li & Strahler
    /// (1995) and Lucht, Schaaf & Strahler (2000), the same formulation used
    /// by the MODIS BRDF/Albedo (MCD43) products:
    ///
    /// ```text
    /// theta'  = arctan( (b/r) * tan(theta) )
    /// D       = sqrt( tan^2 theta_s' + tan^2 theta_v'
    ///                 - 2 * tan theta_s' * tan theta_v' * cos(phi) )
    /// cos(t)  = (h/b) * sqrt( D^2 + (tan theta_s' * tan theta_v' * sin phi)^2 )
    ///                 / (sec theta_s' + sec theta_v')          [clamped to -1..1]
    /// O       = (1/pi) * (t - sin t * cos t) * (sec theta_s' + sec theta_v')
    /// K_geo   = O - sec theta_s' - sec theta_v'
    ///                 + 0.5 * (1 + cos xi') * sec theta_s' * sec theta_v'
    /// ```
    ///
    /// `cos(t)` is clamped to `[-1, 1]` per the literature (values outside this
    /// range correspond to no mutual shadowing) so the returned value is always
    /// finite for any valid `0 <= theta < 90` geometry.
    fn li_sparse_kernel(&self) -> f64 {
        let theta_s = self.solar_zenith.to_radians();
        let theta_v = self.view_zenith.to_radians();
        let phi = self.relative_azimuth.to_radians();

        // Crown shape parameters used by the MODIS Li-Sparse-Reciprocal kernel.
        let h_b = 2.0; // h/b: crown-center height over vertical crown radius
        let b_r = 1.0; // b/r: vertical over horizontal crown radius

        // Adjust the zenith angles for the (non-spherical) crown shape:
        //   theta' = arctan( (b/r) * tan(theta) )
        let theta_s_p = (b_r * theta_s.tan()).atan();
        let theta_v_p = (b_r * theta_v.tan()).atan();

        let tan_s = theta_s_p.tan();
        let tan_v = theta_v_p.tan();
        let sec_s = 1.0 / theta_s_p.cos();
        let sec_v = 1.0 / theta_v_p.cos();
        let cos_phi = phi.cos();
        let sin_phi = phi.sin();

        // Phase-angle cosine in the primed (crown-adjusted) geometry.
        let cos_xi_prime =
            theta_s_p.cos() * theta_v_p.cos() + theta_s_p.sin() * theta_v_p.sin() * cos_phi;

        // Overlap distance D between the sun and view shadow ellipses (squared).
        let d_sq = (tan_s * tan_s + tan_v * tan_v - 2.0 * tan_s * tan_v * cos_phi).max(0.0);

        // cos(t), clamped to [-1, 1] per Wanner et al. 1995.
        let sec_sum = sec_s + sec_v;
        let cos_t =
            (h_b * (d_sq + (tan_s * tan_v * sin_phi).powi(2)).sqrt() / sec_sum).clamp(-1.0, 1.0);
        let t = cos_t.acos();
        let sin_t = (1.0 - cos_t * cos_t).max(0.0).sqrt();

        // Overlap area between the two shadow ellipses.
        let overlap = std::f64::consts::FRAC_1_PI * (t - sin_t * cos_t) * sec_sum;

        // Li-Sparse-Reciprocal geometric-optical kernel.
        overlap - sec_s - sec_v + 0.5 * (1.0 + cos_xi_prime) * sec_s * sec_v
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

        if !brdf_factor.is_finite() {
            return Err(SensorError::brdf_error(
                "BRDF factor is not finite (NaN or infinity)",
            ));
        }

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

        if !correction_factor.is_finite() {
            return Err(SensorError::brdf_error(
                "BRDF correction factor is not finite (NaN or infinity)",
            ));
        }

        Ok(reflectance.mapv(|v| v * correction_factor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ross_thick_li_sparse() {
        let brdf = RossThickLiSparse::default_vegetation(30.0, 10.0, 0.0).expect("valid geometry");

        // The Li-Sparse-Reciprocal kernel must be finite for ordinary
        // geometries (this previously produced NaN because cos_t exceeded 1
        // and was never clamped before acos()).
        let k_geo = brdf.li_sparse_kernel();
        assert!(
            k_geo.is_finite(),
            "Li-Sparse kernel must be finite, got {k_geo}"
        );

        let k_vol = brdf.ross_thick_kernel();
        assert!(
            k_vol.is_finite(),
            "Ross-Thick kernel must be finite, got {k_vol}"
        );

        let refl = brdf.calculate_brdf_reflectance();
        assert!(
            refl.is_finite(),
            "BRDF reflectance must be finite, got {refl}"
        );
    }

    #[test]
    fn test_li_sparse_kernel_finite_across_geometry_grid() {
        // Sweep a wide grid of geometries: none must yield a NaN/inf kernel.
        for &sz in &[0.0, 15.0, 30.0, 45.0, 60.0, 75.0, 89.0] {
            for &vz in &[0.0, 10.0, 25.0, 40.0, 55.0, 70.0, 89.0] {
                for &raz in &[0.0, 45.0, 90.0, 135.0, 180.0, 270.0, 359.0] {
                    let brdf =
                        RossThickLiSparse::default_vegetation(sz, vz, raz).expect("valid geometry");
                    let k = brdf.li_sparse_kernel();
                    assert!(
                        k.is_finite(),
                        "Li-Sparse kernel non-finite at sz={sz}, vz={vz}, raz={raz}: {k}"
                    );
                    let refl = brdf.calculate_brdf_reflectance();
                    assert!(
                        refl.is_finite(),
                        "BRDF reflectance non-finite at sz={sz}, vz={vz}, raz={raz}: {refl}"
                    );
                }
            }
        }
    }

    #[test]
    fn test_li_sparse_reference_value() {
        // Reference value computed from the Wanner et al. 1995 /
        // Lucht, Schaaf & Strahler 2000 Li-Sparse-Reciprocal formulation
        // (h/b = 2, b/r = 1) for sz=30, vz=10, raz=0 degrees.
        let brdf = RossThickLiSparse::default_vegetation(30.0, 10.0, 0.0).expect("valid geometry");
        let k_geo = brdf.li_sparse_kernel();
        // K_geo ~= -0.446 for this geometry.
        assert!(
            (k_geo - (-0.4464)).abs() < 1e-2,
            "Li-Sparse kernel out of expected range: {k_geo}"
        );
    }

    #[test]
    fn test_brdf_normalization() {
        use scirs2_core::ndarray::Array2;

        let brdf = RossThickLiSparse::default_vegetation(30.0, 20.0, 45.0).expect("valid geometry");

        let reflectance = Array2::from_elem((4, 4), 0.25_f64);
        let normalized = brdf
            .normalize(&reflectance.view())
            .expect("normalization should succeed with finite kernels");

        // Every output value must be finite (no NaN poisoning) and positive
        // for a positive input reflectance.
        assert_eq!(normalized.dim(), (4, 4));
        for &v in normalized.iter() {
            assert!(
                v.is_finite(),
                "normalized reflectance must be finite, got {v}"
            );
            assert!(v > 0.0, "normalized reflectance must be positive, got {v}");
        }
    }

    #[test]
    fn test_brdf_normalization_nadir_identity() {
        use scirs2_core::ndarray::Array2;

        // At nadir view with zero relative azimuth, the correction factor is
        // exactly nadir_brdf / brdf_factor where the model already equals its
        // nadir reference, so normalization must be an identity map.
        let brdf = RossThickLiSparse::default_vegetation(30.0, 0.0, 0.0).expect("valid geometry");
        let reflectance = Array2::from_elem((2, 2), 0.5_f64);
        let normalized = brdf
            .normalize(&reflectance.view())
            .expect("normalization should succeed");
        for &v in normalized.iter() {
            assert!(
                (v - 0.5).abs() < 1e-9,
                "nadir normalization must be identity, got {v}"
            );
        }
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
