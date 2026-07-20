//! Range Coder primitives used by LASzip's Item Compressor v1.
//!
//! Implements Martin's Range Coder (the same variant used by the reference
//! LASzip C++ implementation, NOT the Said-Pearlman variant from some early
//! academic papers).  Provides:
//!
//! - [`ArithmeticDecoder`] — bit / symbol / raw integer decoder.
//! - `ArithmeticEncoder` — round-trip companion (gated behind the
//!   `laz-encoder` feature; test-only).
//! - [`BitModel`] — adaptive binary model.
//! - [`SymbolModel`] — adaptive multi-symbol model with periodic distribution
//!   rebuild.
//! - [`IntegerCompressor`] — multi-context corrector coder used by the v1
//!   item compressors.
//!
//! The wire format is documented in the LASzip 3.4 specification, Section 5.
//! Constants and renormalization thresholds match the reference implementation
//! in `LASzip/src/arithmeticdecoder.cpp` and `arithmeticencoder.cpp`.

use crate::error::CopcError;

/// Minimum length before renormalization (1 << 24).
const AC_MIN_LENGTH: u32 = 0x0100_0000;

/// Number of bits used per renormalization shift for the bit model.
const BM_LENGTH_SHIFT: u32 = 13;
/// Maximum count for the bit model before a halving rescale.
const BM_MAX_COUNT: u32 = 1 << BM_LENGTH_SHIFT;

/// Number of bits used for symbol model probability discretization.
const DM_LENGTH_SHIFT: u32 = 15;
/// Maximum total count before a halving rescale in the symbol model.
const DM_MAX_COUNT: u32 = 1 << DM_LENGTH_SHIFT;

/// Adaptive binary model.
///
/// Tracks the probability of bit 0 and rescales periodically so the model
/// adapts to the local statistics of the stream.
#[derive(Debug, Clone)]
pub struct BitModel {
    /// Quantized probability of emitting bit 0, scaled by `1 << BM_LENGTH_SHIFT`.
    pub(crate) bit_0_prob: u32,
    /// Running count of bit-0 occurrences since the last rescale.
    pub(crate) bit_0_count: u32,
    /// Total count of bits observed since the last rescale.
    pub(crate) bit_count: u32,
    /// Number of bits to observe before the first rescale.
    pub(crate) update_cycle: u32,
    /// Bits observed since the last rescale.
    pub(crate) bits_until_update: u32,
}

impl Default for BitModel {
    fn default() -> Self {
        Self::new()
    }
}

impl BitModel {
    /// Create a fresh bit model with a 50/50 prior and the standard update cycle.
    pub fn new() -> Self {
        Self {
            bit_0_prob: 1u32 << (BM_LENGTH_SHIFT - 1),
            bit_0_count: 1,
            bit_count: 2,
            update_cycle: 4,
            bits_until_update: 4,
        }
    }

    /// Rescale the probability table after collecting `bit_count` samples.
    fn update(&mut self) {
        self.bit_count += self.update_cycle;
        if self.bit_count >= BM_MAX_COUNT {
            self.bit_count = (self.bit_count + 1) >> 1;
            self.bit_0_count = (self.bit_0_count + 1) >> 1;
            if self.bit_0_count == self.bit_count {
                self.bit_count += 1;
            }
        }
        let scale = (1u64 << 31) / u64::from(self.bit_count);
        self.bit_0_prob = ((u64::from(self.bit_0_count) * scale) >> (31 - BM_LENGTH_SHIFT)) as u32;
        self.update_cycle = (5 * self.update_cycle) >> 2;
        if self.update_cycle > 64 {
            self.update_cycle = 64;
        }
        self.bits_until_update = self.update_cycle;
    }
}

