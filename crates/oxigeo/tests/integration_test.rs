//! Integration tests for the oxigeo umbrella crate.
//!
//! Tests format detection, conversion matrix, feature-flag re-exports,
//! conversion planning, and edge cases.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use oxigeo::DatasetFormat;
use oxigeo::convert::{
    ConversionPlan, ConversionStep, ConvertOptions, can_convert, detect_format,
    supported_conversions,
};

// ─── Format detection by extension ───────────────────────────────────────────

#[test]
fn detect_tif_extension() {
    assert_eq!(
        detect_format("elevation.tif").ok(),
        Some(DatasetFormat::GeoTiff)
    );
}

#[test]
fn detect_tiff_extension() {
    assert_eq!(detect_format("dem.tiff").ok(), Some(DatasetFormat::GeoTiff));
}

#[test]
fn detect_geojson_extension() {
    assert_eq!(
        detect_format("cities.geojson").ok(),
        Some(DatasetFormat::GeoJson)
    );
}

#[test]
fn detect_gpkg_extension() {
    assert_eq!(
        detect_format("admin.gpkg").ok(),
        Some(DatasetFormat::GeoPackage)
    );
}

#[test]
fn detect_pmtiles_extension() {
    assert_eq!(
        detect_format("basemap.pmtiles").ok(),
        Some(DatasetFormat::PMTiles)
    );
}

#[test]
fn detect_mbtiles_extension() {
    assert_eq!(
        detect_format("osm.mbtiles").ok(),
        Some(DatasetFormat::MBTiles)
    );
}

#[test]
fn detect_shp_extension() {
    assert_eq!(
        detect_format("roads.shp").ok(),
        Some(DatasetFormat::Shapefile)
    );
}

#[test]
fn detect_fgb_extension() {
    assert_eq!(
        detect_format("buildings.fgb").ok(),
        Some(DatasetFormat::FlatGeobuf)
    );
}

#[test]
fn detect_parquet_extension() {
    assert_eq!(
        detect_format("census.parquet").ok(),
        Some(DatasetFormat::GeoParquet)
    );
}

#[test]
fn detect_zarr_extension() {
    assert_eq!(
        detect_format("climate.zarr").ok(),
        Some(DatasetFormat::Zarr)
    );
}

#[test]
fn detect_copc_laz_compound() {
    assert_eq!(
        detect_format("lidar.copc.laz").ok(),
        Some(DatasetFormat::Copc)
    );
}

#[test]
fn detect_laz_extension() {
    // A plain `.laz` file is not COPC-structured (no octree-hierarchy VLR /
    // chunked layout); only the compound `.copc.laz` extension is tagged
    // `Copc`. See `DatasetFormat::Las` doc comment in `format.rs`.
    assert_eq!(detect_format("scan.laz").ok(), Some(DatasetFormat::Las));
}

#[test]
fn detect_las_extension() {
    assert_eq!(detect_format("scan.las").ok(), Some(DatasetFormat::Las));
}

#[test]
fn detect_jp2_extension() {
    assert_eq!(
        detect_format("imagery.jp2").ok(),
        Some(DatasetFormat::Jpeg2000)
    );
}

#[test]
fn detect_grib_extension() {
    assert_eq!(
        detect_format("forecast.grib2").ok(),
        Some(DatasetFormat::Grib)
    );
}

// ─── Edge cases ──────────────────────────────────────────────────────────────

#[test]
fn detect_empty_path_returns_error() {
    assert!(detect_format("").is_err());
}

#[test]
fn detect_unknown_extension_returns_error() {
    assert!(detect_format("readme.md").is_err());
}

#[test]
fn detect_no_extension_returns_error() {
    assert!(detect_format("Makefile").is_err());
}

#[test]
fn detect_dot_only_returns_error() {
    assert!(detect_format(".").is_err());
}

#[test]
fn detect_path_with_directories() {
    assert_eq!(
        detect_format("/data/project/world.tif").ok(),
        Some(DatasetFormat::GeoTiff)
    );
}

#[test]
fn detect_uppercase_extension() {
    // from_extension lowercases internally
    assert_eq!(
        detect_format("IMAGE.TIF").ok(),
        Some(DatasetFormat::GeoTiff)
    );
}

// ─── Conversion matrix tests ────────────────────────────────────────────────

#[test]
fn can_convert_identity_all_formats() {
    let formats = [
        DatasetFormat::GeoTiff,
        DatasetFormat::GeoJson,
        DatasetFormat::Shapefile,
        DatasetFormat::GeoParquet,
        DatasetFormat::FlatGeobuf,
        DatasetFormat::Zarr,
        DatasetFormat::PMTiles,
        DatasetFormat::MBTiles,
        DatasetFormat::Copc,
        DatasetFormat::GeoPackage,
    ];
    for fmt in &formats {
        assert!(
            can_convert(*fmt, *fmt),
            "identity should be true for {fmt:?}"
        );
    }
}

#[test]
fn can_convert_raster_to_raster_pairs() {
    let raster_formats = [
        DatasetFormat::GeoTiff,
        DatasetFormat::Zarr,
        DatasetFormat::NetCdf,
        DatasetFormat::Hdf5,
        DatasetFormat::Jpeg2000,
        DatasetFormat::PMTiles,
        DatasetFormat::MBTiles,
        DatasetFormat::Copc,
    ];
    for &a in &raster_formats {
        for &b in &raster_formats {
            assert!(
                can_convert(a, b),
                "raster-to-raster should work: {a:?} -> {b:?}"
            );
        }
    }
}

#[test]
fn can_convert_vector_to_vector_pairs() {
    let vector_formats = [
        DatasetFormat::GeoJson,
        DatasetFormat::Shapefile,
        DatasetFormat::GeoParquet,
        DatasetFormat::FlatGeobuf,
        DatasetFormat::GeoPackage,
    ];
    for &a in &vector_formats {
        for &b in &vector_formats {
            assert!(
                can_convert(a, b),
                "vector-to-vector should work: {a:?} -> {b:?}"
            );
        }
    }
}

#[test]
fn cannot_convert_raster_to_pure_vector() {
    assert!(!can_convert(DatasetFormat::GeoTiff, DatasetFormat::GeoJson));
    assert!(!can_convert(DatasetFormat::Zarr, DatasetFormat::Shapefile));
    assert!(!can_convert(
        DatasetFormat::PMTiles,
        DatasetFormat::FlatGeobuf
    ));
}

#[test]
fn cannot_convert_vector_to_pure_raster() {
    assert!(!can_convert(DatasetFormat::GeoJson, DatasetFormat::GeoTiff));
    assert!(!can_convert(DatasetFormat::Shapefile, DatasetFormat::Zarr));
}

#[test]
fn can_convert_anything_to_geopackage() {
    // GeoPackage is mixed, should accept anything except Unknown
    assert!(can_convert(
        DatasetFormat::GeoTiff,
        DatasetFormat::GeoPackage
    ));
    assert!(can_convert(
        DatasetFormat::GeoJson,
        DatasetFormat::GeoPackage
    ));
    assert!(can_convert(
        DatasetFormat::PMTiles,
        DatasetFormat::GeoPackage
    ));
}

