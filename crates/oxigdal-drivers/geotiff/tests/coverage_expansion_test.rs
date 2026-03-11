//! Expanded integration tests for GeoTIFF driver - Core Driver Test Coverage
//!
//! This module adds 20+ additional tests covering:
//! - Write and read back various data types (u8, u16, u32, f32, f64)
//! - Multi-band (1, 3, 4 bands)
//! - Various compressions (none, deflate, lzw)
//! - Strip vs tile organization
//! - Edge case dimensions (1x1, 1xN, Nx1)
//! - Metadata/tags round-trip
//! - Error handling (corrupt header, missing data)
//! - Coordinate system / geotransform round-trip

#![allow(clippy::expect_used, clippy::float_cmp)]

use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

use oxigdal_core::io::FileDataSource;
use oxigdal_core::types::{GeoTransform, NoDataValue, RasterDataType};
use oxigdal_geotiff::GeoTiffReader;
use oxigdal_geotiff::tiff::{ByteOrderType, TiffHeader};
use oxigdal_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};

/// Helper function to create a temporary test file with unique prefix
fn temp_test_file(name: &str) -> PathBuf {
    let mut path = env::temp_dir();
    path.push(format!("oxigdal_coverage_{}", name));
    path
}

// ============================================================
// Data type round-trip tests
// ============================================================

/// Test 1: UInt8 round-trip (tiled)
#[test]
#[cfg(feature = "lzw")]
fn test_coverage_uint8_roundtrip() {
    let path = temp_test_file("dt_u8.tif");
    let width = 32u64;
    let height = 32u64;
    let mut data = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            data.push(((x * 7 + y * 3) % 256) as u8);
        }
    }

    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(oxigdal_geotiff::Compression::Lzw)
            .with_tile_size(16, 16)
            .with_overviews(false, oxigdal_geotiff::writer::OverviewResampling::Nearest);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer for UInt8 roundtrip");
        writer.write(&data).expect("Should write UInt8 data");
    }

    {
        let source = FileDataSource::open(&path).expect("Should open UInt8 file");
        let reader = GeoTiffReader::new(source).expect("Should create reader for UInt8");
        assert_eq!(reader.data_type(), Some(RasterDataType::UInt8));
        assert_eq!(reader.width(), width);
        assert_eq!(reader.height(), height);
        let read_data = reader.read_band(0, 0).expect("Should read UInt8 band");
        assert_eq!(read_data, data, "UInt8 data should match exactly");
    }

    let _ = std::fs::remove_file(path);
}

/// Test 2: UInt16 round-trip
#[test]
#[cfg(feature = "lzw")]
fn test_coverage_uint16_roundtrip() {
    let path = temp_test_file("dt_u16.tif");
    let width = 32u64;
    let height = 32u64;
    let mut data = Vec::with_capacity((width * height * 2) as usize);
    for y in 0..height {
        for x in 0..width {
            let value = ((x * 1000 + y * 500) % 65536) as u16;
            data.extend_from_slice(&value.to_le_bytes());
        }
    }

    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt16)
            .with_compression(oxigdal_geotiff::Compression::Lzw)
            .with_tile_size(16, 16)
            .with_overviews(false, oxigdal_geotiff::writer::OverviewResampling::Nearest);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer for UInt16 roundtrip");
        writer.write(&data).expect("Should write UInt16 data");
    }

    {
        let source = FileDataSource::open(&path).expect("Should open UInt16 file");
        let reader = GeoTiffReader::new(source).expect("Should create reader for UInt16");
        assert_eq!(reader.data_type(), Some(RasterDataType::UInt16));
        let read_data = reader.read_band(0, 0).expect("Should read UInt16 band");
        assert_eq!(read_data, data, "UInt16 data should match exactly");
    }

    let _ = std::fs::remove_file(path);
}

/// Test 3: UInt32 round-trip
#[test]
#[cfg(feature = "lzw")]
fn test_coverage_uint32_roundtrip() {
    let path = temp_test_file("dt_u32.tif");
    let width = 16u64;
    let height = 16u64;
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let value = (x * 100_000 + y * 10_000) as u32;
            data.extend_from_slice(&value.to_le_bytes());
        }
    }

    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt32)
            .with_compression(oxigdal_geotiff::Compression::Lzw)
            .with_tile_size(16, 16)
            .with_overviews(false, oxigdal_geotiff::writer::OverviewResampling::Nearest);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer for UInt32 roundtrip");
        writer.write(&data).expect("Should write UInt32 data");
    }

    {
        let source = FileDataSource::open(&path).expect("Should open UInt32 file");
        let reader = GeoTiffReader::new(source).expect("Should create reader for UInt32");
        assert_eq!(reader.data_type(), Some(RasterDataType::UInt32));
        let read_data = reader.read_band(0, 0).expect("Should read UInt32 band");
        assert_eq!(read_data, data, "UInt32 data should match exactly");
    }

    let _ = std::fs::remove_file(path);
}

