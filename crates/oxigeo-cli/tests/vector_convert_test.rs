//! Integration tests for vector format conversion via `util::vector`.

use anyhow::{Context, Result, anyhow};
use oxigeo_cli::util::vector::{AttributeFilter, FilterOp, convert_vector};
use std::io::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Per-test scratch fixture inside the system temp dir (house policy: no
/// hardcoded absolute paths).
///
/// The leaf name embeds the process id and a monotonic counter, so no two test
/// binaries — nor two concurrent runs of this one — can ever land on the same
/// file.  Dropping the guard removes the fixture, so a panicking test leaks
/// nothing.
struct TempPath(std::path::PathBuf);

impl TempPath {
    fn new(name: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!(
            "oxigeo_cli_vct_{}_{seq}_{name}",
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
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Write a minimal GeoJSON FeatureCollection to a temp file and return the path.
fn write_temp_geojson(name: &str, json: &str) -> Result<TempPath> {
    let path = temp_path(name, "geojson");
    let mut f = std::fs::File::create(&*path)
        .with_context(|| format!("create temp geojson: {}", path.display()))?;
    f.write_all(json.as_bytes()).context("write temp geojson")?;
    Ok(path)
}

fn temp_path(name: &str, ext: &str) -> TempPath {
    TempPath::new(&format!("{name}.{ext}"))
}

// 3-feature GeoJSON with different "kind" properties
const GEOJSON_3F: &str = r#"{
  "type": "FeatureCollection",
  "features": [
    {
      "type": "Feature",
      "geometry": { "type": "Point", "coordinates": [10.0, 20.0] },
      "properties": { "name": "Alpha", "kind": "city", "pop": 1000 }
    },
    {
      "type": "Feature",
      "geometry": { "type": "Point", "coordinates": [11.0, 21.0] },
      "properties": { "name": "Beta", "kind": "town", "pop": 500 }
    },
    {
      "type": "Feature",
      "geometry": { "type": "Point", "coordinates": [12.0, 22.0] },
      "properties": { "name": "Gamma", "kind": "city", "pop": 2000 }
    }
  ]
}"#;

// 5-feature GeoJSON with varied names
const GEOJSON_5F: &str = r#"{
  "type": "FeatureCollection",
  "features": [
    {
      "type": "Feature",
      "geometry": { "type": "Point", "coordinates": [1.0, 2.0] },
      "properties": { "label": "apple", "score": 10 }
    },
    {
      "type": "Feature",
      "geometry": { "type": "Point", "coordinates": [2.0, 3.0] },
      "properties": { "label": "banana", "score": 20 }
    },
    {
      "type": "Feature",
      "geometry": { "type": "Point", "coordinates": [3.0, 4.0] },
      "properties": { "label": "apricot", "score": 30 }
    },
    {
      "type": "Feature",
      "geometry": { "type": "Point", "coordinates": [4.0, 5.0] },
      "properties": { "label": "cherry", "score": 40 }
    },
    {
      "type": "Feature",
      "geometry": { "type": "Point", "coordinates": [5.0, 6.0] },
      "properties": { "label": "blueberry", "score": 50 }
    }
  ]
}"#;

// ── Test 1: GeoJSON → GeoJSON ─────────────────────────────────────────────────

#[test]
fn test_geojson_to_geojson() -> Result<()> {
    let input = write_temp_geojson("gj2gj_in", GEOJSON_3F)?;
    let output = temp_path("gj2gj_out", "geojson");

    let count = convert_vector(&input, &output, None)?;

    assert_eq!(count, 3, "expected 3 features written");
    assert!(output.exists(), "output file should exist");

    // Round-trip: read back and verify
    let content = std::fs::read_to_string(&output)?;
    let parsed: serde_json::Value = serde_json::from_str(&content)?;
    let features = parsed["features"]
        .as_array()
        .ok_or_else(|| anyhow!("expected features array in output"))?;
    assert_eq!(features.len(), 3);

    Ok(())
}

// ── Test 2: GeoJSON → Shapefile ───────────────────────────────────────────────

#[test]
fn test_geojson_to_shapefile() -> Result<()> {
    let input = write_temp_geojson("gj2shp_in", GEOJSON_3F)?;
    let output = temp_path("gj2shp_out", "shp");

    let count = convert_vector(&input, &output, None)?;

    assert_eq!(count, 3, "expected 3 features written");
    assert!(output.exists(), ".shp file should exist");

    // Also verify .dbf exists
    let dbf = output.with_extension("dbf");
    assert!(dbf.exists(), ".dbf file should exist");

    // Read back via ShapefileReader
    let base = output.with_extension("");
    let reader = oxigeo_shapefile::ShapefileReader::open(&base)?;
    let features = reader.read_features()?;
    assert_eq!(features.len(), 3);

    // Cleanup: the shapefile sidecars are not covered by the TempPath guard.
    for ext in &["dbf", "shx"] {
        let _ = std::fs::remove_file(output.with_extension(ext));
    }
    Ok(())
}

// ── Test 3: Shapefile → GeoJSON ───────────────────────────────────────────────

#[test]
fn test_shapefile_to_geojson() -> Result<()> {
    // First create a shapefile via GeoJSON→Shapefile conversion
    let gj_input = write_temp_geojson("shp2gj_setup", GEOJSON_3F)?;
    let shp_intermediate = temp_path("shp2gj_inter", "shp");
    convert_vector(&gj_input, &shp_intermediate, None)?;

    // Now convert Shapefile → GeoJSON
    let gj_output = temp_path("shp2gj_out", "geojson");
    let count = convert_vector(&shp_intermediate, &gj_output, None)?;

    assert_eq!(count, 3, "expected 3 features in GeoJSON output");
    assert!(gj_output.exists(), "output GeoJSON should exist");

    let content = std::fs::read_to_string(&gj_output)?;
    let parsed: serde_json::Value = serde_json::from_str(&content)?;
    let features = parsed["features"]
        .as_array()
        .ok_or_else(|| anyhow!("expected features array in output"))?;
    assert_eq!(features.len(), 3);

    // Cleanup: the shapefile sidecars are not covered by the TempPath guard.
    for ext in &["dbf", "shx"] {
        let _ = std::fs::remove_file(shp_intermediate.with_extension(ext));
    }
    Ok(())
}

// ── Test 4: Attribute filter eq ───────────────────────────────────────────────

#[test]
fn test_attribute_filter_eq() -> Result<()> {
    let input = write_temp_geojson("filt_eq_in", GEOJSON_3F)?;
    let output = temp_path("filt_eq_out", "geojson");

    let filter = AttributeFilter {
        field: "kind".to_string(),
        op: FilterOp::Eq,
        value: "city".to_string(),
    };

    let count = convert_vector(&input, &output, Some(&filter))?;

    // Only "Alpha" and "Gamma" have kind="city"
    assert_eq!(count, 2, "eq filter should match 2 features");

    let content = std::fs::read_to_string(&output)?;
    let parsed: serde_json::Value = serde_json::from_str(&content)?;
    let features = parsed["features"]
        .as_array()
        .ok_or_else(|| anyhow!("expected features array"))?;
    assert_eq!(features.len(), 2);

    // Verify all returned features have kind == "city"
    for f in features {
        let kind = f["properties"]["kind"]
            .as_str()
            .ok_or_else(|| anyhow!("expected kind field to be a string"))?;
        assert_eq!(kind, "city");
    }

    Ok(())
}

// ── Test 5: Attribute filter contains ────────────────────────────────────────

#[test]
fn test_attribute_filter_contains() -> Result<()> {
    let input = write_temp_geojson("filt_contains_in", GEOJSON_5F)?;
    let output = temp_path("filt_contains_out", "geojson");

    let filter = AttributeFilter {
        field: "label".to_string(),
        op: FilterOp::Contains,
        value: "ap".to_string(), // matches "apple" and "apricot"
    };

    let count = convert_vector(&input, &output, Some(&filter))?;

    assert_eq!(
        count, 2,
        "contains filter should match 2 features ('apple', 'apricot')"
    );

    let content = std::fs::read_to_string(&output)?;
    let parsed: serde_json::Value = serde_json::from_str(&content)?;
    let features = parsed["features"]
        .as_array()
        .ok_or_else(|| anyhow!("expected features array"))?;
    assert_eq!(features.len(), 2);

    for f in features {
        let label = f["properties"]["label"]
            .as_str()
            .ok_or_else(|| anyhow!("expected label field to be a string"))?;
        assert!(label.contains("ap"), "label '{label}' should contain 'ap'");
    }

    Ok(())
}

// ── Test 6: Unknown output format error ──────────────────────────────────────

#[test]
fn test_unknown_output_format_error() -> Result<()> {
    let input = write_temp_geojson("unk_fmt_in", GEOJSON_3F)?;
    let output = temp_path("unk_fmt_out", "xyz");

    let result = convert_vector(&input, &output, None);
    assert!(result.is_err(), "should return error for unknown extension");

    if let Err(e) = result {
        let err_msg = e.to_string();
        assert!(
            err_msg.contains("Cannot determine output format") || err_msg.contains("Unknown"),
            "error message should mention unknown format, got: {err_msg}"
        );
    }

    Ok(())
}

// ── Test 7: Attribute filter ne ───────────────────────────────────────────────

#[test]
fn test_attribute_filter_ne() -> Result<()> {
    let input = write_temp_geojson("filt_ne_in", GEOJSON_3F)?;
    let output = temp_path("filt_ne_out", "geojson");

    let filter = AttributeFilter {
        field: "kind".to_string(),
        op: FilterOp::Ne,
        value: "city".to_string(),
    };

    let count = convert_vector(&input, &output, Some(&filter))?;

    // Only "Beta" has kind="town" (not city)
    assert_eq!(count, 1, "ne filter should match 1 feature");

    let content = std::fs::read_to_string(&output)?;
    let parsed: serde_json::Value = serde_json::from_str(&content)?;
    let features = parsed["features"]
        .as_array()
        .ok_or_else(|| anyhow!("expected features array"))?;
    assert_eq!(features.len(), 1);

    let name = features[0]["properties"]["name"]
        .as_str()
        .ok_or_else(|| anyhow!("expected name field"))?;
    assert_eq!(name, "Beta");

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// FlatGeobuf conversion tests
// ─────────────────────────────────────────────────────────────────────────────

/// Helper: write a minimal FlatGeobuf file with N point features to `path`.
///
/// Each feature has two string properties: `name` and `kind`.
fn write_temp_fgb(
    name: &str,
    features_data: &[(&str, &str, f64, f64)], // (name, kind, x, y)
) -> Result<TempPath> {
    use oxigeo_core::vector::{Feature as CoreFeature, FieldValue, Geometry, Point};
    use oxigeo_flatgeobuf::{Column, ColumnType, FlatGeobufWriterBuilder, GeometryType};
    use std::fs::File;
    use std::io::BufWriter;

    let path = temp_path(name, "fgb");

    let builder = FlatGeobufWriterBuilder::new(GeometryType::Point)
        .with_index()
        .with_column(Column::new("name", ColumnType::String))
        .with_column(Column::new("kind", ColumnType::String));

    let file =
        File::create(&*path).with_context(|| format!("create temp fgb: {}", path.display()))?;
    let buf_writer = BufWriter::new(file);
    let mut writer = builder.build(buf_writer).context("create FGB writer")?;

    for (feat_name, kind, x, y) in features_data {
        let mut feat = CoreFeature::new(Geometry::Point(Point::new(*x, *y)));
        feat.set_property("name", FieldValue::String(feat_name.to_string()));
        feat.set_property("kind", FieldValue::String(kind.to_string()));
        writer.add_feature(&feat).context("add feature to FGB")?;
    }

    writer.finish().context("finish FGB writer")?;
    Ok(path)
}

/// Helper: write an empty FlatGeobuf file (no features).
fn write_empty_fgb(name: &str) -> Result<TempPath> {
    use oxigeo_flatgeobuf::{FlatGeobufWriterBuilder, GeometryType};
    use std::fs::File;
    use std::io::BufWriter;

    let path = temp_path(name, "fgb");
    let builder = FlatGeobufWriterBuilder::new(GeometryType::Point).with_index();
    let file =
        File::create(&*path).with_context(|| format!("create empty fgb: {}", path.display()))?;
    let buf_writer = BufWriter::new(file);
    let writer = builder
        .build(buf_writer)
        .context("create empty FGB writer")?;
    writer.finish().context("finish empty FGB writer")?;
    Ok(path)
}

// ── Test 8: FlatGeobuf → GeoJSON round-trip ───────────────────────────────────

#[test]
fn test_flatgeobuf_to_geojson() -> Result<()> {
    let fgb_data = &[
        ("Alpha", "city", 10.0f64, 20.0f64),
        ("Beta", "town", 11.0, 21.0),
        ("Gamma", "city", 12.0, 22.0),
    ];
    let input = write_temp_fgb("fgb2gj_in", fgb_data)?;
    let output = temp_path("fgb2gj_out", "geojson");

    let count = convert_vector(&input, &output, None)?;
    assert_eq!(count, 3, "expected 3 features written");
    assert!(output.exists(), "output file should exist");

    let content = std::fs::read_to_string(&output)?;
    let parsed: serde_json::Value = serde_json::from_str(&content)?;
    let features = parsed["features"]
        .as_array()
        .ok_or_else(|| anyhow!("expected features array"))?;
    assert_eq!(features.len(), 3, "GeoJSON should have 3 features");

    // Verify property round-trip
    let names: std::collections::HashSet<&str> = features
        .iter()
        .filter_map(|f| f["properties"]["name"].as_str())
        .collect();
    assert!(names.contains("Alpha"), "Alpha should be present");
    assert!(names.contains("Beta"), "Beta should be present");
    assert!(names.contains("Gamma"), "Gamma should be present");

    Ok(())
}

// ── Test 9: GeoJSON → FlatGeobuf conversion ───────────────────────────────────

#[test]
fn test_geojson_to_flatgeobuf() -> Result<()> {
    let input = write_temp_geojson("gj2fgb_in", GEOJSON_3F)?;
    let output = temp_path("gj2fgb_out", "fgb");

    let count = convert_vector(&input, &output, None)?;
    assert_eq!(count, 3, "expected 3 features written");
    assert!(output.exists(), "FGB output file should exist");

    // Verify the file is a valid FlatGeobuf by reading it back
    let read_back_path = temp_path("gj2fgb_readback", "geojson");
    let readback_count = convert_vector(&output, &read_back_path, None)?;
    assert_eq!(readback_count, 3, "round-trip should yield 3 features");

    let content = std::fs::read_to_string(&read_back_path)?;
    let parsed: serde_json::Value = serde_json::from_str(&content)?;
    let features = parsed["features"]
        .as_array()
        .ok_or_else(|| anyhow!("expected features array in read-back"))?;
    assert_eq!(features.len(), 3);

    Ok(())
}

// ── Test 10: FlatGeobuf round-trip (FGB → GeoJSON → FGB) ──────────────────────

#[test]
fn test_flatgeobuf_round_trip() -> Result<()> {
    let fgb_data = &[
        ("Node1", "type_a", 1.0f64, 2.0f64),
        ("Node2", "type_b", 3.0, 4.0),
    ];
    let input_fgb = write_temp_fgb("fgb_rt_in", fgb_data)?;
    let mid_geojson = temp_path("fgb_rt_mid", "geojson");
    let output_fgb = temp_path("fgb_rt_out", "fgb");
    let final_geojson = temp_path("fgb_rt_final", "geojson");

    // FGB → GeoJSON
    let c1 = convert_vector(&input_fgb, &mid_geojson, None)?;
    assert_eq!(c1, 2, "FGB→GeoJSON should yield 2 features");

    // GeoJSON → FGB
    let c2 = convert_vector(&mid_geojson, &output_fgb, None)?;
    assert_eq!(c2, 2, "GeoJSON→FGB should yield 2 features");

    // FGB → GeoJSON (final verification)
    let c3 = convert_vector(&output_fgb, &final_geojson, None)?;
    assert_eq!(c3, 2, "final FGB→GeoJSON should yield 2 features");

    let content = std::fs::read_to_string(&final_geojson)?;
    let parsed: serde_json::Value = serde_json::from_str(&content)?;
    let features = parsed["features"]
        .as_array()
        .ok_or_else(|| anyhow!("expected features array in final output"))?;
    assert_eq!(features.len(), 2);

    let names: std::collections::HashSet<&str> = features
        .iter()
        .filter_map(|f| f["properties"]["name"].as_str())
        .collect();
    assert!(names.contains("Node1"), "Node1 should survive round-trip");
    assert!(names.contains("Node2"), "Node2 should survive round-trip");

    Ok(())
}

// ── Test 11: FlatGeobuf attribute filter ──────────────────────────────────────

#[test]
fn test_flatgeobuf_attribute_filter() -> Result<()> {
    let fgb_data = &[
        ("Alpha", "city", 10.0f64, 20.0f64),
        ("Beta", "town", 11.0, 21.0),
        ("Gamma", "city", 12.0, 22.0),
    ];
    let input = write_temp_fgb("fgb_filt_in", fgb_data)?;
    let output = temp_path("fgb_filt_out", "geojson");

    let filter = AttributeFilter {
        field: "kind".to_string(),
        op: FilterOp::Eq,
        value: "city".to_string(),
    };

    let count = convert_vector(&input, &output, Some(&filter))?;
    assert_eq!(count, 2, "filter should match 2 city features");

    let content = std::fs::read_to_string(&output)?;
    let parsed: serde_json::Value = serde_json::from_str(&content)?;
    let features = parsed["features"]
        .as_array()
        .ok_or_else(|| anyhow!("expected features array"))?;
    assert_eq!(features.len(), 2);

    for f in features {
        let kind = f["properties"]["kind"]
            .as_str()
            .ok_or_else(|| anyhow!("expected kind field"))?;
        assert_eq!(kind, "city", "all returned features should have kind=city");
    }

    Ok(())
}

// ── Test 12: FlatGeobuf empty file handled gracefully ─────────────────────────

#[test]
fn test_flatgeobuf_empty() -> Result<()> {
    let input = write_empty_fgb("fgb_empty_in")?;
    let output = temp_path("fgb_empty_out", "geojson");

    let count = convert_vector(&input, &output, None)?;
    assert_eq!(count, 0, "empty FGB should yield 0 features");
    assert!(output.exists(), "output GeoJSON should still be created");

    let content = std::fs::read_to_string(&output)?;
    let parsed: serde_json::Value = serde_json::from_str(&content)?;
    let features = parsed["features"]
        .as_array()
        .ok_or_else(|| anyhow!("expected features array even for empty collection"))?;
    assert!(features.is_empty(), "features array should be empty");

    Ok(())
}