#[test]
fn can_convert_geopackage_to_anything() {
    assert!(can_convert(
        DatasetFormat::GeoPackage,
        DatasetFormat::GeoTiff
    ));
    assert!(can_convert(
        DatasetFormat::GeoPackage,
        DatasetFormat::GeoJson
    ));
}

#[test]
fn cannot_convert_unknown() {
    assert!(!can_convert(DatasetFormat::Unknown, DatasetFormat::GeoTiff));
    assert!(!can_convert(DatasetFormat::GeoTiff, DatasetFormat::Unknown));
    // But identity for Unknown is still true
    assert!(can_convert(DatasetFormat::Unknown, DatasetFormat::Unknown));
}

// ─── supported_conversions ───────────────────────────────────────────────────

#[test]
fn supported_conversions_has_many_pairs() {
    let pairs = supported_conversions();
    assert!(pairs.len() > 50, "expected many pairs, got {}", pairs.len());
}

#[test]
fn supported_conversions_excludes_identity() {
    for (from, to) in supported_conversions() {
        assert_ne!(from, to);
    }
}

// ─── Feature flag re-export tests ────────────────────────────────────────────

#[test]
fn reexport_core_types_accessible() {
    // These are always available (not feature-gated)
    let _ = oxigeo::version();
    let count = oxigeo::driver_count();
    assert!(count >= 3, "default features should enable >= 3 drivers");
}

#[test]
fn dataset_format_display() {
    assert_eq!(DatasetFormat::GeoTiff.to_string(), "GTiff");
    assert_eq!(DatasetFormat::PMTiles.to_string(), "PMTiles");
    assert_eq!(DatasetFormat::MBTiles.to_string(), "MBTiles");
    assert_eq!(DatasetFormat::Copc.to_string(), "COPC");
    assert_eq!(DatasetFormat::GeoPackage.to_string(), "GPKG");
}

#[test]
fn dataset_format_driver_name() {
    assert_eq!(DatasetFormat::GeoTiff.driver_name(), "GTiff");
    assert_eq!(DatasetFormat::GeoJson.driver_name(), "GeoJSON");
    assert_eq!(DatasetFormat::PMTiles.driver_name(), "PMTiles");
}

// ─── ConversionPlan tests ────────────────────────────────────────────────────

#[test]
fn plan_simple_conversion_has_expected_steps() {
    let plan = ConversionPlan::build(
        DatasetFormat::GeoJson,
        DatasetFormat::Shapefile,
        ConvertOptions::new(),
    )
    .expect("plan");

    assert_eq!(plan.source, DatasetFormat::GeoJson);
    assert_eq!(plan.target, DatasetFormat::Shapefile);
    assert!(plan.step_count() >= 3);
    assert!(!plan.has_reprojection());
    assert!(!plan.has_bbox_filter());
}

#[test]
fn plan_with_all_options() {
    let opts = ConvertOptions::new()
        .with_bbox(-10.0, -10.0, 10.0, 10.0)
        .with_target_crs("EPSG:3857")
        .with_feature_limit(100)
        .with_compression("lz4");

    let plan = ConversionPlan::build(DatasetFormat::Shapefile, DatasetFormat::GeoParquet, opts)
        .expect("plan");

    assert!(plan.has_reprojection());
    assert!(plan.has_bbox_filter());
    assert!(plan.step_count() >= 5);
}

#[test]
fn plan_unsupported_returns_error() {
    let result = ConversionPlan::build(
        DatasetFormat::GeoTiff,
        DatasetFormat::GeoJson,
        ConvertOptions::new(),
    );
    assert!(result.is_err());
}

#[test]
fn plan_first_step_is_read_source() {
    let plan = ConversionPlan::build(
        DatasetFormat::GeoTiff,
        DatasetFormat::Zarr,
        ConvertOptions::new(),
    )
    .expect("plan");

    assert!(matches!(
        plan.steps.first(),
        Some(ConversionStep::ReadSource(DatasetFormat::GeoTiff))
    ));
}

#[test]
fn plan_last_step_is_write_target() {
    let plan = ConversionPlan::build(
        DatasetFormat::GeoTiff,
        DatasetFormat::Zarr,
        ConvertOptions::new(),
    )
    .expect("plan");

    assert!(matches!(
        plan.steps.last(),
        Some(ConversionStep::WriteTarget(DatasetFormat::Zarr))
    ));
}

#[test]
fn plan_identity_no_transcode() {
    let plan = ConversionPlan::build(
        DatasetFormat::GeoJson,
        DatasetFormat::GeoJson,
        ConvertOptions::new(),
    )
    .expect("plan");

    // Identity: just read + write, no transcode
    assert_eq!(plan.step_count(), 2);
    let has_transcode = plan.steps.iter().any(|s| {
        matches!(
            s,
            ConversionStep::TranscodeRaster(_) | ConversionStep::TranscodeVector(_)
        )
    });
    assert!(!has_transcode, "identity should not have transcode step");
}

// ─── Dataset convenience method tests ────────────────────────────────────────

/// Write a minimal GeoTIFF into a temp file and return its path.
///
/// The image is an 8×8 single-band Float32 raster with sequential pixel values
/// 1.0 … 64.0, a simple north-up geo-transform, and EPSG:4326 as the CRS.
#[cfg(feature = "geotiff")]
fn write_synthetic_tiff(name: &str) -> std::path::PathBuf {
    use oxigeo::core_types::types::{GeoTransform, NoDataValue, RasterDataType};
    use oxigeo::geotiff::{
        GeoTiffWriter, GeoTiffWriterOptions, OverviewResampling, WriterConfig,
        tiff::{Compression, PhotometricInterpretation, Predictor},
    };

    let dir = std::env::temp_dir();
    let path = dir.join(name);

    let config = WriterConfig {
        width: 8,
        height: 8,
        band_count: 1,
        data_type: RasterDataType::Float32,
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

    // 8×8 float32 pixels: 1.0, 2.0, … 64.0 (row-major, BSQ for single band).
    let pixel_data: Vec<u8> = (1u32..=64u32)
        .flat_map(|v| (v as f32).to_le_bytes())
        .collect();

    let mut writer =
        GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default()).expect("create tiff");
    writer.write(&pixel_data).expect("write tiff");

    path
}

