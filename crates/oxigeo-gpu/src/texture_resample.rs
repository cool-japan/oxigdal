//! Texture-based resampling using wgpu hardware samplers.
//!
//! This module provides a `TextureResampler` that performs raster resampling
//! using a `wgpu::Sampler` and a `texture_2d<f32>` binding rather than a flat
//! `array<f32>` storage buffer.  The hardware sampler handles filtering in
//! a single texel-fetch instruction, which is dramatically faster than
//! evaluating a bilinear/bicubic interpolation in WGSL.
//!
//! # When to use this path
//!
//! | Resampling method        | Hardware-sampled?  | Recommendation                |
//! |--------------------------|--------------------|-------------------------------|
//! | `NearestNeighbor`        | yes (Nearest)      | use this module               |
//! | `Bilinear`               | yes (Linear)       | use this module               |
//! | `Bicubic`                | fallback (Linear)  | use this module if speed > Q  |
//! | `Lanczos { a }`          | no                 | use the compute-buffer path   |
//!
//! For high-quality bicubic and Lanczos resampling, prefer the compute-buffer
//! path in `kernels::resampling` which implements the full kernel mathematics.
//!
//! # Workflow
//!
//! 1. Upload source data as a sampleable `wgpu::Texture` with
//!    [`new_input_texture_r32float`].
//! 2. Construct a [`TextureResampler`] with the desired filter mode and
//!    destination format.
//! 3. Create a destination storage texture with
//!    [`crate::storage_texture::new_storage_texture`].
//! 4. Dispatch the resampler with [`TextureResampler::dispatch`].
//! 5. Optionally download the result with
//!    [`crate::storage_texture::read_texture_to_vec_f32`].

use crate::context::GpuContext;
use crate::error::{GpuError, GpuResult};
use crate::kernels::resampling::ResamplingMethod;
use crate::storage_texture::StorageTextureBinding;
use std::sync::Arc;
use tracing::{debug, trace};

// ─────────────────────────────────────────────────────────────────────────────
// Public filter mode enum
// ─────────────────────────────────────────────────────────────────────────────

/// Hardware-sampler filtering mode for [`TextureResampler`].
///
/// Maps directly to [`wgpu::FilterMode`]; both the magnification and
/// minification filters are configured identically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureFilterMethod {
    /// Nearest-neighbour sampling — returns the texel closest to the sample
    /// coordinate.  Produces blocky output but is exact for integer scale
    /// factors.
    Nearest,
    /// Bilinear filtering — returns a weighted average of the four texels
    /// closest to the sample coordinate.  This is the primary use case for the
    /// hardware sampler path.
    Linear,
}

impl TextureFilterMethod {
    /// Returns the wgpu equivalent of this filter mode.
    ///
    /// The same value is used for both magnification and minification filters
    /// in [`TextureResampler::new`].
    pub fn wgpu_filter(&self) -> wgpu::FilterMode {
        match self {
            Self::Nearest => wgpu::FilterMode::Nearest,
            Self::Linear => wgpu::FilterMode::Linear,
        }
    }
}

