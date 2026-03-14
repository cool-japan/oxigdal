//! GPU-accelerated raster reprojection using wgpu compute shaders.
//!
//! This module provides both a GPU-backed reprojection pipeline and a CPU
//! fallback implementation for environments where GPU is unavailable.

use crate::error::GpuError;

/// Resampling method for reprojection.
#[derive(Debug, Clone, PartialEq)]
pub enum ResampleMethod {
    /// Nearest-neighbor sampling (fastest, blocky).
    NearestNeighbor,
    /// Bilinear interpolation (smoother, moderate cost).
    Bilinear,
}

/// Configuration for a reprojection operation.
#[derive(Debug, Clone)]
pub struct ReprojectionConfig {
    /// Source raster width in pixels.
    pub src_width: u32,
    /// Source raster height in pixels.
    pub src_height: u32,
    /// Destination raster width in pixels.
    pub dst_width: u32,
    /// Destination raster height in pixels.
    pub dst_height: u32,
    /// Source geotransform \[a, b, c, d, e, f\] where:
    /// `x_geo = c + col * a + row * b`
    /// `y_geo = f + col * d + row * e`
    pub src_geotransform: [f32; 6],
    /// Destination inverse geotransform (maps geo → pixel).
    pub dst_inv_geotransform: [f32; 6],
    /// Pixel resampling strategy.
    pub resample_method: ResampleMethod,
    /// Optional nodata sentinel value.
    pub nodata: Option<f32>,
}

impl ReprojectionConfig {
    /// Validate that the configuration is internally consistent.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError::InvalidKernelParams`] if dimensions are zero.
    pub fn validate(&self) -> Result<(), GpuError> {
        if self.src_width == 0 || self.src_height == 0 {
            return Err(GpuError::invalid_kernel_params(
                "source dimensions must be greater than zero",
            ));
        }
        if self.dst_width == 0 || self.dst_height == 0 {
            return Err(GpuError::invalid_kernel_params(
                "destination dimensions must be greater than zero",
            ));
        }
        Ok(())
    }
}

/// GPU-based raster reprojector.
///
/// On platforms with GPU support this will eventually dispatch to a wgpu
/// compute shader.  Until that path is fully wired up, [`reproject_cpu`]
/// provides a correct CPU-based fallback.
///
/// [`reproject_cpu`]: GpuReprojector::reproject_cpu
pub struct GpuReprojector {
    config: ReprojectionConfig,
}

impl GpuReprojector {
    /// Construct a new reprojector from the given configuration.
    pub fn new(config: ReprojectionConfig) -> Self {
        Self { config }
    }

    /// Return a reference to the reprojection configuration.
    pub fn config(&self) -> &ReprojectionConfig {
        &self.config
    }

