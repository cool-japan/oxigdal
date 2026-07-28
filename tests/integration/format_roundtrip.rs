//! Format round-trip integration tests
//!
//! Verifies that data survives a write/read round-trip through the *real* OxiGeo
//! drivers without loss. GeoTIFF (`oxigeo-geotiff`), GeoJSON (`oxigeo-geojson`)
//! and Zarr (`oxigeo-zarr`) are exercised against their real reader/writer APIs.
//!
//! NetCDF and GeoParquet round-trips need driver crates that are not
//! dependencies of this test target (`oxigeo-dev-tools`); rather than fake them
//! with a bespoke binary format (as an earlier revision did), they are
//! `#[ignore]`d and return an honest typed error.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use tempfile::TempDir;

use oxigeo_core::io::FileDataSource;
use oxigeo_core::types::RasterDataType;
use oxigeo_geojson::reader::feature_collection_from_str;
use oxigeo_geojson::types::Point as GjPoint;
use oxigeo_geojson::{Feature, FeatureCollection, GeoJsonWriter, Geometry};
use oxigeo_geotiff::tiff::Predictor;
use oxigeo_geotiff::{
    Compression, GeoTiffReader, GeoTiffWriter, GeoTiffWriterOptions, WriterConfig,
};
use oxigeo_zarr::metadata::v3::ArrayMetadataV3;
use oxigeo_zarr::{FilesystemStore, ZarrV3Reader, ZarrV3Writer};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn boxed<E: std::error::Error + Send + Sync + 'static>(e: E) -> Box<dyn std::error::Error> {
    Box::new(e)
}

#[derive(Debug, Clone)]
struct Point {
    x: f64,
    y: f64,
}

// ============================================================================
// GeoTIFF round-trip (real oxigeo-geotiff)
// ============================================================================

#[test]
fn test_geotiff_roundtrip() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let test_file = temp_dir.path().join("test.tif");

    let width = 100usize;
    let height = 100usize;
    let data: Vec<f32> = (0..(width * height)).map(|i| i as f32).collect();

    write_geotiff(&test_file, width, height, &data)?;
    let (read_width, read_height, read_data) = read_geotiff(&test_file)?;

    assert_eq!(width, read_width);
    assert_eq!(height, read_height);
    assert_eq!(data.len(), read_data.len());
    for (i, (&expected, &actual)) in data.iter().zip(read_data.iter()).enumerate() {
        assert!(
            (expected - actual).abs() < 1e-6,
            "geotiff mismatch at {i}: expected {expected}, got {actual}"
        );
    }
    Ok(())
}

fn write_geotiff(path: &Path, width: usize, height: usize, data: &[f32]) -> Result<()> {
    let mut bytes = Vec::with_capacity(data.len() * 4);
    for &v in data {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    let mut config = WriterConfig::new(width as u64, height as u64, 1, RasterDataType::Float32);
    config.compression = Compression::None;
    config.predictor = Predictor::None;
    config.tile_width = None;
    config.tile_height = None;
    config.generate_overviews = false;
    let mut writer =
        GeoTiffWriter::create(path, config, GeoTiffWriterOptions::default()).map_err(boxed)?;
    writer.write(&bytes).map_err(boxed)?;
    Ok(())
}

fn read_geotiff(path: &Path) -> Result<(usize, usize, Vec<f32>)> {
    let source = FileDataSource::open(path).map_err(boxed)?;
    let reader = GeoTiffReader::open(source).map_err(boxed)?;
    let width = reader.width() as usize;
    let height = reader.height() as usize;
    let bytes = reader.read_band(0, 0).map_err(boxed)?;
    let data = bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    Ok((width, height, data))
}

// ============================================================================
// GeoJSON round-trip (real oxigeo-geojson)
// ============================================================================

#[test]
fn test_geojson_roundtrip() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let test_file = temp_dir.path().join("test.geojson");

    let points = vec![
        Point { x: 0.0, y: 0.0 },
        Point { x: 1.0, y: 1.0 },
        Point { x: 2.0, y: 2.0 },
    ];

    write_geojson(&test_file, &points)?;
    let read_points = read_geojson(&test_file)?;

    assert_eq!(points.len(), read_points.len());
    for (expected, actual) in points.iter().zip(read_points.iter()) {
        assert!((expected.x - actual.x).abs() < 1e-6, "x mismatch");
        assert!((expected.y - actual.y).abs() < 1e-6, "y mismatch");
    }
    Ok(())
}

