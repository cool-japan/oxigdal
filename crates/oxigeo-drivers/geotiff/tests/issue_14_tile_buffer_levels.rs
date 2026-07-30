//! Regression tests for cool-japan/oxigeo#14 — `read_tile_buffer` and
//! per-level dimensions.
//!
//! Three defects are covered here:
//!
//! 1. **`read_tile_buffer` was unusable for `SamplesPerPixel > 1`.** It handed
//!    `RasterBuffer::new` a whole chunky block (`tw·th·bps·spp` bytes) while
//!    claiming `tw·th` pixels, so every multi-band file failed the buffer's own
//!    length check — the caller got an error, or (through the server's tile
//!    stitcher) a silently all-zero window.
//! 2. **`read_tile_buffer` ignored its `level` argument for geometry.** Tile
//!    dimensions came from `primary_info()` whatever level was asked for, so an
//!    overview whose IFD declares a different `TileWidth`/`TileLength` was
//!    decoded into a wrongly-shaped buffer.
//! 3. **There was no public per-level dimensions accessor.** Callers had to
//!    infer `ceil(full / 2^level)`, which is wrong for any pyramid that is not
//!    strictly power-of-two. [`GeoTiffReader::level_size`] reads the level's own
//!    `ImageWidth`/`ImageLength`.
//!
//! The fixtures are hand-built multi-IFD TIFFs, because the crate's writer only
//! ever emits `PlanarConfiguration = Chunky` power-of-two pyramids and so cannot
//! express the layouts these defects hide in.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use oxigeo_core::error::{OxiGeoError, Result};
use oxigeo_core::io::{ByteRange, DataSource};
use oxigeo_core::types::RasterDataType;
use oxigeo_geotiff::GeoTiffReader;
use oxigeo_geotiff::tiff::TiffTag;

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
// Multi-IFD synthetic TIFF builder (uncompressed, UInt16, little-endian)
// ---------------------------------------------------------------------------

/// One resolution level of the fixture.
#[derive(Debug, Clone, Copy)]
struct LevelSpec {
    width: u32,
    height: u32,
    /// `Some((tile_w, tile_h))` for a tiled level, `None` for strips.
    tile: Option<(u32, u32)>,
    /// `RowsPerStrip`, used only when `tile` is `None`.
    rows_per_strip: u32,
}

impl LevelSpec {
    fn tiled(width: u32, height: u32, tile_w: u32, tile_h: u32) -> Self {
        Self {
            width,
            height,
            tile: Some((tile_w, tile_h)),
            rows_per_strip: 0,
        }
    }

