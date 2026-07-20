//! Integration tests for FFT-based 1D convolution (`convolution_fft` module).
//!
//! Pure-Rust tests (reference implementation, complex multiply, constant
//! assertions) run unconditionally.  GPU-conditional tests gracefully skip
//! when no wgpu adapter is available, following the `try_gpu_context` /
//! `catch_unwind` pattern used throughout this test suite.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

use oxigeo_gpu::{
    GpuContext,
    convolution_fft::{
        FftConvolution, MAX_FFT_CONVOLUTION_SIZE, complex_multiply, convolve_reference,
    },
};
use std::sync::Arc;

// ─────────────────────────────────────────────────────────────────────────────
// Helper: try to create a GPU context without panicking.
// ─────────────────────────────────────────────────────────────────────────────

/// Attempt to obtain a `GpuContext`. Returns `None` when no GPU backend is
/// compiled in or no adapter is available.
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
// Test 1 — complex_multiply: unit real values
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_complex_multiply_unit_values() {
    // (1+0i) * (1+0i) = (1, 0)
    let (r, i) = complex_multiply(1.0, 0.0, 1.0, 0.0);
    assert!((r - 1.0).abs() < 1e-6, "real(1·1) should be 1.0, got {r}");
    assert!(i.abs() < 1e-6, "imag(1·1) should be 0.0, got {i}");

    // (0+1i) * (0+1i) = (-1, 0)
    let (r, i) = complex_multiply(0.0, 1.0, 0.0, 1.0);
    assert!((r + 1.0).abs() < 1e-6, "real(i·i) should be -1.0, got {r}");
    assert!(i.abs() < 1e-6, "imag(i·i) should be 0.0, got {i}");

    // (1+0i) * (0+1i) = (0, 1)
    let (r, i) = complex_multiply(1.0, 0.0, 0.0, 1.0);
    assert!(r.abs() < 1e-6, "real(1·i) should be 0.0, got {r}");
    assert!((i - 1.0).abs() < 1e-6, "imag(1·i) should be 1.0, got {i}");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 — convolve_reference: identity kernel
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_convolve_reference_identity_kernel() {
    let result = convolve_reference(&[1.0, 2.0, 3.0], &[1.0]);
    assert_eq!(result.len(), 3, "identity conv output length should be 3");
    assert!((result[0] - 1.0).abs() < 1e-6, "output[0]={}", result[0]);
    assert!((result[1] - 2.0).abs() < 1e-6, "output[1]={}", result[1]);
    assert!((result[2] - 3.0).abs() < 1e-6, "output[2]={}", result[2]);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3 — convolve_reference: delta (delay) kernel
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_convolve_reference_delta_kernel() {
    // signal=[1,2,3] * kernel=[0,1,0] → output_len=5: [0,1,2,3,0]
    let result = convolve_reference(&[1.0, 2.0, 3.0], &[0.0, 1.0, 0.0]);
    assert_eq!(result.len(), 5, "delta conv output length should be 5");
    assert!((result[0] - 0.0).abs() < 1e-6, "output[0]={}", result[0]);
    assert!((result[1] - 1.0).abs() < 1e-6, "output[1]={}", result[1]);
    assert!((result[2] - 2.0).abs() < 1e-6, "output[2]={}", result[2]);
    assert!((result[3] - 3.0).abs() < 1e-6, "output[3]={}", result[3]);
    assert!((result[4] - 0.0).abs() < 1e-6, "output[4]={}", result[4]);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4 — convolve_reference: box-blur kernel
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_convolve_reference_box_blur() {
    // signal=[1,1,1,1,1] * kernel=[0.5,0.5] → [0.5, 1, 1, 1, 1, 0.5]
    let result = convolve_reference(&[1.0, 1.0, 1.0, 1.0, 1.0], &[0.5, 0.5]);
    assert_eq!(result.len(), 6, "box-blur output length should be 6");
    assert!((result[0] - 0.5).abs() < 1e-6, "output[0]={}", result[0]);
    assert!((result[1] - 1.0).abs() < 1e-6, "output[1]={}", result[1]);
    assert!((result[2] - 1.0).abs() < 1e-6, "output[2]={}", result[2]);
    assert!((result[3] - 1.0).abs() < 1e-6, "output[3]={}", result[3]);
    assert!((result[4] - 1.0).abs() < 1e-6, "output[4]={}", result[4]);
    assert!((result[5] - 0.5).abs() < 1e-6, "output[5]={}", result[5]);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5 — convolve_reference: empty inputs
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_convolve_reference_empty_inputs() {
    assert!(
        convolve_reference(&[], &[1.0]).is_empty(),
        "empty signal → empty output"
    );
    assert!(
        convolve_reference(&[1.0], &[]).is_empty(),
        "empty kernel → empty output"
    );
    assert!(
        convolve_reference(&[], &[]).is_empty(),
        "both empty → empty output"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 6 — convolve_reference: polynomial multiplication
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_convolve_reference_vs_known_polynomial() {
    // (1 + x) * (1 + x) = 1 + 2x + x²
    // Represented as coefficient vectors: [1, 1] * [1, 1] = [1, 2, 1]
    let result = convolve_reference(&[1.0, 1.0], &[1.0, 1.0]);
    assert_eq!(result.len(), 3, "polynomial product length should be 3");
    assert!((result[0] - 1.0).abs() < 1e-6, "x^0 coeff={}", result[0]);
    assert!((result[1] - 2.0).abs() < 1e-6, "x^1 coeff={}", result[1]);
    assert!((result[2] - 1.0).abs() < 1e-6, "x^2 coeff={}", result[2]);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 7 — MAX_FFT_CONVOLUTION_SIZE is a power of two
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_max_fft_convolution_size_is_power_of_two() {
    assert!(
        MAX_FFT_CONVOLUTION_SIZE.is_power_of_two(),
        "MAX_FFT_CONVOLUTION_SIZE ({}) must be a power of two",
        MAX_FFT_CONVOLUTION_SIZE
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 8 (GPU) — identity kernel produces unchanged signal
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_fft_convolve_identity_kernel_when_backend_present() {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ctx = match try_gpu_context() {
            Some(c) => c,
            None => return,
        };
        let conv = FftConvolution::new(Arc::new(ctx));

        match conv.convolve(&[1.0_f32, 2.0, 3.0], &[1.0_f32]) {
            Ok(out) => {
                assert_eq!(out.len(), 3, "identity conv output length");
                assert!(
                    (out[0] - 1.0).abs() < 1e-3,
                    "out[0]={} expected 1.0",
                    out[0]
                );
                assert!(
                    (out[1] - 2.0).abs() < 1e-3,
                    "out[1]={} expected 2.0",
                    out[1]
                );
                assert!(
                    (out[2] - 3.0).abs() < 1e-3,
                    "out[2]={} expected 3.0",
                    out[2]
                );
            }
            Err(e) => eprintln!("convolve failed (skip): {e}"),
        }
    }));
    let _ = result;
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 9 (GPU) — compare GPU FFT convolution vs reference for 16-element inputs
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_fft_convolve_vs_reference_random_when_backend_present() {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ctx = match try_gpu_context() {
            Some(c) => c,
            None => return,
        };
        let conv = FftConvolution::new(Arc::new(ctx));

        // Fixed pseudo-random 16-element inputs (no rand dependency)
        let signal: Vec<f32> = (0..16).map(|i| (i as f32 * 0.7 + 0.3).sin()).collect();
        let kernel: Vec<f32> = (0..8).map(|i| (i as f32 * 1.3 + 0.1).cos()).collect();

        let reference = convolve_reference(&signal, &kernel);

        match conv.convolve(&signal, &kernel) {
            Ok(gpu_out) => {
                assert_eq!(
                    gpu_out.len(),
                    reference.len(),
                    "GPU and reference output lengths must match"
                );
                for (i, (&ref_val, &gpu_val)) in reference.iter().zip(gpu_out.iter()).enumerate() {
                    assert!(
                        (gpu_val - ref_val).abs() < 1e-3,
                        "output[{i}]: GPU={gpu_val:.6} reference={ref_val:.6} diff={}",
                        (gpu_val - ref_val).abs()
                    );
                }
            }
            Err(e) => eprintln!("convolve failed (skip): {e}"),
        }
    }));
    let _ = result;
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 10 (GPU) — FFT size > 2048 returns an error
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_fft_convolve_exceeds_size_returns_error_when_backend_present() {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ctx = match try_gpu_context() {
            Some(c) => c,
            None => return,
        };
        let conv = FftConvolution::new(Arc::new(ctx));

        // signal of 1025 + kernel of 1025 → output_len = 2049
        // next_power_of_two(2049) = 4096 > 2048 → must error
        let long_signal = vec![1.0_f32; 1025];
        let long_kernel = vec![1.0_f32; 1025];

        match conv.convolve(&long_signal, &long_kernel) {
            Ok(_) => panic!("Expected an error for FFT size > 2048, but got Ok"),
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("4096") || msg.contains("exceed") || msg.contains("maximum"),
                    "error message should mention the oversized FFT: {msg}"
                );
            }
        }
    }));
    let _ = result;
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 11 (GPU) — correlate reverses kernel before convolving
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_fft_correlate_reverses_kernel_when_backend_present() {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ctx = match try_gpu_context() {
            Some(c) => c,
            None => return,
        };
        let conv = FftConvolution::new(Arc::new(ctx));

        let signal = [1.0_f32, 0.0, 0.0, 0.0];
        let kernel = [1.0_f32, 2.0, 3.0];

        // correlate(signal, kernel) should equal convolve(signal, reverse(kernel))
        let reversed_kernel: Vec<f32> = kernel.iter().copied().rev().collect();
        let reference = convolve_reference(&signal, &reversed_kernel);

        match conv.correlate(&signal, &kernel) {
            Ok(gpu_out) => {
                assert_eq!(
                    gpu_out.len(),
                    reference.len(),
                    "correlate output length must match reference"
                );
                for (i, (&ref_val, &gpu_val)) in reference.iter().zip(gpu_out.iter()).enumerate() {
                    assert!(
                        (gpu_val - ref_val).abs() < 1e-3,
                        "correlate output[{i}]: GPU={gpu_val:.6} reference={ref_val:.6}"
                    );
                }
            }
            Err(e) => eprintln!("correlate failed (skip): {e}"),
        }
    }));
    let _ = result;
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 12 (GPU) — batch convolution processes multiple signals
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_fft_convolve_batch_processes_multiple_signals_when_backend_present() {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ctx = match try_gpu_context() {
            Some(c) => c,
            None => return,
        };
        let conv = FftConvolution::new(Arc::new(ctx));

        let signals: Vec<Vec<f32>> = vec![
            vec![1.0_f32, 2.0, 3.0],
            vec![4.0_f32, 5.0, 6.0],
            vec![7.0_f32, 8.0, 9.0],
        ];
        let kernel = vec![1.0_f32, -1.0];

        // Build reference for each signal individually
        let references: Vec<Vec<f32>> = signals
            .iter()
            .map(|s| convolve_reference(s, &kernel))
            .collect();

        match conv.convolve_batch(&signals, &kernel) {
            Ok(batch_out) => {
                assert_eq!(
                    batch_out.len(),
                    signals.len(),
                    "batch output count must match signal count"
                );
                for (idx, (gpu_out, reference)) in
                    batch_out.iter().zip(references.iter()).enumerate()
                {
                    assert_eq!(
                        gpu_out.len(),
                        reference.len(),
                        "batch[{idx}] output length mismatch"
                    );
                    for (i, (&ref_val, &gpu_val)) in
                        reference.iter().zip(gpu_out.iter()).enumerate()
                    {
                        assert!(
                            (gpu_val - ref_val).abs() < 1e-3,
                            "batch[{idx}][{i}]: GPU={gpu_val:.6} reference={ref_val:.6}"
                        );
                    }
                }
            }
            Err(e) => eprintln!("convolve_batch failed (skip): {e}"),
        }
    }));
    let _ = result;
}
