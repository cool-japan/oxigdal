//! Regression tests for cool-japan/oxigeo#14 — facade metadata.
//!
//! Issue #14 reports that this straightforward migration from the C-GDAL
//! wrapper is both slow and fragile:
//!
//! ```ignore
//! let dataset = oxigeo::Dataset::open(path)?;
//! let width = dataset.width() as usize;
//! let height = dataset.height() as usize;
//! let band = dataset.bands().next().ok_or_else(|| anyhow!("Dataset has no bands"))??;
//! ```
//!
//! Two facade-level defects broke exactly this idiom:
//!
//! 1. `Dataset::open` probed only the first **8 KiB** of the file and
//!    hand-parsed an IFD out of that window.  TIFF puts no such constraint on
//!    writers: any GeoTIFF that stores its IFD after the pixel data — which is
//!    *every* file OxiGeo's own `GeoTiffWriter` produces — was reported as
//!    `0×0` with `band_count = 0`, with **no error raised anywhere**.  The
//!    caller's own `ok_or("Dataset has no bands")` then fired with a nonsense
//!    diagnosis, and `width()`/`height()` silently returned `0`.
//! 2. `DatasetInfo` carried no `data_type`, so the only way to learn a
//!    raster's element type was to read an entire band and ask the resulting
//!    `RasterBuffer` — absurd when the type is needed to *size* the
//!    destination buffer before reading.
//!
//! These tests pin both fixes, plus the "an unparseable file must be an error,
//! never a zero-filled descriptor" rule.

#![cfg(feature = "geotiff")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use oxigeo::{Dataset, DatasetFormat, GeoTransform, OxiGeoError, RasterDataType};

/// Per-test scratch fixture under the platform temp dir (house policy: no
/// hardcoded absolute paths, always `std::env::temp_dir()`).
///
/// The leaf name embeds the process id and a monotonic counter, so no two test
/// binaries — nor two concurrent runs of this one, nor two developers on a
/// shared machine — can ever land on the same file.  Dropping the guard removes
/// the fixture, which keeps cleanup honest even when a test panics.
struct TempPath(PathBuf);

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

fn temp_path(name: &str) -> TempPath {
    TempPath::new(name)
}

/// Offset of the first IFD, read straight out of the TIFF header.
///
/// Classic TIFF stores it as a `u32` at byte 4; the byte order is given by the
/// `II` / `MM` marker at byte 0.
fn first_ifd_offset(path: &Path) -> u64 {
    let bytes = std::fs::read(path).expect("read tiff header");
    assert!(bytes.len() >= 8, "file is too short to be a TIFF");
    let le = bytes[0] == 0x49;
    let raw = [bytes[4], bytes[5], bytes[6], bytes[7]];
    u64::from(if le {
        u32::from_le_bytes(raw)
    } else {
        u32::from_be_bytes(raw)
    })
}

/// Write a real GeoTIFF with OxiGeo's own writer.
///
/// The writer emits the header, then all pixel data (and overviews), and only
/// then the IFDs — so for anything but a toy raster the IFD lands far beyond
/// the 8 KiB window the old facade probe could see.
fn write_geotiff(
    path: &Path,
    width: u32,
    height: u32,
    band_count: u16,
    data_type: RasterDataType,
    epsg: Option<u32>,
) {
    use oxigeo::geotiff::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};

    let mut config = WriterConfig::new(u64::from(width), u64::from(height), band_count, data_type)
        .with_geo_transform(GeoTransform::north_up(100.0, 200.0, 0.5, -0.5));
    config.epsg_code = epsg;

    let mut writer = GeoTiffWriter::create(path, config, GeoTiffWriterOptions::default())
        .expect("create GeoTiffWriter");

    let pixels = width as usize * height as usize * band_count as usize;
    let mut data = vec![0u8; pixels * data_type.size_bytes()];
    for (i, chunk) in data.chunks_exact_mut(data_type.size_bytes()).enumerate() {
        // A deterministic, non-constant pattern so a wrong dtype or a wrong
        // stride would be visible if a later test compares pixels.
        let byte = (i % 251) as u8;
        chunk[0] = byte;
    }
    writer.write(&data).expect("write raster data");
}

// ─── Bug 1: the 8 KiB probe window ───────────────────────────────────────────

