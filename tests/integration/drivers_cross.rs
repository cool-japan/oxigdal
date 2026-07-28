//! Cross-Driver Integration Tests
//!
//! Interoperability tests between OxiGeo format drivers.
//!
//! The GeoTIFF <-> Zarr raster pipeline and raster point-sampling tests are
//! wired to the *real* `oxigeo-geotiff` and `oxigeo-zarr` drivers, so those
//! paths genuinely exercise cross-driver conversion and are asserted against
//! independently-computed values.
//!
//! The remaining format pairs (NetCDF, HDF5, Shapefile, GeoParquet, FlatGeobuf,
//! GRIB, VRT, GML, KML, GPKG, PostGIS) require driver crates that are NOT
//! available as dependencies of this test target (`oxigeo-dev-tools`). Rather
//! than fabricate success with local stand-ins — which would inflate the test
//! count while validating nothing — those conversions are `#[ignore]`d and their
//! bodies return an honest typed error. See the crate-level follow-up to add the
//! corresponding driver dev-dependencies and implement them for real.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::error::Error;
use std::path::Path;
use tempfile::TempDir;

use oxigeo_core::io::FileDataSource;
use oxigeo_core::types::RasterDataType;
use oxigeo_geotiff::tiff::Predictor;
use oxigeo_geotiff::{
    Compression, GeoTiffReader, GeoTiffWriter, GeoTiffWriterOptions, WriterConfig,
};
use oxigeo_zarr::metadata::v3::ArrayMetadataV3;
use oxigeo_zarr::{FilesystemStore, ZarrV3Reader, ZarrV3Writer};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn boxed<E: std::error::Error + Send + Sync + 'static>(e: E) -> Box<dyn Error> {
    Box::new(e)
}

/// Honest error returned by the `#[ignore]`d conversions whose driver crate is
/// not wired into this test target.
fn driver_unavailable(driver: &str) -> Box<dyn Error> {
    format!(
        "{driver} driver is not a dependency of oxigeo-dev-tools; real cross-driver \
         conversion cannot be exercised from this test target"
    )
    .into()
}

// ============================================================================
// Real GeoTIFF / Zarr helpers
// ============================================================================