/// Write a synthetic multi-band GeoTIFF (chunky / pixel-interleaved) for
/// exercising multi-band conversion round-trips.
///
/// The image is `4×4`, `band_count` bands, `UInt8`, with a distinct value per
/// `(pixel, band)`.  When `nodata` is `Some`, it is recorded in the header.
#[cfg(feature = "geotiff")]
fn write_multiband_tiff(name: &str, band_count: u16, nodata: Option<i64>) -> std::path::PathBuf {
    use oxigeo::core_types::types::{GeoTransform, NoDataValue, RasterDataType};
    use oxigeo::geotiff::{
        GeoTiffWriter, GeoTiffWriterOptions, OverviewResampling, WriterConfig,
        tiff::{Compression, PhotometricInterpretation, Predictor},
    };

    let dir = std::env::temp_dir();
    let path = dir.join(name);

    let width = 4u32;
    let height = 4u32;
    let nodata_val = match nodata {
        Some(v) => NoDataValue::Integer(v),
        None => NoDataValue::None,
    };

    let config = WriterConfig {
        width: u64::from(width),
        height: u64::from(height),
        band_count,
        data_type: RasterDataType::UInt8,
        compression: Compression::None,
        predictor: Predictor::None,
        tile_width: None,
        tile_height: None,
        photometric: PhotometricInterpretation::BlackIsZero,
        geo_transform: Some(GeoTransform::north_up(10.0, 50.0, 1.0, -1.0)),
        epsg_code: Some(4326),
        nodata: nodata_val,
        use_bigtiff: false,
        generate_overviews: false,
        overview_resampling: OverviewResampling::Average,
        overview_levels: vec![],
    };

    // Chunky / pixel-interleaved: for each of the 16 pixels, one byte per band.
    let mut pixel_data: Vec<u8> =
        Vec::with_capacity((width * height) as usize * band_count as usize);
    for p in 0..(width * height) {
        for b in 0..band_count {
            pixel_data.push(((p as u16 * band_count + b) % 251) as u8);
        }
    }

    let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
        .expect("create multiband tiff");
    writer.write(&pixel_data).expect("write multiband tiff");

    path
}

/// `Dataset::statistics` on a synthetic 8×8 GeoTIFF.
///
/// Verifies: `min <= mean <= max` and `valid_count == 64`.
#[cfg(feature = "geotiff")]
#[test]
fn test_dataset_statistics_geotiff() {
    let path = write_synthetic_tiff("test_ds_statistics_8x8.tif");
    let ds = oxigeo::Dataset::open(path.to_str().expect("path str")).expect("open");

    let stats = ds.statistics(0).expect("statistics band 0");

    assert!(
        stats.min <= stats.mean && stats.mean <= stats.max,
        "expected min <= mean <= max, got min={} mean={} max={}",
        stats.min,
        stats.mean,
        stats.max
    );
    assert_eq!(stats.valid_count, 64, "all 64 pixels should be valid");
    assert_eq!(stats.band, 0);
    // Pixel values are 1..=64; min=1, max=64, mean=32.5
    assert!(
        (stats.min - 1.0).abs() < 1e-3,
        "min should be ~1.0, got {}",
        stats.min
    );
    assert!(
        (stats.max - 64.0).abs() < 1e-3,
        "max should be ~64.0, got {}",
        stats.max
    );
    assert!(
        (stats.mean - 32.5).abs() < 1e-2,
        "mean should be ~32.5, got {}",
        stats.mean
    );
}

/// `Dataset::statistics` returns `Err` when `band` index is out of range.
#[cfg(feature = "geotiff")]
#[test]
fn test_dataset_statistics_band_out_of_range() {
    let path = write_synthetic_tiff("test_ds_statistics_oor.tif");
    let ds = oxigeo::Dataset::open(path.to_str().expect("path str")).expect("open");

    // The synthetic TIFF has 1 band (band_count == 1). Band index 5 is invalid.
    let result = ds.statistics(5);
    assert!(
        result.is_err(),
        "expected error for out-of-range band, got {:?}",
        result
    );
}

/// `Dataset::clip` on a synthetic 16×16 raster returns smaller dimensions.
///
/// The raster covers [0,0]—[16,16] at 1 pixel/unit.  Clipping to [2,2]—[10,10]
/// should yield an 8×8 result.
#[test]
fn test_dataset_clip_raster() {
    use oxigeo::{BoundingBox, Dataset, DatasetFormat};

    // Build a metadata-only Dataset with a known geo-transform (no file on disk).
    // We use open_with_format on a path that doesn't exist, but we want a
    // Dataset with geotransform already set.  Instead, construct directly via
    // Dataset::open after writing a minimal TIFF.
    // For isolation we build a Dataset value via the builder interface.

    // Use a real-but-empty TIFF written to temp dir.
    #[cfg(feature = "geotiff")]
    {
        // Write a 16×16 synthetic tiff covering [0,0]→[16,16].
        use oxigeo::core_types::types::{GeoTransform, NoDataValue, RasterDataType};
        use oxigeo::geotiff::{
            GeoTiffWriter, GeoTiffWriterOptions, OverviewResampling, WriterConfig,
            tiff::{Compression, PhotometricInterpretation, Predictor},
        };

        let dir = std::env::temp_dir();
        let path = dir.join("test_ds_clip_16x16.tif");

        let config = WriterConfig {
            width: 16,
            height: 16,
            band_count: 1,
            data_type: RasterDataType::Float32,
            compression: Compression::None,
            predictor: Predictor::None,
            tile_width: None,
            tile_height: None,
            photometric: PhotometricInterpretation::BlackIsZero,
            // origin at (0,16), 1×1 pixels → covers [0,0]→[16,16]
            geo_transform: Some(GeoTransform::north_up(0.0, 16.0, 1.0, -1.0)),
            epsg_code: Some(4326),
            nodata: NoDataValue::None,
            use_bigtiff: false,
            generate_overviews: false,
            overview_resampling: OverviewResampling::Average,
            overview_levels: vec![],
        };

        let pixel_data = vec![0u8; 16 * 16 * 4]; // zeros, float32
        let mut writer =
            GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default()).expect("create");
        writer.write(&pixel_data).expect("write");

        let ds = Dataset::open(path.to_str().expect("path str")).expect("open");
        assert_eq!(ds.width(), 16);
        assert_eq!(ds.height(), 16);

        // Build a clip bbox that covers pixel rows/cols [2,10) × [2,10] using the
        // dataset's ACTUAL geotransform (which may differ from what we wrote due to
        // TIFF round-trip conventions for ModelPixelScale sign).
        let gt = ds
            .geotransform()
            .expect("16×16 TIFF should have a geotransform");
        let (wx_min, wy_min) = gt.pixel_to_world(2.0, 2.0);
        let (wx_max, wy_max) = gt.pixel_to_world(10.0, 10.0);
        // Ensure min ≤ max for BoundingBox construction.
        let (bmin_x, bmax_x) = if wx_min <= wx_max {
            (wx_min, wx_max)
        } else {
            (wx_max, wx_min)
        };
        let (bmin_y, bmax_y) = if wy_min <= wy_max {
            (wy_min, wy_max)
        } else {
            (wy_max, wy_min)
        };
        let bbox = BoundingBox::new(bmin_x, bmin_y, bmax_x, bmax_y).expect("valid derived bbox");

        let clipped = ds.clip(bbox).expect("clip");

        assert!(
            clipped.width() < ds.width(),
            "clipped width {} should be < original {}",
            clipped.width(),
            ds.width()
        );
        assert!(
            clipped.height() < ds.height(),
            "clipped height {} should be < original {}",
            clipped.height(),
            ds.height()
        );
    }

    // Non-geotiff branch: clip a vector dataset (no geotransform) returns Ok.
    #[cfg(feature = "geojson")]
    {
        use std::io::Write;
        let dir = std::env::temp_dir();
        let path = dir.join("test_ds_clip_vector.geojson");
        std::fs::File::create(&path)
            .and_then(|mut f| f.write_all(b"{\"type\":\"FeatureCollection\",\"features\":[]}"))
            .expect("write");
        let ds = Dataset::open(path.to_str().expect("path str")).expect("open");
        let bbox = BoundingBox::new(-10.0, -10.0, 10.0, 10.0).expect("valid bbox");
        // Vector path should succeed (logical clip annotation).
        let clipped = ds.clip(bbox).expect("clip vector");
        assert_eq!(clipped.format(), DatasetFormat::GeoJson);
    }
}

