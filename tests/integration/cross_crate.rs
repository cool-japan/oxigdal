//! Cross-crate integration tests
//!
//! Tests that verify APIs work correctly across different OxiGDAL crates.
//!
//! Unlike an earlier revision of this file, these tests exercise the *real*
//! crate APIs (algorithms band-math, projection transforms, driver readers/
//! writers, STAC item construction) rather than local no-op stand-ins, so a
//! broken cross-crate contract actually fails the test.

#![allow(dead_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use oxigdal_algorithms::RasterCalculator;
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::io::FileDataSource;
use oxigdal_core::types::RasterDataType;
use oxigdal_geojson::reader::feature_collection_from_str;
use oxigdal_geotiff::tiff::Predictor;
use oxigdal_geotiff::{
    Compression, GeoTiffReader, GeoTiffWriter, GeoTiffWriterOptions, WriterConfig,
};
use oxigdal_proj::{Coordinate, transform_epsg};
use oxigdal_stac::ItemBuilder;
use oxigdal_stac::chrono::Utc;
use oxigdal_zarr::metadata::v3::ArrayMetadataV3;
use oxigdal_zarr::{FilesystemStore, ZarrV3Reader, ZarrV3Writer};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Test core + algorithms integration.
///
/// Feeds `oxigdal-core` `RasterBuffer`s into the `oxigdal-algorithms` raster
/// calculator and checks the NDVI band-math result against the closed-form
/// value, so a regression in the calculator fails the test.
#[test]
fn test_core_algorithms_integration() -> Result<()> {
    // Distinct NIR / RED values so NDVI is a non-trivial, verifiable number.
    let nir = vec![120.0f32; 64];
    let red = vec![40.0f32; 64];

    let processed = apply_ndvi(&nir, &red)?;

    assert_eq!(processed.len(), nir.len());
    let expected = (120.0f32 - 40.0) / (120.0 + 40.0); // 0.5
    for value in processed {
        assert!(
            (value - expected).abs() < 1e-4,
            "NDVI mismatch: got {value}, expected {expected}"
        );
    }

    Ok(())
}

/// Test core + projection integration.
///
/// Uses the real `oxigdal-proj` EPSG:4326 -> EPSG:3857 transform. The origin
/// maps to the origin, while an off-origin geographic point maps to Web
/// Mercator metres far from zero — an identity stand-in would fail this.
#[test]
fn test_core_projection_integration() -> Result<()> {
    let points = vec![(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)];

    let transformed = transform_points(&points, "EPSG:4326", "EPSG:3857")?;

    assert_eq!(transformed.len(), points.len());
    // Origin is invariant.
    assert!(transformed[0].0.abs() < 1e-3 && transformed[0].1.abs() < 1e-3);
    // 1 degree of longitude at the equator is ~111.32 km in Web Mercator.
    assert!(
        (transformed[1].0 - 111_319.49).abs() < 5.0,
        "unexpected easting: {}",
        transformed[1].0
    );
    // The transform must not be the identity.
    assert!(transformed[2].0 > transformed[1].0);

    Ok(())
}

/// Test drivers + core integration.
///
/// Exercises real GeoTIFF (raster) round-trip, GeoJSON parsing, and Zarr array
/// round-trip, all producing/consuming `oxigdal-core` data structures.
#[test]
fn test_drivers_core_integration() -> Result<()> {
    let temp_dir = tempfile::tempdir()?;

    create_dataset_geotiff(&temp_dir.path().join("core.tif"))?;
    create_dataset_geojson()?;
    create_dataset_zarr(&temp_dir.path().join("core.zarr"))?;

    Ok(())
}

/// Test metadata + STAC integration.
///
/// Builds a real `oxigdal-stac` `Item` from extracted metadata and validates it
/// serializes to a STAC Feature carrying the source CRS.
#[test]
fn test_metadata_stac_integration() -> Result<()> {
    let metadata = Metadata {
        bounds: (0.0, 0.0, 1.0, 1.0),
        crs: "EPSG:4326".to_string(),
        bands: vec!["B1".to_string(), "B2".to_string()],
    };

    let stac_item = convert_to_stac(&metadata)?;

    assert!(stac_item.contains("\"type\":\"Feature\""));
    assert!(stac_item.contains("EPSG:4326"));
    assert!(stac_item.contains("B1"));

    Ok(())
}

// Helper types and functions (real crate integrations)

#[derive(Debug)]
struct Metadata {
    bounds: (f64, f64, f64, f64),
    crs: String,
    bands: Vec<String>,
}

/// Computes NDVI = (NIR - RED) / (NIR + RED) via the real `oxigdal-algorithms`
/// raster calculator over `oxigdal-core` `RasterBuffer`s.
fn apply_ndvi(nir: &[f32], red: &[f32]) -> Result<Vec<f32>> {
    assert_eq!(nir.len(), red.len());
    let n = nir.len() as u64;

    let mut nir_buf = RasterBuffer::zeros(n, 1, RasterDataType::Float32);
    let mut red_buf = RasterBuffer::zeros(n, 1, RasterDataType::Float32);
    for (i, (&a, &b)) in nir.iter().zip(red.iter()).enumerate() {
        nir_buf.set_pixel(i as u64, 0, f64::from(a))?;
        red_buf.set_pixel(i as u64, 0, f64::from(b))?;
    }

    let result = RasterCalculator::evaluate("(B1 - B2) / (B1 + B2)", &[nir_buf, red_buf])
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

    let mut out = Vec::with_capacity(nir.len());
    for i in 0..nir.len() as u64 {
        out.push(result.get_pixel(i, 0)? as f32);
    }
    Ok(out)
}

