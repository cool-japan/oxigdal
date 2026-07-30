//! Comprehensive integration tests for GeoTIFF writer with external validation
//!
//! These tests write various GeoTIFF configurations and verify them through:
//! - Round-trip reading
//! - Byte-level format compliance
//! - IFD structure validation
//! - GeoKeys validation

#![allow(
    clippy::expect_used,
    clippy::float_cmp,
    clippy::panic,
    clippy::useless_vec
)]

use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use oxigeo_core::io::FileDataSource;
use oxigeo_core::types::RasterDataType;
use oxigeo_geotiff::GeoTiffReader;
use oxigeo_geotiff::tiff::Compression;
use oxigeo_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};

// Only the codec-gated tests below reach for these, so the imports carry the
// same gates; without them there is nothing in this file that needs them.
#[cfg(feature = "lzw")]
use oxigeo_core::types::{GeoTransform, NoDataValue};
#[cfg(feature = "lzw")]
use oxigeo_geotiff::tiff::PhotometricInterpretation;
#[cfg(any(feature = "deflate", feature = "lzw"))]
use oxigeo_geotiff::tiff::TiffFile;
#[cfg(any(feature = "deflate", feature = "lzw"))]
use oxigeo_geotiff::writer::OverviewResampling;
#[cfg(feature = "lzw")]
use oxigeo_geotiff::writer::{CogWriter, CogWriterOptions};

/// An RAII fixture path inside [`std::env::temp_dir`].
///
/// These fixtures used to share one fixed `test_geotiffs/` directory with fixed
/// leaf names and no cleanup at all: two binaries (or two runs) writing
/// `test_rgb.tif` raced on the same bytes, and every run leaked 22 files.  The
/// leaf name now embeds the process id and a monotonic counter, so no two test
/// binaries — nor two concurrent runs of this one — can ever collide, and
/// dropping the guard removes the fixture even when a test panics.
struct TempPath(PathBuf);

impl TempPath {
    fn new(name: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self(env::temp_dir().join(format!(
            "oxigeo_geotiff_writer_{}_{seq}_{name}",
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
        let _ = fs::remove_file(&self.0);
    }
}

/// Helper to create a uniquely named test output file path.
fn test_file_path(name: &str) -> TempPath {
    TempPath::new(name)
}

/// Helper to create test data with a pattern
fn create_test_pattern_u8(width: u64, height: u64) -> Vec<u8> {
    let mut data = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            // Create a pattern that's visually identifiable
            data.push(((x + y) % 256) as u8);
        }
    }
    data
}

/// Helper to create multi-band test data (only the LZW cases need RGB).
#[cfg(feature = "lzw")]
fn create_test_pattern_rgb(width: u64, height: u64) -> Vec<u8> {
    let mut data = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            data.push(((x * 4) % 256) as u8); // R
            data.push(((y * 4) % 256) as u8); // G
            data.push((((x + y) * 2) % 256) as u8); // B
        }
    }
    data
}

/// Verify TIFF header magic bytes
#[cfg(any(feature = "deflate", feature = "lzw", feature = "zstd"))]
fn verify_tiff_header(path: &std::path::Path) -> Result<(), String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("Failed to open: {}", e))?;
    let mut header = [0u8; 4];
    use std::io::Read;
    file.read_exact(&mut header)
        .map_err(|e| format!("Failed to read header: {}", e))?;

    // Verify TIFF magic
    if !oxigeo_geotiff::is_tiff(&header) {
        return Err("Invalid TIFF header".to_string());
    }

    Ok(())
}

/// Verify IFD count and structure
#[cfg(any(feature = "deflate", feature = "lzw"))]
fn verify_ifd_structure(path: &std::path::Path, expected_ifd_count: usize) -> Result<(), String> {
    let source = FileDataSource::open(path).map_err(|e| format!("Failed to open source: {}", e))?;
    let tiff = TiffFile::parse(&source).map_err(|e| format!("Failed to parse TIFF: {}", e))?;

    let actual_count = tiff.image_count();
    if actual_count != expected_ifd_count {
        return Err(format!(
            "Expected {} IFDs, found {}",
            expected_ifd_count, actual_count
        ));
    }

    Ok(())
}

