//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use crate::fetch::FetchBackend;
use oxigeo_core::error::{IoError, OxiGeoError, Result};
use oxigeo_core::io::ByteRange;
use std::future::Future;

use super::constants::{PHOTOMETRIC_TRANSPARENCY_MASK, SUBFILE_TYPE_TRANSPARENCY_MASK};
use super::types::IfdMetadata;

/// Byte-range source behind the IFD parser.
///
/// In production this is always [`FetchBackend`] (HTTP range requests). The
/// indirection exists so the IFD chain walk — pure byte parsing once the bytes
/// are in hand — can be driven natively over an in-memory buffer in unit tests,
/// where `web_sys::fetch` is unavailable. It is deliberately private and
/// confined to the parse path; [`WasmCogReader`] itself keeps its concrete
/// `FetchBackend`, so no public type gains a parameter.
///
/// Declared with an `-> impl Future` return rather than `async fn` so the trait
/// stays clear of the `async_fn_in_trait` lint.
pub(super) trait RangeSource {
    /// Reads `range` from the source. A range reaching past the end of the
    /// source yields the available prefix — that is what an HTTP server does
    /// for an over-long range, and the IFD reads below rely on it (they always
    /// ask for a fixed 4 KiB window).
    fn read_range(&self, range: ByteRange) -> impl Future<Output = Result<Vec<u8>>>;
}

impl RangeSource for FetchBackend {
    fn read_range(&self, range: ByteRange) -> impl Future<Output = Result<Vec<u8>>> {
        self.read_range_async(range)
    }
}

/// Is this IFD a transparency mask rather than a pyramid level?
///
/// A GDAL internal mask (`GDAL_TIFFBuildOverviews` / `CreateMaskBand`) is
/// stored as an ordinary IFD in the same chain as the overviews, so a naive
/// chain walk counts it as an extra overview level — inflating
/// `overview_count` and shifting every subsequent level's index, which makes
/// callers read the wrong resolution. Either marker alone is conclusive:
/// `NewSubfileType` bit 2 (TIFF 6.0 §Baseline, tag 254) or
/// `PhotometricInterpretation == 4`.
pub(super) fn is_mask_ifd(subfile_type: u64, photometric: u16) -> bool {
    (subfile_type & SUBFILE_TYPE_TRANSPARENCY_MASK) != 0
        || photometric == PHOTOMETRIC_TRANSPARENCY_MASK
}

/// Byte range of one tile, resolved against the tile directory of the level it
/// belongs to.
///
/// The tile grid width comes from `lvl`'s *own* width and tile width, so
/// `(tile_x, tile_y)` are always interpreted in that level's geometry — a
/// level-1 read indexes level 1's (halved) grid and its own `tile_offsets`,
/// never level 0's. `level` is carried only to name itself in the error.
pub(super) fn tile_byte_range(
    lvl: &IfdMetadata,
    level: usize,
    tile_x: u32,
    tile_y: u32,
) -> Result<ByteRange> {
    let tiles_across = lvl.width.div_ceil(u64::from(lvl.tile_width)) as u32;
    let tile_index = (tile_y * tiles_across + tile_x) as usize;

    if tile_index >= lvl.tile_offsets.len() || tile_index >= lvl.tile_byte_counts.len() {
        return Err(OxiGeoError::OutOfBounds {
            message: format!("Tile index {} out of range at level {}", tile_index, level),
        });
    }

    Ok(ByteRange::from_offset_length(
        lvl.tile_offsets[tile_index],
        lvl.tile_byte_counts[tile_index],
    ))
}

