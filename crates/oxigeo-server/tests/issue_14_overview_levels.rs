//! Regression tests for cool-japan/oxigeo#14 — per-level geometry in
//! `oxigeo-server`.
//!
//! Three defects, all of which produce a plausible-looking wrong answer rather
//! than an error:
//!
//! 1. **`Dataset::read_band_window` bounds-checked and clamped against the
//!    *full-resolution* dimensions.** The driver used to expose no per-level
//!    size accessor, so a window that fits the full-resolution grid but not the
//!    overview grid was rejected by the driver instead of clamped here.
//!    `GeoTiffReader::level_size` closes that.
//! 2. **`Dataset::read_tile_buffer` had no band selector**, so a multi-band
//!    dataset could only ever be tiled through band 0.
//! 3. **The WMS and WMTS handlers computed each level's decimation factor as
//!    `1 << level`.** GDAL and this crate's own writer routinely build chains
//!    that are not exact powers of two, and on such a chain that assumption
//!    picks the wrong level *and* maps the request window to the wrong place
//!    inside it — a georeferencing error in served imagery. Both handlers now
//!    share `Dataset::select_overview_level` and `Dataset::window_at_level`,
//!    which measure against each level's real dimensions.
//!
//! The fixture is a hand-built multi-IFD TIFF with the deliberately
//! non-power-of-two chain **100x100 → 40x40 → 10x10** (factors 2.5 and 10, not
//! 2 and 4). The crate's writer only emits power-of-two pyramids, so it cannot
//! express the layout these defects hide in.

#![allow(clippy::expect_used)]
#![allow(clippy::panic)]
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use oxigeo_geotiff::tiff::TiffTag;
use oxigeo_server::dataset_registry::Dataset;

const BANDS: u16 = 2;

/// The fixture's resolution chain. Neither step is a power of two.
const LEVELS: [LevelSpec; 3] = [
    LevelSpec::tiled(100, 100, 16, 16),
    LevelSpec::tiled(40, 40, 16, 16),
    LevelSpec::striped(10, 10, 4),
];

/// Deterministic sample: distinct per level, band and pixel, never zero, so an
/// all-zero buffer or a read from the wrong level cannot pass unnoticed.
fn sample_value(level: usize, band: usize, x: u32, y: u32) -> u8 {
    ((level as u32 * 37 + band as u32 * 91 + x * 5 + y * 11) % 251 + 1) as u8
}

