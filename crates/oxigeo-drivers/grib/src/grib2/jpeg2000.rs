//! GRIB2 DRT 5.40: JPEG2000-packed data decoding.
//!
//! GRIB2 Data Representation Template 5.40 stores the (already scaled) integer
//! field values `X` as a raw JPEG2000 codestream in Section 7. Decoding a 5.40
//! message therefore means:
//!
//! 1. decode the JPEG2000 codestream into its single grayscale component's
//!    integer samples (`X`); then
//! 2. apply the GRIB2 `(R + X * 2^E) / 10^D` scaling to each sample.
//!
//! The codestream is decoded with the Pure-Rust [`oxigeo_jpeg2000`] driver via
//! its public low-level primitives (SIZ/COD/QCD parsing +
//! [`oxigeo_jpeg2000::tier2::tile::decode_tile_components`]). GRIB2 JPEG2000
//! payloads are always a single tile with a single component, which is the
//! path exercised here. Any decode failure — including the sub-cases the
//! decoder does not yet support (9/7 irreversible wavelet, multi-layer
//! streams, custom precincts) — surfaces as a typed [`GribError`] rather than
//! silently returning corrupt values.

use crate::error::{GribError, Result};
use byteorder::{BigEndian, ReadBytesExt};
use oxigeo_jpeg2000::codestream::{CodestreamParser, Marker};
use oxigeo_jpeg2000::tier2::tile::{TileComponentInput, TileDecodeParams, decode_tile_components};
use std::io::{Cursor, Seek, SeekFrom};

/// Maps a JPEG2000 decode error into a GRIB decoding error.
fn map_j2k_err(e: oxigeo_jpeg2000::Jpeg2000Error) -> GribError {
    GribError::DecodingError(format!("DRT 5.40 JPEG2000 decode failed: {e}"))
}

/// Applies the `(R + X * 2^E) / 10^D` GRIB2 scaling formula to a sample.
#[inline]
fn apply_scale(x: i32, reference_value: f32, binary_scale: i16, decimal_scale: i16) -> f32 {
    let two_e = 2.0f64.powi(binary_scale as i32);
    let ten_d = 10.0f64.powi(decimal_scale as i32);
    ((reference_value as f64 + x as f64 * two_e) / ten_d) as f32
}

/// Decodes a DRT 5.40 JPEG2000 Section 7 payload into scaled `f32` values.
///
/// Returns exactly `min(num_points, decoded_samples)` values, scaled by the
/// GRIB2 `(R + X * 2^E) / 10^D` formula. `codestream` must be a raw J2K
/// codestream beginning with the SOC marker.
pub fn decode_jpeg2000_values(
    codestream: &[u8],
    reference_value: f32,
    binary_scale_factor: i16,
    decimal_scale_factor: i16,
    num_points: usize,
) -> Result<Vec<f32>> {
    let samples = decode_single_component(codestream)?;

    let count = samples.len().min(num_points);
    let mut out = Vec::with_capacity(count);
    for &x in samples.iter().take(count) {
        out.push(apply_scale(
            x,
            reference_value,
            binary_scale_factor,
            decimal_scale_factor,
        ));
    }
    Ok(out)
}