/// Test 4: Float32 round-trip with special values
#[test]
#[cfg(feature = "lzw")]
fn test_coverage_float32_roundtrip() {
    let path = temp_test_file("dt_f32.tif");
    let width = 16u64;
    let height = 16u64;
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let value = (x as f32 * 0.1) + (y as f32 * 0.01) - 0.5;
            data.extend_from_slice(&value.to_le_bytes());
        }
    }

    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::Float32)
            .with_compression(oxigdal_geotiff::Compression::Lzw)
            .with_tile_size(16, 16)
            .with_overviews(false, oxigdal_geotiff::writer::OverviewResampling::Nearest);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer for Float32 roundtrip");
        writer.write(&data).expect("Should write Float32 data");
    }

    {
        let source = FileDataSource::open(&path).expect("Should open Float32 file");
        let reader = GeoTiffReader::new(source).expect("Should create reader for Float32");
        assert_eq!(reader.data_type(), Some(RasterDataType::Float32));
        let read_data = reader.read_band(0, 0).expect("Should read Float32 band");
        assert_eq!(
            read_data.len(),
            data.len(),
            "Float32 data length should match"
        );
        for (i, (orig_chunk, read_chunk)) in data
            .chunks_exact(4)
            .zip(read_data.chunks_exact(4))
            .enumerate()
        {
            let orig =
                f32::from_le_bytes([orig_chunk[0], orig_chunk[1], orig_chunk[2], orig_chunk[3]]);
            let read =
                f32::from_le_bytes([read_chunk[0], read_chunk[1], read_chunk[2], read_chunk[3]]);
            assert!(
                (orig - read).abs() < 1e-6,
                "Float32 mismatch at pixel {}: {} vs {}",
                i,
                orig,
                read
            );
        }
    }

    let _ = std::fs::remove_file(path);
}

/// Test 5: Float64 round-trip
#[test]
#[cfg(feature = "lzw")]
fn test_coverage_float64_roundtrip() {
    let path = temp_test_file("dt_f64.tif");
    let width = 16u64;
    let height = 16u64;
    let mut data = Vec::with_capacity((width * height * 8) as usize);
    for y in 0..height {
        for x in 0..width {
            let value = (x as f64 * 0.001) + (y as f64 * 0.002) - 0.1;
            data.extend_from_slice(&value.to_le_bytes());
        }
    }

    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::Float64)
            .with_compression(oxigdal_geotiff::Compression::Lzw)
            .with_tile_size(16, 16)
            .with_overviews(false, oxigdal_geotiff::writer::OverviewResampling::Nearest);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer for Float64 roundtrip");
        writer.write(&data).expect("Should write Float64 data");
    }

    {
        let source = FileDataSource::open(&path).expect("Should open Float64 file");
        let reader = GeoTiffReader::new(source).expect("Should create reader for Float64");
        assert_eq!(reader.data_type(), Some(RasterDataType::Float64));
        let read_data = reader.read_band(0, 0).expect("Should read Float64 band");
        assert_eq!(read_data, data, "Float64 data should match exactly");
    }

    let _ = std::fs::remove_file(path);
}

// ============================================================
// Multi-band tests
// ============================================================

/// Test 6: Single band grayscale
#[test]
#[cfg(feature = "lzw")]
fn test_coverage_single_band() {
    let path = temp_test_file("band_1.tif");
    let width = 64u64;
    let height = 64u64;
    let data = vec![128u8; (width * height) as usize];

    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(oxigdal_geotiff::Compression::Lzw)
            .with_tile_size(32, 32)
            .with_overviews(false, oxigdal_geotiff::writer::OverviewResampling::Nearest);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer for single band");
        writer.write(&data).expect("Should write single band data");
    }

    {
        let source = FileDataSource::open(&path).expect("Should open single band file");
        let reader = GeoTiffReader::new(source).expect("Should create reader for single band");
        assert_eq!(reader.band_count(), 1);
        let read_data = reader.read_band(0, 0).expect("Should read single band");
        assert_eq!(read_data, data, "Single band data should match");
    }

    let _ = std::fs::remove_file(path);
}

