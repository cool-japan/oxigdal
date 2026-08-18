//! Cloud Optimized GeoTIFF (COG) support
//!
//! This module provides functionality specific to Cloud Optimized GeoTIFF files,
//! including tile-based access, overview navigation, optimization tools, and validation.
//!
//! # Phase 2: Advanced COG Tools
//!
//! This module includes advanced COG creation, optimization, and analysis tools:
//!
//! - **Compression Selection**: Analyzes data characteristics and recommends optimal compression
//! - **Overview Optimization**: Determines optimal overview levels and resampling methods
//! - **COG Optimization**: Comprehensive analysis for tile size, compression, and overviews
//! - **Metadata Optimization**: Minimizes metadata size while preserving essential information
//! - **Validation**: Enhanced COG compliance checking with detailed reports
//! - **Conversion**: Universal converter with auto-optimization
//! - **Tools**: High-level convenience functions

use std::sync::{Mutex, TryLockError};

use oxigeo_core::error::{FormatError, OxiGeoError, Result};
use oxigeo_core::io::{ByteRange, DataSource};

use crate::compression;
use crate::geokeys::GeoKeyDirectory;
use crate::tiff::{Compression, ImageInfo, PlanarConfiguration, TiffFile, TiffTag};

mod block_index;

// Phase 2: Advanced COG tools and optimization modules
pub mod compression_selector;
pub mod converter;
pub mod ghost_area;
pub mod metadata_optimizer;
pub mod optimizer;
pub mod overview_optimizer;
pub mod tools;
pub mod validator;

// Re-export commonly used types from Phase 2
pub use compression_selector::{
    CompressionAnalysis, CompressionPreferences, DataCharacteristics, analyze_for_compression,
};
pub use converter::{
    BatchConversionConfig, BatchConversionResult, CogConverter, ConversionConfig,
    ConversionProgress, ConversionResult, ConversionStep, convert_batch,
};
pub use ghost_area::{GhostArea, parse_ghost_area};
pub use metadata_optimizer::{
    GeoKeyOptimization, MetadataOptimization, PreservationLevel, analyze_metadata,
    compress_ascii_fields, find_redundant_tags, optimize_geokeys,
};
pub use optimizer::{
    AccessPattern, CloudCostEstimate, CogOptimization, OptimizationComparison, OptimizationGoal,
    SpatialAccessPattern, analyze_for_cog, compare_optimizations, estimate_cloud_cost,
};
pub use overview_optimizer::{
    OverviewPreferences, OverviewStrategy, ProgressiveOverviewConfig, calculate_optimal_batch_size,
    optimize_overviews, optimize_progressive_order, validate_overview_config,
};
pub use tools::{
    CogComparison, CogInfo, analyze_file_for_cog, compare_cogs, create_cog, create_optimized_cog,
    estimate_storage_cost, get_cog_info, is_valid_cog, optimize_existing_cog, validate_cog_file,
};
pub use validator::{
    DetailedCogValidation, PerformanceMetrics, ValidationCategory, ValidationMessage,
    ValidationSeverity, validate_cog_detailed,
};

/// COG validation result
#[derive(Debug, Clone)]
pub struct CogValidation {
    /// Whether the file is a valid COG
    pub is_valid: bool,
    /// Validation messages (warnings and errors)
    pub messages: Vec<String>,
    /// Whether the file has internal overviews
    pub has_overviews: bool,
    /// Whether tiles are properly ordered (for streaming)
    pub tiles_ordered: bool,
}

/// Validates that a TIFF file is COG-compliant
pub fn validate_cog<S: DataSource>(tiff: &TiffFile, source: &S) -> CogValidation {
    let mut messages = Vec::new();
    let mut is_valid = true;

    // Check 1: Must be tiled
    if let Some(ifd) = tiff.ifds.first() {
        let has_tiles = ifd.get_entry(TiffTag::TileWidth).is_some()
            && ifd.get_entry(TiffTag::TileLength).is_some();

        if !has_tiles {
            messages.push("Primary image must be tiled".to_string());
            is_valid = false;
        }

        // Check tile size is power of 2 and reasonable
        if let (Some(tw_entry), Some(th_entry)) = (
            ifd.get_entry(TiffTag::TileWidth),
            ifd.get_entry(TiffTag::TileLength),
        ) && let (Ok(tw), Ok(th)) = (
            tw_entry.get_u64_from_source(source, tiff.byte_order(), tiff.header.variant),
            th_entry.get_u64_from_source(source, tiff.byte_order(), tiff.header.variant),
        ) {
            if !tw.is_power_of_two() {
                messages.push(format!("Tile width {} is not a power of 2", tw));
            }
            if !th.is_power_of_two() {
                messages.push(format!("Tile height {} is not a power of 2", th));
            }
            if tw != th {
                messages.push(format!("Non-square tiles: {}x{}", tw, th));
            }
        }
    }

    // Check 2: Has overviews (recommended but not required)
    let has_overviews = tiff.ifds.len() > 1;
    if !has_overviews {
        messages.push("No internal overviews found (recommended for COG)".to_string());
    }

    // Check 3: IFDs should be ordered by size (largest first)
    let sizes: Vec<u64> = tiff
        .ifds
        .iter()
        .filter_map(|ifd| {
            let w = ifd
                .get_entry(TiffTag::ImageWidth)?
                .get_u64_from_source(source, tiff.byte_order(), tiff.header.variant)
                .ok()?;
            let h = ifd
                .get_entry(TiffTag::ImageLength)?
                .get_u64_from_source(source, tiff.byte_order(), tiff.header.variant)
                .ok()?;
            Some(w * h)
        })
        .collect();

    for i in 1..sizes.len() {
        if sizes[i] > sizes[i - 1] {
            messages.push("IFDs not ordered by decreasing size".to_string());
            break;
        }
    }

    // Check 4: Tile/strip data must come after the IFD chain and the primary
    // image's tiles must be stored in ascending order (streaming layout).
    let tiles_ordered = check_tiles_ordered(tiff, source);

    CogValidation {
        is_valid,
        messages,
        has_overviews,
        tiles_ordered,
    }
}

