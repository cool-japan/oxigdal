//! Integration tests for GeoTIFF → GeoTIFF conversion (Item 7).
//!
//! These tests drive the underlying raster utilities and geotiff drivers directly
//! since oxigeo-cli exposes them via its lib target (`oxigeo_cli::util`).

use oxigeo_cli::util::raster::{self, CogWriteOptions};
use oxigeo_core::{
    buffer::RasterBuffer,
    io::FileDataSource,
    types::{GeoTransform, NoDataValue, RasterDataType},
};
use oxigeo_geotiff::{
    Compression, GeoTiffReader, GeoTiffWriter, GeoTiffWriterOptions, WriterConfig,
};
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

/// Writes an interleaved multi-band UInt8 GeoTIFF directly via the driver's
/// writer, forcing either a tiled or striped layout. `raster::write_multi_band`
/// always produces a tiled file (`WriterConfig::new` defaults to 256x256
/// tiles), which would leave the striped code path in `read_band` /
/// `read_band_region` untested.
///
/// `band_values[b]` is the constant sample value written for band `b` at
/// every pixel, so a correct single-band read of band `b` must return a
/// buffer filled entirely with `band_values[b]`.
fn write_interleaved_multiband(
    path: &std::path::Path,
    width: u64,
    height: u64,
    band_values: &[u8],
    tiled: bool,
) -> anyhow::Result<()> {
    let band_count = band_values.len();
    let pixel_count = (width * height) as usize;
    let mut data = vec![0u8; pixel_count * band_count];
    for pixel in 0..pixel_count {
        for (band_idx, &value) in band_values.iter().enumerate() {
            data[pixel * band_count + band_idx] = value;
        }
    }

    let mut config = WriterConfig::new(width, height, band_count as u16, RasterDataType::UInt8);
    config.generate_overviews = false;
    if tiled {
        config.tile_width = Some(64);
        config.tile_height = Some(64);
    } else {
        config.tile_width = None;
        config.tile_height = None;
    }
    config.geo_transform = Some(standard_geotransform());

    let mut writer = GeoTiffWriter::create(path, config, GeoTiffWriterOptions::default())
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    writer.write(&data).map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(())
}

