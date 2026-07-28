//! GPU timestamp profiling using `wgpu::Features::TIMESTAMP_QUERY`.
//!
//! Provides an opt-in [`GpuTimestampProfiler`] that records GPU-side
//! timestamps around compute passes via a [`wgpu::QuerySet`] and reports
//! per-pass timings in microseconds after resolution.
//!
//! The profiler is **graceful**: [`GpuTimestampProfiler::try_new`] returns
//! `None` whenever the adapter does not advertise the required features
//! ([`wgpu::Features::TIMESTAMP_QUERY`] **and**
//! [`wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS`] — the latter is
//! required to call [`wgpu::CommandEncoder::write_timestamp`] outside of a
//! render/compute pass).  Callers that do not enable these features when
//! constructing the [`GpuContext`] will simply receive `None` and can fall
//! back to wall-clock timing.
//!
//! # Quick sketch
//!
//! ```rust,no_run
//! use oxigeo_gpu::{GpuContext, GpuTimestampProfiler};
//! # async fn run(ctx: &GpuContext) -> Result<(), Box<dyn std::error::Error>> {
//! let mut prof = match GpuTimestampProfiler::try_new(ctx, 16) {
//!     Some(p) => p,
//!     None => return Ok(()), // adapter lacks TIMESTAMP_QUERY support
//! };
//! let mut encoder = ctx.device().create_command_encoder(&Default::default());
//! if let Some(slot) = prof.begin_pass(&mut encoder, "blur") {
//!     // ... record compute work into `encoder` ...
//!     prof.end_pass(&mut encoder, slot);
//! }
//! ctx.queue().submit([encoder.finish()]);
//! for t in prof.resolve(ctx)? {
//!     println!("{} took {:.3} us", t.label, t.duration_us);
//! }
//! # Ok(()) }
//! ```
//!
//! # Notes on wgpu features
//!
//! - [`wgpu::Features::TIMESTAMP_QUERY`] is the WebGPU baseline feature that
//!   enables creating [`wgpu::QueryType::Timestamp`] query sets and using
//!   them within render/compute pass *descriptors*.
//! - [`wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS`] is the native-only
//!   extension that allows calling
//!   [`wgpu::CommandEncoder::write_timestamp`] **between** passes.  This is
//!   the API used by [`GpuTimestampProfiler::begin_pass`] /
//!   [`GpuTimestampProfiler::end_pass`].
//!
//! Both features must be requested via
//! [`GpuContextConfig::with_features`][crate::context::GpuContextConfig::with_features]
//! before creating the [`GpuContext`].

use crate::context::GpuContext;
use crate::error::{GpuError, GpuResult};

/// Bytes per timestamp value resolved into the destination buffer.
///
/// WGPU writes one `u64` per timestamp into a [`wgpu::BufferUsages::QUERY_RESOLVE`]
/// buffer, so the byte stride is always `8` (see
/// [`wgpu::QUERY_SIZE`]).
const TIMESTAMP_BYTES: u64 = 8;

/// Profiled timing for a single GPU pass.
///
/// The `start_ns` / `end_ns` fields are the absolute timestamp values
/// converted to nanoseconds (multiplied by
/// [`wgpu::Queue::get_timestamp_period`]).  They have no anchor and should
/// only be compared **within** the same profiler instance / submission.
///
/// `duration_us` is precomputed as `(end_ns - start_ns) / 1000.0` for
/// convenience.  Negative differences cannot occur because the underlying
/// subtraction uses saturating arithmetic on `u64`.
#[derive(Debug, Clone, PartialEq)]
pub struct PassTiming {
    /// User-supplied label for the pass.
    pub label: String,
    /// Start timestamp, converted to nanoseconds.
    pub start_ns: u64,
    /// End timestamp, converted to nanoseconds.
    pub end_ns: u64,
    /// Pass duration in microseconds: `(end_ns − start_ns) / 1000.0`.
    pub duration_us: f64,
}

