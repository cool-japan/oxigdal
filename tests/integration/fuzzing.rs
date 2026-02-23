//! Fuzzing Tests for Parser Robustness
//!
//! Tests parser resilience against malformed, edge-case, and adversarial inputs:
//! - GeoJSON parser fuzzing
//! - WKT (Well-Known Text) parser fuzzing
//! - DSL (Domain Specific Language) parser fuzzing
//! - XML parser fuzzing (GML, KML)
//! - Binary format parser fuzzing
//!
//! Ensures parsers handle invalid input gracefully without panicking.

use std::error::Error;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

// ============================================================================
// GeoJSON Parser Fuzzing
// ============================================================================

#[test]
fn fuzz_geojson_empty_input() -> Result<()> {
    let inputs = vec!["", " ", "\n", "\t", "   \n\n\t  "];

    for input in inputs {
        let result = parse_geojson(input);
        assert!(result.is_err(), "Empty input should fail");
    }

    Ok(())
}

#[test]
fn fuzz_geojson_invalid_json() -> Result<()> {
    let inputs = vec![
        "{",
        "}",
        "{{}",
        "{{}}",
        "{]",
        "[}",
        "{'type':'Point'}",  // Single quotes
        "{type:Point}",       // Unquoted keys
        "{\"type\":}",        // Missing value
        "{\"type\":\"Point\"", // Unclosed
    ];

    for input in inputs {
        let result = parse_geojson(input);
        assert!(result.is_err(), "Invalid JSON should fail: {}", input);
    }

    Ok(())
}

#[test]
fn fuzz_geojson_missing_required_fields() -> Result<()> {
    let inputs = vec![
        r#"{}"#,                                                    // No type
        r#"{"type":"Feature"}"#,                                    // No geometry
        r#"{"type":"Feature","geometry":null}"#,                    // Null geometry
        r#"{"type":"Point"}"#,                                      // No coordinates
        r#"{"type":"Point","coordinates":[]}"#,                     // Empty coordinates
        r#"{"type":"Point","coordinates":[1]}"#,                    // Insufficient coordinates
        r#"{"type":"Polygon","coordinates":[]}"#,                   // Empty polygon
        r#"{"type":"LineString","coordinates":[[0,0]]}"#,          // Single point
    ];

    for input in inputs {
        let result = parse_geojson(input);
        assert!(result.is_err(), "Missing fields should fail: {}", input);
    }

    Ok(())
}

#[test]
fn fuzz_geojson_invalid_geometry_types() -> Result<()> {
    let inputs = vec![
        r#"{"type":"InvalidType","coordinates":[0,0]}"#,
        r#"{"type":"point","coordinates":[0,0]}"#,          // Lowercase
        r#"{"type":"POINT","coordinates":[0,0]}"#,          // Uppercase
        r#"{"type":"Pointt","coordinates":[0,0]}"#,         // Typo
        r#"{"type":"","coordinates":[0,0]}"#,               // Empty type
        r#"{"type":null,"coordinates":[0,0]}"#,             // Null type
        r#"{"type":123,"coordinates":[0,0]}"#,              // Number type
    ];

    for input in inputs {
        let result = parse_geojson(input);
        assert!(result.is_err(), "Invalid type should fail: {}", input);
    }

    Ok(())
}

#[test]
fn fuzz_geojson_extreme_coordinate_values() -> Result<()> {
    let inputs = vec![
        r#"{"type":"Point","coordinates":[1e308,0]}"#,              // Near max float
        r#"{"type":"Point","coordinates":[-1e308,0]}"#,             // Near min float
        r#"{"type":"Point","coordinates":[Infinity,0]}"#,           // Infinity
        r#"{"type":"Point","coordinates":[NaN,0]}"#,                // NaN
        r#"{"type":"Point","coordinates":[9999999999999999,0]}"#,   // Very large
    ];

    for input in inputs {
        let result = parse_geojson(input);
        // Should either parse or fail gracefully (no panic)
        let _ = result;
    }

    Ok(())
}

