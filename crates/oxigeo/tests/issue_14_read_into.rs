//! cool-japan/oxigeo#14 — "what is the idiomatic way to read a GeoTIFF DEM into
//! an `Array2<f64>` as fast as possible?"
//!
//! The answer is [`oxigeo::Dataset::read_band_into`] (and its windowed sibling
//! [`oxigeo::Dataset::read_window_into`]): allocate the destination once, let
//! the driver decode straight into it and convert the element type in the same
//! pass.  This file pins that the new readers are *exactly* equivalent to the
//! workaround the reporter had to write — `read_band` → `as_bytes()` →
//! `bytemuck::cast_slice::<f32>()` → `mapv(|v| v as f64)` — and that the
//! clip-window and multi-band paths did not change their observable results
//! when they were rewired onto genuinely windowed / shared-reader I/O.
//!
//! Everything here uses only `oxigeo` itself: the "old workaround" side of each
//! comparison decodes the bytes by hand (`f32::from_ne_bytes`) rather than
//! pulling in `bytemuck`, so the test crate stays dependency-free.

#![cfg(feature = "geotiff")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::atomic::{AtomicU64, Ordering};

use oxigeo::geotiff::tiff::{Compression, PhotometricInterpretation, Predictor};
use oxigeo::geotiff::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};
use oxigeo::{BoundingBox, Dataset, GeoTransform, RasterDataType};

/// Pixel width/height of the DEM fixtures.  Deliberately *not* a multiple of the
/// 32×32 tile size or of the writer's 16-rows-per-strip, so every window test
/// straddles block boundaries and the edge blocks are partial.
const DEM_W: usize = 137;
const DEM_H: usize = 89;

/// A DEM sample with a fractional part and a sign, so an `f32 → f64` conversion
/// is a real conversion (and still exact, which is what makes byte-equality with
/// the old workaround the right assertion).
fn dem_sample(index: usize) -> f32 {
    (index as f32) * 0.5 - 3.25
}

/// Per-test scratch fixture inside the system temp dir (house policy: no
/// hardcoded absolute paths).
///
/// The leaf name embeds the process id and a monotonic counter, so no two test
/// binaries — nor two concurrent runs of this one — can ever land on the same
/// file.  Dropping the guard removes the fixture, so a panicking test leaks
/// nothing.
struct TempPath(std::path::PathBuf);

impl TempPath {
    fn new(name: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!(
            "oxigeo_issue14_{}_{seq}_{name}",
            std::process::id()
        )))
    }
}

impl std::ops::Deref for TempPath {
    type Target = std::path::Path;

    fn deref(&self) -> &std::path::Path {
        &self.0
    }
}

