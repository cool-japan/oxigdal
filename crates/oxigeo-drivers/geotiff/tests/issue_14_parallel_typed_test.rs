//! Regression tests for cool-japan/oxigeo#14 — the *typed* band reads are
//! parallel too.
//!
//! `read_band_into` gained a rayon block-row split early on, but
//! `read_band_into_typed` / `read_window_into_typed` stayed serial: splitting a
//! caller-owned `&mut [T]` across workers needs `T: Send`, and `RasterElement`
//! only promised `Copy + Default + 'static`. A user reading a `Float32` DEM into
//! a `Vec<f64>` — literally the issue's use case — could have the fused
//! conversion **or** the parallel decode, never both.
//!
//! `RasterElement` now declares `Send + Sync` (it is sealed over ten primitive
//! scalars, so the bound is free), and both typed entry points take the same
//! block-row split as the raw path.
//!
//! # How "serial vs parallel" is compared inside one binary
//!
//! A test binary is compiled either with `parallel` or without it, so the two
//! code paths cannot both be exercised by simply calling the same function twice.
//! They can be reached from one build, though: the driver only parallelises a
//! read that spans **at least two block rows** and produces at least 1 MiB. A
//! window that covers exactly one block row is therefore guaranteed to take the
//! serial path in *every* build. Stitching such windows back together yields a
//! serial reference that is valid even in a `--features parallel` build, and both
//! it and the full read are checked against the pattern that was written to the
//! file — ground truth, not just each other.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::path::PathBuf;

use oxigeo_core::buffer::convert_raw_into;
use oxigeo_core::io::FileDataSource;
use oxigeo_core::types::RasterDataType;
use oxigeo_geotiff::GeoTiffReader;
use oxigeo_geotiff::writer::{
    GeoTiffWriter, GeoTiffWriterOptions, OverviewResampling, WriterConfig,
};

/// Tile edge used by every fixture here, matching what GDAL emits for a COG.
const TILE: u64 = 256;
/// Fixture width in pixels.
const WIDTH: u64 = 512;
/// Fixture height in pixels: four 256-pixel block rows.
const HEIGHT: u64 = 1024;

/// A unique temp path per test, removed by the caller.
fn temp_test_file(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "oxigeo_issue14_parallel_typed_{}_{}_{}",
        std::process::id(),
        name,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    path
}

/// A smooth DEM-like `f32` surface, distinct per `(x, y)` so a mis-scattered
/// block row cannot accidentally match.
///
/// Every value is exactly representable in `f32` (and therefore in `f64`), so a
/// bit-for-bit comparison against the pattern is meaningful. Odd columns land on
/// a `.5` fraction, which is what makes the float→integer rounding check below
/// non-trivial, and the range deliberately runs past `i16::MAX` so the saturating
/// path is exercised on worker threads.
fn float_pattern(width: u64, height: u64) -> Vec<f32> {
    let mut values = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            values.push((y as f32) * 64.0 + (x as f32) * 0.5 - 1000.0);
        }
    }
    values
}

/// Writes a single-band `Float32` tiled GeoTIFF and returns its path.
fn write_float_fixture(
    name: &str,
    pattern: &[f32],
    compression: oxigeo_geotiff::Compression,
) -> PathBuf {
    let path = temp_test_file(name);
    let mut raw = Vec::with_capacity(pattern.len() * 4);
    for value in pattern {
        raw.extend_from_slice(&value.to_le_bytes());
    }
    let mut config = WriterConfig::new(WIDTH, HEIGHT, 1, RasterDataType::Float32)
        .with_compression(compression)
        .with_tile_size(TILE as u32, TILE as u32)
        .with_overviews(false, OverviewResampling::Nearest);
    if compression != oxigeo_geotiff::Compression::None {
        config = config.with_predictor(oxigeo_geotiff::tiff::Predictor::FloatingPoint);
    }
    let mut writer =
        GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default()).expect("create");
    writer.write(&raw).expect("write raster");
    path
}

/// Reads the whole band one block row at a time, so every individual call is
/// below the driver's parallel threshold and provably takes the serial path.
fn serial_reference<S: oxigeo_core::io::DataSource>(reader: &GeoTiffReader<S>) -> Vec<f64> {
    let mut out = vec![0.0f64; (WIDTH * HEIGHT) as usize];
    let mut offset = 0usize;
    let mut y = 0u64;
    while y < HEIGHT {
        let rows = TILE.min(HEIGHT - y);
        let take = (WIDTH * rows) as usize;
        let slice = out
            .get_mut(offset..offset + take)
            .expect("reference slice in range");
        reader
            .read_window_into_typed(0, 0, 0, y, WIDTH, rows, slice)
            .expect("single-block-row window read");
        offset += take;
        y += rows;
    }
    out
}

