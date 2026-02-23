//! Pure Rust SZIP/AEC compression filter for HDF5.
//!
//! This module implements the SZIP compression algorithm using Adaptive Entropy
//! Coding (AEC), following the CCSDS 121.0-B standard used by HDF5's SZIP filter.
//! The implementation is 100% Pure Rust with no C/Fortran dependencies.
//!
//! ## Algorithm Overview
//!
//! SZIP uses block-based Rice coding for lossless data compression:
//!
//! 1. **Preprocessing (optional)**: Apply nearest-neighbor (NN) differencing
//!    to decorrelate the data, improving compression for spatially correlated data.
//!
//! 2. **Block encoding**: Data is split into blocks of `pixels_per_block` samples.
//!    For each block, the encoder selects the most efficient coding method:
//!    - **Zero block**: All values in the block are zero (3-bit ID only)
//!    - **Rice coding**: Split each value into a quotient (unary-coded) and
//!      remainder (k fixed bits). The parameter k is chosen per-block to
//!      minimize output size.
//!    - **Raw/uncompressed**: Store values verbatim when Rice coding would expand them.
//!
//! 3. **Block ID codes** (3 bits each):
//!    - `0b000`: Zero block
//!    - `0b001` to `K`: Rice coding with parameter `k = ID - 1`
//!    - `K + 1`: Uncompressed block
//!
//! ## Options Mask
//!
//! The SZIP options mask controls encoding behavior:
//! - Bit 2 (`0x04`): EC mode (Entropy Coding) -- uses Rice coding
//! - Bit 5 (`0x20`): NN mode (Nearest Neighbor preprocessing)
//! - Bit 7 (`0x80`): RAW mode (no preprocessing)
//!
//! ## Stream Format
//!
//! | Offset | Size | Field            | Description                          |
//! |--------|------|------------------|--------------------------------------|
//! | 0      | 4    | options_mask     | Encoding options (u32 LE)            |
//! | 4      | 4    | pixels_per_block | Samples per block (u32 LE)           |
//! | 8      | 4    | bits_per_pixel   | Bits per sample (u32 LE)             |
//! | 12     | 4    | num_pixels       | Total number of samples (u32 LE)     |
//! | 16     | var  | encoded_data     | Block-encoded bit stream             |

use crate::datatype::Datatype;
use crate::error::{Hdf5Error, Result};
use byteorder::{ByteOrder, LittleEndian};

use super::bitpack::{BitReader, BitWriter};

/// SZIP stream header size in bytes
const HEADER_SIZE: usize = 16;

/// SZIP option: Entropy Coding mode (Rice coding)
const SZIP_EC_OPTION_MASK: u32 = 0x04;

/// SZIP option: Nearest Neighbor preprocessing
const SZIP_NN_OPTION_MASK: u32 = 0x20;

/// Maximum number of Rice coding options (determines number of block ID codes)
/// K = ceil(log2(bits_per_pixel)) + 1, but we cap at a reasonable maximum
const MAX_RICE_K: u8 = 32;

