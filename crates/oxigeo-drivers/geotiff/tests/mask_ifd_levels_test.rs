//! GDAL internal-mask IFDs are not pyramid levels.
//!
//! `GDALDataset::CreateMaskBand` with `GDAL_TIFF_INTERNAL_MASK=YES` (and
//! `gdal_translate -co COPY_SRC_OVERVIEWS` on a masked source) writes each
//! transparency mask as an ordinary IFD in the *same* directory chain as the
//! reduced-resolution overviews. A reader that enumerates levels by walking the
//! chain therefore reports one extra "overview" per mask and shifts every level
//! index past it: `read_tile(2, …)` on the fixture below used to return the
//! **mask's** bytes, and `level_size(2)` the mask's dimensions, while the caller
//! believed it was reading the smallest overview.
//!
//! Every level → IFD resolution now goes through `CogReader`'s level map, so the
//! three surfaces that used to index the raw chain independently —
//! `CogReader::read_tile` (block index + byte-range fallback),
//! `band_read::LevelGeometry` and `GeoTiffReader::level_size` — agree on what
//! level *n* is. The raw chain stays visible through `TiffFile`/`ifd_count`.
//!
//! The fixtures are hand-built multi-IFD TIFFs: the crate's writer never emits a
//! mask IFD, so the layout cannot be produced any other way. Each IFD's blocks
//! are filled with a byte that identifies the IFD, so a tile read names the
//! image it actually came from.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use oxigeo_core::error::{OxiGeoError, Result};
use oxigeo_core::io::{ByteRange, DataSource};
use oxigeo_geotiff::{CogReader, GeoTiffReader};

// ---------------------------------------------------------------------------
// Data source
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct MemorySource(Vec<u8>);

impl DataSource for MemorySource {
    fn size(&self) -> Result<u64> {
        Ok(self.0.len() as u64)
    }

    fn read_range(&self, range: ByteRange) -> Result<Vec<u8>> {
        let start = range.start as usize;
        let end = (range.end as usize).min(self.0.len());
        if start > end {
            return Err(OxiGeoError::OutOfBounds {
                message: format!("invalid range {}..{}", range.start, range.end),
            });
        }
        Ok(self.0[start..end].to_vec())
    }
}

// ---------------------------------------------------------------------------
// Multi-IFD synthetic TIFF builder (uncompressed, 8-bit, single band, LE)
// ---------------------------------------------------------------------------

// Raw tag numbers: `NewSubfileType` is deliberately not part of the crate's
// `TiffTag` enum, so the builder speaks raw u16 throughout.
const TAG_NEW_SUBFILE_TYPE: u16 = 254;
const TAG_IMAGE_WIDTH: u16 = 256;
const TAG_IMAGE_LENGTH: u16 = 257;
const TAG_BITS_PER_SAMPLE: u16 = 258;
const TAG_COMPRESSION: u16 = 259;
const TAG_PHOTOMETRIC: u16 = 262;
const TAG_STRIP_OFFSETS: u16 = 273;
const TAG_SAMPLES_PER_PIXEL: u16 = 277;
const TAG_ROWS_PER_STRIP: u16 = 278;
const TAG_STRIP_BYTE_COUNTS: u16 = 279;
const TAG_PLANAR_CONFIGURATION: u16 = 284;
const TAG_TILE_WIDTH: u16 = 322;
const TAG_TILE_LENGTH: u16 = 323;
const TAG_TILE_OFFSETS: u16 = 324;
const TAG_TILE_BYTE_COUNTS: u16 = 325;
const TAG_SAMPLE_FORMAT: u16 = 339;

const SHORT: u16 = 3;
const LONG: u16 = 4;

/// One IFD of the fixture.
#[derive(Debug, Clone, Copy)]
struct IfdSpec {
    width: u32,
    height: u32,
    /// `Some((tile_w, tile_h))` for a tiled IFD, `None` for strips.
    tile: Option<(u32, u32)>,
    /// `RowsPerStrip`, used only when `tile` is `None`.
    rows_per_strip: u32,
    /// `NewSubfileType` (tag 254); `None` omits the tag entirely.
    new_subfile_type: Option<u32>,
    /// `PhotometricInterpretation` (tag 262).
    photometric: u16,
    /// Byte every pixel of this IFD's blocks is filled with, so a tile read
    /// identifies the IFD it came from.
    marker: u8,
}

