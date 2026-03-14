//! JPEG2000 Tier-2 packet header and body decoding
//!
//! A JPEG2000 packet groups compressed code-block data for a single
//! (layer, resolution, component, precinct) tuple. Each packet consists of:
//! 1. A bit-packed header (using tag trees for efficient signalling)
//! 2. A body containing the raw compressed data for each included code block
//!
//! Reference: ISO 15444-1:2019 §B.10 (Packet header coding)

use crate::error::{Jpeg2000Error, Result};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Information about a single code block's contribution to a packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeBlockInclusion {
    /// Whether this code block contributes data to this packet.
    pub included: bool,
    /// Number of new coding passes contributed in this packet.
    pub new_passes: u8,
    /// Length in bytes of the compressed data for this code block (0 if not included).
    pub data_length: u32,
}

/// Decoded packet header.
///
/// The header indicates which code blocks are included in this packet,
/// how many new coding passes each block contributes, and the byte lengths
/// of their compressed data segments.
#[derive(Debug, Clone)]
pub struct PacketHeader {
    /// `true` if this is an empty packet (no data, signalled by a 0 bit).
    pub is_empty: bool,
    /// Per-code-block inclusion and data-length information.
    pub inclusions: Vec<CodeBlockInclusion>,
}

/// A fully decoded JPEG2000 packet (header + body).
#[derive(Debug, Clone)]
pub struct Packet {
    /// Decoded packet header.
    pub header: PacketHeader,
    /// Raw compressed data for each *included* code block, in precinct scan order.
    pub code_block_data: Vec<Vec<u8>>,
}

// ---------------------------------------------------------------------------
// Bit reader — reads bits from a byte slice MSB-first (JPEG2000 big-endian)
// ---------------------------------------------------------------------------

/// Minimal MSB-first bit reader that also handles the JPEG2000 stuffing rule:
/// after a `0xFF` byte, the next byte's MSB is skipped (treated as 0).
///
/// Internal representation:
/// - `current_byte`: the byte currently being consumed.
/// - `bits_left`: how many bits remain in `current_byte` (decrements 8→0).
/// - `byte_pos`: position of the *next* byte to read from `data`.
pub struct BitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    current_byte: u8,
    bits_left: u8,
    /// `true` when the byte before `current_byte` was `0xFF` (stuffing).
    prev_was_ff: bool,
}