/// Apply SZIP compression in the forward direction.
///
/// # Arguments
/// * `data` - Raw byte data
/// * `params` - Filter parameters: `[options_mask, pixels_per_block]`
/// * `datatype` - The HDF5 datatype (used to determine bits per pixel)
pub fn apply_szip_forward(data: &[u8], params: &[u32], datatype: &Datatype) -> Result<Vec<u8>> {
    let options_mask = params.first().copied().unwrap_or(SZIP_EC_OPTION_MASK);
    let pixels_per_block = params.get(1).copied().unwrap_or(16);

    if pixels_per_block == 0 || pixels_per_block > 32 {
        return Err(Hdf5Error::Compression(format!(
            "SZIP: pixels_per_block must be 1..=32, got {}",
            pixels_per_block
        )));
    }

    // Determine bits per pixel from the datatype
    let bytes_per_pixel = datatype.size();
    let bits_per_pixel = (bytes_per_pixel * 8) as u8;

    if data.is_empty() || data.len() % bytes_per_pixel != 0 {
        return Err(Hdf5Error::Compression(format!(
            "SZIP: data length {} not divisible by element size {}",
            data.len(),
            bytes_per_pixel
        )));
    }

    let num_pixels = data.len() / bytes_per_pixel;
    if num_pixels == 0 {
        return Err(Hdf5Error::Compression(
            "SZIP: no pixels in data".to_string(),
        ));
    }

    // Read all pixel values as unsigned integers
    let mut pixels = Vec::with_capacity(num_pixels);
    for i in 0..num_pixels {
        let offset = i * bytes_per_pixel;
        let val = read_pixel(&data[offset..], bytes_per_pixel)?;
        pixels.push(val);
    }

    // Apply NN preprocessing if requested
    let use_nn = (options_mask & SZIP_NN_OPTION_MASK) != 0;
    let preprocessed = if use_nn {
        apply_nn_forward(&pixels)
    } else {
        pixels.clone()
    };

    // Encode blocks using Rice coding
    let ppb = pixels_per_block as usize;
    let max_k = compute_max_k(bits_per_pixel);
    let mut writer = BitWriter::with_capacity(data.len());

    let num_blocks = preprocessed.len().div_ceil(ppb);
    for block_idx in 0..num_blocks {
        let start = block_idx * ppb;
        let end = (start + ppb).min(preprocessed.len());
        let block = &preprocessed[start..end];
        encode_block(&mut writer, block, bits_per_pixel, max_k);
    }

    // Build output with header
    let encoded_data = writer.finish();
    let mut output = Vec::with_capacity(HEADER_SIZE + encoded_data.len());

    let mut header = [0u8; HEADER_SIZE];
    LittleEndian::write_u32(&mut header[0..4], options_mask);
    LittleEndian::write_u32(&mut header[4..8], pixels_per_block);
    LittleEndian::write_u32(&mut header[8..12], bits_per_pixel as u32);
    let num_pixels_u32 = u32::try_from(num_pixels)
        .map_err(|_| Hdf5Error::Compression("SZIP: num_pixels exceeds u32 range".to_string()))?;
    LittleEndian::write_u32(&mut header[12..16], num_pixels_u32);
    output.extend_from_slice(&header);
    output.extend_from_slice(&encoded_data);

    Ok(output)
}

/// Apply SZIP decompression in the reverse direction.
///
/// The header contains all information needed for decompression (bits_per_pixel,
/// num_pixels, options_mask), so only the compressed data buffer is needed.
///
/// # Arguments
/// * `data` - Compressed byte data (header + encoded blocks)
/// * `params` - Filter parameters (not used; all info is in the header)
pub fn apply_szip_reverse(data: &[u8], _params: &[u32]) -> Result<Vec<u8>> {
    if data.len() < HEADER_SIZE {
        return Err(Hdf5Error::Decompression(
            "SZIP: data too short for header".to_string(),
        ));
    }

    let options_mask = LittleEndian::read_u32(&data[0..4]);
    let pixels_per_block = LittleEndian::read_u32(&data[4..8]) as usize;
    let bits_per_pixel = LittleEndian::read_u32(&data[8..12]) as u8;
    let num_pixels = LittleEndian::read_u32(&data[12..16]) as usize;
    let encoded_data = &data[HEADER_SIZE..];

    if pixels_per_block == 0 {
        return Err(Hdf5Error::Decompression(
            "SZIP: pixels_per_block is zero".to_string(),
        ));
    }

    let max_k = compute_max_k(bits_per_pixel);
    let mut reader = BitReader::new(encoded_data);
    let mut decoded_pixels = Vec::with_capacity(num_pixels);

    let num_blocks = num_pixels.div_ceil(pixels_per_block);
    for _block_idx in 0..num_blocks {
        let remaining = num_pixels - decoded_pixels.len();
        let block_size = remaining.min(pixels_per_block);
        let block = decode_block(&mut reader, block_size, bits_per_pixel, max_k)?;
        decoded_pixels.extend_from_slice(&block);
    }

    // Undo NN preprocessing if it was applied
    let use_nn = (options_mask & SZIP_NN_OPTION_MASK) != 0;
    let final_pixels = if use_nn {
        apply_nn_reverse(&decoded_pixels)
    } else {
        decoded_pixels
    };

    // Write pixels to output bytes
    let bytes_per_pixel = bits_per_pixel.div_ceil(8) as usize;
    let mut output = vec![0u8; num_pixels * bytes_per_pixel];
    for (i, &val) in final_pixels.iter().enumerate() {
        let offset = i * bytes_per_pixel;
        write_pixel(&mut output[offset..], val, bytes_per_pixel)?;
    }

    Ok(output)
}

