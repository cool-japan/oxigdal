//! Compile-time guard that the `lib.rs` split (cool-japan/oxigeo#14 lane 4c)
//! kept every public path byte-identical.
//!
//! `DatasetInfo`, `BandIter`, `BandStatistics`, `Compression` and
//! `ConversionOptions` were moved out of `lib.rs` into sibling modules and
//! re-exported at the crate root.  Because this file is a separate crate, it
//! sees only the public API: if any of those items had silently acquired a new
//! path (e.g. `oxigeo::raster_read::BandIter` instead of `oxigeo::BandIter`),
//! or if a private module had leaked as a public one, this file would stop
//! compiling.
//!
//! The moved inherent methods on `Dataset` are named too — an inherent impl may
//! legally live in any module of the defining crate, and this pins the fact
//! that they still resolve on `oxigeo::Dataset` itself.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::atomic::{AtomicU64, Ordering};

// Every moved item, addressed at its original crate-root path.
use oxigeo::{
    BandIter, BandStatistics, BoundingBox, Compression, ConversionOptions, Dataset, DatasetFormat,
    DatasetInfo, GeoTransform, OxiGeoError, RasterDataType, RasterMetadata, Result,
};

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

/// Naming each type in type position forces the paths to resolve.
#[allow(dead_code)]
struct PathsResolve<'a> {
    info: DatasetInfo,
    stats: BandStatistics,
    compression: Compression,
    options: ConversionOptions,
    format: DatasetFormat,
    transform: GeoTransform,
    bbox: BoundingBox,
    data_type: RasterDataType,
    metadata: RasterMetadata,
    iter: BandIter<'a>,
}

/// The `Dataset` methods that moved into sibling modules must still be inherent
/// methods on `oxigeo::Dataset`.
#[allow(dead_code)]
fn moved_methods_stay_on_dataset(ds: &Dataset) -> Result<()> {
    // `raster_read`
    let _read_band: fn(&Dataset, u32) -> Result<oxigeo::core_types::buffer::RasterBuffer> =
        Dataset::read_band;
    let _bands: fn(&Dataset) -> BandIter<'_> = Dataset::bands;
    let _read_window: fn(
        &Dataset,
        u32,
        u32,
        u32,
        u32,
        u32,
    ) -> Result<oxigeo::core_types::buffer::RasterBuffer> = Dataset::read_window;
    // `band_stats`
    let _statistics: fn(&Dataset, u32) -> Result<BandStatistics> = Dataset::statistics;
    // still at their original homes
    let _clip: fn(&Dataset, BoundingBox) -> Result<Dataset> = Dataset::clip;
    let _reproject: fn(&Dataset, u32) -> Result<Dataset> = Dataset::reproject;
    let _ = ds;
    Ok(())
}

/// `DatasetInfo`'s public fields are all still public and still named the same.
#[test]
fn test_dataset_info_fields_keep_their_public_names() {
    let info = DatasetInfo::default();
    let _: DatasetFormat = info.format;
    let _: Option<String> = info.path.clone();
    let _: Option<u32> = info.width;
    let _: Option<u32> = info.height;
    let _: u32 = info.band_count;
    let _: u32 = info.layer_count;
    let _: Option<String> = info.crs.clone();
    let _: Option<GeoTransform> = info.geotransform;
    let _: Option<u64> = info.feature_count;
    let _: Option<BoundingBox> = info.bounds;
    let _: Option<RasterDataType> = info.data_type;
}

/// `BandStatistics` and `ConversionOptions` fields likewise.
#[test]
fn test_moved_struct_fields_keep_their_public_names() {
    let options = ConversionOptions::default();
    let _: Option<Compression> = options.compression;
    let _: Option<u8> = options.compression_level;
    let _: bool = options.cog;
    let _: Vec<u32> = options.overviews.clone();
    let _: Option<u32> = options.tile_size;
    let _: Vec<(String, String)> = options.creation_options.clone();

    assert_eq!(Compression::default(), Compression::None);
    let _ = Compression::Deflate;
    let _ = Compression::Lzw;
    let _ = Compression::PackBits;
    let _ = Compression::Zstd;
}

/// A non-raster dataset still reports the moved read methods as unsupported
/// rather than, say, failing to resolve — proves the dispatch moved intact.
#[test]
fn test_moved_methods_still_dispatch() {
    use std::io::Write;

    let path = TempPath::new("path_stability.geojson");
    std::fs::File::create(&path)
        .and_then(|mut f| f.write_all(br#"{"type":"FeatureCollection","features":[]}"#))
        .expect("write geojson fixture");

    let ds = Dataset::open(path.to_str().expect("utf-8 path")).expect("open geojson");
    assert!(
        matches!(ds.read_band(0), Err(OxiGeoError::NotSupported { .. })),
        "read_band on a vector dataset should report NotSupported"
    );
    assert!(
        matches!(
            ds.read_window(0, 0, 0, 1, 1),
            Err(OxiGeoError::NotSupported { .. })
        ),
        "read_window on a vector dataset should report NotSupported"
    );
    assert!(
        matches!(ds.statistics(0), Err(OxiGeoError::NotSupported { .. })),
        "statistics on a vector dataset should report NotSupported"
    );
    assert_eq!(ds.bands().count(), 0, "a vector dataset has no bands");
}
