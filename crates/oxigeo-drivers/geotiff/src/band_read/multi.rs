//! Multi-band reads that decode every block exactly once.
//!
//! The single-band engine in the parent module is the right shape for one band,
//! and the wrong shape for `n` of them. A chunky (`PlanarConfiguration = 1`)
//! block physically holds *every* band, so asking the single-band engine for
//! three bands decodes each block three times and throws two thirds of each
//! decode away. Decompression dominates the read, so that is an `n`× waste on
//! the most common raster layout there is — RGB, RGBA, multispectral.
//!
//! The engine here decodes a block once and pulls every requested band out of it
//! before the block is discarded. It is the driver-level counterpart of the
//! facade's `Dataset::read_interleaved`, which until now had to be built out of
//! `read_band_into_typed` calls and paid exactly that `n`× cost.
//!
//! # Band selection
//!
//! `bands` is an *ordered list*, not a set: bands may be reordered (`[2, 1, 0]`
//! is BGR out of an RGB file) and repeated (`[0, 0, 0]` fans a grey band out to
//! RGB). Nothing is deduplicated or sorted, because the position in the list is
//! the position in the output pixel.
//!
//! # Layout handling
//!
//! * **A block that holds every band** — every chunky file, and a single-band
//!   planar one, i.e. `LevelGeometry::uses_native_block_layout` — takes the
//!   single-decode path in `read_native_layout`: one decode per block, all slots
//!   filled from it. This is the case the module exists for.
//! * **Multi-band planar** — each block holds exactly *one* band, so there is no
//!   shared decode to exploit and the per-band engine is already optimal. Those
//!   reads simply run the parent module's scatter once per distinct band, with a
//!   strided write so each pass lands in its own slot of the interleaved output
//!   (`read_planar_layout`). A band repeated in the selection is copied from the
//!   slot that already holds it rather than decoded again.
//!
//! # Buffers
//!
//! Same discipline as the single-band engine: nothing is allocated per block.
//! The native-layout path allocates three scratch buffers per read (per rayon
//! worker under the `parallel` feature) — the staged block group it inherits
//! from `ReadPlan::scratch_bytes`, one de-interleave row, and one lane buffer of
//! `bands.len() × run` converted samples — and the planar path allocates only
//! the parent module's own scratch. The lane buffer is what lets the destination
//! be written strictly front to back even though the samples arrive band by
//! band: it is at most one output row wide, so it stays cache-resident while the
//! destination, which is `win_h` times larger, never has to be revisited.
//!
//! # Byte order
//!
//! Unchanged from the parent module: blocks leave `decode_block` already
//! normalised to the host's byte order, strictly after predictor reversal, so
//! the interleave below moves samples that are already native.

use oxigeo_core::buffer::{RasterElement, convert_raw_into};
use oxigeo_core::error::{FormatError, OxiGeoError, Result};
use oxigeo_core::io::DataSource;
use oxigeo_core::types::RasterDataType;

use crate::GeoTiffReader;
use crate::cog::CogReader;

#[cfg(feature = "parallel")]
use super::scatter_parallel;
use super::{ReadPlan, decode_block, scatter_bug, scatter_serial};

/// Samples converted in one `convert_raw_into` call on the planar path.
///
/// The conversion dispatches on a runtime `RasterDataType`, so converting one
/// sample at a time would pay that dispatch per pixel; a fixed stack window
/// amortises it without needing a heap scratch the shared `Fn` write closure
/// could not own anyway.
const PLANAR_CONVERT_CHUNK: usize = 64;

// ---------------------------------------------------------------------------
// Plan
// ---------------------------------------------------------------------------

/// A validated multi-band read: the single-band plan for the first slot, plus
/// the ordered band list every block is fanned out into.
#[derive(Debug, Clone, Copy)]
struct MultiPlan<'a> {
    /// The parent module's plan. Every field except `band` is band-independent,
    /// so this doubles as the geometry, the block range and the scratch sizing
    /// for all slots.
    base: ReadPlan,
    /// The requested bands, in output order, with duplicates preserved.
    bands: &'a [usize],
    /// Element type of the file's samples.
    src_type: RasterDataType,
    /// Longest run of one band the scatter ever copies out of a single block,
    /// i.e. the width of one lane.
    run_max: usize,
}