/// Test 7: RGB 3-band
#[test]
#[cfg(feature = "lzw")]
fn test_coverage_three_band_rgb() {
    let path = temp_test_file("band_3.tif");
    let width = 32u64;
    let height = 32u64;
    let mut data = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            data.push(((x * 8) % 256) as u8);
            data.push(((y * 8) % 256) as u8);
            data.push((((x + y) * 4) % 256) as u8);
        }
    }

    {
        let config = WriterConfig::new(width, height, 3, RasterDataType::UInt8)
            .with_compression(oxigdal_geotiff::Compression::Lzw)
            .with_tile_size(16, 16)
            .with_photometric(oxigdal_geotiff::PhotometricInterpretation::Rgb)
            .with_overviews(false, oxigdal_geotiff::writer::OverviewResampling::Nearest);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer for 3-band RGB");
        writer.write(&data).expect("Should write RGB data");
    }

    {
        let source = FileDataSource::open(&path).expect("Should open RGB file");
        let reader = GeoTiffReader::new(source).expect("Should create reader for RGB");
        assert_eq!(reader.band_count(), 3);
        let read_data = reader.read_band(0, 0).expect("Should read RGB band");
        assert_eq!(read_data, data, "RGB data should match");
    }

    let _ = std::fs::remove_file(path);
}

/// Test 8: RGBA 4-band
#[test]
#[cfg(feature = "lzw")]
fn test_coverage_four_band_rgba() {
    let path = temp_test_file("band_4.tif");
    let width = 32u64;
    let height = 32u64;
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            data.push(((x * 8) % 256) as u8);
            data.push(((y * 8) % 256) as u8);
            data.push((((x + y) * 4) % 256) as u8);
            data.push(255);
        }
    }

    {
        let config = WriterConfig::new(width, height, 4, RasterDataType::UInt8)
            .with_compression(oxigdal_geotiff::Compression::Lzw)
            .with_tile_size(16, 16)
            .with_photometric(oxigdal_geotiff::PhotometricInterpretation::Rgb)
            .with_overviews(false, oxigdal_geotiff::writer::OverviewResampling::Nearest);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer for 4-band RGBA");
        writer.write(&data).expect("Should write RGBA data");
    }

    {
        let source = FileDataSource::open(&path).expect("Should open RGBA file");
        let reader = GeoTiffReader::new(source).expect("Should create reader for RGBA");
        assert_eq!(reader.band_count(), 4);
        let read_data = reader.read_band(0, 0).expect("Should read RGBA band");
        assert_eq!(read_data, data, "RGBA data should match");
    }

    let _ = std::fs::remove_file(path);
}

// ============================================================
// Compression tests
// ============================================================

/// Test 9: No compression (PackBits as baseline, since None may not be directly supported)
#[test]
fn test_coverage_packbits_compression() {
    let path = temp_test_file("comp_packbits.tif");
    let width = 64u64;
    let height = 64u64;
    let mut data = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            data.push(((x + y) % 256) as u8);
        }
    }

    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(oxigdal_geotiff::Compression::Packbits)
            .with_tile_size(32, 32)
            .with_overviews(false, oxigdal_geotiff::writer::OverviewResampling::Nearest);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer for PackBits");
        writer.write(&data).expect("Should write PackBits data");
    }

    {
        let source = FileDataSource::open(&path).expect("Should open PackBits file");
        let reader = GeoTiffReader::new(source).expect("Should create reader for PackBits");
        assert_eq!(reader.compression(), oxigdal_geotiff::Compression::Packbits);
        let read_data = reader.read_band(0, 0).expect("Should read PackBits band");
        assert_eq!(read_data, data, "PackBits data should match");
    }

    let _ = std::fs::remove_file(path);
}

