//! MQ arithmetic decoder for JPEG2000 EBCOT tier-1
//!
//! Implements the MQ-coder as specified in JPEG2000 Part 1 (ISO/IEC 15444-1),
//! Annex C. The MQ-coder is a context-adaptive binary arithmetic coder used
//! for encoding/decoding significance, sign, and refinement information in
//! EBCOT code-blocks.

use crate::error::{Jpeg2000Error, Result};

/// Number of MQ contexts used in EBCOT tier-1
pub const NUM_CONTEXTS: usize = 19;

/// Context indices for EBCOT coding passes
pub mod ctx {
    /// Significance contexts for LL/LH subbands (indices 0-8)
    pub const SIG_LL_LH_START: usize = 0;
    /// Significance contexts for HL subband (indices 0-8)
    pub const SIG_HL_START: usize = 0;
    /// Significance contexts for HH subband (indices 0-8)
    pub const SIG_HH_START: usize = 0;
    /// Sign context base index (index 9-13)
    pub const SIGN_START: usize = 9;
    /// Magnitude refinement context (index 14-15)
    pub const MAG_REF_START: usize = 14;
    /// Run-length context (index 17)
    pub const RUN_LENGTH: usize = 17;
    /// Uniform context (index 18)
    pub const UNIFORM: usize = 18;
    /// Aggregation context (index 17) - same as run-length
    pub const AGGREGATION: usize = 17;
}

/// MQ probability estimation table entry
#[derive(Debug, Clone, Copy)]
struct QeEntry {
    /// Probability value (Qe)
    qe: u16,
    /// Next state on MPS (Most Probable Symbol)
    next_mps: u8,
    /// Next state on LPS (Least Probable Symbol)
    next_lps: u8,
    /// Whether to switch MPS/LPS
    switch_flag: bool,
}