/// `Dataset::open` must report the real geometry of a GeoTIFF whose IFD sits
/// beyond byte 8192 — the exact shape of the file in issue #14.
///
/// Before the fix this reported `0×0`, `band_count = 0`, `crs = None`,
/// `geotransform = None` and returned `Ok`.
#[test]
fn test_issue_14_open_reports_real_dimensions_for_trailing_ifd() {
    let path = temp_path("trailing_ifd.tif");
    let (width, height) = (512u32, 384u32);
    write_geotiff(
        &path,
        width,
        height,
        1,
        RasterDataType::Float32,
        Some(32633),
    );

    // Precondition: the fixture really does reproduce the bug's shape.
    let ifd_offset = first_ifd_offset(&path);
    assert!(
        ifd_offset > 8192,
        "fixture must place its IFD past the old 8 KiB probe window, got offset {ifd_offset}"
    );

    let ds = Dataset::open(path.to_str().expect("utf-8 path")).expect("open trailing-IFD GeoTIFF");

    assert_eq!(ds.format(), DatasetFormat::GeoTiff);
    assert_eq!(ds.width(), width, "width must come from the real IFD");
    assert_eq!(ds.height(), height, "height must come from the real IFD");
    assert_eq!(ds.band_count(), 1, "band_count must come from the real IFD");
    assert_eq!(
        ds.data_type(),
        Some(RasterDataType::Float32),
        "pixel type must be readable from the header alone"
    );
    assert_eq!(ds.info().width, Some(width));
    assert_eq!(ds.info().height, Some(height));
    assert_eq!(ds.info().data_type, Some(RasterDataType::Float32));

    // Georeferencing survives the round-trip too.
    assert_eq!(ds.crs(), Some("EPSG:32633"));
    let gt = ds.geotransform().copied().expect("geotransform");
    assert!(
        (gt.origin_x - 100.0).abs() < 1e-9,
        "origin_x = {}",
        gt.origin_x
    );
    assert!(
        (gt.origin_y - 200.0).abs() < 1e-9,
        "origin_y = {}",
        gt.origin_y
    );
    assert!(
        (gt.pixel_width - 0.5).abs() < 1e-9,
        "pixel_width = {}",
        gt.pixel_width
    );
    assert!(ds.bounds().is_some(), "bounds derive from the geotransform");

    // The reporter's exact idiom must now work.
    let band = ds
        .bands()
        .next()
        .expect("Dataset::bands() must yield the single band")
        .expect("band read");
    assert_eq!(band.width(), u64::from(width));
    assert_eq!(band.height(), u64::from(height));
    assert_eq!(band.data_type(), RasterDataType::Float32);
}

/// OxiGeo must be able to re-open a GeoTIFF that OxiGeo itself just wrote.
///
/// This is the guarantee the 8 KiB probe silently broke: `GeoTiffWriter` writes
/// the IFD *after* the pixel data, so every file it produced was unreadable to
/// `Dataset::open`'s metadata probe while remaining perfectly readable to the
/// real driver.
#[test]
fn test_issue_14_roundtrip_own_writer() {
    use oxigeo::builder::{DatasetCreateBuilder, OutputFormat};

    let path = temp_path("roundtrip_own_writer.tif");
    let (width, height) = (300u32, 200u32);

    // Write through the facade's own writer API (`DatasetCreateBuilder`), which
    // is what a user following the README would reach for.
    let mut writer = DatasetCreateBuilder::new(&path, OutputFormat::GeoTiff)
        .create()
        .expect("create writer");
    writer
        .set_dimensions(width, height, 1)
        .expect("set dimensions");
    writer.set_data_type(RasterDataType::UInt8);
    writer.set_geo_transform(GeoTransform::north_up(0.0, f64::from(height), 1.0, -1.0));
    let data: Vec<u8> = (0..(width as usize * height as usize))
        .map(|i| (i % 251) as u8)
        .collect();
    writer.write_all_bands(&data).expect("write bands");
    writer.finalize().expect("finalize");

    assert!(
        first_ifd_offset(&path) > 8192,
        "the writer must place the IFD after the pixel data for this to be a regression test"
    );

    let ds = Dataset::open(path.to_str().expect("utf-8 path")).expect("re-open written GeoTIFF");
    assert_eq!(ds.width(), width);
    assert_eq!(ds.height(), height);
    assert_eq!(ds.band_count(), 1);
    assert_eq!(ds.data_type(), Some(RasterDataType::UInt8));

    let mut bands = ds.bands();
    let band = bands
        .next()
        .expect("bands() must yield exactly one band")
        .expect("band read");
    assert_eq!(band.width(), u64::from(width));
    assert_eq!(band.height(), u64::from(height));
    assert_eq!(band.data_type(), RasterDataType::UInt8);
    assert_eq!(
        band.as_bytes().len(),
        width as usize * height as usize,
        "UInt8 single band should be exactly width*height bytes"
    );
    assert!(bands.next().is_none(), "only one band was written");
}

