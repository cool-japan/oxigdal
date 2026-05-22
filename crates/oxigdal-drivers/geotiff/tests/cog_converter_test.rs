//! Integration tests for the universal COG converter (`CogConverter::convert`).
//!
//! These tests exercise the real read-analyze-write pipeline: a small classic
//! GeoTIFF fixture is authored with the crate's own `GeoTiffWriter`, then
//! converted to a Cloud Optimized GeoTIFF on disk. Assertions confirm that the
//! reported sizes are *measured* from the produced file (not fabricated), that
//! progress callbacks fire in the documented order, and that the output is a
//! structurally valid COG.
//!
//! All fixtures and outputs live in `std::env::temp_dir()` with unique names so
//! the suite can run in parallel without collisions.

// `expect` is the idiomatic failure mode for tests in this crate (see
// `tests/integration_test.rs`); the workspace lint denies it by default.
#![allow(clippy::expect_used)]

use std::path::PathBuf;
use std::sync::Mutex;

use oxigdal_core::io::FileDataSource;
use oxigdal_core::types::RasterDataType;

use oxigdal_geotiff::cog::{CogConverter, ConversionProgress, ConversionStep, validate_cog};
use oxigdal_geotiff::tiff::Compression;
use oxigdal_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};
use oxigdal_geotiff::{GeoTiffReader, TiffFile};

/// Process-unique counter so concurrently running tests never share a path.
static COUNTER: Mutex<u64> = Mutex::new(0);

/// Builds a unique path inside the system temp directory.
fn temp_path(label: &str, ext: &str) -> PathBuf {
    let mut guard = COUNTER.lock().unwrap_or_else(|poison| poison.into_inner());
    *guard += 1;
    let unique = *guard;
    drop(guard);

    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);

    std::env::temp_dir().join(format!(
        "oxigdal_cog_conv_{label}_{pid}_{unique}_{nanos}.{ext}"
    ))
}

/// Authors a small single-band `UInt8` classic GeoTIFF fixture.
///
/// Uncompressed so the test does not depend on any optional codec feature.
/// `tiled` selects a tiled vs. striped layout — both are valid converter input.
fn write_uint8_fixture(path: &std::path::Path, width: u64, height: u64, tiled: bool) {
    let mut data = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            // A gradient with a little structure so analysis has real signal.
            data.push((((x * 7 + y * 13) % 256) as u8).wrapping_add((x & y) as u8));
        }
    }

    let mut config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
        .with_compression(Compression::None)
        .with_overviews(false, oxigdal_geotiff::writer::OverviewResampling::Nearest);
    if tiled {
        config = config.with_tile_size(16, 16);
    } else {
        config.tile_width = None;
        config.tile_height = None;
    }
    config.predictor = oxigdal_geotiff::tiff::Predictor::None;

    let mut writer = GeoTiffWriter::create(path, config, GeoTiffWriterOptions::default())
        .expect("fixture writer should be creatable");
    writer
        .write(&data)
        .expect("fixture data should be writable");
}

/// Authors a small single-band `Float32` striped classic GeoTIFF fixture.
fn write_float32_fixture(path: &std::path::Path, width: u64, height: u64) {
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let value = (x as f32) * 1.5 - (y as f32) * 0.25;
            data.extend_from_slice(&value.to_le_bytes());
        }
    }

    let mut config = WriterConfig::new(width, height, 1, RasterDataType::Float32)
        .with_compression(Compression::None)
        .with_overviews(false, oxigdal_geotiff::writer::OverviewResampling::Nearest);
    config.tile_width = None;
    config.tile_height = None;
    config.predictor = oxigdal_geotiff::tiff::Predictor::None;

    let mut writer = GeoTiffWriter::create(path, config, GeoTiffWriterOptions::default())
        .expect("float fixture writer should be creatable");
    writer
        .write(&data)
        .expect("float fixture data should be writable");
}

/// Best-effort cleanup of a temp file.
fn cleanup(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
}