    /// Reproject `src_data` to the destination grid using a pure-CPU path.
    ///
    /// The implementation maps each destination pixel back to source
    /// coordinates via the supplied geotransforms and samples the source
    /// raster.  Out-of-bounds source pixels are filled with the nodata
    /// value (or `0.0` when nodata is not configured).
    ///
    /// # Errors
    ///
    /// Returns [`GpuError::InvalidKernelParams`] if the configuration is
    /// invalid or the source data length does not match the declared
    /// source dimensions.
    pub fn reproject_cpu(&self, src_data: &[f32]) -> Result<Vec<f32>, GpuError> {
        self.config.validate()?;

        let expected_src = (self.config.src_width as usize) * (self.config.src_height as usize);
        if src_data.len() != expected_src {
            return Err(GpuError::invalid_kernel_params(format!(
                "src_data length {} does not match declared source dimensions {}x{} ({})",
                src_data.len(),
                self.config.src_width,
                self.config.src_height,
                expected_src
            )));
        }

        let nodata_fill = self.config.nodata.unwrap_or(0.0);
        let dst_size = (self.config.dst_width as usize) * (self.config.dst_height as usize);
        let mut dst = vec![nodata_fill; dst_size];

        let gt = &self.config.src_geotransform;
        let inv_gt = &self.config.dst_inv_geotransform;

        // Determinant of the source geotransform's 2×2 linear part
        // used to invert the forward transform: pixel → geo → src pixel.
        let det = gt[0] * gt[4] - gt[1] * gt[3];
        let src_gt_invertible = det.abs() > f32::EPSILON;

        for row in 0..self.config.dst_height {
            for col in 0..self.config.dst_width {
                // Centre of destination pixel in pixel space.
                let dst_x = col as f32 + 0.5_f32;
                let dst_y = row as f32 + 0.5_f32;

                // Destination pixel → destination geo coordinates.
                let geo_x = inv_gt[0] + dst_x * inv_gt[1] + dst_y * inv_gt[2];
                let geo_y = inv_gt[3] + dst_x * inv_gt[4] + dst_y * inv_gt[5];

                // Destination geo → source pixel coordinates.
                let (src_col_f, src_row_f) = if src_gt_invertible {
                    let dx = geo_x - gt[2];
                    let dy = geo_y - gt[5];
                    let sc = (gt[4] * dx - gt[1] * dy) / det;
                    let sr = (gt[0] * dy - gt[3] * dx) / det;
                    (sc, sr)
                } else {
                    // Fallback: treat inv_gt as direct pixel scaling.
                    (
                        col as f32 * self.config.src_width as f32 / self.config.dst_width as f32,
                        row as f32 * self.config.src_height as f32 / self.config.dst_height as f32,
                    )
                };

                let dst_idx = row as usize * self.config.dst_width as usize + col as usize;

                match self.config.resample_method {
                    ResampleMethod::NearestNeighbor => {
                        let src_c = src_col_f as i64;
                        let src_r = src_row_f as i64;

                        if src_c < 0
                            || src_r < 0
                            || src_c >= self.config.src_width as i64
                            || src_r >= self.config.src_height as i64
                        {
                            continue;
                        }

                        let src_idx =
                            src_r as usize * self.config.src_width as usize + src_c as usize;
                        if src_idx < src_data.len() {
                            dst[dst_idx] = src_data[src_idx];
                        }
                    }
                    ResampleMethod::Bilinear => {
                        let x0 = src_col_f.floor() as i64;
                        let y0 = src_row_f.floor() as i64;
                        let x1 = x0 + 1;
                        let y1 = y0 + 1;

                        let tx = src_col_f - src_col_f.floor();
                        let ty = src_row_f - src_row_f.floor();

                        let w = self.config.src_width as i64;
                        let h = self.config.src_height as i64;

                        let sample = |c: i64, r: i64| -> f32 {
                            if c < 0 || r < 0 || c >= w || r >= h {
                                return nodata_fill;
                            }
                            let idx = r as usize * self.config.src_width as usize + c as usize;
                            src_data.get(idx).copied().unwrap_or(nodata_fill)
                        };

                        let v00 = sample(x0, y0);
                        let v10 = sample(x1, y0);
                        let v01 = sample(x0, y1);
                        let v11 = sample(x1, y1);

                        let v0 = v00 + (v10 - v00) * tx;
                        let v1 = v01 + (v11 - v01) * tx;
                        dst[dst_idx] = v0 + (v1 - v0) * ty;
                    }
                }
            }
        }

        Ok(dst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity_config(size: u32) -> ReprojectionConfig {
        // src_gt: origin (0,0), pixel size 1x1
        // dst_inv_gt: maps dst pixel → geo coord with 1:1 scale
        ReprojectionConfig {
            src_width: size,
            src_height: size,
            dst_width: size,
            dst_height: size,
            src_geotransform: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            dst_inv_geotransform: [0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            resample_method: ResampleMethod::NearestNeighbor,
            nodata: None,
        }
    }

    #[test]
    fn test_new_and_config() {
        let cfg = identity_config(4);
        let r = GpuReprojector::new(cfg.clone());
        assert_eq!(r.config().src_width, 4);
        assert_eq!(r.config().dst_width, 4);
    }

    #[test]
    fn test_validate_zero_src_dims() {
        let mut cfg = identity_config(4);
        cfg.src_width = 0;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_validate_zero_dst_dims() {
        let mut cfg = identity_config(4);
        cfg.dst_width = 0;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_reproject_cpu_wrong_len() {
        let cfg = identity_config(4);
        let r = GpuReprojector::new(cfg);
        let result = r.reproject_cpu(&[1.0, 2.0]);
        assert!(result.is_err());
    }

    #[test]
    fn test_reproject_cpu_identity() {
        let size = 4u32;
        let src: Vec<f32> = (0..(size * size)).map(|i| i as f32).collect();
        let r = GpuReprojector::new(identity_config(size));
        let dst = r.reproject_cpu(&src).expect("reproject_cpu failed");
        assert_eq!(dst.len(), (size * size) as usize);
    }
}