impl<'a> MultiPlan<'a> {
    /// Validates `bands` against the plan's raster and sizes the lane buffer.
    fn new(base: ReadPlan, bands: &'a [usize], operation: &'static str) -> Result<Self> {
        if bands.is_empty() {
            return Err(OxiGeoError::invalid_parameter_builder(
                "bands",
                "band selection must name at least one band",
            )
            .with_operation(operation)
            .with_suggestion("Pass the bands you want, in output order, e.g. &[0, 1, 2]")
            .build());
        }
        for &band in bands {
            if band >= base.geom.samples_per_pixel {
                return Err(OxiGeoError::invalid_parameter_builder(
                    "bands",
                    "band index is out of range for this raster",
                )
                .with_operation(operation)
                .with_parameter("band", band.to_string())
                .with_parameter("band_count", base.geom.samples_per_pixel.to_string())
                .with_suggestion("Band indices are zero-based; use GeoTiffReader::band_count()")
                .build());
            }
        }

        let src_type = base.typed_source_type(operation)?;
        let run_max = base.geom.block_width.min(base.win_w);
        Ok(Self {
            base,
            bands,
            src_type,
            run_max,
        })
    }

    /// Number of output slots per pixel — the interleave factor.
    const fn slots(&self) -> usize {
        self.bands.len()
    }

    /// Exact number of elements the destination must hold.
    fn output_len(&self) -> Result<usize> {
        self.base
            .output_pixels()?
            .checked_mul(self.slots())
            .ok_or_else(|| {
                OxiGeoError::Format(FormatError::InvalidHeader {
                    message: format!(
                        "{}x{} pixels x {} bands overflows usize",
                        self.base.win_w,
                        self.base.win_h,
                        self.slots()
                    ),
                })
            })
    }

    /// Sizes of the three per-worker scratch buffers.
    ///
    /// Computed once so the rayon initialiser that builds one set per worker
    /// cannot fail — `try_for_each_init` has no way to report an error raised
    /// while initialising.
    fn scratch_sizes(&self) -> Result<ScratchSizes> {
        let lane = self.run_max.checked_mul(self.slots()).ok_or_else(|| {
            OxiGeoError::Format(FormatError::InvalidHeader {
                message: "interleave lane buffer size overflows usize".to_string(),
            })
        })?;
        // A block that already stores one sample per pixel needs no
        // de-interleave row: its runs are contiguous.
        let gather = if self.base.geom.src_pixel_stride() == self.base.geom.bytes_per_sample {
            0
        } else {
            self.run_max
                .checked_mul(self.base.geom.bytes_per_sample)
                .ok_or_else(|| {
                    OxiGeoError::Format(FormatError::InvalidHeader {
                        message: "interleave gather row size overflows usize".to_string(),
                    })
                })?
        };
        Ok(ScratchSizes {
            block: self.base.scratch_bytes,
            gather,
            lane,
        })
    }

    /// Rejects a destination whose length is not exactly what this plan writes.
    fn check_len(&self, actual: usize, operation: &'static str) -> Result<()> {
        let expected = self.output_len()?;
        if actual == expected {
            return Ok(());
        }
        Err(OxiGeoError::invalid_parameter_builder(
            "dst",
            "destination length must be exactly pixels x bands",
        )
        .with_operation(operation)
        .with_parameter("dst_len", actual.to_string())
        .with_parameter("required_len", expected.to_string())
        .with_parameter(
            "region",
            format!(
                "{}x{} x {} bands",
                self.base.win_w,
                self.base.win_h,
                self.slots()
            ),
        )
        .with_suggestion("Size the buffer with GeoTiffReader::band_pixel_count x bands.len()")
        .build())
    }

    /// The same plan with block-row staging turned off; see
    /// `ReadPlan::without_block_row_staging` for why the parallel path wants it.
    #[cfg(feature = "parallel")]
    fn without_block_row_staging(&self) -> Self {
        Self {
            base: self.base.without_block_row_staging(),
            ..*self
        }
    }
}

// ---------------------------------------------------------------------------
// Scratch
// ---------------------------------------------------------------------------

/// Lengths of the three scratch buffers, all checked up front.
#[derive(Debug, Clone, Copy)]
struct ScratchSizes {
    block: usize,
    gather: usize,
    lane: usize,
}

