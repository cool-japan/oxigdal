//! FFT-based 1D linear convolution for OxiGeo GPU.
//!
//! Implements linear convolution via the FFT theorem:
//!
//!   `Y = IFFT(FFT(signal) * FFT(kernel))`
//!
//! where `*` is pointwise complex multiplication.  This is equivalent to
//! direct convolution for finite-length sequences, and is efficient when
//! both sequences are padded to the same power-of-two length.
//!
//! # Limitation
//!
//! The underlying [`Fft1d`] kernel supports sizes up to 2048.  For inputs
//! where `(signal.len() + kernel.len() - 1).next_power_of_two() > 2048` this
//! module returns [`GpuError::InvalidKernelParams`].  Long signals require an
//! Overlap-Save or Overlap-Add segmentation strategy (not implemented here).
//!
//! # Sign convention
//!
//! Forward FFT: twiddle angle = `-2π·k/N` (no normalisation).
//! Inverse FFT: twiddle angle = `+2π·k/N`, result divided by `N`.
//!
//! # Thread safety
//!
//! [`FftConvolution`] holds an `Arc<GpuContext>` and is `Send + Sync`.

use crate::context::GpuContext;
use crate::error::{GpuError, GpuResult};
use crate::fft::Fft1d;
use std::sync::Arc;
use tracing::debug;

// ─────────────────────────────────────────────────────────────────────────────
// Public constant
// ─────────────────────────────────────────────────────────────────────────────

/// Maximum single-pass FFT size supported by [`FftConvolution`].
///
/// This is determined by the maximum size accepted by [`Fft1d`] (2048).
/// Convolution of two sequences whose combined output length exceeds this
/// value (after rounding up to the next power of two) will return an error.
pub const MAX_FFT_CONVOLUTION_SIZE: usize = 2048;

// ─────────────────────────────────────────────────────────────────────────────
// Pure-Rust helpers (no GPU required)
// ─────────────────────────────────────────────────────────────────────────────

/// Pointwise complex multiplication: `(ar + i·ai) × (br + i·bi)`.
///
/// Returns `(real, imag)` where:
/// - `real = ar·br − ai·bi`
/// - `imag = ar·bi + ai·br`
///
/// # Examples
///
/// ```
/// use oxigeo_gpu::convolution_fft::complex_multiply;
///
/// // (1+0i) * (1+0i) = 1
/// let (r, i) = complex_multiply(1.0, 0.0, 1.0, 0.0);
/// assert!((r - 1.0).abs() < 1e-6);
/// assert!(i.abs() < 1e-6);
///
/// // (0+1i) * (0+1i) = -1
/// let (r, i) = complex_multiply(0.0, 1.0, 0.0, 1.0);
/// assert!((r + 1.0).abs() < 1e-6);
/// assert!(i.abs() < 1e-6);
/// ```
#[inline]
pub fn complex_multiply(ar: f32, ai: f32, br: f32, bi: f32) -> (f32, f32) {
    (ar * br - ai * bi, ar * bi + ai * br)
}

/// Pure-Rust reference linear convolution (direct O(N·M) algorithm).
///
/// Computes `output[i] = Σ_k signal[i−k] · kernel[k]` for a finite-support
/// linear (non-circular) convolution.
///
/// Output length is `signal.len() + kernel.len() - 1` when both are
/// non-empty, and zero when either is empty.
///
/// This function exists as a ground-truth reference for tests; it is not
/// optimised and should not be used on large inputs.
///
/// # Examples
///
/// ```
/// use oxigeo_gpu::convolution_fft::convolve_reference;
///
/// // Polynomial multiplication: (1+x) * (1+x) = 1 + 2x + x²
/// let result = convolve_reference(&[1.0, 1.0], &[1.0, 1.0]);
/// assert!((result[0] - 1.0).abs() < 1e-6);
/// assert!((result[1] - 2.0).abs() < 1e-6);
/// assert!((result[2] - 1.0).abs() < 1e-6);
/// ```
pub fn convolve_reference(signal: &[f32], kernel: &[f32]) -> Vec<f32> {
    if signal.is_empty() || kernel.is_empty() {
        return vec![];
    }
    let output_len = signal.len() + kernel.len() - 1;
    let mut output = vec![0.0_f32; output_len];
    for (i, &s) in signal.iter().enumerate() {
        for (j, &k) in kernel.iter().enumerate() {
            output[i + j] += s * k;
        }
    }
    output
}

