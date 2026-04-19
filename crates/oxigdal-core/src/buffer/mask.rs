//! Nodata and validity bitmask for raster regions.
//!
//! # Bit-packing convention
//!
//! Pixels are stored in row-major order: pixel `(x, y)` maps to bit index
//! `i = y * width + x`.  Each bit index maps to word `data[i / 64]`, bit
//! position `i % 64` (LSB = bit 0 = leftmost pixel in each group of 64).
//!
//! **Masking semantics**: a `1` bit means the pixel is **invalid / nodata**;
//! a `0` bit means the pixel is **valid**.  This convention matches GDAL's
//! nodata mask band where non-zero → valid data is the *inverse*, but
//! follows the more common raster tradition where "masked out" = 1.
//!
//! # Tail-word invariant
//!
//! When `width * height` is not a multiple of 64 the high bits of the last
//! word are always kept **zero**.  Every mutating method that could set high
//! bits calls `Mask::clear_tail_bits` before returning.  This invariant
//! makes `count_set`, `all_unset`, and `any_set` trivially correct without
//! any per-call tail masking.

use crate::error::{OxiGdalError, Result};

/// A 2-D boolean bitmask for a raster region.
///
/// `Mask` uses bit-packed `u64` words (64 pixels per word) for memory
/// efficiency.  See the [module documentation][self] for the storage and
/// semantic conventions.
///
/// # Examples
///
/// ```
/// use oxigdal_core::buffer::Mask;
///
/// let mut mask = Mask::new(10, 10);         // all valid (0)
/// mask.set(3, 3, true);                      // mark (3,3) as nodata
/// assert!(mask.get(3, 3));
/// assert_eq!(mask.count_set(), 1);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mask {
    /// Width of the raster region in pixels.
    width: usize,
    /// Height of the raster region in pixels.
    height: usize,
    /// Bit-packed words.  Bit `i % 64` of `data[i / 64]` holds pixel
    /// `(i % width, i / width)` where `i = y * width + x`.
    data: Vec<u64>,
}

// ─── Internal helpers ──────────────────────────────────────────────────────────

impl Mask {
    /// Returns the number of `u64` words needed for `pixel_count` pixels.
    #[inline]
    fn words_for(pixel_count: usize) -> usize {
        pixel_count.saturating_add(63) / 64
    }

    /// Bit-index → (word index, bit position within word).
    #[inline]
    fn coords(i: usize) -> (usize, u32) {
        (i / 64, (i % 64) as u32)
    }

    /// Clears the unused high bits of the last word so that the tail-word
    /// invariant holds.  This is a no-op when `width * height` is an exact
    /// multiple of 64.
    fn clear_tail_bits(&mut self) {
        let pixel_count = self.width * self.height;
        let tail = pixel_count % 64;
        if tail != 0 {
            if let Some(last) = self.data.last_mut() {
                // Keep only the lower `tail` bits.
                *last &= (1u64 << tail).wrapping_sub(1);
            }
        }
    }

    /// Validates that `other` has the same dimensions as `self`.
    #[inline]
    fn check_dims(&self, other: &Mask) -> Result<()> {
        if self.width != other.width || self.height != other.height {
            Err(OxiGdalError::InvalidParameter {
                parameter: "other",
                message: format!(
                    "Mask dimension mismatch: self={}×{}, other={}×{}",
                    self.width, self.height, other.width, other.height
                ),
            })
        } else {
            Ok(())
        }
    }
}

// ─── Constructors ─────────────────────────────────────────────────────────────

impl Mask {
    /// Creates a new mask with all bits **unset** (all pixels valid).
    ///
    /// A mask with `width == 0` or `height == 0` is valid and has no pixels.
    #[must_use]
    pub fn new(width: usize, height: usize) -> Self {
        let words = Self::words_for(width * height);
        Self {
            width,
            height,
            data: vec![0u64; words],
        }
    }