/// Decodes the single grayscale component of a GRIB2 JPEG2000 codestream into
/// its integer samples (`X`).
fn decode_single_component(codestream: &[u8]) -> Result<Vec<i32>> {
    let header = parse_main_header(codestream)?;

    if header.image_size.num_tiles() != 1 {
        return Err(GribError::DecodingError(format!(
            "DRT 5.40: multi-tile JPEG2000 codestream ({} tiles) is not supported; \
             GRIB2 fields are single-tile",
            header.image_size.num_tiles()
        )));
    }

    let num_components = header.image_size.num_components as usize;
    let tile_w = header.image_size.tile_width as usize;
    let tile_h = header.image_size.tile_height as usize;

    let mut comp_inputs = Vec::with_capacity(num_components);
    for comp in 0..num_components {
        let dx = usize::from(
            header
                .image_size
                .components
                .get(comp)
                .map(|c| c.dx)
                .unwrap_or(1),
        )
        .max(1);
        let dy = usize::from(
            header
                .image_size
                .components
                .get(comp)
                .map(|c| c.dy)
                .unwrap_or(1),
        )
        .max(1);
        let precision = header
            .image_size
            .components
            .get(comp)
            .map(|c| c.precision)
            .unwrap_or(16);
        comp_inputs.push(TileComponentInput {
            comp_w: tile_w.div_ceil(dx).max(1),
            comp_h: tile_h.div_ceil(dy).max(1),
            precision,
        });
    }

    let guard_bits = header
        .quantization
        .as_ref()
        .map(|q| q.guard_bits)
        .unwrap_or(2);

    let params = TileDecodeParams {
        components: &comp_inputs,
        num_levels: u32::from(header.coding_style.num_levels),
        cbw: header.coding_style.code_block_width_px(),
        cbh: header.coding_style.code_block_height_px(),
        progression: header.coding_style.progression_order,
        num_layers: header.coding_style.num_layers,
        guard_bits,
        quantization: header.quantization.as_ref(),
        has_sop: header.coding_style.has_sop,
        has_eph: header.coding_style.has_eph,
    };

    let components = decode_tile_components(&header.tile_data, &params).map_err(map_j2k_err)?;

    // GRIB2 JPEG2000 fields carry a single grayscale component whose samples
    // are the scaled integer values X.
    components.into_iter().next().ok_or_else(|| {
        GribError::DecodingError("DRT 5.40: JPEG2000 decode produced no components".to_string())
    })
}

/// The main-header markers plus the single tile's packet data.
struct J2kHeader {
    image_size: oxigeo_jpeg2000::codestream::ImageSize,
    coding_style: oxigeo_jpeg2000::codestream::CodingStyle,
    quantization: Option<oxigeo_jpeg2000::codestream::Quantization>,
    tile_data: Vec<u8>,
}

/// Parses the SOC marker and the main-header marker segments (SIZ, COD, QCD,
/// skipping the rest) up to the first tile-part, then extracts that tile's
/// packet data bytes (everything after its SOD marker, up to the end of the
/// tile-part or a trailing EOC).
fn parse_main_header(codestream: &[u8]) -> Result<J2kHeader> {
    let mut cursor = Cursor::new(codestream);

    // SOC (0xFF4F) opens every codestream.
    let soc = read_u16(&mut cursor)?;
    if soc != Marker::Soc as u16 {
        return Err(GribError::DecodingError(format!(
            "DRT 5.40: expected JPEG2000 SOC marker 0xFF4F, found 0x{soc:04X}"
        )));
    }

    let mut image_size = None;
    let mut coding_style = None;
    let mut quantization = None;

    loop {
        let marker = read_u16(&mut cursor)?;
        match marker {
            m if m == Marker::Siz as u16 => {
                let mut parser = CodestreamParser::new(&mut cursor);
                image_size = Some(parser.parse_siz().map_err(map_j2k_err)?);
            }
            m if m == Marker::Cod as u16 => {
                let mut parser = CodestreamParser::new(&mut cursor);
                coding_style = Some(parser.parse_cod().map_err(map_j2k_err)?);
            }
            m if m == Marker::Qcd as u16 => {
                let mut parser = CodestreamParser::new(&mut cursor);
                quantization = Some(parser.parse_qcd().map_err(map_j2k_err)?);
            }
            m if m == Marker::Sot as u16 => {
                let tile_data = extract_tile_data(&mut cursor, codestream)?;
                let image_size = image_size.ok_or_else(|| {
                    GribError::DecodingError(
                        "DRT 5.40: JPEG2000 codestream missing SIZ".to_string(),
                    )
                })?;
                let coding_style = coding_style.ok_or_else(|| {
                    GribError::DecodingError(
                        "DRT 5.40: JPEG2000 codestream missing COD".to_string(),
                    )
                })?;
                return Ok(J2kHeader {
                    image_size,
                    coding_style,
                    quantization,
                    tile_data,
                });
            }
            m if m == Marker::Eoc as u16 => {
                return Err(GribError::DecodingError(
                    "DRT 5.40: JPEG2000 codestream ended before any tile-part".to_string(),
                ));
            }
            _ => {
                // Unknown/other main-header marker: skip its segment.
                let seg_len = read_u16(&mut cursor)?;
                if seg_len < 2 {
                    return Err(GribError::DecodingError(format!(
                        "DRT 5.40: invalid JPEG2000 marker segment length {seg_len}"
                    )));
                }
                seek_forward(&mut cursor, i64::from(seg_len) - 2)?;
            }
        }
    }
}

