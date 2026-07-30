//! TIFF predictor (tag 317) encoding and decoding.
//!
//! Two predictors are defined by TIFF 6.0 / TIFF Technical Note 3:
//!
//! - **Horizontal differencing** (`Predictor::HorizontalDifferencing`, value 2) —
//!   each sample is stored as the difference from the sample `SamplesPerPixel`
//!   positions to its left, using the file's declared byte order and
//!   carry-propagating arithmetic across the whole sample.
//! - **Floating point** (`Predictor::FloatingPoint`, value 3) — each scanline is
//!   byte-plane transposed (most-significant byte plane first) and then a
//!   *byte-wise* horizontal delta with stride `SamplesPerPixel` is applied to the
//!   transposed stream (libtiff's `fpDiff`/`fpAcc`).
//!
//! Split out of [`super`] so both the codec dispatch and the predictor stay
//! comfortably inside the 2000-line-per-file limit.
//!
//! # Performance
//!
//! The floating-point decoder used to allocate a fresh scanline buffer *inside*
//! the per-row loop (one `Vec` per scanline — 262 144 allocations for a single
//! 8000x8000 Float32 band tiled 256x256). The scratch buffer is now hoisted out
//! of the row loop and reused, and the byte-plane de-interleave is specialised
//! on the sample width (2/4/8 bytes) so the inner loop is a straight copy
//! instead of a per-byte `match` on the byte order plus a multiply. Both
//! directions remain bit-for-bit identical to the previous implementation; see
//! the `test_issue_14_float_predictor_*` regression tests.

use oxigeo_core::error::{CompressionError, OxiGeoError, Result};

use crate::tiff::{ByteOrderType, Predictor};

/// Reads a `bytes_per_sample`-wide unsigned sample from `bytes` using `byte_order`.
///
/// Falls back to a single-byte read for widths other than 1/2/4/8 (never panics).
fn read_sample(bytes: &[u8], bytes_per_sample: usize, byte_order: ByteOrderType) -> u64 {
    match bytes_per_sample {
        2 if bytes.len() >= 2 => u64::from(byte_order.read_u16(bytes)),
        4 if bytes.len() >= 4 => u64::from(byte_order.read_u32(bytes)),
        8 if bytes.len() >= 8 => byte_order.read_u64(bytes),
        _ => bytes.first().copied().map_or(0, u64::from),
    }
}

/// Writes `value` back as a `bytes_per_sample`-wide unsigned sample using `byte_order`.
///
/// Falls back to a single-byte write for widths other than 1/2/4/8 (never panics).
fn write_sample(bytes: &mut [u8], bytes_per_sample: usize, byte_order: ByteOrderType, value: u64) {
    match bytes_per_sample {
        2 if bytes.len() >= 2 => byte_order.write_u16(bytes, value as u16),
        4 if bytes.len() >= 4 => byte_order.write_u32(bytes, value as u32),
        8 if bytes.len() >= 8 => byte_order.write_u64(bytes, value),
        _ => {
            if let Some(b) = bytes.first_mut() {
                *b = value as u8;
            }
        }
    }
}

/// Reconstructs original sample values from stored horizontal deltas (decode direction).
///
/// Per the TIFF 6.0 spec, horizontal differencing operates on whole samples in the
/// file's declared byte order, not on individual bytes: for samples wider than 8 bits
/// this requires carry-propagating addition across the low/high bytes of each sample,
/// not independent per-byte wraparound (which silently drops carries and corrupts data).
fn undifference_row(
    row: &mut [u8],
    bytes_per_sample: usize,
    samples_per_pixel: usize,
    byte_order: ByteOrderType,
) {
    if bytes_per_sample == 0 || samples_per_pixel == 0 {
        return;
    }
    let sample_count = row.len() / bytes_per_sample;
    for j in samples_per_pixel..sample_count {
        let cur_off = j * bytes_per_sample;
        let prev_off = (j - samples_per_pixel) * bytes_per_sample;
        let prev = read_sample(&row[prev_off..], bytes_per_sample, byte_order);
        let delta = read_sample(&row[cur_off..], bytes_per_sample, byte_order);
        write_sample(
            &mut row[cur_off..cur_off + bytes_per_sample],
            bytes_per_sample,
            byte_order,
            prev.wrapping_add(delta),
        );
    }
}