/// Test 10: DEFLATE compression
#[test]
#[cfg(feature = "deflate")]
fn test_coverage_deflate_compression() {
    let path = temp_test_file("comp_deflate.tif");
    let width = 64u64;
    let height = 64u64;
    let mut data = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            data.push(((x + y) % 256) as u8);
        }
    }

    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(oxigdal_geotiff::Compression::Deflate)
            .with_tile_size(32, 32)
            .with_overviews(false, oxigdal_geotiff::writer::OverviewResampling::Nearest);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer for DEFLATE");
        writer.write(&data).expect("Should write DEFLATE data");
    }

    {
        let source = FileDataSource::open(&path).expect("Should open DEFLATE file");
        let reader = GeoTiffReader::new(source).expect("Should create reader for DEFLATE");
        assert_eq!(reader.compression(), oxigdal_geotiff::Compression::Deflate);
        let read_data = reader.read_band(0, 0).expect("Should read DEFLATE band");
        assert_eq!(read_data, data, "DEFLATE data should match");
    }

    let _ = std::fs::remove_file(path);
}

/// Test 11: LZW compression
#[test]
#[cfg(feature = "lzw")]
fn test_coverage_lzw_compression() {
    let path = temp_test_file("comp_lzw.tif");
    let width = 64u64;
    let height = 64u64;
    let mut data = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            data.push(((x * 3 + y * 7) % 256) as u8);
        }
    }

    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(oxigdal_geotiff::Compression::Lzw)
            .with_tile_size(32, 32)
            .with_overviews(false, oxigdal_geotiff::writer::OverviewResampling::Nearest);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer for LZW");
        writer.write(&data).expect("Should write LZW data");
    }

    {
        let source = FileDataSource::open(&path).expect("Should open LZW file");
        let reader = GeoTiffReader::new(source).expect("Should create reader for LZW");
        assert_eq!(reader.compression(), oxigdal_geotiff::Compression::Lzw);
        let read_data = reader.read_band(0, 0).expect("Should read LZW band");
        assert_eq!(read_data, data, "LZW data should match");
    }

    let _ = std::fs::remove_file(path);
}

// ============================================================
// Strip vs tile organization
// ============================================================

/// Test 12: Striped layout
#[test]
#[cfg(feature = "lzw")]
fn test_coverage_striped_layout() {
    let path = temp_test_file("layout_striped.tif");
    let width = 64u64;
    let height = 64u64;
    let mut data = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            data.push(((x + y * 2) % 256) as u8);
        }
    }

    {
        let mut config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(oxigdal_geotiff::Compression::Lzw);
        config.tile_width = None;
        config.tile_height = None;
        config.generate_overviews = false;

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer for striped layout");
        writer.write(&data).expect("Should write striped data");
    }

    {
        let source = FileDataSource::open(&path).expect("Should open striped file");
        let reader = GeoTiffReader::new(source).expect("Should create reader for striped");
        assert_eq!(reader.tile_size(), None, "Should be striped (no tile size)");
        assert_eq!(reader.width(), width);
        assert_eq!(reader.height(), height);
    }

    let _ = std::fs::remove_file(path);
}

/// Test 13: Tiled layout
#[test]
#[cfg(feature = "lzw")]
fn test_coverage_tiled_layout() {
    let path = temp_test_file("layout_tiled.tif");
    let width = 64u64;
    let height = 64u64;
    let data = vec![42u8; (width * height) as usize];

    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(oxigdal_geotiff::Compression::Lzw)
            .with_tile_size(32, 32)
            .with_overviews(false, oxigdal_geotiff::writer::OverviewResampling::Nearest);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer for tiled layout");
        writer.write(&data).expect("Should write tiled data");
    }

    {
        let source = FileDataSource::open(&path).expect("Should open tiled file");
        let reader = GeoTiffReader::new(source).expect("Should create reader for tiled");
        assert_eq!(
            reader.tile_size(),
            Some((32, 32)),
            "Should have 32x32 tiles"
        );
        let read_data = reader.read_band(0, 0).expect("Should read tiled band");
        assert_eq!(read_data, data, "Tiled data should match");
    }

    let _ = std::fs::remove_file(path);
}

// ============================================================
// Edge case dimensions
// ============================================================

/// Test 14: 1x1 image
#[test]
#[cfg(feature = "lzw")]
fn test_coverage_1x1_image() {
    let path = temp_test_file("dim_1x1.tif");
    let data = vec![99u8];

    {
        let config = WriterConfig::new(1, 1, 1, RasterDataType::UInt8)
            .with_compression(oxigdal_geotiff::Compression::Lzw)
            .with_tile_size(256, 256)
            .with_overviews(false, oxigdal_geotiff::writer::OverviewResampling::Nearest);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer for 1x1");
        writer.write(&data).expect("Should write 1x1 data");
    }

    {
        let source = FileDataSource::open(&path).expect("Should open 1x1 file");
        let reader = GeoTiffReader::new(source).expect("Should create reader for 1x1");
        assert_eq!(reader.width(), 1);
        assert_eq!(reader.height(), 1);
        let read_data = reader.read_band(0, 0).expect("Should read 1x1 band");
        assert_eq!(read_data, data, "1x1 data should match");
    }

    let _ = std::fs::remove_file(path);
}

