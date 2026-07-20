//! Real Esri/GDAL LERC2 bit-stuffed block decoder.
//!
//! This module implements a faithful, pure-Rust port of the decode side of the
//! Esri LERC2 format (<https://github.com/Esri/lerc>). It reads the LERC2 blob
//! header, the run-length-encoded valid/invalid pixel mask, and the per-block
//! (micro-block) data — both the *one sweep* (raw, lossless) layout and the
//! *tiled* layout whose quantized values are packed with the LERC `BitStuffer2`
//! variable-bit-width bit-stuffer (including the lookup-table variant). Values
//! are dequantized back to `f64` via the exact LERC formula
//! `z = blockMin + quant * (2 * maxZError)` clamped to the block/depth `zMax`.
//!
//! Format versions 1..=6 headers are parsed; the tiled + one-sweep data paths
//! cover the common LERC2 v2/v3/v4 float and integer rasters produced by GDAL.
//! The Huffman-coded byte-tile / delta-Huffman float image modes are **not**
//! decoded: those blobs return an explicit [`CompressionError::DecompressionFailed`]
//! rather than fabricating wrong output (fail loud, never silent-wrong).

use oxigdal_core::error::{CompressionError, OxiGdalError, Result};

use super::{LercDataType, LercDecoded};

/// LERC2 file signature (6 bytes, note the trailing space).
const FILE_KEY: &[u8; 6] = b"Lerc2 ";

/// Highest LERC2 format version this parser recognises in the header.
const CURRENT_VERSION: i32 = 6;

/// Length of the fixed prefix covered *before* the Fletcher-32 checksum region
/// (`FileKey` + `version:i32` + `checksum:u32`).
const CHECKSUM_PREFIX_LEN: usize = 6 + 4 + 4;

/// LERC internal data type codes (matches the reference `Lerc2::DataType` enum).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dt {
    Char = 0,
    Byte = 1,
    Short = 2,
    UShort = 3,
    Int = 4,
    UInt = 5,
    Float = 6,
    Double = 7,
    Undefined = 8,
}

impl Dt {
    /// Maps a raw LERC data-type code onto a [`Dt`], returning
    /// [`Dt::Undefined`] for anything outside `0..=7`.
    fn from_code(code: i32) -> Self {
        match code {
            0 => Self::Char,
            1 => Self::Byte,
            2 => Self::Short,
            3 => Self::UShort,
            4 => Self::Int,
            5 => Self::UInt,
            6 => Self::Float,
            7 => Self::Double,
            _ => Self::Undefined,
        }
    }

    /// Validates an arithmetic result back into a [`Dt`] (`ValidateDataType`).
    fn validate(code: i32) -> Self {
        if (0..=7).contains(&code) {
            Self::from_code(code)
        } else {
            Self::Undefined
        }
    }

    /// Maps onto the public [`LercDataType`] used by the crate's codec API.
    fn to_public(self) -> Option<LercDataType> {
        Some(match self {
            Self::Char => LercDataType::Char,
            Self::Byte => LercDataType::Byte,
            Self::Short => LercDataType::Short,
            Self::UShort => LercDataType::UShort,
            Self::Int => LercDataType::Int,
            Self::UInt => LercDataType::UInt,
            Self::Float => LercDataType::Float,
            Self::Double => LercDataType::Double,
            Self::Undefined => return None,
        })
    }
}

/// Parsed LERC2 blob header (`Lerc2::HeaderInfo`).
#[derive(Debug, Clone)]
struct Header {
    version: i32,
    checksum: u32,
    n_rows: i32,
    n_cols: i32,
    n_depth: i32,
    num_valid_pixel: i32,
    micro_block_size: i32,
    blob_size: i32,
    dt: Dt,
    max_z_error: f64,
    z_min: f64,
    z_max: f64,
}

impl Header {
    /// `Lerc2::HeaderInfo::TryHuffmanInt` — integer Huffman is used for 8-bit
    /// signed/unsigned data at `maxZError == 0.5`.
    fn try_huffman_int(&self) -> bool {
        self.version >= 2 && matches!(self.dt, Dt::Byte | Dt::Char) && self.max_z_error == 0.5
    }

    /// `Lerc2::HeaderInfo::TryHuffmanFlt` — delta-delta Huffman float mode is a
    /// v6-only lossless-float encoding.
    fn try_huffman_flt(&self) -> bool {
        self.version >= 6 && matches!(self.dt, Dt::Float | Dt::Double) && self.max_z_error == 0.0
    }
}

// ---------------------------------------------------------------------------
// Byte cursor
// ---------------------------------------------------------------------------

/// A minimal forward byte cursor with bounds-checked, little-endian reads.
struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| err("LERC2 read overflow"))?;
        if end > self.data.len() {
            return Err(err("LERC2 truncated blob"));
        }
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn read_u8(&mut self) -> Result<u8> {
        let s = self.take(1)?;
        Ok(s[0])
    }

    fn read_i16(&mut self) -> Result<i16> {
        let s = self.take(2)?;
        Ok(i16::from_le_bytes([s[0], s[1]]))
    }

    fn read_u16(&mut self) -> Result<u16> {
        let s = self.take(2)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }

    fn read_i32(&mut self) -> Result<i32> {
        let s = self.take(4)?;
        Ok(i32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }

    fn read_u32(&mut self) -> Result<u32> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }

    fn read_f32(&mut self) -> Result<f32> {
        let s = self.take(4)?;
        Ok(f32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }

    fn read_f64(&mut self) -> Result<f64> {
        let s = self.take(8)?;
        Ok(f64::from_le_bytes([
            s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7],
        ]))
    }

    /// Reads one value of LERC data type `dt`, widened to `f64`.
    fn read_native(&mut self, dt: Dt) -> Result<f64> {
        Ok(match dt {
            Dt::Char => i32::from(self.read_u8()? as i8) as f64,
            Dt::Byte => f64::from(self.read_u8()?),
            Dt::Short => f64::from(self.read_i16()?),
            Dt::UShort => f64::from(self.read_u16()?),
            Dt::Int => f64::from(self.read_i32()?),
            Dt::UInt => f64::from(self.read_u32()?),
            Dt::Float => f64::from(self.read_f32()?),
            Dt::Double => self.read_f64()?,
            Dt::Undefined => return Err(err("LERC2 undefined data type in stream")),
        })
    }
}

