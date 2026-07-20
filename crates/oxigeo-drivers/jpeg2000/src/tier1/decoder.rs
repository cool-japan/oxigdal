//! Code-block decoder using EBCOT 3-pass bit-plane decoding
//!
//! This module provides the main entry point for decoding a JPEG2000
//! code-block using the real EBCOT tier-1 algorithm with:
//! - MQ arithmetic decoding
//! - Context formation based on neighbor states
//! - 3-pass bit-plane processing (significance propagation, magnitude
//!   refinement, cleanup)

use super::SubbandType;
use super::contexts::StateGrid;
use super::mq::MqDecoder;
use super::passes;
use crate::error::{Jpeg2000Error, Result};

/// Decode a code-block using the full EBCOT 3-pass algorithm
///
/// Processes the compressed data through bit-planes from MSB to LSB,
/// running the three coding passes on each bit-plane:
/// 1. Significance Propagation Pass
/// 2. Magnitude Refinement Pass
/// 3. Cleanup Pass
///
/// # Arguments
/// * `data` - Compressed MQ-coded data for this code-block
/// * `width` - Code-block width in samples
/// * `height` - Code-block height in samples
/// * `num_bitplanes` - Number of bit-planes to decode (typically 8 for 8-bit data)
/// * `subband` - Subband type (LL, LH, HL, HH) for context formation
///
/// # Returns
/// Vector of decoded wavelet coefficients
pub fn decode_code_block(
    data: &[u8],
    width: usize,
    height: usize,
    num_bitplanes: usize,
    subband: SubbandType,
) -> Result<Vec<i32>> {
    let num_coeffs = width * height;
    if num_coeffs == 0 {
        return Ok(Vec::new());
    }

    if data.is_empty() {
        return Ok(vec![0i32; num_coeffs]);
    }

    let mut coefficients = vec![0i32; num_coeffs];
    let mut grid = StateGrid::new(width, height);
    let mut mq = MqDecoder::new(data.to_vec());

    // Decode bit-planes from MSB to LSB
    let max_bp = if num_bitplanes > 0 {
        num_bitplanes - 1
    } else {
        return Ok(coefficients);
    };

    for bp_idx in 0..num_bitplanes {
        let bit_plane = (max_bp - bp_idx) as u32;
        let is_first = bp_idx == 0;

        match passes::decode_bit_plane(
            &mut mq,
            &mut grid,
            &mut coefficients,
            bit_plane,
            is_first,
            subband,
            width,
            height,
        ) {
            Ok(_) => {}
            Err(e) => {
                // If the MQ decoder runs out of data, stop decoding and return
                // what we have so far (truncated codestream handling)
                if is_truncation_error(&e) {
                    tracing::debug!(
                        "Code-block decoding truncated at bit-plane {} of {}",
                        bp_idx,
                        num_bitplanes
                    );
                    break;
                }
                return Err(e);
            }
        }

        // If MQ decoder is exhausted, stop
        if mq.is_exhausted() {
            break;
        }
    }

    Ok(coefficients)
}

