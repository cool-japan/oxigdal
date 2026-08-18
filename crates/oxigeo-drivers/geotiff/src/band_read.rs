//! Single-band read engine: whole-band and windowed reads that decode straight
//! into a caller-owned buffer.
//!
//! This module exists because of cool-japan/oxigeo#14. A user migrating from the
//! C-GDAL wrapper wants GDAL's
//! `RasterBand::read_into_slice(window, size, buf_size, &mut [T], None)`: *read*
//! and *convert* in one pass into memory the caller already owns. The driver had
//! no equivalent, so callers had to
//!
//! ```text
//! reader.read_band(0, 0)          // full-size Vec<u8>, zero-filled first
//!   -> bytemuck::cast_slice(..)   // reinterpret
//!   -> .mapv(|v| v as f64)        // second full-size allocation + pass
//! ```
//!
//! which costs two extra full-size buffers and two extra passes over a
//! several-hundred-megabyte band.
//!
//! The engine here decodes into a single reused scratch buffer and scatters the
//! requested band's samples straight into the destination, optionally converting
//! the element type on the way ([`convert_raw_into`]). A whole-band read
//! therefore performs exactly one allocation (the scratch) on top of whatever
//! the caller supplied, and the typed variant needs no full-size intermediate at
//! all.
//!
//! # Destination write order
//!
//! The destination is the only buffer in the read that is too big to cache, so
//! the *serial* scatter is organised around writing it front to back: a tiled
//! raster's blocks are staged a whole block row at a time and then copied out row
//! by row, left to right, rather than one block at a time. See
//! `scatter_block_row` and `BLOCK_GROUP_MAX_BYTES`. The scratch buffer is
//! sized for the staged block row, which is bounded and still just the one
//! allocation; block rows too large for that bound — and every read spread over
//! rayon workers, whose scratch is per-worker — keep the original one-block
//! scratch and copy order.
//!
//! # Layout handling
//!
//! * `PlanarConfiguration::Chunky` (1) — samples are interleaved per pixel
//!   (`RGBRGBRGB…`). The requested band is de-interleaved *while* scattering into
//!   the destination; the interleaved plane is never materialised.
//! * `PlanarConfiguration::Planar` (2) — each band is a contiguous plane, stored
//!   as `SamplesPerPixel × TilesPerImage` blocks in plane-major order. The
//!   requested plane's blocks are selected directly, so a planar read touches
//!   only 1/`SamplesPerPixel` of the file's blocks.
//!
//! # Byte order
//!
//! Every entry point in this module returns samples in the **host's** byte order,
//! whatever the file's `II`/`MM` header declares; see the crate-level *Byte order
//! of decoded samples* section for the workspace-wide contract. The conversion
//! happens once, per decoded block, inside `decode_block` — via
//! [`CogReader::read_tile_into`] for natively-laid-out blocks and via the
//! crate-internal `normalize_samples_to_native` directly for the plane-aware
//! branch — and always *after* the predictor has been reversed, because both TIFF
//! predictors are defined on file-order data. The scatter below therefore moves
//! bytes that are already native, and never has to know the file's byte order.
//!
//! # Parallelism
//!
//! With the non-default `parallel` feature the per-block decode of every
//! `read_*_into` entry point — [`GeoTiffReader::read_band_into`],
//! [`GeoTiffReader::read_window_into`] and their typed counterparts
//! [`GeoTiffReader::read_band_into_typed`] /
//! [`GeoTiffReader::read_window_into_typed`] — is spread over rayon workers. The
//! destination is split into disjoint block-row slices up front, so workers never
//! alias and no locks or `unsafe` are involved, and the result is bit-identical to
//! the serial path. On the typed paths each worker converts its own block rows
//! straight into its slice of the destination, so parallelism does not cost the
//! fused conversion or add an intermediate buffer. The feature is off by default
//! because this crate is compiled to `wasm32-unknown-unknown`, which has no OS
//! threads.
//!
//! # Multi-band reads
//!
//! Everything here reads exactly one band, which makes an `n`-band read of a
//! chunky file decode every block `n` times — a chunky block physically holds
//! all of a pixel's bands, so `n − 1` of them are thrown away each pass. The
//! `multi` submodule adds the interleaved entry points
//! [`GeoTiffReader::read_bands_into_typed`] and
//! [`GeoTiffReader::read_window_bands_into_typed`], which decode each block once
//! and fan it out into every requested slot. They reuse this module's plan,
//! geometry, block decode and (for planar files) scatter verbatim.

mod multi;

use oxigeo_core::buffer::{RasterElement, convert_raw_into};
use oxigeo_core::error::{FormatError, OxiGeoError, Result};
use oxigeo_core::io::DataSource;
use oxigeo_core::types::RasterDataType;

use crate::cog::CogReader;
use crate::compression;
use crate::tiff::{ByteOrderType, Compression, ImageInfo, PlanarConfiguration, Predictor, TiffTag};
use crate::{GeoTiffReader, checked_band_bytes};

/// Smallest output (in bytes) worth spreading over rayon workers.
///
/// Below this the thread hand-off costs more than the decode saves; measured on
/// an 8-core Apple M-series, uncompressed and DEFLATE alike.
#[cfg(feature = "parallel")]
const PARALLEL_MIN_BYTES: usize = 1 << 20;

/// Largest block row, in bytes, the scatter is willing to stage (see
/// [`group_blocks_for`]).
///
/// The scatter decodes a whole block row into one buffer before copying any of
/// it out, so that the destination is written front to back instead of one tile
/// at a time. That buffer replaces the old one-block scratch — it is still the
/// read's only internal allocation, merely wider — and this constant caps how
/// wide it may get.
///
/// 8 MiB covers a block row of a 4000-px `Float64` raster, an 8000-px `Float32`
/// one, or a 2600-px three-band `Float32` one, all with the usual 256-px tiles.
/// Above that the read falls back to the block-at-a-time scatter, so a
/// 100 000-px-wide raster keeps its 256 KiB scratch instead of being handed a
/// 100 MiB one. Under the `parallel` feature the buffer is per-worker and rayon
/// runs at most one job per thread at a time, so the peak is
/// `rayon::current_num_threads() × 8 MiB`.
const BLOCK_GROUP_MAX_BYTES: usize = 8 << 20;

/// How many horizontally adjacent blocks to stage at once: all of them, or one.
///
/// Grouping is deliberately all-or-nothing. Its entire value is that a group
/// spanning the whole window makes *consecutive destination rows contiguous*, so
/// a block row is written as one uninterrupted stream. A partial group only
/// lengthens the runs while leaving the stride in place, which measures no better
/// than — and sometimes worse than — the block-at-a-time scatter it replaced. So
/// either the whole block row fits in [`BLOCK_GROUP_MAX_BYTES`] and is staged, or
/// the read keeps exactly the one-block scratch and the exact copy order it had
/// before grouping existed.
///
/// `1` is likewise the answer whenever the read touches a single block column —
/// every striped raster — and whenever one block on its own exceeds the budget.
const fn group_blocks_for(block_bytes: usize, blocks_in_row: u32) -> u32 {
    if blocks_in_row <= 1 || block_bytes == 0 {
        return 1;
    }
    match block_bytes.checked_mul(blocks_in_row as usize) {
        Some(row_bytes) if row_bytes <= BLOCK_GROUP_MAX_BYTES => blocks_in_row,
        _ => 1,
    }
}

