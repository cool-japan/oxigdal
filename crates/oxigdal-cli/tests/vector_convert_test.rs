//! Integration tests for vector format conversion via `util::vector`.

use anyhow::{Context, Result, anyhow};
use oxigdal_cli::util::vector::{AttributeFilter, FilterOp, convert_vector};
use std::io::Write as _;

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Write a minimal GeoJSON FeatureCollection to a temp file and return the path.
fn write_temp_geojson(name: &str, json: &str) -> Result<std::path::PathBuf> {
    let mut path = std::env::temp_dir();
    path.push(format!("oxigdal_vct_{name}.geojson"));
    let mut f = std::fs::File::create(&path)
        .with_context(|| format!("create temp geojson: {}", path.display()))?;
    f.write_all(json.as_bytes()).context("write temp geojson")?;
    Ok(path)
}

fn temp_path(name: &str, ext: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("oxigdal_vct_{name}.{ext}"));
    path
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

    // Cleanup
    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output);
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
    let reader = oxigdal_shapefile::ShapefileReader::open(&base)?;
    let features = reader.read_features()?;
    assert_eq!(features.len(), 3);

    // Cleanup
    let _ = std::fs::remove_file(&input);
    for ext in &["shp", "dbf", "shx"] {
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

    // Cleanup
    let _ = std::fs::remove_file(&gj_input);
    let _ = std::fs::remove_file(&gj_output);
    for ext in &["shp", "dbf", "shx"] {
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

    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output);
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

    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output);
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

    let _ = std::fs::remove_file(&input);
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

    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output);
    Ok(())
}
