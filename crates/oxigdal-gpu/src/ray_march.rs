//! Ray-marching WGSL shader for volumetric DEM rendering.
//!
//! This module implements GPU-accelerated ray-marching over a Digital Elevation
//! Model (DEM) raster to produce per-pixel shadow and depth information.
//! It also exposes a CPU reference implementation for testing and fallback.
//!
//! # Algorithm overview
//!
//! For each pixel `(px, py)` in the DEM, a ray is cast from the surface point
//! in the direction of the light source.  At each marching step the terrain
//! elevation sampled along the ray is compared to the current ray height.  If
//! the terrain is higher than the ray, the pixel is marked as shadowed and the
//! march stops early.  After all steps the pixel is marked as lit.
//!
//! # GPU dispatch
//!
//! The WGSL kernel dispatches 16 × 16 workgroups.  Each invocation handles one
//! output pixel.  The DEM, configuration uniform, and two output buffers
//! (shadow factor and normalised depth) are bound at group 0.
//!
//! # Pure-CPU helpers
//!
//! [`normalize3`], [`sample_dem_bilinear`], and [`ray_march_cpu`] are fully
//! self-contained CPU functions that mirror the GPU kernel behaviour.  They are
//! used by unit tests and can serve as a CPU fallback on headless systems.

use std::sync::Arc;

use crate::context::GpuContext;
use crate::error::{GpuError, GpuResult};
use crate::shaders::{
    ComputePipelineBuilder, WgslShader, create_compute_bind_group_layout, storage_buffer_layout,
    uniform_buffer_layout,
};

use wgpu::{
    BindGroupDescriptor, BindGroupEntry, BufferDescriptor, BufferUsages, CommandEncoderDescriptor,
    ComputePassDescriptor,
};

// ─────────────────────────────────────────────────────────────────────────────
// RayMarchConfig
// ─────────────────────────────────────────────────────────────────────────────

/// Configuration parameters for the DEM ray-marching kernel.
///
/// All vectors use a right-handed coordinate system where X runs east, Y runs
/// north, and Z runs upward.
#[derive(Debug, Clone)]
pub struct RayMarchConfig {
    /// Unit-vector describing the view direction (from eye toward scene).
    /// Defaults to `[0, 0, -1]` (top-down orthographic).
    pub view_dir: [f32; 3],

    /// Unit-vector pointing toward the light source.
    /// Defaults to the normalised `(1, 1, 1)` diagonal.
    pub light_dir: [f32; 3],

    /// World-space distance advanced per march step.
    /// Must be `> 0`.
    pub step_size: f32,

    /// Maximum number of steps before the ray is considered unoccluded.
    /// Must be `> 0`.
    pub max_steps: u32,

    /// Factor applied to DEM elevations before ray-height comparisons.
    /// Set to `> 1.0` to exaggerate terrain relief.
    pub vertical_exaggeration: f32,

    /// Real-world length of one DEM pixel (in the same units as elevations).
    /// Used to convert pixel-space step offsets to world-space distances.
    pub pixel_size_world: f32,
}

impl Default for RayMarchConfig {
    fn default() -> Self {
        let inv_sqrt3 = 1.0_f32 / 3.0_f32.sqrt();
        Self {
            view_dir: [0.0, 0.0, -1.0],
            light_dir: [inv_sqrt3, inv_sqrt3, inv_sqrt3],
            step_size: 0.5,
            max_steps: 512,
            vertical_exaggeration: 1.0,
            pixel_size_world: 1.0,
        }
    }
}

