//! Compression and decompression for TIFF data
//!
//! This module provides implementations for various TIFF compression schemes.
//!
//! Two decode entry points are available:
//!
//! - [`decompress`] allocates and returns an owned `Vec<u8>` — convenient, and the
//!   only option for codecs whose decoder can exclusively produce an owned buffer.
//! - [`decompress_into`] / [`decompress_into_partial`] decode straight into a
//!   caller-owned buffer, which lets a tile reader reuse one scratch buffer across
//!   every tile of a band instead of allocating (and, for `Compression::None`,
//!   *copying*) one buffer per tile. See [`crate::cog::CogReader::read_tile_into`].

pub mod predictor;

use oxigeo_core::error::{CompressionError, OxiGeoError, Result};

use crate::tiff::Compression;

pub use predictor::{apply_predictor_forward, apply_predictor_reverse};

// Re-export JPEG types for public API
#[cfg(feature = "jpeg")]
pub use jpeg_encoder::ColorType;

// Re-export WebP color type for public API
#[cfg(feature = "webp")]
pub use image_webp::ColorType as WebpColorType;

/// Upper bound (in bytes) on a speculative allocation driven by a decoded-size
/// *hint*.
///
/// Decoded-size hints are derived from IFD tags (tile geometry, sample count),
/// i.e. from untrusted input: a malformed or hostile header can claim a tile of
/// essentially any size. Codecs that pre-size their output buffer from the hint
/// therefore clamp it here first, so a bogus header costs at most one 256 MiB
/// reservation rather than an instant out-of-memory abort. The clamp only ever
/// affects the *initial capacity*: a buffer that legitimately needs to grow past
/// it still does.
const MAX_SIZE_HINT_BYTES: usize = 256 * 1024 * 1024;

/// Clamps an untrusted decoded-size hint to [`MAX_SIZE_HINT_BYTES`].
const fn clamped_hint(expected_size: usize) -> usize {
    if expected_size > MAX_SIZE_HINT_BYTES {
        MAX_SIZE_HINT_BYTES
    } else {
        expected_size
    }
}

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

/// Decompresses `src` directly into `dst`, without allocating an intermediate
/// buffer for the result.
///
/// This is the buffer-reusing counterpart of [`decompress`]. It is the fast path
/// for whole-band reads, where one scratch buffer can serve every tile of the
/// band: `Compression::None` becomes a single `copy_from_slice` (the pure-waste
/// `to_vec()` allocation disappears), PackBits is expanded straight into `dst`,
/// and DEFLATE inflates straight into `dst`. Codecs whose decoder API can only
/// produce an owned `Vec` (LZW, ZSTD, JPEG, WebP, LERC) decode once and are copied
/// into `dst` — one copy, not the previous copy *plus* a redundant `to_vec()`.
///
/// # Errors
/// Returns [`CompressionError::InvalidData`] if the decoded payload is not
/// exactly `dst.len()` bytes long, and propagates any codec error. Use
/// [`decompress_into_partial`] when a short payload must be tolerated (e.g. a
/// truncated final strip).
pub fn decompress_into(src: &[u8], compression: Compression, dst: &mut [u8]) -> Result<()> {
    let written = decompress_into_partial(src, compression, dst)?;
    if written != dst.len() {
        return Err(OxiGeoError::Compression(CompressionError::InvalidData {
            message: format!(
                "decompressed length {} does not match destination length {}",
                written,
                dst.len()
            ),
        }));
    }
    Ok(())
}