/// `Dataset::reproject` returns `Err` when the `proj` feature is disabled.
///
/// When `--all-features` is used this test is skipped via cfg.
#[cfg(not(feature = "proj"))]
#[test]
fn test_dataset_reproject_returns_err_without_proj_feature() {
    use std::io::Write;
    let dir = std::env::temp_dir();
    let path = dir.join("test_ds_reproject_noproj.tif");
    // Write TIFF magic bytes only — we just need the file to exist.
    let bytes: [u8; 8] = [0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00];
    std::fs::File::create(&path)
        .and_then(|mut f| f.write_all(&bytes))
        .expect("write tiff");

    let ds = oxigeo::Dataset::open(path.to_str().expect("path str")).expect("open");
    let result = ds.reproject(3857);
    assert!(
        result.is_err(),
        "reproject() should return Err without 'proj' feature"
    );
    // Verify it is specifically a NotSupported error (not a panic or other failure)
    match result {
        Err(oxigeo::OxiGeoError::NotSupported { .. }) => {}
        other => panic!("expected NotSupported, got {other:?}"),
    }
}

/// `Dataset::reproject` succeeds and updates geo-transform when `proj` is enabled.
#[cfg(feature = "proj")]
#[cfg(feature = "geotiff")]
#[test]
fn test_dataset_reproject_with_proj_feature() {
    let path = write_synthetic_tiff("test_ds_reproject_proj.tif");
    let ds = oxigeo::Dataset::open(path.to_str().expect("path str")).expect("open");

    // Source is EPSG:4326 (degrees), reproject to Web Mercator (EPSG:3857, metres).
    let reprojected = ds.reproject(3857).expect("reproject to 3857");

    // The reprojected dataset should have a new CRS string.
    let new_crs = reprojected
        .crs()
        .expect("reprojected dataset should have CRS");
    assert!(
        new_crs.contains("3857"),
        "CRS should mention 3857, got {new_crs}"
    );

    // Dimensions must be preserved.
    assert_eq!(reprojected.width(), ds.width());
    assert_eq!(reprojected.height(), ds.height());

    // The geo-transform origin should have changed (metres ≠ degrees).
    let orig_gt = ds.geotransform().expect("orig gt");
    let new_gt = reprojected.geotransform().expect("new gt");
    let changed = (new_gt.origin_x - orig_gt.origin_x).abs() > 1.0
        || (new_gt.origin_y - orig_gt.origin_y).abs() > 1.0;
    assert!(
        changed,
        "geo-transform should change after reprojection, orig={orig_gt:?} new={new_gt:?}"
    );
}

// ─── Item 4: BandIter tests ────────────────────────────────────────────────────

/// `Dataset::bands()` iterates over the single band of a synthetic TIFF.
#[cfg(feature = "geotiff")]
#[test]
fn test_bands_iter_single_band() {
    let path = write_synthetic_tiff("test_bands_iter_single.tif");
    let ds = oxigeo::Dataset::open(path.to_str().expect("path str")).expect("open");

    assert_eq!(ds.band_count(), 1, "synthetic tiff has 1 band");

    let bands: Vec<_> = ds.bands().collect();
    assert_eq!(bands.len(), 1, "iterator should yield exactly 1 band");

    let buf = bands
        .into_iter()
        .next()
        .expect("one band")
        .expect("read ok");
    assert_eq!(
        buf.as_bytes().len(),
        8 * 8 * 4, // 8×8 float32 = 256 bytes
        "single band should be 8×8×4 bytes"
    );
}

/// `Dataset::read_band()` returns `Err` for an out-of-range band index.
#[cfg(feature = "geotiff")]
#[test]
fn test_bands_iter_out_of_range_errors() {
    let path = write_synthetic_tiff("test_bands_oor.tif");
    let ds = oxigeo::Dataset::open(path.to_str().expect("path str")).expect("open");

    let result = ds.read_band(5);
    assert!(
        result.is_err(),
        "band index 5 should fail for a 1-band TIFF, got {:?}",
        result
    );
}

/// `BandIter::size_hint()` reports the correct count.
#[cfg(feature = "geotiff")]
#[test]
fn test_bands_iter_size_hint() {
    let path = write_synthetic_tiff("test_bands_size_hint.tif");
    let ds = oxigeo::Dataset::open(path.to_str().expect("path str")).expect("open");

    let iter = ds.bands();
    let (lo, hi) = iter.size_hint();
    assert_eq!(lo, 1, "size_hint low should equal band_count");
    assert_eq!(hi, Some(1), "size_hint high should equal band_count");
}

// ─── Item 5: Dataset::convert tests ───────────────────────────────────────────

/// Identity conversion GeoTIFF → GeoTIFF succeeds and output is readable.
#[cfg(feature = "geotiff")]
#[test]
fn test_dataset_convert_raster_identity() {
    use oxigeo::ConversionOptions;

    let src_path = write_synthetic_tiff("test_convert_src.tif");
    let dst_path = std::env::temp_dir().join("test_convert_dst.tif");
    let src_ds = oxigeo::Dataset::open(src_path.to_str().expect("path")).expect("open src");

    let dst_ds = src_ds
        .convert(
            &dst_path,
            oxigeo::DatasetFormat::GeoTiff,
            ConversionOptions::default(),
        )
        .expect("convert GeoTiff→GeoTiff");

    assert_eq!(dst_ds.format(), oxigeo::DatasetFormat::GeoTiff);
    assert!(dst_path.exists(), "output TIFF file should exist on disk");
}

/// Multi-band GeoTIFF → GeoTIFF conversion round-trips band count, pixel data,
/// and the NoData value.
///
/// Regression for the bug where `convert_geotiff_to_geotiff` looped over
/// `band_count`, appending the full pixel-interleaved image once per band. That
/// produced a buffer `band_count×` too large and tripped the writer's length
/// validation, so every multi-band conversion failed. It also dropped the
/// source NoData value unconditionally.
#[cfg(feature = "geotiff")]
#[test]
fn test_dataset_convert_multiband_roundtrip() {
    use oxigeo::ConversionOptions;
    use oxigeo::core_types::io::FileDataSource;
    use oxigeo::core_types::types::NoDataValue;
    use oxigeo::geotiff::GeoTiffReader;

    let src_path = write_multiband_tiff("test_convert_multiband_src.tif", 3, Some(255));
    let dst_path = std::env::temp_dir().join("test_convert_multiband_dst.tif");

    let src_ds = oxigeo::Dataset::open(src_path.to_str().expect("path")).expect("open src");
    let dst_ds = src_ds
        .convert(
            &dst_path,
            oxigeo::DatasetFormat::GeoTiff,
            ConversionOptions::default(),
        )
        .expect("multi-band GeoTiff→GeoTiff conversion should succeed");

    assert_eq!(dst_ds.format(), oxigeo::DatasetFormat::GeoTiff);
    assert!(dst_path.exists(), "output TIFF file should exist on disk");

    let src_reader =
        GeoTiffReader::open(FileDataSource::open(&src_path).expect("src source")).expect("src rdr");
    let dst_reader =
        GeoTiffReader::open(FileDataSource::open(&dst_path).expect("dst source")).expect("dst rdr");

    assert_eq!(dst_reader.band_count(), 3, "output should preserve 3 bands");
    assert_eq!(dst_reader.width(), src_reader.width());
    assert_eq!(dst_reader.height(), src_reader.height());
    assert_eq!(
        dst_reader.read_band(0, 0).expect("dst pixels"),
        src_reader.read_band(0, 0).expect("src pixels"),
        "pixel data should round-trip byte-for-byte"
    );
    assert_eq!(
        dst_reader.nodata(),
        NoDataValue::Integer(255),
        "source NoData value should be preserved across conversion"
    );
}