/// A multi-band, multi-byte raster round-trips its band count and element type.
#[test]
fn test_issue_14_multiband_uint16_metadata_roundtrip() {
    let path = temp_path("multiband_u16.tif");
    write_geotiff(&path, 128, 96, 3, RasterDataType::UInt16, Some(4326));

    let ds = Dataset::open(path.to_str().expect("utf-8 path")).expect("open");
    assert_eq!(ds.width(), 128);
    assert_eq!(ds.height(), 96);
    assert_eq!(ds.band_count(), 3);
    assert_eq!(ds.data_type(), Some(RasterDataType::UInt16));
    assert_eq!(ds.crs(), Some("EPSG:4326"));
    assert_eq!(
        ds.bands().count(),
        3,
        "bands() must iterate all three bands"
    );
}

// ─── Bug 1, part 2: failures must be errors, never zero-filled metadata ──────

/// A truncated GeoTIFF must produce a typed error from `Dataset::open`, not an
/// `Ok(DatasetInfo)` full of zeros.
#[test]
fn test_issue_14_truncated_geotiff_is_an_error_not_zeros() {
    let path = temp_path("truncated.tif");
    write_geotiff(&path, 256, 256, 1, RasterDataType::Float32, Some(4326));

    // Sanity: intact, the file opens fine.
    Dataset::open(path.to_str().expect("utf-8 path")).expect("intact file opens");

    // Chop the file off in the middle of the pixel data, destroying the
    // trailing IFD entirely.
    let full_len = std::fs::metadata(&path).expect("metadata").len();
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("reopen for truncation");
    file.set_len(full_len / 2).expect("truncate");
    drop(file);

    let err = Dataset::open(path.to_str().expect("utf-8 path"))
        .expect_err("a truncated GeoTIFF must not open silently");
    assert!(
        matches!(err, OxiGeoError::Format(_)) || matches!(err, OxiGeoError::Io(_)),
        "expected a typed Format/Io error explaining the failure, got {err:?}"
    );
    let message = err.to_string();
    assert!(
        message.contains("truncated.tif"),
        "the error must name the offending file, got: {message}"
    );
}

/// A file that carries the TIFF magic but nothing else is an error too.
#[test]
fn test_issue_14_magic_only_geotiff_is_an_error() {
    let path = temp_path("magic_only.tif");
    // `II` + version 42 + first-IFD offset pointing nowhere.
    std::fs::write(&path, [0x49u8, 0x49, 0x2A, 0x00, 0xFF, 0xFF, 0xFF, 0x7F]).expect("write stub");

    let err = Dataset::open(path.to_str().expect("utf-8 path"))
        .expect_err("magic-only stub must not open silently");
    assert!(
        matches!(err, OxiGeoError::Format(_)) || matches!(err, OxiGeoError::Io(_)),
        "expected a typed Format/Io error, got {err:?}"
    );
}

/// The same guarantee holds for the module-level `oxigeo::open::open()` entry
/// point, which is a separate code path from `Dataset::open`.
#[test]
fn test_issue_14_module_open_agrees_with_dataset_open() {
    let path = temp_path("module_open.tif");
    write_geotiff(&path, 320, 240, 1, RasterDataType::Int16, Some(3857));

    let opened = oxigeo::open::open(&path).expect("open::open");
    let info = opened.info().expect("info");
    assert_eq!(info.format, DatasetFormat::GeoTiff);
    assert_eq!(info.width, Some(320));
    assert_eq!(info.height, Some(240));
    assert_eq!(info.band_count, 1);
    assert_eq!(info.data_type, Some(RasterDataType::Int16));
    assert_eq!(info.crs.as_deref(), Some("EPSG:3857"));

    let ds = Dataset::open(path.to_str().expect("utf-8 path")).expect("Dataset::open");
    assert_eq!(ds.width(), info.width.unwrap_or(0));
    assert_eq!(ds.height(), info.height.unwrap_or(0));
    assert_eq!(ds.data_type(), info.data_type);

    // A corrupt file must fail through this path as well.
    let bad = temp_path("module_open_bad.tif");
    std::fs::write(&bad, [0x49u8, 0x49, 0x2A, 0x00, 0x00, 0x00, 0x00, 0x00]).expect("write stub");
    assert!(
        oxigeo::open::open(&bad).is_err(),
        "open::open must not report zeros for an unparseable GeoTIFF"
    );
}

// ─── Bug 2: the missing `data_type` accessor ─────────────────────────────────

/// `Dataset::data_type()` is resolved from the header, so it is available
/// before any pixel is read and can be used to size a destination buffer.
#[test]
fn test_issue_14_data_type_is_known_before_reading_pixels() {
    for dtype in [
        RasterDataType::UInt8,
        RasterDataType::Int16,
        RasterDataType::UInt16,
        RasterDataType::Int32,
        RasterDataType::Float32,
        RasterDataType::Float64,
    ] {
        let path = temp_path(&format!("dtype_{}.tif", dtype.name()));
        write_geotiff(&path, 64, 48, 1, dtype, None);

        let ds = Dataset::open(path.to_str().expect("utf-8 path")).expect("open");
        assert_eq!(
            ds.data_type(),
            Some(dtype),
            "header-derived data_type for {}",
            dtype.name()
        );

        // The whole point: size the buffer from metadata, then verify the read
        // matches exactly.
        let expected_len = ds.width() as usize * ds.height() as usize * dtype.size_bytes();
        let band = ds.read_band(0).expect("read band");
        assert_eq!(
            band.as_bytes().len(),
            expected_len,
            "buffer size predicted from data_type must match the real read for {}",
            dtype.name()
        );
    }
}