/// Pure predicate deciding whether image-data offsets describe a
/// streaming-friendly (COG) layout.
///
/// Two conditions must hold:
/// 1. every tile/strip offset lies strictly beyond the IFD chain
///    (`max_ifd_offset`), so all pixel data is stored after the directories; and
/// 2. the primary image's tile/strip offsets are non-decreasing, i.e. stored in
///    on-disk scan order.
///
/// An empty offset set is treated as "ordered" — there is nothing to contradict
/// the layout (the caller reports missing offsets separately).
fn tiles_are_ordered(all_offsets: &[u64], primary_offsets: &[u64], max_ifd_offset: u64) -> bool {
    if all_offsets.is_empty() {
        return true;
    }
    if all_offsets.iter().any(|&off| off <= max_ifd_offset) {
        return false;
    }
    primary_offsets.windows(2).all(|w| w[0] <= w[1])
}

/// Collects tile/strip offsets from every IFD and applies [`tiles_are_ordered`].
fn check_tiles_ordered<S: DataSource>(tiff: &TiffFile, source: &S) -> bool {
    let byte_order = tiff.byte_order();
    let variant = tiff.header.variant;

    // Highest byte offset occupied by an IFD in the chain that we can observe.
    let mut max_ifd_offset = tiff.header.first_ifd_offset;
    for ifd in &tiff.ifds {
        max_ifd_offset = max_ifd_offset.max(ifd.next_ifd_offset);
    }

    let mut all_offsets: Vec<u64> = Vec::new();
    let mut primary_offsets: Vec<u64> = Vec::new();

    for (idx, ifd) in tiff.ifds.iter().enumerate() {
        let Some(entry) = ifd
            .get_entry(TiffTag::TileOffsets)
            .or_else(|| ifd.get_entry(TiffTag::StripOffsets))
        else {
            continue;
        };
        let Ok(offsets) = entry.get_u64_vec(source, byte_order, variant) else {
            continue;
        };
        if idx == 0 {
            primary_offsets.clone_from(&offsets);
        }
        all_offsets.extend(offsets);
    }

    tiles_are_ordered(&all_offsets, &primary_offsets, max_ifd_offset)
}

/// A Cloud Optimized GeoTIFF reader
#[derive(Debug)]
pub struct CogReader<S: DataSource> {
    source: S,
    tiff: TiffFile,
    primary_info: ImageInfo,
    overview_infos: Vec<ImageInfo>,
    geo_keys: Option<GeoKeyDirectory>,
    /// Index into [`TiffFile::ifds`] of each public level, `level_ifds[0]` being
    /// the full-resolution image.
    ///
    /// A COG's level indices are *not* IFD indices. Two kinds of IFD share the
    /// chain with the overviews but are not resolutions:
    ///
    /// * **GDAL internal masks** ([`crate::tiff::is_mask_ifd`]) — single-bit
    ///   alpha planes for the image that precedes them. Counting one as a level
    ///   inflates [`Self::overview_count`] and shifts every later level onto the
    ///   wrong resolution.
    /// * **IFDs whose [`ImageInfo`] will not parse** — skipped since the reader
    ///   was written, but the block-index cache and the `tile_byte_range`
    ///   fallback both used to index the raw chain by level regardless, so a
    ///   skipped IFD silently desynchronised the tile offsets from the geometry.
    ///
    /// Every level → IFD resolution goes through this map, so those two views can
    /// no longer disagree. The raw chain stays reachable through [`Self::tiff`]
    /// and [`Self::ifd_count`] for consumers that want the masks.
    level_ifds: Vec<usize>,
    /// Pre-parsed tile/strip offsets and byte counts, one entry per level
    /// (index 0 = full resolution, index `n` = overview `n`).
    ///
    /// `None` for a level whose index could not be pre-parsed (missing tags,
    /// short/malformed arrays, implausible declared counts); such a level falls
    /// back to the original per-lookup parse, errors included. See
    /// [`block_index`] for why this cache exists.
    block_indices: Vec<Option<block_index::BlockIndex>>,
    /// Scratch buffer holding one block's raw (compressed) bytes, reused across
    /// calls so a whole-band read allocates nothing per block
    /// (cool-japan/oxigeo#14).
    ///
    /// It is only ever a staging area — every use overwrites the prefix it reads
    /// and then looks at exactly that prefix — so it carries no invariant and a
    /// poisoned lock is safe to recover from. It is taken with `try_lock`, so
    /// threads reading tiles concurrently never serialise on it: whoever finds it
    /// busy falls back to a private allocation, which is what the pre-fix code
    /// did on *every* call.
    block_scratch: Mutex<Vec<u8>>,
}