// ---------------------------------------------------------------------------
// Level geometry
// ---------------------------------------------------------------------------

/// Everything the read engine needs to know about one resolution level.
///
/// Deliberately mirrors `CogReader`'s own internal tile geometry so that the
/// buffer sizes computed here are exactly the ones
/// [`CogReader::read_tile_into`] demands; a disagreement surfaces as that
/// method's explicit length error rather than as corrupt pixels.
#[derive(Debug, Clone, Copy)]
struct LevelGeometry {
    /// Raster width in pixels at this level.
    width: usize,
    /// Raster height in pixels at this level.
    height: usize,
    /// Bytes per sample (0 for sub-byte `BitsPerSample`, matching the reader's
    /// long-standing behaviour).
    bytes_per_sample: usize,
    /// Samples per pixel (band count).
    samples_per_pixel: usize,
    /// Chunky (interleaved) or planar band storage.
    planar: PlanarConfiguration,
    /// Whether the level uses tiles (as opposed to strips).
    is_tiled: bool,
    /// Decoded block width in pixels (tile width, or the image width for strips).
    block_width: usize,
    /// Decoded block height in pixels (tile height, or `RowsPerStrip`).
    block_height: usize,
    /// Number of blocks across one row of the image.
    blocks_across: u32,
    /// Number of block rows in the image (per plane, for planar files).
    blocks_down: u32,
    /// Codec the blocks are stored with.
    compression: Compression,
    /// Predictor applied before compression.
    predictor: Predictor,
    /// Sample type, if it maps onto a [`RasterDataType`].
    data_type: Option<RasterDataType>,
}

impl LevelGeometry {
    /// Derives the geometry from a parsed [`ImageInfo`].
    fn from_info(info: &ImageInfo) -> Result<Self> {
        let width = usize::try_from(info.width).map_err(|_| dimension_overflow(info))?;
        let height = usize::try_from(info.height).map_err(|_| dimension_overflow(info))?;
        let is_tiled = info.is_tiled();

        // Same expressions the pre-fix `read_band` used, so single-band reads are
        // unchanged.
        let (block_width, block_height) = if is_tiled {
            (
                info.tile_width.unwrap_or(info.width as u32) as usize,
                info.tile_height.unwrap_or(info.height as u32) as usize,
            )
        } else {
            (
                width,
                info.rows_per_strip.unwrap_or(info.height as u32) as usize,
            )
        };

        // Mirror `ImageInfo::tiles_across`/`tiles_down` exactly — the flat block
        // index used by `CogReader::tile_byte_range` is derived from them — but
        // reject the zero divisors those methods would panic on.
        let blocks_across = match info.tile_width {
            Some(0) => return Err(zero_block_dimension("TileWidth")),
            Some(tw) => (info.width as u32).div_ceil(tw),
            None => 1,
        };
        let blocks_down = match (info.tile_height, info.rows_per_strip) {
            (Some(0), _) => return Err(zero_block_dimension("TileLength")),
            (Some(th), _) => (info.height as u32).div_ceil(th),
            (None, Some(0)) => return Err(zero_block_dimension("RowsPerStrip")),
            (None, Some(rps)) => (info.height as u32).div_ceil(rps),
            (None, None) => 1,
        };

        Ok(Self {
            width,
            height,
            bytes_per_sample: info
                .bits_per_sample
                .first()
                .map_or(1, |&b| (b / 8) as usize),
            samples_per_pixel: info.samples_per_pixel as usize,
            planar: info.planar_config,
            is_tiled,
            block_width,
            block_height,
            blocks_across,
            blocks_down,
            compression: info.compression,
            predictor: info.predictor,
            data_type: info.data_type(),
        })
    }

    /// Resolves the geometry of `level` (0 = full resolution).
    ///
    /// Overview levels are described by their own IFD. `CogReader` parses those
    /// into `ImageInfo`s but does not expose them, so the level-specific tags
    /// (`ImageWidth`, `ImageLength`, `TileWidth`, `TileLength`, `RowsPerStrip`)
    /// are read straight from the IFD here — they are always single-valued and
    /// therefore stored inline, so no data source is needed. Sample-layout tags
    /// are taken from the overview's IFD when they are inline there and inherited
    /// from the full-resolution image otherwise (overviews always share the
    /// full-resolution sample layout).
    fn resolve<S: DataSource>(reader: &CogReader<S>, level: usize) -> Result<Self> {
        let primary = reader.primary_info();
        if level == 0 {
            return Self::from_info(primary);
        }
        if level > reader.overview_count() {
            return Err(OxiGeoError::OutOfBounds {
                message: format!("Overview level {} out of bounds", level),
            });
        }

        let byte_order = reader.tiff().byte_order();
        // Resolved through the reader's level → IFD map, never by indexing the
        // chain: levels skip GDAL internal masks (and any IFD whose `ImageInfo`
        // will not parse), so on a masked file `ifds[level]` names a different
        // image than `level` does — the geometry taken here would describe one
        // resolution while the tile offsets came from another.
        let ifd = reader
            .level_ifd(level)
            .ok_or_else(|| OxiGeoError::OutOfBounds {
                message: format!("Overview level {} out of bounds", level),
            })?;
        let scalar = |tag: TiffTag| ifd.get_entry(tag).and_then(|e| e.get_u64(byte_order).ok());

        let mut info = primary.clone();
        info.width =
            scalar(TiffTag::ImageWidth).ok_or(OxiGeoError::Format(FormatError::MissingTag {
                tag: "ImageWidth",
            }))?;
        info.height =
            scalar(TiffTag::ImageLength).ok_or(OxiGeoError::Format(FormatError::MissingTag {
                tag: "ImageLength",
            }))?;
        // Layout tags are per-level: an overview may be striped even when the
        // full-resolution image is tiled, so these are never inherited.
        info.tile_width = scalar(TiffTag::TileWidth).and_then(|v| u32::try_from(v).ok());
        info.tile_height = scalar(TiffTag::TileLength).and_then(|v| u32::try_from(v).ok());
        info.rows_per_strip = scalar(TiffTag::RowsPerStrip).and_then(|v| u32::try_from(v).ok());

        if let Some(v) = scalar(TiffTag::SamplesPerPixel) {
            info.samples_per_pixel = v as u16;
        }
        if let Some(v) = scalar(TiffTag::BitsPerSample) {
            info.bits_per_sample = vec![v as u16];
        }
        if let Some(planar) = scalar(TiffTag::PlanarConfiguration)
            .and_then(|v| PlanarConfiguration::from_u16(v as u16))
        {
            info.planar_config = planar;
        }
        if let Some(compression) =
            scalar(TiffTag::Compression).and_then(|v| Compression::from_u16(v as u16))
        {
            info.compression = compression;
        }
        if let Some(predictor) =
            scalar(TiffTag::Predictor).and_then(|v| Predictor::from_u16(v as u16))
        {
            info.predictor = predictor;
        }

        Self::from_info(&info)
    }

    /// Samples stored per pixel *inside one decoded block*.
    ///
    /// Chunky blocks hold every band; a planar block holds exactly one.
    const fn samples_in_block(&self) -> usize {
        match self.planar {
            PlanarConfiguration::Chunky => self.samples_per_pixel,
            PlanarConfiguration::Planar => 1,
        }
    }

    /// Distance in bytes between two horizontally adjacent samples of the same
    /// band inside a decoded block.
    const fn src_pixel_stride(&self) -> usize {
        self.bytes_per_sample * self.samples_in_block()
    }

