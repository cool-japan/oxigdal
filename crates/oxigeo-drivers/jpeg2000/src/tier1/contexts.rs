//! Context formation for EBCOT tier-1 coding passes
//!
//! Implements the context determination logic for significance coding,
//! sign coding, and magnitude refinement coding as specified in
//! JPEG2000 Part 1 (ISO/IEC 15444-1), Annex D.
//!
//! Context formation depends on the states of neighboring samples and
//! the subband type (LL/LH, HL, HH).

use super::mq::ctx;
use crate::tier1::SubbandType;

/// Sample state flags for a single coefficient in a code-block
#[derive(Debug, Clone, Copy, Default)]
pub struct SampleState {
    /// Whether this sample is significant (has been found non-zero)
    pub significant: bool,
    /// Sign of the coefficient (0 = positive, 1 = negative)
    pub sign: u8,
    /// Whether this sample has been refined in the current bit-plane
    pub refined: bool,
    /// Whether this sample has been coded in the significance propagation pass
    pub coded_in_sig_prop: bool,
    /// Whether this sample has been coded in the magnitude refinement pass
    pub coded_in_mag_ref: bool,
    /// Number of times this sample has been refined (for magnitude refinement context)
    pub refinement_count: u8,
}

/// State grid for a code-block, managing neighbor lookups and context formation
pub struct StateGrid {
    /// Width of the code-block
    width: usize,
    /// Height of the code-block
    height: usize,
    /// Sample states (padded with 1-pixel border for neighbor lookups)
    /// Stored as (height+2) x (width+2) to avoid bounds checks
    states: Vec<SampleState>,
    /// Padded width (width + 2)
    padded_width: usize,
}

impl StateGrid {
    /// Create a new state grid for the given code-block dimensions
    pub fn new(width: usize, height: usize) -> Self {
        let padded_width = width + 2;
        let padded_height = height + 2;
        Self {
            width,
            height,
            states: vec![SampleState::default(); padded_width * padded_height],
            padded_width,
        }
    }

    /// Get the index into the padded state array for code-block coordinate (x, y)
    #[inline]
    fn idx(&self, x: usize, y: usize) -> usize {
        (y + 1) * self.padded_width + (x + 1)
    }

    /// Get sample state at code-block coordinate (x, y)
    #[inline]
    pub fn get(&self, x: usize, y: usize) -> &SampleState {
        &self.states[self.idx(x, y)]
    }

    /// Get mutable sample state at code-block coordinate (x, y)
    #[inline]
    pub fn get_mut(&mut self, x: usize, y: usize) -> &mut SampleState {
        let idx = self.idx(x, y);
        &mut self.states[idx]
    }

    /// Mark a sample as significant with the given sign
    pub fn set_significant(&mut self, x: usize, y: usize, sign: u8) {
        let state = self.get_mut(x, y);
        state.significant = true;
        state.sign = sign;
    }

    /// Check if a sample is significant
    #[inline]
    pub fn is_significant(&self, x: usize, y: usize) -> bool {
        self.get(x, y).significant
    }

    /// Get the number of significant 4-connected (horizontal + vertical) neighbors
    pub fn num_significant_4_neighbors(&self, x: usize, y: usize) -> u8 {
        let px = x + 1;
        let py = y + 1;
        let pw = self.padded_width;

        let mut count = 0u8;
        // Left
        if self.states[py * pw + (px - 1)].significant {
            count += 1;
        }
        // Right
        if self.states[py * pw + (px + 1)].significant {
            count += 1;
        }
        // Up
        if self.states[(py - 1) * pw + px].significant {
            count += 1;
        }
        // Down
        if self.states[(py + 1) * pw + px].significant {
            count += 1;
        }
        count
    }

    /// Check if a sample has at least one significant 4-connected neighbor
    pub fn has_significant_4_neighbor(&self, x: usize, y: usize) -> bool {
        self.num_significant_4_neighbors(x, y) > 0
    }

