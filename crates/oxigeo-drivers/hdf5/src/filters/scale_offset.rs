//! HDF5 ScaleOffset filter (`H5Z_SCALEOFFSET`, filter id 6) — true on-disk codec.
//!
//! This module implements the **real** libhdf5 `H5Zscaleoffset.c` on-disk
//! layout, so chunks produced by h5py / netcdf-c decode correctly, and chunks
//! produced here are byte-compatible with libhdf5.
//!
//! ## Parameter contract (`cd_values`)
//!
//! The filter is driven by the dataset filter-pipeline `cd_values` array, which
//! libhdf5 fills in at write time (`H5Z_set_local_scaleoffset`). The layout is:
//!
//! | Index | Field        | Meaning                                             |
//! |-------|--------------|-----------------------------------------------------|
//! | 0     | scale_type   | 0 = `H5Z_SO_FLOAT_DSCALE`, 2 = `H5Z_SO_INT`         |
//! | 1     | scale_factor | float: decimal digits (D); integer: min-bits (0=auto)|
//! | 2     | nelmts       | number of elements in the chunk                     |
//! | 3     | class        | 0 = integer, 1 = floating-point                     |
//! | 4     | size         | datatype size in bytes                              |
//! | 5     | sign         | 0 = unsigned, 1 = signed (two's complement)         |
//! | 6     | order        | 0 = little-endian, 1 = big-endian                   |
//! | 7     | filavail     | 0 = fill undefined, 1 = fill value defined          |
//! | 8..   | filval       | fill value (native representation, little-endian)   |
//!
//! When only the "user" parameters `[scale_type, scale_factor]` are supplied
//! (the short form used internally by [`crate::filters::FilterPipeline`]), the
//! datatype class / size / sign are taken from the `datatype` argument, the byte
//! order defaults to little-endian, and no fill value is assumed. The number of
//! elements is then supplied out-of-band (from the chunk dimensions).
//!
//! ## Per-chunk on-disk layout
//!
//! Each compressed chunk begins with a fixed **21-byte header** followed by the
//! packed payload:
//!
//! | Offset | Size | Field   | Description                                       |
//! |--------|------|---------|---------------------------------------------------|
//! | 0      | 4    | minbits | bits per packed value (little-endian `u32`)       |
//! | 4      | 1    | 0x08    | constant (`sizeof(unsigned long long)`)           |
//! | 5      | 8    | minval  | minimum value (integer) or IEEE bits of min float |
//! | 13     | 8    | (zero)  | reserved, always zero                             |
//! | 21     | var  | payload | MSB-first bit-packed codes, or raw passthrough    |
//!
//! * If `minbits >= size*8` the payload is the raw little-endian element values
//!   (uncompressed passthrough) and `minval` is zero.
//! * Otherwise each element is stored as a `minbits`-wide MSB-first code. The
//!   value is reconstructed as `minval + code` (integer) or
//!   `min_float + code / 10^D` (float).
//! * When a fill value is defined, the all-ones code `(1<<minbits)-1` is reserved
//!   to denote fill-valued elements; the encoder guarantees real codes never
//!   reach it (`minbits = bit_length(span+1)`).

use crate::datatype::Datatype;
use crate::error::{Hdf5Error, Result};
use byteorder::{BigEndian, ByteOrder, LittleEndian};

use super::bitpack::{BitReader, BitWriter, min_bits_for_value};

/// Fixed on-disk header size in bytes.
const HEADER_SIZE: usize = 21;
/// Offset of the `minval` field inside the header.
const MINVAL_OFFSET: usize = 5;
/// Width of the `minval` field in bytes (`sizeof(unsigned long long)`).
const MINVAL_WIDTH: usize = 8;

/// Scale type: floating-point, D-scaling (`H5Z_SO_FLOAT_DSCALE`).
pub const SO_FLOAT_DSCALE: u32 = 0;
/// Scale type: floating-point, E-scaling (`H5Z_SO_FLOAT_ESCALE`, not implemented
/// by libhdf5 itself).
pub const SO_FLOAT_ESCALE: u32 = 1;
/// Scale type: integer, automatic minimum bits (`H5Z_SO_INT`).
pub const SO_INT: u32 = 2;

