//! HDF5 N-Bit filter (`H5Z_NBIT`, filter id 5) — true on-disk codec.
//!
//! This module implements the **real** libhdf5 `H5Znbit.c` on-disk layout, so
//! chunks produced by h5py / netcdf-c decode correctly, and chunks produced here
//! are byte-compatible with libhdf5.
//!
//! ## Parameter contract (`cd_values`)
//!
//! The filter is driven by the dataset filter-pipeline `cd_values` array, which
//! libhdf5 fills in at write time (`H5Z_set_local_nbit`). It begins with a small
//! header followed by a recursive datatype descriptor:
//!
//! | Index | Field   | Meaning                                                 |
//! |-------|---------|---------------------------------------------------------|
//! | 0     | nparms  | total number of `cd_values` entries                     |
//! | 1     | flags   | top-level bookkeeping flag (0 integer / 1 float)        |
//! | 2     | nelmts  | number of elements in the chunk                         |
//! | 3..   | descr   | recursive datatype descriptor (see below)               |
//!
//! An **atomic** descriptor (class marker `1`) is five entries:
//! `[1(atomic), size, order, precision, offset]`, where `order` is 0 (LE) / 1
//! (BE), `precision` is the number of significant bits, and `offset` is the bit
//! offset of those significant bits within the element.
//!
//! Other class markers are `2` (array), `3` (compound) and `4` (no-op — the type
//! is passed through uncompressed). This crate decodes atomic integer and float
//! members; array and compound member recursion is reported as a typed
//! [`Hdf5Error::UnsupportedDatatype`] rather than producing garbage.
//!
//! When `cd_values` is a short crate-internal form (`[]`, `[precision]` or
//! `[precision, offset]`) the datatype size is taken from the `datatype`
//! argument, the byte order defaults to little-endian, the offset defaults to 0,
//! and the number of elements is supplied out-of-band (from the chunk
//! dimensions).
//!
//! ## Per-chunk on-disk layout
//!
//! There is **no per-chunk header**. Each element contributes its `precision`
//! significant bits, packed MSB-first with no padding between elements; the final
//! byte is zero-padded. When the type is already full precision at offset 0 the
//! filter is a no-op and the chunk holds the raw element bytes.

use crate::datatype::Datatype;
use crate::error::{Hdf5Error, Result};

use super::bitpack::{BitReader, BitWriter};

/// Datatype class marker: atomic (integer / floating-point).
const NBIT_ATOMIC: u32 = 1;
/// Datatype class marker: array.
const NBIT_ARRAY: u32 = 2;
/// Datatype class marker: compound.
const NBIT_COMPOUND: u32 = 3;
/// Datatype class marker: no-op (type stored uncompressed).
const NBIT_NOOPTYPE: u32 = 4;

/// Byte order marker: big-endian.
const ORDER_BE: u32 = 1;

// Fixed `cd_values` indices (real filter-pipeline form).
const PARM_NELMTS: usize = 2;
const PARM_DESCR: usize = 3;

/// Resolved parameters for a single atomic N-Bit chunk.
struct AtomicParams {
    marker: u32,
    size: usize,
    order_be: bool,
    precision: u32,
    offset: u32,
    d_nelmts: usize,
}