impl IfdSpec {
    /// An ordinary image: full resolution when `new_subfile_type` is `None`, a
    /// reduced-resolution overview when it is `Some(1)`.
    fn image(width: u32, height: u32, tile: (u32, u32), subfile: Option<u32>, marker: u8) -> Self {
        Self {
            width,
            height,
            tile: Some(tile),
            rows_per_strip: 0,
            new_subfile_type: subfile,
            photometric: 1,
            marker,
        }
    }

    /// A striped image (no tile tags).
    fn striped(width: u32, height: u32, rows_per_strip: u32, marker: u8) -> Self {
        Self {
            width,
            height,
            tile: None,
            rows_per_strip,
            new_subfile_type: Some(1),
            photometric: 1,
            marker,
        }
    }

    /// A GDAL internal mask marked the way GDAL marks it: `NewSubfileType`
    /// bit 2 (plus bit 0 when the mask belongs to an overview).
    fn mask_by_subfile_type(width: u32, height: u32, tile: (u32, u32), marker: u8) -> Self {
        Self {
            new_subfile_type: Some(0x4),
            ..Self::image(width, height, tile, None, marker)
        }
    }

    /// A mask marked *only* by `PhotometricInterpretation == 4` — some writers
    /// set no `NewSubfileType` at all.
    fn mask_by_photometric(width: u32, height: u32, tile: (u32, u32), marker: u8) -> Self {
        Self {
            photometric: 4,
            ..Self::image(width, height, tile, None, marker)
        }
    }

    fn block_dims(&self) -> (u32, u32) {
        match self.tile {
            Some((tw, th)) => (tw, th),
            None => (self.width, self.rows_per_strip),
        }
    }

    fn blocks_across(&self) -> u32 {
        match self.tile {
            Some((tw, _)) => self.width.div_ceil(tw),
            None => 1,
        }
    }

    fn blocks_down(&self) -> u32 {
        match self.tile {
            Some((_, th)) => self.height.div_ceil(th),
            None => self.height.div_ceil(self.rows_per_strip),
        }
    }

    /// This IFD's blocks, in file order. Tiles are always full-size (padded);
    /// the final strip carries only the rows that exist.
    fn blocks(&self) -> Vec<Vec<u8>> {
        let (block_w, block_h) = self.block_dims();
        let mut blocks = Vec::new();
        for by in 0..self.blocks_down() {
            let rows = if self.tile.is_some() {
                block_h
            } else {
                (self.height - by * block_h).min(block_h)
            };
            for _bx in 0..self.blocks_across() {
                blocks.push(vec![self.marker; (block_w * rows) as usize]);
            }
        }
        blocks
    }
}

/// One IFD entry, pre-serialisation: tag, field type, count, payload.
type Entry = (u16, u16, u32, Vec<u8>);

