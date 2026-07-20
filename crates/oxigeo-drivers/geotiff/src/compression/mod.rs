//! Compression and decompression for TIFF data
//!
//! This module provides implementations for various TIFF compression schemes.

use oxigeo_core::error::{CompressionError, OxiGeoError, Result};

use crate::tiff::{ByteOrderType, Compression, Predictor};

// Re-export JPEG types for public API
#[cfg(feature = "jpeg")]
pub use jpeg_encoder::ColorType;

// Re-export WebP color type for public API
#[cfg(feature = "webp")]
pub use image_webp::ColorType as WebpColorType;

/// Decompresses data using the specified compression scheme
pub fn decompress(data: &[u8], compression: Compression, expected_size: usize) -> Result<Vec<u8>> {
    match compression {
        Compression::None => Ok(data.to_vec()),

        #[cfg(feature = "deflate")]
        Compression::Deflate | Compression::AdobeDeflate => decompress_deflate(data, expected_size),

        #[cfg(feature = "lzw")]
        Compression::Lzw => decompress_lzw(data, expected_size),

        #[cfg(feature = "zstd")]
        Compression::Zstd => decompress_zstd(data, expected_size),

        Compression::Packbits => decompress_packbits(data, expected_size),

        #[cfg(feature = "jpeg")]
        Compression::Jpeg => decompress_jpeg(data),

        #[cfg(feature = "webp")]
        Compression::WebP => decompress_webp(data),

        Compression::Lerc => crate::lerc_codec::decompress_lerc(data, expected_size),

        _ => Err(OxiGeoError::Compression(CompressionError::UnknownMethod {
            method: compression as u16,
        })),
    }
}

/// Compresses data using the specified compression scheme
pub fn compress(data: &[u8], compression: Compression) -> Result<Vec<u8>> {
    match compression {
        Compression::None => Ok(data.to_vec()),

        #[cfg(feature = "deflate")]
        Compression::Deflate | Compression::AdobeDeflate => compress_deflate(data),

        #[cfg(feature = "lzw")]
        Compression::Lzw => compress_lzw(data),

        #[cfg(feature = "zstd")]
        Compression::Zstd => compress_zstd(data),

        Compression::Packbits => compress_packbits(data),

        #[cfg(feature = "jpeg")]
        Compression::Jpeg => compress_jpeg(data, 85),

        #[cfg(feature = "webp")]
        Compression::WebP => Err(OxiGeoError::Compression(
            CompressionError::CompressionFailed {
                message: "WebP compression requires image dimensions and color type. \
                          Use compress_webp_with_params instead."
                    .to_string(),
            },
        )),

        Compression::Lerc => Err(OxiGeoError::Compression(
            CompressionError::CompressionFailed {
                message: "LERC encoding to the interoperable Esri/GDAL bit-stuffed format is not \
                          implemented; only decoding (Compression::Lerc via decompress) is \
                          supported. Refusing to write a non-standard LERC blob."
                    .to_string(),
            },
        )),

        _ => Err(OxiGeoError::Compression(CompressionError::UnknownMethod {
            method: compression as u16,
        })),
    }
}

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
fn undo_float_predictor_row(
    row: &mut [u8],
    bytes_per_sample: usize,
    samples_per_pixel: usize,
    byte_order: ByteOrderType,
) -> Result<()> {
    let cc = row.len();
    if bytes_per_sample == 0 || cc == 0 {
        return Ok(());
    }
    if !cc.is_multiple_of(bytes_per_sample) {
        return Err(OxiGeoError::Compression(
            CompressionError::DecompressionFailed {
                message: format!(
                    "Floating-point predictor: scanline length {cc} is not a multiple of \
                     sample size {bytes_per_sample}"
                ),
            },
        ));
    }
    let sample_count = cc / bytes_per_sample;
    let stride = samples_per_pixel.max(1);

    // Step 1: undo the byte-wise horizontal delta (running sum, stride = spp).
    for i in stride..cc {
        row[i] = row[i].wrapping_add(row[i - stride]);
    }

    // Step 2: undo the byte-plane transpose, reassembling samples in `byte_order`.
    let planes = row.to_vec();
    for sample in 0..sample_count {
        for byte in 0..bytes_per_sample {
            // Plane 0 holds the most-significant byte of every sample.
            let plane = match byte_order {
                ByteOrderType::BigEndian => byte,
                ByteOrderType::LittleEndian => bytes_per_sample - byte - 1,
            };
            row[bytes_per_sample * sample + byte] = planes[plane * sample_count + sample];
        }
    }
    Ok(())
}

