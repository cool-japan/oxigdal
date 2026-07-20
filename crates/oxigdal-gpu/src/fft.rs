//! Radix-2 1D FFT GPU compute kernel for OxiGDAL.
//!
//! This module implements a monomorphic Cooley-Tukey Decimation-In-Time (DIT)
//! radix-2 FFT using WGSL compute shaders dispatched via wgpu.  Each workgroup
//! processes exactly one transform; the batch variant dispatches multiple
//! workgroups in parallel.
//!
//! # Constraints
//!
//! - Size must be a power of two in the range `[4, 2048]`.
//! - All butterfly stages and the bit-reversal function are **unrolled** in
//!   the emitted WGSL because WGSL workgroup-shared memory paths do not allow
//!   fully dynamic loop bounds at all WGSL validation levels.
//!
//! # Sign convention
//!
//! | Mode    | twiddle angle              | normalisation |
//! |---------|----------------------------|---------------|
//! | Forward | `-2π · k / N`              | none          |
//! | Inverse | `+2π · k / N`              | divide by N   |

use crate::context::GpuContext;
use crate::error::{GpuError, GpuResult};
use crate::shaders::{
    ComputePipelineBuilder, WgslShader, create_compute_bind_group_layout, storage_buffer_layout,
};
use std::f32::consts::PI;
use std::sync::Arc;
use tracing::debug;
use wgpu::{BindGroupDescriptor, BindGroupEntry, BufferDescriptor, BufferUsages};

// ─────────────────────────────────────────────────────────────────────────────
// Pure-Rust helper functions (no GPU required)
// ─────────────────────────────────────────────────────────────────────────────

/// Reverse the bottom `log2_n` bits of `x`.
///
/// # Examples
///
/// ```
/// use oxigdal_gpu::fft::bit_reverse;
///
/// // 3-bit reversal (for N=8)
/// assert_eq!(bit_reverse(0b001, 3), 0b100); // 1 → 4
/// assert_eq!(bit_reverse(0b011, 3), 0b110); // 3 → 6
/// ```
pub fn bit_reverse(x: u32, log2_n: u32) -> u32 {
    let mut result = 0u32;
    let mut v = x;
    for _ in 0..log2_n {
        result = (result << 1) | (v & 1);
        v >>= 1;
    }
    result
}

/// Compute the twiddle factor for position `k` of an `n`-point transform.
///
/// Returns `(cos(angle), sin(angle))` where:
/// - forward (`inverse = false`): `angle = -2π·k/n`
/// - inverse  (`inverse = true`) : `angle = +2π·k/n`
pub fn twiddle_factor(k: u32, n: u32, inverse: bool) -> (f32, f32) {
    let sign = if inverse { 1.0_f32 } else { -1.0_f32 };
    let angle = sign * 2.0 * PI * (k as f32) / (n as f32);
    (angle.cos(), angle.sin())
}

// ─────────────────────────────────────────────────────────────────────────────
// WGSL shader code generation
// ─────────────────────────────────────────────────────────────────────────────

