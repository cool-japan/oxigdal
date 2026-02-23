//! HDF5 ScaleOffset filter implementation.
//!
//! The ScaleOffset filter provides lossy compression for floating-point data
//! and lossless compression for integer data by:
//!
//! 1. Finding the minimum value in the dataset
//! 2. Subtracting the minimum (offset) from all values
//! 3. For floating-point data, quantizing to integers with given decimal precision
//! 4. Packing the offset values using the minimum number of bits needed
//!
//! ## Scale Types
//!
//! - `H5Z_SO_FLOAT_DSCALE` (0): Fixed decimal precision for floating-point data.
//!   The `scale_factor` specifies the number of decimal digits to preserve.
//! - `H5Z_SO_INT` (2): Automatic minimum-bits for integer data.
//!   The range of values determines the bit width.
//!
//! ## Header Format
//!
//! The compressed data includes a header followed by packed bit data:
//!
//! | Offset | Size | Field            | Description                          |
//! |--------|------|------------------|--------------------------------------|
//! | 0      | 1    | version          | Header version (currently 1)         |
//! | 1      | 1    | dtype_class      | 0=signed int, 1=unsigned int, 2=f32, 3=f64 |
//! | 2      | 1    | orig_elem_size   | Original element size in bytes       |
//! | 3      | 1    | bits_per_value   | Bits per packed value (1..=64)       |
//! | 4      | 4    | scale_factor     | Scale factor as i32 (LE)             |
//! | 8      | 4    | num_elements     | Number of elements as u32 (LE)       |
//! | 12     | 8    | min_value        | Minimum value as i64 (LE)            |
//! | 20     | var  | packed_data      | Bit-packed delta values              |

use crate::datatype::Datatype;
use crate::error::{Hdf5Error, Result};
use byteorder::{ByteOrder, LittleEndian};

use super::bitpack::{BitReader, BitWriter, min_bits_for_value};

/// ScaleOffset header size in bytes
const HEADER_SIZE: usize = 20;

/// Current header version
const HEADER_VERSION: u8 = 1;

/// Scale type: float decimal scale (H5Z_SO_FLOAT_DSCALE)
const SO_FLOAT_DSCALE: u32 = 0;

/// Scale type: integer auto (H5Z_SO_INT)
const SO_INT: u32 = 2;

/// Dtype class identifiers for the header
const DTYPE_SIGNED_INT: u8 = 0;
const DTYPE_UNSIGNED_INT: u8 = 1;
const DTYPE_FLOAT32: u8 = 2;
const DTYPE_FLOAT64: u8 = 3;

/// Apply ScaleOffset filter in the forward (compression) direction.
///
/// # Arguments
/// * `data` - Raw byte data
/// * `params` - Filter parameters: `[scale_type, scale_factor]`
/// * `datatype` - The HDF5 datatype of the elements
pub fn apply_scale_offset_forward(
    data: &[u8],
    params: &[u32],
    datatype: &Datatype,
) -> Result<Vec<u8>> {
    let scale_type = params.first().copied().unwrap_or(SO_INT);
    let scale_factor = params.get(1).copied().unwrap_or(0) as i32;

    match (scale_type, datatype) {
        (SO_FLOAT_DSCALE, Datatype::Float32) => compress_float32(data, scale_factor),
        (SO_FLOAT_DSCALE, Datatype::Float64) => compress_float64(data, scale_factor),
        (SO_INT, dt) if dt.is_integer() => compress_integer(data, datatype),
        // Also handle integer scale_type=0 by treating as auto
        (SO_FLOAT_DSCALE, dt) if dt.is_integer() => compress_integer(data, datatype),
        _ => Err(Hdf5Error::Compression(format!(
            "ScaleOffset: unsupported scale_type={} for datatype {:?}",
            scale_type, datatype
        ))),
    }
}