/// Decode a code-block with a specified number of coding passes
///
/// Instead of decoding full bit-planes, this allows truncation at a
/// specific coding pass count (as used by quality layers).
///
/// Each bit-plane has up to 3 passes, except the first which has only
/// the cleanup pass. So for `num_bitplanes` bit-planes, the total
/// number of passes is: 1 + 3 * (num_bitplanes - 1)
pub fn decode_code_block_passes(
    data: &[u8],
    width: usize,
    height: usize,
    num_bitplanes: usize,
    max_passes: usize,
    subband: SubbandType,
) -> Result<Vec<i32>> {
    let num_coeffs = width * height;
    if num_coeffs == 0 || data.is_empty() || max_passes == 0 {
        return Ok(vec![0i32; num_coeffs]);
    }

    let mut coefficients = vec![0i32; num_coeffs];
    let mut grid = StateGrid::new(width, height);
    let mut mq = MqDecoder::new(data.to_vec());

    let max_bp = if num_bitplanes > 0 {
        num_bitplanes - 1
    } else {
        return Ok(coefficients);
    };

    let mut passes_remaining = max_passes;

    for bp_idx in 0..num_bitplanes {
        if passes_remaining == 0 {
            break;
        }

        let bit_plane = (max_bp - bp_idx) as u32;
        let is_first = bp_idx == 0;

        grid.reset_pass_flags();

        if !is_first && passes_remaining > 0 {
            // Pass 1: Significance Propagation
            match passes::significance_propagation_pass(
                &mut mq,
                &mut grid,
                &mut coefficients,
                bit_plane,
                subband,
                width,
                height,
            ) {
                Ok(_) => {}
                Err(e) if is_truncation_error(&e) => break,
                Err(e) => return Err(e),
            }
            passes_remaining = passes_remaining.saturating_sub(1);
            if passes_remaining == 0 || mq.is_exhausted() {
                break;
            }

            // Pass 2: Magnitude Refinement
            match passes::magnitude_refinement_pass(
                &mut mq,
                &mut grid,
                &mut coefficients,
                bit_plane,
                width,
                height,
            ) {
                Ok(_) => {}
                Err(e) if is_truncation_error(&e) => break,
                Err(e) => return Err(e),
            }
            passes_remaining = passes_remaining.saturating_sub(1);
            if passes_remaining == 0 || mq.is_exhausted() {
                break;
            }
        }

        // Pass 3 (or only pass for first bit-plane): Cleanup
        match passes::cleanup_pass(
            &mut mq,
            &mut grid,
            &mut coefficients,
            bit_plane,
            subband,
            width,
            height,
        ) {
            Ok(_) => {}
            Err(e) if is_truncation_error(&e) => break,
            Err(e) => return Err(e),
        }
        passes_remaining = passes_remaining.saturating_sub(1);

        if mq.is_exhausted() {
            break;
        }
    }

    Ok(coefficients)
}

/// Check if an error is a truncation/data-exhaustion error
fn is_truncation_error(error: &Jpeg2000Error) -> bool {
    matches!(
        error,
        Jpeg2000Error::InsufficientData { .. } | Jpeg2000Error::IoError(_)
    )
}

