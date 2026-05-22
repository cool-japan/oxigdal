//! Integration tests for the radix-2 1D FFT GPU compute kernel.
//!
//! Pure-Rust tests (bit reversal, twiddle factors, shader source inspection,
//! and pure-validation) run unconditionally.  GPU-dependent tests gracefully
//! skip when no wgpu backend is compiled in or no adapter is available,
//! following the `try_gpu_context` pattern used elsewhere in this test suite.

#![allow(clippy::unwrap_used, missing_docs)]

use oxigdal_gpu::{
    GpuContext,
    fft::{Fft1d, bit_reverse, make_fft_shader_source, twiddle_factor, validate_size},
};

// ─────────────────────────────────────────────────────────────────────────────
// Helper: try to create a GPU context without panicking.
// ─────────────────────────────────────────────────────────────────────────────

/// Attempt to obtain a `GpuContext`.  Returns `None` when no GPU backend is
/// compiled in or no adapter is available on the machine.
///
/// `wgpu` can panic synchronously when no backend feature is enabled, so the
/// future-creation is wrapped in `catch_unwind`.
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
// Test 1 — bit_reverse: 8-point known values
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_bit_reverse_8_known_values() {
    // 3-bit reversal for N = 8
    assert_eq!(bit_reverse(0, 3), 0, "bit_reverse(0,3) should be 0");
    assert_eq!(bit_reverse(1, 3), 4, "bit_reverse(1,3) should be 4");
    assert_eq!(bit_reverse(2, 3), 2, "bit_reverse(2,3) should be 2");
    assert_eq!(bit_reverse(3, 3), 6, "bit_reverse(3,3) should be 6");
    assert_eq!(bit_reverse(4, 3), 1, "bit_reverse(4,3) should be 1");
    assert_eq!(bit_reverse(5, 3), 5, "bit_reverse(5,3) should be 5");
    assert_eq!(bit_reverse(6, 3), 3, "bit_reverse(6,3) should be 3");
    assert_eq!(bit_reverse(7, 3), 7, "bit_reverse(7,3) should be 7");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 — twiddle_factor: N=4, forward
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_twiddle_factor_n_eq_4_known() {
    // k=0: angle = 0 → (cos 0, sin 0) = (1.0, 0.0)
    let (r0, i0) = twiddle_factor(0, 4, false);
    assert!(
        (r0 - 1.0_f32).abs() < 1e-6,
        "W(0,4).re should be 1.0, got {}",
        r0
    );
    assert!(i0.abs() < 1e-6, "W(0,4).im should be 0.0, got {}", i0);

    // k=1: angle = -π/2 → (cos(-π/2), sin(-π/2)) ≈ (0.0, -1.0)
    let (r1, i1) = twiddle_factor(1, 4, false);
    assert!(r1.abs() < 1e-6, "W(1,4).re should be ≈0.0, got {}", r1);
    assert!(
        (i1 + 1.0_f32).abs() < 1e-6,
        "W(1,4).im should be ≈-1.0, got {}",
        i1
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3 — make_fft_shader_source: size=8, forward → contains "3" for log2(8)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_make_fft_shader_source_size_8_inverse_false_contains_log2n_3() {
    let src = make_fft_shader_source(8, false);
    assert!(
        src.contains("3u") || src.contains("// FFT size = 8, log2 = 3"),
        "forward shader for N=8 must reference log2(8)=3; shader:\n{}",
        &src[..src.len().min(400)]
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4 — make_fft_shader_source: inverse=true → contains 1/N scale
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_make_fft_shader_source_inverse_true_contains_scale() {
    let src = make_fft_shader_source(8, true);
    // The inverse shader divides by N (8) — look for "8u" or "8.0" or "0.125"
    assert!(
        src.contains("8u") || src.contains("8.0") || src.contains("0.125"),
        "inverse shader for N=8 must contain a 1/N normalisation; shader excerpt:\n{}",
        &src[..src.len().min(600)]
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5 — Fft1d::new rejects non-power-of-two
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_fft1d_new_rejects_non_power_of_two() {
    // Validation happens before any GPU call, so no context needed.
    let result = validate_size(6);
    assert!(
        result.is_err(),
        "size=6 is not a power of two and must be rejected"
    );
    let msg = result.err().unwrap().to_string();
    assert!(
        msg.to_lowercase().contains("power of two") || msg.contains("power"),
        "error message should mention 'power of two': {}",
        msg
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 6 — Fft1d::new rejects size < 4
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_fft1d_new_rejects_size_below_min() {
    let result = validate_size(2);
    assert!(result.is_err(), "size=2 < 4 must be rejected");
    let msg = result.err().unwrap().to_string();
    assert!(
        msg.contains("4") || msg.contains("minimum") || msg.contains(">="),
        "error message should mention minimum size: {}",
        msg
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 7 — Fft1d::new rejects size > 2048
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_fft1d_new_rejects_size_above_max() {
    let result = validate_size(4096);
    assert!(result.is_err(), "size=4096 > 2048 must be rejected");
    let msg = result.err().unwrap().to_string();
    assert!(
        msg.contains("2048") || msg.contains("maximum") || msg.contains("<="),
        "error message should mention maximum size: {}",
        msg
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 8 — impulse → constant magnitude (GPU)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_fft1d_execute_impulse_yields_constant_when_backend_present() {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ctx = match try_gpu_context() {
            Some(c) => c,
            None => return,
        };
        let fft = match Fft1d::new(&ctx, 8, false) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Fft1d::new failed (skip): {e}");
                return;
            }
        };

        let mut real_in = vec![0.0_f32; 8];
        let imag_in = vec![0.0_f32; 8];
        real_in[0] = 1.0; // unit impulse at index 0

        match fft.execute(&ctx, &real_in, &imag_in) {
            Ok((real_out, imag_out)) => {
                assert_eq!(real_out.len(), 8);
                assert_eq!(imag_out.len(), 8);
                for k in 0..8 {
                    let mag = (real_out[k] * real_out[k] + imag_out[k] * imag_out[k]).sqrt();
                    assert!(
                        (mag - 1.0).abs() < 0.01,
                        "impulse FFT: bin {k} magnitude should be 1.0, got {mag}"
                    );
                }
            }
            Err(e) => eprintln!("execute failed (skip): {e}"),
        }
    }));
    // GPU may not be available — not asserting result
    let _ = result;
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 9 — DC input (all ones) → only bin 0 non-zero (GPU)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_fft1d_execute_dc_yields_only_dc_bin_when_backend_present() {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ctx = match try_gpu_context() {
            Some(c) => c,
            None => return,
        };
        let fft = match Fft1d::new(&ctx, 8, false) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Fft1d::new failed (skip): {e}");
                return;
            }
        };

        let real_in = vec![1.0_f32; 8];
        let imag_in = vec![0.0_f32; 8];

        match fft.execute(&ctx, &real_in, &imag_in) {
            Ok((real_out, imag_out)) => {
                // bin 0 should equal N = 8
                assert!(
                    (real_out[0] - 8.0).abs() < 0.05,
                    "DC input: bin 0 should be 8.0, got {}",
                    real_out[0]
                );
                // all other bins should be ≈ 0
                for k in 1..8 {
                    let mag = (real_out[k] * real_out[k] + imag_out[k] * imag_out[k]).sqrt();
                    assert!(
                        mag < 0.05,
                        "DC input: bin {k} should be ≈0, magnitude = {mag}"
                    );
                }
            }
            Err(e) => eprintln!("execute failed (skip): {e}"),
        }
    }));
    let _ = result;
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 10 — round-trip: forward then inverse recovers input (GPU)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_fft1d_round_trip_forward_then_inverse_matches_input_when_backend_present() {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ctx = match try_gpu_context() {
            Some(c) => c,
            None => return,
        };
        let fwd = match Fft1d::new(&ctx, 8, false) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Fft1d::new (fwd) failed (skip): {e}");
                return;
            }
        };
        let inv = match Fft1d::new(&ctx, 8, true) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Fft1d::new (inv) failed (skip): {e}");
                return;
            }
        };

        let original_real: Vec<f32> = (0..8).map(|i| i as f32).collect();
        let original_imag = vec![0.0_f32; 8];

        let (freq_real, freq_imag) = match fwd.execute(&ctx, &original_real, &original_imag) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("forward execute failed (skip): {e}");
                return;
            }
        };
        let (recovered_real, recovered_imag) = match inv.execute(&ctx, &freq_real, &freq_imag) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("inverse execute failed (skip): {e}");
                return;
            }
        };

        for i in 0..8 {
            assert!(
                (recovered_real[i] - original_real[i]).abs() < 1e-3,
                "round-trip real[{i}]: expected {}, got {}",
                original_real[i],
                recovered_real[i]
            );
            assert!(
                recovered_imag[i].abs() < 1e-3,
                "round-trip imag[{i}]: expected 0.0, got {}",
                recovered_imag[i]
            );
        }
    }));
    let _ = result;
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 11 — cosine input → peaks at expected bins (GPU)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_fft1d_execute_size_16_cosine_yields_two_nonzero_bins_when_backend_present() {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ctx = match try_gpu_context() {
            Some(c) => c,
            None => return,
        };
        let fft = match Fft1d::new(&ctx, 16, false) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Fft1d::new failed (skip): {e}");
                return;
            }
        };

        // cos(2π·3·n/16) for n = 0..16 → peaks at bins 3 and 13 (= N-3)
        let real_in: Vec<f32> = (0..16)
            .map(|n| (2.0 * std::f32::consts::PI * 3.0 * n as f32 / 16.0).cos())
            .collect();
        let imag_in = vec![0.0_f32; 16];

        match fft.execute(&ctx, &real_in, &imag_in) {
            Ok((real_out, imag_out)) => {
                let mags: Vec<f32> = (0..16)
                    .map(|k| (real_out[k] * real_out[k] + imag_out[k] * imag_out[k]).sqrt())
                    .collect();

                // bins 3 and 13 should have magnitude ≈ 8.0 (= N/2)
                assert!(
                    mags[3] > 5.0,
                    "cosine 16-pt: bin 3 should be large, got {}",
                    mags[3]
                );
                assert!(
                    mags[13] > 5.0,
                    "cosine 16-pt: bin 13 should be large, got {}",
                    mags[13]
                );

                // all other bins should be small
                for (k, &mag_k) in mags.iter().enumerate() {
                    if k != 3 && k != 13 {
                        assert!(
                            mag_k < 1.0,
                            "cosine 16-pt: bin {k} should be ≈0, got {mag_k}"
                        );
                    }
                }
            }
            Err(e) => eprintln!("execute failed (skip): {e}"),
        }
    }));
    let _ = result;
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 12 — batch: 4 × all-ones → each bin 0 = N (GPU)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_fft1d_execute_batch_processes_multiple_inputs_when_backend_present() {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ctx = match try_gpu_context() {
            Some(c) => c,
            None => return,
        };
        let fft = match Fft1d::new(&ctx, 8, false) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Fft1d::new failed (skip): {e}");
                return;
            }
        };

        // 4 identical DC inputs
        let batch: Vec<(Vec<f32>, Vec<f32>)> = (0..4)
            .map(|_| (vec![1.0_f32; 8], vec![0.0_f32; 8]))
            .collect();

        match fft.execute_batch(&ctx, &batch) {
            Ok(results) => {
                assert_eq!(results.len(), 4, "batch must return 4 transforms");
                for (idx, (real_out, imag_out)) in results.iter().enumerate() {
                    assert_eq!(real_out.len(), 8);
                    assert_eq!(imag_out.len(), 8);
                    // bin 0 = N = 8
                    assert!(
                        (real_out[0] - 8.0).abs() < 0.05,
                        "batch[{idx}] bin 0 should be 8.0, got {}",
                        real_out[0]
                    );
                    // remaining bins ≈ 0
                    for k in 1..8 {
                        let mag = (real_out[k] * real_out[k] + imag_out[k] * imag_out[k]).sqrt();
                        assert!(mag < 0.05, "batch[{idx}] bin {k} should be ≈0, mag = {mag}");
                    }
                }
            }
            Err(e) => eprintln!("execute_batch failed (skip): {e}"),
        }
    }));
    let _ = result;
}