/// Geometry of one decoded tile/strip, shared by the tile read paths.
#[derive(Debug, Clone, Copy)]
struct TileGeometry {
    /// Width in pixels of the decoded block (tile width, or image width for strips).
    tile_width: u32,
    /// Height in pixels of the decoded block (tile height, or this strip's row count).
    tile_height: u32,
    /// Bytes per sample (0 for sub-byte `BitsPerSample`, matching the reader's
    /// long-standing behaviour).
    bytes_per_sample: usize,
    /// Samples this block stores per pixel: `SamplesPerPixel` for a chunky
    /// (interleaved) file, **1** for a planar one, where each block holds a
    /// single band.
    ///
    /// This is both the block's sample density and the predictor's stride — TIFF
    /// 6.0 §14 and TN3 define both predictors over the samples of the block being
    /// decoded, not over the image's nominal `SamplesPerPixel`.
    block_samples_per_pixel: usize,
    /// Expected decoded size in bytes.
    decoded_size: usize,
}

impl<S: DataSource> CogReader<S> {
    /// Opens a COG file
    pub fn open(source: S) -> Result<Self> {
        let tiff = TiffFile::parse(&source)?;

        if tiff.ifds.is_empty() {
            return Err(OxiGeoError::Format(FormatError::InvalidHeader {
                message: "No IFDs found in TIFF".to_string(),
            }));
        }

        let byte_order = tiff.byte_order();
        let variant = tiff.header.variant;

        // Parse primary image info
        let primary_info = ImageInfo::from_ifd(&tiff.ifds[0], &source, byte_order, variant)?;

        // Parse overview infos, recording which IFD each level came from.
        //
        // Two IFDs in the chain are not overviews and must not become levels:
        // a GDAL internal mask (see `crate::tiff::is_mask_ifd`), which is an
        // alpha plane for the image before it, and one whose `ImageInfo` will
        // not parse (best-effort skip, as before). The primary IFD is level 0
        // whatever its own tags claim — a standalone `.msk` opened directly is
        // still that file's image.
        let mut overview_infos = Vec::new();
        let mut level_ifds = vec![0usize];
        for (index, ifd) in tiff.ifds.iter().enumerate().skip(1) {
            if crate::tiff::is_mask_ifd(ifd, byte_order) {
                continue;
            }
            if let Ok(info) = ImageInfo::from_ifd(ifd, &source, byte_order, variant) {
                overview_infos.push(info);
                level_ifds.push(index);
            }
        }

        // Parse GeoKeys (best-effort: continue without geo keys if parsing fails)
        let geo_keys = GeoKeyDirectory::from_ifd(&tiff.ifds[0], &source, byte_order, variant)
            .ok()
            .flatten();

        // Parse each level's tile/strip offset + byte-count arrays exactly once,
        // so `tile_byte_range` is an O(1) index lookup instead of a full re-read
        // and re-parse per block (cool-japan/oxigeo#14). Best-effort: a level that
        // cannot be pre-parsed keeps the original on-demand behaviour.
        let mut block_indices = Vec::with_capacity(overview_infos.len() + 1);
        for level in 0..=overview_infos.len() {
            let info = if level == 0 {
                &primary_info
            } else {
                &overview_infos[level - 1]
            };
            block_indices.push(
                level_ifds
                    .get(level)
                    .and_then(|&index| tiff.ifds.get(index))
                    .and_then(|ifd| {
                        block_index::BlockIndex::try_parse(
                            ifd,
                            &source,
                            byte_order,
                            variant,
                            info.is_tiled(),
                        )
                    }),
            );
        }

        Ok(Self {
            source,
            tiff,
            primary_info,
            overview_infos,
            geo_keys,
            level_ifds,
            block_indices,
            block_scratch: Mutex::new(Vec::new()),
        })
    }

    /// Returns the image width
    #[must_use]
    pub fn width(&self) -> u64 {
        self.primary_info.width
    }

    /// Returns the image height
    #[must_use]
    pub fn height(&self) -> u64 {
        self.primary_info.height
    }

    /// Returns the tile dimensions
    #[must_use]
    pub fn tile_size(&self) -> Option<(u32, u32)> {
        match (self.primary_info.tile_width, self.primary_info.tile_height) {
            (Some(w), Some(h)) => Some((w, h)),
            _ => None,
        }
    }

    /// Returns the number of overview levels
    ///
    /// Levels are *resolutions*: IFDs that are GDAL internal masks
    /// ([`crate::tiff::is_mask_ifd`]) or that fail to parse are not counted, so
    /// this is generally smaller than `tiff().image_count() - 1`. Use
    /// [`Self::ifd_count`] for the raw chain length.
    #[must_use]
    pub fn overview_count(&self) -> usize {
        self.overview_infos.len()
    }

    /// Returns the number of IFDs in the file's directory chain, *including*
    /// GDAL internal masks and any IFD this reader could not parse.
    ///
    /// This is the raw structural count — the counterpart of
    /// [`Self::overview_count`], which counts resolutions only. It is the same
    /// value as `self.tiff().image_count()`, exposed here so a caller reasoning
    /// about levels never has to reach for the [`TiffFile`] to learn how many
    /// non-level IFDs the file carries.
    #[must_use]
    pub fn ifd_count(&self) -> usize {
        self.tiff.ifds.len()
    }