/// Test 1: DEFLATE compression with tiled layout
#[test]
#[cfg(feature = "deflate")]
fn test_write_deflate_tiled() {
    let path = test_file_path("test_deflate_tiled.tif");
    let width = 256u64;
    let height = 256u64;
    let data = create_test_pattern_u8(width, height);

    // Write
    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(Compression::Deflate)
            .with_tile_size(64, 64)
            .with_overviews(false, OverviewResampling::Nearest);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer");

        writer.write(&data).expect("Should write data");
    }

    // Verify header
    verify_tiff_header(&path).expect("Valid TIFF header");

    // Verify IFD
    verify_ifd_structure(&path, 1).expect("Should have 1 IFD");

    // Read back and verify data
    {
        let source = FileDataSource::open(&path).expect("Should open file");
        let reader = GeoTiffReader::new(source).expect("Should create reader");

        assert_eq!(reader.width(), width);
        assert_eq!(reader.height(), height);
        assert_eq!(reader.band_count(), 1);
        assert_eq!(reader.compression(), Compression::Deflate);
        assert_eq!(reader.tile_size(), Some((64, 64)));

        let read_data = reader.read_band(0, 0).expect("Should read band");
        assert_eq!(read_data.len(), data.len());
        assert_eq!(read_data, data, "Data should match exactly");
    }
}

/// Test 2: LZW compression with striped layout
#[test]
#[cfg(feature = "lzw")]
fn test_write_lzw_striped() {
    let path = test_file_path("test_lzw_striped.tif");
    let width = 128u64;
    let height = 128u64;
    let data = create_test_pattern_u8(width, height);

    // Write with striped layout (no tile_size)
    {
        let mut config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(Compression::Lzw);
        config.tile_width = None;
        config.tile_height = None;

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer");

        writer.write(&data).expect("Should write data");
    }

    // Verify
    verify_tiff_header(&path).expect("Valid TIFF header");
    verify_ifd_structure(&path, 1).expect("Should have 1 IFD");

    // Read back
    {
        let source = FileDataSource::open(&path).expect("Should open file");
        let reader = GeoTiffReader::new(source).expect("Should create reader");

        assert_eq!(reader.width(), width);
        assert_eq!(reader.height(), height);
        assert_eq!(reader.compression(), Compression::Lzw);
        assert_eq!(reader.tile_size(), None, "Should be striped, not tiled");

        let read_data = reader.read_band(0, 0).expect("Should read band");
        assert_eq!(read_data, data);
    }
}

/// Test 3: ZSTD compression
#[test]
#[cfg(feature = "zstd")]
fn test_write_zstd() {
    let path = test_file_path("test_zstd.tif");
    let width = 200u64;
    let height = 200u64;
    let data = create_test_pattern_u8(width, height);

    // Write with ZSTD
    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(Compression::Zstd)
            .with_tile_size(100, 100);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer");

        writer.write(&data).expect("Should write data");
    }

    // Verify
    verify_tiff_header(&path).expect("Valid TIFF header");

    // Read back
    {
        let source = FileDataSource::open(&path).expect("Should open file");
        let reader = GeoTiffReader::new(source).expect("Should create reader");

        assert_eq!(reader.compression(), Compression::Zstd);
        let read_data = reader.read_band(0, 0).expect("Should read band");
        assert_eq!(read_data, data);
    }
}

/// Test 4: PackBits compression
#[test]
fn test_write_packbits() {
    let path = test_file_path("test_packbits.tif");
    let width = 100u64;
    let height = 100u64;
    let data = create_test_pattern_u8(width, height);

    // Write with PackBits
    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(Compression::Packbits)
            .with_tile_size(50, 50);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer");

        writer.write(&data).expect("Should write data");
    }

    // Verify and read back
    {
        let source = FileDataSource::open(&path).expect("Should open file");
        let reader = GeoTiffReader::new(source).expect("Should create reader");

        assert_eq!(reader.compression(), Compression::Packbits);
        let read_data = reader.read_band(0, 0).expect("Should read band");
        assert_eq!(read_data, data);
    }
}

