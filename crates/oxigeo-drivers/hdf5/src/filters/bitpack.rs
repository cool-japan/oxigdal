//! Bit packing and unpacking utilities for HDF5 filters.
//!
//! Provides `BitWriter` for packing arbitrary-width values into a byte stream
//! and `BitReader` for unpacking them. Uses MSB-first (big-endian) bit ordering,
//! which is the standard for HDF5 filter compressed data.

use crate::error::{Hdf5Error, Result};

/// Writes individual bits into a byte buffer, MSB-first.
///
/// Bits are accumulated in a byte from the most significant bit downward.
/// When 8 bits have been accumulated, the byte is flushed to the output buffer.
/// Call `finish()` to flush any remaining partial byte (padded with zeros on the right).
pub struct BitWriter {
    /// Accumulated output bytes
    buffer: Vec<u8>,
    /// Current byte being assembled
    current_byte: u8,
    /// Number of bits written into `current_byte` (0..=7)
    bit_count: u8,
}

impl BitWriter {
    /// Create a new BitWriter with default capacity
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            current_byte: 0,
            bit_count: 0,
        }
    }

    /// Create a new BitWriter with pre-allocated capacity in bytes
    pub fn with_capacity(byte_capacity: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(byte_capacity),
            current_byte: 0,
            bit_count: 0,
        }
    }

    /// Write a single bit (0 or 1)
    #[inline]
    pub fn write_bit(&mut self, bit: bool) {
        self.current_byte = (self.current_byte << 1) | (bit as u8);
        self.bit_count += 1;
        if self.bit_count == 8 {
            self.buffer.push(self.current_byte);
            self.current_byte = 0;
            self.bit_count = 0;
        }
    }

    /// Write `num_bits` bits from `value`, MSB-first.
    ///
    /// Only the lowest `num_bits` bits of `value` are written.
    /// `num_bits` must be in the range 0..=64.
    pub fn write_bits(&mut self, value: u64, num_bits: u8) {
        debug_assert!(num_bits <= 64);
        if num_bits == 0 {
            return;
        }
        for i in (0..num_bits).rev() {
            let bit = ((value >> i) & 1) != 0;
            self.write_bit(bit);
        }
    }

    /// Write a unary-coded non-negative integer.
    ///
    /// Encodes `n` as `n` zero bits followed by a single one bit.
    /// For example: 0 -> "1", 1 -> "01", 2 -> "001", 3 -> "0001".
    pub fn write_unary(&mut self, n: u64) {
        for _ in 0..n {
            self.write_bit(false);
        }
        self.write_bit(true);
    }

    /// Write a Rice-coded unsigned integer.
    ///
    /// The value is split into a quotient (encoded in unary) and
    /// a remainder (encoded in `k` fixed bits).
    pub fn write_rice(&mut self, value: u64, k: u8) {
        let quotient = value >> k;
        let remainder = value & ((1u64 << k) - 1);
        self.write_unary(quotient);
        if k > 0 {
            self.write_bits(remainder, k);
        }
    }

    /// Get the number of bits written so far (including unflushed partial byte)
    pub fn bits_written(&self) -> usize {
        self.buffer.len() * 8 + self.bit_count as usize
    }

    /// Finish writing and return the byte buffer.
    ///
    /// If there is a partial byte, it is padded with zero bits on the right
    /// (least significant positions) and appended.
    pub fn finish(mut self) -> Vec<u8> {
        if self.bit_count > 0 {
            // Pad remaining bits to the left (MSB-first)
            self.current_byte <<= 8 - self.bit_count;
            self.buffer.push(self.current_byte);
        }
        self.buffer
    }
}

impl Default for BitWriter {
    fn default() -> Self {
        Self::new()
    }
}

/// Reads individual bits from a byte buffer, MSB-first.
///
/// Bits are read from the most significant bit of each byte downward.
pub struct BitReader<'a> {
    /// Source data
    data: &'a [u8],
    /// Current byte index
    byte_pos: usize,
    /// Current bit position within the byte (0=MSB, 7=LSB)
    bit_pos: u8,
}