    /// Returns the IFD backing `level` (0 = full resolution), or `None` if
    /// `level` names no level.
    ///
    /// The returned IFD is never a mask and never one whose [`ImageInfo`] failed
    /// to parse: this is the level → IFD map every read path uses, so anything
    /// derived from it (dimensions, tile geometry, tile offsets) describes the
    /// same image the tile reads at that level return.
    #[must_use]
    pub fn level_ifd(&self, level: usize) -> Option<&crate::tiff::Ifd> {
        self.level_ifds
            .get(level)
            .and_then(|&index| self.tiff.ifds.get(index))
    }

    /// Returns the index in the raw IFD chain of `level`'s directory, or `None`
    /// if `level` names no level.
    ///
    /// `level_ifd_index(0)` is always `Some(0)`. The mapping is strictly
    /// increasing and skips masks, so a gap between consecutive results is the
    /// count of non-level IFDs between the two resolutions.
    #[must_use]
    pub fn level_ifd_index(&self, level: usize) -> Option<usize> {
        self.level_ifds.get(level).copied()
    }

    /// Returns the primary image info
    #[must_use]
    pub fn primary_info(&self) -> &ImageInfo {
        &self.primary_info
    }

    /// Returns the GeoKey directory, if present
    #[must_use]
    pub fn geo_keys(&self) -> Option<&GeoKeyDirectory> {
        self.geo_keys.as_ref()
    }

    /// Returns the EPSG code, if available
    #[must_use]
    pub fn epsg_code(&self) -> Option<u32> {
        self.geo_keys.as_ref().and_then(|gk| gk.epsg_code())
    }

    /// Extracts the GeoTransform from GeoTIFF tags
    pub fn geo_transform(&self) -> Result<Option<oxigeo_core::types::GeoTransform>> {
        use crate::geokeys;

        geokeys::extract_geo_transform(
            &self.tiff.ifds[0],
            &self.source,
            self.tiff.byte_order(),
            self.tiff.header.variant,
        )
    }

    /// Extracts the NoData value from GDAL_NODATA tag
    pub fn nodata(&self) -> Result<oxigeo_core::types::NoDataValue> {
        use oxigeo_core::types::NoDataValue;

        if let Some(entry) = self.tiff.ifds[0].get_entry(TiffTag::GdalNodata) {
            let nodata_str = entry.get_ascii(&self.source, self.tiff.header.variant)?;

            // Try parsing as integer first (more specific)
            if let Ok(val) = nodata_str.parse::<i64>() {
                return Ok(NoDataValue::from_integer(val));
            }

            // Try parsing as float (more general)
            if let Ok(val) = nodata_str.parse::<f64>() {
                return Ok(NoDataValue::from_float(val));
            }
        }

        Ok(NoDataValue::None)
    }

    /// Returns the internal TIFF file
    #[must_use]
    pub fn tiff(&self) -> &TiffFile {
        &self.tiff
    }

    /// Returns the number of tiles in X and Y
    #[must_use]
    pub fn tile_count(&self) -> (u32, u32) {
        (
            self.primary_info.tiles_across(),
            self.primary_info.tiles_down(),
        )
    }

    /// Returns the [`ImageInfo`] of a level (0 = full resolution).
    ///
    /// # Errors
    /// Returns [`OxiGeoError::OutOfBounds`] if `level` names no overview.
    fn info_for_level(&self, level: usize) -> Result<&ImageInfo> {
        if level == 0 {
            Ok(&self.primary_info)
        } else {
            self.overview_infos
                .get(level - 1)
                .ok_or_else(|| OxiGeoError::OutOfBounds {
                    message: format!("Overview level {} out of bounds", level),
                })
        }
    }

    /// Computes the flat block index of tile/strip `(tile_x, tile_y)`.
    ///
    /// Uses 64-bit arithmetic so a hostile `tiles_across` cannot overflow the
    /// multiplication (which would panic in debug builds and silently alias a
    /// different tile in release builds); an out-of-range result still surfaces as
    /// the usual out-of-bounds error at lookup time.
    fn block_index_of(info: &ImageInfo, tile_x: u32, tile_y: u32) -> u64 {
        u64::from(tile_y) * u64::from(info.tiles_across()) + u64::from(tile_x)
    }