/// The reusable buffers one worker needs for a whole multi-band read.
///
/// Allocated once per read, or once per rayon worker — never per block.
struct MultiScratch<T> {
    /// Staging area for one group of horizontally adjacent decoded blocks,
    /// exactly as `Scratch::block`.
    block: Vec<u8>,
    /// One de-interleaved run of a single band, in file element width. Empty
    /// when the block already stores one sample per pixel.
    gather: Vec<u8>,
    /// `slots x run_max` converted samples, band-major, so the destination can
    /// then be written pixel-major in one forward pass.
    lane: Vec<T>,
}

impl<T: RasterElement> MultiScratch<T> {
    fn new(sizes: ScratchSizes) -> Self {
        Self {
            block: vec![0u8; sizes.block],
            gather: vec![0u8; sizes.gather],
            lane: vec![T::default(); sizes.lane],
        }
    }
}

// ---------------------------------------------------------------------------
// Native-layout scatter: one decode per block, every slot filled from it
// ---------------------------------------------------------------------------

/// Decodes every block of block row `ty` **once** and fills every slot of `out`
/// from those decodes.
///
/// `out` holds exactly this block row's output rows, pixel-interleaved.
///
/// The shape mirrors `scatter_block_row`: a group of horizontally adjacent
/// blocks is decoded into one staging buffer and only then copied out, so the
/// destination is written front to back. The difference is the copy-out, which
/// runs per source run:
///
/// 1. every requested band's samples for that run are de-interleaved out of the
///    staged block and converted into its own lane, and
/// 2. the lanes are woven into the destination in pixel order.
///
/// Step 1 reads the block `slots` times, but the block is small and already in
/// cache; the decode — the expensive part — happened once.
fn native_block_row<S: DataSource, T: RasterElement>(
    reader: &CogReader<S>,
    plan: &MultiPlan<'_>,
    ty: u32,
    out: &mut [T],
    scratch: &mut MultiScratch<T>,
) -> Result<()> {
    let base = &plan.base;
    let geom = &base.geom;
    let bps = geom.bytes_per_sample;
    let src_stride = geom.src_pixel_stride();
    let slots = plan.slots();
    if slots == 0 {
        return Err(scatter_bug("band slot"));
    }
    let row_units = base.win_w * slots;
    let run_max = plan.run_max;

    let block_bytes = geom.block_decoded_bytes(ty)?;
    let MultiScratch {
        block,
        gather,
        lane,
    } = scratch;

    let block_y0 = (ty as usize) * geom.block_height;
    let row_start = base.y.max(block_y0);
    let row_end = (base.y + base.win_h).min(block_y0 + geom.block_rows(ty));
    if row_end <= row_start {
        return Ok(());
    }

    let group_blocks = base.group_blocks.max(1);
    let mut group_start = base.bx0;
    while group_start < base.bx1 {
        let group_end = base.bx1.min(group_start.saturating_add(group_blocks));
        let group_len = (group_end - group_start) as usize;
        let staged_bytes = group_len
            .checked_mul(block_bytes)
            .ok_or_else(|| scatter_bug("scratch"))?;
        let staged = block
            .get_mut(..staged_bytes)
            .ok_or_else(|| scatter_bug("scratch"))?;

        // The whole point of this module: one decode per block, whatever
        // `bands` asks for.
        for (block_index, tx) in (group_start..group_end).enumerate() {
            let block_x0 = (tx as usize) * geom.block_width;
            let col_start = base.x.max(block_x0);
            let col_end = (base.x + base.win_w).min(block_x0 + geom.block_width);
            if col_end <= col_start {
                continue;
            }
            let offset = block_index * block_bytes;
            let into = staged
                .get_mut(offset..offset + block_bytes)
                .ok_or_else(|| scatter_bug("scratch"))?;
            decode_block(reader, base, tx, ty, into)?;
        }

        for row in row_start..row_end {
            let src_row = row - block_y0;
            let out_row = (row - row_start) * row_units;
            for (block_index, tx) in (group_start..group_end).enumerate() {
                let block_x0 = (tx as usize) * geom.block_width;
                let col_start = base.x.max(block_x0);
                let col_end = (base.x + base.win_w).min(block_x0 + geom.block_width);
                if col_end <= col_start {
                    continue;
                }
                let run = col_end - col_start;
                let src_col = col_start - block_x0;
                let out_col = col_start - base.x;
                let run_base =
                    block_index * block_bytes + (src_row * geom.block_width + src_col) * src_stride;

                // 1. one lane per output slot, converted in one call each.
                for (slot, &band) in plan.bands.iter().enumerate() {
                    let lane_start = slot * run_max;
                    let lane_out = lane
                        .get_mut(lane_start..lane_start + run)
                        .ok_or_else(|| scatter_bug("lane"))?;
                    let from = run_base + geom.band_offset(band);
                    if src_stride == bps {
                        // One sample per pixel in the block: the run is already
                        // contiguous, so no de-interleave is needed.
                        let src = staged
                            .get(from..from + run * bps)
                            .ok_or_else(|| scatter_bug("source"))?;
                        convert_raw_into(src, plan.src_type, lane_out)?;
                    } else {
                        let gathered = gather
                            .get_mut(..run * bps)
                            .ok_or_else(|| scatter_bug("gather"))?;
                        for i in 0..run {
                            let at = from + i * src_stride;
                            let sample = staged
                                .get(at..at + bps)
                                .ok_or_else(|| scatter_bug("source"))?;
                            gathered
                                .get_mut(i * bps..(i + 1) * bps)
                                .ok_or_else(|| scatter_bug("gather"))?
                                .copy_from_slice(sample);
                        }
                        convert_raw_into(gathered, plan.src_type, lane_out)?;
                    }
                }

                // 2. weave the lanes into the destination, pixel by pixel and
                //    strictly forwards.
                let out_off = out_row + out_col * slots;
                let dst = out
                    .get_mut(out_off..out_off + run * slots)
                    .ok_or_else(|| scatter_bug("destination"))?;
                for (i, pixel) in dst.chunks_exact_mut(slots).enumerate() {
                    for (slot, cell) in pixel.iter_mut().enumerate() {
                        *cell = *lane
                            .get(slot * run_max + i)
                            .ok_or_else(|| scatter_bug("lane"))?;
                    }
                }
            }
        }

        group_start = group_end;
    }

    Ok(())
}