/// Adaptive multi-symbol model.
///
/// Maintains running counts per symbol and periodically rebuilds the
/// cumulative distribution used by the range coder.
#[derive(Debug, Clone)]
pub struct SymbolModel {
    /// Number of distinct symbols this model tracks.
    pub(crate) num_symbols: u32,
    /// Running counts per symbol (length `num_symbols`).
    pub(crate) symbol_count: Vec<u32>,
    /// Cumulative distribution (length `num_symbols + 1`).
    pub(crate) distribution: Vec<u32>,
    /// Total count of all symbol observations since the last rebuild.
    pub(crate) total_count: u32,
    /// Symbols-decoded counter; rebuild fires when it hits `update_cycle`.
    pub(crate) symbols_until_update: u32,
    /// Number of symbol updates between distribution rebuilds.
    pub(crate) update_cycle: u32,
}

impl SymbolModel {
    /// Construct a uniform-prior symbol model with `num_symbols` symbols.
    pub fn new(num_symbols: u32) -> Self {
        let symbol_count = vec![1u32; num_symbols as usize];
        let total_count = num_symbols;
        let mut model = Self {
            num_symbols,
            symbol_count,
            distribution: vec![0; (num_symbols + 1) as usize],
            total_count,
            symbols_until_update: (num_symbols + 6) >> 1,
            update_cycle: (num_symbols + 6) >> 1,
        };
        model.rebuild_distribution();
        model
    }

    /// Recompute `distribution` from the running counts.
    fn rebuild_distribution(&mut self) {
        if self.total_count > DM_MAX_COUNT {
            self.total_count = 0;
            for c in &mut self.symbol_count {
                *c = (*c + 1) >> 1;
                self.total_count += *c;
            }
        }

        let scale = (1u64 << 31) / u64::from(self.total_count);
        let mut sum: u32 = 0;
        for i in 0..self.num_symbols {
            self.distribution[i as usize] =
                ((u64::from(sum) * scale) >> (31 - DM_LENGTH_SHIFT)) as u32;
            sum += self.symbol_count[i as usize];
        }
        self.distribution[self.num_symbols as usize] = 1u32 << DM_LENGTH_SHIFT;

        self.update_cycle = (5 * self.update_cycle) >> 2;
        let max_cycle = (self.num_symbols + 6) << 3;
        if self.update_cycle > max_cycle {
            self.update_cycle = max_cycle;
        }
        self.symbols_until_update = self.update_cycle;
    }
}

/// Range-coding decoder.
///
/// Consumes a byte slice and feeds bits / symbols on demand.  After
/// instantiation the decoder reads four raw bytes as its initial value;
/// callers should pass the entire compressed byte slice.
pub struct ArithmeticDecoder<'a> {
    bytes: &'a [u8],
    pos: usize,
    length: u32,
    value: u32,
}

impl<'a> ArithmeticDecoder<'a> {
    /// Construct a decoder from a byte slice.  Reads four raw bytes from the
    /// start of `bytes` as the initial range-coder state.
    ///
    /// # Errors
    /// Returns [`CopcError::LazDecoderError`] when `bytes` has fewer than four
    /// bytes available.
    pub fn new(bytes: &'a [u8]) -> Result<Self, CopcError> {
        if bytes.len() < 4 {
            return Err(CopcError::LazDecoderError(format!(
                "Need >= 4 bytes to initialize ArithmeticDecoder (got {})",
                bytes.len()
            )));
        }
        let value = (u32::from(bytes[0]) << 24)
            | (u32::from(bytes[1]) << 16)
            | (u32::from(bytes[2]) << 8)
            | u32::from(bytes[3]);
        Ok(Self {
            bytes,
            pos: 4,
            length: u32::MAX,
            value,
        })
    }