    /// Creates a new mask with all bits **set** (all pixels invalid/nodata).
    ///
    /// The tail-word invariant is enforced — high bits of the last word are
    /// always zero even after the fill.
    #[must_use]
    pub fn new_filled(width: usize, height: usize) -> Self {
        let mut mask = Self {
            width,
            height,
            data: vec![!0u64; Self::words_for(width * height)],
        };
        mask.clear_tail_bits();
        mask
    }

    /// Constructs a mask from a `bool` slice.
    ///
    /// The slice is interpreted in row-major order: `values[y * width + x]`
    /// is the mask value for pixel `(x, y)`.  `true` → bit set (nodata).
    ///
    /// # Errors
    ///
    /// Returns [`OxiGdalError::InvalidParameter`] if `values.len() !=
    /// width * height`.
    pub fn from_slice(width: usize, height: usize, values: &[bool]) -> Result<Self> {
        let pixel_count = width * height;
        if values.len() != pixel_count {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "values",
                message: format!(
                    "Slice length {} does not match {}×{} = {} pixels",
                    values.len(),
                    width,
                    height,
                    pixel_count
                ),
            });
        }
        let mut mask = Self::new(width, height);
        for (i, &v) in values.iter().enumerate() {
            if v {
                let (word, bit) = Self::coords(i);
                mask.data[word] |= 1u64 << bit;
            }
        }
        // No tail clearing needed: we only ever set individual bits; the
        // constructor already initialised unused bits to 0.
        Ok(mask)
    }
}

// ─── Getters / basic accessors ────────────────────────────────────────────────

impl Mask {
    /// Width of the raster region in pixels.
    #[must_use]
    #[inline]
    pub const fn width(&self) -> usize {
        self.width
    }

    /// Height of the raster region in pixels.
    #[must_use]
    #[inline]
    pub const fn height(&self) -> usize {
        self.height
    }

    /// Total number of pixels (`width * height`).
    #[must_use]
    #[inline]
    pub fn pixel_count(&self) -> usize {
        self.width * self.height
    }

    /// Returns the mask bit for pixel `(x, y)`.
    ///
    /// `true` means the pixel is **invalid / nodata**.
    ///
    /// # Panics
    ///
    /// Panics in debug builds if `x >= width` or `y >= height`.  In release
    /// builds the access is clamped silently.
    #[must_use]
    pub fn get(&self, x: usize, y: usize) -> bool {
        debug_assert!(
            x < self.width,
            "x={} out of bounds (width={})",
            x,
            self.width
        );
        debug_assert!(
            y < self.height,
            "y={} out of bounds (height={})",
            y,
            self.height
        );
        let i = y * self.width + x;
        let (word, bit) = Self::coords(i);
        if word < self.data.len() {
            (self.data[word] >> bit) & 1 == 1
        } else {
            false
        }
    }

    /// Sets the mask bit for pixel `(x, y)`.
    ///
    /// `value = true` marks the pixel as **invalid / nodata**.
    ///
    /// # Panics
    ///
    /// Panics in debug builds if `x >= width` or `y >= height`.
    pub fn set(&mut self, x: usize, y: usize, value: bool) {
        debug_assert!(
            x < self.width,
            "x={} out of bounds (width={})",
            x,
            self.width
        );
        debug_assert!(
            y < self.height,
            "y={} out of bounds (height={})",
            y,
            self.height
        );
        let i = y * self.width + x;
        let (word, bit) = Self::coords(i);
        if word < self.data.len() {
            if value {
                self.data[word] |= 1u64 << bit;
            } else {
                self.data[word] &= !(1u64 << bit);
            }
        }
    }
}

// ─── Fill operations ──────────────────────────────────────────────────────────

impl Mask {
    /// Sets all bits to `value`.
    ///
    /// The tail-word invariant is maintained.
    pub fn fill(&mut self, value: bool) {
        let fill_word = if value { !0u64 } else { 0u64 };
        for w in &mut self.data {
            *w = fill_word;
        }
        if value {
            self.clear_tail_bits();
        }
    }