/// Splits `dst` into one disjoint slice per block row and fills each serially,
/// reusing one set of scratch buffers throughout.
fn native_serial<S: DataSource, T: RasterElement>(
    reader: &CogReader<S>,
    plan: &MultiPlan<'_>,
    dst: &mut [T],
) -> Result<()> {
    let row_units = plan.base.win_w * plan.slots();
    let mut scratch = MultiScratch::<T>::new(plan.scratch_sizes()?);
    let mut rest = dst;
    for ty in plan.base.by0..plan.base.by1 {
        let take = plan.base.rows_in_block_row(ty) * row_units;
        if take > rest.len() {
            return Err(scatter_bug("block row"));
        }
        let (head, tail) = core::mem::take(&mut rest).split_at_mut(take);
        native_block_row(reader, plan, ty, head, &mut scratch)?;
        rest = tail;
    }
    Ok(())
}

/// Same as `native_serial`, with the block rows decoded concurrently.
///
/// Each rayon worker owns its scratch and writes into a slice of `dst` no other
/// worker can reach, so there is no locking, no `unsafe`, and the output is
/// bit-identical to the serial path. Block-row staging is dropped for the same
/// reason `scatter_parallel` drops it: the scratch is per-worker, and staging a
/// whole block row per worker stops it being cache-resident.
#[cfg(feature = "parallel")]
fn native_parallel<S: DataSource, T: RasterElement>(
    reader: &CogReader<S>,
    plan: &MultiPlan<'_>,
    dst: &mut [T],
) -> Result<()> {
    use rayon::iter::{IntoParallelIterator, ParallelIterator};

    let plan = &plan.without_block_row_staging();
    let sizes = plan.scratch_sizes()?;
    let row_units = plan.base.win_w * plan.slots();
    let mut pieces: Vec<(u32, &mut [T])> =
        Vec::with_capacity(plan.base.by1.saturating_sub(plan.base.by0) as usize);
    let mut rest = dst;
    for ty in plan.base.by0..plan.base.by1 {
        let take = plan.base.rows_in_block_row(ty) * row_units;
        if take > rest.len() {
            return Err(scatter_bug("block row"));
        }
        let (head, tail) = core::mem::take(&mut rest).split_at_mut(take);
        pieces.push((ty, head));
        rest = tail;
    }

    pieces.into_par_iter().try_for_each_init(
        || MultiScratch::<T>::new(sizes),
        |scratch, (ty, out)| native_block_row(reader, plan, ty, out, scratch),
    )
}