// `cd_values` indices (see the module documentation table).
const PARM_SCALETYPE: usize = 0;
const PARM_SCALEFACTOR: usize = 1;
const PARM_NELMTS: usize = 2;
const PARM_ORDER: usize = 6;
const PARM_FILAVAIL: usize = 7;
const PARM_FILVAL: usize = 8;

/// Fill value existence flag: fill value is defined.
const FILL_DEFINED: u32 = 1;
/// Byte order flag: big-endian.
const ORDER_BE: u32 = 1;

/// Resolved parameters describing a single chunk decode/encode.
struct Params {
    scale_factor: i32,
    size: usize,
    is_float: bool,
    order_be: bool,
    filavail: bool,
    filval: u64,
    d_nelmts: usize,
}

impl Params {
    /// Resolve parameters from the `cd_values` array, the element datatype and a
    /// caller-supplied element-count hint (from the chunk dimensions).
    fn resolve(cd_values: &[u32], datatype: &Datatype, d_nelmts_hint: usize) -> Result<Self> {
        let size = datatype.size();
        let is_float = datatype.is_float();
        if !is_float && !datatype.is_integer() {
            return Err(Hdf5Error::UnsupportedDatatype(format!(
                "ScaleOffset: unsupported datatype {datatype:?} (integer or float only)"
            )));
        }

        let scale_type = cd_values
            .get(PARM_SCALETYPE)
            .copied()
            .unwrap_or(if is_float { SO_FLOAT_DSCALE } else { SO_INT });
        let scale_factor = cd_values.get(PARM_SCALEFACTOR).copied().unwrap_or(0) as i32;
        let order_be = cd_values.get(PARM_ORDER).copied() == Some(ORDER_BE);
        let filavail = cd_values.get(PARM_FILAVAIL).copied() == Some(FILL_DEFINED);

        // Reconstruct the fill value from the little-endian `u32` words.
        let mut filval: u64 = 0;
        for i in 0..MINVAL_WIDTH.div_ceil(4) {
            if let Some(&word) = cd_values.get(PARM_FILVAL + i) {
                filval |= (word as u64) << (32 * i);
            }
        }

        // The real filter-pipeline form carries the element count in cd_values;
        // it is authoritative. The short form relies on the chunk-dimension hint.
        let d_nelmts = match cd_values.get(PARM_NELMTS).copied() {
            Some(n) if cd_values.len() > PARM_NELMTS && n > 0 => n as usize,
            _ => d_nelmts_hint,
        };

        if is_float && scale_type == SO_FLOAT_ESCALE {
            return Err(Hdf5Error::UnsupportedCompressionFilter(
                "ScaleOffset: E-scaling (H5Z_SO_FLOAT_ESCALE) is not supported".to_string(),
            ));
        }

        Ok(Self {
            scale_factor,
            size,
            is_float,
            order_be,
            filavail,
            filval,
            d_nelmts,
        })
    }
}