impl AtomicParams {
    /// Resolve N-Bit parameters from `cd_values`, the datatype and an element
    /// count hint (from the chunk dimensions).
    fn resolve(cd_values: &[u32], datatype: &Datatype, d_nelmts_hint: usize) -> Result<Self> {
        let dt_size = datatype.size();

        // Detect the real filter-pipeline form: a full header plus a valid class
        // marker at the descriptor position.
        let is_real = cd_values.len() > PARM_DESCR
            && matches!(
                cd_values[PARM_DESCR],
                NBIT_ATOMIC | NBIT_ARRAY | NBIT_COMPOUND | NBIT_NOOPTYPE
            );

        if is_real {
            let marker = cd_values[PARM_DESCR];
            if marker != NBIT_ATOMIC {
                // Array / compound / no-op: only the marker is meaningful here.
                let d_nelmts = cd_values
                    .get(PARM_NELMTS)
                    .copied()
                    .filter(|&n| n > 0)
                    .map(|n| n as usize)
                    .unwrap_or(d_nelmts_hint);
                return Ok(Self {
                    marker,
                    size: dt_size,
                    order_be: false,
                    precision: 0,
                    offset: 0,
                    d_nelmts,
                });
            }
            let size = *cd_values.get(PARM_DESCR + 1).ok_or_else(|| {
                Hdf5Error::Decompression("N-Bit: truncated atomic descriptor (size)".to_string())
            })? as usize;
            let order = *cd_values.get(PARM_DESCR + 2).ok_or_else(|| {
                Hdf5Error::Decompression("N-Bit: truncated atomic descriptor (order)".to_string())
            })?;
            let precision = *cd_values.get(PARM_DESCR + 3).ok_or_else(|| {
                Hdf5Error::Decompression(
                    "N-Bit: truncated atomic descriptor (precision)".to_string(),
                )
            })?;
            let offset = *cd_values.get(PARM_DESCR + 4).ok_or_else(|| {
                Hdf5Error::Decompression("N-Bit: truncated atomic descriptor (offset)".to_string())
            })?;
            let d_nelmts = cd_values
                .get(PARM_NELMTS)
                .copied()
                .filter(|&n| n > 0)
                .map(|n| n as usize)
                .unwrap_or(d_nelmts_hint);
            Ok(Self {
                marker: NBIT_ATOMIC,
                size,
                order_be: order == ORDER_BE,
                precision,
                offset,
                d_nelmts,
            })
        } else {
            // Crate-internal short form: [] | [precision] | [precision, offset].
            let precision = cd_values.first().copied().unwrap_or((dt_size * 8) as u32);
            let offset = cd_values.get(1).copied().unwrap_or(0);
            Ok(Self {
                marker: NBIT_ATOMIC,
                size: dt_size,
                order_be: false,
                precision,
                offset,
                d_nelmts: d_nelmts_hint,
            })
        }
    }

    /// Validate the precision / offset against the element size.
    fn validate(&self) -> Result<()> {
        let full = (self.size as u32) * 8;
        if self.size == 0 || self.size > 8 {
            return Err(Hdf5Error::UnsupportedDatatype(format!(
                "N-Bit: unsupported element size {}",
                self.size
            )));
        }
        if self.precision == 0 || self.precision > full || self.offset + self.precision > full {
            return Err(Hdf5Error::Decompression(format!(
                "N-Bit: invalid precision {}/offset {} for {}-byte element",
                self.precision, self.offset, self.size
            )));
        }
        Ok(())
    }

    /// True when the type is full precision at offset 0 (filter is a no-op).
    fn is_full_precision(&self) -> bool {
        self.offset == 0 && self.precision == (self.size as u32) * 8
    }
}

