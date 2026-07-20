//! WGSL push constants (immediates) helper for `oxigdal-gpu`.
//!
//! In wgpu 29 the Vulkan "push constants" concept is exposed as **immediate
//! data** (`var<immediate>` in WGSL, [`wgpu::Features::IMMEDIATES`] on the
//! device, and [`wgpu::PipelineLayoutDescriptor::immediate_size`]).  This
//! module provides ergonomic Rust wrappers around that API and remains
//! forward-compatible with the naming used in earlier wgpu versions.
//!
//! # Quick start
//!
//! ```rust,no_run
//! use oxigdal_gpu::push_constants::{
//!     PushConstantsLayout, PushConstantsBuffer, make_push_constants_shader_source,
//!     build_push_constants_pipeline,
//! };
//! use oxigdal_gpu::GpuContext;
//!
//! # async fn example() -> oxigdal_gpu::GpuResult<()> {
//! let ctx = GpuContext::new().await?;
//!
//! let layout = PushConstantsLayout::compute_only(16);
//! let mut buf = PushConstantsBuffer::new(layout.clone());
//! buf.write_u32(0, 42)?;
//! buf.write_f32(4, 3.14)?;
//!
//! let struct_wgsl = "struct PushConstantsBlock { value: u32, scale: f32, _pad0: u32, _pad1: u32, }";
//! let body_wgsl = "let v = pc.value;";
//! let src = make_push_constants_shader_source(struct_wgsl, body_wgsl);
//! # Ok(())
//! # }
//! ```
//!
//! # Compatibility note
//!
//! wgpu 29 renamed Vulkan push constants to "immediates".  The public API of
//! this module deliberately preserves the `PushConstants*` naming to keep the
//! call-site interface stable regardless of the underlying wgpu version.

use bytemuck::NoUninit;
use std::mem::size_of;
use std::sync::Arc;
use tracing::debug;

use crate::context::GpuContext;
use crate::error::{GpuError, GpuResult};

// ─────────────────────────────────────────────────────────────────────────────
// Constants
// ─────────────────────────────────────────────────────────────────────────────

/// Minimum guaranteed push-constants (immediates) size per the Vulkan spec
/// (128 bytes).  Adapters may expose a larger limit via
/// [`max_push_constants_size`].
pub const MAX_PUSH_CONSTANTS_SIZE_BYTES: u32 = 128;

/// Required alignment for the start and end of each push-constant range.
///
/// Both the `start` and `end` fields of [`PushConstantRange`] must be
/// multiples of this value.  This matches the Vulkan push-constant and wgpu
/// immediate-data alignment requirement.
pub const PUSH_CONSTANTS_ALIGNMENT: u32 = 4;

// ─────────────────────────────────────────────────────────────────────────────
// PushConstantRange
// ─────────────────────────────────────────────────────────────────────────────

/// Describes one logical push-constant range within a pipeline layout.
///
/// In wgpu 29 the underlying API does not support per-stage or per-range
/// configurations — the immediate-data block covers the whole shader and is
/// addressed by byte offset.  This struct deliberately mirrors the pre-wgpu-29
/// interface so that callers can model their data layout without change.
///
/// # Layout constraints
///
/// - `start < end`
/// - `end − start ≤ `[`MAX_PUSH_CONSTANTS_SIZE_BYTES`]
/// - Both `start` and `end` must be 4-byte aligned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushConstantRange {
    /// Shader stages this range is visible to.  Stored for documentation
    /// purposes; in wgpu 29 stage visibility is controlled at the bind-group
    /// layout level, not the pipeline layout level.
    pub stages: wgpu::ShaderStages,
    /// Byte offset of the first byte in this range (inclusive).
    pub start: u32,
    /// Byte offset one past the last byte in this range (exclusive).
    pub end: u32,
}

impl PushConstantRange {
    /// Create a compute-only range that starts at offset 0 with the given
    /// `size` in bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxigdal_gpu::push_constants::PushConstantRange;
    ///
    /// let r = PushConstantRange::compute(32);
    /// assert_eq!(r.start, 0);
    /// assert_eq!(r.end, 32);
    /// assert!(r.validate().is_ok());
    /// ```
    pub fn compute(size: u32) -> Self {
        Self {
            stages: wgpu::ShaderStages::COMPUTE,
            start: 0,
            end: size,
        }
    }

