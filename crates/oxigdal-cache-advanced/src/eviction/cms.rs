//! Count-Min Sketch frequency estimator for W-TinyLFU.
//!
//! Implements a space-efficient probabilistic frequency estimator using
//! 4-bit counters packed two-per-byte, with periodic aging (halving) to
//! prevent frequency inflation of stale items.
//!
//! Reference: Einziger et al. 2017 — "TinyLFU: A Highly Efficient Cache Admission Policy"

/// Count-Min Sketch frequency estimator (Einziger et al. 2017).
///
/// 4-bit counters packed two-per-byte; aging via halving when additions >= sample_size.
/// Uses double-hashing with 4 independent rows to minimize collision probability.
pub struct CountMinSketch {
    /// 4-bit packed counters: even linear_idx in low nibble, odd in high nibble
    counters: Vec<u8>,
    /// Number of columns (capacity.next_power_of_two().max(16))
    width: usize,
    /// Aging threshold (10 * capacity)
    sample_size: usize,
    /// Running count of increments since last reset
    additions: u64,
}

impl CountMinSketch {
    /// Create a new Count-Min Sketch sized for the given cache capacity.
    ///
    /// `width` = capacity.next_power_of_two().max(16), `depth` = 4,
    /// `sample_size` = 10 * capacity.max(1).
    pub fn new(capacity: usize) -> Self {
        let width = capacity.next_power_of_two().max(16);
        // Fixed depth of 4 rows; two 4-bit counters packed per byte
        const DEPTH: usize = 4;
        let byte_count = (width * DEPTH).div_ceil(2);
        let sample_size = 10 * capacity.max(1);
        Self {
            counters: vec![0u8; byte_count],
            width,
            sample_size,
            additions: 0,
        }
    }

    /// Compute the flat linear index for (row, col).
    #[inline]
    fn linear_idx(&self, row: usize, col: usize) -> usize {
        row * self.width + col
    }

    /// Read the 4-bit counter value at (row, col).
    #[inline]
    fn get_cell(&self, row: usize, col: usize) -> u8 {
        let idx = self.linear_idx(row, col);
        let byte = self.counters[idx / 2];
        if idx % 2 == 0 {
            byte & 0x0F
        } else {
            (byte >> 4) & 0x0F
        }
    }

    /// Write a 4-bit counter value (clamped to 15) at (row, col).
    #[inline]
    fn set_cell(&mut self, row: usize, col: usize, val: u8) {
        let idx = self.linear_idx(row, col);
        let clamped = val.min(15);
        if idx % 2 == 0 {
            self.counters[idx / 2] = (self.counters[idx / 2] & 0xF0) | clamped;
        } else {
            self.counters[idx / 2] = (self.counters[idx / 2] & 0x0F) | (clamped << 4);
        }
    }

    /// Derive 4 independent column positions from a 64-bit hash using double hashing.
    ///
    /// Uses the Kirsch-Mitzenmacher trick: h_low + i * h_high (mod width).
    /// h_high is forced odd to ensure all residues are reachable.
    #[inline]
    fn derive_positions(&self, item_hash: u64) -> [usize; 4] {
        let h_low = item_hash & 0xFFFF_FFFF;
        let h_high = (item_hash >> 32) | 1; // force odd for full coverage
        let mut pos = [0usize; 4];
        for (i, slot) in pos.iter_mut().enumerate() {
            *slot = ((h_low.wrapping_add(i as u64 * h_high)) % self.width as u64) as usize;
        }
        pos
    }

    /// Increment the frequency counter for the item identified by `item_hash`.
    ///
    /// If the running addition count reaches `sample_size`, triggers an aging
    /// reset (all counters halved) to maintain recency sensitivity.
    pub fn increment(&mut self, item_hash: u64) {
        let positions = self.derive_positions(item_hash);
        for (row, &col) in positions.iter().enumerate() {
            let current = self.get_cell(row, col);
            // Saturating increment: max nibble value is 15
            self.set_cell(row, col, current.saturating_add(1).min(15));
        }
        self.additions += 1;
        if self.additions >= self.sample_size as u64 {
            self.reset();
        }
    }

    /// Estimate the frequency of the item identified by `item_hash`.
    ///
    /// Returns the minimum across all 4 rows (classic CMS point query).
    pub fn estimate(&self, item_hash: u64) -> u8 {
        let positions = self.derive_positions(item_hash);
        let mut min_val = 15u8;
        for (row, &col) in positions.iter().enumerate() {
            let v = self.get_cell(row, col);
            if v < min_val {
                min_val = v;
            }
        }
        min_val
    }

    /// Age all counters by halving each nibble in place.
    ///
    /// Resets the additions counter to zero, allowing fresh frequency
    /// accumulation from this point forward.
    pub fn reset(&mut self) {
        for b in self.counters.iter_mut() {
            let lo = (*b & 0x0F) >> 1;
            let hi = ((*b >> 4) & 0x0F) >> 1;
            *b = (hi << 4) | lo;
        }
        self.additions = 0;
    }

    /// Number of increments since the last aging reset.
    pub fn additions(&self) -> u64 {
        self.additions
    }

    /// The threshold at which aging (reset) is triggered.
    pub fn sample_size(&self) -> usize {
        self.sample_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_zero_additions() {
        let cms = CountMinSketch::new(100);
        assert_eq!(cms.additions(), 0);
        assert_eq!(cms.sample_size(), 1000);
    }

    #[test]
    fn test_width_is_power_of_two() {
        let cms = CountMinSketch::new(100);
        assert!(cms.width.is_power_of_two());
        assert!(cms.width >= 16);
    }

    #[test]
    fn test_increment_and_estimate() {
        let mut cms = CountMinSketch::new(256);
        let h = 0xDEAD_BEEF_u64;
        for _ in 0..7 {
            cms.increment(h);
        }
        assert_eq!(cms.estimate(h), 7);
    }

    #[test]
    fn test_reset_halves() {
        let mut cms = CountMinSketch::new(256);
        let h = 0xCAFE_BABE_u64;
        for _ in 0..10 {
            cms.increment(h);
        }
        assert_eq!(cms.estimate(h), 10);
        cms.reset();
        assert_eq!(cms.estimate(h), 5);
        assert_eq!(cms.additions(), 0);
    }
}
