//! CPU-only unit tests for GPU device-lost recovery machinery.
//!
//! These tests exercise the `Arc<AtomicBool>` flag mechanism and the
//! `GpuError::DeviceLost` variant without requiring a real GPU device.
//! They are designed to always pass on any host (including CI without a GPU).

#[cfg(test)]
mod tests {
    use crate::error::GpuError;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    /// Verifies that a freshly-created `AtomicBool` flag starts as `false`.
    ///
    /// This mirrors the initial state of `GpuContext::device_lost` before any
    /// device-lost callback fires.
    #[test]
    fn test_device_lost_flag_default_is_false() {
        let flag = Arc::new(AtomicBool::new(false));
        assert!(!flag.load(Ordering::SeqCst));
    }

    /// Verifies that the flag can be atomically set to `true`.
    ///
    /// This replicates what the wgpu device-lost callback does internally when
    /// the GPU hardware is reset or removed.
    #[test]
    fn test_device_lost_flag_set_to_true() {
        let flag = Arc::new(AtomicBool::new(false));
        flag.store(true, Ordering::SeqCst);
        assert!(flag.load(Ordering::SeqCst));
    }

    /// Verifies that `Arc::clone` shares ownership of the same underlying bool.
    ///
    /// The `GpuContext` registers a clone of `device_lost` with the wgpu
    /// callback closure.  This test confirms that a write through the clone is
    /// visible through the original, which is the invariant that makes the
    /// callback mechanism correct.
    #[test]
    fn test_device_lost_flag_clone_shares_state() {
        let flag = Arc::new(AtomicBool::new(false));
        let cloned = Arc::clone(&flag);
        // Simulate callback writing through its cloned handle.
        cloned.store(true, Ordering::SeqCst);
        // The original — held by GpuContext — must observe the update.
        assert!(flag.load(Ordering::SeqCst));
    }

    /// Verifies that `GpuError::device_lost` constructs the `DeviceLost` variant.
    ///
    /// `check_device_lost` returns `Err(GpuError::device_lost(...))` when the
    /// flag is set.  This test ensures the constructor produces the expected
    /// enum variant independently of any GPU context.
    #[test]
    fn test_check_device_lost_returns_err_when_flag_set() {
        let err = GpuError::device_lost("hardware reset");
        assert!(matches!(err, GpuError::DeviceLost { .. }));
    }

    /// Verifies that the `Display` impl for `GpuError::DeviceLost` includes the
    /// reason string in its output.
    ///
    /// The formatted error message is what end-users and log subscribers see.
    /// Ensuring the reason is not silently dropped is a basic sanity check.
    #[test]
    fn test_device_lost_error_formats_as_string() {
        let err = GpuError::device_lost("TDR timeout");
        let s = format!("{err}");
        // The formatted message must be non-empty and contain the reason.
        assert!(!s.is_empty());
        assert!(s.contains("TDR timeout"), "expected reason in '{s}'");
    }
}
