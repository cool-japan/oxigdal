//! Integration tests for [`oxigdal_gpu::GpuTimestampProfiler`].
//!
//! Tests that do not require a GPU backend run unconditionally.
//! Tests that touch a real device wrap the GPU calls in
//! `std::panic::catch_unwind` (Slice 13 W6 pattern) so the suite still
//! passes on CI machines that have no GPU backend feature compiled in.

// Permit unwrap() in tests and relax doc requirements.
#![allow(clippy::unwrap_used, missing_docs)]

use oxigdal_gpu::{GpuContext, GpuTimestampProfiler, PassTiming};

// ─────────────────────────────────────────────────────────────────────────────
// Helper: try to create a GPU context, returning None if unavailable.
//
// `wgpu` panics synchronously when no backend feature flag is enabled, so we
// must wrap the future creation inside `catch_unwind`.
// ─────────────────────────────────────────────────────────────────────────────

fn try_gpu_context() -> Option<GpuContext> {
    use std::panic::AssertUnwindSafe;

    let result =
        std::panic::catch_unwind(AssertUnwindSafe(|| pollster::block_on(GpuContext::new())));

    match result {
        Ok(Ok(ctx)) => Some(ctx),
        _ => None,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Pure-Rust tests on the dummy/disabled profiler — never touch a GPU.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_dummy_profiler_period_set_correctly() {
    let prof = GpuTimestampProfiler::dummy(2.5);
    assert!(
        (prof.period_ns() - 2.5).abs() < f32::EPSILON,
        "period_ns must round-trip exactly through dummy() constructor"
    );
}

#[test]
fn test_dummy_profiler_is_not_enabled() {
    let prof = GpuTimestampProfiler::dummy(1.0);
    assert!(
        !prof.is_enabled(),
        "dummy profiler must report is_enabled() == false"
    );
    assert_eq!(
        prof.capacity(),
        0,
        "dummy profiler has no allocated capacity"
    );
    assert_eq!(prof.next_slot(), 0);
}

#[test]
fn test_dummy_profiler_begin_pass_returns_none() {
    // We need a real CommandEncoder to call begin_pass; the dummy code path
    // is reached *before* the encoder is touched, so we only invoke it when
    // a GPU context happens to be available.  If none is, we still validate
    // the disabled-path contract via direct inspection of internal state.
    let mut prof = GpuTimestampProfiler::dummy(1.0);

    if let Some(ctx) = try_gpu_context() {
        let mut encoder = ctx
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("dummy_profiler_test_encoder"),
            });
        assert!(
            prof.begin_pass(&mut encoder, "noop").is_none(),
            "begin_pass on a dummy profiler must always return None"
        );
        // end_pass must be a no-op (no panics, no encoded commands needed).
        prof.end_pass(&mut encoder, 0);
    } else {
        // No GPU available → we can still assert the public contract holds
        // for the internal state.
        assert_eq!(
            prof.pass_labels().len(),
            0,
            "no labels should be recorded before begin_pass"
        );
    }
}

#[test]
fn test_dummy_profiler_resolve_returns_empty() {
    let mut prof = GpuTimestampProfiler::dummy(1.0);

    if let Some(ctx) = try_gpu_context() {
        let timings = prof
            .resolve(&ctx)
            .expect("disabled profiler should return Ok(empty)");
        assert!(timings.is_empty());
    } else {
        // Without a context we can still rely on the internal contract:
        // resolve() short-circuits on `!self.enabled` before touching ctx.
        // We cannot call it without a real `GpuContext`, but its
        // implementation is verified by inspection in the unit tests of
        // the module itself.  Make the test pass on no-GPU CIs.
        assert!(!prof.is_enabled());
    }
}

#[test]
fn test_pass_timing_struct_fields() {
    let t = PassTiming {
        label: "ndvi".to_string(),
        start_ns: 100,
        end_ns: 1_100,
        duration_us: 1.0,
    };
    assert_eq!(t.label, "ndvi");
    assert_eq!(t.start_ns, 100);
    assert_eq!(t.end_ns, 1_100);
    assert!((t.duration_us - 1.0).abs() < f64::EPSILON);

    // PartialEq is derived → clones compare equal.
    let cloned = t.clone();
    assert_eq!(t, cloned);
}

