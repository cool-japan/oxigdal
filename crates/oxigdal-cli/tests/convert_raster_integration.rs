//! Integration tests for GeoTIFF → GeoTIFF conversion (Item 7).
//!
//! These tests drive the underlying raster utilities and geotiff drivers directly
//! since oxigdal-cli exposes them via its lib target (`oxigdal_cli::util`).

use oxigdal_cli::util::raster::{self, CogWriteOptions};
use oxigdal_core::{
    buffer::RasterBuffer,
    io::FileDataSource,
    types::{GeoTransform, NoDataValue, RasterDataType},
};
use oxigdal_geotiff::{Compression, GeoTiffReader};
use std::path::PathBuf;

type TestResult = anyhow::Result<()>;

/// Helper: build a single-band UInt8 RasterBuffer filled with a ramp pattern.
fn make_uint8_band(width: u64, height: u64) -> anyhow::Result<RasterBuffer> {
    let pixels: Vec<u8> = (0..(width * height) as usize)
        .map(|i| (i % 256) as u8)
        .collect();
    RasterBuffer::new(
        pixels,
        width,
        height,
        RasterDataType::UInt8,
        NoDataValue::None,
    )
    .map_err(|e| anyhow::anyhow!("{}", e))
}

/// Helper: build a standard geo-transform.
fn standard_geotransform() -> GeoTransform {
    GeoTransform {
        origin_x: 0.0,
        origin_y: 100.0,
        pixel_width: 1.0,
        pixel_height: -1.0,
        row_rotation: 0.0,
        col_rotation: 0.0,
    }
}

/// Creates a temporary path that is unique per process and invocation.
fn tmp_path(filename: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut p = std::env::temp_dir();
    p.push(format!(
        "oxigdal_cli_conv_{}_{}_{}",
        std::process::id(),
        unique,
        filename
    ));
    p
}

// ---------------------------------------------------------------------------
// Test 1: GeoTIFF identity conversion (write then read back via read_raster_info)
// ---------------------------------------------------------------------------
#[test]
fn test_convert_geotiff_identity() -> TestResult {
    let band = make_uint8_band(64, 64)?;
    let out_path = tmp_path("identity.tif");
    let _ = std::fs::remove_file(&out_path);

    raster::write_multi_band(
        &out_path,
        &[band],
        Some(standard_geotransform()),
        Some(4326),
        None,
    )?;

    assert!(out_path.exists(), "output file should exist after write");

    let info = raster::read_raster_info(&out_path)?;
    assert_eq!(info.width, 64, "width mismatch");
    assert_eq!(info.height, 64, "height mismatch");
    assert_eq!(info.bands, 1, "band count mismatch");
    assert_eq!(info.epsg_code, Some(4326), "EPSG mismatch");

    let _ = std::fs::remove_file(&out_path);
    Ok(())
}

// ---------------------------------------------------------------------------
// Test 2: COG write round-trips through read_raster_info
// ---------------------------------------------------------------------------
#[test]
fn test_convert_cog_roundtrip() -> TestResult {
    let band = make_uint8_band(256, 256)?;
    let out_path = tmp_path("cog_roundtrip.tif");
    let _ = std::fs::remove_file(&out_path);

    let options = CogWriteOptions {
        geo_transform: Some(standard_geotransform()),
        epsg_code: Some(4326),
        no_data_value: None,
        overview_levels: vec![2, 4],
        tile_size: 256,
        compression: Compression::Lzw,
    };

    raster::write_raster_cog(&out_path, &[band], options)?;

    assert!(out_path.exists(), "COG output should exist");

    let info = raster::read_raster_info(&out_path)?;
    assert_eq!(info.width, 256);
    assert_eq!(info.height, 256);
    assert_eq!(info.bands, 1);
    assert_eq!(info.epsg_code, Some(4326));

    let _ = std::fs::remove_file(&out_path);
    Ok(())
}