impl<'a> BitReader<'a> {
    /// Create a new BitReader over the given data
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte_pos: 0,
            bit_pos: 0,
        }
    }

    /// Read a single bit, returning true for 1 and false for 0
    #[inline]
    pub fn read_bit(&mut self) -> Result<bool> {
        if self.byte_pos >= self.data.len() {
            return Err(Hdf5Error::Decompression(
                "Unexpected end of bit stream".to_string(),
            ));
        }
        let bit = (self.data[self.byte_pos] >> (7 - self.bit_pos)) & 1;
        self.bit_pos += 1;
        if self.bit_pos == 8 {
            self.byte_pos += 1;
            self.bit_pos = 0;
        }
        Ok(bit != 0)
    }

    /// Read `num_bits` bits and return as a u64, MSB-first.
    ///
    /// `num_bits` must be in the range 0..=64.
    pub fn read_bits(&mut self, num_bits: u8) -> Result<u64> {
        debug_assert!(num_bits <= 64);
        if num_bits == 0 {
            return Ok(0);
        }
        let mut value: u64 = 0;
        for _ in 0..num_bits {
            let bit = self.read_bit()?;
            value = (value << 1) | (bit as u64);
        }
        Ok(value)
    }

    /// Read a unary-coded non-negative integer.
    ///
    /// Counts zero bits until a one bit is encountered.
    /// The one bit is consumed but not included in the count.
    pub fn read_unary(&mut self) -> Result<u64> {
        let mut count: u64 = 0;
        loop {
            let bit = self.read_bit()?;
            if bit {
                return Ok(count);
            }
            count += 1;
            // Safety limit to prevent infinite loops on corrupt data
            if count > 1_000_000 {
                return Err(Hdf5Error::Decompression(
                    "Unary value exceeds safety limit (possible corrupt data)".to_string(),
                ));
            }
        }
    }

    /// Read a Rice-coded unsigned integer.
    ///
    /// Reads the quotient (unary-coded) and remainder (`k` fixed bits),
    /// then reconstructs the value as `(quotient << k) | remainder`.
    pub fn read_rice(&mut self, k: u8) -> Result<u64> {
        let quotient = self.read_unary()?;
        let remainder = if k > 0 { self.read_bits(k)? } else { 0 };
        Ok((quotient << k) | remainder)
    }

    /// Get the total number of bits consumed so far
    pub fn bits_consumed(&self) -> usize {
        self.byte_pos * 8 + self.bit_pos as usize
    }

    /// Check if there are remaining bits to read
    pub fn has_remaining(&self) -> bool {
        self.byte_pos < self.data.len()
    }
}

/// Compute the minimum number of bits needed to represent a value.
///
/// Returns 1 for value 0 (need at least 1 bit), and `ceil(log2(value+1))` otherwise.
pub fn min_bits_for_value(value: u64) -> u8 {
    if value == 0 {
        return 1;
    }
    (64 - value.leading_zeros()) as u8
}

/// Zigzag encode a signed i64 to unsigned u64.
///
/// Maps negative values to odd numbers and non-negative to even numbers:
/// 0 -> 0, -1 -> 1, 1 -> 2, -2 -> 3, 2 -> 4, ...
pub fn zigzag_encode(value: i64) -> u64 {
    ((value << 1) ^ (value >> 63)) as u64
}