/// Test 15: 1xN image (single column)
#[test]
#[cfg(feature = "lzw")]
fn test_coverage_1xn_image() {
    let path = temp_test_file("dim_1x100.tif");
    let width = 1u64;
    let height = 100u64;
    let mut data = Vec::with_capacity(height as usize);
    for y in 0..height {
        data.push((y % 256) as u8);
    }

    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(oxigdal_geotiff::Compression::Lzw)
            .with_tile_size(256, 256)
            .with_overviews(false, oxigdal_geotiff::writer::OverviewResampling::Nearest);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer for 1xN");
        writer.write(&data).expect("Should write 1xN data");
    }

    {
        let source = FileDataSource::open(&path).expect("Should open 1xN file");
        let reader = GeoTiffReader::new(source).expect("Should create reader for 1xN");
        assert_eq!(reader.width(), 1);
        assert_eq!(reader.height(), 100);
        let read_data = reader.read_band(0, 0).expect("Should read 1xN band");
        assert_eq!(read_data, data, "1xN data should match");
    }

    let _ = std::fs::remove_file(path);
}

/// Test 16: Nx1 image (single row)
#[test]
#[cfg(feature = "lzw")]
fn test_coverage_nx1_image() {
    let path = temp_test_file("dim_100x1.tif");
    let width = 100u64;
    let height = 1u64;
    let mut data = Vec::with_capacity(width as usize);
    for x in 0..width {
        data.push((x % 256) as u8);
    }

    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(oxigdal_geotiff::Compression::Lzw)
            .with_tile_size(256, 256)
            .with_overviews(false, oxigdal_geotiff::writer::OverviewResampling::Nearest);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer for Nx1");
        writer.write(&data).expect("Should write Nx1 data");
    }

    {
        let source = FileDataSource::open(&path).expect("Should open Nx1 file");
        let reader = GeoTiffReader::new(source).expect("Should create reader for Nx1");
        assert_eq!(reader.width(), 100);
        assert_eq!(reader.height(), 1);
        let read_data = reader.read_band(0, 0).expect("Should read Nx1 band");
        assert_eq!(read_data, data, "Nx1 data should match");
    }

    let _ = std::fs::remove_file(path);
}

/// Test 17: Non-power-of-two dimensions with tiling
#[test]
#[cfg(feature = "lzw")]
fn test_coverage_non_pow2_dimensions() {
    let path = temp_test_file("dim_nonpow2.tif");
    let width = 37u64;
    let height = 53u64;
    let mut data = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            data.push(((x + y) % 256) as u8);
        }
    }

    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(oxigdal_geotiff::Compression::Lzw)
            .with_tile_size(16, 16)
            .with_overviews(false, oxigdal_geotiff::writer::OverviewResampling::Nearest);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer for non-pow2 dimensions");
        writer.write(&data).expect("Should write non-pow2 data");
    }

    {
        let source = FileDataSource::open(&path).expect("Should open non-pow2 file");
        let reader = GeoTiffReader::new(source).expect("Should create reader for non-pow2");
        assert_eq!(reader.width(), 37);
        assert_eq!(reader.height(), 53);
        let read_data = reader.read_band(0, 0).expect("Should read non-pow2 band");
        assert_eq!(read_data, data, "Non-pow2 data should match");
    }

    let _ = std::fs::remove_file(path);
}

// ============================================================
// Coordinate system / geotransform round-trip
// ============================================================