/// Test 5: UInt16 data type
#[test]
#[cfg(feature = "lzw")]
fn test_write_uint16() {
    let path = test_file_path("test_uint16.tif");
    let width = 128u64;
    let height = 128u64;

    // Create UInt16 data
    let mut data = Vec::with_capacity((width * height * 2) as usize);
    for y in 0..height {
        for x in 0..width {
            let value = ((x + y) * 256) as u16;
            data.extend_from_slice(&value.to_le_bytes());
        }
    }

    // Write
    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt16)
            .with_compression(Compression::Lzw)
            .with_tile_size(64, 64);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer");

        writer.write(&data).expect("Should write data");
    }

    // Read back
    {
        let source = FileDataSource::open(&path).expect("Should open file");
        let reader = GeoTiffReader::new(source).expect("Should create reader");

        assert_eq!(reader.data_type(), Some(RasterDataType::UInt16));
        let read_data = reader.read_band(0, 0).expect("Should read band");
        assert_eq!(read_data, data);
    }
}

/// Test 6: Float32 data type
#[test]
#[cfg(feature = "lzw")]
fn test_write_float32() {
    let path = test_file_path("test_float32.tif");
    let width = 64u64;
    let height = 64u64;

    // Create Float32 data
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let value = ((x as f32) + (y as f32)) / 128.0;
            data.extend_from_slice(&value.to_le_bytes());
        }
    }

    // Write
    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::Float32)
            .with_compression(Compression::Lzw)
            .with_tile_size(32, 32);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer");

        writer.write(&data).expect("Should write data");
    }

    // Read back
    {
        let source = FileDataSource::open(&path).expect("Should open file");
        let reader = GeoTiffReader::new(source).expect("Should create reader");

        assert_eq!(reader.data_type(), Some(RasterDataType::Float32));
        let read_data = reader.read_band(0, 0).expect("Should read band");
        assert_eq!(read_data.len(), data.len());

        // For floating point, check approximate equality
        for (i, (a, b)) in data
            .chunks_exact(4)
            .zip(read_data.chunks_exact(4))
            .enumerate()
        {
            let orig = f32::from_le_bytes([a[0], a[1], a[2], a[3]]);
            let read = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            assert!(
                (orig - read).abs() < 1e-6,
                "Mismatch at pixel {}: {} vs {}",
                i,
                orig,
                read
            );
        }
    }
}

/// Test 7: Float64 data type
#[test]
#[cfg(feature = "lzw")]
fn test_write_float64() {
    let path = test_file_path("test_float64.tif");
    let width = 64u64;
    let height = 64u64;

    // Create Float64 data
    let mut data = Vec::with_capacity((width * height * 8) as usize);
    for y in 0..height {
        for x in 0..width {
            let value = ((x as f64) + (y as f64)) / 128.0;
            data.extend_from_slice(&value.to_le_bytes());
        }
    }

    // Write
    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::Float64)
            .with_compression(Compression::Lzw)
            .with_tile_size(32, 32);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer");

        writer.write(&data).expect("Should write data");
    }

    // Read back
    {
        let source = FileDataSource::open(&path).expect("Should open file");
        let reader = GeoTiffReader::new(source).expect("Should create reader");

        assert_eq!(reader.data_type(), Some(RasterDataType::Float64));
        let read_data = reader.read_band(0, 0).expect("Should read band");
        assert_eq!(read_data, data);
    }
}

