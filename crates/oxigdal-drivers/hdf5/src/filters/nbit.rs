//! HDF5 N-Bit filter implementation.
//!
//! The N-Bit filter provides lossless compression by packing data using only
//! the significant bits needed to represent the actual value range. This is
//! particularly effective when:
//!
//! - Integer data uses only a fraction of its storage type's range
//!   (e.g., 12-bit sensor data stored in 16-bit integers)
//! - Floating-point data has limited actual precision
//! - Data has been quantized to a known range
//!
//! ## Algorithm
//!
//! ### Forward (Packing)
//! 1. Analyze all values to determine the actual range
//! 2. For signed types, zigzag-encode to unsigned domain
//! 3. Determine the minimum number of bits to represent `max_value`
//! 4. Pack each value using only those significant bits
//!
//! ### Reverse (Unpacking)
//! 1. Read the header to get original element size, significant bits, and signedness
//! 2. Unpack each value from the significant bits
//! 3. For signed types, zigzag-decode back to signed domain
//! 4. Write full-width values to the output buffer
//!
//! ## Header Format
//!
//! | Offset | Size | Field            | Description                          |
//! |--------|------|------------------|--------------------------------------|
//! | 0      | 1    | version          | Header version (currently 1)         |
//! | 1      | 1    | orig_elem_size   | Original element size in bytes       |
//! | 2      | 1    | significant_bits | Number of significant bits per value |
//! | 3      | 1    | is_signed        | 0 = unsigned, 1 = signed (zigzag)    |
//! | 4      | 4    | num_elements     | Number of elements as u32 (LE)       |
//! | 8      | var  | packed_data      | Bit-packed values                    |

use crate::datatype::Datatype;
use crate::error::{Hdf5Error, Result};
use byteorder::{ByteOrder, LittleEndian};

use super::bitpack::{BitReader, BitWriter, min_bits_for_value, zigzag_decode, zigzag_encode};

/// N-Bit header size in bytes
const HEADER_SIZE: usize = 8;

/// Current header version
const HEADER_VERSION: u8 = 1;

/// Apply N-Bit filter in the forward (packing) direction.
///
/// Analyzes the data to determine the minimum number of significant bits
/// needed and packs each element using only those bits.
///
/// # Arguments
/// * `data` - Raw byte data
/// * `datatype` - The HDF5 datatype of the elements
pub fn apply_nbit_forward(data: &[u8], datatype: &Datatype) -> Result<Vec<u8>> {
    let elem_size = datatype.size();
    if data.is_empty() || data.len() % elem_size != 0 {
        return Err(Hdf5Error::Compression(format!(
            "N-Bit: data length {} not divisible by element size {}",
            data.len(),
            elem_size
        )));
    }

    if !datatype.is_integer() {
        return Err(Hdf5Error::Compression(format!(
            "N-Bit: unsupported datatype {:?} (only integer types supported)",
            datatype
        )));
    }

    let num_elements = data.len() / elem_size;
    let is_signed = matches!(
        datatype,
        Datatype::Int8 | Datatype::Int16 | Datatype::Int32 | Datatype::Int64
    );

    if is_signed {
        pack_signed(data, datatype, num_elements, elem_size)
    } else {
        pack_unsigned(data, datatype, num_elements, elem_size)
    }
}

/// Apply N-Bit filter in the reverse (unpacking) direction.
///
/// Reads the header to determine packing parameters, then unpacks
/// each element to its full-width representation.
///
/// # Arguments
/// * `data` - Packed byte data (header + bit-packed values)
/// * `datatype` - The HDF5 datatype of the elements
pub fn apply_nbit_reverse(data: &[u8], datatype: &Datatype) -> Result<Vec<u8>> {
    if data.len() < HEADER_SIZE {
        return Err(Hdf5Error::Decompression(
            "N-Bit: data too short for header".to_string(),
        ));
    }

    let version = data[0];
    if version != HEADER_VERSION {
        return Err(Hdf5Error::Decompression(format!(
            "N-Bit: unsupported header version {}",
            version
        )));
    }

    let orig_elem_size = data[1] as usize;
    let significant_bits = data[2];
    let is_signed = data[3] != 0;
    let num_elements = LittleEndian::read_u32(&data[4..8]) as usize;
    let packed_data = &data[HEADER_SIZE..];

    if significant_bits == 0 || significant_bits > 64 {
        return Err(Hdf5Error::Decompression(format!(
            "N-Bit: invalid significant_bits {}",
            significant_bits
        )));
    }

    if is_signed {
        unpack_signed(
            packed_data,
            num_elements,
            significant_bits,
            orig_elem_size,
            datatype,
        )
    } else {
        unpack_unsigned(
            packed_data,
            num_elements,
            significant_bits,
            orig_elem_size,
            datatype,
        )
    }
}