/// Standard MQ probability estimation table (47 entries)
/// From JPEG2000 Part 1, Table C.2
const QE_TABLE: [QeEntry; 47] = [
    QeEntry {
        qe: 0x5601,
        next_mps: 1,
        next_lps: 1,
        switch_flag: true,
    }, // 0
    QeEntry {
        qe: 0x3401,
        next_mps: 2,
        next_lps: 6,
        switch_flag: false,
    }, // 1
    QeEntry {
        qe: 0x1801,
        next_mps: 3,
        next_lps: 9,
        switch_flag: false,
    }, // 2
    QeEntry {
        qe: 0x0AC1,
        next_mps: 4,
        next_lps: 12,
        switch_flag: false,
    }, // 3
    QeEntry {
        qe: 0x0521,
        next_mps: 5,
        next_lps: 29,
        switch_flag: false,
    }, // 4
    QeEntry {
        qe: 0x0221,
        next_mps: 38,
        next_lps: 33,
        switch_flag: false,
    }, // 5
    QeEntry {
        qe: 0x5601,
        next_mps: 7,
        next_lps: 6,
        switch_flag: true,
    }, // 6
    QeEntry {
        qe: 0x5401,
        next_mps: 8,
        next_lps: 14,
        switch_flag: false,
    }, // 7
    QeEntry {
        qe: 0x4801,
        next_mps: 9,
        next_lps: 14,
        switch_flag: false,
    }, // 8
    QeEntry {
        qe: 0x3801,
        next_mps: 10,
        next_lps: 14,
        switch_flag: false,
    }, // 9
    QeEntry {
        qe: 0x3001,
        next_mps: 11,
        next_lps: 17,
        switch_flag: false,
    }, // 10
    QeEntry {
        qe: 0x2401,
        next_mps: 12,
        next_lps: 18,
        switch_flag: false,
    }, // 11
    QeEntry {
        qe: 0x1C01,
        next_mps: 13,
        next_lps: 20,
        switch_flag: false,
    }, // 12
    QeEntry {
        qe: 0x1601,
        next_mps: 29,
        next_lps: 21,
        switch_flag: false,
    }, // 13
    QeEntry {
        qe: 0x5601,
        next_mps: 15,
        next_lps: 14,
        switch_flag: true,
    }, // 14
    QeEntry {
        qe: 0x5401,
        next_mps: 16,
        next_lps: 14,
        switch_flag: false,
    }, // 15
    QeEntry {
        qe: 0x5101,
        next_mps: 17,
        next_lps: 15,
        switch_flag: false,
    }, // 16
    QeEntry {
        qe: 0x4801,
        next_mps: 18,
        next_lps: 16,
        switch_flag: false,
    }, // 17
    QeEntry {
        qe: 0x3801,
        next_mps: 19,
        next_lps: 17,
        switch_flag: false,
    }, // 18
    QeEntry {
        qe: 0x3401,
        next_mps: 20,
        next_lps: 18,
        switch_flag: false,
    }, // 19
    QeEntry {
        qe: 0x3001,
        next_mps: 21,
        next_lps: 19,
        switch_flag: false,
    }, // 20
    QeEntry {
        qe: 0x2801,
        next_mps: 22,
        next_lps: 19,
        switch_flag: false,
    }, // 21
    QeEntry {
        qe: 0x2401,
        next_mps: 23,
        next_lps: 20,
        switch_flag: false,
    }, // 22
    QeEntry {
        qe: 0x2201,
        next_mps: 24,
        next_lps: 21,
        switch_flag: false,
    }, // 23
    QeEntry {
        qe: 0x1C01,
        next_mps: 25,
        next_lps: 22,
        switch_flag: false,
    }, // 24
    QeEntry {
        qe: 0x1801,
        next_mps: 26,
        next_lps: 23,
        switch_flag: false,
    }, // 25
    QeEntry {
        qe: 0x1601,
        next_mps: 27,
        next_lps: 24,
        switch_flag: false,
    }, // 26
    QeEntry {
        qe: 0x1401,
        next_mps: 28,
        next_lps: 25,
        switch_flag: false,
    }, // 27
    QeEntry {
        qe: 0x1201,
        next_mps: 29,
        next_lps: 26,
        switch_flag: false,
    }, // 28
    QeEntry {
        qe: 0x1101,
        next_mps: 30,
        next_lps: 27,
        switch_flag: false,
    }, // 29
    QeEntry {
        qe: 0x0AC1,
        next_mps: 31,
        next_lps: 28,
        switch_flag: false,
    }, // 30
    QeEntry {
        qe: 0x09C1,
        next_mps: 32,
        next_lps: 29,
        switch_flag: false,
    }, // 31
    QeEntry {
        qe: 0x08A1,
        next_mps: 33,
        next_lps: 30,
        switch_flag: false,
    }, // 32
    QeEntry {
        qe: 0x0521,
        next_mps: 34,
        next_lps: 31,
        switch_flag: false,
    }, // 33
    QeEntry {
        qe: 0x0441,
        next_mps: 35,
        next_lps: 32,
        switch_flag: false,
    }, // 34
    QeEntry {
        qe: 0x02A1,
        next_mps: 36,
        next_lps: 33,
        switch_flag: false,
    }, // 35
    QeEntry {
        qe: 0x0221,
        next_mps: 37,
        next_lps: 34,
        switch_flag: false,
    }, // 36
    QeEntry {
        qe: 0x0141,
        next_mps: 38,
        next_lps: 35,
        switch_flag: false,
    }, // 37
    QeEntry {
        qe: 0x0111,
        next_mps: 39,
        next_lps: 36,
        switch_flag: false,
    }, // 38
    QeEntry {
        qe: 0x0085,
        next_mps: 40,
        next_lps: 37,
        switch_flag: false,
    }, // 39
    QeEntry {
        qe: 0x0049,
        next_mps: 41,
        next_lps: 38,
        switch_flag: false,
    }, // 40
    QeEntry {
        qe: 0x0025,
        next_mps: 42,
        next_lps: 39,
        switch_flag: false,
    }, // 41
    QeEntry {
        qe: 0x0015,
        next_mps: 43,
        next_lps: 40,
        switch_flag: false,
    }, // 42
    QeEntry {
        qe: 0x0009,
        next_mps: 44,
        next_lps: 41,
        switch_flag: false,
    }, // 43
    QeEntry {
        qe: 0x0005,
        next_mps: 45,
        next_lps: 42,
        switch_flag: false,
    }, // 44
    QeEntry {
        qe: 0x0001,
        next_mps: 45,
        next_lps: 43,
        switch_flag: false,
    }, // 45
    QeEntry {
        qe: 0x5601,
        next_mps: 46,
        next_lps: 46,
        switch_flag: false,
    }, // 46
];

/// MQ context state
#[derive(Debug, Clone, Copy, Default)]
pub struct MqContextState {
    /// Index into the QE probability table
    qe_index: u8,
    /// Most probable symbol value (0 or 1)
    mps: u8,
}