    /// Count significant neighbors in the 8-connected neighborhood
    ///
    /// Returns (horizontal_count, vertical_count, diagonal_count) where:
    /// - horizontal = left + right
    /// - vertical = up + down
    /// - diagonal = upper-left + upper-right + lower-left + lower-right
    pub fn neighbor_significance_counts(&self, x: usize, y: usize) -> (u8, u8, u8) {
        let px = x + 1;
        let py = y + 1;
        let pw = self.padded_width;

        let mut h = 0u8; // horizontal
        let mut v = 0u8; // vertical
        let mut d = 0u8; // diagonal

        // Horizontal neighbors
        if self.states[py * pw + (px - 1)].significant {
            h += 1;
        }
        if self.states[py * pw + (px + 1)].significant {
            h += 1;
        }

        // Vertical neighbors
        if self.states[(py - 1) * pw + px].significant {
            v += 1;
        }
        if self.states[(py + 1) * pw + px].significant {
            v += 1;
        }

        // Diagonal neighbors
        if self.states[(py - 1) * pw + (px - 1)].significant {
            d += 1;
        }
        if self.states[(py - 1) * pw + (px + 1)].significant {
            d += 1;
        }
        if self.states[(py + 1) * pw + (px - 1)].significant {
            d += 1;
        }
        if self.states[(py + 1) * pw + (px + 1)].significant {
            d += 1;
        }

        (h, v, d)
    }

    /// Get the sign of a neighbor, returning 0 if not significant
    fn neighbor_sign(&self, px: usize, py: usize) -> i8 {
        let state = &self.states[py * self.padded_width + px];
        if state.significant {
            if state.sign == 0 { 1 } else { -1 }
        } else {
            0
        }
    }

    /// Compute the sign coding context and sign prediction for position (x, y)
    ///
    /// Returns (context_index, xor_bit) where:
    /// - context_index is the MQ context to use (9-13)
    /// - xor_bit is 0 or 1, used to flip the decoded sign bit
    pub fn sign_context(&self, x: usize, y: usize) -> (usize, u8) {
        let px = x + 1;
        let py = y + 1;

        // Horizontal contribution
        let h_left = self.neighbor_sign(px - 1, py);
        let h_right = self.neighbor_sign(px + 1, py);
        let h = h_left + h_right;

        // Vertical contribution
        let v_up = self.neighbor_sign(px, py - 1);
        let v_down = self.neighbor_sign(px, py + 1);
        let v = v_up + v_down;

        // Table D.2: Sign context from horizontal and vertical contributions
        let (h_contrib, v_contrib, xor_bit) = match (h.signum(), v.signum()) {
            (1, 1) => (1, 1, 0u8),
            (1, 0) => (1, 0, 0),
            (1, -1) => (1, -1, 0),
            (0, 1) => (0, 1, 0),
            (0, 0) => (0, 0, 0),
            (0, -1) => (0, 1, 1),  // flip
            (-1, 1) => (1, -1, 1), // flip
            (-1, 0) => (1, 0, 1),  // flip
            (-1, -1) => (1, 1, 1), // flip
            _ => (0, 0, 0),        // should not happen
        };

        // Map (h_contrib, v_contrib) to context index 9-13
        let ctx = match (h_contrib, v_contrib) {
            (1, 1) => ctx::SIGN_START,      // 9
            (1, 0) => ctx::SIGN_START + 1,  // 10
            (1, -1) => ctx::SIGN_START + 2, // 11
            (0, 1) => ctx::SIGN_START + 3,  // 12
            (0, 0) => ctx::SIGN_START + 4,  // 13
            _ => ctx::SIGN_START + 4,       // 13 (default)
        };

        (ctx, xor_bit)
    }

    /// Compute the significance coding context for position (x, y)
    ///
    /// The context depends on the subband type and the significance state
    /// of the 8-connected neighbors, as specified in Table D.1 of JPEG2000.
    pub fn significance_context(&self, x: usize, y: usize, subband: SubbandType) -> usize {
        let (h, v, d) = self.neighbor_significance_counts(x, y);

        match subband {
            SubbandType::Hl => {
                // HL band: horizontal and vertical swapped
                self.sig_context_ll_lh(v, h, d)
            }
            SubbandType::Hh => {
                // HH band: uses diagonal-dominant context
                self.sig_context_hh(h, v, d)
            }
            SubbandType::Ll | SubbandType::Lh => {
                // LL/LH band: standard context
                self.sig_context_ll_lh(h, v, d)
            }
        }
    }

    /// Significance context for LL/LH subbands (Table D.1)
    fn sig_context_ll_lh(&self, h: u8, v: u8, d: u8) -> usize {
        if h == 2 {
            ctx::SIG_LL_LH_START + 8
        } else if h == 1 {
            if v >= 1 {
                ctx::SIG_LL_LH_START + 7
            } else if d >= 1 {
                ctx::SIG_LL_LH_START + 6
            } else {
                ctx::SIG_LL_LH_START + 5
            }
        } else {
            // h == 0
            if v == 2 {
                ctx::SIG_LL_LH_START + 4
            } else if v == 1 {
                if d >= 1 {
                    ctx::SIG_LL_LH_START + 3
                } else {
                    ctx::SIG_LL_LH_START + 2
                }
            } else {
                // v == 0
                if d >= 2 {
                    ctx::SIG_LL_LH_START + 1
                } else if d == 1 {
                    ctx::SIG_LL_LH_START
                } else {
                    // All zero neighborhood - zero context
                    ctx::SIG_LL_LH_START
                }
            }
        }
    }