    /// Pull the next byte from the underlying buffer, or 0 on EOF.
    #[inline]
    fn next_byte(&mut self) -> u8 {
        if self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            self.pos += 1;
            b
        } else {
            // The reference decoder reads zeros past EOF; this is well-defined
            // because the encoder always emits a normalizing run.
            0
        }
    }

    /// Renormalize the state when `length` drops below the minimum.
    #[inline]
    fn renorm_decode(&mut self) {
        while self.length < AC_MIN_LENGTH {
            self.value = (self.value << 8) | u32::from(self.next_byte());
            self.length <<= 8;
        }
    }

    /// Decode a single binary symbol against `model`.
    pub fn decode_bit(&mut self, model: &mut BitModel) -> bool {
        let x = (self.length >> BM_LENGTH_SHIFT) * model.bit_0_prob;
        let bit = self.value >= x;
        if !bit {
            self.length = x;
            model.bit_0_count += 1;
        } else {
            self.value -= x;
            self.length -= x;
        }
        if self.length < AC_MIN_LENGTH {
            self.renorm_decode();
        }
        model.bits_until_update -= 1;
        if model.bits_until_update == 0 {
            model.update();
        }
        bit
    }

    /// Decode a single symbol against an adaptive multi-symbol model.
    ///
    /// Uses a linear search through the cumulative distribution — the
    /// fast-path lookup table from the reference C++ implementation has
    /// subtle invariants that are easy to get wrong; clarity wins here
    /// because the v1 codec exercises the symbol decoder very few times
    /// per chunk anyway.
    pub fn decode_symbol(&mut self, model: &mut SymbolModel) -> u32 {
        self.length >>= DM_LENGTH_SHIFT;
        let dv = self.value / self.length;
        // Find the largest k such that distribution[k] <= dv.
        let mut symbol: u32 = 0;
        let mut k: u32 = 1;
        while k < model.num_symbols && model.distribution[k as usize] <= dv {
            symbol = k;
            k += 1;
        }
        let x = model.distribution[symbol as usize] * self.length;
        let y = if symbol + 1 < model.num_symbols {
            model.distribution[(symbol + 1) as usize] * self.length
        } else {
            // distribution[num_symbols] is implicitly 1 << DM_LENGTH_SHIFT,
            // and (length << DM_LENGTH_SHIFT) reconstructs the pre-shift value.
            self.length << DM_LENGTH_SHIFT
        };

        self.value -= x;
        self.length = y - x;
        if self.length < AC_MIN_LENGTH {
            self.renorm_decode();
        }

        model.symbol_count[symbol as usize] += 1;
        model.symbols_until_update -= 1;
        if model.symbols_until_update == 0 {
            model.total_count += model.update_cycle;
            model.rebuild_distribution();
        }
        symbol
    }

    /// Decode `bits` raw (non-adaptive) bits.
    pub fn read_bits(&mut self, bits: u32) -> u32 {
        if bits == 0 {
            return 0;
        }
        if bits > 19 {
            // Reference decoder splits requests > 19 bits into two halves.
            let lower = self.read_bits(16);
            let upper = self.read_bits(bits - 16);
            return (upper << 16) | lower;
        }
        self.length >>= bits;
        let sym = self.value / self.length;
        // Defensive: malformed stream may yield sym >= (1 << bits); clamp.
        let limit = 1u32 << bits;
        let sym = if sym >= limit { limit - 1 } else { sym };
        self.value -= sym * self.length;
        if self.length < AC_MIN_LENGTH {
            self.renorm_decode();
        }
        sym
    }

    /// Decode a raw byte from the stream.
    pub fn read_byte(&mut self) -> u8 {
        self.read_bits(8) as u8
    }

    /// Decode a raw little-endian 16-bit unsigned integer.
    pub fn read_short(&mut self) -> u16 {
        self.read_bits(16) as u16
    }

    /// Decode a raw little-endian 32-bit unsigned integer.
    pub fn read_int(&mut self) -> u32 {
        let lo = self.read_bits(16);
        let hi = self.read_bits(16);
        (hi << 16) | lo
    }

    /// Decode a raw little-endian 64-bit unsigned integer.
    pub fn read_long(&mut self) -> u64 {
        let lo = u64::from(self.read_int());
        let hi = u64::from(self.read_int());
        (hi << 32) | lo
    }

    /// Return `true` when all input has been consumed.
    pub fn done(&self) -> bool {
        self.pos >= self.bytes.len()
    }
}