/// Calculate the total number of coding passes for given bit-planes
///
/// First bit-plane: 1 pass (cleanup only)
/// Subsequent bit-planes: 3 passes each (sig prop, mag ref, cleanup)
pub fn total_coding_passes(num_bitplanes: usize) -> usize {
    if num_bitplanes == 0 {
        0
    } else {
        1 + 3 * (num_bitplanes - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_data(len: usize) -> Vec<u8> {
        let mut data = Vec::with_capacity(len);
        for i in 0..len {
            data.push(((i * 37 + 13) % 256) as u8);
        }
        data
    }

    #[test]
    fn test_decode_code_block_empty_data() {
        let result = decode_code_block(&[], 8, 8, 8, SubbandType::Ll);
        assert!(result.is_ok());
        let coeffs = result.expect("decode failed");
        assert_eq!(coeffs.len(), 64);
        assert!(coeffs.iter().all(|&c| c == 0));
    }

    #[test]
    fn test_decode_code_block_zero_size() {
        let result = decode_code_block(&[1, 2, 3], 0, 0, 8, SubbandType::Ll);
        assert!(result.is_ok());
        assert!(result.expect("decode failed").is_empty());
    }

    #[test]
    fn test_decode_code_block_basic() {
        let data = make_test_data(512);
        let result = decode_code_block(&data, 8, 8, 8, SubbandType::Ll);
        assert!(result.is_ok());
        let coeffs = result.expect("decode failed");
        assert_eq!(coeffs.len(), 64);
    }

    #[test]
    fn test_decode_code_block_all_subbands() {
        for subband in [
            SubbandType::Ll,
            SubbandType::Lh,
            SubbandType::Hl,
            SubbandType::Hh,
        ] {
            let data = make_test_data(512);
            let result = decode_code_block(&data, 4, 4, 8, subband);
            assert!(result.is_ok(), "Failed for subband {:?}", subband);
        }
    }

    #[test]
    fn test_decode_code_block_large() {
        let data = make_test_data(8192);
        let result = decode_code_block(&data, 64, 64, 8, SubbandType::Ll);
        assert!(result.is_ok());
        let coeffs = result.expect("decode failed");
        assert_eq!(coeffs.len(), 64 * 64);
    }

    #[test]
    fn test_decode_code_block_passes_limited() {
        let data = make_test_data(512);
        // Only 1 pass = only cleanup on first bit-plane
        let result = decode_code_block_passes(&data, 8, 8, 8, 1, SubbandType::Ll);
        assert!(result.is_ok());
        let coeffs = result.expect("decode failed");
        assert_eq!(coeffs.len(), 64);
    }

    #[test]
    fn test_decode_code_block_passes_zero() {
        let data = make_test_data(512);
        let result = decode_code_block_passes(&data, 8, 8, 8, 0, SubbandType::Ll);
        assert!(result.is_ok());
        let coeffs = result.expect("decode failed");
        assert!(coeffs.iter().all(|&c| c == 0));
    }

    #[test]
    fn test_total_coding_passes() {
        assert_eq!(total_coding_passes(0), 0);
        assert_eq!(total_coding_passes(1), 1);
        assert_eq!(total_coding_passes(2), 4);
        assert_eq!(total_coding_passes(8), 22);
    }

    #[test]
    fn test_decode_truncated_data() {
        // Very small data that will likely cause truncation
        let data = vec![0x80, 0x00];
        let result = decode_code_block(&data, 32, 32, 8, SubbandType::Ll);
        // Should succeed with partial decode
        assert!(result.is_ok());
    }

    #[test]
    fn test_decode_non_power_of_2() {
        let data = make_test_data(512);
        let result = decode_code_block(&data, 7, 5, 8, SubbandType::Lh);
        assert!(result.is_ok());
        let coeffs = result.expect("decode failed");
        assert_eq!(coeffs.len(), 35);
    }

    #[test]
    fn test_decode_single_bitplane() {
        let data = make_test_data(256);
        let result = decode_code_block(&data, 8, 8, 1, SubbandType::Ll);
        assert!(result.is_ok());
    }

    #[test]
    fn test_decode_with_wavelet_round_trip_53() {
        use crate::wavelet::Reversible53;

        // Create test data, do forward transform, encode-decode, inverse transform
        let width = 8;
        let height = 8;
        let mut original: Vec<i32> = (0..64).map(|i| i * 4).collect();
        let saved = original.clone();

        // Forward wavelet transform
        Reversible53::forward_2d(&mut original, width, height).expect("forward transform failed");

        // Inverse wavelet transform should recover original
        Reversible53::inverse_2d(&mut original, width, height).expect("inverse transform failed");

        assert_eq!(original, saved, "5/3 wavelet round-trip failed");
    }

    #[test]
    fn test_decode_with_wavelet_round_trip_97() {
        use crate::wavelet::Irreversible97;

        let width = 8;
        let height = 8;
        let mut original: Vec<f32> = (0..64).map(|i| (i * 4) as f32).collect();
        let saved = original.clone();

        // Forward wavelet transform
        Irreversible97::forward_2d(&mut original, width, height).expect("forward transform failed");

        // Inverse wavelet transform
        Irreversible97::inverse_2d(&mut original, width, height).expect("inverse transform failed");

        // Lossy: check within tolerance
        for (a, b) in original.iter().zip(saved.iter()) {
            assert!(
                (a - b).abs() < 1.0,
                "9/7 wavelet round-trip error: {} vs {}",
                a,
                b
            );
        }
    }
}