// ---------------------------------------------------------------------------
// Multi-IFD synthetic TIFF builder (uncompressed, UInt8, chunky, little-endian)
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
    const fn tiled(width: u32, height: u32, tile_w: u32, tile_h: u32) -> Self {
        Self {
            width,
            height,
            tile: Some((tile_w, tile_h)),
            rows_per_strip: 0,
        }
    }

    const fn striped(width: u32, height: u32, rows_per_strip: u32) -> Self {
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

/// Serialises one level's blocks, chunky and in block-raster order.
fn level_blocks(level: usize) -> Vec<Vec<u8>> {
    let lvl = LEVELS[level];
    let spp = BANDS as usize;
    let (block_w, block_h) = lvl.block_dims();
    let across = lvl.blocks_across();
    let down = lvl.blocks_down();

    let mut blocks = Vec::with_capacity(across as usize * down as usize);
    for by in 0..down {
        let rows = if lvl.tile.is_some() {
            block_h
        } else {
            (lvl.height - by * block_h).min(block_h)
        };
        for bx in 0..across {
            let mut block = Vec::with_capacity(block_w as usize * rows as usize * spp);
            for row in 0..rows {
                let y = by * block_h + row;
                for col in 0..block_w {
                    let x = bx * block_w + col;
                    for band in 0..spp {
                        if x >= lvl.width || y >= lvl.height {
                            block.push(0); // tile padding outside the image
                        } else {
                            block.push(sample_value(level, band, x, y));
                        }
                    }
                }
            }
            blocks.push(block);
        }
    }
    blocks
}

/// One IFD entry, pre-serialisation.
type Entry = (TiffTag, u16, u32, Vec<u8>);

const SHORT: u16 = 3;
const LONG: u16 = 4;

/// Builds the tag list for one level (block offsets are patched in later).
fn level_entries(level: usize, blocks: &[Vec<u8>]) -> Vec<Entry> {
    let lvl = LEVELS[level];
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
            u32::from(BANDS),
            (0..BANDS).flat_map(|_| 8u16.to_le_bytes()).collect(),
        ),
        (TiffTag::Compression, SHORT, 1, 1u16.to_le_bytes().to_vec()),
        (
            TiffTag::PhotometricInterpretation,
            SHORT,
            1,
            1u16.to_le_bytes().to_vec(),
        ),
        (
            TiffTag::SamplesPerPixel,
            SHORT,
            1,
            BANDS.to_le_bytes().to_vec(),
        ),
        (
            TiffTag::PlanarConfiguration,
            SHORT,
            1,
            1u16.to_le_bytes().to_vec(),
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
fn build_multi_level_tiff() -> Vec<u8> {
    let per_level_blocks: Vec<Vec<Vec<u8>>> = (0..LEVELS.len()).map(level_blocks).collect();
    let mut per_level_entries: Vec<Vec<Entry>> = (0..LEVELS.len())
        .map(|l| level_entries(l, &per_level_blocks[l]))
        .collect();

    // Pass 1 — lay out IFDs and their out-of-line payloads.
    let mut ifd_starts = Vec::with_capacity(LEVELS.len());
    let mut external_offsets: Vec<Vec<Option<u32>>> = Vec::with_capacity(LEVELS.len());
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
        let offsets_tag = if LEVELS[level].tile.is_some() {
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

/// Per-test scratch fixture inside the system temp dir (house policy: no
/// hardcoded absolute paths).
///
/// The leaf name embeds the process id and a monotonic counter, so no two test
/// binaries — nor two concurrent runs of this one — can ever land on the same
/// file.  Dropping the guard removes the fixture, so a panicking test leaks
/// nothing.
struct TempPath(PathBuf);

impl TempPath {
    fn new(name: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!(
            "oxigeo_issue14_server_levels_{}_{seq}_{name}",
            std::process::id()
        )))
    }
}

impl std::ops::Deref for TempPath {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for TempPath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Writes the fixture into the platform temp dir and opens it.
///
/// The returned [`TempPath`] guard deletes the fixture when the caller drops
/// it, panic or not.
fn fixture(tag: &str) -> (Dataset, TempPath) {
    let path = TempPath::new(&format!("{tag}.tif"));
    std::fs::write(&path, build_multi_level_tiff()).expect("write fixture");
    let dataset = Dataset::open(&path).expect("open fixture");
    (dataset, path)
}

// ---------------------------------------------------------------------------
// 1. level_size
// ---------------------------------------------------------------------------

#[test]
fn test_issue_14_level_size_reports_each_levels_own_dimensions() {
    let (dataset, _fixture) = fixture("level_size");
    assert_eq!(dataset.overview_count(), 2);

    for (level, spec) in LEVELS.iter().enumerate() {
        let size = dataset.level_size(level).expect("level_size");
        assert_eq!(
            size,
            (u64::from(spec.width), u64::from(spec.height)),
            "level {level} must report its own IFD's dimensions"
        );
    }

    // The whole point: the chain is not `ceil(full / 2^level)`.
    assert_ne!(
        dataset.level_size(1).expect("level 1"),
        (50, 50),
        "the fixture's level 1 is 40x40, not the 50x50 a power-of-two chain \
         would have; a caller inferring `1 << level` reads the wrong pixels"
    );
    assert_ne!(
        dataset.level_size(2).expect("level 2"),
        (25, 25),
        "the fixture's level 2 is 10x10, not 25x25"
    );

    assert!(dataset.level_size(3).is_err(), "level 3 does not exist");
}

// ---------------------------------------------------------------------------
// 2. read_band_window at level > 0
// ---------------------------------------------------------------------------

#[test]
fn test_issue_14_read_band_window_bounds_check_uses_the_level() {
    let (dataset, _fixture) = fixture("band_window");

    // A full read of level 1, per band.
    for band in 0..BANDS as usize {
        let buffer = dataset
            .read_band_window(1, band, 0, 0, 40, 40)
            .unwrap_or_else(|e| panic!("level 1 band {band}: {e}"));
        assert_eq!((buffer.width(), buffer.height()), (40, 40));
        for y in 0..40u32 {
            for x in 0..40u32 {
                let got = buffer
                    .get_pixel(u64::from(x), u64::from(y))
                    .expect("pixel in range");
                assert_eq!(
                    got,
                    f64::from(sample_value(1, band, x, y)),
                    "level 1 band {band} pixel ({x},{y})"
                );
            }
        }
    }

    // A window that fits the *full-resolution* grid but overhangs level 1's.
    // The old bounds check used the full-resolution dimensions, so it let this
    // through to the driver, which rejected it: an error where a clamp was due.
    let buffer = dataset
        .read_band_window(1, 0, 30, 30, 20, 20)
        .expect("a window overhanging level 1 must be clamped, not rejected");
    assert_eq!((buffer.width(), buffer.height()), (20, 20));
    for y in 0..20u32 {
        for x in 0..20u32 {
            let got = buffer
                .get_pixel(u64::from(x), u64::from(y))
                .expect("pixel in range");
            let expected = if x < 10 && y < 10 {
                f64::from(sample_value(1, 0, 30 + x, 30 + y))
            } else {
                0.0 // the overhang stays zeroed
            };
            assert_eq!(got, expected, "clamped window pixel ({x},{y})");
        }
    }

    // An offset past the level's extent is still an error, and names the level.
    assert!(
        dataset.read_band_window(1, 0, 45, 0, 1, 1).is_err(),
        "x=45 is outside level 1's 40-pixel width"
    );
    // ...and it is inside the full-resolution grid, so the old check accepted it.
    assert!(dataset.read_band_window(0, 0, 45, 0, 1, 1).is_ok());
}

// ---------------------------------------------------------------------------
// 3. read_tile_band_buffer
// ---------------------------------------------------------------------------

#[test]
fn test_issue_14_read_tile_band_buffer_exposes_the_band_selector() {
    let (dataset, _fixture) = fixture("tile_band");

    // Level 1 is tiled 16x16; block (1, 1) covers pixels [16..32) x [16..32).
    for band in 0..BANDS as usize {
        let buffer = dataset
            .read_tile_band_buffer(1, band, 1, 1)
            .unwrap_or_else(|e| panic!("level 1 band {band} block (1,1): {e}"));
        assert_eq!((buffer.width(), buffer.height()), (16, 16));
        for row in 0..16u32 {
            for col in 0..16u32 {
                let got = buffer
                    .get_pixel(u64::from(col), u64::from(row))
                    .expect("pixel in range");
                assert_eq!(
                    got,
                    f64::from(sample_value(1, band, 16 + col, 16 + row)),
                    "level 1 band {band} block(1,1) pixel ({col},{row})"
                );
            }
        }
    }

    // The band-0 shorthand must agree with the explicit selector.
    let shorthand = dataset.read_tile_buffer(1, 1, 1).expect("read_tile_buffer");
    let explicit = dataset
        .read_tile_band_buffer(1, 0, 1, 1)
        .expect("read_tile_band_buffer");
    assert_eq!(shorthand.as_bytes(), explicit.as_bytes());

    // The two bands must differ, or the assertion above proves nothing.
    let band_1 = dataset
        .read_tile_band_buffer(1, 1, 1, 1)
        .expect("read_tile_band_buffer band 1");
    assert_ne!(explicit.as_bytes(), band_1.as_bytes());
}

// ---------------------------------------------------------------------------
// 4. select_overview_level on a non-power-of-two chain
// ---------------------------------------------------------------------------

#[test]
fn test_issue_14_select_overview_level_measures_real_decimation() {
    let (dataset, _fixture) = fixture("select_level");

    // Full resolution or upsampling always stays at level 0.
    assert_eq!(dataset.select_overview_level(50, 50, 50, 50), 0);
    assert_eq!(dataset.select_overview_level(50, 50, 200, 200), 0);

    // The discriminating case. The client is downsampling 3x. The real chain is
    // 100 → 40 → 10, i.e. decimation 2.5 and 10:
    //   * level 1 (2.5x) is within the 1.5x overshoot allowance of 3x, and
    //   * level 2 (10x) is far coarser than the request.
    // The old `1 << level` arithmetic claimed the levels were 2x and 4x, and 4
    // is still within 1.5 * 3 = 4.5, so it picked level 2 — a 10x10 image
    // upsampled to fill a request that wanted 30x30 of real detail.
    assert_eq!(
        dataset.select_overview_level(90, 90, 30, 30),
        1,
        "a 3x request must pick the 2.5x level, not the 10x one that `1 << level` \
         mistakes for 4x"
    );

    // A 10x request genuinely warrants the coarsest level.
    assert_eq!(dataset.select_overview_level(100, 100, 10, 10), 2);
}

// ---------------------------------------------------------------------------
// 5. window_at_level — the georeferencing half
// ---------------------------------------------------------------------------

#[test]
fn test_issue_14_window_at_level_maps_by_real_ratio() {
    let (dataset, _fixture) = fixture("window_at_level");

    // Level 0 is the identity (modulo the clamp).
    assert_eq!(
        dataset.window_at_level(0, 10, 20, 30, 40).expect("level 0"),
        (10, 20, 30, 40)
    );

    // 100 → 40 is a scale of 0.4, so full-resolution (50, 50) is level-1
    // (20, 20) and a 20x20 window becomes 8x8. A power-of-two assumption would
    // have said (25, 25) and 10x10 — pixels 5 rows and columns away from the
    // ground they actually cover.
    assert_eq!(
        dataset.window_at_level(1, 50, 50, 20, 20).expect("level 1"),
        (20, 20, 8, 8),
        "the window must be scaled by the level's real ratio (0.4), not by 1/2"
    );

    // 100 → 10 is a scale of 0.1.
    assert_eq!(
        dataset.window_at_level(2, 50, 50, 20, 20).expect("level 2"),
        (5, 5, 2, 2)
    );

    // The mapped window is clamped into the level and never zero-sized.
    let (x, y, w, h) = dataset.window_at_level(2, 95, 95, 5, 5).expect("edge");
    assert!(x + w <= 10 && y + h <= 10, "clamped into level 2's 10x10");
    assert!(w >= 1 && h >= 1, "never zero-sized");

    // The pixels the mapped window yields really are level 1's.
    let (x, y, w, h) = dataset.window_at_level(1, 50, 50, 20, 20).expect("level 1");
    let buffer = dataset
        .read_band_window(1, 0, x, y, w, h)
        .expect("read mapped window");
    assert_eq!(
        buffer.get_pixel(0, 0).expect("pixel"),
        f64::from(sample_value(1, 0, 20, 20))
    );
}