/// Encodes sample deltas from original values (encode direction).
///
/// Processes samples from right to left so that the "previous pixel" value read for
/// each delta is always the still-original (undifferenced) sample. See
/// [`undifference_row`] for why this must operate on whole samples, not bytes.
fn difference_row(
    row: &mut [u8],
    bytes_per_sample: usize,
    samples_per_pixel: usize,
    byte_order: ByteOrderType,
) {
    if bytes_per_sample == 0 || samples_per_pixel == 0 {
        return;
    }
    let sample_count = row.len() / bytes_per_sample;
    for j in (samples_per_pixel..sample_count).rev() {
        let cur_off = j * bytes_per_sample;
        let prev_off = (j - samples_per_pixel) * bytes_per_sample;
        let cur = read_sample(&row[cur_off..], bytes_per_sample, byte_order);
        let prev = read_sample(&row[prev_off..], bytes_per_sample, byte_order);
        write_sample(
            &mut row[cur_off..cur_off + bytes_per_sample],
            bytes_per_sample,
            byte_order,
            cur.wrapping_sub(prev),
        );
    }
}

/// Undoes a byte-wise horizontal delta with the given `stride` (running sum).
///
/// Equivalent to `for i in stride..row.len() { row[i] += row[i - stride] }` with
/// wrapping arithmetic; the `stride == 1` case (single-band imagery, by far the
/// most common) is rewritten as a single sequential pass with the running value
/// held in a register instead of re-loaded from memory on every iteration.
fn accumulate_bytes(row: &mut [u8], stride: usize) {
    if stride == 0 {
        return;
    }
    if stride == 1 {
        let Some((first, rest)) = row.split_first_mut() else {
            return;
        };
        let mut acc = *first;
        for byte in rest {
            acc = acc.wrapping_add(*byte);
            *byte = acc;
        }
        return;
    }
    for i in stride..row.len() {
        row[i] = row[i].wrapping_add(row[i - stride]);
    }
}

/// Applies a byte-wise horizontal delta with the given `stride` (encode direction).
///
/// Equivalent to the back-to-front loop
/// `for i in (stride..len).rev() { row[i] -= row[i - stride] }`: every output is a
/// function of the *original* bytes only, so for `stride == 1` the same result is
/// produced by a forward pass that carries the previous original byte in a register.
fn differentiate_bytes(row: &mut [u8], stride: usize) {
    if stride == 0 {
        return;
    }
    if stride == 1 {
        let mut prev = match row.first() {
            Some(&b) => b,
            None => return,
        };
        for byte in row.iter_mut().skip(1) {
            let cur = *byte;
            *byte = cur.wrapping_sub(prev);
            prev = cur;
        }
        return;
    }
    for i in (stride..row.len()).rev() {
        row[i] = row[i].wrapping_sub(row[i - stride]);
    }
}

/// Byte-plane index that holds byte `byte` of every sample.
///
/// Plane 0 always holds the most-significant byte of every sample, regardless of
/// the file's byte order; only the reassembly into native sample layout depends
/// on `byte_order`.
const fn plane_of_byte(byte: usize, bytes_per_sample: usize, byte_order: ByteOrderType) -> usize {
    match byte_order {
        ByteOrderType::BigEndian => byte,
        ByteOrderType::LittleEndian => bytes_per_sample - byte - 1,
    }
}

/// De-interleaves `planes` (byte-plane layout, most-significant plane first) back
/// into sample layout in `row`, for a sample width known at compile time.
///
/// `planes` and `row` must both be exactly `N * sample_count` bytes; the function
/// is a no-op otherwise (callers guarantee this).
fn undo_byte_planes<const N: usize>(
    row: &mut [u8],
    planes: &[u8],
    sample_count: usize,
    byte_order: ByteOrderType,
) {
    if planes.len() != N * sample_count || row.len() != planes.len() {
        return;
    }
    // `ordered[b]` is the plane supplying byte `b` of every sample.
    let ordered: [&[u8]; N] = core::array::from_fn(|byte| {
        let plane = plane_of_byte(byte, N, byte_order);
        &planes[plane * sample_count..(plane + 1) * sample_count]
    });
    for (i, chunk) in row.chunks_exact_mut(N).enumerate() {
        for (dst, plane) in chunk.iter_mut().zip(ordered) {
            *dst = plane[i];
        }
    }
}

/// Sample-width-generic fallback for [`undo_byte_planes`] (widths other than 2/4/8).
fn undo_byte_planes_generic(
    row: &mut [u8],
    planes: &[u8],
    bytes_per_sample: usize,
    sample_count: usize,
    byte_order: ByteOrderType,
) {
    for sample in 0..sample_count {
        for byte in 0..bytes_per_sample {
            let plane = plane_of_byte(byte, bytes_per_sample, byte_order);
            row[bytes_per_sample * sample + byte] = planes[plane * sample_count + sample];
        }
    }
}