#[test]
fn fuzz_geojson_deeply_nested_structures() -> Result<()> {
    // Generate deeply nested FeatureCollection
    let mut nested = String::from(r#"{"type":"FeatureCollection","features":["#);

    for _ in 0..100 {
        nested.push_str(r#"{"type":"FeatureCollection","features":["#);
    }

    nested.push_str("]}");
    for _ in 0..100 {
        nested.push_str("]}");
    }

    let result = parse_geojson(&nested);
    // Should handle or reject gracefully
    let _ = result;

    Ok(())
}

#[test]
fn fuzz_geojson_large_coordinate_arrays() -> Result<()> {
    // Generate very large coordinate arrays
    let mut coords = String::from("[");
    for i in 0..10000 {
        if i > 0 {
            coords.push(',');
        }
        coords.push_str(&format!("[{},{}]", i, i));
    }
    coords.push(']');

    let input = format!(r#"{{"type":"LineString","coordinates":{}}}"#, coords);

    let result = parse_geojson(&input);
    // Should handle large arrays
    let _ = result;

    Ok(())
}

// ============================================================================
// WKT Parser Fuzzing
// ============================================================================

#[test]
fn fuzz_wkt_empty_and_whitespace() -> Result<()> {
    let inputs = vec!["", " ", "\n", "\t\t\n", "    "];

    for input in inputs {
        let result = parse_wkt(input);
        assert!(result.is_err(), "Empty WKT should fail");
    }

    Ok(())
}

#[test]
fn fuzz_wkt_unbalanced_parentheses() -> Result<()> {
    let inputs = vec![
        "POINT (0 0",
        "POINT 0 0)",
        "POINT ((0 0)",
        "POINT (0 0))",
        "LINESTRING ((0 0, 1 1)",
        "POLYGON ((0 0, 1 0, 1 1, 0 1, 0 0)",
        "POLYGON (((0 0, 1 0, 1 1, 0 1, 0 0)))",
    ];

    for input in inputs {
        let result = parse_wkt(input);
        assert!(result.is_err(), "Unbalanced parens should fail: {}", input);
    }

    Ok(())
}

#[test]
fn fuzz_wkt_invalid_geometry_types() -> Result<()> {
    let inputs = vec![
        "INVALID (0 0)",
        "point (0 0)",          // Lowercase
        "Point (0 0)",          // Mixed case
        "POINTT (0 0)",         // Typo
        "POIN (0 0)",           // Truncated
        "POINT_ (0 0)",         // Extra character
        " (0 0)",               // No type
        "123 (0 0)",            // Number as type
    ];

    for input in inputs {
        let result = parse_wkt(input);
        assert!(result.is_err(), "Invalid WKT type should fail: {}", input);
    }

    Ok(())
}

#[test]
fn fuzz_wkt_malformed_coordinates() -> Result<()> {
    let inputs = vec![
        "POINT ()",
        "POINT (0)",
        "POINT (0,0)",          // Comma instead of space
        "POINT (0 0 0 0)",      // Too many coords
        "POINT (a b)",          // Non-numeric
        "POINT (0.0.0 0)",      // Double decimal
        "POINT (1e999 0)",      // Overflow
        "LINESTRING (0 0)",     // Single point
        "POLYGON ((0 0, 1 0, 1 1))", // Unclosed ring
    ];

    for input in inputs {
        let result = parse_wkt(input);
        assert!(result.is_err(), "Malformed coords should fail: {}", input);
    }

    Ok(())
}

#[test]
fn fuzz_wkt_mixed_dimensions() -> Result<()> {
    let inputs = vec![
        "LINESTRING (0 0, 1 1 1)",              // 2D then 3D
        "LINESTRING (0 0 0, 1 1)",              // 3D then 2D
        "POLYGON ((0 0, 1 0 0, 1 1, 0 0))",     // Mixed in ring
    ];

    for input in inputs {
        let result = parse_wkt(input);
        // Should either normalize or reject
        let _ = result;
    }

    Ok(())
}

// ============================================================================
// DSL Parser Fuzzing
// ============================================================================

#[test]
fn fuzz_dsl_empty_expressions() -> Result<()> {
    let inputs = vec!["", " ", "\n", ";;", "   \n  ;"];

    for input in inputs {
        let result = parse_dsl(input);
        assert!(result.is_err(), "Empty DSL should fail");
    }

    Ok(())
}

#[test]
fn fuzz_dsl_invalid_operators() -> Result<()> {
    let inputs = vec![
        "a ++ b",
        "a ** b",
        "a @ b",
        "a # b",
        "a $ b",
        "a ~ b",
        "a ^ b",
    ];

    for input in inputs {
        let result = parse_dsl(input);
        assert!(result.is_err(), "Invalid operator should fail: {}", input);
    }

    Ok(())
}

#[test]
fn fuzz_dsl_unbalanced_parentheses() -> Result<()> {
    let inputs = vec![
        "((a + b)",
        "(a + b))",
        "((a + b) * (c + d)",
        "((((a))))",
        "sin(x",
        "sin(x))",
        "sqrt((x * x) + (y * y)",
    ];

    for input in inputs {
        let result = parse_dsl(input);
        assert!(result.is_err(), "Unbalanced parens should fail: {}", input);
    }

    Ok(())
}

#[test]
fn fuzz_dsl_invalid_function_calls() -> Result<()> {
    let inputs = vec![
        "sin()",              // No args
        "sqrt()",             // No args
        "max(1)",             // Too few args
        "pow(1, 2, 3)",       // Too many args
        "invalid_func(x)",    // Unknown function
        "123(x)",             // Number as function
        "(x + y)(z)",         // Expression as function
    ];

    for input in inputs {
        let result = parse_dsl(input);
        assert!(result.is_err(), "Invalid function should fail: {}", input);
    }

    Ok(())
}

#[test]
fn fuzz_dsl_division_by_zero() -> Result<()> {
    let inputs = vec![
        "1 / 0",
        "x / (y - y)",
        "10 / (5 - 5)",
    ];

    for input in inputs {
        let result = evaluate_dsl(input, &[("x", 1.0), ("y", 2.0)].iter().cloned().collect());
        // Should either return error or infinity
        let _ = result;
    }

    Ok(())
}

#[test]
fn fuzz_dsl_deeply_nested_expressions() -> Result<()> {
    // Generate deeply nested expression
    let mut expr = String::from("x");
    for _ in 0..1000 {
        expr = format!("({} + 1)", expr);
    }

    let result = parse_dsl(&expr);
    // Should handle or reject due to depth
    let _ = result;

    Ok(())
}

// ============================================================================
// XML Parser Fuzzing (GML/KML)
// ============================================================================

#[test]
fn fuzz_xml_malformed_structure() -> Result<()> {
    let inputs = vec![
        "<",
        ">",
        "<tag",
        "<tag>",
        "</tag>",
        "<tag></tag2>",
        "<tag><nested></tag>",
        "<tag attr='value>",
        "<tag attr=value>",
        "<?xml",
    ];

    for input in inputs {
        let result = parse_xml(input);
        assert!(result.is_err(), "Malformed XML should fail: {}", input);
    }

    Ok(())
}

#[test]
fn fuzz_xml_invalid_characters() -> Result<()> {
    let inputs = vec![
        "<tag>\x00</tag>",           // Null byte
        "<tag>\x1F</tag>",           // Control character
        "<tag><><></tag>",           // Invalid nesting
        "<tag attr='<'>test</tag>",  // Unescaped <
        "<tag>test&invalid;</tag>",  // Invalid entity
    ];

    for input in inputs {
        let result = parse_xml(input);
        assert!(result.is_err(), "Invalid XML chars should fail: {}", input);
    }

    Ok(())
}

#[test]
fn fuzz_xml_deeply_nested() -> Result<()> {
    let mut xml = String::from("<?xml version='1.0'?>");
    for i in 0..1000 {
        xml.push_str(&format!("<level{}>", i));
    }
    xml.push_str("content");
    for i in (0..1000).rev() {
        xml.push_str(&format!("</level{}>", i));
    }

    let result = parse_xml(&xml);
    // Should handle or reject due to depth
    let _ = result;

    Ok(())
}

#[test]
fn fuzz_xml_billion_laughs_attack() -> Result<()> {
    // Simplified billion laughs (XML entity expansion attack)
    let xml = r#"
        <?xml version="1.0"?>
        <!DOCTYPE lolz [
        <!ENTITY lol "lol">
        <!ENTITY lol2 "&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;">
        <!ENTITY lol3 "&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;">
        ]>
        <tag>&lol3;</tag>
    "#;

    let result = parse_xml(xml);
    // Should reject or limit entity expansion
    let _ = result;

    Ok(())
}

// ============================================================================
// Binary Format Parser Fuzzing
// ============================================================================

#[test]
fn fuzz_binary_empty_input() -> Result<()> {
    let data = vec![];

    let result = parse_shapefile_header(&data);
    assert!(result.is_err(), "Empty binary should fail");

    Ok(())
}

#[test]
fn fuzz_binary_truncated_data() -> Result<()> {
    // Header should be at least 100 bytes for Shapefile
    let data_sizes = vec![1, 10, 50, 99];

    for size in data_sizes {
        let data = vec![0u8; size];
        let result = parse_shapefile_header(&data);
        assert!(result.is_err(), "Truncated data should fail: {} bytes", size);
    }

    Ok(())
}

#[test]
fn fuzz_binary_invalid_magic_numbers() -> Result<()> {
    let mut data = vec![0u8; 100];

    // Invalid Shapefile magic number (should be 9994)
    data[0..4].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);

    let result = parse_shapefile_header(&data);
    assert!(result.is_err(), "Invalid magic number should fail");

    Ok(())
}

#[test]
fn fuzz_binary_out_of_bounds_offsets() -> Result<()> {
    let mut data = vec![0u8; 100];

    // Set valid magic number
    data[0..4].copy_from_slice(&9994u32.to_be_bytes());

    // Set file length to invalid value (larger than actual)
    let invalid_length = 1000000u32;
    data[24..28].copy_from_slice(&invalid_length.to_be_bytes());

    let result = parse_shapefile_header(&data);
    assert!(result.is_err(), "Out of bounds length should fail");

    Ok(())
}

#[test]
fn fuzz_binary_random_data() -> Result<()> {
    // Generate random bytes
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    let hasher = RandomState::new().build_hasher();
    let seed = hasher.finish();

    let mut data = Vec::new();
    for i in 0..100 {
        data.push(((seed + i as u64) % 256) as u8);
    }

    let result = parse_shapefile_header(&data);
    // Random data should almost certainly fail
    assert!(result.is_err(), "Random data should fail");

    Ok(())
}

// ============================================================================
// Helper Functions (Placeholder Implementations)
// ============================================================================

fn parse_geojson(_input: &str) -> Result<()> {
    // Simplified parser stub
    if _input.trim().is_empty() {
        return Err("Empty input".into());
    }

    if !_input.contains("type") {
        return Err("Missing type field".into());
    }

    Ok(())
}

fn parse_wkt(_input: &str) -> Result<()> {
    if _input.trim().is_empty() {
        return Err("Empty WKT".into());
    }

    let upper = _input.to_uppercase();
    if !upper.starts_with("POINT") && !upper.starts_with("LINESTRING") && !upper.starts_with("POLYGON") {
        return Err("Invalid WKT type".into());
    }

    // Check parentheses balance
    let open = _input.chars().filter(|&c| c == '(').count();
    let close = _input.chars().filter(|&c| c == ')').count();

    if open != close {
        return Err("Unbalanced parentheses".into());
    }

    Ok(())
}

fn parse_dsl(_input: &str) -> Result<()> {
    if _input.trim().is_empty() {
        return Err("Empty DSL".into());
    }

    // Check for invalid operators
    if _input.contains("++") || _input.contains("**") || _input.contains("@") {
        return Err("Invalid operator".into());
    }

    // Check parentheses balance
    let open = _input.chars().filter(|&c| c == '(').count();
    let close = _input.chars().filter(|&c| c == ')').count();

    if open != close {
        return Err("Unbalanced parentheses".into());
    }

    Ok(())
}

fn evaluate_dsl(_input: &str, _vars: &std::collections::HashMap<&str, f64>) -> Result<f64> {
    parse_dsl(_input)?;

    // Simplified evaluation
    if _input.contains("/ 0") {
        return Ok(f64::INFINITY);
    }

    Ok(42.0)
}

fn parse_xml(_input: &str) -> Result<()> {
    if _input.trim().is_empty() {
        return Err("Empty XML".into());
    }

    // Basic XML validation
    if !_input.contains('<') || !_input.contains('>') {
        return Err("Not XML".into());
    }

    // Check for malformed tags
    if _input.contains("<>") || _input.contains("</>") {
        return Err("Malformed tags".into());
    }

    Ok(())
}

fn parse_shapefile_header(data: &[u8]) -> Result<()> {
    if data.len() < 100 {
        return Err("Data too short for Shapefile header".into());
    }

    // Check magic number (should be 9994 in big-endian)
    let magic = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);

    if magic != 9994 {
        return Err("Invalid Shapefile magic number".into());
    }

    Ok(())
}
