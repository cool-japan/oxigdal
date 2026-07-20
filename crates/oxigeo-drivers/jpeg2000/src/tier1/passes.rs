//! EBCOT coding passes implementation
//!
//! Implements the three coding passes used in JPEG2000 EBCOT tier-1 decoding:
//! - Significance Propagation Pass
//! - Magnitude Refinement Pass
//! - Cleanup Pass
//!
//! Each bit-plane is processed with these three passes in order.
//! The passes determine the order and method of decoding individual bits
//! as specified in JPEG2000 Part 1 (ISO/IEC 15444-1), Annex D.

use super::contexts::StateGrid;
use super::mq::{MqDecoder, ctx};
use crate::error::Result;
use crate::tier1::SubbandType;

/// Stripe height for EBCOT coding passes (always 4 rows)
const STRIPE_HEIGHT: usize = 4;

/// Result of a coding pass - number of samples processed
#[derive(Debug, Clone, Copy, Default)]
pub struct PassResult {
    /// Number of samples newly made significant
    pub newly_significant: usize,
    /// Number of samples refined
    pub refined: usize,
    /// Number of samples processed in cleanup
    pub cleaned_up: usize,
}

/// Perform the Significance Propagation Pass on one bit-plane
///
/// Processes samples in scan order (column stripes of 4 rows).
/// For each non-significant sample that has at least one significant
/// 4-connected neighbor:
/// 1. Decode significance bit using significance context
/// 2. If newly significant, decode sign bit using sign context
///
/// Returns the number of newly significant samples.
pub fn significance_propagation_pass(
    mq: &mut MqDecoder,
    grid: &mut StateGrid,
    coefficients: &mut [i32],
    bit_plane: u32,
    subband: SubbandType,
    width: usize,
    height: usize,
) -> Result<PassResult> {
    let mut result = PassResult::default();
    let bit_value = 1i32 << bit_plane;

    // Process in column stripes of STRIPE_HEIGHT
    for stripe_y in (0..height).step_by(STRIPE_HEIGHT) {
        for x in 0..width {
            let stripe_end = (stripe_y + STRIPE_HEIGHT).min(height);
            for y in stripe_y..stripe_end {
                let state = grid.get(x, y);

                // Skip if already significant or no significant neighbor
                if state.significant {
                    continue;
                }
                if !grid.has_significant_4_neighbor(x, y) {
                    continue;
                }

                // Decode significance bit
                let sig_ctx = grid.significance_context(x, y, subband);
                let sig_bit = mq.decode(sig_ctx)?;

                if sig_bit != 0 {
                    // Newly significant - decode sign
                    let (sign_ctx, xor_bit) = grid.sign_context(x, y);
                    let sign_raw = mq.decode(sign_ctx)?;
                    let sign = sign_raw ^ xor_bit;

                    // Set the coefficient value
                    let idx = y * width + x;
                    if idx < coefficients.len() {
                        coefficients[idx] = if sign != 0 { -bit_value } else { bit_value };
                    }

                    // Update state
                    grid.set_significant(x, y, sign);
                    result.newly_significant += 1;
                }

                // Mark as coded in this pass
                grid.get_mut(x, y).coded_in_sig_prop = true;
            }
        }
    }

    Ok(result)
}

/// Perform the Magnitude Refinement Pass on one bit-plane
///
/// Processes samples that were already significant BEFORE this bit-plane
/// (i.e., became significant in a higher bit-plane). For each such sample,
/// decode one refinement bit using the magnitude refinement context.
///
/// Returns the number of samples refined.
pub fn magnitude_refinement_pass(
    mq: &mut MqDecoder,
    grid: &mut StateGrid,
    coefficients: &mut [i32],
    bit_plane: u32,
    width: usize,
    height: usize,
) -> Result<PassResult> {
    let mut result = PassResult::default();
    let bit_value = 1i32 << bit_plane;

    // Process in column stripes
    for stripe_y in (0..height).step_by(STRIPE_HEIGHT) {
        for x in 0..width {
            let stripe_end = (stripe_y + STRIPE_HEIGHT).min(height);
            for y in stripe_y..stripe_end {
                let state = grid.get(x, y);

                // Only process samples that were already significant and not
                // coded in the significance propagation pass of THIS bit-plane
                if !state.significant || state.coded_in_sig_prop {
                    continue;
                }

                // Decode refinement bit
                let ref_ctx = grid.magnitude_refinement_context(x, y);
                let ref_bit = mq.decode(ref_ctx)?;

                // Apply refinement: add the bit at the current bit-plane position
                let idx = y * width + x;
                if idx < coefficients.len() {
                    let abs_val = coefficients[idx].unsigned_abs() as i32;
                    let new_abs = if ref_bit != 0 {
                        abs_val | bit_value
                    } else {
                        abs_val & !bit_value
                    };
                    let sign = if coefficients[idx] < 0 { -1i32 } else { 1 };
                    coefficients[idx] = new_abs * sign;
                }

                // Update refinement state
                grid.get_mut(x, y).refined = true;
                grid.get_mut(x, y).refinement_count =
                    grid.get(x, y).refinement_count.saturating_add(1);
                grid.get_mut(x, y).coded_in_mag_ref = true;

                result.refined += 1;
            }
        }
    }

    Ok(result)
}

