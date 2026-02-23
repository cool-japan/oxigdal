//! Integration tests for GeoTIFF driver
//!
//! These tests create synthetic TIFF files and verify reading capabilities.

#![allow(clippy::expect_used)]
#![allow(clippy::float_cmp)]

use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

use oxigdal_core::io::FileDataSource;
use oxigdal_core::types::{GeoTransform, RasterDataType};
use oxigdal_geotiff::tiff::{ByteOrderType, TiffHeader};

/// Helper function to create a temporary test file
fn temp_test_file(name: &str) -> PathBuf {
    let mut path = env::temp_dir();
    path.push(format!("oxigdal_test_{}", name));
    path
}

/// Creates a minimal valid TIFF file for testing
fn create_minimal_tiff(path: &PathBuf) -> std::io::Result<()> {
    let mut file = File::create(path)?;

    // Write TIFF header (little-endian, classic)
    let header = TiffHeader::classic(ByteOrderType::LittleEndian, 8);
    file.write_all(&header.to_bytes())?;

    // Write a minimal IFD
    // Entry count (2 entries: ImageWidth, ImageLength)
    file.write_all(&[2, 0])?;

    // ImageWidth (tag 256), LONG (type 4), count 1, value 64
    file.write_all(&[
        0x00, 0x01, // Tag: 256
        0x04, 0x00, // Type: LONG
        0x01, 0x00, 0x00, 0x00, // Count: 1
        0x40, 0x00, 0x00, 0x00, // Value: 64
    ])?;

    // ImageLength (tag 257), LONG (type 4), count 1, value 64
    file.write_all(&[
        0x01, 0x01, // Tag: 257
        0x04, 0x00, // Type: LONG
        0x01, 0x00, 0x00, 0x00, // Count: 1
        0x40, 0x00, 0x00, 0x00, // Value: 64
    ])?;

    // Next IFD offset (0 = no more IFDs)
    file.write_all(&[0x00, 0x00, 0x00, 0x00])?;

    file.flush()?;
    Ok(())
}

#[test]
fn test_parse_minimal_tiff() {
    let path = temp_test_file("minimal.tif");
    create_minimal_tiff(&path).expect("Failed to create test file");

    let source = FileDataSource::open(&path).expect("Failed to open test file");
    let tiff = oxigdal_geotiff::TiffFile::parse(&source).expect("Failed to parse TIFF");

    assert_eq!(tiff.image_count(), 1);
    assert!(!tiff.is_bigtiff());
    assert_eq!(tiff.byte_order(), ByteOrderType::LittleEndian);

    // Cleanup
    let _ = std::fs::remove_file(path);
}

#[test]
fn test_header_parsing() {
    // Test classic little-endian
    let header_le = TiffHeader::classic(ByteOrderType::LittleEndian, 1024);
    let bytes_le = header_le.to_bytes();
    let parsed_le = TiffHeader::parse(&bytes_le).expect("Should parse");
    assert_eq!(parsed_le, header_le);

    // Test BigTIFF big-endian
    let header_be = TiffHeader::bigtiff(ByteOrderType::BigEndian, 0x123456789ABCDEF0);
    let bytes_be = header_be.to_bytes();
    let parsed_be = TiffHeader::parse(&bytes_be).expect("Should parse");
    assert_eq!(parsed_be, header_be);
}

#[test]
fn test_is_tiff_detection() {
    // Classic TIFF, little-endian
    assert!(oxigdal_geotiff::is_tiff(&[0x49, 0x49, 0x2A, 0x00]));

    // Classic TIFF, big-endian
    assert!(oxigdal_geotiff::is_tiff(&[0x4D, 0x4D, 0x00, 0x2A]));

    // BigTIFF, little-endian
    assert!(oxigdal_geotiff::is_tiff(&[0x49, 0x49, 0x2B, 0x00]));

    // Not a TIFF
    assert!(!oxigdal_geotiff::is_tiff(&[0x89, 0x50, 0x4E, 0x47])); // PNG
    assert!(!oxigdal_geotiff::is_tiff(&[0xFF, 0xD8, 0xFF, 0xE0])); // JPEG
}