/// Decompress a raw tile payload according to its TIFF compression code.
///
/// Handles the codecs relevant to Sentinel-2 / GeoLab COGs: `1` (uncompressed),
/// `8` and `32946` (both Zlib-wrapped DEFLATE — `32946` is an older, pre-Adobe
/// code for the same stream layout), `5` (LZW) and `32773` (PackBits). Any
/// other code returns a typed [`OxiGeoError::NotSupported`].
///
/// `expected_size` is the byte length of one fully-decoded tile (`tile_width *
/// tile_height * samples_per_pixel * bits_per_sample` bits, rounded up — see
/// [`expected_tile_byte_size`]). LZW and PackBits need it to know how much
/// output to produce; it is derived from the tile's own IFD-declared geometry,
/// so codecs that use it treat it as a hint/bound rather than as a promise the
/// compressed stream actually contains that many bytes.
pub(super) fn decompress_tile(
    compressed: Vec<u8>,
    compression: u16,
    expected_size: usize,
) -> Result<Vec<u8>> {
    match compression {
        1 => Ok(compressed), // No compression
        5 => oxiarc_lzw::decompress_tiff(&compressed, expected_size).map_err(|e| {
            OxiGeoError::Io(IoError::Read {
                message: format!("LZW decompression failed: {}", e),
            })
        }),
        8 | 32946 => oxiarc_deflate::zlib_decompress(&compressed).map_err(|e| {
            OxiGeoError::Io(IoError::Read {
                message: format!("DEFLATE decompression failed: {}", e),
            })
        }),
        32773 => decompress_packbits(&compressed, expected_size),
        other => Err(OxiGeoError::NotSupported {
            operation: format!("Compression type {} not supported", other),
        }),
    }
}

/// Byte length of one fully-decoded, uncompressed tile at `lvl`: `tile_width *
/// tile_height * samples_per_pixel * bits_per_sample` bits, rounded up to
/// whole bytes. Used as the expected-output-size hint threaded into
/// [`decompress_tile`] for codecs (LZW, PackBits) that need to know how much
/// data to produce.
pub(super) fn expected_tile_byte_size(lvl: &IfdMetadata) -> usize {
    let total_bits = u64::from(lvl.tile_width)
        .saturating_mul(u64::from(lvl.tile_height))
        .saturating_mul(u64::from(lvl.samples_per_pixel))
        .saturating_mul(u64::from(lvl.bits_per_sample));
    total_bits.div_ceil(8) as usize
}

/// Decompress a PackBits (TIFF Compression `32773`) tile payload.
///
/// PackBits is a byte-oriented RLE: each control byte `n` (read as signed
/// `i8`) is followed either by `n + 1` literal bytes (`0 <= n <= 127`) or by
/// one byte repeated `1 - n` times (`-127 <= n <= -1`); `n == -128` is a
/// no-op padding byte with nothing following it.
///
/// `expected_size` comes from the tile's own IFD-declared geometry (see
/// [`expected_tile_byte_size`]), which is untrusted input — a compressed
/// stream is never assumed to agree with it. It is used only to cap
/// allocation and stop expansion once `expected_size` bytes have been
/// produced; a run that would overshoot it is truncated rather than
/// over-allocated. Every byte the decoder reads from `data` is bounds-checked,
/// so a truncated or adversarial stream returns an error instead of
/// panicking.
pub(super) fn decompress_packbits(data: &[u8], expected_size: usize) -> Result<Vec<u8>> {
    fn truncated() -> OxiGeoError {
        OxiGeoError::Io(IoError::Read {
            message: "PackBits: unexpected end of data".to_string(),
        })
    }

    let mut out = Vec::with_capacity(expected_size.min(64 * 1024 * 1024));
    let mut i = 0usize;

    while i < data.len() && out.len() < expected_size {
        let n = data[i] as i8;
        i += 1;

        if n >= 0 {
            // Literal run: the next n+1 bytes are copied verbatim.
            let count = n as usize + 1;
            let end = i
                .checked_add(count)
                .filter(|&end| end <= data.len())
                .ok_or_else(truncated)?;
            let remaining = expected_size - out.len();
            let take = count.min(remaining);
            out.extend_from_slice(&data[i..i + take]);
            i = end;
        } else if n > -128 {
            // Repeat run: the next single byte is repeated 1-n times.
            let count = (1 - i16::from(n)) as usize;
            let byte = *data.get(i).ok_or_else(truncated)?;
            i += 1;
            let remaining = expected_size - out.len();
            out.extend(std::iter::repeat_n(byte, count.min(remaining)));
        }
        // n == -128: no-op padding byte, consumes nothing further.
    }

    Ok(out)
}