    fn striped(width: u32, height: u32, rows_per_strip: u32) -> Self {
        Self {
            width,
            height,
            tile: None,
            rows_per_strip,
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
}

/// The whole fixture: shared sample layout plus one or more levels.
#[derive(Debug, Clone)]
struct FileSpec {
    samples_per_pixel: u16,
    /// TIFF `PlanarConfiguration`: 1 = chunky, 2 = planar.
    planar: u16,
    levels: Vec<LevelSpec>,
}

/// Deterministic sample value: distinct per level, band and pixel.
fn sample_value(level: usize, band: usize, x: u32, y: u32) -> u16 {
    (level as u16) * 10_000 + (band as u16) * 1_000 + (y as u16) * 31 + (x as u16)
}

/// Serialises one level's blocks in the plane-major order the TIFF spec mandates.
fn level_blocks(spec: &FileSpec, level: usize) -> Vec<Vec<u8>> {
    let lvl = spec.levels[level];
    let spp = spec.samples_per_pixel as usize;
    let planes = if spec.planar == 2 { spp } else { 1 };
    let samples_in_block = if spec.planar == 2 { 1 } else { spp };
    let (block_w, block_h) = lvl.block_dims();
    let across = lvl.blocks_across();
    let down = lvl.blocks_down();

    let mut blocks = Vec::with_capacity(planes * across as usize * down as usize);
    for plane in 0..planes {
        for by in 0..down {
            let rows = if lvl.tile.is_some() {
                block_h
            } else {
                (lvl.height - by * block_h).min(block_h)
            };
            for bx in 0..across {
                let mut block =
                    Vec::with_capacity(block_w as usize * rows as usize * samples_in_block * 2);
                for row in 0..rows {
                    let y = by * block_h + row;
                    for col in 0..block_w {
                        let x = bx * block_w + col;
                        for s in 0..samples_in_block {
                            if x >= lvl.width || y >= lvl.height {
                                // Tile padding outside the image.
                                block.extend_from_slice(&0u16.to_le_bytes());
                                continue;
                            }
                            let band = plane * samples_in_block + s;
                            block.extend_from_slice(&sample_value(level, band, x, y).to_le_bytes());
                        }
                    }
                }
                blocks.push(block);
            }
        }
    }
    blocks
}

/// One IFD entry, pre-serialisation.
type Entry = (TiffTag, u16, u32, Vec<u8>);

const SHORT: u16 = 3;
const LONG: u16 = 4;

/// Builds the tag list for one level (offsets are patched in later).
fn level_entries(spec: &FileSpec, level: usize, blocks: &[Vec<u8>]) -> Vec<Entry> {
    let lvl = spec.levels[level];
    let spp = spec.samples_per_pixel;
    let block_count = blocks.len() as u32;
    let counts: Vec<u8> = blocks
        .iter()
        .flat_map(|b| (b.len() as u32).to_le_bytes())
        .collect();

    let mut entries: Vec<Entry> = vec![
        (
            TiffTag::ImageWidth,
            LONG,
            1,
            lvl.width.to_le_bytes().to_vec(),
        ),
        (
            TiffTag::ImageLength,
            LONG,
            1,
            lvl.height.to_le_bytes().to_vec(),
        ),
        (
            TiffTag::BitsPerSample,
            SHORT,
            u32::from(spp),
            (0..spp).flat_map(|_| 16u16.to_le_bytes()).collect(),
        ),
        (TiffTag::Compression, SHORT, 1, 1u16.to_le_bytes().to_vec()),
        (
            TiffTag::PhotometricInterpretation,
            SHORT,
            1,
            if spp >= 3 { 2u16 } else { 1u16 }.to_le_bytes().to_vec(),
        ),
        (
            TiffTag::SamplesPerPixel,
            SHORT,
            1,
            spp.to_le_bytes().to_vec(),
        ),
        (
            TiffTag::PlanarConfiguration,
            SHORT,
            1,
            spec.planar.to_le_bytes().to_vec(),
        ),
        (TiffTag::SampleFormat, SHORT, 1, 1u16.to_le_bytes().to_vec()),
    ];

    match lvl.tile {
        Some((tw, th)) => {
            entries.push((TiffTag::TileWidth, LONG, 1, tw.to_le_bytes().to_vec()));
            entries.push((TiffTag::TileLength, LONG, 1, th.to_le_bytes().to_vec()));
            entries.push((
                TiffTag::TileOffsets,
                LONG,
                block_count,
                vec![0; block_count as usize * 4],
            ));
            entries.push((TiffTag::TileByteCounts, LONG, block_count, counts));
        }
        None => {
            entries.push((
                TiffTag::StripOffsets,
                LONG,
                block_count,
                vec![0; block_count as usize * 4],
            ));
            entries.push((
                TiffTag::RowsPerStrip,
                LONG,
                1,
                lvl.rows_per_strip.to_le_bytes().to_vec(),
            ));
            entries.push((TiffTag::StripByteCounts, LONG, block_count, counts));
        }
    }

    entries.sort_by_key(|(tag, _, _, _)| *tag as u16);
    entries
}

/// Emits a little-endian classic TIFF with one IFD per level.
fn build_multi_level_tiff(spec: &FileSpec) -> Vec<u8> {
    let per_level_blocks: Vec<Vec<Vec<u8>>> = (0..spec.levels.len())
        .map(|l| level_blocks(spec, l))
        .collect();
    let mut per_level_entries: Vec<Vec<Entry>> = (0..spec.levels.len())
        .map(|l| level_entries(spec, l, &per_level_blocks[l]))
        .collect();

    // Pass 1 — lay out IFDs and their out-of-line payloads.
    let mut ifd_starts = Vec::with_capacity(spec.levels.len());
    let mut external_offsets: Vec<Vec<Option<u32>>> = Vec::with_capacity(spec.levels.len());
    let mut cursor = 8u32;
    for entries in &per_level_entries {
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

    // Pass 2 — block payload offsets, now that the headers' size is known.
    let data_start = cursor;
    for (level, blocks) in per_level_blocks.iter().enumerate() {
        let mut block_offsets = Vec::with_capacity(blocks.len());
        for block in blocks {
            block_offsets.push(cursor);
            cursor += block.len() as u32;
        }
        let payload: Vec<u8> = block_offsets.iter().flat_map(|o| o.to_le_bytes()).collect();
        let offsets_tag = if spec.levels[level].tile.is_some() {
            TiffTag::TileOffsets
        } else {
            TiffTag::StripOffsets
        };
        for entry in per_level_entries[level].iter_mut() {
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
    for (level, entries) in per_level_entries.iter().enumerate() {
        assert_eq!(out.len() as u32, ifd_starts[level]);
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for (index, (tag, field_type, count, payload)) in entries.iter().enumerate() {
            out.extend_from_slice(&(*tag as u16).to_le_bytes());
            out.extend_from_slice(&field_type.to_le_bytes());
            out.extend_from_slice(&count.to_le_bytes());
            match external_offsets[level][index] {
                Some(offset) => out.extend_from_slice(&offset.to_le_bytes()),
                None => {
                    let mut inline = [0u8; 4];
                    inline[..payload.len()].copy_from_slice(payload);
                    out.extend_from_slice(&inline);
                }
            }
        }
        let next = ifd_starts.get(level + 1).copied().unwrap_or(0);
        out.extend_from_slice(&next.to_le_bytes());
        for (index, (_, _, _, payload)) in entries.iter().enumerate() {
            if let Some(offset) = external_offsets[level][index] {
                assert_eq!(out.len() as u32, offset);
                out.extend_from_slice(payload);
                if out.len() % 2 != 0 {
                    out.push(0);
                }
            }
        }
    }
    assert_eq!(out.len() as u32, data_start);
    for blocks in &per_level_blocks {
        for block in blocks {
            out.extend_from_slice(block);
        }
    }
    assert_eq!(out.len() as u32, cursor);
    out
}

fn open(spec: &FileSpec) -> GeoTiffReader<MemorySource> {
    let bytes = build_multi_level_tiff(spec);
    GeoTiffReader::open(MemorySource(bytes)).expect("open fixture")
}

/// Reads a `UInt16` buffer back as values, row-major.
fn buffer_values(buffer: &oxigeo_core::buffer::RasterBuffer) -> Vec<u16> {
    assert_eq!(buffer.data_type(), RasterDataType::UInt16);
    buffer
        .as_bytes()
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect()
}

/// Two levels whose overview is *not* `ceil(full / 2)` and whose tile size
/// differs from the full-resolution level's.
fn pyramid_spec(planar: u16, samples_per_pixel: u16) -> FileSpec {
    FileSpec {
        samples_per_pixel,
        planar,
        levels: vec![
            LevelSpec::tiled(8, 6, 4, 4),
            // 5x3, not ceil(8/2)=4 x ceil(6/2)=3: a non-power-of-two pyramid.
            LevelSpec::tiled(5, 3, 2, 2),
        ],
    }
}

// ---------------------------------------------------------------------------
// T3 — level_size
// ---------------------------------------------------------------------------

#[test]
fn test_issue_14_level_size_reads_the_levels_own_ifd() {
    let reader = open(&pyramid_spec(1, 3));

    assert_eq!(reader.level_size(0).expect("level 0"), (8, 6));
    // The inference every caller had to make — ceil(8/2) x ceil(6/2) = 4x3 —
    // is wrong here; only the overview's own IFD knows it is 5x3.
    assert_eq!(reader.level_size(1).expect("level 1"), (5, 3));
    assert_ne!(
        reader.level_size(1).expect("level 1"),
        (8u64.div_ceil(2), 3)
    );
}

#[test]
fn test_issue_14_level_size_rejects_missing_level() {
    let reader = open(&pyramid_spec(1, 2));
    assert_eq!(reader.overview_count(), 1);
    let err = reader
        .level_size(2)
        .expect_err("level 2 does not exist in a two-level file");
    assert!(
        matches!(err, OxiGeoError::OutOfBounds { .. }),
        "expected OutOfBounds, got {err:?}"
    );
}

#[test]
fn test_issue_14_level_size_agrees_with_band_pixel_count() {
    // `band_pixel_count` gives only the product; `level_size` must factor it the
    // way the file actually stores it.
    let reader = open(&pyramid_spec(2, 3));
    for level in 0..=reader.overview_count() {
        let (w, h) = reader.level_size(level).expect("level size");
        assert_eq!(
            reader.band_pixel_count(level).expect("pixel count") as u64,
            w * h,
            "level {level}"
        );
    }
}

// ---------------------------------------------------------------------------
// T2 — read_tile_band_buffer / read_tile_buffer
// ---------------------------------------------------------------------------

#[test]
fn test_issue_14_read_tile_buffer_multiband_chunky() {
    // Before the fix this returned `Err` for every `SamplesPerPixel > 1` file:
    // a chunky tile is `4*4*2*3` bytes, `RasterBuffer::new` was told `4x4` u16.
    let spec = pyramid_spec(1, 3);
    let reader = open(&spec);

    let buffer = reader
        .read_tile_buffer(0, 0, 0)
        .expect("band 0 of tile (0,0) at full resolution");
    assert_eq!((buffer.width(), buffer.height()), (4, 4));
    let values = buffer_values(&buffer);
    for row in 0..4u32 {
        for col in 0..4u32 {
            assert_eq!(
                values[(row * 4 + col) as usize],
                sample_value(0, 0, col, row),
                "band 0 pixel ({col}, {row})"
            );
        }
    }
}

#[test]
fn test_issue_14_read_tile_band_buffer_selects_the_band() {
    for planar in [1u16, 2] {
        let spec = pyramid_spec(planar, 3);
        let reader = open(&spec);
        for band in 0..3usize {
            let buffer = reader
                .read_tile_band_buffer(0, band, 1, 0)
                .unwrap_or_else(|e| panic!("planar={planar} band={band}: {e}"));
            assert_eq!((buffer.width(), buffer.height()), (4, 4));
            let values = buffer_values(&buffer);
            for row in 0..4u32 {
                for col in 0..4u32 {
                    // Tile (1, 0) starts at x = 4.
                    assert_eq!(
                        values[(row * 4 + col) as usize],
                        sample_value(0, band, 4 + col, row),
                        "planar={planar} band={band} pixel ({col}, {row})"
                    );
                }
            }
        }
    }
}

#[test]
fn test_issue_14_read_tile_band_buffer_honours_level_geometry() {
    for planar in [1u16, 2] {
        let spec = pyramid_spec(planar, 2);
        let reader = open(&spec);

        // The overview's tiles are 2x2, the full-resolution ones 4x4. Taking the
        // geometry from `primary_info()` (the old behaviour) produced a 4x4
        // buffer for a 2x2 tile's worth of bytes.
        let buffer = reader
            .read_tile_band_buffer(1, 1, 0, 0)
            .unwrap_or_else(|e| panic!("planar={planar}: {e}"));
        assert_eq!((buffer.width(), buffer.height()), (2, 2));
        let values = buffer_values(&buffer);
        for row in 0..2u32 {
            for col in 0..2u32 {
                assert_eq!(
                    values[(row * 2 + col) as usize],
                    sample_value(1, 1, col, row),
                    "planar={planar} overview pixel ({col}, {row})"
                );
            }
        }
    }
}

#[test]
fn test_issue_14_read_tile_band_buffer_pads_edge_tiles() {
    // Level 1 is 5x3 with 2x2 tiles: the last tile column is 1 px wide and the
    // last tile row is 1 px tall. The buffer stays 2x2, padded with zeros.
    let reader = open(&pyramid_spec(1, 2));
    let buffer = reader
        .read_tile_band_buffer(1, 0, 2, 1)
        .expect("bottom-right overview tile");
    assert_eq!((buffer.width(), buffer.height()), (2, 2));
    let values = buffer_values(&buffer);
    assert_eq!(values[0], sample_value(1, 0, 4, 2));
    assert_eq!(&values[1..], &[0, 0, 0], "overhang must be zero-padded");
}

#[test]
fn test_issue_14_read_tile_band_buffer_striped_last_strip_is_short() {
    // A striped level's "tile" is a strip. The old code sized the buffer
    // `ImageWidth x ImageLength` for every strip, so any file with more than one
    // strip failed outright.
    let spec = FileSpec {
        samples_per_pixel: 2,
        planar: 1,
        levels: vec![LevelSpec::striped(8, 6, 4)],
    };
    let reader = open(&spec);

    let first = reader
        .read_tile_band_buffer(0, 1, 0, 0)
        .expect("first strip");
    assert_eq!((first.width(), first.height()), (8, 4));

    let last = reader
        .read_tile_band_buffer(0, 1, 0, 1)
        .expect("last strip");
    assert_eq!(
        (last.width(), last.height()),
        (8, 2),
        "the final strip holds only the remaining rows"
    );
    let values = buffer_values(&last);
    for row in 0..2u32 {
        for col in 0..8u32 {
            assert_eq!(
                values[(row * 8 + col) as usize],
                sample_value(0, 1, col, 4 + row),
                "strip pixel ({col}, {row})"
            );
        }
    }
}

#[test]
fn test_issue_14_read_tile_band_buffer_rejects_out_of_range_band_and_level() {
    let reader = open(&pyramid_spec(1, 3));

    let err = reader
        .read_tile_band_buffer(0, 3, 0, 0)
        .expect_err("band 3 of a 3-band raster");
    assert!(
        matches!(err, OxiGeoError::InvalidParameter { parameter, .. } if parameter == "band"),
        "expected a typed band error, got {err:?}"
    );

    let err = reader
        .read_tile_band_buffer(2, 0, 0, 0)
        .expect_err("level 2 of a two-level file");
    assert!(
        matches!(err, OxiGeoError::OutOfBounds { .. }),
        "expected OutOfBounds, got {err:?}"
    );

    let err = reader
        .read_tile_band_buffer(1, 0, 3, 0)
        .expect_err("tile column 3 of a 3-column level");
    assert!(
        matches!(err, OxiGeoError::OutOfBounds { .. }),
        "expected OutOfBounds, got {err:?}"
    );
}

#[test]
fn test_issue_14_read_tile_band_buffer_matches_read_band() {
    // Whatever the tile path returns must be a sub-rectangle of what the
    // whole-band path returns — the two engines may not disagree.
    for planar in [1u16, 2] {
        let spec = pyramid_spec(planar, 3);
        let reader = open(&spec);
        for level in 0..=reader.overview_count() {
            let (w, h) = reader.level_size(level).expect("level size");
            for band in 0..3usize {
                let whole = reader.read_band(level, band).expect("read_band");
                let (block_w, block_h) = if level == 0 { (4u64, 4u64) } else { (2, 2) };
                for ty in 0..h.div_ceil(block_h) {
                    for tx in 0..w.div_ceil(block_w) {
                        let buffer = reader
                            .read_tile_band_buffer(level, band, tx as u32, ty as u32)
                            .expect("read_tile_band_buffer");
                        let values = buffer_values(&buffer);
                        for row in 0..block_h {
                            for col in 0..block_w {
                                let (x, y) = (tx * block_w + col, ty * block_h + row);
                                let expected = if x < w && y < h {
                                    let idx = ((y * w + x) * 2) as usize;
                                    u16::from_le_bytes([whole[idx], whole[idx + 1]])
                                } else {
                                    0
                                };
                                assert_eq!(
                                    values[(row * block_w + col) as usize],
                                    expected,
                                    "planar={planar} level={level} band={band} \
                                     tile=({tx},{ty}) pixel=({col},{row})"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