/// Whole-block-holds-every-band path: chunky files, and single-band planar ones.
fn read_native_layout<S: DataSource, T: RasterElement>(
    reader: &CogReader<S>,
    plan: &MultiPlan<'_>,
    dst: &mut [T],
) -> Result<()> {
    #[cfg(feature = "parallel")]
    if plan.base.should_parallelise() {
        return native_parallel(reader, plan, dst);
    }
    native_serial(reader, plan, dst)
}

// ---------------------------------------------------------------------------
// Planar layout: one pass per distinct band, written into its own slot
// ---------------------------------------------------------------------------

/// Runs the parent module's single-band scatter for `plan.band`, writing every
/// sample into slot `slot` of a `slots`-wide interleaved destination.
///
/// `units_per_pixel = slots` makes the parent's arithmetic address whole output
/// pixels; the write closure then picks this slot out of each of them.
fn scatter_slot<S: DataSource, T: RasterElement>(
    reader: &CogReader<S>,
    plan: &ReadPlan,
    src_type: RasterDataType,
    slot: usize,
    slots: usize,
    dst: &mut [T],
) -> Result<()> {
    let bps = plan.geom.bytes_per_sample;
    let write_run = move |out: &mut [T], src: &[u8]| -> Result<()> {
        if bps == 0 || slots == 0 {
            return Err(scatter_bug("sample width"));
        }
        if !src.len().is_multiple_of(bps) || out.len() != (src.len() / bps) * slots {
            return Err(scatter_bug("run length"));
        }
        let mut window = [T::default(); PLANAR_CONVERT_CHUNK];
        for (src_chunk, out_chunk) in src
            .chunks(PLANAR_CONVERT_CHUNK * bps)
            .zip(out.chunks_mut(PLANAR_CONVERT_CHUNK * slots))
        {
            let samples = src_chunk.len() / bps;
            let lane = window
                .get_mut(..samples)
                .ok_or_else(|| scatter_bug("lane"))?;
            convert_raw_into(src_chunk, src_type, lane)?;
            for (i, value) in lane.iter().enumerate() {
                *out_chunk
                    .get_mut(i * slots + slot)
                    .ok_or_else(|| scatter_bug("destination"))? = *value;
            }
        }
        Ok(())
    };

    #[cfg(feature = "parallel")]
    if plan.should_parallelise() {
        return scatter_parallel(reader, plan, dst, slots, &write_run);
    }
    scatter_serial(reader, plan, dst, slots, &write_run)
}

/// Copies slot `from` of every pixel of `dst` into slot `to`.
///
/// Used when the same planar band appears twice in the selection: the plane has
/// already been decoded into one slot, and decoding it again to fill the second
/// would be pure waste.
fn duplicate_slot<T: RasterElement>(dst: &mut [T], from: usize, to: usize, slots: usize) {
    if slots == 0 || from == to {
        return;
    }
    for pixel in dst.chunks_exact_mut(slots) {
        if let Some(&value) = pixel.get(from)
            && let Some(cell) = pixel.get_mut(to)
        {
            *cell = value;
        }
    }
}

/// Multi-band planar path: one block holds one band, so there is no shared
/// decode to exploit and each distinct band simply gets the parent module's
/// scatter, written into its own slot.
fn read_planar_layout<S: DataSource, T: RasterElement>(
    reader: &CogReader<S>,
    plan: &MultiPlan<'_>,
    dst: &mut [T],
) -> Result<()> {
    let slots = plan.slots();
    for (slot, &band) in plan.bands.iter().enumerate() {
        if let Some(done) = plan
            .bands
            .get(..slot)
            .and_then(|earlier| earlier.iter().position(|&b| b == band))
        {
            duplicate_slot(dst, done, slot, slots);
            continue;
        }
        let band_plan = ReadPlan { band, ..plan.base };
        scatter_slot(reader, &band_plan, plan.src_type, slot, slots, dst)?;
    }
    Ok(())
}