/// Writes a real single-band Float32 GeoTIFF filled with a deterministic ramp
/// (pixel value == its row-major index).
fn write_real_geotiff(path: &Path, width: usize, height: usize) -> Result<()> {
    let sample_count = width * height;
    let mut bytes = Vec::with_capacity(sample_count * 4);
    for i in 0..sample_count {
        bytes.extend_from_slice(&(i as f32).to_le_bytes());
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

/// Reads a real GeoTIFF's first band as `(width, height, f32 samples)`.
fn read_real_geotiff(path: &Path) -> Result<(usize, usize, Vec<f32>)> {
    let source = FileDataSource::open(path).map_err(boxed)?;
    let reader = GeoTiffReader::open(source).map_err(boxed)?;
    let width = reader.width() as usize;
    let height = reader.height() as usize;
    let bytes = reader.read_band(0, 0).map_err(boxed)?;
    Ok((width, height, f32_from_le(&bytes)))
}

fn f32_from_le(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Writes raw little-endian Float32 samples into a real Zarr v3 array.
fn write_real_zarr(path: &Path, rows: usize, cols: usize, samples: &[f32]) -> Result<()> {
    let mut bytes = Vec::with_capacity(samples.len() * 4);
    for &v in samples {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    let metadata = ArrayMetadataV3::new(vec![rows, cols], vec![rows, cols], "float32");
    let store = FilesystemStore::create(path).map_err(boxed)?;
    let mut writer = ZarrV3Writer::new(store, "data", metadata).map_err(boxed)?;
    writer.write_chunk(vec![0, 0], bytes).map_err(boxed)?;
    writer.finalize().map_err(boxed)?;
    Ok(())
}

fn read_real_zarr(path: &Path) -> Result<Vec<f32>> {
    let store = FilesystemStore::open(path).map_err(boxed)?;
    let reader = ZarrV3Reader::new(store, "data").map_err(boxed)?;
    let bytes = reader.read_all().map_err(boxed)?;
    Ok(f32_from_le(&bytes))
}

/// Real GeoTIFF -> Zarr conversion: read every pixel via the GeoTIFF driver,
/// write it back through the Zarr driver.
fn convert_geotiff_to_zarr(input: &Path, output: &Path) -> Result<()> {
    let (width, height, samples) = read_real_geotiff(input)?;
    write_real_zarr(output, height, width, &samples)
}

// ============================================================================
// Real Raster Conversion Tests
// ============================================================================

#[test]
fn test_geotiff_to_zarr_conversion() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let geotiff_path = temp_dir.path().join("input.tif");
    let zarr_path = temp_dir.path().join("output.zarr");

    write_real_geotiff(&geotiff_path, 100, 100)?;
    convert_geotiff_to_zarr(&geotiff_path, &zarr_path)?;

    let (_, _, geotiff_data) = read_real_geotiff(&geotiff_path)?;
    let zarr_data = read_real_zarr(&zarr_path)?;

    assert_eq!(geotiff_data.len(), zarr_data.len());
    assert_eq!(geotiff_data.len(), 100 * 100);
    for (i, (&a, &b)) in geotiff_data.iter().zip(zarr_data.iter()).enumerate() {
        assert!((a - b).abs() < 1e-6, "mismatch at index {i}: {a} != {b}");
    }
    Ok(())
}

#[test]
fn test_streaming_conversion_large_files() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let input_path = temp_dir.path().join("large_input.tif");
    let output_path = temp_dir.path().join("large_output.zarr");

    // 1000x1000 = 1M samples exercised end-to-end through both drivers.
    write_real_geotiff(&input_path, 1000, 1000)?;
    convert_geotiff_to_zarr(&input_path, &output_path)?;

    let zarr_data = read_real_zarr(&output_path)?;
    assert_eq!(zarr_data.len(), 1000 * 1000);
    // Spot-check the ramp survived the round-trip.
    assert!((zarr_data[0] - 0.0).abs() < 1e-6);
    assert!((zarr_data[123_456] - 123_456.0).abs() < 1e-1);
    Ok(())
}

#[test]
fn test_extract_raster_values_at_points() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let raster_path = temp_dir.path().join("input.tif");

    let (width, height) = (100usize, 100usize);
    write_real_geotiff(&raster_path, width, height)?;

    // Read the raster back through the real driver, then sample it at integer
    // pixel coordinates. Ramp value at (col,row) is row*width + col.
    let (w, h, data) = read_real_geotiff(&raster_path)?;
    assert_eq!((w, h), (width, height));

    let sample_points: [(usize, usize); 4] = [(0, 0), (10, 20), (50, 50), (99, 99)];
    for &(col, row) in &sample_points {
        let sampled = data[row * width + col];
        let expected = (row * width + col) as f32;
        assert!(
            (sampled - expected).abs() < 1e-6,
            "sampled raster value at ({col},{row}) = {sampled}, expected {expected}"
        );
    }

    // Build the output GeoJSON exactly as a real point-sampling tool would, and
    // verify the emitted attribute matches the value read from the raster.
    let (col, row) = sample_points[2];
    let value = data[row * width + col];
    let feature = serde_json::json!({
        "type": "FeatureCollection",
        "features": [{
            "type": "Feature",
            "geometry": { "type": "Point", "coordinates": [col as f64, row as f64] },
            "properties": { "raster_value": value }
        }]
    });
    let text = serde_json::to_string(&feature)?;
    let parsed: serde_json::Value = serde_json::from_str(&text)?;
    let emitted = parsed["features"][0]["properties"]["raster_value"]
        .as_f64()
        .ok_or("raster_value attribute missing")?;
    assert!((emitted - f64::from(value)).abs() < 1e-6);
    Ok(())
}

// ============================================================================
// Cross-driver conversions requiring driver crates not wired into this target.
//
// These are honestly `#[ignore]`d and return a typed error instead of
// fabricating success. Implement them for real once the driver crates are added
// as dev-dependencies of oxigeo-dev-tools.
// ============================================================================

#[test]
#[ignore = "requires oxigeo-netcdf dev-dependency (not wired into oxigeo-dev-tools)"]
fn test_zarr_to_netcdf_conversion() -> Result<()> {
    Err(driver_unavailable("NetCDF"))
}

#[test]
#[ignore = "requires oxigeo-netcdf + oxigeo-hdf5 dev-dependencies"]
fn test_netcdf_to_hdf5_conversion() -> Result<()> {
    Err(driver_unavailable("NetCDF/HDF5"))
}

#[test]
#[ignore = "requires oxigeo-hdf5 dev-dependency"]
fn test_hdf5_to_geotiff_extraction() -> Result<()> {
    Err(driver_unavailable("HDF5"))
}

#[test]
#[ignore = "requires oxigeo-grib dev-dependency"]
fn test_geotiff_to_grib_conversion() -> Result<()> {
    Err(driver_unavailable("GRIB"))
}

#[test]
#[ignore = "requires oxigeo-vrt dev-dependency"]
fn test_vrt_mosaic_mixed_formats() -> Result<()> {
    Err(driver_unavailable("VRT"))
}

#[test]
#[ignore = "requires a COG optimizer wired to the real GeoTIFF/Zarr drivers"]
fn test_cog_optimization_cross_format() -> Result<()> {
    Err(driver_unavailable("COG optimizer"))
}

#[test]
#[ignore = "requires oxigeo-shapefile dev-dependency"]
fn test_geojson_to_shapefile_conversion() -> Result<()> {
    Err(driver_unavailable("Shapefile"))
}

#[test]
#[ignore = "requires oxigeo-shapefile + oxigeo-geoparquet dev-dependencies"]
fn test_shapefile_to_geoparquet_conversion() -> Result<()> {
    Err(driver_unavailable("Shapefile/GeoParquet"))
}

#[test]
#[ignore = "requires oxigeo-geoparquet + oxigeo-flatgeobuf dev-dependencies"]
fn test_geoparquet_to_flatgeobuf_conversion() -> Result<()> {
    Err(driver_unavailable("GeoParquet/FlatGeobuf"))
}

#[test]
#[ignore = "requires oxigeo-flatgeobuf + oxigeo-gpkg dev-dependencies"]
fn test_flatgeobuf_to_gpkg_conversion() -> Result<()> {
    Err(driver_unavailable("FlatGeobuf/GPKG"))
}

#[test]
#[ignore = "requires oxigeo-gpkg + oxigeo-postgis and a live PostgreSQL/PostGIS (set PG_TEST_URL)"]
fn test_gpkg_to_postgis_import() -> Result<()> {
    Err(driver_unavailable("GPKG/PostGIS"))
}

#[test]
#[ignore = "requires a KML driver dev-dependency"]
fn test_kml_to_geojson_conversion() -> Result<()> {
    Err(driver_unavailable("KML"))
}

#[test]
#[ignore = "requires a GML driver + oxigeo-shapefile dev-dependencies"]
fn test_gml_to_shapefile_conversion() -> Result<()> {
    Err(driver_unavailable("GML/Shapefile"))
}

#[test]
#[ignore = "requires real vector rasterization wired to oxigeo-algorithms"]
fn test_rasterize_vector_to_geotiff() -> Result<()> {
    Err(driver_unavailable("vector rasterization"))
}

#[test]
#[ignore = "requires real raster polygonization wired to oxigeo-algorithms"]
fn test_polygonize_raster_to_vector() -> Result<()> {
    Err(driver_unavailable("raster polygonization"))
}

#[test]
#[ignore = "requires real vector clipping wired to oxigeo-algorithms + a vector reader"]
fn test_vector_clip_by_raster_extent() -> Result<()> {
    Err(driver_unavailable("vector clip"))
}

#[test]
#[ignore = "requires oxigeo-geoparquet + oxigeo-gpkg dev-dependencies for the vector leg"]
fn test_complete_etl_pipeline() -> Result<()> {
    Err(driver_unavailable("ETL pipeline (GeoParquet/GPKG leg)"))
}

#[test]
#[ignore = "requires a format-agnostic Dataset facade over the real drivers (incl. NetCDF)"]
fn test_format_agnostic_dataset_api() -> Result<()> {
    Err(driver_unavailable("format-agnostic Dataset"))
}