/// Decompresses `src` into the front of `dst`, returning how many bytes were
/// written.
///
/// Identical to [`decompress_into`] except that a payload *shorter* than `dst` is
/// accepted and reported rather than rejected; the untouched tail of `dst` keeps
/// whatever the caller put there. A payload *longer* than `dst` is always an
/// error — silently discarding decoded pixels would corrupt the raster.
///
/// # Errors
/// Returns [`CompressionError::InvalidData`] if the decoded payload does not fit
/// in `dst`, and propagates any codec error.
pub fn decompress_into_partial(
    src: &[u8],
    compression: Compression,
    dst: &mut [u8],
) -> Result<usize> {
    match compression {
        // Uncompressed: a straight copy into the caller's buffer. Previously this
        // went through `data.to_vec()`, i.e. a full-size allocation plus copy that
        // the caller then had to copy out of again.
        Compression::None => {
            if src.len() > dst.len() {
                return Err(too_large(src.len(), dst.len()));
            }
            dst[..src.len()].copy_from_slice(src);
            Ok(src.len())
        }

        // PackBits expands run-length codes directly into `dst`.
        Compression::Packbits => decompress_packbits_into(src, dst),

        // DEFLATE inflates directly into `dst` (oxiarc-deflate 0.4.0+).
        #[cfg(feature = "deflate")]
        Compression::Deflate | Compression::AdobeDeflate => decompress_deflate_into(src, dst),

        // Every other codec returns an owned buffer from its decoder; copy it in.
        _ => {
            let decoded = decompress(src, compression, dst.len())?;
            if decoded.len() > dst.len() {
                return Err(too_large(decoded.len(), dst.len()));
            }
            dst[..decoded.len()].copy_from_slice(&decoded);
            Ok(decoded.len())
        }
    }
}

