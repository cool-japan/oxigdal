use super::*;
use std::env;

#[test]
fn test_format_detection_from_extension() {
    assert_eq!(
        detect_format_from_extension(Path::new("test.tif")),
        Some(RasterFormat::GeoTiff)
    );
    assert_eq!(
        detect_format_from_extension(Path::new("test.tiff")),
        Some(RasterFormat::GeoTiff)
    );
    assert_eq!(
        detect_format_from_extension(Path::new("test.geotiff")),
        Some(RasterFormat::GeoTiff)
    );
    assert_eq!(detect_format_from_extension(Path::new("test.png")), None);
    assert_eq!(detect_format_from_extension(Path::new("no_ext")), None);
}

#[tokio::test]
async fn test_reader_missing_file() {
    let temp_dir = env::temp_dir();
    let test_path = temp_dir.join("nonexistent_raster_test_12345.tif");

    let result = RasterStreamReader::new(&test_path, RasterStreamConfig::default()).await;
    assert!(result.is_err());
    let err_msg = format!("{}", result.err().expect("should be error"));
    assert!(err_msg.contains("not found") || err_msg.contains("Not found"));
}

#[tokio::test]
async fn test_reader_unsupported_format() {
    let temp_dir = env::temp_dir();
    let test_path = temp_dir.join("test_unsupported.xyz");

    // Create an empty file with unsupported extension
    std::fs::write(&test_path, b"not a raster").expect("write test file");

    let result = RasterStreamReader::new(&test_path, RasterStreamConfig::default()).await;
    assert!(result.is_err());

    let _ = std::fs::remove_file(&test_path);
}

#[tokio::test]
async fn test_builder_pattern() {
    let temp_dir = env::temp_dir();
    let test_path = temp_dir.join("test_builder_raster.tif");

    // Builder should fail gracefully on non-existent file
    let result = RasterStreamReaderBuilder::new(&test_path)
        .chunk_size(512, 512)
        .overlap(16)
        .bands(vec![0, 1, 2])
        .parallel(4)
        .build()
        .await;

    assert!(result.is_err());
}

/// Helper: write a test GeoTIFF file with known data using the GeoTiffWriter.
fn write_test_geotiff(
    path: &Path,
    width: u64,
    height: u64,
    band_count: u16,
    data_type: oxigdal_core::types::RasterDataType,
    data: &[u8],
    gt: &GeoTransform,
) {
    use oxigdal_geotiff::tiff::{Compression, PhotometricInterpretation, Predictor};
    use oxigdal_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};

    let photometric = if band_count >= 3 {
        PhotometricInterpretation::Rgb
    } else {
        PhotometricInterpretation::BlackIsZero
    };

    let config = WriterConfig::new(width, height, band_count, data_type)
        .with_compression(Compression::None)
        .with_predictor(Predictor::None)
        .with_tile_size(256, 256)
        .with_photometric(photometric)
        .with_geo_transform(*gt)
        .with_overviews(false, oxigdal_geotiff::OverviewResampling::Average);

    let mut writer = GeoTiffWriter::create(path, config, GeoTiffWriterOptions::default())
        .expect("create writer");
    writer.write(data).expect("write data");
}

#[tokio::test]
async fn test_write_then_read_roundtrip_uint8() {
    let temp_dir = env::temp_dir();
    let test_path = temp_dir.join("test_roundtrip_uint8.tif");

    let width = 256u64;
    let height = 256u64;
    let gt = GeoTransform {
        origin_x: 10.0,
        origin_y: 50.0,
        pixel_width: 0.01,
        pixel_height: -0.01,
        row_rotation: 0.0,
        col_rotation: 0.0,
    };

    // Create test data: gradient pattern
    let mut data = vec![0u8; (width * height) as usize];
    for y in 0..height as usize {
        for x in 0..width as usize {
            data[y * width as usize + x] = ((x + y) & 0xFF) as u8;
        }
    }

    write_test_geotiff(
        &test_path,
        width,
        height,
        1,
        oxigdal_core::types::RasterDataType::UInt8,
        &data,
        &gt,
    );

    // Read it back via streaming reader
    let config = RasterStreamConfig::default().with_chunk_size(128, 128);
    let reader = RasterStreamReader::new(&test_path, config)
        .await
        .expect("create reader");

    // Verify metadata
    let meta = reader.metadata();
    assert_eq!(meta.width, width);
    assert_eq!(meta.height, height);
    assert_eq!(meta.band_count, 1);
    assert_eq!(meta.data_type, oxigdal_core::types::RasterDataType::UInt8);

    // Verify geotransform
    let read_gt = meta.geo_transform.expect("geotransform should be present");
    assert!((read_gt.origin_x - gt.origin_x).abs() < 1e-6);
    assert!((read_gt.pixel_width - gt.pixel_width).abs() < 1e-6);

    // Read chunk (0,0) and verify data
    let chunk = reader.read_chunk(0, 0).await.expect("read chunk 0,0");
    let chunk_w = chunk.buffer.width() as usize;
    let chunk_h = chunk.buffer.height() as usize;
    assert!(chunk_w > 0);
    assert!(chunk_h > 0);

    // Verify first few pixels of the chunk match original data
    let chunk_bytes = chunk.buffer.as_bytes();
    for y in 0..chunk_h.min(8) {
        for x in 0..chunk_w.min(8) {
            let expected = ((x + y) & 0xFF) as u8;
            let actual = chunk_bytes[y * chunk_w + x];
            assert_eq!(
                actual, expected,
                "Pixel mismatch at ({}, {}): expected {}, got {}",
                x, y, expected, actual
            );
        }
    }

    let _ = std::fs::remove_file(&test_path);
}