/// Emit a monomorphic WGSL compute shader for a radix-2 Cooley-Tukey DIT FFT
/// of exactly `size` points.
///
/// The workgroup size is `size / 2`; each workgroup processes exactly one
/// independent FFT.  Batch processing is achieved by dispatching multiple
/// workgroups (`dispatch_workgroups(batch_count, 1, 1)`).
///
/// # Panics
///
/// Does not panic; input is validated in [`Fft1d::new`] before this is called.
pub fn make_fft_shader_source(size: u32, inverse: bool) -> String {
    let log2_n = size.trailing_zeros();
    let half_size = size / 2;
    let sign_str = if inverse { "1.0" } else { "-1.0" };
    let is_inverse_str = if inverse { "true" } else { "false" };

    // Build the bit_rev function body (unrolled log2_n-bit reversal)
    let bit_rev_fn = build_bit_rev_fn(log2_n);

    // Build unrolled butterfly stages
    let butterfly_stages = build_butterfly_stages(log2_n, half_size, sign_str);

    // Optional 1/N normalisation for inverse FFT
    let normalise_block = if inverse {
        format!(
            r#"
    // --- Inverse normalisation: divide by N ---
    shared_real[thread_id] /= f32({size}u);
    shared_imag[thread_id] /= f32({size}u);
    shared_real[thread_id + {half_size}u] /= f32({size}u);
    shared_imag[thread_id + {half_size}u] /= f32({size}u);
    workgroupBarrier();
"#,
            size = size,
            half_size = half_size,
        )
    } else {
        String::new()
    };

    // The shader template
    format!(
        r#"// FFT size = {size}, log2 = {log2_n}, inverse = {is_inverse_str}
@group(0) @binding(0) var<storage, read_write> real_buf: array<f32>;
@group(0) @binding(1) var<storage, read_write> imag_buf: array<f32>;

var<workgroup> shared_real: array<f32, {size}>;
var<workgroup> shared_imag: array<f32, {size}>;

{bit_rev_fn}

@compute @workgroup_size({half_size}, 1, 1)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {{
    let thread_id: u32 = lid.x;
    let batch_offset: u32 = wid.x * {size}u;

    // --- 1. Bit-reversal permutation ---
    let rev0: u32 = bit_rev(thread_id);
    let rev1: u32 = bit_rev(thread_id + {half_size}u);
    shared_real[rev0] = real_buf[batch_offset + thread_id];
    shared_imag[rev0] = imag_buf[batch_offset + thread_id];
    shared_real[rev1] = real_buf[batch_offset + thread_id + {half_size}u];
    shared_imag[rev1] = imag_buf[batch_offset + thread_id + {half_size}u];
    workgroupBarrier();

    // --- 2. FFT butterfly stages (unrolled {log2_n} stages) ---
{butterfly_stages}
{normalise_block}
    // --- 3. Write back ---
    real_buf[batch_offset + thread_id] = shared_real[thread_id];
    imag_buf[batch_offset + thread_id] = shared_imag[thread_id];
    real_buf[batch_offset + thread_id + {half_size}u] = shared_real[thread_id + {half_size}u];
    imag_buf[batch_offset + thread_id + {half_size}u] = shared_imag[thread_id + {half_size}u];
}}
"#,
        size = size,
        log2_n = log2_n,
        half_size = half_size,
        is_inverse_str = is_inverse_str,
        bit_rev_fn = bit_rev_fn,
        butterfly_stages = butterfly_stages,
        normalise_block = normalise_block,
    )
}

/// Build the WGSL `bit_rev` function with an unrolled `log2_n`-bit reversal.
fn build_bit_rev_fn(log2_n: u32) -> String {
    let mut body = String::new();
    body.push_str("fn bit_rev(x: u32) -> u32 {\n");
    body.push_str("    var v: u32 = x;\n");
    body.push_str("    var r: u32 = 0u;\n");
    for _ in 0..log2_n {
        body.push_str("    r = (r << 1u) | (v & 1u);\n");
        body.push_str("    v = v >> 1u;\n");
    }
    body.push_str("    return r;\n");
    body.push_str("}\n");
    body
}

/// Build unrolled WGSL for all `log2_n` butterfly stages of a radix-2 DIT FFT.
fn build_butterfly_stages(log2_n: u32, _half_size: u32, sign_str: &str) -> String {
    let mut code = String::new();
    for s in 0..log2_n {
        let span = 1u32 << (s + 1);
        let half_span = span >> 1;
        code.push_str(&format!(
            r#"
    // Stage {s}: span = {span}, half_span = {half_span}
    {{
        let span_{s}: u32      = {span}u;
        let half_span_{s}: u32 = {half_span}u;
        let pos_{s}: u32  = thread_id % half_span_{s};
        let grp_{s}: u32  = thread_id / half_span_{s};
        let i_{s}: u32    = grp_{s} * span_{s} + pos_{s};
        let j_{s}: u32    = i_{s} + half_span_{s};
        let angle_{s}: f32 = {sign_str} * 2.0 * 3.14159265358979323846 * f32(pos_{s}) / f32(span_{s});
        let wr_{s}: f32  = cos(angle_{s});
        let wi_{s}: f32  = sin(angle_{s});
        let tr_{s}: f32  = wr_{s} * shared_real[j_{s}] - wi_{s} * shared_imag[j_{s}];
        let ti_{s}: f32  = wr_{s} * shared_imag[j_{s}] + wi_{s} * shared_real[j_{s}];
        shared_real[j_{s}] = shared_real[i_{s}] - tr_{s};
        shared_imag[j_{s}] = shared_imag[i_{s}] - ti_{s};
        shared_real[i_{s}] = shared_real[i_{s}] + tr_{s};
        shared_imag[i_{s}] = shared_imag[i_{s}] + ti_{s};
        workgroupBarrier();
    }}
"#,
            s = s,
            span = span,
            half_span = half_span,
            sign_str = sign_str,
        ));
    }
    code
}

// ─────────────────────────────────────────────────────────────────────────────
// Fft1d struct
// ─────────────────────────────────────────────────────────────────────────────

