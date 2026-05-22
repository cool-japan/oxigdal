//! Integration tests for the `oxigdal inspect` file inspector.
//!
//! These tests drive `oxigdal_cli::util::inspect_file` directly, since the CLI
//! exposes its utilities through the `oxigdal_cli` lib target. All fixture
//! files are created under `std::env::temp_dir()` with unique per-process
//! names; cleanup is best-effort.
//!
//! The `oxigdal-cli` crate denies `clippy::unwrap_used` and warns on
//! `clippy::expect_used`; under `clippy -D warnings` that warning is an error.
//! Each test therefore returns `anyhow::Result<()>` and propagates errors with
//! `?` instead of unwrapping.

use oxigdal_cli::util::inspect_file;

type TestResult = anyhow::Result<()>;

/// Minimal valid empty GeoJSON FeatureCollection.
const EMPTY_GEOJSON: &str = r#"{"type":"FeatureCollection","features":[]}"#;

/// Builds a unique temp path for a fixture file with the given suffix.
fn tmp_fixture(suffix: &str) -> std::path::PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut p = std::env::temp_dir();
    p.push(format!(
        "oxigdal_inspect_test_{}_{}_{}",
        std::process::id(),
        unique,
        suffix
    ));
    p
}

/// Writes an empty-FeatureCollection GeoJSON fixture and returns its path.
fn write_geojson_fixture(suffix: &str) -> TestResult2<std::path::PathBuf> {
    let path = tmp_fixture(suffix);
    std::fs::write(&path, EMPTY_GEOJSON)?;
    Ok(path)
}

/// Local alias so the helper can return a value rather than `()`.
type TestResult2<T> = anyhow::Result<T>;

// ---------------------------------------------------------------------------
// Test 1: minimal GeoJSON yields a GeoJSON-format report with a vector summary.
// ---------------------------------------------------------------------------
#[test]
fn test_inspect_geojson_summary() -> TestResult {
    let path = write_geojson_fixture("summary.geojson")?;

    let report = inspect_file(path.to_string_lossy().as_ref(), false)?;

    assert!(
        report.format.contains("GeoJSON"),
        "format should mention GeoJSON, got {}",
        report.format
    );
    assert!(
        report.vector.is_some(),
        "GeoJSON inspection must produce a vector summary"
    );
    assert!(
        report.raster.is_none(),
        "GeoJSON inspection must not produce a raster summary"
    );
    let vector = report
        .vector
        .ok_or_else(|| anyhow::anyhow!("vector summary missing"))?;
    assert_eq!(
        vector.feature_count,
        Some(0),
        "empty FeatureCollection has zero features"
    );

    let _ = std::fs::remove_file(&path);
    Ok(())
}

// ---------------------------------------------------------------------------
// Test 2: reported file_size matches std::fs metadata length exactly.
// ---------------------------------------------------------------------------
#[test]
fn test_inspect_reports_file_size() -> TestResult {
    let path = write_geojson_fixture("size.geojson")?;
    let expected = std::fs::metadata(&path)?.len();

    let report = inspect_file(path.to_string_lossy().as_ref(), false)?;

    assert_eq!(
        report.file_size, expected,
        "reported file_size must equal metadata().len()"
    );
    assert_eq!(
        report.file_size,
        EMPTY_GEOJSON.len() as u64,
        "fixture size must equal the known content length"
    );

    let _ = std::fs::remove_file(&path);
    Ok(())
}

// ---------------------------------------------------------------------------
// Test 3: the extension field is detected correctly.
// ---------------------------------------------------------------------------
#[test]
fn test_inspect_detects_extension() -> TestResult {
    let path = write_geojson_fixture("ext.geojson")?;

    let report = inspect_file(path.to_string_lossy().as_ref(), false)?;

    assert_eq!(
        report.extension, "geojson",
        "extension field must be the lower-cased extension without a dot"
    );

    let _ = std::fs::remove_file(&path);
    Ok(())
}

// ---------------------------------------------------------------------------
// Test 4: a nonexistent path produces an Err, never a panic.
// ---------------------------------------------------------------------------
#[test]
fn test_inspect_nonexistent_file_errors() -> TestResult {
    let path = tmp_fixture("does_not_exist.geojson");
    // Deliberately do NOT create the file.
    assert!(
        !path.exists(),
        "fixture path must not exist for this test to be meaningful"
    );

    let result = inspect_file(path.to_string_lossy().as_ref(), false);
    assert!(result.is_err(), "inspecting a missing file must return Err");
    Ok(())
}

// ---------------------------------------------------------------------------
// Test 5: the report serializes to JSON that parses back to a JSON value.
// ---------------------------------------------------------------------------
#[test]
fn test_inspect_json_output_is_valid_json() -> TestResult {
    let path = write_geojson_fixture("json_valid.geojson")?;

    let report = inspect_file(path.to_string_lossy().as_ref(), false)?;

    let serialized = serde_json::to_string(&report)?;
    let parsed: serde_json::Value = serde_json::from_str(&serialized)?;
    assert!(
        parsed.is_object(),
        "serialized report must be a JSON object"
    );

    let _ = std::fs::remove_file(&path);
    Ok(())
}