/// Decode a ScaleOffset chunk (reverse / decompression direction).
///
/// * `data` — the compressed chunk (21-byte header + payload).
/// * `cd_values` — filter parameters (see the module documentation).
/// * `datatype` — element datatype.
/// * `d_nelmts` — number of elements in the chunk (from the chunk dimensions);
///   used when `cd_values` does not carry the count.
pub fn apply_scale_offset_reverse(
    data: &[u8],
    cd_values: &[u32],
    datatype: &Datatype,
    d_nelmts: usize,
) -> Result<Vec<u8>> {
    let params = Params::resolve(cd_values, datatype, d_nelmts)?;

    if data.len() < HEADER_SIZE {
        return Err(Hdf5Error::Decompression(format!(
            "ScaleOffset: chunk of {} bytes is smaller than the {}-byte header",
            data.len(),
            HEADER_SIZE
        )));
    }

    let minbits = LittleEndian::read_u32(&data[0..4]);
    let minval_bytes = &data[MINVAL_OFFSET..MINVAL_OFFSET + MINVAL_WIDTH];
    let minval = if params.order_be {
        BigEndian::read_u64(minval_bytes)
    } else {
        LittleEndian::read_u64(minval_bytes)
    };

    let size = params.size;
    let full_bits = (size as u32) * 8;
    let n = params.d_nelmts;
    let mut output = vec![0u8; n * size];

    // Defensive: minbits == 0 means every element equals the minimum. libhdf5
    // never emits this (it uses bit_length(span+1) >= 1) but we handle it rather
    // than misread the payload.
    if minbits == 0 {
        for i in 0..n {
            write_int_element(&mut output[i * size..(i + 1) * size], minval, size);
        }
        return Ok(output);
    }

    // Uncompressed passthrough: raw little-endian (or big-endian) element values.
    if minbits >= full_bits {
        let payload = &data[HEADER_SIZE..];
        if payload.len() < n * size {
            return Err(Hdf5Error::Decompression(format!(
                "ScaleOffset: passthrough payload {} bytes, need {}",
                payload.len(),
                n * size
            )));
        }
        for i in 0..n {
            let chunk = &payload[i * size..(i + 1) * size];
            let raw = if params.order_be {
                read_uint_be(chunk, size)
            } else {
                read_uint_le(chunk, size)
            };
            let value = minval.wrapping_add(raw);
            write_int_element(&mut output[i * size..(i + 1) * size], value, size);
        }
        return Ok(output);
    }

    // Bit-packed codes.
    let minbits_u8 = u8::try_from(minbits).map_err(|_| {
        Hdf5Error::Decompression(format!("ScaleOffset: minbits {minbits} exceeds 255"))
    })?;
    let all_ones: u64 = if minbits >= 64 {
        u64::MAX
    } else {
        (1u64 << minbits) - 1
    };

    let mut reader = BitReader::new(&data[HEADER_SIZE..]);
    for i in 0..n {
        let code = reader.read_bits(minbits_u8)?;
        let dst = &mut output[i * size..(i + 1) * size];

        if params.filavail && code == all_ones {
            write_int_element(dst, params.filval, size);
            continue;
        }

        if params.is_float {
            write_float_element(dst, minval, code, params.scale_factor, size)?;
        } else {
            write_int_element(dst, minval.wrapping_add(code), size);
        }
    }

    Ok(output)
}

/// Encode raw element bytes with the ScaleOffset filter (forward / compression).
///
/// Produces the exact libhdf5 on-disk layout (21-byte header + payload), so the
/// result round-trips through [`apply_scale_offset_reverse`] and is readable by
/// libhdf5 / h5py. The internal short form does not reserve a fill value.
pub fn apply_scale_offset_forward(
    data: &[u8],
    cd_values: &[u32],
    datatype: &Datatype,
) -> Result<Vec<u8>> {
    let size = datatype.size();
    if size == 0 {
        return Err(Hdf5Error::Compression(
            "ScaleOffset: zero-sized datatype".to_string(),
        ));
    }
    if data.is_empty() || !data.len().is_multiple_of(size) {
        return Err(Hdf5Error::Compression(format!(
            "ScaleOffset: data length {} is not a positive multiple of element size {}",
            data.len(),
            size
        )));
    }
    let d_nelmts = data.len() / size;
    let params = Params::resolve(cd_values, datatype, d_nelmts)?;

    // Compute the offset codes and the minimum value.
    let (codes, minval) = if params.is_float {
        encode_float_codes(data, size, params.scale_factor)?
    } else {
        encode_int_codes(data, size, datatype)?
    };

    let span = codes.iter().copied().max().unwrap_or(0);
    let mut minbits = min_bits_for_value(span.saturating_add(1)) as u32;
    let full_bits = (size as u32) * 8;

    // Passthrough when the codes need the full precision (no compression gain).
    if minbits >= full_bits {
        let mut out = Vec::with_capacity(HEADER_SIZE + d_nelmts * size);
        write_header(&mut out, full_bits, 0);
        out.extend_from_slice(data);
        return Ok(out);
    }

    if minbits == 0 {
        minbits = 1;
    }
    let minbits_u8 = u8::try_from(minbits).map_err(|_| {
        Hdf5Error::Compression(format!("ScaleOffset: minbits {minbits} exceeds 255"))
    })?;

    let total_bits = (d_nelmts as u64) * (minbits as u64);
    let mut out = Vec::with_capacity(HEADER_SIZE + (total_bits / 8) as usize + 1);
    write_header(&mut out, minbits, minval);

    let mut writer = BitWriter::with_capacity((total_bits / 8) as usize + 1);
    for &code in &codes {
        writer.write_bits(code, minbits_u8);
    }
    let mut payload = writer.finish();
    // libhdf5 always reserves a trailing partial byte; append one when the packed
    // bits fill whole bytes exactly so the layout matches byte-for-byte.
    if total_bits.is_multiple_of(8) {
        payload.push(0);
    }
    out.extend_from_slice(&payload);
    Ok(out)
}

