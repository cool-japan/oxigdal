//! Regression tests for <https://github.com/cool-japan/oxigeo/issues/14>.
//!
//! `Dataset::read_window` used to stitch tiles together via
//! `Dataset::read_tile_buffer`. A `RasterBuffer` cannot represent a chunky
//! multi-band tile, so for every dataset with `band_count > 1` each tile read
//! failed and the failure was swallowed -- the caller got a silently all-zero
//! buffer and no error. The WMS/WMTS/XYZ RGB renderers took the red channel
//! from `read_window` and green/blue from a per-band helper, so the rendered
//! images had a dead red channel.
//!
//! `read_window` now delegates to `Dataset::read_band_window`, which is built
//! on `GeoTiffReader::read_window` and returns one de-interleaved band plane.

#![allow(clippy::expect_used)]
#![allow(clippy::panic)]

use oxigeo_core::types::{GeoTransform, RasterDataType};
use oxigeo_geotiff::tiff::{Compression, PhotometricInterpretation, Predictor};
use oxigeo_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};
use oxigeo_server::dataset_registry::Dataset;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Fixture dimensions. 8x8 tiles over a 16x16 raster means every interesting
/// window straddles a tile boundary.
const FIXTURE_WIDTH: u64 = 16;
const FIXTURE_HEIGHT: u64 = 16;
const FIXTURE_BANDS: usize = 3;
const FIXTURE_TILE: u32 = 8;