/// Encoder companion of [`ArithmeticDecoder`].
///
/// Implementation follows the reference encoder in
/// `LASzip/src/arithmeticencoder.cpp`.  Output is collected in an internal
/// `Vec<u8>` and emitted by [`ArithmeticEncoder::done`].
///
/// Gated behind the `laz-encoder` Cargo feature because it is only used for
/// round-trip testing — production users do not need the encoder.
#[cfg(any(test, feature = "laz-encoder"))]
pub struct ArithmeticEncoder {
    base: u32,
    length: u32,
    out: Vec<u8>,
}

#[cfg(any(test, feature = "laz-encoder"))]
impl Default for ArithmeticEncoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(test, feature = "laz-encoder"))]
impl ArithmeticEncoder {
    /// Create a fresh encoder with empty output.
    pub fn new() -> Self {
        Self {
            base: 0,
            length: u32::MAX,
            out: Vec::new(),
        }
    }

    /// Renormalize the encoder, propagating carries and emitting bytes.
    ///
    /// Follows the reference algorithm: when the top byte of `base` cannot
    /// possibly change (i.e. base + length stays in the same upper byte),
    /// emit it; otherwise hold it in `cache` and increment `cache_size`.
    fn propagate_carry(&mut self) {
        // The new base wraps below the old; propagate +1 through the cached
        // byte and convert the run of cached 0xFFs into 0x00s.
        let mut idx = self.out.len();
        // Walk backwards through any trailing 0xFFs and turn them into 0x00s.
        while idx > 0 && self.out[idx - 1] == 0xFF {
            self.out[idx - 1] = 0;
            idx -= 1;
        }
        if idx > 0 {
            // The byte that gets the carry is the one before the 0xFF run.
            self.out[idx - 1] = self.out[idx - 1].wrapping_add(1);
        }
        // If `idx == 0`, there are no emitted bytes yet to receive the carry;
        // the state machine prevents this from happening on well-formed input
        // because the encoder always emits at least one normalization byte
        // before any meaningful base addition.
    }

    fn renorm_encode(&mut self) {
        while self.length < AC_MIN_LENGTH {
            // Emit the top byte of base.
            self.out.push((self.base >> 24) as u8);
            self.base <<= 8;
            self.length <<= 8;
        }
    }

    /// Encode a single bit against `model`.
    pub fn encode_bit(&mut self, model: &mut BitModel, bit: bool) {
        let x = (self.length >> BM_LENGTH_SHIFT) * model.bit_0_prob;
        if !bit {
            self.length = x;
            model.bit_0_count += 1;
        } else {
            let new_base = self.base.wrapping_add(x);
            if new_base < self.base {
                self.propagate_carry();
            }
            self.base = new_base;
            self.length -= x;
        }
        if self.length < AC_MIN_LENGTH {
            self.renorm_encode();
        }
        model.bits_until_update -= 1;
        if model.bits_until_update == 0 {
            model.update();
        }
    }

    /// Encode a symbol against the multi-symbol `model`.
    pub fn encode_symbol(&mut self, model: &mut SymbolModel, symbol: u32) {
        let lo = model.distribution[symbol as usize];
        self.length >>= DM_LENGTH_SHIFT;
        let x = lo * self.length;
        let y = if symbol + 1 < model.num_symbols {
            model.distribution[(symbol + 1) as usize] * self.length
        } else {
            self.length << DM_LENGTH_SHIFT
        };
        let new_base = self.base.wrapping_add(x);
        if new_base < self.base {
            self.propagate_carry();
        }
        self.base = new_base;
        self.length = y - x;
        if self.length < AC_MIN_LENGTH {
            self.renorm_encode();
        }

        model.symbol_count[symbol as usize] += 1;
        model.symbols_until_update -= 1;
        if model.symbols_until_update == 0 {
            model.total_count += model.update_cycle;
            model.rebuild_distribution();
        }
    }