    /// Sets all bits in the rectangle `[x, x+w) × [y, y+h)` to `value`.
    ///
    /// Coordinates that fall outside the mask bounds are silently clamped.
    /// The tail-word invariant is maintained.
    pub fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, value: bool) {
        // Clamp rectangle to mask bounds.
        let x_end = (x + w).min(self.width);
        let y_end = (y + h).min(self.height);
        for row in y..y_end {
            for col in x..x_end {
                self.set(col, row, value);
            }
        }
        // `set` never touches tail bits beyond pixel bounds, so no extra
        // clear_tail_bits call is required here.
    }
}

// ─── Bitwise operations ───────────────────────────────────────────────────────

impl Mask {
    /// Applies bitwise AND with `other` in place.
    ///
    /// # Errors
    ///
    /// Returns [`OxiGdalError::InvalidParameter`] if dimensions differ.
    pub fn and_assign(&mut self, other: &Mask) -> Result<()> {
        self.check_dims(other)?;
        for (a, b) in self.data.iter_mut().zip(other.data.iter()) {
            *a &= b;
        }
        Ok(())
    }

    /// Applies bitwise OR with `other` in place.
    ///
    /// # Errors
    ///
    /// Returns [`OxiGdalError::InvalidParameter`] if dimensions differ.
    pub fn or_assign(&mut self, other: &Mask) -> Result<()> {
        self.check_dims(other)?;
        for (a, b) in self.data.iter_mut().zip(other.data.iter()) {
            *a |= b;
        }
        // OR can set tail bits only if `other` has tail bits set, but by our
        // invariant `other`'s tail bits are 0, so no extra clear needed.
        Ok(())
    }

    /// Applies bitwise NOT in place.
    ///
    /// The tail-word invariant is re-established after inversion.
    pub fn not_in_place(&mut self) {
        for w in &mut self.data {
            *w = !*w;
        }
        self.clear_tail_bits();
    }

    /// Applies bitwise XOR with `other` in place.
    ///
    /// # Errors
    ///
    /// Returns [`OxiGdalError::InvalidParameter`] if dimensions differ.
    pub fn xor_assign(&mut self, other: &Mask) -> Result<()> {
        self.check_dims(other)?;
        for (a, b) in self.data.iter_mut().zip(other.data.iter()) {
            *a ^= b;
        }
        // Same argument as or_assign — tail stays zero.
        Ok(())
    }

    /// Returns a new mask that is the bitwise AND of `self` and `other`.
    ///
    /// # Errors
    ///
    /// Returns [`OxiGdalError::InvalidParameter`] if dimensions differ.
    pub fn and(&self, other: &Mask) -> Result<Mask> {
        self.check_dims(other)?;
        let data: Vec<u64> = self
            .data
            .iter()
            .zip(other.data.iter())
            .map(|(a, b)| a & b)
            .collect();
        Ok(Mask {
            width: self.width,
            height: self.height,
            data,
        })
    }

    /// Returns a new mask that is the bitwise OR of `self` and `other`.
    ///
    /// # Errors
    ///
    /// Returns [`OxiGdalError::InvalidParameter`] if dimensions differ.
    pub fn or(&self, other: &Mask) -> Result<Mask> {
        self.check_dims(other)?;
        let data: Vec<u64> = self
            .data
            .iter()
            .zip(other.data.iter())
            .map(|(a, b)| a | b)
            .collect();
        Ok(Mask {
            width: self.width,
            height: self.height,
            data,
        })
    }

    /// Returns a new mask that is the bitwise NOT of `self`.
    ///
    /// The tail-word invariant is preserved.
    #[must_use]
    pub fn not(&self) -> Mask {
        let mut result = Mask {
            width: self.width,
            height: self.height,
            data: self.data.iter().map(|w| !w).collect(),
        };
        result.clear_tail_bits();
        result
    }
}

// ─── Statistics ───────────────────────────────────────────────────────────────