    /// Byte offset of `band`'s sample within one source pixel.
    const fn band_offset(&self, band: usize) -> usize {
        match self.planar {
            PlanarConfiguration::Chunky => band * self.bytes_per_sample,
            // The plane *is* the band, so there is nothing to skip.
            PlanarConfiguration::Planar => 0,
        }
    }

    /// Whether `CogReader`'s own tile decode describes this level's blocks.
    ///
    /// True for every chunky file and for planar files with a single band (where
    /// the two layouts coincide); a multi-band planar file needs the plane-aware
    /// decode in [`decode_block`].
    const fn uses_native_block_layout(&self) -> bool {
        matches!(self.planar, PlanarConfiguration::Chunky) || self.samples_per_pixel == 1
    }

    /// Pixel rows contained in the decoded block at block row `ty`.
    ///
    /// Tiles are always full height (the bottom row is padded); the final strip
    /// of a striped image is short.
    fn block_rows(&self, ty: u32) -> usize {
        if self.is_tiled {
            self.block_height
        } else {
            let start = (ty as usize).saturating_mul(self.block_height);
            self.height.saturating_sub(start).min(self.block_height)
        }
    }

    /// Decoded size in bytes of the block at block row `ty`.
    fn block_decoded_bytes(&self, ty: u32) -> Result<usize> {
        self.block_width
            .checked_mul(self.block_rows(ty))
            .and_then(|v| v.checked_mul(self.bytes_per_sample))
            .and_then(|v| v.checked_mul(self.samples_in_block()))
            .ok_or_else(|| {
                OxiGeoError::Format(FormatError::InvalidHeader {
                    message: format!(
                        "tile dimensions overflow usize: {}x{} x {} bytes x {} samples",
                        self.block_width,
                        self.block_rows(ty),
                        self.bytes_per_sample,
                        self.samples_in_block()
                    ),
                })
            })
    }
}

/// Error for a raster whose declared dimensions do not fit in `usize`.
fn dimension_overflow(info: &ImageInfo) -> OxiGeoError {
    OxiGeoError::Format(FormatError::InvalidHeader {
        message: format!(
            "raster dimensions {}x{} do not fit in usize on this target",
            info.width, info.height
        ),
    })
}

/// Error for a zero-valued tile/strip dimension (a malformed header that would
/// otherwise divide by zero).
fn zero_block_dimension(tag: &str) -> OxiGeoError {
    OxiGeoError::Format(FormatError::InvalidHeader {
        message: format!("{tag} is zero; the file declares zero-sized tiles/strips"),
    })
}

// ---------------------------------------------------------------------------
// Read plan
// ---------------------------------------------------------------------------

/// A validated description of one band read: which level, which band, which
/// pixel window, and which blocks that window touches.
#[derive(Debug, Clone, Copy)]
struct ReadPlan {
    level: usize,
    band: usize,
    byte_order: ByteOrderType,
    geom: LevelGeometry,
    /// Window origin (column) in level pixel coordinates.
    x: usize,
    /// Window origin (row) in level pixel coordinates.
    y: usize,
    /// Window width in pixels.
    win_w: usize,
    /// Window height in pixels.
    win_h: usize,
    /// First block column touched by the window (inclusive).
    bx0: u32,
    /// Last block column touched by the window (exclusive).
    bx1: u32,
    /// First block row touched by the window (inclusive).
    by0: u32,
    /// Last block row touched by the window (exclusive).
    by1: u32,
    /// Decoded size in bytes of the largest single block this read touches.
    block_bytes: usize,
    /// Bytes needed by the reusable staging buffer (`group_blocks` blocks).
    scratch_bytes: usize,
    /// Bytes needed by the de-interleave gather buffer (0 when not needed).
    gather_bytes: usize,
    /// How many horizontally adjacent blocks the scatter stages at once: either
    /// the whole block row, or `1`.
    ///
    /// `1` reproduces the original block-at-a-time scatter exactly and is what a
    /// striped raster, a single-block-column window, and any block row too big
    /// for the budget all get. See [`group_blocks_for`].
    group_blocks: u32,
}

impl ReadPlan {
    /// Plans a full-band read of `level`.
    fn full_band<S: DataSource>(reader: &CogReader<S>, level: usize, band: usize) -> Result<Self> {
        let geom = LevelGeometry::resolve(reader, level)?;
        let (width, height) = (geom.width, geom.height);
        Self::build(reader, geom, level, band, 0, 0, width, height, false)
    }