/// Vector datasets have no pixels, so `data_type()` is `None` rather than a
/// misleading default.
#[cfg(feature = "geojson")]
#[test]
fn test_issue_14_vector_dataset_has_no_data_type() {
    let path = temp_path("vector.geojson");
    std::fs::write(
        &path,
        br#"{"type":"FeatureCollection","features":[{"type":"Feature","geometry":null,"properties":{}}]}"#,
    )
    .expect("write geojson");

    let ds = Dataset::open(path.to_str().expect("utf-8 path")).expect("open geojson");
    assert_eq!(ds.format(), DatasetFormat::GeoJson);
    assert_eq!(ds.data_type(), None, "vector datasets carry no pixel type");
    assert_eq!(ds.info().data_type, None);
}

/// `DatasetInfo::default()` gives downstream code a construction base, so
/// construction sites survive future field additions.
///
/// `DatasetInfo` is `#[non_exhaustive]`, which rules out struct expressions
/// outside the defining crate — including the functional-update form
/// `DatasetInfo { .., ..Default::default() }` (`E0639`).  The supported
/// downstream pattern is therefore "default, then assign", exercised below.
#[test]
fn test_issue_14_dataset_info_default_is_all_unknown() {
    let info = oxigeo::DatasetInfo::default();
    assert_eq!(info.format, DatasetFormat::Unknown);
    assert_eq!(info.path, None);
    assert_eq!(info.width, None);
    assert_eq!(info.height, None);
    assert_eq!(info.band_count, 0);
    assert_eq!(info.layer_count, 0);
    assert_eq!(info.crs, None);
    assert!(info.geotransform.is_none());
    assert_eq!(info.feature_count, None);
    assert!(info.bounds.is_none());
    assert_eq!(info.data_type, None);

    let mut raster = oxigeo::DatasetInfo::default();
    raster.format = DatasetFormat::GeoTiff;
    raster.width = Some(10);
    raster.height = Some(20);
    raster.band_count = 2;
    raster.data_type = Some(RasterDataType::Float64);
    assert_eq!(raster.data_type, Some(RasterDataType::Float64));
    assert_eq!(raster.layer_count, 0, "untouched fields keep their default");
}

// ─── Fast-open property ──────────────────────────────────────────────────────

/// `Dataset::open` must stay a header-only operation: it may seek to the IFD
/// and read the block-offset tables, but it must never decode pixels.
///
/// The guard is deliberately generous (a full decode of this raster is orders
/// of magnitude slower than the threshold) so the test is not flaky on loaded
/// CI machines, while still failing loudly if `open` ever starts reading pixel
/// data.
#[test]
fn test_issue_14_open_does_not_decode_pixels() {
    let path = temp_path("fast_open.tif");
    // 4 MB of Float32 pixels, tiled and with the writer's default overview
    // pyramid — so the probe has a multi-IFD file to walk, as a real COG does.
    // Decoding a band takes milliseconds; parsing the header takes microseconds.
    const SIDE: u32 = 1024;
    write_geotiff(&path, SIDE, SIDE, 1, RasterDataType::Float32, Some(4326));
    let path_str = path.to_str().expect("utf-8 path");

    // Warm the page cache so this measures work, not first-touch I/O.
    let _ = Dataset::open(path_str).expect("warm-up open");

    let mut best = std::time::Duration::from_secs(3600);
    for _ in 0..10 {
        let start = std::time::Instant::now();
        let ds = Dataset::open(path_str).expect("open");
        let elapsed = start.elapsed();
        assert_eq!(ds.width(), SIDE);
        if elapsed < best {
            best = elapsed;
        }
    }

    let decode = {
        let ds = Dataset::open(path_str).expect("open");
        let start = std::time::Instant::now();
        let band = ds.read_band(0).expect("read band");
        assert_eq!(band.as_bytes().len(), SIDE as usize * SIDE as usize * 4);
        start.elapsed()
    };

    assert!(
        best.as_millis() < 5,
        "Dataset::open must remain a header-only probe, took {best:?} (full decode: {decode:?})"
    );
    assert!(
        best * 10 < decode,
        "Dataset::open ({best:?}) should be at least an order of magnitude cheaper \
         than decoding a band ({decode:?}) — it must not be reading pixels"
    );
}