/// Test 18: GeoTransform with WGS84 (EPSG:4326) round-trip
#[test]
#[cfg(feature = "lzw")]
fn test_coverage_geotransform_wgs84() {
    let path = temp_test_file("geo_wgs84.tif");
    let width = 64u64;
    let height = 64u64;
    let data = vec![128u8; (width * height) as usize];
    let geo_transform = GeoTransform::north_up(-122.4194, 37.7749, 0.001, -0.001);

    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(oxigdal_geotiff::Compression::Lzw)
            .with_tile_size(32, 32)
            .with_geo_transform(geo_transform)
            .with_epsg_code(4326)
            .with_overviews(false, oxigdal_geotiff::writer::OverviewResampling::Nearest);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer for WGS84");
        writer.write(&data).expect("Should write WGS84 data");
    }

    {
        let source = FileDataSource::open(&path).expect("Should open WGS84 file");
        let reader = GeoTiffReader::new(source).expect("Should create reader for WGS84");

        let gt = reader
            .geo_transform()
            .expect("Should have GeoTransform for WGS84");
        assert!(
            (gt.origin_x - (-122.4194)).abs() < 1e-6,
            "Origin X should match for WGS84"
        );
        assert!(
            (gt.origin_y - 37.7749).abs() < 1e-6,
            "Origin Y should match for WGS84"
        );
        assert!(
            (gt.pixel_width - 0.001).abs() < 1e-6,
            "Pixel width should match for WGS84"
        );
        assert!(
            (gt.pixel_height + 0.001).abs() < 1e-6,
            "Pixel height should match for WGS84"
        );

        assert_eq!(reader.epsg_code(), Some(4326), "EPSG should be 4326");
    }

    let _ = std::fs::remove_file(path);
}

/// Test 19: GeoTransform with Web Mercator (EPSG:3857) round-trip
#[test]
#[cfg(feature = "lzw")]
fn test_coverage_geotransform_webmercator() {
    let path = temp_test_file("geo_webmercator.tif");
    let width = 32u64;
    let height = 32u64;
    let data = vec![64u8; (width * height) as usize];
    let geo_transform = GeoTransform::north_up(-13627665.0, 4547675.0, 10.0, -10.0);

    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(oxigdal_geotiff::Compression::Lzw)
            .with_tile_size(32, 32)
            .with_geo_transform(geo_transform)
            .with_epsg_code(3857)
            .with_overviews(false, oxigdal_geotiff::writer::OverviewResampling::Nearest);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer for Web Mercator");
        writer.write(&data).expect("Should write Web Mercator data");
    }

    {
        let source = FileDataSource::open(&path).expect("Should open Web Mercator file");
        let reader = GeoTiffReader::new(source).expect("Should create reader for Web Mercator");

        let gt = reader
            .geo_transform()
            .expect("Should have GeoTransform for Web Mercator");
        assert!(
            (gt.origin_x - (-13627665.0)).abs() < 1e-3,
            "Origin X should match for Web Mercator"
        );
        assert!(
            (gt.origin_y - 4547675.0).abs() < 1e-3,
            "Origin Y should match for Web Mercator"
        );
        assert!(
            (gt.pixel_width - 10.0).abs() < 1e-6,
            "Pixel width should match for Web Mercator"
        );
    }

    let _ = std::fs::remove_file(path);
}

/// Test 20: NoData value round-trip
#[test]
#[cfg(feature = "lzw")]
fn test_coverage_nodata_roundtrip() {
    let path = temp_test_file("nodata_rt.tif");
    let width = 32u64;
    let height = 32u64;
    let data = vec![0u8; (width * height) as usize];

    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(oxigdal_geotiff::Compression::Lzw)
            .with_tile_size(32, 32)
            .with_nodata(NoDataValue::from_integer(255))
            .with_overviews(false, oxigdal_geotiff::writer::OverviewResampling::Nearest);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer for NoData test");
        writer.write(&data).expect("Should write NoData test data");
    }

    {
        let source = FileDataSource::open(&path).expect("Should open NoData file");
        let reader = GeoTiffReader::new(source).expect("Should create reader for NoData test");
        assert_eq!(
            reader.nodata(),
            NoDataValue::from_integer(255),
            "NoData value should be 255"
        );
    }

    let _ = std::fs::remove_file(path);
}

// ============================================================
// Error handling tests
// ============================================================

/// Test 21: Corrupt header (not a TIFF)
#[test]
fn test_coverage_corrupt_header() {
    let path = temp_test_file("corrupt_header.tif");

    // Write garbage data
    {
        let mut file = File::create(&path).expect("Should create corrupt file");
        file.write_all(b"NOT A TIFF FILE AT ALL")
            .expect("Should write garbage data");
    }

    {
        let result = FileDataSource::open(&path);
        match result {
            Ok(source) => {
                let tiff_result = oxigdal_geotiff::TiffFile::parse(&source);
                assert!(tiff_result.is_err(), "Should fail to parse corrupt TIFF");
            }
            Err(_) => {
                // Also acceptable - can't even open
            }
        }
    }

    let _ = std::fs::remove_file(path);
}