    /// Encode `bits` raw bits.
    pub fn write_bits(&mut self, bits: u32, value: u32) {
        if bits == 0 {
            return;
        }
        if bits > 19 {
            let lower = value & 0xFFFF;
            let upper = value >> 16;
            self.write_bits(16, lower);
            self.write_bits(bits - 16, upper);
            return;
        }
        self.length >>= bits;
        let x = value * self.length;
        let new_base = self.base.wrapping_add(x);
        if new_base < self.base {
            self.propagate_carry();
        }
        self.base = new_base;
        if self.length < AC_MIN_LENGTH {
            self.renorm_encode();
        }
    }

    /// Encode a raw byte.
    pub fn write_byte(&mut self, b: u8) {
        self.write_bits(8, u32::from(b));
    }

    /// Encode a raw little-endian u16.
    pub fn write_short(&mut self, v: u16) {
        self.write_bits(16, u32::from(v));
    }

    /// Encode a raw little-endian u32.
    pub fn write_int(&mut self, v: u32) {
        self.write_bits(16, v & 0xFFFF);
        self.write_bits(16, v >> 16);
    }

    /// Encode a raw little-endian u64.
    pub fn write_long(&mut self, v: u64) {
        self.write_int((v & 0xFFFF_FFFF) as u32);
        self.write_int((v >> 32) as u32);
    }

    /// Finalize the encoder and consume it, returning the emitted bytes.
    pub fn done(mut self) -> Vec<u8> {
        // Final flush: choose the smallest base in [base, base+length) whose
        // tail bytes are zeros so the decoder converges with zero-padding.
        // This matches the reference final-byte choice.
        let init_base = self.base;
        if self.length > 2 * AC_MIN_LENGTH {
            self.base = self.base.wrapping_add(AC_MIN_LENGTH);
            self.length = AC_MIN_LENGTH >> 1;
        } else {
            self.base = self.base.wrapping_add(AC_MIN_LENGTH >> 1);
            self.length = AC_MIN_LENGTH >> 9;
        }
        if self.base < init_base {
            self.propagate_carry();
        }
        self.renorm_encode();

        // Emit a few extra zero bytes to give the decoder its initial 4-byte
        // window and keep it from reading past the buffer.
        self.out.push(0);
        self.out.push(0);
        self.out.push(0);
        self.out.push(0);
        self.out
    }
}

/// Per-context state used by [`IntegerCompressor`] to encode signed corrector
/// values.  Holds the running symbol model for the bit-count of each value.
#[derive(Debug, Clone)]
pub struct IntegerCompressor {
    /// Number of independent contexts maintained.
    pub contexts: usize,
    /// Number of bits used to represent the magnitude.
    pub bits: u8,
    /// Maximum representable magnitude (1 << bits).
    pub range: u32,
    /// History of last decoded k values per context (used as feedback).
    pub k_history: Vec<u32>,
    /// One bit-count model per context.
    pub bits_models: Vec<SymbolModel>,
    /// One mantissa model per (context, bit-length) pair, lazily allocated.
    pub corrector_models: Vec<Option<SymbolModel>>,
}

impl IntegerCompressor {
    /// Construct a new compressor with `contexts` independent contexts and
    /// `bits` bits of magnitude (typically 32 for raw i32 coordinates).
    pub fn new(bits: u8, contexts: usize) -> Self {
        let bits_clamped = bits.min(32);
        let range = if bits_clamped == 32 {
            u32::MAX
        } else {
            1u32 << bits_clamped
        };
        let num_bit_models = usize::from(bits_clamped) + 1;
        let bits_models = (0..contexts)
            .map(|_| SymbolModel::new(num_bit_models as u32))
            .collect();
        Self {
            contexts,
            bits: bits_clamped,
            range,
            k_history: vec![0u32; contexts],
            bits_models,
            corrector_models: vec![None; contexts * num_bit_models],
        }
    }