    /// Significance context for HH subband (Table D.1)
    fn sig_context_hh(&self, h: u8, v: u8, d: u8) -> usize {
        let hv = h + v;

        if d >= 3 {
            ctx::SIG_HH_START + 8
        } else if d == 2 {
            if hv >= 1 {
                ctx::SIG_HH_START + 7
            } else {
                ctx::SIG_HH_START + 6
            }
        } else if d == 1 {
            if hv >= 2 {
                ctx::SIG_HH_START + 5
            } else if hv == 1 {
                ctx::SIG_HH_START + 4
            } else {
                ctx::SIG_HH_START + 3
            }
        } else {
            // d == 0
            if hv >= 2 {
                ctx::SIG_HH_START + 2
            } else if hv == 1 {
                ctx::SIG_HH_START + 1
            } else {
                ctx::SIG_HH_START
            }
        }
    }

    /// Compute the magnitude refinement context for position (x, y)
    ///
    /// Returns context index 14 or 15, based on whether this is the first
    /// refinement and the significance of neighbors.
    pub fn magnitude_refinement_context(&self, x: usize, y: usize) -> usize {
        let state = self.get(x, y);

        if state.refinement_count > 0 {
            // Not the first refinement - use context 16
            ctx::MAG_REF_START + 2
        } else {
            // First refinement
            let (h, v, d) = self.neighbor_significance_counts(x, y);
            let total = h + v + d;
            if total > 0 {
                ctx::MAG_REF_START + 1
            } else {
                ctx::MAG_REF_START
            }
        }
    }

    /// Check if all 4 samples in a column stripe at (x, y..y+3) have no
    /// significant neighbors, enabling run-length coding in cleanup pass
    pub fn can_run_length_code(&self, x: usize, y: usize) -> bool {
        // All 4 samples must be non-significant and have no significant neighbors
        for row in y..y.saturating_add(4).min(self.height) {
            if self.is_significant(x, row) {
                return false;
            }
            let (h, v, d) = self.neighbor_significance_counts(x, row);
            if h + v + d > 0 {
                return false;
            }
        }
        true
    }

    /// Reset all sample states for a new bit-plane pass
    pub fn reset_pass_flags(&mut self) {
        for state in &mut self.states {
            state.coded_in_sig_prop = false;
            state.coded_in_mag_ref = false;
        }
    }

    /// Get code-block dimensions
    pub fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_grid_creation() {
        let grid = StateGrid::new(64, 64);
        assert_eq!(grid.dimensions(), (64, 64));
        assert!(!grid.is_significant(0, 0));
        assert!(!grid.is_significant(63, 63));
    }

    #[test]
    fn test_set_significant() {
        let mut grid = StateGrid::new(8, 8);
        grid.set_significant(3, 4, 0);
        assert!(grid.is_significant(3, 4));
        assert_eq!(grid.get(3, 4).sign, 0);
    }

    #[test]
    fn test_neighbor_counts_isolated() {
        let grid = StateGrid::new(8, 8);
        let (h, v, d) = grid.neighbor_significance_counts(4, 4);
        assert_eq!(h, 0);
        assert_eq!(v, 0);
        assert_eq!(d, 0);
    }

    #[test]
    fn test_neighbor_counts_with_neighbors() {
        let mut grid = StateGrid::new(8, 8);
        // Set horizontal neighbors
        grid.set_significant(3, 4, 0); // left
        grid.set_significant(5, 4, 0); // right
        // Set vertical neighbors
        grid.set_significant(4, 3, 0); // up
        // Set diagonal neighbor
        grid.set_significant(3, 3, 0); // upper-left

        let (h, v, d) = grid.neighbor_significance_counts(4, 4);
        assert_eq!(h, 2);
        assert_eq!(v, 1);
        assert_eq!(d, 1);
    }

    #[test]
    fn test_has_significant_4_neighbor() {
        let mut grid = StateGrid::new(8, 8);
        assert!(!grid.has_significant_4_neighbor(4, 4));

        grid.set_significant(4, 3, 0); // up
        assert!(grid.has_significant_4_neighbor(4, 4));
    }

    #[test]
    fn test_significance_context_ll_zero() {
        let grid = StateGrid::new(8, 8);
        let ctx = grid.significance_context(4, 4, SubbandType::Ll);
        // All zero neighbors -> context 0
        assert_eq!(ctx, 0);
    }