/// Opt-in GPU timestamp profiler backed by a single [`wgpu::QuerySet`].
///
/// Each pass consumes two adjacent timestamp slots (start, end).  After
/// recording, call [`Self::resolve`] to copy the query results into a
/// staging buffer, map it, and decode the raw `u64` timestamps into
/// [`PassTiming`] structs.
///
/// See the module-level documentation for usage.
pub struct GpuTimestampProfiler {
    /// `true` if the underlying adapter supports timestamp queries and the
    /// profiler is recording GPU work; `false` for the test-only
    /// [`Self::dummy`] constructor.
    enabled: bool,
    /// Total number of timestamp slots in the query set (always even).
    capacity: u32,
    /// Index of the next free slot.  Incremented by 2 on each [`Self::begin_pass`].
    next_slot: u32,
    /// Nanoseconds per timestamp tick, from [`wgpu::Queue::get_timestamp_period`].
    period_ns: f32,
    /// One entry per recorded pass (indexed by `slot / 2`).
    labels: Vec<String>,
    /// WGPU query set holding the timestamps (None for [`Self::dummy`]).
    query_set: Option<wgpu::QuerySet>,
    /// Buffer that receives the resolved query data on the GPU.
    resolve_buffer: Option<wgpu::Buffer>,
    /// Staging buffer that the CPU maps to read the timestamps back.
    staging_buffer: Option<wgpu::Buffer>,
}

impl std::fmt::Debug for GpuTimestampProfiler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuTimestampProfiler")
            .field("enabled", &self.enabled)
            .field("capacity", &self.capacity)
            .field("next_slot", &self.next_slot)
            .field("period_ns", &self.period_ns)
            .field("labels", &self.labels)
            .field("has_query_set", &self.query_set.is_some())
            .field("has_resolve_buffer", &self.resolve_buffer.is_some())
            .field("has_staging_buffer", &self.staging_buffer.is_some())
            .finish()
    }
}

impl GpuTimestampProfiler {
    /// Attempt to construct a profiler with `capacity` timestamp slots.
    ///
    /// `capacity` is rounded **up** to the nearest even number (and clamped
    /// to a minimum of 2) because each pass uses one start slot and one end
    /// slot.
    ///
    /// Returns `None` if the adapter does not support both
    /// [`wgpu::Features::TIMESTAMP_QUERY`] and
    /// [`wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS`].  Callers must
    /// enable these features in the
    /// [`crate::context::GpuContextConfig`] used to create the context
    /// (see the module-level docs).
    pub fn try_new(ctx: &GpuContext, capacity: u32) -> Option<Self> {
        let features = ctx.device().features();
        if !features.contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        if !features.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS) {
            return None;
        }

        // Round up to even, with a minimum of 2 (one start + one end slot).
        let capacity = capacity.max(2);
        let capacity = if capacity.is_multiple_of(2) {
            capacity
        } else {
            capacity.saturating_add(1)
        };

        let period_ns = ctx.queue().get_timestamp_period();