/// Interleaves `samples` (sample layout) into byte-plane layout in `row`
/// (most-significant plane first), for a sample width known at compile time.
///
/// `samples` and `row` must both be exactly `N * sample_count` bytes; the function
/// is a no-op otherwise (callers guarantee this).
fn apply_byte_planes<const N: usize>(
    row: &mut [u8],
    samples: &[u8],
    sample_count: usize,
    byte_order: ByteOrderType,
) {
    if samples.len() != N * sample_count || row.len() != samples.len() || sample_count == 0 {
        return;
    }
    let mut chunks = row.chunks_exact_mut(sample_count);
    // Planes in on-disk order (plane 0 = most-significant byte plane).
    let mut ordered: [&mut [u8]; N] = core::array::from_fn(|_| match chunks.next() {
        Some(plane) => plane,
        None => &mut [],
    });
    // Reorder so `ordered[b]` is the plane that receives byte `b` of each sample.
    if matches!(byte_order, ByteOrderType::LittleEndian) {
        ordered.reverse();
    }
    for (i, chunk) in samples.chunks_exact(N).enumerate() {
        for (src, plane) in chunk.iter().zip(ordered.iter_mut()) {
            plane[i] = *src;
        }
    }
}

/// Sample-width-generic fallback for [`apply_byte_planes`] (widths other than 2/4/8).
fn apply_byte_planes_generic(
    row: &mut [u8],
    samples: &[u8],
    bytes_per_sample: usize,
    sample_count: usize,
    byte_order: ByteOrderType,
) {
    for sample in 0..sample_count {
        for byte in 0..bytes_per_sample {
            let plane = plane_of_byte(byte, bytes_per_sample, byte_order);
            row[plane * sample_count + sample] = samples[bytes_per_sample * sample + byte];
        }
    }
}

/// Validates a floating-point-predictor scanline and returns its sample count.
fn float_row_sample_count(
    row_len: usize,
    bytes_per_sample: usize,
    encoding: bool,
) -> Result<Option<usize>> {
    if bytes_per_sample == 0 || row_len == 0 {
        return Ok(None);
    }
    if !row_len.is_multiple_of(bytes_per_sample) {
        let message = format!(
            "Floating-point predictor: scanline length {row_len} is not a multiple of \
             sample size {bytes_per_sample}"
        );
        return Err(OxiGeoError::Compression(if encoding {
            CompressionError::CompressionFailed { message }
        } else {
            CompressionError::DecompressionFailed { message }
        }));
    }
    Ok(Some(row_len / bytes_per_sample))
}

/// Reconstructs one scanline that was encoded with the TIFF 6.0 floating-point
/// predictor (Predictor tag = 3).
///
/// The on-disk layout produced by the floating-point predictor is, per scanline:
/// 1. a *byte-plane transpose* — the most-significant byte of every sample first
///    (grouped across the whole row), then the next byte plane, and so on down to
///    the least-significant byte plane; and
/// 2. a *byte-wise horizontal delta* applied to that transposed stream with a
///    stride equal to `samples_per_pixel` (matching libtiff's `fpAcc`/`fpDiff`).
///
/// Decoding therefore first undoes the byte-wise delta, then undoes the transpose,
/// reassembling each sample in the file's declared `byte_order` so that downstream
/// sample readers interpret the bytes correctly. The plane order on disk is always
/// most-significant-byte-first regardless of `byte_order`; only the reassembly step
/// depends on the byte order.
///
/// `scratch` is a caller-owned buffer reused across scanlines; its contents on entry
/// are irrelevant and it is left holding the pre-transpose bytes on exit.
fn undo_float_predictor_row(
    row: &mut [u8],
    bytes_per_sample: usize,
    samples_per_pixel: usize,
    byte_order: ByteOrderType,
    scratch: &mut Vec<u8>,
) -> Result<()> {
    let Some(sample_count) = float_row_sample_count(row.len(), bytes_per_sample, false)? else {
        return Ok(());
    };

    // Step 1: undo the byte-wise horizontal delta (running sum, stride = spp).
    accumulate_bytes(row, samples_per_pixel.max(1));

    // Step 2: undo the byte-plane transpose, reassembling samples in `byte_order`.
    scratch.clear();
    scratch.extend_from_slice(row);
    match bytes_per_sample {
        2 => undo_byte_planes::<2>(row, scratch, sample_count, byte_order),
        4 => undo_byte_planes::<4>(row, scratch, sample_count, byte_order),
        8 => undo_byte_planes::<8>(row, scratch, sample_count, byte_order),
        _ => undo_byte_planes_generic(row, scratch, bytes_per_sample, sample_count, byte_order),
    }
    Ok(())
}

