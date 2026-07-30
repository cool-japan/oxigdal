//! Band-aware block reads for the mobile tile paths.
//!
//! # Why this exists
//!
//! Both mobile tile paths — `oxigeo_dataset_read_tile` and
//! `oxigeo_mobile_prefetch_tiles` — used to call `GeoTiffReader::read_tile`,
//! which hands back one *raw block* addressed by the flat
//! `tile_y * tiles_across + tile_x` index, and then labelled those bytes with
//! the dataset's band count.
//!
//! That labelling is only true for `PlanarConfiguration = 1` (chunky), where a
//! block really does hold `SamplesPerPixel` interleaved samples per pixel. In a
//! planar file (`= 2`) each block holds **one band's plane** and the blocks are
//! stored `SamplesPerPixel × TilesPerImage` in plane-major order. So the tile
//! that reached the cache — and the screen — was:
//!
//! * `1/SamplesPerPixel` of the bytes the consumer was told to expect, and
//! * band 0's plane presented as if it were interleaved RGB, so the RGBA
//!   converters in `ios::raster` / `android::raster` read pixel *n*'s red,
//!   green and blue out of pixels *3n*, *3n+1* and *3n+2* of the red plane.
//!
//! Nothing errored: the result was a plausible, wrong picture, and it was then
//! cached under a key that made it authoritative for that tile
//! (cool-japan/oxigeo#14).
//!
//! # What it does instead
//!
//! [`read_block_interleaved`] reads each band separately through
//! [`GeoTiffReader::read_tile_band_buffer`] — the driver's band-aware block
//! read, which de-interleaves a chunky band, plane-selects a planar one and
//! takes its block geometry from the requested level's own IFD — and
//! interleaves the planes into the chunky, band-per-pixel layout the FFI
//! consumers document. The returned geometry is the block's real size, not an
//! assumed one.

use oxigeo_core::error::{OxiGeoError, Result};
use oxigeo_core::io::DataSource;
use oxigeo_geotiff::GeoTiffReader;

/// One decoded block, in chunky (band-interleaved-by-pixel) order.
#[derive(Debug, Clone)]
pub(crate) struct BlockTile {
    /// `width × height × channels × bytes_per_sample` bytes, band-interleaved.
    pub(crate) data: Vec<u8>,
    /// Block width in pixels, from the level's own IFD.
    pub(crate) width: i32,
    /// Block height in pixels, from the level's own IFD.
    pub(crate) height: i32,
    /// Number of bands interleaved into `data`.
    pub(crate) channels: i32,
}

/// Reads one block of `level` with every band interleaved.
///
/// Works identically for `PlanarConfiguration` 1 and 2, and honours `level`:
/// the block geometry comes from that level's IFD rather than from the
/// full-resolution image.
///
/// # Errors
/// Propagates the driver's errors: an out-of-range block or level, an
/// unsupported sample type, or a block that cannot be read or decoded. Also
/// errors if the raster declares no bands or if the per-band planes disagree on
/// geometry, rather than silently truncating.
pub(crate) fn read_block_interleaved<S: DataSource>(
    reader: &GeoTiffReader<S>,
    level: usize,
    tile_x: u32,
    tile_y: u32,
) -> Result<BlockTile> {
    let band_count = reader.band_count() as usize;
    if band_count == 0 {
        return Err(OxiGeoError::Format(
            oxigeo_core::error::FormatError::InvalidHeader {
                message: "raster declares zero bands".to_string(),
            },
        ));
    }

    let first = reader.read_tile_band_buffer(level, 0, tile_x, tile_y)?;
    let block_width = first.width();
    let block_height = first.height();
    let bytes_per_sample = first.data_type().size_bytes();
    if bytes_per_sample == 0 {
        return Err(OxiGeoError::Format(
            oxigeo_core::error::FormatError::InvalidHeader {
                message: format!(
                    "unsupported sample type {:?} (zero bytes per sample)",
                    first.data_type()
                ),
            },
        ));
    }

    let pixels = usize::try_from(block_width.saturating_mul(block_height)).map_err(|_| {
        OxiGeoError::OutOfBounds {
            message: format!("block of {block_width}x{block_height} pixels does not fit in memory"),
        }
    })?;
    let total = pixels
        .checked_mul(band_count)
        .and_then(|n| n.checked_mul(bytes_per_sample))
        .ok_or_else(|| OxiGeoError::OutOfBounds {
            message: format!(
                "block of {block_width}x{block_height}x{band_count} samples does not fit in memory"
            ),
        })?;

    let mut data = vec![0u8; total];
    let stride = band_count * bytes_per_sample;
    for band in 0..band_count {
        let plane = if band == 0 {
            first.as_bytes().to_vec()
        } else {
            let buffer = reader.read_tile_band_buffer(level, band, tile_x, tile_y)?;
            if buffer.width() != block_width || buffer.height() != block_height {
                return Err(OxiGeoError::Internal {
                    message: format!(
                        "band {band} block is {}x{}, band 0's is {block_width}x{block_height}",
                        buffer.width(),
                        buffer.height()
                    ),
                });
            }
            buffer.into_bytes()
        };

        // Scatter this plane into the band's slot of every pixel. The slice
        // starts at the band's offset, so each `stride`-sized chunk is one
        // pixel's sample group and its first `bytes_per_sample` bytes are this
        // band's. The final chunk is short by `band * bytes_per_sample`, which
        // still leaves room for the sample itself.
        let offset = band * bytes_per_sample;
        for (dst, src) in data[offset..]
            .chunks_mut(stride)
            .zip(plane.chunks_exact(bytes_per_sample))
        {
            if let Some(slot) = dst.get_mut(..bytes_per_sample) {
                slot.copy_from_slice(src);
            }
        }
    }

    let to_i32 = |v: u64| {
        i32::try_from(v).map_err(|_| OxiGeoError::OutOfBounds {
            message: format!("block dimension {v} does not fit in the FFI's i32"),
        })
    };
    let channels = i32::try_from(band_count).map_err(|_| OxiGeoError::OutOfBounds {
        message: format!("band count {band_count} does not fit in the FFI's i32"),
    })?;

    Ok(BlockTile {
        data,
        width: to_i32(block_width)?,
        height: to_i32(block_height)?,
        channels,
    })
}