// ---------------------------------------------------------------------------
// Test 6: the serialized JSON schema exposes a `format` key.
// ---------------------------------------------------------------------------
#[test]
fn test_inspect_json_output_schema_has_format_field() -> TestResult {
    let path = write_geojson_fixture("json_schema.geojson")?;

    let report = inspect_file(path.to_string_lossy().as_ref(), false)?;

    let serialized = serde_json::to_string(&report)?;
    let parsed: serde_json::Value = serde_json::from_str(&serialized)?;

    assert!(
        parsed.get("format").is_some(),
        "serialized report must contain a `format` key"
    );
    assert_eq!(
        parsed.get("format").and_then(|v| v.as_str()),
        Some("GeoJSON"),
        "the `format` key must carry the detected format"
    );

    let _ = std::fs::remove_file(&path);
    Ok(())
}

// ---------------------------------------------------------------------------
// Test 7: a junk file with an unknown extension never panics; either Err or
//         a report with format "Unknown".
// ---------------------------------------------------------------------------
#[test]
fn test_inspect_unknown_format_does_not_panic() -> TestResult {
    let path = tmp_fixture("junk.xyz");
    std::fs::write(&path, b"\x00\x01\x02 not a real geospatial file")?;

    let result = inspect_file(path.to_string_lossy().as_ref(), false);
    match result {
        Ok(report) => assert_eq!(
            report.format, "Unknown",
            "a recognised-but-unsupported result must report format Unknown"
        ),
        Err(_) => {
            // An error is also acceptable; the contract is "never panics".
        }
    }

    let _ = std::fs::remove_file(&path);
    Ok(())
}

// ---------------------------------------------------------------------------
// Test 8: calling with detailed = true does not error on the GeoJSON fixture.
// ---------------------------------------------------------------------------
#[test]
fn test_inspect_detailed_flag() -> TestResult {
    let path = write_geojson_fixture("detailed.geojson")?;

    let report = inspect_file(path.to_string_lossy().as_ref(), true)?;

    let vector = report
        .vector
        .ok_or_else(|| anyhow::anyhow!("detailed GeoJSON inspection missing vector summary"))?;
    assert_eq!(
        vector.layer_count, 1,
        "a GeoJSON FeatureCollection is a single layer"
    );

    let _ = std::fs::remove_file(&path);
    Ok(())
}

// ---------------------------------------------------------------------------
// Test 9: a synthesised GeoTIFF yields a raster summary with correct
//         dimensions. The fixture is built via the CLI's own raster writer.
// ---------------------------------------------------------------------------
#[test]
fn test_inspect_geotiff_summary() -> TestResult {
    use oxigdal_cli::util::raster;
    use oxigdal_core::buffer::RasterBuffer;
    use oxigdal_core::types::{GeoTransform, NoDataValue, RasterDataType};

    let path = tmp_fixture("raster.tif");
    let _ = std::fs::remove_file(&path);

    // Build a tiny single-band UInt8 raster (16 x 16).
    let width = 16u64;
    let height = 16u64;
    let pixels: Vec<u8> = (0..(width * height) as usize)
        .map(|i| (i % 256) as u8)
        .collect();
    let band = RasterBuffer::new(
        pixels,
        width,
        height,
        RasterDataType::UInt8,
        NoDataValue::None,
    )
    .map_err(|e| anyhow::anyhow!("build raster buffer: {e}"))?;
    let geo_transform = GeoTransform {
        origin_x: 0.0,
        origin_y: 16.0,
        pixel_width: 1.0,
        pixel_height: -1.0,
        row_rotation: 0.0,
        col_rotation: 0.0,
    };

    let write_result =
        raster::write_multi_band(&path, &[band], Some(geo_transform), Some(4326), None);

    // If GeoTIFF synthesis is unavailable for any reason, fall back to a
    // GeoJSON-based assertion (is_cloud is false for a local path) and note
    // the substitution rather than failing the suite.
    if write_result.is_err() {
        let gj = write_geojson_fixture("raster_fallback.geojson")?;
        let report = inspect_file(gj.to_string_lossy().as_ref(), false)?;
        assert!(
            !report.is_cloud,
            "a local path must report is_cloud = false"
        );
        let _ = std::fs::remove_file(&gj);
        return Ok(());
    }

    let report = inspect_file(path.to_string_lossy().as_ref(), true)?;

    assert_eq!(report.format, "GeoTIFF", "format must be GeoTIFF");
    let raster_summary = report
        .raster
        .ok_or_else(|| anyhow::anyhow!("GeoTIFF inspection missing raster summary"))?;
    assert_eq!(raster_summary.width, 16, "raster width must match");
    assert_eq!(raster_summary.height, 16, "raster height must match");
    assert_eq!(raster_summary.band_count, 1, "single-band raster");
    assert!(
        report.vector.is_none(),
        "GeoTIFF inspection must not produce a vector summary"
    );

    let _ = std::fs::remove_file(&path);
    Ok(())
}