/// Bit-for-bit comparison; `assert_eq!` on `f64` would let a `NaN` pair through
/// unnoticed and would not distinguish `-0.0` from `0.0`.
fn assert_bits_eq(actual: &[f64], expected: &[f64], label: &str) {
    assert_eq!(actual.len(), expected.len(), "{label}: length");
    for (i, (a, b)) in actual.iter().zip(expected.iter()).enumerate() {
        assert_eq!(
            a.to_bits(),
            b.to_bits(),
            "{label}: sample {i} ({a} vs {b}, row {} col {})",
            i as u64 / WIDTH,
            i as u64 % WIDTH
        );
    }
}

/// The core claim: `read_band_into_typed::<f64>` on a multi-block `Float32` file
/// is bit-identical to the serial path and to the pattern that was written,
/// whether or not `parallel` is on.
#[test]
fn test_issue_14_typed_band_read_matches_ground_truth_and_serial() {
    let pattern = float_pattern(WIDTH, HEIGHT);
    let truth: Vec<f64> = pattern.iter().map(|v| *v as f64).collect();
    let path = write_float_fixture("uncompressed", &pattern, oxigeo_geotiff::Compression::None);

    let reader = GeoTiffReader::open(FileDataSource::open(&path).expect("open")).expect("reader");
    assert_eq!(
        reader.band_pixel_count(0).expect("pixels"),
        (WIDTH * HEIGHT) as usize
    );
    // The fixture must actually be big enough to trip the driver's parallel
    // threshold (>= 2 block rows and >= 1 MiB), otherwise this test proves
    // nothing about the parallel path.
    const { assert!(HEIGHT / TILE >= 2, "fixture must span >= 2 block rows") };
    assert!(
        reader.band_byte_len(0).expect("bytes") >= 1 << 20,
        "fixture must exceed the parallel threshold"
    );

    // Poison the destination so a block row the workers failed to write shows up.
    let mut typed = vec![f64::NAN; (WIDTH * HEIGHT) as usize];
    reader
        .read_band_into_typed(0, 0, &mut typed)
        .expect("read_band_into_typed");
    assert_bits_eq(&typed, &truth, "full typed read vs ground truth");

    let serial = serial_reference(&reader);
    assert_bits_eq(&serial, &truth, "serial reference vs ground truth");
    assert_bits_eq(&typed, &serial, "parallel-capable read vs serial reference");

    // And against the raw path the rustdoc used to send users to as a workaround.
    let mut raw = vec![0u8; reader.band_byte_len(0).expect("bytes")];
    reader
        .read_band_into(0, 0, &mut raw)
        .expect("read_band_into");
    let mut manual = vec![0.0f64; (WIDTH * HEIGHT) as usize];
    convert_raw_into(&raw, RasterDataType::Float32, &mut manual).expect("convert_raw_into");
    assert_bits_eq(&typed, &manual, "typed read vs read_band_into + convert");

    let _ = std::fs::remove_file(path);
}

/// The issue's actual file shape: DEFLATE with the floating-point predictor.
/// Decompression is the expensive part, so this is the case parallelism has to
/// get right.
#[test]
#[cfg(feature = "deflate")]
fn test_issue_14_typed_band_read_deflate_matches_ground_truth_and_serial() {
    let pattern = float_pattern(WIDTH, HEIGHT);
    let truth: Vec<f64> = pattern.iter().map(|v| *v as f64).collect();
    let path = write_float_fixture("deflate", &pattern, oxigeo_geotiff::Compression::Deflate);

    let reader = GeoTiffReader::open(FileDataSource::open(&path).expect("open")).expect("reader");

    let mut typed = vec![f64::NAN; (WIDTH * HEIGHT) as usize];
    reader
        .read_band_into_typed(0, 0, &mut typed)
        .expect("read_band_into_typed");
    assert_bits_eq(&typed, &truth, "DEFLATE typed read vs ground truth");

    let serial = serial_reference(&reader);
    assert_bits_eq(&serial, &truth, "DEFLATE serial reference vs ground truth");
    assert_bits_eq(&typed, &serial, "DEFLATE parallel-capable read vs serial");

    // A narrower destination type must saturate identically on every worker.
    let mut narrowed = vec![0i16; (WIDTH * HEIGHT) as usize];
    reader
        .read_band_into_typed(0, 0, &mut narrowed)
        .expect("read_band_into_typed::<i16>");
    let expected: Vec<i16> = truth
        .iter()
        // `f64::round` is half-away-from-zero, matching `convert_raw_into`.
        .map(|v| v.round().clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16)
        .collect();
    assert_eq!(narrowed, expected, "saturating f32 -> i16 across workers");
    assert!(
        narrowed.contains(&i16::MAX),
        "fixture must exercise the saturating clamp"
    );

    let _ = std::fs::remove_file(path);
}