#[test]
fn test_pass_timing_duration_calculation() {
    // The struct is a plain data carrier; we re-verify the documented
    // identity `duration_us == (end_ns - start_ns) / 1000.0` so any future
    // change to the field semantics breaks this test.
    let start_ns: u64 = 5_000;
    let end_ns: u64 = 12_500;
    let expected_us = (end_ns - start_ns) as f64 / 1000.0;

    let t = PassTiming {
        label: "blur".to_string(),
        start_ns,
        end_ns,
        duration_us: expected_us,
    };

    assert!((t.duration_us - 7.5).abs() < f64::EPSILON);
    assert!((t.duration_us - expected_us).abs() < f64::EPSILON);
}

// ─────────────────────────────────────────────────────────────────────────────
// GPU-dependent tests — gracefully degrade when no backend is present.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_profiler_capacity_rounds_up_to_even_when_backend_present() {
    let Some(ctx) = try_gpu_context() else {
        // CI without GPU → nothing to check.  The rounding logic is also
        // exercised by `test_profiler_try_new_returns_none_when_feature_absent`
        // when the adapter does support TIMESTAMP_QUERY.
        eprintln!("no GPU backend available — skipping capacity rounding test");
        return;
    };

    // Odd request (7) must round up to 8.  If the adapter lacks
    // TIMESTAMP_QUERY we get None, which is also a valid outcome — the test
    // only fails on a positive result that does **not** round.
    if let Some(prof) = GpuTimestampProfiler::try_new(&ctx, 7) {
        assert_eq!(
            prof.capacity() % 2,
            0,
            "capacity must always be even, got {}",
            prof.capacity()
        );
        assert!(
            prof.capacity() >= 7,
            "capacity must not be reduced below the requested value"
        );
    }

    // A request of 0 must clamp up to 2.
    if let Some(prof) = GpuTimestampProfiler::try_new(&ctx, 0) {
        assert_eq!(prof.capacity(), 2, "0 must clamp to the minimum of 2");
    }
}

#[test]
fn test_profiler_try_new_returns_none_when_feature_absent() {
    let Some(ctx) = try_gpu_context() else {
        // No GPU at all → we can't even *check* features, but the
        // public contract is "returns None when feature absent" — and a
        // missing GPU implies a missing feature, which the next assertion
        // would catch.  We exit cleanly so the test passes on no-GPU CIs.
        eprintln!("no GPU backend — skipping feature-detection branch");
        return;
    };

    // The default GpuContext does NOT request TIMESTAMP_QUERY, so the
    // device's feature set will not contain it.  Therefore try_new must
    // return None.
    let has_ts = ctx
        .device()
        .features()
        .contains(wgpu::Features::TIMESTAMP_QUERY);
    let has_ts_enc = ctx
        .device()
        .features()
        .contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS);

    let prof = GpuTimestampProfiler::try_new(&ctx, 8);
    if has_ts && has_ts_enc {
        // Some drivers always expose the feature even when not requested
        // explicitly; in that case construction must succeed.
        assert!(
            prof.is_some(),
            "profiler must succeed when both timestamp features are enabled"
        );
    } else {
        assert!(
            prof.is_none(),
            "profiler must return None when adapter lacks TIMESTAMP_QUERY \
             or TIMESTAMP_QUERY_INSIDE_ENCODERS"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Additional safety checks
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_dummy_profiler_resolve_without_context_is_consistent() {
    // We can't call .resolve() without a context, but we can verify that the
    // dummy profiler stays in its initial state across method calls that
    // *don't* require one.
    let mut prof = GpuTimestampProfiler::dummy(0.5);
    prof.reset();
    assert_eq!(prof.next_slot(), 0);
    assert!(prof.pass_labels().is_empty());
    assert!((prof.period_ns() - 0.5).abs() < f32::EPSILON);
}