/// Apply ScaleOffset filter in the reverse (decompression) direction.
///
/// # Arguments
/// * `data` - Compressed byte data (header + packed bits)
/// * `params` - Filter parameters: `[scale_type, scale_factor]`
/// * `datatype` - The HDF5 datatype of the elements
pub fn apply_scale_offset_reverse(
    data: &[u8],
    _params: &[u32],
    datatype: &Datatype,
) -> Result<Vec<u8>> {
    if data.len() < HEADER_SIZE {
        return Err(Hdf5Error::Decompression(
            "ScaleOffset: data too short for header".to_string(),
        ));
    }

    let version = data[0];
    if version != HEADER_VERSION {
        return Err(Hdf5Error::Decompression(format!(
            "ScaleOffset: unsupported header version {}",
            version
        )));
    }

    let dtype_class = data[1];
    let _orig_elem_size = data[2];
    let bits_per_value = data[3];
    let scale_factor = LittleEndian::read_i32(&data[4..8]);
    let num_elements = LittleEndian::read_u32(&data[8..12]) as usize;
    let min_value = LittleEndian::read_i64(&data[12..20]);
    let packed_data = &data[HEADER_SIZE..];

    match dtype_class {
        DTYPE_SIGNED_INT => decompress_signed_int(
            packed_data,
            num_elements,
            bits_per_value,
            min_value,
            datatype,
        ),
        DTYPE_UNSIGNED_INT => decompress_unsigned_int(
            packed_data,
            num_elements,
            bits_per_value,
            min_value,
            datatype,
        ),
        DTYPE_FLOAT32 => decompress_float32(
            packed_data,
            num_elements,
            bits_per_value,
            min_value,
            scale_factor,
        ),
        DTYPE_FLOAT64 => decompress_float64(
            packed_data,
            num_elements,
            bits_per_value,
            min_value,
            scale_factor,
        ),
        _ => Err(Hdf5Error::Decompression(format!(
            "ScaleOffset: unknown dtype class {}",
            dtype_class
        ))),
    }
}

// =============================================================================
// Integer compression
// =============================================================================

/// Compress integer data using ScaleOffset.
fn compress_integer(data: &[u8], datatype: &Datatype) -> Result<Vec<u8>> {
    let elem_size = datatype.size();
    if data.is_empty() || data.len() % elem_size != 0 {
        return Err(Hdf5Error::Compression(format!(
            "ScaleOffset: data length {} not divisible by element size {}",
            data.len(),
            elem_size
        )));
    }
    let num_elements = data.len() / elem_size;

    let is_signed = matches!(
        datatype,
        Datatype::Int8 | Datatype::Int16 | Datatype::Int32 | Datatype::Int64
    );

    if is_signed {
        compress_signed_int_impl(data, datatype, num_elements, elem_size)
    } else {
        compress_unsigned_int_impl(data, datatype, num_elements, elem_size)
    }
}

/// Compress signed integer data.
fn compress_signed_int_impl(
    data: &[u8],
    datatype: &Datatype,
    num_elements: usize,
    elem_size: usize,
) -> Result<Vec<u8>> {
    // Read all values as i64
    let mut values = Vec::with_capacity(num_elements);
    for i in 0..num_elements {
        let offset = i * elem_size;
        let chunk = &data[offset..offset + elem_size];
        let val = read_signed_value(chunk, datatype)?;
        values.push(val);
    }

    // Find min value
    let min_val = values.iter().copied().min().unwrap_or(0);

    // Compute deltas (all non-negative)
    let deltas: Vec<u64> = values.iter().map(|&v| (v - min_val) as u64).collect();

    // Find maximum delta to determine bit width
    let max_delta = deltas.iter().copied().max().unwrap_or(0);
    let bits_per_value = min_bits_for_value(max_delta);

    // Pack into output
    pack_with_header(
        &deltas,
        bits_per_value,
        DTYPE_SIGNED_INT,
        elem_size as u8,
        0, // scale_factor not used for integers
        num_elements,
        min_val,
    )
}