/// Compute integer offset codes (`value - min`) and the raw `minval` word.
fn encode_int_codes(data: &[u8], size: usize, datatype: &Datatype) -> Result<(Vec<u64>, u64)> {
    let n = data.len() / size;
    let signed = matches!(
        datatype,
        Datatype::Int8 | Datatype::Int16 | Datatype::Int32 | Datatype::Int64
    );

    if signed {
        let mut values = Vec::with_capacity(n);
        for i in 0..n {
            values.push(read_int_le(&data[i * size..(i + 1) * size], size));
        }
        let min = values.iter().copied().min().unwrap_or(0);
        let codes = values
            .iter()
            .map(|&v| (v.wrapping_sub(min)) as u64)
            .collect();
        Ok((codes, min as u64))
    } else {
        let mut values = Vec::with_capacity(n);
        for i in 0..n {
            values.push(read_uint_le(&data[i * size..(i + 1) * size], size));
        }
        let min = values.iter().copied().min().unwrap_or(0);
        let codes = values.iter().map(|&v| v - min).collect();
        Ok((codes, min))
    }
}

/// Compute floating-point D-scaled offset codes and the IEEE bits of the minimum.
fn encode_float_codes(data: &[u8], size: usize, scale_factor: i32) -> Result<(Vec<u64>, u64)> {
    let n = data.len() / size;
    let mult = 10f64.powi(scale_factor);
    let mut values = Vec::with_capacity(n);
    for i in 0..n {
        let chunk = &data[i * size..(i + 1) * size];
        let v = match size {
            4 => LittleEndian::read_f32(chunk) as f64,
            8 => LittleEndian::read_f64(chunk),
            _ => {
                return Err(Hdf5Error::Compression(format!(
                    "ScaleOffset: unsupported float element size {size}"
                )));
            }
        };
        values.push(v);
    }

    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let min = if min.is_finite() { min } else { 0.0 };

    let codes = values
        .iter()
        .map(|&v| {
            let d = ((v - min) * mult).round();
            if d < 0.0 { 0 } else { d as u64 }
        })
        .collect();

    let minval = match size {
        4 => (min as f32).to_bits() as u64,
        _ => min.to_bits(),
    };
    Ok((codes, minval))
}

/// Write the 21-byte header (`minbits`, constant `0x08`, `minval`, zero pad).
fn write_header(out: &mut Vec<u8>, minbits: u32, minval: u64) {
    let mut header = [0u8; HEADER_SIZE];
    LittleEndian::write_u32(&mut header[0..4], minbits);
    header[4] = MINVAL_WIDTH as u8;
    LittleEndian::write_u64(
        &mut header[MINVAL_OFFSET..MINVAL_OFFSET + MINVAL_WIDTH],
        minval,
    );
    out.extend_from_slice(&header);
}