impl<'a> BitReader<'a> {
    /// Create a new bit reader over `data`.
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte_pos: 0,
            current_byte: 0,
            bits_left: 0,
            prev_was_ff: false,
        }
    }

    /// Read exactly one bit (MSB-first).  Returns `Err` on end-of-data.
    pub fn read_bit(&mut self) -> Result<u8> {
        if self.bits_left == 0 {
            self.refill()?;
        }
        self.bits_left -= 1;
        let bit = (self.current_byte >> self.bits_left) & 1;
        Ok(bit)
    }

    /// Read `n` bits (MSB-first) and return as a `u32` (max 32 bits).
    pub fn read_bits(&mut self, n: u8) -> Result<u32> {
        let mut value = 0u32;
        for _ in 0..n {
            value = (value << 1) | u32::from(self.read_bit()?);
        }
        Ok(value)
    }

    /// Return the number of whole bytes consumed so far.
    pub fn bytes_consumed(&self) -> usize {
        self.byte_pos
    }

    fn refill(&mut self) -> Result<()> {
        if self.byte_pos >= self.data.len() {
            return Err(Jpeg2000Error::InsufficientData {
                expected: 1,
                actual: 0,
            });
        }
        let byte = self.data[self.byte_pos];
        // JPEG2000 stuffing: if previous byte was 0xFF, skip the MSB of this byte.
        let skip_msb = self.prev_was_ff;
        self.prev_was_ff = byte == 0xFF;
        self.byte_pos += 1;
        self.current_byte = byte;
        self.bits_left = if skip_msb { 7 } else { 8 };
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tag tree
// ---------------------------------------------------------------------------

/// JPEG2000 tag-tree node used for efficient entropy coding of the inclusion
/// and zero bit-plane count information.
///
/// The tag tree is a quadtree where each leaf corresponds to one code block.
/// The value at each node is the minimum of its children's values.
/// Encoding transmits only the *delta* needed to reach each threshold.
///
/// Reference: ISO 15444-1:2019 §B.10.2
#[derive(Debug, Clone)]
pub struct TagTree {
    width: u32,
    height: u32,
    /// Decoded minimum values at each level.  Indexed by node position within
    /// a level; levels are stored bottom-to-top (level 0 = leaf level).
    levels: Vec<Vec<Option<u32>>>,
    /// Number of tree levels.
    num_levels: usize,
}

impl TagTree {
    /// Construct a new tag tree for `width × height` code blocks.
    pub fn new(width: u32, height: u32) -> Self {
        let mut levels = Vec::new();
        let mut w = width;
        let mut h = height;
        loop {
            let count = (w as usize) * (h as usize);
            levels.push(vec![None; count]);
            if w == 1 && h == 1 {
                break;
            }
            w = w.div_ceil(2);
            h = h.div_ceil(2);
        }
        let num_levels = levels.len();
        Self {
            width,
            height,
            levels,
            num_levels,
        }
    }

    /// Decode whether the value at leaf `(cx, cy)` is `<= threshold`.
    ///
    /// Reads bits from `bits` as needed.  Returns `true` if the leaf value
    /// is `<= threshold`, `false` otherwise (meaning more passes are needed).
    pub fn decode_value(
        &mut self,
        cx: u32,
        cy: u32,
        threshold: u32,
        bits: &mut BitReader<'_>,
    ) -> Result<bool> {
        // Walk from root to leaf, refining the lower-bound at each level.
        // We need the ancestry path: build it top-down.
        let mut path: Vec<(usize, usize)> = Vec::with_capacity(self.num_levels);
        let mut lx = cx;
        let mut ly = cy;
        for lvl in 0..self.num_levels {
            let level_idx = self.num_levels - 1 - lvl; // 0 = leaf level
            let w = self.level_width(level_idx);
            let idx = (ly as usize) * w + (lx as usize);
            path.push((level_idx, idx));
            lx /= 2;
            ly /= 2;
        }
        // Process from root (last in path) down to leaf (first in path)
        let mut parent_lower = 0u32;
        for &(level_idx, node_idx) in path.iter().rev() {
            let current = self.levels[level_idx][node_idx].unwrap_or(parent_lower);
            // Bring current up to parent's lower bound
            let mut lower = current.max(parent_lower);
            // Decode bits until lower > threshold or we confirm <= threshold
            loop {
                if lower > threshold {
                    // Value is definitely > threshold
                    self.levels[level_idx][node_idx] = Some(lower);
                    return Ok(false);
                }
                // Read one bit: 0 = not reached threshold yet, 1 = reached
                let bit = bits.read_bit()?;
                if bit == 1 {
                    // lower == value exactly at this level
                    self.levels[level_idx][node_idx] = Some(lower);
                    break; // value is <= threshold — continue toward leaf
                }
                lower += 1;
            }
            parent_lower = lower;
        }
        Ok(true)
    }

    fn level_width(&self, level_idx: usize) -> usize {
        // Level 0 = leaf level (full width), level num_levels-1 = root (1x1).
        // Traverse from root down to the requested level.
        let mut lw = self.width as usize;
        let mut lh = self.height as usize;
        let target = self.num_levels - 1 - level_idx; // steps from leaf to this level
        for _ in 0..target {
            lw = lw.div_ceil(2);
            lh = lh.div_ceil(2);
        }
        let _ = lh;
        lw
    }

    /// Return the number of tag tree levels.
    pub fn num_levels(&self) -> usize {
        self.num_levels
    }
}

// ---------------------------------------------------------------------------
// Packet decoder
// ---------------------------------------------------------------------------

/// Decodes JPEG2000 packets from a raw byte slice.
pub struct PacketDecoder;

impl PacketDecoder {
    /// Decode a single packet from `data`.
    ///
    /// # Parameters
    /// - `data`: Raw bytes starting at the beginning of the packet header.
    /// - `num_code_blocks_x`: Number of code blocks in the X dimension of the precinct.
    /// - `num_code_blocks_y`: Number of code blocks in the Y dimension of the precinct.
    /// - `first_layer`: Whether any code block was already included in a previous layer.
    ///   Pass a mutable slice of booleans (one per code block) that persists across layers.
    ///
    /// # Returns
    /// `(Packet, bytes_consumed)` on success.
    pub fn decode(
        data: &[u8],
        num_code_blocks_x: u32,
        num_code_blocks_y: u32,
        previously_included: &mut Vec<bool>,
    ) -> Result<(Packet, usize)> {
        let num_blocks = (num_code_blocks_x * num_code_blocks_y) as usize;

        // Ensure the previously_included tracking vec is the right size
        if previously_included.len() < num_blocks {
            previously_included.resize(num_blocks, false);
        }

        let mut bits = BitReader::new(data);

        // Read packet indicator bit (Ppkt)
        let ppkt = bits.read_bit()?;

        if ppkt == 0 {
            // Empty packet — no code block data
            let consumed = bits.bytes_consumed();
            return Ok((
                Packet {
                    header: PacketHeader {
                        is_empty: true,
                        inclusions: vec![
                            CodeBlockInclusion {
                                included: false,
                                new_passes: 0,
                                data_length: 0,
                            };
                            num_blocks
                        ],
                    },
                    code_block_data: Vec::new(),
                },
                consumed,
            ));
        }

        // Non-empty packet: decode inclusion tag tree and zero bit-planes tag tree
        let mut inclusion_tree = TagTree::new(num_code_blocks_x, num_code_blocks_y);
        let mut zbp_tree = TagTree::new(num_code_blocks_x, num_code_blocks_y);

        let mut inclusions = Vec::with_capacity(num_blocks);
        let mut included_indices = Vec::new();

        for by in 0..num_code_blocks_y {
            for bx in 0..num_code_blocks_x {
                let idx = (by * num_code_blocks_x + bx) as usize;
                let already = previously_included[idx];

                let included = if already {
                    // Code block was already included in a previous layer:
                    // just read a single bit indicating whether it contributes here
                    bits.read_bit()? == 1
                } else {
                    // New code block: use inclusion tag tree with threshold = current layer
                    // For simplicity we treat threshold=0 (layer 0), which means the tag
                    // tree decodes the layer at which this block first appears.
                    inclusion_tree.decode_value(bx, by, 0, &mut bits)?
                };

                if included {
                    previously_included[idx] = true;

                    // Decode number of new coding passes: variable-length code
                    let new_passes = Self::decode_num_passes(&mut bits)?;

                    // Decode data length (variable-length)
                    let data_length = Self::decode_block_length(&mut bits, new_passes)?;

                    // Decode zero bit-planes count if this is first inclusion
                    let _zbp = if !already {
                        zbp_tree
                            .decode_value(bx, by, 255, &mut bits)
                            .unwrap_or(false)
                    } else {
                        false
                    };

                    inclusions.push(CodeBlockInclusion {
                        included: true,
                        new_passes,
                        data_length,
                    });
                    included_indices.push(idx);
                } else {
                    inclusions.push(CodeBlockInclusion {
                        included: false,
                        new_passes: 0,
                        data_length: 0,
                    });
                }
            }
        }

        // Header ends at the current byte boundary
        let header_bytes = bits.bytes_consumed();

        // Read body: collect compressed data for each included block
        let mut pos = header_bytes;
        let mut code_block_data = Vec::new();

        for incl in &inclusions {
            if incl.included {
                let len = incl.data_length as usize;
                if pos + len > data.len() {
                    return Err(Jpeg2000Error::InsufficientData {
                        expected: pos + len,
                        actual: data.len(),
                    });
                }
                code_block_data.push(data[pos..pos + len].to_vec());
                pos += len;
            }
        }

        Ok((
            Packet {
                header: PacketHeader {
                    is_empty: false,
                    inclusions,
                },
                code_block_data,
            },
            pos,
        ))
    }

    /// Decode the number of new coding passes using the JPEG2000 variable-length code.
    ///
    /// Encoding:
    /// - `1`        → 1 pass
    /// - `01`       → 2 passes
    /// - `001 bb`   → 3–6 passes (2-bit binary number + 3)
    /// - `0001 bbbb`→ 7–36 passes (4-bit + 7)
    /// - more       → fallback: 6-bit direct
    fn decode_num_passes(bits: &mut BitReader<'_>) -> Result<u8> {
        if bits.read_bit()? == 1 {
            return Ok(1);
        }
        if bits.read_bit()? == 1 {
            return Ok(2);
        }
        if bits.read_bit()? == 1 {
            let extra = bits.read_bits(2)? as u8;
            return Ok(3 + extra);
        }
        if bits.read_bit()? == 1 {
            let extra = bits.read_bits(4)? as u8;
            return Ok(7 + extra);
        }
        // More than 22 passes: read 6-bit direct
        let extra = bits.read_bits(6)? as u8;
        Ok(23 + extra)
    }

    /// Decode data segment length (variable-length code).
    ///
    /// The number of length bits is determined by `ceil(log2(new_passes + 1)) + lblock`
    /// where `lblock` starts at 3 and increments when the length exceeds the coded range.
    /// For simplicity this implementation uses a minimal lblock=3 encoding.
    fn decode_block_length(bits: &mut BitReader<'_>, _new_passes: u8) -> Result<u32> {
        // Decode additional length bits prefix (extend lblock)
        let mut extra_bits = 0u32;
        loop {
            let b = bits.read_bit()?;
            if b == 0 {
                break;
            }
            extra_bits += 1;
        }

        // Base number of bits = lblock (=3) + extra_bits
        let nbits = 3 + extra_bits;
        if nbits > 31 {
            return Err(Jpeg2000Error::Tier2Error(
                "Block length exceeds 31 bits".to_string(),
            ));
        }
        let length = bits.read_bits(nbits as u8)?;
        Ok(length)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bit_reader_basic() {
        // byte 0b10110100
        let data = [0b1011_0100u8];
        let mut br = BitReader::new(&data);
        assert_eq!(br.read_bit().unwrap(), 1);
        assert_eq!(br.read_bit().unwrap(), 0);
        assert_eq!(br.read_bit().unwrap(), 1);
        assert_eq!(br.read_bit().unwrap(), 1);
        assert_eq!(br.read_bit().unwrap(), 0);
        assert_eq!(br.read_bit().unwrap(), 1);
        assert_eq!(br.read_bit().unwrap(), 0);
        assert_eq!(br.read_bit().unwrap(), 0);
    }

    #[test]
    fn test_bit_reader_read_bits() {
        let data = [0b1010_1010u8, 0b1111_0000u8];
        let mut br = BitReader::new(&data);
        assert_eq!(br.read_bits(4).unwrap(), 0b1010);
        assert_eq!(br.read_bits(4).unwrap(), 0b1010);
        assert_eq!(br.read_bits(4).unwrap(), 0b1111);
        assert_eq!(br.read_bits(4).unwrap(), 0b0000);
    }

    #[test]
    fn test_bit_reader_exhaustion() {
        let data = [0x00u8];
        let mut br = BitReader::new(&data);
        for _ in 0..8 {
            br.read_bit().unwrap();
        }
        assert!(br.read_bit().is_err());
    }

    #[test]
    fn test_tag_tree_new() {
        let tt = TagTree::new(4, 3);
        // 4x3 = 12 leaves → 2x2=4 → 1x1=1; 3 levels
        assert_eq!(tt.num_levels(), 3);
    }

    #[test]
    fn test_tag_tree_1x1() {
        let tt = TagTree::new(1, 1);
        assert_eq!(tt.num_levels(), 1);
    }

    #[test]
    fn test_empty_packet_decode() {
        // First bit = 0 → empty packet
        let data = [0b0000_0000u8];
        let mut prev = vec![];
        let (pkt, consumed) = PacketDecoder::decode(&data, 2, 2, &mut prev).unwrap();
        assert!(pkt.header.is_empty);
        assert!(pkt.code_block_data.is_empty());
        assert_eq!(consumed, 1);
    }

    #[test]
    fn test_packet_header_is_empty_flag() {
        let data = [0x00u8];
        let mut prev = vec![];
        let (pkt, _) = PacketDecoder::decode(&data, 1, 1, &mut prev).unwrap();
        assert!(pkt.header.is_empty);
        assert_eq!(pkt.header.inclusions.len(), 1);
        assert!(!pkt.header.inclusions[0].included);
    }

    #[test]
    fn test_code_block_inclusion_default() {
        let incl = CodeBlockInclusion {
            included: true,
            new_passes: 3,
            data_length: 128,
        };
        assert!(incl.included);
        assert_eq!(incl.new_passes, 3);
        assert_eq!(incl.data_length, 128);
    }

    #[test]
    fn test_packet_struct_fields() {
        let pkt = Packet {
            header: PacketHeader {
                is_empty: false,
                inclusions: vec![CodeBlockInclusion {
                    included: true,
                    new_passes: 1,
                    data_length: 4,
                }],
            },
            code_block_data: vec![vec![1, 2, 3, 4]],
        };
        assert!(!pkt.header.is_empty);
        assert_eq!(pkt.code_block_data.len(), 1);
        assert_eq!(pkt.code_block_data[0].len(), 4);
    }
}