/// Decode an N-Bit chunk (reverse / decompression direction).
///
/// * `data` — the packed chunk (no per-chunk header).
/// * `cd_values` — filter parameters (see the module documentation).
/// * `datatype` — element datatype (supplies signedness and, for the short form,
///   the element size).
/// * `d_nelmts` — number of elements (from the chunk dimensions); used when
///   `cd_values` does not carry the count.
pub fn apply_nbit_reverse(
    data: &[u8],
    cd_values: &[u32],
    datatype: &Datatype,
    d_nelmts: usize,
) -> Result<Vec<u8>> {
    let params = AtomicParams::resolve(cd_values, datatype, d_nelmts)?;

    // No-op datatype: the chunk is stored uncompressed.
    if params.marker == NBIT_NOOPTYPE {
        return Ok(data.to_vec());
    }
    if params.marker != NBIT_ATOMIC {
        return Err(Hdf5Error::UnsupportedDatatype(format!(
            "N-Bit: {} member layout is not supported (atomic integer/float only)",
            match params.marker {
                NBIT_ARRAY => "array",
                NBIT_COMPOUND => "compound",
                _ => "unknown",
            }
        )));
    }
    params.validate()?;

    let size = params.size;
    let n = params.d_nelmts;
    let is_float = datatype.is_float();
    let signed = matches!(
        datatype,
        Datatype::Int8 | Datatype::Int16 | Datatype::Int32 | Datatype::Int64
    );

    let mut output = vec![0u8; n * size];

    // Full precision at offset 0: the filter did nothing, the chunk is raw data.
    if params.is_full_precision() {
        if data.len() < n * size {
            return Err(Hdf5Error::Decompression(format!(
                "N-Bit: passthrough chunk {} bytes, need {}",
                data.len(),
                n * size
            )));
        }
        for i in 0..n {
            let src = &data[i * size..(i + 1) * size];
            let dst = &mut output[i * size..(i + 1) * size];
            if params.order_be {
                for (k, b) in src.iter().rev().enumerate() {
                    dst[k] = *b;
                }
            } else {
                dst.copy_from_slice(src);
            }
        }
        return Ok(output);
    }

    // Reduced-precision floats are not representable as standard IEEE values.
    if is_float {
        return Err(Hdf5Error::UnsupportedDatatype(format!(
            "N-Bit: reduced-precision/offset float (precision {}, offset {}) is not supported",
            params.precision, params.offset
        )));
    }

    // Integer: unpack `precision` significant bits per element (MSB-first) and
    // reconstruct the full-width little-endian value. The offset only affects the
    // in-file bit position; the logical value is offset-independent, so we place
    // the (sign-extended) code at offset 0.
    let precision = u8::try_from(params.precision).map_err(|_| {
        Hdf5Error::Decompression(format!("N-Bit: precision {} exceeds 255", params.precision))
    })?;
    let sign_bit = params.precision - 1;
    let sign_mask = !0u64 << params.precision;

    let mut reader = BitReader::new(data);
    for i in 0..n {
        let code = reader.read_bits(precision)?;
        let value = if signed && (code >> sign_bit) & 1 == 1 {
            code | sign_mask
        } else {
            code
        };
        write_le(&mut output[i * size..(i + 1) * size], value, size);
    }

    Ok(output)
}

/// Encode raw element bytes with the N-Bit filter (forward / compression).
///
/// Produces the exact libhdf5 on-disk layout so the result round-trips through
/// [`apply_nbit_reverse`]. The input is little-endian full-width element bytes;
/// only offset 0 is supported for encoding.
pub fn apply_nbit_forward(data: &[u8], cd_values: &[u32], datatype: &Datatype) -> Result<Vec<u8>> {
    let size = datatype.size();
    if size == 0 || size > 8 {
        return Err(Hdf5Error::Compression(format!(
            "N-Bit: unsupported element size {size}"
        )));
    }
    if data.is_empty() || !data.len().is_multiple_of(size) {
        return Err(Hdf5Error::Compression(format!(
            "N-Bit: data length {} is not a positive multiple of element size {}",
            data.len(),
            size
        )));
    }
    let n = data.len() / size;
    let params = AtomicParams::resolve(cd_values, datatype, n)?;
    params.validate()?;

    if params.marker != NBIT_ATOMIC {
        return Err(Hdf5Error::Compression(
            "N-Bit: only atomic types can be encoded".to_string(),
        ));
    }
    if params.offset != 0 {
        return Err(Hdf5Error::Compression(
            "N-Bit: encoding with a non-zero bit offset is not supported".to_string(),
        ));
    }

    // Full precision: the filter is a no-op, the chunk is the raw bytes.
    if params.is_full_precision() {
        return Ok(data.to_vec());
    }
    if datatype.is_float() {
        return Err(Hdf5Error::Compression(format!(
            "N-Bit: reduced-precision float (precision {}) is not supported",
            params.precision
        )));
    }

    let precision = u8::try_from(params.precision).map_err(|_| {
        Hdf5Error::Compression(format!("N-Bit: precision {} exceeds 255", params.precision))
    })?;
    let mask = if params.precision >= 64 {
        u64::MAX
    } else {
        (1u64 << params.precision) - 1
    };

    let total_bits = (n as u64) * (params.precision as u64);
    let mut writer = BitWriter::with_capacity((total_bits / 8) as usize + 1);
    for i in 0..n {
        let value = read_le(&data[i * size..(i + 1) * size], size);
        writer.write_bits(value & mask, precision);
    }
    let mut payload = writer.finish();
    // libhdf5 always reserves a trailing partial byte; append one when the packed
    // bits fill whole bytes exactly so the layout matches byte-for-byte.
    if total_bits.is_multiple_of(8) {
        payload.push(0);
    }
    Ok(payload)
}