/// Compress unsigned integer data.
fn compress_unsigned_int_impl(
    data: &[u8],
    datatype: &Datatype,
    num_elements: usize,
    elem_size: usize,
) -> Result<Vec<u8>> {
    // Read all values as u64
    let mut values = Vec::with_capacity(num_elements);
    for i in 0..num_elements {
        let offset = i * elem_size;
        let chunk = &data[offset..offset + elem_size];
        let val = read_unsigned_value(chunk, datatype)?;
        values.push(val);
    }

    // Find min value
    let min_val = values.iter().copied().min().unwrap_or(0);

    // Compute deltas
    let deltas: Vec<u64> = values.iter().map(|&v| v - min_val).collect();

    // Find maximum delta
    let max_delta = deltas.iter().copied().max().unwrap_or(0);
    let bits_per_value = min_bits_for_value(max_delta);

    // Store min_val as i64 (safe for unsigned values up to i64::MAX range)
    pack_with_header(
        &deltas,
        bits_per_value,
        DTYPE_UNSIGNED_INT,
        elem_size as u8,
        0,
        num_elements,
        min_val as i64,
    )
}

// =============================================================================
// Float compression
// =============================================================================

/// Compress f32 data using ScaleOffset with decimal scaling.
fn compress_float32(data: &[u8], scale_factor: i32) -> Result<Vec<u8>> {
    let elem_size = 4;
    if data.is_empty() || data.len() % elem_size != 0 {
        return Err(Hdf5Error::Compression(format!(
            "ScaleOffset: data length {} not divisible by f32 element size {}",
            data.len(),
            elem_size
        )));
    }
    let num_elements = data.len() / elem_size;
    let multiplier = 10.0_f64.powi(scale_factor);

    // Read all f32 values, scale to integer domain
    let mut scaled_values = Vec::with_capacity(num_elements);
    for i in 0..num_elements {
        let offset = i * elem_size;
        let val = LittleEndian::read_f32(&data[offset..offset + elem_size]) as f64;
        let scaled = (val * multiplier).round() as i64;
        scaled_values.push(scaled);
    }

    // Find min
    let min_val = scaled_values.iter().copied().min().unwrap_or(0);

    // Compute deltas
    let deltas: Vec<u64> = scaled_values
        .iter()
        .map(|&v| (v - min_val) as u64)
        .collect();

    let max_delta = deltas.iter().copied().max().unwrap_or(0);
    let bits_per_value = min_bits_for_value(max_delta);

    pack_with_header(
        &deltas,
        bits_per_value,
        DTYPE_FLOAT32,
        elem_size as u8,
        scale_factor,
        num_elements,
        min_val,
    )
}

/// Compress f64 data using ScaleOffset with decimal scaling.
fn compress_float64(data: &[u8], scale_factor: i32) -> Result<Vec<u8>> {
    let elem_size = 8;
    if data.is_empty() || data.len() % elem_size != 0 {
        return Err(Hdf5Error::Compression(format!(
            "ScaleOffset: data length {} not divisible by f64 element size {}",
            data.len(),
            elem_size
        )));
    }
    let num_elements = data.len() / elem_size;
    let multiplier = 10.0_f64.powi(scale_factor);

    // Read all f64 values, scale to integer domain
    let mut scaled_values = Vec::with_capacity(num_elements);
    for i in 0..num_elements {
        let offset = i * elem_size;
        let val = LittleEndian::read_f64(&data[offset..offset + elem_size]);
        let scaled = (val * multiplier).round() as i64;
        scaled_values.push(scaled);
    }

    // Find min
    let min_val = scaled_values.iter().copied().min().unwrap_or(0);

    // Compute deltas
    let deltas: Vec<u64> = scaled_values
        .iter()
        .map(|&v| (v - min_val) as u64)
        .collect();

    let max_delta = deltas.iter().copied().max().unwrap_or(0);
    let bits_per_value = min_bits_for_value(max_delta);

    pack_with_header(
        &deltas,
        bits_per_value,
        DTYPE_FLOAT64,
        elem_size as u8,
        scale_factor,
        num_elements,
        min_val,
    )
}