fn entries_for(spec: &IfdSpec, blocks: &[Vec<u8>]) -> Vec<Entry> {
    let block_count = blocks.len() as u32;
    let counts: Vec<u8> = blocks
        .iter()
        .flat_map(|b| (b.len() as u32).to_le_bytes())
        .collect();

    let mut entries: Vec<Entry> = vec![
        (TAG_IMAGE_WIDTH, LONG, 1, spec.width.to_le_bytes().to_vec()),
        (
            TAG_IMAGE_LENGTH,
            LONG,
            1,
            spec.height.to_le_bytes().to_vec(),
        ),
        (TAG_BITS_PER_SAMPLE, SHORT, 1, 8u16.to_le_bytes().to_vec()),
        (TAG_COMPRESSION, SHORT, 1, 1u16.to_le_bytes().to_vec()),
        (
            TAG_PHOTOMETRIC,
            SHORT,
            1,
            spec.photometric.to_le_bytes().to_vec(),
        ),
        (TAG_SAMPLES_PER_PIXEL, SHORT, 1, 1u16.to_le_bytes().to_vec()),
        (
            TAG_PLANAR_CONFIGURATION,
            SHORT,
            1,
            1u16.to_le_bytes().to_vec(),
        ),
        (TAG_SAMPLE_FORMAT, SHORT, 1, 1u16.to_le_bytes().to_vec()),
    ];

    if let Some(subfile) = spec.new_subfile_type {
        entries.push((
            TAG_NEW_SUBFILE_TYPE,
            LONG,
            1,
            subfile.to_le_bytes().to_vec(),
        ));
    }

    match spec.tile {
        Some((tw, th)) => {
            entries.push((TAG_TILE_WIDTH, LONG, 1, tw.to_le_bytes().to_vec()));
            entries.push((TAG_TILE_LENGTH, LONG, 1, th.to_le_bytes().to_vec()));
            entries.push((
                TAG_TILE_OFFSETS,
                LONG,
                block_count,
                vec![0; block_count as usize * 4],
            ));
            entries.push((TAG_TILE_BYTE_COUNTS, LONG, block_count, counts));
        }
        None => {
            entries.push((
                TAG_STRIP_OFFSETS,
                LONG,
                block_count,
                vec![0; block_count as usize * 4],
            ));
            entries.push((
                TAG_ROWS_PER_STRIP,
                LONG,
                1,
                spec.rows_per_strip.to_le_bytes().to_vec(),
            ));
            entries.push((TAG_STRIP_BYTE_COUNTS, LONG, block_count, counts));
        }
    }

    entries.sort_by_key(|(tag, _, _, _)| *tag);
    entries
}

/// Emits a little-endian classic TIFF whose IFD chain is exactly `specs`.
fn build_tiff(specs: &[IfdSpec]) -> Vec<u8> {
    let per_ifd_blocks: Vec<Vec<Vec<u8>>> = specs.iter().map(IfdSpec::blocks).collect();
    let mut per_ifd_entries: Vec<Vec<Entry>> = specs
        .iter()
        .zip(&per_ifd_blocks)
        .map(|(spec, blocks)| entries_for(spec, blocks))
        .collect();

    // Pass 1 — lay out the IFDs and their out-of-line payloads.
    let mut ifd_starts = Vec::with_capacity(specs.len());
    let mut external_offsets: Vec<Vec<Option<u32>>> = Vec::with_capacity(specs.len());
    let mut cursor = 8u32;
    for entries in &per_ifd_entries {
        ifd_starts.push(cursor);
        cursor += 2 + entries.len() as u32 * 12 + 4;
        let mut offsets = Vec::with_capacity(entries.len());
        for (_, _, _, payload) in entries {
            if payload.len() <= 4 {
                offsets.push(None);
            } else {
                offsets.push(Some(cursor));
                cursor += payload.len() as u32;
                cursor += cursor % 2;
            }
        }
        external_offsets.push(offsets);
    }

    // Pass 2 — block offsets, now that the directory size is known.
    let data_start = cursor;
    for (index, blocks) in per_ifd_blocks.iter().enumerate() {
        let mut block_offsets = Vec::with_capacity(blocks.len());
        for block in blocks {
            block_offsets.push(cursor);
            cursor += block.len() as u32;
        }
        let payload: Vec<u8> = block_offsets.iter().flat_map(|o| o.to_le_bytes()).collect();
        let offsets_tag = if specs[index].tile.is_some() {
            TAG_TILE_OFFSETS
        } else {
            TAG_STRIP_OFFSETS
        };
        for entry in per_ifd_entries[index].iter_mut() {
            if entry.0 == offsets_tag {
                entry.3 = payload.clone();
            }
        }
    }

    // Pass 3 — emit.
    let mut out = Vec::with_capacity(cursor as usize);
    out.extend_from_slice(b"II");
    out.extend_from_slice(&42u16.to_le_bytes());
    out.extend_from_slice(&ifd_starts[0].to_le_bytes());
    for (index, entries) in per_ifd_entries.iter().enumerate() {
        assert_eq!(out.len() as u32, ifd_starts[index]);
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for (position, (tag, field_type, count, payload)) in entries.iter().enumerate() {
            out.extend_from_slice(&tag.to_le_bytes());
            out.extend_from_slice(&field_type.to_le_bytes());
            out.extend_from_slice(&count.to_le_bytes());
            match external_offsets[index][position] {
                Some(offset) => out.extend_from_slice(&offset.to_le_bytes()),
                None => {
                    let mut inline = [0u8; 4];
                    inline[..payload.len()].copy_from_slice(payload);
                    out.extend_from_slice(&inline);
                }
            }
        }
        let next = ifd_starts.get(index + 1).copied().unwrap_or(0);
        out.extend_from_slice(&next.to_le_bytes());
        for (position, (_, _, _, payload)) in entries.iter().enumerate() {
            if let Some(offset) = external_offsets[index][position] {
                assert_eq!(out.len() as u32, offset);
                out.extend_from_slice(payload);
                if out.len() % 2 != 0 {
                    out.push(0);
                }
            }
        }
    }
    assert_eq!(out.len() as u32, data_start);
    for blocks in &per_ifd_blocks {
        for block in blocks {
            out.extend_from_slice(block);
        }
    }
    assert_eq!(out.len() as u32, cursor);
    out
}