/// Asserts that every byte in `buffer` equals `expected`.
fn assert_band_is_constant(buffer: &RasterBuffer, expected: u8, context: &str) {
    for (i, byte) in buffer.as_bytes().iter().enumerate() {
        assert_eq!(
            *byte, expected,
            "{context}: byte {i} was {byte}, expected constant {expected}"
        );
    }
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
        "oxigeo_cli_conv_{}_{}_{}",
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
// Regression tests: read_band / read_band_region must return the requested
// band's data in isolation, for both tiled and striped multi-band GeoTIFFs.
// ---------------------------------------------------------------------------

const MULTIBAND_VALUES: [u8; 3] = [10, 120, 250];

#[test]
fn test_read_band_multiband_tiled() -> TestResult {
    let out_path = tmp_path("read_band_tiled.tif");
    let _ = std::fs::remove_file(&out_path);
    write_interleaved_multiband(&out_path, 32, 32, &MULTIBAND_VALUES, true)?;

    for (band_idx, &expected) in MULTIBAND_VALUES.iter().enumerate() {
        let band = raster::read_band(&out_path, band_idx as u32)?;
        assert_eq!(band.width(), 32);
        assert_eq!(band.height(), 32);
        assert_eq!(
            band.as_bytes().len(),
            32 * 32,
            "band {band_idx}: buffer should hold exactly one band's worth of data"
        );
        assert_band_is_constant(&band, expected, &format!("tiled band {band_idx}"));
    }

    let _ = std::fs::remove_file(&out_path);
    Ok(())
}

#[test]
fn test_read_band_multiband_striped() -> TestResult {
    let out_path = tmp_path("read_band_striped.tif");
    let _ = std::fs::remove_file(&out_path);
    write_interleaved_multiband(&out_path, 20, 17, &MULTIBAND_VALUES, false)?;

    for (band_idx, &expected) in MULTIBAND_VALUES.iter().enumerate() {
        let band = raster::read_band(&out_path, band_idx as u32)?;
        assert_eq!(band.width(), 20);
        assert_eq!(band.height(), 17);
        assert_eq!(
            band.as_bytes().len(),
            20 * 17,
            "band {band_idx}: buffer should hold exactly one band's worth of data"
        );
        assert_band_is_constant(&band, expected, &format!("striped band {band_idx}"));
    }

    let _ = std::fs::remove_file(&out_path);
    Ok(())
}

#[test]
fn test_read_band_region_multiband_tiled() -> TestResult {
    let out_path = tmp_path("read_band_region_tiled.tif");
    let _ = std::fs::remove_file(&out_path);
    // Wider than one tile (64x64) so the region spans multiple tiles.
    write_interleaved_multiband(&out_path, 96, 80, &MULTIBAND_VALUES, true)?;

    for (band_idx, &expected) in MULTIBAND_VALUES.iter().enumerate() {
        let region = raster::read_band_region(&out_path, band_idx as u32, 10, 5, 50, 40)?;
        assert_eq!(region.width(), 50);
        assert_eq!(region.height(), 40);
        assert_eq!(region.as_bytes().len(), 50 * 40);
        assert_band_is_constant(&region, expected, &format!("tiled region band {band_idx}"));
    }

    let _ = std::fs::remove_file(&out_path);
    Ok(())
}

#[test]
fn test_read_band_region_multiband_striped() -> TestResult {
    let out_path = tmp_path("read_band_region_striped.tif");
    let _ = std::fs::remove_file(&out_path);
    write_interleaved_multiband(&out_path, 40, 30, &MULTIBAND_VALUES, false)?;

    for (band_idx, &expected) in MULTIBAND_VALUES.iter().enumerate() {
        let region = raster::read_band_region(&out_path, band_idx as u32, 5, 3, 20, 15)?;
        assert_eq!(region.width(), 20);
        assert_eq!(region.height(), 15);
        assert_eq!(region.as_bytes().len(), 20 * 15);
        assert_band_is_constant(
            &region,
            expected,
            &format!("striped region band {band_idx}"),
        );
    }

    let _ = std::fs::remove_file(&out_path);
    Ok(())
}

#[test]
fn test_read_band_out_of_range_rejected() -> TestResult {
    let out_path = tmp_path("read_band_oob.tif");
    let _ = std::fs::remove_file(&out_path);
    write_interleaved_multiband(&out_path, 16, 16, &MULTIBAND_VALUES, true)?;

    let err = raster::read_band(&out_path, 3).expect_err("band index 3 is out of range");
    assert!(
        err.to_string().contains("out of range"),
        "unexpected error message: {err}"
    );

    let err = raster::read_band_region(&out_path, 99, 0, 0, 4, 4)
        .expect_err("band index 99 is out of range");
    assert!(
        err.to_string().contains("out of range"),
        "unexpected error message: {err}"
    );

    let _ = std::fs::remove_file(&out_path);
    Ok(())
}

#[test]
fn test_read_band_single_band_still_works() -> TestResult {
    // Single-band files exercise the size == single_band_len fast path in
    // extract_single_band, distinct from the multi-band de-interleaving path.
    let out_path = tmp_path("read_band_single.tif");
    let _ = std::fs::remove_file(&out_path);
    write_interleaved_multiband(&out_path, 24, 24, &[77], true)?;

    let band = raster::read_band(&out_path, 0)?;
    assert_eq!(band.as_bytes().len(), 24 * 24);
    assert_band_is_constant(&band, 77, "single band");

    let _ = std::fs::remove_file(&out_path);
    Ok(())
}

// ---------------------------------------------------------------------------
// Test 7: cloud URI is_cloud_uri classification
// ---------------------------------------------------------------------------
#[test]
fn test_cloud_uri_classification() {
    use oxigeo_cli::util::cloud::is_cloud_uri;

    assert!(is_cloud_uri("s3://bucket/key"));
    assert!(is_cloud_uri("gs://bucket/obj"));
    assert!(is_cloud_uri("az://container/blob"));
    assert!(!is_cloud_uri("/local/path.tif"));
    assert!(!is_cloud_uri("file:///local.tif"));
    assert!(!is_cloud_uri("relative/path.tif"));
    assert!(!is_cloud_uri(""));
}