/// Encodes one scanline with the TIFF 6.0 floating-point predictor (Predictor
/// tag = 3). This is the exact inverse of [`undo_float_predictor_row`]: it first
/// performs the byte-plane transpose (most-significant byte plane first) and then
/// applies the byte-wise horizontal delta with stride `samples_per_pixel`.
fn apply_float_predictor_row(
    row: &mut [u8],
    bytes_per_sample: usize,
    samples_per_pixel: usize,
    byte_order: ByteOrderType,
) -> Result<()> {
    let cc = row.len();
    if bytes_per_sample == 0 || cc == 0 {
        return Ok(());
    }
    if !cc.is_multiple_of(bytes_per_sample) {
        return Err(OxiGeoError::Compression(
            CompressionError::CompressionFailed {
                message: format!(
                    "Floating-point predictor: scanline length {cc} is not a multiple of \
                     sample size {bytes_per_sample}"
                ),
            },
        ));
    }
    let sample_count = cc / bytes_per_sample;
    let stride = samples_per_pixel.max(1);

    // Step 1: byte-plane transpose (most-significant byte plane first).
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

    // Step 2: byte-wise horizontal delta (applied back-to-front, stride = spp).
    for i in (stride..cc).rev() {
        row[i] = row[i].wrapping_sub(row[i - stride]);
    }
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
            for_each_row(data, bytes_per_sample, samples_per_pixel, width, |row| {
                undo_float_predictor_row(row, bytes_per_sample, samples_per_pixel, byte_order)
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
            for_each_row(data, bytes_per_sample, samples_per_pixel, width, |row| {
                apply_float_predictor_row(row, bytes_per_sample, samples_per_pixel, byte_order)
            })
        }
    }
}

#[cfg(feature = "deflate")]
fn decompress_deflate(data: &[u8], _expected_size: usize) -> Result<Vec<u8>> {
    oxiarc_deflate::zlib_decompress(data).map_err(|e| {
        OxiGeoError::Compression(CompressionError::DecompressionFailed {
            message: format!("DEFLATE decompression failed: {}", e),
        })
    })
}

#[cfg(feature = "deflate")]
fn compress_deflate(data: &[u8]) -> Result<Vec<u8>> {
    // Default compression level 6
    oxiarc_deflate::zlib_compress(data, 6).map_err(|e| {
        OxiGeoError::Compression(CompressionError::CompressionFailed {
            message: format!("DEFLATE compression failed: {}", e),
        })
    })
}

#[cfg(feature = "lzw")]
fn decompress_lzw(data: &[u8], expected_size: usize) -> Result<Vec<u8>> {
    // Use oxiarc-lzw for TIFF LZW decompression
    // This fixes the truncation bug found in weezl
    oxiarc_lzw::decompress_tiff(data, expected_size).map_err(|e| {
        OxiGeoError::Compression(CompressionError::DecompressionFailed {
            message: format!("LZW decompression failed: {}", e),
        })
    })
}

#[cfg(feature = "lzw")]
fn compress_lzw(data: &[u8]) -> Result<Vec<u8>> {
    // Use oxiarc-lzw for TIFF LZW compression
    oxiarc_lzw::compress_tiff(data).map_err(|e| {
        OxiGeoError::Compression(CompressionError::CompressionFailed {
            message: format!("LZW compression failed: {}", e),
        })
    })
}

#[cfg(feature = "zstd")]
fn decompress_zstd(data: &[u8], _expected_size: usize) -> Result<Vec<u8>> {
    oxiarc_zstd::decode_all(data).map_err(|e| {
        OxiGeoError::Compression(CompressionError::DecompressionFailed {
            message: format!("ZSTD decompression failed: {}", e),
        })
    })
}