/// Encodes one scanline with the TIFF 6.0 floating-point predictor (Predictor
/// tag = 3). This is the exact inverse of [`undo_float_predictor_row`]: it first
/// performs the byte-plane transpose (most-significant byte plane first) and then
/// applies the byte-wise horizontal delta with stride `samples_per_pixel`.
///
/// `scratch` is a caller-owned buffer reused across scanlines (see
/// [`undo_float_predictor_row`]).
fn apply_float_predictor_row(
    row: &mut [u8],
    bytes_per_sample: usize,
    samples_per_pixel: usize,
    byte_order: ByteOrderType,
    scratch: &mut Vec<u8>,
) -> Result<()> {
    let Some(sample_count) = float_row_sample_count(row.len(), bytes_per_sample, true)? else {
        return Ok(());
    };

    // Step 1: byte-plane transpose (most-significant byte plane first).
    scratch.clear();
    scratch.extend_from_slice(row);
    match bytes_per_sample {
        2 => apply_byte_planes::<2>(row, scratch, sample_count, byte_order),
        4 => apply_byte_planes::<4>(row, scratch, sample_count, byte_order),
        8 => apply_byte_planes::<8>(row, scratch, sample_count, byte_order),
        _ => apply_byte_planes_generic(row, scratch, bytes_per_sample, sample_count, byte_order),
    }

    // Step 2: byte-wise horizontal delta (stride = spp).
    differentiate_bytes(row, samples_per_pixel.max(1));
    Ok(())
}

/// Iterates each scanline of `data` and applies `op` to it.
fn for_each_row<F>(
    data: &mut [u8],
    bytes_per_sample: usize,
    samples_per_pixel: usize,
    width: usize,
    mut op: F,
) -> Result<()>
where
    F: FnMut(&mut [u8]) -> Result<()>,
{
    let row_bytes = width
        .saturating_mul(samples_per_pixel)
        .saturating_mul(bytes_per_sample);
    if row_bytes == 0 {
        return Ok(());
    }
    for row_start in (0..data.len()).step_by(row_bytes) {
        let row_end = (row_start + row_bytes).min(data.len());
        op(&mut data[row_start..row_end])?;
    }
    Ok(())
}

/// Applies a predictor in the reverse (decode) direction.
///
/// `byte_order` must match the byte order the samples are stored in on disk (i.e. the
/// TIFF file's own byte order for reads), since multi-byte samples must be reassembled
/// with carry-propagating arithmetic (horizontal differencing) or byte-plane transposed
/// (floating-point) rather than treated as independent bytes.
///
/// # Errors
/// Returns an error if the floating-point predictor is requested but a scanline length
/// is not a whole multiple of the sample size (a corrupt/malformed tile), rather than
/// silently returning undecoded data.
pub fn apply_predictor_reverse(
    data: &mut [u8],
    predictor: Predictor,
    bytes_per_sample: usize,
    samples_per_pixel: usize,
    width: usize,
    byte_order: ByteOrderType,
) -> Result<()> {
    match predictor {
        Predictor::None => Ok(()),
        Predictor::HorizontalDifferencing => {
            for_each_row(data, bytes_per_sample, samples_per_pixel, width, |row| {
                undifference_row(row, bytes_per_sample, samples_per_pixel, byte_order);
                Ok(())
            })
        }
        Predictor::FloatingPoint => {
            // One scratch scanline for the whole tile, reused per row.
            let mut scratch = Vec::new();
            for_each_row(data, bytes_per_sample, samples_per_pixel, width, |row| {
                undo_float_predictor_row(
                    row,
                    bytes_per_sample,
                    samples_per_pixel,
                    byte_order,
                    &mut scratch,
                )
            })
        }
    }
}