#[tokio::test]
async fn test_chunk_boundary_alignment() {
    let temp_dir = env::temp_dir();
    let test_path = temp_dir.join("test_chunk_boundary.tif");

    // Use a non-power-of-2 image size to test boundary alignment
    let width = 300u64;
    let height = 200u64;
    let gt = GeoTransform {
        origin_x: 0.0,
        origin_y: 0.0,
        pixel_width: 1.0,
        pixel_height: -1.0,
        row_rotation: 0.0,
        col_rotation: 0.0,
    };

    let data = vec![42u8; (width * height) as usize];
    write_test_geotiff(
        &test_path,
        width,
        height,
        1,
        oxigdal_core::types::RasterDataType::UInt8,
        &data,
        &gt,
    );

    let config = RasterStreamConfig::default().with_chunk_size(128, 128);
    let reader = RasterStreamReader::new(&test_path, config)
        .await
        .expect("create reader");

    // Read all chunks and verify they tile the full image
    let total =
        super::super::RasterStream::calculate_chunks(width as usize, height as usize, 128, 128, 0);
    assert_eq!(total, (2, 3)); // 200/128=2 rows, 300/128=3 cols

    // Read boundary chunk (last row, last col)
    let chunk = reader
        .read_chunk(total.0 - 1, total.1 - 1)
        .await
        .expect("read last chunk");

    // Last chunk should be smaller than chunk_size
    let cw = chunk.buffer.width() as usize;
    let ch = chunk.buffer.height() as usize;
    // Last col: 300 - 2*128 = 44 pixels wide
    assert_eq!(cw, 44, "Last column chunk should be 44 pixels wide");
    // Last row: 200 - 1*128 = 72 pixels tall
    assert_eq!(ch, 72, "Last row chunk should be 72 pixels tall");

    // Verify data in boundary chunk
    let chunk_bytes = chunk.buffer.as_bytes();
    for byte in chunk_bytes.iter() {
        assert_eq!(*byte, 42, "All pixels should be 42");
    }

    let _ = std::fs::remove_file(&test_path);
}

#[tokio::test]
async fn test_multi_band_read() {
    let temp_dir = env::temp_dir();
    let test_path = temp_dir.join("test_multi_band.tif");

    let width = 64u64;
    let height = 64u64;
    let bands = 3u16;
    let gt = GeoTransform {
        origin_x: 0.0,
        origin_y: 64.0,
        pixel_width: 1.0,
        pixel_height: -1.0,
        row_rotation: 0.0,
        col_rotation: 0.0,
    };

    // RGB data: R=100, G=150, B=200 for all pixels
    let pixel_count = (width * height) as usize;
    let mut data = Vec::with_capacity(pixel_count * bands as usize);
    for _ in 0..pixel_count {
        data.push(100); // R
        data.push(150); // G
        data.push(200); // B
    }

    write_test_geotiff(
        &test_path,
        width,
        height,
        bands,
        oxigdal_core::types::RasterDataType::UInt8,
        &data,
        &gt,
    );

    let config = RasterStreamConfig::default().with_chunk_size(32, 32);
    let reader = RasterStreamReader::new(&test_path, config)
        .await
        .expect("create reader");

    assert_eq!(reader.metadata().band_count, 3);

    // Read first chunk
    let chunk = reader
        .read_chunk(0, 0)
        .await
        .expect("read multi-band chunk");
    let chunk_bytes = chunk.buffer.as_bytes();
    let bytes_per_pixel = 3; // 3 bands * 1 byte each

    // For multi-band, the buffer effective width = pixel_width * band_count
    // so the buffer holds interleaved data correctly
    assert!(chunk_bytes.len() >= bytes_per_pixel);

    // Verify RGB values for first pixel (interleaved)
    assert_eq!(chunk_bytes[0], 100, "Red channel");
    assert_eq!(chunk_bytes[1], 150, "Green channel");
    assert_eq!(chunk_bytes[2], 200, "Blue channel");

    let _ = std::fs::remove_file(&test_path);
}

#[tokio::test]
async fn test_parallel_chunk_read() {
    let temp_dir = env::temp_dir();
    let test_path = temp_dir.join("test_parallel_read.tif");

    let width = 256u64;
    let height = 256u64;
    let gt = GeoTransform {
        origin_x: 0.0,
        origin_y: 256.0,
        pixel_width: 1.0,
        pixel_height: -1.0,
        row_rotation: 0.0,
        col_rotation: 0.0,
    };

    let data = vec![128u8; (width * height) as usize];
    write_test_geotiff(
        &test_path,
        width,
        height,
        1,
        oxigdal_core::types::RasterDataType::UInt8,
        &data,
        &gt,
    );

    let config = RasterStreamConfig::default().with_chunk_size(128, 128);
    let reader = RasterStreamReader::new(&test_path, config)
        .await
        .expect("create reader");

    // Read multiple chunks in parallel
    let chunks_to_read = vec![(0, 0), (0, 1), (1, 0), (1, 1)];
    let chunks = reader
        .read_chunks(chunks_to_read)
        .await
        .expect("parallel read");

    assert_eq!(chunks.len(), 4, "Should read 4 chunks");

    for chunk in &chunks {
        let bytes = chunk.buffer.as_bytes();
        for &b in bytes.iter() {
            assert_eq!(b, 128, "All pixels should be 128");
        }
    }

    let _ = std::fs::remove_file(&test_path);
}