#[cfg(feature = "zstd")]
fn compress_zstd(data: &[u8]) -> Result<Vec<u8>> {
    oxiarc_zstd::encode_all(data, 3).map_err(|e| {
        OxiGeoError::Compression(CompressionError::CompressionFailed {
            message: format!("ZSTD compression failed: {}", e),
        })
    })
}

#[cfg(feature = "jpeg")]
fn decompress_jpeg(data: &[u8]) -> Result<Vec<u8>> {
    use jpeg_decoder::Decoder;

    let mut decoder = Decoder::new(data);
    let pixels = decoder.decode().map_err(|e| {
        OxiGeoError::Compression(CompressionError::DecompressionFailed {
            message: format!("JPEG decompression failed: {}", e),
        })
    })?;

    // Get image metadata
    let info = decoder.info().ok_or_else(|| {
        OxiGeoError::Compression(CompressionError::DecompressionFailed {
            message: "JPEG decoder missing image info".to_string(),
        })
    })?;

    // Handle YCbCr to RGB conversion if needed
    // jpeg-decoder already handles this conversion internally
    // The output is in RGB format for color images

    // For grayscale or RGB, pixels are already in the correct format
    // TIFF expects RGB for color images
    match info.pixel_format {
        jpeg_decoder::PixelFormat::L8 => {
            // Grayscale, no conversion needed
            Ok(pixels)
        }
        jpeg_decoder::PixelFormat::RGB24 => {
            // RGB, no conversion needed
            Ok(pixels)
        }
        jpeg_decoder::PixelFormat::CMYK32 => {
            // CMYK needs conversion to RGB
            cmyk_to_rgb(&pixels)
        }
        _ => Err(OxiGeoError::Compression(
            CompressionError::DecompressionFailed {
                message: format!("Unsupported JPEG pixel format: {:?}", info.pixel_format),
            },
        )),
    }
}

/// Decompresses a JPEG strip/tile using pre-loaded shared JPEGTables (TIFF tag 347).
///
/// Call this variant from the COG reader when the IFD contains a `JPEGTables` tag.
/// The tables and strip data are merged according to TIFF Technical Note 1 before
/// decoding.
///
/// # Errors
/// Returns an error if the merge or the JPEG decode fails.
#[cfg(feature = "jpeg")]
pub fn decompress_jpeg_with_tables(tables: &[u8], strip_data: &[u8]) -> Result<Vec<u8>> {
    use crate::jpeg_codec::merge_jpeg_tables;

    let merged = merge_jpeg_tables(tables, strip_data)?;
    decompress_jpeg(&merged)
}

#[cfg(feature = "jpeg")]
fn compress_jpeg(_data: &[u8], _quality: u8) -> Result<Vec<u8>> {
    // Determine image properties from data
    // For now, assume RGB 8-bit data
    // In a real implementation, this would need additional parameters
    // or context to determine the correct dimensions and color type

    // Note: This is a simplified version. In practice, we'd need width, height,
    // and color type information passed separately or derived from context.

    // For demonstration, we'll create a simple encoder
    // Real usage would require image dimensions to be passed as parameters

    // Since we don't have dimensions here, we'll need to refactor this
    // to accept width, height, and color_type as parameters

    // For now, return an error indicating this needs more information
    Err(OxiGeoError::Compression(CompressionError::CompressionFailed {
        message: "JPEG compression requires image dimensions and color type information. Use compress_jpeg_with_params instead.".to_string(),
    }))
}

/// JPEG compression with explicit parameters
#[cfg(feature = "jpeg")]
pub fn compress_jpeg_with_params(
    data: &[u8],
    width: u16,
    height: u16,
    color_type: jpeg_encoder::ColorType,
    quality: u8,
) -> Result<Vec<u8>> {
    use jpeg_encoder::Encoder;

    let mut output = Vec::new();
    let encoder = Encoder::new(&mut output, quality);

    encoder
        .encode(data, width, height, color_type)
        .map_err(|e| {
            OxiGeoError::Compression(CompressionError::CompressionFailed {
                message: format!("JPEG compression failed: {}", e),
            })
        })?;

    Ok(output)
}