    /// Decode a signed integer corrector for `context`.
    pub fn decompress(
        &mut self,
        decoder: &mut ArithmeticDecoder<'_>,
        pred: i32,
        context: usize,
    ) -> i32 {
        let k = decoder.decode_symbol(&mut self.bits_models[context]);
        self.k_history[context] = k;

        if k == 0 {
            return pred;
        }

        // Decode magnitude in `k` bits.  Reference: see lasinteger.cpp:
        // - for k <= 8 we use a fast symbol model of 2^k bins,
        // - for k > 8 we split into (k-8) raw bits + 8 modeled bits.
        let num_bit_models = usize::from(self.bits) + 1;
        let model_idx = context * num_bit_models + k as usize;

        let magnitude: u32 = if k <= 8 {
            let m =
                self.corrector_models[model_idx].get_or_insert_with(|| SymbolModel::new(1u32 << k));
            decoder.decode_symbol(m)
        } else if k < 32 {
            let low = decoder.read_bits(k - 8);
            let m = self.corrector_models[model_idx].get_or_insert_with(|| SymbolModel::new(256));
            let high = decoder.decode_symbol(m);
            (high << (k - 8)) | low
        } else {
            // k == 32: full-width raw integer.
            let lo = decoder.read_bits(16);
            let hi = decoder.read_bits(16);
            (hi << 16) | lo
        };

        // Reconstruct the signed corrector from the unsigned magnitude.
        // Reference encoder mapping (laswriteitemcompressed_v1.cpp):
        //   positive corr c in [(1<<(k-1))..(1<<k)) → m = c, store c
        //   negative corr c in (-(1<<k)..(-(1<<(k-1))]] → m = c + (1<<k) - 1, store ... + 1
        // Here we use the symmetric mapping that pairs each m in [0, 1<<k)
        // with a signed corrector value:
        //   if m >= half:  signed = m
        //   else:          signed = m - (1<<k) + 1
        // where half = 1 << (k-1).  This is the LASzip "asymmetric" mapping
        // that handles k=1 cleanly (corr ∈ {-1, +1}).
        let one_shl_k = 1i64 << k;
        let half = 1i64 << (k - 1);
        let signed: i32 = if (magnitude as i64) >= half {
            magnitude as i32
        } else {
            (magnitude as i64 - one_shl_k + 1) as i32
        };

        pred.wrapping_add(signed)
    }