// ─────────────────────────────────────────────────────────────────────────────
// FftConvolution struct
// ─────────────────────────────────────────────────────────────────────────────

/// GPU-accelerated FFT-based 1D linear convolution.
///
/// Internally uses two [`Fft1d`] dispatches (forward FFT of signal and kernel,
/// then inverse FFT of the pointwise product).  Complex multiplication is done
/// on the CPU because the buffer is small relative to the GPU dispatch overhead.
///
/// # Construction
///
/// ```rust,no_run
/// use std::sync::Arc;
/// use oxigeo_gpu::convolution_fft::FftConvolution;
/// use oxigeo_gpu::GpuContext;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let ctx = Arc::new(GpuContext::new().await?);
/// let conv = FftConvolution::new(Arc::clone(&ctx));
/// # Ok(())
/// # }
/// ```
pub struct FftConvolution {
    ctx: Arc<GpuContext>,
}

impl FftConvolution {
    /// Create a new [`FftConvolution`] bound to the given GPU context.
    pub fn new(ctx: Arc<GpuContext>) -> Self {
        Self { ctx }
    }

    // ── Core single-call convolution ─────────────────────────────────────────

    /// Compute the linear 1D convolution of `signal` with `kernel`.
    ///
    /// Output length: `signal.len() + kernel.len() - 1`.
    ///
    /// Both sequences are zero-padded to `fft_size = (output_len).next_power_of_two()`.
    /// Forward FFTs of each padded sequence are computed on the GPU, complex
    /// multiplication is performed on the CPU, and then the inverse FFT is
    /// computed on the GPU.  Only the first `output_len` samples of the IFFT
    /// result are returned (the remainder is circular aliasing that vanishes
    /// because of the zero-padding).
    ///
    /// # Errors
    ///
    /// - Returns [`GpuError::InvalidKernelParams`] when `fft_size > MAX_FFT_CONVOLUTION_SIZE`.
    /// - Returns a [`GpuError`] variant on any GPU pipeline or buffer error.
    pub fn convolve(&self, signal: &[f32], kernel: &[f32]) -> GpuResult<Vec<f32>> {
        if signal.is_empty() || kernel.is_empty() {
            return Ok(vec![]);
        }

        let output_len = signal.len() + kernel.len() - 1;

        // Choose FFT size: next power of two >= output_len
        let fft_size = output_len.next_power_of_two();

        if fft_size > MAX_FFT_CONVOLUTION_SIZE {
            return Err(GpuError::InvalidKernelParams {
                reason: format!(
                    "FFT convolution size {} exceeds maximum {}; \
                     use overlap-save for long signals",
                    fft_size, MAX_FFT_CONVOLUTION_SIZE
                ),
            });
        }

        let fft_size_u32 = fft_size as u32;

        // Guard: Fft1d requires size >= 4 and power of two.
        // When output_len < 4 the next_power_of_two may be 1 or 2.
        // Clamp fft_size_u32 upward to 4 in that case.
        let fft_size_u32 = fft_size_u32.max(4);
        let fft_size = fft_size_u32 as usize;

        debug!(
            "FftConvolution::convolve signal_len={} kernel_len={} output_len={} fft_size={}",
            signal.len(),
            kernel.len(),
            output_len,
            fft_size
        );

        // Zero-pad both inputs to fft_size
        let mut signal_padded = signal.to_vec();
        signal_padded.resize(fft_size, 0.0_f32);
        let signal_imag = vec![0.0_f32; fft_size];

        let mut kernel_padded = kernel.to_vec();
        kernel_padded.resize(fft_size, 0.0_f32);
        let kernel_imag = vec![0.0_f32; fft_size];

        // Forward FFT of signal
        let fft_fwd = Fft1d::new(&self.ctx, fft_size_u32, false)?;
        let (s_re, s_im) = fft_fwd.execute(&self.ctx, &signal_padded, &signal_imag)?;

        // Forward FFT of kernel (reuse the same Fft1d — same size, same direction)
        let (k_re, k_im) = fft_fwd.execute(&self.ctx, &kernel_padded, &kernel_imag)?;

        // Pointwise complex multiply (CPU — buffer is at most 2048 f32 each)
        let mut y_re = vec![0.0_f32; fft_size];
        let mut y_im = vec![0.0_f32; fft_size];
        for i in 0..fft_size {
            let (r, im) = complex_multiply(s_re[i], s_im[i], k_re[i], k_im[i]);
            y_re[i] = r;
            y_im[i] = im;
        }

        // Inverse FFT
        let fft_inv = Fft1d::new(&self.ctx, fft_size_u32, true)?;
        let (result_re, _result_im) = fft_inv.execute(&self.ctx, &y_re, &y_im)?;

        // Trim to output_len (the zero-pad region contains circular artefacts)
        Ok(result_re[..output_len].to_vec())
    }