// =============================================================================
// Block encoding
// =============================================================================

/// Encode a single block of samples.
///
/// Selects the best coding method (zero block, Rice with optimal k, or raw)
/// and writes the block ID followed by the coded data.
fn encode_block(writer: &mut BitWriter, block: &[u64], bits_per_pixel: u8, max_k: u8) {
    // Check for zero block
    if block.iter().all(|&v| v == 0) {
        // ID 0: zero block
        writer.write_bits(0, 3);
        return;
    }

    // Try each Rice parameter and pick the smallest output
    let mut best_k: Option<u8> = None;
    let mut best_cost = usize::MAX;

    for k in 0..=max_k.min(MAX_RICE_K) {
        let cost = estimate_rice_cost(block, k, bits_per_pixel);
        if cost < best_cost {
            best_cost = cost;
            best_k = Some(k);
        }
    }

    // Cost of raw encoding: block_size * bits_per_pixel + 3 (ID)
    let raw_cost = block.len() * (bits_per_pixel as usize) + 3;

    if best_cost < raw_cost {
        if let Some(k) = best_k {
            // ID = k + 1 (IDs 1..=max_k+1 are Rice with parameter 0..=max_k)
            let id = (k as u64) + 1;
            writer.write_bits(id, 3);
            for &val in block {
                writer.write_rice(val, k);
            }
        } else {
            // Fallback to raw
            encode_raw_block(writer, block, bits_per_pixel, max_k);
        }
    } else {
        encode_raw_block(writer, block, bits_per_pixel, max_k);
    }
}

/// Encode a raw (uncompressed) block.
fn encode_raw_block(writer: &mut BitWriter, block: &[u64], bits_per_pixel: u8, max_k: u8) {
    // ID for raw = max_k + 2
    let id = (max_k as u64) + 2;
    // Clamp to 3 bits
    let id = id.min(7);
    writer.write_bits(id, 3);
    for &val in block {
        writer.write_bits(val, bits_per_pixel);
    }
}

/// Estimate the total bit cost of Rice-coding a block with parameter k.
///
/// For each value v, Rice coding uses `(v >> k) + 1 + k` bits.
/// We cap the unary part to avoid degenerate cases.
fn estimate_rice_cost(block: &[u64], k: u8, bits_per_pixel: u8) -> usize {
    let max_unary = (bits_per_pixel as usize) * 4; // Cap unary length
    let mut total_bits = 3; // 3-bit block ID

    for &val in block {
        let quotient = (val >> k) as usize;
        if quotient > max_unary {
            // This k is too small; penalize heavily
            return usize::MAX;
        }
        total_bits += quotient + 1 + (k as usize);
    }

    total_bits
}

// =============================================================================
// Block decoding
// =============================================================================

/// Decode a single block of samples.
fn decode_block(
    reader: &mut BitReader<'_>,
    block_size: usize,
    bits_per_pixel: u8,
    max_k: u8,
) -> Result<Vec<u64>> {
    let id = reader.read_bits(3)? as u8;

    if id == 0 {
        // Zero block
        return Ok(vec![0u64; block_size]);
    }

    let raw_id = (max_k + 2).min(7);

    if id == raw_id {
        // Raw block
        let mut block = Vec::with_capacity(block_size);
        for _ in 0..block_size {
            let val = reader.read_bits(bits_per_pixel)?;
            block.push(val);
        }
        return Ok(block);
    }

    // Rice coding with parameter k = id - 1
    let k = id - 1;
    let mut block = Vec::with_capacity(block_size);
    for _ in 0..block_size {
        let val = reader.read_rice(k)?;
        block.push(val);
    }

    Ok(block)
}