/// Test 8: RGB (3-band)
#[test]
#[cfg(feature = "lzw")]
fn test_write_rgb() {
    let path = test_file_path("test_rgb.tif");
    let width = 128u64;
    let height = 128u64;
    let data = create_test_pattern_rgb(width, height);

    // Write
    {
        let config = WriterConfig::new(width, height, 3, RasterDataType::UInt8)
            .with_compression(Compression::Lzw)
            .with_tile_size(64, 64)
            .with_photometric(PhotometricInterpretation::Rgb);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer");

        writer.write(&data).expect("Should write data");
    }

    // Read back
    {
        let source = FileDataSource::open(&path).expect("Should open file");
        let reader = GeoTiffReader::new(source).expect("Should create reader");

        assert_eq!(reader.band_count(), 3);
        // `read_band` returns the requested band de-interleaved, not the whole
        // interleaved plane (cool-japan/oxigeo#14).
        let pixels = (width * height) as usize;
        for band in 0..3usize {
            let read_data = reader.read_band(0, band).expect("Should read band");
            let expected: Vec<u8> = (0..pixels).map(|i| data[i * 3 + band]).collect();
            assert_eq!(read_data.len(), pixels, "band {band} length");
            assert_eq!(read_data, expected, "band {band} data");
        }
        assert!(
            reader.read_band(0, 3).is_err(),
            "band 3 of a 3-band file must be rejected"
        );
    }
}

/// Test 9: RGBA (4-band)
#[test]
#[cfg(feature = "lzw")]
fn test_write_rgba() {
    let path = test_file_path("test_rgba.tif");
    let width = 100u64;
    let height = 100u64;

    // Create RGBA data
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            data.push(((x * 2) % 256) as u8); // R
            data.push(((y * 2) % 256) as u8); // G
            data.push(((x + y) % 256) as u8); // B
            data.push(255); // A (opaque)
        }
    }

    // Write
    {
        let config = WriterConfig::new(width, height, 4, RasterDataType::UInt8)
            .with_compression(Compression::Lzw)
            .with_tile_size(50, 50)
            .with_photometric(PhotometricInterpretation::Rgb); // RGBA is still RGB photometric

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer");

        writer.write(&data).expect("Should write data");
    }

    // Read back
    {
        let source = FileDataSource::open(&path).expect("Should open file");
        let reader = GeoTiffReader::new(source).expect("Should create reader");

        assert_eq!(reader.band_count(), 4);
        // `read_band` returns the requested band de-interleaved, not the whole
        // interleaved plane (cool-japan/oxigeo#14).
        let pixels = (width * height) as usize;
        for band in 0..4usize {
            let read_data = reader.read_band(0, band).expect("Should read band");
            let expected: Vec<u8> = (0..pixels).map(|i| data[i * 4 + band]).collect();
            assert_eq!(read_data.len(), pixels, "band {band} length");
            assert_eq!(read_data, expected, "band {band} data");
        }
        assert!(
            reader.read_band(0, 4).is_err(),
            "band 4 of a 4-band file must be rejected"
        );
    }
}

/// Test 10: Multi-spectral (6-band)
#[test]
#[cfg(feature = "lzw")]
fn test_write_multispectral() {
    let path = test_file_path("test_multispectral.tif");
    let width = 64u64;
    let height = 64u64;
    let band_count = 6u16;

    // Create multi-spectral data
    let mut data = Vec::with_capacity((width * height * u64::from(band_count)) as usize);
    for _y in 0..height {
        for x in 0..width {
            for b in 0..band_count {
                data.push(((x + u64::from(b) * 10) % 256) as u8);
            }
        }
    }

    // Write
    {
        let config = WriterConfig::new(width, height, band_count, RasterDataType::UInt8)
            .with_compression(Compression::Lzw)
            .with_tile_size(32, 32);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer");

        writer.write(&data).expect("Should write data");
    }

    // Read back
    {
        let source = FileDataSource::open(&path).expect("Should open file");
        let reader = GeoTiffReader::new(source).expect("Should create reader");

        assert_eq!(reader.band_count(), u32::from(band_count));
        // `read_band` returns the requested band de-interleaved, not the whole
        // interleaved plane (cool-japan/oxigeo#14).
        let bands = band_count as usize;
        let pixels = (width * height) as usize;
        for band in 0..bands {
            let read_data = reader.read_band(0, band).expect("Should read band");
            let expected: Vec<u8> = (0..pixels).map(|i| data[i * bands + band]).collect();
            assert_eq!(read_data.len(), pixels, "band {band} length");
            assert_eq!(read_data, expected, "band {band} data");
        }
        assert!(
            reader.read_band(0, bands).is_err(),
            "band {bands} of a {bands}-band file must be rejected"
        );
    }
}