    /// Validate that this range satisfies all layout constraints.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError::InvalidKernelParams`] when:
    /// - `end <= start` (zero or negative size).
    /// - The range size exceeds [`MAX_PUSH_CONSTANTS_SIZE_BYTES`].
    /// - `start` or `end` is not 4-byte aligned.
    pub fn validate(&self) -> GpuResult<()> {
        if self.end <= self.start {
            return Err(GpuError::invalid_kernel_params(format!(
                "push-constant range start ({}) must be less than end ({})",
                self.start, self.end
            )));
        }

        let size = self.end - self.start;
        if size > MAX_PUSH_CONSTANTS_SIZE_BYTES {
            return Err(GpuError::invalid_kernel_params(format!(
                "push-constant range size {} bytes exceeds the maximum of {} bytes",
                size, MAX_PUSH_CONSTANTS_SIZE_BYTES
            )));
        }

        if !self.start.is_multiple_of(PUSH_CONSTANTS_ALIGNMENT) {
            return Err(GpuError::invalid_kernel_params(format!(
                "push-constant range start {} is not {}-byte aligned",
                self.start, PUSH_CONSTANTS_ALIGNMENT
            )));
        }

        if !self.end.is_multiple_of(PUSH_CONSTANTS_ALIGNMENT) {
            return Err(GpuError::invalid_kernel_params(format!(
                "push-constant range end {} is not {}-byte aligned",
                self.end, PUSH_CONSTANTS_ALIGNMENT
            )));
        }

        Ok(())
    }

