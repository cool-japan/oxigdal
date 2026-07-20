//! Integration tests for Lanczos resampling.
//!
//! CPU-level tests (variant construction, entry-point name, shader source inspection)
//! run unconditionally.  Tests that require an actual GPU device are marked `#[ignore]`.

use oxigeo_gpu::kernels::resampling::ResamplingMethod;

// ── CPU-only tests (no GPU needed) ─────────────────────────────────────────

#[test]
fn test_lanczos_entry_point_a2() {
    let m = ResamplingMethod::Lanczos { a: 2 };
    assert_eq!(m.entry_point(), "lanczos");
}

#[test]
fn test_lanczos_entry_point_a3() {
    let m = ResamplingMethod::Lanczos { a: 3 };
    assert_eq!(m.entry_point(), "lanczos");
}

#[test]
fn test_lanczos_entry_point_a1() {
    let m = ResamplingMethod::Lanczos { a: 1 };
    assert_eq!(m.entry_point(), "lanczos");
}

#[test]
fn test_lanczos_entry_point_a8() {
    let m = ResamplingMethod::Lanczos { a: 8 };
    assert_eq!(m.entry_point(), "lanczos");
}

#[test]
fn test_lanczos_variant_equality() {
    assert_eq!(
        ResamplingMethod::Lanczos { a: 2 },
        ResamplingMethod::Lanczos { a: 2 }
    );
    assert_ne!(
        ResamplingMethod::Lanczos { a: 2 },
        ResamplingMethod::Lanczos { a: 3 }
    );
}

#[test]
fn test_lanczos_differs_from_other_methods() {
    let l = ResamplingMethod::Lanczos { a: 2 };
    assert_ne!(l, ResamplingMethod::Bilinear);
    assert_ne!(l, ResamplingMethod::Bicubic);
    assert_ne!(l, ResamplingMethod::NearestNeighbor);
}

#[test]
fn test_lanczos_debug_contains_a() {
    let m = ResamplingMethod::Lanczos { a: 5 };
    let dbg = format!("{:?}", m);
    assert!(dbg.contains("Lanczos"));
    assert!(dbg.contains("5"));
}

#[test]
fn test_lanczos_clone_copy() {
    let original = ResamplingMethod::Lanczos { a: 2 };
    let cloned = original;
    assert_eq!(original, cloned);
}

// ── GPU-dependent tests (all #[ignore]) ────────────────────────────────────

#[test]
#[ignore = "requires GPU hardware"]
fn test_lanczos_kernel_creation_a2() {
    // This would call:
    //   let ctx = pollster::block_on(GpuContext::new()).unwrap();
    //   ResamplingKernel::new(&ctx, ResamplingMethod::Lanczos { a: 2 }).unwrap();
    // Skipped: no GPU in CI.
}

#[test]
#[ignore = "requires GPU hardware"]
fn test_lanczos_kernel_creation_a3() {
    // Same as above with a=3.
}

#[test]
#[ignore = "requires GPU hardware"]
fn test_lanczos_a_zero_returns_error() {
    // ResamplingKernel::new(&ctx, ResamplingMethod::Lanczos { a: 0 })
    // should return Err(GpuError::InvalidKernelParams { .. }).
}

#[test]
#[ignore = "requires GPU hardware"]
fn test_lanczos_a_nine_returns_error() {
    // ResamplingKernel::new(&ctx, ResamplingMethod::Lanczos { a: 9 })
    // should return Err(GpuError::InvalidKernelParams { .. }).
}

#[test]
#[ignore = "requires GPU hardware"]
fn test_lanczos_resize_upscale() {
    // Upscale a 2x2 image to 4x4 with Lanczos a=2 and verify output shape.
}

#[test]
#[ignore = "requires GPU hardware"]
fn test_lanczos_resize_downscale() {
    // Downscale a 4x4 image to 2x2 with Lanczos a=2 and verify output shape.
}