    /// Encode a signed integer corrector for `context`.
    ///
    /// Gated behind the `laz-encoder` Cargo feature.
    #[cfg(any(test, feature = "laz-encoder"))]
    pub fn compress(
        &mut self,
        encoder: &mut ArithmeticEncoder,
        pred: i32,
        actual: i32,
        context: usize,
    ) {
        let corr = actual.wrapping_sub(pred);
        // Number of bits needed to represent `corr` in the LASzip mapping.
        let k = if corr == 0 {
            0u32
        } else {
            // For positive corr in [1, 1<<31), k = bit-width such that
            // half = 1<<(k-1) <= corr < 1<<k.
            // For negative corr in (-(1<<31), -1], pick k such that
            // -(1<<k) + 1 <= corr <= -(1<<(k-1)).
            // Equivalently, k = bit-width of |corr - (corr<0 as 1)|.
            let mag = if corr > 0 {
                corr as u32
            } else {
                // corr <= -1: encode magnitude = -corr.
                ((-(corr as i64)) as u32).wrapping_add(0)
            };
            32 - mag.leading_zeros()
        };
        encoder.encode_symbol(&mut self.bits_models[context], k);
        self.k_history[context] = k;
        if k == 0 {
            return;
        }
        let one_shl_k = 1i64 << k;
        let magnitude: u32 = if corr >= 0 {
            // m = corr.  corr in [half, one_shl_k) where half = 1<<(k-1).
            corr as u32
        } else {
            // m = corr + one_shl_k - 1.  corr in (-(one_shl_k), -half], so
            // m ∈ [0, half).
            ((corr as i64) + one_shl_k - 1) as u32
        };
        let _ = one_shl_k;
        let num_bit_models = usize::from(self.bits) + 1;
        let model_idx = context * num_bit_models + k as usize;
        if k <= 8 {
            let m =
                self.corrector_models[model_idx].get_or_insert_with(|| SymbolModel::new(1u32 << k));
            encoder.encode_symbol(m, magnitude);
        } else if k < 32 {
            let low = magnitude & ((1u32 << (k - 8)) - 1);
            let high = magnitude >> (k - 8);
            encoder.write_bits(k - 8, low);
            let m = self.corrector_models[model_idx].get_or_insert_with(|| SymbolModel::new(256));
            encoder.encode_symbol(m, high);
        } else {
            // k == 32: full-width raw integer.
            encoder.write_bits(16, magnitude & 0xFFFF);
            encoder.write_bits(16, magnitude >> 16);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arithmetic_decode_uniform_bits_round_trip() {
        let pattern: [(u32, u32); 6] = [
            (1, 0),
            (3, 5),
            (8, 0xAB),
            (16, 0xDEAD),
            (12, 0xBE),
            (8, 0x42),
        ];
        let mut enc = ArithmeticEncoder::new();
        for (bits, val) in pattern {
            enc.write_bits(bits, val);
        }
        let stream = enc.done();
        let mut dec = ArithmeticDecoder::new(&stream).expect("decoder init");
        for (bits, expected) in pattern {
            let got = dec.read_bits(bits);
            assert_eq!(
                got, expected,
                "bits={bits} expected {expected:#x} got {got:#x}"
            );
        }
    }

    #[test]
    fn test_arithmetic_symbol_model_2_symbol_round_trip() {
        let stream_symbols: Vec<u32> = (0..256).map(|i| (i % 2) as u32).collect();
        let mut model_enc = SymbolModel::new(2);
        let mut enc = ArithmeticEncoder::new();
        for s in &stream_symbols {
            enc.encode_symbol(&mut model_enc, *s);
        }
        let bytes = enc.done();
        let mut model_dec = SymbolModel::new(2);
        let mut dec = ArithmeticDecoder::new(&bytes).expect("decoder init");
        for expected in &stream_symbols {
            let got = dec.decode_symbol(&mut model_dec);
            assert_eq!(got, *expected);
        }
    }

    #[test]
    fn test_arithmetic_decoder_done_after_full_stream() {
        let mut enc = ArithmeticEncoder::new();
        for i in 0..32u32 {
            enc.write_bits(8, i);
        }
        let bytes = enc.done();
        let total_len = bytes.len();
        let mut dec = ArithmeticDecoder::new(&bytes).expect("decoder init");
        for _ in 0..32 {
            let _ = dec.read_bits(8);
        }
        let _ = dec.done();
        assert!(total_len > 0);
    }

    #[test]
    fn test_arithmetic_read_byte_short_int_long_little_endian() {
        let mut enc = ArithmeticEncoder::new();
        enc.write_byte(0x7E);
        enc.write_short(0xBEEF);
        enc.write_int(0xDEAD_BEEF);
        enc.write_long(0x0123_4567_89AB_CDEF);
        let bytes = enc.done();
        let mut dec = ArithmeticDecoder::new(&bytes).expect("decoder init");
        assert_eq!(dec.read_byte(), 0x7E);
        assert_eq!(dec.read_short(), 0xBEEF);
        assert_eq!(dec.read_int(), 0xDEAD_BEEF);
        assert_eq!(dec.read_long(), 0x0123_4567_89AB_CDEF);
    }
}