#[test]
fn test_data_types() {
    // Test UInt8
    let dt = RasterDataType::from_tiff_sample_format(1, 8);
    assert_eq!(dt, Some(RasterDataType::UInt8));

    // Test Int16
    let dt = RasterDataType::from_tiff_sample_format(2, 16);
    assert_eq!(dt, Some(RasterDataType::Int16));

    // Test Float32
    let dt = RasterDataType::from_tiff_sample_format(3, 32);
    assert_eq!(dt, Some(RasterDataType::Float32));

    // Test Float64
    let dt = RasterDataType::from_tiff_sample_format(3, 64);
    assert_eq!(dt, Some(RasterDataType::Float64));
}

#[test]
fn test_geo_transform_extraction() {
    // This test verifies that GeoTransform can be created from bounds
    use oxigdal_core::types::BoundingBox;

    let bbox = BoundingBox::new(-180.0, -90.0, 180.0, 90.0).expect("Valid bbox");
    let gt = GeoTransform::from_bounds(&bbox, 360, 180).expect("Valid transform");

    assert!((gt.pixel_width - 1.0).abs() < 1e-10);
    assert!((gt.pixel_height + 1.0).abs() < 1e-10);
    assert!(gt.is_north_up());

    let computed_bounds = gt.compute_bounds(360, 180);
    assert!((computed_bounds.min_x - bbox.min_x).abs() < 1e-10);
    assert!((computed_bounds.max_x - bbox.max_x).abs() < 1e-10);
}

#[test]
fn test_compression_roundtrip() {
    use oxigdal_geotiff::Compression;
    use oxigdal_geotiff::compression::{compress, decompress};

    let original = b"This is a test of compression! ".repeat(10);

    // Test DEFLATE
    #[cfg(feature = "deflate")]
    {
        let compressed =
            compress(&original, Compression::Deflate).expect("Compression should work");
        let decompressed = decompress(&compressed, Compression::Deflate, original.len())
            .expect("Decompression should work");
        assert_eq!(&decompressed, &original);
        println!(
            "DEFLATE: {} -> {} bytes ({:.1}% of original)",
            original.len(),
            compressed.len(),
            (compressed.len() as f64 / original.len() as f64) * 100.0
        );
    }

    // Test LZW
    #[cfg(feature = "lzw")]
    {
        let compressed = compress(&original, Compression::Lzw).expect("Compression should work");
        let decompressed = decompress(&compressed, Compression::Lzw, original.len())
            .expect("Decompression should work");
        assert_eq!(&decompressed, &original);
        println!(
            "LZW: {} -> {} bytes ({:.1}% of original)",
            original.len(),
            compressed.len(),
            (compressed.len() as f64 / original.len() as f64) * 100.0
        );
    }

    // Test ZSTD
    #[cfg(feature = "zstd")]
    {
        let compressed = compress(&original, Compression::Zstd).expect("Compression should work");
        let decompressed = decompress(&compressed, Compression::Zstd, original.len())
            .expect("Decompression should work");
        assert_eq!(&decompressed, &original);
        println!(
            "ZSTD: {} -> {} bytes ({:.1}% of original)",
            original.len(),
            compressed.len(),
            (compressed.len() as f64 / original.len() as f64) * 100.0
        );
    }

    // Test PackBits
    let compressed = compress(&original, Compression::Packbits).expect("Compression should work");
    let decompressed = decompress(&compressed, Compression::Packbits, original.len())
        .expect("Decompression should work");
    assert_eq!(&decompressed, &original);

    // Test JPEG (note: JPEG is lossy, so we verify compression/decompression works)
    #[cfg(feature = "jpeg")]
    {
        use jpeg_encoder::ColorType;
        use oxigdal_geotiff::compression::compress_jpeg_with_params;

        // Create simple 16x16 RGB test image
        let mut rgb_data = Vec::new();
        for y in 0..16u16 {
            for x in 0..16u16 {
                rgb_data.push((x * 16) as u8);
                rgb_data.push((y * 16) as u8);
                rgb_data.push(((x + y) * 8) as u8);
            }
        }

        let compressed = compress_jpeg_with_params(&rgb_data, 16, 16, ColorType::Rgb, 85)
            .expect("JPEG compression should work");
        let decompressed = decompress(&compressed, Compression::Jpeg, rgb_data.len())
            .expect("JPEG decompression should work");

        // For JPEG, we just check that dimensions match (it's lossy)
        assert_eq!(decompressed.len(), rgb_data.len());
        println!(
            "JPEG: {} -> {} bytes ({:.1}% of original)",
            rgb_data.len(),
            compressed.len(),
            (compressed.len() as f64 / rgb_data.len() as f64) * 100.0
        );
    }
}

