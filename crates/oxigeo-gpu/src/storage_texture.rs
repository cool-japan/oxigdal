//! Storage-texture output kernel support for OxiGeo GPU.
//!
//! This module enables compute shaders to write their output directly to a
//! 2-D storage texture (WGSL `texture_storage_2d<format, write>`), bypassing
//! the round-trip to a CPU-visible staging buffer.  The resulting texture lives
//! on the GPU and can be consumed by subsequent render passes, image-processing
//! pipelines, or downloaded via [`read_texture_to_vec_f32`] when the host
//! needs the data.
//!
//! # Typical workflow
//!
//! 1. Create a storage texture with [`new_storage_texture`].
//! 2. Build a compute kernel from WGSL source with [`build_storage_texture_kernel`].
//! 3. Upload input data as a `GpuBuffer` using the standard buffer API.
//! 4. Execute the kernel with [`StorageTextureKernel::dispatch_to_texture`].
//! 5. Optionally download results with [`read_texture_to_vec_f32`].
//!
//! # Supported formats
//!
//! | [`wgpu::TextureFormat`]  | WGSL format string |
//! |--------------------------|--------------------|
//! | `Rgba32Float`            | `rgba32float`      |
//! | `Rgba8Unorm`             | `rgba8unorm`       |
//! | `R32Float`               | `r32float`         |
//!
//! Other formats are rejected by [`new_storage_texture`] with
//! [`GpuError::UnsupportedFormat`].

use crate::context::GpuContext;
use crate::error::{GpuError, GpuResult};
use crate::pipeline_cache::PipelineCacheKey;
use std::sync::Arc;
use tracing::{debug, trace};

// ─────────────────────────────────────────────────────────────────────────────
// Public types
// ─────────────────────────────────────────────────────────────────────────────

/// A 2-D GPU texture created with `STORAGE_BINDING | COPY_SRC` usage.
///
/// Created by [`new_storage_texture`] and consumed by
/// [`StorageTextureKernel::dispatch_to_texture`].  The texture lives entirely
/// on the GPU; use [`read_texture_to_vec_f32`] to copy data back to the host.
pub struct StorageTextureBinding {
    /// The underlying wgpu texture.
    pub texture: wgpu::Texture,
    /// Default view over the entire texture (all mip-levels, all layers).
    pub view: wgpu::TextureView,
    /// Pixel format of this texture.
    pub format: wgpu::TextureFormat,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// A compiled compute kernel that writes its output to a storage texture.
///
/// Constructed by [`build_storage_texture_kernel`].  A single kernel can be
/// dispatched to multiple different textures of compatible format.
pub struct StorageTextureKernel {
    pipeline: Arc<wgpu::ComputePipeline>,
    bind_group_layout: wgpu::BindGroupLayout,
}

// ─────────────────────────────────────────────────────────────────────────────
// Format helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Returns the WGSL storage-texture format string for a given
/// [`wgpu::TextureFormat`], or `None` if the format is not supported as a
/// storage texture by this crate.
fn wgsl_storage_format_str(format: wgpu::TextureFormat) -> Option<&'static str> {
    match format {
        wgpu::TextureFormat::Rgba32Float => Some("rgba32float"),
        wgpu::TextureFormat::Rgba8Unorm => Some("rgba8unorm"),
        wgpu::TextureFormat::R32Float => Some("r32float"),
        _ => None,
    }
}

/// Returns `true` if `format` is accepted by [`new_storage_texture`].
pub fn is_supported_storage_format(format: wgpu::TextureFormat) -> bool {
    wgsl_storage_format_str(format).is_some()
}