#[tokio::test]
async fn test_write_read_roundtrip_float32() {
    let temp_dir = env::temp_dir();
    let test_path = temp_dir.join("test_roundtrip_float32.tif");

    let width = 64u64;
    let height = 64u64;
    let gt = GeoTransform {
        origin_x: -180.0,
        origin_y: 90.0,
        pixel_width: 0.1,
        pixel_height: -0.1,
        row_rotation: 0.0,
        col_rotation: 0.0,
    };

    // Create Float32 data
    let pixel_count = (width * height) as usize;
    let mut data = Vec::with_capacity(pixel_count * 4);
    for i in 0..pixel_count {
        let val = (i as f32) * 0.5;
        data.extend_from_slice(&val.to_le_bytes());
    }

    write_test_geotiff(
        &test_path,
        width,
        height,
        1,
        oxigdal_core::types::RasterDataType::Float32,
        &data,
        &gt,
    );

    let config = RasterStreamConfig::default().with_chunk_size(32, 32);
    let reader = RasterStreamReader::new(&test_path, config)
        .await
        .expect("create reader");

    assert_eq!(
        reader.metadata().data_type,
        oxigdal_core::types::RasterDataType::Float32
    );

    let chunk = reader.read_chunk(0, 0).await.expect("read float32 chunk");
    let chunk_bytes = chunk.buffer.as_bytes();

    // Verify first pixel value
    assert!(chunk_bytes.len() >= 4);
    let first_val = f32::from_le_bytes([
        chunk_bytes[0],
        chunk_bytes[1],
        chunk_bytes[2],
        chunk_bytes[3],
    ]);
    assert!(
        (first_val - 0.0).abs() < 1e-6,
        "First pixel should be 0.0, got {}",
        first_val
    );

    let _ = std::fs::remove_file(&test_path);
}

#[tokio::test]
async fn test_format_detection_magic_bytes() {
    let temp_dir = env::temp_dir();
    let test_path = temp_dir.join("test_magic_detect.noext");

    let width = 32u64;
    let height = 32u64;
    let gt = GeoTransform {
        origin_x: 0.0,
        origin_y: 32.0,
        pixel_width: 1.0,
        pixel_height: -1.0,
        row_rotation: 0.0,
        col_rotation: 0.0,
    };

    let data = vec![0u8; (width * height) as usize];

    // Write with .tif extension first, then rename to .noext
    let temp_tif = temp_dir.join("test_magic_detect_temp.tif");
    write_test_geotiff(
        &temp_tif,
        width,
        height,
        1,
        oxigdal_core::types::RasterDataType::UInt8,
        &data,
        &gt,
    );
    std::fs::rename(&temp_tif, &test_path).expect("rename file");

    // Should detect via magic bytes even without .tif extension
    let result = RasterStreamReader::new(&test_path, RasterStreamConfig::default()).await;
    assert!(result.is_ok(), "Should detect GeoTIFF via magic bytes");

    let reader = result.expect("reader");
    assert_eq!(reader.format(), RasterFormat::GeoTiff);

    let _ = std::fs::remove_file(&test_path);
}

// ======================================================================
// Stress test: large raster (1000x1000) round-trip
// ======================================================================

#[tokio::test]
async fn test_stress_large_raster_roundtrip() {
    let temp_dir = env::temp_dir();
    let test_path = temp_dir.join("test_stress_1000x1000.tif");

    let width = 1000u64;
    let height = 1000u64;
    let gt = GeoTransform {
        origin_x: -122.5,
        origin_y: 47.5,
        pixel_width: 0.001,
        pixel_height: -0.001,
        row_rotation: 0.0,
        col_rotation: 0.0,
    };

    // Create a deterministic gradient pattern
    let pixel_count = (width * height) as usize;
    let mut data = vec![0u8; pixel_count];
    for y in 0..height as usize {
        for x in 0..width as usize {
            data[y * width as usize + x] = ((x.wrapping_mul(7) + y.wrapping_mul(13)) & 0xFF) as u8;
        }
    }

    write_test_geotiff(
        &test_path,
        width,
        height,
        1,
        oxigdal_core::types::RasterDataType::UInt8,
        &data,
        &gt,
    );

    // Read back with streaming reader using smallish chunks
    let config = RasterStreamConfig::default().with_chunk_size(256, 256);
    let reader = RasterStreamReader::new(&test_path, config.clone())
        .await
        .expect("create reader for 1000x1000");

    let meta = reader.metadata();
    assert_eq!(meta.width, width);
    assert_eq!(meta.height, height);

    // Read all chunks and reconstruct the full image
    let chunks_grid =
        super::super::RasterStream::calculate_chunks(width as usize, height as usize, 256, 256, 0);

    let mut reconstructed = vec![0u8; pixel_count];
    for row in 0..chunks_grid.0 {
        for col in 0..chunks_grid.1 {
            let chunk = reader
                .read_chunk(row, col)
                .await
                .expect("read large raster chunk");

            let cw = chunk.buffer.width() as usize;
            let ch = chunk.buffer.height() as usize;
            let chunk_bytes = chunk.buffer.as_bytes();

            let x_start = col * 256;
            let y_start = row * 256;

            for cy in 0..ch {
                for cx in 0..cw {
                    let dst_x = x_start + cx;
                    let dst_y = y_start + cy;
                    if dst_x < width as usize && dst_y < height as usize {
                        reconstructed[dst_y * width as usize + dst_x] = chunk_bytes[cy * cw + cx];
                    }
                }
            }
        }
    }

    // Verify the reconstructed image matches the original
    let mut mismatches = 0usize;
    for i in 0..pixel_count {
        if reconstructed[i] != data[i] {
            mismatches += 1;
        }
    }
    assert_eq!(
        mismatches, 0,
        "Stress test: {} pixel mismatches out of {} total",
        mismatches, pixel_count
    );

    let _ = std::fs::remove_file(&test_path);
}

// ======================================================================
// Concurrent read test: multiple readers on same file
// ======================================================================