/// Test 22: Truncated TIFF header
#[test]
fn test_coverage_truncated_header() {
    let path = temp_test_file("truncated_header.tif");

    // Write only the byte order mark, missing the rest
    {
        let mut file = File::create(&path).expect("Should create truncated file");
        file.write_all(&[0x49, 0x49])
            .expect("Should write partial header");
    }

    {
        let result = FileDataSource::open(&path);
        match result {
            Ok(source) => {
                let tiff_result = oxigdal_geotiff::TiffFile::parse(&source);
                assert!(tiff_result.is_err(), "Should fail to parse truncated TIFF");
            }
            Err(_) => {
                // Also acceptable
            }
        }
    }

    let _ = std::fs::remove_file(path);
}

/// Test 23: Data size mismatch error
#[test]
#[cfg(feature = "lzw")]
fn test_coverage_data_size_mismatch() {
    let path = temp_test_file("size_mismatch.tif");
    let width = 32u64;
    let height = 32u64;

    // Provide wrong amount of data (too small)
    let data = vec![0u8; 100]; // Should be 1024

    let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
        .with_compression(oxigdal_geotiff::Compression::Lzw)
        .with_tile_size(16, 16)
        .with_overviews(false, oxigdal_geotiff::writer::OverviewResampling::Nearest);

    let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
        .expect("Should create writer for size mismatch test");
    let result = writer.write(&data);
    assert!(result.is_err(), "Should reject mismatched data size");

    let _ = std::fs::remove_file(path);
}

/// Test 24: Zero-dimension validation
#[test]
fn test_coverage_zero_dimensions() {
    let config_zero_w = WriterConfig::new(0, 100, 1, RasterDataType::UInt8);
    assert!(
        config_zero_w.validate().is_err(),
        "Should reject zero width"
    );

    let config_zero_h = WriterConfig::new(100, 0, 1, RasterDataType::UInt8);
    assert!(
        config_zero_h.validate().is_err(),
        "Should reject zero height"
    );

    let config_zero_b = WriterConfig::new(100, 100, 0, RasterDataType::UInt8);
    assert!(
        config_zero_b.validate().is_err(),
        "Should reject zero bands"
    );
}

// ============================================================
// BigTIFF and metadata tests
// ============================================================

/// Test 25: BigTIFF format round-trip
#[test]
#[cfg(feature = "lzw")]
fn test_coverage_bigtiff_roundtrip() {
    let path = temp_test_file("bigtiff_rt.tif");
    let width = 64u64;
    let height = 64u64;
    let data = vec![200u8; (width * height) as usize];

    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(oxigdal_geotiff::Compression::Lzw)
            .with_tile_size(32, 32)
            .with_bigtiff(true)
            .with_overviews(false, oxigdal_geotiff::writer::OverviewResampling::Nearest);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create BigTIFF writer");
        writer.write(&data).expect("Should write BigTIFF data");
    }

    {
        let source = FileDataSource::open(&path).expect("Should open BigTIFF file");
        let tiff = oxigdal_geotiff::TiffFile::parse(&source).expect("Should parse BigTIFF");
        assert!(tiff.is_bigtiff(), "Should be BigTIFF format");

        let reader = GeoTiffReader::new(source).expect("Should create BigTIFF reader");
        let read_data = reader.read_band(0, 0).expect("Should read BigTIFF band");
        assert_eq!(read_data, data, "BigTIFF data should match");
    }

    let _ = std::fs::remove_file(path);
}