    /// Plans a windowed read of `level`.
    fn window<S: DataSource>(
        reader: &CogReader<S>,
        level: usize,
        band: usize,
        x: u64,
        y: u64,
        width: u64,
        height: u64,
    ) -> Result<Self> {
        let geom = LevelGeometry::resolve(reader, level)?;
        let to_usize = |name: &'static str, v: u64| {
            usize::try_from(v).map_err(|_| {
                OxiGeoError::invalid_parameter_builder(
                    "window",
                    "window coordinate does not fit in usize on this target",
                )
                .with_operation("read_window")
                .with_parameter(name, v.to_string())
                .build()
            })
        };
        let x = to_usize("x", x)?;
        let y = to_usize("y", y)?;
        let width = to_usize("width", width)?;
        let height = to_usize("height", height)?;
        Self::build(reader, geom, level, band, x, y, width, height, true)
    }

    /// Shared validation and block-range computation.
    ///
    /// `reject_empty` distinguishes a user-supplied window (where a zero-sized
    /// request is a mistake and must be reported) from a full-band read of a
    /// zero-area raster (which legitimately yields an empty band).
    #[allow(clippy::too_many_arguments)]
    fn build<S: DataSource>(
        reader: &CogReader<S>,
        geom: LevelGeometry,
        level: usize,
        band: usize,
        x: usize,
        y: usize,
        win_w: usize,
        win_h: usize,
        reject_empty: bool,
    ) -> Result<Self> {
        if band >= geom.samples_per_pixel {
            return Err(OxiGeoError::invalid_parameter_builder(
                "band",
                "band index is out of range for this raster",
            )
            .with_operation("read_band")
            .with_parameter("band", band.to_string())
            .with_parameter("band_count", geom.samples_per_pixel.to_string())
            .with_suggestion("Band indices are zero-based; use GeoTiffReader::band_count()")
            .build());
        }

        if reject_empty && (win_w == 0 || win_h == 0) {
            return Err(OxiGeoError::invalid_parameter_builder(
                "window",
                "window size must be non-zero",
            )
            .with_operation("read_window")
            .with_parameter("width", win_w.to_string())
            .with_parameter("height", win_h.to_string())
            .build());
        }

        let past_x = x.checked_add(win_w).is_none_or(|end| end > geom.width);
        let past_y = y.checked_add(win_h).is_none_or(|end| end > geom.height);
        if past_x || past_y {
            return Err(OxiGeoError::invalid_parameter_builder(
                "window",
                "window extends past the raster extent",
            )
            .with_operation("read_window")
            .with_parameter("window", format!("[{x},{y} {win_w}x{win_h}]"))
            .with_parameter("extent", format!("{}x{}", geom.width, geom.height))
            .build());
        }

        let mut plan = Self {
            level,
            band,
            byte_order: reader.tiff().byte_order(),
            geom,
            x,
            y,
            win_w,
            win_h,
            bx0: 0,
            bx1: 0,
            by0: 0,
            by1: 0,
            block_bytes: 0,
            scratch_bytes: 0,
            gather_bytes: 0,
            group_blocks: 1,
        };

        if win_w == 0 || win_h == 0 {
            // Nothing to decode; leave the block range empty.
            return Ok(plan);
        }
        if geom.block_width == 0 || geom.block_height == 0 {
            return Err(zero_block_dimension("TileWidth/TileLength/RowsPerStrip"));
        }

        plan.bx0 = (x / geom.block_width) as u32;
        plan.bx1 = ((x + win_w).div_ceil(geom.block_width) as u32).min(geom.blocks_across);
        plan.by0 = (y / geom.block_height) as u32;
        plan.by1 = ((y + win_h).div_ceil(geom.block_height) as u32).min(geom.blocks_down);

        let mut block_bytes = 0usize;
        for ty in plan.by0..plan.by1 {
            block_bytes = block_bytes.max(geom.block_decoded_bytes(ty)?);
        }

        // How many horizontally adjacent blocks to stage before copying any of
        // them into the destination. Grouping only exists to lengthen the
        // destination write runs, so it is pointless when the window touches a
        // single block column — which is every striped raster — and there the
        // plan degenerates to the historical one-block scratch.
        plan.block_bytes = block_bytes;
        plan.group_blocks = group_blocks_for(block_bytes, plan.bx1.saturating_sub(plan.bx0));
        plan.scratch_bytes = block_bytes
            .checked_mul(plan.group_blocks as usize)
            .ok_or_else(|| {
                OxiGeoError::Format(FormatError::InvalidHeader {
                    message: "block group size overflows usize".to_string(),
                })
            })?;
        plan.gather_bytes = if geom.src_pixel_stride() == geom.bytes_per_sample {
            0
        } else {
            geom.block_width
                .checked_mul(geom.bytes_per_sample)
                .ok_or_else(|| {
                    OxiGeoError::Format(FormatError::InvalidHeader {
                        message: "tile row size overflows usize".to_string(),
                    })
                })?
        };

        Ok(plan)
    }

    /// Number of output pixels.
    fn output_pixels(&self) -> Result<usize> {
        self.win_w.checked_mul(self.win_h).ok_or_else(|| {
            OxiGeoError::Format(FormatError::InvalidHeader {
                message: format!("window {}x{} overflows usize", self.win_w, self.win_h),
            })
        })
    }

    /// Number of output bytes for the raw (unconverted) read.
    fn output_bytes(&self) -> Result<usize> {
        self.output_pixels()?
            .checked_mul(self.geom.bytes_per_sample)
            .ok_or_else(|| {
                OxiGeoError::Format(FormatError::InvalidHeader {
                    message: "window byte size overflows usize".to_string(),
                })
            })
    }

    /// Pixel rows of the *output* covered by block row `ty`.
    fn rows_in_block_row(&self, ty: u32) -> usize {
        let block_y0 = (ty as usize).saturating_mul(self.geom.block_height);
        let start = self.y.max(block_y0);
        let end = (self.y + self.win_h).min(block_y0 + self.geom.block_rows(ty));
        end.saturating_sub(start)
    }

    /// Rejects a destination whose length is not exactly what this plan produces.
    fn check_len(&self, actual: usize, expected: usize, operation: &'static str) -> Result<()> {
        if actual == expected {
            return Ok(());
        }
        Err(OxiGeoError::invalid_parameter_builder(
            "dst",
            "destination length must exactly match the requested region",
        )
        .with_operation(operation)
        .with_parameter("dst_len", actual.to_string())
        .with_parameter("required_len", expected.to_string())
        .with_parameter("region", format!("{}x{}", self.win_w, self.win_h))
        .with_suggestion("Size the buffer with GeoTiffReader::band_byte_len / band_pixel_count")
        .build())
    }

    /// The source [`RasterDataType`] for a typed read, or a typed error.
    fn typed_source_type(&self, operation: &'static str) -> Result<RasterDataType> {
        let data_type = self.geom.data_type.ok_or_else(|| {
            OxiGeoError::not_supported_builder("typed read of an unrecognised sample type")
                .with_operation(operation)
                .with_suggestion("Use read_band_into for raw bytes")
                .build()
        })?;
        if data_type.size_bytes() != self.geom.bytes_per_sample {
            return Err(OxiGeoError::not_supported_builder(
                "typed read where the sample type width disagrees with BitsPerSample",
            )
            .with_operation(operation)
            .with_parameter("data_type", format!("{data_type:?}"))
            .with_parameter("bytes_per_sample", self.geom.bytes_per_sample.to_string())
            .with_suggestion("Use read_band_into for raw bytes")
            .build());
        }
        Ok(data_type)
    }

    /// The same plan with the block-row staging turned off, so the scatter falls
    /// back to the one-block scratch and the block-at-a-time copy order.
    ///
    /// Used by [`scatter_parallel`]; see its documentation for why.
    #[cfg(feature = "parallel")]
    fn without_block_row_staging(&self) -> Self {
        Self {
            scratch_bytes: self.block_bytes,
            group_blocks: 1,
            ..*self
        }
    }

    /// Whether spreading this read over rayon workers is worth the hand-off.
    #[cfg(feature = "parallel")]
    fn should_parallelise(&self) -> bool {
        self.by1.saturating_sub(self.by0) >= 2
            && self.output_bytes().unwrap_or(0) >= PARALLEL_MIN_BYTES
    }
}

// ---------------------------------------------------------------------------
// Scratch buffers
// ---------------------------------------------------------------------------

/// The reusable buffers one worker needs for a whole band.
///
/// Allocated once per read (serial) or once per rayon worker (parallel) — never
/// per tile, which is exactly the allocation the pre-fix `read_band` paid for
/// every block.
struct Scratch {
    /// Staging area for one group of horizontally adjacent decoded blocks
    /// (`ReadPlan::group_blocks` of them, one block for a striped raster).
    block: Vec<u8>,
    /// One de-interleaved source row, only used for chunky multi-band files.
    gather: Vec<u8>,
}

impl Scratch {
    fn new(plan: &ReadPlan) -> Self {
        Self {
            block: vec![0u8; plan.scratch_bytes],
            gather: vec![0u8; plan.gather_bytes],
        }
    }
}

// ---------------------------------------------------------------------------
// Block decode
// ---------------------------------------------------------------------------