/// Maps a [`ResamplingMethod`] to the hardware-sampler filter that best
/// approximates it.
///
/// - `NearestNeighbor` → `Some(Nearest)`
/// - `Bilinear`        → `Some(Linear)`
/// - `Bicubic`         → `Some(Linear)`  (lossy fallback)
/// - `Lanczos { .. }`  → `None`          (use the compute-buffer path)
///
/// Returns `None` to signal that the caller must dispatch the compute-buffer
/// resampler in [`crate::kernels::resampling`] instead.
pub fn texture_filter_for_resampling(method: ResamplingMethod) -> Option<TextureFilterMethod> {
    match method {
        ResamplingMethod::NearestNeighbor => Some(TextureFilterMethod::Nearest),
        ResamplingMethod::Bilinear => Some(TextureFilterMethod::Linear),
        // Bicubic has no hardware sampler — fall back to bilinear for the
        // texture path; callers wanting true bicubic should use the compute
        // buffer kernel.
        ResamplingMethod::Bicubic => Some(TextureFilterMethod::Linear),
        // Lanczos is intentionally not supported by the texture path; the
        // mathematical kernel requires a full convolution loop.
        ResamplingMethod::Lanczos { .. } => None,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// WGSL shader source
// ─────────────────────────────────────────────────────────────────────────────

/// WGSL format string for a destination storage texture format.
///
/// Returns the default `rgba32float` if the format is not recognised so the
/// shader text is always well-formed.  Caller-side validation in
/// [`TextureResampler::new`] rejects unsupported formats earlier.
fn dst_format_wgsl(fmt: wgpu::TextureFormat) -> &'static str {
    match fmt {
        wgpu::TextureFormat::Rgba32Float => "rgba32float",
        wgpu::TextureFormat::R32Float => "r32float",
        wgpu::TextureFormat::Rgba8Unorm => "rgba8unorm",
        _ => "rgba32float",
    }
}

/// Returns `true` if `format` is one of the destination formats supported by
/// the texture-resample shader.
fn is_supported_dst_format(format: wgpu::TextureFormat) -> bool {
    matches!(
        format,
        wgpu::TextureFormat::Rgba32Float
            | wgpu::TextureFormat::R32Float
            | wgpu::TextureFormat::Rgba8Unorm,
    )
}

/// Generate WGSL source for a texture-based resampling compute shader.
///
/// The shader binds:
/// - `binding 0`: a sampleable `texture_2d<f32>` (the source raster).
/// - `binding 1`: a `sampler` configured with the requested filter mode.
/// - `binding 2`: a write-only `texture_storage_2d<{dst_fmt}, write>` (the
///   destination raster).
///
/// Each invocation maps its destination pixel `(gid.x, gid.y)` to a normalised
/// `uv` coordinate at the pixel centre, samples the source texture with
/// `textureSampleLevel(..., 0.0)`, and writes the result to the storage
/// texture.
///
/// The `filter` parameter is **not** baked into the shader text — the wgpu
/// `Sampler` binding determines the actual filtering behaviour.  The parameter
/// is accepted here for documentation purposes and to enable future shader
/// specialisations.
pub fn make_texture_resample_shader_source(
    filter: TextureFilterMethod,
    dst_fmt: wgpu::TextureFormat,
) -> String {
    // Suppress unused-variable warning while keeping the parameter as a
    // public-API hook for future specialisations (e.g., manual filtering
    // in WGSL when hardware filtering is not available for a given format).
    let _ = filter;

    let dst_fmt_str = dst_format_wgsl(dst_fmt);

    format!(
        r#"
@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_samp: sampler;
@group(0) @binding(2) var dst_tex: texture_storage_2d<{dst_fmt_str}, write>;

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {{
    let dst_dims = textureDimensions(dst_tex);
    if (gid.x >= dst_dims.x || gid.y >= dst_dims.y) {{
        return;
    }}

    // Sample at the destination pixel centre: (i+0.5)/N maps the discrete
    // texel index to the [0, 1] normalised UV coordinate used by the sampler.
    let uv = vec2<f32>(
        (f32(gid.x) + 0.5) / f32(dst_dims.x),
        (f32(gid.y) + 0.5) / f32(dst_dims.y),
    );

    let val = textureSampleLevel(src_tex, src_samp, uv, 0.0);
    textureStore(dst_tex, vec2<i32>(i32(gid.x), i32(gid.y)), val);
}}
"#,
        dst_fmt_str = dst_fmt_str
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// TextureResampler
// ─────────────────────────────────────────────────────────────────────────────

/// A compiled compute kernel that resamples a sampleable source texture into a
/// destination storage texture using a hardware sampler.
///
/// Constructed once per `(method, dst_format)` pair via [`TextureResampler::new`]
/// and reused across many dispatches.
pub struct TextureResampler {
    pipeline: Arc<wgpu::ComputePipeline>,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    method: TextureFilterMethod,
    dst_format: wgpu::TextureFormat,
}

impl TextureResampler {
    /// Construct a new texture resampler.
    ///
    /// # Parameters
    ///
    /// - `ctx` — the GPU context that owns the device and queue.
    /// - `method` — the filter mode (Nearest or Linear) applied by the sampler.
    /// - `dst_format` — the storage texture format the kernel will write to.
    ///   Must be one of `Rgba32Float`, `R32Float`, or `Rgba8Unorm`.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError::UnsupportedFormat`] when `dst_format` is not one of
    /// the supported destination formats.
    pub fn new(
        ctx: &GpuContext,
        method: TextureFilterMethod,
        dst_format: wgpu::TextureFormat,
    ) -> GpuResult<Self> {
        if !is_supported_dst_format(dst_format) {
            return Err(GpuError::UnsupportedFormat(format!(
                "texture_resample dst_format {dst_format:?} not supported (expected Rgba32Float, R32Float, or Rgba8Unorm)"
            )));
        }

        let sampler = ctx.device().create_sampler(&wgpu::SamplerDescriptor {
            label: Some("texture_resample_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: method.wgpu_filter(),
            min_filter: method.wgpu_filter(),
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let bind_group_layout =
            ctx.device()
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("texture_resample_bgl"),
                    entries: &[
                        // binding 0: sampleable source texture
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::COMPUTE,
                            ty: wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: None,
                        },
                        // binding 1: filtering sampler
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::COMPUTE,
                            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                            count: None,
                        },
                        // binding 2: destination storage texture (write-only)
                        wgpu::BindGroupLayoutEntry {
                            binding: 2,
                            visibility: wgpu::ShaderStages::COMPUTE,
                            ty: wgpu::BindingType::StorageTexture {
                                access: wgpu::StorageTextureAccess::WriteOnly,
                                format: dst_format,
                                view_dimension: wgpu::TextureViewDimension::D2,
                            },
                            count: None,
                        },
                    ],
                });

        let shader_src = make_texture_resample_shader_source(method, dst_format);
        let module = ctx
            .device()
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("texture_resample_shader"),
                source: wgpu::ShaderSource::Wgsl(shader_src.into()),
            });

        let pipeline_layout =
            ctx.device()
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("texture_resample_pl"),
                    bind_group_layouts: &[Some(&bind_group_layout)],
                    immediate_size: 0,
                });

        let pipeline = ctx
            .device()
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("texture_resample_pipeline"),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some("main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            });

        debug!(
            "Created TextureResampler method={:?} dst_format={:?}",
            method, dst_format
        );

        Ok(Self {
            pipeline: Arc::new(pipeline),
            bind_group_layout,
            sampler,
            method,
            dst_format,
        })
    }

    /// The filter mode this resampler was constructed with.
    pub fn method(&self) -> TextureFilterMethod {
        self.method
    }

    /// The destination storage-texture format this resampler writes to.
    pub fn dst_format(&self) -> wgpu::TextureFormat {
        self.dst_format
    }

    /// Dispatch the compute kernel, reading from `src_texture` and writing to
    /// `dst_texture`.
    ///
    /// # Errors
    ///
    /// - Returns [`GpuError::UnsupportedFormat`] when the destination texture
    ///   format does not match the resampler's `dst_format`.
    /// - Returns the underlying error if the device has been lost.
    pub fn dispatch(
        &self,
        ctx: &GpuContext,
        src_texture: &wgpu::Texture,
        dst_texture: &StorageTextureBinding,
    ) -> GpuResult<()> {
        ctx.check_device_lost()?;

        if dst_texture.format != self.dst_format {
            return Err(GpuError::UnsupportedFormat(format!(
                "destination texture format {:?} does not match resampler dst_format {:?}",
                dst_texture.format, self.dst_format
            )));
        }

        let src_view = src_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let bind_group = ctx.device().create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("texture_resample_bind_group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&src_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&dst_texture.view),
                },
            ],
        });

        let wg_x = dst_texture.width.div_ceil(16);
        let wg_y = dst_texture.height.div_ceil(16);

        let mut encoder = ctx
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("texture_resample_encoder"),
            });

        {
            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("texture_resample_pass"),
                timestamp_writes: None,
            });
            compute_pass.set_pipeline(&self.pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);
            compute_pass.dispatch_workgroups(wg_x, wg_y, 1);
        }

        ctx.queue().submit(std::iter::once(encoder.finish()));

        trace!(
            "Dispatched texture_resample {}×{} → {}×{} workgroups",
            dst_texture.width, dst_texture.height, wg_x, wg_y
        );

        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Input-texture upload helper