    // ── Correlation ──────────────────────────────────────────────────────────

    /// Compute the linear 1D cross-correlation of `signal` with `kernel`.
    ///
    /// Cross-correlation is equivalent to convolution with a time-reversed
    /// kernel.  This method reverses `kernel` and delegates to [`Self::convolve`].
    ///
    /// Output length: `signal.len() + kernel.len() - 1` (same as convolution).
    ///
    /// # Errors
    ///
    /// Propagates any error from [`Self::convolve`].
    pub fn correlate(&self, signal: &[f32], kernel: &[f32]) -> GpuResult<Vec<f32>> {
        if signal.is_empty() || kernel.is_empty() {
            return Ok(vec![]);
        }
        // Time-reverse the kernel to convert correlation → convolution
        let reversed_kernel: Vec<f32> = kernel.iter().copied().rev().collect();
        self.convolve(signal, &reversed_kernel)
    }

    // ── Batch convolution ────────────────────────────────────────────────────

    /// Apply the same `kernel` to each signal in `signals`.
    ///
    /// This is a sequential loop over [`Self::convolve`].  A future
    /// optimisation could share the forward-FFT of the kernel across all
    /// signals; for now each signal gets an independent GPU dispatch.
    ///
    /// Returns a `Vec<Vec<f32>>` with one result per input signal, in the
    /// same order.
    ///
    /// # Errors
    ///
    /// Returns the first error encountered (processing stops on error).
    pub fn convolve_batch(&self, signals: &[Vec<f32>], kernel: &[f32]) -> GpuResult<Vec<Vec<f32>>> {
        let mut results = Vec::with_capacity(signals.len());
        for signal in signals {
            let output = self.convolve(signal, kernel)?;
            results.push(output);
        }
        Ok(results)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests (pure-Rust, no GPU required)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_complex_multiply_real_unit() {
        let (r, i) = complex_multiply(1.0, 0.0, 1.0, 0.0);
        assert!((r - 1.0).abs() < 1e-6, "real(1*1) should be 1, got {r}");
        assert!(i.abs() < 1e-6, "imag(1*1) should be 0, got {i}");
    }

    #[test]
    fn test_complex_multiply_imaginary_squared() {
        // (0+1i) * (0+1i) = -1
        let (r, i) = complex_multiply(0.0, 1.0, 0.0, 1.0);
        assert!((r + 1.0).abs() < 1e-6, "real(i²) should be -1, got {r}");
        assert!(i.abs() < 1e-6, "imag(i²) should be 0, got {i}");
    }

    #[test]
    fn test_complex_multiply_real_times_imaginary() {
        // (1+0i) * (0+1i) = i
        let (r, i) = complex_multiply(1.0, 0.0, 0.0, 1.0);
        assert!(r.abs() < 1e-6, "real(1*i) should be 0, got {r}");
        assert!((i - 1.0).abs() < 1e-6, "imag(1*i) should be 1, got {i}");
    }

    #[test]
    fn test_convolve_reference_identity_kernel() {
        let result = convolve_reference(&[1.0, 2.0, 3.0], &[1.0]);
        assert_eq!(result.len(), 3);
        assert!((result[0] - 1.0).abs() < 1e-6);
        assert!((result[1] - 2.0).abs() < 1e-6);
        assert!((result[2] - 3.0).abs() < 1e-6);
    }

    #[test]
    fn test_convolve_reference_delta_kernel() {
        // signal=[1,2,3] kernel=[0,1,0] → output_len=5: [0,1,2,3,0]
        let result = convolve_reference(&[1.0, 2.0, 3.0], &[0.0, 1.0, 0.0]);
        assert_eq!(result.len(), 5);
        assert!((result[0] - 0.0).abs() < 1e-6, "output[0]={}", result[0]);
        assert!((result[1] - 1.0).abs() < 1e-6, "output[1]={}", result[1]);
        assert!((result[2] - 2.0).abs() < 1e-6, "output[2]={}", result[2]);
        assert!((result[3] - 3.0).abs() < 1e-6, "output[3]={}", result[3]);
        assert!((result[4] - 0.0).abs() < 1e-6, "output[4]={}", result[4]);
    }

    #[test]
    fn test_convolve_reference_box_blur() {
        // signal=[1,1,1,1,1] kernel=[0.5,0.5] → [0.5,1,1,1,1,0.5]
        let result = convolve_reference(&[1.0, 1.0, 1.0, 1.0, 1.0], &[0.5, 0.5]);
        assert_eq!(result.len(), 6);
        assert!((result[0] - 0.5).abs() < 1e-6, "output[0]={}", result[0]);
        assert!((result[1] - 1.0).abs() < 1e-6, "output[1]={}", result[1]);
        assert!((result[2] - 1.0).abs() < 1e-6, "output[2]={}", result[2]);
        assert!((result[3] - 1.0).abs() < 1e-6, "output[3]={}", result[3]);
        assert!((result[4] - 1.0).abs() < 1e-6, "output[4]={}", result[4]);
        assert!((result[5] - 0.5).abs() < 1e-6, "output[5]={}", result[5]);
    }

    #[test]
    fn test_convolve_reference_empty_inputs() {
        assert!(convolve_reference(&[], &[1.0, 2.0]).is_empty());
        assert!(convolve_reference(&[1.0, 2.0], &[]).is_empty());
        assert!(convolve_reference(&[], &[]).is_empty());
    }

    #[test]
    fn test_convolve_reference_polynomial_multiplication() {
        // (1 + x) * (1 + x) = 1 + 2x + x²
        let result = convolve_reference(&[1.0, 1.0], &[1.0, 1.0]);
        assert_eq!(result.len(), 3);
        assert!((result[0] - 1.0).abs() < 1e-6, "coeff x^0 = {}", result[0]);
        assert!((result[1] - 2.0).abs() < 1e-6, "coeff x^1 = {}", result[1]);
        assert!((result[2] - 1.0).abs() < 1e-6, "coeff x^2 = {}", result[2]);
    }

    #[test]
    fn test_max_fft_convolution_size_is_power_of_two() {
        assert!(
            MAX_FFT_CONVOLUTION_SIZE.is_power_of_two(),
            "MAX_FFT_CONVOLUTION_SIZE ({}) must be a power of two",
            MAX_FFT_CONVOLUTION_SIZE
        );
    }
}