// =============================================================================
// Decompression
// =============================================================================

/// Decompress signed integer data.
fn decompress_signed_int(
    packed_data: &[u8],
    num_elements: usize,
    bits_per_value: u8,
    min_value: i64,
    datatype: &Datatype,
) -> Result<Vec<u8>> {
    let elem_size = datatype.size();
    let deltas = unpack_deltas(packed_data, num_elements, bits_per_value)?;

    let mut output = vec![0u8; num_elements * elem_size];
    for (i, &delta) in deltas.iter().enumerate() {
        let val = min_value.wrapping_add(delta as i64);
        let offset = i * elem_size;
        write_signed_value(&mut output[offset..offset + elem_size], val, datatype)?;
    }

    Ok(output)
}

/// Decompress unsigned integer data.
fn decompress_unsigned_int(
    packed_data: &[u8],
    num_elements: usize,
    bits_per_value: u8,
    min_value: i64,
    datatype: &Datatype,
) -> Result<Vec<u8>> {
    let elem_size = datatype.size();
    let deltas = unpack_deltas(packed_data, num_elements, bits_per_value)?;

    let min_unsigned = min_value as u64;
    let mut output = vec![0u8; num_elements * elem_size];
    for (i, &delta) in deltas.iter().enumerate() {
        let val = min_unsigned.wrapping_add(delta);
        let offset = i * elem_size;
        write_unsigned_value(&mut output[offset..offset + elem_size], val, datatype)?;
    }

    Ok(output)
}

/// Decompress f32 data.
fn decompress_float32(
    packed_data: &[u8],
    num_elements: usize,
    bits_per_value: u8,
    min_value: i64,
    scale_factor: i32,
) -> Result<Vec<u8>> {
    let elem_size = 4;
    let divisor = 10.0_f64.powi(scale_factor);
    let deltas = unpack_deltas(packed_data, num_elements, bits_per_value)?;

    let mut output = vec![0u8; num_elements * elem_size];
    for (i, &delta) in deltas.iter().enumerate() {
        let scaled_val = min_value + delta as i64;
        let float_val = (scaled_val as f64) / divisor;
        let offset = i * elem_size;
        LittleEndian::write_f32(&mut output[offset..offset + elem_size], float_val as f32);
    }

    Ok(output)
}

/// Decompress f64 data.
fn decompress_float64(
    packed_data: &[u8],
    num_elements: usize,
    bits_per_value: u8,
    min_value: i64,
    scale_factor: i32,
) -> Result<Vec<u8>> {
    let elem_size = 8;
    let divisor = 10.0_f64.powi(scale_factor);
    let deltas = unpack_deltas(packed_data, num_elements, bits_per_value)?;

    let mut output = vec![0u8; num_elements * elem_size];
    for (i, &delta) in deltas.iter().enumerate() {
        let scaled_val = min_value + delta as i64;
        let float_val = (scaled_val as f64) / divisor;
        let offset = i * elem_size;
        LittleEndian::write_f64(&mut output[offset..offset + elem_size], float_val);
    }

    Ok(output)
}

// =============================================================================
// Helper functions
// =============================================================================

