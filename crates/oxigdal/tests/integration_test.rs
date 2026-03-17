//! Integration tests for the oxigdal umbrella crate.
//!
//! Tests format detection, conversion matrix, feature-flag re-exports,
//! conversion planning, and edge cases.

use oxigdal::DatasetFormat;
use oxigdal::convert::{
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
    assert_eq!(detect_format("scan.laz").ok(), Some(DatasetFormat::Copc));
}

#[test]
fn detect_las_extension() {
    assert_eq!(detect_format("scan.las").ok(), Some(DatasetFormat::Copc));
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
    let _ = oxigdal::version();
    let count = oxigdal::driver_count();
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