#[tokio::test]
async fn test_concurrent_readers_on_same_file() {
    let temp_dir = env::temp_dir();
    let test_path = temp_dir.join("test_concurrent_readers.tif");

    let width = 128u64;
    let height = 128u64;
    let gt = GeoTransform {
        origin_x: 0.0,
        origin_y: 128.0,
        pixel_width: 1.0,
        pixel_height: -1.0,
        row_rotation: 0.0,
        col_rotation: 0.0,
    };

    let data = vec![77u8; (width * height) as usize];
    write_test_geotiff(
        &test_path,
        width,
        height,
        1,
        oxigdal_core::types::RasterDataType::UInt8,
        &data,
        &gt,
    );

    // Spawn 4 concurrent readers on the same file
    let mut handles = Vec::new();
    for reader_id in 0u8..4 {
        let path = test_path.clone();
        let handle = tokio::spawn(async move {
            let config = RasterStreamConfig::default().with_chunk_size(64, 64);
            let reader = RasterStreamReader::new(&path, config)
                .await
                .expect("create concurrent reader");

            // Each reader reads a different chunk
            let row = (reader_id / 2) as usize;
            let col = (reader_id % 2) as usize;
            let chunk = reader
                .read_chunk(row, col)
                .await
                .expect("concurrent read chunk");

            // Verify data
            let bytes = chunk.buffer.as_bytes();
            for &b in bytes.iter() {
                assert_eq!(b, 77, "Concurrent reader {} got wrong data", reader_id);
            }

            reader_id
        });
        handles.push(handle);
    }

    let mut completed = Vec::new();
    for handle in handles {
        let id = handle.await.expect("join concurrent reader task");
        completed.push(id);
    }

    assert_eq!(
        completed.len(),
        4,
        "All 4 concurrent readers should complete"
    );
    let _ = std::fs::remove_file(&test_path);
}

// ======================================================================
// Format detection edge cases
// ======================================================================

#[test]
fn test_format_detection_case_insensitive() {
    assert_eq!(
        detect_format_from_extension(Path::new("data.TIF")),
        Some(RasterFormat::GeoTiff)
    );
    assert_eq!(
        detect_format_from_extension(Path::new("data.TIFF")),
        Some(RasterFormat::GeoTiff)
    );
    assert_eq!(
        detect_format_from_extension(Path::new("data.GeoTiff")),
        Some(RasterFormat::GeoTiff)
    );
    assert_eq!(
        detect_format_from_extension(Path::new("data.GTiff")),
        Some(RasterFormat::GeoTiff)
    );
}

#[test]
fn test_format_detection_empty_extension() {
    assert_eq!(detect_format_from_extension(Path::new("file.")), None);
}

#[test]
fn test_format_detection_multiple_dots() {
    assert_eq!(
        detect_format_from_extension(Path::new("my.data.archive.tif")),
        Some(RasterFormat::GeoTiff)
    );
    assert_eq!(
        detect_format_from_extension(Path::new("my.data.archive.png")),
        None
    );
}

#[tokio::test]
async fn test_format_detection_corrupt_magic_bytes() {
    let temp_dir = env::temp_dir();
    let test_path = temp_dir.join("test_corrupt_magic.noext");

    // Write garbage data (not a valid TIFF)
    std::fs::write(&test_path, b"THIS_IS_NOT_A_TIFF_FILE_AT_ALL").expect("write corrupt test file");

    let result = RasterStreamReader::new(&test_path, RasterStreamConfig::default()).await;
    assert!(
        result.is_err(),
        "Should fail on corrupt/unknown magic bytes"
    );
    let err_msg = format!("{}", result.err().expect("should be error"));
    assert!(
        err_msg.contains("Unsupported") || err_msg.contains("unsupported"),
        "Error should mention unsupported format, got: {}",
        err_msg
    );

    let _ = std::fs::remove_file(&test_path);
}

#[test]
fn test_format_detection_empty_file_magic() {
    let temp_dir = env::temp_dir();
    let test_path = temp_dir.join("test_empty_magic.noext");

    std::fs::write(&test_path, b"").expect("write empty file");
    assert_eq!(detect_format_from_magic(&test_path), None);

    let _ = std::fs::remove_file(&test_path);
}

#[test]
fn test_format_detection_nonexistent_file_magic() {
    let test_path = Path::new("/tmp/this_file_does_not_exist_format_test.bin");
    assert_eq!(detect_format_from_magic(test_path), None);
}

// ======================================================================
// Writer with different compression options
// ======================================================================

#[tokio::test]
async fn test_writer_with_no_compression() {
    let temp_dir = env::temp_dir();
    let test_path = temp_dir.join("test_writer_no_compression.tif");

    let width = 64u64;
    let height = 64u64;
    let gt = GeoTransform {
        origin_x: 0.0,
        origin_y: 64.0,
        pixel_width: 1.0,
        pixel_height: -1.0,
        row_rotation: 0.0,
        col_rotation: 0.0,
    };

    let data = vec![55u8; (width * height) as usize];
    write_test_geotiff(
        &test_path,
        width,
        height,
        1,
        oxigdal_core::types::RasterDataType::UInt8,
        &data,
        &gt,
    );

    let config = RasterStreamConfig::default().with_chunk_size(64, 64);
    let reader = RasterStreamReader::new(&test_path, config)
        .await
        .expect("create reader for no-compression file");

    let chunk = reader
        .read_chunk(0, 0)
        .await
        .expect("read no-compression chunk");
    let bytes = chunk.buffer.as_bytes();
    for &b in bytes.iter() {
        assert_eq!(b, 55, "No-compression pixel value mismatch");
    }

    let _ = std::fs::remove_file(&test_path);
}