impl AsRef<std::path::Path> for TempPath {
    fn as_ref(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn temp_path(name: &str) -> TempPath {
    TempPath::new(name)
}

/// Writes a single-band `Float32` DEM, tiled or striped.
fn write_f32_dem(path: &std::path::Path, tiled: bool) {
    let mut bytes = Vec::with_capacity(DEM_W * DEM_H * 4);
    for index in 0..DEM_W * DEM_H {
        bytes.extend_from_slice(&dem_sample(index).to_ne_bytes());
    }

    let mut config = WriterConfig::new(DEM_W as u64, DEM_H as u64, 1, RasterDataType::Float32)
        .with_compression(Compression::None)
        .with_predictor(Predictor::None)
        .with_photometric(PhotometricInterpretation::BlackIsZero)
        .with_geo_transform(GeoTransform::north_up(100.0, 200.0, 2.0, 2.0));
    config.generate_overviews = false;
    if tiled {
        config.tile_width = Some(32);
        config.tile_height = Some(32);
    } else {
        config.tile_width = None;
        config.tile_height = None;
    }

    let mut writer = GeoTiffWriter::create(path, config, GeoTiffWriterOptions::default())
        .expect("create writer");
    writer.write(&bytes).expect("write DEM");
}

/// Writes a 4-band `UInt16` raster; band `b` of pixel `p` holds `p * 4 + b`.
fn write_4band_u16(path: &std::path::Path, width: usize, height: usize) {
    let mut bytes = Vec::with_capacity(width * height * 4 * 2);
    for pixel in 0..width * height {
        for band in 0..4u16 {
            let value = (pixel as u16).wrapping_mul(4).wrapping_add(band);
            bytes.extend_from_slice(&value.to_ne_bytes());
        }
    }

    let mut config = WriterConfig::new(width as u64, height as u64, 4, RasterDataType::UInt16)
        .with_compression(Compression::None)
        .with_predictor(Predictor::None)
        .with_photometric(PhotometricInterpretation::BlackIsZero)
        .with_geo_transform(GeoTransform::north_up(0.0, height as f64, 1.0, 1.0));
    config.generate_overviews = false;
    config.tile_width = Some(16);
    config.tile_height = Some(16);

    let mut writer = GeoTiffWriter::create(path, config, GeoTiffWriterOptions::default())
        .expect("create writer");
    writer.write(&bytes).expect("write bands");
}

/// The workaround from the issue, spelled out: raw band bytes → `&[f32]` →
/// `Vec<f64>`.  `bytemuck::cast_slice` is replaced by an explicit native-endian
/// decode, which is byte-for-byte what the cast would have produced.
fn old_workaround_f64(buffer: &oxigeo::core_types::buffer::RasterBuffer) -> Vec<f64> {
    buffer
        .as_bytes()
        .chunks_exact(4)
        .map(|chunk| {
            let mut word = [0u8; 4];
            word.copy_from_slice(chunk);
            f32::from_ne_bytes(word) as f64
        })
        .collect()
}

/// Crop a row-major `f64` grid — the "read everything, throw most away" shape
/// the windowed readers replaced.
fn crop(values: &[f64], full_width: usize, col: usize, row: usize, w: usize, h: usize) -> Vec<f64> {
    let mut out = Vec::with_capacity(w * h);
    for r in 0..h {
        let start = (row + r) * full_width + col;
        out.extend_from_slice(&values[start..start + w]);
    }
    out
}

// ---------------------------------------------------------------------------
// T2 — the API the issue asked for
// ---------------------------------------------------------------------------

/// `read_band_into::<f64>` on a `Float32` file must equal the old
/// `as_bytes` + `cast_slice` + `mapv(|v| v as f64)` result exactly.
#[test]
fn test_issue_14_read_band_into_f64_equals_old_workaround() {
    let path = temp_path("read_into_tiled.tif");
    write_f32_dem(&path, true);

    let ds = Dataset::open(path.to_str().expect("utf-8 path")).expect("open");
    assert_eq!(ds.width() as usize, DEM_W);
    assert_eq!(ds.height() as usize, DEM_H);
    assert_eq!(ds.data_type(), Some(RasterDataType::Float32));

    // Old way.
    let legacy = old_workaround_f64(&ds.read_band(0).expect("read_band"));

    // New way: one allocation, conversion fused into the decode.
    let mut fast = vec![0.0f64; DEM_W * DEM_H];
    ds.read_band_into(0, &mut fast).expect("read_band_into");

    assert_eq!(fast.len(), legacy.len());
    assert_eq!(
        fast, legacy,
        "fast path must be bit-identical to the workaround"
    );

    // And it really is the DEM.
    for (index, value) in fast.iter().enumerate() {
        assert!((*value - dem_sample(index) as f64).abs() < f64::EPSILON);
    }
}

/// The same equivalence on a striped file, where the block geometry is entirely
/// different (16-row strips instead of 32×32 tiles).
#[test]
fn test_issue_14_read_band_into_f64_equals_old_workaround_striped() {
    let path = temp_path("read_into_striped.tif");
    write_f32_dem(&path, false);

    let ds = Dataset::open(path.to_str().expect("utf-8 path")).expect("open");
    let legacy = old_workaround_f64(&ds.read_band(0).expect("read_band"));

    let mut fast = vec![0.0f64; DEM_W * DEM_H];
    ds.read_band_into(0, &mut fast).expect("read_band_into");
    assert_eq!(fast, legacy);
}

/// A wrong-length destination is an error that names the expected length — never
/// a truncated read, never a panic.
#[test]
fn test_issue_14_read_band_into_wrong_length_errors() {
    let path = temp_path("wrong_len.tif");
    write_f32_dem(&path, true);
    let ds = Dataset::open(path.to_str().expect("utf-8 path")).expect("open");

    let expected = DEM_W * DEM_H;

    let mut short = vec![0.0f64; expected - 1];
    let err = ds
        .read_band_into(0, &mut short)
        .expect_err("short destination must error");
    let message = err.to_string();
    assert!(
        message.contains(&expected.to_string()),
        "error must name the expected length {expected}: {message}"
    );
    assert!(
        short.iter().all(|v| *v == 0.0),
        "a rejected read must not have written anything"
    );

    let mut long = vec![0.0f64; expected + 1];
    assert!(
        ds.read_band_into(0, &mut long).is_err(),
        "over-long destination must error too"
    );

    // Windowed reader validates the same way.
    let mut wrong = vec![0.0f64; 5];
    let err = ds
        .read_window_into(0, 0, 0, 4, 4, &mut wrong)
        .expect_err("wrong window length must error");
    assert!(err.to_string().contains("16"), "{err}");
}

/// Degenerate and out-of-range windows are rejected before any I/O.
#[test]
fn test_issue_14_read_window_into_rejects_bad_windows() {
    let path = temp_path("bad_windows.tif");
    write_f32_dem(&path, true);
    let ds = Dataset::open(path.to_str().expect("utf-8 path")).expect("open");

    let mut empty: Vec<f64> = Vec::new();
    assert!(ds.read_window_into(0, 0, 0, 0, 4, &mut empty).is_err());

    let mut past = vec![0.0f64; 16];
    assert!(
        ds.read_window_into(0, DEM_W as u32 - 2, 0, 4, 4, &mut past)
            .is_err(),
        "window running past the right edge must error"
    );

    let mut wrong_band = vec![0.0f64; 4];
    assert!(ds.read_window_into(7, 0, 0, 2, 2, &mut wrong_band).is_err());
    assert!(ds.read_band_into(7, &mut wrong_band).is_err());
}

// ---------------------------------------------------------------------------
// T1 — the windowed readers must equal a crop of the full read
// ---------------------------------------------------------------------------

/// Every window — tile-aligned, straddling a block boundary, clipped to the
/// ragged right/bottom edge — must return exactly the crop of the full band.
#[test]
fn test_issue_14_window_equals_crop_of_full_read_tiled() {
    let path = temp_path("window_tiled.tif");
    write_f32_dem(&path, true);
    let ds = Dataset::open(path.to_str().expect("utf-8 path")).expect("open");

    let full = old_workaround_f64(&ds.read_band(0).expect("read_band"));
    assert_window_equivalence(&ds, &full);
}

/// The same, on a striped file: block geometry must not change the answer.
#[test]
fn test_issue_14_window_equals_crop_of_full_read_striped() {
    let path = temp_path("window_striped.tif");
    write_f32_dem(&path, false);
    let ds = Dataset::open(path.to_str().expect("utf-8 path")).expect("open");

    let full = old_workaround_f64(&ds.read_band(0).expect("read_band"));
    assert_window_equivalence(&ds, &full);
}

/// Shared body of the two window tests.
fn assert_window_equivalence(ds: &Dataset, full: &[f64]) {
    // (col, row, width, height): tile-aligned, straddling one boundary,
    // straddling four, a single pixel, a whole row band, and the ragged corner.
    let windows: &[(u32, u32, u32, u32)] = &[
        (0, 0, 32, 32),
        (30, 30, 5, 5),
        (31, 31, 34, 34),
        (100, 70, 1, 1),
        (0, 40, DEM_W as u32, 3),
        (DEM_W as u32 - 9, DEM_H as u32 - 5, 9, 5),
    ];

    for &(col, row, w, h) in windows {
        let expected = crop(
            full,
            DEM_W,
            col as usize,
            row as usize,
            w as usize,
            h as usize,
        );

        // Buffer-returning windowed read.
        let buffer = ds
            .read_window(0, col, row, w, h)
            .unwrap_or_else(|e| panic!("read_window {col},{row} {w}×{h}: {e}"));
        assert_eq!(buffer.width(), u64::from(w));
        assert_eq!(buffer.height(), u64::from(h));
        assert_eq!(
            old_workaround_f64(&buffer),
            expected,
            "read_window {col},{row} {w}×{h}"
        );

        // Read-into-caller-buffer windowed read, with conversion.
        let mut dst = vec![0.0f64; (w * h) as usize];
        ds.read_window_into(0, col, row, w, h, &mut dst)
            .unwrap_or_else(|e| panic!("read_window_into {col},{row} {w}×{h}: {e}"));
        assert_eq!(dst, expected, "read_window_into {col},{row} {w}×{h}");
    }
}

// ---------------------------------------------------------------------------
// T1 / T2 — clip semantics
// ---------------------------------------------------------------------------

/// Build the clip bbox covering pixel window `(col, row, w, h)` of `ds`.
fn bbox_for_pixels(ds: &Dataset, col: f64, row: f64, w: f64, h: f64) -> BoundingBox {
    let gt = ds.geotransform().copied().expect("geotransform");
    let (x0, y0) = gt.pixel_to_world(col, row);
    let (x1, y1) = gt.pixel_to_world(col + w, row + h);
    BoundingBox::new(x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)).expect("bbox")
}

/// A clipped dataset now performs a windowed read instead of reading the whole
/// band and cropping.  The pixels must be identical to what the old
/// read-then-crop produced.
#[test]
fn test_issue_14_clipped_read_equals_read_then_crop() {
    let path = temp_path("clip.tif");
    write_f32_dem(&path, true);
    let ds = Dataset::open(path.to_str().expect("utf-8 path")).expect("open");

    // The pre-change behaviour, reproduced explicitly.
    let full = old_workaround_f64(&ds.read_band(0).expect("read_band"));

    for &(col, row, w, h) in &[(1u32, 1u32, 2u32, 2u32), (33, 17, 40, 30), (0, 0, 137, 89)] {
        let bbox = bbox_for_pixels(
            &ds,
            f64::from(col),
            f64::from(row),
            f64::from(w),
            f64::from(h),
        );
        let clipped = ds.clip(bbox).expect("clip");
        assert_eq!(clipped.width(), w);
        assert_eq!(clipped.height(), h);

        let expected = crop(
            &full,
            DEM_W,
            col as usize,
            row as usize,
            w as usize,
            h as usize,
        );

        // `read_band` on the clipped dataset == crop of the full read.
        let buffer = clipped.read_band(0).expect("clipped read_band");
        assert_eq!(buffer.width(), u64::from(w));
        assert_eq!(buffer.height(), u64::from(h));
        assert_eq!(old_workaround_f64(&buffer), expected);

        // `read_band_into` follows the documented clip semantics: `dst` is sized
        // by the dataset's *current* extent.
        let mut dst = vec![0.0f64; (w * h) as usize];
        clipped
            .read_band_into(0, &mut dst)
            .expect("clipped read_band_into");
        assert_eq!(dst, expected);

        // The full-file length is the wrong length for a clipped dataset.
        if (w as usize * h as usize) != DEM_W * DEM_H {
            let mut whole = vec![0.0f64; DEM_W * DEM_H];
            assert!(
                clipped.read_band_into(0, &mut whole).is_err(),
                "clipped dataset must reject a full-file-sized destination"
            );
        }

        // Statistics ride on the same reader and must agree.
        let stats = clipped.statistics(0).expect("clipped statistics");
        assert_eq!(stats.valid_count, u64::from(w) * u64::from(h));
    }
}

/// Windows of a clipped dataset are relative to the clipped grid, and clipping
/// composes.
#[test]
fn test_issue_14_clipped_window_is_relative_to_clip() {
    let path = temp_path("clip_window.tif");
    write_f32_dem(&path, true);
    let ds = Dataset::open(path.to_str().expect("utf-8 path")).expect("open");
    let full = old_workaround_f64(&ds.read_band(0).expect("read_band"));

    let clipped = ds
        .clip(bbox_for_pixels(&ds, 20.0, 10.0, 50.0, 40.0))
        .expect("clip");

    // Window (5,7 12×9) inside the clip == window (25,17 12×9) of the file.
    let mut inner = vec![0.0f64; 12 * 9];
    clipped
        .read_window_into(0, 5, 7, 12, 9, &mut inner)
        .expect("clipped window");
    assert_eq!(inner, crop(&full, DEM_W, 25, 17, 12, 9));

    // A window that fits the file but not the clip is rejected.
    let mut past = vec![0.0f64; 100 * 10];
    assert!(
        clipped
            .read_window_into(0, 0, 0, 100, 10, &mut past)
            .is_err()
    );

    // Chained clips compose into a single source window.
    let twice = clipped
        .clip(bbox_for_pixels(&clipped, 5.0, 7.0, 12.0, 9.0))
        .expect("second clip");
    let mut chained = vec![0.0f64; 12 * 9];
    twice
        .read_band_into(0, &mut chained)
        .expect("chained clip read");
    assert_eq!(chained, crop(&full, DEM_W, 25, 17, 12, 9));
}

// ---------------------------------------------------------------------------
// T3 — `bands()` opens the file once
// ---------------------------------------------------------------------------

/// Every band the iterator yields must equal the corresponding `read_band`, and
/// the iterator's advertised length must stay exact.
#[test]
fn test_issue_14_bands_yield_correct_per_band_data() {
    let path = temp_path("bands_4.tif");
    write_4band_u16(&path, 40, 24);
    let ds = Dataset::open(path.to_str().expect("utf-8 path")).expect("open");
    assert_eq!(ds.band_count(), 4);

    let mut iter = ds.bands();
    assert_eq!(iter.len(), 4);

    let mut seen = 0usize;
    for (index, band) in ds.bands().enumerate() {
        let buffer = band.expect("band read");
        let direct = ds.read_band(index as u32).expect("read_band");
        assert_eq!(buffer.as_bytes(), direct.as_bytes(), "band {index}");
        assert_eq!(buffer.width(), 40);
        assert_eq!(buffer.height(), 24);

        // Band b of pixel p holds p * 4 + b.
        let first = u16::from_ne_bytes([buffer.as_bytes()[0], buffer.as_bytes()[1]]);
        assert_eq!(first, index as u16, "band {index} first sample");
        seen += 1;
    }
    assert_eq!(seen, 4);

    // Consuming one item decrements the exact length.
    let _ = iter.next();
    assert_eq!(iter.len(), 3);
}

/// The iterator holds ONE open reader for its whole lifetime.
///
/// Proof without instrumenting the driver: on Unix an unlinked file stays fully
/// readable through an already-open descriptor, but can no longer be opened by
/// path.  So if the remaining bands still read correctly after the file is
/// deleted mid-iteration, the iterator cannot be re-opening it per band — which
/// is exactly the N-opens/N-IFD-parses/N-decodes behaviour this replaced.
#[cfg(unix)]
#[test]
fn test_issue_14_bands_opens_the_file_once() {
    let path = temp_path("bands_single_open.tif");
    write_4band_u16(&path, 32, 16);
    let ds = Dataset::open(path.to_str().expect("utf-8 path")).expect("open");

    let expected: Vec<Vec<u8>> = (0..4)
        .map(|b| ds.read_band(b).expect("read_band").as_bytes().to_vec())
        .collect();

    let mut iter = ds.bands();
    let first = iter.next().expect("band 0").expect("band 0 read");
    assert_eq!(first.as_bytes(), expected[0].as_slice());

    // Unlink the file: any further `open()` by path would now fail.
    std::fs::remove_file(&path).expect("remove file");
    assert!(!path.exists());
    assert!(
        Dataset::open(path.to_str().expect("utf-8 path")).is_err(),
        "sanity: the path is really gone"
    );

    for (index, band) in iter.enumerate() {
        let buffer = band.unwrap_or_else(|e| {
            panic!(
                "band {} failed after unlink — the file was re-opened per band: {e}",
                index + 1
            )
        });
        assert_eq!(buffer.as_bytes(), expected[index + 1].as_slice());
    }
}

/// `bands()` on a clipped dataset yields the clipped region of every band.
#[test]
fn test_issue_14_bands_honour_clip() {
    let path = temp_path("bands_clip.tif");
    write_4band_u16(&path, 40, 24);
    let ds = Dataset::open(path.to_str().expect("utf-8 path")).expect("open");

    let clipped = ds
        .clip(bbox_for_pixels(&ds, 4.0, 3.0, 10.0, 6.0))
        .expect("clip");

    let mut count = 0;
    for (index, band) in clipped.bands().enumerate() {
        let buffer = band.expect("clipped band");
        assert_eq!(buffer.width(), 10, "band {index} width");
        assert_eq!(buffer.height(), 6, "band {index} height");
        assert_eq!(
            buffer.as_bytes(),
            clipped
                .read_band(index as u32)
                .expect("read_band")
                .as_bytes()
        );
        count += 1;
    }
    assert_eq!(count, 4);
}