// ─────────────────────────────────────────────────────────────────────────────

/// Create a sampleable R32Float `wgpu::Texture` and upload `data` into it.
///
/// The texture is allocated with `TEXTURE_BINDING | COPY_DST` usage so it can
/// be bound as a `texture_2d<f32>` in a compute shader and have its contents
/// updated via `queue.write_texture`.
///
/// # Errors
///
/// Returns [`GpuError::ExecutionFailed`] when `data.len()` does not equal
/// `width × height`.
pub fn new_input_texture_r32float(
    ctx: &GpuContext,
    width: u32,
    height: u32,
    data: &[f32],
) -> GpuResult<wgpu::Texture> {
    let expected = (width as usize) * (height as usize);
    if data.len() != expected {
        return Err(GpuError::execution_failed(format!(
            "new_input_texture_r32float: data length {} != width*height {expected}",
            data.len()
        )));
    }

    let texture = ctx.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("texture_resample_input"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    let bytes: &[u8] = bytemuck::cast_slice(data);
    ctx.queue().write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );

    Ok(texture)
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests (pure-Rust, no GPU required)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_texture_filter_method_wgpu_filter_nearest() {
        assert_eq!(
            TextureFilterMethod::Nearest.wgpu_filter(),
            wgpu::FilterMode::Nearest
        );
    }

    #[test]
    fn test_texture_filter_method_wgpu_filter_linear() {
        assert_eq!(
            TextureFilterMethod::Linear.wgpu_filter(),
            wgpu::FilterMode::Linear
        );
    }

    #[test]
    fn test_dst_format_wgsl_rgba32float() {
        assert_eq!(
            dst_format_wgsl(wgpu::TextureFormat::Rgba32Float),
            "rgba32float"
        );
    }

    #[test]
    fn test_dst_format_wgsl_r32float() {
        assert_eq!(dst_format_wgsl(wgpu::TextureFormat::R32Float), "r32float");
    }

    #[test]
    fn test_dst_format_wgsl_rgba8unorm() {
        assert_eq!(
            dst_format_wgsl(wgpu::TextureFormat::Rgba8Unorm),
            "rgba8unorm"
        );
    }

    #[test]
    fn test_dst_format_wgsl_unsupported_defaults_to_rgba32float() {
        assert_eq!(
            dst_format_wgsl(wgpu::TextureFormat::Depth32Float),
            "rgba32float"
        );
    }

    #[test]
    fn test_is_supported_dst_format_accepted() {
        assert!(is_supported_dst_format(wgpu::TextureFormat::Rgba32Float));
        assert!(is_supported_dst_format(wgpu::TextureFormat::R32Float));
        assert!(is_supported_dst_format(wgpu::TextureFormat::Rgba8Unorm));
    }

    #[test]
    fn test_is_supported_dst_format_rejected() {
        assert!(!is_supported_dst_format(wgpu::TextureFormat::Depth32Float));
        assert!(!is_supported_dst_format(wgpu::TextureFormat::Rgba16Float));
    }
}