/// Decodes one block of the plan's band into `dst`.
///
/// `dst.len()` must equal `plan.geom.block_decoded_bytes(ty)`.
fn decode_block<S: DataSource>(
    reader: &CogReader<S>,
    plan: &ReadPlan,
    tx: u32,
    ty: u32,
    dst: &mut [u8],
) -> Result<()> {
    if plan.geom.uses_native_block_layout() {
        return reader.read_tile_into(plan.level, tx, ty, dst);
    }

    // Planar, multi-band: TIFF stores `SamplesPerPixel × TilesPerImage` blocks in
    // plane-major order, so the requested plane's block row is offset by
    // `band × blocks_down`. `tile_byte_range` turns `(tx, ty)` into the flat block
    // index `ty × blocks_across + tx`, so offsetting the block row addresses the
    // right plane without needing a plane-aware API on `CogReader`.
    let block_row = u64::try_from(plan.band)
        .ok()
        .and_then(|band| band.checked_mul(u64::from(plan.geom.blocks_down)))
        .and_then(|base| base.checked_add(u64::from(ty)))
        .and_then(|row| u32::try_from(row).ok())
        .ok_or_else(|| OxiGeoError::OutOfBounds {
            message: format!(
                "planar block row for band {} of {} overflows",
                plan.band, plan.geom.samples_per_pixel
            ),
        })?;

    // Same refusal `CogReader::read_tile_into` makes on the chunky path above:
    // a codec whose predictor reversal is undefined cannot yield right pixels.
    crate::reject_undefined_predictor(plan.geom.compression, plan.geom.predictor)?;

    let compressed = reader.read_tile_raw(plan.level, tx, block_row)?;
    let written = compression::decompress_into_partial(&compressed, plan.geom.compression, dst)?;
    // Each planar block holds one sample per pixel, so the predictor runs with a
    // stride of one sample regardless of the file's SamplesPerPixel.
    if let Some(decoded) = dst.get_mut(..written) {
        compression::apply_predictor_reverse(
            decoded,
            plan.geom.predictor,
            plan.geom.bytes_per_sample,
            1,
            plan.geom.block_width,
            plan.byte_order,
        )?;
        // The other half of `CogReader::read_tile_into`'s contract, which this
        // branch bypasses: samples leave the decode in *host* byte order, and
        // strictly after the predictor, which is defined on file-order data.
        crate::normalize_samples_to_native(
            decoded,
            plan.geom.bytes_per_sample,
            plan.byte_order,
            plan.geom.compression,
        );
    }
    // A short block leaves the tail zeroed, exactly as a freshly allocated band
    // buffer would have been, so a reused scratch can never leak stale pixels.
    if let Some(tail) = dst.get_mut(written..) {
        tail.fill(0);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Scatter
// ---------------------------------------------------------------------------

/// Internal error for an index that the plan's own arithmetic proves impossible.
fn scatter_bug(what: &str) -> OxiGeoError {
    OxiGeoError::Internal {
        message: format!("band read produced an out-of-range {what} offset"),
    }
}

/// Decodes every block of block row `ty` and scatters the band's samples into
/// `out`, which holds exactly this block row's output rows.
///
/// # Destination write order
///
/// When [`ReadPlan::group_blocks`] says so, the whole block row is decoded into
/// adjacent slots of the one staging buffer and only then copied out, one
/// destination row at a time. Consecutive blocks own consecutive pixels of the
/// same output row and consecutive rows are then contiguous as well, so the
/// entire block row leaves as one uninterrupted forward stream instead of
/// `block_height` scattered `block_width`-sample writes per block.
///
/// That matters because the destination is the one buffer in the read that is
/// far too large to cache: a 4000 × 4000 `Float32` band read into `&mut [f64]`
/// is 128 MiB, and writing it in 256-sample runs at a 4000-sample stride made it
/// pay write-allocate traffic that long sequential runs avoid. The staging
/// buffer is small enough to stay resident, so it absorbs the scattered writes
/// instead. Nothing is copied twice: the blocks were always decoded into a
/// scratch buffer first, that buffer is merely wide enough to hold a group now.
///
/// With `group_blocks == 1` — a striped raster, a single-block-column window, or
/// a block row too large for [`BLOCK_GROUP_MAX_BYTES`] — this is exactly the
/// historical block-at-a-time scatter, including the whole-block-row fast path
/// below.
fn scatter_block_row<S, D, F>(
    reader: &CogReader<S>,
    plan: &ReadPlan,
    ty: u32,
    out: &mut [D],
    units_per_pixel: usize,
    scratch: &mut Scratch,
    write_run: &F,
) -> Result<()>
where
    S: DataSource,
    D: Copy,
    F: Fn(&mut [D], &[u8]) -> Result<()>,
{
    let geom = &plan.geom;
    let bps = geom.bytes_per_sample;
    let src_stride = geom.src_pixel_stride();
    let band_offset = geom.band_offset(plan.band);
    let row_units = plan.win_w * units_per_pixel;

    let block_bytes = geom.block_decoded_bytes(ty)?;
    let Scratch { block, gather } = scratch;

    let block_y0 = (ty as usize) * geom.block_height;
    let row_start = plan.y.max(block_y0);
    let row_end = (plan.y + plan.win_h).min(block_y0 + geom.block_rows(ty));
    if row_end <= row_start {
        return Ok(());
    }
    let rows = row_end - row_start;

    // Fast path: one block covers the entire output row, so the source rows and
    // the destination rows are both contiguous and the whole block row is a
    // single run. This is what a striped single-band raster (and any full-width
    // tile) hits, and it turns the per-row loop into a single memcpy — or, for
    // the typed variant, a single `convert_raw_into` over the block. It can only
    // ever fire for a lone block column, because it demands a run as wide as the
    // whole window.
    if src_stride == bps && plan.bx1.saturating_sub(plan.bx0) == 1 {
        let block_x0 = (plan.bx0 as usize) * geom.block_width;
        let col_start = plan.x.max(block_x0);
        let col_end = (plan.x + plan.win_w).min(block_x0 + geom.block_width);
        let run = col_end.saturating_sub(col_start);
        if col_start == block_x0 && run == geom.block_width && run * units_per_pixel == row_units {
            let staged = block
                .get_mut(..block_bytes)
                .ok_or_else(|| scatter_bug("scratch"))?;
            decode_block(reader, plan, plan.bx0, ty, staged)?;
            let src_off = (row_start - block_y0) * geom.block_width * bps;
            let src_len = rows * geom.block_width * bps;
            let src = staged
                .get(src_off..src_off + src_len)
                .ok_or_else(|| scatter_bug("source"))?;
            let dst = out
                .get_mut(..rows * row_units)
                .ok_or_else(|| scatter_bug("destination"))?;
            return write_run(dst, src);
        }
    }

    let group_blocks = plan.group_blocks.max(1);
    let mut group_start = plan.bx0;
    while group_start < plan.bx1 {
        let group_end = plan.bx1.min(group_start.saturating_add(group_blocks));
        let group_len = (group_end - group_start) as usize;
        let staged_bytes = group_len
            .checked_mul(block_bytes)
            .ok_or_else(|| scatter_bug("scratch"))?;
        let staged = block
            .get_mut(..staged_bytes)
            .ok_or_else(|| scatter_bug("scratch"))?;

        // Decode the whole group before touching `out`, so that the copies below
        // can run in destination order.
        for (slot, tx) in (group_start..group_end).enumerate() {
            let block_x0 = (tx as usize) * geom.block_width;
            let col_start = plan.x.max(block_x0);
            let col_end = (plan.x + plan.win_w).min(block_x0 + geom.block_width);
            if col_end <= col_start {
                continue;
            }
            let offset = slot * block_bytes;
            let into = staged
                .get_mut(offset..offset + block_bytes)
                .ok_or_else(|| scatter_bug("scratch"))?;
            decode_block(reader, plan, tx, ty, into)?;
        }

        // Copy out in destination order: outer loop over output rows, inner loop
        // left to right across the group.
        for row in row_start..row_end {
            let src_row = row - block_y0;
            let out_row = (row - row_start) * row_units;
            for (slot, tx) in (group_start..group_end).enumerate() {
                let block_x0 = (tx as usize) * geom.block_width;
                let col_start = plan.x.max(block_x0);
                let col_end = (plan.x + plan.win_w).min(block_x0 + geom.block_width);
                if col_end <= col_start {
                    continue;
                }
                let run = col_end - col_start;
                let src_col = col_start - block_x0;
                let out_col = col_start - plan.x;

                let src_off = slot * block_bytes
                    + (src_row * geom.block_width + src_col) * src_stride
                    + band_offset;
                let out_off = out_row + out_col * units_per_pixel;
                let dst = out
                    .get_mut(out_off..out_off + run * units_per_pixel)
                    .ok_or_else(|| scatter_bug("destination"))?;

                if src_stride == bps {
                    // One sample per pixel in the block (planar, or a single-band
                    // chunky file): the run is already contiguous.
                    let src = staged
                        .get(src_off..src_off + run * bps)
                        .ok_or_else(|| scatter_bug("source"))?;
                    write_run(dst, src)?;
                } else {
                    // Chunky multi-band: de-interleave this run into the gather
                    // row. The interleaved plane is never materialised.
                    let gathered = gather
                        .get_mut(..run * bps)
                        .ok_or_else(|| scatter_bug("gather"))?;
                    for i in 0..run {
                        let from = src_off + i * src_stride;
                        let sample = staged
                            .get(from..from + bps)
                            .ok_or_else(|| scatter_bug("source"))?;
                        let into = gathered
                            .get_mut(i * bps..(i + 1) * bps)
                            .ok_or_else(|| scatter_bug("gather"))?;
                        into.copy_from_slice(sample);
                    }
                    write_run(dst, gathered)?;
                }
            }
        }

        group_start = group_end;
    }

    Ok(())
}

/// Splits `dst` into one disjoint slice per block row and runs `body` on each,
/// serially, reusing one scratch buffer throughout.
fn scatter_serial<S, D, F>(
    reader: &CogReader<S>,
    plan: &ReadPlan,
    dst: &mut [D],
    units_per_pixel: usize,
    write_run: &F,
) -> Result<()>
where
    S: DataSource,
    D: Copy,
    F: Fn(&mut [D], &[u8]) -> Result<()>,
{
    let row_units = plan.win_w * units_per_pixel;
    let mut scratch = Scratch::new(plan);
    let mut rest = dst;
    for ty in plan.by0..plan.by1 {
        let take = plan.rows_in_block_row(ty) * row_units;
        if take > rest.len() {
            return Err(scatter_bug("block row"));
        }
        let (head, tail) = core::mem::take(&mut rest).split_at_mut(take);
        scatter_block_row(
            reader,
            plan,
            ty,
            head,
            units_per_pixel,
            &mut scratch,
            write_run,
        )?;
        rest = tail;
    }
    Ok(())
}

/// Same as [`scatter_serial`], but the block rows are decoded concurrently.
///
/// Each rayon worker owns its own scratch buffer and writes into a slice of
/// `dst` that no other worker can reach, so there is no locking, no `unsafe`,
/// and the output is bit-identical to the serial path.
///
/// # Why this path does not stage whole block rows
///
/// The scratch is per-worker, so block-row staging would multiply its footprint
/// by the thread count — eight workers staging a 4 MiB block row each is 32 MiB
/// of staging traffic that no longer fits in cache, and cache residency is the
/// entire reason staging is faster serially. Measured on an 8-core Apple
/// M-series it cost 10–20 % on the raw-byte and same-width typed reads. A
/// parallel read therefore keeps the one-block scratch and the block-at-a-time
/// copy order (see [`ReadPlan::without_block_row_staging`]); it already spreads
/// the destination over as many sequential streams as there are workers.
#[cfg(feature = "parallel")]
fn scatter_parallel<S, D, F>(
    reader: &CogReader<S>,
    plan: &ReadPlan,
    dst: &mut [D],
    units_per_pixel: usize,
    write_run: &F,
) -> Result<()>
where
    S: DataSource,
    D: Copy + Send,
    F: Fn(&mut [D], &[u8]) -> Result<()> + Sync + Send,
{
    use rayon::iter::{IntoParallelIterator, ParallelIterator};

    let plan = &plan.without_block_row_staging();
    let row_units = plan.win_w * units_per_pixel;
    let mut pieces: Vec<(u32, &mut [D])> =
        Vec::with_capacity(plan.by1.saturating_sub(plan.by0) as usize);
    let mut rest = dst;
    for ty in plan.by0..plan.by1 {
        let take = plan.rows_in_block_row(ty) * row_units;
        if take > rest.len() {
            return Err(scatter_bug("block row"));
        }
        let (head, tail) = core::mem::take(&mut rest).split_at_mut(take);
        pieces.push((ty, head));
        rest = tail;
    }

    pieces.into_par_iter().try_for_each_init(
        || Scratch::new(plan),
        |scratch, (ty, out)| {
            scatter_block_row(reader, plan, ty, out, units_per_pixel, scratch, write_run)
        },
    )
}

/// Raw-byte scatter: copies the band's samples verbatim.
fn scatter_bytes<S: DataSource>(
    reader: &CogReader<S>,
    plan: &ReadPlan,
    dst: &mut [u8],
) -> Result<()> {
    let write_run = |out: &mut [u8], src: &[u8]| -> Result<()> {
        if out.len() != src.len() {
            return Err(scatter_bug("run length"));
        }
        out.copy_from_slice(src);
        Ok(())
    };

    #[cfg(feature = "parallel")]
    if plan.should_parallelise() {
        return scatter_parallel(reader, plan, dst, plan.geom.bytes_per_sample, &write_run);
    }

    scatter_serial(reader, plan, dst, plan.geom.bytes_per_sample, &write_run)
}

/// Typed scatter: converts the band's samples into `T` in the same pass.
///
/// Parallelises exactly like [`scatter_bytes`]: the destination is split into
/// disjoint block-row slices and each worker runs `convert_raw_into` straight
/// into its own slice, so the conversion stays fused with the decode and no
/// shared or full-size intermediate buffer is ever materialised. `T: Send` comes
/// from [`RasterElement`], whose ten implementors are all primitive scalars.
fn scatter_typed<S: DataSource, T: RasterElement>(
    reader: &CogReader<S>,
    plan: &ReadPlan,
    dst: &mut [T],
    src_type: RasterDataType,
) -> Result<()> {
    let write_run =
        |out: &mut [T], src: &[u8]| -> Result<()> { convert_raw_into(src, src_type, out) };

    #[cfg(feature = "parallel")]
    if plan.should_parallelise() {
        return scatter_parallel(reader, plan, dst, 1, &write_run);
    }

    scatter_serial(reader, plan, dst, 1, &write_run)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

impl<S: DataSource> GeoTiffReader<S> {
    /// Returns the exact length, in bytes, of one band of `level`.
    ///
    /// This is the length [`Self::read_band_into`] requires and the length
    /// [`Self::read_band`] returns: `width × height × bytes_per_sample` for a
    /// **single** band, not the interleaved plane.
    ///
    /// # Errors
    /// Returns an error if `level` names no overview or the declared dimensions
    /// overflow `usize`.
    pub fn band_byte_len(&self, level: usize) -> Result<usize> {
        let geom = LevelGeometry::resolve(&self.cog_reader, level)?;
        geom.width
            .checked_mul(geom.height)
            .and_then(|v| v.checked_mul(geom.bytes_per_sample))
            .ok_or_else(|| {
                OxiGeoError::Format(FormatError::InvalidHeader {
                    message: "band byte size overflows usize".to_string(),
                })
            })
    }

    /// Returns the number of pixels in one band of `level`.
    ///
    /// This is the length [`Self::read_band_into_typed`] requires.
    ///
    /// # Errors
    /// Returns an error if `level` names no overview or the declared dimensions
    /// overflow `usize`.
    pub fn band_pixel_count(&self, level: usize) -> Result<usize> {
        let geom = LevelGeometry::resolve(&self.cog_reader, level)?;
        geom.width.checked_mul(geom.height).ok_or_else(|| {
            OxiGeoError::Format(FormatError::InvalidHeader {
                message: "band pixel count overflows usize".to_string(),
            })
        })
    }

    /// Reads one band's data.
    ///
    /// Returns exactly that band's samples — `width × height × bytes_per_sample`
    /// bytes — de-interleaved from the file's pixel-interleaved (chunky) storage
    /// or selected out of its planar storage, as appropriate. `band` is
    /// zero-based.
    ///
    /// This performs a single allocation (the returned buffer) plus one reusable
    /// tile-sized scratch; it is [`Self::read_band_into`] with the allocation
    /// done for you.
    ///
    /// Samples are returned in the **host's** byte order (crate-level *Byte order
    /// of decoded samples*).
    ///
    /// # Errors
    /// Returns an error if `band` is out of range, if `level` names no overview,
    /// or if a block cannot be read or decoded.
    pub fn read_band(&self, level: usize, band: usize) -> Result<Vec<u8>> {
        let plan = ReadPlan::full_band(&self.cog_reader, level, band)?;
        // Dimensions come from untrusted IFD tags, so the allocation is still
        // bounded even though nothing but the caller's band is materialised.
        let len = checked_band_bytes(
            plan.geom.width,
            plan.geom.height,
            plan.geom.bytes_per_sample,
            1,
        )?;
        let mut out = vec![0u8; len];
        scatter_bytes(&self.cog_reader, &plan, &mut out)?;
        Ok(out)
    }

    /// Decodes one band directly into `dst`, with no intermediate full-band
    /// allocation.
    ///
    /// `dst` must be exactly [`Self::band_byte_len`] bytes long. Blocks are
    /// decoded into a single reusable scratch buffer and scattered straight into
    /// `dst`; for a chunky multi-band file the de-interleave happens during that
    /// scatter, so the interleaved plane is never materialised.
    ///
    /// This is the raw-bytes half of the GDAL `read_into_slice` equivalent; see
    /// [`Self::read_band_into_typed`] to convert the element type in the same
    /// pass. "Raw" means unconverted, not un-normalised: the bytes are the
    /// band's samples in the **host's** byte order, so `bytemuck::cast_slice`
    /// over them is correct for an `MM` file too (crate-level *Byte order of
    /// decoded samples*).
    ///
    /// # Errors
    /// Returns an error if `band` is out of range, if `dst.len()` is not exactly
    /// the band's byte length, if `level` names no overview, or if a block cannot
    /// be read or decoded.
    ///
    /// # Examples
    /// ```ignore
    /// let mut buf = vec![0u8; reader.band_byte_len(0)?];
    /// reader.read_band_into(0, 0, &mut buf)?;
    /// ```
    pub fn read_band_into(&self, level: usize, band: usize, dst: &mut [u8]) -> Result<()> {
        let plan = ReadPlan::full_band(&self.cog_reader, level, band)?;
        plan.check_len(dst.len(), plan.output_bytes()?, "read_band_into")?;
        scatter_bytes(&self.cog_reader, &plan, dst)
    }

    /// Decodes one band into `dst`, converting from the file's element type to
    /// `T` in the same pass.
    ///
    /// `dst` must be exactly [`Self::band_pixel_count`] elements long — one
    /// element per pixel, *not* per byte. A `Float32` file read into a
    /// `&mut [f64]` therefore costs zero extra full-size buffers and one pass,
    /// instead of the `as_bytes()` + `cast_slice` + `mapv(|v| v as f64)` dance
    /// that needs two of each.
    ///
    /// Conversion follows [`convert_raw_into`]: saturating, with floats rounded
    /// to nearest (halves away from zero), matching GDAL's `RasterIO`. It reads
    /// native-endian, which is sound because the decode has already normalised
    /// the samples to the **host's** byte order (crate-level *Byte order of
    /// decoded samples*) — this is the end-to-end path cool-japan/oxigeo#14 is
    /// about.
    ///
    /// With the `parallel` feature the block rows are decoded concurrently and
    /// each worker converts its own rows straight into its slice of `dst`, so
    /// the fused conversion and the parallel decode come together; the result is
    /// bit-identical to the serial path.
    ///
    /// # Errors
    /// Returns an error if `band` is out of range, if `dst.len()` is not exactly
    /// the band's pixel count, if the file's sample type is not a recognised
    /// [`RasterDataType`], if `level` names no overview, or if a block cannot be
    /// read or decoded.
    ///
    /// # Examples
    /// ```ignore
    /// let mut pixels = vec![0.0f64; reader.band_pixel_count(0)?];
    /// reader.read_band_into_typed(0, 0, &mut pixels)?;
    /// let array = ndarray::Array2::from_shape_vec((h, w), pixels)?;
    /// ```
    pub fn read_band_into_typed<T: RasterElement>(
        &self,
        level: usize,
        band: usize,
        dst: &mut [T],
    ) -> Result<()> {
        let plan = ReadPlan::full_band(&self.cog_reader, level, band)?;
        let src_type = plan.typed_source_type("read_band_into_typed")?;
        plan.check_len(dst.len(), plan.output_pixels()?, "read_band_into_typed")?;
        scatter_typed(&self.cog_reader, &plan, dst, src_type)
    }

    /// Reads a rectangular window of one band, touching only the tiles or strips
    /// that overlap it.
    ///
    /// The window origin is `(x, y)` in the level's pixel grid and the returned
    /// buffer is `width × height × bytes_per_sample` bytes, row-major, with the
    /// samples in the **host's** byte order (crate-level *Byte order of decoded
    /// samples*).
    ///
    /// # Errors
    /// Returns an error if `band` is out of range, if the window is zero-sized or
    /// extends past the raster extent, if `level` names no overview, or if a
    /// block cannot be read or decoded.
    pub fn read_window(
        &self,
        level: usize,
        band: usize,
        x: u64,
        y: u64,
        width: u64,
        height: u64,
    ) -> Result<Vec<u8>> {
        let plan = ReadPlan::window(&self.cog_reader, level, band, x, y, width, height)?;
        let len = checked_band_bytes(plan.win_w, plan.win_h, plan.geom.bytes_per_sample, 1)?;
        let mut out = vec![0u8; len];
        scatter_bytes(&self.cog_reader, &plan, &mut out)?;
        Ok(out)
    }

    /// Reads a rectangular window of one band into `dst`, touching only the tiles
    /// or strips that overlap it.
    ///
    /// `dst` must be exactly `width × height × bytes_per_sample` bytes long, and
    /// receives the samples in the **host's** byte order (crate-level *Byte order
    /// of decoded samples*).
    ///
    /// # Errors
    /// Returns an error if `band` is out of range, if the window is zero-sized or
    /// extends past the raster extent, if `dst.len()` is wrong, if `level` names
    /// no overview, or if a block cannot be read or decoded.
    pub fn read_window_into(
        &self,
        level: usize,
        band: usize,
        x: u64,
        y: u64,
        width: u64,
        height: u64,
        dst: &mut [u8],
    ) -> Result<()> {
        let plan = ReadPlan::window(&self.cog_reader, level, band, x, y, width, height)?;
        plan.check_len(dst.len(), plan.output_bytes()?, "read_window_into")?;
        scatter_bytes(&self.cog_reader, &plan, dst)
    }

    /// Reads a rectangular window of one band into `dst`, converting from the
    /// file's element type to `T` in the same pass.
    ///
    /// `dst` must be exactly `width × height` elements long. Like
    /// [`Self::read_band_into_typed`], the conversion reads samples the decode
    /// has already normalised to the **host's** byte order.
    ///
    /// Like [`Self::read_band_into_typed`], the `parallel` feature spreads the
    /// block decode over rayon workers with the conversion still fused per
    /// worker.
    ///
    /// # Errors
    /// Returns an error if `band` is out of range, if the window is zero-sized or
    /// extends past the raster extent, if `dst.len()` is wrong, if the file's
    /// sample type is not a recognised [`RasterDataType`], if `level` names no
    /// overview, or if a block cannot be read or decoded.
    pub fn read_window_into_typed<T: RasterElement>(
        &self,
        level: usize,
        band: usize,
        x: u64,
        y: u64,
        width: u64,
        height: u64,
        dst: &mut [T],
    ) -> Result<()> {
        let plan = ReadPlan::window(&self.cog_reader, level, band, x, y, width, height)?;
        let src_type = plan.typed_source_type("read_window_into_typed")?;
        plan.check_len(dst.len(), plan.output_pixels()?, "read_window_into_typed")?;
        scatter_typed(&self.cog_reader, &plan, dst, src_type)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tiff::{PhotometricInterpretation, SampleFormat};

    fn base_info() -> ImageInfo {
        ImageInfo {
            width: 64,
            height: 48,
            bits_per_sample: vec![16, 16, 16],
            samples_per_pixel: 3,
            sample_format: SampleFormat::UnsignedInteger,
            compression: Compression::None,
            photometric: PhotometricInterpretation::Rgb,
            planar_config: PlanarConfiguration::Chunky,
            tile_width: Some(16),
            tile_height: Some(16),
            rows_per_strip: None,
            predictor: Predictor::None,
        }
    }

    #[test]
    fn geometry_matches_tiled_layout() {
        let geom = LevelGeometry::from_info(&base_info()).expect("geometry");
        assert_eq!(geom.block_width, 16);
        assert_eq!(geom.block_height, 16);
        assert_eq!(geom.blocks_across, 4);
        assert_eq!(geom.blocks_down, 3);
        assert_eq!(geom.bytes_per_sample, 2);
        assert_eq!(geom.samples_in_block(), 3);
        assert_eq!(geom.src_pixel_stride(), 6);
        assert_eq!(geom.band_offset(2), 4);
        // Every tile is full height even at the bottom edge.
        assert_eq!(geom.block_rows(2), 16);
        assert!(matches!(geom.block_decoded_bytes(0), Ok(n) if n == 16 * 16 * 2 * 3));
    }

    #[test]
    fn geometry_matches_striped_layout() {
        let mut info = base_info();
        info.tile_width = None;
        info.tile_height = None;
        info.rows_per_strip = Some(20);
        let geom = LevelGeometry::from_info(&info).expect("geometry");
        assert!(!geom.is_tiled);
        assert_eq!(geom.block_width, 64);
        assert_eq!(geom.block_height, 20);
        assert_eq!(geom.blocks_across, 1);
        assert_eq!(geom.blocks_down, 3);
        // The final strip is short: 48 - 2*20 = 8 rows.
        assert_eq!(geom.block_rows(0), 20);
        assert_eq!(geom.block_rows(2), 8);
        assert!(matches!(geom.block_decoded_bytes(2), Ok(n) if n == 64 * 8 * 2 * 3));
    }

    #[test]
    fn geometry_planar_block_holds_one_sample_per_pixel() {
        let mut info = base_info();
        info.planar_config = PlanarConfiguration::Planar;
        let geom = LevelGeometry::from_info(&info).expect("geometry");
        assert_eq!(geom.samples_in_block(), 1);
        assert_eq!(geom.src_pixel_stride(), 2);
        assert_eq!(geom.band_offset(2), 0);
        assert!(matches!(geom.block_decoded_bytes(0), Ok(n) if n == 16 * 16 * 2));
        assert!(!geom.uses_native_block_layout());
    }

    #[test]
    fn geometry_rejects_zero_sized_blocks() {
        let mut info = base_info();
        info.tile_width = Some(0);
        assert!(LevelGeometry::from_info(&info).is_err());

        let mut info = base_info();
        info.tile_height = Some(0);
        assert!(LevelGeometry::from_info(&info).is_err());

        let mut info = base_info();
        info.tile_width = None;
        info.tile_height = None;
        info.rows_per_strip = Some(0);
        assert!(LevelGeometry::from_info(&info).is_err());
    }

    #[test]
    fn group_stays_within_the_documented_memory_bound() {
        // A striped raster (one block column) never groups, so it allocates and
        // copies exactly what it did before grouping existed.
        assert_eq!(group_blocks_for(1 << 20, 1), 1);
        // Sub-byte samples report a zero-byte block; the guard must not divide.
        assert_eq!(group_blocks_for(0, 64), 1);

        // 256x256 Float32 tiles: a 4000-px-wide raster's block row is 16 of them,
        // 4 MiB, and is staged whole.
        let tile_bytes = 256 * 256 * 4;
        assert_eq!(group_blocks_for(tile_bytes, 16), 16);
        assert_eq!(group_blocks_for(tile_bytes, 5), 5);
        // Exactly at the budget, still staged.
        assert_eq!(
            group_blocks_for(tile_bytes, (BLOCK_GROUP_MAX_BYTES / tile_bytes) as u32),
            (BLOCK_GROUP_MAX_BYTES / tile_bytes) as u32
        );

        // A 100 000-px-wide raster's block row is 391 tiles — 100 MiB — so the
        // read falls back to the one-block scratch rather than being handed a
        // buffer that size. Grouping is all-or-nothing: a partial group buys
        // nothing, so it is never taken.
        assert_eq!(group_blocks_for(tile_bytes, 391), 1);
        // One block bigger than the whole budget likewise yields one block, so
        // the staging buffer never shrinks below what the decode needs.
        assert_eq!(group_blocks_for(BLOCK_GROUP_MAX_BYTES * 3, 64), 1);
        // Overflowing the multiplication must fall back, not wrap.
        assert_eq!(group_blocks_for(usize::MAX / 2, u32::MAX), 1);

        // Whatever the answer, the staging buffer stays within the budget or is
        // exactly one block.
        for &(block_bytes, blocks) in &[
            (tile_bytes, 16u32),
            (tile_bytes, 391),
            (1 << 20, 3),
            (BLOCK_GROUP_MAX_BYTES * 3, 64),
        ] {
            let group = group_blocks_for(block_bytes, blocks) as usize;
            assert!(group == 1 || group * block_bytes <= BLOCK_GROUP_MAX_BYTES);
            assert!(group <= blocks as usize);
        }
    }

    #[test]
    fn geometry_sub_byte_samples_report_zero_bytes() {
        // Bilevel imagery: BitsPerSample = 1 rounds to 0 bytes per sample, which
        // is how the reader has always described these files (and why they read
        // back as an empty band).
        let mut info = base_info();
        info.bits_per_sample = vec![1];
        info.samples_per_pixel = 1;
        let geom = LevelGeometry::from_info(&info).expect("geometry");
        assert_eq!(geom.bytes_per_sample, 0);
        assert!(matches!(geom.block_decoded_bytes(0), Ok(0)));
    }
}