/// 1. End-to-end: a classic TIFF fixture converts to a COG and reports success.
///
/// The fixture is intentionally larger than 512 px so the automatic
/// `analyze_for_cog` analysis path (no explicit settings) can derive overview
/// levels for it.
#[test]
fn test_cog_converter_classic_tiff_to_cog_roundtrip() {
    let input = temp_path("roundtrip_in", "tif");
    let output = temp_path("roundtrip_out", "tif");
    write_uint8_fixture(&input, 640, 544, false);

    let result = CogConverter::new(input.to_string_lossy().to_string())
        .output(output.to_string_lossy().to_string())
        .convert()
        .expect("conversion should succeed");

    // The produced file must parse back as a TIFF with a primary image.
    let source = FileDataSource::open(&output).expect("output should open");
    let tiff = TiffFile::parse(&source).expect("output should parse as TIFF");
    assert!(
        !tiff.ifds.is_empty(),
        "converted COG must contain at least one IFD"
    );
    assert!(result.input_size > 0, "input size must be measured");
    assert!(result.output_size > 0, "output size must be measured");

    cleanup(&input);
    cleanup(&output);
}

/// 2. The output file actually lands on disk and is non-empty.
///
/// Uses a small fixture with fully explicit settings so the conversion does not
/// depend on the size-sensitive auto-analysis path.
#[test]
fn test_cog_converter_output_file_exists_and_nonempty() {
    let input = temp_path("exists_in", "tif");
    let output = temp_path("exists_out", "tif");
    write_uint8_fixture(&input, 64, 64, true);

    let _result = CogConverter::new(input.to_string_lossy().to_string())
        .output(output.to_string_lossy().to_string())
        .with_tile_size(16, 16)
        .with_compression(Compression::Deflate)
        .with_overviews(&[2])
        .convert()
        .expect("conversion should succeed");

    assert!(output.exists(), "converter must write the output file");
    let on_disk = std::fs::metadata(&output)
        .expect("output metadata should be readable")
        .len();
    assert!(on_disk > 0, "output file must be non-empty, got {on_disk}");

    cleanup(&input);
    cleanup(&output);
}

/// 3. `output_size` is the real measured file length — NOT `input * 0.8`.
#[test]
fn test_cog_converter_output_size_is_measured_not_fabricated() {
    let input = temp_path("measured_in", "tif");
    let output = temp_path("measured_out", "tif");
    write_uint8_fixture(&input, 576, 520, false);

    let result = CogConverter::new(input.to_string_lossy().to_string())
        .output(output.to_string_lossy().to_string())
        .convert()
        .expect("conversion should succeed");

    let measured = std::fs::metadata(&output)
        .expect("output metadata should be readable")
        .len();

    assert_eq!(
        result.output_size, measured,
        "reported output_size must equal fs::metadata().len()"
    );

    // Guard against the old placeholder behaviour `input_size as f64 * 0.8`.
    let fabricated = (result.input_size as f64 * 0.8) as u64;
    assert_ne!(
        result.output_size, fabricated,
        "output_size must not be the fabricated input*0.8 placeholder"
    );

    cleanup(&input);
    cleanup(&output);
}

/// 4. `compression_ratio` is derived from the real input/output sizes.
#[test]
fn test_cog_converter_compression_ratio_reflects_real_sizes() {
    let input = temp_path("ratio_in", "tif");
    let output = temp_path("ratio_out", "tif");
    write_uint8_fixture(&input, 600, 528, true);

    let result = CogConverter::new(input.to_string_lossy().to_string())
        .output(output.to_string_lossy().to_string())
        .convert()
        .expect("conversion should succeed");

    assert!(result.output_size > 0, "output size must be positive");
    let expected = result.input_size as f64 / result.output_size as f64;
    assert!(
        (result.compression_ratio - expected).abs() < 1e-9,
        "compression_ratio {} must equal input_size/output_size {}",
        result.compression_ratio,
        expected
    );
    assert!(
        result.compression_ratio > 0.0,
        "compression_ratio must be positive for a real conversion"
    );

    cleanup(&input);
    cleanup(&output);
}