fn cog(specs: &[IfdSpec]) -> CogReader<MemorySource> {
    CogReader::open(MemorySource(build_tiff(specs))).expect("open fixture")
}

/// The marker byte every pixel of the tile carries, asserting the tile is
/// uniform so a mis-sized or mis-addressed read cannot pass by accident.
fn marker_of(tile: &[u8]) -> u8 {
    assert!(!tile.is_empty(), "empty tile");
    let first = tile[0];
    assert!(
        tile.iter().all(|&b| b == first),
        "tile is not uniform: it spans more than one block"
    );
    first
}

// Markers, one per IFD role.
const FULL: u8 = 0xF0;
const OVERVIEW_A: u8 = 0xA0;
const OVERVIEW_B: u8 = 0xB0;
const MASK: u8 = 0x0D;

/// `[full, overviewA, mask, overviewB]` — a mask between two overviews, the
/// layout `COPY_SRC_OVERVIEWS` produces when only one overview is masked.
fn mask_between_overviews() -> Vec<IfdSpec> {
    vec![
        IfdSpec::image(64, 64, (32, 32), None, FULL),
        IfdSpec::image(32, 32, (16, 16), Some(1), OVERVIEW_A),
        IfdSpec::mask_by_subfile_type(64, 64, (32, 32), MASK),
        IfdSpec::image(16, 16, (8, 8), Some(1), OVERVIEW_B),
    ]
}

/// `[full, mask, overviewA, overviewB]` — the canonical `CreateMaskBand`
/// layout: the full-resolution mask sits immediately after the primary IFD, so
/// level 1 is the overview *after* the mask.
fn mask_before_overviews(mask: IfdSpec) -> Vec<IfdSpec> {
    vec![
        IfdSpec::image(64, 64, (32, 32), None, FULL),
        mask,
        IfdSpec::image(32, 32, (16, 16), Some(1), OVERVIEW_A),
        IfdSpec::image(16, 16, (8, 8), Some(1), OVERVIEW_B),
    ]
}

// ---------------------------------------------------------------------------
// Level enumeration
// ---------------------------------------------------------------------------

#[test]
fn mask_between_overviews_is_not_counted_as_a_level() {
    let reader = cog(&mask_between_overviews());

    assert_eq!(
        reader.overview_count(),
        2,
        "the mask IFD was counted as a third overview"
    );
    // The raw chain is unchanged and still reachable: masks are hidden from the
    // level API, not from the file.
    assert_eq!(reader.ifd_count(), 4);
    assert_eq!(reader.tiff().image_count(), 4);
}