/// Decompresses a WebP-encoded TIFF strip or tile.
///
/// Handles both lossy (VP8) and lossless (VP8L) WebP. The decoder returns
/// interleaved bytes — 3 bytes per pixel (RGB) for opaque images, 4 bytes
/// per pixel (RGBA) for images carrying an alpha channel. The caller is
/// responsible for matching this layout against the TIFF tile/strip
/// geometry and `SamplesPerPixel`/`PhotometricInterpretation` tags.
///
/// TIFF compression tag value for WebP is **50001**, registered as part of
/// the LERC/WebP draft extension and now widely produced by GDAL's COG
/// driver.
///
/// # Errors
/// Returns a [`CompressionError`] if the WebP stream is malformed or the
/// pure Rust `image-webp` decoder rejects the bitstream.
#[cfg(feature = "webp")]
fn decompress_webp(data: &[u8]) -> Result<Vec<u8>> {
    use image_webp::WebPDecoder;
    use std::io::Cursor;

    if data.is_empty() {
        return Err(OxiGeoError::Compression(
            CompressionError::DecompressionFailed {
                message: "WebP strip/tile data is empty".to_string(),
            },
        ));
    }

    let mut decoder = WebPDecoder::new(Cursor::new(data)).map_err(|e| {
        OxiGeoError::Compression(CompressionError::DecompressionFailed {
            message: format!("WebP decoder init failed: {}", e),
        })
    })?;

    let buf_size = decoder.output_buffer_size().ok_or_else(|| {
        OxiGeoError::Compression(CompressionError::DecompressionFailed {
            message: "WebP output buffer size overflows usize".to_string(),
        })
    })?;

    let mut buf = vec![0_u8; buf_size];
    decoder.read_image(&mut buf).map_err(|e| {
        OxiGeoError::Compression(CompressionError::DecompressionFailed {
            message: format!("WebP decode failed: {}", e),
        })
    })?;

    Ok(buf)
}

/// WebP compression with explicit image parameters.
///
/// `color_type` chooses the byte layout: `L8` (grayscale), `La8`
/// (grayscale + alpha), `Rgb8`, or `Rgba8`. The pure Rust `image-webp`
/// encoder currently emits **lossless VP8L** only, which is exact (perfect
/// roundtrip) and well suited to integer raster bands and discrete-class
/// imagery. Lossy VP8 encoding would require linking the C `libwebp`
/// library and is therefore disallowed by COOLJAPAN's Pure Rust Policy.
///
/// # Errors
/// Returns a [`CompressionError`] if `data.len()` does not match the
/// expected size for `width * height * bytes_per_pixel` or if the encoder
/// fails internally.
#[cfg(feature = "webp")]
pub fn compress_webp_with_params(
    data: &[u8],
    width: u32,
    height: u32,
    color_type: image_webp::ColorType,
) -> Result<Vec<u8>> {
    use image_webp::{ColorType as WebpColor, WebPEncoder};

    let bytes_per_pixel = match color_type {
        WebpColor::L8 => 1,
        WebpColor::La8 => 2,
        WebpColor::Rgb8 => 3,
        WebpColor::Rgba8 => 4,
    };

    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(bytes_per_pixel))
        .ok_or_else(|| {
            OxiGeoError::Compression(CompressionError::CompressionFailed {
                message: "WebP input dimensions overflow usize".to_string(),
            })
        })?;

    if data.len() != expected {
        return Err(OxiGeoError::Compression(
            CompressionError::CompressionFailed {
                message: format!(
                    "WebP input length {} does not match expected {} ({}x{}x{} bytes)",
                    data.len(),
                    expected,
                    width,
                    height,
                    bytes_per_pixel
                ),
            },
        ));
    }

    let mut output = Vec::new();
    let encoder = WebPEncoder::new(&mut output);
    encoder
        .encode(data, width, height, color_type)
        .map_err(|e| {
            OxiGeoError::Compression(CompressionError::CompressionFailed {
                message: format!("WebP encoding failed: {}", e),
            })
        })?;

    Ok(output)
}