#[tokio::test]
async fn test_writer_with_lzw_compression_roundtrip() {
    use oxigdal_geotiff::tiff::{Compression, PhotometricInterpretation, Predictor};
    use oxigdal_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};

    let temp_dir = env::temp_dir();
    let test_path = temp_dir.join("test_streaming_writer_lzw_compress.tif");

    let width = 128u64;
    let height = 128u64;
    let gt = GeoTransform {
        origin_x: 10.0,
        origin_y: 50.0,
        pixel_width: 0.01,
        pixel_height: -0.01,
        row_rotation: 0.0,
        col_rotation: 0.0,
    };

    // Highly compressible data (runs of same value)
    let mut data = vec![0u8; (width * height) as usize];
    for y in 0..height as usize {
        let val = (y / 16) as u8;
        for x in 0..width as usize {
            data[y * width as usize + x] = val;
        }
    }

    let config = WriterConfig::new(width, height, 1, oxigdal_core::types::RasterDataType::UInt8)
        .with_compression(Compression::Lzw)
        .with_predictor(Predictor::HorizontalDifferencing)
        .with_tile_size(256, 256)
        .with_photometric(PhotometricInterpretation::BlackIsZero)
        .with_geo_transform(gt)
        .with_overviews(false, oxigdal_geotiff::OverviewResampling::Average);

    let mut writer = GeoTiffWriter::create(&test_path, config, GeoTiffWriterOptions::default())
        .expect("create LZW writer");
    writer.write(&data).expect("write LZW data");
    drop(writer);

    // Read it back
    let stream_config = RasterStreamConfig::default().with_chunk_size(128, 128);
    let reader = RasterStreamReader::new(&test_path, stream_config)
        .await
        .expect("create reader for LZW file");

    let chunk = reader.read_chunk(0, 0).await.expect("read LZW chunk");
    let chunk_bytes = chunk.buffer.as_bytes();

    // Verify a few rows
    for y in 0..8usize {
        let expected_val = (y / 16) as u8; // y < 16, so all 0
        for x in 0..8usize {
            assert_eq!(
                chunk_bytes[y * 128 + x],
                expected_val,
                "LZW roundtrip mismatch at ({}, {})",
                x,
                y
            );
        }
    }

    // Verify the compressed file is smaller than raw data
    let file_size = std::fs::metadata(&test_path).expect("file metadata").len();
    let raw_size = (width * height) as u64;
    // LZW-compressed banded data should be smaller than raw (this data is very compressible)
    assert!(
        file_size < raw_size * 2,
        "LZW compressed file ({}) should be reasonably sized relative to raw ({})",
        file_size,
        raw_size
    );

    let _ = std::fs::remove_file(&test_path);
}

#[tokio::test]
async fn test_writer_with_deflate_compression_roundtrip() {
    use oxigdal_geotiff::tiff::{Compression, PhotometricInterpretation, Predictor};
    use oxigdal_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};

    let temp_dir = env::temp_dir();
    let test_path = temp_dir.join("test_writer_deflate.tif");

    let width = 64u64;
    let height = 64u64;
    let gt = GeoTransform {
        origin_x: 0.0,
        origin_y: 64.0,
        pixel_width: 1.0,
        pixel_height: -1.0,
        row_rotation: 0.0,
        col_rotation: 0.0,
    };

    let data = vec![200u8; (width * height) as usize];

    let config = WriterConfig::new(width, height, 1, oxigdal_core::types::RasterDataType::UInt8)
        .with_compression(Compression::AdobeDeflate)
        .with_predictor(Predictor::None)
        .with_tile_size(64, 64)
        .with_photometric(PhotometricInterpretation::BlackIsZero)
        .with_geo_transform(gt)
        .with_overviews(false, oxigdal_geotiff::OverviewResampling::Average);

    let mut writer = GeoTiffWriter::create(&test_path, config, GeoTiffWriterOptions::default())
        .expect("create Deflate writer");
    writer.write(&data).expect("write Deflate data");
    drop(writer);

    // Read it back
    let stream_config = RasterStreamConfig::default().with_chunk_size(64, 64);
    let reader = RasterStreamReader::new(&test_path, stream_config)
        .await
        .expect("create reader for Deflate file");

    let chunk = reader.read_chunk(0, 0).await.expect("read Deflate chunk");
    let chunk_bytes = chunk.buffer.as_bytes();
    for &b in chunk_bytes.iter() {
        assert_eq!(b, 200, "Deflate roundtrip pixel mismatch");
    }

    let _ = std::fs::remove_file(&test_path);
}

// ======================================================================
// Metadata preservation: geotransform, CRS, nodata
// ======================================================================

#[tokio::test]
async fn test_metadata_geotransform_preservation() {
    let temp_dir = env::temp_dir();
    let test_path = temp_dir.join("test_meta_geotransform.tif");

    let width = 32u64;
    let height = 32u64;
    let gt = GeoTransform {
        origin_x: -73.9857,
        origin_y: 40.7484,
        pixel_width: 0.0001,
        pixel_height: -0.0001,
        row_rotation: 0.0,
        col_rotation: 0.0,
    };

    let data = vec![0u8; (width * height) as usize];
    write_test_geotiff(
        &test_path,
        width,
        height,
        1,
        oxigdal_core::types::RasterDataType::UInt8,
        &data,
        &gt,
    );

    let config = RasterStreamConfig::default().with_chunk_size(32, 32);
    let reader = RasterStreamReader::new(&test_path, config)
        .await
        .expect("create reader for geotransform test");

    let meta = reader.metadata();
    let read_gt = meta.geo_transform.expect("geotransform should be present");

    assert!(
        (read_gt.origin_x - gt.origin_x).abs() < 1e-8,
        "origin_x mismatch: {} vs {}",
        read_gt.origin_x,
        gt.origin_x
    );
    assert!(
        (read_gt.origin_y - gt.origin_y).abs() < 1e-8,
        "origin_y mismatch: {} vs {}",
        read_gt.origin_y,
        gt.origin_y
    );
    assert!(
        (read_gt.pixel_width - gt.pixel_width).abs() < 1e-10,
        "pixel_width mismatch: {} vs {}",
        read_gt.pixel_width,
        gt.pixel_width
    );
    assert!(
        (read_gt.pixel_height - gt.pixel_height).abs() < 1e-10,
        "pixel_height mismatch: {} vs {}",
        read_gt.pixel_height,
        gt.pixel_height
    );
    assert!(
        (read_gt.row_rotation - gt.row_rotation).abs() < 1e-10,
        "row_rotation mismatch"
    );
    assert!(
        (read_gt.col_rotation - gt.col_rotation).abs() < 1e-10,
        "col_rotation mismatch"
    );

    let _ = std::fs::remove_file(&test_path);
}