/// MQ arithmetic decoder
///
/// Implements the full JPEG2000 MQ-coder with proper probability estimation
/// and byte-stuffing handling as defined in Annex C of ISO/IEC 15444-1.
#[derive(Debug, Clone)]
pub struct MqDecoder {
    /// Compressed data buffer
    buffer: Vec<u8>,
    /// Current byte position in buffer
    position: usize,
    /// Code register (C) - 32-bit
    c_register: u32,
    /// Interval register (A) - 16-bit effective
    a_register: u32,
    /// Bit count (CT)
    ct: u32,
    /// Temporary byte for byte-stuffing detection
    t_bar: u8,
    /// Decoder contexts
    contexts: [MqContextState; NUM_CONTEXTS],
    /// Whether the decoder has been exhausted
    exhausted: bool,
}

impl MqDecoder {
    /// Create a new MQ decoder from compressed data
    pub fn new(data: Vec<u8>) -> Self {
        let mut decoder = Self {
            buffer: data,
            position: 0,
            c_register: 0,
            a_register: 0x8000,
            ct: 0,
            t_bar: 0,
            contexts: [MqContextState::default(); NUM_CONTEXTS],
            exhausted: false,
        };
        decoder.init_contexts();
        decoder.init_dec();
        decoder
    }

    /// Initialize context states per JPEG2000 spec
    fn init_contexts(&mut self) {
        // All contexts start at state 0, MPS = 0
        for ctx in &mut self.contexts {
            ctx.qe_index = 0;
            ctx.mps = 0;
        }
        // The uniform context (18) is initialized to state 46 (equiprobable)
        self.contexts[ctx::UNIFORM].qe_index = 46;
        self.contexts[ctx::UNIFORM].mps = 0;
        // The run-length/aggregation context (17) is initialized to state 3
        self.contexts[ctx::RUN_LENGTH].qe_index = 3;
        self.contexts[ctx::RUN_LENGTH].mps = 0;
    }

    /// Initialize the decoder registers (INITDEC procedure)
    fn init_dec(&mut self) {
        // Read first byte
        self.t_bar = if !self.buffer.is_empty() {
            let b = self.buffer[0];
            self.position = 1;
            b
        } else {
            0xFF
        };

        // BYTEIN to fill the C register
        self.c_register = (u32::from(self.t_bar) ^ 0xFF) << 16;

        self.byte_in();

        self.c_register <<= 7;
        self.ct = self.ct.saturating_sub(7);
        self.a_register = 0x8000;
    }

    /// BYTEIN procedure - read a byte handling marker stuffing
    fn byte_in(&mut self) {
        if self.t_bar == 0xFF {
            // After 0xFF, check for stuffing
            if self.position < self.buffer.len() {
                let next = self.buffer[self.position];
                self.position += 1;
                if next > 0x8F {
                    // This is a marker - don't consume it, pad with 0xFF
                    self.position -= 1; // put it back
                    self.c_register += 0xFF00;
                    self.ct = 8;
                    self.exhausted = true;
                } else {
                    self.t_bar = next;
                    self.c_register += u32::from(self.t_bar) << 9;
                    self.ct = 7;
                }
            } else {
                // End of data
                self.c_register += 0xFF00;
                self.ct = 8;
                self.exhausted = true;
            }
        } else {
            if self.position < self.buffer.len() {
                self.t_bar = self.buffer[self.position];
                self.position += 1;
                self.c_register += u32::from(self.t_bar) << 8;
                self.ct = 8;
            } else {
                // End of data - fill with 0xFF
                self.t_bar = 0xFF;
                self.c_register += 0xFF00;
                self.ct = 8;
                self.exhausted = true;
            }
        }
    }

    /// RENORMD procedure - renormalize after decoding
    fn renorm_d(&mut self) {
        loop {
            if self.ct == 0 {
                self.byte_in();
            }
            self.a_register <<= 1;
            self.c_register <<= 1;
            self.ct = self.ct.saturating_sub(1);

            if self.a_register >= 0x8000 {
                break;
            }
        }
    }