/// Test 11: GeoTIFF with GeoTransform and EPSG
#[test]
#[cfg(feature = "lzw")]
fn test_write_georeferenced() {
    let path = test_file_path("test_georeferenced.tif");
    let width = 100u64;
    let height = 100u64;
    let data = create_test_pattern_u8(width, height);

    let geo_transform = GeoTransform::north_up(-122.0, 37.0, 0.001, -0.001);

    // Write
    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(Compression::Lzw)
            .with_tile_size(50, 50)
            .with_geo_transform(geo_transform)
            .with_epsg_code(4326); // WGS84

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer");

        writer.write(&data).expect("Should write data");
    }

    // Read back and verify georeferencing
    {
        let source = FileDataSource::open(&path).expect("Should open file");
        let reader = GeoTiffReader::new(source).expect("Should create reader");

        // Verify GeoTransform
        let read_gt = reader.geo_transform().expect("Should have GeoTransform");
        assert!((read_gt.origin_x - geo_transform.origin_x).abs() < 1e-9);
        assert!((read_gt.origin_y - geo_transform.origin_y).abs() < 1e-9);
        assert!((read_gt.pixel_width - geo_transform.pixel_width).abs() < 1e-9);
        assert!((read_gt.pixel_height - geo_transform.pixel_height).abs() < 1e-9);

        // Verify EPSG
        assert_eq!(reader.epsg_code(), Some(4326));

        let read_data = reader.read_band(0, 0).expect("Should read band");
        assert_eq!(read_data, data);
    }
}

/// Test 12: GeoTIFF with NoData value
#[test]
#[cfg(feature = "lzw")]
fn test_write_with_nodata() {
    let path = test_file_path("test_nodata.tif");
    let width = 100u64;
    let height = 100u64;
    let data = create_test_pattern_u8(width, height);

    // Write with NoData
    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(Compression::Lzw)
            .with_tile_size(50, 50)
            .with_nodata(NoDataValue::from_integer(0));

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer");

        writer.write(&data).expect("Should write data");
    }

    // Read back
    {
        let source = FileDataSource::open(&path).expect("Should open file");
        let reader = GeoTiffReader::new(source).expect("Should create reader");

        assert_eq!(reader.nodata(), NoDataValue::from_integer(0));
        let read_data = reader.read_band(0, 0).expect("Should read band");
        assert_eq!(read_data, data);
    }
}

/// Test 13: COG with overviews
#[test]
#[cfg(feature = "lzw")]
fn test_write_cog_with_overviews() {
    let path = test_file_path("test_cog_overviews.tif");
    let width = 512u64;
    let height = 512u64;
    let data = create_test_pattern_u8(width, height);

    // Write COG with overviews
    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(Compression::Lzw)
            .with_tile_size(256, 256)
            .with_overviews(true, OverviewResampling::Average)
            .with_overview_levels(vec![2, 4]);

        let mut writer = CogWriter::create(&path, config, CogWriterOptions::default())
            .expect("Should create writer");

        let validation = writer.write(&data).expect("Should write data");
        assert!(validation.is_valid, "COG should be valid");
        assert!(validation.has_overviews, "COG should have overviews");
    }

    // Verify IFD structure (primary + 2 overviews)
    verify_ifd_structure(&path, 3).expect("Should have 3 IFDs");

    // Read back
    {
        let source = FileDataSource::open(&path).expect("Should open file");
        let reader = GeoTiffReader::new(source).expect("Should create reader");

        assert_eq!(reader.width(), width);
        assert_eq!(reader.height(), height);
        assert_eq!(reader.overview_count(), 2);

        let read_data = reader.read_band(0, 0).expect("Should read band");
        assert_eq!(read_data, data);
    }
}

