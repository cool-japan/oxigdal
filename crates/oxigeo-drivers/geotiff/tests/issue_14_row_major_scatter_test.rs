//! cool-japan/oxigeo#14 — the band scatter writes its destination in row-major
//! order (staging a *group* of horizontally adjacent tiles and copying them out
//! one output row at a time) instead of one tile at a time. That reordering is
//! purely a memory-traffic optimisation and must not move a single byte.
//!
//! This file pins that: for a tiled multi-band raster it recomputes the band the
//! way the old tile-at-a-time scatter did — decode tile `(tx, ty)`, then splatter
//! its rows into the band buffer — using nothing but [`CogReader::read_tile`], and
//! requires every `read_band*` entry point to agree with it exactly. A hard-coded
//! FNV-1a digest of the expected band is asserted too, so the pin survives even a
//! change that moved both the engine and the reference together.
//!
//! Windowed reads are covered as well, because a window that starts and ends
//! mid-tile is what exercises the partial runs inside a staged group.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use oxigeo_core::error::{OxiGeoError, Result};
use oxigeo_core::io::{ByteRange, DataSource};
use oxigeo_core::types::RasterDataType;
use oxigeo_geotiff::GeoTiffReader;
use oxigeo_geotiff::cog::CogReader;
use oxigeo_geotiff::tiff::{Compression, Predictor};
use oxigeo_geotiff::writer::{
    GeoTiffWriter, GeoTiffWriterOptions, OverviewResampling, WriterConfig,
};

/// Wide enough that a block row spans several tiles — grouping only exists for
/// that case, and with a single block column the engine takes the untouched
/// striped path instead.
const WIDTH: u64 = 200;
const HEIGHT: u64 = 140;
const TILE_W: u32 = 64;
const TILE_H: u32 = 48;
const BANDS: usize = 3;
const BPS: usize = 2;

/// In-memory data source over the written file.
struct SliceSource(Vec<u8>);

impl DataSource for SliceSource {
    fn size(&self) -> Result<u64> {
        Ok(self.0.len() as u64)
    }

    fn read_range(&self, range: ByteRange) -> Result<Vec<u8>> {
        let start = range.start as usize;
        let end = (range.end as usize).min(self.0.len());
        if start > end {
            return Err(OxiGeoError::OutOfBounds {
                message: "invalid range".to_string(),
            });
        }
        Ok(self.0[start..end].to_vec())
    }
}

/// Deterministic interleaved test pattern; every sample of every band differs.
fn pattern() -> Vec<u8> {
    let mut out = Vec::with_capacity(WIDTH as usize * HEIGHT as usize * BANDS * BPS);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            for band in 0..BANDS as u64 {
                let n = x * 7 + y * 131 + band * 20_011;
                out.extend_from_slice(&((n % 65_536) as u16).to_le_bytes());
            }
        }
    }
    out
}