/// Normalise a raw GeoTIFF `ModelPixelScaleTag` (tag 33550) Y component to
/// this crate's storage convention: a positive magnitude.
///
/// Split out of the tag-33550 arm in [`WasmCogReader::parse_ifd`] purely so
/// the transform is reachable from a native unit test — `parse_ifd` itself
/// needs a live `FetchBackend` to fetch the tag's out-of-line `DOUBLE` array
/// (any count >= 1 exceeds the 4-byte inline field) and so cannot run outside
/// a browser, the same reason [`finish_tile_decode`] was split out of
/// `read_tile_level`.
///
/// The GeoTIFF spec defines `ModelPixelScaleTag` as strictly positive, and
/// conforming writers (GDAL included) store it that way; a small number of
/// nonconforming writers instead bake the north-up sign (a negative Y step)
/// directly into the tag. Normalising here keeps this URL-backed path in
/// agreement with the `openBytes` path, which already applies the same
/// `.abs()` to its geo-transform-derived pixel height. Neither path computes a
/// pixel-to-CRS affine transform, so re-applying the northing sign when
/// building one is the consumer's responsibility.
pub(super) fn normalize_pixel_scale_y(raw: f64) -> f64 {
    raw.abs()
}

/// Reinterpret a decoded tile byte buffer as `u16` samples.
///
/// The input comes from [`WasmCogReader::read_tile_level`], which has already
/// normalised samples to host order, so this is a plain `from_ne_bytes` — it
/// takes no byte-order argument precisely so that it cannot undo that
/// normalisation (cool-japan/oxigeo#14).
///
/// A trailing odd byte (if any) is dropped via `chunks_exact`.
#[allow(dead_code)] // Used by read_window_u16 (A4 pipeline surface)
pub(super) fn bytes_to_u16(bytes: &[u8]) -> Vec<u16> {
    bytes
        .chunks_exact(2)
        .map(|c| u16::from_ne_bytes([c[0], c[1]]))
        .collect()
}

/// Finish decoding one decompressed tile: undo the predictor, then normalise.
///
/// Split out of [`WasmCogReader::read_tile_level`] so that the *ordering* of
/// these two steps — not merely the existence of each — is reachable from a
/// native unit test, since `read_tile_level` itself needs a browser `fetch`.
/// The order is load-bearing and not interchangeable:
///
/// 1. TIFF's horizontal differencing predictor is defined over samples in the
///    **file's** byte order (TIFF 6.0 §14), so it must be undone first.
/// 2. Only then are samples rewritten into the **host's** order, which is the
///    contract every caller above this function relies on.
///
/// Swapping the two silently corrupts predicted `MM` tiles; dropping step 2
/// re-splits the crate into two byte-order contracts, which is precisely the
/// bug cool-japan/oxigeo#14 removed.
pub(super) fn finish_tile_decode(data: &mut [u8], lvl: &IfdMetadata, file_is_little_endian: bool) {
    if lvl.predictor == 2 {
        apply_horizontal_predictor(
            data,
            lvl.tile_width,
            lvl.tile_height,
            lvl.bits_per_sample,
            lvl.samples_per_pixel,
            file_is_little_endian,
        );
    }
    normalize_samples_to_native(data, lvl.bits_per_sample, file_is_little_endian);
}