/// Reconstruct and write a floating-point element from `min_float + code / 10^D`.
fn write_float_element(
    dst: &mut [u8],
    minval_bits: u64,
    code: u64,
    scale_factor: i32,
    size: usize,
) -> Result<()> {
    let divisor = 10f64.powi(scale_factor);
    match size {
        4 => {
            let min_f = f32::from_bits(minval_bits as u32) as f64;
            let value = (min_f + (code as f64) / divisor) as f32;
            LittleEndian::write_f32(dst, value);
            Ok(())
        }
        8 => {
            let min_f = f64::from_bits(minval_bits);
            let value = min_f + (code as f64) / divisor;
            LittleEndian::write_f64(dst, value);
            Ok(())
        }
        _ => Err(Hdf5Error::Decompression(format!(
            "ScaleOffset: unsupported float element size {size}"
        ))),
    }
}

/// Write the low `size` bytes of `value` in little-endian order.
fn write_int_element(dst: &mut [u8], value: u64, size: usize) {
    let bytes = value.to_le_bytes();
    let n = size.min(bytes.len()).min(dst.len());
    dst[..n].copy_from_slice(&bytes[..n]);
}

/// Read a little-endian unsigned integer of `size` bytes (1..=8).
fn read_uint_le(chunk: &[u8], size: usize) -> u64 {
    let mut buf = [0u8; 8];
    let n = size.min(chunk.len()).min(8);
    buf[..n].copy_from_slice(&chunk[..n]);
    u64::from_le_bytes(buf)
}

/// Read a big-endian unsigned integer of `size` bytes (1..=8).
fn read_uint_be(chunk: &[u8], size: usize) -> u64 {
    let mut buf = [0u8; 8];
    let n = size.min(chunk.len()).min(8);
    // Right-align the big-endian bytes.
    buf[8 - n..].copy_from_slice(&chunk[..n]);
    u64::from_be_bytes(buf)
}