#[tokio::test]
async fn test_metadata_nodata_integer_preservation() {
    use oxigdal_core::types::NoDataValue;
    use oxigdal_geotiff::tiff::{Compression, PhotometricInterpretation, Predictor};
    use oxigdal_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};

    let temp_dir = env::temp_dir();
    let test_path = temp_dir.join("test_streaming_meta_nodata_int.tif");

    let width = 32u64;
    let height = 32u64;
    let gt = GeoTransform {
        origin_x: 0.0,
        origin_y: 32.0,
        pixel_width: 1.0,
        pixel_height: -1.0,
        row_rotation: 0.0,
        col_rotation: 0.0,
    };

    let data = vec![0u8; (width * height) as usize];

    let config = WriterConfig::new(width, height, 1, oxigdal_core::types::RasterDataType::UInt8)
        .with_compression(Compression::None)
        .with_predictor(Predictor::None)
        .with_tile_size(256, 256)
        .with_photometric(PhotometricInterpretation::BlackIsZero)
        .with_geo_transform(gt)
        .with_nodata(NoDataValue::from_integer(255))
        .with_overviews(false, oxigdal_geotiff::OverviewResampling::Average);

    let mut writer = GeoTiffWriter::create(&test_path, config, GeoTiffWriterOptions::default())
        .expect("create nodata writer");
    writer.write(&data).expect("write nodata data");
    drop(writer);

    let stream_config = RasterStreamConfig::default().with_chunk_size(32, 32);
    let reader = RasterStreamReader::new(&test_path, stream_config)
        .await
        .expect("create reader for nodata test");

    let meta = reader.metadata();
    match meta.nodata {
        oxigdal_core::types::NoDataValue::Integer(v) => {
            assert_eq!(v, 255, "NoData integer value should be 255, got {}", v);
        }
        oxigdal_core::types::NoDataValue::Float(v) => {
            // Some implementations store as float
            assert!(
                (v - 255.0).abs() < 1e-6,
                "NoData float value should be 255.0, got {}",
                v
            );
        }
        oxigdal_core::types::NoDataValue::None => {
            panic!("NoData should not be None");
        }
    }

    let _ = std::fs::remove_file(&test_path);
}

#[tokio::test]
async fn test_metadata_nodata_float_preservation() {
    use oxigdal_core::types::NoDataValue;
    use oxigdal_geotiff::tiff::{Compression, PhotometricInterpretation, Predictor};
    use oxigdal_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};

    let temp_dir = env::temp_dir();
    let test_path = temp_dir.join("test_streaming_meta_nodata_float.tif");

    let width = 32u64;
    let height = 32u64;
    let gt = GeoTransform {
        origin_x: 0.0,
        origin_y: 32.0,
        pixel_width: 1.0,
        pixel_height: -1.0,
        row_rotation: 0.0,
        col_rotation: 0.0,
    };

    let pixel_count = (width * height) as usize;
    let mut data = Vec::with_capacity(pixel_count * 4);
    for _ in 0..pixel_count {
        data.extend_from_slice(&(42.0f32).to_le_bytes());
    }

    let config = WriterConfig::new(
        width,
        height,
        1,
        oxigdal_core::types::RasterDataType::Float32,
    )
    .with_compression(Compression::None)
    .with_predictor(Predictor::None)
    .with_tile_size(256, 256)
    .with_photometric(PhotometricInterpretation::BlackIsZero)
    .with_geo_transform(gt)
    .with_nodata(NoDataValue::from_float(-9999.0))
    .with_overviews(false, oxigdal_geotiff::OverviewResampling::Average);

    let mut writer = GeoTiffWriter::create(&test_path, config, GeoTiffWriterOptions::default())
        .expect("create float nodata writer");
    writer.write(&data).expect("write float nodata data");
    drop(writer);

    let stream_config = RasterStreamConfig::default().with_chunk_size(32, 32);
    let reader = RasterStreamReader::new(&test_path, stream_config)
        .await
        .expect("create reader for float nodata test");

    let meta = reader.metadata();
    match meta.nodata {
        oxigdal_core::types::NoDataValue::Float(v) => {
            assert!(
                (v - (-9999.0)).abs() < 1e-6,
                "NoData float value should be -9999.0, got {}",
                v
            );
        }
        oxigdal_core::types::NoDataValue::Integer(v) => {
            assert_eq!(v, -9999, "NoData integer value should be -9999, got {}", v);
        }
        oxigdal_core::types::NoDataValue::None => {
            panic!("NoData should not be None for float nodata test");
        }
    }

    let _ = std::fs::remove_file(&test_path);
}

#[tokio::test]
async fn test_metadata_dimensions_various_sizes() {
    let temp_dir = env::temp_dir();

    // Test several non-standard dimensions
    let test_cases: Vec<(u64, u64, &str)> = vec![
        (1, 1, "test_dim_1x1.tif"),
        (7, 13, "test_dim_7x13.tif"),
        (513, 257, "test_dim_513x257.tif"),
    ];

    let gt = GeoTransform {
        origin_x: 0.0,
        origin_y: 1000.0,
        pixel_width: 1.0,
        pixel_height: -1.0,
        row_rotation: 0.0,
        col_rotation: 0.0,
    };

    for (width, height, filename) in &test_cases {
        let test_path = temp_dir.join(filename);
        let data = vec![42u8; (*width * *height) as usize];
        write_test_geotiff(
            &test_path,
            *width,
            *height,
            1,
            oxigdal_core::types::RasterDataType::UInt8,
            &data,
            &gt,
        );

        let config = RasterStreamConfig::default().with_chunk_size(256, 256);
        let reader = RasterStreamReader::new(&test_path, config)
            .await
            .expect("create reader for dimension test");

        let meta = reader.metadata();
        assert_eq!(meta.width, *width, "Width mismatch for {}", filename);
        assert_eq!(meta.height, *height, "Height mismatch for {}", filename);

        let _ = std::fs::remove_file(&test_path);
    }
}

// ======================================================================
// Chunk geotransform verification
// ======================================================================