/// Number of bytes per texel for the supported storage formats.
///
/// Returns `None` for unsupported formats.
fn bytes_per_texel(format: wgpu::TextureFormat) -> Option<u64> {
    match format {
        wgpu::TextureFormat::Rgba32Float => Some(16), // 4 × f32
        wgpu::TextureFormat::Rgba8Unorm => Some(4),   // 4 × u8
        wgpu::TextureFormat::R32Float => Some(4),     // 1 × f32
        _ => None,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Shader source generation
// ─────────────────────────────────────────────────────────────────────────────

/// Generate default WGSL source for a kernel that copies a flat `f32` input
/// buffer into a 2-D storage texture.
///
/// The generated shader:
/// - Reads a flat `array<f32>` from binding 0 (storage, read).
/// - Writes to a `texture_storage_2d<{fmt}, write>` at binding 1.
/// - Dispatches 16×16 workgroups; out-of-bounds invocations are discarded.
///
/// For `Rgba32Float` and `Rgba8Unorm` the input scalar `val` is replicated to
/// all four channels; for `R32Float` only the first channel is written.
///
/// This is the **default** shader used by [`build_storage_texture_kernel`]
/// when no custom WGSL source is provided.  Callers may supply their own WGSL
/// to [`build_storage_texture_kernel`] for arbitrary kernel logic.
pub fn make_storage_texture_shader_source(format: wgpu::TextureFormat) -> String {
    let fmt_str = wgsl_storage_format_str(format).unwrap_or("rgba32float"); // safe fallback — caller should use supported formats

    // `textureStore` for `rgba8unorm` and `rgba32float` takes `vec4<f32>`;
    // for `r32float` it also takes `vec4<f32>` (padded) per WGSL spec.
    format!(
        r#"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var output: texture_storage_2d<{fmt}, write>;

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {{
    let dims = textureDimensions(output);
    if gid.x >= dims.x || gid.y >= dims.y {{ return; }}
    let idx = gid.y * dims.x + gid.x;
    let val = input[idx];
    textureStore(output, vec2<i32>(i32(gid.x), i32(gid.y)), vec4<f32>(val, val, val, 1.0));
}}
"#,
        fmt = fmt_str
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// StorageTextureBinding constructor
// ─────────────────────────────────────────────────────────────────────────────

/// Create a new 2-D storage texture on the GPU.
///
/// The texture is allocated with `STORAGE_BINDING | COPY_SRC` usage so it can
/// be written by a compute shader and subsequently copied to a staging buffer
/// for host readback.
///
/// # Errors
///
/// Returns [`GpuError::UnsupportedFormat`] when `format` is not one of the
/// formats supported as storage textures:
/// `Rgba32Float`, `Rgba8Unorm`, or `R32Float`.
///
/// # Example
///
/// ```rust,no_run
/// use oxigeo_gpu::{GpuContext, storage_texture::new_storage_texture};
///
/// # async fn ex() -> oxigeo_gpu::GpuResult<()> {
/// let ctx = GpuContext::new().await?;
/// let tex = new_storage_texture(&ctx, 512, 512, wgpu::TextureFormat::Rgba32Float)?;
/// assert_eq!(tex.width, 512);
/// assert_eq!(tex.height, 512);
/// # Ok(())
/// # }
/// ```
pub fn new_storage_texture(
    ctx: &GpuContext,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> GpuResult<StorageTextureBinding> {
    if !is_supported_storage_format(format) {
        return Err(GpuError::UnsupportedFormat(format!("{format:?}")));
    }

    let texture = ctx.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("oxigeo_storage_texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });

    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    debug!(
        "Created storage texture {}×{} format={format:?}",
        width, height
    );

    Ok(StorageTextureBinding {
        texture,
        view,
        format,
        width,
        height,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Build the bind group layout
// ─────────────────────────────────────────────────────────────────────────────

/// Build the bind-group layout expected by storage-texture kernels.
///
/// Layout:
/// - **binding 0**: `storage` buffer (read-only) — the input data.
/// - **binding 1**: `texture_storage_2d` (write-only) — the output texture.
///
/// The format supplied must be the same format used for the actual texture.
fn make_bind_group_layout(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("storage_texture_bgl"),
        entries: &[
            // binding 0: input storage buffer (read)
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            // binding 1: storage texture (write)
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::StorageTexture {
                    access: wgpu::StorageTextureAccess::WriteOnly,
                    format,
                    view_dimension: wgpu::TextureViewDimension::D2,
                },
                count: None,
            },
        ],
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Build the compute kernel
// ─────────────────────────────────────────────────────────────────────────────

/// Compile a WGSL compute shader into a [`StorageTextureKernel`].
///
/// The shader is looked up in the context's [`crate::pipeline_cache::PipelineCache`]
/// by the FNV-1a hash of `wgsl_source`, `entry_point`, and the output texture
/// format.  On a cache hit the existing compiled pipeline is returned
/// immediately; on a miss the shader is compiled and cached for future calls.
///
/// # Parameters
///
/// - `ctx` — the active GPU context.
/// - `wgsl_source` — full WGSL compute shader text.  Use
///   [`make_storage_texture_shader_source`] to generate a default copy kernel.
/// - `entry_point` — the `@compute` function name, usually `"main"`.
/// - `output_format` — the format of the storage texture this kernel will
///   write to.  Must be one of the formats accepted by [`new_storage_texture`].
///
/// # Errors
///
/// Returns [`GpuError::UnsupportedFormat`] if `output_format` is unsupported,
/// or [`GpuError::PipelineCreation`] if shader compilation fails.
pub fn build_storage_texture_kernel(
    ctx: &GpuContext,
    wgsl_source: &str,
    entry_point: &str,
    output_format: wgpu::TextureFormat,
) -> GpuResult<StorageTextureKernel> {
    if !is_supported_storage_format(output_format) {
        return Err(GpuError::UnsupportedFormat(format!("{output_format:?}")));
    }

    let bind_group_layout = make_bind_group_layout(ctx.device(), output_format);

    let pipeline_layout = ctx
        .device()
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("storage_texture_pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

    // Key for the pipeline cache: hash(wgsl_source) + entry_point + format tag.
    let layout_tag = format!("r-tex:{output_format:?}");
    let cache_key = PipelineCacheKey::new(wgsl_source, entry_point, &layout_tag);

    // Try the shared pipeline cache first.
    let pipeline = {
        let cache_result = ctx
            .pipeline_cache()
            .lock()
            .map_err(|_| GpuError::internal("pipeline cache mutex poisoned"));

        match cache_result {
            Ok(mut guard) => {
                let source_snapshot = wgsl_source.to_owned();
                let device = ctx.device();
                let entry_owned = entry_point.to_owned();
                let layout_ref = &pipeline_layout;

                guard.get_or_insert_with(cache_key.clone(), || {
                    compile_compute_pipeline(device, &source_snapshot, &entry_owned, layout_ref)
                        .map_err(|e| GpuError::pipeline_creation(e))
                })?
            }
            Err(_) => {
                // Cache unavailable — compile directly without caching.
                tracing::warn!(
                    "Pipeline cache mutex poisoned; compiling storage_texture pipeline without cache"
                );
                Arc::new(
                    compile_compute_pipeline(
                        ctx.device(),
                        wgsl_source,
                        entry_point,
                        &pipeline_layout,
                    )
                    .map_err(GpuError::pipeline_creation)?,
                )
            }
        }
    };

    trace!("StorageTextureKernel built, cache_key={cache_key}");

    Ok(StorageTextureKernel {
        pipeline,
        bind_group_layout,
    })
}

/// Compile a `wgpu::ComputePipeline` from WGSL source.
///
/// Returns the raw pipeline (not wrapped in `Arc`) so the caller can decide
/// whether to cache it.
fn compile_compute_pipeline(
    device: &wgpu::Device,
    wgsl_source: &str,
    entry_point: &str,
    pipeline_layout: &wgpu::PipelineLayout,
) -> Result<wgpu::ComputePipeline, String> {
    let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("storage_texture_shader"),
        source: wgpu::ShaderSource::Wgsl(wgsl_source.into()),
    });

    Ok(
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("storage_texture_pipeline"),
            layout: Some(pipeline_layout),
            module: &shader_module,
            entry_point: Some(entry_point),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        }),
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// StorageTextureKernel methods
// ─────────────────────────────────────────────────────────────────────────────

impl StorageTextureKernel {
    /// Dispatch the compute kernel, writing output directly into `texture`.
    ///
    /// The input data must already be resident on the GPU as `input_buffer`.
    /// The kernel is dispatched with `ceil(width / 16) × ceil(height / 16) × 1`
    /// workgroups, matching the default 16×16 workgroup size in
    /// [`make_storage_texture_shader_source`].
    ///
    /// # Key benefit
    ///
    /// The results are **not** copied back to CPU memory.  This avoids the
    /// expensive PCIe transfer and is useful when the texture will be consumed
    /// by subsequent GPU operations (e.g., compositing, rendering).
    ///
    /// # Errors
    ///
    /// Returns an error if bind-group creation fails or the device is lost.
    pub fn dispatch_to_texture<T: bytemuck::Pod>(
        &self,
        ctx: &GpuContext,
        input_buffer: &crate::buffer::GpuBuffer<T>,
        texture: &StorageTextureBinding,
    ) -> GpuResult<()> {
        ctx.check_device_lost()?;

        let bind_group = ctx.device().create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("storage_texture_bind_group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: input_buffer.buffer().as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&texture.view),
                },
            ],
        });

        let wg_x = dispatch_count(texture.width, 16);
        let wg_y = dispatch_count(texture.height, 16);

        let mut encoder = ctx
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("storage_texture_dispatch_encoder"),
            });

        {
            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("storage_texture_compute_pass"),
                timestamp_writes: None,
            });
            compute_pass.set_pipeline(&self.pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);
            compute_pass.dispatch_workgroups(wg_x, wg_y, 1);
        }

        ctx.queue().submit(std::iter::once(encoder.finish()));

        trace!(
            "Dispatched storage_texture kernel {}×{} → {}×{} workgroups",
            texture.width, texture.height, wg_x, wg_y
        );

        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Texture readback
// ─────────────────────────────────────────────────────────────────────────────

/// Copy a storage texture to host memory, returning a `Vec<f32>`.
///
/// For `Rgba32Float` the returned vector contains `width × height × 4` values
/// (R, G, B, A interleaved).  For `Rgba8Unorm` and `R32Float` the byte layout
/// is first converted to `f32` accordingly:
///
/// | Format           | Conversion               | Vec length          |
/// |------------------|--------------------------|---------------------|
/// | `Rgba32Float`    | raw bytes cast to `f32`  | `w × h × 4`        |
/// | `Rgba8Unorm`     | u8 → f32 (÷ 255)        | `w × h × 4`        |
/// | `R32Float`       | raw bytes cast to `f32`  | `w × h`             |
///
/// # Errors
///
/// - [`GpuError::UnsupportedFormat`] — texture format is not supported.
/// - [`GpuError::BufferMapping`] — failed to map the staging buffer.
pub fn read_texture_to_vec_f32(
    ctx: &GpuContext,
    texture: &StorageTextureBinding,
) -> GpuResult<Vec<f32>> {
    ctx.check_device_lost()?;

    let bpp = bytes_per_texel(texture.format)
        .ok_or_else(|| GpuError::UnsupportedFormat(format!("{:?}", texture.format)))?;

    // wgpu requires the bytes-per-row to be a multiple of COPY_BYTES_PER_ROW_ALIGNMENT (256).
    let bytes_per_row_unaligned = (texture.width as u64) * bpp;
    let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as u64;
    let bytes_per_row_aligned = (bytes_per_row_unaligned + alignment - 1) / alignment * alignment;

    let staging_size = bytes_per_row_aligned * (texture.height as u64);

    let staging_buffer = ctx.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("storage_texture_staging"),
        size: staging_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    // Encode texture → buffer copy.
    let mut encoder = ctx
        .device()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("storage_texture_readback_encoder"),
        });

    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture.texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &staging_buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row_aligned as u32),
                rows_per_image: Some(texture.height),
            },
        },
        wgpu::Extent3d {
            width: texture.width,
            height: texture.height,
            depth_or_array_layers: 1,
        },
    );

    ctx.queue().submit(std::iter::once(encoder.finish()));

    // Map and read the staging buffer (blocking via pollster).
    let result = read_staging_buffer_blocking(ctx, &staging_buffer, staging_size)?;

    // Decode bytes → f32 depending on format.
    let floats = decode_texture_bytes(
        &result,
        texture.format,
        texture.width,
        texture.height,
        bytes_per_row_aligned,
    );

    Ok(floats)
}