/// Dispatches a validated multi-band read to the layout-appropriate engine.
fn read_bands<S: DataSource, T: RasterElement>(
    reader: &CogReader<S>,
    plan: &MultiPlan<'_>,
    dst: &mut [T],
) -> Result<()> {
    if plan.base.geom.uses_native_block_layout() {
        read_native_layout(reader, plan, dst)
    } else {
        read_planar_layout(reader, plan, dst)
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

impl<S: DataSource> GeoTiffReader<S> {
    /// Decodes several bands into one pixel-interleaved buffer, decoding each
    /// block exactly once.
    ///
    /// `dst` must be exactly `band_pixel_count(level) × bands.len()` elements
    /// long and receives `RGBRGB…` order: element `p × bands.len() + s` is band
    /// `bands[s]` of pixel `p`, row-major.
    ///
    /// `bands` is an ordered list, and neither sorted nor deduplicated:
    ///
    /// * `&[0, 1, 2]` — the file's first three bands, in file order;
    /// * `&[2, 1, 0]` — the same bands as BGR;
    /// * `&[0, 0, 0]` — one grey band fanned out to three output slots.
    ///
    /// # Why this exists next to [`Self::read_band_into_typed`]
    ///
    /// In a chunky file — `PlanarConfiguration = 1`, which is what almost every
    /// RGB, RGBA or multispectral GeoTIFF is — one tile physically contains all
    /// of a pixel's bands. Reading `n` bands with `n` calls to
    /// [`Self::read_band_into_typed`] therefore decompresses every tile `n`
    /// times and discards `n − 1` bands each pass. This entry point decompresses
    /// each tile once and pulls every requested band out of it before moving on,
    /// so the block decode — which dominates the read — is paid once.
    ///
    /// For a multi-band **planar** file (`PlanarConfiguration = 2`) a block
    /// holds exactly one band, so there is no shared decode to save and this is
    /// simply one single-band read per distinct band woven into `dst`; a band
    /// repeated in `bands` is copied, not decoded twice.
    ///
    /// Conversion to `T` follows [`convert_raw_into`] exactly as
    /// [`Self::read_band_into_typed`] does, and the samples it converts have
    /// already been normalised to the **host's** byte order (crate-level *Byte
    /// order of decoded samples*).
    ///
    /// Nothing is allocated per block: the read takes a fixed handful of scratch
    /// buffers, per rayon worker under the `parallel` feature, which also
    /// spreads the block decode across workers without changing a single output
    /// element.
    ///
    /// # Errors
    /// Returns an error if `bands` is empty or names a band out of range, if
    /// `dst.len()` is not exactly `pixels × bands.len()`, if the file's sample
    /// type is not a recognised [`RasterDataType`], if `level` names no
    /// overview, or if a block cannot be read or decoded.
    ///
    /// # Examples
    /// ```ignore
    /// // BGR out of an RGB file, one decode per tile.
    /// let mut bgr = vec![0u8; reader.band_pixel_count(0)? * 3];
    /// reader.read_bands_into_typed(0, &[2, 1, 0], &mut bgr)?;
    /// ```
    pub fn read_bands_into_typed<T: RasterElement>(
        &self,
        level: usize,
        bands: &[usize],
        dst: &mut [T],
    ) -> Result<()> {
        const OP: &str = "read_bands_into_typed";
        let base = ReadPlan::full_band(&self.cog_reader, level, 0)?;
        let plan = MultiPlan::new(base, bands, OP)?;
        plan.check_len(dst.len(), OP)?;
        read_bands(&self.cog_reader, &plan, dst)
    }

    /// Reads a rectangular window of several bands into one pixel-interleaved
    /// buffer, decoding each block exactly once.
    ///
    /// The windowed counterpart of [`Self::read_bands_into_typed`]: the window
    /// origin is `(x, y)` in the level's pixel grid and `dst` must be exactly
    /// `width × height × bands.len()` elements long. Band selection, interleave
    /// order, conversion and byte order are identical.
    ///
    /// # Errors
    /// Returns an error if `bands` is empty or names a band out of range, if the
    /// window is zero-sized or extends past the raster extent, if `dst.len()` is
    /// wrong, if the file's sample type is not a recognised [`RasterDataType`],
    /// if `level` names no overview, or if a block cannot be read or decoded.
    #[allow(clippy::too_many_arguments)]
    pub fn read_window_bands_into_typed<T: RasterElement>(
        &self,
        level: usize,
        bands: &[usize],
        x: u64,
        y: u64,
        width: u64,
        height: u64,
        dst: &mut [T],
    ) -> Result<()> {
        const OP: &str = "read_window_bands_into_typed";
        let base = ReadPlan::window(&self.cog_reader, level, 0, x, y, width, height)?;
        let plan = MultiPlan::new(base, bands, OP)?;
        plan.check_len(dst.len(), OP)?;
        read_bands(&self.cog_reader, &plan, dst)
    }
}