/// Build a real-form N-Bit `cd_values` atomic descriptor for `datatype`.
///
/// Useful for constructing the parameter array a filter-pipeline message parser
/// would supply, and for tests. `precision`/`offset` are in bits.
pub fn build_atomic_cd_values(
    datatype: &Datatype,
    precision: u32,
    offset: u32,
    d_nelmts: u32,
) -> Vec<u32> {
    let size = datatype.size() as u32;
    let flags = if datatype.is_float() { 1 } else { 0 };
    vec![
        8, // nparms
        flags,
        d_nelmts,
        NBIT_ATOMIC,
        size,
        0, // order LE
        precision,
        offset,
    ]
}

/// Write the low `size` bytes of `value` in little-endian order.
fn write_le(dst: &mut [u8], value: u64, size: usize) {
    let bytes = value.to_le_bytes();
    let n = size.min(bytes.len()).min(dst.len());
    dst[..n].copy_from_slice(&bytes[..n]);
}

/// Read a little-endian unsigned integer of `size` bytes (1..=8).
fn read_le(chunk: &[u8], size: usize) -> u64 {
    let mut buf = [0u8; 8];
    let n = size.min(chunk.len()).min(8);
    buf[..n].copy_from_slice(&chunk[..n]);
    u64::from_le_bytes(buf)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use byteorder::{ByteOrder, LittleEndian};

    fn make_u16(values: &[u16]) -> Vec<u8> {
        let mut d = vec![0u8; values.len() * 2];
        for (i, &v) in values.iter().enumerate() {
            LittleEndian::write_u16(&mut d[i * 2..(i + 1) * 2], v);
        }
        d
    }
    fn read_u16(d: &[u8]) -> Vec<u16> {
        d.chunks_exact(2).map(LittleEndian::read_u16).collect()
    }
    fn make_i16(values: &[i16]) -> Vec<u8> {
        let mut d = vec![0u8; values.len() * 2];
        for (i, &v) in values.iter().enumerate() {
            LittleEndian::write_i16(&mut d[i * 2..(i + 1) * 2], v);
        }
        d
    }
    fn read_i16(d: &[u8]) -> Vec<i16> {
        d.chunks_exact(2).map(LittleEndian::read_i16).collect()
    }
    fn read_u32(d: &[u8]) -> Vec<u32> {
        d.chunks_exact(4).map(LittleEndian::read_u32).collect()
    }
    fn read_f32(d: &[u8]) -> Vec<f32> {
        d.chunks_exact(4).map(LittleEndian::read_f32).collect()
    }
    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("valid hex"))
            .collect()
    }

    // ------------------------------------------------------------------
    // Interop: decode real libhdf5 (h5py 3.16.0 / hdf5 2.0.0) chunk bytes.
    //
    // The hex byte strings and cd_values below were captured directly from
    // libhdf5-written datasets (read_direct_chunk + get_filter), validating
    // the true on-disk N-Bit layout against a real HDF5 producer.
    // ------------------------------------------------------------------

    #[test]
    fn interop_hdf5_u16_precision12() {
        let raw = hex("00003f07e0bd0fc13b17a1b900");
        let cd = vec![8, 0, 8, 1, 2, 0, 12, 0];
        let out = apply_nbit_reverse(&raw, &cd, &Datatype::UInt16, 8).unwrap();
        assert_eq!(read_u16(&out), vec![0, 63, 126, 189, 252, 315, 378, 441]);
    }

    #[test]
    fn interop_hdf5_u16_precision12_offset4() {
        // Offset 4: identical packed bytes, logical values unchanged.
        let raw = hex("00003f07e0bd0fc13b17a1b900");
        let cd = vec![8, 0, 8, 1, 2, 0, 12, 4];
        let out = apply_nbit_reverse(&raw, &cd, &Datatype::UInt16, 8).unwrap();
        assert_eq!(read_u16(&out), vec![0, 63, 126, 189, 252, 315, 378, 441]);
    }

    #[test]
    fn interop_hdf5_u16_precision8() {
        let raw = hex("0a141e28323c465000");
        let cd = vec![8, 0, 8, 1, 2, 0, 8, 0];
        let out = apply_nbit_reverse(&raw, &cd, &Datatype::UInt16, 8).unwrap();
        assert_eq!(read_u16(&out), vec![10, 20, 30, 40, 50, 60, 70, 80]);
    }

    #[test]
    fn interop_hdf5_i16_precision10_sign_extend() {
        let raw = hex("feffd0000201ff8013ff00");
        let cd = vec![8, 0, 8, 1, 2, 0, 10, 0];
        let out = apply_nbit_reverse(&raw, &cd, &Datatype::Int16, 8).unwrap();
        assert_eq!(read_i16(&out), vec![-5, -3, 0, 2, 7, -8, 4, -1]);
    }

    #[test]
    fn interop_hdf5_u32_precision20() {
        let raw = hex("00000003e8007d07a120fffff000070002af423f00");
        let cd = vec![8, 0, 8, 1, 4, 0, 20, 0];
        let out = apply_nbit_reverse(&raw, &cd, &Datatype::UInt32, 8).unwrap();
        assert_eq!(
            read_u32(&out),
            vec![0, 1000, 2000, 500000, 1048575, 7, 42, 999999]
        );
    }

    #[test]
    fn interop_hdf5_f32_full_precision_passthrough() {
        let raw = hex("0000c03f000010c00000404000000000");
        let cd = vec![8, 1, 4, 1, 4, 0, 32, 0];
        let out = apply_nbit_reverse(&raw, &cd, &Datatype::Float32, 4).unwrap();
        assert_eq!(read_f32(&out), vec![1.5, -2.25, 3.0, 0.0]);
    }

    #[test]
    fn interop_hdf5_nelmts_from_cd_values() {
        // 2x3 chunk -> 6 elements, count taken from cd_values (hint = 0).
        let mut w = BitWriter::new();
        for v in [0u64, 63, 126, 189, 252, 315] {
            w.write_bits(v, 12);
        }
        let raw = w.finish();
        let cd = vec![8, 0, 6, 1, 2, 0, 12, 0];
        let out = apply_nbit_reverse(&raw, &cd, &Datatype::UInt16, 0).unwrap();
        assert_eq!(read_u16(&out), vec![0, 63, 126, 189, 252, 315]);
    }

    // ------------------------------------------------------------------
    // Hand-built spec-layout bytes (exact assertions).
    // ------------------------------------------------------------------

    #[test]
    fn handbuilt_packing_exact() {
        // precision 4, values 0..8 packed MSB-first -> 0x01 0x23 0x45 0x67 pad.
        let mut w = BitWriter::new();
        for v in 0u64..8 {
            w.write_bits(v, 4);
        }
        let raw = w.finish();
        assert_eq!(raw, hex("01234567"));
        let cd = build_atomic_cd_values(&Datatype::UInt16, 4, 0, 8);
        let out = apply_nbit_reverse(&raw, &cd, &Datatype::UInt16, 8).unwrap();
        assert_eq!(read_u16(&out), vec![0, 1, 2, 3, 4, 5, 6, 7]);
    }

    // ------------------------------------------------------------------
    // Errors: never fabricate output for unsupported member layouts.
    // ------------------------------------------------------------------

    #[test]
    fn reverse_compound_member_is_typed_error() {
        let cd = vec![10, 0, 4, NBIT_COMPOUND, 8, 0, 0, 0, 0, 0];
        let res = apply_nbit_reverse(&[0u8; 8], &cd, &Datatype::UInt16, 4);
        assert!(matches!(res, Err(Hdf5Error::UnsupportedDatatype(_))));
    }

    #[test]
    fn reverse_array_member_is_typed_error() {
        let cd = vec![10, 0, 4, NBIT_ARRAY, 8, 0, 0, 0, 0, 0];
        let res = apply_nbit_reverse(&[0u8; 8], &cd, &Datatype::UInt16, 4);
        assert!(matches!(res, Err(Hdf5Error::UnsupportedDatatype(_))));
    }

    #[test]
    fn reverse_reduced_float_is_typed_error() {
        // precision 20 < 32 for f32 -> not a standard IEEE value.
        let cd = vec![8, 1, 4, 1, 4, 0, 20, 0];
        let res = apply_nbit_reverse(&[0u8; 16], &cd, &Datatype::Float32, 4);
        assert!(matches!(res, Err(Hdf5Error::UnsupportedDatatype(_))));
    }

    #[test]
    fn reverse_nooptype_passes_through() {
        let cd = vec![8, 0, 4, NBIT_NOOPTYPE, 2, 0, 0, 0];
        let data = make_u16(&[1, 2, 3, 4]);
        let out = apply_nbit_reverse(&data, &cd, &Datatype::UInt16, 4).unwrap();
        assert_eq!(out, data);
    }

    // ------------------------------------------------------------------
    // Self round-trip (encode -> decode) — output is byte-compatible with
    // libhdf5, verified against captured chunks where applicable.
    // ------------------------------------------------------------------

    #[test]
    fn roundtrip_u16_precision12_matches_hdf5() {
        let values = vec![0u16, 63, 126, 189, 252, 315, 378, 441];
        let data = make_u16(&values);
        let cd = build_atomic_cd_values(&Datatype::UInt16, 12, 0, values.len() as u32);
        let packed = apply_nbit_forward(&data, &cd, &Datatype::UInt16).unwrap();
        // Byte-exact against libhdf5 (96 packed bits + one reserved trailing byte).
        assert_eq!(packed, hex("00003f07e0bd0fc13b17a1b900"));
        let out = apply_nbit_reverse(&packed, &cd, &Datatype::UInt16, values.len()).unwrap();
        assert_eq!(read_u16(&out), values);
    }

    #[test]
    fn roundtrip_i16_precision10() {
        let values = vec![-5i16, -3, 0, 2, 7, -8, 4, -1];
        let data = make_i16(&values);
        let cd = build_atomic_cd_values(&Datatype::Int16, 10, 0, values.len() as u32);
        let packed = apply_nbit_forward(&data, &cd, &Datatype::Int16).unwrap();
        assert!(packed.len() < data.len());
        let out = apply_nbit_reverse(&packed, &cd, &Datatype::Int16, values.len()).unwrap();
        assert_eq!(read_i16(&out), values);
    }

    #[test]
    fn roundtrip_u8_precision4() {
        let values: Vec<u8> = vec![0, 1, 2, 3, 15, 7, 8, 9];
        let cd = build_atomic_cd_values(&Datatype::UInt8, 4, 0, values.len() as u32);
        let packed = apply_nbit_forward(&values, &cd, &Datatype::UInt8).unwrap();
        let out = apply_nbit_reverse(&packed, &cd, &Datatype::UInt8, values.len()).unwrap();
        assert_eq!(out, values);
    }

    #[test]
    fn roundtrip_f32_full_precision_identity() {
        let values = vec![1.5f32, -2.25, 3.0, 0.0];
        let mut data = vec![0u8; values.len() * 4];
        for (i, &v) in values.iter().enumerate() {
            LittleEndian::write_f32(&mut data[i * 4..(i + 1) * 4], v);
        }
        let cd = build_atomic_cd_values(&Datatype::Float32, 32, 0, values.len() as u32);
        let packed = apply_nbit_forward(&data, &cd, &Datatype::Float32).unwrap();
        assert_eq!(packed, data); // identity passthrough
        let out = apply_nbit_reverse(&packed, &cd, &Datatype::Float32, values.len()).unwrap();
        assert_eq!(read_f32(&out), values);
    }

    #[test]
    fn roundtrip_short_form_default_full_precision() {
        // Empty cd_values -> full precision -> identity round-trip.
        let values = vec![10u16, 20, 30, 40];
        let data = make_u16(&values);
        let packed = apply_nbit_forward(&data, &[], &Datatype::UInt16).unwrap();
        assert_eq!(packed, data);
        let out = apply_nbit_reverse(&packed, &[], &Datatype::UInt16, values.len()).unwrap();
        assert_eq!(read_u16(&out), values);
    }

    #[test]
    fn roundtrip_short_form_precision_hint() {
        let values = vec![0u16, 63, 126, 189, 252, 315, 378, 441];
        let data = make_u16(&values);
        let packed = apply_nbit_forward(&data, &[12], &Datatype::UInt16).unwrap();
        assert!(packed.len() < data.len());
        let out = apply_nbit_reverse(&packed, &[12], &Datatype::UInt16, values.len()).unwrap();
        assert_eq!(read_u16(&out), values);
    }

    #[test]
    fn forward_rejects_empty() {
        assert!(apply_nbit_forward(&[], &[12], &Datatype::UInt16).is_err());
    }
}