    /// Decode a single bit using the specified context
    ///
    /// Implements the DECODE procedure from JPEG2000 Part 1, Annex C.
    pub fn decode(&mut self, context_id: usize) -> Result<u8> {
        if context_id >= NUM_CONTEXTS {
            return Err(Jpeg2000Error::Tier1Error(format!(
                "Invalid MQ context ID: {} (max {})",
                context_id,
                NUM_CONTEXTS - 1
            )));
        }

        let ctx = self.contexts[context_id];
        let qe_entry = QE_TABLE[ctx.qe_index as usize];
        let qe = u32::from(qe_entry.qe);

        // Subtract Qe from A
        self.a_register = self.a_register.wrapping_sub(qe);

        // Check if C < A (MPS path) or C >= A (conditional exchange)
        let symbol;
        if (self.c_register >> 16) < self.a_register {
            // MPS sub-interval
            if self.a_register < 0x8000 {
                // Conditional exchange possible
                symbol = self.mps_exchange(context_id, &qe_entry);
                self.renorm_d();
            } else {
                symbol = ctx.mps;
            }
        } else {
            // LPS sub-interval: subtract A from C
            self.c_register = self.c_register.wrapping_sub(self.a_register << 16);
            symbol = self.lps_exchange(context_id, &qe_entry);
            self.renorm_d();
        }

        Ok(symbol)
    }

    /// MPS exchange procedure
    fn mps_exchange(&mut self, context_id: usize, entry: &QeEntry) -> u8 {
        let qe = u32::from(entry.qe);
        let mps = self.contexts[context_id].mps;

        if self.a_register < qe {
            // Exchange: output LPS
            let d = 1 - mps;
            if entry.switch_flag {
                self.contexts[context_id].mps = 1 - mps;
            }
            self.contexts[context_id].qe_index = entry.next_lps;
            self.a_register = qe;
            d
        } else {
            // No exchange: output MPS
            self.contexts[context_id].qe_index = entry.next_mps;
            mps
        }
    }

    /// LPS exchange procedure
    fn lps_exchange(&mut self, context_id: usize, entry: &QeEntry) -> u8 {
        let qe = u32::from(entry.qe);
        let mps = self.contexts[context_id].mps;

        if self.a_register < qe {
            // Exchange: output MPS
            self.a_register = qe;
            self.contexts[context_id].qe_index = entry.next_mps;
            mps
        } else {
            // No exchange: output LPS
            let d = 1 - mps;
            self.a_register = qe;
            if entry.switch_flag {
                self.contexts[context_id].mps = 1 - mps;
            }
            self.contexts[context_id].qe_index = entry.next_lps;
            d
        }
    }

    /// Decode a raw (bypass) bit without arithmetic coding
    ///
    /// Used in bypass mode for some coding passes. Reads a single bit
    /// directly from the bitstream without probability adaptation.
    pub fn decode_raw(&mut self) -> Result<u8> {
        if self.ct == 0 {
            self.byte_in();
        }

        self.ct = self.ct.saturating_sub(1);
        let bit = ((self.c_register >> (16 + self.ct)) & 1) as u8;

        Ok(bit)
    }

    /// Check if the decoder has exhausted its input data
    pub fn is_exhausted(&self) -> bool {
        self.exhausted
    }

    /// Get the number of bytes consumed so far
    pub fn bytes_consumed(&self) -> usize {
        self.position
    }

    /// Reset a specific context to its initial state
    pub fn reset_context(&mut self, context_id: usize) {
        if context_id < NUM_CONTEXTS {
            self.contexts[context_id] = MqContextState::default();
            // Restore special contexts
            if context_id == ctx::UNIFORM {
                self.contexts[context_id].qe_index = 46;
            } else if context_id == ctx::RUN_LENGTH {
                self.contexts[context_id].qe_index = 3;
            }
        }
    }