/// Perform the Cleanup Pass on one bit-plane
///
/// Processes all remaining samples not processed in the significance
/// propagation or magnitude refinement passes. Uses run-length coding
/// when applicable: if 4 consecutive samples in a column stripe have
/// no significant neighbors, they can be coded with a single run-length
/// symbol.
///
/// This is also the only pass used for the most significant bit-plane
/// (first bit-plane).
///
/// Returns the number of samples processed.
pub fn cleanup_pass(
    mq: &mut MqDecoder,
    grid: &mut StateGrid,
    coefficients: &mut [i32],
    bit_plane: u32,
    subband: SubbandType,
    width: usize,
    height: usize,
) -> Result<PassResult> {
    let mut result = PassResult::default();
    let bit_value = 1i32 << bit_plane;

    // Process in column stripes
    for stripe_y in (0..height).step_by(STRIPE_HEIGHT) {
        for x in 0..width {
            let stripe_end = (stripe_y + STRIPE_HEIGHT).min(height);
            let stripe_len = stripe_end - stripe_y;

            let mut row = stripe_y;

            // Check if run-length coding can be applied at the start of the stripe
            if stripe_len == STRIPE_HEIGHT
                && row + STRIPE_HEIGHT <= height
                && can_run_length_in_cleanup(grid, x, row, stripe_end)
            {
                // Try run-length coding
                let rl_bit = mq.decode(ctx::RUN_LENGTH)?;

                if rl_bit == 0 {
                    // All 4 samples are zero in this bit-plane - skip them
                    for y in row..stripe_end {
                        grid.get_mut(x, y).coded_in_sig_prop = false;
                        grid.get_mut(x, y).coded_in_mag_ref = false;
                    }
                    result.cleaned_up += STRIPE_HEIGHT;
                    continue;
                }

                // Run-length interrupted: decode which of the 4 samples is significant
                // using uniform context (2 bits for position 0-3)
                let pos_hi = mq.decode(ctx::UNIFORM)?;
                let pos_lo = mq.decode(ctx::UNIFORM)?;
                let first_sig_pos = ((pos_hi as usize) << 1) | (pos_lo as usize);

                // All samples before the significant one are zero
                for _y in row..row.saturating_add(first_sig_pos).min(stripe_end) {
                    result.cleaned_up += 1;
                }

                // The significant sample
                let sig_y = row + first_sig_pos;
                if sig_y < stripe_end {
                    // Decode sign
                    let (sign_ctx, xor_bit) = grid.sign_context(x, sig_y);
                    let sign_raw = mq.decode(sign_ctx)?;
                    let sign = sign_raw ^ xor_bit;

                    let idx = sig_y * width + x;
                    if idx < coefficients.len() {
                        coefficients[idx] = if sign != 0 { -bit_value } else { bit_value };
                    }

                    grid.set_significant(x, sig_y, sign);
                    result.newly_significant += 1;
                    result.cleaned_up += 1;

                    row = sig_y + 1;
                }
            }

            // Process remaining samples in the stripe individually
            while row < stripe_end {
                let state = grid.get(x, row);

                // Skip if already processed in sig prop or mag ref
                if state.coded_in_sig_prop || state.coded_in_mag_ref || state.significant {
                    row += 1;
                    result.cleaned_up += 1;
                    continue;
                }

                // Decode significance
                let sig_ctx = grid.significance_context(x, row, subband);
                let sig_bit = mq.decode(sig_ctx)?;

                if sig_bit != 0 {
                    // Decode sign
                    let (sign_ctx, xor_bit) = grid.sign_context(x, row);
                    let sign_raw = mq.decode(sign_ctx)?;
                    let sign = sign_raw ^ xor_bit;

                    let idx = row * width + x;
                    if idx < coefficients.len() {
                        coefficients[idx] = if sign != 0 { -bit_value } else { bit_value };
                    }

                    grid.set_significant(x, row, sign);
                    result.newly_significant += 1;
                }

                result.cleaned_up += 1;
                row += 1;
            }
        }
    }

    Ok(result)
}