/// Pack deltas with a standard header into the output buffer.
fn pack_with_header(
    deltas: &[u64],
    bits_per_value: u8,
    dtype_class: u8,
    orig_elem_size: u8,
    scale_factor: i32,
    num_elements: usize,
    min_value: i64,
) -> Result<Vec<u8>> {
    // Estimate output size: header + packed bits
    let packed_bits = (num_elements as u64) * (bits_per_value as u64);
    let packed_bytes = packed_bits.div_ceil(8) as usize;
    let total_size = HEADER_SIZE + packed_bytes;

    let mut output = Vec::with_capacity(total_size);

    // Write header
    output.push(HEADER_VERSION);
    output.push(dtype_class);
    output.push(orig_elem_size);
    output.push(bits_per_value);

    let mut sf_bytes = [0u8; 4];
    LittleEndian::write_i32(&mut sf_bytes, scale_factor);
    output.extend_from_slice(&sf_bytes);

    let mut ne_bytes = [0u8; 4];
    let ne_u32 = u32::try_from(num_elements).map_err(|_| {
        Hdf5Error::Compression("ScaleOffset: num_elements exceeds u32 range".to_string())
    })?;
    LittleEndian::write_u32(&mut ne_bytes, ne_u32);
    output.extend_from_slice(&ne_bytes);

    let mut mv_bytes = [0u8; 8];
    LittleEndian::write_i64(&mut mv_bytes, min_value);
    output.extend_from_slice(&mv_bytes);

    // Pack deltas
    let mut writer = BitWriter::with_capacity(packed_bytes);
    for &delta in deltas {
        writer.write_bits(delta, bits_per_value);
    }
    output.extend_from_slice(&writer.finish());

    Ok(output)
}

/// Unpack deltas from packed bit data.
fn unpack_deltas(packed_data: &[u8], num_elements: usize, bits_per_value: u8) -> Result<Vec<u64>> {
    let mut reader = BitReader::new(packed_data);
    let mut deltas = Vec::with_capacity(num_elements);
    for _ in 0..num_elements {
        let delta = reader.read_bits(bits_per_value)?;
        deltas.push(delta);
    }
    Ok(deltas)
}

/// Read a signed integer value from bytes based on datatype.
fn read_signed_value(chunk: &[u8], datatype: &Datatype) -> Result<i64> {
    match datatype {
        Datatype::Int8 => {
            if chunk.is_empty() {
                return Err(Hdf5Error::Decompression("Empty data for Int8".to_string()));
            }
            Ok(chunk[0] as i8 as i64)
        }
        Datatype::Int16 => {
            if chunk.len() < 2 {
                return Err(Hdf5Error::Decompression(
                    "Insufficient data for Int16".to_string(),
                ));
            }
            Ok(LittleEndian::read_i16(chunk) as i64)
        }
        Datatype::Int32 => {
            if chunk.len() < 4 {
                return Err(Hdf5Error::Decompression(
                    "Insufficient data for Int32".to_string(),
                ));
            }
            Ok(LittleEndian::read_i32(chunk) as i64)
        }
        Datatype::Int64 => {
            if chunk.len() < 8 {
                return Err(Hdf5Error::Decompression(
                    "Insufficient data for Int64".to_string(),
                ));
            }
            Ok(LittleEndian::read_i64(chunk))
        }
        _ => Err(Hdf5Error::Compression(format!(
            "ScaleOffset: expected signed integer type, got {:?}",
            datatype
        ))),
    }
}

/// Read an unsigned integer value from bytes based on datatype.
fn read_unsigned_value(chunk: &[u8], datatype: &Datatype) -> Result<u64> {
    match datatype {
        Datatype::UInt8 => {
            if chunk.is_empty() {
                return Err(Hdf5Error::Decompression("Empty data for UInt8".to_string()));
            }
            Ok(chunk[0] as u64)
        }
        Datatype::UInt16 => {
            if chunk.len() < 2 {
                return Err(Hdf5Error::Decompression(
                    "Insufficient data for UInt16".to_string(),
                ));
            }
            Ok(LittleEndian::read_u16(chunk) as u64)
        }
        Datatype::UInt32 => {
            if chunk.len() < 4 {
                return Err(Hdf5Error::Decompression(
                    "Insufficient data for UInt32".to_string(),
                ));
            }
            Ok(LittleEndian::read_u32(chunk) as u64)
        }
        Datatype::UInt64 => {
            if chunk.len() < 8 {
                return Err(Hdf5Error::Decompression(
                    "Insufficient data for UInt64".to_string(),
                ));
            }
            Ok(LittleEndian::read_u64(chunk))
        }
        _ => Err(Hdf5Error::Compression(format!(
            "ScaleOffset: expected unsigned integer type, got {:?}",
            datatype
        ))),
    }
}