fn write_geojson(path: &Path, points: &[Point]) -> Result<()> {
    let mut fc = FeatureCollection::with_capacity(points.len());
    for p in points {
        let geom = Geometry::Point(GjPoint::new_2d(p.x, p.y).map_err(boxed)?);
        fc.add_feature(Feature::new(Some(geom), None));
    }

    let mut buffer: Vec<u8> = Vec::new();
    {
        let mut writer = GeoJsonWriter::new(&mut buffer);
        writer.write_feature_collection(&fc).map_err(boxed)?;
    }
    std::fs::write(path, &buffer)?;
    Ok(())
}

fn read_geojson(path: &Path) -> Result<Vec<Point>> {
    let content = std::fs::read_to_string(path)?;
    let fc = feature_collection_from_str(&content).map_err(boxed)?;
    let mut points = Vec::with_capacity(fc.features.len());
    for feature in &fc.features {
        match &feature.geometry {
            Some(Geometry::Point(p)) => {
                let coords = &p.coordinates;
                if coords.len() < 2 {
                    return Err("point geometry missing coordinates".into());
                }
                points.push(Point {
                    x: coords[0],
                    y: coords[1],
                });
            }
            other => {
                return Err(format!("expected Point geometry, got {other:?}").into());
            }
        }
    }
    Ok(points)
}

// ============================================================================
// Zarr round-trip (real oxigeo-zarr)
// ============================================================================

#[test]
fn test_zarr_roundtrip() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let test_dir = temp_dir.path().join("test.zarr");

    let (rows, cols) = (10usize, 30usize);
    let data: Vec<f64> = (0..(rows * cols)).map(|i| i as f64).collect();

    write_zarr(&test_dir, rows, cols, &data)?;
    let (read_shape, read_data) = read_zarr(&test_dir)?;

    assert_eq!(read_shape, vec![rows, cols]);
    assert_eq!(data.len(), read_data.len());
    for (expected, actual) in data.iter().zip(read_data.iter()) {
        assert!((expected - actual).abs() < 1e-10, "zarr value mismatch");
    }
    Ok(())
}

fn write_zarr(path: &Path, rows: usize, cols: usize, data: &[f64]) -> Result<()> {
    let mut bytes = Vec::with_capacity(data.len() * 8);
    for &v in data {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    let metadata = ArrayMetadataV3::new(vec![rows, cols], vec![rows, cols], "float64");
    let store = FilesystemStore::create(path).map_err(boxed)?;
    let mut writer = ZarrV3Writer::new(store, "data", metadata).map_err(boxed)?;
    writer.write_chunk(vec![0, 0], bytes).map_err(boxed)?;
    writer.finalize().map_err(boxed)?;
    Ok(())
}

fn read_zarr(path: &Path) -> Result<(Vec<usize>, Vec<f64>)> {
    let store = FilesystemStore::open(path).map_err(boxed)?;
    let reader = ZarrV3Reader::new(store, "data").map_err(boxed)?;
    let shape = reader.shape().to_vec();
    let bytes = reader.read_all().map_err(boxed)?;
    let data = bytes
        .chunks_exact(8)
        .map(|c| f64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
        .collect();
    Ok((shape, data))
}

// ============================================================================
// Round-trips requiring drivers not wired into this test target
// ============================================================================

#[test]
#[ignore = "requires oxigeo-netcdf dev-dependency (not wired into oxigeo-dev-tools)"]
fn test_netcdf_roundtrip() -> Result<()> {
    Err(
        "NetCDF round-trip requires the oxigeo-netcdf driver, which is not a \
         dependency of oxigeo-dev-tools"
            .into(),
    )
}

#[test]
#[ignore = "requires oxigeo-geoparquet dev-dependency (not wired into oxigeo-dev-tools)"]
fn test_geoparquet_roundtrip() -> Result<()> {
    Err(
        "GeoParquet round-trip requires the oxigeo-geoparquet driver, which is \
         not a dependency of oxigeo-dev-tools"
            .into(),
    )
}
