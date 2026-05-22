//! Indirect compute dispatch support for GPU-driven workloads.
//!
//! Indirect dispatch allows the workgroup counts for a compute shader to be
//! stored in a GPU-side buffer (written by a preceding GPU pass), eliminating
//! the need for a CPU readback round-trip before issuing the next dispatch.
//!
//! # Key types
//!
//! - [`DispatchIndirectArgs`] — layout-stable, 12-byte struct that maps
//!   directly onto the `VkDispatchIndirectCommand` / D3D12 / Metal equivalent.
//! - [`IndirectDispatchBuffer`] — a GPU buffer holding one or more dispatch
//!   argument slots, initialised from the CPU and updateable at any time.
//!
//! # Helper functions
//!
//! - [`workgroup_count_1d`] / [`workgroup_count_2d`] / [`workgroup_count_3d`]
//!   — compute the ceiling-division workgroup counts for common dispatch
//!   dimensions without requiring a GPU context.
//! - [`args_for_elements`] — short-hand for building a 1-D dispatch args struct.
//! - [`dispatch_indirect_on_pass`] — convenience wrapper that sets the pipeline,
//!   bind groups, and issues the indirect dispatch in one call.

use std::sync::Arc;
use wgpu::{Buffer, BufferAddress, BufferDescriptor, BufferUsages, ComputePipeline};

use crate::context::GpuContext;
use crate::error::{GpuError, GpuResult};

// ─────────────────────────────────────────────────────────────────────────────
// DispatchIndirectArgs
// ─────────────────────────────────────────────────────────────────────────────

/// Layout-stable representation of workgroup dispatch dimensions stored in GPU
/// memory.
///
/// The struct is `repr(C)` so that its in-memory byte layout is well-defined
/// and can be written directly to a GPU buffer via
/// [`wgpu::Queue::write_buffer`].  It corresponds to the 12-byte
/// `VkDispatchIndirectCommand` structure (and its equivalents in D3D12 and
/// Metal).
///
/// # Layout
///
/// | Bytes | Field | Meaning                         |
/// |-------|-------|---------------------------------|
/// | 0–3   | `x`   | Workgroup count along X axis    |
/// | 4–7   | `y`   | Workgroup count along Y axis    |
/// | 8–11  | `z`   | Workgroup count along Z axis    |
///
/// All values are stored in little-endian byte order when serialised via
/// [`DispatchIndirectArgs::as_bytes`].
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatchIndirectArgs {
    /// Workgroup count along the X dispatch dimension.
    pub x: u32,
    /// Workgroup count along the Y dispatch dimension.
    pub y: u32,
    /// Workgroup count along the Z dispatch dimension.
    pub z: u32,
}

impl DispatchIndirectArgs {
    /// Create new dispatch arguments with explicit x / y / z workgroup counts.
    #[inline]
    pub fn new(x: u32, y: u32, z: u32) -> Self {
        Self { x, y, z }
    }

    /// Create 1-D dispatch arguments for `total` elements with a workgroup
    /// size of `workgroup_size`.
    ///
    /// The Y and Z dimensions are set to 1. If `workgroup_size` is 0 the X
    /// count is also 0.
    #[inline]
    pub fn for_1d(total: u32, workgroup_size: u32) -> Self {
        Self {
            x: workgroup_count_1d(total, workgroup_size),
            y: 1,
            z: 1,
        }
    }

