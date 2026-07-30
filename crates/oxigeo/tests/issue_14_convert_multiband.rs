//! Regression tests for cool-japan/oxigeo#14 — multi-band `Dataset::convert`.
//!
//! `GeoTiffReader::read_band(level, band)` used to ignore its `band` argument
//! and hand back the whole pixel-interleaved image, and `Dataset::convert`
//! leaned on that: it called `read_band(0, 0)` exactly once and fed the result
//! straight to the writer.  Once `read_band` was fixed to return a single
//! de-interleaved band plane, that call started returning `1 / band_count` of
//! the pixels — a length mismatch on a good day, and silent per-band corruption
//! on a bad one.
//!
//! These tests pin the contract that matters to a user: after a round-trip
//! through `convert`, **band `b` still holds band `b`'s values**.  Asserting on
//! the total byte count is not enough — a stride or byte-order slip in the
//! re-interleave keeps the length correct while scrambling which band each
//! sample belongs to.

#![cfg(feature = "geotiff")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::atomic::{AtomicU64, Ordering};

use oxigeo::core_types::types::{GeoTransform, NoDataValue, RasterDataType};
use oxigeo::geotiff::{
    GeoTiffWriter, GeoTiffWriterOptions, OverviewResampling, WriterConfig,
    tiff::{Compression, PhotometricInterpretation, Predictor},
};
use oxigeo::{ConversionOptions, Dataset, DatasetFormat};

const WIDTH: u32 = 4;
const HEIGHT: u32 = 3;

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

/// Value stored at `band`, linear pixel index `pixel`.
///
/// Bands are 1000 apart so their high bytes differ (1000 = `0x03E8`,
/// 2000 = `0x07D0`, 3000 = `0x0BB8`); pixels differ within a band.  Any
/// cross-band mix-up, half-sample shift, or endianness flip therefore changes
/// the decoded number rather than cancelling out.
fn expected_u16(band: u32, pixel: u32) -> u16 {
    let value = (band + 1) * 1000 + pixel;
    u16::try_from(value).expect("test values fit in u16")
}

/// Write a `WIDTH × HEIGHT`, `band_count`-band UInt16 GeoTIFF whose samples are
/// chunky (pixel-interleaved) little-endian, matching what the writer consumes.
fn write_multiband_u16_source(name: &str, band_count: u16) -> TempPath {
    let path = TempPath::new(name);
    let pixel_count = WIDTH * HEIGHT;

    let mut chunky: Vec<u8> =
        Vec::with_capacity(pixel_count as usize * usize::from(band_count) * 2);
    for pixel in 0..pixel_count {
        for band in 0..u32::from(band_count) {
            chunky.extend_from_slice(&expected_u16(band, pixel).to_le_bytes());
        }
    }

    let config = WriterConfig {
        width: u64::from(WIDTH),
        height: u64::from(HEIGHT),
        band_count,
        data_type: RasterDataType::UInt16,
        compression: Compression::None,
        predictor: Predictor::None,
        tile_width: None,
        tile_height: None,
        photometric: PhotometricInterpretation::BlackIsZero,
        geo_transform: Some(GeoTransform::north_up(10.0, 50.0, 1.0, -1.0)),
        epsg_code: Some(4326),
        nodata: NoDataValue::None,
        use_bigtiff: false,
        generate_overviews: false,
        overview_resampling: OverviewResampling::Average,
        overview_levels: vec![],
    };

    let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
        .expect("create source TIFF");
    writer.write(&chunky).expect("write source pixels");
    path
}