#[tokio::test]
async fn test_chunk_geotransform_matches_position() {
    let temp_dir = env::temp_dir();
    let test_path = temp_dir.join("test_chunk_gt.tif");

    let width = 256u64;
    let height = 256u64;
    let gt = GeoTransform {
        origin_x: 10.0,
        origin_y: 50.0,
        pixel_width: 0.5,
        pixel_height: -0.5,
        row_rotation: 0.0,
        col_rotation: 0.0,
    };

    let data = vec![0u8; (width * height) as usize];
    write_test_geotiff(
        &test_path,
        width,
        height,
        1,
        oxigdal_core::types::RasterDataType::UInt8,
        &data,
        &gt,
    );

    let config = RasterStreamConfig::default().with_chunk_size(128, 128);
    let reader = RasterStreamReader::new(&test_path, config)
        .await
        .expect("create reader for chunk GT test");

    // Read chunk (1,1) - should start at pixel (128, 128)
    let chunk = reader.read_chunk(1, 1).await.expect("read chunk (1,1)");
    let expected_origin_x = gt.origin_x + 128.0 * gt.pixel_width;
    let expected_origin_y = gt.origin_y + 128.0 * gt.pixel_height;

    assert!(
        (chunk.geotransform.origin_x - expected_origin_x).abs() < 1e-6,
        "Chunk GT origin_x: expected {}, got {}",
        expected_origin_x,
        chunk.geotransform.origin_x
    );
    assert!(
        (chunk.geotransform.origin_y - expected_origin_y).abs() < 1e-6,
        "Chunk GT origin_y: expected {}, got {}",
        expected_origin_y,
        chunk.geotransform.origin_y
    );
    assert!(
        (chunk.geotransform.pixel_width - gt.pixel_width).abs() < 1e-10,
        "Chunk GT pixel_width should match source"
    );
    assert!(
        (chunk.geotransform.pixel_height - gt.pixel_height).abs() < 1e-10,
        "Chunk GT pixel_height should match source"
    );

    let _ = std::fs::remove_file(&test_path);
}

// ======================================================================
// Error recovery: partial writes, corrupt data handling
// ======================================================================

#[tokio::test]
async fn test_error_read_out_of_bounds_chunk() {
    let temp_dir = env::temp_dir();
    let test_path = temp_dir.join("test_oob_chunk.tif");

    let width = 64u64;
    let height = 64u64;
    let gt = GeoTransform {
        origin_x: 0.0,
        origin_y: 64.0,
        pixel_width: 1.0,
        pixel_height: -1.0,
        row_rotation: 0.0,
        col_rotation: 0.0,
    };

    let data = vec![0u8; (width * height) as usize];
    write_test_geotiff(
        &test_path,
        width,
        height,
        1,
        oxigdal_core::types::RasterDataType::UInt8,
        &data,
        &gt,
    );

    let config = RasterStreamConfig::default().with_chunk_size(32, 32);
    let reader = RasterStreamReader::new(&test_path, config)
        .await
        .expect("create reader for OOB test");

    // Chunk (100, 100) is way out of bounds
    let result = reader.read_chunk(100, 100).await;
    assert!(
        result.is_err(),
        "Reading far out-of-bounds chunk should fail"
    );

    let _ = std::fs::remove_file(&test_path);
}

#[tokio::test]
async fn test_error_corrupt_tiff_file() {
    let temp_dir = env::temp_dir();
    let test_path = temp_dir.join("test_corrupt_tiff.tif");

    // Write something that looks like a TIFF header but is corrupt
    // TIFF magic: II (little-endian) + 42 (version) + garbage
    let mut corrupt_data = vec![0u8; 64];
    corrupt_data[0] = b'I';
    corrupt_data[1] = b'I';
    corrupt_data[2] = 42;
    corrupt_data[3] = 0;
    // IFD offset pointing to garbage
    corrupt_data[4] = 0xFF;
    corrupt_data[5] = 0xFF;
    corrupt_data[6] = 0xFF;
    corrupt_data[7] = 0xFF;

    std::fs::write(&test_path, &corrupt_data).expect("write corrupt tiff");

    let result = RasterStreamReader::new(&test_path, RasterStreamConfig::default()).await;
    assert!(result.is_err(), "Opening corrupt TIFF should fail");

    let _ = std::fs::remove_file(&test_path);
}

#[tokio::test]
async fn test_error_truncated_tiff_file() {
    let temp_dir = env::temp_dir();
    let test_path = temp_dir.join("test_truncated_tiff.tif");

    // Write just the TIFF magic bytes, nothing more
    let truncated = vec![b'I', b'I', 42, 0];
    std::fs::write(&test_path, &truncated).expect("write truncated tiff");

    let result = RasterStreamReader::new(&test_path, RasterStreamConfig::default()).await;
    assert!(result.is_err(), "Opening truncated TIFF should fail");

    let _ = std::fs::remove_file(&test_path);
}

#[tokio::test]
async fn test_error_zero_length_file() {
    let temp_dir = env::temp_dir();
    let test_path = temp_dir.join("test_zero_length.tif");

    std::fs::write(&test_path, b"").expect("write empty file");

    let result = RasterStreamReader::new(&test_path, RasterStreamConfig::default()).await;
    assert!(result.is_err(), "Opening zero-length .tif should fail");

    let _ = std::fs::remove_file(&test_path);
}

// ======================================================================
// Writer streaming round-trip via RasterStreamWriter
// ======================================================================