/// GeoJSON → Shapefile conversion tolerates multi-byte UTF-8 property names.
///
/// Regression for the byte-index slice `&key[..10]` that panicked with "byte
/// index 10 is not a char boundary" on non-ASCII field names longer than 10
/// bytes (Japanese, emoji, accented text).
#[cfg(all(feature = "geojson", feature = "shapefile"))]
#[test]
fn test_dataset_convert_geojson_shapefile_multibyte_keys() {
    use oxigeo::ConversionOptions;
    use std::io::Write;

    let dir = std::env::temp_dir();
    let src_path = dir.join("test_convert_multibyte_keys.geojson");
    // Property key "日本語フィールド名前" is 30 UTF-8 bytes; byte 10 lands
    // mid-character. Emoji key "🌍🌏🌎longitude" also straddles the boundary.
    let content = r#"{"type":"FeatureCollection","features":[
        {"type":"Feature","geometry":{"type":"Point","coordinates":[1.0,2.0]},
         "properties":{"日本語フィールド名前":"値","🌍🌏🌎region":"east"}}
    ]}"#;
    std::fs::File::create(&src_path)
        .and_then(|mut f| f.write_all(content.as_bytes()))
        .expect("write geojson");

    let dst_path = dir.join("test_convert_multibyte_keys.shp");
    let src_ds = oxigeo::Dataset::open(src_path.to_str().expect("path")).expect("open src");
    // Must not panic; conversion should complete.
    let result = src_ds.convert(
        &dst_path,
        oxigeo::DatasetFormat::Shapefile,
        ConversionOptions::default(),
    );
    assert!(
        result.is_ok(),
        "multi-byte property keys should not panic or fail: {result:?}"
    );
}

/// Converting raster to vector returns NotSupported.
#[test]
fn test_dataset_convert_unsupported_pair_errors() {
    use oxigeo::{ConversionOptions, DatasetFormat};
    use std::io::Write;

    // Create a minimal GeoTIFF on disk (just magic bytes)
    let dir = std::env::temp_dir();
    let path = dir.join("test_convert_unsupported.tif");
    let bytes: [u8; 8] = [0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00];
    std::fs::File::create(&path)
        .and_then(|mut f| f.write_all(&bytes))
        .expect("write tiff");

    let ds = oxigeo::Dataset::open(path.to_str().expect("path")).expect("open");
    // GeoTIFF → GeoJSON is a raster→vector cross-conversion, not supported.
    let output = dir.join("test_convert_unsupported_out.geojson");
    let result = ds.convert(
        &output,
        DatasetFormat::GeoJson,
        ConversionOptions::default(),
    );
    assert!(
        result.is_err(),
        "raster→vector conversion should return Err, got {:?}",
        result
    );
}

// ─── Item 6: Cloud URI detection tests ────────────────────────────────────────

/// `is_cloud_uri` recognises all standard cloud URI schemes.
#[test]
fn test_cloud_uri_detection() {
    assert!(oxigeo::is_cloud_uri("s3://bucket/key.tif"));
    assert!(oxigeo::is_cloud_uri("gs://bucket/key.geojson"));
    assert!(oxigeo::is_cloud_uri("az://container/blob.tif"));
    assert!(oxigeo::is_cloud_uri(
        "abfs://container@account.dfs.core.windows.net/path"
    ));
    assert!(oxigeo::is_cloud_uri("http://example.com/file.tif"));
    assert!(oxigeo::is_cloud_uri("https://example.com/file.tif"));
}

/// `is_cloud_uri` returns `false` for local paths.
#[test]
fn test_open_bare_path_still_works() {
    assert!(!oxigeo::is_cloud_uri("/local/path/to/file.tif"));
    assert!(!oxigeo::is_cloud_uri("relative/path/file.tif"));
    assert!(!oxigeo::is_cloud_uri("file.tif"));
}

/// Opening an `s3://` URI without the `cloud` feature returns `NotSupported`.
#[cfg(not(feature = "cloud"))]
#[test]
fn test_open_s3_uri_parses_without_cloud_feature() {
    let result = oxigeo::Dataset::open("s3://my-bucket/data/elevation.tif");
    assert!(
        result.is_err(),
        "s3:// open should fail without 'cloud' feature"
    );
    match result {
        Err(oxigeo::OxiGeoError::NotSupported { .. }) => {}
        other => panic!("expected NotSupported, got {other:?}"),
    }
}

// ─── Item 7: Dataset::info() populated tests ──────────────────────────────────

/// `Dataset::info()` has populated `feature_count` when GeoJSON contains features.
#[cfg(feature = "geojson")]
#[test]
fn test_info_geojson_populated() {
    use std::io::Write;
    let dir = std::env::temp_dir();
    let path = dir.join("test_info_geojson.geojson");
    let content = br#"{"type":"FeatureCollection","features":[
        {"type":"Feature","geometry":{"type":"Point","coordinates":[0,0]},"properties":{}},
        {"type":"Feature","geometry":{"type":"Point","coordinates":[1,1]},"properties":{}},
        {"type":"Feature","geometry":{"type":"Point","coordinates":[2,2]},"properties":{}}
    ]}"#;
    std::fs::File::create(&path)
        .and_then(|mut f| f.write_all(content))
        .expect("write geojson");

    let ds = oxigeo::Dataset::open(path.to_str().expect("path")).expect("open");
    let info = ds.info();

    assert_eq!(info.layer_count, 1, "GeoJSON FeatureCollection has 1 layer");
    assert_eq!(
        info.feature_count,
        Some(3),
        "should count 3 features, got {:?}",
        info.feature_count
    );
}

/// `Dataset::info()` has `feature_count = None` for an empty FeatureCollection.
#[cfg(feature = "geojson")]
#[test]
fn test_info_geojson_empty_collection() {
    use std::io::Write;
    let dir = std::env::temp_dir();
    let path = dir.join("test_info_geojson_empty.geojson");
    std::fs::File::create(&path)
        .and_then(|mut f| f.write_all(b"{\"type\":\"FeatureCollection\",\"features\":[]}"))
        .expect("write geojson");

    let ds = oxigeo::Dataset::open(path.to_str().expect("path")).expect("open");
    // Empty FeatureCollection has no "type":"Feature" tokens
    assert_eq!(
        ds.info().feature_count,
        None,
        "empty FeatureCollection should yield feature_count=None"
    );
}