#[test]
fn level_indices_skip_the_mask_ifd() {
    let reader = cog(&mask_between_overviews());

    assert_eq!(reader.level_ifd_index(0), Some(0));
    assert_eq!(reader.level_ifd_index(1), Some(1));
    // Level 2 is chain index *3*: index 2 is the mask.
    assert_eq!(reader.level_ifd_index(2), Some(3));
    assert_eq!(reader.level_ifd_index(3), None);
}

/// Every level's identity, pinned by the bytes its tiles carry. Before the fix
/// `read_tile(2, …)` returned `MASK`.
#[test]
fn tile_reads_land_on_the_level_the_caller_asked_for() {
    let reader = cog(&mask_between_overviews());

    assert_eq!(
        marker_of(&reader.read_tile(0, 0, 0).expect("level 0")),
        FULL
    );
    assert_eq!(
        marker_of(&reader.read_tile(1, 0, 0).expect("level 1")),
        OVERVIEW_A
    );
    assert_eq!(
        marker_of(&reader.read_tile(2, 0, 0).expect("level 2")),
        OVERVIEW_B,
        "level 2 returned the mask's pixels, not the smallest overview's"
    );
    assert!(
        reader.read_tile(3, 0, 0).is_err(),
        "there is no level 3: the mask must not extend the pyramid"
    );
}

/// The canonical layout, where the mask precedes *every* overview: level 1 is
/// then the first real overview, i.e. the IFD after the mask.
#[test]
fn mask_before_the_overviews_shifts_nothing() {
    let reader = cog(&mask_before_overviews(IfdSpec::mask_by_subfile_type(
        64,
        64,
        (32, 32),
        MASK,
    )));

    assert_eq!(reader.overview_count(), 2);
    assert_eq!(reader.level_ifd_index(1), Some(2));
    assert_eq!(
        marker_of(&reader.read_tile(1, 0, 0).expect("level 1")),
        OVERVIEW_A,
        "level 1 returned the mask's pixels, not the first overview's"
    );
    assert_eq!(
        marker_of(&reader.read_tile(2, 0, 0).expect("level 2")),
        OVERVIEW_B
    );
}

/// A mask that carries no `NewSubfileType` at all and is marked only by
/// `PhotometricInterpretation == 4` is skipped just the same.
#[test]
fn mask_marked_only_by_photometric_is_skipped() {
    let reader = cog(&mask_before_overviews(IfdSpec::mask_by_photometric(
        64,
        64,
        (32, 32),
        MASK,
    )));

    assert_eq!(reader.overview_count(), 2);
    assert_eq!(
        marker_of(&reader.read_tile(1, 0, 0).expect("level 1")),
        OVERVIEW_A
    );
    assert_eq!(
        marker_of(&reader.read_tile(2, 0, 0).expect("level 2")),
        OVERVIEW_B
    );
}

/// A pyramid with no mask at all must be untouched: every reduced-resolution
/// IFD is still a level, in chain order.
#[test]
fn unmasked_pyramid_is_unaffected() {
    let reader = cog(&[
        IfdSpec::image(64, 64, (32, 32), None, FULL),
        IfdSpec::image(32, 32, (16, 16), Some(1), OVERVIEW_A),
        IfdSpec::image(16, 16, (8, 8), Some(1), OVERVIEW_B),
    ]);

    assert_eq!(reader.overview_count(), 2);
    assert_eq!(reader.level_ifd_index(2), Some(2));
    assert_eq!(
        marker_of(&reader.read_tile(2, 0, 0).expect("level 2")),
        OVERVIEW_B
    );
}

/// The primary IFD is level 0 whatever its tags say: a standalone `.msk` opened
/// directly is still that file's image, and skipping it would leave the reader
/// with no full-resolution level at all.
#[test]
fn primary_ifd_is_never_classified_as_a_mask() {
    let reader = cog(&[
        IfdSpec::mask_by_subfile_type(64, 64, (32, 32), FULL),
        IfdSpec::image(32, 32, (16, 16), Some(1), OVERVIEW_A),
    ]);

    assert_eq!(reader.width(), 64);
    assert_eq!(reader.overview_count(), 1);
    assert_eq!(
        marker_of(&reader.read_tile(0, 0, 0).expect("level 0")),
        FULL
    );
}