/// Test 26: TIFF header parsing with both byte orders
#[test]
fn test_coverage_header_byte_orders() {
    // Little-endian classic
    let header_le = TiffHeader::classic(ByteOrderType::LittleEndian, 42);
    let bytes_le = header_le.to_bytes();
    let parsed_le = TiffHeader::parse(&bytes_le).expect("Should parse LE classic header");
    assert_eq!(parsed_le, header_le, "LE classic header should round-trip");

    // Big-endian classic
    let header_be = TiffHeader::classic(ByteOrderType::BigEndian, 1024);
    let bytes_be = header_be.to_bytes();
    let parsed_be = TiffHeader::parse(&bytes_be).expect("Should parse BE classic header");
    assert_eq!(parsed_be, header_be, "BE classic header should round-trip");

    // Little-endian BigTIFF
    let header_le_big = TiffHeader::bigtiff(ByteOrderType::LittleEndian, 0x1234);
    let bytes_le_big = header_le_big.to_bytes();
    let parsed_le_big = TiffHeader::parse(&bytes_le_big).expect("Should parse LE BigTIFF header");
    assert_eq!(
        parsed_le_big, header_le_big,
        "LE BigTIFF header should round-trip"
    );

    // Big-endian BigTIFF
    let header_be_big = TiffHeader::bigtiff(ByteOrderType::BigEndian, 0x5678);
    let bytes_be_big = header_be_big.to_bytes();
    let parsed_be_big = TiffHeader::parse(&bytes_be_big).expect("Should parse BE BigTIFF header");
    assert_eq!(
        parsed_be_big, header_be_big,
        "BE BigTIFF header should round-trip"
    );
}

/// Test 27: Metadata: combined geotransform + nodata + EPSG round-trip
#[test]
#[cfg(feature = "lzw")]
fn test_coverage_full_metadata_roundtrip() {
    let path = temp_test_file("meta_full.tif");
    let width = 64u64;
    let height = 64u64;
    let data = vec![100u8; (width * height) as usize];
    let geo_transform = GeoTransform::north_up(10.0, 50.0, 0.01, -0.01);

    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(oxigdal_geotiff::Compression::Lzw)
            .with_tile_size(32, 32)
            .with_geo_transform(geo_transform)
            .with_epsg_code(4326)
            .with_nodata(NoDataValue::from_integer(0))
            .with_overviews(false, oxigdal_geotiff::writer::OverviewResampling::Nearest);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer for full metadata");
        writer
            .write(&data)
            .expect("Should write full metadata data");
    }

    {
        let source = FileDataSource::open(&path).expect("Should open full metadata file");
        let reader = GeoTiffReader::new(source).expect("Should create reader for full metadata");

        // Verify geotransform
        let gt = reader
            .geo_transform()
            .expect("Should have GeoTransform in full metadata");
        assert!((gt.origin_x - 10.0).abs() < 1e-6, "Origin X should be 10.0");
        assert!((gt.origin_y - 50.0).abs() < 1e-6, "Origin Y should be 50.0");
        assert!(gt.is_north_up(), "Should be north-up orientation");

        // Verify EPSG
        assert_eq!(reader.epsg_code(), Some(4326), "EPSG should be 4326");

        // Verify NoData
        assert_eq!(
            reader.nodata(),
            NoDataValue::from_integer(0),
            "NoData should be 0"
        );

        // Verify data
        let read_data = reader
            .read_band(0, 0)
            .expect("Should read full metadata band");
        assert_eq!(read_data, data, "Full metadata data should match");
    }

    let _ = std::fs::remove_file(path);
}

/// Test 28: TIFF detection for various formats
#[test]
fn test_coverage_is_tiff_extended() {
    // Valid TIFF formats
    assert!(oxigdal_geotiff::is_tiff(&[0x49, 0x49, 0x2A, 0x00])); // LE classic
    assert!(oxigdal_geotiff::is_tiff(&[0x4D, 0x4D, 0x00, 0x2A])); // BE classic
    assert!(oxigdal_geotiff::is_tiff(&[0x49, 0x49, 0x2B, 0x00])); // LE BigTIFF
    assert!(oxigdal_geotiff::is_tiff(&[0x4D, 0x4D, 0x00, 0x2B])); // BE BigTIFF

    // Invalid formats
    assert!(!oxigdal_geotiff::is_tiff(&[0x89, 0x50, 0x4E, 0x47])); // PNG
    assert!(!oxigdal_geotiff::is_tiff(&[0xFF, 0xD8, 0xFF, 0xE0])); // JPEG
    assert!(!oxigdal_geotiff::is_tiff(&[0x25, 0x50, 0x44, 0x46])); // PDF
    assert!(!oxigdal_geotiff::is_tiff(&[0x47, 0x49, 0x46, 0x38])); // GIF
    assert!(!oxigdal_geotiff::is_tiff(&[])); // Empty
    assert!(!oxigdal_geotiff::is_tiff(&[0x00])); // Single byte
    assert!(!oxigdal_geotiff::is_tiff(&[0x49, 0x49])); // Too short
}