    /// Reset all contexts to initial state
    pub fn reset_all_contexts(&mut self) {
        self.init_contexts();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mq_decoder_creation() {
        // Use data without 0xFF to avoid marker detection during init
        let data = vec![0x80, 0x40, 0x55, 0x00, 0x12, 0x34, 0x56, 0x78];
        let decoder = MqDecoder::new(data);
        assert!(!decoder.is_exhausted());
        assert_eq!(decoder.a_register, 0x8000);
    }

    #[test]
    fn test_mq_decoder_empty_data() {
        let decoder = MqDecoder::new(vec![]);
        assert!(decoder.is_exhausted());
    }

    #[test]
    fn test_mq_decode_symbol() {
        let data = vec![0x00, 0x00, 0x00, 0x01, 0x12, 0x34, 0x56, 0x78];
        let mut decoder = MqDecoder::new(data);

        // Decode a symbol from context 0
        let result = decoder.decode(0);
        assert!(result.is_ok());
        let bit = result.expect("decode failed");
        assert!(bit <= 1);
    }

    #[test]
    fn test_mq_decode_multiple_symbols() {
        let data = vec![
            0x80, 0x00, 0x00, 0x00, 0xFF, 0x7F, 0xAA, 0x55, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC,
            0xDE, 0xF0,
        ];
        let mut decoder = MqDecoder::new(data);

        // Decode multiple symbols from different contexts
        for ctx_id in 0..NUM_CONTEXTS {
            let result = decoder.decode(ctx_id);
            assert!(result.is_ok(), "Failed to decode from context {}", ctx_id);
        }
    }

    #[test]
    fn test_mq_invalid_context() {
        let data = vec![0x00, 0x00, 0x00, 0x01];
        let mut decoder = MqDecoder::new(data);

        let result = decoder.decode(NUM_CONTEXTS);
        assert!(result.is_err());
    }

    #[test]
    fn test_mq_decode_raw() {
        let data = vec![0xAA, 0x55, 0xFF, 0x00, 0x12, 0x34, 0x56, 0x78];
        let mut decoder = MqDecoder::new(data);

        for _ in 0..8 {
            let result = decoder.decode_raw();
            assert!(result.is_ok());
            let bit = result.expect("raw decode failed");
            assert!(bit <= 1);
        }
    }

    #[test]
    fn test_mq_context_reset() {
        let data = vec![0x80, 0x00, 0x00, 0x01, 0x12, 0x34, 0x56, 0x78];
        let mut decoder = MqDecoder::new(data);

        // Decode some symbols to change context state
        let _ = decoder.decode(0);
        let _ = decoder.decode(0);

        // Reset and verify
        decoder.reset_context(0);
        assert_eq!(decoder.contexts[0].qe_index, 0);
        assert_eq!(decoder.contexts[0].mps, 0);
    }

    #[test]
    fn test_mq_uniform_context_init() {
        let data = vec![0x00, 0x01, 0x02, 0x03];
        let decoder = MqDecoder::new(data);

        // Uniform context should be at state 46 (equiprobable)
        assert_eq!(decoder.contexts[ctx::UNIFORM].qe_index, 46);
    }

    #[test]
    fn test_mq_run_length_context_init() {
        let data = vec![0x00, 0x01, 0x02, 0x03];
        let decoder = MqDecoder::new(data);

        // Run-length context should be at state 3
        assert_eq!(decoder.contexts[ctx::RUN_LENGTH].qe_index, 3);
    }

    #[test]
    fn test_mq_byte_stuffing_marker() {
        // Data with 0xFF followed by byte > 0x8F (marker)
        let data = vec![0xFF, 0x90, 0x00, 0x01];
        let mut decoder = MqDecoder::new(data);

        // Should handle the marker gracefully
        let result = decoder.decode(0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_mq_byte_stuffing_non_marker() {
        // Data with 0xFF followed by byte <= 0x8F (stuffing)
        let data = vec![0xFF, 0x00, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC];
        let mut decoder = MqDecoder::new(data);

        let result = decoder.decode(0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_qe_table_valid() {
        // Verify QE table entries are valid
        for (i, entry) in QE_TABLE.iter().enumerate() {
            assert!(
                (entry.next_mps as usize) < QE_TABLE.len(),
                "Invalid next_mps at index {}: {}",
                i,
                entry.next_mps
            );
            assert!(
                (entry.next_lps as usize) < QE_TABLE.len(),
                "Invalid next_lps at index {}: {}",
                i,
                entry.next_lps
            );
        }
    }

    #[test]
    fn test_mq_reset_all_contexts() {
        let data = vec![0x80, 0x00, 0x00, 0x01, 0x12, 0x34, 0x56, 0x78];
        let mut decoder = MqDecoder::new(data);

        // Modify some contexts
        let _ = decoder.decode(0);
        let _ = decoder.decode(5);
        let _ = decoder.decode(10);

        // Reset all
        decoder.reset_all_contexts();

        // Verify uniform and run-length are properly initialized
        assert_eq!(decoder.contexts[ctx::UNIFORM].qe_index, 46);
        assert_eq!(decoder.contexts[ctx::RUN_LENGTH].qe_index, 3);

        // Regular contexts should be at state 0
        for i in 0..9 {
            assert_eq!(decoder.contexts[i].qe_index, 0);
        }
    }
}