/// Rewrite a decoded tile's samples from the file's byte order into the host's.
///
/// This is the crate's one and only sample byte-swap, called at the end of
/// [`WasmCogReader::read_tile_level`]. Everything downstream — `bytes_to_u16`,
/// the window assemblers, `crate::WasmCogViewer` and its elevation decoder —
/// reads host-native and must stay that way; a second swap anywhere above this
/// line silently corrupts every `MM` file (cool-japan/oxigeo#14).
///
/// Only 16-, 32- and 64-bit samples are swapped. 8-bit samples have nothing to
/// swap, and any other `BitsPerSample` (sub-byte packing, or an exotic width
/// like 24) has no defined sample boundary to swap across, so both pass through
/// untouched — the same scope `oxigeo_geotiff`'s normalisation uses.
pub(super) fn normalize_samples_to_native(
    data: &mut [u8],
    bits_per_sample: u16,
    file_is_little_endian: bool,
) {
    if file_is_little_endian == cfg!(target_endian = "little") {
        return;
    }
    let sample_bytes = match bits_per_sample {
        16 => 2usize,
        32 => 4,
        64 => 8,
        _ => return,
    };
    for sample in data.chunks_exact_mut(sample_bytes) {
        sample.reverse();
    }
}

/// Undo TIFF horizontal differencing (Predictor 2) for one row of 16-bit
/// single-sample data: each sample becomes the running sum of the deltas.
///
/// This is the single-band (`samples_per_pixel == 1`) case used by Sentinel-2
/// reflectance bands. Wrapping addition matches the encoder's wrapping
/// subtraction so the transform is exactly invertible.
pub(super) fn undo_horizontal_predictor_u16(row: &mut [u16]) {
    for i in 1..row.len() {
        row[i] = row[i].wrapping_add(row[i - 1]);
    }
}

/// Undo TIFF horizontal differencing (Predictor 2) for one row of 8-bit data
/// with `spp` interleaved samples per pixel (e.g. `spp == 3` for TCI RGB).
///
/// Each sample references the sample `spp` positions earlier so channels are
/// reconstructed independently. Wrapping addition mirrors the encoder.
pub(super) fn undo_horizontal_predictor_u8(row: &mut [u8], spp: usize) {
    let spp = spp.max(1);
    for i in spp..row.len() {
        row[i] = row[i].wrapping_add(row[i - spp]);
    }
}

/// Undo the horizontal predictor over a whole decoded tile, in place.
///
/// Operates row by row using the tile geometry and sample layout. Supports
/// 8-bit and 16-bit samples; other bit depths are left untouched (no predictor
/// support). For 16-bit data the row is decoded/re-encoded using the reader's
/// byte order.
pub(super) fn apply_horizontal_predictor(
    data: &mut [u8],
    tile_width: u32,
    tile_height: u32,
    bits_per_sample: u16,
    samples_per_pixel: u16,
    little_endian: bool,
) {
    let tw = tile_width as usize;
    let th = tile_height as usize;
    let spp = samples_per_pixel.max(1) as usize;

    match bits_per_sample {
        16 => {
            let row_samples = tw * spp;
            let row_bytes = row_samples * 2;
            for r in 0..th {
                let start = r * row_bytes;
                let end = start + row_bytes;
                if end > data.len() {
                    break;
                }
                let mut row: Vec<u16> = data[start..end]
                    .chunks_exact(2)
                    .map(|c| {
                        if little_endian {
                            u16::from_le_bytes([c[0], c[1]])
                        } else {
                            u16::from_be_bytes([c[0], c[1]])
                        }
                    })
                    .collect();

                if spp == 1 {
                    undo_horizontal_predictor_u16(&mut row);
                } else {
                    for i in spp..row.len() {
                        row[i] = row[i].wrapping_add(row[i - spp]);
                    }
                }

                for (i, &v) in row.iter().enumerate() {
                    let b = start + i * 2;
                    let out = if little_endian {
                        v.to_le_bytes()
                    } else {
                        v.to_be_bytes()
                    };
                    data[b] = out[0];
                    data[b + 1] = out[1];
                }
            }
        }
        8 => {
            let row_bytes = tw * spp;
            for r in 0..th {
                let start = r * row_bytes;
                let end = start + row_bytes;
                if end > data.len() {
                    break;
                }
                undo_horizontal_predictor_u8(&mut data[start..end], spp);
            }
        }
        _ => {}
    }
}