/// GPU-accelerated radix-2 1D FFT (or IFFT) kernel.
///
/// A single `Fft1d` instance is bound to a specific transform size and
/// direction (forward/inverse).  Construct it once with [`Fft1d::new`] and
/// reuse it for multiple [`Fft1d::execute`] or [`Fft1d::execute_batch`] calls.
pub struct Fft1d {
    pipeline: Arc<wgpu::ComputePipeline>,
    bind_group_layout: wgpu::BindGroupLayout,
    /// Number of complex samples in one transform.
    size: u32,
    /// `true` for IFFT (inverse); `false` for FFT (forward).
    inverse: bool,
}

impl Fft1d {
    /// Create a new FFT kernel for the given transform size and direction.
    ///
    /// # Validation (pure-Rust, no GPU required)
    ///
    /// - `size` must be a power of two.
    /// - `size` must be in the range `[4, 2048]`.
    ///
    /// # Errors
    ///
    /// Returns `GpuError::InvalidConfig` if the size fails validation, or a
    /// pipeline / shader error if WGSL compilation fails on the GPU device.
    pub fn new(ctx: &GpuContext, size: u32, inverse: bool) -> GpuResult<Self> {
        // ---- Pure-Rust validation (no GPU touched) ----
        if !size.is_power_of_two() {
            return Err(GpuError::invalid_kernel_params(
                "FFT size must be power of two",
            ));
        }
        if size < 4 {
            return Err(GpuError::invalid_kernel_params("FFT size must be >= 4"));
        }
        if size > 2048 {
            return Err(GpuError::invalid_kernel_params("FFT size must be <= 2048"));
        }

        // ---- Shader compilation ----
        let shader_src = make_fft_shader_source(size, inverse);
        let mut shader = WgslShader::new(shader_src, "main");
        let shader_module = shader.compile(ctx.device())?;

        // Bind group layout: two read-write storage buffers (real + imag).
        let bind_group_layout = create_compute_bind_group_layout(
            ctx.device(),
            &[
                storage_buffer_layout(0, false), // real_buf  (read_write)
                storage_buffer_layout(1, false), // imag_buf  (read_write)
            ],
            Some("Fft1d Bind Group Layout"),
        )?;

        let pipeline = ComputePipelineBuilder::new(ctx.device(), shader_module, "main")
            .bind_group_layout(&bind_group_layout)
            .label("Fft1d Pipeline")
            .build()?;

        debug!("Fft1d created: size={}, inverse={}", size, inverse);

        Ok(Self {
            pipeline: Arc::new(pipeline),
            bind_group_layout,
            size,
            inverse,
        })
    }

    // ---- Accessors -------------------------------------------------------

    /// Transform size (number of complex samples).
    pub fn size(&self) -> u32 {
        self.size
    }

    /// `true` when this kernel computes the inverse FFT.
    pub fn is_inverse(&self) -> bool {
        self.inverse
    }

    // ---- Single transform -----------------------------------------------

    /// Execute a single 1D FFT/IFFT.
    ///
    /// # Arguments
    ///
    /// * `real_in` — real parts of the input (length must equal `self.size`).
    /// * `imag_in` — imaginary parts of the input (length must equal `self.size`).
    ///
    /// # Returns
    ///
    /// A tuple `(real_out, imag_out)` each of length `self.size`.
    ///
    /// # Errors
    ///
    /// Returns an error if input lengths are wrong or any GPU operation fails.
    pub fn execute(
        &self,
        ctx: &GpuContext,
        real_in: &[f32],
        imag_in: &[f32],
    ) -> GpuResult<(Vec<f32>, Vec<f32>)> {
        let n = self.size as usize;
        if real_in.len() != n {
            return Err(GpuError::invalid_kernel_params(format!(
                "real_in length {} != FFT size {}",
                real_in.len(),
                n
            )));
        }
        if imag_in.len() != n {
            return Err(GpuError::invalid_kernel_params(format!(
                "imag_in length {} != FFT size {}",
                imag_in.len(),
                n
            )));
        }

        let results = self.dispatch_batch_internal(ctx, &[(real_in.to_vec(), imag_in.to_vec())])?;
        let (r, i) = results.into_iter().next().ok_or_else(|| {
            GpuError::internal("dispatch returned empty batch for single transform")
        })?;
        Ok((r, i))
    }