/// Builds the "decoded payload does not fit" error shared by the decode-into paths.
fn too_large(decoded: usize, capacity: usize) -> OxiGeoError {
    OxiGeoError::Compression(CompressionError::InvalidData {
        message: format!("decompressed length {decoded} exceeds destination length {capacity}"),
    })
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

/// DEFLATE (zlib-wrapped) decompression.
///
/// # The size hint
///
/// `expected_size` pre-sizes the output buffer so the decoder writes into one
/// allocation instead of growing a `Vec` by repeated doubling (a 1 MiB tile cost
/// five reallocations and five full copies of the bytes decoded so far). The hint
/// comes from IFD tags, i.e. from untrusted input, so it is clamped by
/// [`clamped_hint`] first.
///
/// The hint is an optimisation, never a constraint: if the payload turns out to
/// decode to more than the hint — a wrong tag, a clamped hint, or a caller that
/// passed `0` — this falls back to the growable path and returns exactly what that
/// path would have returned, including its errors. That fallback is also what
/// handles the one input `zlib_decompress_into` cannot take, a zlib stream with a
/// preset dictionary (never emitted by a TIFF encoder).
///
/// Requires oxiarc-deflate 0.4.0 for [`oxiarc_deflate::zlib::zlib_decompress_into`];
/// 0.3.6 had no decompress-into-slice or capacity-hint entry point at all.
#[cfg(feature = "deflate")]
fn decompress_deflate(data: &[u8], expected_size: usize) -> Result<Vec<u8>> {
    let hint = clamped_hint(expected_size);
    if hint > 0 {
        let mut out = vec![0u8; hint];
        if let Ok(written) = oxiarc_deflate::zlib::zlib_decompress_into(data, &mut out) {
            out.truncate(written);
            return Ok(out);
        }
    }

    oxiarc_deflate::zlib_decompress(data).map_err(|e| {
        OxiGeoError::Compression(CompressionError::DecompressionFailed {
            message: format!("DEFLATE decompression failed: {}", e),
        })
    })
}

/// DEFLATE decode straight into `dst`, with no intermediate `Vec` at all.
///
/// This is the whole-band read path: one caller-owned scratch buffer serves every
/// tile of the band, so a 4000×4000 DEFLATE raster performs zero decode-side
/// allocations rather than one growable `Vec` per tile.
///
/// Falls back to the owned-buffer path on any error so that failure modes — a
/// corrupt stream, a payload larger than `dst`, a preset dictionary — stay exactly
/// what [`decompress_into_partial`]'s generic arm produced before, error message
/// included. `dst` may have been partially written when that happens; the caller
/// is returning an error either way.
#[cfg(feature = "deflate")]
fn decompress_deflate_into(src: &[u8], dst: &mut [u8]) -> Result<usize> {
    if let Ok(written) = oxiarc_deflate::zlib::zlib_decompress_into(src, dst) {
        return Ok(written);
    }

    // Hint `0` deliberately: the sized attempt is the one that just failed, so go
    // straight to the growable path rather than repeating it.
    let decoded = decompress_deflate(src, 0)?;
    if decoded.len() > dst.len() {
        return Err(too_large(decoded.len(), dst.len()));
    }
    dst[..decoded.len()].copy_from_slice(&decoded);
    Ok(decoded.len())
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

/// ZSTD decompression.
///
/// `_expected_size` cannot be honoured: `oxiarc_zstd::decode_all` owns and sizes
/// its own output buffer, and oxiarc-zstd 0.4.0 exposes no capacity-hint or
/// decompress-into-slice entry point to pass the hint to (unlike
/// [`decompress_deflate`], which gained one in oxiarc-deflate 0.4.0).
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
///
/// `expected_size` is treated strictly as a *hint*: it sizes the output buffer
/// (clamped by [`clamped_hint`], since it derives from untrusted IFD tags) and
/// stops the expansion loop, but it is never trusted as the authoritative output
/// length — a stream that ends early yields a shorter buffer, exactly as before.
fn decompress_packbits(data: &[u8], expected_size: usize) -> Result<Vec<u8>> {
    let mut output = Vec::with_capacity(clamped_hint(expected_size));
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

/// PackBits decompression straight into a caller-owned buffer.
///
/// Mirrors [`decompress_packbits`] byte for byte (including its "unexpected end
/// of data" errors) but writes into `dst` instead of a fresh `Vec`, and refuses
/// to expand past the end of `dst` rather than growing without bound.
///
/// Returns the number of bytes written, which may be less than `dst.len()` if the
/// stream ends early.
fn decompress_packbits_into(data: &[u8], dst: &mut [u8]) -> Result<usize> {
    let mut written = 0usize;
    let mut i = 0;

    while i < data.len() && written < dst.len() {
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
            if written + count > dst.len() {
                return Err(too_large(written + count, dst.len()));
            }
            dst[written..written + count].copy_from_slice(&data[i..i + count]);
            written += count;
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
            if written + count > dst.len() {
                return Err(too_large(written + count, dst.len()));
            }
            dst[written..written + count].fill(byte);
            written += count;
        }
        // n == -128: no-op
    }

    Ok(written)
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

    #[cfg(feature = "deflate")]
    #[test]
    fn test_deflate_roundtrip() {
        let original = b"Hello, World! This is a test of DEFLATE compression.";
        let compressed = compress_deflate(original).expect("compression should work");
        let decompressed =
            decompress_deflate(&compressed, original.len()).expect("decompression should work");
        assert_eq!(&decompressed, original);
    }

    /// A payload comfortably larger than the decoder's 32 KiB history window and
    /// its old 64 KiB starting capacity, so the sized and growable paths differ in
    /// how many times they (would) reallocate — mixing incompressible noise with
    /// long runs to produce both literals and far back-references.
    #[cfg(feature = "deflate")]
    fn large_deflate_payload() -> Vec<u8> {
        let mut data = Vec::with_capacity(300 * 1024);
        let mut state = 0x1234_5678u32;
        while data.len() < 300 * 1024 {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            data.extend_from_slice(&state.to_le_bytes());
            data.extend(std::iter::repeat_n((state >> 24) as u8, 64));
        }
        data
    }

    /// cool-japan/oxigeo#14: the decoded-size hint added on top of
    /// oxiarc-deflate 0.4.0 is an optimisation, never a constraint. A hint that is
    /// exact, absent, too small, or too large must all yield identical bytes.
    #[cfg(feature = "deflate")]
    #[test]
    fn test_issue_14_deflate_size_hint_never_changes_output() {
        let original = large_deflate_payload();
        let compressed = compress_deflate(&original).expect("deflate encode");

        for hint in [
            original.len(),     // exact: takes the sized fast path
            0,                  // absent: goes straight to the growable path
            1,                  // far too small: fast path fails, falls back
            original.len() - 1, // off by one: fast path fails, falls back
            original.len() + 1, // slightly large: fast path succeeds, truncates
            original.len() * 4, // very large: fast path succeeds, truncates
            usize::MAX,         // hostile: clamped, then falls back
        ] {
            let decoded = decompress_deflate(&compressed, hint)
                .unwrap_or_else(|e| panic!("hint {hint} must decode: {e}"));
            assert_eq!(decoded, original, "hint {hint} changed the decoded bytes");
        }
    }

    /// cool-japan/oxigeo#14: DEFLATE now inflates straight into the caller's
    /// buffer. A payload shorter than `dst` must be reported, not rejected, and
    /// must leave the tail of `dst` untouched (the truncated-final-strip case).
    #[cfg(feature = "deflate")]
    #[test]
    fn test_issue_14_deflate_into_partial_leaves_tail_intact() {
        let original = large_deflate_payload();
        let compressed = compress_deflate(&original).expect("deflate encode");

        let mut dst = vec![0xAAu8; original.len() + 128];
        let written = decompress_into_partial(&compressed, Compression::Deflate, &mut dst)
            .expect("short payload must be accepted");

        assert_eq!(written, original.len());
        assert_eq!(&dst[..written], &original[..]);
        assert!(
            dst[written..].iter().all(|&b| b == 0xAA),
            "decode must not disturb the tail of the destination"
        );
    }

    /// cool-japan/oxigeo#14: a payload larger than `dst` must stay an error on the
    /// decode-into path — silently dropping decoded pixels would corrupt the
    /// raster. The inflate-into fast path reports this itself; this pins that it
    /// still surfaces as an error rather than a truncated success.
    #[cfg(feature = "deflate")]
    #[test]
    fn test_issue_14_deflate_into_rejects_oversized_payload() {
        let original = large_deflate_payload();
        let compressed = compress_deflate(&original).expect("deflate encode");

        let mut dst = vec![0u8; original.len() / 2];
        decompress_into_partial(&compressed, Compression::Deflate, &mut dst)
            .expect_err("payload larger than dst must fail");
    }

    /// cool-japan/oxigeo#14: corrupt input must still fail, and fail the same way,
    /// through the inflate-into path as through the owned-buffer path.
    #[cfg(feature = "deflate")]
    #[test]
    fn test_issue_14_deflate_into_rejects_corrupt_stream() {
        let original = large_deflate_payload();
        let mut compressed = compress_deflate(&original).expect("deflate encode");
        // Corrupt the middle of the stream, leaving the zlib header intact so the
        // failure comes from the inflate body rather than header validation.
        let mid = compressed.len() / 2;
        compressed[mid] ^= 0xFF;

        let mut dst = vec![0u8; original.len()];
        let into_err = decompress_into_partial(&compressed, Compression::Deflate, &mut dst);
        let owned_err = decompress_deflate(&compressed, 0);
        assert_eq!(
            into_err.is_err(),
            owned_err.is_err(),
            "decode-into and owned paths must agree on whether a corrupt stream fails"
        );
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

    /// Payload used by the `decompress_into` equivalence tests: long enough to
    /// exercise real codec paths (literals, runs, back-references), and with a
    /// repetitive tail so PackBits produces both literal and repeat runs.
    fn sample_payload() -> Vec<u8> {
        let mut data: Vec<u8> = (0..=255u8).collect();
        data.extend(std::iter::repeat_n(0x42u8, 300));
        data.extend((0..=255u8).rev());
        data
    }

    /// cool-japan/oxigeo#14: for every supported `Compression` variant,
    /// `decompress_into` must produce exactly what `decompress` produces.
    #[test]
    fn test_issue_14_decompress_into_matches_decompress() {
        let original = sample_payload();

        // (variant, compressed bytes) for every codec this build supports.
        let mut cases: Vec<(Compression, Vec<u8>)> = vec![
            (Compression::None, original.clone()),
            (
                Compression::Packbits,
                compress(&original, Compression::Packbits).expect("packbits encode"),
            ),
        ];
        #[cfg(feature = "deflate")]
        {
            let encoded = compress(&original, Compression::Deflate).expect("deflate encode");
            cases.push((Compression::Deflate, encoded.clone()));
            cases.push((Compression::AdobeDeflate, encoded));
        }
        #[cfg(feature = "lzw")]
        cases.push((
            Compression::Lzw,
            compress(&original, Compression::Lzw).expect("lzw encode"),
        ));
        #[cfg(feature = "zstd")]
        cases.push((
            Compression::Zstd,
            compress(&original, Compression::Zstd).expect("zstd encode"),
        ));
        #[cfg(feature = "webp")]
        {
            // WebP needs explicit dimensions: 16x16 RGB = 768 bytes.
            let pixels: Vec<u8> = (0..768u32).map(|i| (i % 251) as u8).collect();
            let encoded = compress_webp_with_params(&pixels, 16, 16, image_webp::ColorType::Rgb8)
                .expect("webp encode");
            cases.push((Compression::WebP, encoded));
        }
        #[cfg(feature = "jpeg")]
        {
            // JPEG is lossy, but `decompress_into` must still agree with
            // `decompress` byte for byte on the same input.
            let pixels: Vec<u8> = (0..64u32).map(|i| (i * 4) as u8).collect();
            let encoded =
                compress_jpeg_with_params(&pixels, 8, 8, jpeg_encoder::ColorType::Luma, 85)
                    .expect("jpeg encode");
            cases.push((Compression::Jpeg, encoded));
        }
        // LERC: a minimal constant-image blob (see `test_lerc_dispatch_decodes_constant_image`).
        cases.push((Compression::Lerc, lerc_constant_blob()));

        for (compression, compressed) in cases {
            let expected = decompress(&compressed, compression, original.len())
                .unwrap_or_else(|e| panic!("decompress failed for {compression:?}: {e}"));

            let mut dst = vec![0xAAu8; expected.len()];
            decompress_into(&compressed, compression, &mut dst)
                .unwrap_or_else(|e| panic!("decompress_into failed for {compression:?}: {e}"));
            assert_eq!(
                dst, expected,
                "decompress_into mismatch for {compression:?}"
            );

            // The partial variant must report the same length.
            let mut dst = vec![0xAAu8; expected.len()];
            let written = decompress_into_partial(&compressed, compression, &mut dst)
                .unwrap_or_else(|e| panic!("partial failed for {compression:?}: {e}"));
            assert_eq!(written, expected.len(), "written mismatch {compression:?}");
            assert_eq!(dst, expected, "partial mismatch for {compression:?}");
        }
    }

    /// cool-japan/oxigeo#14: an unknown compression method must fail through the
    /// decode-into path exactly as it does through [`decompress`].
    #[test]
    fn test_issue_14_decompress_into_rejects_unknown_method() {
        let mut dst = [0u8; 8];
        let err = decompress_into(&[0u8; 4], Compression::Lzma, &mut dst)
            .expect_err("unknown method must fail");
        assert!(
            format!("{err}").contains("Unknown compression method"),
            "unexpected error: {err}"
        );
    }

    /// cool-japan/oxigeo#14: `decompress_into` is strict about length — a payload
    /// that is shorter or longer than `dst` is a typed error, while
    /// `decompress_into_partial` accepts (and reports) a short payload.
    #[test]
    fn test_issue_14_decompress_into_length_mismatch() {
        let payload = b"0123456789";

        // Destination too large for the payload.
        let mut dst = [0u8; 16];
        let err = decompress_into(payload, Compression::None, &mut dst)
            .expect_err("short payload must be rejected");
        assert!(
            format!("{err}").contains("does not match destination length"),
            "unexpected error: {err}"
        );
        // ... but the partial variant accepts it and reports the true length.
        let mut dst = [0u8; 16];
        let written = decompress_into_partial(payload, Compression::None, &mut dst)
            .expect("short payload is fine for the partial variant");
        assert_eq!(written, payload.len());
        assert_eq!(&dst[..written], payload);
        assert!(dst[written..].iter().all(|&b| b == 0), "tail untouched");

        // Destination too small for the payload.
        let mut dst = [0u8; 4];
        let err = decompress_into(payload, Compression::None, &mut dst)
            .expect_err("oversized payload must be rejected");
        assert!(
            format!("{err}").contains("exceeds destination length"),
            "unexpected error: {err}"
        );
        let mut dst = [0u8; 4];
        let err = decompress_into_partial(payload, Compression::None, &mut dst)
            .expect_err("oversized payload must be rejected by the partial variant too");
        assert!(
            format!("{err}").contains("exceeds destination length"),
            "unexpected error: {err}"
        );
    }

    /// cool-japan/oxigeo#14: the PackBits decode-into path must reproduce the
    /// `Vec`-returning decoder exactly, including its truncated-stream error.
    #[test]
    fn test_issue_14_packbits_into_matches_vec_decoder() {
        let original = sample_payload();
        let compressed = compress_packbits(&original).expect("packbits encode");

        let expected = decompress_packbits(&compressed, original.len()).expect("packbits decode");
        assert_eq!(expected, original);

        let mut dst = vec![0u8; original.len()];
        let written = decompress_packbits_into(&compressed, &mut dst).expect("packbits into");
        assert_eq!(written, expected.len());
        assert_eq!(dst, expected);

        // A literal run that claims more bytes than remain must error in both.
        let truncated = [0x05u8, 1, 2];
        let vec_err = decompress_packbits(&truncated, 6).expect_err("vec decoder must error");
        let mut dst = [0u8; 6];
        let into_err = decompress_packbits_into(&truncated, &mut dst).expect_err("into must error");
        assert_eq!(format!("{vec_err}"), format!("{into_err}"));
        assert!(format!("{into_err}").contains("PackBits: unexpected end of data"));
    }

    /// cool-japan/oxigeo#14: an untrusted decoded-size hint must not drive an
    /// unbounded speculative allocation.
    #[test]
    fn test_issue_14_size_hint_is_clamped() {
        assert_eq!(clamped_hint(0), 0);
        assert_eq!(clamped_hint(1024), 1024);
        assert_eq!(clamped_hint(MAX_SIZE_HINT_BYTES), MAX_SIZE_HINT_BYTES);
        assert_eq!(clamped_hint(usize::MAX), MAX_SIZE_HINT_BYTES);

        // A hostile hint must not abort the process; decoding still succeeds and
        // the output length is decided by the data, never by the hint.
        let compressed = compress_packbits(b"abc").expect("packbits encode");
        let out = decompress_packbits(&compressed, usize::MAX).expect("hostile hint tolerated");
        assert_eq!(out, b"abc");
    }

    /// Minimal checksum-free LERC2 v2 constant-image blob (2x2 Float, all valid,
    /// `zMin == zMax`), shared by the LERC tests.
    fn lerc_constant_blob() -> Vec<u8> {
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
        blob
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
    /// `decompress` dispatch for `Compression::Lerc` (TIFF tag 34887) into
    /// **host**-order f32 sample bytes — not the `UnknownMethod` fall-through.
    ///
    /// Host order, not little-endian: the LERC decoder is excluded from the
    /// byte-order normalisation (`crate::decoded_needs_native_swap`) on the
    /// grounds that it already emits native samples, so that is what it must be
    /// asserted to do.
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
            .map(|c| f32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
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