        let query_set = ctx.device().create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("oxigeo_profiler_timestamps"),
            count: capacity,
            ty: wgpu::QueryType::Timestamp,
        });

        let byte_size = (capacity as u64).saturating_mul(TIMESTAMP_BYTES);

        let resolve_buffer = ctx.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("oxigeo_profiler_resolve"),
            size: byte_size,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let staging_buffer = ctx.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("oxigeo_profiler_staging"),
            size: byte_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Some(Self {
            enabled: true,
            capacity,
            next_slot: 0,
            period_ns,
            labels: Vec::new(),
            query_set: Some(query_set),
            resolve_buffer: Some(resolve_buffer),
            staging_buffer: Some(staging_buffer),
        })
    }

    /// Test-only constructor that allocates no GPU resources.
    ///
    /// Useful for unit tests that need to exercise the `PassTiming` types
    /// or the disabled-path branches without touching wgpu.  All recording
    /// methods become no-ops and [`Self::resolve`] returns an empty vector.
    pub fn dummy(period_ns: f32) -> Self {
        Self {
            enabled: false,
            capacity: 0,
            next_slot: 0,
            period_ns,
            labels: Vec::new(),
            query_set: None,
            resolve_buffer: None,
            staging_buffer: None,
        }
    }

    /// Returns `true` if this profiler is actively recording GPU timestamps.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Timestamp period in nanoseconds per tick, as reported by
    /// [`wgpu::Queue::get_timestamp_period`].
    pub fn period_ns(&self) -> f32 {
        self.period_ns
    }

    /// Total number of timestamp slots available in the query set.
    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    /// Index of the next free start slot.  Always even when the profiler is
    /// in a consistent recording state.
    pub fn next_slot(&self) -> u32 {
        self.next_slot
    }

    /// Records the **start** timestamp for a pass labelled `label`.
    ///
    /// Returns the slot index that was used (which is also the index of the
    /// start–end pair).  Pass this value to [`Self::end_pass`] to record the
    /// matching end timestamp.
    ///
    /// Returns `None` when:
    /// * the profiler is disabled (constructed via [`Self::dummy`]), or
    /// * the query set capacity has been exhausted.
    pub fn begin_pass(&mut self, encoder: &mut wgpu::CommandEncoder, label: &str) -> Option<u32> {
        if !self.enabled {
            return None;
        }
        if self.next_slot.saturating_add(1) >= self.capacity {
            return None;
        }
        let qs = self.query_set.as_ref()?;
        let slot = self.next_slot;
        encoder.write_timestamp(qs, slot);
        self.labels.push(label.to_string());
        self.next_slot = self.next_slot.saturating_add(2);
        Some(slot)
    }

    /// Records the **end** timestamp for a pass at `start_slot + 1`.
    ///
    /// Silently no-ops when the profiler is disabled or has no query set.
    /// The caller is responsible for passing the slot index returned by the
    /// matching [`Self::begin_pass`] call.
    pub fn end_pass(&self, encoder: &mut wgpu::CommandEncoder, start_slot: u32) {
        if !self.enabled {
            return;
        }
        if let Some(qs) = &self.query_set {
            encoder.write_timestamp(qs, start_slot.saturating_add(1));
        }
    }

    /// Resolves all recorded timestamps and decodes them into per-pass
    /// [`PassTiming`] entries.
    ///
    /// Steps:
    /// 1. Encodes a `resolve_query_set` + `copy_buffer_to_buffer` command
    ///    sequence to move the timestamps from the GPU-side query set into
    ///    a CPU-mappable staging buffer.
    /// 2. Submits the command buffer.
    /// 3. Maps the staging buffer for reading, blocking on
    ///    [`wgpu::Device::poll`] until the mapping completes.
    /// 4. Converts each raw `u64` tick to nanoseconds using
    ///    [`Self::period_ns`] and computes `duration_us`.
    ///
    /// Returns an empty `Vec` if the profiler is disabled or no passes were
    /// recorded.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError::ExecutionFailed`] when the device poll, the
    /// buffer mapping callback, or the channel that delivers the mapping
    /// result fails.
    pub fn resolve(&mut self, ctx: &GpuContext) -> GpuResult<Vec<PassTiming>> {
        if !self.enabled || self.next_slot == 0 {
            return Ok(Vec::new());
        }

        let (Some(qs), Some(resolve_buf), Some(staging_buf)) = (
            self.query_set.as_ref(),
            self.resolve_buffer.as_ref(),
            self.staging_buffer.as_ref(),
        ) else {
            return Ok(Vec::new());
        };

        let recorded_slots = self.next_slot;
        let recorded_bytes = (recorded_slots as u64).saturating_mul(TIMESTAMP_BYTES);

        // 1. Encode resolve + copy into the staging buffer.
        let mut encoder = ctx
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("oxigeo_profiler_resolve_encoder"),
            });
        encoder.resolve_query_set(qs, 0..recorded_slots, resolve_buf, 0);
        encoder.copy_buffer_to_buffer(resolve_buf, 0, staging_buf, 0, recorded_bytes);
        ctx.check_device_lost()?;
        ctx.queue().submit([encoder.finish()]);

        // 2. Map the staging buffer synchronously.
        let slice = staging_buf.slice(0..recorded_bytes);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        ctx.device()
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| {
                GpuError::execution_failed(format!(
                    "profiler: device poll while mapping staging buffer failed: {e:?}"
                ))
            })?;
        rx.recv()
            .map_err(|e| {
                GpuError::execution_failed(format!(
                    "profiler: mapping callback channel closed: {e}"
                ))
            })?
            .map_err(|e| {
                GpuError::execution_failed(format!("profiler: map_async failed: {e:?}"))
            })?;

        // 3. Decode raw u64 timestamps.
        let data = slice.get_mapped_range().map_err(|e| {
            GpuError::execution_failed(format!("profiler: get_mapped_range failed: {e}"))
        })?;
        let raw_ts: Vec<u64> = data
            .chunks_exact(TIMESTAMP_BYTES as usize)
            .map(|c| u64::from_ne_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
            .collect();
        drop(data);
        staging_buf.unmap();

        // 4. Build PassTiming entries.
        let mut out = Vec::with_capacity(self.labels.len());
        for (i, label) in self.labels.iter().enumerate() {
            let start_raw = raw_ts.get(i * 2).copied().unwrap_or(0);
            let end_raw = raw_ts.get(i * 2 + 1).copied().unwrap_or(0);
            let duration_ns = end_raw.saturating_sub(start_raw) as f64 * self.period_ns as f64;
            let start_ns = (start_raw as f64 * self.period_ns as f64) as u64;
            let end_ns = (end_raw as f64 * self.period_ns as f64) as u64;
            out.push(PassTiming {
                label: label.clone(),
                start_ns,
                end_ns,
                duration_us: duration_ns / 1000.0,
            });
        }
        Ok(out)
    }

    /// Returns the recorded pass labels in registration order.
    pub fn pass_labels(&self) -> &[String] {
        &self.labels
    }

    /// Clears all recorded labels and resets the next-slot pointer back to
    /// zero, allowing the same query-set capacity to be re-used for another
    /// round of measurements.
    ///
    /// Note that the underlying [`wgpu::QuerySet`] storage is **not**
    /// re-allocated; previously written timestamps remain in the resolve
    /// buffer until overwritten by subsequent `write_timestamp` calls.
    pub fn reset(&mut self) {
        self.labels.clear();
        self.next_slot = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dummy_profiler_has_no_query_set() {
        let prof = GpuTimestampProfiler::dummy(1.0);
        assert!(!prof.is_enabled());
        assert_eq!(prof.capacity(), 0);
        assert_eq!(prof.next_slot(), 0);
        assert_eq!(prof.pass_labels().len(), 0);
        // None of the wgpu resources should be allocated.
        assert!(prof.query_set.is_none());
        assert!(prof.resolve_buffer.is_none());
        assert!(prof.staging_buffer.is_none());
    }

    #[test]
    fn dummy_profiler_reset_is_idempotent() {
        let mut prof = GpuTimestampProfiler::dummy(2.0);
        prof.reset();
        prof.reset();
        assert_eq!(prof.next_slot(), 0);
        assert!(prof.pass_labels().is_empty());
    }

    #[test]
    fn pass_timing_construction() {
        let t = PassTiming {
            label: "blur".to_string(),
            start_ns: 1_000,
            end_ns: 2_500,
            duration_us: 1.5,
        };
        assert_eq!(t.label, "blur");
        assert_eq!(t.end_ns - t.start_ns, 1_500);
        assert!((t.duration_us - 1.5).abs() < f64::EPSILON);
    }

    #[test]
    fn debug_impl_does_not_panic() {
        let prof = GpuTimestampProfiler::dummy(1.0);
        let s = format!("{:?}", prof);
        assert!(s.contains("GpuTimestampProfiler"));
    }
}