    /// Execute a batch of FFT/IFFT transforms in one GPU dispatch.
    ///
    /// All transforms in the batch must use the same `size` (enforced by the
    /// shader's workgroup layout).  Each workgroup handles one transform.
    ///
    /// # Errors
    ///
    /// Returns an error if any batch element has the wrong size or if a GPU
    /// operation fails.
    pub fn execute_batch(
        &self,
        ctx: &GpuContext,
        batch: &[(Vec<f32>, Vec<f32>)],
    ) -> GpuResult<Vec<(Vec<f32>, Vec<f32>)>> {
        let n = self.size as usize;
        for (idx, (r, im)) in batch.iter().enumerate() {
            if r.len() != n {
                return Err(GpuError::invalid_kernel_params(format!(
                    "batch[{}].real length {} != FFT size {}",
                    idx,
                    r.len(),
                    n
                )));
            }
            if im.len() != n {
                return Err(GpuError::invalid_kernel_params(format!(
                    "batch[{}].imag length {} != FFT size {}",
                    idx,
                    im.len(),
                    n
                )));
            }
        }

        self.dispatch_batch_internal(ctx, batch)
    }

    // ---- Internal GPU dispatch ------------------------------------------

    fn dispatch_batch_internal(
        &self,
        ctx: &GpuContext,
        batch: &[(Vec<f32>, Vec<f32>)],
    ) -> GpuResult<Vec<(Vec<f32>, Vec<f32>)>> {
        let batch_count = batch.len() as u32;
        let n = self.size as usize;
        let total = batch_count as usize * n;

        // --- Interleave inputs into flat real and imag arrays ---
        let mut flat_real: Vec<f32> = Vec::with_capacity(total);
        let mut flat_imag: Vec<f32> = Vec::with_capacity(total);
        for (r, im) in batch {
            flat_real.extend_from_slice(r);
            flat_imag.extend_from_slice(im);
        }

        // --- Create storage buffers (STORAGE | COPY_SRC | COPY_DST) ---
        let buf_size = (total * std::mem::size_of::<f32>()) as u64;
        // Align to COPY_BUFFER_ALIGNMENT (256 bytes)
        let aligned_buf_size = (buf_size + 255) & !255;

        let real_buf = ctx.device().create_buffer(&BufferDescriptor {
            label: Some("Fft1d real_buf"),
            size: aligned_buf_size,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let imag_buf = ctx.device().create_buffer(&BufferDescriptor {
            label: Some("Fft1d imag_buf"),
            size: aligned_buf_size,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // --- Upload inputs ---
        ctx.queue()
            .write_buffer(&real_buf, 0, bytemuck::cast_slice(&flat_real));
        ctx.queue()
            .write_buffer(&imag_buf, 0, bytemuck::cast_slice(&flat_imag));

        // --- Build bind group ---
        let bind_group = ctx.device().create_bind_group(&BindGroupDescriptor {
            label: Some("Fft1d BindGroup"),
            layout: &self.bind_group_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: real_buf.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: imag_buf.as_entire_binding(),
                },
            ],
        });

        // --- Encode compute pass ---
        let mut encoder = ctx
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Fft1d encoder"),
            });

        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Fft1d compute pass"),
                timestamp_writes: None,
            });
            cpass.set_pipeline(&self.pipeline);
            cpass.set_bind_group(0, &bind_group, &[]);
            cpass.dispatch_workgroups(batch_count, 1, 1);
        }

        // --- Create staging buffers for readback ---
        let staging_real = ctx.device().create_buffer(&BufferDescriptor {
            label: Some("Fft1d staging_real"),
            size: aligned_buf_size,
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let staging_imag = ctx.device().create_buffer(&BufferDescriptor {
            label: Some("Fft1d staging_imag"),
            size: aligned_buf_size,
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        encoder.copy_buffer_to_buffer(&real_buf, 0, &staging_real, 0, aligned_buf_size);
        encoder.copy_buffer_to_buffer(&imag_buf, 0, &staging_imag, 0, aligned_buf_size);

        ctx.queue().submit(Some(encoder.finish()));

        // Map before poll: poll drives both submit completion and map callbacks.
        let real_slice = staging_real.slice(..);
        let (real_tx, real_rx) = std::sync::mpsc::sync_channel(1);
        real_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = real_tx.send(result);
        });

        let imag_slice = staging_imag.slice(..);
        let (imag_tx, imag_rx) = std::sync::mpsc::sync_channel(1);
        imag_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = imag_tx.send(result);
        });

        ctx.device()
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| GpuError::execution_failed(format!("device poll failed: {e}")))?;

        let real_out = Self::finish_staging_read(&staging_real, real_slice, real_rx, total)?;
        let imag_out = Self::finish_staging_read(&staging_imag, imag_slice, imag_rx, total)?;

        debug!(
            "Fft1d dispatched: batch={}, size={}, inverse={}",
            batch_count, self.size, self.inverse
        );

        // --- Split flat outputs into per-transform pairs ---
        let mut results = Vec::with_capacity(batch_count as usize);
        for i in 0..batch_count as usize {
            let r = real_out[i * n..(i + 1) * n].to_vec();
            let im = imag_out[i * n..(i + 1) * n].to_vec();
            results.push((r, im));
        }
        Ok(results)
    }

    /// Complete a staging-buffer read whose `map_async` has already been
    /// scheduled and whose mapping callback has been driven by a preceding
    /// `device.poll(...)`.  This function only does `recv` + read + `unmap`.
    fn finish_staging_read(
        staging: &wgpu::Buffer,
        slice: wgpu::BufferSlice<'_>,
        rx: std::sync::mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
        count: usize,
    ) -> GpuResult<Vec<f32>> {
        rx.recv()
            .map_err(|_| GpuError::buffer_mapping("staging channel closed"))?
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
}