/// Per-band, per-pixel sample value of the fixture.
///
/// The band offset (`band * 64`) makes the three planes trivially
/// distinguishable, and the `+ 1` guarantees no sample is ever zero, so an
/// all-zero buffer can never be mistaken for correct data.
fn expected_sample(band: usize, x: u64, y: u64) -> u8 {
    let spatial = (y * FIXTURE_WIDTH + x) % 61;
    (band as u64 * 64 + spatial + 1) as u8
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
        Self(env::temp_dir().join(format!(
            "oxigeo_issue14_server_{}_{seq}_{name}",
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

/// Writes the shared 16x16 3-band tiled UInt8 fixture and returns its guard.
fn write_multiband_fixture(file_name: &str) -> TempPath {
    let path = TempPath::new(file_name);

    let gt = GeoTransform {
        origin_x: 0.0,
        origin_y: FIXTURE_HEIGHT as f64,
        pixel_width: 1.0,
        pixel_height: -1.0,
        row_rotation: 0.0,
        col_rotation: 0.0,
    };

    // Chunky (band-interleaved-by-pixel) source data, which is what the
    // GeoTIFF writer expects.
    let mut data = Vec::with_capacity((FIXTURE_WIDTH * FIXTURE_HEIGHT) as usize * FIXTURE_BANDS);
    for y in 0..FIXTURE_HEIGHT {
        for x in 0..FIXTURE_WIDTH {
            for band in 0..FIXTURE_BANDS {
                data.push(expected_sample(band, x, y));
            }
        }
    }

    let config = WriterConfig::new(
        FIXTURE_WIDTH,
        FIXTURE_HEIGHT,
        FIXTURE_BANDS as u16,
        RasterDataType::UInt8,
    )
    .with_compression(Compression::None)
    .with_predictor(Predictor::None)
    .with_tile_size(FIXTURE_TILE, FIXTURE_TILE)
    .with_photometric(PhotometricInterpretation::Rgb)
    .with_geo_transform(gt)
    .with_overviews(false, oxigeo_geotiff::OverviewResampling::Average);

    let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
        .expect("create multiband fixture writer");
    writer.write(&data).expect("write multiband fixture data");
    drop(writer);

    path
}

#[test]
fn test_issue_14_read_window_multiband_returns_band_zero_not_zeros() {
    let path = write_multiband_fixture("read_window.tif");
    let dataset = Dataset::open(&path).expect("open multiband fixture as Dataset");
    assert_eq!(
        dataset.raster_count(),
        FIXTURE_BANDS,
        "fixture must be multi-band for this regression to be meaningful"
    );

    // Deliberately a sub-rectangle, not the whole image, and one that straddles
    // the 8x8 tile boundary in both axes.
    let (x_off, y_off, w, h) = (5u64, 3u64, 6u64, 7u64);
    let buffer = dataset
        .read_window(x_off, y_off, w, h)
        .expect("read_window over a multi-band dataset must not fail");

    assert_eq!(
        buffer.width(),
        w,
        "window buffer width: expected {}, got {}",
        w,
        buffer.width()
    );
    assert_eq!(
        buffer.height(),
        h,
        "window buffer height: expected {}, got {}",
        h,
        buffer.height()
    );

    let bytes = buffer.as_bytes();
    assert!(
        bytes.iter().any(|&b| b != 0),
        "band 0 window ({}, {}) {}x{} came back entirely zero -- this is the \
         issue #14 failure mode (every tile read failed silently)",
        x_off,
        y_off,
        w,
        h
    );

    for row in 0..h {
        for col in 0..w {
            let expected = expected_sample(0, x_off + col, y_off + row);
            let actual = bytes[(row * w + col) as usize];
            assert_eq!(
                actual,
                expected,
                "band 0 pixel ({}, {}) [window-local ({}, {})]: expected {}, got {}",
                x_off + col,
                y_off + row,
                col,
                row,
                expected,
                actual
            );
        }
    }
}

#[test]
fn test_issue_14_read_band_window_selects_requested_band() {
    let path = write_multiband_fixture("band_window.tif");
    let dataset = Dataset::open(&path).expect("open multiband fixture as Dataset");

    let (x_off, y_off, w, h) = (5u64, 3u64, 6u64, 7u64);

    for band in 0..FIXTURE_BANDS {
        let buffer = dataset
            .read_band_window(0, band, x_off, y_off, w, h)
            .unwrap_or_else(|e| panic!("read_band_window for band {} failed: {}", band, e));

        assert_eq!(
            buffer.width(),
            w,
            "band {} window buffer width: expected {}, got {}",
            band,
            w,
            buffer.width()
        );
        assert_eq!(
            buffer.height(),
            h,
            "band {} window buffer height: expected {}, got {}",
            band,
            h,
            buffer.height()
        );

        let bytes = buffer.as_bytes();
        for row in 0..h {
            for col in 0..w {
                let expected = expected_sample(band, x_off + col, y_off + row);
                let actual = bytes[(row * w + col) as usize];
                assert_eq!(
                    actual,
                    expected,
                    "band {} pixel ({}, {}) [window-local ({}, {})]: expected {}, got {}",
                    band,
                    x_off + col,
                    y_off + row,
                    col,
                    row,
                    expected,
                    actual
                );
            }
        }
    }

    // The renderers read red from `read_window` and green/blue from the
    // per-band helper; those two paths must agree on band 0 or RGB output
    // desynchronises again.
    let via_read_window = dataset
        .read_window(x_off, y_off, w, h)
        .expect("read_window band 0");
    let via_band_window = dataset
        .read_band_window(0, 0, x_off, y_off, w, h)
        .expect("read_band_window band 0");
    assert_eq!(
        via_read_window.as_bytes(),
        via_band_window.as_bytes(),
        "read_window must be exactly read_band_window(level 0, band 0)"
    );
}

#[test]
fn test_issue_14_read_band_window_overhang_is_zero_padded() {
    let path = write_multiband_fixture("overhang.tif");
    let dataset = Dataset::open(&path).expect("open multiband fixture as Dataset");

    // 8x8 window anchored at (12, 12) on a 16x16 raster: only the top-left
    // 4x4 quadrant overlaps the raster, the rest overhangs right and bottom.
    let (x_off, y_off, w, h) = (12u64, 12u64, 8u64, 8u64);
    let overlap_w = FIXTURE_WIDTH - x_off;
    let overlap_h = FIXTURE_HEIGHT - y_off;

    for band in 0..FIXTURE_BANDS {
        let buffer = dataset
            .read_band_window(0, band, x_off, y_off, w, h)
            .unwrap_or_else(|e| panic!("overhanging read_band_window band {} failed: {}", band, e));

        assert_eq!(
            buffer.width(),
            w,
            "band {} overhanging window must still be full width: expected {}, got {}",
            band,
            w,
            buffer.width()
        );
        assert_eq!(
            buffer.height(),
            h,
            "band {} overhanging window must still be full height: expected {}, got {}",
            band,
            h,
            buffer.height()
        );

        let bytes = buffer.as_bytes();
        for row in 0..h {
            for col in 0..w {
                let in_raster = col < overlap_w && row < overlap_h;
                let expected = if in_raster {
                    expected_sample(band, x_off + col, y_off + row)
                } else {
                    0
                };
                let actual = bytes[(row * w + col) as usize];
                assert_eq!(
                    actual,
                    expected,
                    "band {} window-local pixel ({}, {}) ({}): expected {}, got {}",
                    band,
                    col,
                    row,
                    if in_raster {
                        "inside raster"
                    } else {
                        "overhang, must be zero-padded"
                    },
                    expected,
                    actual
                );
            }
        }
    }

    // An offset fully outside the raster is an error, not a zero buffer.
    let outside = dataset.read_band_window(0, 0, FIXTURE_WIDTH, 0, 4, 4);
    assert!(
        outside.is_err(),
        "a window whose origin is outside the raster must be rejected, got Ok"
    );
}