// =============================================================================
// Nearest-Neighbor preprocessing
// =============================================================================

/// Apply NN (Nearest Neighbor) preprocessing in the forward direction.
///
/// Computes differences between consecutive samples. The first sample
/// is stored as-is. Differences are mapped to unsigned via zigzag encoding.
fn apply_nn_forward(pixels: &[u64]) -> Vec<u64> {
    if pixels.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::with_capacity(pixels.len());
    // First pixel stored as-is
    result.push(pixels[0]);

    for i in 1..pixels.len() {
        let diff = pixels[i] as i64 - pixels[i - 1] as i64;
        // Map signed difference to unsigned using zigzag encoding
        let encoded = super::bitpack::zigzag_encode(diff);
        result.push(encoded);
    }

    result
}

/// Apply NN preprocessing in the reverse direction.
///
/// Reverses the differencing to reconstruct original pixel values.
fn apply_nn_reverse(preprocessed: &[u64]) -> Vec<u64> {
    if preprocessed.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::with_capacity(preprocessed.len());
    result.push(preprocessed[0]);

    for i in 1..preprocessed.len() {
        let diff = super::bitpack::zigzag_decode(preprocessed[i]);
        let prev = result[i - 1] as i64;
        let val = prev.wrapping_add(diff);
        result.push(val as u64);
    }

    result
}

// =============================================================================
// Helper functions
// =============================================================================

/// Compute the maximum Rice parameter K for the given bits_per_pixel.
///
/// K is typically `ceil(log2(bits_per_pixel)) + 1`, capped to fit in 3-bit IDs.
fn compute_max_k(bits_per_pixel: u8) -> u8 {
    if bits_per_pixel == 0 {
        return 0;
    }
    // log2(bits_per_pixel) rounded up, plus 1
    let log2_bpp = (bits_per_pixel as f32).log2().ceil() as u8;
    // IDs 1..=(max_k+1) for Rice, ID max_k+2 for raw, must fit in 3 bits (0..7)
    // So max_k+2 <= 7, max_k <= 5
    let k = log2_bpp.saturating_add(1);
    k.min(5)
}

/// Read a pixel value from bytes.
fn read_pixel(data: &[u8], bytes_per_pixel: usize) -> Result<u64> {
    match bytes_per_pixel {
        1 => {
            if data.is_empty() {
                return Err(Hdf5Error::Compression("SZIP: empty pixel data".to_string()));
            }
            Ok(data[0] as u64)
        }
        2 => {
            if data.len() < 2 {
                return Err(Hdf5Error::Compression(
                    "SZIP: insufficient pixel data for 2-byte sample".to_string(),
                ));
            }
            Ok(LittleEndian::read_u16(data) as u64)
        }
        4 => {
            if data.len() < 4 {
                return Err(Hdf5Error::Compression(
                    "SZIP: insufficient pixel data for 4-byte sample".to_string(),
                ));
            }
            Ok(LittleEndian::read_u32(data) as u64)
        }
        8 => {
            if data.len() < 8 {
                return Err(Hdf5Error::Compression(
                    "SZIP: insufficient pixel data for 8-byte sample".to_string(),
                ));
            }
            Ok(LittleEndian::read_u64(data))
        }
        _ => Err(Hdf5Error::Compression(format!(
            "SZIP: unsupported bytes_per_pixel {}",
            bytes_per_pixel
        ))),
    }
}