/// Check if run-length coding is applicable for a column stripe in cleanup pass
///
/// Run-length coding can be used when:
/// 1. We have a full stripe of 4 samples
/// 2. None of the 4 samples are significant
/// 3. None have been coded in previous passes
/// 4. None have any significant neighbors
fn can_run_length_in_cleanup(
    grid: &StateGrid,
    x: usize,
    stripe_y: usize,
    stripe_end: usize,
) -> bool {
    for y in stripe_y..stripe_end {
        let state = grid.get(x, y);
        if state.significant || state.coded_in_sig_prop || state.coded_in_mag_ref {
            return false;
        }
        let (h, v, d) = grid.neighbor_significance_counts(x, y);
        if h + v + d > 0 {
            return false;
        }
    }
    true
}

/// Decode all three passes for a single bit-plane
///
/// For the first (most significant) bit-plane, only the cleanup pass is run.
/// For subsequent bit-planes, all three passes run in order:
/// 1. Significance Propagation
/// 2. Magnitude Refinement
/// 3. Cleanup
#[allow(clippy::too_many_arguments)]
pub fn decode_bit_plane(
    mq: &mut MqDecoder,
    grid: &mut StateGrid,
    coefficients: &mut [i32],
    bit_plane: u32,
    is_first_plane: bool,
    subband: SubbandType,
    width: usize,
    height: usize,
) -> Result<PassResult> {
    grid.reset_pass_flags();

    let mut total = PassResult::default();

    if !is_first_plane {
        // Pass 1: Significance Propagation
        let r1 = significance_propagation_pass(
            mq,
            grid,
            coefficients,
            bit_plane,
            subband,
            width,
            height,
        )?;
        total.newly_significant += r1.newly_significant;

        // Pass 2: Magnitude Refinement
        let r2 = magnitude_refinement_pass(mq, grid, coefficients, bit_plane, width, height)?;
        total.refined += r2.refined;
    }

    // Pass 3: Cleanup
    let r3 = cleanup_pass(mq, grid, coefficients, bit_plane, subband, width, height)?;
    total.newly_significant += r3.newly_significant;
    total.cleaned_up += r3.cleaned_up;

    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create test data that the MQ decoder can consume
    fn make_test_data(len: usize) -> Vec<u8> {
        let mut data = Vec::with_capacity(len);
        for i in 0..len {
            data.push(((i * 37 + 13) % 256) as u8);
        }
        data
    }

    #[test]
    fn test_significance_propagation_no_neighbors() {
        // With no significant samples, sig prop should do nothing
        let data = make_test_data(256);
        let mut mq = MqDecoder::new(data);
        let mut grid = StateGrid::new(8, 8);
        let mut coefficients = vec![0i32; 64];

        let result = significance_propagation_pass(
            &mut mq,
            &mut grid,
            &mut coefficients,
            7,
            SubbandType::Ll,
            8,
            8,
        );
        assert!(result.is_ok());
        let r = result.expect("pass failed");
        assert_eq!(r.newly_significant, 0);
    }

    #[test]
    fn test_significance_propagation_with_neighbor() {
        let data = make_test_data(256);
        let mut mq = MqDecoder::new(data);
        let mut grid = StateGrid::new(8, 8);
        let mut coefficients = vec![0i32; 64];

        // Set one sample as significant to trigger propagation to neighbors
        grid.set_significant(4, 4, 0);
        coefficients[4 * 8 + 4] = 128;

        let result = significance_propagation_pass(
            &mut mq,
            &mut grid,
            &mut coefficients,
            6,
            SubbandType::Ll,
            8,
            8,
        );
        assert!(result.is_ok());
        // Some neighbors should have been processed
    }

    #[test]
    fn test_magnitude_refinement_no_significant() {
        let data = make_test_data(256);
        let mut mq = MqDecoder::new(data);
        let mut grid = StateGrid::new(8, 8);
        let mut coefficients = vec![0i32; 64];

        let result = magnitude_refinement_pass(&mut mq, &mut grid, &mut coefficients, 6, 8, 8);
        assert!(result.is_ok());
        let r = result.expect("pass failed");
        assert_eq!(r.refined, 0);
    }

    #[test]
    fn test_magnitude_refinement_with_significant() {
        let data = make_test_data(256);
        let mut mq = MqDecoder::new(data);
        let mut grid = StateGrid::new(8, 8);
        let mut coefficients = vec![0i32; 64];

        // Mark a sample as significant (from a previous bit-plane)
        grid.set_significant(3, 3, 0);
        coefficients[3 * 8 + 3] = 128;

        let result = magnitude_refinement_pass(&mut mq, &mut grid, &mut coefficients, 6, 8, 8);
        assert!(result.is_ok());
        let r = result.expect("pass failed");
        assert_eq!(r.refined, 1);
    }

    #[test]
    fn test_cleanup_pass_empty() {
        let data = make_test_data(256);
        let mut mq = MqDecoder::new(data);
        let mut grid = StateGrid::new(8, 8);
        let mut coefficients = vec![0i32; 64];

        let result = cleanup_pass(
            &mut mq,
            &mut grid,
            &mut coefficients,
            7,
            SubbandType::Ll,
            8,
            8,
        );
        assert!(result.is_ok());
        let r = result.expect("pass failed");
        // All samples should be processed in cleanup
        assert!(r.cleaned_up > 0);
    }

    #[test]
    fn test_cleanup_pass_run_length() {
        // With all-zero neighborhoods, run-length coding should be attempted
        let data = make_test_data(512);
        let mut mq = MqDecoder::new(data);
        let mut grid = StateGrid::new(8, 8);
        let mut coefficients = vec![0i32; 64];

        let result = cleanup_pass(
            &mut mq,
            &mut grid,
            &mut coefficients,
            7,
            SubbandType::Ll,
            8,
            8,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_decode_bit_plane_first() {
        let data = make_test_data(512);
        let mut mq = MqDecoder::new(data);
        let mut grid = StateGrid::new(8, 8);
        let mut coefficients = vec![0i32; 64];

        // First bit-plane: only cleanup pass
        let result = decode_bit_plane(
            &mut mq,
            &mut grid,
            &mut coefficients,
            7,
            true,
            SubbandType::Ll,
            8,
            8,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_decode_bit_plane_subsequent() {
        let data = make_test_data(512);
        let mut mq = MqDecoder::new(data);
        let mut grid = StateGrid::new(8, 8);
        let mut coefficients = vec![0i32; 64];

        // Make some samples significant from "previous" bit-plane
        grid.set_significant(2, 2, 0);
        coefficients[2 * 8 + 2] = 128;

        // Subsequent bit-plane: all three passes
        let result = decode_bit_plane(
            &mut mq,
            &mut grid,
            &mut coefficients,
            6,
            false,
            SubbandType::Ll,
            8,
            8,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_pass_result_default() {
        let r = PassResult::default();
        assert_eq!(r.newly_significant, 0);
        assert_eq!(r.refined, 0);
        assert_eq!(r.cleaned_up, 0);
    }

    #[test]
    fn test_all_subband_types() {
        for subband in [
            SubbandType::Ll,
            SubbandType::Lh,
            SubbandType::Hl,
            SubbandType::Hh,
        ] {
            let data = make_test_data(512);
            let mut mq = MqDecoder::new(data);
            let mut grid = StateGrid::new(4, 4);
            let mut coefficients = vec![0i32; 16];

            let result = decode_bit_plane(
                &mut mq,
                &mut grid,
                &mut coefficients,
                7,
                true,
                subband,
                4,
                4,
            );
            assert!(result.is_ok(), "Failed for subband {:?}", subband);
        }
    }

    #[test]
    fn test_multiple_bit_planes() {
        let data = make_test_data(4096);
        let mut mq = MqDecoder::new(data);
        let mut grid = StateGrid::new(4, 4);
        let mut coefficients = vec![0i32; 16];

        // Decode from MSB to LSB
        for bp in (0..8).rev() {
            let is_first = bp == 7;
            let result = decode_bit_plane(
                &mut mq,
                &mut grid,
                &mut coefficients,
                bp,
                is_first,
                SubbandType::Ll,
                4,
                4,
            );
            assert!(result.is_ok(), "Failed at bit-plane {}", bp);
        }
    }

    #[test]
    fn test_non_multiple_of_4_height() {
        // Height not a multiple of 4 (stripe height)
        let data = make_test_data(512);
        let mut mq = MqDecoder::new(data);
        let mut grid = StateGrid::new(4, 6);
        let mut coefficients = vec![0i32; 24];

        let result = decode_bit_plane(
            &mut mq,
            &mut grid,
            &mut coefficients,
            7,
            true,
            SubbandType::Ll,
            4,
            6,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_single_pixel_block() {
        let data = make_test_data(64);
        let mut mq = MqDecoder::new(data);
        let mut grid = StateGrid::new(1, 1);
        let mut coefficients = vec![0i32; 1];

        let result = decode_bit_plane(
            &mut mq,
            &mut grid,
            &mut coefficients,
            7,
            true,
            SubbandType::Ll,
            1,
            1,
        );
        assert!(result.is_ok());
    }
}