    /// Gets the byte range for a specific tile or strip
    ///
    /// This is an O(1) lookup into the per-level index parsed once at
    /// [`Self::open`]; only a level whose index could not be pre-parsed falls back
    /// to re-reading and re-parsing the offset arrays (cool-japan/oxigeo#14).
    pub fn tile_byte_range(&self, level: usize, tile_x: u32, tile_y: u32) -> Result<ByteRange> {
        let info = self.info_for_level(level)?;
        let out_of_bounds = || OxiGeoError::OutOfBounds {
            message: format!("Tile/strip ({}, {}) out of bounds", tile_x, tile_y),
        };
        let block = Self::block_index_of(info, tile_x, tile_y);
        let block = usize::try_from(block).map_err(|_| out_of_bounds())?;

        // Fast path: the pre-parsed index.
        if let Some(Some(index)) = self.block_indices.get(level) {
            return index.byte_range(block).ok_or_else(out_of_bounds);
        }

        // Fallback for levels that could not be pre-parsed (missing tags, short or
        // malformed arrays, implausible declared counts): re-read on demand so the
        // original error surfaces verbatim. Resolved through the level → IFD map,
        // not by indexing the chain directly: a mask or an unparseable IFD earlier
        // in the chain would otherwise make this read a different image's offsets
        // than `info_for_level` above described.
        let ifd = self
            .level_ifd(level)
            .ok_or_else(|| OxiGeoError::OutOfBounds {
                message: format!("Overview level {} out of bounds", level),
            })?;
        let index = block_index::BlockIndex::parse(
            ifd,
            &self.source,
            self.tiff.byte_order(),
            self.tiff.header.variant,
            info.is_tiled(),
        )?;
        index.byte_range(block).ok_or_else(out_of_bounds)
    }

    /// Reads a tile's raw (compressed) data
    ///
    /// The bytes are the block exactly as stored: still compressed, still
    /// predicted, still in the file's byte order. It is deliberately **not**
    /// covered by the host-byte-order contract the decode paths honour — there
    /// are no decoded samples here to normalise. Callers that decode these bytes
    /// themselves own both the predictor reversal and the byte-order conversion,
    /// in that order (see the crate-level *Byte order of decoded samples*
    /// section).
    ///
    /// Returning an owned `Vec` inherently costs one allocation per block; the
    /// decode paths ([`Self::read_tile`], [`Self::read_tile_into`]) read the block
    /// through [`DataSource::read_range_into`] (or borrow it outright) instead and
    /// allocate nothing.
    pub fn read_tile_raw(&self, level: usize, tile_x: u32, tile_y: u32) -> Result<Vec<u8>> {
        let range = self.tile_byte_range(level, tile_x, tile_y)?;
        self.source.read_range(range)
    }