/// Block until the staging buffer is mapped and return its bytes.
fn read_staging_buffer_blocking(
    ctx: &GpuContext,
    staging_buffer: &wgpu::Buffer,
    staging_size: u64,
) -> GpuResult<Vec<u8>> {
    use std::sync::{Arc as StdArc, Mutex as StdMutex};

    let result: StdArc<StdMutex<Option<Result<(), wgpu::BufferAsyncError>>>> =
        StdArc::new(StdMutex::new(None));
    let result_clone = StdArc::clone(&result);

    staging_buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |r| {
            let mut guard = result_clone.lock().unwrap_or_else(|e| e.into_inner());
            *guard = Some(r);
        });

    // Poll until the mapping is complete.
    let _poll_handle = ctx.spawn_poll_task();
    loop {
        {
            let guard = result.lock().unwrap_or_else(|e| e.into_inner());
            if guard.is_some() {
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    let map_result = result
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .ok_or_else(|| GpuError::buffer_mapping("mapping never completed"))?;

    map_result.map_err(|e| GpuError::buffer_mapping(format!("map_async failed: {e}")))?;

    let view = staging_buffer
        .slice(..)
        .get_mapped_range()
        .map_err(|e| GpuError::buffer_mapping(e.to_string()))?;
    let bytes = view[..staging_size as usize].to_vec();
    drop(view);
    staging_buffer.unmap();

    Ok(bytes)
}

/// Decode raw texture bytes into `f32` values according to the texture format.
fn decode_texture_bytes(
    raw: &[u8],
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
    bytes_per_row_aligned: u64,
) -> Vec<f32> {
    let bpp = bytes_per_texel(format).unwrap_or(4);
    let bytes_per_row_unaligned = (width as u64) * bpp;

    match format {
        wgpu::TextureFormat::Rgba32Float => {
            // 4 × f32 per texel; must strip padding rows.
            let mut out = Vec::with_capacity((width * height * 4) as usize);
            for row in 0..height as usize {
                let row_start = row * bytes_per_row_aligned as usize;
                let row_end = row_start + bytes_per_row_unaligned as usize;
                let row_bytes = &raw[row_start..row_end];
                // Cast &[u8] → &[f32] via bytemuck.
                let row_floats: &[f32] = bytemuck::cast_slice(row_bytes);
                out.extend_from_slice(row_floats);
            }
            out
        }
        wgpu::TextureFormat::Rgba8Unorm => {
            // 4 × u8 per texel; convert to f32 by dividing by 255.
            let mut out = Vec::with_capacity((width * height * 4) as usize);
            for row in 0..height as usize {
                let row_start = row * bytes_per_row_aligned as usize;
                let row_end = row_start + bytes_per_row_unaligned as usize;
                for &byte in &raw[row_start..row_end] {
                    out.push(byte as f32 / 255.0);
                }
            }
            out
        }
        wgpu::TextureFormat::R32Float => {
            // 1 × f32 per texel.
            let mut out = Vec::with_capacity((width * height) as usize);
            for row in 0..height as usize {
                let row_start = row * bytes_per_row_aligned as usize;
                let row_end = row_start + bytes_per_row_unaligned as usize;
                let row_floats: &[f32] = bytemuck::cast_slice(&raw[row_start..row_end]);
                out.extend_from_slice(row_floats);
            }
            out
        }
        // Unreachable: we validated the format at the top of read_texture_to_vec_f32.
        _ => vec![],
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Utility
// ─────────────────────────────────────────────────────────────────────────────

/// Ceiling division — equivalent to `(n + d - 1) / d` for u32.
#[inline]
fn dispatch_count(n: u32, d: u32) -> u32 {
    n.div_ceil(d)
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests (pure-Rust, no GPU required)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wgsl_format_str_rgba32float() {
        assert_eq!(
            wgsl_storage_format_str(wgpu::TextureFormat::Rgba32Float),
            Some("rgba32float")
        );
    }

    #[test]
    fn test_wgsl_format_str_rgba8unorm() {
        assert_eq!(
            wgsl_storage_format_str(wgpu::TextureFormat::Rgba8Unorm),
            Some("rgba8unorm")
        );
    }

    #[test]
    fn test_wgsl_format_str_r32float() {
        assert_eq!(
            wgsl_storage_format_str(wgpu::TextureFormat::R32Float),
            Some("r32float")
        );
    }

    #[test]
    fn test_wgsl_format_str_unsupported_returns_none() {
        assert_eq!(
            wgsl_storage_format_str(wgpu::TextureFormat::Depth32Float),
            None
        );
    }

    #[test]
    fn test_is_supported_storage_format_accepted() {
        assert!(is_supported_storage_format(
            wgpu::TextureFormat::Rgba32Float
        ));
        assert!(is_supported_storage_format(wgpu::TextureFormat::Rgba8Unorm));
        assert!(is_supported_storage_format(wgpu::TextureFormat::R32Float));
    }

    #[test]
    fn test_is_supported_storage_format_rejected() {
        assert!(!is_supported_storage_format(
            wgpu::TextureFormat::Depth32Float
        ));
        assert!(!is_supported_storage_format(
            wgpu::TextureFormat::Rgba16Float
        ));
    }

    #[test]
    fn test_dispatch_count_exact_multiple() {
        assert_eq!(dispatch_count(64, 16), 4);
    }

    #[test]
    fn test_dispatch_count_rounds_up() {
        assert_eq!(dispatch_count(65, 16), 5);
        assert_eq!(dispatch_count(1, 16), 1);
        assert_eq!(dispatch_count(16, 16), 1);
        assert_eq!(dispatch_count(17, 16), 2);
    }

    #[test]
    fn test_bytes_per_texel_rgba32float() {
        assert_eq!(bytes_per_texel(wgpu::TextureFormat::Rgba32Float), Some(16));
    }

    #[test]
    fn test_bytes_per_texel_rgba8unorm() {
        assert_eq!(bytes_per_texel(wgpu::TextureFormat::Rgba8Unorm), Some(4));
    }

    #[test]
    fn test_bytes_per_texel_r32float() {
        assert_eq!(bytes_per_texel(wgpu::TextureFormat::R32Float), Some(4));
    }

    #[test]
    fn test_bytes_per_texel_unsupported_returns_none() {
        assert_eq!(bytes_per_texel(wgpu::TextureFormat::Depth32Float), None);
    }
}