impl Mask {
    /// Number of bits that are set (pixels marked invalid/nodata).
    #[must_use]
    pub fn count_set(&self) -> usize {
        self.data.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// Number of bits that are unset (pixels that are valid).
    #[must_use]
    pub fn count_unset(&self) -> usize {
        self.pixel_count() - self.count_set()
    }

    /// Returns `true` if **all** pixels are masked (all bits set).
    #[must_use]
    pub fn all_set(&self) -> bool {
        self.count_set() == self.pixel_count()
    }

    /// Returns `true` if **no** pixels are masked (all bits unset).
    #[must_use]
    pub fn all_unset(&self) -> bool {
        self.data.iter().all(|&w| w == 0)
    }

    /// Returns `true` if at least one pixel is masked.
    #[must_use]
    pub fn any_set(&self) -> bool {
        self.data.iter().any(|&w| w != 0)
    }
}

// ─── Iterator over set positions ─────────────────────────────────────────────

impl Mask {
    /// Returns an iterator over `(x, y)` coordinates of all **set** bits
    /// (masked/nodata pixels).
    ///
    /// Iterates in row-major order.
    pub fn set_positions(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        SetPositions {
            mask: self,
            word_idx: 0,
            current_word: self.data.first().copied().unwrap_or(0),
            pixel_base: 0,
        }
    }
}

/// Iterator over `(x, y)` positions of set bits.
struct SetPositions<'a> {
    mask: &'a Mask,
    /// Index of the word currently being scanned.
    word_idx: usize,
    /// Copy of the current word with already-visited bits cleared.
    current_word: u64,
    /// Bit index of word `word_idx` bit 0.
    pixel_base: usize,
}

impl<'a> Iterator for SetPositions<'a> {
    type Item = (usize, usize);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.current_word != 0 {
                // Find the lowest set bit.
                let bit = self.current_word.trailing_zeros() as usize;
                // Clear that bit.
                self.current_word &= self.current_word - 1;
                let i = self.pixel_base + bit;
                if i < self.mask.pixel_count() {
                    let x = i % self.mask.width;
                    let y = i / self.mask.width;
                    return Some((x, y));
                }
                // Bit was in the tail padding — skip.
                continue;
            }
            // Advance to the next word.
            self.word_idx += 1;
            if self.word_idx >= self.mask.data.len() {
                return None;
            }
            self.pixel_base = self.word_idx * 64;
            self.current_word = self.mask.data[self.word_idx];
        }
    }
}

// ─── Nodata-based constructors ────────────────────────────────────────────────