// ---------------------------------------------------------------------------
// Test 3: DEFLATE compression COG write + read
// ---------------------------------------------------------------------------
#[test]
fn test_convert_compression_deflate() -> TestResult {
    let band = make_uint8_band(256, 256)?;
    let out_path = tmp_path("cog_deflate.tif");
    let _ = std::fs::remove_file(&out_path);

    let options = CogWriteOptions {
        geo_transform: None,
        epsg_code: None,
        no_data_value: None,
        overview_levels: Vec::new(),
        tile_size: 256,
        compression: Compression::AdobeDeflate,
    };

    raster::write_raster_cog(&out_path, &[band], options)?;

    let info = raster::read_raster_info(&out_path)?;
    assert_eq!(info.width, 256);
    assert_eq!(info.height, 256);

    let _ = std::fs::remove_file(&out_path);
    Ok(())
}

// ---------------------------------------------------------------------------
// Test 4: multi-band COG write (3 bands)
// ---------------------------------------------------------------------------
#[test]
fn test_convert_cog_multiband() -> TestResult {
    let r = make_uint8_band(128, 128)?;
    let g = make_uint8_band(128, 128)?;
    let b = make_uint8_band(128, 128)?;
    let out_path = tmp_path("cog_multiband.tif");
    let _ = std::fs::remove_file(&out_path);

    let options = CogWriteOptions {
        geo_transform: Some(standard_geotransform()),
        epsg_code: None,
        no_data_value: None,
        overview_levels: Vec::new(),
        tile_size: 128,
        compression: Compression::Lzw,
    };

    raster::write_raster_cog(&out_path, &[r, g, b], options)?;

    let info = raster::read_raster_info(&out_path)?;
    assert_eq!(info.width, 128);
    assert_eq!(info.height, 128);
    assert_eq!(info.bands, 3);

    let _ = std::fs::remove_file(&out_path);
    Ok(())
}

// ---------------------------------------------------------------------------
// Test 5: tile size is respected
// ---------------------------------------------------------------------------
#[test]
fn test_convert_tile_size_respected() -> TestResult {
    let band = make_uint8_band(128, 128)?;
    let out_path = tmp_path("cog_tile128.tif");
    let _ = std::fs::remove_file(&out_path);

    let options = CogWriteOptions {
        geo_transform: None,
        epsg_code: None,
        no_data_value: None,
        overview_levels: Vec::new(),
        tile_size: 128,
        compression: Compression::Lzw,
    };

    raster::write_raster_cog(&out_path, &[band], options)?;

    let source =
        FileDataSource::open(&out_path).map_err(|e| anyhow::anyhow!("open datasource: {}", e))?;
    let reader =
        GeoTiffReader::open(source).map_err(|e| anyhow::anyhow!("open geotiff reader: {}", e))?;
    let tile_size = reader.tile_size();
    assert!(tile_size.is_some(), "COG must be tiled");
    let (tw, th) = tile_size.ok_or_else(|| anyhow::anyhow!("tile size missing"))?;
    assert_eq!(tw, 128, "tile width should be 128");
    assert_eq!(th, 128, "tile height should be 128");

    let _ = std::fs::remove_file(&out_path);
    Ok(())
}

// ---------------------------------------------------------------------------
// Test 6: write_raster_cog rejects empty band list
// ---------------------------------------------------------------------------
#[test]
fn test_convert_cog_rejects_empty_bands() {
    let out_path = tmp_path("cog_empty.tif");
    let result = raster::write_raster_cog(&out_path, &[], CogWriteOptions::default());
    assert!(
        result.is_err(),
        "write_raster_cog should error on empty band list"
    );
}

// ---------------------------------------------------------------------------
// Test 7: cloud URI is_cloud_uri classification
// ---------------------------------------------------------------------------
#[test]
fn test_cloud_uri_classification() {
    use oxigdal_cli::util::cloud::is_cloud_uri;

    assert!(is_cloud_uri("s3://bucket/key"));
    assert!(is_cloud_uri("gs://bucket/obj"));
    assert!(is_cloud_uri("az://container/blob"));
    assert!(!is_cloud_uri("/local/path.tif"));
    assert!(!is_cloud_uri("file:///local.tif"));
    assert!(!is_cloud_uri("relative/path.tif"));
    assert!(!is_cloud_uri(""));
}