/// Parses `EPSG:XXXX` into a numeric code.
fn parse_epsg(crs: &str) -> Result<u32> {
    let code = crs
        .trim()
        .strip_prefix("EPSG:")
        .ok_or("expected EPSG:<code> CRS string")?;
    Ok(code.parse::<u32>()?)
}

/// Transforms geographic points via the real `oxigdal-proj` EPSG transform.
fn transform_points(
    points: &[(f64, f64)],
    from_crs: &str,
    to_crs: &str,
) -> Result<Vec<(f64, f64)>> {
    let from = parse_epsg(from_crs)?;
    let to = parse_epsg(to_crs)?;
    let mut out = Vec::with_capacity(points.len());
    for &(x, y) in points {
        let c = transform_epsg(&Coordinate::new(x, y), from, to)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        out.push((c.x, c.y));
    }
    Ok(out)
}

/// Writes then reads back a GeoTIFF via the real `oxigdal-geotiff` driver and
/// verifies the pixel data survives the round-trip.
fn create_dataset_geotiff(path: &Path) -> Result<()> {
    let (width, height) = (16u64, 16u64);
    let sample_count = (width * height) as usize;

    // Deterministic ramp so the round-trip is verifiable.
    let mut bytes = Vec::with_capacity(sample_count * 4);
    for i in 0..sample_count {
        bytes.extend_from_slice(&(i as f32).to_le_bytes());
    }

    let mut config = WriterConfig::new(width, height, 1, RasterDataType::Float32);
    config.compression = Compression::None;
    config.predictor = Predictor::None;
    config.tile_width = None;
    config.tile_height = None;
    config.generate_overviews = false;

    let mut writer = GeoTiffWriter::create(path, config, GeoTiffWriterOptions::default())
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    writer
        .write(&bytes)
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    drop(writer);

    let source =
        FileDataSource::open(path).map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    let reader =
        GeoTiffReader::open(source).map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    assert_eq!(reader.width(), width);
    assert_eq!(reader.height(), height);

    let read_bytes = reader
        .read_band(0, 0)
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    let read: Vec<f32> = read_bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    assert_eq!(read.len(), sample_count);
    for (i, &v) in read.iter().enumerate() {
        assert!(
            (v - i as f32).abs() < 1e-6,
            "geotiff round-trip mismatch at {i}"
        );
    }

    Ok(())
}

/// Parses a real GeoJSON `FeatureCollection` via the `oxigdal-geojson` driver.
fn create_dataset_geojson() -> Result<()> {
    let json = r#"{
        "type": "FeatureCollection",
        "features": [
            {"type":"Feature","geometry":{"type":"Point","coordinates":[1.0,2.0]},"properties":{"id":1}},
            {"type":"Feature","geometry":{"type":"Point","coordinates":[3.0,4.0]},"properties":{"id":2}}
        ]
    }"#;

    let fc =
        feature_collection_from_str(json).map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    assert_eq!(fc.features.len(), 2);
    Ok(())
}

/// Writes then reads back a Zarr v3 array via the real `oxigdal-zarr` driver.
fn create_dataset_zarr(path: &Path) -> Result<()> {
    let (rows, cols) = (8usize, 8usize);
    let n = rows * cols;

    let mut bytes = Vec::with_capacity(n * 4);
    for i in 0..n {
        bytes.extend_from_slice(&(i as f32).to_le_bytes());
    }

    let metadata = ArrayMetadataV3::new(vec![rows, cols], vec![rows, cols], "float32");

    let store =
        FilesystemStore::create(path).map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    let mut writer = ZarrV3Writer::new(store, "data", metadata)
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    writer
        .write_chunk(vec![0, 0], bytes.clone())
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    writer
        .finalize()
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

    let store =
        FilesystemStore::open(path).map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    let reader =
        ZarrV3Reader::new(store, "data").map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    assert_eq!(reader.shape(), &[rows, cols]);
    let read = reader
        .read_all()
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    assert_eq!(read, bytes);

    Ok(())
}

/// Builds a real `oxigdal-stac` `Item` from extracted metadata and serializes it.
fn convert_to_stac(metadata: &Metadata) -> Result<String> {
    let (west, south, east, north) = metadata.bounds;
    let item = ItemBuilder::new("metadata-item")
        .bbox(west, south, east, north)
        .datetime(Utc::now())
        .property("crs", serde_json::Value::String(metadata.crs.clone()))
        .property(
            "bands",
            serde_json::Value::Array(
                metadata
                    .bands
                    .iter()
                    .map(|b| serde_json::Value::String(b.clone()))
                    .collect(),
            ),
        )
        .build()
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

    Ok(serde_json::to_string(&item)?)
}

#[test]
fn test_dev_tools_profiler() -> Result<()> {
    // Test using dev tools with core operations
    use oxigdal_dev_tools::profiler::Profiler;

    let mut profiler = Profiler::new("test_operation");
    profiler.start();

    // Simulate some work
    std::thread::sleep(std::time::Duration::from_millis(10));

    profiler.stop();

    let report = profiler.report();
    assert!(report.contains("Profile Report"));

    Ok(())
}

#[test]
fn test_dev_tools_validator() -> Result<()> {
    use oxigdal_dev_tools::validator::DataValidator;

    // Validate raster dimensions
    let result = DataValidator::validate_raster_dimensions(100, 100, 3);
    assert!(result.passed);

    // Validate bounds
    let result = DataValidator::validate_bounds(0.0, 0.0, 100.0, 100.0);
    assert!(result.passed);

    Ok(())
}

#[test]
fn test_jupyter_kernel() -> Result<()> {
    use oxigdal_jupyter::OxiGdalKernel;

    let mut kernel = OxiGdalKernel::new()?;

    // Execute a magic command
    let result = kernel.execute("%list")?;
    assert_eq!(result.status, "ok");

    Ok(())
}