/// Write a signed integer value to bytes based on datatype.
fn write_signed_value(buf: &mut [u8], value: i64, datatype: &Datatype) -> Result<()> {
    match datatype {
        Datatype::Int8 => {
            if buf.is_empty() {
                return Err(Hdf5Error::Decompression(
                    "Empty buffer for Int8".to_string(),
                ));
            }
            buf[0] = value as i8 as u8;
            Ok(())
        }
        Datatype::Int16 => {
            if buf.len() < 2 {
                return Err(Hdf5Error::Decompression(
                    "Insufficient buffer for Int16".to_string(),
                ));
            }
            LittleEndian::write_i16(buf, value as i16);
            Ok(())
        }
        Datatype::Int32 => {
            if buf.len() < 4 {
                return Err(Hdf5Error::Decompression(
                    "Insufficient buffer for Int32".to_string(),
                ));
            }
            LittleEndian::write_i32(buf, value as i32);
            Ok(())
        }
        Datatype::Int64 => {
            if buf.len() < 8 {
                return Err(Hdf5Error::Decompression(
                    "Insufficient buffer for Int64".to_string(),
                ));
            }
            LittleEndian::write_i64(buf, value);
            Ok(())
        }
        _ => Err(Hdf5Error::Decompression(format!(
            "ScaleOffset: cannot write signed value to {:?}",
            datatype
        ))),
    }
}