/// Converts CMYK to RGB
#[cfg(feature = "jpeg")]
fn cmyk_to_rgb(cmyk_data: &[u8]) -> Result<Vec<u8>> {
    if !cmyk_data.len().is_multiple_of(4) {
        return Err(OxiGeoError::Compression(CompressionError::InvalidData {
            message: "CMYK data length must be multiple of 4".to_string(),
        }));
    }

    let pixel_count = cmyk_data.len() / 4;
    let mut rgb_data = Vec::with_capacity(pixel_count * 3);

    for i in 0..pixel_count {
        let c = cmyk_data[i * 4] as f32 / 255.0;
        let m = cmyk_data[i * 4 + 1] as f32 / 255.0;
        let y = cmyk_data[i * 4 + 2] as f32 / 255.0;
        let k = cmyk_data[i * 4 + 3] as f32 / 255.0;

        // CMYK to RGB conversion
        let r = ((1.0 - c) * (1.0 - k) * 255.0) as u8;
        let g = ((1.0 - m) * (1.0 - k) * 255.0) as u8;
        let b = ((1.0 - y) * (1.0 - k) * 255.0) as u8;

        rgb_data.push(r);
        rgb_data.push(g);
        rgb_data.push(b);
    }

    Ok(rgb_data)
}

/// PackBits decompression (simple RLE)
fn decompress_packbits(data: &[u8], expected_size: usize) -> Result<Vec<u8>> {
    let mut output = Vec::with_capacity(expected_size);
    let mut i = 0;

    while i < data.len() && output.len() < expected_size {
        let n = data[i] as i8;
        i += 1;

        if n >= 0 {
            // Literal run: copy next n+1 bytes
            let count = (n as usize) + 1;
            if i + count > data.len() {
                return Err(OxiGeoError::Compression(CompressionError::InvalidData {
                    message: "PackBits: unexpected end of data".to_string(),
                }));
            }
            output.extend_from_slice(&data[i..i + count]);
            i += count;
        } else if n > -128 {
            // Repeat run: repeat next byte -n+1 times
            if i >= data.len() {
                return Err(OxiGeoError::Compression(CompressionError::InvalidData {
                    message: "PackBits: unexpected end of data".to_string(),
                }));
            }
            let count = ((-n) as usize) + 1;
            let byte = data[i];
            i += 1;
            output.extend(std::iter::repeat_n(byte, count));
        }
        // n == -128: no-op
    }

    Ok(output)
}