/// Test 14: COG with multiple overview levels
#[test]
#[cfg(feature = "lzw")]
fn test_write_cog_multiple_overviews() {
    let path = test_file_path("test_cog_multi_overviews.tif");
    let width = 1024u64;
    let height = 1024u64;
    let data = create_test_pattern_u8(width, height);

    // Write COG with multiple overviews
    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(Compression::Lzw)
            .with_tile_size(256, 256)
            .with_overviews(true, OverviewResampling::Nearest)
            .with_overview_levels(vec![2, 4, 8]);

        let mut writer = CogWriter::create(&path, config, CogWriterOptions::default())
            .expect("Should create writer");

        let validation = writer.write(&data).expect("Should write data");
        assert!(validation.is_valid);
        assert!(validation.has_overviews);
    }

    // Verify IFD structure (primary + 3 overviews)
    verify_ifd_structure(&path, 4).expect("Should have 4 IFDs");

    // Read back
    {
        let source = FileDataSource::open(&path).expect("Should open file");
        let reader = GeoTiffReader::new(source).expect("Should create reader");

        assert_eq!(reader.overview_count(), 3);
    }
}

/// Test 15: COG without overviews (regular tiled GeoTIFF)
#[test]
#[cfg(feature = "lzw")]
fn test_write_cog_no_overviews() {
    let path = test_file_path("test_cog_no_overviews.tif");
    let width = 256u64;
    let height = 256u64;
    let data = create_test_pattern_u8(width, height);

    // Write COG without overviews
    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(Compression::Lzw)
            .with_tile_size(256, 256)
            .with_overviews(false, OverviewResampling::Nearest);

        let mut writer = CogWriter::create(&path, config, CogWriterOptions::default())
            .expect("Should create writer");

        let validation = writer.write(&data).expect("Should write data");
        assert!(validation.is_valid);
        assert!(!validation.has_overviews);
    }

    // Verify IFD structure (primary only)
    verify_ifd_structure(&path, 1).expect("Should have 1 IFD");
}

/// Test 16: BigTIFF format
#[test]
#[cfg(feature = "lzw")]
fn test_write_bigtiff() {
    let path = test_file_path("test_bigtiff.tif");
    let width = 128u64;
    let height = 128u64;
    let data = create_test_pattern_u8(width, height);

    // Write as BigTIFF
    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(Compression::Lzw)
            .with_tile_size(64, 64)
            .with_bigtiff(true);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer");

        writer.write(&data).expect("Should write data");
    }

    // Read back and verify BigTIFF
    {
        let source = FileDataSource::open(&path).expect("Should open file");
        let tiff = TiffFile::parse(&source).expect("Should parse TIFF");

        assert!(tiff.is_bigtiff(), "Should be BigTIFF format");

        let reader = GeoTiffReader::new(source).expect("Should create reader");
        let read_data = reader.read_band(0, 0).expect("Should read band");
        assert_eq!(read_data, data);
    }
}

/// Test 17: Different tile sizes
#[test]
#[cfg(feature = "lzw")]
fn test_write_various_tile_sizes() {
    let test_cases = vec![
        (128, 128, 16, 16),   // Small tiles
        (256, 256, 128, 128), // Medium tiles
        (512, 512, 256, 256), // Large tiles (COG standard)
        (100, 100, 32, 32),   // Non-power-of-2 image with power-of-2 tiles
    ];

    for (idx, (width, height, tile_w, tile_h)) in test_cases.iter().enumerate() {
        let path = test_file_path(&format!("test_tile_size_{}.tif", idx));
        let data = create_test_pattern_u8(*width, *height);

        // Write
        {
            let config = WriterConfig::new(*width, *height, 1, RasterDataType::UInt8)
                .with_compression(Compression::Lzw)
                .with_tile_size(*tile_w, *tile_h);

            let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
                .expect("Should create writer");

            writer.write(&data).expect("Should write data");
        }

        // Read back and verify tile size
        {
            let source = FileDataSource::open(&path).expect("Should open file");
            let reader = GeoTiffReader::new(source).expect("Should create reader");

            assert_eq!(reader.tile_size(), Some((*tile_w, *tile_h)));
            let read_data = reader.read_band(0, 0).expect("Should read band");
            assert_eq!(read_data, data);
        }
    }
}