/// Round-trip test: write → read → verify
#[test]
#[cfg(feature = "lzw")]
fn test_geotiff_writer_roundtrip_tiled() {
    use oxigdal_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};
    use oxigdal_geotiff::{Compression, TiffFile};

    let path = temp_test_file("roundtrip_tiled.tif");

    // Create test data (64x64 single-band UInt8)
    let width = 64u64;
    let height = 64u64;
    let mut data = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            data.push(((x + y) % 256) as u8);
        }
    }

    // Write
    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(Compression::Lzw)
            .with_tile_size(16, 16)
            .with_overviews(false, oxigdal_geotiff::writer::OverviewResampling::Nearest);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer");

        writer.write(&data).expect("Should write data");
    }

    // Read back
    {
        let source = FileDataSource::open(&path).expect("Should open file");
        let tiff = TiffFile::parse(&source).expect("Should parse TIFF");

        assert_eq!(tiff.image_count(), 1);

        // Create reader and verify
        let reader = oxigdal_geotiff::GeoTiffReader::new(source).expect("Should create reader");
        assert_eq!(reader.width(), width);
        assert_eq!(reader.height(), height);
        assert_eq!(reader.band_count(), 1);

        let read_data = reader.read_band(0, 0).expect("Should read band");

        assert_eq!(read_data.len(), data.len());
        for (i, (&original, &read)) in data.iter().zip(read_data.iter()).enumerate() {
            assert_eq!(
                original,
                read,
                "Mismatch at pixel {} (x={}, y={})",
                i,
                i as u64 % width,
                i as u64 / width
            );
        }
    }

    // Cleanup
    let _ = std::fs::remove_file(path);
}

/// Test striped TIFF writing
#[test]
#[cfg(feature = "deflate")]
fn test_geotiff_writer_roundtrip_striped() {
    use oxigdal_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};
    use oxigdal_geotiff::{Compression, TiffFile};

    let path = temp_test_file("roundtrip_striped.tif");

    // Create test data (32x32 single-band UInt8)
    let width = 32u64;
    let height = 32u64;
    let mut data = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            data.push(((x * y) % 256) as u8);
        }
    }

    // Write (no tile_size means striped)
    {
        let mut config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(Compression::Deflate);
        config.tile_width = None;
        config.tile_height = None;

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer");

        writer.write(&data).expect("Should write data");
    }

    // Read back
    {
        let source = FileDataSource::open(&path).expect("Should open file");
        let tiff = TiffFile::parse(&source).expect("Should parse TIFF");

        assert_eq!(tiff.image_count(), 1);

        let reader = oxigdal_geotiff::GeoTiffReader::new(source).expect("Should create reader");
        assert_eq!(reader.width(), width);
        assert_eq!(reader.height(), height);
    }

    // Cleanup
    let _ = std::fs::remove_file(path);
}

/// Test GeoTIFF with geotransform and EPSG code
#[test]
#[cfg(feature = "lzw")]
fn test_geotiff_writer_georeferencing() {
    use oxigdal_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};

    let path = temp_test_file("roundtrip_geo.tif");

    let width = 100u64;
    let height = 100u64;
    let data = vec![128u8; (width * height) as usize];

    // Write with geotransform and EPSG
    {
        let geo_transform = GeoTransform::north_up(100.0, 200.0, 0.5, -0.5);

        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(oxigdal_geotiff::Compression::Lzw)
            .with_tile_size(32, 32)
            .with_geo_transform(geo_transform)
            .with_epsg_code(4326); // WGS84

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer");

        writer.write(&data).expect("Should write data");
    }

    // Read back and verify geotransform
    {
        let source = FileDataSource::open(&path).expect("Should open file");
        let reader = oxigdal_geotiff::GeoTiffReader::new(source).expect("Should create reader");

        // Check if geotransform is present
        let gt = reader.geo_transform();
        assert!(gt.is_some(), "GeoTransform should be present");

        if let Some(gt) = gt {
            assert!((gt.origin_x - 100.0).abs() < 1e-6);
            assert!((gt.origin_y - 200.0).abs() < 1e-6);
            assert!((gt.pixel_width - 0.5).abs() < 1e-6);
            assert!((gt.pixel_height + 0.5).abs() < 1e-6);
        }
    }

    // Cleanup
    let _ = std::fs::remove_file(path);
}