/// From a cursor positioned immediately after an SOT marker, parses the
/// tile-part header, walks its inner marker segments to the SOD marker, and
/// returns the packet-data byte range that follows (bounded by the tile-part
/// length, and with a trailing EOC stripped).
fn extract_tile_data(cursor: &mut Cursor<&[u8]>, codestream: &[u8]) -> Result<Vec<u8>> {
    let lsot = read_u16(cursor)?;
    let _isot = read_u16(cursor)?;
    let psot = read_u32(cursor)?;
    let _tpsot = read_u8(cursor)?;
    let _tnsot = read_u8(cursor)?;

    // Psot counts from the SOT marker; recover the SOT start offset (we have
    // consumed the 2-byte marker plus `lsot` bytes of the SOT segment).
    let cur = cursor.position() as usize;
    let sot_start = cur.saturating_sub(usize::from(lsot)).saturating_sub(2);
    let tile_part_end = if psot > 0 {
        (sot_start + psot as usize).min(codestream.len())
    } else {
        codestream.len()
    };

    loop {
        let inner = read_u16(cursor)?;
        if inner == Marker::Sod as u16 {
            let sod_pos = cursor.position() as usize;
            let mut end = tile_part_end.min(codestream.len());
            // Strip a trailing EOC (0xFFD9) if present at the very end.
            if end >= sod_pos + 2 && codestream[end - 2] == 0xFF && codestream[end - 1] == 0xD9 {
                end -= 2;
            }
            let start = sod_pos.min(end);
            return Ok(codestream[start..end].to_vec());
        }
        let seg_len = read_u16(cursor)?;
        if seg_len < 2 {
            return Err(GribError::DecodingError(format!(
                "DRT 5.40: invalid JPEG2000 tile-part marker segment length {seg_len}"
            )));
        }
        seek_forward(cursor, i64::from(seg_len) - 2)?;
    }
}

fn read_u16(cursor: &mut Cursor<&[u8]>) -> Result<u16> {
    cursor
        .read_u16::<BigEndian>()
        .map_err(|_| truncated(cursor))
}

fn read_u32(cursor: &mut Cursor<&[u8]>) -> Result<u32> {
    cursor
        .read_u32::<BigEndian>()
        .map_err(|_| truncated(cursor))
}

fn read_u8(cursor: &mut Cursor<&[u8]>) -> Result<u8> {
    cursor.read_u8().map_err(|_| truncated(cursor))
}

fn seek_forward(cursor: &mut Cursor<&[u8]>, delta: i64) -> Result<()> {
    cursor
        .seek(SeekFrom::Current(delta))
        .map(|_| ())
        .map_err(|_| truncated(cursor))
}

fn truncated(cursor: &Cursor<&[u8]>) -> GribError {
    GribError::TruncatedMessage {
        expected: cursor.position() as usize + 1,
        actual: cursor.get_ref().len(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn test_apply_scale_identity() {
        // R=0, E=0, D=0 -> value == X.
        assert!((apply_scale(42, 0.0, 0, 0) - 42.0).abs() < 1e-6);
    }

    #[test]
    fn test_apply_scale_reference_and_binary() {
        // (5 + 3 * 2^1) / 10^0 = 5 + 6 = 11.
        assert!((apply_scale(3, 5.0, 1, 0) - 11.0).abs() < 1e-6);
    }

    #[test]
    fn test_apply_scale_decimal_divisor() {
        // (0 + 850 * 2^0) / 10^1 = 85.0.
        assert!((apply_scale(850, 0.0, 0, 1) - 85.0).abs() < 1e-6);
    }

    #[test]
    fn test_non_soc_codestream_errors() {
        // A payload that does not start with SOC must error, not panic.
        let data = [0x00u8, 0x00, 0x00, 0x00];
        let err = decode_single_component(&data).unwrap_err();
        assert!(matches!(err, GribError::DecodingError(_)));
    }

    #[test]
    fn test_truncated_codestream_errors() {
        // SOC only, then EOF -> typed error.
        let data = [0xFFu8, 0x4F];
        let err = decode_single_component(&data).unwrap_err();
        assert!(matches!(
            err,
            GribError::TruncatedMessage { .. } | GribError::DecodingError(_)
        ));
    }
}
