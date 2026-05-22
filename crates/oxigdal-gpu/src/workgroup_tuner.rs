//! Workgroup-size auto-tuning derived from GPU adapter limits.
//!
//! Different GPUs expose different compute limits. This module derives
//! optimal workgroup sizes at context-creation time so every kernel
//! dispatches with sizes that the actual device can support.

use wgpu::Limits;

/// Optimal workgroup sizes derived from GPU adapter limits.
///
/// Workgroup sizes are clamped to the adapter's reported maximums,
/// so kernels compile and dispatch correctly on every device.
///
/// # Derivation
///
/// Call [`WorkgroupTuner::derive_from_limits`] after obtaining the adapter
/// to get device-specific sizes.  Use [`WorkgroupTuner::unlimited`] (or
/// [`Default`]) when no real device is available (e.g. in unit tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkgroupTuner {
    /// 2D workgroup size for raster/image kernels (e.g. 16×16 or 8×8).
    pub raster_2d: (u32, u32),
    /// 1D workgroup size for reduction kernels (e.g. 256 or 64).
    pub reduction: u32,
    /// 1D workgroup size for FFT kernels (e.g. 64 or 32).
    pub fft: u32,
}

impl WorkgroupTuner {
    /// Derive optimal workgroup sizes from the adapter's [`Limits`].
    ///
    /// The function tries preferred sizes in descending order of quality and
    /// falls back to smaller values when the adapter cannot accommodate the
    /// larger ones.
    ///
    /// | Class         | Preferred | Fallback 1 | Fallback 2 |
    /// |---------------|-----------|------------|------------|
    /// | `raster_2d`   | 16 × 16   | 8 × 8      | 4 × 4      |
    /// | `reduction`   | 256       | —          | —          |
    /// | `fft`         | 64        | —          | —          |
    ///
    /// `reduction` and `fft` are clamped to `min(preferred, max_x, max_total)`.
    pub fn derive_from_limits(limits: &Limits) -> Self {
        let max_x = limits.max_compute_workgroup_size_x;
        let max_y = limits.max_compute_workgroup_size_y;
        let max_total = limits.max_compute_invocations_per_workgroup;

        // Raster 2D: prefer 16×16 = 256 total, fall back to 8×8 = 64, then 4×4 = 16.
        let raster_2d = if max_x >= 16 && max_y >= 16 && max_total >= 256 {
            (16, 16)
        } else if max_x >= 8 && max_y >= 8 && max_total >= 64 {
            (8, 8)
        } else {
            (4, 4)
        };

        // Reduction 1D: min(256, max_x, max_total) — must never exceed either limit.
        let reduction = 256u32.min(max_x).min(max_total);

        // FFT 1D: min(64, max_x, max_total).
        let fft = 64u32.min(max_x).min(max_total);

        Self {
            raster_2d,
            reduction,
            fft,
        }
    }

    /// Default tuner assuming unbounded adapter limits (production default).
    ///
    /// Equivalent to calling [`derive_from_limits`] with `Limits::default()`,
    /// which reports `max_compute_workgroup_size_x = 256`,
    /// `max_compute_workgroup_size_y = 256`, and
    /// `max_compute_invocations_per_workgroup = 256`.
    ///
    /// [`derive_from_limits`]: WorkgroupTuner::derive_from_limits
    pub fn unlimited() -> Self {
        Self {
            raster_2d: (16, 16),
            reduction: 256,
            fft: 64,
        }
    }
}

impl Default for WorkgroupTuner {
    fn default() -> Self {
        Self::unlimited()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits_with(max_x: u32, max_y: u32, max_total: u32) -> Limits {
        Limits {
            max_compute_workgroup_size_x: max_x,
            max_compute_workgroup_size_y: max_y,
            max_compute_invocations_per_workgroup: max_total,
            ..Limits::default()
        }
    }

    #[test]
    fn test_tuner_picks_16x16_when_limits_unbounded() {
        let tuner = WorkgroupTuner::derive_from_limits(&Limits::default());
        assert_eq!(tuner.raster_2d, (16, 16));
        assert_eq!(tuner.reduction, 256);
        assert_eq!(tuner.fft, 64);
    }

    #[test]
    fn test_tuner_falls_back_8x8_on_low_end_adapter() {
        // max_x=8, max_y=8, max_total=64 → can't fit 16×16=256, so use 8×8.
        let tuner = WorkgroupTuner::derive_from_limits(&limits_with(8, 8, 64));
        assert_eq!(tuner.raster_2d, (8, 8));
    }

    #[test]
    fn test_tuner_falls_back_4x4_on_minimum_adapter() {
        // Extremely constrained: max 4×4=16 total.
        let tuner = WorkgroupTuner::derive_from_limits(&limits_with(4, 4, 16));
        assert_eq!(tuner.raster_2d, (4, 4));
    }

    #[test]
    fn test_tuner_respects_max_invocations_per_workgroup() {
        // max_x=32, max_y=32, but max_total=64 → 16×16=256 > 64, so fall back.
        let tuner = WorkgroupTuner::derive_from_limits(&limits_with(32, 32, 64));
        assert_eq!(tuner.raster_2d, (8, 8));
    }

    #[test]
    fn test_tuner_reduction_capped_at_256() {
        let tuner = WorkgroupTuner::derive_from_limits(&Limits::default());
        assert!(tuner.reduction <= 256);
        // Also verify it doesn't exceed adapter limits.
        let constrained = limits_with(128, 128, 128);
        let tuner2 = WorkgroupTuner::derive_from_limits(&constrained);
        assert!(tuner2.reduction <= 128);
    }

    #[test]
    fn test_tuner_unlimited_returns_max_defaults() {
        let tuner = WorkgroupTuner::unlimited();
        assert_eq!(tuner.raster_2d, (16, 16));
        assert_eq!(tuner.reduction, 256);
        assert_eq!(tuner.fft, 64);
    }

    #[test]
    fn test_tuner_default_equals_unlimited() {
        let a = WorkgroupTuner::default();
        let b = WorkgroupTuner::unlimited();
        assert_eq!(a, b);
    }

    #[test]
    fn test_tuner_reduction_respects_max_x_cap() {
        // max_x is the binding limit for 1D kernels.
        let tuner = WorkgroupTuner::derive_from_limits(&limits_with(64, 256, 256));
        assert!(tuner.reduction <= 64);
    }

    #[test]
    fn test_tuner_fft_capped_at_64() {
        let tuner = WorkgroupTuner::derive_from_limits(&Limits::default());
        assert!(tuner.fft <= 64);
    }

    #[test]
    fn test_tuner_fft_respects_low_adapter_limits() {
        let tuner = WorkgroupTuner::derive_from_limits(&limits_with(32, 32, 32));
        assert!(tuner.fft <= 32);
        assert_eq!(tuner.fft, 32);
    }

    #[test]
    fn test_tuner_clone_and_copy() {
        let a = WorkgroupTuner::unlimited();
        let b = a; // Copy
        let c = a; // Copy again (a still valid)
        assert_eq!(b, c);
        assert_eq!(a, b);
    }

    #[test]
    fn test_tuner_debug_format() {
        let tuner = WorkgroupTuner::unlimited();
        let s = format!("{tuner:?}");
        assert!(s.contains("WorkgroupTuner"));
        assert!(s.contains("raster_2d"));
    }
}