    /// Runs `f` over the raw (compressed) bytes of `range` without allocating a
    /// buffer per block.
    ///
    /// Three tiers, best first:
    ///
    /// 1. **Borrowed** — a source that can hand out a view of `range`
    ///    ([`DataSource::range_slice`]: memory-mapped or fully in-memory) needs no
    ///    buffer at all, so the block is neither allocated nor copied.
    /// 2. **Reused scratch** — otherwise the bytes are read straight into the
    ///    reader's own buffer with [`DataSource::read_range_into`]. The buffer
    ///    only ever grows, so after the first (largest) block a whole-band walk
    ///    allocates nothing.
    /// 3. **Private allocation** — if another thread holds the scratch, this call
    ///    allocates its own, exactly as every call did before #14.
    ///
    /// The bytes handed to `f` are byte-for-byte what `source.read_range(range)`
    /// would have returned, including the short slice a clamping source produces,
    /// and every error path is that of `read_range`.
    fn with_block_bytes<R>(
        &self,
        range: ByteRange,
        f: impl FnOnce(&[u8]) -> Result<R>,
    ) -> Result<R> {
        // 1. Zero-copy.
        if let Some(bytes) = self.source.range_slice(range) {
            return f(bytes);
        }

        let len = usize::try_from(range.len()).map_err(|_| OxiGeoError::OutOfBounds {
            message: format!(
                "block byte range {}..{} is wider than usize",
                range.start, range.end
            ),
        })?;

        // 2. Reused scratch. A poisoned lock means some other call panicked while
        //    staging bytes here; the buffer holds no invariant, so recovering it
        //    is sound and keeps the fast path alive.
        let scratch = match self.block_scratch.try_lock() {
            Ok(guard) => Some(guard),
            Err(TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
            Err(TryLockError::WouldBlock) => None,
        };

        match scratch {
            Some(mut scratch) => {
                if scratch.len() < len {
                    scratch.resize(len, 0);
                }
                let read = self.source.read_range_into(range, &mut scratch[..len])?;
                f(&scratch[..read])
            }
            // 3. Contended: fall back to the pre-#14 behaviour.
            None => {
                let owned = self.source.read_range(range)?;
                f(&owned)
            }
        }
    }

    /// Computes the decoded geometry of tile/strip row `tile_y` at `level`.
    ///
    /// # Planar files
    ///
    /// `PlanarConfiguration = 2` stores `SamplesPerPixel × blocks-per-plane`
    /// blocks in plane-major order and each block holds exactly **one** band, so
    /// `tile_y` runs over every plane's block rows (`band × blocks_down + row`)
    /// — the addressing `band_read::decode_block` uses. Two consequences, both of
    /// which this function used to get wrong by taking `SamplesPerPixel` at face
    /// value:
    ///
    /// * a block is `block_w · block_h · bytes_per_sample` bytes, not that times
    ///   `SamplesPerPixel`; and
    /// * the predictor's stride is one sample, not `SamplesPerPixel` — a stride
    ///   of 3 on a single-band plane subtracts the wrong neighbour from every
    ///   sample *and* mis-computes the scanline length, silently.
    ///
    /// The planar decision is made once per block here and costs nothing on the
    /// chunky path.
    fn tile_geometry(&self, level: usize, tile_y: u32) -> Result<TileGeometry> {
        let info = self.info_for_level(level)?;
        let planar = matches!(info.planar_config, PlanarConfiguration::Planar);

        let (tile_width, tile_height) = if info.is_tiled() {
            // Tiled layout
            (
                info.tile_width.unwrap_or(info.width as u32),
                info.tile_height.unwrap_or(info.height as u32),
            )
        } else {
            // Striped layout: width is full image width, height is rows_per_strip.
            // Which strip *of its plane* this is decides whether it is the short
            // last one; for a chunky file that is `tile_y` itself.
            let strips_down = info.tiles_down();
            let row_in_plane = if planar && strips_down > 0 {
                tile_y % strips_down
            } else {
                tile_y
            };
            let strip_height = info.rows_per_strip.unwrap_or(info.height as u32);
            let actual_height = if row_in_plane == strips_down.saturating_sub(1) {
                // Last strip might be shorter
                let remaining = (info.height as u32).saturating_sub(row_in_plane * strip_height);
                remaining.min(strip_height)
            } else {
                strip_height
            };
            (info.width as u32, actual_height)
        };

        let bytes_per_sample = info
            .bits_per_sample
            .first()
            .map_or(1, |&b| (b / 8) as usize);
        let block_samples_per_pixel = if planar {
            1
        } else {
            info.samples_per_pixel as usize
        };

        let decoded_size = (tile_width as usize)
            .checked_mul(tile_height as usize)
            .and_then(|v| v.checked_mul(bytes_per_sample))
            .and_then(|v| v.checked_mul(block_samples_per_pixel))
            .ok_or_else(|| {
                OxiGeoError::Format(FormatError::InvalidHeader {
                    message: format!(
                        "tile dimensions overflow usize: {tile_width}x{tile_height} x \
                         {bytes_per_sample} bytes x {block_samples_per_pixel} samples"
                    ),
                })
            })?;

        Ok(TileGeometry {
            tile_width,
            tile_height,
            bytes_per_sample,
            block_samples_per_pixel,
            decoded_size,
        })
    }

    /// Returns the decoded (uncompressed, post-predictor) size in bytes of one
    /// tile or strip.
    ///
    /// This is the exact buffer length [`Self::read_tile_into`] requires, so a
    /// caller reusing one scratch buffer across a whole band can size it up front.
    ///
    /// # Planar files
    ///
    /// On a `PlanarConfiguration = 2` raster a block holds **one** band, so this
    /// is `block_w · block_h · bytes_per_sample` — it is *not* multiplied by
    /// `SamplesPerPixel`, which is what a chunky (interleaved) block stores. It
    /// used to be multiplied regardless, making every planar block report (and
    /// `read_tile_into` demand) a buffer `SamplesPerPixel ×` too large.
    ///
    /// `tile_y` indexes the plane-major block grid the file actually stores:
    /// `band × tiles_down + row`, matching how the block offsets are laid out
    /// (TIFF 6.0 §on PlanarConfiguration) and how `band_read` addresses them.
    ///
    /// # Errors
    /// Returns an error if `level` names no overview, or if the tile dimensions
    /// declared by the IFD overflow `usize`.
    pub fn tile_decoded_size(&self, level: usize, tile_y: u32) -> Result<usize> {
        Ok(self.tile_geometry(level, tile_y)?.decoded_size)
    }

    /// Returns the decoded pixel dimensions `(width, height)` of one tile or
    /// strip at `level`.
    ///
    /// This is the pixel geometry of exactly the block [`Self::read_tile`]
    /// returns — it comes from the same computation, so a caller sizing an image
    /// buffer from it can never disagree with the bytes it gets:
    ///
    /// * on a **tiled** level it is that level's own `TileWidth`/`TileLength`,
    ///   which an overview may declare differently from the full-resolution
    ///   image (a level-0 tile size is not a property of the file);
    /// * on a **striped** level it is `ImageWidth × RowsPerStrip`, narrowed to
    ///   the real row count for the short final strip of the plane;
    /// * on a **planar** level `tile_y` indexes the plane-major block grid
    ///   (`band × tiles_down + row`), as everywhere else in this API.
    ///
    /// Multiplying the two by `bytes_per_sample` and the block's samples per
    /// pixel gives [`Self::tile_decoded_size`].
    ///
    /// # Errors
    /// Returns an error if `level` names no overview, or if the tile dimensions
    /// declared by the IFD overflow `usize`.
    pub fn tile_pixel_size(&self, level: usize, tile_y: u32) -> Result<(u32, u32)> {
        let geometry = self.tile_geometry(level, tile_y)?;
        Ok((geometry.tile_width, geometry.tile_height))
    }

    /// Reads and decompresses a tile or strip
    ///
    /// Returns a buffer of whatever length the codec produced, which for a
    /// well-formed file equals [`Self::tile_decoded_size`]. Prefer
    /// [`Self::read_tile_into`] when reading many tiles: it decodes into a
    /// caller-owned buffer and so allocates nothing per tile.
    ///
    /// # Byte order
    ///
    /// The samples are returned in the **host's** byte order, not the file's, so
    /// a caller never has to consult the file's `II`/`MM` header. The block is
    /// normalised exactly once, and strictly after the predictor is reversed —
    /// the predictor is defined on file-order data. See the crate-level *Byte
    /// order of decoded samples* section for the full contract and for the
    /// widths and codecs it deliberately leaves alone.
    ///
    /// # Planar files
    ///
    /// A `PlanarConfiguration = 2` block holds one band; `tile_y` indexes the
    /// plane-major block grid (`band × tiles_down + row`) and the returned buffer
    /// is that single band's block. See [`Self::tile_decoded_size`].
    ///
    /// # Errors
    ///
    /// Besides the usual out-of-range and codec errors, a block whose
    /// codec/predictor combination has no defined reversal — today only LERC with
    /// a `Predictor` tag — is refused outright rather than decoded to wrong
    /// pixels; see the crate-level *Codec/predictor combinations that are
    /// refused* section.
    ///
    /// This is deliberately *not* implemented on top of `read_tile_into`: that
    /// would impose the exact-size contract on files where the two legitimately
    /// differ — most notably sub-byte `BitsPerSample` (bilevel/4-bit imagery),
    /// where `bytes_per_sample` rounds to 0 and the expected size is 0 while the
    /// block still decodes to real bytes. Callers of `read_tile` keep getting the
    /// codec's own output.
    pub fn read_tile(&self, level: usize, tile_x: u32, tile_y: u32) -> Result<Vec<u8>> {
        let range = self.tile_byte_range(level, tile_x, tile_y)?;
        // `tile_byte_range` resolved the level, so this cannot fail here; hoisting
        // it above the read therefore cannot change which error a caller sees.
        let info = self.info_for_level(level)?;

        // Fast path: `decompress(.., Compression::None, ..)` is `data.to_vec()`,
        // so the block is read straight into the buffer that is about to be
        // returned — one allocation and one copy, where the old code did two of
        // each.
        if info.compression == Compression::None {
            let len = usize::try_from(range.len()).map_err(|_| OxiGeoError::OutOfBounds {
                message: format!(
                    "block byte range {}..{} is wider than usize",
                    range.start, range.end
                ),
            })?;
            let mut decompressed = vec![0u8; len];
            let read = self.source.read_range_into(range, &mut decompressed)?;
            decompressed.truncate(read);

            let geometry = self.tile_geometry(level, tile_y)?;
            compression::apply_predictor_reverse(
                &mut decompressed,
                info.predictor,
                geometry.bytes_per_sample,
                geometry.block_samples_per_pixel,
                geometry.tile_width as usize,
                self.tiff.byte_order(),
            )?;
            // Strictly after the predictor: it reads and writes file-order
            // samples, so the block only becomes a plain sample array here.
            crate::normalize_samples_to_native(
                &mut decompressed,
                geometry.bytes_per_sample,
                self.tiff.byte_order(),
                info.compression,
            );
            return Ok(decompressed);
        }

        // The closure body is the old code verbatim, only reading the compressed
        // bytes from a borrowed slice or the reader's scratch instead of a freshly
        // allocated `Vec`; it still runs *after* the block has been fetched, so
        // the order in which errors surface is unchanged.
        self.with_block_bytes(range, |compressed| {
            let geometry = self.tile_geometry(level, tile_y)?;
            // Before the codec runs: a combination whose predictor reversal is
            // undefined can never produce the right pixels, so decoding it would
            // only be wasted work ahead of a wrong answer.
            crate::reject_undefined_predictor(info.compression, info.predictor)?;

            let mut decompressed =
                compression::decompress(compressed, info.compression, geometry.decoded_size)?;

            // Apply predictor
            compression::apply_predictor_reverse(
                &mut decompressed,
                info.predictor,
                geometry.bytes_per_sample,
                geometry.block_samples_per_pixel,
                geometry.tile_width as usize,
                self.tiff.byte_order(),
            )?;
            // ... and only then normalise to host byte order (see above).
            crate::normalize_samples_to_native(
                &mut decompressed,
                geometry.bytes_per_sample,
                self.tiff.byte_order(),
                info.compression,
            );

            Ok(decompressed)
        })
    }

    /// Reads one tile/strip and decodes it directly into `dst`, applying the
    /// predictor in place. No intermediate allocation for the decoded pixels.
    ///
    /// `dst` must be exactly [`Self::tile_decoded_size`] bytes long, which lets a
    /// whole-band read allocate **one** scratch buffer and reuse it for every tile
    /// instead of allocating (and, for uncompressed data, needlessly copying) a
    /// fresh `Vec` per tile — see cool-japan/oxigeo#14.
    ///
    /// Semantics match [`read_tile`](Self::read_tile): the same compression
    /// dispatch, the same predictor application over exactly the bytes the codec
    /// produced, the same **host**-byte-order normalisation of those bytes
    /// afterwards (see the crate-level *Byte order of decoded samples* section),
    /// and the same errors. The one addition is that a codec producing
    /// *fewer* bytes than the tile's expected size (a truncated block) leaves the
    /// remainder of `dst` zero-filled, so a reused buffer can never leak the
    /// previous tile's pixels; that is byte-for-byte what a freshly allocated
    /// zeroed band buffer would have contained.
    ///
    /// Nothing is allocated per block: uncompressed blocks are read from the file
    /// straight into `dst` through [`DataSource::read_range_into`] (one copy, the
    /// theoretical minimum), and compressed ones stage through the reader's
    /// reusable scratch buffer or, for sources that can lend their bytes
    /// ([`DataSource::range_slice`]), a borrowed slice.
    ///
    /// # Errors
    /// Returns an error if `dst.len()` differs from the tile's decoded size, if
    /// the tile coordinates are out of range, if the codec/predictor fails, or if
    /// the block's codec/predictor combination has no defined reversal (LERC plus
    /// a `Predictor` tag), which is refused rather than mis-decoded.
    pub fn read_tile_into(
        &self,
        level: usize,
        tile_x: u32,
        tile_y: u32,
        dst: &mut [u8],
    ) -> Result<()> {
        let geometry = self.tile_geometry(level, tile_y)?;
        if dst.len() != geometry.decoded_size {
            return Err(OxiGeoError::invalid_parameter_builder(
                "dst",
                "Destination buffer length must equal the tile's decoded size",
            )
            .with_operation("read_tile_into")
            .with_parameter("dst_len", dst.len().to_string())
            .with_parameter("decoded_size", geometry.decoded_size.to_string())
            .with_parameter("level", level.to_string())
            .with_parameter("tile", format!("({tile_x}, {tile_y})"))
            .with_suggestion("Size the buffer with CogReader::tile_decoded_size")
            .build());
        }

        let range = self.tile_byte_range(level, tile_x, tile_y)?;
        // Hoisting `info_for_level` above the read cannot introduce a new error:
        // `tile_geometry` already called it and returned its error.
        let info = self.info_for_level(level)?;
        // Refuse a codec/predictor pair with no defined reversal before touching
        // the file, exactly as `read_tile` does.
        crate::reject_undefined_predictor(info.compression, info.predictor)?;

        // Fast path: an uncompressed block *is* the decoded block, so it is read
        // from the file directly into the caller's buffer — no intermediate
        // buffer, no second copy. The guard mirrors `decompress_into_partial`'s
        // own "payload larger than dst" check, so a block that does not fit takes
        // the general path below and reports that check's error verbatim.
        let raw_len = usize::try_from(range.len()).unwrap_or(usize::MAX);
        let written = if info.compression == Compression::None && raw_len <= dst.len() {
            self.source.read_range_into(range, &mut dst[..raw_len])?
        } else {
            self.with_block_bytes(range, |compressed| {
                compression::decompress_into_partial(compressed, info.compression, dst)
            })?
        };

        // Apply the predictor over exactly the bytes the codec produced, matching
        // `read_tile`, normalise those same bytes to host byte order, then zero
        // any tail a short block left behind.
        compression::apply_predictor_reverse(
            &mut dst[..written],
            info.predictor,
            geometry.bytes_per_sample,
            geometry.block_samples_per_pixel,
            geometry.tile_width as usize,
            self.tiff.byte_order(),
        )?;
        crate::normalize_samples_to_native(
            &mut dst[..written],
            geometry.bytes_per_sample,
            self.tiff.byte_order(),
            info.compression,
        );
        dst[written..].fill(0);

        Ok(())
    }

    /// Selects the best overview level for the given resolution
    pub fn select_overview(&self, target_width: u64, target_height: u64) -> usize {
        // Start with full resolution
        let mut best_level = 0;
        let mut best_width = self.primary_info.width;
        let mut best_height = self.primary_info.height;

        // Find the smallest overview that's still larger than the target
        for (i, info) in self.overview_infos.iter().enumerate() {
            if info.width >= target_width
                && info.height >= target_height
                && info.width < best_width
                && info.height < best_height
            {
                best_level = i + 1;
                best_width = info.width;
                best_height = info.height;
            }
        }

        best_level
    }
}

/// Overview level information
#[derive(Debug, Clone)]
pub struct OverviewInfo {
    /// Level index (0 = full resolution)
    pub level: usize,
    /// Width in pixels
    pub width: u64,
    /// Height in pixels
    pub height: u64,
    /// Scale factor from full resolution
    pub scale: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_overview() {
        // Test overview info structure creation
        let info = OverviewInfo {
            level: 1,
            width: 512,
            height: 512,
            scale: 2.0,
        };

        assert_eq!(info.level, 1);
        assert_eq!(info.width, 512);
        assert_eq!(info.height, 512);
        assert_eq!(info.scale, 2.0);
    }

    #[test]
    fn test_tiles_are_ordered_streaming_layout() {
        // IFDs occupy up to offset 200; all tile data lies beyond and the
        // primary tiles ascend => streaming-friendly.
        let primary = [1000u64, 2000, 3000];
        let all = [1000u64, 2000, 3000, 800_000];
        assert!(tiles_are_ordered(&all, &primary, 200));
    }

    #[test]
    fn test_tiles_are_ordered_data_before_ifds() {
        // A tile offset at 150 sits inside/before the IFD chain (max 200).
        let primary = [150u64, 2000];
        let all = [150u64, 2000];
        assert!(!tiles_are_ordered(&all, &primary, 200));
    }

    #[test]
    fn test_tiles_are_ordered_primary_out_of_order() {
        // All data is after the IFDs, but the primary tiles are not ascending.
        let primary = [3000u64, 1000, 2000];
        let all = [3000u64, 1000, 2000];
        assert!(!tiles_are_ordered(&all, &primary, 200));
    }

    #[test]
    fn test_tiles_are_ordered_empty_is_vacuously_true() {
        assert!(tiles_are_ordered(&[], &[], 200));
    }
}