/// `Dataset::info()` extracts `bounds` from a GeoJSON with a top-level `bbox`.
#[cfg(feature = "geojson")]
#[test]
fn test_info_geojson_bbox_parsed() {
    use std::io::Write;
    let dir = std::env::temp_dir();
    let path = dir.join("test_info_geojson_bbox.geojson");
    let content = br#"{"type":"FeatureCollection","bbox":[-180,-90,180,90],"features":[]}"#;
    std::fs::File::create(&path)
        .and_then(|mut f| f.write_all(content))
        .expect("write geojson");

    let ds = oxigeo::Dataset::open(path.to_str().expect("path")).expect("open");
    let bounds = ds.bounds();
    assert!(
        bounds.is_some(),
        "bounds should be populated from GeoJSON bbox"
    );
    let b = bounds.expect("bounds");
    assert!((b.min_x + 180.0).abs() < 1e-6, "min_x should be -180");
    assert!((b.max_x - 180.0).abs() < 1e-6, "max_x should be 180");
}

// ─── Item 5 (new): LZW compression, COG, and GeoJSON→Shapefile tests ──────────

/// `Dataset::convert` with `cog: true` writes a Cloud-Optimized GeoTIFF.
///
/// Uses the synthetic 8×8 source (reused from other tests) with `overviews: vec![2]`
/// to avoid degenerate 0×0 overview levels.  The output is re-opened via
/// `CogReader` to verify COG compliance; the reader's validation step would
/// return an error if the IFD ordering or tile layout is non-compliant.
#[cfg(feature = "geotiff")]
#[test]
fn test_dataset_convert_geotiff_cog() {
    use oxigeo::ConversionOptions;
    use oxigeo::core_types::io::FileDataSource;
    use oxigeo::geotiff::CogReader;

    let src_path = write_synthetic_tiff("test_cog_src.tif");
    let dst_path = std::env::temp_dir().join("test_cog_dst.tif");

    let src_ds = oxigeo::Dataset::open(src_path.to_str().expect("src path")).expect("open src");
    let opts = ConversionOptions {
        cog: true,
        // Use a single safe overview level so 8×8 → 4×4, no zero-dimension levels.
        overviews: vec![2],
        ..ConversionOptions::default()
    };
    let dst_ds = src_ds
        .convert(&dst_path, oxigeo::DatasetFormat::GeoTiff, opts)
        .expect("COG conversion should succeed");

    assert_eq!(dst_ds.format(), oxigeo::DatasetFormat::GeoTiff);
    assert!(dst_path.exists(), "COG output should exist on disk");

    // Verify dimensions are preserved.
    assert_eq!(dst_ds.width(), src_ds.width());
    assert_eq!(dst_ds.height(), src_ds.height());

    // Open with CogReader — successfully parsing the file confirms it is a
    // structurally valid tiled TIFF.  The CogWriter::write() already ran its
    // built-in COG validator before returning, so a successful conversion is
    // sufficient proof of compliance.
    let source = FileDataSource::open(&dst_path).expect("open COG source");
    let cog_reader = CogReader::open(source).expect("output should be a valid COG");
    assert!(
        cog_reader.overview_count() > 0,
        "COG output should have at least one overview level (requested [2])"
    );
    assert!(
        cog_reader.tile_size().is_some(),
        "COG output should be tiled (tile_size must be set)"
    );
}

/// `Dataset::convert` with LZW compression produces a readable GeoTIFF.
///
/// Also verifies that the output TIFF has the LZW compression tag (code 5)
/// written into the file.
#[cfg(feature = "geotiff")]
#[test]
fn test_dataset_convert_geotiff_with_lzw_compression() {
    use oxigeo::core_types::io::FileDataSource;
    use oxigeo::geotiff::tiff::{Compression as TiffCompression, ImageInfo, TiffFile};
    use oxigeo::{Compression, ConversionOptions};

    let src_path = write_synthetic_tiff("test_convert_lzw_src.tif");
    let dst_path = std::env::temp_dir().join("test_convert_lzw_dst.tif");
    let src_ds = oxigeo::Dataset::open(src_path.to_str().expect("path")).expect("open src");

    let opts = ConversionOptions {
        compression: Some(Compression::Lzw),
        ..ConversionOptions::default()
    };

    let dst_ds = src_ds
        .convert(&dst_path, oxigeo::DatasetFormat::GeoTiff, opts)
        .expect("convert with LZW");

    assert_eq!(dst_ds.format(), oxigeo::DatasetFormat::GeoTiff);
    assert!(
        dst_path.exists(),
        "LZW-compressed TIFF should exist on disk"
    );
    // The converted dataset must still have the same dimensions.
    assert_eq!(dst_ds.width(), src_ds.width());
    assert_eq!(dst_ds.height(), src_ds.height());

    // Verify that the TIFF compression tag is actually LZW (code 5).
    let source = FileDataSource::open(&dst_path).expect("open dst source");
    let tiff = TiffFile::parse(&source).expect("parse TIFF");
    let variant = tiff.header.variant;
    let img_info = ImageInfo::from_ifd::<FileDataSource>(
        tiff.primary_ifd(),
        &source,
        tiff.byte_order(),
        variant,
    )
    .expect("read image info");
    assert_eq!(
        img_info.compression,
        TiffCompression::Lzw,
        "output TIFF compression should be LZW (code 5), got {:?}",
        img_info.compression,
    );
}

/// `Dataset::convert` GeoJSON→GeoJSON succeeds (file-copy path).
#[cfg(feature = "geojson")]
#[test]
fn test_dataset_convert_geojson_to_geojson() {
    use oxigeo::ConversionOptions;
    use std::io::Write;

    let dir = std::env::temp_dir();
    let src_path = dir.join("test_convert_gj_src.geojson");
    let dst_path = dir.join("test_convert_gj_dst.geojson");

    let content = br#"{"type":"FeatureCollection","features":[
        {"type":"Feature","geometry":{"type":"Point","coordinates":[10,20]},"properties":{"name":"A"}}
    ]}"#;
    std::fs::File::create(&src_path)
        .and_then(|mut f| f.write_all(content))
        .expect("write src geojson");

    let src_ds = oxigeo::Dataset::open(src_path.to_str().expect("path")).expect("open src");
    let dst_ds = src_ds
        .convert(
            &dst_path,
            oxigeo::DatasetFormat::GeoJson,
            ConversionOptions::default(),
        )
        .expect("convert GeoJSON→GeoJSON");

    assert_eq!(dst_ds.format(), oxigeo::DatasetFormat::GeoJson);
    assert!(dst_path.exists(), "output GeoJSON should exist on disk");
}