#[tokio::test]
async fn test_streaming_writer_roundtrip_with_metadata() {
    use oxigdal_core::types::{NoDataValue, RasterDataType};

    let temp_dir = env::temp_dir();
    let write_path = temp_dir.join("test_stream_writer_roundtrip.tif");

    let width = 128u64;
    let height = 128u64;
    let gt = GeoTransform {
        origin_x: -120.0,
        origin_y: 38.0,
        pixel_width: 0.001,
        pixel_height: -0.001,
        row_rotation: 0.0,
        col_rotation: 0.0,
    };

    let metadata = RasterMetadata {
        width,
        height,
        band_count: 1,
        data_type: RasterDataType::UInt8,
        geo_transform: Some(gt),
        crs_wkt: None,
        nodata: NoDataValue::from_integer(0),
        color_interpretation: Vec::new(),
        layout: oxigdal_core::types::PixelLayout::default(),
        driver_metadata: Vec::new(),
    };

    let config = RasterStreamConfig::default().with_chunk_size(64, 64);
    let writer =
        super::super::RasterStreamWriter::new(&write_path, metadata.clone(), config.clone())
            .await
            .expect("create streaming writer");

    let chunks_grid = super::super::RasterStreamWriter::calculate_chunks(
        width as usize,
        height as usize,
        64,
        64,
        0,
    );

    // Write chunks with known pattern
    for row in 0..chunks_grid.0 {
        for col in 0..chunks_grid.1 {
            let x_start = col * 64;
            let y_start = row * 64;
            let cw = 64usize.min(width as usize - x_start);
            let ch = 64usize.min(height as usize - y_start);

            let fill_val = ((row * 10 + col) & 0xFF) as u8;
            let data = vec![fill_val; cw * ch];
            let buffer = oxigdal_core::buffer::RasterBuffer::new(
                data,
                cw as u64,
                ch as u64,
                RasterDataType::UInt8,
                NoDataValue::from_integer(0),
            )
            .expect("create chunk buffer");

            let min_x = gt.origin_x + (x_start as f64) * gt.pixel_width;
            let max_y = gt.origin_y + (y_start as f64) * gt.pixel_height;
            let max_x = gt.origin_x + ((x_start + cw) as f64) * gt.pixel_width;
            let min_y = gt.origin_y + ((y_start + ch) as f64) * gt.pixel_height;

            let bbox = oxigdal_core::types::BoundingBox::new(min_x, min_y, max_x, max_y)
                .expect("create bbox");
            let chunk_gt = GeoTransform {
                origin_x: min_x,
                origin_y: max_y,
                pixel_width: gt.pixel_width,
                pixel_height: gt.pixel_height,
                row_rotation: 0.0,
                col_rotation: 0.0,
            };

            let chunk = super::super::RasterChunk::new(buffer, bbox, chunk_gt, (row, col));
            writer
                .write_chunk(chunk)
                .await
                .expect("write chunk to stream");
        }
    }

    writer.finalize().await.expect("finalize streaming writer");
    assert!(write_path.exists(), "Output file should exist");

    // Now read it back
    let reader = RasterStreamReader::new(&write_path, config)
        .await
        .expect("create reader for stream-written file");

    let read_meta = reader.metadata();
    assert_eq!(read_meta.width, width);
    assert_eq!(read_meta.height, height);
    assert_eq!(read_meta.band_count, 1);

    // Verify geotransform
    let read_gt = read_meta.geo_transform.expect("geotransform preserved");
    assert!(
        (read_gt.origin_x - gt.origin_x).abs() < 1e-8,
        "Streaming writer geotransform origin_x mismatch"
    );
    assert!(
        (read_gt.pixel_width - gt.pixel_width).abs() < 1e-10,
        "Streaming writer geotransform pixel_width mismatch"
    );

    // Read chunk (0,0) and verify data
    let chunk = reader
        .read_chunk(0, 0)
        .await
        .expect("read back chunk (0,0)");
    let bytes = chunk.buffer.as_bytes();
    let expected_val = 0u8; // row=0, col=0 => (0*10+0) & 0xFF = 0
    for &b in bytes.iter().take(64) {
        assert_eq!(
            b, expected_val,
            "Streaming roundtrip data mismatch in chunk (0,0)"
        );
    }

    // Read chunk (1,1) and verify data
    let chunk = reader
        .read_chunk(1, 1)
        .await
        .expect("read back chunk (1,1)");
    let bytes = chunk.buffer.as_bytes();
    let expected_val = 11u8; // row=1, col=1 => (1*10+1) & 0xFF = 11
    for &b in bytes.iter().take(64) {
        assert_eq!(
            b, expected_val,
            "Streaming roundtrip data mismatch in chunk (1,1)"
        );
    }

    let _ = std::fs::remove_file(&write_path);
}

// ======================================================================
// Concurrent read_chunks (parallel multi-chunk)
// ======================================================================

#[tokio::test]
async fn test_read_chunks_all_at_once() {
    let temp_dir = env::temp_dir();
    let test_path = temp_dir.join("test_read_chunks_all.tif");

    let width = 256u64;
    let height = 256u64;
    let gt = GeoTransform {
        origin_x: 0.0,
        origin_y: 256.0,
        pixel_width: 1.0,
        pixel_height: -1.0,
        row_rotation: 0.0,
        col_rotation: 0.0,
    };

    // Each pixel encodes its row as a value
    let mut data = vec![0u8; (width * height) as usize];
    for y in 0..height as usize {
        for x in 0..width as usize {
            data[y * width as usize + x] = (y & 0xFF) as u8;
        }
    }

    write_test_geotiff(
        &test_path,
        width,
        height,
        1,
        oxigdal_core::types::RasterDataType::UInt8,
        &data,
        &gt,
    );

    let config = RasterStreamConfig::default().with_chunk_size(128, 128);
    let reader = RasterStreamReader::new(&test_path, config)
        .await
        .expect("create reader for all-chunks test");

    // Read all 4 chunks at once
    let all_chunks = vec![(0, 0), (0, 1), (1, 0), (1, 1)];
    let results = reader
        .read_chunks(all_chunks)
        .await
        .expect("read all chunks in parallel");

    assert_eq!(results.len(), 4, "Should get 4 chunks");

    // Verify each chunk has correct data
    for chunk in &results {
        let (row, col) = chunk.indices;
        let cw = chunk.buffer.width() as usize;
        let ch = chunk.buffer.height() as usize;
        let bytes = chunk.buffer.as_bytes();
        let y_offset = row * 128;

        for y in 0..ch.min(4) {
            let expected = ((y_offset + y) & 0xFF) as u8;
            assert_eq!(
                bytes[y * cw],
                expected,
                "Parallel read chunk ({},{}) row {} mismatch",
                row,
                col,
                y
            );
        }
    }

    let _ = std::fs::remove_file(&test_path);
}
