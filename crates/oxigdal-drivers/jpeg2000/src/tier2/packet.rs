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

/// JPEG2000 tag-tree used for efficient entropy coding of the inclusion and
/// zero-bit-plane-count information.
///
/// The tag tree is a quadtree where each leaf corresponds to one code block and
/// every internal node holds the minimum of its children's values.  Only the
/// *delta* needed to reach a queried threshold is transmitted, so partial
/// state must persist between successive queries within one precinct.
///
/// This implementation stores, per node, a running lower bound (`low`) and the
/// resolved value (`value`, `u32::MAX` while still unknown), and refines them
/// top-down (root → leaf) exactly like the reference decoder (ISO 15444-1
/// §B.10.2, cf. `opj_tgt_decode`).
#[derive(Debug, Clone)]
pub struct TagTree {
    /// Dimensions per level (`level 0` = leaves, last = 1×1 root).
    level_dims: Vec<(u32, u32)>,
    /// Running lower bound per node, per level.
    low: Vec<Vec<u32>>,
    /// Resolved value per node (`u32::MAX` while unresolved), per level.
    value: Vec<Vec<u32>>,
    /// Number of tree levels.
    num_levels: usize,
}

impl TagTree {
    /// Construct a new tag tree for `width × height` code blocks.
    pub fn new(width: u32, height: u32) -> Self {
        let mut level_dims = Vec::new();
        let mut low = Vec::new();
        let mut value = Vec::new();
        let mut w = width.max(1);
        let mut h = height.max(1);
        loop {
            let count = (w as usize) * (h as usize);
            level_dims.push((w, h));
            low.push(vec![0u32; count]);
            value.push(vec![u32::MAX; count]);
            if w == 1 && h == 1 {
                break;
            }
            w = w.div_ceil(2);
            h = h.div_ceil(2);
        }
        let num_levels = level_dims.len();
        Self {
            level_dims,
            low,
            value,
            num_levels,
        }
    }

    /// Decode whether the value at leaf `(cx, cy)` is strictly `< threshold`,
    /// consuming bits and refining node state as needed.
    fn decode_lt(
        &mut self,
        cx: u32,
        cy: u32,
        threshold: u32,
        bits: &mut BitReader<'_>,
    ) -> Result<bool> {
        let mut low = 0u32;
        // Root (level num_levels-1) down to leaf (level 0).
        for k in (0..self.num_levels).rev() {
            let (wk, _hk) = self.level_dims[k];
            let nx = cx >> k;
            let ny = cy >> k;
            let idx = (ny as usize) * (wk as usize) + (nx as usize);

            // Inherit the parent's lower bound / publish ours.
            if low > self.low[k][idx] {
                self.low[k][idx] = low;
            } else {
                low = self.low[k][idx];
            }

            while low < threshold && low < self.value[k][idx] {
                if bits.read_bit()? == 1 {
                    self.value[k][idx] = low; // value resolved exactly at `low`
                } else {
                    low += 1;
                }
            }
            self.low[k][idx] = low;
        }
        Ok(low < threshold)
    }

    /// Decode whether the value at leaf `(cx, cy)` is `<= threshold`.
    ///
    /// Reads bits from `bits` as needed.  Returns `true` if the leaf value
    /// is `<= threshold`, `false` otherwise.
    pub fn decode_value(
        &mut self,
        cx: u32,
        cy: u32,
        threshold: u32,
        bits: &mut BitReader<'_>,
    ) -> Result<bool> {
        self.decode_lt(cx, cy, threshold.saturating_add(1), bits)
    }