/// A window large enough to be parallelised, deliberately misaligned to the tile
/// grid so the per-row scatter (not just the full-width fast path) runs on
/// workers.
#[test]
fn test_issue_14_typed_window_read_matches_ground_truth_and_serial() {
    let pattern = float_pattern(WIDTH, HEIGHT);
    let path = write_float_fixture("window", &pattern, oxigeo_geotiff::Compression::None);
    let reader = GeoTiffReader::open(FileDataSource::open(&path).expect("open")).expect("reader");

    let (wx, wy, ww, wh) = (13u64, 7u64, 499u64, 1000u64);
    assert!(
        (ww * wh * 4) as usize >= 1 << 20,
        "window must exceed the parallel threshold"
    );

    let truth: Vec<f64> = (0..wh)
        .flat_map(|row| {
            let base = ((wy + row) * WIDTH + wx) as usize;
            pattern[base..base + ww as usize]
                .iter()
                .map(|v| *v as f64)
                .collect::<Vec<_>>()
        })
        .collect();

    let mut typed = vec![f64::NAN; (ww * wh) as usize];
    reader
        .read_window_into_typed(0, 0, wx, wy, ww, wh, &mut typed)
        .expect("read_window_into_typed");
    assert_bits_eq(&typed, &truth, "typed window vs ground truth");

    // Serial reference: the same window taken one block row at a time.
    let mut serial = vec![f64::NAN; (ww * wh) as usize];
    let mut offset = 0usize;
    let mut y = wy;
    while y < wy + wh {
        // Stop at the next tile boundary so each call touches one block row.
        let next_boundary = (y / TILE + 1) * TILE;
        let rows = next_boundary.min(wy + wh) - y;
        let take = (ww * rows) as usize;
        let slice = serial
            .get_mut(offset..offset + take)
            .expect("reference slice in range");
        reader
            .read_window_into_typed(0, 0, wx, y, ww, rows, slice)
            .expect("single-block-row window read");
        offset += take;
        y += rows;
    }
    assert_bits_eq(&serial, &truth, "serial window reference vs ground truth");
    assert_bits_eq(&typed, &serial, "typed window vs serial reference");

    let _ = std::fs::remove_file(path);
}

/// Chunky multi-band: each worker must de-interleave into its own gather buffer.
/// A shared gather row would corrupt this test immediately.
#[test]
#[cfg(feature = "deflate")]
fn test_issue_14_typed_multiband_read_deinterleaves_per_worker() {
    let path = temp_test_file("multiband");
    let bands = 3u16;
    let mut interleaved = Vec::with_capacity((WIDTH * HEIGHT) as usize * bands as usize * 2);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            for band in 0..bands as u64 {
                let value = ((y * WIDTH + x) * 3 + band) as u16;
                interleaved.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
    {
        let config = WriterConfig::new(WIDTH, HEIGHT, bands, RasterDataType::UInt16)
            .with_compression(oxigeo_geotiff::Compression::Deflate)
            .with_tile_size(TILE as u32, TILE as u32)
            .with_overviews(false, OverviewResampling::Nearest);
        let mut writer =
            GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default()).expect("create");
        writer.write(&interleaved).expect("write raster");
    }

    let reader = GeoTiffReader::open(FileDataSource::open(&path).expect("open")).expect("reader");
    // One band is exactly 1 MiB here, so the parallel threshold is met.
    assert!(reader.band_byte_len(0).expect("bytes") >= 1 << 20);

    for band in 0..bands as usize {
        let truth: Vec<f64> = (0..(WIDTH * HEIGHT) as usize)
            .map(|i| ((i * 3 + band) as u16) as f64)
            .collect();
        let mut typed = vec![f64::NAN; (WIDTH * HEIGHT) as usize];
        reader
            .read_band_into_typed(0, band, &mut typed)
            .expect("read_band_into_typed");
        assert_bits_eq(&typed, &truth, "chunky multi-band typed read");
    }

    let _ = std::fs::remove_file(path);
}

/// Reads below the parallel threshold must keep working (and stay serial); this
/// guards the `should_parallelise` short-circuit from regressing into an
/// unconditional split.
#[test]
fn test_issue_14_small_typed_read_still_correct() {
    let pattern = float_pattern(WIDTH, HEIGHT);
    let path = write_float_fixture("small", &pattern, oxigeo_geotiff::Compression::None);
    let reader = GeoTiffReader::open(FileDataSource::open(&path).expect("open")).expect("reader");

    // 8x8 window: two orders of magnitude below the 1 MiB threshold.
    let mut typed = vec![f64::NAN; 64];
    reader
        .read_window_into_typed(0, 0, 250, 250, 8, 8, &mut typed)
        .expect("read_window_into_typed");
    for row in 0..8usize {
        for col in 0..8usize {
            let expected = pattern[(250 + row) * WIDTH as usize + 250 + col] as f64;
            assert_eq!(typed[row * 8 + col].to_bits(), expected.to_bits());
        }
    }

    let _ = std::fs::remove_file(path);
}