/// `Dataset::convert` GeoJSON→Shapefile: 2 features round-trip through
/// the converter and are visible when the output Shapefile is re-opened.
#[cfg(all(feature = "geojson", feature = "shapefile"))]
#[test]
fn test_dataset_convert_geojson_to_shapefile() {
    use oxigeo::ConversionOptions;
    use std::io::Write;

    let dir = std::env::temp_dir();
    let src_path = dir.join("test_convert_gj2shp_src.geojson");
    let dst_path = dir.join("test_convert_gj2shp_dst.shp");

    // Two Point features with a string property.
    let content = br#"{"type":"FeatureCollection","features":[
        {"type":"Feature","geometry":{"type":"Point","coordinates":[10.0,20.0]},"properties":{"name":"Alpha"}},
        {"type":"Feature","geometry":{"type":"Point","coordinates":[30.0,40.0]},"properties":{"name":"Beta"}}
    ]}"#;
    std::fs::File::create(&src_path)
        .and_then(|mut f| f.write_all(content))
        .expect("write src geojson");

    let src_ds = oxigeo::Dataset::open(src_path.to_str().expect("src path")).expect("open src");
    let dst_ds = src_ds
        .convert(
            &dst_path,
            oxigeo::DatasetFormat::Shapefile,
            ConversionOptions::default(),
        )
        .expect("convert GeoJSON→Shapefile");

    assert_eq!(dst_ds.format(), oxigeo::DatasetFormat::Shapefile);
    assert!(dst_path.exists(), "output Shapefile should exist on disk");

    // Re-open the output Shapefile via Dataset and verify feature count.
    let reopened = oxigeo::Dataset::open(dst_path.to_str().expect("dst path")).expect("reopen shp");
    let info = reopened.info();
    assert_eq!(
        info.feature_count,
        Some(2),
        "Shapefile produced from 2-feature GeoJSON should report feature_count=2, got {:?}",
        info.feature_count,
    );
}

// ─── Item 7 (new): Dataset::info() tests for Shapefile / FlatGeobuf / GeoParquet ─

/// `Dataset::info()` for a Shapefile has populated `feature_count` and `bounds`.
#[cfg(feature = "shapefile")]
#[test]
fn test_info_shapefile_populated() {
    use oxigeo_core::vector::{Coordinate, Feature as CoreFeature, FieldValue, Geometry, Point};
    use oxigeo_shapefile::{
        ShapeType, ShapefileWriter,
        dbf::{FieldDescriptor, FieldType},
    };

    let dir = std::env::temp_dir();
    let base = dir.join("test_info_shapefile");

    // Write a minimal shapefile with two Point features.
    let field = FieldDescriptor::new("name".to_string(), FieldType::Character, 20, 0)
        .expect("field descriptor");
    let mut writer = ShapefileWriter::new(&base, ShapeType::Point, vec![field])
        .expect("create shapefile writer");

    let mut f1 = CoreFeature::new(Geometry::Point(Point::from_coord(Coordinate {
        x: 10.0,
        y: 20.0,
        z: None,
        m: None,
    })));
    f1.set_property("name", FieldValue::String("A".to_string()));

    let mut f2 = CoreFeature::new(Geometry::Point(Point::from_coord(Coordinate {
        x: 30.0,
        y: 40.0,
        z: None,
        m: None,
    })));
    f2.set_property("name", FieldValue::String("B".to_string()));

    writer
        .write_oxigeo_features(&[f1, f2])
        .expect("write features");

    // Re-open via Dataset
    let shp_path = base.with_extension("shp");
    let ds = oxigeo::Dataset::open(shp_path.to_str().expect("path")).expect("open shp");
    let info = ds.info();

    assert_eq!(
        info.feature_count,
        Some(2),
        "shapefile with 2 features should report feature_count=2"
    );
    assert!(
        info.bounds.is_some(),
        "shapefile bounds should be populated"
    );
    let b = info.bounds.expect("bounds");
    assert!((b.min_x - 10.0).abs() < 1e-6, "min_x should be 10.0");
    assert!((b.max_x - 30.0).abs() < 1e-6, "max_x should be 30.0");
}

/// `Dataset::info()` for a GeoParquet file has populated `feature_count`.
///
/// This test writes a minimal GeoParquet file using `oxigeo_geoparquet` and
/// verifies that `Dataset::info()` reports the correct row count.
#[cfg(feature = "geoparquet")]
#[test]
fn test_info_geoparquet_populated() {
    use oxigeo_geoparquet::geometry::{Geometry as ParquetGeom, Point as ParquetPoint};
    use oxigeo_geoparquet::{GeoParquetWriter, GeometryColumnMetadata};

    let dir = std::env::temp_dir();
    let path = dir.join("test_info_geoparquet.parquet");

    // Create a minimal GeoParquet file with 3 Point features using the writer.
    {
        let meta = GeometryColumnMetadata::new_wkb();
        let mut writer =
            GeoParquetWriter::new(&path, "geometry", meta).expect("create parquet writer");

        for i in 0u8..3 {
            let point = ParquetGeom::Point(ParquetPoint::new_2d(i as f64, i as f64));
            writer.add_geometry(&point).expect("add geometry");
        }
        writer.finish().expect("finish writer");
    }

    let ds = oxigeo::Dataset::open(path.to_str().expect("path")).expect("open parquet");
    let info = ds.info();

    assert_eq!(
        info.feature_count,
        Some(3),
        "GeoParquet with 3 rows should report feature_count=3, got {:?}",
        info.feature_count
    );
}

/// `Dataset::info()` for a FlatGeobuf file has populated `feature_count` and `bounds`.
#[cfg(feature = "flatgeobuf")]
#[test]
fn test_info_flatgeobuf_populated() {
    use oxigeo_core::vector::{Coordinate, Feature as CoreFeature, FieldValue, Geometry, Point};
    use oxigeo_flatgeobuf::{FlatGeobufWriterBuilder, GeometryType as FgbGeomType};

    let dir = std::env::temp_dir();
    let path = dir.join("test_info_flatgeobuf.fgb");

    // Write a minimal FlatGeobuf with 2 Point features.
    {
        let file = std::fs::File::create(&path).expect("create fgb");
        // with_index enables bbox tracking so extent is written to the header.
        let mut writer = FlatGeobufWriterBuilder::new(FgbGeomType::Point)
            .with_index()
            .build(std::io::BufWriter::new(file))
            .expect("create writer");

        let mut f1 = CoreFeature::new(Geometry::Point(Point::from_coord(Coordinate {
            x: 5.0,
            y: 15.0,
            z: None,
            m: None,
        })));
        f1.set_property("id", FieldValue::Integer(1));

        let mut f2 = CoreFeature::new(Geometry::Point(Point::from_coord(Coordinate {
            x: 25.0,
            y: 35.0,
            z: None,
            m: None,
        })));
        f2.set_property("id", FieldValue::Integer(2));

        writer.add_feature(&f1).expect("add feature 1");
        writer.add_feature(&f2).expect("add feature 2");
        writer.finish().expect("finish writer");
    }

    let ds = oxigeo::Dataset::open(path.to_str().expect("path")).expect("open fgb");
    let info = ds.info();

    assert_eq!(
        info.feature_count,
        Some(2),
        "FlatGeobuf with 2 features should report feature_count=2, got {:?}",
        info.feature_count
    );
    assert!(
        info.bounds.is_some(),
        "FlatGeobuf bounds should be populated"
    );
}