/// Writes the fixture and returns its bytes.
///
/// This fixture is *not* cached: it is written, read back once, and deleted, so
/// the leaf name embeds the process id and a monotonic counter to keep two test
/// binaries — or two concurrent runs of this one — off each other's bytes.  A
/// drop guard removes it even if the `expect`s below panic.
fn fixture() -> Vec<u8> {
    /// Removes the fixture on scope exit, including on a panic.
    struct TempPath(PathBuf);

    impl Drop for TempPath {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let guard = TempPath(std::env::temp_dir().join(format!(
        "oxigeo_geotiff_issue14_rowmajor_{}_{seq}_{WIDTH}x{HEIGHT}_{BANDS}b_t{TILE_W}x{TILE_H}.tif",
        std::process::id()
    )));
    let path = &guard.0;
    let config = WriterConfig::new(WIDTH, HEIGHT, BANDS as u16, RasterDataType::UInt16)
        .with_compression(Compression::None)
        .with_predictor(Predictor::None)
        .with_tile_size(TILE_W, TILE_H)
        .with_overviews(false, OverviewResampling::Nearest);
    let mut writer = GeoTiffWriter::create(path, config, GeoTiffWriterOptions::default())
        .expect("create writer");
    writer.write(&pattern()).expect("write fixture");
    std::fs::read(path).expect("read fixture")
}

/// Rebuilds one band the way the pre-row-major scatter did: whole tile first,
/// then its rows splattered into the band buffer at a `WIDTH`-sample stride.
///
/// Deliberately written against [`CogReader::read_tile`] only, so it shares no
/// code at all with the engine under test.
fn tile_major_reference(cog: &CogReader<SliceSource>, band: usize) -> Vec<u8> {
    let width = WIDTH as usize;
    let height = HEIGHT as usize;
    let tile_w = TILE_W as usize;
    let tile_h = TILE_H as usize;
    let across = (WIDTH as u32).div_ceil(TILE_W);
    let down = (HEIGHT as u32).div_ceil(TILE_H);

    let mut out = vec![0u8; width * height * BPS];
    for ty in 0..down {
        for tx in 0..across {
            let tile = cog.read_tile(0, tx, ty).expect("read_tile");
            for row in 0..tile_h {
                let y = ty as usize * tile_h + row;
                if y >= height {
                    continue;
                }
                for col in 0..tile_w {
                    let x = tx as usize * tile_w + col;
                    if x >= width {
                        continue;
                    }
                    let from = ((row * tile_w + col) * BANDS + band) * BPS;
                    let to = (y * width + x) * BPS;
                    out[to..to + BPS].copy_from_slice(&tile[from..from + BPS]);
                }
            }
        }
    }
    out
}

/// FNV-1a, so the expectation is pinned to a literal and not merely to a second
/// implementation that could drift alongside the first.
fn fnv1a(data: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &byte in data {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

/// Crops the band-sized reference to a window, row-major.
fn crop(band: &[u8], x: usize, y: usize, w: usize, h: usize) -> Vec<u8> {
    let width = WIDTH as usize;
    let mut out = Vec::with_capacity(w * h * BPS);
    for row in 0..h {
        let start = ((y + row) * width + x) * BPS;
        out.extend_from_slice(&band[start..start + w * BPS]);
    }
    out
}

#[test]
fn test_issue_14_row_major_scatter_matches_tile_major_reference() {
    let bytes = fixture();
    let reader = GeoTiffReader::open(SliceSource(bytes.clone())).expect("open reader");
    let cog = CogReader::open(SliceSource(bytes)).expect("open cog");

    // The fixture must actually span several block columns, or this test would
    // silently be measuring the untouched single-block-column path.
    assert!(
        (WIDTH as u32).div_ceil(TILE_W) >= 2,
        "the fixture must span more than one block column"
    );

    // Digests of the tile-major reference for bands 0, 1, 2, pinned as literals.
    const GOLDEN: [u64; BANDS] = [
        0xa775_1f9d_1b0f_a8a7,
        0xd9f9_a151_d464_5314,
        0x714b_4637_c537_55a9,
    ];
    let mut digests = [0u64; BANDS];

    for (band, digest) in digests.iter_mut().enumerate() {
        let expected = tile_major_reference(&cog, band);
        *digest = fnv1a(&expected);

        // read_band (owned) …
        let owned = reader.read_band(0, band).expect("read_band");
        assert_eq!(
            owned, expected,
            "read_band band {band} disagrees with the tile-major reference"
        );

        // … read_band_into (caller-owned bytes) …
        let mut raw = vec![0u8; reader.band_byte_len(0).expect("band_byte_len")];
        reader
            .read_band_into(0, band, &mut raw)
            .expect("read_band_into");
        assert_eq!(
            raw, expected,
            "read_band_into band {band} disagrees with the tile-major reference"
        );

        // … and the fused typed conversion.
        let mut typed = vec![0.0f64; reader.band_pixel_count(0).expect("band_pixel_count")];
        reader
            .read_band_into_typed(0, band, &mut typed)
            .expect("read_band_into_typed");
        let expected_typed: Vec<f64> = expected
            .chunks_exact(BPS)
            .map(|c| f64::from(u16::from_le_bytes([c[0], c[1]])))
            .collect();
        assert_eq!(
            typed, expected_typed,
            "read_band_into_typed band {band} disagrees with the tile-major reference"
        );

        // Windows that start and end mid-tile, so the staged group is copied out
        // in partial runs at both ends.
        for &(x, y, w, h) in &[
            (0usize, 0usize, WIDTH as usize, HEIGHT as usize),
            (30, 17, 150, 100),
            (63, 47, 2, 2),
            (TILE_W as usize, 0, TILE_W as usize, TILE_H as usize),
        ] {
            let window = reader
                .read_window(0, band, x as u64, y as u64, w as u64, h as u64)
                .expect("read_window");
            assert_eq!(
                window,
                crop(&expected, x, y, w, h),
                "read_window({x},{y} {w}x{h}) band {band} disagrees with the reference"
            );

            let mut into = vec![0u8; w * h * BPS];
            reader
                .read_window_into(0, band, x as u64, y as u64, w as u64, h as u64, &mut into)
                .expect("read_window_into");
            assert_eq!(
                into,
                crop(&expected, x, y, w, h),
                "read_window_into({x},{y} {w}x{h}) band {band} disagrees with the reference"
            );
        }
    }

    // Print first so a legitimate fixture change can be re-pinned from the log.
    eprintln!("issue#14 tile-major reference digests: {digests:#018x?}");
    assert_ne!(
        digests[0], digests[1],
        "the fixture's bands must differ, or the test proves nothing"
    );
    assert_eq!(
        digests, GOLDEN,
        "the band bytes changed; the row-major scatter must be byte-for-byte \
         identical to the tile-major one"
    );
}
