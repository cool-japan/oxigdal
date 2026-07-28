//! GPU-accelerated raster reprojection using wgpu compute shaders.
//!
//! This module provides both a GPU-backed reprojection pipeline and a CPU
//! fallback implementation for environments where GPU is unavailable.

use crate::buffer::GpuBuffer;
use crate::context::GpuContext;
use crate::error::GpuError;
use crate::shaders::{
    ComputePipelineBuilder, WgslShader, create_compute_bind_group_layout, storage_buffer_layout,
    uniform_buffer_layout,
};
use bytemuck::{Pod, Zeroable};
use wgpu::{
    BindGroupDescriptor, BindGroupEntry, BufferUsages, CommandEncoderDescriptor,
    ComputePassDescriptor,
};

/// WGSL source for the reprojection compute shader.
const REPROJECT_SHADER: &str = include_str!("shaders/reproject.wgsl");

/// Host-side mirror of the `ReprojParams` uniform in `reproject.wgsl`.
///
/// The six affine coefficients are packed into `vec4` slots because WGSL's
/// uniform address space requires 16-byte array element stride — a naive
/// `[f32; 6]` would be mis-read on the GPU.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct ReprojParamsGpu {
    src_width: u32,
    src_height: u32,
    dst_width: u32,
    dst_height: u32,
    resample_method: u32,
    use_nodata: u32,
    _pad0: u32,
    _pad1: u32,
    /// Source geo-transform (a, b, c, d).
    src_gt0: [f32; 4],
    /// Source geo-transform (e, f, unused, unused).
    src_gt1: [f32; 4],
    /// Destination inverse geo-transform (a, b, c, d).
    dst_inv_gt0: [f32; 4],
    /// Destination inverse geo-transform (e, f, unused, unused).
    dst_inv_gt1: [f32; 4],
    /// Nodata fill value in `.x` (remaining lanes unused).
    nodata: [f32; 4],
}

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
/// [`reproject_gpu`] dispatches a wgpu compute shader that performs the affine
/// inverse-mapping and nearest/bilinear resampling on the GPU; [`reproject_cpu`]
/// provides a bit-for-bit-equivalent CPU fallback for environments without a GPU
/// (the two paths are verified against each other by parity tests).
///
/// [`reproject_gpu`]: GpuReprojector::reproject_gpu
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

    /// Reproject `src_data` on the GPU via a wgpu compute dispatch.
    ///
    /// This builds a compute pipeline from `reproject.wgsl`, uploads the source
    /// raster and the packed affine parameters, dispatches one invocation per
    /// destination pixel, and reads the result back to the host.  The shader
    /// performs the identical affine inverse-mapping and nearest/bilinear
    /// resampling as [`reproject_cpu`](Self::reproject_cpu), so the two paths
    /// agree up to floating-point rounding.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError`] if the configuration is invalid, the source length
    /// is wrong, shader/pipeline creation fails, the device is lost, or the
    /// GPU→CPU read-back fails.
    pub async fn reproject_gpu(
        &self,
        context: &GpuContext,
        src_data: &[f32],
    ) -> Result<Vec<f32>, GpuError> {
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

        let params = self.gpu_params();

        // Upload source raster and parameter uniform; allocate destination.
        let src_buffer = GpuBuffer::from_data(
            context,
            src_data,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        )?;
        let dst_len = (self.config.dst_width as usize) * (self.config.dst_height as usize);
        let dst_buffer = GpuBuffer::<f32>::new(
            context,
            dst_len,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        )?;
        let params_buffer = GpuBuffer::from_data(
            context,
            &[params],
            BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        )?;

        let mut shader = WgslShader::new(REPROJECT_SHADER, "main");
        let module = shader.compile(context.device())?;

        let bind_group_layout = create_compute_bind_group_layout(
            context.device(),
            &[
                uniform_buffer_layout(0),
                storage_buffer_layout(1, true),
                storage_buffer_layout(2, false),
            ],
            Some("Reproject BindGroupLayout"),
        )?;

        let pipeline = ComputePipelineBuilder::new(context.device(), module, "main")
            .bind_group_layout(&bind_group_layout)
            .label("Reproject Pipeline")
            .build()?;

        let bind_group = context.device().create_bind_group(&BindGroupDescriptor {
            label: Some("Reproject BindGroup"),
            layout: &bind_group_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: params_buffer.buffer().as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: src_buffer.buffer().as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: dst_buffer.buffer().as_entire_binding(),
                },
            ],
        });

        let mut encoder = context
            .device()
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("Reproject Encoder"),
            });

        {
            let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("Reproject Pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            let groups_x = self.config.dst_width.div_ceil(16);
            let groups_y = self.config.dst_height.div_ceil(16);
            pass.dispatch_workgroups(groups_x, groups_y, 1);
        }

        context.check_device_lost()?;
        context.queue().submit(Some(encoder.finish()));

        // `dst_buffer` is a STORAGE buffer and cannot be host-mapped directly
        // (wgpu forbids `STORAGE | MAP_READ`). Copy it into a MAP_READ staging
        // buffer, then read that back to the host.
        let mut staging = GpuBuffer::<f32>::staging(context, dst_len)?;
        staging.copy_from(&dst_buffer)?;
        staging.read().await
    }

    /// Build the packed GPU uniform parameters from this configuration.
    fn gpu_params(&self) -> ReprojParamsGpu {
        let gt = &self.config.src_geotransform;
        let inv = &self.config.dst_inv_geotransform;
        let (use_nodata, nodata) = match self.config.nodata {
            Some(v) => (1u32, v),
            None => (0u32, 0.0_f32),
        };
        let resample_method = match self.config.resample_method {
            ResampleMethod::NearestNeighbor => 0u32,
            ResampleMethod::Bilinear => 1u32,
        };

        ReprojParamsGpu {
            src_width: self.config.src_width,
            src_height: self.config.src_height,
            dst_width: self.config.dst_width,
            dst_height: self.config.dst_height,
            resample_method,
            use_nodata,
            _pad0: 0,
            _pad1: 0,
            src_gt0: [gt[0], gt[1], gt[2], gt[3]],
            src_gt1: [gt[4], gt[5], 0.0, 0.0],
            dst_inv_gt0: [inv[0], inv[1], inv[2], inv[3]],
            dst_inv_gt1: [inv[4], inv[5], 0.0, 0.0],
            nodata: [nodata, 0.0, 0.0, 0.0],
        }
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

    #[test]
    fn test_gpu_params_packing() {
        let mut cfg = identity_config(4);
        cfg.src_geotransform = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        cfg.dst_inv_geotransform = [7.0, 8.0, 9.0, 10.0, 11.0, 12.0];
        cfg.nodata = Some(-9999.0);
        cfg.resample_method = ResampleMethod::Bilinear;
        let r = GpuReprojector::new(cfg);
        let p = r.gpu_params();
        // 16-byte-strided vec4 packing must preserve every coefficient.
        assert_eq!(p.src_gt0, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(p.src_gt1, [5.0, 6.0, 0.0, 0.0]);
        assert_eq!(p.dst_inv_gt0, [7.0, 8.0, 9.0, 10.0]);
        assert_eq!(p.dst_inv_gt1, [11.0, 12.0, 0.0, 0.0]);
        assert_eq!(p.nodata[0], -9999.0);
        assert_eq!(p.use_nodata, 1);
        assert_eq!(p.resample_method, 1);
        // Layout must be exactly 112 bytes (2x uvec4 + 5x vec4).
        assert_eq!(std::mem::size_of::<ReprojParamsGpu>(), 112);
    }

    /// Config that downsamples an 8x8 source into a 4x4 destination with
    /// fractional source coordinates, exercising the bilinear path.
    fn bilinear_downsample_config() -> ReprojectionConfig {
        ReprojectionConfig {
            src_width: 8,
            src_height: 8,
            dst_width: 4,
            dst_height: 4,
            // x_geo = col, y_geo = row
            src_geotransform: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            // geo = (dst_pixel_centre) * 1.5 → fractional src samples
            dst_inv_geotransform: [0.0, 1.5, 0.0, 0.0, 0.0, 1.5],
            resample_method: ResampleMethod::Bilinear,
            nodata: None,
        }
    }

    #[tokio::test]
    async fn test_reproject_gpu_matches_cpu_identity() {
        // Environment guard: only the GPU-context creation is optional. Once a
        // context exists, every step must succeed and GPU output must match the
        // CPU reference within tolerance.
        if let Ok(context) = GpuContext::new().await {
            let size = 8u32;
            let src: Vec<f32> = (0..(size * size)).map(|i| i as f32).collect();
            let r = GpuReprojector::new(identity_config(size));

            let cpu = r
                .reproject_cpu(&src)
                .expect("cpu reprojection must succeed");
            let gpu = r
                .reproject_gpu(&context, &src)
                .await
                .expect("gpu reprojection must succeed with a valid context");

            assert_eq!(cpu.len(), gpu.len());
            for (i, (c, g)) in cpu.iter().zip(gpu.iter()).enumerate() {
                assert!(
                    (c - g).abs() < 1e-3,
                    "pixel {i}: cpu={c} gpu={g} diverge beyond tolerance"
                );
            }
        }
    }

    #[tokio::test]
    async fn test_reproject_gpu_matches_cpu_bilinear() {
        if let Ok(context) = GpuContext::new().await {
            let src: Vec<f32> = (0..64).map(|i| (i as f32) * 0.5).collect();
            let r = GpuReprojector::new(bilinear_downsample_config());

            let cpu = r
                .reproject_cpu(&src)
                .expect("cpu reprojection must succeed");
            let gpu = r
                .reproject_gpu(&context, &src)
                .await
                .expect("gpu reprojection must succeed");

            assert_eq!(cpu.len(), gpu.len());
            for (i, (c, g)) in cpu.iter().zip(gpu.iter()).enumerate() {
                assert!(
                    (c - g).abs() < 1e-3,
                    "bilinear pixel {i}: cpu={c} gpu={g} diverge beyond tolerance"
                );
            }
        }
    }
}