    /// Fully decode the value at leaf `(cx, cy)` (used for zero-bit-plane
    /// counts) by sweeping the threshold until the value is pinned down.
    pub fn decode_full(&mut self, cx: u32, cy: u32, bits: &mut BitReader<'_>) -> Result<u32> {
        let mut t = 0u32;
        loop {
            if self.decode_lt(cx, cy, t + 1, bits)? {
                return Ok(t);
            }
            t += 1;
            if t > (1 << 20) {
                return Err(Jpeg2000Error::Tier2Error(
                    "tag-tree value did not converge".to_string(),
                ));
            }
        }
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

// ---------------------------------------------------------------------------
// Standard multi-subband precinct packet parser
// ---------------------------------------------------------------------------

/// One code block's contribution to a single packet, as recovered from a
/// packet header, together with the byte range of its compressed data inside
/// the packet buffer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CblkContribution {
    /// Whether this code block contributes to (is included in) this packet.
    pub included: bool,
    /// Number of new coding passes contributed.
    pub num_passes: u32,
    /// Zero-bit-plane count decoded at first inclusion.
    pub zbp: u32,
    /// Byte offset of the compressed data within the packet buffer.
    pub data_offset: usize,
    /// Byte length of the compressed data.
    pub data_len: usize,
}

/// `floor(log2(x))` for `x >= 1`; returns 0 for `x == 0`.
#[inline]
pub fn floor_log2(x: u32) -> u32 {
    if x == 0 { 0 } else { 31 - x.leading_zeros() }
}

/// Decode the number of new coding passes (ISO 15444-1 Table B.4).
pub fn read_num_coding_passes(bits: &mut BitReader<'_>) -> Result<u32> {
    if bits.read_bit()? == 0 {
        return Ok(1);
    }
    if bits.read_bit()? == 0 {
        return Ok(2);
    }
    let n = bits.read_bits(2)?;
    if n != 3 {
        return Ok(3 + n);
    }
    let n = bits.read_bits(5)?;
    if n != 31 {
        return Ok(6 + n);
    }
    let n = bits.read_bits(7)?;
    Ok(37 + n)
}

/// Decode the `Lblock` length-indicator increment (a comma code: a run of `1`
/// bits terminated by a `0`).
pub fn read_length_increment(bits: &mut BitReader<'_>) -> Result<u32> {
    let mut increment = 0u32;
    while bits.read_bit()? == 1 {
        increment += 1;
        if increment > 32 {
            return Err(Jpeg2000Error::Tier2Error(
                "Lblock length increment out of range".to_string(),
            ));
        }
    }
    Ok(increment)
}

/// Skip a trailing EPH (end-of-packet-header, `0xFF92`) marker if `has_eph`
/// is set and one is present at `pos`.
fn maybe_skip_eph(data: &[u8], pos: usize, has_eph: bool) -> usize {
    if has_eph && pos + 2 <= data.len() && data[pos] == 0xFF && data[pos + 1] == 0x92 {
        pos + 2
    } else {
        pos
    }
}

/// Parse one precinct packet spanning the given subbands.
///
/// This is the real Tier-2 demultiplexer: it reads the zero-length packet bit,
/// then — for each subband, in packet scan order — an inclusion tag tree and a
/// zero-bit-plane tag tree, the number of coding passes, and the `Lblock`
/// length signalling, so the compressed byte range of every included code block
/// can be sliced precisely (ISO 15444-1 §B.10).
///
/// # Parameters
/// - `data`: bytes starting at the first byte of this packet's header.
/// - `subband_grids`: `(cblk_nx, cblk_ny)` code-block grid for each subband
///   present in the packet (`[LL]` at resolution 0, `[HL, LH, HH]` otherwise).
/// - `layer`: quality layer index (inclusion threshold).
/// - `has_eph`: whether an EPH marker terminates the packet header.
///
/// # Returns
/// `(bytes_consumed, contributions_per_subband)`, where each inner vector is in
/// raster (`by * nx + bx`) order and included entries carry the byte range of
/// their compressed data inside `data`.
pub fn parse_precinct_packet(
    data: &[u8],
    subband_grids: &[(u32, u32)],
    layer: u16,
    has_eph: bool,
) -> Result<(usize, Vec<Vec<CblkContribution>>)> {
    let mut bits = BitReader::new(data);

    // Zero-length packet bit.
    let present = bits.read_bit()? == 1;
    if !present {
        let consumed = maybe_skip_eph(data, bits.bytes_consumed(), has_eph);
        let empty = subband_grids
            .iter()
            .map(|&(nx, ny)| vec![CblkContribution::default(); (nx * ny) as usize])
            .collect();
        return Ok((consumed, empty));
    }

    let layer_threshold = u32::from(layer);
    let mut per_subband: Vec<Vec<CblkContribution>> = Vec::with_capacity(subband_grids.len());

    for &(nx, ny) in subband_grids {
        let count = (nx * ny) as usize;
        let mut inclusion_tree = TagTree::new(nx, ny);
        let mut zbp_tree = TagTree::new(nx, ny);
        let mut contributions = Vec::with_capacity(count);

        for by in 0..ny {
            for bx in 0..nx {
                let included = inclusion_tree.decode_value(bx, by, layer_threshold, &mut bits)?;
                if !included {
                    contributions.push(CblkContribution::default());
                    continue;
                }

                // First inclusion (single-layer): read the zero-bit-plane count.
                let zbp = zbp_tree.decode_full(bx, by, &mut bits)?;
                let num_passes = read_num_coding_passes(&mut bits)?;

                let lblock = 3 + read_length_increment(&mut bits)?;
                let nbits = lblock + floor_log2(num_passes);
                if nbits > 31 {
                    return Err(Jpeg2000Error::Tier2Error(
                        "code-block length field exceeds 31 bits".to_string(),
                    ));
                }
                let length = bits.read_bits(nbits as u8)?;

                contributions.push(CblkContribution {
                    included: true,
                    num_passes,
                    zbp,
                    data_offset: 0,
                    data_len: length as usize,
                });
            }
        }
        per_subband.push(contributions);
    }

    // The packet body begins at the next byte boundary (optionally past EPH).
    let mut pos = maybe_skip_eph(data, bits.bytes_consumed(), has_eph);

    for subband in &mut per_subband {
        for contribution in subband.iter_mut() {
            if !contribution.included {
                continue;
            }
            let end = pos.checked_add(contribution.data_len).ok_or_else(|| {
                Jpeg2000Error::Tier2Error("code-block length overflow".to_string())
            })?;
            if end > data.len() {
                return Err(Jpeg2000Error::InsufficientData {
                    expected: end,
                    actual: data.len(),
                });
            }
            contribution.data_offset = pos;
            pos = end;
        }
    }

    Ok((pos, per_subband))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    // --- Tag-tree encoder mirroring the standard decoder, for tests only. ---

    /// Minimal MSB-first bit writer for building synthetic packet headers.
    #[derive(Default)]
    struct BitWriter {
        bytes: Vec<u8>,
        cur: u8,
        nbits: u8,
    }

    impl BitWriter {
        fn write_bit(&mut self, bit: u8) {
            self.cur = (self.cur << 1) | (bit & 1);
            self.nbits += 1;
            if self.nbits == 8 {
                self.bytes.push(self.cur);
                self.cur = 0;
                self.nbits = 0;
            }
        }
        fn write_bits(&mut self, value: u32, n: u8) {
            for i in (0..n).rev() {
                self.write_bit(((value >> i) & 1) as u8);
            }
        }
        /// Pad with zero bits to the next byte boundary.
        fn align(&mut self) {
            while self.nbits != 0 {
                self.write_bit(0);
            }
        }
        fn into_bytes(mut self) -> Vec<u8> {
            self.align();
            self.bytes
        }
    }

    /// Encode the number of coding passes as the inverse of
    /// [`read_num_coding_passes`] for the common small values.
    fn write_num_passes(w: &mut BitWriter, passes: u32) {
        match passes {
            1 => w.write_bit(0),
            2 => {
                w.write_bit(1);
                w.write_bit(0);
            }
            3..=5 => {
                w.write_bit(1);
                w.write_bit(1);
                w.write_bits(passes - 3, 2);
            }
            6..=36 => {
                w.write_bit(1);
                w.write_bit(1);
                w.write_bits(3, 2);
                w.write_bits(passes - 6, 5);
            }
            _ => panic!("test helper only supports <=36 passes"),
        }
    }

    /// Encode a single-code-block (1×1 tag-tree) inclusion + length contribution.
    fn write_single_cblk(w: &mut BitWriter, zbp: u32, passes: u32, length: u32) {
        // 1x1 inclusion tag tree, threshold 0 => a single `1` bit = included.
        w.write_bit(1);
        // 1x1 zero-bit-plane tag tree => `zbp` zeros then a `1`.
        for _ in 0..zbp {
            w.write_bit(0);
        }
        w.write_bit(1);
        write_num_passes(w, passes);
        // Lblock increment so that (3 + inc) + floor_log2(passes) covers length.
        let need = if length == 0 {
            1
        } else {
            32 - length.leading_zeros()
        };
        let base = 3 + floor_log2(passes);
        let inc = need.saturating_sub(base);
        for _ in 0..inc {
            w.write_bit(1);
        }
        w.write_bit(0); // comma terminator
        let nbits = (base + inc) as u8;
        w.write_bits(length, nbits);
    }

    #[test]
    fn test_floor_log2() {
        assert_eq!(floor_log2(0), 0);
        assert_eq!(floor_log2(1), 0);
        assert_eq!(floor_log2(2), 1);
        assert_eq!(floor_log2(3), 1);
        assert_eq!(floor_log2(4), 2);
        assert_eq!(floor_log2(255), 7);
    }

    #[test]
    fn test_num_passes_round_trip() {
        for passes in [1u32, 2, 3, 4, 5, 6, 10, 20, 36] {
            let mut w = BitWriter::default();
            write_num_passes(&mut w, passes);
            let bytes = w.into_bytes();
            let mut br = BitReader::new(&bytes);
            assert_eq!(
                read_num_coding_passes(&mut br).expect("decode passes"),
                passes
            );
        }
    }

    #[test]
    fn test_tag_tree_full_value_1x1() {
        // Encode zbp = 3 as "0 0 0 1".
        let data = [0b0001_0000u8];
        let mut br = BitReader::new(&data);
        let mut tt = TagTree::new(1, 1);
        assert_eq!(tt.decode_full(0, 0, &mut br).expect("full"), 3);
    }

    #[test]
    fn test_tag_tree_multi_leaf_inclusion() {
        // 2x1 tag tree, both leaves value 0 (included at layer 0).
        // Encode with a matching encoder and check both decode as included.
        let mut w = BitWriter::default();
        // decode order: leaf(0,0) walks root then leaf; leaf(1,0) reuses root.
        // Root value 0: emit `1` (resolved at low 0) at root, `1` at leaf0.
        // Then leaf1: root already resolved -> emit `1` at leaf1.
        w.write_bit(1); // root -> value 0
        w.write_bit(1); // leaf0 -> value 0
        w.write_bit(1); // leaf1 -> value 0
        let bytes = w.into_bytes();
        let mut br = BitReader::new(&bytes);
        let mut tt = TagTree::new(2, 1);
        assert!(tt.decode_value(0, 0, 0, &mut br).expect("leaf0"));
        assert!(tt.decode_value(1, 0, 0, &mut br).expect("leaf1"));
    }

    #[test]
    fn test_parse_empty_packet() {
        let data = [0x00u8, 0xAA];
        let (consumed, subs) =
            parse_precinct_packet(&data, &[(1, 1)], 0, false).expect("empty packet");
        assert_eq!(consumed, 1);
        assert_eq!(subs.len(), 1);
        assert!(!subs[0][0].included);
    }

    #[test]
    fn test_parse_single_block_packet() {
        // Build header for one included code block, zbp=2, 1 pass, length=5.
        let mut w = BitWriter::default();
        w.write_bit(1); // packet present
        write_single_cblk(&mut w, 2, 1, 5);
        let mut bytes = w.into_bytes();
        let header_len = bytes.len();
        let body = [0x11u8, 0x22, 0x33, 0x44, 0x55];
        bytes.extend_from_slice(&body);

        let (consumed, subs) =
            parse_precinct_packet(&bytes, &[(1, 1)], 0, false).expect("single block");
        assert_eq!(subs.len(), 1);
        let c = &subs[0][0];
        assert!(c.included);
        assert_eq!(c.zbp, 2);
        assert_eq!(c.num_passes, 1);
        assert_eq!(c.data_len, 5);
        assert_eq!(c.data_offset, header_len);
        assert_eq!(&bytes[c.data_offset..c.data_offset + c.data_len], &body);
        assert_eq!(consumed, header_len + 5);
    }

    #[test]
    fn test_parse_multi_subband_packet() {
        // Resolution >0 packet: three subbands (HL, LH, HH), each 1x1, all
        // included with distinct lengths.  Verify byte ranges are contiguous
        // and routed to the right subband.
        let mut w = BitWriter::default();
        w.write_bit(1); // present
        write_single_cblk(&mut w, 0, 1, 2); // HL
        write_single_cblk(&mut w, 1, 1, 3); // LH
        write_single_cblk(&mut w, 0, 2, 4); // HH
        let mut bytes = w.into_bytes();
        let header_len = bytes.len();
        let body: Vec<u8> = (0..(2 + 3 + 4)).map(|i| i as u8 + 1).collect();
        bytes.extend_from_slice(&body);

        let grids = [(1u32, 1u32), (1, 1), (1, 1)];
        let (consumed, subs) =
            parse_precinct_packet(&bytes, &grids, 0, false).expect("multi subband");
        assert_eq!(subs.len(), 3);
        assert_eq!(subs[0][0].data_len, 2);
        assert_eq!(subs[1][0].data_len, 3);
        assert_eq!(subs[2][0].data_len, 4);
        // Contiguous, in HL, LH, HH order.
        assert_eq!(subs[0][0].data_offset, header_len);
        assert_eq!(subs[1][0].data_offset, header_len + 2);
        assert_eq!(subs[2][0].data_offset, header_len + 5);
        assert_eq!(consumed, header_len + 9);
    }

    #[test]
    fn test_parse_length_overrun_errors() {
        // Header claims a 200-byte code block but no body follows.
        let mut w = BitWriter::default();
        w.write_bit(1);
        write_single_cblk(&mut w, 0, 1, 200);
        let bytes = w.into_bytes();
        let err = parse_precinct_packet(&bytes, &[(1, 1)], 0, false);
        assert!(matches!(err, Err(Jpeg2000Error::InsufficientData { .. })));
    }

    #[test]
    fn test_bit_reader_basic() {
        // byte 0b10110100
        let data = [0b1011_0100u8];
        let mut br = BitReader::new(&data);
        assert_eq!(br.read_bit().expect("read bit 0"), 1);
        assert_eq!(br.read_bit().expect("read bit 1"), 0);
        assert_eq!(br.read_bit().expect("read bit 2"), 1);
        assert_eq!(br.read_bit().expect("read bit 3"), 1);
        assert_eq!(br.read_bit().expect("read bit 4"), 0);
        assert_eq!(br.read_bit().expect("read bit 5"), 1);
        assert_eq!(br.read_bit().expect("read bit 6"), 0);
        assert_eq!(br.read_bit().expect("read bit 7"), 0);
    }

    #[test]
    fn test_bit_reader_read_bits() {
        let data = [0b1010_1010u8, 0b1111_0000u8];
        let mut br = BitReader::new(&data);
        assert_eq!(br.read_bits(4).expect("read nibble 0"), 0b1010);
        assert_eq!(br.read_bits(4).expect("read nibble 1"), 0b1010);
        assert_eq!(br.read_bits(4).expect("read nibble 2"), 0b1111);
        assert_eq!(br.read_bits(4).expect("read nibble 3"), 0b0000);
    }

    #[test]
    fn test_bit_reader_exhaustion() {
        let data = [0x00u8];
        let mut br = BitReader::new(&data);
        for _ in 0..8 {
            br.read_bit().expect("read bit before exhaustion");
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
        let (pkt, consumed) =
            PacketDecoder::decode(&data, 2, 2, &mut prev).expect("decode empty packet");
        assert!(pkt.header.is_empty);
        assert!(pkt.code_block_data.is_empty());
        assert_eq!(consumed, 1);
    }

    #[test]
    fn test_packet_header_is_empty_flag() {
        let data = [0x00u8];
        let mut prev = vec![];
        let (pkt, _) = PacketDecoder::decode(&data, 1, 1, &mut prev).expect("decode packet header");
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