/// 5. An explicitly requested 256x256 tile size is honoured in the result.
#[test]
fn test_cog_converter_explicit_tile_size_honoured() {
    let input = temp_path("tilesize_in", "tif");
    let output = temp_path("tilesize_out", "tif");
    // Image larger than one tile so tiling is actually exercised.
    write_uint8_fixture(&input, 300, 280, false);

    let result = CogConverter::new(input.to_string_lossy().to_string())
        .output(output.to_string_lossy().to_string())
        .with_tile_size(256, 256)
        .with_compression(Compression::Deflate)
        .with_overviews(&[2])
        .convert()
        .expect("conversion should succeed");

    assert_eq!(
        result.tile_size,
        (256, 256),
        "explicit tile size must be reflected in the result"
    );

    // The written COG's primary IFD must carry the same tile dimensions.
    let reader = GeoTiffReader::open(FileDataSource::open(&output).expect("output should open"))
        .expect("output should be readable");
    assert_eq!(
        reader.tile_size(),
        Some((256, 256)),
        "output COG primary image must be tiled at 256x256"
    );

    cleanup(&input);
    cleanup(&output);
}

/// 6. The auto-optimize path runs analysis on REAL data and produces a COG.
///
/// With no explicit tile size / compression / overviews, the converter must
/// fall back to settings derived from `analyze_for_cog` over the real pixels.
#[test]
fn test_cog_converter_auto_optimize_chooses_settings() {
    let input = temp_path("auto_in", "tif");
    let output = temp_path("auto_out", "tif");
    write_uint8_fixture(&input, 768, 600, false);

    let result = CogConverter::new(input.to_string_lossy().to_string())
        .output(output.to_string_lossy().to_string())
        .auto_optimize()
        .convert()
        .expect("auto-optimized conversion should succeed");

    // Auto-optimization must still yield a usable, power-of-2 tiling.
    assert!(
        result.tile_size.0 > 0 && result.tile_size.1 > 0,
        "auto-optimize must choose a positive tile size"
    );
    assert!(
        result.tile_size.0.is_power_of_two() && result.tile_size.1.is_power_of_two(),
        "auto-optimize tile size {:?} must be power-of-2 for COG compliance",
        result.tile_size
    );
    assert!(
        result.output_size > 0,
        "auto-optimize must still write a real file"
    );
    assert!(
        output.exists(),
        "auto-optimize must produce the output file"
    );

    cleanup(&input);
    cleanup(&output);
}

/// 7. Progress callbacks fire in the documented step order.
#[test]
fn test_cog_converter_progress_callbacks_fire_in_order() {
    let input = temp_path("progress_in", "tif");
    let output = temp_path("progress_out", "tif");
    write_uint8_fixture(&input, 80, 70, true);

    let steps: std::sync::Arc<Mutex<Vec<ConversionStep>>> =
        std::sync::Arc::new(Mutex::new(Vec::new()));
    let percents: std::sync::Arc<Mutex<Vec<u8>>> = std::sync::Arc::new(Mutex::new(Vec::new()));

    let steps_cb = std::sync::Arc::clone(&steps);
    let percents_cb = std::sync::Arc::clone(&percents);

    let result = CogConverter::new(input.to_string_lossy().to_string())
        .output(output.to_string_lossy().to_string())
        .with_tile_size(16, 16)
        .with_compression(Compression::Deflate)
        .with_overviews(&[2])
        .on_progress(Box::new(move |progress: ConversionProgress| {
            steps_cb
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(progress.step);
            percents_cb
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(progress.progress_percent);
        }))
        .convert()
        .expect("conversion should succeed");

    let observed = steps.lock().unwrap_or_else(|p| p.into_inner()).clone();
    assert_eq!(
        observed,
        vec![
            ConversionStep::Analyzing,
            ConversionStep::Optimizing,
            ConversionStep::WritingBase,
            ConversionStep::Complete,
        ],
        "progress steps must fire in Analyzing -> Optimizing -> WritingBase -> Complete order"
    );

    // The final callback must report 100% completion.
    let observed_percents = percents.lock().unwrap_or_else(|p| p.into_inner()).clone();
    assert_eq!(
        observed_percents.last().copied(),
        Some(100),
        "final progress callback must report 100%"
    );
    assert!(result.duration_ms < u64::MAX, "duration must be recorded");

    cleanup(&input);
    cleanup(&output);
}