impl Mask {
    /// Builds a nodata mask from an `f32` slice.
    ///
    /// A pixel is marked **invalid** if its value is NaN **or** exactly equals
    /// `nodata` (bitwise-exact comparison after handling NaN).
    ///
    /// # Errors
    ///
    /// Returns [`OxiGdalError::InvalidParameter`] if `data.len() != width *
    /// height`.
    pub fn from_nodata_f32(data: &[f32], width: usize, height: usize, nodata: f32) -> Result<Self> {
        let pixel_count = width * height;
        if data.len() != pixel_count {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "data",
                message: format!(
                    "Slice length {} ≠ {}×{} = {} pixels",
                    data.len(),
                    width,
                    height,
                    pixel_count
                ),
            });
        }
        let nodata_is_nan = nodata.is_nan();
        let mut mask = Self::new(width, height);
        for (i, &v) in data.iter().enumerate() {
            let is_nodata = if nodata_is_nan {
                v.is_nan()
            } else {
                v == nodata
            };
            if is_nodata {
                let (word, bit) = Self::coords(i);
                mask.data[word] |= 1u64 << bit;
            }
        }
        Ok(mask)
    }

    /// Builds a nodata mask from an `f64` slice.
    ///
    /// A pixel is marked **invalid** if its value is NaN **or** exactly equals
    /// `nodata`.
    ///
    /// # Errors
    ///
    /// Returns [`OxiGdalError::InvalidParameter`] if `data.len() != width *
    /// height`.
    pub fn from_nodata_f64(data: &[f64], width: usize, height: usize, nodata: f64) -> Result<Self> {
        let pixel_count = width * height;
        if data.len() != pixel_count {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "data",
                message: format!(
                    "Slice length {} ≠ {}×{} = {} pixels",
                    data.len(),
                    width,
                    height,
                    pixel_count
                ),
            });
        }
        let nodata_is_nan = nodata.is_nan();
        let mut mask = Self::new(width, height);
        for (i, &v) in data.iter().enumerate() {
            let is_nodata = if nodata_is_nan {
                v.is_nan()
            } else {
                v == nodata
            };
            if is_nodata {
                let (word, bit) = Self::coords(i);
                mask.data[word] |= 1u64 << bit;
            }
        }
        Ok(mask)
    }

    /// Builds a nodata mask from an `i32` slice.
    ///
    /// A pixel is marked **invalid** if its value exactly equals `nodata`.
    ///
    /// # Errors
    ///
    /// Returns [`OxiGdalError::InvalidParameter`] if `data.len() != width *
    /// height`.
    pub fn from_nodata_i32(data: &[i32], width: usize, height: usize, nodata: i32) -> Result<Self> {
        let pixel_count = width * height;
        if data.len() != pixel_count {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "data",
                message: format!(
                    "Slice length {} ≠ {}×{} = {} pixels",
                    data.len(),
                    width,
                    height,
                    pixel_count
                ),
            });
        }
        let mut mask = Self::new(width, height);
        for (i, &v) in data.iter().enumerate() {
            if v == nodata {
                let (word, bit) = Self::coords(i);
                mask.data[word] |= 1u64 << bit;
            }
        }
        Ok(mask)
    }

    /// Builds a nodata mask from a `u8` slice.
    ///
    /// A pixel is marked **invalid** if its value exactly equals `nodata`.
    ///
    /// # Errors
    ///
    /// Returns [`OxiGdalError::InvalidParameter`] if `data.len() != width *
    /// height`.
    pub fn from_nodata_u8(data: &[u8], width: usize, height: usize, nodata: u8) -> Result<Self> {
        let pixel_count = width * height;
        if data.len() != pixel_count {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "data",
                message: format!(
                    "Slice length {} ≠ {}×{} = {} pixels",
                    data.len(),
                    width,
                    height,
                    pixel_count
                ),
            });
        }
        let mut mask = Self::new(width, height);
        for (i, &v) in data.iter().enumerate() {
            if v == nodata {
                let (word, bit) = Self::coords(i);
                mask.data[word] |= 1u64 << bit;
            }
        }
        Ok(mask)
    }
}

// ─── Interop ──────────────────────────────────────────────────────────────────

impl Mask {
    /// Converts the mask to a `Vec<bool>` in row-major order.
    ///
    /// The returned vector has length `pixel_count()`.  `true` at index
    /// `y * width + x` means pixel `(x, y)` is **invalid / nodata**.
    #[must_use]
    pub fn to_bool_vec(&self) -> Vec<bool> {
        let n = self.pixel_count();
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let (word, bit) = Self::coords(i);
            out.push((self.data[word] >> bit) & 1 == 1);
        }
        out
    }
}

// ─── Apply-to-data methods ────────────────────────────────────────────────────

impl Mask {
    /// Replaces every masked pixel in `data` with `nodata_value`.
    ///
    /// Operates on an `f32` slice of the same shape as the mask.
    /// Pixels where the mask bit is **set** (invalid) are written with
    /// `nodata_value`.
    pub fn apply_to_f32(&self, data: &mut [f32], nodata_value: f32) {
        let n = self.pixel_count().min(data.len());
        for (i, elem) in data.iter_mut().enumerate().take(n) {
            let (word, bit) = Self::coords(i);
            if word < self.data.len() && (self.data[word] >> bit) & 1 == 1 {
                *elem = nodata_value;
            }
        }
    }

    /// Replaces every masked pixel in `data` with `nodata_value`.
    ///
    /// Operates on an `f64` slice of the same shape as the mask.
    pub fn apply_to_f64(&self, data: &mut [f64], nodata_value: f64) {
        let n = self.pixel_count().min(data.len());
        for (i, elem) in data.iter_mut().enumerate().take(n) {
            let (word, bit) = Self::coords(i);
            if word < self.data.len() && (self.data[word] >> bit) & 1 == 1 {
                *elem = nodata_value;
            }
        }
    }