    #[test]
    fn test_significance_context_ll_with_h_neighbors() {
        let mut grid = StateGrid::new(8, 8);
        grid.set_significant(3, 4, 0);
        grid.set_significant(5, 4, 0);

        let ctx = grid.significance_context(4, 4, SubbandType::Ll);
        // h=2 -> context 8
        assert_eq!(ctx, 8);
    }

    #[test]
    fn test_significance_context_hh() {
        let mut grid = StateGrid::new(8, 8);
        grid.set_significant(3, 3, 0); // diagonal
        grid.set_significant(5, 3, 0); // diagonal
        grid.set_significant(3, 5, 0); // diagonal

        let ctx = grid.significance_context(4, 4, SubbandType::Hh);
        // d=3 -> context 8
        assert_eq!(ctx, 8);
    }

    #[test]
    fn test_sign_context_positive_neighbors() {
        let mut grid = StateGrid::new(8, 8);
        grid.set_significant(3, 4, 0); // left, positive
        grid.set_significant(4, 3, 0); // up, positive

        let (ctx, xor_bit) = grid.sign_context(4, 4);
        // h > 0, v > 0 => context 9, xor_bit 0
        assert_eq!(ctx, ctx::SIGN_START);
        assert_eq!(xor_bit, 0);
    }

    #[test]
    fn test_sign_context_no_neighbors() {
        let grid = StateGrid::new(8, 8);
        let (ctx, xor_bit) = grid.sign_context(4, 4);
        // h=0, v=0 => context 13, xor_bit 0
        assert_eq!(ctx, ctx::SIGN_START + 4);
        assert_eq!(xor_bit, 0);
    }

    #[test]
    fn test_magnitude_refinement_context_first() {
        let grid = StateGrid::new(8, 8);
        let ctx = grid.magnitude_refinement_context(4, 4);
        // First refinement, no significant neighbors -> context 14
        assert_eq!(ctx, ctx::MAG_REF_START);
    }

    #[test]
    fn test_magnitude_refinement_context_with_neighbors() {
        let mut grid = StateGrid::new(8, 8);
        grid.set_significant(3, 4, 0); // a significant neighbor

        let ctx = grid.magnitude_refinement_context(4, 4);
        // First refinement, has significant neighbors -> context 15
        assert_eq!(ctx, ctx::MAG_REF_START + 1);
    }

    #[test]
    fn test_magnitude_refinement_context_subsequent() {
        let mut grid = StateGrid::new(8, 8);
        grid.get_mut(4, 4).refinement_count = 1;

        let ctx = grid.magnitude_refinement_context(4, 4);
        // Subsequent refinement -> context 16
        assert_eq!(ctx, ctx::MAG_REF_START + 2);
    }

    #[test]
    fn test_can_run_length_code_empty() {
        let grid = StateGrid::new(8, 8);
        assert!(grid.can_run_length_code(0, 0));
    }

    #[test]
    fn test_can_run_length_code_with_significant() {
        let mut grid = StateGrid::new(8, 8);
        grid.set_significant(0, 2, 0);
        assert!(!grid.can_run_length_code(0, 0));
    }

    #[test]
    fn test_can_run_length_code_with_neighbor() {
        let mut grid = StateGrid::new(8, 8);
        grid.set_significant(1, 1, 0); // neighbor of (0, 1)
        assert!(!grid.can_run_length_code(0, 0));
    }

    #[test]
    fn test_reset_pass_flags() {
        let mut grid = StateGrid::new(8, 8);
        grid.get_mut(0, 0).coded_in_sig_prop = true;
        grid.get_mut(1, 1).coded_in_mag_ref = true;

        grid.reset_pass_flags();
        assert!(!grid.get(0, 0).coded_in_sig_prop);
        assert!(!grid.get(1, 1).coded_in_mag_ref);
    }

    #[test]
    fn test_edge_coordinates() {
        let mut grid = StateGrid::new(4, 4);
        // Test corners - should not panic
        grid.set_significant(0, 0, 1);
        grid.set_significant(3, 0, 0);
        grid.set_significant(0, 3, 1);
        grid.set_significant(3, 3, 0);

        // Check neighbor counts at corners
        let (h, v, d) = grid.neighbor_significance_counts(0, 0);
        assert_eq!(h + v + d, 0); // no significant neighbors in interior

        // (1, 1) should see (0,0) as a diagonal neighbor
        let (_, _, d) = grid.neighbor_significance_counts(1, 1);
        assert_eq!(d, 1);
    }
}