/// Test COG writer with overviews
#[test]
#[cfg(feature = "lzw")]
fn test_cog_writer_with_overviews() {
    use oxigdal_geotiff::TiffFile;
    use oxigdal_geotiff::writer::{CogWriter, CogWriterOptions, OverviewResampling, WriterConfig};

    let path = temp_test_file("roundtrip_cog.tif");

    let width = 512u64;
    let height = 512u64;
    let mut data = Vec::with_capacity((width * height) as usize);

    // Create gradient test pattern
    for y in 0..height {
        for x in 0..width {
            data.push(((x + y) / 4) as u8);
        }
    }

    // Write COG with overviews
    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(oxigdal_geotiff::Compression::Lzw)
            .with_tile_size(256, 256)
            .with_overviews(true, OverviewResampling::Average)
            .with_overview_levels(vec![2, 4]);

        let mut writer = CogWriter::create(&path, config, CogWriterOptions::default())
            .expect("Should create writer");

        let validation = writer.write(&data).expect("Should write data");
        assert!(validation.is_valid, "COG should be valid");
        assert!(validation.has_overviews, "COG should have overviews");
    }

    // Read back and verify overview count
    {
        let source = FileDataSource::open(&path).expect("Should open file");
        let tiff = TiffFile::parse(&source).expect("Should parse TIFF");

        // Should have 3 IFDs: primary + 2 overviews
        assert_eq!(
            tiff.image_count(),
            3,
            "Should have 3 IFDs (primary + 2 overviews)"
        );

        let reader = oxigdal_geotiff::GeoTiffReader::new(source).expect("Should create reader");
        assert_eq!(reader.width(), width);
        assert_eq!(reader.height(), height);
        assert_eq!(reader.overview_count(), 2, "Should have 2 overviews");
    }

    // Cleanup
    let _ = std::fs::remove_file(path);
}

/// Test multi-band writing
#[test]
#[cfg(feature = "lzw")]
fn test_geotiff_writer_multiband() {
    use oxigdal_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};

    let path = temp_test_file("roundtrip_rgb.tif");

    let width = 64u64;
    let height = 64u64;
    let band_count = 3u16;

    // Create RGB test data
    let mut data = Vec::with_capacity((width * height * band_count as u64) as usize);
    for y in 0..height {
        for x in 0..width {
            data.push(((x * 4) % 256) as u8); // R
            data.push(((y * 4) % 256) as u8); // G
            data.push((((x + y) * 2) % 256) as u8); // B
        }
    }

    // Write
    {
        let config = WriterConfig::new(width, height, band_count, RasterDataType::UInt8)
            .with_compression(oxigdal_geotiff::Compression::Lzw)
            .with_tile_size(32, 32);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer");

        writer.write(&data).expect("Should write data");
    }

    // Read back
    {
        let source = FileDataSource::open(&path).expect("Should open file");
        let reader = oxigdal_geotiff::GeoTiffReader::new(source).expect("Should create reader");
        assert_eq!(reader.width(), width);
        assert_eq!(reader.height(), height);
        assert_eq!(reader.band_count(), u32::from(band_count));
    }

    // Cleanup
    let _ = std::fs::remove_file(path);
}

/// Test BigTIFF writing
#[test]
#[cfg(feature = "lzw")]
fn test_geotiff_writer_bigtiff() {
    use oxigdal_geotiff::TiffFile;
    use oxigdal_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};

    let path = temp_test_file("roundtrip_bigtiff.tif");

    let width = 100u64;
    let height = 100u64;
    let data = vec![42u8; (width * height) as usize];

    // Write as BigTIFF
    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(oxigdal_geotiff::Compression::Lzw)
            .with_tile_size(32, 32)
            .with_bigtiff(true);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer");

        writer.write(&data).expect("Should write data");
    }

    // Read back
    {
        let source = FileDataSource::open(&path).expect("Should open file");
        let tiff = TiffFile::parse(&source).expect("Should parse TIFF");

        assert!(tiff.is_bigtiff(), "Should be BigTIFF");

        let reader = oxigdal_geotiff::GeoTiffReader::new(source).expect("Should create reader");
        assert_eq!(reader.width(), width);
        assert_eq!(reader.height(), height);
    }

    // Cleanup
    let _ = std::fs::remove_file(path);
}