// =============================================================================
// Signed integer packing
// =============================================================================

/// Pack signed integer data using zigzag encoding and minimum bits.
fn pack_signed(
    data: &[u8],
    datatype: &Datatype,
    num_elements: usize,
    elem_size: usize,
) -> Result<Vec<u8>> {
    // Read all values as i64, then zigzag-encode to u64
    let mut encoded_values = Vec::with_capacity(num_elements);
    let mut max_encoded: u64 = 0;

    for i in 0..num_elements {
        let offset = i * elem_size;
        let chunk = &data[offset..offset + elem_size];
        let signed_val = read_signed(chunk, datatype)?;
        let encoded = zigzag_encode(signed_val);
        max_encoded = max_encoded.max(encoded);
        encoded_values.push(encoded);
    }

    let significant_bits = min_bits_for_value(max_encoded);

    // Build output
    build_packed_output(
        &encoded_values,
        significant_bits,
        elem_size as u8,
        true,
        num_elements,
    )
}

/// Unpack signed integer data.
fn unpack_signed(
    packed_data: &[u8],
    num_elements: usize,
    significant_bits: u8,
    orig_elem_size: usize,
    datatype: &Datatype,
) -> Result<Vec<u8>> {
    let mut reader = BitReader::new(packed_data);
    let mut output = vec![0u8; num_elements * orig_elem_size];

    for i in 0..num_elements {
        let encoded = reader.read_bits(significant_bits)?;
        let signed_val = zigzag_decode(encoded);
        let offset = i * orig_elem_size;
        write_signed(
            &mut output[offset..offset + orig_elem_size],
            signed_val,
            datatype,
        )?;
    }

    Ok(output)
}

// =============================================================================
// Unsigned integer packing
// =============================================================================

/// Pack unsigned integer data using minimum bits.
fn pack_unsigned(
    data: &[u8],
    datatype: &Datatype,
    num_elements: usize,
    elem_size: usize,
) -> Result<Vec<u8>> {
    let mut values = Vec::with_capacity(num_elements);
    let mut max_val: u64 = 0;

    for i in 0..num_elements {
        let offset = i * elem_size;
        let chunk = &data[offset..offset + elem_size];
        let val = read_unsigned(chunk, datatype)?;
        max_val = max_val.max(val);
        values.push(val);
    }

    let significant_bits = min_bits_for_value(max_val);

    build_packed_output(
        &values,
        significant_bits,
        elem_size as u8,
        false,
        num_elements,
    )
}

/// Unpack unsigned integer data.
fn unpack_unsigned(
    packed_data: &[u8],
    num_elements: usize,
    significant_bits: u8,
    orig_elem_size: usize,
    datatype: &Datatype,
) -> Result<Vec<u8>> {
    let mut reader = BitReader::new(packed_data);
    let mut output = vec![0u8; num_elements * orig_elem_size];

    for i in 0..num_elements {
        let val = reader.read_bits(significant_bits)?;
        let offset = i * orig_elem_size;
        write_unsigned(&mut output[offset..offset + orig_elem_size], val, datatype)?;
    }

    Ok(output)
}

// =============================================================================
// Helper functions
// =============================================================================