/// Assemble decoded 16-bit tiles into a dense `w × h` row-major window.
///
/// Pure and natively testable. Each tile is given as
/// `(tile_x, tile_y, samples)` where `samples` are the tile's `u16` values in
/// raster order (`tile_width × tile_height`). Pixels of a tile that fall inside
/// the window `[x0, x0+w) × [y0, y0+h)` (all in level pixel coordinates) are
/// scattered into the output; window pixels not covered by any supplied tile
/// (off-grid overhang) remain zero.
#[allow(dead_code)] // Used by read_window_u16 (A4 pipeline surface)
pub(super) fn assemble_window(
    tiles: &[(u32, u32, Vec<u16>)],
    tile_width: u32,
    tile_height: u32,
    x0: u64,
    y0: u64,
    w: u32,
    h: u32,
) -> Vec<u16> {
    let w_usize = w as usize;
    let mut out = vec![0u16; w_usize * h as usize];
    let tw = tile_width as u64;
    let th = tile_height as u64;
    let x1 = x0 + w as u64;
    let y1 = y0 + h as u64;

    for (tx, ty, data) in tiles {
        let origin_x = *tx as u64 * tw;
        let origin_y = *ty as u64 * th;
        for row in 0..th {
            let gy = origin_y + row;
            if gy < y0 || gy >= y1 {
                continue;
            }
            let out_y = (gy - y0) as usize;
            for col in 0..tw {
                let gx = origin_x + col;
                if gx < x0 || gx >= x1 {
                    continue;
                }
                let src_idx = (row * tw + col) as usize;
                if src_idx < data.len() {
                    let out_x = (gx - x0) as usize;
                    out[out_y * w_usize + out_x] = data[src_idx];
                }
            }
        }
    }

    out
}

/// Assemble decoded 8-bit RGB tiles into a dense `w × h × 3` interleaved
/// row-major window. RGB analogue of [`assemble_window`]; each tile's bytes are
/// `tile_width × tile_height × 3` in raster order.
#[allow(dead_code)] // Used by read_window_rgb8 (A4 pipeline surface)
pub(super) fn assemble_window_rgb8(
    tiles: &[(u32, u32, Vec<u8>)],
    tile_width: u32,
    tile_height: u32,
    x0: u64,
    y0: u64,
    w: u32,
    h: u32,
) -> Vec<u8> {
    let w_usize = w as usize;
    let mut out = vec![0u8; w_usize * h as usize * 3];
    let tw = tile_width as u64;
    let th = tile_height as u64;
    let x1 = x0 + w as u64;
    let y1 = y0 + h as u64;

    for (tx, ty, data) in tiles {
        let origin_x = *tx as u64 * tw;
        let origin_y = *ty as u64 * th;
        for row in 0..th {
            let gy = origin_y + row;
            if gy < y0 || gy >= y1 {
                continue;
            }
            let out_y = (gy - y0) as usize;
            for col in 0..tw {
                let gx = origin_x + col;
                if gx < x0 || gx >= x1 {
                    continue;
                }
                let src_idx = ((row * tw + col) as usize) * 3;
                if src_idx + 3 <= data.len() {
                    let out_x = (gx - x0) as usize;
                    let dst = (out_y * w_usize + out_x) * 3;
                    out[dst] = data[src_idx];
                    out[dst + 1] = data[src_idx + 1];
                    out[dst + 2] = data[src_idx + 2];
                }
            }
        }
    }

    out
}