/// Evidence, not an assertion: times the *typed* whole-band read against a
/// serial reference **in the same process**, on a DEFLATE + floating-point
/// predictor `Float32` COG — the file shape issue #14 actually reports.
///
/// Comparing two separate `cargo test` runs (one with `parallel`, one without)
/// is only honest on an idle machine; running both halves back to back in one
/// process removes that variable entirely. The serial half decodes exactly the
/// same tiles through the same code, one block row per call, which is below the
/// driver's parallel threshold in every build.
///
/// Size it with `OXIGEO_ISSUE14_TYPED_MIB` (default 64 MiB) and run with
/// `--release --features parallel,deflate -- --nocapture`.
#[test]
#[cfg(feature = "deflate")]
fn test_issue_14_typed_parallel_speed_evidence() {
    use std::time::Instant;

    let target_mib: usize = std::env::var("OXIGEO_ISSUE14_TYPED_MIB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(64);
    let side =
        (((target_mib * 1024 * 1024 / 4) as f64).sqrt() as usize).next_multiple_of(256) as u64;
    let mib = (side * side * 4) as f64 / (1024.0 * 1024.0);

    let path = temp_test_file("typed_speed");
    let mut raw = Vec::with_capacity((side * side * 4) as usize);
    for y in 0..side {
        for x in 0..side {
            let value = (y as f32) * 0.5 + (x as f32) * 0.125 - 1000.0;
            raw.extend_from_slice(&value.to_le_bytes());
        }
    }
    {
        let config = WriterConfig::new(side, side, 1, RasterDataType::Float32)
            .with_compression(oxigeo_geotiff::Compression::Deflate)
            .with_predictor(oxigeo_geotiff::tiff::Predictor::FloatingPoint)
            .with_tile_size(TILE as u32, TILE as u32)
            .with_overviews(false, OverviewResampling::Nearest);
        let mut writer =
            GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default()).expect("create");
        writer.write(&raw).expect("write raster");
    }
    drop(raw);

    let reader = GeoTiffReader::open(FileDataSource::open(&path).expect("open")).expect("reader");
    let pixels = reader.band_pixel_count(0).expect("pixels");

    // Serial: one block row per call, so `should_parallelise` is always false.
    let mut serial = vec![0.0f64; pixels];
    let mut best_serial = f64::MAX;
    for _ in 0..3 {
        let start = Instant::now();
        let mut offset = 0usize;
        let mut y = 0u64;
        while y < side {
            let rows = TILE.min(side - y);
            let take = (side * rows) as usize;
            let slice = serial
                .get_mut(offset..offset + take)
                .expect("slice in range");
            reader
                .read_window_into_typed(0, 0, 0, y, side, rows, slice)
                .expect("block-row window read");
            offset += take;
            y += rows;
        }
        best_serial = best_serial.min(start.elapsed().as_secs_f64());
    }

    // Whole band: parallel when the feature is on.
    let mut whole = vec![0.0f64; pixels];
    let mut best_whole = f64::MAX;
    for _ in 0..3 {
        let start = Instant::now();
        reader
            .read_band_into_typed(0, 0, &mut whole)
            .expect("read_band_into_typed");
        best_whole = best_whole.min(start.elapsed().as_secs_f64());
    }

    // The measurement is only meaningful if both halves produced the same pixels.
    assert_bits_eq_slice(&whole, &serial, "speed evidence: whole vs block-row serial");

    eprintln!(
        "issue#14 TYPED band read {mib:.0} MiB f32 ({side}x{side}, 256px tiles, \
         DEFLATE+floatpred) -> Vec<f64>: block-row serial {:.2} ms ({:.0} MiB/s)  \
         read_band_into_typed {:.2} ms ({:.0} MiB/s)  speedup {:.2}x  (parallel={})",
        best_serial * 1e3,
        mib / best_serial,
        best_whole * 1e3,
        mib / best_whole,
        best_serial / best_whole,
        cfg!(feature = "parallel"),
    );

    let _ = std::fs::remove_file(path);
}

/// [`assert_bits_eq`] without the `WIDTH`-relative row/column hint, for fixtures
/// whose width is not the module constant.
#[cfg(feature = "deflate")]
fn assert_bits_eq_slice(actual: &[f64], expected: &[f64], label: &str) {
    assert_eq!(actual.len(), expected.len(), "{label}: length");
    for (i, (a, b)) in actual.iter().zip(expected.iter()).enumerate() {
        assert_eq!(a.to_bits(), b.to_bits(), "{label}: sample {i} ({a} vs {b})");
    }
}