    /// Replaces every masked pixel in `data` with `nodata_value`.
    ///
    /// Operates on a `u8` slice of the same shape as the mask.
    pub fn apply_to_u8(&self, data: &mut [u8], nodata_value: u8) {
        let n = self.pixel_count().min(data.len());
        for (i, elem) in data.iter_mut().enumerate().take(n) {
            let (word, bit) = Self::coords(i);
            if word < self.data.len() && (self.data[word] >> bit) & 1 == 1 {
                *elem = nodata_value;
            }
        }
    }

    /// Replaces every masked pixel in `data` with `nodata_value`.
    ///
    /// Operates on an `i32` slice of the same shape as the mask.
    pub fn apply_to_i32(&self, data: &mut [i32], nodata_value: i32) {
        let n = self.pixel_count().min(data.len());
        for (i, elem) in data.iter_mut().enumerate().take(n) {
            let (word, bit) = Self::coords(i);
            if word < self.data.len() && (self.data[word] >> bit) & 1 == 1 {
                *elem = nodata_value;
            }
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    // ── Basic construction ─────────────────────────────────────────────────

    #[test]
    fn test_mask_basic_set_get() {
        let mut mask = Mask::new(8, 8);
        // All zero initially.
        for y in 0..8 {
            for x in 0..8 {
                assert!(!mask.get(x, y), "({x},{y}) should be unset");
            }
        }
        mask.set(0, 0, true);
        mask.set(7, 7, true);
        mask.set(3, 5, true);
        assert!(mask.get(0, 0));
        assert!(mask.get(7, 7));
        assert!(mask.get(3, 5));
        assert!(!mask.get(1, 1));
        assert_eq!(mask.count_set(), 3);
    }

    #[test]
    fn test_mask_fill_true() {
        let mut mask = Mask::new(10, 10);
        mask.fill(true);
        assert_eq!(mask.count_set(), 100);
        assert_eq!(mask.count_unset(), 0);
    }

    #[test]
    fn test_mask_fill_false() {
        let mut mask = Mask::new_filled(5, 5);
        mask.fill(false);
        assert_eq!(mask.count_set(), 0);
        assert!(mask.all_unset());
    }

    #[test]
    fn test_mask_fill_rect() {
        let mut mask = Mask::new(10, 10);
        // Fill the 3×3 block starting at (2,2).
        mask.fill_rect(2, 2, 3, 3, true);
        assert_eq!(mask.count_set(), 9);
        // Verify interior.
        for dy in 0..3 {
            for dx in 0..3 {
                assert!(mask.get(2 + dx, 2 + dy));
            }
        }
        // Verify boundary untouched.
        assert!(!mask.get(1, 2));
        assert!(!mask.get(5, 5));
    }

    // ── Bitwise operations ─────────────────────────────────────────────────

    #[test]
    fn test_mask_and_or_not() {
        let mut a = Mask::new(4, 4); // all 0
        a.set(0, 0, true);
        a.set(1, 1, true);

        let mut b = Mask::new(4, 4); // all 0
        b.set(1, 1, true);
        b.set(2, 2, true);

        // AND: only (1,1) common.
        let and = a.and(&b).expect("and should succeed");
        assert_eq!(and.count_set(), 1);
        assert!(and.get(1, 1));

        // OR: (0,0),(1,1),(2,2) = 3 pixels.
        let or = a.or(&b).expect("or should succeed");
        assert_eq!(or.count_set(), 3);

        // NOT of a: 16 - 2 = 14 set.
        let not_a = a.not();
        assert_eq!(not_a.count_set(), 14);
        assert!(!not_a.get(0, 0));
        assert!(!not_a.get(1, 1));
        assert!(not_a.get(0, 1));
    }

    #[test]
    fn test_mask_dimension_mismatch_err() {
        let mut a = Mask::new(4, 4);
        let b = Mask::new(4, 5);
        assert!(a.and_assign(&b).is_err());
        assert!(a.or_assign(&b).is_err());
        assert!(a.xor_assign(&b).is_err());
        assert!(a.and(&b).is_err());
        assert!(a.or(&b).is_err());
    }

    // ── from_slice roundtrip ───────────────────────────────────────────────

    #[test]
    fn test_mask_from_slice_roundtrip() {
        let values: Vec<bool> = (0..20usize).map(|i| i % 3 == 0).collect();
        let mask = Mask::from_slice(4, 5, &values).expect("from_slice");
        let back = mask.to_bool_vec();
        assert_eq!(back, values);
    }

    #[test]
    fn test_mask_from_slice_length_err() {
        // Wrong length should error.
        assert!(Mask::from_slice(4, 4, &[true; 10]).is_err());
    }

    // ── Nodata constructors ────────────────────────────────────────────────

    #[test]
    fn test_mask_from_nodata_f32() {
        let nodata = -9999.0f32;
        let data = vec![1.0f32, nodata, f32::NAN, 3.0, nodata];
        let mask = Mask::from_nodata_f32(&data, 5, 1, nodata).expect("from_nodata_f32");
        // index 1 and 4 = exact nodata, index 2 = NaN (only for NaN nodata)
        // nodata is -9999 (not NaN), so NaN at index 2 is NOT marked.
        assert!(!mask.get(0, 0));
        assert!(mask.get(1, 0));
        assert!(!mask.get(2, 0)); // NaN but nodata != NaN → not masked
        assert!(!mask.get(3, 0));
        assert!(mask.get(4, 0));
        assert_eq!(mask.count_set(), 2);
    }

    #[test]
    fn test_mask_from_nodata_f32_nan_nodata() {
        // When nodata itself is NaN, NaN pixels are masked; non-NaN are not.
        let data = vec![1.0f32, f32::NAN, 2.0, f32::NAN];
        let mask = Mask::from_nodata_f32(&data, 4, 1, f32::NAN).expect("from_nodata_f32 NaN");
        assert!(!mask.get(0, 0));
        assert!(mask.get(1, 0));
        assert!(!mask.get(2, 0));
        assert!(mask.get(3, 0));
    }

    #[test]
    fn test_mask_from_nodata_u8_zero() {
        let data: Vec<u8> = vec![0, 1, 0, 5, 0];
        let mask = Mask::from_nodata_u8(&data, 5, 1, 0).expect("from_nodata_u8");
        assert!(mask.get(0, 0));
        assert!(!mask.get(1, 0));
        assert!(mask.get(2, 0));
        assert!(!mask.get(3, 0));
        assert!(mask.get(4, 0));
        assert_eq!(mask.count_set(), 3);
    }

    // ── apply_to_* ─────────────────────────────────────────────────────────

    #[test]
    fn test_mask_apply_to_f32() {
        let mut mask = Mask::new(4, 1);
        mask.set(1, 0, true);
        mask.set(3, 0, true);
        let mut data = vec![1.0f32, 2.0, 3.0, 4.0];
        mask.apply_to_f32(&mut data, -9999.0);
        assert!((data[0] - 1.0).abs() < 1e-7);
        assert!((data[1] - (-9999.0)).abs() < 1e-1);
        assert!((data[2] - 3.0).abs() < 1e-7);
        assert!((data[3] - (-9999.0)).abs() < 1e-1);
    }

    #[test]
    fn test_mask_apply_to_u8() {
        let mut mask = Mask::new(3, 1);
        mask.set(0, 0, true);
        mask.set(2, 0, true);
        let mut data: Vec<u8> = vec![10, 20, 30];
        mask.apply_to_u8(&mut data, 0);
        assert_eq!(data, vec![0, 20, 0]);
    }

    // ── count_set / popcount ────────────────────────────────────────────────

    #[test]
    fn test_mask_count_set_checkerboard() {
        // 8×8 checkerboard: even bit indices set.
        let values: Vec<bool> = (0..64usize).map(|i| i % 2 == 0).collect();
        let mask = Mask::from_slice(8, 8, &values).expect("from_slice");
        assert_eq!(mask.count_set(), 32);
        assert_eq!(mask.count_unset(), 32);
    }

    // ── set_positions iterator ─────────────────────────────────────────────

    #[test]
    fn test_mask_set_positions_iterator() {
        let mut mask = Mask::new(5, 5);
        let positions = [(1usize, 0usize), (4, 2), (0, 4)];
        for &(x, y) in &positions {
            mask.set(x, y, true);
        }
        let mut collected: Vec<(usize, usize)> = mask.set_positions().collect();
        collected.sort_unstable();
        let mut expected = positions.to_vec();
        expected.sort_unstable();
        assert_eq!(collected, expected);
    }

    // ── all_set / all_unset edge cases ─────────────────────────────────────

    #[test]
    fn test_mask_all_set_all_unset() {
        let empty = Mask::new(0, 0);
        // Zero-pixel mask: both are vacuously true.
        assert!(empty.all_set());
        assert!(empty.all_unset());

        let zeros = Mask::new(10, 10);
        assert!(zeros.all_unset());
        assert!(!zeros.all_set());

        let ones = Mask::new_filled(10, 10);
        assert!(ones.all_set());
        assert!(!ones.all_unset());
    }

    // ── Word-boundary correctness ──────────────────────────────────────────

    #[test]
    fn test_mask_large_width_crosses_word_boundary() {
        // width=65 means pixel (64, 0) is in word 1, bit 0.
        let mut mask = Mask::new(65, 1);
        mask.set(63, 0, true); // last bit of word 0
        mask.set(64, 0, true); // first bit of word 1
        assert!(mask.get(63, 0));
        assert!(mask.get(64, 0));
        assert!(!mask.get(0, 0));
        assert_eq!(mask.count_set(), 2);
    }

    // ── Single-pixel edge case ─────────────────────────────────────────────

    #[test]
    fn test_mask_single_pixel() {
        let mut mask = Mask::new(1, 1);
        assert!(!mask.get(0, 0));
        assert!(mask.all_unset());
        mask.set(0, 0, true);
        assert!(mask.get(0, 0));
        assert!(mask.all_set());
        assert_eq!(mask.count_set(), 1);
        mask.not_in_place();
        assert!(!mask.get(0, 0));
        assert!(mask.all_unset());
    }

    // ── XOR ────────────────────────────────────────────────────────────────

    #[test]
    fn test_mask_xor_self_is_zero() {
        let mut mask = Mask::new(6, 6);
        mask.fill_rect(0, 0, 3, 3, true);
        let clone = mask.clone();
        mask.xor_assign(&clone).expect("xor_assign");
        assert!(mask.all_unset());
    }

    // ── new_filled tail-word invariant ─────────────────────────────────────

    #[test]
    fn test_mask_new_filled_all_set() {
        // Non-multiple-of-64 dimensions.
        let mask = Mask::new_filled(9, 7); // 63 pixels
        assert_eq!(mask.pixel_count(), 63);
        assert_eq!(mask.count_set(), 63);
        assert!(mask.all_set());
    }

    #[test]
    fn test_mask_not_in_place_tail_invariant() {
        let mut mask = Mask::new(9, 7); // 63 pixels
        mask.not_in_place(); // should give all-set
        assert_eq!(mask.count_set(), 63);
        mask.not_in_place(); // back to all-unset
        assert_eq!(mask.count_set(), 0);
    }

    // ── or_assign idempotent ───────────────────────────────────────────────

    #[test]
    fn test_mask_or_idempotent() {
        let mut a = Mask::new(4, 4);
        a.fill_rect(0, 0, 2, 2, true);
        let b = a.clone();
        a.or_assign(&b).expect("or_assign");
        assert_eq!(a.count_set(), 4);
    }

    // ── to_bool_vec ─────────────────────────────────────────────────────────

    #[test]
    fn test_mask_to_bool_vec_roundtrip() {
        let mask = Mask::new_filled(5, 3);
        let bv = mask.to_bool_vec();
        assert_eq!(bv.len(), 15);
        assert!(bv.iter().all(|&b| b));
    }
}