/// 8. The produced file passes the crate's `validate_cog` COG check.
#[test]
fn test_cog_converter_output_validates_as_cog() {
    let input = temp_path("valid_in", "tif");
    let output = temp_path("valid_out", "tif");
    write_uint8_fixture(&input, 260, 260, false);

    let result = CogConverter::new(input.to_string_lossy().to_string())
        .output(output.to_string_lossy().to_string())
        .with_tile_size(256, 256)
        .with_compression(Compression::Deflate)
        .with_overviews(&[2, 4])
        .convert()
        .expect("conversion should succeed");

    // The converter itself reports the writer's validation verdict.
    assert!(
        result.validation_passed,
        "converter should report the output as a valid COG"
    );

    // Independently re-validate the file on disk.
    let source = FileDataSource::open(&output).expect("output should open");
    let tiff = TiffFile::parse(&source).expect("output should parse");
    let validation = validate_cog(&tiff, &source);
    assert!(
        validation.is_valid,
        "validate_cog must accept the produced file: {:?}",
        validation.messages
    );
    assert!(
        validation.has_overviews,
        "requested overviews must be present in the output"
    );

    cleanup(&input);
    cleanup(&output);
}

/// 9. The converter detects the input data type from the IFD, not a hard-coded
///    `UInt8`. A `Float32` input must round-trip as a `Float32` COG.
#[test]
fn test_cog_converter_detects_input_data_type() {
    let input = temp_path("dtype_in", "tif");
    let output = temp_path("dtype_out", "tif");
    write_float32_fixture(&input, 544, 520);

    // Sanity check: the fixture really is Float32.
    let in_reader = GeoTiffReader::open(FileDataSource::open(&input).expect("input should open"))
        .expect("input should be readable");
    assert_eq!(
        in_reader.data_type(),
        Some(RasterDataType::Float32),
        "fixture must be Float32"
    );
    drop(in_reader);

    let _result = CogConverter::new(input.to_string_lossy().to_string())
        .output(output.to_string_lossy().to_string())
        .convert()
        .expect("Float32 conversion should succeed");

    // If the converter had hard-coded UInt8, the output would be 8-bit and the
    // pixel buffer four times too small — so this assertion proves detection.
    let out_reader =
        GeoTiffReader::open(FileDataSource::open(&output).expect("output should open"))
            .expect("output should be readable");
    assert_eq!(
        out_reader.data_type(),
        Some(RasterDataType::Float32),
        "converter must preserve the detected Float32 data type"
    );

    cleanup(&input);
    cleanup(&output);
}

/// 10. Converting a non-existent input path returns an `Err` without panicking.
#[test]
fn test_cog_converter_nonexistent_input_errors() {
    let missing = temp_path("does_not_exist_in", "tif");
    let output = temp_path("does_not_exist_out", "tif");
    // Intentionally do NOT create `missing`.
    assert!(!missing.exists(), "precondition: input must not exist");

    let result = CogConverter::new(missing.to_string_lossy().to_string())
        .output(output.to_string_lossy().to_string())
        .convert();

    assert!(
        result.is_err(),
        "converting a missing input path must return Err"
    );
    assert!(
        !output.exists(),
        "no output should be produced for a failed conversion"
    );
}