    /// Return the size of this range in bytes.
    #[inline]
    pub fn size(&self) -> u32 {
        self.end.saturating_sub(self.start)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// PushConstantsLayout
// ─────────────────────────────────────────────────────────────────────────────

/// Describes all push-constant data ranges used by a pipeline.
///
/// In wgpu 29 only the total byte size matters at pipeline-creation time (set
/// via [`wgpu::PipelineLayoutDescriptor::immediate_size`]).  The per-range
/// breakdown is kept here for documentation and validation purposes.
///
/// # Invariants
///
/// All ranges must individually satisfy [`PushConstantRange::validate`], and
/// `total_size` must equal `end` of the last range (or the extent of all
/// non-overlapping ranges combined).
#[derive(Debug, Clone)]
pub struct PushConstantsLayout {
    /// Individual data ranges (may overlap in theory, though callers should
    /// keep them distinct for clarity).
    pub ranges: Vec<PushConstantRange>,
    /// Total number of bytes reserved in the immediate-data block.  Must
    /// be 4-byte aligned and ≤ [`MAX_PUSH_CONSTANTS_SIZE_BYTES`].
    pub total_size: u32,
}

impl PushConstantsLayout {
    /// Create a layout with a single compute-only range covering `[0, size)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxigdal_gpu::push_constants::PushConstantsLayout;
    ///
    /// let l = PushConstantsLayout::compute_only(64);
    /// assert_eq!(l.total_size, 64);
    /// assert_eq!(l.ranges.len(), 1);
    /// assert!(l.validate().is_ok());
    /// ```
    pub fn compute_only(size: u32) -> Self {
        Self {
            ranges: vec![PushConstantRange::compute(size)],
            total_size: size,
        }
    }

    /// Validate all contained ranges.
    ///
    /// # Errors
    ///
    /// Propagates the first validation error from any constituent
    /// [`PushConstantRange::validate`] call.  Also returns an error when
    /// `total_size` exceeds [`MAX_PUSH_CONSTANTS_SIZE_BYTES`] or is not
    /// 4-byte aligned.
    pub fn validate(&self) -> GpuResult<()> {
        if !self.total_size.is_multiple_of(PUSH_CONSTANTS_ALIGNMENT) {
            return Err(GpuError::invalid_kernel_params(format!(
                "push-constants total_size {} is not {}-byte aligned",
                self.total_size, PUSH_CONSTANTS_ALIGNMENT
            )));
        }

        if self.total_size > MAX_PUSH_CONSTANTS_SIZE_BYTES {
            return Err(GpuError::invalid_kernel_params(format!(
                "push-constants total_size {} bytes exceeds maximum of {} bytes",
                self.total_size, MAX_PUSH_CONSTANTS_SIZE_BYTES
            )));
        }

        for range in &self.ranges {
            range.validate()?;
        }

        Ok(())
    }

    /// Return the `immediate_size` value to pass to
    /// [`wgpu::PipelineLayoutDescriptor`].
    ///
    /// This is simply `self.total_size`.
    #[inline]
    pub fn immediate_size_for_wgpu(&self) -> u32 {
        self.total_size
    }

    /// Convert all ranges to a vector of descriptive strings (for logging).
    pub fn to_wgpu_ranges(&self) -> Vec<PushConstantRangeDesc> {
        self.ranges
            .iter()
            .map(|r| PushConstantRangeDesc {
                stages: r.stages,
                start: r.start,
                end: r.end,
            })
            .collect()
    }
}

/// Descriptive wrapper returned by [`PushConstantsLayout::to_wgpu_ranges`].
///
/// In wgpu 29 there is no `wgpu::PushConstantRange` type; this struct mirrors
/// the pre-wgpu-29 shape for documentation purposes and to satisfy tests that
/// inspect range fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushConstantRangeDesc {
    /// Shader stages associated with this range (informational).
    pub stages: wgpu::ShaderStages,
    /// Start byte offset (inclusive).
    pub start: u32,
    /// End byte offset (exclusive).
    pub end: u32,
}

impl PushConstantRangeDesc {
    /// Return the byte range as a `std::ops::Range<u32>`.
    pub fn range(&self) -> std::ops::Range<u32> {
        self.start..self.end
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// PushConstantsBuffer
// ─────────────────────────────────────────────────────────────────────────────

/// A typed byte buffer holding push-constant (immediate) data.
///
/// Callers write scalar and vector values into the buffer via the typed helper
/// methods (`write_u32`, `write_f32`, etc.) and then upload the raw bytes to
/// the GPU via [`wgpu::ComputePass::set_immediates`].
///
/// # Layout
///
/// The buffer is zero-initialised at construction.  Writes use
/// **little-endian** byte order (matching wgpu's expectation on all supported
/// platforms).
#[derive(Debug, Clone)]
pub struct PushConstantsBuffer {
    /// Raw byte storage.
    pub data: Vec<u8>,
    /// Layout that governs this buffer's structure.
    pub layout: PushConstantsLayout,
}

impl PushConstantsBuffer {
    /// Allocate a new zero-initialised buffer sized to `layout.total_size`.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxigdal_gpu::push_constants::{PushConstantsBuffer, PushConstantsLayout};
    ///
    /// let layout = PushConstantsLayout::compute_only(16);
    /// let buf = PushConstantsBuffer::new(layout);
    /// assert_eq!(buf.size(), 16);
    /// assert!(buf.as_bytes().iter().all(|&b| b == 0));
    /// ```
    pub fn new(layout: PushConstantsLayout) -> Self {
        let size = layout.total_size as usize;
        Self {
            data: vec![0u8; size],
            layout,
        }
    }

    /// Write any [`NoUninit`] + [`Copy`] type at `offset` bytes into the
    /// buffer.
    ///
    /// The write is bounds-checked: an error is returned if
    /// `offset + size_of::<T>()` would exceed the buffer size.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError::InvalidKernelParams`] on bounds overflow.
    pub fn write<T: NoUninit + Copy>(&mut self, offset: u32, value: T) -> GpuResult<()> {
        let type_size = size_of::<T>();
        let start = offset as usize;
        let end = start.checked_add(type_size).ok_or_else(|| {
            GpuError::invalid_kernel_params(format!(
                "push-constants write: offset {} + type size {} overflows usize",
                offset, type_size
            ))
        })?;

        if end > self.data.len() {
            return Err(GpuError::invalid_kernel_params(format!(
                "push-constants write: offset {} + {} bytes = {} exceeds buffer size {}",
                offset,
                type_size,
                end,
                self.data.len()
            )));
        }

        let bytes = bytemuck::bytes_of(&value);
        self.data[start..end].copy_from_slice(bytes);
        Ok(())
    }

    /// Write a `u32` at `offset` bytes (little-endian).
    ///
    /// # Errors
    ///
    /// Returns an error when the write would exceed the buffer bounds.
    #[inline]
    pub fn write_u32(&mut self, offset: u32, value: u32) -> GpuResult<()> {
        self.write(offset, value)
    }

    /// Write an `i32` at `offset` bytes (little-endian).
    ///
    /// # Errors
    ///
    /// Returns an error when the write would exceed the buffer bounds.
    #[inline]
    pub fn write_i32(&mut self, offset: u32, value: i32) -> GpuResult<()> {
        self.write(offset, value)
    }

    /// Write an `f32` at `offset` bytes (little-endian, IEEE 754).
    ///
    /// # Errors
    ///
    /// Returns an error when the write would exceed the buffer bounds.
    #[inline]
    pub fn write_f32(&mut self, offset: u32, value: f32) -> GpuResult<()> {
        self.write(offset, value)
    }

    /// Write a `[f32; 4]` vec4 starting at `offset` bytes.
    ///
    /// This writes 16 bytes: `[x, y, z, w]` in little-endian order.
    ///
    /// # Errors
    ///
    /// Returns an error when the write would exceed the buffer bounds.
    #[inline]
    pub fn write_vec4_f32(&mut self, offset: u32, value: [f32; 4]) -> GpuResult<()> {
        self.write(offset, value)
    }

    /// Write a `[u32; 4]` uvec4 starting at `offset` bytes (16 bytes total).
    ///
    /// # Errors
    ///
    /// Returns an error when the write would exceed the buffer bounds.
    #[inline]
    pub fn write_uvec4(&mut self, offset: u32, value: [u32; 4]) -> GpuResult<()> {
        self.write(offset, value)
    }

    /// Write a `[f32; 2]` vec2 starting at `offset` bytes (8 bytes total).
    ///
    /// # Errors
    ///
    /// Returns an error when the write would exceed the buffer bounds.
    #[inline]
    pub fn write_vec2_f32(&mut self, offset: u32, value: [f32; 2]) -> GpuResult<()> {
        self.write(offset, value)
    }

    /// Return a view of the raw byte content.
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    /// Return the size of the buffer in bytes.
    #[inline]
    pub fn size(&self) -> u32 {
        self.data.len() as u32
    }

    /// Reset all bytes to zero.
    pub fn clear(&mut self) {
        self.data.iter_mut().for_each(|b| *b = 0);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Feature query helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Return `true` if the GPU context supports immediate data (push constants).
///
/// This checks for [`wgpu::Features::IMMEDIATES`].  On platforms where this
/// feature is unavailable, callers should fall back to uniform buffers.
///
/// # Examples
///
/// ```rust,no_run
/// use oxigdal_gpu::{GpuContext, push_constants::supports_push_constants};
///
/// # async fn ex() -> oxigdal_gpu::GpuResult<()> {
/// let ctx = GpuContext::new().await?;
/// println!("push constants supported: {}", supports_push_constants(&ctx));
/// # Ok(())
/// # }
/// ```
pub fn supports_push_constants(ctx: &GpuContext) -> bool {
    ctx.device().features().contains(wgpu::Features::IMMEDIATES)
}

/// Return the maximum immediate-data (push-constants) size in bytes supported
/// by the current adapter's device.
///
/// Returns `0` when [`wgpu::Features::IMMEDIATES`] is not enabled on the
/// device (the limit is reported as 0 in that case by wgpu).
///
/// # Examples
///
/// ```rust,no_run
/// use oxigdal_gpu::{GpuContext, push_constants::max_push_constants_size};
///
/// # async fn ex() -> oxigdal_gpu::GpuResult<()> {
/// let ctx = GpuContext::new().await?;
/// println!("max push constants: {} bytes", max_push_constants_size(&ctx));
/// # Ok(())
/// # }
/// ```
pub fn max_push_constants_size(ctx: &GpuContext) -> u32 {
    ctx.device().limits().max_immediate_size
}

// ─────────────────────────────────────────────────────────────────────────────
// Shader source generation
// ─────────────────────────────────────────────────────────────────────────────

/// Generate a WGSL compute shader source that uses a `var<immediate>` block.
///
/// The generated shader:
/// - Embeds `struct_def_wgsl` verbatim (must define `PushConstantsBlock`).
/// - Declares `var<immediate> pc: PushConstantsBlock;` (wgpu 29 naming).
/// - Wraps `body_wgsl` in a `@compute @workgroup_size(16, 16, 1)` entry point.
///
/// # Arguments
///
/// - `struct_def_wgsl` — a complete WGSL struct definition, **must** be named
///   `PushConstantsBlock` so that the `var<immediate>` declaration refers to
///   the correct type.
/// - `body_wgsl` — statements for the compute entry-point body; may reference
///   `pc` to access push-constant data.
///
/// # Example
///
/// ```
/// use oxigdal_gpu::push_constants::make_push_constants_shader_source;
///
/// let src = make_push_constants_shader_source(
///     "struct PushConstantsBlock { width: u32, height: u32 }",
///     "let w = pc.width;",
/// );
/// assert!(src.contains("var<immediate>"));
/// assert!(src.contains("@compute"));
/// ```
pub fn make_push_constants_shader_source(struct_def_wgsl: &str, body_wgsl: &str) -> String {
    format!(
        "{struct_def}\n\nvar<immediate> pc: PushConstantsBlock;\n\n\
@compute @workgroup_size(16, 16, 1)\n\
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {{\n\
{body}\n\
}}\n",
        struct_def = struct_def_wgsl,
        body = body_wgsl
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// Pipeline construction
// ─────────────────────────────────────────────────────────────────────────────

/// Compile a compute pipeline that uses immediate data (push constants) with
/// the given layout.
///
/// # Algorithm
///
/// 1. Validate `layout`.
/// 2. Compile `wgsl` into a [`wgpu::ShaderModule`].
/// 3. Create a [`wgpu::PipelineLayout`] with
///    `immediate_size = layout.total_size` and the
///    [`wgpu::Features::IMMEDIATES`] requirement already satisfied (callers
///    must have requested the feature during context construction).
/// 4. Create and return the [`wgpu::ComputePipeline`] wrapped in [`Arc`].
///
/// # Errors
///
/// - [`GpuError::InvalidKernelParams`] — layout validation failed.
/// - [`GpuError::UnsupportedOperation`] — the device does not support the
///   `IMMEDIATES` feature.
/// - [`GpuError::ShaderCompilation`] — the WGSL source failed to compile.
///
/// # Examples
///
/// ```rust,no_run
/// use oxigdal_gpu::{GpuContext, push_constants::{
///     PushConstantsLayout, make_push_constants_shader_source,
///     build_push_constants_pipeline,
/// }};
///
/// # async fn ex() -> oxigdal_gpu::GpuResult<()> {
/// let ctx = oxigdal_gpu::GpuContextConfig::new()
///     .with_push_constants()
///     .build().await?;
/// let layout = PushConstantsLayout::compute_only(16);
/// let src = make_push_constants_shader_source(
///     "struct PushConstantsBlock { value: u32, _pad0: u32, _pad1: u32, _pad2: u32 }",
///     "let v = pc.value;",
/// );
/// let pipeline = build_push_constants_pipeline(&ctx, &src, "main", &layout)?;
/// # Ok(())
/// # }
/// ```
pub fn build_push_constants_pipeline(
    ctx: &GpuContext,
    wgsl: &str,
    entry: &str,
    layout: &PushConstantsLayout,
) -> GpuResult<Arc<wgpu::ComputePipeline>> {
    layout.validate()?;

    // Verify the IMMEDIATES feature is available on this device.
    if !ctx.device().features().contains(wgpu::Features::IMMEDIATES) {
        return Err(GpuError::unsupported_operation(
            "wgpu::Features::IMMEDIATES is required for push-constants pipelines; \
             create the GpuContext with GpuContextConfig::with_push_constants()",
        ));
    }

    let device = ctx.device();

    // Compile the shader module.
    let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("push_constants_shader"),
        source: wgpu::ShaderSource::Wgsl(wgsl.into()),
    });

    // Build the pipeline layout, requesting immediate_size bytes.
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("push_constants_layout"),
        bind_group_layouts: &[],
        immediate_size: layout.immediate_size_for_wgpu(),
    });

    // Compile the compute pipeline.
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("push_constants_pipeline"),
        layout: Some(&pipeline_layout),
        module: &shader_module,
        entry_point: Some(entry),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    });