/// Write an unsigned integer value to bytes based on datatype.
fn write_unsigned_value(buf: &mut [u8], value: u64, datatype: &Datatype) -> Result<()> {
    match datatype {
        Datatype::UInt8 => {
            if buf.is_empty() {
                return Err(Hdf5Error::Decompression(
                    "Empty buffer for UInt8".to_string(),
                ));
            }
            buf[0] = value as u8;
            Ok(())
        }
        Datatype::UInt16 => {
            if buf.len() < 2 {
                return Err(Hdf5Error::Decompression(
                    "Insufficient buffer for UInt16".to_string(),
                ));
            }
            LittleEndian::write_u16(buf, value as u16);
            Ok(())
        }
        Datatype::UInt32 => {
            if buf.len() < 4 {
                return Err(Hdf5Error::Decompression(
                    "Insufficient buffer for UInt32".to_string(),
                ));
            }
            LittleEndian::write_u32(buf, value as u32);
            Ok(())
        }
        Datatype::UInt64 => {
            if buf.len() < 8 {
                return Err(Hdf5Error::Decompression(
                    "Insufficient buffer for UInt64".to_string(),
                ));
            }
            LittleEndian::write_u64(buf, value);
            Ok(())
        }
        _ => Err(Hdf5Error::Decompression(format!(
            "ScaleOffset: cannot write unsigned value to {:?}",
            datatype
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_i32_data(values: &[i32]) -> Vec<u8> {
        let mut data = vec![0u8; values.len() * 4];
        for (i, &v) in values.iter().enumerate() {
            LittleEndian::write_i32(&mut data[i * 4..(i + 1) * 4], v);
        }
        data
    }

    fn read_i32_data(data: &[u8]) -> Vec<i32> {
        let mut values = Vec::new();
        for chunk in data.chunks(4) {
            if chunk.len() == 4 {
                values.push(LittleEndian::read_i32(chunk));
            }
        }
        values
    }

    fn make_u16_data(values: &[u16]) -> Vec<u8> {
        let mut data = vec![0u8; values.len() * 2];
        for (i, &v) in values.iter().enumerate() {
            LittleEndian::write_u16(&mut data[i * 2..(i + 1) * 2], v);
        }
        data
    }

    fn read_u16_data(data: &[u8]) -> Vec<u16> {
        let mut values = Vec::new();
        for chunk in data.chunks(2) {
            if chunk.len() == 2 {
                values.push(LittleEndian::read_u16(chunk));
            }
        }
        values
    }

    fn make_f32_data(values: &[f32]) -> Vec<u8> {
        let mut data = vec![0u8; values.len() * 4];
        for (i, &v) in values.iter().enumerate() {
            LittleEndian::write_f32(&mut data[i * 4..(i + 1) * 4], v);
        }
        data
    }

    fn read_f32_data(data: &[u8]) -> Vec<f32> {
        let mut values = Vec::new();
        for chunk in data.chunks(4) {
            if chunk.len() == 4 {
                values.push(LittleEndian::read_f32(chunk));
            }
        }
        values
    }

    fn make_f64_data(values: &[f64]) -> Vec<u8> {
        let mut data = vec![0u8; values.len() * 8];
        for (i, &v) in values.iter().enumerate() {
            LittleEndian::write_f64(&mut data[i * 8..(i + 1) * 8], v);
        }
        data
    }

    fn read_f64_data(data: &[u8]) -> Vec<f64> {
        let mut values = Vec::new();
        for chunk in data.chunks(8) {
            if chunk.len() == 8 {
                values.push(LittleEndian::read_f64(chunk));
            }
        }
        values
    }

    #[test]
    fn test_scale_offset_i32_roundtrip() {
        let values = vec![100i32, 105, 110, 103, 108, 115, 100, 120];
        let data = make_i32_data(&values);
        let params = [SO_INT, 0];

        let compressed =
            apply_scale_offset_forward(&data, &params, &Datatype::Int32).expect("compress failed");

        // Should be smaller than original (8 values * 4 bytes = 32 bytes)
        // Range is 0-20, needs 5 bits, so 8*5 = 40 bits = 5 bytes + 20 header = 25 bytes
        assert!(compressed.len() < data.len());

        let decompressed = apply_scale_offset_reverse(&compressed, &params, &Datatype::Int32)
            .expect("decompress failed");
        let result = read_i32_data(&decompressed);
        assert_eq!(result, values);
    }

    #[test]
    fn test_scale_offset_u16_roundtrip() {
        let values = vec![1000u16, 1001, 1002, 1003, 1004, 1005, 1006, 1007];
        let data = make_u16_data(&values);
        let params = [SO_INT, 0];

        let compressed =
            apply_scale_offset_forward(&data, &params, &Datatype::UInt16).expect("compress failed");
        let decompressed = apply_scale_offset_reverse(&compressed, &params, &Datatype::UInt16)
            .expect("decompress failed");
        let result = read_u16_data(&decompressed);
        assert_eq!(result, values);
    }

    #[test]
    fn test_scale_offset_constant_values() {
        // All same value - should compress very well (1 bit per value)
        let values = vec![42i32; 100];
        let data = make_i32_data(&values);
        let params = [SO_INT, 0];

        let compressed =
            apply_scale_offset_forward(&data, &params, &Datatype::Int32).expect("compress failed");

        // With all same values, delta is 0 for all, needs only 1 bit each
        // 100 bits = 13 bytes + 20 header = 33 bytes vs 400 bytes original
        assert!(compressed.len() < 40);

        let decompressed = apply_scale_offset_reverse(&compressed, &params, &Datatype::Int32)
            .expect("decompress failed");
        let result = read_i32_data(&decompressed);
        assert_eq!(result, values);
    }

    #[test]
    fn test_scale_offset_negative_values() {
        let values = vec![-50i32, -45, -40, -55, -30, -60, -35, -42];
        let data = make_i32_data(&values);
        let params = [SO_INT, 0];

        let compressed =
            apply_scale_offset_forward(&data, &params, &Datatype::Int32).expect("compress failed");
        let decompressed = apply_scale_offset_reverse(&compressed, &params, &Datatype::Int32)
            .expect("decompress failed");
        let result = read_i32_data(&decompressed);
        assert_eq!(result, values);
    }

    #[test]
    fn test_scale_offset_f32_roundtrip() {
        let values = vec![20.5f32, 20.6, 20.7, 20.3, 20.8, 20.1, 20.9, 20.4];
        let data = make_f32_data(&values);
        // 1 decimal digit of precision
        let params = [SO_FLOAT_DSCALE, 1];

        let compressed = apply_scale_offset_forward(&data, &params, &Datatype::Float32)
            .expect("compress failed");
        let decompressed = apply_scale_offset_reverse(&compressed, &params, &Datatype::Float32)
            .expect("decompress failed");
        let result = read_f32_data(&decompressed);

        // Check values match within the precision of 1 decimal digit
        for (orig, decoded) in values.iter().zip(result.iter()) {
            assert!(
                (orig - decoded).abs() < 0.1,
                "Mismatch: orig={orig}, decoded={decoded}"
            );
        }
    }

    #[test]
    fn test_scale_offset_f64_roundtrip() {
        let values = vec![100.123f64, 100.456, 100.789, 100.012, 100.345, 100.678];
        let data = make_f64_data(&values);
        // 3 decimal digits of precision
        let params = [SO_FLOAT_DSCALE, 3];

        let compressed = apply_scale_offset_forward(&data, &params, &Datatype::Float64)
            .expect("compress failed");
        let decompressed = apply_scale_offset_reverse(&compressed, &params, &Datatype::Float64)
            .expect("decompress failed");
        let result = read_f64_data(&decompressed);

        for (orig, decoded) in values.iter().zip(result.iter()) {
            assert!(
                (orig - decoded).abs() < 0.001,
                "Mismatch: orig={orig}, decoded={decoded}"
            );
        }
    }

    #[test]
    fn test_scale_offset_single_element() {
        let values = vec![42i32];
        let data = make_i32_data(&values);
        let params = [SO_INT, 0];

        let compressed =
            apply_scale_offset_forward(&data, &params, &Datatype::Int32).expect("compress failed");
        let decompressed = apply_scale_offset_reverse(&compressed, &params, &Datatype::Int32)
            .expect("decompress failed");
        let result = read_i32_data(&decompressed);
        assert_eq!(result, values);
    }

    #[test]
    fn test_scale_offset_i8_roundtrip() {
        let byte_values: Vec<i8> = vec![-10, -5, 0, 5, 10, 15, 20, -3];
        let data: Vec<u8> = byte_values.iter().map(|&v| v as u8).collect();
        let params = [SO_INT, 0];

        let compressed =
            apply_scale_offset_forward(&data, &params, &Datatype::Int8).expect("compress failed");
        let decompressed = apply_scale_offset_reverse(&compressed, &params, &Datatype::Int8)
            .expect("decompress failed");
        let result: Vec<i8> = decompressed.iter().map(|&b| b as i8).collect();
        assert_eq!(result, byte_values);
    }

    #[test]
    fn test_scale_offset_empty_data_error() {
        let data: Vec<u8> = vec![];
        let params = [SO_INT, 0];
        let result = apply_scale_offset_forward(&data, &params, &Datatype::Int32);
        assert!(result.is_err());
    }

    #[test]
    fn test_scale_offset_header_too_short_error() {
        let data = vec![0u8; 10]; // Less than HEADER_SIZE
        let params = [SO_INT, 0];
        let result = apply_scale_offset_reverse(&data, &params, &Datatype::Int32);
        assert!(result.is_err());
    }
}