impl RayMarchConfig {
    /// Validate configuration parameters against the supplied raster dimensions.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError::InvalidKernelParams`] if:
    /// - `step_size <= 0.0`
    /// - `max_steps == 0`
    /// - `width == 0` or `height == 0`
    pub fn validate(&self, width: u32, height: u32) -> GpuResult<()> {
        if self.step_size <= 0.0 {
            return Err(GpuError::invalid_kernel_params(format!(
                "step_size must be > 0, got {}",
                self.step_size
            )));
        }
        if self.max_steps == 0 {
            return Err(GpuError::invalid_kernel_params("max_steps must be > 0"));
        }
        if width == 0 {
            return Err(GpuError::invalid_kernel_params("DEM width must be > 0"));
        }
        if height == 0 {
            return Err(GpuError::invalid_kernel_params("DEM height must be > 0"));
        }
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// RayMarchResult
// ─────────────────────────────────────────────────────────────────────────────

/// Output buffers produced by a DEM ray-march pass.
pub struct RayMarchResult {
    /// Width of the output raster in pixels.
    pub width: u32,
    /// Height of the output raster in pixels.
    pub height: u32,

    /// Per-pixel shadow factor stored in row-major order.
    ///
    /// `0.0` means the pixel is fully in shadow; `1.0` means the pixel is lit.
    pub shaded: Vec<f32>,

    /// Per-pixel normalised march depth stored in row-major order.
    ///
    /// `0.0` means no marching was performed (pixel is in shadow at step 0) or
    /// the pixel was not marched.  `1.0` means the full `max_steps` were walked
    /// without finding occlusion (pixel is lit after exhausting all steps).
    pub depth: Vec<f32>,
}

// ─────────────────────────────────────────────────────────────────────────────
// GPU uniform layout (must match WGSL `RayMarchUniforms`)
// ─────────────────────────────────────────────────────────────────────────────

/// WGSL-layout-compatible uniform struct for the ray-march kernel.
///
/// # WGSL memory layout notes
///
/// WGSL host-shareable uniform layout (spec §13.4.3) only inserts padding
/// after a `vec3<f32>` when the following member itself requires alignment
/// of 16 or greater (e.g. another `vec3<f32>` or a `mat`).  When the next
/// member is a scalar like `f32`, it packs directly into the trailing 4
/// bytes of the `vec3`'s 16-byte slot.  Hence `step_size` follows
/// `light_dir` at offset 28, not 32.
///
/// | Offset | Size | Field                |
/// |--------|------|----------------------|
/// |  0     |  12  | view_dir             |
/// | 12     |   4  | _pad0 (align next vec3 to 16) |
/// | 16     |  12  | light_dir            |
/// | 28     |   4  | step_size            |
/// | 32     |   4  | max_steps            |
/// | 36     |   4  | vertical_exaggeration|
/// | 40     |   4  | pixel_size_world     |
/// | 44     |   4  | width                |
/// | 48     |   4  | height               |
/// | 52     |  12  | _pad_end (align struct size to 16) |
/// Total = 64 bytes (multiple of 16).
#[repr(C, align(16))]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct RayMarchUniforms {
    view_dir: [f32; 3],
    _pad0: f32,
    light_dir: [f32; 3],
    step_size: f32,
    max_steps: u32,
    vertical_exaggeration: f32,
    pixel_size_world: f32,
    width: u32,
    height: u32,
    _pad_end: [u32; 3],
}

impl RayMarchUniforms {
    fn from_config(cfg: &RayMarchConfig, width: u32, height: u32) -> Self {
        Self {
            view_dir: cfg.view_dir,
            _pad0: 0.0,
            light_dir: cfg.light_dir,
            step_size: cfg.step_size,
            max_steps: cfg.max_steps,
            vertical_exaggeration: cfg.vertical_exaggeration,
            pixel_size_world: cfg.pixel_size_world,
            width,
            height,
            _pad_end: [0, 0, 0],
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// DemRayMarcher
// ─────────────────────────────────────────────────────────────────────────────

/// GPU-accelerated DEM ray-marching kernel.
///
/// Construct once with [`DemRayMarcher::new`] and call [`DemRayMarcher::march`]
/// for each DEM raster you wish to process.
pub struct DemRayMarcher {
    ctx: Arc<GpuContext>,
    pipeline: Arc<wgpu::ComputePipeline>,
    bind_group_layout: wgpu::BindGroupLayout,
}

impl DemRayMarcher {
    /// Compile the ray-march compute shader and create the GPU pipeline.
    ///
    /// # Errors
    ///
    /// Returns a shader compilation or pipeline creation error when the WGSL
    /// source cannot be compiled on the current device.
    pub fn new(ctx: Arc<GpuContext>) -> GpuResult<Self> {
        let shader_src = make_ray_march_shader_source();
        let mut shader = WgslShader::new(shader_src, "main");
        let shader_module = shader.compile(ctx.device())?;

        // Bind group layout:
        //   binding 0 — DEM storage buffer (read-only)
        //   binding 1 — config uniform buffer
        //   binding 2 — shaded output buffer (read-write)
        //   binding 3 — depth  output buffer (read-write)
        let bind_group_layout = create_compute_bind_group_layout(
            ctx.device(),
            &[
                storage_buffer_layout(0, true),  // dem
                uniform_buffer_layout(1),        // config
                storage_buffer_layout(2, false), // shaded
                storage_buffer_layout(3, false), // depth_buf
            ],
            Some("DemRayMarcher Bind Group Layout"),
        )?;

        let pipeline = ComputePipelineBuilder::new(ctx.device(), shader_module, "main")
            .bind_group_layout(&bind_group_layout)
            .label("DemRayMarcher Pipeline")
            .build()?;

        Ok(Self {
            ctx,
            pipeline: Arc::new(pipeline),
            bind_group_layout,
        })
    }

    /// Execute the ray-march kernel over the supplied DEM raster.
    ///
    /// # Arguments
    ///
    /// * `dem`    — flat row-major f32 elevations, length must equal `width * height`.
    /// * `width`  — number of columns in the raster.
    /// * `height` — number of rows in the raster.
    /// * `config` — ray-marching parameters (validated before dispatch).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `config.validate()` fails.
    /// - `dem.len() != width * height`.
    /// - Any GPU operation (buffer creation, dispatch, readback) fails.
    pub fn march(
        &self,
        dem: &[f32],
        width: u32,
        height: u32,
        config: &RayMarchConfig,
    ) -> GpuResult<RayMarchResult> {
        config.validate(width, height)?;

        let n_pixels = (width as usize) * (height as usize);
        if dem.len() != n_pixels {
            return Err(GpuError::invalid_kernel_params(format!(
                "DEM length {} does not match width*height = {}",
                dem.len(),
                n_pixels
            )));
        }

        let device = self.ctx.device();
        let queue = self.ctx.queue();

        // ---- Build GPU buffers ----

        let f32_size = std::mem::size_of::<f32>() as u64;
        let dem_byte_size = (n_pixels as u64) * f32_size;
        let out_byte_size = (n_pixels as u64) * f32_size;

        // Align buffer sizes to COPY_BUFFER_ALIGNMENT (256 bytes).
        let align = wgpu::COPY_BUFFER_ALIGNMENT;
        let dem_aligned = ((dem_byte_size + align - 1) / align) * align;
        let out_aligned = ((out_byte_size + align - 1) / align) * align;

        // DEM input buffer
        let dem_buf = device.create_buffer(&BufferDescriptor {
            label: Some("DemRayMarcher dem_buf"),
            size: dem_aligned,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&dem_buf, 0, bytemuck::cast_slice(dem));

        // Config uniform buffer (64 bytes, aligned to 16)
        let uniforms = RayMarchUniforms::from_config(config, width, height);
        let uniform_bytes: &[u8] = bytemuck::bytes_of(&uniforms);
        let uniform_size = std::mem::size_of::<RayMarchUniforms>() as u64;
        // Uniform buffers must satisfy min-binding-size and COPY_BUFFER_ALIGNMENT
        let uniform_aligned = ((uniform_size + align - 1) / align) * align;
        let uniform_buf = device.create_buffer(&BufferDescriptor {
            label: Some("DemRayMarcher uniform_buf"),
            size: uniform_aligned,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&uniform_buf, 0, uniform_bytes);

        // Shaded output buffer
        let shaded_buf = device.create_buffer(&BufferDescriptor {
            label: Some("DemRayMarcher shaded_buf"),
            size: out_aligned,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        // Depth output buffer
        let depth_buf = device.create_buffer(&BufferDescriptor {
            label: Some("DemRayMarcher depth_buf"),
            size: out_aligned,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        // Staging buffers for CPU readback
        let shaded_staging = device.create_buffer(&BufferDescriptor {
            label: Some("DemRayMarcher shaded_staging"),
            size: out_aligned,
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let depth_staging = device.create_buffer(&BufferDescriptor {
            label: Some("DemRayMarcher depth_staging"),
            size: out_aligned,
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // ---- Bind group ----

        let bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("DemRayMarcher BindGroup"),
            layout: &self.bind_group_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: dem_buf.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: uniform_buf.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: shaded_buf.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: depth_buf.as_entire_binding(),
                },
            ],
        });

        // ---- Encode and submit ----

        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("DemRayMarcher encoder"),
        });

        {
            let mut cpass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("DemRayMarcher compute pass"),
                timestamp_writes: None,
            });
            cpass.set_pipeline(&self.pipeline);
            cpass.set_bind_group(0, &bind_group, &[]);

            // Dispatch 16×16 workgroups to cover width × height pixels
            let wg_x = (width + 15) / 16;
            let wg_y = (height + 15) / 16;
            cpass.dispatch_workgroups(wg_x, wg_y, 1);
        }

        encoder.copy_buffer_to_buffer(&shaded_buf, 0, &shaded_staging, 0, out_aligned);
        encoder.copy_buffer_to_buffer(&depth_buf, 0, &depth_staging, 0, out_aligned);

        queue.submit(Some(encoder.finish()));

        // Map before poll: poll drives both submit completion and map callbacks.
        let shaded_slice = shaded_staging.slice(..);
        let (shaded_tx, shaded_rx) = std::sync::mpsc::sync_channel(1);
        shaded_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = shaded_tx.send(result);
        });

        let depth_slice = depth_staging.slice(..);
        let (depth_tx, depth_rx) = std::sync::mpsc::sync_channel(1);
        depth_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = depth_tx.send(result);
        });

        device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| GpuError::execution_failed(format!("device poll failed: {e}")))?;

        let shaded_vec =
            finish_staging_f32_read(&shaded_staging, shaded_slice, shaded_rx, n_pixels)?;
        let depth_vec = finish_staging_f32_read(&depth_staging, depth_slice, depth_rx, n_pixels)?;

        Ok(RayMarchResult {
            width,
            height,
            shaded: shaded_vec,
            depth: depth_vec,
        })
    }
}

/// Complete a staging-buffer read whose `map_async` has already been
/// scheduled and whose mapping callback has been driven by a preceding
/// `device.poll(...)`.  This function only does `recv` + read + `unmap`.
fn finish_staging_f32_read(
    staging: &wgpu::Buffer,
    slice: wgpu::BufferSlice<'_>,
    rx: std::sync::mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
    count: usize,
) -> GpuResult<Vec<f32>> {
    rx.recv()
        .map_err(|_| GpuError::buffer_mapping("staging channel closed unexpectedly"))?
        .map_err(|e| GpuError::buffer_mapping(e.to_string()))?;

    let mapped = slice
        .get_mapped_range()
        .map_err(|e| GpuError::buffer_mapping(e.to_string()))?;
    let floats: &[f32] = bytemuck::cast_slice(&mapped[..count * 4]);
    let out = floats.to_vec();
    drop(mapped);
    staging.unmap();
    Ok(out)
}

// ─────────────────────────────────────────────────────────────────────────────
// Pure-CPU helper: normalize3
// ─────────────────────────────────────────────────────────────────────────────

/// Normalise a 3-element vector to unit length.
///
/// Returns `[0.0, 0.0, 0.0]` when the input magnitude is less than `1e-10`.
///
/// # Examples
///
/// ```
/// use oxigdal_gpu::normalize3;
///
/// let n = normalize3([3.0, 0.0, 0.0]);
/// assert!((n[0] - 1.0).abs() < 1e-6);
/// ```
pub fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let mag = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if mag < 1e-10 {
        [0.0, 0.0, 0.0]
    } else {
        [v[0] / mag, v[1] / mag, v[2] / mag]
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Pure-CPU helper: sample_dem_bilinear
// ─────────────────────────────────────────────────────────────────────────────

/// Sample a DEM raster at a sub-pixel position using bilinear interpolation.
///
/// `fx` and `fy` are pixel-space coordinates with `(0.0, 0.0)` at the
/// top-left corner of the first pixel.  Values outside the raster extent are
/// clamped to the nearest valid pixel.
///
/// # Panics
///
/// Never panics; all index computations are bounds-checked.
///
/// # Examples
///
/// ```
/// use oxigdal_gpu::sample_dem_bilinear;
///
/// // 2×2 raster: [1, 2; 3, 4]
/// let dem = [1.0_f32, 2.0, 3.0, 4.0];
/// assert_eq!(sample_dem_bilinear(&dem, 2, 2, 0.0, 0.0), 1.0);
/// assert_eq!(sample_dem_bilinear(&dem, 2, 2, 1.0, 1.0), 4.0);
/// ```
pub fn sample_dem_bilinear(dem: &[f32], width: u32, height: u32, fx: f32, fy: f32) -> f32 {
    let w = width as f32;
    let h = height as f32;

    // Clamp sample coordinates to the valid pixel-space range.
    let cx = fx.clamp(0.0, w - 1.0);
    let cy = fy.clamp(0.0, h - 1.0);

    let x0 = cx.floor() as u32;
    let y0 = cy.floor() as u32;

    // Clamp index bounds to avoid out-of-bounds on the upper edge.
    let x1 = (x0 + 1).min(width - 1);
    let y1 = (y0 + 1).min(height - 1);

    let tx = cx - x0 as f32; // fractional part in [0, 1]
    let ty = cy - y0 as f32;

    let w_u = width as usize;
    let v00 = dem[y0 as usize * w_u + x0 as usize];
    let v10 = dem[y0 as usize * w_u + x1 as usize];
    let v01 = dem[y1 as usize * w_u + x0 as usize];
    let v11 = dem[y1 as usize * w_u + x1 as usize];

    // Bilinear interpolation: lerp in X first, then in Y.
    let top = v00 + (v10 - v00) * tx;
    let bot = v01 + (v11 - v01) * tx;
    top + (bot - top) * ty
}

// ─────────────────────────────────────────────────────────────────────────────
// Pure-CPU reference: ray_march_cpu
// ─────────────────────────────────────────────────────────────────────────────

/// CPU reference implementation of the DEM ray-march algorithm.
///
/// For each pixel `(px, py)` a ray originates at the surface point
/// `(px, py, dem[py*width+px] * vert_exag)` and advances in the light
/// direction by `step_size` per step.  If the terrain elevation sampled along
/// the ray (bilinearly) exceeds the current ray height, the pixel is shadowed
/// and the march stops early.  Otherwise the pixel is lit and receives a
/// normalised depth of `1.0`.
///
/// This function never panics and never requires GPU access.
pub fn ray_march_cpu(
    dem: &[f32],
    width: u32,
    height: u32,
    config: &RayMarchConfig,
) -> RayMarchResult {
    let n_pixels = (width as usize) * (height as usize);
    let mut shaded = vec![1.0_f32; n_pixels];
    let mut depth = vec![1.0_f32; n_pixels];

    let ld = config.light_dir;
    let step = config.step_size;
    let vert = config.vertical_exaggeration;
    let max_s = config.max_steps;

    for py in 0..height {
        for px in 0..width {
            let idx = (py as usize) * (width as usize) + (px as usize);

            let start_x = px as f32;
            let start_y = py as f32;
            let start_z = dem[idx] * vert;

            let mut shadowed = false;
            let mut last_step = max_s; // used to compute depth

            for s in 1..=max_s {
                let sf = s as f32;
                let sample_x = start_x + ld[0] * step * sf;
                let sample_y = start_y + ld[1] * step * sf;
                let terrain_z = sample_dem_bilinear(dem, width, height, sample_x, sample_y) * vert;
                let ray_z = start_z + ld[2] * step * sf;

                if terrain_z > ray_z {
                    shadowed = true;
                    last_step = s;
                    break;
                }
            }

            if shadowed {
                shaded[idx] = 0.0;
                // Normalised depth: fraction of steps taken before occlusion.
                depth[idx] = (last_step as f32) / (max_s as f32);
            } else {
                shaded[idx] = 1.0;
                depth[idx] = 1.0;
            }
        }
    }

    RayMarchResult {
        width,
        height,
        shaded,
        depth,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// WGSL shader source generation
// ─────────────────────────────────────────────────────────────────────────────

/// Return the WGSL compute shader source for GPU DEM ray-marching.
///
/// The emitted shader:
/// - Binds the DEM at `@group(0) @binding(0)` as a read-only storage buffer.
/// - Binds the config struct at `@binding(1)` as a uniform buffer.
/// - Binds the shadow-factor output at `@binding(2)` as a read-write storage buffer.
/// - Binds the depth output at `@binding(3)` as a read-write storage buffer.
/// - Uses `@workgroup_size(16, 16, 1)`.
/// - Uses a `for` loop with early `break` instead of recursion (WGSL has no recursion).
///
/// The WGSL config struct layout matches `RayMarchUniforms` exactly.
pub fn make_ray_march_shader_source() -> String {
    r#"
// ── Ray-march DEM shadow kernel ─────────────────────────────────────────────
//
// Bindings
//   0  dem         : array<f32>  (read-only storage) — row-major DEM elevations
//   1  config      : RayMarchUniforms (uniform)
//   2  shaded      : array<f32>  (read-write storage) — 0=shadow, 1=lit
//   3  depth_buf   : array<f32>  (read-write storage) — normalised march depth
//
// Workgroup: 16 × 16 × 1

struct RayMarchUniforms {
    view_dir              : vec3<f32>,
    // _pad0 (4 bytes) implicitly added by vec3 alignment
    light_dir             : vec3<f32>,
    // step_size follows light_dir at offset 28 (no extra pad — WGSL only pads
    // vec3 when the *next* member is itself align-≥-16, e.g. another vec3).
    step_size             : f32,
    max_steps             : u32,
    vertical_exaggeration : f32,
    pixel_size_world      : f32,
    width                 : u32,
    height                : u32,
    // _pad_end (12 bytes) — aligns total struct size to 16
}

@group(0) @binding(0) var<storage, read>       dem       : array<f32>;
@group(0) @binding(1) var<uniform>              config    : RayMarchUniforms;
@group(0) @binding(2) var<storage, read_write>  shaded    : array<f32>;
@group(0) @binding(3) var<storage, read_write>  depth_buf : array<f32>;

// ── Bilinear sample of the DEM at fractional pixel coordinates ───────────────
fn sample_dem(fx: f32, fy: f32) -> f32 {
    let w  = f32(config.width);
    let h  = f32(config.height);
    let cx = clamp(fx, 0.0, w - 1.0);
    let cy = clamp(fy, 0.0, h - 1.0);

    let x0 = u32(cx);
    let y0 = u32(cy);
    let x1 = min(x0 + 1u, config.width  - 1u);
    let y1 = min(y0 + 1u, config.height - 1u);

    let tx = cx - f32(x0);
    let ty = cy - f32(y0);

    let v00 = dem[y0 * config.width + x0];
    let v10 = dem[y0 * config.width + x1];
    let v01 = dem[y1 * config.width + x0];
    let v11 = dem[y1 * config.width + x1];

    let top = v00 + (v10 - v00) * tx;
    let bot = v01 + (v11 - v01) * tx;
    return top + (bot - top) * ty;
}

@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let px = id.x;
    let py = id.y;

    // Discard threads that fall outside the raster bounds.
    if px >= config.width || py >= config.height {
        return;
    }

    let idx      = py * config.width + px;
    let start_x  = f32(px);
    let start_y  = f32(py);
    let start_z  = dem[idx] * config.vertical_exaggeration;

    let ld       = config.light_dir;
    let step     = config.step_size;
    let vert     = config.vertical_exaggeration;
    let max_s    = config.max_steps;

    var shadow_flag : f32 = 1.0;
    var depth_val   : f32 = 1.0;

    for (var s: u32 = 1u; s <= max_s; s = s + 1u) {
        let sf       = f32(s);
        let sample_x = start_x + ld.x * step * sf;
        let sample_y = start_y + ld.y * step * sf;
        let terrain_z = sample_dem(sample_x, sample_y) * vert;
        let ray_z     = start_z + ld.z * step * sf;

        if terrain_z > ray_z {
            shadow_flag = 0.0;
            depth_val   = f32(s) / f32(max_s);
            break;
        }
    }

    shaded[idx]    = shadow_flag;
    depth_buf[idx] = depth_val;
}
"#
    .to_string()
}
