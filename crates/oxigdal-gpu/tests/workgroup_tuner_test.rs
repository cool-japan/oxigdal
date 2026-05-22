//! CPU-only tests for WorkgroupTuner — no wgpu device required.
//!
//! All assertions here are pure arithmetic on limit structs, so these tests
//! run on any machine regardless of GPU availability.

use oxigdal_gpu::WorkgroupTuner;
use wgpu::Limits;

/// Build a `Limits` that reports specific compute limits while keeping every
/// other field at its default value.
fn limits_with(max_x: u32, max_y: u32, max_total: u32) -> Limits {
    Limits {
        max_compute_workgroup_size_x: max_x,
        max_compute_workgroup_size_y: max_y,
        max_compute_invocations_per_workgroup: max_total,
        ..Limits::default()
    }
}

// ── raster_2d tier selection ────────────────────────────────────────────────

#[test]
fn test_tuner_picks_16x16_when_limits_unbounded() {
    // wgpu::Limits::default() → max_x=256, max_y=256, max_total=256.
    // 256 >= 256 so the tuner should pick (16, 16).
    let tuner = WorkgroupTuner::derive_from_limits(&Limits::default());
    assert_eq!(tuner.raster_2d, (16, 16));
    assert_eq!(tuner.reduction, 256);
    assert_eq!(tuner.fft, 64);
}

#[test]
fn test_tuner_falls_back_8x8_on_low_end_adapter() {
    // max_x=8, max_y=8, max_total=64 → 16×16=256 > 64, so use 8×8.
    let tuner = WorkgroupTuner::derive_from_limits(&limits_with(8, 8, 64));
    assert_eq!(tuner.raster_2d, (8, 8));
}

#[test]
fn test_tuner_falls_back_4x4_on_minimum_adapter() {
    // Extremely constrained: max total = 16, max_x=4, max_y=4.
    let tuner = WorkgroupTuner::derive_from_limits(&limits_with(4, 4, 16));
    assert_eq!(tuner.raster_2d, (4, 4));
}

#[test]
fn test_tuner_respects_max_invocations_per_workgroup() {
    // max_x=32, max_y=32, but max_total=64 → 16×16=256 > 64, so fall back.
    // 8×8=64 ≤ 64, so (8, 8) is selected.
    let tuner = WorkgroupTuner::derive_from_limits(&limits_with(32, 32, 64));
    assert_eq!(tuner.raster_2d, (8, 8));
}

#[test]
fn test_tuner_16x16_requires_both_axis_and_total() {
    // max_x=16, max_y=16, but max_total=64 → total invocations too low for 16×16=256.
    let tuner = WorkgroupTuner::derive_from_limits(&limits_with(16, 16, 64));
    assert_eq!(tuner.raster_2d, (8, 8));
}

// ── reduction and fft capping ──────────────────────────────────────────────

#[test]
fn test_tuner_reduction_capped_at_256() {
    let tuner = WorkgroupTuner::derive_from_limits(&Limits::default());
    assert!(tuner.reduction <= 256);
    // Also verify it doesn't exceed the explicit constrained adapter limits.
    let constrained = limits_with(128, 128, 128);
    let tuner2 = WorkgroupTuner::derive_from_limits(&constrained);
    assert!(tuner2.reduction <= 128);
}

#[test]
fn test_tuner_reduction_respects_max_x_cap() {
    // max_x is the binding axis for 1D compute kernels.
    let tuner = WorkgroupTuner::derive_from_limits(&limits_with(64, 256, 256));
    assert!(tuner.reduction <= 64);
    assert_eq!(tuner.reduction, 64);
}

#[test]
fn test_tuner_fft_capped_at_64() {
    let tuner = WorkgroupTuner::derive_from_limits(&Limits::default());
    assert!(tuner.fft <= 64);
    assert_eq!(tuner.fft, 64);
}

#[test]
fn test_tuner_fft_respects_low_adapter_limits() {
    let tuner = WorkgroupTuner::derive_from_limits(&limits_with(32, 32, 32));
    assert!(tuner.fft <= 32);
    assert_eq!(tuner.fft, 32);
}

// ── unlimited / default constructors ────────────────────────────────────────

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

// ── trait derivations ────────────────────────────────────────────────────────

#[test]
fn test_tuner_clone_and_copy() {
    let a = WorkgroupTuner::unlimited();
    let b = a; // Copy
    let c = a; // a still valid — Copy
    assert_eq!(b, c);
    assert_eq!(a, b);
}

#[test]
fn test_tuner_debug_format() {
    let tuner = WorkgroupTuner::unlimited();
    let s = format!("{tuner:?}");
    assert!(s.contains("WorkgroupTuner"));
    assert!(s.contains("raster_2d"));
    assert!(s.contains("reduction"));
    assert!(s.contains("fft"));
}

#[test]
fn test_tuner_equality_distinguishes_different_sizes() {
    let a = WorkgroupTuner::unlimited();
    let b = WorkgroupTuner::derive_from_limits(&limits_with(8, 8, 64));
    assert_ne!(a, b);
    assert_eq!(b.raster_2d, (8, 8));
}