// ─── Item 8: Dataset::build_vrt tests ─────────────────────────────────────────

/// `Dataset::build_vrt` with a single source produces a valid VRT file.
#[cfg(feature = "geotiff")]
#[test]
fn test_build_vrt_single_source() {
    use oxigeo::vrt_builder::VrtOptions;

    let src_path = write_synthetic_tiff("test_vrt_single_src.tif");
    let vrt_path = std::env::temp_dir().join("test_vrt_single.vrt");

    let ds = oxigeo::Dataset::build_vrt(&[src_path.as_path()], &vrt_path, VrtOptions::default())
        .expect("build_vrt");

    assert_eq!(ds.format(), oxigeo::DatasetFormat::Vrt);
    assert!(vrt_path.exists(), "VRT file should exist on disk");
    assert!(ds.width() > 0, "VRT width should be non-zero");
    assert!(ds.height() > 0, "VRT height should be non-zero");

    // Verify VRT XML contains expected elements
    let xml = std::fs::read_to_string(&vrt_path).expect("read vrt");
    assert!(
        xml.contains("<VRTDataset"),
        "VRT file should have VRTDataset element"
    );
    assert!(
        xml.contains("VRTRasterBand"),
        "VRT should have VRTRasterBand"
    );
    // The synthetic source is Float32, so the VRT dataType must be Float32.
    assert!(
        xml.contains("dataType=\"Float32\""),
        "Float32 source should yield dataType=\"Float32\", got: {xml}"
    );
}

/// `Dataset::build_vrt` emits the source's real pixel type, not a hardcoded
/// `Float32`.
///
/// Regression for `read_source_meta` unconditionally stamping every band as
/// `Float32`, which made VRT readers misinterpret UInt8 imagery as 4-byte floats.
#[cfg(feature = "geotiff")]
#[test]
fn test_build_vrt_preserves_uint8_datatype() {
    use oxigeo::vrt_builder::VrtOptions;

    let src_path = write_multiband_tiff("test_vrt_uint8_src.tif", 1, None);
    let vrt_path = std::env::temp_dir().join("test_vrt_uint8.vrt");

    oxigeo::Dataset::build_vrt(&[src_path.as_path()], &vrt_path, VrtOptions::default())
        .expect("build_vrt");

    let xml = std::fs::read_to_string(&vrt_path).expect("read vrt");
    // GDAL spells 8-bit unsigned as "Byte".
    assert!(
        xml.contains("dataType=\"Byte\""),
        "UInt8 source should yield dataType=\"Byte\", got: {xml}"
    );
    assert!(
        !xml.contains("dataType=\"Float32\""),
        "UInt8 source must not be mislabeled as Float32: {xml}"
    );
}

/// `Dataset::build_vrt` with two same-CRS TIFFs produces a union-extent VRT.
#[cfg(feature = "geotiff")]
#[test]
fn test_build_vrt_two_tiffs_union_extent() {
    use oxigeo::core_types::types::{GeoTransform, NoDataValue, RasterDataType};
    use oxigeo::geotiff::{
        GeoTiffWriter, GeoTiffWriterOptions, OverviewResampling, WriterConfig,
        tiff::{Compression, PhotometricInterpretation, Predictor},
    };
    use oxigeo::vrt_builder::VrtOptions;

    // Write two 8×8 TIFFs side by side
    let dir = std::env::temp_dir();

    let config_a = WriterConfig {
        width: 8,
        height: 8,
        band_count: 1,
        data_type: RasterDataType::Float32,
        compression: Compression::None,
        predictor: Predictor::None,
        tile_width: None,
        tile_height: None,
        photometric: PhotometricInterpretation::BlackIsZero,
        geo_transform: Some(GeoTransform::north_up(0.0, 8.0, 1.0, 1.0)),
        epsg_code: Some(4326),
        nodata: NoDataValue::None,
        use_bigtiff: false,
        generate_overviews: false,
        overview_resampling: OverviewResampling::Average,
        overview_levels: vec![],
    };
    let config_b = WriterConfig {
        geo_transform: Some(GeoTransform::north_up(8.0, 8.0, 1.0, 1.0)),
        ..config_a.clone()
    };

    let path_a = dir.join("test_vrt_union_a.tif");
    let path_b = dir.join("test_vrt_union_b.tif");
    let vrt_path = dir.join("test_vrt_union.vrt");

    let pixel_data = vec![0u8; 8 * 8 * 4];
    let mut w_a = GeoTiffWriter::create(&path_a, config_a, GeoTiffWriterOptions::default())
        .expect("create a");
    w_a.write(&pixel_data).expect("write a");

    let mut w_b = GeoTiffWriter::create(&path_b, config_b, GeoTiffWriterOptions::default())
        .expect("create b");
    w_b.write(&pixel_data).expect("write b");

    let ds = oxigeo::Dataset::build_vrt(
        &[path_a.as_path(), path_b.as_path()],
        &vrt_path,
        VrtOptions::default(),
    )
    .expect("build_vrt");

    // Union of two 8×8 side-by-side → 16×8
    assert!(ds.width() >= 8, "union width >= 8");
    assert!(ds.height() >= 8, "union height >= 8");

    let xml = std::fs::read_to_string(&vrt_path).expect("read vrt");
    // Both source files should appear in the VRT (may appear multiple times for multi-band)
    assert!(
        xml.contains("SourceFilename"),
        "VRT should reference source files"
    );
    // Each source file appears at least once
    let src_a = path_a.file_name().expect("a filename").to_string_lossy();
    let src_b = path_b.file_name().expect("b filename").to_string_lossy();
    assert!(
        xml.contains(src_a.as_ref()),
        "VRT should reference source file A"
    );
    assert!(
        xml.contains(src_b.as_ref()),
        "VRT should reference source file B"
    );
}

/// `Dataset::build_vrt` returns error for empty sources.
#[test]
fn test_build_vrt_empty_sources_errors() {
    use oxigeo::vrt_builder::VrtOptions;

    let vrt_path = std::env::temp_dir().join("test_vrt_empty.vrt");
    let result = oxigeo::Dataset::build_vrt(&[], &vrt_path, VrtOptions::default());
    assert!(
        result.is_err(),
        "build_vrt with empty sources should return Err"
    );
}

// ─── Item 10: GDAL compat shim tests ─────────────────────────────────────────

/// GDALAllRegister and GDALVersionInfo are accessible.
#[cfg(feature = "gdal-compat")]
#[test]
fn test_gdal_compat_version_and_register() {
    // No panics; version is a non-empty string
    oxigeo::gdal_compat::GDALAllRegister();
    let v = oxigeo::gdal_compat::GDALVersionInfo();
    assert!(!v.is_empty());
}

/// GDALOpen on a nonexistent path returns Err.
#[cfg(feature = "gdal-compat")]
#[test]
fn test_gdal_compat_open_nonexistent_errors() {
    let result = oxigeo::gdal_compat::GDALOpen("/nonexistent/path/file.tif");
    assert!(
        result.is_err(),
        "GDALOpen on nonexistent path should return Err"
    );
}