/// Build the packed output with header.
fn build_packed_output(
    values: &[u64],
    significant_bits: u8,
    orig_elem_size: u8,
    is_signed: bool,
    num_elements: usize,
) -> Result<Vec<u8>> {
    let packed_bits = (num_elements as u64) * (significant_bits as u64);
    let packed_bytes = packed_bits.div_ceil(8) as usize;
    let total_size = HEADER_SIZE + packed_bytes;

    let mut output = Vec::with_capacity(total_size);

    // Write header
    output.push(HEADER_VERSION);
    output.push(orig_elem_size);
    output.push(significant_bits);
    output.push(if is_signed { 1 } else { 0 });

    let ne_u32 = u32::try_from(num_elements)
        .map_err(|_| Hdf5Error::Compression("N-Bit: num_elements exceeds u32 range".to_string()))?;
    let mut ne_bytes = [0u8; 4];
    LittleEndian::write_u32(&mut ne_bytes, ne_u32);
    output.extend_from_slice(&ne_bytes);

    // Pack values
    let mut writer = BitWriter::with_capacity(packed_bytes);
    for &val in values {
        writer.write_bits(val, significant_bits);
    }
    output.extend_from_slice(&writer.finish());

    Ok(output)
}

/// Read a signed integer from bytes based on datatype.
fn read_signed(chunk: &[u8], datatype: &Datatype) -> Result<i64> {
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
            "N-Bit: expected signed integer, got {:?}",
            datatype
        ))),
    }
}

/// Read an unsigned integer from bytes based on datatype.
fn read_unsigned(chunk: &[u8], datatype: &Datatype) -> Result<u64> {
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
            "N-Bit: expected unsigned integer, got {:?}",
            datatype
        ))),
    }
}

/// Write a signed integer to bytes based on datatype.
fn write_signed(buf: &mut [u8], value: i64, datatype: &Datatype) -> Result<()> {
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
            "N-Bit: cannot write signed to {:?}",
            datatype
        ))),
    }
}