/// Read a little-endian signed integer of `size` bytes, sign-extended to `i64`.
fn read_int_le(chunk: &[u8], size: usize) -> i64 {
    let raw = read_uint_le(chunk, size);
    let bits = (size * 8) as u32;
    if bits >= 64 {
        raw as i64
    } else if (raw >> (bits - 1)) & 1 == 1 {
        (raw | (!0u64 << bits)) as i64
    } else {
        raw as i64
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn make_i32(values: &[i32]) -> Vec<u8> {
        let mut data = vec![0u8; values.len() * 4];
        for (i, &v) in values.iter().enumerate() {
            LittleEndian::write_i32(&mut data[i * 4..(i + 1) * 4], v);
        }
        data
    }
    fn read_i32(data: &[u8]) -> Vec<i32> {
        data.chunks_exact(4).map(LittleEndian::read_i32).collect()
    }
    fn make_u16(values: &[u16]) -> Vec<u8> {
        let mut data = vec![0u8; values.len() * 2];
        for (i, &v) in values.iter().enumerate() {
            LittleEndian::write_u16(&mut data[i * 2..(i + 1) * 2], v);
        }
        data
    }
    fn read_u16(data: &[u8]) -> Vec<u16> {
        data.chunks_exact(2).map(LittleEndian::read_u16).collect()
    }
    fn read_f32(data: &[u8]) -> Vec<f32> {
        data.chunks_exact(4).map(LittleEndian::read_f32).collect()
    }

    // Full 20-element cd_values as libhdf5 writes them.
    #[allow(clippy::too_many_arguments)]
    fn so_cd(
        scale_type: u32,
        scale_factor: i32,
        nelmts: u32,
        class: u32,
        size: u32,
        sign: u32,
        filavail: u32,
        filval: u32,
    ) -> Vec<u32> {
        let mut cd = vec![0u32; 20];
        cd[0] = scale_type;
        cd[1] = scale_factor as u32;
        cd[2] = nelmts;
        cd[3] = class;
        cd[4] = size;
        cd[5] = sign;
        cd[6] = 0; // order LE
        cd[7] = filavail;
        cd[8] = filval;
        cd
    }

    // ------------------------------------------------------------------
    // Interop: decode real libhdf5 (h5py 3.16.0 / hdf5 2.0.0) chunk bytes.
    //
    // These hex byte strings were extracted with h5py's `read_direct_chunk`
    // from files written by libhdf5 (not by this crate). They validate that
    // the on-disk layout decodes bit-for-bit against a real HDF5 producer.
    // ------------------------------------------------------------------

    #[test]
    fn interop_hdf5_i32_no_fill_present() {
        // int32 [100,105,110,103,108,115,100,120], scaleoffset=0, default fill (0, unused)
        let raw = hex("050000000864000000000000000000000000000000", "0154343c1400");
        let cd = so_cd(SO_INT, 0, 8, 0, 4, 1, FILL_DEFINED, 0);
        let out = apply_scale_offset_reverse(&raw, &cd, &Datatype::Int32, 8).unwrap();
        assert_eq!(read_i32(&out), vec![100, 105, 110, 103, 108, 115, 100, 120]);
    }

    #[test]
    fn interop_hdf5_i32_with_fill_valued_element() {
        // int32 with fillvalue=999 and one fill-valued element -> all-ones code.
        let raw = hex("050000000864000000000000000000000000000000", "0154343c1f00");
        let cd = so_cd(SO_INT, 0, 8, 0, 4, 1, FILL_DEFINED, 999);
        let out = apply_scale_offset_reverse(&raw, &cd, &Datatype::Int32, 8).unwrap();
        assert_eq!(read_i32(&out), vec![100, 105, 110, 103, 108, 115, 100, 999]);
    }

    #[test]
    fn interop_hdf5_i32_full_precision_passthrough() {
        // int32 spanning the full range -> minbits==32 passthrough, minval==0.
        let raw = hex(
            "200000000800000000000000000000000000000000",
            "00000080ffffff7f0000000005000000",
        );
        let cd = so_cd(SO_INT, 0, 4, 0, 4, 1, FILL_DEFINED, 0);
        let out = apply_scale_offset_reverse(&raw, &cd, &Datatype::Int32, 4).unwrap();
        assert_eq!(read_i32(&out), vec![i32::MIN, i32::MAX, 0, 5]);
    }

    #[test]
    fn interop_hdf5_u8_full_precision_passthrough() {
        let raw = hex(
            "080000000800000000000000000000000000000000",
            "00ff8001fe07c809",
        );
        let cd = so_cd(SO_INT, 0, 8, 0, 1, 0, FILL_DEFINED, 0);
        let out = apply_scale_offset_reverse(&raw, &cd, &Datatype::UInt8, 8).unwrap();
        assert_eq!(out, vec![0, 255, 128, 1, 254, 7, 200, 9]);
    }

    #[test]
    fn interop_hdf5_i32_2d_chunk_nelmts_from_cd() {
        // 2x3 chunk -> nelmts=6 (taken from cd_values, hint ignored).
        let raw = hex("030000000801000000000000000000000000000000", "053940");
        let cd = so_cd(SO_INT, 0, 6, 0, 4, 1, FILL_DEFINED, 0);
        let out = apply_scale_offset_reverse(&raw, &cd, &Datatype::Int32, 0).unwrap();
        assert_eq!(read_i32(&out), vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn interop_hdf5_f32_dscale() {
        // float32 D=1, min=20.1; decoded values must match libhdf5 within f32 ULP.
        let raw = hex("0400000008cdcca041000000000000000000000000", "4562708300");
        let cd = so_cd(SO_FLOAT_DSCALE, 1, 8, 1, 4, 0, FILL_DEFINED, 0);
        let out = apply_scale_offset_reverse(&raw, &cd, &Datatype::Float32, 8).unwrap();
        let got = read_f32(&out);
        let expected = [20.5f32, 20.6, 20.7, 20.3, 20.8, 20.1, 20.9, 20.4];
        for (g, e) in got.iter().zip(expected.iter()) {
            assert!((g - e).abs() < 1e-4, "got {g}, expected {e}");
        }
    }

    // ------------------------------------------------------------------
    // Hand-built spec-layout bytes (exact assertions).
    // ------------------------------------------------------------------

    #[test]
    fn handbuilt_minbits_and_packing_exact() {
        // Build a chunk by hand: minbits=3, minval=10, codes [0,1,2,3,4,5,6,7].
        let mut raw = vec![0u8; HEADER_SIZE];
        LittleEndian::write_u32(&mut raw[0..4], 3);
        raw[4] = 8;
        LittleEndian::write_u64(&mut raw[5..13], 10);
        // codes 000 001 010 011 100 101 110 111 -> pack MSB-first.
        let mut w = BitWriter::new();
        for c in 0u64..8 {
            w.write_bits(c, 3);
        }
        raw.extend_from_slice(&w.finish());
        let cd = so_cd(SO_INT, 0, 8, 0, 4, 1, 0, 0);
        let out = apply_scale_offset_reverse(&raw, &cd, &Datatype::Int32, 8).unwrap();
        assert_eq!(read_i32(&out), vec![10, 11, 12, 13, 14, 15, 16, 17]);
    }

    #[test]
    fn handbuilt_fill_code_maps_to_fill_value() {
        // minbits=3 reserves code 7 as fill; fill value = -1.
        let mut raw = vec![0u8; HEADER_SIZE];
        LittleEndian::write_u32(&mut raw[0..4], 3);
        raw[4] = 8;
        LittleEndian::write_u64(&mut raw[5..13], 100);
        let mut w = BitWriter::new();
        for c in [0u64, 1, 7, 2] {
            w.write_bits(c, 3);
        }
        raw.extend_from_slice(&w.finish());
        // fill value -1 stored as its two's-complement u32 word.
        let cd = so_cd(SO_INT, 0, 4, 0, 4, 1, FILL_DEFINED, (-1i32) as u32);
        let out = apply_scale_offset_reverse(&raw, &cd, &Datatype::Int32, 4).unwrap();
        assert_eq!(read_i32(&out), vec![100, 101, -1, 102]);
    }

    // ------------------------------------------------------------------
    // Self round-trip (encode -> decode) across types and edge cases.
    // ------------------------------------------------------------------

    #[test]
    fn roundtrip_i32() {
        let values = vec![100i32, 105, 110, 103, 108, 115, 100, 120];
        let data = make_i32(&values);
        let cd = [SO_INT, 0];
        let comp = apply_scale_offset_forward(&data, &cd, &Datatype::Int32).unwrap();
        assert!(comp.len() < data.len());
        let out = apply_scale_offset_reverse(&comp, &cd, &Datatype::Int32, values.len()).unwrap();
        assert_eq!(read_i32(&out), values);
    }

    #[test]
    fn roundtrip_forward_matches_hdf5_bytes() {
        // Our encoder must reproduce libhdf5's exact chunk for this input.
        let data = make_i32(&[100, 105, 110, 103, 108, 115, 100, 120]);
        let comp = apply_scale_offset_forward(&data, &[SO_INT, 0], &Datatype::Int32).unwrap();
        let expected = hex("050000000864000000000000000000000000000000", "0154343c1400");
        assert_eq!(comp, expected);
    }

    #[test]
    fn roundtrip_i32_negative() {
        let values = vec![-50i32, -45, -40, -55, -30, -60, -35, -42];
        let data = make_i32(&values);
        let cd = [SO_INT, 0];
        let comp = apply_scale_offset_forward(&data, &cd, &Datatype::Int32).unwrap();
        // Byte-exact against libhdf5.
        let expected = hex("0500000008c4ffffffffffffff0000000000000000", "53e85f033200");
        assert_eq!(comp, expected);
        let out = apply_scale_offset_reverse(&comp, &cd, &Datatype::Int32, values.len()).unwrap();
        assert_eq!(read_i32(&out), values);
    }

    #[test]
    fn roundtrip_u16() {
        let values = vec![1000u16, 1001, 1002, 1003, 1004, 1005, 1006, 1007];
        let data = make_u16(&values);
        let cd = [SO_INT, 0];
        let comp = apply_scale_offset_forward(&data, &cd, &Datatype::UInt16).unwrap();
        let out = apply_scale_offset_reverse(&comp, &cd, &Datatype::UInt16, values.len()).unwrap();
        assert_eq!(read_u16(&out), values);
    }

    #[test]
    fn roundtrip_constant() {
        let values = vec![42i32; 100];
        let data = make_i32(&values);
        let cd = [SO_INT, 0];
        let comp = apply_scale_offset_forward(&data, &cd, &Datatype::Int32).unwrap();
        assert!(comp.len() < 40);
        let out = apply_scale_offset_reverse(&comp, &cd, &Datatype::Int32, values.len()).unwrap();
        assert_eq!(read_i32(&out), values);
    }

    #[test]
    fn roundtrip_single_element() {
        let data = make_i32(&[42]);
        let cd = [SO_INT, 0];
        let comp = apply_scale_offset_forward(&data, &cd, &Datatype::Int32).unwrap();
        let out = apply_scale_offset_reverse(&comp, &cd, &Datatype::Int32, 1).unwrap();
        assert_eq!(read_i32(&out), vec![42]);
    }

    #[test]
    fn roundtrip_i8() {
        let byte_values: Vec<i8> = vec![-10, -5, 0, 5, 10, 15, 20, -3];
        let data: Vec<u8> = byte_values.iter().map(|&v| v as u8).collect();
        let cd = [SO_INT, 0];
        let comp = apply_scale_offset_forward(&data, &cd, &Datatype::Int8).unwrap();
        let out =
            apply_scale_offset_reverse(&comp, &cd, &Datatype::Int8, byte_values.len()).unwrap();
        let got: Vec<i8> = out.iter().map(|&b| b as i8).collect();
        assert_eq!(got, byte_values);
    }

    #[test]
    fn roundtrip_f32() {
        let values = [20.5f32, 20.6, 20.7, 20.3, 20.8, 20.1, 20.9, 20.4];
        let mut data = vec![0u8; values.len() * 4];
        for (i, &v) in values.iter().enumerate() {
            LittleEndian::write_f32(&mut data[i * 4..(i + 1) * 4], v);
        }
        let cd = [SO_FLOAT_DSCALE, 1];
        let comp = apply_scale_offset_forward(&data, &cd, &Datatype::Float32).unwrap();
        // Byte-exact against libhdf5.
        let expected = hex("0400000008cdcca041000000000000000000000000", "4562708300");
        assert_eq!(comp, expected);
        let out = apply_scale_offset_reverse(&comp, &cd, &Datatype::Float32, values.len()).unwrap();
        for (g, e) in read_f32(&out).iter().zip(values.iter()) {
            assert!((g - e).abs() < 0.1);
        }
    }

    #[test]
    fn roundtrip_i32_full_range_passthrough() {
        let values = vec![i32::MIN, i32::MAX, 0, 5];
        let data = make_i32(&values);
        let cd = [SO_INT, 0];
        let comp = apply_scale_offset_forward(&data, &cd, &Datatype::Int32).unwrap();
        // Passthrough: 21-byte header + 4*4 raw bytes.
        assert_eq!(comp.len(), HEADER_SIZE + 16);
        let out = apply_scale_offset_reverse(&comp, &cd, &Datatype::Int32, values.len()).unwrap();
        assert_eq!(read_i32(&out), values);
    }

    #[test]
    fn reverse_rejects_short_chunk() {
        let data = vec![0u8; 10];
        let res = apply_scale_offset_reverse(&data, &[SO_INT, 0], &Datatype::Int32, 1);
        assert!(res.is_err());
    }

    #[test]
    fn forward_rejects_empty() {
        let res = apply_scale_offset_forward(&[], &[SO_INT, 0], &Datatype::Int32);
        assert!(res.is_err());
    }

    fn hex(a: &str, b: &str) -> Vec<u8> {
        let mut s = String::from(a);
        s.push_str(b);
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("valid hex"))
            .collect()
    }
}