// ---------------------------------------------------------------------------
// Cross-surface agreement
// ---------------------------------------------------------------------------

/// `GeoTiffReader::level_size` resolves the level's IFD independently of the
/// tile read path; both must name the same image. Before the fix `level_size(2)`
/// returned the mask's 64x64 while `read_tile(2, …)` returned 8x8 tiles.
#[test]
fn level_size_agrees_with_the_tile_read_path() {
    let reader =
        GeoTiffReader::open(MemorySource(build_tiff(&mask_between_overviews()))).expect("open");

    assert_eq!(reader.level_size(0).expect("level 0"), (64, 64));
    assert_eq!(reader.level_size(1).expect("level 1"), (32, 32));
    assert_eq!(
        reader.level_size(2).expect("level 2"),
        (16, 16),
        "level 2 reported the mask's dimensions"
    );
    assert!(reader.level_size(3).is_err());
}

/// The band/window read engine resolves geometry through
/// `band_read::LevelGeometry`, a third path onto the same IFDs. Reading a whole
/// level through it must return that level's pixels — all `OVERVIEW_B`, never a
/// byte of the mask.
#[test]
fn window_reads_use_the_same_level_map() {
    let reader =
        GeoTiffReader::open(MemorySource(build_tiff(&mask_between_overviews()))).expect("open");

    let window = reader
        .read_window(2, 0, 0, 0, 16, 16)
        .expect("window at level 2");
    assert_eq!(window.len(), 16 * 16);
    assert!(
        window.iter().all(|&b| b == OVERVIEW_B),
        "window read at level 2 returned pixels from another IFD"
    );
}

// ---------------------------------------------------------------------------
// Per-level tile geometry
// ---------------------------------------------------------------------------

/// `tile_pixel_size` is the geometry of the block `read_tile` returns, taken
/// from the same computation — so a caller sizing an image buffer from it can
/// never disagree with the bytes it gets, even though each level here declares
/// a different `TileWidth`/`TileLength`.
#[test]
fn tile_pixel_size_is_per_level_and_matches_the_decoded_block() {
    let reader = cog(&mask_between_overviews());

    for (level, expected) in [(0usize, (32u32, 32u32)), (1, (16, 16)), (2, (8, 8))] {
        assert_eq!(
            reader.tile_pixel_size(level, 0).expect("tile size"),
            expected,
            "level {level}"
        );
        let (w, h) = expected;
        assert_eq!(
            reader.read_tile(level, 0, 0).expect("tile").len(),
            (w * h) as usize,
            "level {level}: decoded block does not match its declared geometry"
        );
        assert_eq!(
            reader.tile_decoded_size(level, 0).expect("decoded size"),
            (w * h) as usize
        );
    }

    assert!(reader.tile_pixel_size(3, 0).is_err());
}

/// On a striped level the block geometry is `ImageWidth × RowsPerStrip`, and the
/// last strip is short. `tile_pixel_size` reports the real row count of each
/// strip, so a buffer sized from it is exactly the decoded block.
#[test]
fn tile_pixel_size_narrows_the_short_final_strip() {
    let reader = cog(&[
        IfdSpec::image(64, 64, (32, 32), None, FULL),
        // 20 rows in strips of 8: 8, 8, 4.
        IfdSpec::striped(16, 20, 8, OVERVIEW_A),
    ]);

    assert_eq!(reader.tile_pixel_size(1, 0).expect("strip 0"), (16, 8));
    assert_eq!(reader.tile_pixel_size(1, 1).expect("strip 1"), (16, 8));
    assert_eq!(
        reader.tile_pixel_size(1, 2).expect("strip 2"),
        (16, 4),
        "the final strip holds 4 rows, not a full 8"
    );

    for strip in 0..3 {
        let (w, h) = reader.tile_pixel_size(1, strip).expect("strip");
        assert_eq!(
            reader.read_tile(1, 0, strip).expect("strip bytes").len(),
            (w * h) as usize
        );
        assert_eq!(
            reader.tile_decoded_size(1, strip).expect("decoded size"),
            (w * h) as usize
        );
    }
}