/// Test 18: Error handling - invalid dimensions
#[test]
fn test_write_error_invalid_dimensions() {
    let _path = test_file_path("test_invalid_dims.tif");

    // Try to create with zero width
    let config = WriterConfig::new(0, 100, 1, RasterDataType::UInt8);
    assert!(config.validate().is_err(), "Should reject zero width");

    // Try to create with zero height
    let config = WriterConfig::new(100, 0, 1, RasterDataType::UInt8);
    assert!(config.validate().is_err(), "Should reject zero height");

    // Try to create with zero bands
    let config = WriterConfig::new(100, 100, 0, RasterDataType::UInt8);
    assert!(config.validate().is_err(), "Should reject zero bands");
}

/// Test 19: Verify byte alignment and padding
#[test]
#[cfg(feature = "lzw")]
fn test_write_byte_alignment() {
    let path = test_file_path("test_alignment.tif");
    // Use odd dimensions to test edge cases
    let width = 127u64;
    let height = 127u64;
    let data = create_test_pattern_u8(width, height);

    // Write
    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(Compression::Lzw)
            .with_tile_size(64, 64);

        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("Should create writer");

        writer.write(&data).expect("Should write data");
    }

    // Verify file size is reasonable
    let metadata = std::fs::metadata(&path).expect("Should get metadata");
    assert!(metadata.len() > 0, "File should have content");
    assert!(
        metadata.len() < (width * height * 2),
        "File should be compressed"
    );

    // Read back
    {
        let source = FileDataSource::open(&path).expect("Should open file");
        let reader = GeoTiffReader::new(source).expect("Should create reader");
        let read_data = reader.read_band(0, 0).expect("Should read band");
        assert_eq!(read_data, data);
    }
}

/// Test 20: Comprehensive COG validation
#[test]
#[cfg(feature = "lzw")]
fn test_cog_validation_comprehensive() {
    let path = test_file_path("test_cog_validation.tif");
    let width = 512u64;
    let height = 512u64;
    let data = create_test_pattern_u8(width, height);

    let geo_transform = GeoTransform::north_up(0.0, 0.0, 1.0, -1.0);

    // Write COG with all features
    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(Compression::Lzw)
            .with_tile_size(256, 256)
            .with_overviews(true, OverviewResampling::Average)
            .with_overview_levels(vec![2, 4])
            .with_geo_transform(geo_transform)
            .with_epsg_code(4326)
            .with_nodata(NoDataValue::from_integer(0));

        let mut writer = CogWriter::create(&path, config, CogWriterOptions::default())
            .expect("Should create writer");

        let validation = writer.write(&data).expect("Should write data");
        assert!(validation.is_valid, "COG should be valid");
        assert!(validation.has_overviews, "COG should have overviews");
    }

    // Comprehensive validation
    {
        let source = FileDataSource::open(&path).expect("Should open file");

        // Verify it's recognized as COG
        let is_cog = oxigeo_geotiff::is_cog(&source).expect("Should validate");
        assert!(is_cog, "Should be recognized as COG");

        let reader = GeoTiffReader::new(source).expect("Should create reader");

        // Verify all attributes
        assert_eq!(reader.width(), width);
        assert_eq!(reader.height(), height);
        assert_eq!(reader.tile_size(), Some((256, 256)));
        assert_eq!(reader.overview_count(), 2);
        assert_eq!(reader.epsg_code(), Some(4326));
        assert_eq!(reader.nodata(), NoDataValue::from_integer(0));

        let read_gt = reader.geo_transform().expect("Should have GeoTransform");
        assert!((read_gt.origin_x - geo_transform.origin_x).abs() < 1e-9);
        assert!((read_gt.origin_y - geo_transform.origin_y).abs() < 1e-9);

        let read_data = reader.read_band(0, 0).expect("Should read band");
        assert_eq!(read_data, data);
    }
}