/// Write a pixel value to bytes.
fn write_pixel(buf: &mut [u8], value: u64, bytes_per_pixel: usize) -> Result<()> {
    match bytes_per_pixel {
        1 => {
            if buf.is_empty() {
                return Err(Hdf5Error::Decompression(
                    "SZIP: empty pixel buffer".to_string(),
                ));
            }
            buf[0] = value as u8;
            Ok(())
        }
        2 => {
            if buf.len() < 2 {
                return Err(Hdf5Error::Decompression(
                    "SZIP: insufficient pixel buffer for 2-byte sample".to_string(),
                ));
            }
            LittleEndian::write_u16(buf, value as u16);
            Ok(())
        }
        4 => {
            if buf.len() < 4 {
                return Err(Hdf5Error::Decompression(
                    "SZIP: insufficient pixel buffer for 4-byte sample".to_string(),
                ));
            }
            LittleEndian::write_u32(buf, value as u32);
            Ok(())
        }
        8 => {
            if buf.len() < 8 {
                return Err(Hdf5Error::Decompression(
                    "SZIP: insufficient pixel buffer for 8-byte sample".to_string(),
                ));
            }
            LittleEndian::write_u64(buf, value);
            Ok(())
        }
        _ => Err(Hdf5Error::Decompression(format!(
            "SZIP: unsupported bytes_per_pixel {}",
            bytes_per_pixel
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

    fn make_u8_data(values: &[u8]) -> Vec<u8> {
        values.to_vec()
    }

    #[test]
    fn test_szip_u8_roundtrip_ec() {
        let values: Vec<u8> = (0..64).collect();
        let data = make_u8_data(&values);
        let params = [SZIP_EC_OPTION_MASK, 16]; // EC mode, 16 pixels per block

        let compressed =
            apply_szip_forward(&data, &params, &Datatype::UInt8).expect("compress failed");
        let decompressed = apply_szip_reverse(&compressed, &params).expect("decompress failed");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_szip_u8_roundtrip_nn() {
        // Slowly varying data benefits from NN preprocessing
        let values: Vec<u8> = (0..64).map(|i| (100 + i / 4) as u8).collect();
        let data = make_u8_data(&values);
        let params = [SZIP_EC_OPTION_MASK | SZIP_NN_OPTION_MASK, 16]; // EC+NN, 16 ppb

        let compressed =
            apply_szip_forward(&data, &params, &Datatype::UInt8).expect("compress failed");
        let decompressed = apply_szip_reverse(&compressed, &params).expect("decompress failed");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_szip_u16_roundtrip() {
        let values: Vec<u16> = (0..32).map(|i| 1000 + i * 10).collect();
        let data = make_u16_data(&values);
        let params = [SZIP_EC_OPTION_MASK, 8]; // EC mode, 8 ppb

        let compressed =
            apply_szip_forward(&data, &params, &Datatype::UInt16).expect("compress failed");
        let decompressed = apply_szip_reverse(&compressed, &params).expect("decompress failed");
        let result = read_u16_data(&decompressed);
        assert_eq!(result, values);
    }

    #[test]
    fn test_szip_u32_roundtrip() {
        let values: Vec<u32> = (0..32).map(|i| 50000 + i * 100).collect();
        let mut data = vec![0u8; values.len() * 4];
        for (i, &v) in values.iter().enumerate() {
            LittleEndian::write_u32(&mut data[i * 4..(i + 1) * 4], v);
        }
        let params = [SZIP_EC_OPTION_MASK, 8];

        let compressed =
            apply_szip_forward(&data, &params, &Datatype::UInt32).expect("compress failed");
        let decompressed = apply_szip_reverse(&compressed, &params).expect("decompress failed");

        let mut result = Vec::new();
        for chunk in decompressed.chunks(4) {
            if chunk.len() == 4 {
                result.push(LittleEndian::read_u32(chunk));
            }
        }
        assert_eq!(result, values);
    }

    #[test]
    fn test_szip_constant_data() {
        // All same values - should compress well (zero blocks after NN)
        let values: Vec<u8> = vec![42; 64];
        let data = make_u8_data(&values);
        let params = [SZIP_EC_OPTION_MASK | SZIP_NN_OPTION_MASK, 16];

        let compressed =
            apply_szip_forward(&data, &params, &Datatype::UInt8).expect("compress failed");
        // With NN, all diffs are 0, so should compress very well
        assert!(compressed.len() < data.len());

        let decompressed = apply_szip_reverse(&compressed, &params).expect("decompress failed");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_szip_zeros() {
        let values: Vec<u8> = vec![0; 32];
        let data = make_u8_data(&values);
        let params = [SZIP_EC_OPTION_MASK, 8];

        let compressed =
            apply_szip_forward(&data, &params, &Datatype::UInt8).expect("compress failed");
        let decompressed = apply_szip_reverse(&compressed, &params).expect("decompress failed");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_szip_single_block() {
        let values: Vec<u8> = vec![10, 20, 30, 40, 50, 60, 70, 80];
        let data = make_u8_data(&values);
        let params = [SZIP_EC_OPTION_MASK, 8]; // Exactly one block

        let compressed =
            apply_szip_forward(&data, &params, &Datatype::UInt8).expect("compress failed");
        let decompressed = apply_szip_reverse(&compressed, &params).expect("decompress failed");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_szip_partial_last_block() {
        // 10 values with ppb=8 means 1 full block + 1 partial block of 2
        let values: Vec<u8> = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let data = make_u8_data(&values);
        let params = [SZIP_EC_OPTION_MASK, 8];

        let compressed =
            apply_szip_forward(&data, &params, &Datatype::UInt8).expect("compress failed");
        let decompressed = apply_szip_reverse(&compressed, &params).expect("decompress failed");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_szip_invalid_ppb() {
        let data = vec![1u8; 32];
        let params = [SZIP_EC_OPTION_MASK, 0]; // Invalid: 0 ppb
        let result = apply_szip_forward(&data, &params, &Datatype::UInt8);
        assert!(result.is_err());

        let params = [SZIP_EC_OPTION_MASK, 64]; // Invalid: >32 ppb
        let result = apply_szip_forward(&data, &params, &Datatype::UInt8);
        assert!(result.is_err());
    }

    #[test]
    fn test_szip_header_too_short() {
        let data = vec![0u8; 10]; // Less than HEADER_SIZE
        let result = apply_szip_reverse(&data, &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_nn_preprocessing_roundtrip() {
        let pixels: Vec<u64> = vec![100, 102, 104, 103, 105, 108, 107, 110];
        let preprocessed = apply_nn_forward(&pixels);
        let recovered = apply_nn_reverse(&preprocessed);
        assert_eq!(recovered, pixels);
    }

    #[test]
    fn test_nn_constant_data() {
        let pixels: Vec<u64> = vec![50; 10];
        let preprocessed = apply_nn_forward(&pixels);
        // First value preserved, rest should be 0 (zigzag(0) = 0)
        assert_eq!(preprocessed[0], 50);
        for &v in &preprocessed[1..] {
            assert_eq!(v, 0);
        }
        let recovered = apply_nn_reverse(&preprocessed);
        assert_eq!(recovered, pixels);
    }

    #[test]
    fn test_compute_max_k() {
        assert_eq!(compute_max_k(8), 4); // log2(8)=3, +1=4
        assert_eq!(compute_max_k(16), 5); // log2(16)=4, +1=5
        assert_eq!(compute_max_k(32), 5); // log2(32)=5, +1=6, capped to 5
        assert_eq!(compute_max_k(64), 5); // capped to 5
        assert_eq!(compute_max_k(1), 1); // log2(1)=0, +1=1
    }

    #[test]
    fn test_szip_u16_nn_roundtrip() {
        // Slowly varying 16-bit data
        let values: Vec<u16> = (0..32).map(|i| 5000 + (i as u16) * 3).collect();
        let data = make_u16_data(&values);
        let params = [SZIP_EC_OPTION_MASK | SZIP_NN_OPTION_MASK, 8];

        let compressed =
            apply_szip_forward(&data, &params, &Datatype::UInt16).expect("compress failed");
        let decompressed = apply_szip_reverse(&compressed, &params).expect("decompress failed");
        let result = read_u16_data(&decompressed);
        assert_eq!(result, values);
    }

    #[test]
    fn test_szip_large_values() {
        // Test with values that span the full 8-bit range
        let values: Vec<u8> = (0..=255).collect();
        let data = make_u8_data(&values);
        let params = [SZIP_EC_OPTION_MASK, 16];

        let compressed =
            apply_szip_forward(&data, &params, &Datatype::UInt8).expect("compress failed");
        let decompressed = apply_szip_reverse(&compressed, &params).expect("decompress failed");
        assert_eq!(decompressed, data);
    }
}