/// Write an unsigned integer to bytes based on datatype.
fn write_unsigned(buf: &mut [u8], value: u64, datatype: &Datatype) -> Result<()> {
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
            "N-Bit: cannot write unsigned to {:?}",
            datatype
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_nbit_u16_roundtrip() {
        // 12-bit sensor data stored in u16 (values 0..4095)
        let values: Vec<u16> = (0..64).map(|i| (i * 63) as u16).collect();
        let data = make_u16_data(&values);

        let packed = apply_nbit_forward(&data, &Datatype::UInt16).expect("pack failed");

        // Should be smaller: 64 values * 12 bits = 768 bits = 96 bytes + 8 header = 104
        // vs original 64 * 2 = 128 bytes
        assert!(packed.len() < data.len());

        let unpacked = apply_nbit_reverse(&packed, &Datatype::UInt16).expect("unpack failed");
        let result = read_u16_data(&unpacked);
        assert_eq!(result, values);
    }

    #[test]
    fn test_nbit_u8_roundtrip() {
        let values: Vec<u8> = vec![0, 1, 2, 3, 4, 5, 6, 7, 0, 1, 2, 3, 4, 5, 6, 7];
        let data = values.clone();

        let packed = apply_nbit_forward(&data, &Datatype::UInt8).expect("pack failed");
        let unpacked = apply_nbit_reverse(&packed, &Datatype::UInt8).expect("unpack failed");
        assert_eq!(unpacked, values);
    }

    #[test]
    fn test_nbit_i32_signed_roundtrip() {
        let values = vec![-10i32, -5, 0, 5, 10, -3, 7, -1, 0, 3];
        let data = make_i32_data(&values);

        let packed = apply_nbit_forward(&data, &Datatype::Int32).expect("pack failed");

        // Values range from -10 to 10; zigzag: max = zigzag(10)=20, needs 5 bits
        // 10 values * 5 bits = 50 bits = 7 bytes + 8 header = 15 bytes
        // vs original 10 * 4 = 40 bytes
        assert!(packed.len() < data.len());

        let unpacked = apply_nbit_reverse(&packed, &Datatype::Int32).expect("unpack failed");
        let result = read_i32_data(&unpacked);
        assert_eq!(result, values);
    }

    #[test]
    fn test_nbit_all_zeros() {
        let values = vec![0u16; 100];
        let data = make_u16_data(&values);

        let packed = apply_nbit_forward(&data, &Datatype::UInt16).expect("pack failed");

        // All zeros: max=0, bits=1, so 100 bits = 13 bytes + 8 header = 21
        // vs original 100 * 2 = 200 bytes
        assert!(packed.len() < 30);

        let unpacked = apply_nbit_reverse(&packed, &Datatype::UInt16).expect("unpack failed");
        let result = read_u16_data(&unpacked);
        assert_eq!(result, values);
    }

    #[test]
    fn test_nbit_single_value() {
        let values = vec![12345u16];
        let data = make_u16_data(&values);

        let packed = apply_nbit_forward(&data, &Datatype::UInt16).expect("pack failed");
        let unpacked = apply_nbit_reverse(&packed, &Datatype::UInt16).expect("unpack failed");
        let result = read_u16_data(&unpacked);
        assert_eq!(result, values);
    }

    #[test]
    fn test_nbit_max_range_u16() {
        // Full 16-bit range - should not compress (or minimal overhead)
        let values: Vec<u16> = vec![0, 65535, 32768, 1, 65534];
        let data = make_u16_data(&values);

        let packed = apply_nbit_forward(&data, &Datatype::UInt16).expect("pack failed");
        let unpacked = apply_nbit_reverse(&packed, &Datatype::UInt16).expect("unpack failed");
        let result = read_u16_data(&unpacked);
        assert_eq!(result, values);
    }

    #[test]
    fn test_nbit_i32_large_negative() {
        let values = vec![-100000i32, -50000, 0, 50000, 100000];
        let data = make_i32_data(&values);

        let packed = apply_nbit_forward(&data, &Datatype::Int32).expect("pack failed");
        let unpacked = apply_nbit_reverse(&packed, &Datatype::Int32).expect("unpack failed");
        let result = read_i32_data(&unpacked);
        assert_eq!(result, values);
    }

    #[test]
    fn test_nbit_empty_data_error() {
        let data: Vec<u8> = vec![];
        let result = apply_nbit_forward(&data, &Datatype::UInt16);
        assert!(result.is_err());
    }

    #[test]
    fn test_nbit_float_error() {
        let data = vec![0u8; 16];
        let result = apply_nbit_forward(&data, &Datatype::Float32);
        assert!(result.is_err());
    }

    #[test]
    fn test_nbit_header_too_short() {
        let data = vec![0u8; 4]; // Less than HEADER_SIZE
        let result = apply_nbit_reverse(&data, &Datatype::UInt16);
        assert!(result.is_err());
    }

    #[test]
    fn test_nbit_u32_roundtrip() {
        let values: Vec<u32> = vec![100, 200, 300, 150, 250, 350, 175, 225];
        let mut data = vec![0u8; values.len() * 4];
        for (i, &v) in values.iter().enumerate() {
            LittleEndian::write_u32(&mut data[i * 4..(i + 1) * 4], v);
        }

        let packed = apply_nbit_forward(&data, &Datatype::UInt32).expect("pack failed");
        let unpacked = apply_nbit_reverse(&packed, &Datatype::UInt32).expect("unpack failed");

        let mut result = Vec::new();
        for chunk in unpacked.chunks(4) {
            if chunk.len() == 4 {
                result.push(LittleEndian::read_u32(chunk));
            }
        }
        assert_eq!(result, values);
    }

    #[test]
    fn test_nbit_i64_roundtrip() {
        let values: Vec<i64> = vec![-1000, -500, 0, 500, 1000, -250, 750];
        let mut data = vec![0u8; values.len() * 8];
        for (i, &v) in values.iter().enumerate() {
            LittleEndian::write_i64(&mut data[i * 8..(i + 1) * 8], v);
        }

        let packed = apply_nbit_forward(&data, &Datatype::Int64).expect("pack failed");
        let unpacked = apply_nbit_reverse(&packed, &Datatype::Int64).expect("unpack failed");

        let mut result = Vec::new();
        for chunk in unpacked.chunks(8) {
            if chunk.len() == 8 {
                result.push(LittleEndian::read_i64(chunk));
            }
        }
        assert_eq!(result, values);
    }
}