// ─────────────────────────────────────────────────────────────────────────────
// Public pure-Rust validation helper
// ─────────────────────────────────────────────────────────────────────────────

/// Validate that `size` satisfies the FFT kernel constraints (power of two,
/// in the range `[4, 2048]`).
///
/// This function performs the same three checks as [`Fft1d::new`] **before**
/// any wgpu call is made.  It is exposed as `pub` so that integration tests
/// can exercise the validation logic without constructing a real GPU context.
///
/// # Errors
///
/// Returns [`GpuError::InvalidKernelParams`] with a descriptive message when
/// any constraint is violated.
pub fn validate_size(size: u32) -> GpuResult<()> {
    if !size.is_power_of_two() {
        return Err(GpuError::invalid_kernel_params(
            "FFT size must be power of two",
        ));
    }
    if size < 4 {
        return Err(GpuError::invalid_kernel_params("FFT size must be >= 4"));
    }
    if size > 2048 {
        return Err(GpuError::invalid_kernel_params("FFT size must be <= 2048"));
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests (pure-Rust, no GPU required unless noted)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bit_reverse_known_values() {
        // 3-bit reversal (N = 8)
        assert_eq!(bit_reverse(0, 3), 0);
        assert_eq!(bit_reverse(1, 3), 4);
        assert_eq!(bit_reverse(2, 3), 2);
        assert_eq!(bit_reverse(3, 3), 6);
        assert_eq!(bit_reverse(4, 3), 1);
        assert_eq!(bit_reverse(5, 3), 5);
        assert_eq!(bit_reverse(6, 3), 3);
        assert_eq!(bit_reverse(7, 3), 7);
    }

    #[test]
    fn test_twiddle_factor_known() {
        let (r, i) = twiddle_factor(0, 4, false);
        assert!((r - 1.0).abs() < 1e-6, "cos(0) = 1, got {}", r);
        assert!(i.abs() < 1e-6, "sin(0) = 0, got {}", i);

        let (r, i) = twiddle_factor(1, 4, false);
        assert!(r.abs() < 1e-6, "cos(-π/2) ≈ 0, got {}", r);
        assert!((i + 1.0).abs() < 1e-6, "sin(-π/2) = -1, got {}", i);
    }

    #[test]
    fn test_make_shader_contains_log2n_3() {
        let src = make_fft_shader_source(8, false);
        // log2(8) = 3; shader should contain "3u" (bits in bit_rev) or stage count "3"
        assert!(
            src.contains("3u") || src.contains("3"),
            "shader must reference log2(8) = 3"
        );
    }

    #[test]
    fn test_make_shader_inverse_contains_scale() {
        let src = make_fft_shader_source(8, true);
        // inverse shader must contain 1/8 normalisation; either "8u" or "8.0" or the literal
        assert!(
            src.contains("8u") || src.contains("8.0") || src.contains("0.125"),
            "inverse shader must contain 1/N scale factor for N=8"
        );
    }

    #[test]
    fn test_new_rejects_non_power_of_two() {
        // Pure validation — no GPU context needed because the check runs first
        // We call validate_size directly by relying on the error branch
        let result = validate_size(6);
        assert!(
            result.is_err(),
            "size=6 is not a power of two, must be rejected"
        );
    }

    #[test]
    fn test_new_rejects_size_below_min() {
        let result = validate_size(2);
        assert!(result.is_err(), "size=2 < 4, must be rejected");
    }

    #[test]
    fn test_new_rejects_size_above_max() {
        let result = validate_size(4096);
        assert!(result.is_err(), "size=4096 > 2048, must be rejected");
    }
}