/// Applies a predictor in the forward (encode) direction, for compression.
///
/// `byte_order` selects the on-disk byte order the encoded samples will be written in;
/// callers must pass the same byte order that the resulting file's header declares.
///
/// # Errors
/// Returns an error if the floating-point predictor is requested but a scanline length
/// is not a whole multiple of the sample size.
pub fn apply_predictor_forward(
    data: &mut [u8],
    predictor: Predictor,
    bytes_per_sample: usize,
    samples_per_pixel: usize,
    width: usize,
    byte_order: ByteOrderType,
) -> Result<()> {
    match predictor {
        Predictor::None => Ok(()),
        Predictor::HorizontalDifferencing => {
            for_each_row(data, bytes_per_sample, samples_per_pixel, width, |row| {
                difference_row(row, bytes_per_sample, samples_per_pixel, byte_order);
                Ok(())
            })
        }
        Predictor::FloatingPoint => {
            // One scratch scanline for the whole tile, reused per row.
            let mut scratch = Vec::new();
            for_each_row(data, bytes_per_sample, samples_per_pixel, width, |row| {
                apply_float_predictor_row(
                    row,
                    bytes_per_sample,
                    samples_per_pixel,
                    byte_order,
                    &mut scratch,
                )
            })
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    /// Reference implementation of the floating-point predictor decode step,
    /// transcribed verbatim from the pre-optimisation code (allocating `to_vec()`
    /// per scanline, scalar `match byte_order` per byte). Used to prove the
    /// optimised implementation is bit-for-bit identical.
    fn reference_undo_float_row(
        row: &mut [u8],
        bytes_per_sample: usize,
        samples_per_pixel: usize,
        byte_order: ByteOrderType,
    ) {
        let cc = row.len();
        if bytes_per_sample == 0 || cc == 0 || !cc.is_multiple_of(bytes_per_sample) {
            return;
        }
        let sample_count = cc / bytes_per_sample;
        let stride = samples_per_pixel.max(1);
        for i in stride..cc {
            row[i] = row[i].wrapping_add(row[i - stride]);
        }
        let planes = row.to_vec();
        for sample in 0..sample_count {
            for byte in 0..bytes_per_sample {
                let plane = match byte_order {
                    ByteOrderType::BigEndian => byte,
                    ByteOrderType::LittleEndian => bytes_per_sample - byte - 1,
                };
                row[bytes_per_sample * sample + byte] = planes[plane * sample_count + sample];
            }
        }
    }

    /// Reference implementation of the floating-point predictor encode step
    /// (pre-optimisation code, verbatim).
    fn reference_apply_float_row(
        row: &mut [u8],
        bytes_per_sample: usize,
        samples_per_pixel: usize,
        byte_order: ByteOrderType,
    ) {
        let cc = row.len();
        if bytes_per_sample == 0 || cc == 0 || !cc.is_multiple_of(bytes_per_sample) {
            return;
        }
        let sample_count = cc / bytes_per_sample;
        let stride = samples_per_pixel.max(1);
        let samples = row.to_vec();
        for sample in 0..sample_count {
            for byte in 0..bytes_per_sample {
                let plane = match byte_order {
                    ByteOrderType::BigEndian => byte,
                    ByteOrderType::LittleEndian => bytes_per_sample - byte - 1,
                };
                row[plane * sample_count + sample] = samples[bytes_per_sample * sample + byte];
            }
        }
        for i in (stride..cc).rev() {
            row[i] = row[i].wrapping_sub(row[i - stride]);
        }
    }

    /// Deterministic pseudo-random bytes (no external RNG dependency).
    fn pseudo_random_bytes(len: usize, seed: u64) -> Vec<u8> {
        let mut state = seed | 1;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 24) as u8
            })
            .collect()
    }

    #[test]
    fn test_predictor() {
        let original = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let mut data = original.clone();

        // Apply forward then reverse should give original
        apply_predictor_forward(
            &mut data,
            Predictor::HorizontalDifferencing,
            1,
            1,
            8,
            ByteOrderType::LittleEndian,
        )
        .expect("forward predictor");
        apply_predictor_reverse(
            &mut data,
            Predictor::HorizontalDifferencing,
            1,
            1,
            8,
            ByteOrderType::LittleEndian,
        )
        .expect("reverse predictor");

        assert_eq!(data, original);
    }

    #[test]
    fn test_predictor_16bit_roundtrip() {
        // Single-band, 16-bit samples, little-endian: a round-trip through
        // forward+reverse must reproduce the original regardless of carries.
        let original: Vec<u8> = [100i16, 300, -50, 20000, -20000, 7]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let mut data = original.clone();

        apply_predictor_forward(
            &mut data,
            Predictor::HorizontalDifferencing,
            2,
            1,
            original.len() / 2,
            ByteOrderType::LittleEndian,
        )
        .expect("forward predictor");
        apply_predictor_reverse(
            &mut data,
            Predictor::HorizontalDifferencing,
            2,
            1,
            original.len() / 2,
            ByteOrderType::LittleEndian,
        )
        .expect("reverse predictor");

        assert_eq!(data, original);
    }

    #[test]
    fn test_predictor_reverse_16bit_carry() {
        // Regression test for the byte-wise-vs-sample-wise bug: a delta that
        // carries from the low byte into the high byte of a 16-bit sample must
        // be reconstructed correctly, not silently dropped.
        //
        // sample0 = 100 (0x0064), delta1 = 200 (0x00C8) => sample1 must be 300.
        let mut row = Vec::new();
        row.extend_from_slice(&100u16.to_le_bytes());
        row.extend_from_slice(&200u16.to_le_bytes());

        apply_predictor_reverse(
            &mut row,
            Predictor::HorizontalDifferencing,
            2,
            1,
            2,
            ByteOrderType::LittleEndian,
        )
        .expect("reverse predictor");

        let sample0 = u16::from_le_bytes([row[0], row[1]]);
        let sample1 = u16::from_le_bytes([row[2], row[3]]);
        assert_eq!(sample0, 100);
        assert_eq!(sample1, 300);
    }

    #[test]
    fn test_predictor_32bit_roundtrip() {
        let original: Vec<u8> = [10i32, 100_000, -5, 2_000_000_000]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect();
        let mut data = original.clone();

        apply_predictor_forward(
            &mut data,
            Predictor::HorizontalDifferencing,
            4,
            1,
            original.len() / 4,
            ByteOrderType::BigEndian,
        )
        .expect("forward predictor");
        apply_predictor_reverse(
            &mut data,
            Predictor::HorizontalDifferencing,
            4,
            1,
            original.len() / 4,
            ByteOrderType::BigEndian,
        )
        .expect("reverse predictor");

        assert_eq!(data, original);
    }

    #[test]
    fn test_predictor_16bit_multiband_roundtrip() {
        // 2 interleaved bands (samples_per_pixel = 2), so the predictor stride
        // must skip over the other band's sample, not the adjacent byte.
        let original: Vec<u8> = [1i16, 2, 3, 4, 5, 6, 7, 8]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let mut data = original.clone();

        apply_predictor_forward(
            &mut data,
            Predictor::HorizontalDifferencing,
            2,
            2,
            original.len() / 4,
            ByteOrderType::LittleEndian,
        )
        .expect("forward predictor");
        apply_predictor_reverse(
            &mut data,
            Predictor::HorizontalDifferencing,
            2,
            2,
            original.len() / 4,
            ByteOrderType::LittleEndian,
        )
        .expect("reverse predictor");

        assert_eq!(data, original);
    }

    /// Round-trips a single-band Float32 scanline through the TIFF 6.0
    /// floating-point predictor (Predictor=3). Forward then reverse must
    /// reproduce the original bytes bit-for-bit.
    #[test]
    fn test_float_predictor_f32_roundtrip_le() {
        let values: [f32; 6] = [1.0, 1.5, -2.25, 3.125, 1000.0, -0.0001];
        let original: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        let mut data = original.clone();

        apply_predictor_forward(
            &mut data,
            Predictor::FloatingPoint,
            4,
            1,
            values.len(),
            ByteOrderType::LittleEndian,
        )
        .expect("forward float predictor");
        // Encoded form must differ from the raw bytes (the predictor did work).
        assert_ne!(data, original, "float predictor must transform the data");

        apply_predictor_reverse(
            &mut data,
            Predictor::FloatingPoint,
            4,
            1,
            values.len(),
            ByteOrderType::LittleEndian,
        )
        .expect("reverse float predictor");

        assert_eq!(data, original);

        // Values must decode exactly.
        let decoded: Vec<f32> = data
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        assert_eq!(decoded, values);
    }

    /// Round-trips a single-band Float64 scanline through the floating-point
    /// predictor in big-endian byte order.
    #[test]
    fn test_float_predictor_f64_roundtrip_be() {
        let values: [f64; 5] = [1.0, -1.0, 123456.789, f64::MIN_POSITIVE, -42.0];
        let original: Vec<u8> = values.iter().flat_map(|v| v.to_be_bytes()).collect();
        let mut data = original.clone();

        apply_predictor_forward(
            &mut data,
            Predictor::FloatingPoint,
            8,
            1,
            values.len(),
            ByteOrderType::BigEndian,
        )
        .expect("forward float predictor");
        apply_predictor_reverse(
            &mut data,
            Predictor::FloatingPoint,
            8,
            1,
            values.len(),
            ByteOrderType::BigEndian,
        )
        .expect("reverse float predictor");

        assert_eq!(data, original);
    }

    /// Round-trips a 3-band (RGB) interleaved Float32 image through the
    /// floating-point predictor: the byte-wise delta stride must equal
    /// `samples_per_pixel`, so per-band structure is preserved.
    #[test]
    fn test_float_predictor_f32_multiband_roundtrip() {
        // 2 pixels x 3 bands (interleaved) x 1 row.
        let values: [f32; 6] = [10.0, 20.0, 30.0, 11.0, 21.0, 31.0];
        let original: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        let mut data = original.clone();

        apply_predictor_forward(
            &mut data,
            Predictor::FloatingPoint,
            4,
            3,
            2, // width = 2 pixels
            ByteOrderType::LittleEndian,
        )
        .expect("forward float predictor");
        apply_predictor_reverse(
            &mut data,
            Predictor::FloatingPoint,
            4,
            3,
            2,
            ByteOrderType::LittleEndian,
        )
        .expect("reverse float predictor");

        assert_eq!(data, original);
    }

    /// Round-trips multiple scanlines (a full tile) so the per-row iteration
    /// in `for_each_row` is exercised for the floating-point predictor.
    #[test]
    fn test_float_predictor_multirow_roundtrip() {
        // 4 columns x 3 rows, single band Float32.
        let width = 4usize;
        let rows = 3usize;
        let values: Vec<f32> = (0..(width * rows)).map(|i| i as f32 * 0.5 - 3.0).collect();
        let original: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        let mut data = original.clone();

        apply_predictor_forward(
            &mut data,
            Predictor::FloatingPoint,
            4,
            1,
            width,
            ByteOrderType::LittleEndian,
        )
        .expect("forward float predictor");
        apply_predictor_reverse(
            &mut data,
            Predictor::FloatingPoint,
            4,
            1,
            width,
            ByteOrderType::LittleEndian,
        )
        .expect("reverse float predictor");

        assert_eq!(data, original);
    }

    /// Decodes a hand-constructed Predictor=3 Float32 scanline built exactly as
    /// libtiff's `fpDiff` would (byte-plane transpose, MSB plane first, then a
    /// byte-wise delta with stride = samples_per_pixel) and asserts the recovered
    /// floats are bit-exact. This locks the on-disk format so externally produced
    /// (GDAL/libtiff) tiles decode correctly, not just our own round-trips.
    #[test]
    fn test_float_predictor_decodes_libtiff_layout() {
        // Two little-endian Float32 samples: 1.0 and 2.0.
        // 1.0f32 LE bytes: 00 00 80 3F ; 2.0f32 LE bytes: 00 00 00 40.
        let s0 = 1.0f32.to_le_bytes(); // [0x00,0x00,0x80,0x3F]
        let s1 = 2.0f32.to_le_bytes(); // [0x00,0x00,0x00,0x40]

        // Byte-plane transpose, MSB plane first (plane 0 = byte index 3 of each LE sample):
        //   plane0 = [s0[3], s1[3]] = [0x3F, 0x40]
        //   plane1 = [s0[2], s1[2]] = [0x80, 0x00]
        //   plane2 = [s0[1], s1[1]] = [0x00, 0x00]
        //   plane3 = [s0[0], s1[0]] = [0x00, 0x00]
        let transposed = [
            s0[3], s1[3], // plane0
            s0[2], s1[2], // plane1
            s0[1], s1[1], // plane2
            s0[0], s1[0], // plane3
        ];
        // Apply byte-wise forward delta (stride = 1), back-to-front, to get the
        // on-disk stream.
        let mut on_disk = transposed;
        for i in (1..on_disk.len()).rev() {
            on_disk[i] = on_disk[i].wrapping_sub(on_disk[i - 1]);
        }

        // Decode through the public reverse predictor.
        let mut data = on_disk.to_vec();
        apply_predictor_reverse(
            &mut data,
            Predictor::FloatingPoint,
            4,
            1,
            2,
            ByteOrderType::LittleEndian,
        )
        .expect("reverse float predictor");

        let decoded: Vec<f32> = data
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        assert_eq!(decoded, vec![1.0f32, 2.0f32]);
    }

    /// A malformed floating-point tile (scanline length not a multiple of the
    /// sample size) must return an explicit error, never silently pass through
    /// undecoded bytes.
    #[test]
    fn test_float_predictor_rejects_ragged_row() {
        // width=2, spp=1, bps=4 => expected row length 8, but supply 6 bytes.
        let mut data = vec![0u8; 6];
        let err = apply_predictor_reverse(
            &mut data,
            Predictor::FloatingPoint,
            4,
            1,
            2,
            ByteOrderType::LittleEndian,
        )
        .expect_err("ragged float scanline must error");
        let msg = format!("{err}");
        assert!(
            msg.contains("Floating-point predictor"),
            "unexpected error message: {msg}"
        );
    }

    /// cool-japan/oxigeo#14: the optimised (scratch-reusing, width-specialised)
    /// floating-point predictor must be **bit-for-bit identical** to the previous
    /// per-scanline-allocating implementation for every sample width and byte
    /// order, in both directions.
    #[test]
    fn test_issue_14_float_predictor_matches_reference() {
        for &bytes_per_sample in &[1usize, 2, 3, 4, 8] {
            for &samples_per_pixel in &[1usize, 2, 3] {
                for &byte_order in &[ByteOrderType::LittleEndian, ByteOrderType::BigEndian] {
                    let sample_count = 37; // deliberately not a power of two
                    let len = sample_count * bytes_per_sample;
                    let original = pseudo_random_bytes(len, 0x5EED_1234 ^ (len as u64));

                    // Decode direction.
                    let mut expected = original.clone();
                    reference_undo_float_row(
                        &mut expected,
                        bytes_per_sample,
                        samples_per_pixel,
                        byte_order,
                    );
                    let mut actual = original.clone();
                    let mut scratch = Vec::new();
                    undo_float_predictor_row(
                        &mut actual,
                        bytes_per_sample,
                        samples_per_pixel,
                        byte_order,
                        &mut scratch,
                    )
                    .expect("undo float row");
                    assert_eq!(
                        actual, expected,
                        "decode mismatch for bps={bytes_per_sample} spp={samples_per_pixel} \
                         order={byte_order:?}"
                    );

                    // Encode direction.
                    let mut expected = original.clone();
                    reference_apply_float_row(
                        &mut expected,
                        bytes_per_sample,
                        samples_per_pixel,
                        byte_order,
                    );
                    let mut actual = original.clone();
                    apply_float_predictor_row(
                        &mut actual,
                        bytes_per_sample,
                        samples_per_pixel,
                        byte_order,
                        &mut scratch,
                    )
                    .expect("apply float row");
                    assert_eq!(
                        actual, expected,
                        "encode mismatch for bps={bytes_per_sample} spp={samples_per_pixel} \
                         order={byte_order:?}"
                    );
                }
            }
        }
    }

    /// cool-japan/oxigeo#14: full tile round-trip (forward then reverse) for both
    /// byte orders and both f32 and f64, over multiple scanlines, proving the
    /// specialised de-interleave preserves endianness exactly.
    #[test]
    fn test_issue_14_float_predictor_roundtrip_all_orders() {
        let width = 11usize;
        let rows = 5usize;

        for &byte_order in &[ByteOrderType::LittleEndian, ByteOrderType::BigEndian] {
            // f32
            let values: Vec<f32> = (0..(width * rows))
                .map(|i| (i as f32) * -3.25 + 0.125)
                .collect();
            let original: Vec<u8> = values
                .iter()
                .flat_map(|v| match byte_order {
                    ByteOrderType::LittleEndian => v.to_le_bytes(),
                    ByteOrderType::BigEndian => v.to_be_bytes(),
                })
                .collect();
            let mut data = original.clone();
            apply_predictor_forward(&mut data, Predictor::FloatingPoint, 4, 1, width, byte_order)
                .expect("forward f32");
            assert_ne!(data, original, "predictor must transform f32 data");
            apply_predictor_reverse(&mut data, Predictor::FloatingPoint, 4, 1, width, byte_order)
                .expect("reverse f32");
            assert_eq!(data, original, "f32 round-trip failed for {byte_order:?}");

            // f64
            let values: Vec<f64> = (0..(width * rows))
                .map(|i| (i as f64) * 1.0e-7 - 12345.6789)
                .collect();
            let original: Vec<u8> = values
                .iter()
                .flat_map(|v| match byte_order {
                    ByteOrderType::LittleEndian => v.to_le_bytes(),
                    ByteOrderType::BigEndian => v.to_be_bytes(),
                })
                .collect();
            let mut data = original.clone();
            apply_predictor_forward(&mut data, Predictor::FloatingPoint, 8, 1, width, byte_order)
                .expect("forward f64");
            assert_ne!(data, original, "predictor must transform f64 data");
            apply_predictor_reverse(&mut data, Predictor::FloatingPoint, 8, 1, width, byte_order)
                .expect("reverse f64");
            assert_eq!(data, original, "f64 round-trip failed for {byte_order:?}");
        }
    }

    /// cool-japan/oxigeo#14: the horizontal predictor (the other decode path) is
    /// untouched by the floating-point optimisation — round-trip it for both byte
    /// orders and 1/2/4/8-byte samples.
    #[test]
    fn test_issue_14_horizontal_predictor_roundtrip_all_orders() {
        for &bytes_per_sample in &[1usize, 2, 4, 8] {
            for &byte_order in &[ByteOrderType::LittleEndian, ByteOrderType::BigEndian] {
                let width = 13usize;
                let rows = 3usize;
                let original = pseudo_random_bytes(width * rows * bytes_per_sample, 0xC0FFEE);
                let mut data = original.clone();
                apply_predictor_forward(
                    &mut data,
                    Predictor::HorizontalDifferencing,
                    bytes_per_sample,
                    1,
                    width,
                    byte_order,
                )
                .expect("forward horizontal");
                apply_predictor_reverse(
                    &mut data,
                    Predictor::HorizontalDifferencing,
                    bytes_per_sample,
                    1,
                    width,
                    byte_order,
                )
                .expect("reverse horizontal");
                assert_eq!(
                    data, original,
                    "horizontal round-trip failed for bps={bytes_per_sample} {byte_order:?}"
                );
            }
        }
    }

    /// cool-japan/oxigeo#14: the byte-wise delta helpers must match the naive
    /// indexed loops they replaced for every stride (including the specialised
    /// `stride == 1` fast path).
    #[test]
    fn test_issue_14_byte_delta_helpers_match_naive() {
        for stride in 1usize..=5 {
            let original = pseudo_random_bytes(64, 0xABCD_EF01 + stride as u64);

            let mut naive = original.clone();
            for i in stride..naive.len() {
                naive[i] = naive[i].wrapping_add(naive[i - stride]);
            }
            let mut fast = original.clone();
            accumulate_bytes(&mut fast, stride);
            assert_eq!(fast, naive, "accumulate_bytes mismatch at stride {stride}");

            let mut naive = original.clone();
            for i in (stride..naive.len()).rev() {
                naive[i] = naive[i].wrapping_sub(naive[i - stride]);
            }
            let mut fast = original.clone();
            differentiate_bytes(&mut fast, stride);
            assert_eq!(
                fast, naive,
                "differentiate_bytes mismatch at stride {stride}"
            );
        }
    }
}