/// Zigzag decode an unsigned u64 back to signed i64.
pub fn zigzag_decode(value: u64) -> i64 {
    ((value >> 1) as i64) ^ (-((value & 1) as i64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bitwriter_basic() {
        let mut writer = BitWriter::new();
        // Write 0b1010 in 4 bits
        writer.write_bits(0b1010, 4);
        // Write 0b1100 in 4 bits
        writer.write_bits(0b1100, 4);
        let result = writer.finish();
        assert_eq!(result, vec![0b10101100]);
    }

    #[test]
    fn test_bitwriter_partial_byte() {
        let mut writer = BitWriter::new();
        writer.write_bits(0b101, 3);
        let result = writer.finish();
        // 101 padded to 10100000
        assert_eq!(result, vec![0b10100000]);
    }

    #[test]
    fn test_bitwriter_multi_byte() {
        let mut writer = BitWriter::new();
        writer.write_bits(0b1111_0000_1010, 12);
        let result = writer.finish();
        // 1111_0000 | 1010_0000 (padded)
        assert_eq!(result, vec![0b11110000, 0b10100000]);
    }

    #[test]
    fn test_bitreader_basic() {
        let data = vec![0b10101100];
        let mut reader = BitReader::new(&data);
        assert_eq!(reader.read_bits(4).expect("read failed"), 0b1010);
        assert_eq!(reader.read_bits(4).expect("read failed"), 0b1100);
    }

    #[test]
    fn test_bitreader_cross_byte() {
        let data = vec![0b11110000, 0b10100000];
        let mut reader = BitReader::new(&data);
        assert_eq!(reader.read_bits(12).expect("read failed"), 0b1111_0000_1010);
    }

    #[test]
    fn test_roundtrip_bits() {
        let values = [42u64, 0, 1, 255, 1023, 65535, 100000];
        for &val in &values {
            let bits = min_bits_for_value(val);
            let mut writer = BitWriter::new();
            writer.write_bits(val, bits);
            let data = writer.finish();
            let mut reader = BitReader::new(&data);
            let decoded = reader.read_bits(bits).expect("read failed");
            assert_eq!(decoded, val, "Failed roundtrip for value {val}");
        }
    }

    #[test]
    fn test_unary_coding() {
        for n in 0..20u64 {
            let mut writer = BitWriter::new();
            writer.write_unary(n);
            let data = writer.finish();
            let mut reader = BitReader::new(&data);
            let decoded = reader.read_unary().expect("read failed");
            assert_eq!(decoded, n, "Failed unary roundtrip for {n}");
        }
    }

    #[test]
    fn test_rice_coding() {
        for k in 0..6u8 {
            for val in 0..100u64 {
                let mut writer = BitWriter::new();
                writer.write_rice(val, k);
                let data = writer.finish();
                let mut reader = BitReader::new(&data);
                let decoded = reader.read_rice(k).expect("read failed");
                assert_eq!(decoded, val, "Failed rice roundtrip for val={val}, k={k}");
            }
        }
    }

    #[test]
    fn test_min_bits_for_value() {
        assert_eq!(min_bits_for_value(0), 1);
        assert_eq!(min_bits_for_value(1), 1);
        assert_eq!(min_bits_for_value(2), 2);
        assert_eq!(min_bits_for_value(3), 2);
        assert_eq!(min_bits_for_value(4), 3);
        assert_eq!(min_bits_for_value(7), 3);
        assert_eq!(min_bits_for_value(8), 4);
        assert_eq!(min_bits_for_value(255), 8);
        assert_eq!(min_bits_for_value(256), 9);
        assert_eq!(min_bits_for_value(u64::MAX), 64);
    }

    #[test]
    fn test_zigzag_roundtrip() {
        let test_values: Vec<i64> =
            vec![0, 1, -1, 2, -2, 127, -128, 1000, -1000, i64::MAX, i64::MIN];
        for &val in &test_values {
            let encoded = zigzag_encode(val);
            let decoded = zigzag_decode(encoded);
            assert_eq!(decoded, val, "Failed zigzag roundtrip for {val}");
        }
    }

    #[test]
    fn test_zigzag_ordering() {
        // zigzag should map: 0->0, -1->1, 1->2, -2->3, 2->4
        assert_eq!(zigzag_encode(0), 0);
        assert_eq!(zigzag_encode(-1), 1);
        assert_eq!(zigzag_encode(1), 2);
        assert_eq!(zigzag_encode(-2), 3);
        assert_eq!(zigzag_encode(2), 4);
    }

    #[test]
    fn test_bits_written_consumed() {
        let mut writer = BitWriter::new();
        assert_eq!(writer.bits_written(), 0);
        writer.write_bits(0xFF, 8);
        assert_eq!(writer.bits_written(), 8);
        writer.write_bits(0b101, 3);
        assert_eq!(writer.bits_written(), 11);
        let data = writer.finish();

        let mut reader = BitReader::new(&data);
        assert_eq!(reader.bits_consumed(), 0);
        let _ = reader.read_bits(8).expect("read failed");
        assert_eq!(reader.bits_consumed(), 8);
        let _ = reader.read_bits(3).expect("read failed");
        assert_eq!(reader.bits_consumed(), 11);
    }
}