/// Assert that every band of `dataset` decodes to the values `expected_u16`
/// prescribes — i.e. no band picked up another band's samples.
fn assert_per_band_values(dataset: &Dataset, band_count: u32, label: &str) {
    for band in 0..band_count {
        let buf = dataset
            .read_band(band)
            .unwrap_or_else(|e| panic!("{label}: read band {band}: {e}"));
        assert_eq!(buf.width(), u64::from(WIDTH), "{label}: band {band} width");
        assert_eq!(
            buf.height(),
            u64::from(HEIGHT),
            "{label}: band {band} height"
        );
        for y in 0..u64::from(HEIGHT) {
            for x in 0..u64::from(WIDTH) {
                let pixel = u32::try_from(y * u64::from(WIDTH) + x).expect("pixel index");
                let got = buf
                    .get_u16(x, y)
                    .unwrap_or_else(|e| panic!("{label}: band {band} pixel ({x},{y}): {e}"));
                assert_eq!(
                    got,
                    expected_u16(band, pixel),
                    "{label}: band {band} pixel ({x},{y}) carries the wrong value \
                     — bands were interleaved incorrectly"
                );
            }
        }
    }
}

/// A 3-band UInt16 raster survives `Dataset::convert` with each band's values
/// intact.
///
/// This is the test the old `read_band(0, 0)` shortcut cannot pass: with one
/// plane read and three bands declared, the writer either rejects the buffer or
/// writes one band's pixels across all three.
#[test]
fn test_issue_14_convert_multiband_preserves_per_band_values() {
    let src_path = write_multiband_u16_source("convert_mb_u16_src.tif", 3);
    let dst_path = TempPath::new("convert_mb_u16_dst.tif");

    let src = Dataset::open(src_path.to_str().expect("utf-8 path")).expect("open source");
    assert_eq!(src.band_count(), 3, "source should declare 3 bands");
    assert_eq!(
        src.data_type(),
        Some(RasterDataType::UInt16),
        "source should declare UInt16 samples"
    );

    // Guard the fixture itself: if the source does not already hold the values
    // this test expects, a later mismatch would be the fixture's fault, not
    // `convert`'s.
    assert_per_band_values(&src, 3, "source");

    let dst = src
        .convert(
            &dst_path,
            DatasetFormat::GeoTiff,
            ConversionOptions::default(),
        )
        .expect("3-band UInt16 GeoTIFF→GeoTIFF conversion should succeed");

    assert_eq!(dst.format(), DatasetFormat::GeoTiff);
    assert_eq!(dst.band_count(), 3, "conversion should preserve band count");
    assert_eq!(dst.width(), WIDTH);
    assert_eq!(dst.height(), HEIGHT);
    assert_eq!(
        dst.data_type(),
        Some(RasterDataType::UInt16),
        "conversion should preserve the sample type"
    );

    assert_per_band_values(&dst, 3, "converted");
}

/// The same property with two bands, to cover an even band count (an off-by-one
/// in the destination stride shows up differently for 2 bands than for 3).
#[test]
fn test_issue_14_convert_two_band_preserves_per_band_values() {
    let src_path = write_multiband_u16_source("convert_mb_u16_2b_src.tif", 2);
    let dst_path = TempPath::new("convert_mb_u16_2b_dst.tif");

    let src = Dataset::open(src_path.to_str().expect("utf-8 path")).expect("open source");
    let dst = src
        .convert(
            &dst_path,
            DatasetFormat::GeoTiff,
            ConversionOptions::default(),
        )
        .expect("2-band conversion should succeed");

    assert_eq!(dst.band_count(), 2);
    assert_per_band_values(&dst, 2, "converted-2band");
}

/// Single-band conversion stays a straight move through the reader's plane —
/// the fast path must not regress while the multi-band path is being fixed.
#[test]
fn test_issue_14_convert_single_band_still_round_trips() {
    let src_path = write_multiband_u16_source("convert_mb_u16_1b_src.tif", 1);
    let dst_path = TempPath::new("convert_mb_u16_1b_dst.tif");

    let src = Dataset::open(src_path.to_str().expect("utf-8 path")).expect("open source");
    let dst = src
        .convert(
            &dst_path,
            DatasetFormat::GeoTiff,
            ConversionOptions::default(),
        )
        .expect("single-band conversion should succeed");

    assert_eq!(dst.band_count(), 1);
    assert_per_band_values(&dst, 1, "converted-1band");
}