    debug!(
        "Built push-constants compute pipeline (entry={}, immediate_size={})",
        entry,
        layout.immediate_size_for_wgpu()
    );

    Ok(Arc::new(pipeline))
}

/// Upload immediate (push-constant) data and dispatch a compute pass.
///
/// This is a convenience function that:
/// 1. Begins a compute pass on `encoder`.
/// 2. Sets the pipeline.
/// 3. Calls `set_immediates` with the buffer contents.
/// 4. Dispatches `(workgroups_x, workgroups_y, workgroups_z)` workgroups.
///
/// The caller is responsible for submitting `encoder` to the queue.
///
/// # Errors
///
/// None — wgpu compute-pass encoding is infallible at this level.  Errors
/// surface when the command buffer is submitted to the queue.
pub fn dispatch_with_push_constants(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::ComputePipeline,
    buf: &PushConstantsBuffer,
    workgroups_x: u32,
    workgroups_y: u32,
    workgroups_z: u32,
) {
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some("push_constants_dispatch"),
        timestamp_writes: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_immediates(0, buf.as_bytes());
    pass.dispatch_workgroups(workgroups_x, workgroups_y, workgroups_z);
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_constant_range_compute_size() {
        let r = PushConstantRange::compute(32);
        assert_eq!(r.start, 0);
        assert_eq!(r.end, 32);
        assert_eq!(r.size(), 32);
        assert!(r.validate().is_ok());
    }

    #[test]
    fn test_push_constant_range_validate_rejects_zero_size() {
        let r = PushConstantRange {
            stages: wgpu::ShaderStages::COMPUTE,
            start: 0,
            end: 0,
        };
        assert!(r.validate().is_err());
    }

    #[test]
    fn test_push_constant_range_validate_rejects_over_limit() {
        let r = PushConstantRange {
            stages: wgpu::ShaderStages::COMPUTE,
            start: 0,
            end: MAX_PUSH_CONSTANTS_SIZE_BYTES + 4,
        };
        assert!(r.validate().is_err());
    }

    #[test]
    fn test_push_constant_range_validate_rejects_unaligned_end() {
        let r = PushConstantRange {
            stages: wgpu::ShaderStages::COMPUTE,
            start: 0,
            end: 6, // not 4-byte aligned
        };
        assert!(r.validate().is_err());
    }

    #[test]
    fn test_push_constants_layout_compute_only() {
        let l = PushConstantsLayout::compute_only(64);
        assert_eq!(l.total_size, 64);
        assert_eq!(l.ranges.len(), 1);
        assert!(l.validate().is_ok());
    }

    #[test]
    fn test_push_constants_layout_to_wgpu_ranges() {
        let l = PushConstantsLayout::compute_only(16);
        let ranges = l.to_wgpu_ranges();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].range(), 0..16);
    }

    #[test]
    fn test_push_constants_buffer_write_u32() {
        let layout = PushConstantsLayout::compute_only(16);
        let mut buf = PushConstantsBuffer::new(layout);
        buf.write_u32(0, 42).expect("write_u32 failed");
        let bytes = buf.as_bytes();
        let val = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        assert_eq!(val, 42);
    }

    #[test]
    fn test_push_constants_buffer_write_f32() {
        let layout = PushConstantsLayout::compute_only(16);
        let mut buf = PushConstantsBuffer::new(layout);
        buf.write_f32(4, std::f32::consts::PI)
            .expect("write_f32 failed");
        let bytes = buf.as_bytes();
        let val = f32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        assert!(
            (val - std::f32::consts::PI).abs() < 1e-6,
            "expected π, got {val}"
        );
    }

    #[test]
    fn test_push_constants_buffer_write_overflow_errors() {
        let layout = PushConstantsLayout::compute_only(4);
        let mut buf = PushConstantsBuffer::new(layout);
        // Offset 4 + 4 bytes = 8 > 4: must fail.
        let result = buf.write_u32(4, 1);
        assert!(result.is_err(), "expected overflow error, got Ok");
    }
}