    /// Serialise the struct into a 12-byte little-endian array suitable for
    /// writing to a GPU buffer.
    #[inline]
    pub fn as_bytes(&self) -> [u8; 12] {
        let mut buf = [0u8; 12];
        buf[0..4].copy_from_slice(&self.x.to_le_bytes());
        buf[4..8].copy_from_slice(&self.y.to_le_bytes());
        buf[8..12].copy_from_slice(&self.z.to_le_bytes());
        buf
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// IndirectDispatchBuffer
// ─────────────────────────────────────────────────────────────────────────────

/// A GPU buffer that holds one or more [`DispatchIndirectArgs`] slots for use
/// with `wgpu::ComputePass::dispatch_workgroups_indirect`.
///
/// Each slot occupies exactly 12 bytes (the size of one
/// [`DispatchIndirectArgs`]).  The buffer is created with
/// `INDIRECT | COPY_DST | STORAGE` usages so it can both receive CPU-side
/// writes and be written by a preceding compute shader pass.
///
/// # Multi-slot layout
///
/// ```text
/// Offset 0       12     24     36 …
///         ├──────┼──────┼──────┤
///         │ [0]  │ [1]  │ [2]  │  …
///         └──────┴──────┴──────┘
///           12 B   12 B   12 B
/// ```
///
/// The active slot for a dispatch is selected via [`IndirectDispatchBuffer::offset`]
/// (defaults to slot 0) and can be overridden by calling
/// [`IndirectDispatchBuffer::update_at`] to write to a specific slot and then
/// constructing a view with the desired byte offset — or by creating separate
/// buffers for each independently-driven dispatch.
pub struct IndirectDispatchBuffer {
    buffer: Arc<Buffer>,
    /// Byte offset into `buffer` at which slot 0 begins.
    ///
    /// Always 0 for buffers created via the public constructors; exposed as a
    /// field to allow future sub-buffer views.
    offset: BufferAddress,
    /// Total number of 12-byte dispatch slots in the buffer.
    capacity: u32,
}

impl IndirectDispatchBuffer {
    /// Create a single-slot indirect dispatch buffer initialised with `args`.
    ///
    /// The buffer is 12 bytes and supports `INDIRECT | COPY_DST | STORAGE`
    /// usages.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError::InvalidKernelParams`] if the device rejects the
    /// buffer descriptor (should not happen in practice).
    pub fn new(ctx: &GpuContext, args: DispatchIndirectArgs) -> GpuResult<Self> {
        let buffer = ctx.device().create_buffer(&BufferDescriptor {
            label: Some("indirect_dispatch"),
            size: 12,
            usage: BufferUsages::INDIRECT | BufferUsages::COPY_DST | BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let buf = Arc::new(buffer);
        ctx.queue().write_buffer(&buf, 0, &args.as_bytes());
        Ok(Self {
            buffer: buf,
            offset: 0,
            capacity: 1,
        })
    }

    /// Create a multi-slot indirect dispatch buffer with `max_dispatches`
    /// slots, all initialised to zero.
    ///
    /// The total buffer size is `12 * max_dispatches` bytes.  Each slot can be
    /// written individually via [`IndirectDispatchBuffer::update_at`].
    ///
    /// # Errors
    ///
    /// Returns [`GpuError::InvalidKernelParams`] if `max_dispatches` is 0.
    pub fn new_with_capacity(ctx: &GpuContext, max_dispatches: u32) -> GpuResult<Self> {
        if max_dispatches == 0 {
            return Err(GpuError::InvalidKernelParams {
                reason: "max_dispatches must be > 0".to_string(),
            });
        }
        let size = 12u64 * max_dispatches as u64;
        let buffer = ctx.device().create_buffer(&BufferDescriptor {
            label: Some("indirect_dispatch_multi"),
            size,
            usage: BufferUsages::INDIRECT | BufferUsages::COPY_DST | BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        Ok(Self {
            buffer: Arc::new(buffer),
            offset: 0,
            capacity: max_dispatches,
        })
    }

    /// Overwrite slot 0 with new dispatch arguments.
    ///
    /// This is a CPU-side write using `queue.write_buffer`; the data is
    /// uploaded before the next `queue.submit`.
    pub fn update(&self, ctx: &GpuContext, args: DispatchIndirectArgs) {
        ctx.queue()
            .write_buffer(&self.buffer, self.offset, &args.as_bytes());
    }

    /// Overwrite a specific slot (0-indexed) with new dispatch arguments.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError::InvalidKernelParams`] if `slot_idx >= capacity`.
    pub fn update_at(
        &self,
        ctx: &GpuContext,
        slot_idx: u32,
        args: DispatchIndirectArgs,
    ) -> GpuResult<()> {
        if slot_idx >= self.capacity {
            return Err(GpuError::InvalidKernelParams {
                reason: format!(
                    "slot_idx {} is out of range for capacity {}",
                    slot_idx, self.capacity
                ),
            });
        }
        let slot_offset = self.offset + 12 * slot_idx as u64;
        ctx.queue()
            .write_buffer(&self.buffer, slot_offset, &args.as_bytes());
        Ok(())
    }

    /// Return a reference to the underlying [`wgpu::Buffer`].
    #[inline]
    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    /// Return the byte offset of slot 0 within the buffer.
    ///
    /// This is the value passed as the second argument to
    /// `ComputePass::dispatch_workgroups_indirect`.
    #[inline]
    pub fn offset(&self) -> BufferAddress {
        self.offset
    }

    /// Return the number of 12-byte dispatch slots in the buffer.
    #[inline]
    pub fn capacity(&self) -> u32 {
        self.capacity
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// dispatch_indirect_on_pass
// ─────────────────────────────────────────────────────────────────────────────

/// Set the pipeline, bind groups, and issue an indirect dispatch in one call.
///
/// This is a thin convenience wrapper around the raw `wgpu::ComputePass` API:
///
/// ```text
/// pass.set_pipeline(pipeline);
/// for (i, bg) in bind_groups.iter().enumerate() {
///     pass.set_bind_group(i as u32, *bg, &[]);
/// }
/// pass.dispatch_workgroups_indirect(indirect.buffer(), indirect.offset());
/// ```
///
/// # Arguments
///
/// * `pass`        — an in-progress `ComputePass`.
/// * `pipeline`    — the compute pipeline to bind.
/// * `bind_groups` — slice of bind groups; index 0 → slot 0, etc.
/// * `indirect`    — the buffer supplying the workgroup counts.
///
/// # Panics
///
/// Panics with a wgpu validation error (not a Rust panic) if the pipeline,
/// bind groups, or indirect buffer are invalid for the current device.
pub fn dispatch_indirect_on_pass<'a>(
    pass: &mut wgpu::ComputePass<'a>,
    pipeline: &'a ComputePipeline,
    bind_groups: &[&'a wgpu::BindGroup],
    indirect: &'a IndirectDispatchBuffer,
) {
    pass.set_pipeline(pipeline);
    for (i, bg) in bind_groups.iter().enumerate() {
        pass.set_bind_group(i as u32, *bg, &[]);
    }
    pass.dispatch_workgroups_indirect(indirect.buffer(), indirect.offset());
}

// ─────────────────────────────────────────────────────────────────────────────
// Pure arithmetic helpers — no GPU context required
// ─────────────────────────────────────────────────────────────────────────────

/// Compute the ceiling-division workgroup count for a 1-D dispatch.
///
/// Returns `ceil(total_elements / workgroup_size)`.  If `workgroup_size` is 0
/// the result is 0 (to avoid a division-by-zero panic).
///
/// # Examples
///
/// ```
/// use oxigdal_gpu::workgroup_count_1d;
/// assert_eq!(workgroup_count_1d(64, 64), 1);
/// assert_eq!(workgroup_count_1d(65, 64), 2);
/// assert_eq!(workgroup_count_1d(0,  64), 0);
/// ```
#[inline]
pub fn workgroup_count_1d(total_elements: u32, workgroup_size: u32) -> u32 {
    if workgroup_size == 0 {
        return 0;
    }
    total_elements.saturating_add(workgroup_size - 1) / workgroup_size
}

/// Compute the ceiling-division workgroup counts for a 2-D dispatch.
///
/// Returns `(ceil(width / wg_x), ceil(height / wg_y))`.
///
/// # Examples
///
/// ```
/// use oxigdal_gpu::workgroup_count_2d;
/// assert_eq!(workgroup_count_2d(128, 64, 16, 8), (8, 8));
/// ```
#[inline]
pub fn workgroup_count_2d(width: u32, height: u32, wg_x: u32, wg_y: u32) -> (u32, u32) {
    (
        workgroup_count_1d(width, wg_x),
        workgroup_count_1d(height, wg_y),
    )
}

/// Compute the ceiling-division workgroup counts for a 3-D dispatch.
///
/// Returns `(ceil(d0 / wg0), ceil(d1 / wg1), ceil(d2 / wg2))`.
///
/// # Examples
///
/// ```
/// use oxigdal_gpu::workgroup_count_3d;
/// assert_eq!(workgroup_count_3d((10, 10, 10), (4, 4, 4)), (3, 3, 3));
/// ```
#[inline]
pub fn workgroup_count_3d(dims: (u32, u32, u32), wg_dims: (u32, u32, u32)) -> (u32, u32, u32) {
    (
        workgroup_count_1d(dims.0, wg_dims.0),
        workgroup_count_1d(dims.1, wg_dims.1),
        workgroup_count_1d(dims.2, wg_dims.2),
    )
}

/// Build [`DispatchIndirectArgs`] for a 1-D dispatch over `total` elements
/// with the given `workgroup_size`.
///
/// Y and Z are set to 1.
///
/// # Examples
///
/// ```
/// use oxigdal_gpu::{args_for_elements, DispatchIndirectArgs};
/// let args = args_for_elements(128, 64);
/// assert_eq!(args, DispatchIndirectArgs { x: 2, y: 1, z: 1 });
/// ```
#[inline]
pub fn args_for_elements(total: u32, workgroup_size: u32) -> DispatchIndirectArgs {
    DispatchIndirectArgs {
        x: workgroup_count_1d(total, workgroup_size),
        y: 1,
        z: 1,
    }
}