/// Builds a `DecompressionFailed` error with the given message.
fn err(msg: impl Into<String>) -> OxiGdalError {
    OxiGdalError::Compression(CompressionError::DecompressionFailed {
        message: msg.into(),
    })
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Attempts to decode `data` as a real Esri/GDAL LERC2 blob.
///
/// Returns:
/// * `None` — the blob is *not* a real LERC2 stream (wrong magic, or a version
///   field outside the valid `0..=6` range, as produced by this crate's own
///   raw-value payload format). The caller should fall back to its raw decoder.
/// * `Some(Ok(..))` — a real LERC2 blob that decoded successfully.
/// * `Some(Err(..))` — a real LERC2 blob (valid magic + version) that could not
///   be decoded (truncation, bad checksum, or an unsupported Huffman sub-variant).
///   This is a hard, explicit failure — never a silently-wrong raster.
pub fn try_decode(data: &[u8]) -> Option<Result<LercDecoded>> {
    if data.len() < CHECKSUM_PREFIX_LEN || &data[0..6] != FILE_KEY {
        return None;
    }
    let version = i32::from_le_bytes([data[6], data[7], data[8], data[9]]);
    if !(0..=CURRENT_VERSION).contains(&version) {
        // Not a real LERC2 blob — e.g. this crate's own raw-value format whose
        // header stores nDim=1 at byte 9, pushing the version field far out of
        // range. Let the caller try its raw decoder.
        return None;
    }
    Some(decode_real(data))
}

/// Decodes a blob already confirmed (by [`try_decode`]) to have valid magic and
/// an in-range version field.
fn decode_real(data: &[u8]) -> Result<LercDecoded> {
    let mut cur = Cursor::new(data);
    let hd = parse_header(&mut cur)?;

    if hd.n_cols <= 0 || hd.n_rows <= 0 || hd.n_depth <= 0 {
        return Err(err(format!(
            "LERC2 header has non-positive dimension: cols={}, rows={}, depth={}",
            hd.n_cols, hd.n_rows, hd.n_depth
        )));
    }
    if hd.blob_size < 0 || (hd.blob_size as usize) > data.len() {
        return Err(err(format!(
            "LERC2 blobSize {} inconsistent with blob length {}",
            hd.blob_size,
            data.len()
        )));
    }
    if hd.micro_block_size <= 0 || hd.micro_block_size > 32 {
        return Err(err(format!(
            "LERC2 microBlockSize {} out of range",
            hd.micro_block_size
        )));
    }

    let n_cols = hd.n_cols as usize;
    let n_rows = hd.n_rows as usize;
    let n_depth = hd.n_depth as usize;
    let n_pix = n_cols
        .checked_mul(n_rows)
        .and_then(|v| v.checked_mul(n_depth))
        .ok_or_else(|| err("LERC2 dimensions overflow"))?;

    // Verify the Fletcher-32 checksum for v3+ (the reference rejects on mismatch).
    if hd.version >= 3 {
        let blob = &data[..hd.blob_size as usize];
        if blob.len() < CHECKSUM_PREFIX_LEN {
            return Err(err("LERC2 blob too small for checksum"));
        }
        let computed = fletcher32(&blob[CHECKSUM_PREFIX_LEN..]);
        if computed != hd.checksum {
            return Err(err(format!(
                "LERC2 checksum mismatch: header {:#010x}, computed {:#010x}",
                hd.checksum, computed
            )));
        }
    }

    let mask = read_mask(&mut cur, &hd)?;

    let public_dt = hd
        .dt
        .to_public()
        .ok_or_else(|| err(format!("LERC2 undefined data type code {}", hd.dt as i32)))?;

    let mut values = vec![0.0_f64; n_pix];

    let build = |values: Vec<f64>| LercDecoded {
        values,
        n_cols: hd.n_cols as u32,
        n_rows: hd.n_rows as u32,
        n_bands: hd.n_depth as u32,
        data_type: public_dt,
    };

    // No valid pixels: the whole raster is invalid (all zeros / nodata).
    if hd.num_valid_pixel == 0 {
        return Ok(build(values));
    }

    // Whole image constant (single value across all valid pixels).
    if hd.z_min == hd.z_max {
        fill_const(&mut values, &hd, &mask, None);
        return Ok(build(values));
    }

    // v4+ carries explicit per-depth min/max ranges before the data section.
    let mut z_max_vec: Option<Vec<f64>> = None;
    if hd.version >= 4 {
        let (z_min_vec, z_maxs) = read_min_max_ranges(&mut cur, &hd)?;
        if z_min_vec == z_maxs {
            fill_const(&mut values, &hd, &mask, Some(&z_min_vec));
            return Ok(build(values));
        }
        z_max_vec = Some(z_maxs);
    }

    let read_data_one_sweep = cur.read_u8()?;
    if read_data_one_sweep != 0 {
        read_one_sweep(&mut cur, &hd, &mask, &mut values)?;
        return Ok(build(values));
    }

    // Optional image-encode-mode flag (only present for the Huffman-eligible
    // configurations). Non-tiling modes are Huffman/delta variants we do not
    // decode — fail loud rather than emit garbage.
    if hd.try_huffman_int() || hd.try_huffman_flt() {
        let flag = cur.read_u8()?;
        if flag > 3 || (flag > 2 && hd.version < 6) || (flag > 1 && hd.version < 4) {
            return Err(err(format!("LERC2 invalid image encode mode {flag}")));
        }
        // 0 == IEM_Tiling. Anything else is a Huffman/delta image mode.
        if flag != 0 {
            return Err(err(format!(
                "LERC2 Huffman/delta image encode mode {flag} is not implemented \
                 (data type {:?}, {}x{}x{}); refusing to fabricate output",
                hd.dt, hd.n_cols, hd.n_rows, hd.n_depth
            )));
        }
    }

    read_tiles(&mut cur, &hd, &mask, z_max_vec.as_deref(), &mut values)?;
    Ok(build(values))
}

// ---------------------------------------------------------------------------
// Header
// ---------------------------------------------------------------------------

/// Parses the LERC2 header, advancing `cur` to the first byte of the mask.
fn parse_header(cur: &mut Cursor) -> Result<Header> {
    let key = cur.take(6)?;
    if key != FILE_KEY {
        return Err(err("LERC2 bad file signature"));
    }
    let version = cur.read_i32()?;
    if !(0..=CURRENT_VERSION).contains(&version) {
        return Err(err(format!("LERC2 unsupported version {version}")));
    }
    let checksum = if version >= 3 { cur.read_u32()? } else { 0 };

    let n_rows = cur.read_i32()?;
    let n_cols = cur.read_i32()?;
    let n_depth = if version >= 4 { cur.read_i32()? } else { 1 };
    let num_valid_pixel = cur.read_i32()?;
    let micro_block_size = cur.read_i32()?;
    let blob_size = cur.read_i32()?;
    let dt_code = cur.read_i32()?;
    if version >= 6 {
        let _n_blobs_more = cur.read_i32()?;
        // 4 flag bytes: bPassNoDataValues, bIsInt, bReserved3, bReserved4.
        let _flags = cur.take(4)?;
    }

    let max_z_error = cur.read_f64()?;
    let z_min = cur.read_f64()?;
    let z_max = cur.read_f64()?;
    if version >= 6 {
        // noDataVal, noDataValOrig.
        let _no_data = cur.read_f64()?;
        let _no_data_orig = cur.read_f64()?;
    }

    Ok(Header {
        version,
        checksum,
        n_rows,
        n_cols,
        n_depth,
        num_valid_pixel,
        micro_block_size,
        blob_size,
        dt: Dt::from_code(dt_code),
        max_z_error,
        z_min,
        z_max,
    })
}

// ---------------------------------------------------------------------------
// Fletcher-32 checksum
// ---------------------------------------------------------------------------

/// Computes the LERC Fletcher-32 checksum (`Lerc2::ComputeChecksumFletcher32`).
fn fletcher32(bytes: &[u8]) -> u32 {
    let mut sum1: u32 = 0xffff;
    let mut sum2: u32 = 0xffff;
    let len = bytes.len();
    let mut words = len / 2;
    let mut idx = 0usize;

    while words > 0 {
        let mut tlen = if words >= 359 { 359 } else { words };
        words -= tlen;
        loop {
            let hi = u32::from(bytes[idx]) << 8;
            let lo = u32::from(bytes[idx + 1]);
            idx += 2;
            sum1 = sum1.wrapping_add(hi);
            sum1 = sum1.wrapping_add(lo);
            sum2 = sum2.wrapping_add(sum1);
            tlen -= 1;
            if tlen == 0 {
                break;
            }
        }
        sum1 = (sum1 & 0xffff) + (sum1 >> 16);
        sum2 = (sum2 & 0xffff) + (sum2 >> 16);
    }

    if len & 1 == 1 {
        sum1 = sum1.wrapping_add(u32::from(bytes[idx]) << 8);
        sum2 = sum2.wrapping_add(sum1);
    }

    sum1 = (sum1 & 0xffff) + (sum1 >> 16);
    sum2 = (sum2 & 0xffff) + (sum2 >> 16);
    (sum2 << 16) | sum1
}

// ---------------------------------------------------------------------------
// Valid/invalid mask
// ---------------------------------------------------------------------------

/// A packed 1-bit-per-pixel valid mask, MSB-first within each byte
/// (`BitMask`, `Bit(k) = (1 << 7) >> (k & 7)`).
struct BitMask {
    bits: Vec<u8>,
    n: usize,
    all_valid: bool,
}

impl BitMask {
    fn all_valid(n: usize) -> Self {
        Self {
            bits: Vec::new(),
            n,
            all_valid: true,
        }
    }

    fn all_invalid(n: usize) -> Self {
        Self {
            bits: vec![0u8; n.div_ceil(8)],
            n,
            all_valid: false,
        }
    }

    fn from_bits(bits: Vec<u8>, n: usize) -> Self {
        Self {
            bits,
            n,
            all_valid: false,
        }
    }

    #[inline]
    fn is_valid(&self, k: usize) -> bool {
        if self.all_valid {
            return true;
        }
        if k >= self.n {
            return false;
        }
        let byte = self.bits.get(k >> 3).copied().unwrap_or(0);
        (byte & (0x80u8 >> (k & 7))) != 0
    }
}

/// Reads the mask section (`Lerc2::ReadMask`): a 4-byte `numBytesMask` count
/// followed, when positive, by an RLE-compressed bitmap.
fn read_mask(cur: &mut Cursor, hd: &Header) -> Result<BitMask> {
    let w = hd.n_cols as usize;
    let h = hd.n_rows as usize;
    let n = w
        .checked_mul(h)
        .ok_or_else(|| err("LERC2 mask size overflow"))?;

    let num_bytes_mask = cur.read_i32()?;
    let num_valid = hd.num_valid_pixel;

    // A fully-valid or fully-invalid raster must not carry a mask body.
    if (num_valid == 0 || (num_valid as i64) == n as i64) && num_bytes_mask != 0 {
        return Err(err(
            "LERC2 mask present but pixel count is all-valid/all-invalid",
        ));
    }

    if num_valid == 0 {
        return Ok(BitMask::all_invalid(n));
    }
    if (num_valid as i64) == n as i64 {
        return Ok(BitMask::all_valid(n));
    }
    if num_bytes_mask < 0 {
        return Err(err("LERC2 negative mask byte count"));
    }
    if num_bytes_mask == 0 {
        // Mask is unchanged from the (implicit) previous state; without a prior
        // frame there is nothing to reuse. Treat as all-valid: LERC only writes
        // 0 here when the mask matches the running state, and for a standalone
        // blob that running state is all-valid.
        return Ok(BitMask::all_valid(n));
    }

    let nbytes = num_bytes_mask as usize;
    let body = cur.take(nbytes)?;
    let mut bits = vec![0u8; n.div_ceil(8)];
    rle_decompress(body, &mut bits)?;
    Ok(BitMask::from_bits(bits, n))
}

/// Decodes the LERC RLE stream (`RLE::decompress`) into `out`.
///
/// Wire format: a sequence of `[count:i16-le][payload]` tokens terminated by the
/// sentinel count `-32768`. A positive count copies that many literal bytes; a
/// non-positive count `-c` repeats one following byte `c` times.
fn rle_decompress(src: &[u8], out: &mut [u8]) -> Result<()> {
    let mut cur = Cursor::new(src);
    let mut out_idx = 0usize;
    loop {
        let cnt = cur.read_i16()?;
        if cnt == -32768 {
            break;
        }
        if cnt > 0 {
            let n = cnt as usize;
            let slice = cur.take(n)?;
            if out_idx + n > out.len() {
                return Err(err("LERC2 RLE mask overflow (literal)"));
            }
            out[out_idx..out_idx + n].copy_from_slice(slice);
            out_idx += n;
        } else {
            // cnt <= 0: repeat one byte |cnt| times (|0| == 0 consumes a byte,
            // writes nothing — matches the reference).
            let n = (-(cnt as i32)) as usize;
            let b = cur.read_u8()?;
            if out_idx + n > out.len() {
                return Err(err("LERC2 RLE mask overflow (run)"));
            }
            for slot in out.iter_mut().skip(out_idx).take(n) {
                *slot = b;
            }
            out_idx += n;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Min/max ranges (v4+)
// ---------------------------------------------------------------------------

/// Reads the per-depth `zMin`/`zMax` ranges (`Lerc2::ReadMinMaxRanges`) written
/// for version >= 4. Returns `(zMinVec, zMaxVec)` widened to `f64`.
fn read_min_max_ranges(cur: &mut Cursor, hd: &Header) -> Result<(Vec<f64>, Vec<f64>)> {
    let n_depth = hd.n_depth as usize;
    let mut z_min = Vec::with_capacity(n_depth);
    for _ in 0..n_depth {
        z_min.push(cur.read_native(hd.dt)?);
    }
    let mut z_max = Vec::with_capacity(n_depth);
    for _ in 0..n_depth {
        z_max.push(cur.read_native(hd.dt)?);
    }
    Ok((z_min, z_max))
}

// ---------------------------------------------------------------------------
// Constant / one-sweep fills
// ---------------------------------------------------------------------------

/// Fills all valid pixels with a constant value (`Lerc2::FillConstImage`).
///
/// When `per_depth_min` is `Some`, each depth slot `d` is filled with
/// `per_depth_min[d]`; otherwise every slot is filled with `hd.z_min`.
fn fill_const(values: &mut [f64], hd: &Header, mask: &BitMask, per_depth_min: Option<&[f64]>) {
    let n_cols = hd.n_cols as usize;
    let n_rows = hd.n_rows as usize;
    let n_depth = hd.n_depth as usize;
    let z0 = cast_to_dt(hd.z_min, hd.dt);

    let mut k = 0usize;
    let mut m = 0usize;
    for _i in 0..n_rows {
        for _j in 0..n_cols {
            if mask.is_valid(k) {
                for d in 0..n_depth {
                    let v = match per_depth_min {
                        Some(mins) => cast_to_dt(mins.get(d).copied().unwrap_or(hd.z_min), hd.dt),
                        None => z0,
                    };
                    if let Some(slot) = values.get_mut(m + d) {
                        *slot = v;
                    }
                }
            }
            k += 1;
            m += n_depth;
        }
    }
}

/// Reads the raw, losslessly-stored valid pixels (`Lerc2::ReadDataOneSweep`).
fn read_one_sweep(cur: &mut Cursor, hd: &Header, mask: &BitMask, values: &mut [f64]) -> Result<()> {
    let n_cols = hd.n_cols as usize;
    let n_rows = hd.n_rows as usize;
    let n_depth = hd.n_depth as usize;

    let mut k = 0usize;
    let mut m = 0usize;
    for _i in 0..n_rows {
        for _j in 0..n_cols {
            if mask.is_valid(k) {
                for d in 0..n_depth {
                    let v = cur.read_native(hd.dt)?;
                    if let Some(slot) = values.get_mut(m + d) {
                        *slot = v;
                    }
                }
            }
            k += 1;
            m += n_depth;
        }
    }
    Ok(())
}

/// Casts `z` through the header data type and back to `f64`, replicating the
/// reference `(T)z` narrowing for integer output types (`as` saturates rather
/// than invoking undefined behaviour).
fn cast_to_dt(z: f64, dt: Dt) -> f64 {
    match dt {
        Dt::Char => f64::from(z as i8),
        Dt::Byte => f64::from(z as u8),
        Dt::Short => f64::from(z as i16),
        Dt::UShort => f64::from(z as u16),
        Dt::Int => f64::from(z as i32),
        Dt::UInt => f64::from(z as u32),
        Dt::Float => f64::from(z as f32),
        Dt::Double | Dt::Undefined => z,
    }
}

// ---------------------------------------------------------------------------
// Tiled data
// ---------------------------------------------------------------------------

/// Iterates the micro-block grid, decoding each tile (`Lerc2::ReadTiles`).
fn read_tiles(
    cur: &mut Cursor,
    hd: &Header,
    mask: &BitMask,
    z_max_vec: Option<&[f64]>,
    values: &mut [f64],
) -> Result<()> {
    let mb = hd.micro_block_size as usize;
    let n_rows = hd.n_rows as usize;
    let n_cols = hd.n_cols as usize;
    let n_depth = hd.n_depth as usize;

    let num_tiles_vert = n_rows.div_ceil(mb);
    let num_tiles_hori = n_cols.div_ceil(mb);

    let mut buffer: Vec<u32> = Vec::new();

    for i_tile in 0..num_tiles_vert {
        let i0 = i_tile * mb;
        let tile_h = if i_tile == num_tiles_vert - 1 {
            n_rows - i0
        } else {
            mb
        };
        for j_tile in 0..num_tiles_hori {
            let j0 = j_tile * mb;
            let tile_w = if j_tile == num_tiles_hori - 1 {
                n_cols - j0
            } else {
                mb
            };
            for i_depth in 0..n_depth {
                read_tile(
                    cur,
                    hd,
                    mask,
                    z_max_vec,
                    values,
                    i0,
                    i0 + tile_h,
                    j0,
                    j0 + tile_w,
                    i_depth,
                    &mut buffer,
                )?;
            }
        }
    }
    Ok(())
}

/// Decodes a single micro-block for one depth slice (`Lerc2::ReadTile`).
#[allow(clippy::too_many_arguments)]
fn read_tile(
    cur: &mut Cursor,
    hd: &Header,
    mask: &BitMask,
    z_max_vec: Option<&[f64]>,
    values: &mut [f64],
    i0: usize,
    i1: usize,
    j0: usize,
    j1: usize,
    i_depth: usize,
    buffer: &mut Vec<u32>,
) -> Result<()> {
    let n_cols = hd.n_cols as usize;
    let n_depth = hd.n_depth as usize;

    let compr_flag_full = cur.read_u8()?;

    let b_diff_enc = if hd.version >= 5 {
        (compr_flag_full & 4) != 0
    } else {
        false
    };
    let pattern: u8 = if hd.version >= 5 { 14 } else { 15 };

    // Column-offset integrity check.
    if ((compr_flag_full >> 2) & pattern) != (((j0 >> 3) as u8) & pattern) {
        return Err(err("LERC2 tile column offset check failed (corrupt blob)"));
    }
    if b_diff_enc && i_depth == 0 {
        return Err(err("LERC2 differential tile at depth 0 (corrupt blob)"));
    }

    let bits67 = (compr_flag_full >> 6) as i32;
    let compr_flag = compr_flag_full & 3;

    // Helper to index into the flat, depth-interleaved `values` buffer.
    let idx = |i: usize, j: usize| -> usize { (i * n_cols + j) * n_depth + i_depth };

    if compr_flag == 2 {
        // Entire tile is constant 0 (or a copy of the previous depth for diff).
        for i in i0..i1 {
            for j in j0..j1 {
                let k = i * n_cols + j;
                if mask.is_valid(k) {
                    let m = idx(i, j);
                    let v = if b_diff_enc {
                        values.get(m.wrapping_sub(1)).copied().unwrap_or(0.0)
                    } else {
                        0.0
                    };
                    if let Some(slot) = values.get_mut(m) {
                        *slot = v;
                    }
                }
            }
        }
        return Ok(());
    }

    if compr_flag == 0 {
        // Raw z's, uncompressed, native type, in valid-pixel order.
        if b_diff_enc {
            return Err(err("LERC2 raw tile flagged differential (corrupt blob)"));
        }
        for i in i0..i1 {
            for j in j0..j1 {
                let k = i * n_cols + j;
                if mask.is_valid(k) {
                    let v = cur.read_native(hd.dt)?;
                    if let Some(slot) = values.get_mut(idx(i, j)) {
                        *slot = v;
                    }
                }
            }
        }
        return Ok(());
    }

    // compr_flag == 1 or 3: a block minimum (offset) prefixes the payload.
    let base_dt = if b_diff_enc && (hd.dt as i32) < (Dt::Float as i32) {
        Dt::Int
    } else {
        hd.dt
    };
    let dt_used = get_data_type_used(base_dt, bits67);
    if dt_used == Dt::Undefined {
        return Err(err("LERC2 tile uses undefined reduced data type"));
    }
    let offset = cur.read_native(dt_used)?;

    let z_max = if hd.version >= 4 && n_depth > 1 {
        z_max_vec
            .and_then(|v| v.get(i_depth).copied())
            .unwrap_or(hd.z_max)
    } else {
        hd.z_max
    };

    if compr_flag == 3 {
        // Entire tile is the constant block minimum.
        for i in i0..i1 {
            for j in j0..j1 {
                let k = i * n_cols + j;
                if mask.is_valid(k) {
                    let m = idx(i, j);
                    let z = if b_diff_enc {
                        offset + values.get(m.wrapping_sub(1)).copied().unwrap_or(0.0)
                    } else {
                        offset
                    };
                    let v = cast_to_dt(z.min(z_max), hd.dt);
                    if let Some(slot) = values.get_mut(m) {
                        *slot = v;
                    }
                }
            }
        }
        return Ok(());
    }

    // compr_flag == 1: bit-stuffed quantized values.
    let tile_h = i1 - i0;
    let tile_w = j1 - j0;
    let max_element_count = tile_h
        .checked_mul(tile_w)
        .ok_or_else(|| err("LERC2 tile element count overflow"))?;

    bitstuffer_decode(cur, buffer, max_element_count, hd.version)?;
    let inv_scale = 2.0 * hd.max_z_error;

    // When the encoder stored one quantized value per tile pixel, `all_valid`
    // is true and every position (mask notwithstanding) consumes a value; else
    // only valid pixels consume values, in raster order.
    let all_valid = buffer.len() == max_element_count;
    let mut src = 0usize;

    for i in i0..i1 {
        for j in j0..j1 {
            let k = i * n_cols + j;
            if !all_valid && !mask.is_valid(k) {
                continue;
            }
            let q = *buffer
                .get(src)
                .ok_or_else(|| err("LERC2 bit-stuffed tile underflow"))?;
            src += 1;
            let m = idx(i, j);
            let mut z = offset + f64::from(q) * inv_scale;
            if b_diff_enc {
                z += values.get(m.wrapping_sub(1)).copied().unwrap_or(0.0);
            }
            let v = cast_to_dt(z.min(z_max), hd.dt);
            if let Some(slot) = values.get_mut(m) {
                *slot = v;
            }
        }
    }
    Ok(())
}

/// `Lerc2::GetDataTypeUsed` — the reduced storage type for a block minimum.
fn get_data_type_used(dt: Dt, tc: i32) -> Dt {
    let d = dt as i32;
    match dt {
        Dt::Short | Dt::Int => Dt::validate(d - tc),
        Dt::UShort | Dt::UInt => Dt::validate(d - 2 * tc),
        Dt::Float => {
            if tc == 0 {
                Dt::Float
            } else if tc == 1 {
                Dt::Short
            } else {
                Dt::Byte
            }
        }
        Dt::Double => {
            if tc == 0 {
                Dt::Double
            } else {
                Dt::validate(d - 2 * tc + 1)
            }
        }
        _ => dt,
    }
}

// ---------------------------------------------------------------------------
// BitStuffer2
// ---------------------------------------------------------------------------

/// Decodes one `BitStuffer2` block into `out` (`BitStuffer2::Decode`).
///
/// The first byte packs `numBits` (bits 0-4), a LUT flag (bit 5) and a 2-bit
/// code (bits 6-7) selecting how many bytes encode the element count. The body
/// is either simple bit-stuffed values or a lookup table followed by
/// bit-stuffed indices.
fn bitstuffer_decode(
    cur: &mut Cursor,
    out: &mut Vec<u32>,
    max_element_count: usize,
    version: i32,
) -> Result<()> {
    out.clear();

    let num_bits_byte = cur.read_u8()?;
    let bits67 = (num_bits_byte >> 6) as i32;
    let nb = if bits67 == 0 { 4 } else { 3 - bits67 };
    let do_lut = (num_bits_byte & (1 << 5)) != 0;
    let num_bits = (num_bits_byte & 31) as i32;

    let num_elements = decode_var_uint(cur, nb)? as usize;
    if num_elements > max_element_count {
        return Err(err("LERC2 bit-stuffer element count exceeds tile"));
    }

    if !do_lut {
        if num_bits > 0 {
            bit_unstuff(cur, out, num_elements, num_bits, version)?;
        }
        // num_bits == 0 leaves `out` empty (all-zero deltas).
        return Ok(());
    }

    // LUT variant.
    if num_bits == 0 {
        return Err(err("LERC2 bit-stuffer LUT with numBits==0"));
    }
    let n_lut_byte = cur.read_u8()?;
    let n_lut = (n_lut_byte as i32) - 1;
    if n_lut < 0 {
        return Err(err("LERC2 bit-stuffer bad LUT size"));
    }
    let n_lut = n_lut as usize;

    let mut lut: Vec<u32> = Vec::new();
    bit_unstuff(cur, &mut lut, n_lut, num_bits, version)?;

    let mut n_bits_lut = 0i32;
    while (n_lut >> n_bits_lut) != 0 {
        n_bits_lut += 1;
    }
    if n_bits_lut == 0 {
        return Err(err("LERC2 bit-stuffer degenerate LUT index width"));
    }

    bit_unstuff(cur, out, num_elements, n_bits_lut, version)?;

    // Re-insert the implicit 0 at the front and map indices to values.
    lut.insert(0, 0);
    for v in out.iter_mut() {
        let i = *v as usize;
        *v = *lut
            .get(i)
            .ok_or_else(|| err("LERC2 bit-stuffer LUT index out of range"))?;
    }
    Ok(())
}

/// `BitStuffer2::DecodeUInt` — reads a 1/2/4-byte little-endian element count.
fn decode_var_uint(cur: &mut Cursor, num_bytes: i32) -> Result<u32> {
    match num_bytes {
        1 => Ok(u32::from(cur.read_u8()?)),
        2 => Ok(u32::from(cur.read_u16()?)),
        4 => cur.read_u32(),
        _ => Err(err("LERC2 bit-stuffer invalid count width")),
    }
}

/// `BitStuffer2::NumTailBytesNotNeeded`.
fn num_tail_bytes_not_needed(num_elem: usize, num_bits: i32) -> usize {
    let num_bits_tail = ((num_elem as u64).wrapping_mul(num_bits as u64) & 31) as u32;
    let num_bytes_tail = (num_bits_tail + 7) >> 3;
    if num_bytes_tail > 0 {
        (4 - num_bytes_tail) as usize
    } else {
        0
    }
}

/// Dispatches to the version-appropriate bit-unstuffer.
fn bit_unstuff(
    cur: &mut Cursor,
    out: &mut Vec<u32>,
    num_elements: usize,
    num_bits: i32,
    version: i32,
) -> Result<()> {
    if version >= 3 {
        bit_unstuff_v3(cur, out, num_elements, num_bits)
    } else {
        bit_unstuff_pre_v3(cur, out, num_elements, num_bits)
    }
}

/// `BitStuffer2::BitUnStuff` (LERC2 v3+ LSB-first packing).
fn bit_unstuff_v3(
    cur: &mut Cursor,
    out: &mut Vec<u32>,
    num_elements: usize,
    num_bits: i32,
) -> Result<()> {
    if num_elements == 0 || !(1..32).contains(&num_bits) {
        return Err(err("LERC2 bit-stuffer bad numBits"));
    }
    let num_uints = (num_elements * num_bits as usize).div_ceil(32);
    let num_bytes = num_uints * 4;
    let num_bytes_used = num_bytes - num_tail_bytes_not_needed(num_elements, num_bits);

    let body = cur.take(num_bytes_used)?;
    let src = load_u32_le(body, num_uints);

    out.clear();
    out.resize(num_elements, 0);

    let nb = 32 - num_bits;
    let mut bit_pos = 0i32;
    let mut src_idx = 0usize;

    for slot in out.iter_mut() {
        let cur_word = *src
            .get(src_idx)
            .ok_or_else(|| err("LERC2 bit-stuffer source underflow"))?;
        if nb - bit_pos >= 0 {
            *slot = (cur_word << (nb - bit_pos)) >> nb;
            bit_pos += num_bits;
            if bit_pos == 32 {
                src_idx += 1;
                bit_pos = 0;
            }
        } else {
            let mut v = cur_word >> bit_pos;
            src_idx += 1;
            let next = *src
                .get(src_idx)
                .ok_or_else(|| err("LERC2 bit-stuffer source underflow"))?;
            let shl = 64 - num_bits - bit_pos;
            v |= (next << shl) >> nb;
            *slot = v;
            bit_pos -= nb;
        }
    }

    Ok(())
}

/// `BitStuffer2::BitUnStuff_Before_Lerc2v3` (LERC2 v1/v2 MSB-first packing).
fn bit_unstuff_pre_v3(
    cur: &mut Cursor,
    out: &mut Vec<u32>,
    num_elements: usize,
    num_bits: i32,
) -> Result<()> {
    if num_elements == 0 || !(1..32).contains(&num_bits) {
        return Err(err("LERC2 bit-stuffer bad numBits"));
    }
    let num_uints = (num_elements * num_bits as usize).div_ceil(32);
    let n_bytes_to_copy = (num_elements * num_bits as usize).div_ceil(8);

    let body = cur.take(n_bytes_to_copy)?;
    let mut src = load_u32_le(body, num_uints);

    // Shift the last uint left by 8 bits per missing tail byte.
    let ntbnn = num_tail_bytes_not_needed(num_elements, num_bits);
    if let Some(last) = src.get_mut(num_uints.saturating_sub(1)) {
        for _ in 0..ntbnn {
            *last <<= 8;
        }
    }

    out.clear();
    out.resize(num_elements, 0);

    let mut bit_pos = 0i32;
    let mut src_idx = 0usize;

    for slot in out.iter_mut() {
        if 32 - bit_pos >= num_bits {
            let val = *src
                .get(src_idx)
                .ok_or_else(|| err("LERC2 bit-stuffer source underflow"))?;
            let n = val << bit_pos;
            *slot = n >> (32 - num_bits);
            bit_pos += num_bits;
            if bit_pos == 32 {
                bit_pos = 0;
                src_idx += 1;
            }
        } else {
            let val = *src
                .get(src_idx)
                .ok_or_else(|| err("LERC2 bit-stuffer source underflow"))?;
            src_idx += 1;
            let n = val << bit_pos;
            *slot = n >> (32 - num_bits);
            bit_pos -= 32 - num_bits;
            let val2 = *src
                .get(src_idx)
                .ok_or_else(|| err("LERC2 bit-stuffer source underflow"))?;
            *slot |= val2 >> (32 - bit_pos);
        }
    }

    Ok(())
}

/// Loads `num_uints` little-endian `u32` words from `body`, zero-padding the
/// final partial word (the reference presets the last uint to 0 before copy).
fn load_u32_le(body: &[u8], num_uints: usize) -> Vec<u32> {
    let mut out = Vec::with_capacity(num_uints);
    for w in 0..num_uints {
        let base = w * 4;
        let b0 = body.get(base).copied().unwrap_or(0);
        let b1 = body.get(base + 1).copied().unwrap_or(0);
        let b2 = body.get(base + 2).copied().unwrap_or(0);
        let b3 = body.get(base + 3).copied().unwrap_or(0);
        out.push(u32::from_le_bytes([b0, b1, b2, b3]));
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    // -- BitStuffer2 encode helpers (for building spec-layout fixtures) --

    /// Minimal `BitStuff` (v3+, LSB-first) encoder mirroring the reference so
    /// tests can round-trip against the decoder.
    fn bit_stuff_v3(values: &[u32], num_bits: i32) -> Vec<u8> {
        let num_elements = values.len();
        let num_uints = (num_elements * num_bits as usize).div_ceil(32);
        let mut words = vec![0u32; num_uints.max(1)];

        let mut bit_pos = 0i32;
        let mut idx = 0usize;
        for &val in values {
            if 32 - bit_pos >= num_bits {
                words[idx] |= val << bit_pos;
                bit_pos += num_bits;
                if bit_pos == 32 {
                    bit_pos = 0;
                    idx += 1;
                }
            } else {
                words[idx] |= val << bit_pos;
                idx += 1;
                words[idx] |= val >> (32 - bit_pos);
                bit_pos -= 32 - num_bits;
            }
        }

        let num_bytes = num_uints * 4;
        let num_bytes_used = num_bytes - num_tail_bytes_not_needed(num_elements, num_bits);
        let mut bytes = Vec::with_capacity(num_bytes_used);
        for &w in &words {
            bytes.extend_from_slice(&w.to_le_bytes());
        }
        bytes.truncate(num_bytes_used);
        bytes
    }

    /// Builds a full simple (non-LUT) `BitStuffer2` block for v3+.
    fn build_bitstuffer_simple(values: &[u32], num_bits: i32) -> Vec<u8> {
        // Element count fits in one byte for our small fixtures => nb code = 2.
        assert!(values.len() < 256);
        let bits67: u8 = 2; // 1-byte element count
        let head = (bits67 << 6) | (num_bits as u8 & 31);
        let mut out = vec![head, values.len() as u8];
        out.extend_from_slice(&bit_stuff_v3(values, num_bits));
        out
    }

    #[test]
    fn test_num_tail_bytes_not_needed_matches_reference() {
        // 4 elements * 3 bits = 12 bits => 2 bytes tail => 4-2 = 2 not needed.
        assert_eq!(num_tail_bytes_not_needed(4, 3), 2);
        // 8 elements * 4 bits = 32 bits => tail 0 => 0 not needed.
        assert_eq!(num_tail_bytes_not_needed(8, 4), 0);
    }

    #[test]
    fn test_bitstuffer_simple_roundtrip_v3() {
        let values: Vec<u32> = vec![0, 1, 2, 3, 4, 5, 6, 7, 5, 3, 1, 0];
        let num_bits = 3;
        let blob = build_bitstuffer_simple(&values, num_bits);

        let mut cur = Cursor::new(&blob);
        let mut out = Vec::new();
        bitstuffer_decode(&mut cur, &mut out, values.len(), 3).expect("decode");
        assert_eq!(out, values);
        assert_eq!(cur.remaining(), 0, "decoder must consume the whole block");
    }

    #[test]
    fn test_bitstuffer_widths_1_to_20() {
        for num_bits in 1..=20i32 {
            let maxv = if num_bits >= 32 {
                u32::MAX
            } else {
                (1u32 << num_bits) - 1
            };
            let values: Vec<u32> = (0..17u32)
                .map(|i| (i.wrapping_mul(2_654_435u32)) & maxv)
                .collect();
            let blob = build_bitstuffer_simple(&values, num_bits);
            let mut cur = Cursor::new(&blob);
            let mut out = Vec::new();
            bitstuffer_decode(&mut cur, &mut out, values.len(), 3)
                .unwrap_or_else(|e| panic!("decode nb={num_bits}: {e}"));
            assert_eq!(out, values, "mismatch at num_bits={num_bits}");
        }
    }

    #[test]
    fn test_fletcher32_known_vector() {
        // Recompute the LERC Fletcher-32 for "abcde" by an independent hand walk
        // (pairs (ab)(cd) then the straggler e) to lock the port.
        let v = fletcher32(b"abcde");
        let mut s1: u32 = 0xffff;
        let mut s2: u32 = 0xffff;
        let bytes = b"abcde";
        // pair ab
        s1 += (u32::from(bytes[0]) << 8) + u32::from(bytes[1]);
        s2 += s1;
        // pair cd
        s1 += (u32::from(bytes[2]) << 8) + u32::from(bytes[3]);
        s2 += s1;
        s1 = (s1 & 0xffff) + (s1 >> 16);
        s2 = (s2 & 0xffff) + (s2 >> 16);
        // straggler e
        s1 += u32::from(bytes[4]) << 8;
        s2 += s1;
        s1 = (s1 & 0xffff) + (s1 >> 16);
        s2 = (s2 & 0xffff) + (s2 >> 16);
        assert_eq!(v, (s2 << 16) | s1);
    }

    #[test]
    fn test_rle_decompress_literal_and_run() {
        // token1: count=+3 literal "ABC"; token2: count=-4 run of 0x7F; end.
        let mut src = Vec::new();
        src.extend_from_slice(&3i16.to_le_bytes());
        src.extend_from_slice(b"ABC");
        src.extend_from_slice(&(-4i16).to_le_bytes());
        src.push(0x7F);
        src.extend_from_slice(&(-32768i16).to_le_bytes());

        let mut out = vec![0u8; 7];
        rle_decompress(&src, &mut out).expect("rle");
        assert_eq!(&out, &[b'A', b'B', b'C', 0x7F, 0x7F, 0x7F, 0x7F]);
    }

    // -- pre-v3 (LERC2 v1/v2) bit-stuffer encoder (exact inverse of the decoder) --

    /// Encodes MSB-first (LERC2 v1/v2 layout) by building the reconstructed word
    /// array the decoder expects, then inverting its tail-word left shift and the
    /// little-endian byte load. This is the provable inverse of
    /// [`bit_unstuff_pre_v3`].
    fn bit_stuff_pre_v3(values: &[u32], num_bits: i32) -> Vec<u8> {
        let num_elements = values.len();
        let num_uints = (num_elements * num_bits as usize).div_ceil(32);
        let mut wfinal = vec![0u32; num_uints.max(1)];

        let mut bit = 0usize; // global bit index; 0 == MSB of word 0
        for &v in values {
            for b in (0..num_bits).rev() {
                if (v >> b) & 1 != 0 {
                    let word = bit / 32;
                    let within = bit % 32;
                    wfinal[word] |= 1u32 << (31 - within);
                }
                bit += 1;
            }
        }

        let ntbnn = num_tail_bytes_not_needed(num_elements, num_bits);
        if ntbnn > 0 {
            let last = num_uints - 1;
            wfinal[last] >>= (8 * ntbnn) as u32;
        }

        let n_bytes_to_copy = (num_elements * num_bits as usize).div_ceil(8);
        let mut bytes = Vec::new();
        for &w in &wfinal {
            bytes.extend_from_slice(&w.to_le_bytes());
        }
        bytes.truncate(n_bytes_to_copy);
        bytes
    }

    fn build_bitstuffer_simple_pre_v3(values: &[u32], num_bits: i32) -> Vec<u8> {
        assert!(values.len() < 256);
        let bits67: u8 = 2; // 1-byte element count
        let head = (bits67 << 6) | (num_bits as u8 & 31);
        let mut out = vec![head, values.len() as u8];
        out.extend_from_slice(&bit_stuff_pre_v3(values, num_bits));
        out
    }

    #[test]
    fn test_bitstuffer_simple_roundtrip_pre_v3() {
        for num_bits in 1..=17i32 {
            let maxv = (1u32 << num_bits) - 1;
            let values: Vec<u32> = (0..13u32)
                .map(|i| (i.wrapping_mul(97) ^ 0x5a) & maxv)
                .collect();
            let blob = build_bitstuffer_simple_pre_v3(&values, num_bits);
            let mut cur = Cursor::new(&blob);
            let mut out = Vec::new();
            // version 2 selects the pre-v3 unstuffer.
            bitstuffer_decode(&mut cur, &mut out, values.len(), 2)
                .unwrap_or_else(|e| panic!("pre-v3 decode nb={num_bits}: {e}"));
            assert_eq!(out, values, "pre-v3 mismatch at num_bits={num_bits}");
        }
    }

    // -- LUT variant --

    #[test]
    fn test_bitstuffer_lut_variant() {
        // values with repeats; distinct nonzero sorted LUT = [5, 7].
        let values: Vec<u32> = vec![5, 0, 7, 5, 7, 0];
        let lut = [5u32, 7u32]; // without the implicit 0
        let indices: Vec<u32> = vec![1, 0, 2, 1, 2, 0];
        let num_bits = 3; // holds max LUT value 7
        let n_lut = lut.len();
        let mut n_bits_lut = 0i32;
        while (n_lut >> n_bits_lut) != 0 {
            n_bits_lut += 1;
        }
        assert_eq!(n_bits_lut, 2);

        let bits67: u8 = 2; // 1-byte count
        let head = (bits67 << 6) | (1 << 5) | (num_bits as u8 & 31);
        let mut blob = vec![head, values.len() as u8, (n_lut as u8) + 1];
        blob.extend_from_slice(&bit_stuff_v3(&lut, num_bits));
        blob.extend_from_slice(&bit_stuff_v3(&indices, n_bits_lut));

        let mut cur = Cursor::new(&blob);
        let mut out = Vec::new();
        bitstuffer_decode(&mut cur, &mut out, values.len(), 3).expect("lut decode");
        assert_eq!(out, values);
    }

    // -- Full spec-layout LERC2 blob builders --

    /// Assembles a complete LERC2 v3 blob (single band), patching `blobSize` and
    /// the Fletcher-32 checksum. `after_header` is the mask section followed by
    /// the data section (the `readDataOneSweep` byte + tiles / one-sweep).
    #[allow(clippy::too_many_arguments)]
    fn assemble_v3(
        dt: Dt,
        n_cols: i32,
        n_rows: i32,
        num_valid: i32,
        mb: i32,
        max_z_error: f64,
        z_min: f64,
        z_max: f64,
        after_header: &[u8],
    ) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(FILE_KEY);
        b.extend_from_slice(&3i32.to_le_bytes()); // version
        let checksum_pos = b.len();
        b.extend_from_slice(&0u32.to_le_bytes()); // checksum placeholder
        b.extend_from_slice(&n_rows.to_le_bytes());
        b.extend_from_slice(&n_cols.to_le_bytes());
        b.extend_from_slice(&num_valid.to_le_bytes());
        b.extend_from_slice(&mb.to_le_bytes());
        let blobsize_pos = b.len();
        b.extend_from_slice(&0i32.to_le_bytes()); // blobSize placeholder
        b.extend_from_slice(&(dt as i32).to_le_bytes());
        b.extend_from_slice(&max_z_error.to_le_bytes());
        b.extend_from_slice(&z_min.to_le_bytes());
        b.extend_from_slice(&z_max.to_le_bytes());
        b.extend_from_slice(after_header);

        let blob_size = b.len() as i32;
        b[blobsize_pos..blobsize_pos + 4].copy_from_slice(&blob_size.to_le_bytes());
        let cs = fletcher32(&b[CHECKSUM_PREFIX_LEN..]);
        b[checksum_pos..checksum_pos + 4].copy_from_slice(&cs.to_le_bytes());
        b
    }

    #[test]
    fn test_decode_v3_float_bitstuffed_tile_exact() {
        // 2x2 Float image, one 8x8 micro-block => single tile, all valid.
        // maxZError = 0.5 => invScale = 1.0; offset = 10.0, quant = [0,2,4,6]
        // => values [10,12,14,16].
        let offset = 10.0f32;
        let quant: Vec<u32> = vec![0, 2, 4, 6];

        let mut tile = vec![1u8]; // comprFlag=1 (bit-stuffed), bits67=0, col check=0
        tile.extend_from_slice(&offset.to_le_bytes());
        tile.extend_from_slice(&build_bitstuffer_simple(&quant, 3));

        let mut after = Vec::new();
        after.extend_from_slice(&0i32.to_le_bytes()); // numBytesMask = 0 (all valid)
        after.push(0u8); // readDataOneSweep = 0 => tiled
        after.extend_from_slice(&tile);

        let blob = assemble_v3(Dt::Float, 2, 2, 4, 8, 0.5, 10.0, 16.0, &after);

        let dec = try_decode(&blob)
            .expect("recognised as real LERC2")
            .expect("decodes");
        assert_eq!((dec.n_cols, dec.n_rows, dec.n_bands), (2, 2, 1));
        assert_eq!(dec.data_type, LercDataType::Float);
        assert_eq!(dec.values, vec![10.0, 12.0, 14.0, 16.0]);
    }

    #[test]
    fn test_decode_v3_int_one_sweep_with_mask() {
        // 2x2 Int image, one pixel invalid. numValidPixel = 3.
        // One-sweep raw values for the 3 valid pixels: 100, 200, 300.
        // Mask: pixel (0,0) valid, (0,1) invalid, (1,0) valid, (1,1) valid.
        // Bit order MSB-first: k=0 valid, k=1 invalid, k=2 valid, k=3 valid
        // => first byte 0b1011_0000 = 0xB0.
        let mask_bitmap = [0xB0u8];
        // RLE: literal count=+1 then the mask byte, then sentinel.
        let mut rle = Vec::new();
        rle.extend_from_slice(&1i16.to_le_bytes());
        rle.push(mask_bitmap[0]);
        rle.extend_from_slice(&(-32768i16).to_le_bytes());

        let mut after = Vec::new();
        after.extend_from_slice(&(rle.len() as i32).to_le_bytes()); // numBytesMask
        after.extend_from_slice(&rle);
        after.push(1u8); // readDataOneSweep = 1 => raw values follow
        for v in [100i32, 200, 300] {
            after.extend_from_slice(&v.to_le_bytes());
        }

        // zMin != zMax so we don't hit the const-image short circuit.
        let blob = assemble_v3(Dt::Int, 2, 2, 3, 8, 0.0, 100.0, 300.0, &after);

        let dec = try_decode(&blob).expect("real LERC2").expect("decodes");
        assert_eq!(dec.data_type, LercDataType::Int);
        // Invalid pixel (index 1) stays 0.0; valid pixels carry their values.
        assert_eq!(dec.values, vec![100.0, 0.0, 200.0, 300.0]);
    }

    #[test]
    fn test_decode_v3_const_image() {
        // zMin == zMax => whole raster is the constant zMin for valid pixels.
        let mut after = Vec::new();
        after.extend_from_slice(&0i32.to_le_bytes()); // all-valid mask
        // No data section: FillConstImage returns before reading further.
        let blob = assemble_v3(Dt::Float, 3, 2, 6, 8, 0.0, 42.5, 42.5, &after);

        let dec = try_decode(&blob).expect("real LERC2").expect("decodes");
        assert_eq!(dec.values, vec![42.5; 6]);
    }

    /// Assembles a LERC2 v2 blob (no checksum), single band.
    #[allow(clippy::too_many_arguments)]
    fn assemble_v2(
        dt: Dt,
        n_cols: i32,
        n_rows: i32,
        num_valid: i32,
        mb: i32,
        max_z_error: f64,
        z_min: f64,
        z_max: f64,
        after_header: &[u8],
    ) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(FILE_KEY);
        b.extend_from_slice(&2i32.to_le_bytes()); // version (no checksum for v2)
        b.extend_from_slice(&n_rows.to_le_bytes());
        b.extend_from_slice(&n_cols.to_le_bytes());
        b.extend_from_slice(&num_valid.to_le_bytes());
        b.extend_from_slice(&mb.to_le_bytes());
        let blobsize_pos = b.len();
        b.extend_from_slice(&0i32.to_le_bytes()); // blobSize placeholder
        b.extend_from_slice(&(dt as i32).to_le_bytes());
        b.extend_from_slice(&max_z_error.to_le_bytes());
        b.extend_from_slice(&z_min.to_le_bytes());
        b.extend_from_slice(&z_max.to_le_bytes());
        b.extend_from_slice(after_header);
        let blob_size = b.len() as i32;
        b[blobsize_pos..blobsize_pos + 4].copy_from_slice(&blob_size.to_le_bytes());
        b
    }

    #[test]
    fn test_decode_v2_short_bitstuffed_pre_v3() {
        // 2x2 Short image, all valid, single tile, maxZError=0.5 (invScale=1).
        // offset=1000 stored as Short (tc=0), quant=[0,5,10,25] => vals
        // [1000,1005,1010,1025].
        let quant: Vec<u32> = vec![0, 5, 10, 25];
        let offset: i16 = 1000;

        let mut tile = vec![1u8]; // comprFlag=1, bits67=0, col check 0
        tile.extend_from_slice(&offset.to_le_bytes());
        // pre-v3 simple bit-stuffer (5 bits holds 25).
        tile.extend_from_slice(&build_bitstuffer_simple_pre_v3(&quant, 5));

        let mut after = Vec::new();
        after.extend_from_slice(&0i32.to_le_bytes()); // all-valid mask
        after.push(0u8); // tiled
        after.extend_from_slice(&tile);

        let blob = assemble_v2(Dt::Short, 2, 2, 4, 8, 0.5, 1000.0, 1025.0, &after);

        let dec = try_decode(&blob).expect("real LERC2 v2").expect("decodes");
        assert_eq!(dec.data_type, LercDataType::Short);
        assert_eq!(dec.values, vec![1000.0, 1005.0, 1010.0, 1025.0]);
    }

    #[test]
    fn test_bad_checksum_fails_loud() {
        let mut after = Vec::new();
        after.extend_from_slice(&0i32.to_le_bytes());
        let mut blob = assemble_v3(Dt::Float, 2, 2, 4, 8, 0.0, 1.0, 1.0, &after);
        // Corrupt the checksum.
        blob[10] ^= 0xFF;
        let res = try_decode(&blob).expect("recognised as real LERC2");
        assert!(res.is_err(), "corrupted checksum must fail loud");
    }

    #[test]
    fn test_crate_raw_format_not_treated_as_real_lerc2() {
        // The crate's raw format stores nDim=1 at byte 9, so the version field is
        // far out of range and try_decode must decline (return None).
        let mut data = Vec::new();
        data.extend_from_slice(FILE_KEY);
        data.extend_from_slice(&2u16.to_le_bytes()); // crate's u16 version
        data.push(LercDataType::Float.code()); // byte 8 = data type
        data.extend_from_slice(&1u32.to_le_bytes()); // nDim = 1 (byte 9 = 1)
        data.extend_from_slice(&2u32.to_le_bytes());
        data.extend_from_slice(&2u32.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&[0u8; 16]);
        assert!(try_decode(&data).is_none());
    }
}