/// PackBits compression (simple RLE)
fn compress_packbits(data: &[u8]) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut i = 0;

    while i < data.len() {
        // Look for runs
        let mut run_length = 1;
        while i + run_length < data.len() && run_length < 128 && data[i + run_length] == data[i] {
            run_length += 1;
        }

        if run_length > 1 {
            // Write repeat run
            // For PackBits: repeat run of N bytes is encoded as (1 - N)
            // run_length ranges from 2 to 128, so (1 - run_length) ranges from -1 to -127
            output.push(((1_i16 - run_length as i16) as i8) as u8);
            output.push(data[i]);
            i += run_length;
        } else {
            // Look for literal run
            let literal_start = i;
            let mut literal_end = i + 1;

            while literal_end < data.len() && literal_end - literal_start < 128 {
                // Check if a run starts here
                if literal_end + 1 < data.len() && data[literal_end] == data[literal_end + 1] {
                    break;
                }
                literal_end += 1;
            }

            let count = literal_end - literal_start;
            output.push((count - 1) as u8);
            output.extend_from_slice(&data[literal_start..literal_end]);
            i = literal_end;
        }
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn test_packbits_roundtrip() {
        let original = b"AAAAAABBBBCCCCCCCCCCDDDEEEEEEEEEE";
        let compressed = compress_packbits(original).expect("compression should work");
        let decompressed =
            decompress_packbits(&compressed, original.len()).expect("decompression should work");
        assert_eq!(&decompressed, original);
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

    #[cfg(feature = "deflate")]
    #[test]
    fn test_deflate_roundtrip() {
        let original = b"Hello, World! This is a test of DEFLATE compression.";
        let compressed = compress_deflate(original).expect("compression should work");
        let decompressed =
            decompress_deflate(&compressed, original.len()).expect("decompression should work");
        assert_eq!(&decompressed, original);
    }

    #[cfg(feature = "zstd")]
    #[test]
    fn test_zstd_roundtrip() {
        let original = b"Hello, World! This is a test of ZSTD compression.";
        let compressed = compress_zstd(original).expect("compression should work");
        let decompressed =
            decompress_zstd(&compressed, original.len()).expect("decompression should work");
        assert_eq!(&decompressed, original);
    }

    #[cfg(feature = "jpeg")]
    #[test]
    fn test_jpeg_grayscale_roundtrip() {
        use jpeg_encoder::ColorType;

        // Create a simple 8x8 grayscale test image
        let width = 8;
        let height = 8;
        let mut original = Vec::new();
        for y in 0..height {
            for x in 0..width {
                // Create a gradient pattern
                original.push(((x + y * width) * 4) as u8);
            }
        }

        // Compress
        let compressed =
            compress_jpeg_with_params(&original, width as u16, height as u16, ColorType::Luma, 85)
                .expect("compression should work");

        // Decompress
        let decompressed = decompress_jpeg(&compressed).expect("decompression should work");

        // JPEG is lossy, so we check that dimensions match and values are close
        assert_eq!(decompressed.len(), original.len());

        // Check that most pixels are within a reasonable threshold (JPEG is lossy)
        let mut close_count = 0;
        for (i, (&orig, &decomp)) in original.iter().zip(decompressed.iter()).enumerate() {
            let diff = (orig as i16 - decomp as i16).abs();
            if diff <= 20 {
                // Allow up to 20 levels difference for JPEG artifacts
                close_count += 1;
            } else {
                tracing::debug!("Pixel {} differs by {}: {} vs {}", i, diff, orig, decomp);
            }
        }

        // At least 90% of pixels should be close
        let close_ratio = close_count as f64 / original.len() as f64;
        assert!(
            close_ratio >= 0.9,
            "Only {:.1}% of pixels are close (expected >= 90%)",
            close_ratio * 100.0
        );
    }

    #[cfg(feature = "jpeg")]
    #[test]
    fn test_jpeg_rgb_roundtrip() {
        use jpeg_encoder::ColorType;

        // Create a simple 8x8 RGB test image
        let width = 8;
        let height = 8;
        let mut original = Vec::new();
        for y in 0..height {
            for x in 0..width {
                // Create a colorful gradient pattern
                original.push((x * 32) as u8); // R
                original.push((y * 32) as u8); // G
                original.push(((x + y) * 16) as u8); // B
            }
        }

        // Compress
        let compressed =
            compress_jpeg_with_params(&original, width as u16, height as u16, ColorType::Rgb, 85)
                .expect("compression should work");

        // Decompress
        let decompressed = decompress_jpeg(&compressed).expect("decompression should work");

        // Check dimensions
        assert_eq!(decompressed.len(), original.len());

        // Check that most pixels are within a reasonable threshold
        let mut close_count = 0;
        for (i, (&orig, &decomp)) in original.iter().zip(decompressed.iter()).enumerate() {
            let diff = (orig as i16 - decomp as i16).abs();
            if diff <= 25 {
                // Allow up to 25 levels difference for JPEG artifacts
                close_count += 1;
            } else {
                tracing::debug!(
                    "Pixel component {} differs by {}: {} vs {}",
                    i,
                    diff,
                    orig,
                    decomp
                );
            }
        }

        // At least 85% of pixel components should be close
        let close_ratio = close_count as f64 / original.len() as f64;
        assert!(
            close_ratio >= 0.85,
            "Only {:.1}% of pixel components are close (expected >= 85%)",
            close_ratio * 100.0
        );
    }

    #[cfg(feature = "jpeg")]
    #[test]
    fn test_jpeg_quality_settings() {
        use jpeg_encoder::ColorType;

        // Create a simple 16x16 grayscale test image
        let width = 16;
        let height = 16;
        let mut original = Vec::new();
        for y in 0..height {
            for x in 0..width {
                original.push(((x + y * width) * 2) as u8);
            }
        }

        // Test different quality levels
        let quality_low =
            compress_jpeg_with_params(&original, width as u16, height as u16, ColorType::Luma, 50)
                .expect("low quality compression should work");

        let quality_high =
            compress_jpeg_with_params(&original, width as u16, height as u16, ColorType::Luma, 95)
                .expect("high quality compression should work");

        // Higher quality should produce larger files
        assert!(
            quality_high.len() >= quality_low.len(),
            "High quality ({} bytes) should be >= low quality ({} bytes)",
            quality_high.len(),
            quality_low.len()
        );

        // Both should decompress successfully
        let _decompressed_low = decompress_jpeg(&quality_low).expect("should decompress");
        let _decompressed_high = decompress_jpeg(&quality_high).expect("should decompress");
    }

    #[cfg(feature = "jpeg")]
    #[test]
    fn test_cmyk_to_rgb_conversion() {
        // Test CMYK to RGB conversion
        // Pure black: C=0, M=0, Y=0, K=100
        let cmyk = vec![0, 0, 0, 255];
        let rgb = cmyk_to_rgb(&cmyk).expect("conversion should work");
        assert_eq!(rgb, vec![0, 0, 0]);

        // Pure white: C=0, M=0, Y=0, K=0
        let cmyk = vec![0, 0, 0, 0];
        let rgb = cmyk_to_rgb(&cmyk).expect("conversion should work");
        assert_eq!(rgb, vec![255, 255, 255]);

        // Pure cyan: C=100, M=0, Y=0, K=0
        let cmyk = vec![255, 0, 0, 0];
        let rgb = cmyk_to_rgb(&cmyk).expect("conversion should work");
        assert_eq!(rgb, vec![0, 255, 255]);

        // Pure magenta: C=0, M=100, Y=0, K=0
        let cmyk = vec![0, 255, 0, 0];
        let rgb = cmyk_to_rgb(&cmyk).expect("conversion should work");
        assert_eq!(rgb, vec![255, 0, 255]);

        // Pure yellow: C=0, M=0, Y=100, K=0
        let cmyk = vec![0, 0, 255, 0];
        let rgb = cmyk_to_rgb(&cmyk).expect("conversion should work");
        assert_eq!(rgb, vec![255, 255, 0]);
    }

    /// Regression test for cool-japan/oxigeo#6 — WebP compression codec
    /// (TIFF compression tag 50001) must round-trip via the public
    /// `compress`/`decompress` dispatch.
    ///
    /// VP8L (lossless) is exact, so we assert byte-for-byte equality.
    #[cfg(feature = "webp")]
    #[test]
    fn test_issue_6_webp_compression() {
        use image_webp::ColorType as WebpColor;

        // 8x8 RGB tile — small enough to encode quickly, large enough to
        // exercise the VP8L Huffman + transform pipeline.
        let width: u32 = 8;
        let height: u32 = 8;
        let mut original = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            for x in 0..width {
                original.push((x as u8) * 32);
                original.push((y as u8) * 32);
                original.push(((x + y) as u8) * 16);
            }
        }

        // Encode via the explicit-parameter API (mirrors JPEG).
        let compressed = compress_webp_with_params(&original, width, height, WebpColor::Rgb8)
            .expect("WebP VP8L encoding should succeed");

        // WebP-encoded payloads start with the RIFF/WEBP container magic.
        assert_eq!(&compressed[0..4], b"RIFF");
        assert_eq!(&compressed[8..12], b"WEBP");

        // Decode through the public dispatch entry point — this is the
        // path a TIFF reader takes when it encounters compression=50001.
        let decompressed = decompress(&compressed, Compression::WebP, original.len())
            .expect("WebP decompress dispatch should succeed");

        // VP8L is lossless — exact equality is the right assertion.
        assert_eq!(decompressed, original);
    }

    /// Verifies that `Compression::WebP` (TIFF tag 50001) is recognised by
    /// the dispatch table and produces a decoder-initialisation error
    /// (not `UnknownMethod`) when fed invalid bytes.
    #[cfg(feature = "webp")]
    #[test]
    fn test_issue_6_webp_dispatch_recognises_tag_50001() {
        // Confirm the tag value is 50001 as per the LERC/WebP TIFF draft.
        assert_eq!(Compression::WebP as u16, 50001);

        // Invalid input should reach the WebP decoder (and fail there),
        // not bounce off the dispatch table as `UnknownMethod`.
        let invalid = b"not a webp stream";
        let err = decompress(invalid, Compression::WebP, 64)
            .expect_err("invalid WebP bytes must fail decode");
        let msg = format!("{}", err);
        assert!(
            msg.contains("WebP") || msg.contains("webp") || msg.contains("RIFF"),
            "expected WebP-specific decode error, got: {}",
            msg
        );
    }

    /// Builds a minimal, checksum-free LERC2 v2 constant-image blob (2x2 Float,
    /// all valid, `zMin == zMax`) and verifies it decodes through the public
    /// `decompress` dispatch for `Compression::Lerc` (TIFF tag 34887) into native
    /// little-endian f32 sample bytes — not the `UnknownMethod` fall-through.
    #[test]
    fn test_lerc_dispatch_decodes_constant_image() {
        let mut blob = Vec::new();
        blob.extend_from_slice(b"Lerc2 ");
        blob.extend_from_slice(&2i32.to_le_bytes()); // version 2 (no checksum)
        blob.extend_from_slice(&2i32.to_le_bytes()); // nRows
        blob.extend_from_slice(&2i32.to_le_bytes()); // nCols
        blob.extend_from_slice(&4i32.to_le_bytes()); // numValidPixel
        blob.extend_from_slice(&8i32.to_le_bytes()); // microBlockSize
        let blobsize_pos = blob.len();
        blob.extend_from_slice(&0i32.to_le_bytes()); // blobSize placeholder
        blob.extend_from_slice(&6i32.to_le_bytes()); // dt = Float
        blob.extend_from_slice(&0.0f64.to_le_bytes()); // maxZError
        blob.extend_from_slice(&7.0f64.to_le_bytes()); // zMin
        blob.extend_from_slice(&7.0f64.to_le_bytes()); // zMax == zMin => const
        blob.extend_from_slice(&0i32.to_le_bytes()); // numBytesMask = 0 (all valid)
        let blob_size = blob.len() as i32;
        blob[blobsize_pos..blobsize_pos + 4].copy_from_slice(&blob_size.to_le_bytes());

        // TIFF tag value must be 34887.
        assert_eq!(Compression::Lerc as u16, 34887);

        let out = decompress(&blob, Compression::Lerc, 16).expect("LERC dispatch decodes");
        // 2x2 single-band Float => 4 samples * 4 bytes = 16 bytes, all == 7.0f32.
        assert_eq!(out.len(), 16);
        let decoded: Vec<f32> = out
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        assert_eq!(decoded, vec![7.0f32; 4]);
    }

    /// LERC encoding to the interoperable format is intentionally unimplemented:
    /// the compress dispatch must return an explicit typed error (never a
    /// non-standard blob written under the LERC tag).
    #[test]
    fn test_lerc_compress_returns_typed_error() {
        let err = compress(&[0u8; 16], Compression::Lerc).expect_err("LERC encode unsupported");
        let msg = format!("{err}");
        assert!(
            msg.contains("LERC encoding") && msg.contains("not implemented"),
            "expected a typed LERC-unsupported error, got: {msg}"
        );
    }

    /// Verifies that the WebP encoder validates input length against
    /// declared dimensions before invoking the underlying encoder.
    #[cfg(feature = "webp")]
    #[test]
    fn test_issue_6_webp_encoder_validates_input_length() {
        use image_webp::ColorType as WebpColor;

        // Claim 4x4 RGB (48 bytes) but provide only 10 bytes.
        let bogus = vec![0_u8; 10];
        let err = compress_webp_with_params(&bogus, 4, 4, WebpColor::Rgb8)
            .expect_err("length mismatch must be rejected");
        let msg = format!("{}", err);
        assert!(
            msg.contains("does not match expected"),
            "expected length-mismatch message, got: {}",
            msg
        );
    }
}
